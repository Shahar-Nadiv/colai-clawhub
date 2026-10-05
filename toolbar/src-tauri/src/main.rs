// colai: point at anything on screen and hand it to an agent.
//
// This is only the toolbar. It was built inside OpenClaw's Linux desktop app, which is
// where the Gateway client, the tray and the window all already existed — and which is
// why nobody could install it without running that fork. Here it is its own program, so
// a plugin can put it on somebody's machine.
//
// It came across with a copy of that app's Gateway client — `gateway_ws`, `gateway`,
// `cli` and `gateway_device_identity` — and none of the four is here now: there is no
// Gateway on this host to talk to. `session.rs` runs `claude` as a child process
// instead, which is the whole of the transport.
//
// What it does not need is most of that app — Quick Chat, the updater, the installer,
// discovery, sleep handling. Fourteen thousand lines the toolbar never called.
//
// It had a tray icon too, for a while, and the reason was real: the toolbar can be put
// away from its own keyboard and has no other window to bring it back. But a tray is a
// poor place to learn a keyboard shortcut from — you have to already be looking for the
// thing before the icon tells you how to reach it. The plugin says it at the top of every
// session instead, and the key hides the toolbar as well as showing it, so the surface
// that was only there to undo an Escape has nothing left to undo. `/colai:quit` stops it.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod colai;
mod colai_attach;
mod colai_capture;
mod colai_context;
mod colai_files;
// Cross-platform: the D-Bus/AT-SPI body inside is Linux-gated, but `colai_showing` has a
// non-Linux arm that answers `None`, and the command is registered on every platform.
mod colai_inspect;
mod colai_claude;
mod colai_marks;
mod colai_receivers;
mod colai_render;
mod colai_revert;
mod colai_schedule;
mod colai_send;
mod commands;
mod outbox;
mod relay;
#[cfg(target_os = "linux")]
mod wrap;

/// One lock for every test that changes the environment.
///
/// `HOME` and the rest are process-wide, and tests run in threads. Three modules here each
/// had a mutex of their own to serialise their own env fiddling, which serialised each
/// module against itself and nothing against the others — so a test that set `HOME` for its
/// own temporary directory had it changed underneath by a test in another file, and failed
/// only in a full run. Everything that touches the environment takes this one.
#[cfg(test)]
pub(crate) static THE_ENVIRONMENT: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) fn hold_the_environment() -> std::sync::MutexGuard<'static, ()> {
    THE_ENVIRONMENT.lock().unwrap_or_else(|held| held.into_inner())
}
mod session;
mod wire;
mod hotkey;
mod screen;
mod whereabouts;

use tauri::Manager;

/// Variables that tell the loader where to find libraries, modules and locales.
///
/// The ones a confined application rewrites when it launches something. Linux-only: these
/// are the glibc loader's and GTK's, and the whole confinement problem they describe is a
/// Linux one — see `without_somebody_elses_libraries`.
#[cfg(target_os = "linux")]
const LOADED_FROM: [&str; 8] = [
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
    "GTK_PATH",
    "GTK_EXE_PREFIX",
    "GTK_IM_MODULE_FILE",
    "GIO_MODULE_DIR",
    "GSETTINGS_SCHEMA_DIR",
    "LOCPATH",
];

/// Set on the way through, so the second run knows not to do this again.
#[cfg(target_os = "linux")]
const ALREADY: &str = "COLAI_CLEANED_ENV";

/// Where a snap keeps its libraries — both spellings, because snapd mounts at
/// `/var/lib/snapd/snap` where the distribution does not create `/snap`.
#[cfg(target_os = "linux")]
fn a_snap(entry: &str) -> bool {
    let trimmed = entry.trim_start_matches('/');
    trimmed.starts_with("snap/") || trimmed.starts_with("var/lib/snapd/snap/")
}

/// Start again without somebody else's libraries, if we were handed any.
///
/// The toolbar is a system GTK program, and a confined application — a snap-packaged
/// editor or terminal — rewrites these variables to point inside itself before launching
/// anything. Inheriting them, this loads that snap's libraries against the system libc and
/// dies before drawing:
///
///     symbol lookup error: /snap/core20/current/lib/x86_64-linux-gnu/libpthread.so.0:
///     undefined symbol: __libc_pthread_init, version GLIBC_PRIVATE
///
/// Nothing is missing and nothing is mispackaged; the same binary starts normally with
/// those variables gone. It was fixed once in the Node layer that used to spawn this, and
/// then Claude Code launched it directly and it happened again — which is the argument for
/// doing it here instead. Whoever starts the toolbar, it starts clean.
///
/// Entries are removed, not whole variables: a list is only partly poisoned, and dropping
/// all of it takes somebody's own paths with it. A variable is unset only when nothing
/// survives, because an empty `LD_LIBRARY_PATH` is not the same thing to the loader as an
/// absent one.
///
/// Linux-only, and not stubbed shorter than it reads: none of these variables exist on
/// Windows or macOS, there is no snap to inherit from, and the fix — `exec` in place to
/// keep the pid — is a Unix call. On those platforms starting clean is the default, so the
/// no-op below is the whole of it.
#[cfg(target_os = "linux")]
fn without_somebody_elses_libraries() {
    if std::env::var_os(ALREADY).is_some() {
        return;
    }
    let mut touched = false;
    let mut cleaned: Vec<(&str, Option<String>)> = Vec::new();
    for name in LOADED_FROM {
        let Ok(value) = std::env::var(name) else {
            continue;
        };
        // `LD_PRELOAD` is space- or colon-separated per ld.so(8); the rest are colons.
        let kept: Vec<&str> = value
            .split(|c| c == ':' || c == ' ')
            .filter(|entry| !entry.is_empty() && !a_snap(entry))
            .collect();
        let dropped = value.split(|c| c == ':' || c == ' ').filter(|e| !e.is_empty()).count()
            - kept.len();
        if dropped == 0 {
            continue;
        }
        touched = true;
        let joiner = if name == "LD_PRELOAD" { " " } else { ":" };
        cleaned.push((
            name,
            (!kept.is_empty()).then(|| kept.join(joiner)),
        ));
    }
    if !touched {
        return;
    }

    for (name, value) in &cleaned {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }
    std::env::set_var(ALREADY, "1");

    // Replace this process rather than spawning beside it: the loader has already mapped
    // the wrong libraries, so only a fresh image helps, and `exec` keeps the pid — which
    // matters because a supervisor or a pidfile may already be holding it.
    let program = std::env::current_exe().unwrap_or_else(|_| "colai-toolbar".into());
    let args: Vec<String> = std::env::args().skip(1).collect();
    eprintln!("[colai] a confined application's library paths were handed to this; restarting without them.");
    let trouble = std::os::unix::process::CommandExt::exec(
        std::process::Command::new(program).args(args),
    );
    // `exec` only returns on failure.
    eprintln!("[colai] could not restart cleanly: {trouble}");
}

/// Nothing to clean where there is no confining loader to inherit from.
#[cfg(not(target_os = "linux"))]
fn without_somebody_elses_libraries() {}

/// Set on the child, so it knows it is already the detached one.
const DETACHED: &str = "COLAI_DETACHED";

/// Step out of whoever started us, so the toolbar outlives them.
///
/// `show` runs an event loop and does not return. Started as an ordinary child — from a
/// terminal, from a slash command, from anything — it dies when its parent does, which
/// looks exactly like a toolbar that never opened.
///
/// The obvious fix is `setsid` in front of the command, and it is not available: Claude
/// Code's sandbox refuses it outright as something it cannot statically analyse. That is a
/// good reason to do it here rather than in one launcher's command line — a desktop overlay
/// that only survives when it is started a particular way is a fragile thing to ship.
///
/// A new process group rather than a double fork: the point is to be detached from the
/// parent's job control and to keep no shared stdio, and `process_group(0)` plus null
/// stdio does both with nothing to get wrong.
///
/// `COLAI_FOREGROUND=1` opts out, for anything that wants to supervise the toolbar and
/// needs it to stay a child — a service manager, or a test that must know when it exits.
fn step_out_of_the_way(args: &[String]) {
    let wanted = args.first().map(String::as_str);
    if !matches!(wanted, Some("show") | Some("toggle") | None) {
        // `hide` and `quit` talk to a toolbar that is already running and then exit. There
        // is nothing to outlive.
        return;
    }
    if std::env::var_os(DETACHED).is_some() || std::env::var_os("COLAI_FOREGROUND").is_some() {
        return;
    }
    let Ok(program) = std::env::current_exe() else {
        return;
    };
    let mut command = std::process::Command::new(program);
    command
        .args(std::env::args().skip(1))
        .env(DETACHED, "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    detach(&mut command);
    let started = command.spawn();
    match started {
        Ok(_) => std::process::exit(0),
        // Carrying on in the foreground is better than not starting: the toolbar appears,
        // and the only cost is that it goes when its parent does.
        Err(trouble) => eprintln!("[colai] could not detach ({trouble}); staying in the foreground."),
    }
}

/// Cut the toolbar loose from whatever started it, in the way each OS provides.
///
/// A new process group on Unix: off the parent's job control, keeping no shared stdio.
#[cfg(unix)]
fn detach(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

/// The Windows equivalent, spelled in creation flags. `DETACHED_PROCESS` inherits no
/// console; `CREATE_NEW_PROCESS_GROUP` keeps a Ctrl-C aimed at the parent's group from
/// reaching the toolbar. A Job Object that kills its children when the launcher closes is
/// left for later — breaking away from one can itself fail the spawn, and this path already
/// falls back to the foreground if the spawn fails, which is a working toolbar either way.
#[cfg(windows)]
fn detach(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

fn main() {
    without_somebody_elses_libraries();

    /*
     * `colai claude` is a terminal, not a window.
     *
     * First, because everything below this assumes a screen to draw on and refuses to start
     * without one — and wrapping somebody's terminal must work over ssh, in a tmux pane, on
     * a machine with no X at all. It must not detach either: it *is* the foreground process
     * of that terminal, which is the whole point of it.
     */
    let asked: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(target_os = "linux")]
    if asked.first().map(String::as_str) == Some("claude") {
        std::process::exit(wrap::run(&asked[1..]));
    }

    // Before the screen check and before anything is built: whoever asked should be free
    // the moment they have asked, not after a window has gone up.
    step_out_of_the_way(&asked);

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
            // Said, not swallowed. This is the only path `colai-toolbar show|hide|toggle|quit`
            // takes while a toolbar is up, and the caller has already exited
            // 0 by the time it runs — so a failure here is a command that appeared to
            // work and did nothing, with this line the only trace it ever left.
            // Which chat this one came from, before the window moves. A second
            // `/colai:show` from another conversation is somebody saying "this one now",
            // and the rail should be pointed there by the time it comes up.
            colai::followed(app, &args);
            if let Err(trouble) = colai::asked_for(app, &args) {
                eprintln!("[colai] could not do what was asked: {trouble}");
            }
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // A layer, not an application, on a Mac too. `skip_taskbar` means nothing to
            // AppKit; what keeps a program out of the Dock and the Cmd-Tab switcher is its
            // activation policy, and `Accessory` is the one for a thing that floats over
            // other apps without being one — no Dock icon, no menu bar of its own, and still
            // able to take the keyboard when a panel asks for it. Set before any window is
            // made, so the overlay never appears as a regular app first.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            /*
             * The Claude Code this machine has, and the conversation the toolbar is
             * pointed at. Discovered once at startup: a toolbar that cannot find `claude`
             * has nothing to say and should say so now rather than at the first send.
             */
            match session::Session::discover() {
                Some(claude) => {
                    eprintln!("[colai] talking to {}", claude.display());
                    // The relay shares the same `claude`, and is a different child with a
                    // different job: one conversation the toolbar has, one small agent that
                    // hands marks to conversations other people are in.
                    app.manage(relay::Relay::new(claude.clone()));
                    // What `/` should offer, asked now rather than waited for. The list used
                    // to arrive only on the first send through the toolbar's own agent, and
                    // marks that go straight to a live chat never start one — so `/` was
                    // offering four modes on a machine with a hundred commands.
                    commands::learn_what_slash_offers(app.handle(), claude.clone());
                    app.manage(session::Session::new(claude));
                }
                None => {
                    eprintln!("[colai] no `claude` on PATH — the toolbar has nothing to talk to.");
                    eprintln!("[colai] Install Claude Code, or set COLAI_CLAUDE to its path.");
                }
            }
            app.manage(colai::ShapeState::default());
            // The conversation this was started from, read once. Both re-execs above pass
            // the environment and the arguments through untouched, so what Claude Code set
            // is still here.
            let from = session::CameFrom::default();
            from.heard(session::came_from(&std::env::args().collect::<Vec<_>>()));
            match from.read() {
                Some(chat) => eprintln!("[colai] pointed at the conversation that opened it ({chat})."),
                None => eprintln!("[colai] started outside a conversation, so the rail opens on a choice."),
            }
            app.manage(from);
            app.manage(colai_capture::MarkShots::default());

            // The way in from anywhere, and now the only one.
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
            // And whether the desk is still the shape the overlay is covering. Its own
            // thread: monitors change a few times a day, the front window every few seconds.
            colai::watch_the_desk(app.handle());

            // Scheduled tasks: a thread that fires the ones that have come due, each in its own
            // short-lived `claude` so a task coming due never disturbs the live conversation.
            colai_schedule::watch(app.handle().clone());

            // Click-through, by hit-testing the pointer against the drawn rail. Windows has
            // no input-only window shape, so this stands in for the X11 input region: the
            // overlay stays whole and visible, and this hands it the mouse only over the
            // rail. Its own thread for the life of the app. macOS has no input-only shape
            // either, so it runs the same arrangement through its own watcher.
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            colai::watch_clickthrough(app.handle());

            // Nothing to connect to, and nothing to discover at startup beyond the
            // `claude` found above. The Gateway had to be located through the OpenClaw
            // CLI, installed if absent, started if stopped, and then retried until it
            // answered — all before the toolbar could say anything. `claude` is a program
            // on this machine that is run per conversation, so there is no connection to
            // hold open and no service to bring up.
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            colai::colai_shape,
            colai::colai_frontmost,
            colai::colai_screens,
            colai::colai_take_keyboard,
            colai::colai_summon,
            colai::colai_release,
            colai_attach::colai_in_front,
            colai_capture::colai_capture_mark,
            colai_capture::colai_cut_recording,
            colai_files::colai_describe_files,
            colai_files::colai_pick_files,
            colai_files::colai_pick_folder,
            colai_files::colai_search_files,
            // Registered everywhere: the command exists on every platform and simply
            // answers `None` where it has no way to read a window's address (Windows,
            // macOS). Gating the registration to Linux instead left the page's call
            // failing with "command not found" rather than the quiet "no address" it
            // handles.
            colai_inspect::colai_showing,
            colai_receivers::colai_at_work,
            colai_receivers::colai_sessions,
            colai::colai_came_from,
            colai::colai_quit,
            colai_send::colai_said,
            colai_send::colai_send,
            colai_send::colai_stop,
            colai_send::colai_answer,
            colai_send::colai_allow_now,
            colai_send::colai_undo,
            colai_revert::colai_revert_edits,
            colai_render::colai_render_shot,
            colai_claude::colai_claude_settings,
            colai_claude::colai_claude_toggle,
            colai_schedule::colai_schedule_list,
            colai_schedule::colai_schedule_add,
            colai_schedule::colai_schedule_remove,
            colai_schedule::colai_schedule_toggle,
            colai_schedule::colai_schedule_run_now,
            colai_schedule::colai_schedule_open_web,
            colai_context::colai_contexts_list,
            colai_context::colai_context_save_session,
            colai_context::colai_context_resolve,
            colai_context::colai_context_clear_staged
        ])
        .build(tauri::generate_context!())
        .expect("colai failed to start")
        .run(|app, event| {
            // Said we have gone, at the one moment that is true for every way of going —
            // `/colai:quit`, `colai-toolbar quit`, and a signal from a supervisor all end
            // the run loop here.
            if matches!(event, tauri::RunEvent::Exit) {
                if let Some(said) = whereabouts::asked_to_record(&std::env::args().collect::<Vec<_>>())
                {
                    whereabouts::forget(&said);
                }
                let _ = app;
            }
        });
}
