// colai: point at anything on screen and hand it to an agent.
//
// This is only the toolbar. It was built inside OpenClaw's Linux desktop app, which is
// where the Gateway client, the tray and the window all already existed — and which is
// why nobody could install it without running that fork. Here it is its own program, so
// a plugin can put it on somebody's machine.
//
// What that costs is one copy: `gateway_ws`, `gateway`, `cli` and
// `gateway_device_identity` came across from the desktop app, trimmed to what the
// toolbar calls. See the header on `gateway_ws.rs`.
//
// What it does not need is most of that app — Quick Chat, the updater, the installer,
// discovery, sleep handling. Fourteen thousand lines the toolbar never called.
//
// The tray it does keep. The toolbar can be put away from its own keyboard and has no
// other window to bring it back, and it cannot add an entry to OpenClaw's — that menu is
// compiled into OpenClaw's desktop app, with no seam for a plugin, and this plugin does
// not change OpenClaw. So it carries an icon of its own, beside OpenClaw's, which is what
// staying out of somebody else's source costs.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;
mod colai;
mod colai_attach;
mod colai_capture;
mod colai_files;
#[cfg(target_os = "linux")]
mod colai_inspect;
mod colai_library;
mod colai_marks;
mod colai_receivers;
mod colai_send;
mod gateway;
mod gateway_device_identity;
mod gateway_ws;
mod hotkey;
mod screen;
mod tray;
mod whereabouts;

use std::time::Duration;
use tauri::Manager;

/// How long to wait before asking for the Gateway again, and the cap it grows to.
///
/// `ensure_ready` already spends up to fifteen seconds installing and starting the
/// service, so this is for the case where that whole attempt failed — a machine still
/// booting, a Gateway being upgraded. Doubling to a minute keeps a laptop that is simply
/// offline from spinning.
const FIRST_RETRY: Duration = Duration::from_secs(2);
const LONGEST_RETRY: Duration = Duration::from_secs(60);

fn main() {
    /*
     * Before anything is built, because the alternative is a toolbar that works.
     *
     * `src/screen.ts` asks this already and guards the two doors Node knows about — the
     * plugin's service start and `openclaw colai show`. Running this binary directly, or
     * from a desktop autostart entry, or from a keyboard shortcut, went past both. On
     * Wayland what came up was not a broken toolbar but a confident one: it draws, it
     * accepts marks, and it attributes them to whichever XWayland client it could see
     * rather than to the window somebody pointed at.
     *
     * Said on stderr because that is where the plugin is listening — `toolbar-process.ts`
     * routes it to the log and `cli.ts` prints the first breath of it — so whoever typed
     * the command reads the sentence instead of finding it later.
     */
    let asked: Vec<String> = std::env::args().collect();
    if screen::would_show(&asked) {
        if let Some(no) = screen::trouble_here() {
            eprintln!("[colai] the toolbar did not start: {}", no.why);
            eprintln!("[colai] {}", no.fix);
            std::process::exit(1);
        }
        // Not fatal, and not silent either. See `screen::grumble_about_tools`.
        screen::grumble_about_tools();
    }

    tauri::Builder::default()
        // One toolbar. A second copy hands its arguments to the first and exits, which is
        // also what stops a plugin that starts it twice from putting two on the screen.
        //
        // That handoff is also how anything outside this process reaches it. `openclaw
        // colai toggle` runs this binary again with an argument, and what arrives here is
        // the argument rather than a second window — which is why the toolbar needs no
        // socket, no port and nothing listening.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // Said, not swallowed. This is the only path `openclaw colai show|hide|toggle`
            // and the tray take while a toolbar is up, and the caller has already exited
            // 0 by the time it runs — so a failure here is a command that appeared to
            // work and did nothing, with this line the only trace it ever left.
            if let Err(trouble) = colai::asked_for(app, &args) {
                eprintln!("[colai] could not do what was asked: {trouble}");
            }
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            app.manage(gateway_ws::GatewayClient::new());
            app.manage(colai::ShapeState::default());
            app.manage(colai::ControlUi::default());
            app.manage(colai_capture::MarkShots::default());

            // The tray, before the overlay, so the first summon has something to tick.
            // Not fatal: a desktop with no tray is still a desktop with a toolbar on it,
            // and the only thing lost is the way back after Escape.
            match tray::build(app) {
                Ok(tray) => {
                    app.manage(tray);
                }
                Err(trouble) => eprintln!("[colai] no tray: {trouble}"),
            }

            // The way in from anywhere. After the tray, so the two lines about how to
            // reach the toolbar are printed together.
            hotkey::listen(app.handle());

            // The overlay, straight away: this program is the toolbar, so there is
            // nothing to wait for and nowhere else to be. Unless the very command that
            // started it said otherwise — being launched by `colai hide` should leave the
            // screen alone rather than flash a toolbar and take it away again.
            let asked: Vec<String> = std::env::args().collect();
            // Where we are, for whoever started us. Written before the window, so a
            // Gateway that restarts immediately still finds us. Removed on the way out,
            // below, where the run loop ends.
            if let Some(said) = whereabouts::asked_to_record(&asked) {
                whereabouts::record(&said);
            }
            if let Err(trouble) = colai::asked_for(app.handle(), &asked) {
                eprintln!("[colai] could not open the toolbar: {trouble}");
            }

            // Which window somebody is looking at, so a mark is drawn on the application
            // it was made on and nowhere else.
            colai_attach::watch_the_front(app.handle());

            // And the Gateway, found the way the desktop app finds it: by asking the
            // OpenClaw CLI. That is why this needs no pairing, no credential store and no
            // identity of its own — the CLI that installed this already has all three.
            //
            // On a background thread because it may install and start the service, which
            // is slow, and a toolbar that will not draw until the Gateway answers is a
            // toolbar that looks broken on a cold machine.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let found = match cli::OpenClawCli::discover() {
                    Ok(found) => found,
                    Err(trouble) => {
                        eprintln!("[colai] no openclaw CLI: {trouble}");
                        return;
                    }
                };
                // How to ask where OpenClaw is, kept for the claw. The address itself is
                // not: it carries a one-time grant, so the claw asks again on every press
                // rather than replaying a spent one.
                handle.state::<colai::ControlUi>().found(found.clone());
                // Kept trying. One attempt was a toolbar that stayed disconnected for the
                // rest of the session if the Gateway happened to be slow that morning,
                // with every menu empty and no way back but killing it.
                let mut wait = FIRST_RETRY;
                loop {
                    match gateway::ensure_ready(&found) {
                        Ok(socket) => {
                            handle
                                .state::<gateway_ws::GatewayClient>()
                                .configure(&handle, socket);
                            return;
                        }
                        Err(trouble) => {
                            eprintln!("[colai] no Gateway yet ({trouble}); trying again in {wait:?}");
                            std::thread::sleep(wait);
                            wait = (wait * 2).min(LONGEST_RETRY);
                        }
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            colai::colai_shape,
            colai::colai_frontmost,
            colai::colai_screens,
            colai::colai_take_keyboard,
            colai::colai_summon,
            colai::colai_release,
            colai::colai_open_settings,
            colai_attach::colai_in_front,
            colai_capture::colai_capture_mark,
            colai_capture::colai_forget_marks,
            colai_capture::colai_cut_recording,
            colai_files::colai_describe_files,
            colai_files::colai_pick_files,
            colai_files::colai_pick_folder,
            colai_files::colai_search_files,
            #[cfg(target_os = "linux")]
            colai_inspect::colai_showing,
            colai_library::colai_libraries,
            colai_library::colai_library_search,
            colai_receivers::colai_agents,
            colai_receivers::colai_allowed,
            colai_receivers::colai_at_work,
            colai_receivers::colai_models,
            colai_receivers::colai_sessions,
            colai_receivers::colai_threads,
            colai_send::colai_automate,
            colai_send::colai_points,
            colai_send::colai_said,
            colai_send::colai_rewind,
            colai_send::colai_send,
            colai_send::colai_start_here,
            colai_send::colai_stop,
            colai_send::colai_watch,
            colai_send::colai_unwatch
        ])
        .build(tauri::generate_context!())
        .expect("colai failed to start")
        .run(|app, event| {
            // Said we have gone, at the one moment that is true for every way of going —
            // the tray's Quit, `openclaw colai quit`, and a signal from a supervisor all
            // end the run loop here.
            if matches!(event, tauri::RunEvent::Exit) {
                if let Some(said) = whereabouts::asked_to_record(&std::env::args().collect::<Vec<_>>())
                {
                    whereabouts::forget(&said);
                }
                let _ = app;
            }
        });
}
