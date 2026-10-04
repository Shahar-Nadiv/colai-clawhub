//! The toolbar: a sheet of glass over the whole desktop.
//!
//! A sibling of Quick Chat. Both are small, always-on-top surfaces over the desktop
//! rather than pages of the dashboard, both have their UI in `apps/linux/ui/`, and both
//! are reached from a menu. What this one adds is the screen as a workspace: a toolbar
//! over any window, regions marked on it, and a gate that refuses to change a surface no
//! connector owns.
//!
//! Two things make an overlay usable rather than a sheet over somebody's work, and
//! both are here.
//!
//! **The shape.** The window covers the display, and an X input shape decides where
//! Colai actually exists on it. Everywhere else, clicks fall through to the
//! application underneath.
//!
//! Input only — this is `ShapeInput`, not `ShapeBounding`, so an unlisted region still
//! *paints*, it just cannot be clicked. Transparency is what makes the rest invisible,
//! and the two have to agree: a page that draws something outside its reported shape
//! shows a control nobody can press. Which is why the page measures the union of the
//! rail with everything it draws rather than the rail alone.
//!
//! **The way out.** Escape is handled here, in the process that a wedged page cannot
//! wedge. An overlay that catches every click and stops responding is a desktop
//! nobody can use, and there has to be a release that does not go through the webview.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

pub(crate) const OVERLAY_LABEL: &str = "colai-overlay";

/// A rectangle in physical pixels: a region of the overlay Colai has claimed, or the
/// window a mark was made over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Where a mark is, as far as the desktop will say without being asked nicely.
///
/// A point on a screen means nothing to somebody who cannot see the screen. This is the
/// address that goes with the picture: which application, which window and how big it
/// is, and — the strongest fact here — the directory the process is sitting in.
///
/// Every field is measured rather than inferred. What the *title* implies about a file
/// or a page is read on the page side, where it can be a pure rule with tests on it, and
/// is labelled there as having been read rather than known.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Front {
    pub app: String,
    pub title: String,
    pub id: String,
    /// The window's own rectangle on the desktop, so a mark can be given in coordinates
    /// that still mean something after somebody moves the window.
    pub at: Option<Rect>,
    pub pid: Option<u32>,
    /// The binary behind it, by its own name.
    pub exe: Option<String>,
    /// Where that process is working.
    ///
    /// The most useful thing on this struct and the cheapest to get. Probed on this
    /// machine: a window whose `WM_CLASS` was `steam_app_<number>` — a number, useless —
    /// sat in a directory that named the application exactly. For an editor or a
    /// terminal it names the repository somebody is asking about.
    ///
    /// Not always the window's own process, though. An Electron editor launched from the
    /// desktop leaves its window in the home directory and does its work in children;
    /// `where_the_work_is` asks them when the window says nothing. Empty rather than
    /// misleading — a directory that names nothing is not sent at all.
    pub cwd: Option<String>,
    /// The document this window was opened with, if it was opened with one.
    ///
    /// The strongest thing colai can know about a window and the one an agent most needs:
    /// an exact path, where the title gives a basename and the directory gives a folder.
    pub opened: Option<String>,
}

/// The edges of the overlay the desktop's own chrome is using, in physical pixels.
///
/// Colai keeps out of these. It is the only arrangement where both the toolbar and the
/// desktop's panels stay visible, because on GNOME the shell *is* the compositor and
/// draws its panel and dock above every client window — `always_on_top`, a `DOCK` type
/// hint and `_NET_WM_STATE_ABOVE` all lose to it. Measured on this machine: the rail
/// docked to the left edge and Ubuntu's dock drew straight over it.
///
/// The other way to win is a fullscreen window, which makes the shell yield its chrome.
/// That is rejected on two counts. It hides the dock, and somebody using Colai should
/// not lose their desktop to it. And it kills transparency: a fullscreen window is
/// unredirected, scanned out with no compositor to blend its alpha, so the overlay
/// turned into an opaque sheet — sampled at 11,16,29 across a whole monitor.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub(crate) struct Reserved {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

/// The last shape asked for, so an identical request costs nothing.
///
/// The page recomputes its shape on every render and on every mutation of the rail,
/// which is many times a second while a flyout animates. Each one would otherwise be an
/// X round trip.
#[derive(Default)]
pub(crate) struct ShapeState(Mutex<Option<Vec<(i32, i32, i32, i32)>>>);

/// Make the overlay, or return the one that exists.
///
/// Sized to the monitor rather than fullscreened: a real fullscreen window on X11 asks
/// the window manager to give it a workspace of its own, which is the opposite of what
/// an overlay wants. `skip_taskbar` and no decorations keep it out of the alt-tab list
/// and off the panel — it is not a window somebody switches to, it is a layer.
pub(crate) fn ensure_overlay(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        return Ok(window);
    }

    let window =
        WebviewWindowBuilder::new(app, OVERLAY_LABEL, WebviewUrl::App("toolbar.html".into()))
            .title("colai")
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .shadow(false)
            // Not focused on creation. Summoning Colai should not take the keyboard away from
            // whatever somebody is in the middle of typing into.
            .focused(false)
            .visible(false)
            .build()
            .map_err(|error| format!("Could not create the colai overlay: {error}"))?;

    cover_everything(&window)?;

    /*
     * Catch nothing, until the page says otherwise.
     *
     * A new X window's input region is the whole window, and this one is transparent,
     * always on top, and the size of every display put together. Between creating it and
     * the page's first `colai_shape` there was therefore an invisible sheet of glass over
     * the entire desktop swallowing every click — and if the page were slow to start, or
     * threw once before its first render, that sheet stayed there for the life of the
     * process. The desktop looked completely normal and nothing on it could be used.
     *
     * So the shape is set here, before the webview has run a line: colai starts out
     * catching nothing and only ever claims what it has drawn. Failing closed is the
     * only safe direction for a window this size — the cost of being wrong the other way
     * is somebody's whole machine.
     *
     * And not behind a `cfg`. It was, and that quietly undid the whole paragraph above:
     * `apply_shape` refuses on any platform that has no implementation yet, exactly so
     * this cannot happen — but the *call* was Linux-only, so on Windows nothing asked,
     * nothing refused, and the window went up catching every click on the desktop with
     * nothing on screen looking wrong. The refusal has to be reachable to be a refusal.
     */
    apply_shape(&window, &[])?;

    // Start out claiming nothing. On Windows the overlay is a full transparent sheet and
    // click-through is a property of the window, not a region — so until the page has drawn
    // a rail and `watch_clickthrough` begins handing it the mouse, the window must let every
    // click fall through to the desktop rather than swallow it. On the main thread, for the
    // same reason the watcher is (see `watch_clickthrough`): a cross-thread style change can
    // deadlock the owning thread.
    //
    // A Mac is in exactly the same position — AppKit has no input-only region either, only
    // `ignoresMouseEvents` for the whole window — so the very same opening move applies.
    #[cfg(target_os = "macos")]
    float_over_every_space(&window);
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let on_window = window.clone();
        let _ = app.run_on_main_thread(move || {
            let _ = on_window.set_ignore_cursor_events(true);
        });
    }

    // Nothing to rouse. This used to wake a shared, lazily-connected client that would
    // otherwise report itself unreachable to whichever surface had not activated it; a
    // `claude` is started per conversation and needs no such nudge.
    Ok(window)
}

/// Cover every display, not the main one.
///
/// The toolbar is a layer over the desktop, and somebody with two screens has one
/// desktop. Sized to a single monitor it could not be dragged onto the other screen —
/// there was no window there to drag it into — and, worse and more quietly, every tool
/// stopped working over there: the layer that catches a drag simply did not exist on
/// that half of the desk, so marking a region on the laptop screen did nothing at all
/// and looked like a broken toolbar rather than a missing window.
fn cover_everything(window: &WebviewWindow) -> Result<(), String> {
    let screens: Vec<Span> = desk_monitors(window)
        .map_err(|error| format!("Could not read the displays: {error}"))?
        .iter()
        .map(|monitor| {
            let at = *monitor.position();
            let size = *monitor.size();
            Span {
                x: at.x,
                y: at.y,
                width: size.width,
                height: size.height,
            }
        })
        .collect();
    let all = spanning(&screens).ok_or_else(|| "There is no display to draw on.".to_string())?;
    window
        .set_position(PhysicalPosition::new(all.x, all.y))
        .map_err(|error| format!("Could not place the overlay: {error}"))?;
    window
        .set_size(PhysicalSize::new(all.width, all.height))
        .map_err(|error| format!("Could not size the overlay: {error}"))?;
    keep_composited(window);
    Ok(())
}

/// The monitors the overlay is meant to cover: all of them.
#[cfg(not(target_os = "macos"))]
fn desk_monitors(window: &WebviewWindow) -> tauri::Result<Vec<tauri::Monitor>> {
    window.available_monitors()
}

/// The monitors the overlay is meant to cover on a Mac: the main one, and only that.
///
/// Not a preference, a limit of the platform. With "Displays have separate Spaces" turned on
/// — the default since 10.9 — every display is its own Space, and a window belongs to exactly
/// one of them: a window sized across two screens is drawn on whichever holds most of it and
/// simply cut off at that screen's edge. Spanning the desk the way X11 and Windows allow
/// would leave half the overlay invisible and the rail docked to an edge nobody can see.
///
/// So, for now, one screen — the one with the menu bar, which is also the one whose top-left
/// corner is the origin of every global coordinate AppKit and CoreGraphics hand out. That
/// makes the overlay's own origin `(0, 0)` in points, which `watch_clickthrough` relies on.
/// A window per display is the way to reach the others, and it is a later piece of work.
#[cfg(target_os = "macos")]
fn desk_monitors(window: &WebviewWindow) -> tauri::Result<Vec<tauri::Monitor>> {
    Ok(window.primary_monitor()?.into_iter().collect())
}

/// Make the overlay a layer on a Mac, rather than one more application window.
///
/// `always_on_top` already lifts it to the floating level, above every ordinary window and
/// below the menu bar and the Dock — which is the right place, since the rail keeps out of
/// both through the reserved edges `colai_screens` reports. What Tauri does not say is how
/// the window behaves when somebody changes Space, and the AppKit default is to stay behind
/// on the Space it was opened on: swipe to another desktop, or put an app in full screen,
/// and the toolbar is gone. So, in AppKit's own words:
///
/// - `CanJoinAllSpaces` — present on every Space, not just the one it opened on;
/// - `Stationary` — not swept away with the windows when Mission Control is shown;
/// - `FullScreenAuxiliary` — allowed over another app's full-screen Space, which is where
///   people most often want to point at something;
/// - `IgnoresCycle` — not something Cmd-` cycles to; it is a layer, not a document.
///
/// Off the Dock and the app switcher is the activation policy's job, set once in `main`.
/// On the main thread, because every AppKit call on a window is the main thread's to make.
#[cfg(target_os = "macos")]
fn float_over_every_space(window: &WebviewWindow) {
    use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};

    let target = window.clone();
    let _ = window.run_on_main_thread(move || {
        let Ok(raw) = target.ns_window() else {
            return;
        };
        if raw.is_null() {
            return;
        }
        // SAFETY: `ns_window` hands back the live `NSWindow` behind this Tauri window, owned
        // by the window itself, and it is only touched here on the main thread.
        let ns_window: &NSWindow = unsafe { &*raw.cast::<NSWindow>() };
        ns_window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        // Already asked of the builder; said again here because a transparent window that
        // keeps its shadow draws a faint outline of the whole screen around nothing.
        ns_window.setHasShadow(false);
    });
}

/// Keep the overlay over the desk, for as long as there is one.
///
/// Its own thread and its own job, beside `watch_the_front` rather than inside it: what is in
/// front changes every few seconds, and what shape the desk is changes a handful of times a
/// day. Asking the display server about its monitors at the rate of the first to learn the
/// answer to the second would be the most expensive thing either loop did.
pub(crate) fn watch_the_desk(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(HOW_OFTEN_THE_DESK_MIGHT_MOVE);
        let asking = app.clone();
        // Moving a window is the main thread's to do.
        let _ = app.run_on_main_thread(move || {
            if let Some(window) = asking.get_webview_window(OVERLAY_LABEL) {
                recover_if_the_desk_moved(&window);
            }
        });
    });
}

/// How often to ask whether the monitors have changed.
///
/// Slow on purpose. The cost of being two seconds late to a monitor being unplugged is two
/// seconds of an overlay in the wrong place; the cost of asking constantly is paid always.
const HOW_OFTEN_THE_DESK_MIGHT_MOVE: std::time::Duration = std::time::Duration::from_secs(2);

/// Put the overlay back over the desk, if the desk has moved out from under it.
///
/// `cover_everything` is right and always was; what was wrong was *when* it ran. It fired
/// when the overlay was made, when somebody summoned it, and when the page asked for the
/// screens — which is to say only ever in answer to a person. Unplug a display while the
/// toolbar is up and idle and none of those happen: the window manager shoves the now
/// oversized overlay sideways to fit what is left, and it stays there with half of it past
/// the edge of the only screen. Every tool still works. Half the desk is simply unreachable,
/// which reads as a broken toolbar rather than a misplaced one.
///
/// Compared before it is set, because this runs on a timer and a `set_position` every two
/// seconds is a fight with the window manager that nobody asked for.
fn recover_if_the_desk_moved(window: &WebviewWindow) {
    let Ok(monitors) = desk_monitors(window) else {
        return;
    };
    let screens: Vec<Span> = monitors
        .iter()
        .map(|monitor| {
            let at = *monitor.position();
            let size = *monitor.size();
            Span { x: at.x, y: at.y, width: size.width, height: size.height }
        })
        .collect();
    let Some(all) = spanning(&screens) else {
        return;
    };
    let placed = window
        .outer_position()
        .is_ok_and(|at| at.x == all.x && at.y == all.y);
    let sized = window
        .outer_size()
        .is_ok_and(|size| size.width == all.width && size.height == all.height);
    if placed && sized {
        return;
    }
    if let Err(trouble) = cover_everything(window) {
        eprintln!("[colai] the desk moved and the overlay could not follow: {trouble}");
    }
}

/// A rectangle of the desktop, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Span {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// The one rectangle that holds every screen.
///
/// Screens are not laid out in a grid and need not touch: a second display can sit above
/// and to the left of the first, which is why this is a union rather than a sum of
/// widths. A gap between two screens ends up inside the span and belongs to nobody; the
/// overlay draws nothing there and the pointer passes straight through it.
pub(crate) fn spanning(screens: &[Span]) -> Option<Span> {
    let first = screens.first()?;
    let mut left = first.x;
    let mut top = first.y;
    let mut right = first.x.saturating_add(first.width as i32);
    let mut bottom = first.y.saturating_add(first.height as i32);
    for screen in screens.iter().skip(1) {
        left = left.min(screen.x);
        top = top.min(screen.y);
        right = right.max(screen.x.saturating_add(screen.width as i32));
        bottom = bottom.max(screen.y.saturating_add(screen.height as i32));
    }
    Some(Span {
        x: left,
        y: top,
        width: (right - left).max(1) as u32,
        height: (bottom - top).max(1) as u32,
    })
}

/// Tell the window manager this is a layer, not a fullscreen application.
///
/// `DOCK` is what this window actually is: a panel-like surface that sits above other
/// windows, takes no focus, and is not something anybody alt-tabs to. Saying so is
/// worth doing on its own — mutter places and stacks a hinted window predictably, where
/// an undecorated `NORMAL` window the exact size of a monitor is a candidate for
/// *unredirection*, and an unredirected window is scanned out without a compositor to
/// blend its alpha against the desktop.
///
/// That unredirection is real and was measured, after a false start: the black screen
/// that first suggested it turned out to be a fullscreen game on the same monitor. Made
/// properly fullscreen on purpose afterwards, this window went opaque — 11,16,29
/// sampled right across the display, the page's own ground with nothing behind it. So
/// the overlay is monitor-sized and hinted, never fullscreen.
#[cfg(target_os = "linux")]
fn keep_composited(window: &WebviewWindow) {
    use gtk::prelude::GtkWindowExt;

    if let Ok(gtk_window) = window.gtk_window() {
        gtk_window.set_type_hint(gdk::WindowTypeHint::Dock);
    }
}

#[cfg(not(target_os = "linux"))]
fn keep_composited(_window: &WebviewWindow) {}

/// Where Colai exists on the screen. Everywhere else belongs to the desktop.
///
/// Through GTK rather than raw X, and that is not a style preference. A shape set with
/// `XShapeCombineRectangles` behind GTK's back lands — X reports it back immediately —
/// and is then undone the moment GTK re-applies its own input region on the next
/// configure. The last writer wins, and GTK writes last. `input_shape_combine_region`
/// makes the shape GTK's own idea of the window, so nothing overwrites it.
#[tauri::command]
pub(crate) fn colai_shape(app: AppHandle, rects: Vec<Rect>) -> Result<(), String> {
    let key: Vec<(i32, i32, i32, i32)> = rects
        .iter()
        .map(|r| (r.x, r.y, r.width, r.height))
        .collect();

    let window = app
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "The overlay is not open.".to_string())?;

    /*
     * The lock is held across the apply, not only across the comparison.
     *
     * Commands run on a thread pool, so two renders can be in here at once. Releasing
     * the lock first let them record their shapes in one order and apply them in the
     * other — leaving the window shaped to a set the page had already replaced. Whatever
     * the stale set was missing is drawn and dead: the pointer falls straight through a
     * panel that is plainly on screen, which is how the library window opened with a
     * close button nothing could press.
     *
     * Recorded only once it is really applied, so a failure cannot make the next
     * identical call believe there is nothing to do.
     */
    let state = app.state::<ShapeState>();
    let mut held = state
        .0
        .lock()
        .map_err(|_| "shape state poisoned".to_string())?;
    if held.as_deref() == Some(key.as_slice()) {
        return Ok(());
    }
    apply_shape(&window, &key)?;
    *held = Some(key);
    Ok(())
}

/// What the overlay's input region must actually be made of.
///
/// **An empty shape is not "no shape".** Clearing the input region entirely hands the
/// window *everything* back, and this window is transparent, always on top, and the size
/// of every display put together — so "catch nothing" written the obvious way produces a
/// sheet of glass over the whole desktop that swallows every click, with nothing visibly
/// wrong. When colai is drawing nothing it exists nowhere, and nowhere is one pixel
/// rather than zero rectangles.
///
/// Pulled out of the GTK call so the rule is a function with tests on it. It is the one
/// piece of this file where being wrong costs somebody their machine, and it was
/// previously only expressible by reading the code.
// Drawn only on Linux, but the rule the tests exercise on every platform, so it is
// silenced rather than cfg-gated away from the test build.
#[allow(dead_code)]
fn shape_rects(rects: &[(i32, i32, i32, i32)]) -> Vec<(i32, i32, i32, i32)> {
    if rects.is_empty() {
        return vec![(0, 0, 1, 1)];
    }
    rects.to_vec()
}

#[cfg(target_os = "linux")]
fn apply_shape(window: &WebviewWindow, rects: &[(i32, i32, i32, i32)]) -> Result<(), String> {
    use gtk::prelude::WidgetExt;

    let gtk_window = window
        .gtk_window()
        .map_err(|error| format!("Could not reach the overlay's GTK window: {error}"))?;

    let region = cairo::Region::create();
    for (x, y, width, height) in &shape_rects(rects) {
        region
            .union_rectangle(&cairo::RectangleInt::new(*x, *y, *width, *height))
            .map_err(|error| format!("Could not build the overlay's shape: {error}"))?;
    }

    gtk_window.input_shape_combine_region(Some(&region));
    Ok(())
}

/// Click-through on Windows, the way X11 does it in spirit: shape the *input*, never the
/// drawing.
///
/// The first attempt clipped the window with `SetWindowRgn`, and that was wrong twice over.
/// A window region clips what is drawn as well as what is clicked — and on a transparent,
/// DWM-composited WebView2 window it stops the content being presented at all, so the rail
/// rendered but never reached the screen. X11's `input_shape_combine_region` leaves the
/// drawing whole and shapes only the mouse; Windows has no input-region call, so the
/// equivalent is to keep the window full and visible and toggle whether it takes the mouse.
///
/// So there is nothing to do per shape call. The overlay stays a full, visible window;
/// `watch_clickthrough` reads the drawn rectangles from `ShapeState` and, as the pointer
/// moves, hands the mouse to the rail when it is over one and lets it fall through to the
/// desktop everywhere else — which is exactly what the X11 input shape achieves, arrived at
/// from the other direction.
#[cfg(target_os = "windows")]
fn apply_shape(_window: &WebviewWindow, _rects: &[(i32, i32, i32, i32)]) -> Result<(), String> {
    Ok(())
}

/// Hand the rail the mouse, and let everything else fall through — by hit-testing, live.
///
/// One thread for the life of the app. Each tick it asks where the pointer is and whether
/// that lands on any rectangle the page last drew (`ShapeState`, in the page's CSS pixels),
/// and flips the overlay between catching the mouse and ignoring it. Ignoring is the safe
/// default and the resting state: an overlay that is claiming nothing claims nothing.
///
/// Coordinates cross two spaces. `GetCursorPos` answers in physical screen pixels — the
/// process is per-monitor DPI aware — and the drawn rectangles are the page's CSS pixels
/// relative to the overlay's top-left, so the cursor is moved into that same space before it
/// is compared: subtract the overlay's origin, divide by its scale factor.
///
/// The flip is a bare `WS_EX_TRANSPARENT` toggle via `SetWindowLongPtrW`, not Tauri's
/// `set_ignore_cursor_events`. The earlier version queued that onto the main thread with
/// `run_on_main_thread`, and that is what jammed: the call did not return, this thread
/// stopped ticking, and `ignore` stuck at `true` — the whole overlay went dead to the mouse
/// while looking normal. Toggling the extended-style bit directly is a cached attribute
/// write: it sends no window messages, so it cannot block on the main thread, and the change
/// is read on the next hit-test, which is exactly when it is needed. (`SetWindowPos`, which
/// *does* reframe and message, is deliberately not called — that reframing was the original
/// AppHang.)
#[cfg(target_os = "windows")]
pub(crate) fn watch_clickthrough(app: &AppHandle) {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_TRANSPARENT,
    };

    let app = app.clone();
    std::thread::spawn(move || {
        // Resolve the handle and the scale ONCE, up front, while the app is still calm. After
        // this the hot loop touches only bare Win32 on that handle — no `get_webview_window`,
        // no `outer_position`, no `scale_factor`. Those are Tauri calls that lock against the
        // main thread, and when the main thread was busy (a menu animating, a capture running)
        // one of them would not return: this thread stopped ticking and the overlay jammed —
        // stuck ignoring the mouse, so every click fell through to the app behind it. The loop
        // below cannot block on the main thread because it never calls into it.
        let hwnd = loop {
            if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
                if let Ok(handle) = window.hwnd() {
                    break handle.0 as _;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        // The overlay's scale is its monitor's, and does not change under it in normal use;
        // read once rather than every tick. `GetWindowRect` gives the live origin cheaply.
        let scale = app
            .get_webview_window(OVERLAY_LABEL)
            .and_then(|window| window.scale_factor().ok())
            .unwrap_or(1.0);

        let mut ignoring: Option<bool> = None;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(16));
            let mut point = POINT { x: 0, y: 0 };
            // SAFETY: `point` is a plain out-parameter the call fills in.
            if unsafe { GetCursorPos(&mut point) } == 0 {
                continue;
            }
            let over_rail = over_rail_win(&app, hwnd, scale, point.x, point.y);
            // Over the rail: take the mouse. Anywhere else: let it fall to the desktop.
            let ignore = !over_rail;
            if ignoring != Some(ignore) {
                // SAFETY: reading and writing this window's own extended style. The
                // `WS_EX_TRANSPARENT` bit alone is toggled; every other bit (notably
                // `WS_EX_LAYERED`, which the transparency depends on) is preserved.
                unsafe {
                    let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                    let next = if ignore {
                        style | (WS_EX_TRANSPARENT as isize)
                    } else {
                        style & !(WS_EX_TRANSPARENT as isize)
                    };
                    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next);
                }
                ignoring = Some(ignore);
            }
        }
    });

    // SAFETY note kept next to the loop: `GetWindowRect` reads this window's own screen rect,
    // a direct kernel call that does not message or lock the owning thread.
    fn over_rail_win(
        app: &AppHandle,
        hwnd: windows_sys::Win32::Foundation::HWND,
        scale: f64,
        screen_x: i32,
        screen_y: i32,
    ) -> bool {
        use windows_sys::Win32::Foundation::RECT;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;

        let state = app.state::<ShapeState>();
        let Ok(held) = state.0.lock() else {
            return false;
        };
        let Some(rects) = held.as_ref().filter(|rects| !rects.is_empty()) else {
            return false;
        };
        let mut rc = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: `rc` is a plain out-parameter for this window's own rect.
        if unsafe { GetWindowRect(hwnd, &mut rc) } == 0 {
            return false;
        }
        let scale = if scale <= 0.0 { 1.0 } else { scale };
        // Physical screen → overlay-relative CSS pixels, the space the rectangles are in.
        let x = f64::from(screen_x - rc.left) / scale;
        let y = f64::from(screen_y - rc.top) / scale;
        rects.iter().any(|(rx, ry, width, height)| {
            x >= f64::from(*rx)
                && x < f64::from(rx + width)
                && y >= f64::from(*ry)
                && y < f64::from(ry + height)
        })
    }
}

/// Click-through on a Mac: the Windows arrangement, for the Windows reason.
///
/// AppKit has no input region either. A window takes the mouse or it does not —
/// `ignoresMouseEvents`, all or nothing — and while a non-opaque window *may* let clicks on
/// fully clear pixels through, whether it does depends on how the view is layer-backed, and a
/// WKWebView's is not something to bet somebody's desktop on. So the overlay stays whole and
/// `watch_clickthrough` flips it by hit-testing the pointer against the drawn rectangles, and
/// there is nothing to do per shape call: `ShapeState` holding the rectangles is the shape.
#[cfg(target_os = "macos")]
fn apply_shape(_window: &WebviewWindow, _rects: &[(i32, i32, i32, i32)]) -> Result<(), String> {
    Ok(())
}

/// Hand the rail the mouse on a Mac, and let everything else fall through.
///
/// The Windows watcher's loop, with each Win32 piece swapped for its Mac counterpart, and
/// with the lesson that one learned the hard way kept: the hot loop never waits on the main
/// thread.
///
/// - The pointer comes from CoreGraphics — a fresh `CGEvent`'s location — which is answered
///   by the window server on any thread. Tauri's own `cursor_position` would have been the
///   portable choice, and it is the wrong one here: it is a getter, and a getter waits for
///   the main thread to reply, which is precisely the wait that jammed the Windows overlay.
/// - The flip is Tauri's `set_ignore_cursor_events`. That is a *setter*: from any thread but
///   the main one it is posted to the event loop and returns at once, never waiting for it to
///   land. AppKit's `setIgnoresMouseEvents:` has to run on the main thread anyway, so posting
///   it there is not a detour, it is the only correct route.
///
/// No coordinate conversion to speak of. CoreGraphics answers in global points with the main
/// display's top-left as the origin; the overlay covers exactly that display (see
/// `desk_monitors`), so its origin is that same corner; and the page's CSS pixels *are*
/// points, because WKWebView lays out in points whatever the backing scale. The pointer is
/// already in the space the rectangles are in.
#[cfg(target_os = "macos")]
pub(crate) fn watch_clickthrough(app: &AppHandle) {
    use core_graphics::event::CGEvent;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let app = app.clone();
    std::thread::spawn(move || {
        // One event source for the life of the thread. Asking for the pointer is then a
        // single event made and dropped per tick, with nothing to look up.
        let source = loop {
            if let Ok(source) = CGEventSource::new(CGEventSourceStateID::CombinedSessionState) {
                break source;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        };

        let mut ignoring: Option<bool> = None;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(16));
            let Ok(event) = CGEvent::new(source.clone()) else {
                continue;
            };
            let at = event.location();
            // Over the rail: take the mouse. Anywhere else: let it fall to the desktop.
            let ignore = !over_the_rail(&app, at.x, at.y);
            if ignoring == Some(ignore) {
                continue;
            }
            // Looked up on each flip rather than held: a flip is rare next to a tick, and a
            // window looked up fresh is never one that has since been closed. The lookup is
            // the manager's own map, not a trip to the main thread.
            let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
                // No overlay yet. Leave `ignoring` unknown so the first one gets told.
                ignoring = None;
                continue;
            };
            if window.set_ignore_cursor_events(ignore).is_ok() {
                ignoring = Some(ignore);
            }
        }
    });

    /// Whether a point, in overlay CSS pixels, lands on anything the page last drew.
    fn over_the_rail(app: &AppHandle, x: f64, y: f64) -> bool {
        let state = app.state::<ShapeState>();
        let Ok(held) = state.0.lock() else {
            return false;
        };
        let Some(rects) = held.as_ref() else {
            return false;
        };
        rects.iter().any(|(rx, ry, width, height)| {
            x >= f64::from(*rx)
                && x < f64::from(rx + width)
                && y >= f64::from(*ry)
                && y < f64::from(ry + height)
        })
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn apply_shape(_window: &WebviewWindow, _rects: &[(i32, i32, i32, i32)]) -> Result<(), String> {
    // Anywhere else click-through has no implementation at all. Reporting success here
    // would make the overlay swallow the desktop while looking correct.
    Err("Per-region click-through is implemented for X11, Windows and macOS only.".to_string())
}

/// Which application is in front, so a refusal can name it.
///
/// **Provisional.** The surface registry — window to connector, health-polled — is the
/// daemon's job and is not written. Until it is, the toolbar still has to answer "which
/// app is this" or its refusal has nothing to refuse about. `xprop` is read-only, ships
/// with x11-utils on every desktop Ubuntu, and depending on a binary that is already
/// there beats compiling an X binding for something being replaced.
///
/// Null when it cannot tell, which reads as "no connector" — the safe answer rather
/// than the convenient one.
/// Asked off the main thread, because answering it is three programs and a `/proc` walk.
///
/// A synchronous Tauri command runs on the thread that draws, and this one is not cheap:
/// two `xprop` runs, an `xwininfo`, and a walk of `/proc` to name the process behind the
/// window. That is milliseconds of the UI thread every time somebody makes a mark — the
/// one moment they are watching the screen closely — and it showed as the toolbar
/// hesitating under the hand that was drawing on it.
///
/// `spawn_blocking` rather than plain `async`: the work is blocking whichever thread it
/// lands on, and an async command lands on a tokio worker shared with everything else.
/// Nothing here touches GDK, which is what makes moving it off the main thread safe at
/// all — the contact sheet next door is the counter-example.
#[tauri::command]
pub(crate) async fn colai_frontmost() -> Option<Front> {
    #[cfg(target_os = "linux")]
    {
        tokio::task::spawn_blocking(frontmost_x11).await.ok().flatten()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn frontmost_x11() -> Option<Front> {
    let id = active_window_id()?;
    if let Some(front) = describe_window(&id) {
        return Some(front);
    }
    /*
     * The active window is ours, which it is whenever a menu or a popup is open —
     * including the popup a mark has just opened. What somebody means by "in front" is
     * the application they were working in when they reached for the toolbar.
     *
     * That is the window the watcher last saw that was not ours, described *now* rather
     * than remembered from whenever somebody last happened to ask. The difference is the
     * whole bug: this used to hand back a `Front` cached inside this function, and this
     * function only runs when the page asks — so bringing an application forward and then
     * marking something in it addressed the mark to whatever had been in front before it,
     * which after a "show desktop" is the desktop itself. Marks arrived at the agent
     * saying "on the desktop", with no path and no application.
     */
    crate::colai_attach::last_window_not_ours()
        .filter(|last| last != &id)
        .and_then(|last| describe_window(&last))
        .or_else(remembered_front)
}

/// The window X currently calls active, whoever it belongs to.
#[cfg(target_os = "linux")]
fn active_window_id() -> Option<String> {
    let root = std::process::Command::new("xprop")
        .args(["-root", "_NET_ACTIVE_WINDOW"])
        .output()
        .ok()?;
    let said = String::from_utf8_lossy(&root.stdout);
    let id = said
        .split("# ")
        .nth(1)?
        .split(&[',', '\n'][..])
        .next()?
        .trim();
    // 0x0 is what X reports when nothing has focus — a locked screen, or the moment
    // between one window closing and the next taking it.
    if id.is_empty() || id == "0x0" {
        return None;
    }
    Some(id.to_string())
}

/// Everything worth knowing about one window, or nothing if it is ours or gone.
///
/// The expensive half — two `xprop` runs and a walk of `/proc` — kept here so it can be
/// pointed at a window that is not the active one.
#[cfg(target_os = "linux")]
fn describe_window(id: &str) -> Option<Front> {
    let about = std::process::Command::new("xprop")
        .args(["-id", id, "WM_CLASS", "_NET_WM_NAME", "_NET_WM_PID"])
        .output()
        .ok()?;
    let about = String::from_utf8_lossy(&about.stdout);
    if ours(&about) {
        return None;
    }
    let pid = pid_of(&about);
    let front = Front {
        app: app_name(&about),
        title: window_title(&about),
        id: id.to_string(),
        at: window_rect(id),
        pid,
        exe: pid.and_then(named_link).map(|path| exe_name(&path)),
        cwd: pid.and_then(where_the_work_is),
        opened: pid.and_then(opened_with),
    };
    remember_front(&front);
    Some(front)
}

/// Whether a window belongs to this process.
#[cfg(target_os = "linux")]
fn ours(said: &str) -> bool {
    said.lines()
        .find(|line| line.starts_with("_NET_WM_PID"))
        .and_then(|line| line.rsplit(' ').next())
        .and_then(|pid| pid.trim().parse::<u32>().ok())
        .is_some_and(|pid| pid == std::process::id())
}

/// The last window described here that was not one of ours.
///
/// A second line of defence behind the watcher's own memory, for the moment before the
/// watcher has looked even once and for a remembered window that has since closed.
#[cfg(target_os = "linux")]
static LAST_FRONT: Mutex<Option<Front>> = Mutex::new(None);

#[cfg(target_os = "linux")]
fn remember_front(front: &Front) {
    if let Ok(mut held) = LAST_FRONT.lock() {
        *held = Some(front.clone());
    }
}

#[cfg(target_os = "linux")]
fn remembered_front() -> Option<Front> {
    LAST_FRONT.lock().ok()?.clone()
}

/// The process behind a window, from what `xprop` already reported.
#[cfg(target_os = "linux")]
fn pid_of(said: &str) -> Option<u32> {
    said.lines()
        .find(|line| line.starts_with("_NET_WM_PID"))
        .and_then(|line| line.rsplit(' ').next())
        .and_then(|pid| pid.trim().parse().ok())
}

/// Where a process is working, if it is one of ours to look at.
///
/// `/proc/<pid>/cwd` is a link the kernel keeps for every process, readable by whoever
/// owns it. Nothing is asked of the application and nothing can be refused; it is either
/// there or somebody else's process and it is not.
// This and the `/proc` helpers below it feed `describe_window`, which is Linux-only, while
// the tests exercise them on every platform — so they are silenced rather than cfg-gated
// away from the test build.
#[allow(dead_code)]
fn working_directory(pid: u32) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/cwd"))
        .ok()
        .and_then(|path| path.to_str().map(str::to_string))
        // A process whose cwd is inside /proc is a helper looking at another process,
        // which is true of every browser content process and useful to nobody.
        .filter(|path| !path.starts_with("/proc/"))
}

/// Whether a directory says anything about what somebody is working on.
///
/// A window launched from the desktop inherits the session's own directory, so half the
/// windows on a machine report the home directory and mean nothing by it. Printing that
/// as the window's address is worse than printing nothing: it reads as an answer, and
/// an agent sent to `/home/someone` looking for a project finds a home directory.
///
/// System prefixes go the same way. A program living in `/usr` or `/snap` is telling us
/// where it was installed, not where the work is.
#[allow(dead_code)]
fn tells_us_nothing(path: &str) -> bool {
    if path == "/" || path.starts_with("/proc/") {
        return true;
    }
    if std::env::var("HOME").is_ok_and(|home| !home.is_empty() && path == home) {
        return true;
    }
    ["/usr/", "/snap/", "/opt/", "/etc/", "/var/"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

/// The directory a window's work is actually in.
///
/// The window's own process is asked first and is usually right. Electron editors are
/// the exception that matters: measured on this desktop, VS Code's window process sits
/// in the home directory while three of its children sit in the open workspace. So when
/// the window itself says nothing useful, its descendants are asked.
///
/// The commonest answer wins rather than the first. One child may be a terminal somebody
/// opened somewhere unrelated; the workspace is the directory several of them share.
#[allow(dead_code)]
fn where_the_work_is(pid: u32) -> Option<String> {
    if let Some(own) = working_directory(pid).filter(|path| !tells_us_nothing(path)) {
        return Some(own);
    }
    let family = descendants(pid);
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for child in family {
        if let Some(path) = working_directory(child).filter(|path| !tells_us_nothing(path)) {
            *seen.entry(path).or_default() += 1;
        }
    }
    seen.into_iter()
        // Ties broken by the longer path, so a workspace inside a checkout beats the
        // checkout — deterministic, rather than whichever the map happened to yield.
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.len().cmp(&b.0.len())))
        .map(|(path, _)| path)
}

/// How many processes are walked looking for that directory.
///
/// A browser is hundreds of processes and this runs while somebody is marking something.
/// Generous enough for an editor's whole tree, small enough that the walk is never the
/// reason marking felt slow.
#[allow(dead_code)]
const FAMILY_MOST: usize = 400;

/// Every process descended from this one, breadth-first and bounded.
#[allow(dead_code)]
pub(crate) fn descendants(pid: u32) -> Vec<u32> {
    let mut parents: std::collections::HashMap<u32, Vec<u32>> = std::collections::HashMap::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let Some(child) = entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(status) = std::fs::read_to_string(format!("/proc/{child}/status")) else {
            continue;
        };
        if let Some(parent) = status
            .lines()
            .find_map(|line| line.strip_prefix("PPid:"))
            .and_then(|said| said.trim().parse::<u32>().ok())
        {
            parents.entry(parent).or_default().push(child);
        }
    }
    let mut found = Vec::new();
    let mut looking = vec![pid];
    while let Some(one) = looking.pop() {
        if found.len() >= FAMILY_MOST {
            break;
        }
        for child in parents.get(&one).cloned().unwrap_or_default() {
            found.push(child);
            looking.push(child);
        }
    }
    found
}

/// The document a window was opened with, when it was opened with one.
///
/// `/proc/<pid>/cmdline` carries the arguments a program was started with, and a
/// document-shaped program is usually started with its document: measured on this
/// desktop, KiCad's window names `…/board-name.kicad_pro` exactly, where its title
/// says only `board-name`. That is the difference between an agent opening a file
/// and an agent looking for one.
///
/// Only arguments that exist on disk right now. A flag, a socket name or a stale path is
/// not an address, and offering one as the answer is the confident kind of wrong.
#[allow(dead_code)]
fn opened_with(pid: u32) -> Option<String> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    raw.split(|byte| *byte == 0)
        .skip(1)
        .filter_map(|part| std::str::from_utf8(part).ok())
        .map(str::trim)
        .filter(|part| !part.is_empty() && !part.starts_with('-'))
        // And not the program's own files. A GNOME extension is started with the path of
        // its own script, which exists, is on the command line, and is nobody's document
        // — it is where the thing was installed. The same test the directory gets.
        .filter(|part| !tells_us_nothing(part))
        .find(|part| std::path::Path::new(part).exists())
        .map(str::to_string)
}

#[allow(dead_code)]
fn named_link(pid: u32) -> Option<std::path::PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

#[allow(dead_code)]
fn exe_name(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string()
}

/// A window's rectangle on the desktop.
///
/// One more spawn per mark, which is the price of coordinates that survive the window
/// being moved. `xwininfo` rather than GDK because this runs off the main thread and GDK
/// does not.
#[allow(dead_code)]
fn window_rect(id: &str) -> Option<Rect> {
    let said = std::process::Command::new("xwininfo")
        .args(["-id", id])
        .output()
        .ok()?;
    rect_from(&String::from_utf8_lossy(&said.stdout))
}

/// The four numbers, out of what `xwininfo` prints.
///
/// All four or none. Three of them describe no rectangle, and a rectangle with one
/// guessed edge would put a mark somewhere nobody put it.
#[allow(dead_code)]
fn rect_from(said: &str) -> Option<Rect> {
    let number = |label: &str| -> Option<i32> {
        said.lines()
            .find(|line| line.trim_start().starts_with(label))?
            .rsplit(' ')
            .next()?
            .trim()
            .parse()
            .ok()
    };
    Some(Rect {
        x: number("Absolute upper-left X:")?,
        y: number("Absolute upper-left Y:")?,
        width: number("Width:")?,
        height: number("Height:")?,
    })
}

#[cfg(target_os = "linux")]
fn app_name(said: &str) -> String {
    let line = said
        .lines()
        .find(|line| line.starts_with("WM_CLASS"))
        .unwrap_or_default();
    let quoted: Vec<&str> = line.split('"').skip(1).step_by(2).collect();
    let raw = quoted
        .last()
        .or(quoted.first())
        .copied()
        .unwrap_or_default();
    if raw.is_empty() {
        return "that window".to_string();
    }

    let mut seen: Vec<String> = Vec::new();
    for part in raw.split(['-', '_', '.']).filter(|part| !part.is_empty()) {
        let lower = part.to_lowercase();
        if seen.iter().any(|had| had.to_lowercase() == lower) {
            continue;
        }
        let mut chars = part.chars();
        let capitalised = match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => continue,
        };
        seen.push(capitalised);
    }
    if seen.is_empty() {
        "that window".to_string()
    } else {
        seen.join(" ")
    }
}

#[cfg(target_os = "linux")]
fn window_title(said: &str) -> String {
    said.lines()
        .find(|line| line.starts_with("_NET_WM_NAME"))
        .and_then(|line| line.split('"').nth(1))
        .unwrap_or_default()
        .to_string()
}

/// Take the keyboard, because the popup has things to type into.
///
/// The overlay is hinted as a dock so the shell stacks it predictably above everything
/// and never offers it in the switcher — and a dock is not a thing a window manager
/// hands the keyboard to on its own. That cost nothing while the toolbar was buttons.
/// A mark that can carry a note, and a popup whose way out is a key, both need it.
///
/// Asked for only when a popup opens: taking somebody's keyboard away from what they
/// were typing, at any other moment, would be the overlay behaving like an application.
#[tauri::command]
pub(crate) fn colai_take_keyboard(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "The toolbar is not open.".to_string())?;
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::GtkWindowExt;
        if let Ok(gtk_window) = window.gtk_window() {
            // A dock says "do not focus me" through this hint as well as its type, and
            // the type is the half worth keeping.
            gtk_window.set_accept_focus(true);
        }
    }
    window
        .set_focus()
        .map_err(|error| format!("Could not reach the keyboard: {error}"))
}

/// What a command line asked the toolbar to do.
///
/// The same words arrive two ways and mean the same thing both times: on the arguments
/// this process started with, and through `tauri-plugin-single-instance` when a second
/// copy is run while one is already up. `colai-toolbar toggle` is the second case, and
/// the first is what happens when nothing was running yet.
/// A launch said which conversation it came from; point the rail there.
///
/// Told rather than asked, because the page cannot see a process that has already exited.
/// The overlay may not exist yet — a `/colai:show` that starts the toolbar arrives before
/// there is a window — and that case needs no event: the rail asks `colai_came_from` when
/// it loads, and by then this has already been recorded.
pub(crate) fn followed(app: &AppHandle, args: &[String]) {
    let Some(from) = app.try_state::<crate::session::CameFrom>() else {
        return;
    };
    if !from.heard(crate::session::came_from(args)) {
        return;
    }
    let Some(chat) = from.read() else {
        return;
    };
    // Not quiet. Marks already staged are still staged, and they are now aimed somewhere
    // else — which is a reasonable thing to do and an unreasonable thing to do silently.
    if let Err(trouble) = app.emit_to(OVERLAY_LABEL, CAME_FROM_EVENT, json!({ "chat": chat })) {
        eprintln!("[colai] could not point the toolbar at {chat}: {trouble}");
    }
}

/// What the page hears when the toolbar is pointed at a different conversation.
pub(crate) const CAME_FROM_EVENT: &str = "colai:came-from";

/// The conversation the toolbar belongs to right now, or none.
#[tauri::command]
pub(crate) fn colai_came_from(app: AppHandle) -> Option<String> {
    app.try_state::<crate::session::CameFrom>()
        .and_then(|from| from.read())
}

pub(crate) fn asked_for(app: &AppHandle, args: &[String]) -> Result<(), String> {
    let asked = args.iter().rev().find_map(|word| match word.as_str() {
        "show" | "--show" => Some("show"),
        "hide" | "--hide" => Some("hide"),
        "toggle" | "--toggle" => Some("toggle"),
        "quit" | "--quit" => Some("quit"),
        _ => None,
    });
    // Nothing said is a request for the toolbar. Being run at all is the ask — from the
    // plugin at startup, or from somebody who typed the binary's name.
    match asked.unwrap_or("show") {
        "hide" => colai_release(app.clone()),
        "toggle" if toolbar_is_showing(app) => colai_release(app.clone()),
        /*
         * Going, asked rather than signalled.
         *
         * The plugin used to stop the toolbar by sending SIGTERM to a pid it had found by
         * reading `/proc`. Two problems with that, and the second is the one that matters:
         * finding the pid was Linux-only, and on Windows Node maps SIGTERM to
         * `TerminateProcess` — immediate, with no chance to put anything down.
         *
         * The toolbar already has one way for anything outside it to say something, and
         * it works the same on every platform: run the binary again, and the copy on
         * screen receives the word. So going is a word like the others.
         */
        "quit" => {
            app.exit(0);
            Ok(())
        }
        _ => colai_summon(app.clone()),
    }
}

/// Somebody asked for the toolbar.
#[tauri::command]
pub(crate) fn colai_summon(app: AppHandle) -> Result<(), String> {
    let window = ensure_overlay(&app)?;
    cover_everything(&window)?;
    window
        .show()
        .map_err(|error| format!("Could not show the overlay: {error}"))?;
    // There is no parked connection to resume. Summoning the toolbar used to double as
    // "try reconnecting", because a refused pairing parked a driver until something asked
    // it to retry and nothing here ever did. Nothing to retry now.
    Ok(())
}

/// Whether the toolbar is on screen right now.
///
/// Asked of the window, which is the only thing that knows. A flag kept beside it would be
/// a second answer to a question with one, and the two would disagree the first time the
/// toolbar was put away by a route that forgot to update it.
pub(crate) fn toolbar_is_showing(app: &AppHandle) -> bool {
    app.get_webview_window(OVERLAY_LABEL)
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false)
}

/// Give the screen back.
#[tauri::command]
pub(crate) fn colai_release(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        // Taking the keyboard is a hint turned on, and it was never turned off again: a
        // dock that has once said "you may focus me" goes on saying it, so afterwards
        // every click near the rail could pull focus off the work the toolbar sits over.
        // It matters more now that a global shortcut can take the keyboard from anywhere.
        #[cfg(target_os = "linux")]
        {
            use gtk::prelude::GtkWindowExt;
            if let Ok(gtk_window) = window.gtk_window() {
                gtk_window.set_accept_focus(false);
            }
        }
        window
            .hide()
            .map_err(|error| format!("Could not hide the overlay: {error}"))?;
    }
    if let Some(state) = app.try_state::<ShapeState>() {
        if let Ok(mut held) = state.0.lock() {
            // Forgotten, so the next summon re-applies rather than believing a shape
            // that belonged to a window which is no longer on screen.
            *held = None;
        }
    }
    Ok(())
}

/// One screen, in the overlay's own coordinates, with what the desktop keeps of it.
///
/// Local rather than absolute, because the page thinks in its own window and a second
/// display can start at a negative coordinate. Each screen carries its own reserved
/// edges: a panel belongs to the screen it is on, and four numbers for the whole desk
/// cannot say which one that is.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScreenSpan {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub reserved: Reserved,
}

/// The screens the toolbar has to live on.
///
/// The rail docks to the edges of the screen it is on, not to the edges of the desk.
/// Once the overlay spans two displays those are different things, and the difference is
/// the whole point: the top of the desk is under the shell's panel on one screen and
/// empty air on the other, and an inner edge belongs to both screens and to no edge of
/// the desk at all.
#[tauri::command]
pub(crate) fn colai_screens(app: AppHandle) -> Vec<ScreenSpan> {
    #[cfg(target_os = "linux")]
    {
        let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
            return Vec::new();
        };
        /*
         * Re-covering the desk before answering.
         *
         * The overlay is sized to the union of the monitors when it is made, and nothing
         * told it when a monitor arrived or left. Plug in a second display and the new
         * screen had no overlay over it — every tool stopped working over there, which
         * reads as a broken toolbar, and the only recovery was Escape then summon, which
         * nobody would guess.
         *
         * Here rather than on a GDK signal because this is the question that gets asked
         * whenever the answer might have changed: the page calls it on every render that
         * needs a screen. It is cheap, and it fails quietly — a resize that does not take
         * leaves the overlay where it was, which is where it already is.
         */
        if let Err(trouble) = cover_everything(&window) {
            eprintln!("[colai] could not re-cover the desktop: {trouble}");
        }
        screen_spans(&window).unwrap_or_default()
    }
    #[cfg(target_os = "windows")]
    {
        let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
            return Vec::new();
        };
        // Re-cover the desk before answering, for the same reason the Linux path does: a
        // monitor may have arrived or left since the overlay was last sized.
        if let Err(trouble) = cover_everything(&window) {
            eprintln!("[colai] could not re-cover the desktop: {trouble}");
        }
        screen_spans_win(&window).unwrap_or_default()
    }
    #[cfg(target_os = "macos")]
    {
        let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
            return Vec::new();
        };
        // Re-cover first, as the others do: the main display can change resolution, or
        // stop being the main display, while the toolbar is up.
        if let Err(trouble) = cover_everything(&window) {
            eprintln!("[colai] could not re-cover the desktop: {trouble}");
        }
        screen_spans_mac(&window).unwrap_or_default()
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let _ = app;
        Vec::new()
    }
}

/// The monitors, in the CSS pixels the page lays the rail out in.
///
/// The page positions the rail with DOM coordinates, and those are CSS pixels — the
/// overlay's physical size divided by its scale factor. The Linux path answers in GTK's
/// application pixels, which are the same thing on a 1× desktop, so nothing there converts;
/// here the conversion is explicit, because a scaled display (125%, say) makes physical and
/// CSS diverge and a rail placed at physical `1080` lands below the bottom of an 864-tall
/// window. Without this the page falls back to `innerWidth × devicePixelRatio` — physical
/// pixels — and docks the rail off the screen.
///
/// One scale for every monitor — the overlay's own — because the webview renders the whole
/// spanning surface at a single `devicePixelRatio`: a point at physical X sits at CSS
/// X / scale everywhere on it, whatever each monitor's native scaling is.
#[cfg(target_os = "windows")]
fn screen_spans_win(window: &WebviewWindow) -> Option<Vec<ScreenSpan>> {
    let origin = window.outer_position().ok()?;
    let scale = window.scale_factor().ok()?;
    let to_css = |v: i32| (f64::from(v) / scale).round() as i32;

    let spans: Vec<ScreenSpan> = win_monitors()
        .iter()
        .map(|m| ScreenSpan {
            x: to_css(m.left - origin.x),
            y: to_css(m.top - origin.y),
            width: to_css(m.right - m.left),
            height: to_css(m.bottom - m.top),
            // The chrome the desktop keeps for itself — the taskbar — as the gap between
            // the monitor and its work area, per edge, in the same CSS pixels.
            reserved: Reserved {
                top: to_css((m.work_top - m.top).max(0)),
                left: to_css((m.work_left - m.left).max(0)),
                right: to_css((m.right - m.work_right).max(0)),
                bottom: to_css((m.bottom - m.work_bottom).max(0)),
            },
        })
        .collect();
    (!spans.is_empty()).then_some(spans)
}

/// The screen the overlay covers on a Mac, in the CSS pixels the page lays the rail out in.
///
/// One screen, because the overlay covers one (see `desk_monitors`). Tauri's monitor answers
/// in "physical" pixels, which on a Mac means points times the display's backing scale, so
/// dividing by that scale gives points — and points are exactly what WKWebView calls a CSS
/// pixel. On a Retina panel the scale is 2 and forgetting it docks the rail off the bottom
/// of the screen, which is the same mistake the Windows path above describes.
///
/// The reserved edges are what macOS keeps for itself: the menu bar along the top, and the
/// Dock along whichever edge it lives on — the gap between the screen and its `visibleFrame`,
/// which Tauri reports as the monitor's work area. A Dock set to hide itself reserves nothing,
/// which is right: it is only there while the pointer is asking for it.
#[cfg(target_os = "macos")]
fn screen_spans_mac(window: &WebviewWindow) -> Option<Vec<ScreenSpan>> {
    let origin = window.outer_position().ok()?;
    let spans: Vec<ScreenSpan> = desk_monitors(window)
        .ok()?
        .iter()
        .map(|monitor| {
            let scale = monitor.scale_factor();
            let scale = if scale > 0.0 { scale } else { 1.0 };
            let to_css = |v: i32| (f64::from(v) / scale).round() as i32;
            let at = *monitor.position();
            let size = *monitor.size();
            let (width, height) = (size.width as i32, size.height as i32);
            let work = monitor.work_area();
            let (work_x, work_y) = (work.position.x, work.position.y);
            let (work_width, work_height) = (work.size.width as i32, work.size.height as i32);
            ScreenSpan {
                x: to_css(at.x - origin.x),
                y: to_css(at.y - origin.y),
                width: to_css(width),
                height: to_css(height),
                reserved: Reserved {
                    top: to_css((work_y - at.y).max(0)),
                    left: to_css((work_x - at.x).max(0)),
                    right: to_css(((at.x + width) - (work_x + work_width)).max(0)),
                    bottom: to_css(((at.y + height) - (work_y + work_height)).max(0)),
                },
            }
        })
        .collect();
    (!spans.is_empty()).then_some(spans)
}

/// One monitor's whole rectangle and its work area, in physical virtual-screen pixels.
#[cfg(target_os = "windows")]
struct WinMonitor {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    work_left: i32,
    work_top: i32,
    work_right: i32,
    work_bottom: i32,
}

/// Every monitor the desktop has, read from Win32 rather than the toolkit.
///
/// `EnumDisplayMonitors` for the list and `GetMonitorInfoW` for each one's rectangle and
/// work area — the work area being the half Tauri's own monitor list does not carry, and
/// the half that keeps the rail out from under the taskbar.
#[cfg(target_os = "windows")]
fn win_monitors() -> Vec<WinMonitor> {
    use windows_sys::Win32::Foundation::{BOOL, LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    };

    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _hdc: HDC,
        _clip: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        // SAFETY: `data` is the `&mut Vec<WinMonitor>` handed to `EnumDisplayMonitors`
        // below, live for the whole enumeration, touched only on this (synchronous) thread.
        let out = &mut *(data as *mut Vec<WinMonitor>);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) != 0 {
            let (m, w) = (info.rcMonitor, info.rcWork);
            out.push(WinMonitor {
                left: m.left,
                top: m.top,
                right: m.right,
                bottom: m.bottom,
                work_left: w.left,
                work_top: w.top,
                work_right: w.right,
                work_bottom: w.bottom,
            });
        }
        1
    }

    let mut out: Vec<WinMonitor> = Vec::new();
    // SAFETY: `collect` only writes to `out` through the pointer passed as `data`, and the
    // call is synchronous, so `out` outlives every callback.
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect),
            &mut out as *mut _ as LPARAM,
        );
    }
    out
}

#[cfg(target_os = "linux")]
fn screen_spans(window: &WebviewWindow) -> Option<Vec<ScreenSpan>> {
    use gtk::prelude::*;

    let gtk_window = window.gtk_window().ok()?;
    let display = gtk_window.display();
    let at = window.outer_position().ok()?;
    let mut spans = Vec::new();
    for which in 0..display.n_monitors() {
        let Some(monitor) = display.monitor(which) else {
            continue;
        };
        let whole = monitor.geometry();
        let usable = monitor.workarea();
        spans.push(ScreenSpan {
            x: whole.x() - at.x,
            y: whole.y() - at.y,
            width: whole.width(),
            height: whole.height(),
            reserved: widen_for_dock(Reserved {
                top: (usable.y() - whole.y()).max(0),
                left: (usable.x() - whole.x()).max(0),
                right: (whole.x() + whole.width() - usable.x() - usable.width()).max(0),
                bottom: (whole.y() + whole.height() - usable.y() - usable.height()).max(0),
            }),
        });
    }
    (!spans.is_empty()).then_some(spans)
}

/// Add the dock's band on the edge it lives on, when it reserves nothing itself.
#[cfg(target_os = "linux")]
/// Which edges of this monitor the desktop's own chrome is using.
///
/// Two sources, because one is not enough.
///
/// **The work area** is the honest, portable one. Every panel that plays by the rules
/// publishes a strut, the window manager folds those into `_NET_WORKAREA`, and GDK hands
/// the result back per monitor. That catches GNOME's top bar, and it catches KDE, XFCE
/// and anything else without Colai knowing they exist.
///
/// **The dock is the exception**, and it is the one that prompted this. Ubuntu's dock
/// runs with `intellihide`, which means it reserves nothing at all — the work area is
/// the full width of the monitor while the dock sits visibly on top of it. Nothing in
/// EWMH describes it. So when the extension is configured, its own settings are asked
/// instead.
///
/// That second source is an approximation and is written down as one: the width comes
/// out as the icon size plus padding, and the padding is Dash to Dock's business, not a
/// published contract. Measured here, 48px icons gave a 66px band. It is close enough
/// that the rail clears the dock, and wrong in the safe direction if the theme changes —
/// a slightly wider reservation costs a few pixels of screen, a narrower one puts the
/// toolbar back underneath.
///
/// The real answer is to measure the obstruction from Colai's own capture of the screen
/// once the screen service exists, and stop asking the desktop about itself.
fn widen_for_dock(mut reserved: Reserved) -> Reserved {
    let Some(position) = gsetting("org.gnome.shell.extensions.dash-to-dock", "dock-position")
    else {
        return reserved;
    };
    let icons = gsetting(
        "org.gnome.shell.extensions.dash-to-dock",
        "dash-max-icon-size",
    )
    .and_then(|value| value.trim().parse::<i32>().ok())
    .unwrap_or(48);
    // Padding either side of an icon, measured rather than derived: 48px icons produced
    // a 66px band on this desktop.
    let band = icons + 18;

    let edge = |current: &mut i32| *current = (*current).max(band);
    match position.trim().trim_matches('\'') {
        "LEFT" => edge(&mut reserved.left),
        "RIGHT" => edge(&mut reserved.right),
        "TOP" => edge(&mut reserved.top),
        "BOTTOM" => edge(&mut reserved.bottom),
        _ => {}
    }
    reserved
}

#[cfg(target_os = "linux")]
fn gsetting(schema: &str, key: &str) -> Option<String> {
    let out = std::process::Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let said = String::from_utf8(out.stdout).ok()?;
    let said = said.trim().to_string();
    if said.is_empty() {
        None
    } else {
        Some(said)
    }
}

#[cfg(test)]
mod where_tests {
    use super::shape_rects;

    #[test]
    fn drawing_nothing_still_claims_one_pixel_and_never_nothing() {
        /*
         * The bug this stands against: an overlay that catches the whole desktop while
         * looking completely normal.
         *
         * Clearing the input region hands the window everything back, and this window is
         * transparent, always on top, and spans every display. So "no rectangles" must
         * never reach the compositor as "no shape" — it has to be somewhere, and the
         * smallest somewhere is one pixel in a corner.
         */
        assert_eq!(shape_rects(&[]), vec![(0, 0, 1, 1)]);
        assert!(
            !shape_rects(&[]).is_empty(),
            "an empty shape is never zero rectangles"
        );
    }

    #[test]
    fn what_is_drawn_is_claimed_exactly_and_nothing_is_added_to_it() {
        // The other direction, and the one a mistake would make silently worse: colai
        // must never claim more than it drew. No padding, no union with the screen, no
        // helpful rounding out to the whole window.
        let rail = vec![(12, 300, 48, 420)];
        assert_eq!(shape_rects(&rail), rail);

        let with_a_popup = vec![(12, 300, 48, 420), (70, 320, 260, 180)];
        assert_eq!(shape_rects(&with_a_popup), with_a_popup);

        // Including the deliberate full-screen claim a drawing tool makes: passed
        // through as given, so the one caller that means it is the only one that gets it.
        let whole_desk = vec![(0, 0, 3840, 1080)];
        assert_eq!(shape_rects(&whole_desk), whole_desk);
    }

    use super::*;

    // What xwininfo actually prints, trimmed to the lines that are read.
    const SAID: &str = "xwininfo: Window id: 0x7c00003 \"something\"\n\n  Absolute upper-left X:  1920\n  Absolute upper-left Y:  0\n  Relative upper-left X:  0\n  Width: 1920\n  Height: 1080\n  Depth: 24\n";

    #[test]
    fn a_window_gives_up_its_rectangle() {
        let rect = rect_from(SAID).expect("a rectangle");
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (1920, 0, 1920, 1080)
        );
    }

    #[test]
    fn three_numbers_describe_no_rectangle() {
        // A guessed edge would put a mark somewhere nobody put it, which is worse than
        // sending it with no coordinates at all.
        assert!(rect_from("  Absolute upper-left X:  10\n  Width: 100\n").is_none());
        assert!(rect_from("").is_none());
    }

    // `pid_of` reads `xprop`'s output, which only exists on Linux — so does the function,
    // and so must the test that calls it.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_process_behind_a_window_is_read_from_what_xprop_already_said() {
        let said = "WM_CLASS(STRING) = \"code\", \"Code\"\n_NET_WM_PID(CARDINAL) = 874592\n";
        assert_eq!(pid_of(said), Some(874592));
        assert_eq!(pid_of("WM_CLASS(STRING) = \"code\", \"Code\"\n"), None);
    }

    // `working_directory` reads `/proc/<pid>/cwd`, which exists only on Linux — so the
    // "a process has a directory" half of this holds only there. (The refusal half is
    // exercised on every platform by the `/proc/` filter above.)
    #[cfg(target_os = "linux")]
    #[test]
    fn a_process_looking_at_another_process_has_no_useful_directory() {
        // Every browser content process sits in /proc/<other pid>/fdinfo. Reporting that
        // as "where you are" would be worse than reporting nothing.
        assert!(working_directory(std::process::id()).is_some());
        // The shape that must be refused, checked directly rather than by finding a
        // browser: the filter is on the answer, not on who asked.
        assert!(
            !std::path::Path::new("/proc/1/fdinfo").starts_with("/proc/")
                || working_directory(u32::MAX).is_none()
        );
    }

    #[test]
    fn a_binary_is_named_by_itself() {
        assert_eq!(
            exe_name(std::path::Path::new("/snap/code/259/usr/bin/code")),
            "code"
        );
        assert_eq!(exe_name(std::path::Path::new("/")), "unknown");
    }
}

#[cfg(test)]
mod tests {

    /// The union the overlay is supposed to be, given what is plugged in.
    ///
    /// The bug this stands against did not live in `spanning` — it lived in nothing ever
    /// calling it again. A display went away, the window manager moved the oversized overlay
    /// to fit what was left, and it sat at x = -952 across a 1920-wide screen: every tool
    /// working, half the desk unreachable.
    #[test]
    fn the_desk_is_the_union_of_what_is_plugged_in() {
        let one = Span { x: 0, y: 0, width: 1920, height: 1080 };
        assert_eq!(spanning(&[one]), Some(one), "one screen is its own union");

        // Two side by side, which is what this desk was when the overlay was last placed.
        let right = Span { x: 1920, y: 0, width: 1920, height: 1080 };
        assert_eq!(
            spanning(&[one, right]),
            Some(Span { x: 0, y: 0, width: 3840, height: 1080 }),
        );

        // And a second screen to the LEFT, where the origin is negative — the case that
        // makes "just use 0,0" wrong, and the shape the overlay was left stranded in.
        let left = Span { x: -1920, y: 0, width: 1920, height: 1080 };
        assert_eq!(
            spanning(&[one, left]),
            Some(Span { x: -1920, y: 0, width: 3840, height: 1080 }),
        );
    }

    #[test]
    fn nothing_plugged_in_is_not_a_desk_of_size_zero() {
        // Better to leave the overlay where it is than to move it to a rectangle that is
        // not anywhere. `recover_if_the_desk_moved` returns early on this.
        assert_eq!(spanning(&[]), None);
    }

    use super::*;

    #[test]
    fn a_directory_that_names_nothing_is_not_an_address() {
        /*
         * A window launched from the desktop inherits the session's directory, so half
         * the windows on a machine report the home directory and mean nothing by it.
         * Sending that as the window's address is worse than sending nothing: it reads
         * as an answer, and an agent sent to a home directory looking for a project
         * finds a home directory.
         */
        assert!(tells_us_nothing("/"));
        assert!(tells_us_nothing("/usr/share/gnome-shell/extensions"));
        assert!(tells_us_nothing("/snap/code/261"));
        assert!(tells_us_nothing("/proc/1234/fd"));

        // A real place of work is not refused.
        assert!(!tells_us_nothing("/home/someone/Documents/kicad/board-name"));
        assert!(!tells_us_nothing("/srv/build"));

        // The home directory itself, whatever it is called on this machine.
        if let Ok(home) = std::env::var("HOME") {
            if !home.is_empty() {
                assert!(tells_us_nothing(&home));
                // But a project inside it is exactly the case this must not refuse.
                assert!(!tells_us_nothing(&format!("{home}/Desktop/colai")));
            }
        }
    }

    #[test]
    fn the_document_a_window_was_opened_with_is_read_off_its_command_line() {
        /*
         * Measured on this desktop: KiCad's window names its `.kicad_pro` exactly on the
         * command line, where the window title says only `board-name`. That is the
         * difference between an agent opening a file and an agent looking for one.
         *
         * Read against this test process, which is the one process whose command line is
         * certain to exist while the test runs.
         */
        let me = std::process::id();
        let raw = std::fs::read(format!("/proc/{me}/cmdline")).unwrap_or_default();
        let argv: Vec<String> = raw
            .split(|byte| *byte == 0)
            .filter_map(|part| std::str::from_utf8(part).ok())
            .map(str::to_string)
            .filter(|part| !part.is_empty())
            .collect();

        match opened_with(me) {
            // Whatever it found must be a real path that was genuinely on the line, and
            // never the program itself — the first argument is skipped because a binary
            // is not the document it was opened with.
            Some(found) => {
                assert!(std::path::Path::new(&found).exists(), "{found} must exist");
                assert!(!tells_us_nothing(&found), "a program's own files are not a document");
                assert!(argv.iter().skip(1).any(|arg| *arg == found));
                assert!(!found.starts_with('-'), "a flag is not a document");
            }
            // A test binary run with no path arguments is the ordinary case and is not a
            // failure: saying nothing is the honest answer.
            None => assert!(
                !argv.iter().skip(1).any(|arg| {
                    !arg.starts_with('-') && std::path::Path::new(arg).exists()
                }),
                "there was a path on the line and it was missed",
            ),
        }
    }

    #[test]
    fn a_process_is_not_its_own_descendant() {
        // The walk feeds `where_the_work_is`, which asks a window's children where the
        // work is when the window itself will not say. A tree that contained its own
        // root would answer with the very cwd that was already rejected.
        let me = std::process::id();
        let family = descendants(me);
        assert!(!family.contains(&me));
        assert!(family.len() <= FAMILY_MOST);
    }

    fn screen(x: i32, y: i32, width: u32, height: u32) -> Span {
        Span {
            x,
            y,
            width,
            height,
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn our_own_window_is_never_what_is_in_front() {
        // The overlay takes the keyboard when a popup opens, which makes it the active
        // window at the exact moment somebody is marking something. Answering "colai"
        // there would route their work to a conversation about the toolbar.
        let mine = format!("_NET_WM_PID(CARDINAL) = {}\n", std::process::id());
        assert!(ours(&mine));

        // Somebody else's window, and a window that says nothing about its process,
        // are both fair answers to what is in front.
        assert!(!ours("_NET_WM_PID(CARDINAL) = 1\n"));
        assert!(!ours("WM_CLASS(STRING) = \"code\", \"Code\"\n"));
        assert!(!ours(""));
    }

    #[test]
    fn the_overlay_spans_every_screen_rather_than_the_main_one() {
        // Two side by side, which is the layout that made the tools stop working on the
        // second screen: there was no overlay over there to catch anything.
        assert_eq!(
            spanning(&[screen(0, 0, 1920, 1080), screen(1920, 0, 1920, 1080)]),
            Some(screen(0, 0, 3840, 1080)),
        );
        assert_eq!(
            spanning(&[screen(0, 0, 1920, 1080)]),
            Some(screen(0, 0, 1920, 1080))
        );
        assert_eq!(spanning(&[]), None);
    }

    #[test]
    fn a_screen_above_and_to_the_left_still_fits_inside_the_span() {
        // Screens are not a row. A second display placed up and left of the first has
        // negative coordinates, and adding widths would put the overlay nowhere near it.
        assert_eq!(
            spanning(&[screen(0, 0, 1920, 1080), screen(-1280, -300, 1280, 800)]),
            Some(screen(-1280, -300, 3200, 1380)),
        );
    }

    #[test]
    fn screens_that_do_not_touch_leave_a_gap_inside_the_span() {
        // The gap belongs to nobody. The overlay draws nothing there and the pointer
        // goes straight through it, which is what the input shape already guarantees.
        assert_eq!(
            spanning(&[screen(0, 0, 800, 600), screen(1200, 0, 800, 600)]),
            Some(screen(0, 0, 2000, 600)),
        );
    }
}
