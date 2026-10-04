// The one way in that does not require the toolbar to already have your attention.
//
// Every other shortcut this program offers is a single letter handled by the page, and
// the page only hears a key once the overlay has taken the keyboard — which it does when
// a panel opens and at no other time. So the rail advertised eleven shortcuts that could
// not be reached from the desktop, which is where somebody is when they want to mark
// something. `V`, `P`, `D` and `B` were decoration.
//
// This is the binding that fixes that: it brings the toolbar up and hands it the
// keyboard, so the letters mean what the keys say they mean.

use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// What the toolbar answers to when nothing has said otherwise.
///
/// Chosen for being unclaimed rather than for being memorable: Super belongs to the
/// desktop on GNOME and KDE both, Alt+Space is the window menu on several, and Ctrl+Space
/// is an input-method switch wherever one is installed. This is a three-finger chord
/// nobody else wants.
pub(crate) const BY_DEFAULT: &str = "Ctrl+Alt+Space";

/// The binding to use, from the environment or the default.
///
/// Read from the environment because the plugin that starts this process is where a
/// user's configuration already lives — putting a second settings file beside it would
/// give the same answer two homes.
pub(crate) fn wanted() -> String {
    match std::env::var("COLAI_HOTKEY") {
        Ok(said) if !said.trim().is_empty() => said.trim().to_string(),
        _ => BY_DEFAULT.to_string(),
    }
}

/// Show the toolbar, or hide it.
///
/// Away, so bring it up and hand it the keyboard — both, because the letters the rail
/// advertises are only reachable once the overlay is listening, and somebody who pressed a
/// key to summon a toolbar has already said where their attention is.
fn summoned(app: &AppHandle) {
    let showing = crate::colai::toolbar_is_showing(app);
    if !showing {
        if let Err(trouble) = crate::colai::colai_summon(app.clone()) {
            eprintln!("[colai] hotkey could not open the toolbar: {trouble}");
            return;
        }
        keep_asking_for_the_keyboard(app);
        return;
    }

    /*
     * Showing, so put it away. One key, two states, and that is the whole contract.
     *
     * There were three states here: away, showing-but-not-listening, and listening — and a
     * press meant something different in each, so that a toolbar sitting on screen without
     * the keyboard would be handed it rather than hidden. Reasonable, and it did not work:
     * the branch turned on `is_focused()` of the top-level window, while X gives the focus
     * to the webview's child. It read false while the toolbar plainly had the keyboard, so
     * every press took the same branch and nothing ever closed. Pressing it twice left it
     * exactly where pressing it once did.
     *
     * A key called show-or-hide should show or hide.
     */
    let _ = crate::colai::colai_release(app.clone());
}

/// How long to go on asking for the keyboard after showing the window.
const UNTIL_IT_LISTENS: Duration = Duration::from_millis(700);

/// How often to ask again while waiting.
const ASK_AGAIN: Duration = Duration::from_millis(50);

/// Ask for the keyboard until the window actually has it.
///
/// One ask was not enough, and the reason is a race rather than a refusal. Putting the
/// toolbar away turns off the hint that lets it be focused at all and unmaps the window;
/// summoning it asks X to map it again. Focusing a window that X has not finished mapping
/// does nothing and reports nothing, so pressing the binding straight after Escape opened
/// the toolbar without the keyboard — and then every single-letter shortcut went to
/// whatever was underneath. That is how a take came to type into somebody's editor.
///
/// Each attempt hops to the main thread, because taking the keyboard sets a GTK hint and
/// GTK may only be touched from the thread that started it — the same rule the contact
/// sheet broke. Stops the moment the window says it is focused, so the common case costs
/// one attempt.
fn keep_asking_for_the_keyboard(app: &AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let began = std::time::Instant::now();
        while began.elapsed() < UNTIL_IT_LISTENS {
            let (done, wait) = std::sync::mpsc::channel();
            let asking = handle.clone();
            if handle
                .run_on_main_thread(move || {
                    let _ = crate::colai::colai_take_keyboard(asking.clone());
                    let listening = asking
                        .get_webview_window(crate::colai::OVERLAY_LABEL)
                        .and_then(|window| window.is_focused().ok())
                        .unwrap_or(false);
                    let _ = done.send(listening);
                })
                .is_err()
            {
                return;
            }
            if wait
                .recv_timeout(Duration::from_millis(400))
                .unwrap_or(false)
            {
                return;
            }
            tokio::time::sleep(ASK_AGAIN).await;
        }
        eprintln!("[colai] the toolbar is open but the desktop would not give it the keyboard.");
    });
}

/// Register the binding, and say plainly if the desktop already has it.
///
/// Not fatal. A chord another program grabbed first is a thing to tell somebody about,
/// not a reason to refuse to run a toolbar that works perfectly well without it — and
/// failing to start over a keyboard shortcut would be the worst possible trade.
pub(crate) fn listen(app: &AppHandle) {
    let said = wanted();
    let chord: Shortcut = match said.parse() {
        Ok(chord) => chord,
        Err(trouble) => {
            eprintln!("[colai] “{said}” is not a shortcut this can bind ({trouble}). Using none.");
            return;
        }
    };

    let handle = app.clone();
    let asked = app.global_shortcut().on_shortcut(chord, move |_app, _chord, event| {
        // Once per press. Without this the binding fires again on release and the
        // toolbar opens and closes in the same gesture.
        if event.state() == ShortcutState::Pressed {
            summoned(&handle);
        }
    });

    match asked {
        Ok(()) => eprintln!("[colai] {said} shows the toolbar, and hides it again."),
        Err(trouble) => eprintln!(
            "[colai] {said} is already taken by something else, so there is no global shortcut ({trouble}). Set COLAI_HOTKEY to choose another."
        ),
    }
}
