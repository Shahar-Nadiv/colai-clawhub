//! Which window somebody is actually looking at, moment to moment.
//!
//! A mark is a place in an application, not a place on the desktop. To draw one only
//! while its own window is in front — and at that window's rectangle *now* rather than
//! the one it had when the mark was made — the page needs to be told about the front
//! window as it changes. That is all this does: watch, and say when it differs.
//!
//! Read straight from X rather than through `xprop` and `xwininfo`. Those spawn a process
//! per question, which is fine for the once-per-mark question they answer elsewhere and
//! absurd several times a second. Three small round trips on a socket cost microseconds.
//!
//! Polled rather than driven by events. Xlib's event masks would be cheaper again, but
//! they bring error handlers, races on windows that die between the select and the read,
//! and a second event loop; a tenth of a second of latency on "which window is in front"
//! is not worth any of that. What matters for cost is that nothing is *sent* unless the
//! answer changed, and that is the part with a test on it.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter as _, Runtime};

/// How often the front window is looked at.
///
/// Fast enough that switching application feels like the marks were never there, slow
/// enough to be free. Each look is three X round trips on a local socket.
/// How often the front window is looked at while something is moving.
///
/// Faster than any display refreshes, because a mark that updates on its own slower
/// clock trails the window it is drawn on — and a dot lagging a hand's-width behind a
/// dragged window reads as broken rather than as attached.
const WHILE_MOVING: std::time::Duration = std::time::Duration::from_millis(8);

/// And while nothing is happening, which is nearly always.
const WHEN_STILL: std::time::Duration = std::time::Duration::from_millis(100);

/// How long after the last change to keep looking quickly.
///
/// Long enough to cover the pauses inside a drag — a hand stops for a moment mid-move
/// far more often than it finishes — so the whole gesture is smooth rather than smooth
/// in bursts.
const STAYS_LIVELY: std::time::Duration = std::time::Duration::from_millis(400);

/// How soon to look again, given how long ago anything last moved.
///
/// A plain function so the pacing is a decision with a test on it rather than a number
/// buried in a loop. Idle is the case that has to stay cheap: ten looks a second, of
/// three small round trips each, against a hundred and twenty-five while a window is
/// actually under somebody's hand.
fn how_soon(since_change: std::time::Duration) -> std::time::Duration {
    if since_change < STAYS_LIVELY {
        WHILE_MOVING
    } else {
        WHEN_STILL
    }
}

/// The event the page listens for.
pub(crate) const FRONT_EVENT: &str = "colai:front";

/// The window in front, as much of it as decides whether a mark is drawn.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InFront {
    /// The X window id, as the same hex string a mark's address already carries. Empty
    /// when nothing is in front, which is an answer rather than the lack of one.
    pub id: String,
    pub title: Option<String>,
    /// Where it is now. A mark is drawn from this rather than from the rectangle the
    /// window had when it was made, which is what makes marks ride along when it moves.
    pub at: Option<crate::colai::Rect>,
    /// Ours. The overlay takes the keyboard whenever a popup opens, so without this
    /// every mark would vanish the instant somebody reached for the toolbar — including
    /// the one whose popup they just opened.
    pub ours: bool,
}

/// Whether this look is worth remembering as "the application somebody is working in".
///
/// Ours is not an answer to that question — the overlay takes the keyboard every time a
/// menu or a popup opens — and neither is an empty id, which is what X says when nothing
/// has focus at all.
pub(crate) fn worth_remembering(seen: &InFront) -> bool {
    !seen.ours && !seen.id.is_empty()
}

/// The last window this watcher saw that was not ours.
///
/// Kept as an id rather than as a description, deliberately. A window's title, working
/// directory and rectangle all move while it sits there, so what is worth remembering is
/// *which* window it was; whoever needs the details looks them up when they need them.
static LAST_NOT_OURS: Mutex<Option<String>> = Mutex::new(None);

/// Which window somebody was last working in, if this has seen one.
pub(crate) fn last_window_not_ours() -> Option<String> {
    LAST_NOT_OURS.lock().ok()?.clone()
}

/// The last look, kept so it can be asked for.
///
/// The watcher only speaks when the answer changes, which means the first thing it says
/// is said at startup — before the page exists to hear it. Without somewhere to ask, a
/// toolbar opened onto a desktop nobody then touches never learns which window is in
/// front, and every mark stays pinned to the screen exactly as it did before any of this
/// was written. That was the bug: not the tracking, the not-knowing.
static LAST_LOOK: Mutex<Option<InFront>> = Mutex::new(None);

/// What is in front right now, as far as the watcher has seen.
#[tauri::command]
pub(crate) fn colai_in_front() -> Option<InFront> {
    LAST_LOOK.lock().ok().and_then(|held| held.clone())
}

/// Start watching, once, for the life of the app.
pub(crate) fn watch_the_front<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "linux")]
    {
        let app = app.clone();
        std::thread::spawn(move || {
            let Some(mut eyes) = x11::Eyes::open() else {
                // No X display: nothing to watch and nothing to say about it. The page
                // draws every mark, which is exactly what it did before this existed.
                return;
            };
            let mut said: Option<InFront> = None;
            let mut moved = std::time::Instant::now();
            // For as long as the process. There was an `AtomicBool` here under a comment
            // saying the thread stopped when the app did — nothing ever stored `false`,
            // so it was always true and the thread ended because the process did. Saying
            // so is better than a flag that documents a stop nobody implemented.
            loop {
                let now = eyes.in_front();
                if let Ok(mut held) = LAST_LOOK.lock() {
                    held.clone_from(&now);
                }
                // The one place that keeps track of which application somebody is in.
                // It is kept here rather than where it is consumed because this is the
                // only thing that is always looking: a memory that advanced only when
                // the page asked would still be pointing at whatever was in front before
                // somebody brought their application forward.
                if let Some(seen) = now.as_ref().filter(|seen| worth_remembering(seen)) {
                    if let Ok(mut held) = LAST_NOT_OURS.lock() {
                        *held = Some(seen.id.clone());
                    }
                }
                if changed(said.as_ref(), now.as_ref()) {
                    // Only a window that actually moved earns the fast rate. A title
                    // that ticks — a terminal with a clock in it, a tab counting
                    // notifications — changes the answer every look, and treating that
                    // as motion would hold the quick rate for as long as the clock ran.
                    if shifted(said.as_ref(), now.as_ref()) {
                        moved = std::time::Instant::now();
                    }
                    let _ = app.emit_to(crate::colai::OVERLAY_LABEL, FRONT_EVENT, now.clone());
                    said = now;
                }
                std::thread::sleep(how_soon(moved.elapsed()));
            }
        });
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = app;
    }
}

/// Whether the answer is worth sending.
///
/// The whole cost of watching. Somebody working in one window produces one of these
/// every tenth of a second and none of them are worth a message, so the comparison is
/// the thing that keeps this free — and being a plain function, it is the thing that can
/// be shown to be right.
fn changed(said: Option<&InFront>, now: Option<&InFront>) -> bool {
    match (said, now) {
        (None, None) => false,
        (Some(said), Some(now)) => said != now,
        _ => true,
    }
}

/// Whether the window in front is in a different place from the one before it.
///
/// The reason to look quickly, and only this. Everything else about a window can change
/// without anything needing to move on the screen.
fn shifted(said: Option<&InFront>, now: Option<&InFront>) -> bool {
    let place = |front: Option<&InFront>| front.map(|front| (front.id.clone(), front.at));
    place(said) != place(now)
}

#[cfg(target_os = "linux")]
mod x11 {
    use super::InFront;
    use crate::colai::Rect;
    use std::ffi::{c_char, c_int, c_long, c_uchar, c_ulong, c_void, CString};

    #[link(name = "X11")]
    unsafe extern "C" {
        fn XOpenDisplay(name: *const c_char) -> *mut c_void;
        fn XDefaultRootWindow(display: *mut c_void) -> c_ulong;
        fn XInternAtom(display: *mut c_void, name: *const c_char, only_if_exists: c_int)
            -> c_ulong;
        #[allow(clippy::too_many_arguments)]
        fn XGetWindowProperty(
            display: *mut c_void,
            window: c_ulong,
            property: c_ulong,
            long_offset: c_long,
            long_length: c_long,
            delete: c_int,
            req_type: c_ulong,
            actual_type: *mut c_ulong,
            actual_format: *mut c_int,
            nitems: *mut c_ulong,
            bytes_after: *mut c_ulong,
            prop: *mut *mut c_uchar,
        ) -> c_int;
        fn XGetGeometry(
            display: *mut c_void,
            drawable: c_ulong,
            root: *mut c_ulong,
            x: *mut c_int,
            y: *mut c_int,
            width: *mut u32,
            height: *mut u32,
            border: *mut u32,
            depth: *mut u32,
        ) -> c_int;
        #[allow(clippy::too_many_arguments)]
        fn XTranslateCoordinates(
            display: *mut c_void,
            src: c_ulong,
            dest: c_ulong,
            src_x: c_int,
            src_y: c_int,
            dest_x: *mut c_int,
            dest_y: *mut c_int,
            child: *mut c_ulong,
        ) -> c_int;
        fn XFree(data: *mut c_void) -> c_int;
        fn XSetErrorHandler(
            handler: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int>,
        ) -> *mut c_void;
    }

    /// Xlib kills the process on a protocol error unless something says otherwise.
    ///
    /// Not a nicety. Every read here is about a window somebody else owns, and the gap
    /// between learning its id and asking about it is long enough for them to close it —
    /// a `BadWindow` at that moment, with the default handler, ends colai. Ignored is the
    /// correct answer: a window that has gone is a window that is not in front.
    unsafe extern "C" fn ignore(_display: *mut c_void, _error: *mut c_void) -> c_int {
        0
    }

    pub(super) struct Eyes {
        display: *mut c_void,
        root: c_ulong,
        active: c_ulong,
        name: c_ulong,
        utf8: c_ulong,
        pid: c_ulong,
        ours: u32,
        /// The window the answer below was worked out for.
        asked: c_ulong,
        mine: bool,
    }

    impl Eyes {
        pub(super) fn open() -> Option<Self> {
            // A connection of its own, so nothing here can disturb the one GTK is using
            // for the toolbar's own window on the main thread.
            let display = unsafe { XOpenDisplay(std::ptr::null()) };
            if display.is_null() {
                return None;
            }
            unsafe { XSetErrorHandler(Some(ignore)) };
            let atom = |name: &str| -> c_ulong {
                let Ok(text) = CString::new(name) else {
                    return 0;
                };
                unsafe { XInternAtom(display, text.as_ptr(), 0) }
            };
            Some(Self {
                root: unsafe { XDefaultRootWindow(display) },
                active: atom("_NET_ACTIVE_WINDOW"),
                name: atom("_NET_WM_NAME"),
                utf8: atom("UTF8_STRING"),
                pid: atom("_NET_WM_PID"),
                ours: std::process::id(),
                asked: 0,
                mine: false,
                display,
            })
        }

        pub(super) fn in_front(&mut self) -> Option<InFront> {
            let window = self.one_number(self.root, self.active).unwrap_or(0);
            // 0 is what X reports when nothing has focus — a minimised window, a locked
            // screen, the moment between one window closing and the next taking over.
            //
            // Reported as a window with no id rather than as no answer, because those
            // are different things to the page: "nothing is in front" hides every mark
            // that belongs to an application, and "I could not tell" leaves them alone.
            if window == 0 {
                return Some(InFront::default());
            }
            // Whose window it is, asked once per window rather than once per look. A
            // window cannot change process under its own id, and this runs a hundred
            // times a second while somebody drags something.
            if self.asked != window {
                self.asked = window;
                self.mine = self.one_number(window, self.pid) == Some(self.ours as c_ulong);
            }
            Some(InFront {
                id: format!("0x{window:x}"),
                title: self.text(window, self.name),
                at: self.rect(window),
                ours: self.mine,
            })
        }

        fn one_number(&self, window: c_ulong, property: c_ulong) -> Option<c_ulong> {
            if property == 0 {
                return None;
            }
            let mut kind: c_ulong = 0;
            let mut format: c_int = 0;
            let mut count: c_ulong = 0;
            let mut after: c_ulong = 0;
            let mut data: *mut c_uchar = std::ptr::null_mut();
            let ok = unsafe {
                XGetWindowProperty(
                    self.display,
                    window,
                    property,
                    0,
                    1,
                    0,
                    0, // AnyPropertyType
                    &mut kind,
                    &mut format,
                    &mut count,
                    &mut after,
                    &mut data,
                )
            };
            if ok != 0 || data.is_null() || count == 0 || format != 32 {
                if !data.is_null() {
                    unsafe { XFree(data.cast()) };
                }
                return None;
            }
            // 32-bit X properties are handed back as an array of `long`, whatever the
            // wire format says — reading them as u32 is the classic way to get half a
            // number on a 64-bit machine.
            let value = unsafe { *(data as *const c_ulong) };
            unsafe { XFree(data.cast()) };
            Some(value)
        }

        fn text(&self, window: c_ulong, property: c_ulong) -> Option<String> {
            if property == 0 {
                return None;
            }
            let mut kind: c_ulong = 0;
            let mut format: c_int = 0;
            let mut count: c_ulong = 0;
            let mut after: c_ulong = 0;
            let mut data: *mut c_uchar = std::ptr::null_mut();
            let ok = unsafe {
                XGetWindowProperty(
                    self.display,
                    window,
                    property,
                    0,
                    // Titles are read only to tell one tab from another, so a long one
                    // can be cut off: what matters is that it changes when the tab does.
                    256,
                    0,
                    self.utf8,
                    &mut kind,
                    &mut format,
                    &mut count,
                    &mut after,
                    &mut data,
                )
            };
            if ok != 0 || data.is_null() || count == 0 {
                if !data.is_null() {
                    unsafe { XFree(data.cast()) };
                }
                return None;
            }
            let bytes = unsafe { std::slice::from_raw_parts(data, count as usize) };
            let said = String::from_utf8_lossy(bytes).trim().to_string();
            unsafe { XFree(data.cast()) };
            (!said.is_empty()).then_some(said)
        }

        /// Where a window is on the desktop, in the desktop's own coordinates.
        ///
        /// `XGetGeometry` answers relative to the parent, and a window manager reparents
        /// almost everything into a frame — so on its own it reports a position inside a
        /// title bar rather than on the screen. Translating to the root is what turns it
        /// into the number a mark can be drawn from.
        fn rect(&self, window: c_ulong) -> Option<Rect> {
            let mut root: c_ulong = 0;
            let (mut x, mut y) = (0, 0);
            let (mut width, mut height, mut border, mut depth) = (0, 0, 0, 0);
            let ok = unsafe {
                XGetGeometry(
                    self.display,
                    window,
                    &mut root,
                    &mut x,
                    &mut y,
                    &mut width,
                    &mut height,
                    &mut border,
                    &mut depth,
                )
            };
            if ok == 0 || width == 0 || height == 0 {
                return None;
            }
            let (mut screen_x, mut screen_y) = (0, 0);
            let mut child: c_ulong = 0;
            unsafe {
                XTranslateCoordinates(
                    self.display,
                    window,
                    self.root,
                    0,
                    0,
                    &mut screen_x,
                    &mut screen_y,
                    &mut child,
                )
            };
            Some(Rect {
                x: screen_x,
                y: screen_y,
                width: width as i32,
                height: height as i32,
            })
        }
    }

    // The display outlives the thread that opened it and is touched by nothing else.
    unsafe impl Send for Eyes {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn front(id: &str, title: &str) -> InFront {
        InFront {
            id: id.to_string(),
            title: Some(title.to_string()),
            at: Some(crate::colai::Rect {
                x: 100,
                y: 100,
                width: 800,
                height: 600,
            }),
            ours: false,
        }
    }

    /// What the watcher writes down as "the application somebody is working in".
    ///
    /// This is the whole of a bug that reached the agent. A mark carries the address of
    /// the window it was drawn on, and the overlay holds the keyboard at that exact
    /// moment — every mark opens a popup — so the address comes from the last window seen
    /// that was not ours. That memory used only to advance when the page asked for it,
    /// and nothing asks between bringing an application forward and marking something in
    /// it. After a "show desktop" the remembered window was the desktop, so marks arrived
    /// saying "on the desktop", with no path and no application behind them.
    #[test]
    fn the_overlay_is_never_the_application_somebody_is_working_in() {
        let app = front("0x2a", "MarketLab");
        assert!(worth_remembering(&app));

        // Ours. The overlay takes the keyboard whenever a menu or a popup opens, which
        // is the one moment this question is ever asked.
        let mine = InFront {
            ours: true,
            ..front("0x2b", "colai")
        };
        assert!(!worth_remembering(&mine));

        // Nothing has focus: a locked screen, or the moment between one window closing
        // and the next taking over. Not an application either.
        let nobody = InFront::default();
        assert!(!worth_remembering(&nobody));
    }

    #[test]
    fn only_a_window_that_moved_earns_the_quick_rate() {
        // A title that ticks — a terminal with a clock in it, a tab counting
        // notifications — changes the answer on every look. Treating that as motion
        // would hold the quick rate for as long as the clock ran, which is a hundred
        // and twenty-five looks a second for nothing.
        let was = front("0x1", "Prices — Shop");
        let ticking = front("0x1", "(3) Prices — Shop");
        assert!(
            changed(Some(&was), Some(&ticking)),
            "still worth telling the page"
        );
        assert!(!shifted(Some(&was), Some(&ticking)), "but nothing moved");

        let mut dragged = was.clone();
        dragged.at = Some(crate::colai::Rect {
            x: 420,
            y: 100,
            width: 800,
            height: 600,
        });
        assert!(shifted(Some(&was), Some(&dragged)));
        // Another window in front is a different place even at the same rectangle.
        assert!(shifted(Some(&was), Some(&front("0x2", "Prices — Shop"))));
    }

    #[test]
    fn it_keeps_up_with_a_hand_and_costs_nothing_the_rest_of_the_time() {
        // A mark that updates on its own slower clock trails the window it is drawn on,
        // and a dot lagging behind a dragged window reads as broken rather than
        // attached. So while anything is moving it looks faster than any display
        // refreshes.
        assert_eq!(how_soon(std::time::Duration::ZERO), WHILE_MOVING);
        assert!(WHILE_MOVING.as_millis() <= 8, "faster than a frame");

        // And a pause inside a drag — a hand stops far more often than it finishes — is
        // still part of the drag.
        assert_eq!(how_soon(STAYS_LIVELY / 2), WHILE_MOVING);

        // Nothing has happened for a while: back to a rate nobody pays for.
        assert_eq!(how_soon(STAYS_LIVELY), WHEN_STILL);
        assert_eq!(how_soon(std::time::Duration::from_secs(60)), WHEN_STILL);
        assert!(
            WHEN_STILL > WHILE_MOVING * 4,
            "idle is meaningfully cheaper"
        );
    }

    #[test]
    fn working_in_one_window_says_nothing() {
        // Eight of these a second, and none of them are news. This comparison is the
        // entire cost of watching.
        let same = front("0x1", "Prices — Shop");
        assert!(!changed(Some(&same), Some(&same.clone())));
        assert!(!changed(None, None));
    }

    #[test]
    fn switching_window_tab_or_place_is_news() {
        let was = front("0x1", "Prices — Shop");
        // Another application.
        assert!(changed(Some(&was), Some(&front("0x2", "Prices — Shop"))));
        // The same window, another tab: one browser window keeps one id, and the title
        // is the only thing that moved.
        assert!(changed(Some(&was), Some(&front("0x1", "Basket — Shop"))));
        // The same window, dragged. Marks ride along, so the page has to hear about it.
        let mut moved = was.clone();
        moved.at = Some(crate::colai::Rect {
            x: 400,
            y: 100,
            width: 800,
            height: 600,
        });
        assert!(changed(Some(&was), Some(&moved)));
    }

    #[test]
    fn the_screen_going_dark_and_coming_back_are_both_news() {
        let was = front("0x1", "Prices — Shop");
        assert!(changed(Some(&was), None));
        assert!(changed(None, Some(&was)));
    }

    #[test]
    fn the_toolbar_taking_the_keyboard_is_a_different_answer() {
        // And has to be, or the page cannot tell "you are looking at something else"
        // from "you reached for the toolbar" — which decides whether every mark on the
        // screen disappears.
        let was = front("0x1", "Prices — Shop");
        let mut ours = was.clone();
        ours.ours = true;
        assert!(changed(Some(&was), Some(&ours)));
    }
}
