// The toolbar's own place in the system tray.
//
// The toolbar can be put away from its own keyboard, and standing alone it has no other
// window — so without this, somebody who pressed Escape had nothing left on screen to
// press and no way back except killing the process.
//
// That is the whole reason it exists. The rail can be put away — dragged off, double
// clicked shut, or never summoned — and a window with no taskbar entry and no menu bar has
// nowhere else to be found. The tray is the one surface that is still there when the
// toolbar is not, which is why it also carries the keyboard shortcut as a label: a global
// shortcut nobody has been told about is a shortcut nobody uses.

use crate::colai;
use std::sync::Mutex;
use tauri::menu::{CheckMenuItem, MenuBuilder, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{App, AppHandle};

const TOOLBAR_ID: &str = "colai-toolbar";
const BINDING_ID: &str = "colai-binding";
const TIPS_ID: &str = "colai-tips";

/// What the page hears when somebody asks for the basics again.
pub(crate) const TIPS_EVENT: &str = "colai:tips";
const QUIT_ID: &str = "quit";

/// The tray, kept alive and reachable so the tick can be corrected.
pub(crate) struct Tray {
    _icon: TrayIcon<tauri::Wry>,
    toolbar: CheckMenuItem<tauri::Wry>,
    /// Whether the last thing anybody was told is still true, so a redundant set is free.
    showing: Mutex<bool>,
}

impl Tray {
    /// Say whether the toolbar is on screen.
    ///
    /// Told by whatever put it there or took it away, rather than worked out when the
    /// menu opens: it can go from the toolbar's own keyboard, and a tray that only
    /// learned about its own clicks would be confidently wrong the first time somebody
    /// pressed Escape.
    pub(crate) fn says_toolbar(&self, showing: bool) {
        let Ok(mut last) = self.showing.lock() else {
            return;
        };
        if *last == showing {
            return;
        }
        *last = showing;
        let _ = self.toolbar.set_checked(showing);
    }
}

/// Put colai in the tray.
pub(crate) fn build(app: &App) -> tauri::Result<Tray> {
    // Ticked on, because the toolbar opens with the program.
    let toolbar = CheckMenuItem::with_id(app, TOOLBAR_ID, "Toolbar", true, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    // The binding, written where somebody looking for the toolbar will find it. A global
    // shortcut nobody has been told about is a shortcut nobody uses, and this menu is the
    // only surface that is reachable when the toolbar is put away. Disabled: it is a
    // label, and pressing it should not pretend to do something.
    let binding = MenuItem::with_id(
        app,
        BINDING_ID,
        format!("{} opens it", crate::hotkey::wanted()),
        false,
        None::<&str>,
    )?;
    // The way back to the four things nobody guesses. They are shown once, on a first
    // run, and once is not many for a card somebody can dismiss before reading — so the
    // one surface that is reachable with the toolbar put away offers them again.
    let tips = MenuItem::with_id(app, TIPS_ID, "Show the basics", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT_ID, "Quit colai", true, None::<&str>)?;
    let menu = MenuBuilder::new(app)
        .items(&[&toolbar, &binding, &tips, &separator, &quit])
        .build()?;

    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))?;
    let built = TrayIconBuilder::with_id("colai")
        .icon(icon)
        .menu(&menu)
        // Left click opens the menu. The one-click alternative would be to toggle the
        // toolbar, and a control that changes the screen without saying which way it is
        // about to go is worse than one more click.
        .show_menu_on_left_click(true)
        .on_menu_event(pressed)
        .build(app)?;

    Ok(Tray {
        _icon: built,
        toolbar,
        showing: Mutex::new(true),
    })
}

/// What the menu does.
fn pressed(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        TOOLBAR_ID => {
            // The same word the command line sends, through the same decision: `toggle`
            // reads the window rather than this tick, which is what the press is about to
            // correct. Three ways in — the menu, the command line, and the toolbar's own
            // keyboard — and one place that decides what they mean.
            if let Err(trouble) = colai::asked_for(app, &["toggle".to_string()]) {
                eprintln!("[colai] could not turn the toolbar over: {trouble}");
            }
        }
        TIPS_ID => {
            // Shown, which means the toolbar has to be on screen to show it. Somebody
            // asking for the basics from a tray with the toolbar put away means both.
            if let Err(trouble) = colai::asked_for(app, &["show".to_string()]) {
                eprintln!("[colai] could not open the toolbar: {trouble}");
            }
            if let Err(trouble) = tauri::Emitter::emit(app, TIPS_EVENT, ()) {
                eprintln!("[colai] could not ask for the basics: {trouble}");
            }
        }
        QUIT_ID => app.exit(0),
        _ => {}
    }
}
