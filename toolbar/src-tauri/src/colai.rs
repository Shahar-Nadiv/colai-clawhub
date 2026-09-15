//! The toolbar: a sheet of glass over the whole desktop.
//!
//! A sibling of Quick Chat. Both are small, always-on-top surfaces over the desktop
//! rather than pages of the dashboard, both have their UI in `apps/linux/ui/`, and both
//! are reached from the tray. What this one adds is the screen as a workspace: a toolbar
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
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
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

    /*
     * Wake the Gateway connection, the way Quick Chat does when its window opens.
     *
     * The client is shared and connects lazily, so a surface that never activates it
     * finds it unreachable no matter how long the app has been running — the toolbar
     * asked for agents and was told "Gateway unreachable — retrying" while the dashboard
     * beside it was connected and fine.
     */
    // `try_state`, because the toolbar can be opened before the client is registered —
    // from the tray during startup, or from the setup hook itself. A missing client means
    // the connection is not ready to wake, not that anything is wrong.
    // Nothing to wake. The Gateway held a connection that had to be told the toolbar was
    // up; `claude` is started per conversation and needs no rousing.
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
    let screens: Vec<Span> = window
        .available_monitors()
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

#[cfg(not(target_os = "linux"))]
fn apply_shape(_window: &WebviewWindow, _rects: &[(i32, i32, i32, i32)]) -> Result<(), String> {
    // macOS and Windows reach click-through by other means, and neither is written yet.
    // Reporting success here would make the overlay swallow the desktop on those
    // platforms while looking correct on this one.
    Err("Per-region click-through is implemented for X11 only.".to_string())
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
/// lands on, and an async command lands on a tokio worker shared with the Gateway socket.
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
const FAMILY_MOST: usize = 400;

/// Every process descended from this one, breadth-first and bounded.
fn descendants(pid: u32) -> Vec<u32> {
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

fn named_link(pid: u32) -> Option<std::path::PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

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
/// copy is run while one is already up. `openclaw colai toggle` is the second case, and
/// the first is what happens when nothing was running yet.
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
    tray_says_toolbar(&app, true);
    /*
     * And try the Gateway again, if it had given up.
     *
     * A refused pairing or a missing credential parks the driver until something tells it
     * to retry, and in the desktop app that something was reopening Quick Chat. There is
     * no Quick Chat here, so nothing ever did — an operator who reset their state left the
     * toolbar disconnected for the rest of the session with no way back but killing it.
     *
     * Somebody summoning the toolbar is the closest thing to "try again" this program
     * has, and it costs nothing when the driver is not parked.
     */
    // Same: there is no parked reconnect to resume.
    Ok(())
}

/// Tell the tray whether the toolbar is on screen.
///
/// Showing and hiding are the only two things that move it, and both report here —
/// Escape reaches the second without the menu being involved, so a tray that learned only
/// from its own clicks would be wrong the first time anybody pressed it.
fn tray_says_toolbar(app: &AppHandle, showing: bool) {
    if let Some(tray) = app.try_state::<crate::tray::Tray>() {
        tray.says_toolbar(showing);
    }
}

/// Whether the toolbar is on screen right now.
///
/// Asked of the window, which is the only thing that knows. Anything else — a flag beside
/// it, the tray's own tick — is a second answer to a question with one.
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
    tray_says_toolbar(&app, false);
    Ok(())
}

/// There is no second application to open. See the body.
///
/// This used to show the desktop app's own window, because the toolbar lived inside that
/// app and colai's settings page *was* that window. Standing alone there is no such
/// window and pressing the claw said so — "There is no main window to show" — which is
/// true and useless.
///
/// It answers even when it cannot help. The claw is the one control that survives the
/// rail being put away, so a press on it that does nothing at all is the toolbar looking
/// broken at the moment it has least to show for itself.
#[tauri::command]
pub(crate) async fn colai_open_settings(
    #[allow(unused_variables)] app: AppHandle,
) -> Result<(), String> {
    /*
     * Nothing to open.
     *
     * This opened OpenClaw's dashboard — a web application the Gateway served, where the
     * agents and their settings lived. Claude Code is the terminal the person already has
     * in front of them; there is no second window to send them to, and opening a browser
     * at nothing would be worse than the key not being there.
     *
     * The key itself comes off the rail; this stays so that a stale page cannot invoke a
     * command that no longer exists.
     */
    Err("There is no separate application to open — Claude Code is where this goes.".to_string())
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
    #[cfg(not(target_os = "linux"))]
    {
        let _ = app;
        Vec::new()
    }
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

    #[test]
    fn the_process_behind_a_window_is_read_from_what_xprop_already_said() {
        let said = "WM_CLASS(STRING) = \"code\", \"Code\"\n_NET_WM_PID(CARDINAL) = 874592\n";
        assert_eq!(pid_of(said), Some(874592));
        assert_eq!(pid_of("WM_CLASS(STRING) = \"code\", \"Code\"\n"), None);
    }

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
