// The terminal colai owns, so it can type into the conversation you are having.
//
// This is the answer to the one thing the toolbar could not do: put a mark into the Claude
// Code session somebody is actually sitting in. Everything else was tried and ruled out by
// measurement rather than argument —
//
//   - The Agent SDK has no way to send into a running session. `resume` and `continue` both
//     start a new process against a transcript, which is what caused the bug this fixes: two
//     processes on one transcript, the mark answered where nobody was looking. The one API
//     that resembled what was wanted, `createSession()` with `send`/`stream`, was removed in
//     TypeScript Agent SDK 0.3.142.
//   - Claude Code's own peer socket does exactly this and is private: it wants the session's
//     `peerToken`, and the auth frame is undocumented. Building a product on that means it
//     breaks silently on somebody else's release.
//   - Synthesising keystrokes at the display server cannot aim. On the machine this was
//     written for, three sessions run inside one VS Code process behind one X window, so the
//     keys would land in whichever pane had focus — an editor, if you were unlucky.
//   - `TIOCSTI`, which would push bytes into a tty's input queue, is disabled by default on
//     every current kernel (`dev.tty.legacy_tiocsti = 0`) and turning it back on is a
//     system-wide regression nobody should make for a toolbar.
//
// What is left is the oldest trick there is, and it is the reliable one: be the terminal.
// `colai claude` creates a pty, runs the real `claude` inside it, and relays. You see Claude
// Code exactly as you always do. colai holds the master end, so it can type — instantly, with
// no hook, no keystroke from you, and nothing private underneath it. `openpty`, `fork`,
// `exec`, `poll`. None of that can be taken away by a release.
//
// The cost is honest and it is the whole cost: Claude has to be started through colai. A
// session colai did not start still falls back to the outbox in `outbox.rs`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::io::RawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

/// Set by the signal handler, read by the relay loop.
///
/// A handler may do almost nothing safely, so it does the one thing that is safe — store a
/// flag — and the loop, woken by the `EINTR` the signal causes, does the rest. That matters
/// more than usual here: the terminal is in raw mode, and a path that exits without putting
/// it back leaves somebody with a shell that does not echo.
static ASKED_TO_GO: AtomicBool = AtomicBool::new(false);

/// Set by the `SIGWINCH` handler. The window changed size and the child has not been told.
static SIZE_CHANGED: AtomicBool = AtomicBool::new(true);

/// The pty master, for the signal handler's benefit. Handlers cannot take a `Mutex`.
static MASTER: AtomicI32 = AtomicI32::new(-1);

unsafe extern "C" fn note_we_should_go(_signal: libc::c_int) {
    ASKED_TO_GO.store(true, Ordering::SeqCst);
}

unsafe extern "C" fn note_the_size_changed(_signal: libc::c_int) {
    SIZE_CHANGED.store(true, Ordering::SeqCst);
}

/// The terminal's settings, put back however this ends.
///
/// A guard rather than a line at the end of `run`, because there are five ways out of the
/// relay loop and only one of them is the ordinary one. `Drop` covers the returns and the
/// panics; the signal handlers above cover the rest by asking the loop to end rather than
/// ending the process themselves.
struct TerminalAsWeFoundIt {
    fd: RawFd,
    settings: libc::termios,
    restored: bool,
}

impl TerminalAsWeFoundIt {
    /// Take the terminal into raw mode, remembering how to undo it.
    ///
    /// Raw, because the TUI wants every keystroke as it happens: no line buffering, no echo
    /// from us (the child echoes what it chooses), and no signal generation — `Ctrl+C` has to
    /// reach Claude Code as a byte rather than killing this wrapper, or the wrapper would
    /// die every time somebody interrupted a turn.
    fn take(fd: RawFd) -> Option<Self> {
        let mut settings = unsafe { std::mem::zeroed::<libc::termios>() };
        if unsafe { libc::tcgetattr(fd, &mut settings) } != 0 {
            // Not a terminal — being piped, or run from something with no tty. The relay
            // still works; there is simply nothing to put into raw mode.
            return None;
        }
        let mut raw = settings;
        unsafe { libc::cfmakeraw(&mut raw) };
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
            return None;
        }
        Some(Self { fd, settings, restored: false })
    }

    fn put_it_back(&mut self) {
        if self.restored {
            return;
        }
        self.restored = true;
        unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &self.settings) };
    }
}

impl Drop for TerminalAsWeFoundIt {
    fn drop(&mut self) {
        self.put_it_back();
    }
}

/// How big the window is, asked of the terminal we are attached to.
fn how_big(fd: RawFd) -> Option<libc::winsize> {
    let mut size = unsafe { std::mem::zeroed::<libc::winsize>() };
    (unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut size) } == 0).then_some(size)
}

/// Tell the child how big the window is.
///
/// Without this the TUI draws into a terminal it believes is zero by zero, which is a blank
/// screen and looks exactly like a Claude Code that failed to start.
fn say_how_big(master: RawFd, size: &libc::winsize) {
    unsafe { libc::ioctl(master, libc::TIOCSWINSZ, size) };
}

/// Where a wrapped session listens, so the toolbar can find it.
///
/// The same expression as the pidfile in `bin/colai-toolbar`, the notice in
/// `hooks/say-colai-is-here.sh`, and the outbox in `outbox.rs`. Four places now derive a path
/// under this directory; a test holds them together.
fn typing_at(session: &str) -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(said) if !said.is_empty() => PathBuf::from(said),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("ai.colai.toolbar").join("typing").join(format!("{session}.sock")))
}

/// Which conversation a running `claude` is having, asked of Claude Code itself.
///
/// `claude agents --json` is a documented command and reports both interactive sessions and
/// background ones, with the pid beside each. The alternative was reading
/// `~/.claude/sessions/<pid>.json`, which is private and has no promise attached to it.
///
/// It costs about 150ms, so it is not something to sit in a loop on. It is called rarely
/// while idle and once more at the moment of delivery, which is the only moment the answer
/// has to be right.
fn which_conversation(pid: i32) -> Option<String> {
    let said = std::process::Command::new("claude")
        .args(["agents", "--json"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let listed: Value = serde_json::from_slice(&said.stdout).ok()?;

    /*
     * Ours, or anything descended from ours.
     *
     * The first version compared the registered pid to the one `forkpty` returned, and never
     * matched anything. What we exec is a launcher that starts the real Claude Code beside
     * itself, so the session registers under a grandchild — measured: our 316960 had a 316962
     * under it, and that was the one in the list.
     *
     * The tree is read once per call rather than held, because a session that restarts under
     * the same wrapper gets a new pid and a cached tree would keep pointing at the dead one.
     */
    let mut family = crate::colai::descendants(pid as u32);
    family.push(pid as u32);

    listed.as_array()?.iter().find_map(|one| {
        let theirs = one.get("pid").and_then(Value::as_i64)? as u32;
        family
            .contains(&theirs)
            .then(|| one.get("sessionId").and_then(Value::as_str))
            .flatten()
            .map(str::to_string)
    })
}

/// A request to type, once it has been understood.
#[derive(Debug, PartialEq)]
pub(crate) struct Typing {
    pub text: String,
    /// The conversation it is meant for, checked before a key is pressed. `None` means the
    /// caller did not say, which is only ever a test or somebody at a terminal by hand.
    pub session: Option<String>,
    pub enter: bool,
}

impl Typing {
    /// The bytes to put down the pty.
    ///
    /// Enter is separate and deliberate. Typing without sending leaves the words in the box
    /// for somebody to add to, which is the right default for a half-formed thought — and it
    /// is what makes this safe to test against a real session without spending a turn.
    pub fn keystrokes(&self) -> Vec<u8> {
        let mut bytes = self.text.as_bytes().to_vec();
        if self.enter {
            bytes.push(b'\r');
        }
        bytes
    }
}

/// Read one request, or say what is wrong with it.
///
/// Separate from the socket so it can be tested without one: everything that decides what
/// gets typed into somebody's terminal belongs somewhere a test can reach.
pub(crate) fn what_was_asked(asked: &Value) -> Result<Typing, &'static str> {
    if asked.get("type").and_then(Value::as_str) != Some("type") {
        return Err("only `type` is understood here");
    }
    let text = asked.get("text").and_then(Value::as_str).unwrap_or_default();
    if text.trim().is_empty() {
        return Err("there was nothing to type");
    }
    /*
     * No control characters, and this is the one that matters.
     *
     * Whatever is in `text` is about to be pressed, key by key, into a terminal running an
     * agent that can edit files and run commands. A stray `\r` in the middle would send half
     * a thought and start typing the rest into the answer; an escape would be read as a key
     * the TUI binds to something. Newlines become spaces because a mark's message is
     * genuinely multi-line and flattening it is what somebody meant; everything else is
     * refused rather than guessed at.
     */
    if text.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return Err("that text carries control characters, which are keys");
    }
    let text = text.replace(['\n', '\t'], " ").trim().to_string();

    Ok(Typing {
        text,
        session: asked
            .get("session")
            .and_then(Value::as_str)
            .map(str::to_string),
        enter: asked.get("enter").and_then(Value::as_bool).unwrap_or(true),
    })
}

/// What the toolbar may ask of a wrapped terminal.
///
/// One line of JSON in, one line out. colai's own protocol on colai's own socket, which is
/// the point: there is nothing here that somebody else's release can change.
fn answer_one(stream: UnixStream, master: &Mutex<RawFd>, child: i32) {
    let mut reading = BufReader::new(match stream.try_clone() {
        Ok(copy) => copy,
        Err(_) => return,
    });
    let mut said = String::new();
    if reading.read_line(&mut said).is_err() {
        return;
    }
    let asked: Value = match serde_json::from_str(&said) {
        Ok(asked) => asked,
        Err(_) => return,
    };

    let answer = |ok: bool, why: &str| {
        let mut writing = &stream;
        let _ = writeln!(writing, "{}", json!({ "ok": ok, "why": why }));
    };

    let wanted = match what_was_asked(&asked) {
        Ok(wanted) => wanted,
        Err(why) => {
            answer(false, why);
            return;
        }
    };

    /*
     * Asked again, now, rather than trusted from the last poll.
     *
     * The socket is named for the conversation this terminal was having when it was bound,
     * and `/clear` starts a new one without the name changing until the next poll notices.
     * Typing somebody's mark into the wrong conversation is the one failure worth a round
     * trip to avoid, and this is the only moment the answer has to be right.
     */
    if let Some(meant) = wanted.session.as_deref() {
        match which_conversation(child) {
            Some(now) if now == meant => {}
            Some(now) => {
                answer(false, &format!("this terminal is on {now} now"));
                return;
            }
            None => {
                answer(false, "cannot tell which conversation this terminal is having");
                return;
            }
        }
    }

    let bytes = wanted.keystrokes();

    let Ok(fd) = master.lock() else {
        answer(false, "the terminal is gone");
        return;
    };
    let wrote = unsafe { libc::write(*fd, bytes.as_ptr() as *const libc::c_void, bytes.len()) };
    drop(fd);
    answer(wrote > 0, if wrote > 0 { "" } else { "the terminal would not take it" });
}

/// Listen for the toolbar, rebinding when the conversation changes.
///
/// Its own thread, because the relay loop must not block on anything: a terminal that stops
/// echoing while colai talks to itself is worse than a terminal colai cannot reach.
fn listen_for_the_toolbar(child: i32, master: Arc<Mutex<RawFd>>, going: Arc<AtomicBool>) {
    let mut bound: Option<(String, PathBuf, UnixListener)> = None;
    let mut looked = std::time::Instant::now() - std::time::Duration::from_secs(60);

    while !going.load(Ordering::SeqCst) {
        // Rarely, because it costs a process. The answer only has to be right at the moment
        // of delivery, and `answer_one` asks again there.
        let slowly = if bound.is_some() { 30 } else { 1 };
        if looked.elapsed() >= std::time::Duration::from_secs(slowly) {
            looked = std::time::Instant::now();
            if let Some(now) = which_conversation(child) {
                let stale = bound.as_ref().map(|(was, _, _)| was != &now).unwrap_or(true);
                if stale {
                    if let Some((_, old, _)) = bound.take() {
                        let _ = std::fs::remove_file(&old);
                    }
                    if let Some(at) = typing_at(&now) {
                        if let Some(within) = at.parent() {
                            let _ = std::fs::create_dir_all(within);
                        }
                        // A socket left by a crash is not a listener; binding over it is the
                        // only way to take the name back.
                        let _ = std::fs::remove_file(&at);
                        if let Ok(listener) = UnixListener::bind(&at) {
                            use std::os::unix::fs::PermissionsExt;
                            let _ = std::fs::set_permissions(
                                &at,
                                std::fs::Permissions::from_mode(0o600),
                            );
                            let _ = listener.set_nonblocking(true);
                            bound = Some((now, at, listener));
                        }
                    }
                }
            }
        }

        if let Some((_, _, listener)) = bound.as_ref() {
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    answer_one(stream, &master, child);
                    continue;
                }
                Err(ref trouble) if trouble.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => {}
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(120));
    }

    if let Some((_, at, _)) = bound {
        let _ = std::fs::remove_file(at);
    }
}

/// Run `claude` in a terminal colai owns, and relay.
///
/// Returns the exit status to leave with, so `colai claude` is as transparent as a shell
/// alias: the same screen, the same keys, the same exit code.
pub(crate) fn run(args: &[String]) -> i32 {
    let Some(claude) = crate::session::Session::discover() else {
        eprintln!("[colai] no `claude` on PATH. Install Claude Code, or set COLAI_CLAUDE.");
        return 127;
    };

    let outer = libc::STDIN_FILENO;
    let size = how_big(outer);

    let mut master: RawFd = -1;
    // `forkpty` returns twice, like `fork`: 0 in the child, the child's pid in the parent.
    let forked = unsafe {
        libc::forkpty(
            &mut master,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            size.as_ref()
                .map(|size| size as *const libc::winsize as *mut libc::winsize)
                .unwrap_or(std::ptr::null_mut()),
        )
    };
    if forked < 0 {
        eprintln!("[colai] could not open a terminal for Claude Code.");
        return 1;
    }
    if forked == 0 {
        /*
         * The child. Nothing here may allocate or take a lock — between `fork` and `exec`
         * only async-signal-safe calls are sound, and `exec` is the next thing that happens.
         * (`unsetenv` is, and is the same thing Claude Code itself does when it spawns a
         * child that must not look like part of the session that started it.)
         *
         * A session started through this wrapper is a new, top-level conversation, and it
         * has to look like one. Run from inside a Claude Code — a Bash tool call, a script
         * in a session — it would otherwise inherit that session's identity and register as
         * a child of it, which is to say not register at all: measured, it never appeared in
         * `claude agents --json`, never wrote a session record, and so could never be found
         * or typed into. An hour went into that before the environment was the suspect.
         *
         * Only the variables that say *which session this is*. `CLAUDE_CONFIG_DIR` and the
         * rest are somebody's configuration and none of our business.
         */
        for said in [
            c"CLAUDECODE",
            c"CLAUDE_CODE_SESSION_ID",
            c"CLAUDE_CODE_CHILD_SESSION",
            c"CLAUDE_CODE_ENTRYPOINT",
            c"CLAUDE_CODE_MESSAGING_SOCKET",
            c"CLAUDE_CODE_MESSAGING_TOKEN",
            c"CLAUDE_CODE_BRIDGE_SESSION_ID",
            c"CLAUDE_CODE_SESSION_ATTENDED",
            c"CLAUDE_CODE_SSE_PORT",
            c"CLAUDE_PID",
        ] {
            unsafe { libc::unsetenv(said.as_ptr()) };
        }

        let program = std::ffi::CString::new(claude.as_os_str().as_encoded_bytes()).unwrap();
        let mut argv: Vec<std::ffi::CString> = vec![program.clone()];
        for one in args {
            if let Ok(one) = std::ffi::CString::new(one.as_str()) {
                argv.push(one);
            }
        }
        let mut raw: Vec<*const libc::c_char> = argv.iter().map(|one| one.as_ptr()).collect();
        raw.push(std::ptr::null());
        unsafe { libc::execv(program.as_ptr(), raw.as_ptr()) };
        // Only reached if exec failed.
        unsafe { libc::_exit(127) };
    }

    MASTER.store(master, Ordering::SeqCst);
    let mut terminal = TerminalAsWeFoundIt::take(outer);
    unsafe {
        libc::signal(libc::SIGWINCH, note_the_size_changed as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, note_we_should_go as *const () as libc::sighandler_t);
        libc::signal(libc::SIGHUP, note_we_should_go as *const () as libc::sighandler_t);
    }

    let shared = Arc::new(Mutex::new(master));
    let going = Arc::new(AtomicBool::new(false));
    let listening = {
        let shared = Arc::clone(&shared);
        let going = Arc::clone(&going);
        std::thread::spawn(move || listen_for_the_toolbar(forked, shared, going))
    };

    let code = relay(outer, master, forked);

    going.store(true, Ordering::SeqCst);
    if let Some(terminal) = terminal.as_mut() {
        terminal.put_it_back();
    }
    let _ = listening.join();
    unsafe { libc::close(master) };
    code
}

/// Everything the person types goes down; everything Claude Code draws comes up.
fn relay(outer: RawFd, master: RawFd, child: i32) -> i32 {
    let mut buffer = [0u8; 8192];
    loop {
        if ASKED_TO_GO.load(Ordering::SeqCst) {
            unsafe { libc::kill(child, libc::SIGTERM) };
            return wait_for(child);
        }
        if SIZE_CHANGED.swap(false, Ordering::SeqCst) {
            if let Some(size) = how_big(outer) {
                say_how_big(master, &size);
            }
        }

        let mut watching = [
            libc::pollfd { fd: outer, events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: master, events: libc::POLLIN, revents: 0 },
        ];
        let ready = unsafe { libc::poll(watching.as_mut_ptr(), 2, 200) };
        if ready < 0 {
            // A signal arrived. The top of the loop is where that is dealt with.
            continue;
        }

        // Keys, down to Claude Code.
        if watching[0].revents & libc::POLLIN != 0 {
            let read = unsafe {
                libc::read(outer, buffer.as_mut_ptr() as *mut libc::c_void, buffer.len())
            };
            if read > 0 {
                let _ = write_all(master, &buffer[..read as usize]);
            }
        }

        // What Claude Code drew, up to the screen.
        if watching[1].revents & (libc::POLLIN | libc::POLLHUP) != 0 {
            let read = unsafe {
                libc::read(master, buffer.as_mut_ptr() as *mut libc::c_void, buffer.len())
            };
            if read > 0 {
                let mut out = std::io::stdout();
                let _ = out.write_all(&buffer[..read as usize]);
                let _ = out.flush();
            } else {
                // The child closed its end: it has gone, or is going.
                return wait_for(child);
            }
        }
    }
}

/// Write the whole slice, however many goes it takes.
///
/// A short write on a pty is ordinary once the child's input buffer fills, and a keystroke
/// dropped because half of an escape sequence went is a key that does something else.
fn write_all(fd: RawFd, bytes: &[u8]) -> bool {
    let mut sent = 0;
    while sent < bytes.len() {
        let wrote = unsafe {
            libc::write(
                fd,
                bytes[sent..].as_ptr() as *const libc::c_void,
                bytes.len() - sent,
            )
        };
        if wrote <= 0 {
            return false;
        }
        sent += wrote as usize;
    }
    true
}

/// The child's exit code, so `colai claude` leaves with whatever `claude` would have.
fn wait_for(child: i32) -> i32 {
    let mut status = 0;
    unsafe { libc::waitpid(child, &mut status, 0) };
    if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else if libc::WIFSIGNALED(status) {
        128 + libc::WTERMSIG(status)
    } else {
        0
    }
}

#[cfg(test)]
mod what_gets_typed {
    use super::*;

    fn asked(said: &str) -> Result<Typing, &'static str> {
        what_was_asked(&serde_json::from_str::<Value>(said).expect("json"))
    }

    #[test]
    fn an_ordinary_mark_is_typed_and_sent() {
        let one = asked(r#"{"type":"type","session":"abc","text":"why is this grey"}"#).unwrap();
        assert_eq!(one.text, "why is this grey");
        assert_eq!(one.session.as_deref(), Some("abc"));
        // Sending is the default: somebody pressed send on a toolbar, not "put it in the box".
        assert!(one.enter);
        assert_eq!(one.keystrokes(), b"why is this grey\r".to_vec());
    }

    #[test]
    fn it_can_be_left_in_the_box_unsent() {
        let one = asked(r#"{"type":"type","text":"half a thought","enter":false}"#).unwrap();
        assert_eq!(one.keystrokes(), b"half a thought".to_vec());
    }

    #[test]
    fn a_control_character_is_a_key_and_is_refused() {
        /*
         * The one that matters. This text is about to be pressed, character by character,
         * into a terminal running an agent that can edit files and run commands. A `\r` in
         * the middle sends half a thought and types the rest into the answer; an escape is
         * read as whatever the TUI binds it to.
         *
         * Refused rather than stripped, because a message that arrives altered is worse than
         * one that visibly did not arrive.
         */
        for bad in ["send\rthis", "esc\x1b[Ahere", "bell\x07", "nul\0here"] {
            let said = serde_json::json!({ "type": "type", "text": bad });
            assert_eq!(
                what_was_asked(&said),
                Err("that text carries control characters, which are keys"),
                "{bad:?} must be refused"
            );
        }
    }

    #[test]
    fn the_lines_of_a_mark_become_one_line() {
        // A mark's message is genuinely several lines — what was pointed at, then the ask.
        // Flattening is what somebody meant by it; a newline would submit it early.
        let one = asked("{\"type\":\"type\",\"text\":\"look at this\\n\\nwhy is it grey\"}").unwrap();
        assert_eq!(one.text, "look at this  why is it grey");
        assert!(!one.keystrokes().contains(&b'\n'));
    }

    #[test]
    fn nothing_to_say_is_not_a_request() {
        for empty in [r#"{"type":"type","text":""}"#, r#"{"type":"type","text":"   "}"#,
                      r#"{"type":"type"}"#] {
            assert_eq!(asked(empty), Err("there was nothing to type"));
        }
    }

    #[test]
    fn only_typing_is_understood() {
        // The socket is colai's own, and it stays that way: a wrapped terminal accepts one
        // verb. Anything that arrives wanting to run a command is not answered.
        for other in [r#"{"type":"run","text":"rm -rf /"}"#, r#"{"text":"no verb"}"#] {
            assert_eq!(asked(other), Err("only `type` is understood here"));
        }
    }

    #[test]
    fn the_socket_lives_where_everything_else_colai_writes_does() {
        let held = crate::hold_the_environment();
        std::env::set_var("XDG_CONFIG_HOME", "/tmp/somewhere");
        assert_eq!(
            typing_at("abc").unwrap(),
            PathBuf::from("/tmp/somewhere/ai.colai.toolbar/typing/abc.sock")
        );
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::set_var("HOME", "/home/nobody");
        assert_eq!(
            typing_at("abc").unwrap(),
            PathBuf::from("/home/nobody/.config/ai.colai.toolbar/typing/abc.sock")
        );
        drop(held);
    }
}
