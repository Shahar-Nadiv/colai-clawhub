//! Whether this machine has a screen the toolbar can work on.
//!
//! The same question `src/screen.ts` asks in Node, asked again here — and it has to be
//! asked twice, because the Node answer only guards the two doors Node knows about. The
//! plugin's service start checks it and `openclaw colai show` checks it, and then anybody
//! who runs `bin/colai-toolbar show` directly, or puts it in a desktop autostart entry, or
//! wires it to a keyboard shortcut, walks straight past both.
//!
//! What that costs is not a crash. On Wayland the toolbar starts, draws, and lets somebody
//! mark a window it cannot see: XWayland answers `_NET_ACTIVE_WINDOW`, `_NET_CLIENT_LIST`
//! and the geometry queries, and answers them only about XWayland's own clients. A native
//! Wayland window is not in the answer. So the capture comes back without it, "which
//! window is this" names something else, and the mark that reaches the agent is quietly
//! about the wrong thing.
//!
//! That is worse than not starting, which is the argument `screen.ts` makes at length and
//! which is not repeated here. The two must agree, so the wording is kept the same
//! deliberately — somebody who hits this from the plugin and somebody who hits it from the
//! binary should be reading the same sentence.

/// What is wrong, and what to do about it.
pub(crate) struct NoScreen {
    pub why: &'static str,
    pub fix: &'static str,
}

/// Whether the word this was started with would put a toolbar on the screen.
///
/// `hide` and `quit` are let through for the same reason the Node side lets them through:
/// they reach a toolbar that is already running, and refusing them would leave somebody who
/// switched session with an overlay they could not put away.
pub(crate) fn would_show(args: &[String]) -> bool {
    !args.iter().any(|word| word == "hide" || word == "quit")
}

/// The same three cases, in the same order, as `screenTrouble`.
///
/// Wayland is tested before `DISPLAY`, because a Wayland session sets `DISPLAY` too — that
/// is exactly how this went unnoticed in Node: the check that existed passed.
pub(crate) fn trouble(
    session: Option<&str>,
    wayland: Option<&str>,
    display: Option<&str>,
) -> Option<NoScreen> {
    if session.is_some_and(|kind| kind.eq_ignore_ascii_case("wayland"))
        || wayland.is_some_and(|socket| !socket.is_empty())
    {
        return Some(NoScreen {
            why: "this is a Wayland session, and the toolbar can only see X11 windows on one. It would photograph and point at the wrong things rather than say so.",
            fix: "Log out, and at the login screen choose the X11 session — on Ubuntu it is the gear beside the Sign In button, marked \u{201c}Ubuntu on Xorg\u{201d}.",
        });
    }

    if display.is_none_or(str::is_empty) {
        return Some(NoScreen {
            why: "there is no screen to draw on — DISPLAY is not set.",
            fix: "Run this where the desktop is, or set DISPLAY to the one it should use.",
        });
    }

    None
}

/// The programs the toolbar shells out to, and what they are for.
///
/// `xprop` answers "which window is in front" and "what is it called"; `xwininfo` answers
/// "where is it". Every call site treats a failure as "cannot tell", which is the honest
/// degrade and is also completely silent — so on a machine without `x11-utils` the toolbar
/// works, marks work, and every mark is addressed to nothing, for ever, with no clue why.
///
/// The comment at the top of `colai.rs` says `xprop` "ships with x11-utils on every
/// desktop Ubuntu", and that is true and is not the same as "every machine". Debian
/// netinst, Fedora Workstation, Arch and any minimal X session have none of it.
///
/// A warning rather than a refusal, deliberately. Without these the toolbar tells the
/// agent "not inside any window the desktop would name" — it says it does not know, which
/// is the failure mode this codebase asks for everywhere else. What it is missing is a
/// person being told once that it is fixable.
const NEEDS: [(&str, &str); 2] = [
    ("xprop", "which window is in front, and what it is called"),
    ("xwininfo", "where that window is on the screen"),
];

/// Which of them are not on this machine.
pub(crate) fn missing_tools(found: impl Fn(&str) -> bool) -> Vec<&'static str> {
    NEEDS
        .iter()
        .filter(|(tool, _)| !found(tool))
        .map(|(tool, _)| *tool)
        .collect()
}

/// Whether a program can be run, by asking the same PATH the shell-outs will ask.
fn on_path(tool: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(tool).is_file())
}

/// Said once, at startup, to whoever is listening on stderr.
pub(crate) fn grumble_about_tools() {
    let missing = missing_tools(on_path);
    if missing.is_empty() {
        return;
    }
    for (tool, what) in NEEDS.iter().filter(|(tool, _)| missing.contains(tool)) {
        eprintln!("[colai] {tool} is not installed, so the toolbar cannot tell {what}.");
    }
    eprintln!(
        "[colai] Marks will still work, but they will not say which window they were made \
         on. Install them with: sudo apt install x11-utils   (Fedora: dnf install xprop \
         xwininfo, Arch: pacman -S xorg-xprop xorg-xwininfo)"
    );
}

/// Read from the environment this process actually has.
pub(crate) fn trouble_here() -> Option<NoScreen> {
    let session = std::env::var("XDG_SESSION_TYPE").ok();
    let wayland = std::env::var("WAYLAND_DISPLAY").ok();
    let display = std::env::var("DISPLAY").ok();
    trouble(session.as_deref(), wayland.as_deref(), display.as_deref())
}

#[cfg(test)]
mod tests {
    use super::{trouble, would_show};

    fn words(said: &[&str]) -> Vec<String> {
        said.iter().map(|word| (*word).to_string()).collect()
    }

    #[test]
    fn a_wayland_session_is_refused_however_it_announces_itself() {
        assert!(trouble(Some("wayland"), None, Some(":0")).is_some());
        assert!(trouble(Some("Wayland"), None, Some(":0")).is_some());
        // Some compositors set only the socket.
        assert!(trouble(None, Some("wayland-0"), Some(":0")).is_some());
    }

    #[test]
    fn wayland_is_decided_before_display_is_looked_at() {
        // A Wayland session sets DISPLAY too, which is how the Node check that only
        // looked at DISPLAY passed on every Wayland desktop it ever ran on.
        let said = trouble(Some("wayland"), Some("wayland-0"), Some(":0")).expect("refused");
        assert!(said.why.contains("Wayland"));
    }

    #[test]
    fn an_x11_session_is_left_alone() {
        assert!(trouble(Some("x11"), None, Some(":0")).is_none());
        assert!(trouble(None, Some(""), Some(":0")).is_none());
    }

    #[test]
    fn no_display_at_all_says_so_rather_than_crashing_later() {
        assert!(trouble(Some("x11"), None, None).is_some());
        assert!(trouble(Some("x11"), None, Some("")).is_some());
    }

    #[test]
    fn a_machine_without_x11_utils_is_told_which_one_is_missing() {
        use super::missing_tools;
        // The whole point is naming them: "something is missing" is not a thing anybody
        // can act on, and these two are in different packages on some distributions.
        assert_eq!(missing_tools(|_| true), Vec::<&str>::new());
        assert_eq!(missing_tools(|_| false), vec!["xprop", "xwininfo"]);
        assert_eq!(missing_tools(|tool| tool != "xwininfo"), vec!["xwininfo"]);
    }

    #[test]
    fn putting_the_toolbar_away_is_allowed_on_a_screen_it_cannot_draw_on() {
        assert!(!would_show(&words(&["colai-toolbar", "hide"])));
        assert!(!would_show(&words(&["colai-toolbar", "quit"])));
        assert!(would_show(&words(&["colai-toolbar", "show"])));
        assert!(would_show(&words(&["colai-toolbar", "toggle"])));
        // No word at all means show, which is what the plugin's service start does.
        assert!(would_show(&words(&["colai-toolbar"])));
    }
}
