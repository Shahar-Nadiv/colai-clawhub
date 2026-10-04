// The Claude Code session the toolbar is pointed at.
//
// This replaced a WebSocket client that is no longer in the tree, and it is a twentieth of
// the size. That client needed a protocol, an ed25519 device identity, a TLS pin and a
// credential bootstrap because it was a service on a socket that had to be told who was
// calling. Claude Code is a program on this machine and the toolbar runs it, so the trust
// boundary is process ancestry and there is nothing to authenticate.
//
//     claude --print --input-format stream-json --output-format stream-json --verbose
//
// That is the protocol the Agent SDK wraps, and speaking it here rather than through the
// SDK matters: Claude Code ships as one self-contained binary with no Node runtime in it,
// so a Node sidecar would make every user install Node to run a wrapper around something
// this file can say directly.
//
// One child per conversation, kept alive across turns — measured: a second message to the
// same process is answered with the first one still in mind. Which makes this the same
// shape the Gateway had, a long-lived connection with requests going down it, so everything
// above this file barely changes.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

use crate::wire::Change;
use tauri::{AppHandle, Emitter};

/// Keep a spawned console program from popping its own window on Windows.
///
/// `claude` is a console program; a GUI process starting one gets a visible black console window
/// unless it asks not to — which is what flashed onto the desktop on every send. `CREATE_NO_WINDOW`
/// suppresses the window without detaching the child, so it stays a process the toolbar owns and
/// can kill, and the toolbar goes on speaking to it over pipes. A no-op off Windows. Shared, so
/// every place that starts a `claude` (here, the relay, the launcher probe) gets it the same way.
#[cfg(target_os = "windows")]
pub(crate) fn no_console_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}
#[cfg(not(target_os = "windows"))]
pub(crate) fn no_console_window(_cmd: &mut Command) {}

/// What the page already listens for. Not new names — the same three the Gateway emitted,
/// so `toolbar.js` needs no new idea about where an answer comes from.
const REPLY_EVENT: &str = "colai:reply";
const DOING_EVENT: &str = "colai:doing";
/// The one genuinely new fact on this host: what the conversation has cost so far.
const SPENT_EVENT: &str = "colai:spent";
/// How a tool call ended. Without this the pill says "Editing hero.css" and then nothing
/// ever says whether it worked — the worst shape a failure can take, because it looks
/// exactly like still working.
const DID_EVENT: &str = "colai:did";
/// What Claude Code says about itself at the start of every turn: which model, which
/// session, which directory, what it can do. colai flew blind without it.
const SESSION_EVENT: &str = "colai:session";
/// The conversation was summarised and the middle of it is gone. Said out loud, because
/// otherwise history appears to silently lose its own past.
const FOLDED_EVENT: &str = "colai:folded";
/// Claude Code is asking whether it may do something. The one frame that needs an answer
/// rather than a listener: nothing happens until it gets one.
const ASKS_EVENT: &str = "colai:asks";
/// What putting the files back would change, or did. The only control response the page
/// needs to see, because it is the only one whose answer it has to show somebody.
const UNDO_EVENT: &str = "colai:undone";
/// The typed overlay the response card renders from — the "Agent Responses" PRD's
/// `ResponseEvent`, synthesised from Claude Code's own frames because there is no Gateway on
/// this host to emit one. Additive: every native `colai:*` event above still fires; this is
/// the single typed event the new card listens to, so nothing that draws from the old events
/// has to move before it is proven.
const RESPONSE_EVENT: &str = "colai:response";
/// The agent in a chat colai handed a mark to has answered again, after the mirror had already
/// settled — the "I can't yet" that turned, several turns later, into the work itself. One per
/// finished reply: `{ sessionKey, id, said }`, `id` being the turn's own uuid so the page can
/// tell a reply it has already shown from a new one.
const LATER_REPLY_EVENT: &str = "colai:later-reply";

/// One conversation on this machine, for the rail to choose between.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Conversation {
    pub session_key: String,
    pub name: String,
    pub cwd: Option<String>,
    /// Milliseconds since the epoch, from the transcript's own mtime.
    pub at: u64,
    /// What it has cost so far, which Claude Code records in the transcript itself.
    pub spent: f64,
    /// The last thing somebody typed into it, whole rather than cut to a title — capped at
    /// `LAST_PROMPT_AT_MOST` so one pasted log cannot swell the session list.
    pub last_prompt: Option<String>,
}

/// How much of a last prompt the session list carries. Room for a real question; not room
/// for a pasted stack trace.
const LAST_PROMPT_AT_MOST: usize = 400;

/// The names `claude` goes by on PATH. Bare on Unix; on Windows a bare name is not
/// executable — Claude Code installs a `claude.cmd` there, with `.exe` possible too — and
/// PATH lookup is by extension, so the candidates are spelled out and tried in turn.
#[cfg(windows)]
const CLAUDE_NAMES: &[&str] = &["claude.cmd", "claude.exe", "claude.bat", "claude"];
#[cfg(not(windows))]
const CLAUDE_NAMES: &[&str] = &["claude"];

/// A running `claude`, and which conversation it is having.
struct Talking {
    child: Child,
    saying: ChildStdin,
    /// The conversation being resumed, or none for one started here.
    key: Option<String>,
}

pub(crate) struct Session {
    /// The `claude` this machine has. Resolved once — a toolbar whose binary moves
    /// underneath it has bigger problems than a stale path.
    claude: PathBuf,
    /// How much may happen without being asked. See `start` for what each mode allows and
    /// for why a conversation with no known directory does not get this one.
    permission: String,
    talking: Mutex<Option<Talking>>,
}

impl Session {
    /// The `claude` on PATH, if there is one.
    ///
    /// Walked here rather than asked of a shell. `sh -lc "command -v claude"` sources the
    /// user's rc files, and on at least one machine that died on a dangling snap path and
    /// returned nothing — which the caller reads as "no claude installed". A toolbar
    /// launched from a desktop session has no business running shell startup.
    pub fn discover() -> Option<PathBuf> {
        if let Ok(said) = std::env::var("COLAI_CLAUDE") {
            let named = PathBuf::from(said);
            return named.exists().then_some(named);
        }
        let path = std::env::var_os("PATH")?;
        // `split_paths` for the separator, not a literal `:` — Windows joins PATH with `;`
        // and its entries *contain* colons (`C:\…`), so splitting on `:` there finds no
        // directory that exists. Same call `screen::on_path` already uses.
        for dir in std::env::split_paths(&path).filter(|dir| !dir.as_os_str().is_empty()) {
            for name in CLAUDE_NAMES {
                let here = dir.join(name);
                if here.is_file() {
                    // Resolved, because what is on PATH is usually a shim.
                    return Some(fs::canonicalize(&here).unwrap_or(here));
                }
            }
        }
        None
    }

    pub fn new(claude: PathBuf) -> Self {
        Self::with_permission(claude, permission_asked())
    }

    pub fn with_permission(claude: PathBuf, permission: String) -> Self {
        Self {
            claude,
            permission,
            talking: Mutex::new(None),
        }
    }

    /// Say something, with whatever was marked attached to it.
    ///
    /// Images first and the sentence last, which is not decoration: the text refers to the
    /// pictures — "the thing I boxed" — and a reference that arrives before the thing it
    /// refers to reads as a question about nothing.
    pub fn send(
        &self,
        app: &AppHandle,
        key: Option<String>,
        message: String,
        images: Vec<String>,
        cwd: Option<String>,
    ) -> Result<String, String> {
        let mut content: Vec<Value> = images
            .into_iter()
            .map(|data| {
                json!({
                    "type": "image",
                    "source": {"type": "base64", "media_type": "image/png", "data": data},
                })
            })
            .collect();
        content.push(json!({"type": "text", "text": message}));

        /* Named, and said to be a person's.
         *
         * The uuid is the handle `rewind_files` is addressed by — without one there is no
         * way to say "put the files back to before I asked". It comes back on the result
         * frame as `user_message_uuids`, which is also how a reply is tied to the send that
         * caused it rather than to whatever was waiting.
         *
         * `origin` says a person typed this. Claude Code gates some behaviour on knowing
         * that, and absent it the answer is no — a host wrapping somebody's keyboard is
         * expected to say so. */
        let prompt = format!("colai-{}", now_ms());
        let frame = json!({
            "type": "user",
            "uuid": prompt,
            "origin": {"kind": "human"},
            "message": {"role": "user", "content": content},
        });

        let mut talking = self.talking.lock().expect("talking");
        let matches = talking
            .as_mut()
            .is_some_and(|live| live.key == key && live.still_there());
        if !matches {
            // A different conversation, or a child that has gone. Either way this one is
            // finished with: `claude` holds the transcript on disk, so ending the process
            // loses nothing, and keeping several alive would mean several `claude`
            // processes for somebody who believes they are running none.
            if let Some(mut old) = talking.take() {
                let _ = old.child.kill();
            }
            *talking = Some(self.start(app, key.clone(), cwd)?);
        }

        let live = talking.as_mut().ok_or("no session to speak to")?;
        writeln!(live.saying, "{frame}").map_err(|trouble| {
            format!("could not reach claude: {trouble}")
        })?;
        live
            .saying
            .flush()
            .map_err(|trouble| format!("could not reach claude: {trouble}"))?;
        Ok(prompt)
    }

    /// Stop the turn by ending the process.
    ///
    /// Blunt, and correct here: the transcript is written as it goes, so nothing is lost,
    /// and the next send starts a child that resumes exactly where this one stopped.
    pub fn interrupt(&self) {
        if let Some(mut live) = self.talking.lock().expect("talking").take() {
            let _ = live.child.kill();
        }
    }

    fn start(
        &self,
        app: &AppHandle,
        key: Option<String>,
        cwd: Option<String>,
    ) -> Result<Talking, String> {
        let mut run = Command::new(&self.claude);
        run.args([
            "--print",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            // Without this the stream carries only the result, and the toolbar would have
            // nothing to say between somebody pressing send and the answer arriving.
            "--verbose",
            /*
             * Nobody is sitting in front of this to approve anything.
             *
             * `host` means this toolbar answers. It used to say `none`, which denied
             * anything that would ask before a person ever heard about it — deliberate at
             * the time, because there was nowhere to show the question. There is now: a
             * `can_use_tool` control request arrives here, the rail draws what is about to
             * happen, and the answer goes back down the same pipe. Nothing proceeds until
             * it does, which is the point.
             */
            "--permission-prompts",
            "host",
        ]);
        /*
         * What may happen without being asked.
         *
         * Measured rather than assumed, because this is the toolbar's whole security
         * posture. With `acceptEdits`: a write inside the working directory succeeds, a
         * write outside it is denied, and Bash is blocked. So the thing the toolbar exists
         * for — "change this thing I am pointing at" — works, and it is confined to the
         * project the conversation belongs to, with no shell.
         *
         * That confinement *is* the working directory, which is why a conversation whose
         * directory is unknown gets `default` instead: for a new conversation `claude`
         * would inherit wherever the toolbar happens to have been launched from, and
         * auto-approving edits across somebody's home directory is not a default anybody
         * asked for. Strict until we know where we are.
         */
        run.args([
            "--permission-mode",
            match cwd.as_deref() {
                Some(_) => self.permission.as_str(),
                None => "default",
            },
        ]);
        if let Some(key) = &key {
            /*
             * Not into a conversation somebody is sitting in.
             *
             * `--resume` here starts a second Claude Code on one transcript. Both read it,
             * both append to it, and nothing arbitrates between them — and the symptom is
             * not a crash but a silence: the mark is answered by this child, in the Work
             * panel, while the chat the person is looking at never hears about it. That is
             * the bug this guard exists for, and it was reported as "nothing was sent".
             *
             * Refusing is the honest answer while the toolbar has no way to reach a running
             * session. Saying so beats resuming anyway and beats quietly starting a fresh
             * conversation under the same name, which would answer in a Work panel the
             * person is not reading either.
             */
            if let Some(held) = already_open_in_a_chat(key) {
                let whose = held
                    .name
                    .as_deref()
                    .map(|name| format!(" ({name})"))
                    .unwrap_or_default();
                return Err(format!(
                    "That conversation is open in Claude Code right now{whose}, and the \
                     toolbar cannot send into a running chat yet — it would start a second \
                     agent on the same transcript and answer where you are not looking. \
                     Pick another conversation, or close that one first."
                ));
            }
            // Only resume a conversation that actually exists on disk. A key that names no
            // transcript — a stale id, a placeholder, a receiver that was never a real session —
            // makes `claude --resume` hard-fail with "requires a valid session ID", and the turn
            // then dies with nothing on screen: no reply, no working pill, no error the person
            // sees. There is nothing to resume in that case anyway, so start a fresh conversation
            // instead. That answers rather than failing silently.
            if transcript_of(key).is_some() {
                run.args(["--resume", key]);
            } else {
                eprintln!("[colai] no transcript for '{key}'; starting a fresh conversation");
            }
        }
        if let Some(cwd) = cwd.as_deref().filter(|cwd| Path::new(cwd).is_dir()) {
            // Where the conversation was had. It decides which files the agent can reach,
            // so resuming somewhere else would quietly change what the answer is about.
            run.current_dir(cwd);
        }
        // No console window. `claude` is a console program, and a console program spawned by a
        // GUI on Windows gets its own black window — which flashed up on the desktop every time
        // somebody sent a prompt. The toolbar speaks to this child over pipes; nobody is meant to
        // see it. `CREATE_NO_WINDOW` keeps it windowless without detaching it (it stays a child we
        // can kill). The conversation is the toolbar's to show; a person who wants a terminal
        // opens one themselves with `claude --resume`.
        no_console_window(&mut run);
        let mut child = run
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|trouble| format!("could not start claude: {trouble}"))?;

        let saying = child.stdin.take().ok_or("claude took no input")?;
        let hearing = child.stdout.take().ok_or("claude said nothing")?;
        // Kept rather than dropped on the floor. `Stdio::null()` meant a claude that died
        // of a missing library, a bad flag or a refused login said so into nowhere, and the
        // toolbar reported only that nothing had happened.
        let trouble = child.stderr.take();
        let app = app.clone();
        if let Some(trouble) = trouble {
            let said = app.clone();
            std::thread::spawn(move || complain(said, trouble));
        }
        std::thread::spawn(move || listen(app, hearing));

        Ok(Talking { child, saying, key })
    }
}

impl Talking {
    /// Write one frame down the pipe.
    fn say(&mut self, frame: &Value) -> Result<(), String> {
        writeln!(self.saying, "{frame}")
            .and_then(|()| self.saying.flush())
            .map_err(|trouble| format!("claude stopped listening: {trouble}"))
    }

    /// Whether the child is still running.
    ///
    /// `try_wait` rather than assuming: a `claude` that has exited — crashed, or ended
    /// because somebody pressed stop — leaves a pipe that still accepts writes. Every one
    /// of them goes nowhere, and the toolbar would sit there having apparently sent
    /// something, waiting for an answer from a process that is not there.
    fn still_there(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

/// Everything `claude` says, turned into the events the page already draws — and, alongside
/// them, the typed `colai:response` the new response card renders from.
fn listen(app: AppHandle, hearing: std::process::ChildStdout) {
    // Orders a streamed body and the sequence of a turn's responses; monotonic for the life of
    // this child. There is no Gateway run-id, so this is the toolbar's own ordering.
    let mut seq: u64 = 0;
    for line in BufReader::new(hearing).lines() {
        let Ok(line) = line else { break };
        let Ok(frame) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        for (name, body) in what_it_said(&frame) {
            let _ = app.emit_to(crate::colai::OVERLAY_LABEL, name, body);
        }
        if let Some(response) = to_response(&frame, seq) {
            seq += 1;
            let _ = app.emit_to(crate::colai::OVERLAY_LABEL, RESPONSE_EVENT, response);
        }
    }
}

/// One frame, as a typed `ResponseEvent` for the response card — or `None` when the frame
/// carries nothing the card renders.
///
/// The PRD imagines the Gateway emitting this; there is none, so it is synthesised from
/// Claude Code's native frames. Phase 1 derives the kind from the frame alone: assistant prose
/// is an Answer; a `can_use_tool` that edits is a Change and any other is a yes/no Question;
/// the `result` frame settles the turn. `turnId` is the session for now — one live turn per
/// session, which is how the page already correlates a reply to its mark — and is refined when
/// the structured block (Phase 2) can name a turn. `seq` orders the body and the sequence.
fn to_response(frame: &Value, seq: u64) -> Option<Value> {
    let session = frame.get("session_id").and_then(Value::as_str);
    match frame.get("type").and_then(Value::as_str)? {
        "assistant" => {
            let message = frame.get("message")?;
            let body = words_in(message.get("content"));
            // A message that is only a tool call has no prose to show; the change or question it
            // carries arrives as its own `can_use_tool` frame, so there is nothing to say here.
            if body.trim().is_empty() {
                return None;
            }
            Some(json!({
                "turnId": session, "seq": seq, "sessionKey": session,
                "kind": "answer", "state": "streaming", "body": body,
            }))
        }
        "control_request"
            if frame.pointer("/request/subtype").and_then(Value::as_str) == Some("can_use_tool") =>
        {
            let tool = frame
                .pointer("/request/tool_name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let input = frame.pointer("/request/input");
            if matches!(tool, "Edit" | "Write" | "NotebookEdit") {
                let field = |name: &str| {
                    input
                        .and_then(|value| value.get(name))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                };
                let old = field("old_string");
                let new = if field("new_string").is_empty() {
                    field("content")
                } else {
                    field("new_string")
                };
                Some(json!({
                    "turnId": session, "seq": seq, "sessionKey": session,
                    "kind": "change", "state": "asking",
                    "change": {
                        "id": frame.get("request_id"),
                        "mode": "staged",
                        // The change itself, so the card can build the diff the way `askedFor`
                        // does rather than re-deriving it from prose.
                        "files": [{
                            "path": input.and_then(|value| value.get("file_path")).cloned().unwrap_or(Value::Null),
                            "added": new.lines().count(),
                            "removed": old.lines().count(),
                            "oldString": old,
                            "newString": new,
                        }],
                    },
                }))
            } else {
                Some(json!({
                    "turnId": session, "seq": seq, "sessionKey": session,
                    "kind": "question", "state": "asking",
                    "question": {
                        "shape": "yesno",
                        "id": frame.get("request_id"),
                        "title": frame.pointer("/request/title"),
                    },
                }))
            }
        }
        "result" => {
            let failed = frame
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Some(json!({
                "turnId": session, "seq": seq, "sessionKey": session,
                "kind": "answer", "state": if failed { "failed" } else { "done" },
            }))
        }
        _ => None,
    }
}

// ── mirroring a conversation somebody has open in a terminal ──────────────────────────────
//
// A mark sent to a chat that is already open is *handed over*, not taken over (see
// `colai_send`): colai cannot start a second `claude` on a live transcript without two
// processes fighting over one file, so the mark is delivered into the session the person is
// sitting in and answered there. That used to be the end of it here — the reply appeared in
// their terminal and the toolbar heard nothing, so the card at the mark stayed empty and
// never said the agent had finished.
//
// It does not have to be silent. Claude Code writes every turn to the transcript on disk as
// it goes, so the mirror tails that file from the hand-off point and feeds the new frames
// through the very same synthesis the live stream uses — the card fills in, the tool pills
// light, and the turn settles, all from a session running somewhere colai cannot speak into.

/// How often the mirror looks for new lines.
const MIRROR_POLL: Duration = Duration::from_millis(400);
/// A short grace after the model says it is finished (`end_turn`) before the card is settled —
/// just long enough to be sure no trailing line is still mid-write. The turn's end is read from
/// `stop_reason`, not from quiet: a real agent pauses far longer than any safe timeout between
/// reading something and acting on it, and settling on that pause froze the card on the wrong
/// text and called it done while the agent was still working.
const MIRROR_END_GRACE: Duration = Duration::from_millis(700);
/// The fallback for a turn that goes silent without ever saying `end_turn` — a crash, an interrupt,
/// a kill. It fires only once nothing is still running (a tool in flight is work, not a stall) and
/// only after the agent has spoken, and it has to sit above a realistic think-between-steps gap so
/// it never clips a live turn — the normal end is `end_turn`, this is just the net under a crash.
const MIRROR_STALL: Duration = Duration::from_secs(150);
/// The longest the mirror runs before it settles on its own. A turn still going after this is
/// one the person is watching in their own terminal anyway; the thread should not live forever.
const MIRROR_CAP: Duration = Duration::from_secs(20 * 60);
/// How long to wait for the transcript to show the handed turn before concluding nothing is
/// coming. A delivered `SendMessage` reaches the session in a second or two; this is generous
/// for a session that was mid-turn when the mark arrived and queued it.
const MIRROR_WAIT_FOR_FIRST: Duration = Duration::from_secs(90);

/// One transcript line, seen the way the mirror needs it: the frame normalised to the live
/// stream's shape (so `what_it_said`/`to_response` read it unchanged), plus which tool calls
/// it starts and ends and whether it is the agent speaking. `None` for a line that carries
/// nothing the card renders — the transcript's own bookkeeping (mode and system notes), a
/// subagent's sidechain, a meta entry, or any frame that is neither a message nor a result.
struct MirrorFrame {
    frame: Value,
    started: Vec<String>,
    ended: Vec<String>,
    spoke: bool,
    /// The model said it was finished — `stop_reason: "end_turn"`, as opposed to `"tool_use"`
    /// (more is coming) — which is the only reliable "the turn is over" a transcript carries.
    /// Inferring it from quiet instead settled on the wrong text: a real agent pauses longer
    /// than any safe timeout between reading something and acting on it.
    ended_turn: bool,
}

/// A transcript entry that is beside the conversation rather than part of it: a subagent's
/// sidechain, or a meta note Claude Code wrote about the conversation. Shared by the mirror
/// and by `what_was_said`, so the card and the Work panel agree about what was said.
fn an_aside(frame: &Value) -> bool {
    frame.get("isSidechain").and_then(Value::as_bool).unwrap_or(false)
        || frame.get("isMeta").and_then(Value::as_bool).unwrap_or(false)
}

/// Read one transcript line for the session `key`, or `None` to skip it.
///
/// The one shape difference between a transcript frame and a live stream frame is the session
/// field — `sessionId` on disk, `session_id` on the wire — so the frame is stamped with the
/// key we already know (the handed session's id) and everything downstream is the same code
/// that serves the toolbar's own agent.
///
/// Bytes rather than text, so a line goes from the read buffer straight into the parser
/// without first being copied into a `String`; whitespace either side (the newline, a `\r`)
/// is the parser's to skip.
fn mirror_frame(line: impl AsRef<[u8]>, key: &str) -> Option<MirrorFrame> {
    let frame = serde_json::from_slice::<Value>(line.as_ref()).ok()?;
    // Owned, because the frame is restamped (and so moved) further down and `kind` is still
    // wanted after that to tell an assistant's prose from a tool result's.
    let kind = frame
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    // Only the two frame kinds that carry a turn's words and work. A subagent's sidechain is
    // not the answer to this mark, and a meta entry is a note about the conversation, not part
    // of it.
    if !matches!(kind.as_str(), "assistant" | "user") {
        return None;
    }
    if an_aside(&frame) {
        return None;
    }
    let mut frame = frame;
    if let Some(obj) = frame.as_object_mut() {
        obj.insert("session_id".into(), Value::String(key.to_string()));
    }
    let ended_turn = kind == "assistant"
        && frame.pointer("/message/stop_reason").and_then(Value::as_str) == Some("end_turn");
    let mut started = Vec::new();
    let mut ended = Vec::new();
    let mut spoke = false;
    if let Some(content) = frame.pointer("/message/content").and_then(Value::as_array) {
        for block in content {
            match block.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    if let Some(id) = block.get("id").and_then(Value::as_str) {
                        started.push(id.to_string());
                    }
                }
                Some("tool_result") => {
                    if let Some(id) = block.get("tool_use_id").and_then(Value::as_str) {
                        ended.push(id.to_string());
                    }
                }
                Some("text") if kind == "assistant" => {
                    if !block
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .is_empty()
                    {
                        spoke = true;
                    }
                }
                _ => {}
            }
        }
    }
    Some(MirrorFrame { frame, started, ended, spoke, ended_turn })
}

/// Pull every whole line out of `buffer` and turn it into the events it carries for the
/// handed session `key`, leaving the last partial line (a line still being written) in the
/// buffer for the next read. Advances `seq` per response, and updates what is still running
/// (`pending`) and whether the agent has spoken (`spoke`) so the tailing loop can tell a
/// finished turn from one with a tool still in flight.
///
/// Separated from the loop so the draining — partial lines, ordering, the pending set — can be
/// tested without a file, a thread, or a clock.
fn mirror_drain(
    buffer: &mut Vec<u8>,
    key: &str,
    seq: &mut u64,
    pending: &mut HashSet<String>,
    spoke: &mut bool,
    ended_turn: &mut bool,
) -> Vec<(&'static str, Value)> {
    let mut events = Vec::new();
    // Walked with a cursor and drained once at the end. Draining each line off the front
    // shifted everything behind it every time, which is quadratic in a chunk of many lines —
    // and the first read of a long turn is exactly that.
    let mut start = 0;
    while let Some(nl) = buffer[start..].iter().position(|&byte| byte == b'\n') {
        let line = &buffer[start..start + nl + 1];
        start += nl + 1;
        let Some(seen) = mirror_frame(line, key) else {
            continue;
        };
        for id in seen.started {
            pending.insert(id);
        }
        for id in &seen.ended {
            pending.remove(id);
        }
        *spoke |= seen.spoke;
        // The turn is over once the model stops for its own reasons rather than to run a tool.
        // A later `tool_use` frame would be a new turn's; within this one, end_turn is the last.
        *ended_turn |= seen.ended_turn;
        events.extend(what_it_said(&seen.frame));
        if let Some(response) = to_response(&seen.frame, *seq) {
            *seq += 1;
            events.push((RESPONSE_EVENT, response));
        }
    }
    buffer.drain(..start);
    events
}

/// Keys whose mirror has been asked to stop watching.
///
/// A handed turn runs in a terminal colai does not own, so Stop cannot interrupt it — but the
/// toolbar can stop *watching* it: settle the card and let the run go. Stop drops a key here; the
/// running mirror notices on its next poll, settles, and exits. Process-global because the mirror
/// runs on a detached thread with only its key, not a handle back to any state.
fn mirror_stops() -> &'static Mutex<HashSet<String>> {
    static STOPS: std::sync::OnceLock<Mutex<HashSet<String>>> = std::sync::OnceLock::new();
    STOPS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Ask the mirror for `key` to stop watching, if one is running. Idempotent; the mirror settles
/// and exits on its next poll. Harmless when no mirror is running — a fresh mirror clears any
/// stale request before it starts.
pub(crate) fn stop_mirror(key: &str) {
    if let Ok(mut stops) = mirror_stops().lock() {
        stops.insert(key.to_string());
    }
}

/// Whether a stop was requested for `key`, taking it (so it fires once).
fn take_mirror_stop(key: &str) -> bool {
    mirror_stops().lock().map(|mut stops| stops.remove(key)).unwrap_or(false)
}

/// How far the transcript has been written, so the mirror starts from the end of what was
/// there before the hand-off and sees only the new turn. Zero when there is no file yet.
pub(crate) fn transcript_len(key: &str) -> u64 {
    transcript_of(key)
        .and_then(|path| fs::metadata(path).ok())
        .map(|meta| meta.len())
        .unwrap_or(0)
}

/// Mirror a handed-over turn into the toolbar by tailing its transcript from `from`.
///
/// Spawned after a mark has been delivered into a session somebody has open. Emits the same
/// `colai:*` events the live stream does, addressed to `key`, so the response card at the mark
/// and the Work log fill in exactly as they do for the toolbar's own agent — and settles the
/// turn when the transcript goes quiet, because a transcript has no end-of-turn frame of its
/// own to settle on.
pub(crate) fn mirror_handed_turn(app: AppHandle, key: String, from: u64) {
    // The mirror owns this turn now. A follower still watching the chat from an earlier mark
    // would otherwise announce the same answer a second time, as a "later reply".
    unfollow(&key);
    std::thread::spawn(move || {
        let emitter = app.clone();
        let settled_at = mirror_tail(&key, from, |name, body| {
            let _ = emitter.emit_to(crate::colai::OVERLAY_LABEL, name, body);
        });
        // The turn is settled, but the chat is not over: the agent may well answer again later
        // in it. Keep listening from exactly where the mirror stopped, so the turn the card
        // already shows is not reported twice.
        if let Some(offset) = settled_at {
            follow(app, key, offset);
        }
    });
}

/// The mirror's whole body, with emitting left to the caller.
///
/// Separated from `mirror_handed_turn` so the tailing — waiting for the file, reading only what
/// was appended, and settling when the turn goes quiet — can be tested against a real growing
/// file, with the events collected instead of sent to a window. Blocks until the turn settles,
/// so the wrapper runs it on its own thread.
///
/// Returns how far into the transcript the mirror had read when it settled — the end of the last
/// whole line — so a follower can pick up from there; `None` when there was no transcript to read.
fn mirror_tail<F: FnMut(&str, Value)>(key: &str, from: u64, mut emit: F) -> Option<u64> {
    // Clear any stop left over from a previous turn on this key, so it cannot cut this one short.
    let _ = take_mirror_stop(key);
    let began = Instant::now();
    // Wait for the transcript to exist. An open chat already has one, but a session only just
    // started may not have hit disk yet.
    let path = loop {
        if let Some(path) = transcript_of(key) {
            break path;
        }
        if began.elapsed() > MIRROR_WAIT_FOR_FIRST {
            settle_mirror(&mut emit, key, 0);
            return None;
        }
        std::thread::sleep(MIRROR_POLL);
    };
    mirror_tail_at(&path, key, from, began, &mut emit)
}

/// The tail proper, once the transcript file is known. Split out so a test can point it at a
/// real growing file rather than one under `~/.claude/projects`.
fn mirror_tail_at<F: FnMut(&str, Value)>(
    path: &Path,
    key: &str,
    from: u64,
    began: Instant,
    emit: &mut F,
) -> Option<u64> {
    let Ok(mut file) = fs::File::open(path) else {
        settle_mirror(emit, key, 0);
        return None;
    };
    // Only the frames written after the hand-off; everything before is the conversation the
    // person already had, which the panel is not being asked about.
    if file.seek(SeekFrom::Start(from)).is_err() {
        settle_mirror(emit, key, 0);
        return None;
    }

    let mut leftover: Vec<u8> = Vec::new();
    let mut seq: u64 = 0;
    let mut pending: HashSet<String> = HashSet::new();
    let mut spoke = false;
    let mut ended_turn = false;
    let mut last_growth = Instant::now();
    // Everything read so far; less what is still sitting in `leftover`, it is where the next
    // reader should start.
    let mut read: u64 = 0;
    loop {
        let mut chunk: Vec<u8> = Vec::new();
        // Reads from the cursor to the current end; the cursor stays put at EOF, so the next
        // read after the file grows picks up only what was appended.
        if matches!(file.read_to_end(&mut chunk), Ok(n) if n > 0) {
            last_growth = Instant::now();
            read += chunk.len() as u64;
            leftover.extend_from_slice(&chunk);
            for (name, body) in mirror_drain(
                &mut leftover,
                key,
                &mut seq,
                &mut pending,
                &mut spoke,
                &mut ended_turn,
            ) {
                emit(name, body);
            }
        }
        // Stop asked to stop watching — the person abandoned this run. colai cannot interrupt a
        // terminal turn, but it can let go of it: settle the card and exit, so the toolbar stops
        // showing a run they are done with.
        let stopped = take_mirror_stop(key);
        if stopped
            || settle_decision(spoke, ended_turn, pending.is_empty(), last_growth.elapsed(), began.elapsed())
        {
            settle_mirror(emit, key, seq);
            return Some(from + read - leftover.len() as u64);
        }
        std::thread::sleep(MIRROR_POLL);
    }
}

/// Whether a mirrored turn is over, given what the tail has seen and how long it has been quiet.
///
/// A transcript has no end-of-turn frame, so this is the whole decision — pulled out of the loop so
/// its thresholds are testable without waiting on real time. Three ways a turn ends:
///
///   - `done`: the model said so (`end_turn`) and nothing it started is still running, after a
///     short grace to let a trailing line land. The normal path.
///   - `stalled`: it went quiet without ever saying `end_turn` — a crash, an interrupt, a kill.
///     Gated on `nothing_pending`, because a tool that simply takes a while (a build, a long
///     command) keeps the transcript quiet with work still in flight, and that is not a stall; and
///     on `spoke`, so a turn that never began is not declared finished. This was the bug: the stall
///     ignored in-flight work, so any tool slower than the timeout settled the card to "done" while
///     the agent was still working.
///   - `capped`: a hard backstop so the watcher thread never lives forever.
fn settle_decision(
    spoke: bool,
    ended_turn: bool,
    nothing_pending: bool,
    quiet: Duration,
    elapsed: Duration,
) -> bool {
    let done = ended_turn && nothing_pending && quiet > MIRROR_END_GRACE;
    let stalled = spoke && nothing_pending && quiet > MIRROR_STALL;
    done || stalled || elapsed > MIRROR_CAP
}

/// Tell the card a mirrored turn has finished. There is no `result` frame in a transcript, so
/// this is the toolbar's own full stop — without it the card would shimmer "working" for ever on
/// a turn that ended in a terminal colai cannot hear.
fn settle_mirror<F: FnMut(&str, Value)>(emit: &mut F, key: &str, seq: u64) {
    emit(
        RESPONSE_EVENT,
        json!({
            "turnId": key, "seq": seq, "sessionKey": key,
            "kind": "answer", "state": "done",
        }),
    );
}

// Settling the card used to be the last the toolbar heard of a handed chat. But a chat does not
// end with one answer: the agent that first said "I can't yet, I'm in plan mode" may do the work
// three turns later, and the person who sent the mark would never know. So once a mirror settles,
// the chat is *followed* — one quiet thread looks at each followed transcript every few seconds,
// reads only what was appended, and announces each reply the agent finishes there.

/// How often the follower looks. Slower than the mirror, which is drawing a card live; this only
/// has to notice a finished reply.
const FOLLOW_POLL: Duration = Duration::from_millis(2500);
/// How many chats are followed at once. The most recently sent-to win; the oldest is let go.
const FOLLOW_MOST: usize = 10;
/// A chat whose transcript has not grown for this long is not coming back to the mark.
const FOLLOW_IDLE: Duration = Duration::from_secs(2 * 60 * 60);

/// The reply being written in a followed chat, assembled the way the Work panel assembles one:
/// asides skipped, tool results and machine echoes not mistaken for a person, and the stretches
/// of prose between tool calls joined into the one answer they were.
#[derive(Default)]
struct FollowedTurn {
    /// A line still being written, kept for the next read.
    leftover: Vec<u8>,
    /// The uuid of the turn's first assistant entry — stable, so the page can dedupe on it.
    id: String,
    said: String,
    pending: HashSet<String>,
    /// The model said `end_turn`, on the message named here.
    ended: Option<String>,
}

impl FollowedTurn {
    /// Take in newly appended bytes, returning every reply that is known to be over because
    /// something after it began — a person's next prompt, or the agent starting a new turn. The
    /// last reply in the chunk is left for `settled`, since a trailing line may still be landing.
    fn drain(&mut self, chunk: &[u8]) -> Vec<(String, String)> {
        let mut over = Vec::new();
        // Taken out of `self` for the walk, because `finish` below wants all of `self` while a
        // line is still borrowed from the buffer; put back, less what was read, at the end.
        // A cursor and one drain, as in `mirror_drain`, rather than a drain per line.
        let mut buffer = std::mem::take(&mut self.leftover);
        buffer.extend_from_slice(chunk);
        let mut start = 0;
        while let Some(nl) = buffer[start..].iter().position(|&byte| byte == b'\n') {
            let line = &buffer[start..start + nl + 1];
            start += nl + 1;
            let Ok(entry) = serde_json::from_slice::<Value>(line) else {
                continue;
            };
            if an_aside(&entry) {
                continue;
            }
            match entry.get("type").and_then(Value::as_str) {
                Some("user") => {
                    if let Some(parts) = entry.pointer("/message/content").and_then(Value::as_array) {
                        for id in parts.iter().filter_map(|part| part.get("tool_use_id")) {
                            if let Some(id) = id.as_str() {
                                self.pending.remove(id);
                            }
                        }
                    }
                    // A person (or a peer's mark) spoke: whatever came before is over. A reply
                    // it interrupted was never finished, and `finish` lets that go unsaid.
                    if typed_by_somebody(&entry).is_some_and(|words| !words.trim().is_empty()) {
                        over.extend(self.finish());
                    }
                }
                Some("assistant") => {
                    let message = entry.pointer("/message/id").and_then(Value::as_str);
                    // The agent going on after it said it was done is a new turn — unless this
                    // is just another block of the very message that carried the end_turn.
                    let same_message = message.is_some() && self.ended.as_deref() == message;
                    if self.ended.is_some() && !same_message {
                        over.extend(self.finish());
                    }
                    if self.id.is_empty() {
                        self.id = entry
                            .get("uuid")
                            .and_then(Value::as_str)
                            .or(message)
                            .unwrap_or_default()
                            .to_string();
                    }
                    if let Some(parts) = entry.pointer("/message/content").and_then(Value::as_array) {
                        for part in parts {
                            if part.get("type").and_then(Value::as_str) == Some("tool_use") {
                                if let Some(id) = part.get("id").and_then(Value::as_str) {
                                    self.pending.insert(id.to_string());
                                }
                            }
                        }
                    }
                    if let Some(words) = answered(&entry) {
                        let words = words.trim();
                        if !words.is_empty() {
                            if !self.said.is_empty() {
                                self.said.push_str("\n\n");
                            }
                            self.said.push_str(words);
                        }
                    }
                    if entry.pointer("/message/stop_reason").and_then(Value::as_str) == Some("end_turn") {
                        self.ended = Some(message.unwrap_or_default().to_string());
                    }
                }
                _ => {}
            }
        }
        buffer.drain(..start);
        self.leftover = buffer;
        over
    }

    /// The reply in progress, if it is over by the mirror's own rule — `end_turn` said, nothing
    /// still running, and quiet for the grace — and starts afresh. `None` while it is still going.
    fn settled(&mut self, quiet: Duration) -> Option<(String, String)> {
        // The stall and cap nets are the mirror's, for a card stuck on "working"; here a reply
        // that never ends is simply never announced, so they are switched off.
        if settle_decision(false, self.ended.is_some(), self.pending.is_empty(), quiet, Duration::ZERO) {
            self.finish()
        } else {
            None
        }
    }

    /// Close the turn: hand back its reply if it really finished and said something, and clear
    /// the slate for the next one either way.
    fn finish(&mut self) -> Option<(String, String)> {
        let leftover = std::mem::take(&mut self.leftover);
        let done = std::mem::take(self);
        self.leftover = leftover;
        (done.ended.is_some() && done.pending.is_empty() && !done.said.is_empty())
            .then_some((done.id, done.said))
    }
}

/// One followed chat: where its transcript is, how far it has been read, and the reply being
/// assembled from what was read.
struct Followed {
    path: Option<PathBuf>,
    offset: u64,
    turn: FollowedTurn,
    /// When the transcript last grew, for letting go of a chat that has gone quiet.
    grew: Instant,
    /// When colai last sent to it, for choosing which chats to keep when there are too many.
    since: Instant,
}

/// The chats being followed, by session key. Process-global for the same reason as the stops:
/// the mirror that hands a chat over and the follower that takes it are separate threads.
fn followed() -> &'static Mutex<HashMap<String, Followed>> {
    static FOLLOWED: std::sync::OnceLock<Mutex<HashMap<String, Followed>>> = std::sync::OnceLock::new();
    FOLLOWED.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Stop following `key` — a new mark has been handed to it and its mirror owns the turn.
fn unfollow(key: &str) {
    if let Ok(mut chats) = followed().lock() {
        chats.remove(key);
    }
}

/// Follow `key` from transcript offset `from`, starting the follower thread the first time.
fn follow(app: AppHandle, key: String, from: u64) {
    if let Ok(mut chats) = followed().lock() {
        let now = Instant::now();
        chats.insert(
            key,
            Followed { path: None, offset: from, turn: FollowedTurn::default(), grew: now, since: now },
        );
        while chats.len() > FOLLOW_MOST {
            let Some(oldest) = chats.iter().min_by_key(|(_, chat)| chat.since).map(|(key, _)| key.clone())
            else {
                break;
            };
            chats.remove(&oldest);
        }
    }
    // One thread for every followed chat, not one each; it lives as long as the app does.
    static FOLLOWER: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    FOLLOWER.get_or_init(|| {
        std::thread::spawn(move || loop {
            std::thread::sleep(FOLLOW_POLL);
            let mut heard: Vec<(String, String, String)> = Vec::new();
            if let Ok(mut chats) = followed().lock() {
                chats.retain(|_, chat| chat.grew.elapsed() < FOLLOW_IDLE);
                for (key, chat) in chats.iter_mut() {
                    for (id, said) in follow_poll(key, chat) {
                        heard.push((key.clone(), id, said));
                    }
                }
            }
            // Emitted with the lock let go, so a send handing a chat over is never held up.
            for (key, id, said) in heard {
                let _ = app.emit_to(
                    crate::colai::OVERLAY_LABEL,
                    LATER_REPLY_EVENT,
                    json!({ "sessionKey": key, "id": id, "said": said }),
                );
            }
        });
    });
}

/// One look at one followed chat: read whatever was appended since last time and return the
/// replies it finished. Cheap when nothing happened — a stat, and no read at all.
fn follow_poll(key: &str, chat: &mut Followed) -> Vec<(String, String)> {
    if chat.path.is_none() {
        chat.path = transcript_of(key);
    }
    let Some(path) = chat.path.as_ref() else {
        return Vec::new();
    };
    let Ok(len) = fs::metadata(path).map(|meta| meta.len()) else {
        return Vec::new();
    };
    // Shorter than where we were: the file was rewritten, so the old offset means nothing.
    if len < chat.offset {
        chat.offset = len;
        chat.turn = FollowedTurn::default();
    }
    let mut replies = Vec::new();
    if len > chat.offset {
        let mut chunk: Vec<u8> = Vec::new();
        let read = fs::File::open(path).and_then(|mut file| {
            file.seek(SeekFrom::Start(chat.offset))?;
            file.take(len - chat.offset).read_to_end(&mut chunk)
        });
        if read.is_ok() && !chunk.is_empty() {
            chat.offset += chunk.len() as u64;
            chat.grew = Instant::now();
            replies.extend(chat.turn.drain(&chunk));
        }
    }
    // Settled on a later poll than the one that read the end, so a trailing line has had the
    // mirror's grace to land.
    replies.extend(chat.turn.settled(chat.grew.elapsed()));
    replies
}

#[cfg(test)]
mod following {
    //! The follower's reading of a chat after the mirror let go of it: one announcement per
    //! finished reply, none for a reply still going or for anything a person said.
    use super::*;

    fn lines(entries: &[Value]) -> Vec<u8> {
        entries.iter().map(|entry| format!("{entry}\n")).collect::<String>().into_bytes()
    }

    fn a_finished_reply() -> Vec<Value> {
        vec![
            json!({"type": "assistant", "uuid": "a1", "message": {"id": "m1", "role": "assistant",
                "content": [{"type": "text", "text": "Out of plan mode now, doing it."}]}}),
            json!({"type": "assistant", "uuid": "a2", "message": {"id": "m1", "role": "assistant",
                "stop_reason": "tool_use",
                "content": [{"type": "tool_use", "id": "t1", "name": "Edit", "input": {}}]}}),
            json!({"type": "user", "uuid": "u1", "message": {"role": "user",
                "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]}}),
            json!({"type": "assistant", "uuid": "a3", "message": {"id": "m2", "role": "assistant",
                "stop_reason": "end_turn",
                "content": [{"type": "text", "text": "The header is blue."}]}}),
        ]
    }

    #[test]
    fn a_finished_reply_is_announced_once_with_its_fragments_joined() {
        let mut turn = FollowedTurn::default();
        assert!(turn.drain(&lines(&a_finished_reply())).is_empty(), "the last reply waits for quiet");
        let said = turn.settled(Duration::from_secs(5)).expect("one reply");
        assert_eq!(said.0, "a1", "named by its first entry, so the page can dedupe on it");
        assert_eq!(said.1, "Out of plan mode now, doing it.\n\nThe header is blue.");
        assert!(turn.settled(Duration::from_secs(5)).is_none(), "and only once");
    }

    #[test]
    fn a_reply_still_going_is_not_announced() {
        let mut turn = FollowedTurn::default();
        let mut going = a_finished_reply();
        going.pop();
        assert!(turn.drain(&lines(&going)).is_empty());
        assert!(turn.settled(Duration::from_secs(600)).is_none(), "no end_turn, no reply");
        // Nor one that said end_turn with a tool still out.
        let mut turn = FollowedTurn::default();
        let mut open = a_finished_reply();
        open.remove(2);
        turn.drain(&lines(&open));
        assert!(turn.settled(Duration::from_secs(600)).is_none(), "a tool in flight is work");
    }

    #[test]
    fn a_person_speaking_is_not_a_reply() {
        let mut turn = FollowedTurn::default();
        let prompt = json!({"type": "user", "uuid": "p1",
            "message": {"role": "user", "content": "now do the footer"}});
        assert!(turn.drain(&lines(&[prompt])).is_empty());
        assert!(turn.settled(Duration::from_secs(5)).is_none());
    }

    #[test]
    fn a_reply_followed_by_the_next_prompt_is_announced_at_the_prompt() {
        let mut turn = FollowedTurn::default();
        let mut chat = a_finished_reply();
        chat.push(json!({"type": "user", "uuid": "p2",
            "message": {"role": "user", "content": "thanks, and the footer?"}}));
        let over = turn.drain(&lines(&chat));
        assert_eq!(over.len(), 1, "the prompt closes the reply before it");
        assert_eq!(over[0].0, "a1");
        assert!(turn.settled(Duration::from_secs(5)).is_none(), "and it is not announced again");
    }

    #[test]
    fn the_offset_advances_so_nothing_is_read_or_announced_twice() {
        let path =
            std::env::temp_dir().join(format!("colai-follow-{}.jsonl", std::process::id()));
        let bytes = lines(&a_finished_reply());
        // Half a line at first: the follower keeps it for the next read rather than misreading it.
        let cut = bytes.len() - 10;
        fs::write(&path, &bytes[..cut]).unwrap();
        let mut chat = Followed {
            path: Some(path.clone()),
            offset: 0,
            turn: FollowedTurn::default(),
            grew: Instant::now(),
            since: Instant::now(),
        };
        assert!(follow_poll("k", &mut chat).is_empty());
        assert_eq!(chat.offset, cut as u64);
        fs::write(&path, &bytes).unwrap();
        assert!(follow_poll("k", &mut chat).is_empty(), "just read; the grace has not passed");
        assert_eq!(chat.offset, bytes.len() as u64);
        chat.grew = Instant::now() - Duration::from_secs(5);
        let heard = follow_poll("k", &mut chat);
        assert_eq!(heard.len(), 1);
        assert_eq!(heard[0].1, "Out of plan mode now, doing it.\n\nThe header is blue.");
        chat.grew = Instant::now() - Duration::from_secs(5);
        assert!(follow_poll("k", &mut chat).is_empty(), "nothing new, nothing said");
        let _ = fs::remove_file(&path);
    }
}

impl Session {
    /// Answer a `can_use_tool` the page has just shown somebody.
    ///
    /// Nothing happens in the conversation until this is sent — the turn is stopped, waiting.
    /// That is why a refusal carries words: `message` is what Claude is told, so it can try
    /// something else rather than stall, and a person who says no gets to say why.
    pub fn answer(&self, id: &str, allow: bool, message: &str) -> Result<(), String> {
        let decided = if allow {
            json!({ "behavior": "allow" })
        } else {
            json!({
                "behavior": "deny",
                "message": if message.is_empty() { "You turned this down." } else { message },
                // What chose it, which is not the same as what was chosen. Claude Code keeps
                // this to tell a person's decision from a rule's.
                "decisionClassification": "user_reject",
            })
        };
        self.control(json!({
            "subtype": "can_use_tool_response",
            "request_id": id,
            "response": decided,
        }))
    }

    /// Change what may happen without being asked, mid-conversation.
    ///
    /// The mode was an environment variable read once when the process started, so changing
    /// it meant a new conversation. It is a control request, so it applies to the next tool.
    pub fn allow_now(&self, mode: &str) -> Result<(), String> {
        // The same allow-list `permission_asked` uses, and for the same reason:
        // bypassPermissions needs a flag this never passes, so asking for it here would be a
        // request Claude Code refuses and a rail that lies about what it did.
        if !matches!(mode, "default" | "acceptEdits" | "plan" | "auto" | "dontAsk") {
            return Err(format!("{mode} is not a mode this toolbar offers"));
        }
        self.control(json!({ "subtype": "set_permission_mode", "mode": mode }))
    }

    /// Stop the turn without killing the process.
    ///
    /// `colai_stop` used to kill the child, which loses the result frame — and with it the
    /// cost of everything that had already happened. This ends the turn and leaves the
    /// conversation standing.
    pub fn stop_turn(&self) -> Result<(), String> {
        self.control(json!({ "subtype": "interrupt" }))
    }

    /// Put every file back the way it was before the given prompt.
    ///
    /// Addressed by the uuid of the user message that started the turn, which is why one is
    /// stamped on everything sent. `dry_run` asks what would change without changing it —
    /// that answer is what a confirmation shows, because rewinding also reverts anything a
    /// person edited by hand while the agent was working, and those are not the agent's to
    /// put back.
    pub fn undo_since(&self, prompt: &str, dry_run: bool) -> Result<(), String> {
        self.control(json!({
            "subtype": "rewind_files",
            "user_message_id": prompt,
            "dry_run": dry_run,
        }))
    }

    fn control(&self, request: Value) -> Result<(), String> {
        let mut held = self.talking.lock().map_err(|_| "the session is wedged")?;
        let live = held.as_mut().ok_or("nothing is running to answer")?;
        if !live.still_there() {
            return Err("claude is no longer running".into());
        }
        let id = request
            .get("request_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("colai-{}", now_ms()));
        live.say(&json!({
            "type": "control_request",
            "request_id": id,
            "request": request,
        }))
    }
}

/// A reason somebody typed, made safe to put in a prompt.
///
/// These are the user's own words rather than something read off the machine, so they are
/// not fenced the way a window title is — they are simply flattened. Control characters go
/// because they are not typed, and a length cap goes on because a deny message is a
/// sentence to an agent and not a place to paste a file.
pub(crate) fn plainly(said: &str) -> String {
    let flat: String = said
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(500).collect()
}

/// Milliseconds since the epoch, for naming a request uniquely.
fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis())
        .unwrap_or(0)
}

/// One frame of stream-json, as the events the page should hear about it.
///
/// Separated from the reading so it can be tested against recorded frames without a window,
/// a process or a display — which is the only way the frames colai used to drop can be
/// proved to stay handled.
fn what_it_said(frame: &Value) -> Vec<(&'static str, Value)> {
    let mut out: Vec<(&'static str, Value)> = Vec::new();
    let kind = frame.get("type").and_then(Value::as_str).unwrap_or_default();
    let session = frame
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::to_string);

    {
        macro_rules! to {
            ($name:expr, $body:expr $(,)?) => {
                out.push(($name, $body))
            };
        }

        match kind {
            // ── what Claude Code says about itself ───────────────────────────────────
            //
            // Re-sent at the start of every turn rather than once, so this is the current
            // truth and not a greeting. It carries the session id — which is the only way a
            // conversation colai started has a name before it reaches disk — along with the
            // model actually answering, the directory it is working in, and the lists the
            // rail needs to stop guessing: tools, slash commands, capabilities.
            "system" if subtype(&frame) == "init" => to!(SESSION_EVENT,
                json!({
                    "sessionKey": session,
                    "model": frame.get("model"),
                    "cwd": frame.get("cwd"),
                    "permissionMode": frame.get("permissionMode"),
                    "tools": frame.get("tools"),
                    "slashCommands": frame.get("slash_commands"),
                    "terminalOnly": frame.get("terminal_slash_commands"),
                    "capabilities": frame.get("capabilities"),
                    "agents": frame.get("agents"),
                    "version": frame.get("claude_code_version"),
                }),
            ),

            // The conversation was summarised. Said out loud so the Work panel can mark the
            // seam, rather than letting its own history appear to lose the middle of itself.
            "system" if subtype(&frame) == "compact_boundary" => to!(FOLDED_EVENT,
                json!({
                    "sessionKey": session,
                    "why": frame.pointer("/compact_metadata/trigger"),
                    "before": frame.pointer("/compact_metadata/pre_tokens"),
                    "after": frame.pointer("/compact_metadata/post_tokens"),
                }),
            ),

            // ── what it is saying, and what it has picked up ─────────────────────────
            "assistant" => {
                let Some(message) = frame.get("message") else {
                    return out;
                };
                // Forwarded whole. `spokenBy` in `toolbar-answers.js` reads `{role:
                // "assistant", content: [{type: "text"}]}`, which is the shape this already
                // is — re-packing it would be a second format for the same thing.
                to!(REPLY_EVENT,
                    json!({
                        "sessionKey": session,
                        "message": message,
                        // How much room is left. The question every long conversation
                        // eventually raises, and it arrives here unasked for.
                        "context": frame.get("context_usage"),
                        // Non-null when a subagent said it, so the rail can say whose work
                        // this is rather than attributing it to the main thread.
                        "inside": frame.get("parent_tool_use_id"),
                    }),
                );
                // And what it has just picked up, for the pill on the rail. Only the start
                // of a call: a result names something that has already stopped happening.
                if let Some(content) = message.get("content").and_then(Value::as_array) {
                    for block in content {
                        if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                            to!(DOING_EVENT,
                                json!({
                                    "name": block.get("name"),
                                    "args": block.get("input").cloned().unwrap_or(json!({})),
                                    // The handle the outcome arrives under, so a pill can be
                                    // finished rather than left running for ever.
                                    "id": block.get("id"),
                                    "sessionKey": session,
                                }),
                            );
                        }
                    }
                }
            }

            // ── how those calls ended ────────────────────────────────────────────────
            //
            // The frame colai used to drop, and the reason a failed tool left no trace at
            // all. Claude Code echoes each `tool_use` back as a `tool_result` inside a user
            // message; `is_error` says whether it worked, and `tool_result_meta` says when
            // it never ran — refused by a rule, rejected by a person, interrupted.
            "user" => {
                let Some(content) = frame.pointer("/message/content").and_then(Value::as_array)
                else {
                    return out;
                };
                let refusals = frame.get("tool_result_meta").and_then(Value::as_array);
                for block in content {
                    if block.get("type").and_then(Value::as_str) != Some("tool_result") {
                        continue;
                    }
                    let id = block.get("tool_use_id").and_then(Value::as_str);
                    let refused = refusals.and_then(|rows| {
                        rows.iter()
                            .find(|row| row.get("id").and_then(Value::as_str) == id)
                            .and_then(|row| row.get("non_execution_kind").cloned())
                    });
                    to!(DID_EVENT,
                        json!({
                            "id": id,
                            "wrong": block.get("is_error").and_then(Value::as_bool).unwrap_or(false),
                            "said": briefly(&said_in(block.get("content"))),
                            "never": refused,
                            "sessionKey": session,
                        }),
                    );
                }
            }

            /* ── the answer to something we asked ────────────────────────────────────
             *
             * Only the rewind's. Everything else on this channel is bookkeeping between
             * the toolbar and Claude Code; this one is a list of files somebody is about to
             * be asked to agree to losing, or has just lost. */
            "control_response"
                if frame.pointer("/response/response/subtype").and_then(Value::as_str)
                    == Some("rewind_files")
                    || frame.pointer("/response/response/restored").is_some()
                    || frame.pointer("/response/response/files").is_some() =>
            {
                let answer = frame.pointer("/response/response");
                to!(
                    UNDO_EVENT,
                    json!({
                        "id": frame.pointer("/response/request_id"),
                        // Named both ways because the field has moved between versions and
                        // an empty list is indistinguishable from the wrong key.
                        "files": answer.and_then(|a| a.get("files").or_else(|| a.get("restored"))),
                        "asked": answer.and_then(|a| a.get("dry_run")),
                        "sessionKey": session,
                    })
                );
            }

            /* ── may I? ──────────────────────────────────────────────────────────────
             *
             * The turn is stopped until this is answered, so it is the one frame that is a
             * question rather than news. Everything the card needs to draw the thing about
             * to happen is already here: the tool, and its real input — for an edit that is
             * `{file_path, old_string, new_string}`, which is the change itself and not a
             * description of it.
             *
             * `title`, `description` and `decision_reason` are Claude Code's own words for
             * the request, used for tools with no preview worth drawing. `decision_reason`
             * may carry terminal escapes, so it goes through the same fence as anything
             * else read off the machine rather than straight onto a rail. */
            "control_request"
                if frame.pointer("/request/subtype").and_then(Value::as_str)
                    == Some("can_use_tool") =>
            {
                to!(
                    ASKS_EVENT,
                    json!({
                        "id": frame.get("request_id"),
                        "tool": frame.pointer("/request/tool_name"),
                        "input": frame.pointer("/request/input"),
                        "title": frame.pointer("/request/title"),
                        "description": frame.pointer("/request/description"),
                        "why": frame.pointer("/request/decision_reason"),
                        "defaultNo": frame.pointer("/request/default_to_no"),
                        "sessionKey": session,
                    })
                );
            }

            // ── the end of a turn ────────────────────────────────────────────────────
            "result" => {
                /* Read, not accumulated.
                 *
                 * `total_cost_usd` is already the running total for the session — the
                 * schema says so in as many words, and says to read the latest rather than
                 * summing across results. Adding each turn to a total of our own counted
                 * every turn twice over, and worse the longer somebody talked. */
                let spent = cost_in(frame.get("total_cost_usd")).unwrap_or(0.0);
                to!(SPENT_EVENT,
                    json!({
                        "sessionKey": session,
                        "spent": spent,
                        "outcome": frame.get("subtype"),
                        "wrong": frame.get("is_error"),
                        "turns": frame.get("num_turns"),
                        "took": frame.get("duration_ms"),
                        "usage": frame.get("usage"),
                        // Tools that were asked for and refused. Until the approval card
                        // exists this is the only account of what a permission mode cost.
                        "refused": frame.get("permission_denials"),
                        // More is coming without anybody sending anything.
                        "queued": frame.get("queued_turn_count"),
                    }),
                );
            }

            // Something we asked for was refused. The page has to hear it, or a button
            // that did nothing looks exactly like a button that worked.
            "control_response"
                if frame.pointer("/response/subtype").and_then(Value::as_str) == Some("error") =>
            {
                to!(
                    "colai:trouble",
                    json!({
                        "said": briefly(
                            frame
                                .pointer("/response/error")
                                .and_then(Value::as_str)
                                .unwrap_or("Claude Code refused that")
                        )
                    })
                );
            }

            _ => {}
        }
    }

    out
}

#[cfg(test)]
mod frames {
    //! Every frame colai used to drop, and what it now says about it.
    //!
    //! Recorded shapes rather than live ones: the point is that a frame arriving in the
    //! documented form produces the event the page is waiting for. A live session proves the
    //! form is right; these prove it stays handled.
    use super::*;

    fn heard(frame: Value) -> Vec<(&'static str, Value)> {
        what_it_said(&frame)
    }
    fn only(frame: Value, name: &str) -> Value {
        let heard = heard(frame);
        let found: Vec<_> = heard.iter().filter(|(kind, _)| *kind == name).collect();
        assert_eq!(found.len(), 1, "expected one {name}, got {heard:?}");
        found[0].1.clone()
    }

    #[test]
    fn init_carries_the_session_the_model_and_what_this_host_can_do() {
        // The frame colai never read, which is why a conversation it started had no id until
        // a disk scan found one, and why the model picker had nothing to offer.
        let said = only(
            json!({
                "type": "system", "subtype": "init",
                "session_id": "abc-123", "model": "claude-sonnet-5",
                "cwd": "/home/someone/project", "permissionMode": "acceptEdits",
                "tools": ["Read", "Edit"], "slash_commands": ["review", "clear"],
                "terminal_slash_commands": ["clear"],
                "capabilities": ["interrupt_receipt_v1"],
            }),
            SESSION_EVENT,
        );
        assert_eq!(said["sessionKey"], "abc-123");
        assert_eq!(said["model"], "claude-sonnet-5");
        assert_eq!(said["cwd"], "/home/someone/project");
        assert_eq!(said["permissionMode"], "acceptEdits");
        assert_eq!(said["tools"][1], "Edit");
        assert_eq!(said["slashCommands"][0], "review");
        // Which of those a GUI must not offer, because they only mean something in a terminal.
        assert_eq!(said["terminalOnly"][0], "clear");
        assert_eq!(said["capabilities"][0], "interrupt_receipt_v1");
    }

    #[test]
    fn a_tool_that_worked_says_so_against_the_call_that_started_it() {
        let said = only(
            json!({
                "type": "user", "session_id": "s",
                "message": {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "call_1", "content": "ok"}
                ]},
            }),
            DID_EVENT,
        );
        assert_eq!(said["id"], "call_1", "tied to the tool_use, or a pill cannot be finished");
        assert_eq!(said["wrong"], false);
    }

    // ── the typed ResponseEvent the card renders from, synthesised from these same frames ──

    #[test]
    fn assistant_prose_is_an_answer_response() {
        let said = to_response(
            &json!({
                "type": "assistant", "session_id": "s",
                "message": {"role": "assistant", "content": [{"type": "text", "text": "Here is what you asked."}]},
            }),
            3,
        )
        .expect("a response");
        assert_eq!(said["kind"], "answer");
        assert_eq!(said["state"], "streaming");
        assert_eq!(said["seq"], 3);
        assert_eq!(said["turnId"], "s");
        assert_eq!(said["body"], "Here is what you asked.");
    }

    #[test]
    fn a_message_that_is_only_a_tool_call_has_no_answer_body() {
        // The change it carries arrives as its own can_use_tool frame; nothing to say here.
        let said = to_response(
            &json!({
                "type": "assistant", "session_id": "s",
                "message": {"role": "assistant", "content": [
                    {"type": "tool_use", "name": "Edit", "id": "t1", "input": {}}
                ]},
            }),
            0,
        );
        assert!(said.is_none());
    }

    #[test]
    fn a_can_use_tool_edit_is_a_change_carrying_the_diff_itself() {
        let said = to_response(
            &json!({
                "type": "control_request", "request_id": "req_9",
                "request": {
                    "subtype": "can_use_tool", "tool_name": "Edit",
                    "input": {"file_path": "PriceCard.tsx", "old_string": "a\nb", "new_string": "a\nb\nc"},
                },
                "session_id": "s",
            }),
            0,
        )
        .expect("a response");
        assert_eq!(said["kind"], "change");
        assert_eq!(said["state"], "asking");
        assert_eq!(said["change"]["id"], "req_9");
        assert_eq!(said["change"]["files"][0]["path"], "PriceCard.tsx");
        assert_eq!(said["change"]["files"][0]["added"], 3);
        assert_eq!(said["change"]["files"][0]["removed"], 2);
        assert_eq!(said["change"]["files"][0]["newString"], "a\nb\nc");
    }

    #[test]
    fn a_can_use_tool_that_is_not_an_edit_is_a_yesno_question() {
        let said = to_response(
            &json!({
                "type": "control_request", "request_id": "req_2",
                "request": {"subtype": "can_use_tool", "tool_name": "Bash", "title": "Run the tests?"},
                "session_id": "s",
            }),
            0,
        )
        .expect("a response");
        assert_eq!(said["kind"], "question");
        assert_eq!(said["question"]["shape"], "yesno");
        assert_eq!(said["question"]["id"], "req_2");
    }

    #[test]
    fn the_result_frame_settles_the_turn_done_or_failed() {
        let done = to_response(&json!({"type": "result", "session_id": "s"}), 0).expect("done");
        assert_eq!(done["state"], "done");
        let failed =
            to_response(&json!({"type": "result", "session_id": "s", "is_error": true}), 0).expect("failed");
        assert_eq!(failed["state"], "failed");
    }

    #[test]
    fn a_tool_that_failed_is_not_silence() {
        // The whole reason for this event. Before it, the pill said "Editing hero.css" and
        // then nothing ever said whether it worked — which looks exactly like still working.
        let said = only(
            json!({
                "type": "user",
                "message": {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "call_2", "is_error": true,
                     "content": [{"type": "text", "text": "File does not exist."}]}
                ]},
            }),
            DID_EVENT,
        );
        assert_eq!(said["wrong"], true);
        assert!(said["said"].as_str().unwrap().contains("does not exist"));
    }

    #[test]
    fn a_tool_that_never_ran_says_which_kind_of_never() {
        let said = only(
            json!({
                "type": "user",
                "message": {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "call_3", "content": ""}
                ]},
                "tool_result_meta": [{"id": "call_3", "non_execution_kind": "user-rejected"}],
            }),
            DID_EVENT,
        );
        assert_eq!(said["never"], "user-rejected");
    }

    #[test]
    fn a_tool_call_carries_the_handle_its_outcome_will_arrive_under() {
        let said = only(
            json!({
                "type": "assistant",
                "message": {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "call_9", "name": "Edit",
                     "input": {"file_path": "/x/y.js"}}
                ]},
            }),
            DOING_EVENT,
        );
        assert_eq!(said["id"], "call_9");
        assert_eq!(said["name"], "Edit");
        assert_eq!(said["args"]["file_path"], "/x/y.js");
    }

    #[test]
    fn cost_is_read_and_never_accumulated() {
        /* The bug this replaces: `total_cost_usd` is already the running total for the
         * session, and colai added each turn's figure to a total of its own — counting every
         * turn twice over, and worse the longer somebody talked. */
        let one = only(json!({"type": "result", "subtype": "success", "total_cost_usd": 0.11}), SPENT_EVENT);
        assert_eq!(one["spent"], 0.11);
        let two = only(json!({"type": "result", "subtype": "success", "total_cost_usd": 0.19}), SPENT_EVENT);
        assert_eq!(two["spent"], 0.19, "the latest total, not 0.11 + 0.19");
    }

    #[test]
    fn a_result_carries_how_it_ended_not_only_what_it_cost() {
        let said = only(
            json!({
                "type": "result", "subtype": "error_during_execution", "is_error": true,
                "num_turns": 3, "duration_ms": 8100, "total_cost_usd": 0.4,
                "usage": {"input_tokens": 10},
                "permission_denials": [{"tool_name": "Bash"}],
            }),
            SPENT_EVENT,
        );
        assert_eq!(said["outcome"], "error_during_execution");
        assert_eq!(said["wrong"], true);
        assert_eq!(said["turns"], 3);
        assert_eq!(said["refused"][0]["tool_name"], "Bash");
    }

    #[test]
    fn a_folded_conversation_says_so_rather_than_losing_its_middle() {
        let said = only(
            json!({
                "type": "system", "subtype": "compact_boundary",
                "compact_metadata": {"trigger": "auto", "pre_tokens": 90000, "post_tokens": 12000},
            }),
            FOLDED_EVENT,
        );
        assert_eq!(said["why"], "auto");
        assert_eq!(said["before"], 90000);
    }

    #[test]
    fn a_permission_request_carries_the_change_itself() {
        /* The fact the whole approval card rests on: `can_use_tool` hands over the tool's
         * real input before it runs, and an edit's input is the before and after text. So
         * the rail can draw the change rather than a sentence about it. */
        let said = only(
            json!({
                "type": "control_request", "request_id": "req_7",
                "request": {
                    "subtype": "can_use_tool", "tool_name": "Edit",
                    "input": {
                        "file_path": "/p/src/rail.js",
                        "old_string": "const gap = 8;",
                        "new_string": "const gap = 12;",
                    },
                    "title": "Edit rail.js", "default_to_no": false,
                },
            }),
            ASKS_EVENT,
        );
        assert_eq!(said["id"], "req_7", "the handle the answer must go back under");
        assert_eq!(said["tool"], "Edit");
        assert_eq!(said["input"]["old_string"], "const gap = 8;");
        assert_eq!(said["input"]["new_string"], "const gap = 12;");
        assert_eq!(said["defaultNo"], false);
    }

    #[test]
    fn a_control_request_that_is_not_a_question_is_not_one() {
        // Only `can_use_tool` stops the world. Everything else on that channel is ours.
        assert!(heard(json!({
            "type": "control_request", "request_id": "x",
            "request": {"subtype": "mcp_message"}
        }))
        .is_empty());
    }

    #[test]
    fn a_typed_reason_is_flattened_before_it_becomes_part_of_a_prompt() {
        assert_eq!(plainly("  no   thanks \n\t stop "), "no thanks stop");
        assert_eq!(plainly("a\u{0}b"), "a b");
        assert_eq!(plainly(&"x".repeat(900)).chars().count(), 500);
    }

    #[test]
    fn a_frame_nobody_handles_is_silence_not_a_crash() {
        assert!(heard(json!({"type": "stream_event", "event": {}})).is_empty());
        assert!(heard(json!({"type": "control_request", "request": {}})).is_empty());
        assert!(heard(json!({})).is_empty());
    }

    // ── mirroring a conversation held in a terminal, off its transcript on disk ──────────────

    #[test]
    fn a_transcript_assistant_line_is_read_as_an_answer_for_the_handed_session() {
        // The disk frame names the session `sessionId`; the mirror stamps it with the key it
        // was handed, so what_it_said/to_response — the live stream's own code — address the
        // reply to the mark that is waiting for it, not to whatever the transcript happened to
        // call itself.
        let seen = mirror_frame(
            &json!({
                "type": "assistant", "sessionId": "on-disk-name", "isSidechain": false,
                "message": {"role": "assistant", "content": [{"type": "text", "text": "Done — folder created."}]},
            })
            .to_string(),
            "the-handed-key",
        )
        .expect("a mirrored frame");
        assert!(seen.spoke, "an assistant with prose has spoken");
        assert!(!seen.ended_turn, "no stop_reason yet, so the turn is not called finished");
        assert!(seen.started.is_empty());
        let response = to_response(&seen.frame, 0).expect("an answer");
        assert_eq!(response["kind"], "answer");
        assert_eq!(response["turnId"], "the-handed-key", "addressed to the mark, not the disk name");
        assert_eq!(response["body"], "Done — folder created.");
    }

    #[test]
    fn a_mirrored_tool_call_and_its_result_open_and_close_the_same_handle() {
        // So the mirror can tell a turn still running (a tool out, no result yet) from one that
        // has finished — the only way to settle a transcript, which has no result frame.
        let call = mirror_frame(
            &json!({
                "type": "assistant", "sessionId": "s",
                "message": {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "call_1", "name": "Bash", "input": {"command": "mkdir doc"}}
                ]},
            })
            .to_string(),
            "k",
        )
        .expect("a call");
        assert_eq!(call.started, vec!["call_1".to_string()]);
        assert!(!call.spoke, "a bare tool call has not spoken");
        // And it still lights the pill, off the very same synthesis the live stream uses.
        assert_eq!(
            what_it_said(&call.frame)
                .iter()
                .find(|(name, _)| *name == DOING_EVENT)
                .map(|(_, body)| body["name"].clone()),
            Some(json!("Bash")),
        );

        let done = mirror_frame(
            &json!({
                "type": "user", "sessionId": "s",
                "message": {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "call_1", "content": "ok"}
                ]},
            })
            .to_string(),
            "k",
        )
        .expect("a result");
        assert_eq!(done.ended, vec!["call_1".to_string()]);
    }

    #[test]
    fn the_tailer_drains_whole_lines_and_keeps_the_half_written_one() {
        // How the mirror reads a file being appended to: a chunk that ends mid-line must not be
        // parsed until the rest of it arrives, or a reply would be lost the moment it was split
        // across two reads — which on this surface reads as the agent going silent.
        let mut buffer: Vec<u8> = Vec::new();
        let mut seq = 0u64;
        let mut pending = HashSet::new();
        let mut spoke = false;
        let mut ended = false;

        // First read: one whole assistant line, then the front half of a tool-call line.
        let whole = json!({
            "type": "assistant", "sessionId": "disk",
            "message": {"role": "assistant", "content": [{"type": "text", "text": "On it."}]},
        })
        .to_string();
        buffer.extend_from_slice(format!("{whole}\n{{\"type\":\"assist").as_bytes());
        let first = mirror_drain(&mut buffer, "k", &mut seq, &mut pending, &mut spoke, &mut ended);
        assert!(spoke, "the whole line was the agent speaking");
        assert!(
            first.iter().any(|(name, body)| *name == RESPONSE_EVENT && body["body"] == "On it."),
            "the whole line became an answer addressed to the mark",
        );
        assert!(!buffer.is_empty(), "the half-written line is held, not parsed");

        // Second read completes that line — a tool call — so it now drains and opens a handle.
        let rest = "ant\",\"sessionId\":\"disk\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"c1\",\"name\":\"Bash\",\"input\":{}}]}}\n";
        buffer.extend_from_slice(rest.as_bytes());
        let second = mirror_drain(&mut buffer, "k", &mut seq, &mut pending, &mut spoke, &mut ended);
        assert!(pending.contains("c1"), "the tool is now in flight, so the turn is not finished");
        assert!(
            second.iter().any(|(name, body)| *name == DOING_EVENT && body["name"] == "Bash"),
            "and the pill lights",
        );
        assert!(buffer.is_empty(), "nothing left over once the line completed");

        // Its result closes the handle: now nothing is running and the turn may settle.
        let result_line = json!({
            "type": "user", "sessionId": "disk",
            "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "c1", "content": "ok"}]},
        })
        .to_string();
        buffer.extend_from_slice(format!("{result_line}\n").as_bytes());
        mirror_drain(&mut buffer, "k", &mut seq, &mut pending, &mut spoke, &mut ended);
        assert!(pending.is_empty(), "the tool finished, so the mirror may now call the turn done");
    }

    #[test]
    fn the_tail_mirrors_a_growing_transcript_and_settles_when_it_goes_quiet() {
        // The end-to-end of the mirror, minus the window: a file being appended to by one thread
        // while the tail reads it on another — the real shape of a session answering in a
        // terminal while colai watches its transcript. Proves the reply comes back, the tool
        // lights and finishes, the turn settles, and the conversation from before the hand-off
        // is not re-read.
        use std::io::Write as _;

        let path = std::env::temp_dir().join(format!("colai-mirror-{}.jsonl", std::process::id()));
        let old = json!({
            "type": "assistant", "sessionId": "disk",
            "message": {"role": "assistant", "content": [{"type": "text", "text": "OLD TURN"}]},
        })
        .to_string();
        std::fs::write(&path, format!("{old}\n")).unwrap();
        // The hand-off is now: everything already written is the old conversation.
        let from = std::fs::metadata(&path).unwrap().len();

        let appender = {
            let path = path.clone();
            std::thread::spawn(move || {
                let line = |value: Value| format!("{value}\n");
                let step = Duration::from_millis(250);
                let push = |text: &str| {
                    let mut file =
                        fs::OpenOptions::new().append(true).open(&path).unwrap();
                    file.write_all(text.as_bytes()).unwrap();
                };
                std::thread::sleep(step);
                push(&line(json!({
                    "type": "assistant", "sessionId": "disk",
                    "message": {"role": "assistant", "content": [{"type": "text", "text": "Working on it."}]},
                })));
                std::thread::sleep(step);
                push(&line(json!({
                    "type": "assistant", "sessionId": "disk",
                    "message": {"role": "assistant", "content": [{"type": "tool_use", "id": "c1", "name": "Bash", "input": {"command": "mkdir doc"}}]},
                })));
                std::thread::sleep(step);
                push(&line(json!({
                    "type": "user", "sessionId": "disk",
                    "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "c1", "content": "ok"}]},
                })));
                std::thread::sleep(step);
                push(&line(json!({
                    "type": "assistant", "sessionId": "disk",
                    "message": {"role": "assistant", "stop_reason": "end_turn", "content": [{"type": "text", "text": "All done."}]},
                })));
                // The end_turn is what the tail settles on — not the quiet after it.
            })
        };

        let mut got: Vec<(String, Value)> = Vec::new();
        let mut emit = |name: &str, body: Value| got.push((name.to_string(), body));
        mirror_tail_at(&path, "the-mark", from, Instant::now(), &mut emit);
        appender.join().unwrap();
        let _ = std::fs::remove_file(&path);

        let bodies: Vec<String> = got
            .iter()
            .filter(|(name, _)| name == RESPONSE_EVENT)
            .filter_map(|(_, body)| body["body"].as_str().map(str::to_string))
            .collect();
        assert!(bodies.iter().any(|body| body == "Working on it."), "the first reply came back");
        assert!(bodies.iter().any(|body| body == "All done."), "and the last");
        assert!(
            !got.iter().any(|(_, body)| body["body"] == "OLD TURN"),
            "the conversation from before the hand-off is not re-read",
        );
        assert!(
            got.iter().any(|(name, body)| name == DOING_EVENT && body["name"] == "Bash"),
            "the tool lit",
        );
        assert!(
            got.iter().any(|(name, body)| name == DID_EVENT && body["id"] == "c1"),
            "and finished",
        );
        let last = got.last().expect("some events");
        assert_eq!(last.0, RESPONSE_EVENT);
        assert_eq!(last.1["state"], "done", "the turn settles at the end, so the card stops working");
        assert_eq!(last.1["turnId"], "the-mark", "addressed to the mark that is waiting");
    }

    #[test]
    fn a_stop_request_settles_the_mirror_even_with_a_tool_still_in_flight() {
        // Stop cannot interrupt a terminal turn, but it must let colai stop watching one — settle
        // the card at once, not wait out the 45s stall. A turn with a tool call open and no
        // end_turn would otherwise keep the mirror running.
        let path =
            std::env::temp_dir().join(format!("colai-mirror-stop-{}.jsonl", std::process::id()));
        let line = json!({
            "type": "assistant", "sessionId": "disk",
            "message": {"role": "assistant", "content": [
                {"type": "tool_use", "id": "c1", "name": "Bash", "input": {}}
            ]},
        })
        .to_string();
        std::fs::write(&path, format!("{line}\n")).unwrap();

        stop_mirror("mid-turn-key");
        let mut got: Vec<(String, Value)> = Vec::new();
        let mut emit = |name: &str, body: Value| got.push((name.to_string(), body));
        let began = Instant::now();
        mirror_tail_at(&path, "mid-turn-key", 0, Instant::now(), &mut emit);
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "the stop is honoured on the first poll, not after the stall",
        );
        let _ = std::fs::remove_file(&path);
        let last = got.last().expect("a settle event");
        assert_eq!(last.0, RESPONSE_EVENT);
        assert_eq!(last.1["state"], "done", "the card settles rather than hanging on working");
        // And the request is one-shot: a later mirror on the same key is not cut short by it.
        assert!(!take_mirror_stop("mid-turn-key"), "the stop was taken, not left to fire again");
    }

    #[test]
    fn a_long_running_tool_is_not_mistaken_for_a_finished_turn() {
        use std::time::Duration;
        let s = |spoke, ended, nothing_pending, quiet_s, elapsed_s| {
            settle_decision(spoke, ended, nothing_pending, Duration::from_secs_f64(quiet_s), Duration::from_secs(elapsed_s))
        };
        // The bug: a tool running longer than the stall timeout (pending, long quiet) used to
        // settle the card to "done" while the agent was still working. It must not.
        assert!(!s(true, false, false, 600.0, 600), "a tool in flight is work, never a stall");
        // The normal end: the model said end_turn, nothing pending, past the short grace.
        assert!(s(true, true, true, 1.0, 5), "end_turn with nothing pending settles");
        // end_turn but a tool somehow still open — do not settle until it resolves (or the cap).
        assert!(!s(true, true, false, 10.0, 30), "end_turn does not settle over work still pending");
        // The grace: right after end_turn, wait a moment for any trailing line.
        assert!(!s(true, true, true, 0.1, 5), "settle waits out the end grace");
        // The stall net: no end_turn, nothing pending, gone quiet long enough — a crash/interrupt.
        assert!(s(true, false, true, 200.0, 240), "a quiet turn with no work pending finally stalls");
        assert!(!s(true, false, true, 60.0, 90), "but not after an ordinary think-between-steps gap");
        assert!(!s(false, false, true, 999.0, 999), "a turn that never spoke is not declared finished");
        // The cap always wins, even with a tool still in flight (a turn that truly never ends).
        assert!(s(true, false, false, 300.0, 20 * 60 + 1), "the hard cap is the last backstop");
    }

    #[test]
    fn the_turn_is_called_finished_only_when_the_model_says_end_turn() {
        // The fix for a card that settled on the wrong text: end_turn is read from the frame, not
        // guessed from a pause. A tool_use stop is more-to-come; end_turn is the full stop.
        let more = mirror_frame(
            &json!({
                "type": "assistant", "sessionId": "s", "message": {"role": "assistant",
                "stop_reason": "tool_use", "content": [{"type": "tool_use", "id": "c", "name": "Bash", "input": {}}]},
            })
            .to_string(),
            "k",
        )
        .expect("a frame");
        assert!(!more.ended_turn, "a tool_use stop means the agent is about to do something");

        let last = mirror_frame(
            &json!({
                "type": "assistant", "sessionId": "s", "message": {"role": "assistant",
                "stop_reason": "end_turn", "content": [{"type": "text", "text": "Finished."}]},
            })
            .to_string(),
            "k",
        )
        .expect("a frame");
        assert!(last.ended_turn, "end_turn is the honest end of the turn");
    }

    #[test]
    fn the_transcripts_own_bookkeeping_is_not_mistaken_for_the_turn() {
        // None of these is the answer to a mark: a subagent's private working, a meta note, or
        // the mode and system lines a transcript keeps for itself.
        assert!(mirror_frame(
            &json!({
                "type": "assistant", "isSidechain": true,
                "message": {"role": "assistant", "content": [{"type": "text", "text": "subagent thinking"}]},
            })
            .to_string(),
            "k",
        )
        .is_none());
        assert!(mirror_frame(
            &json!({
                "type": "user", "isMeta": true,
                "message": {"role": "user", "content": [{"type": "text", "text": "a note"}]},
            })
            .to_string(),
            "k",
        )
        .is_none());
        assert!(mirror_frame(&json!({"type": "mode", "mode": "default"}).to_string(), "k").is_none());
        assert!(mirror_frame(&json!({"type": "system", "subtype": "init"}).to_string(), "k").is_none());
        assert!(mirror_frame("not json", "k").is_none());
    }
}

/// The frame's `subtype`, or nothing.
fn subtype(frame: &Value) -> &str {
    frame.get("subtype").and_then(Value::as_str).unwrap_or_default()
}

/// Whatever a tool result actually said, as text.
///
/// The content is a string on the simple path and a list of blocks on the rest; both mean
/// the same thing to somebody reading a row on the rail. `pub(crate)` because `relay.rs`
/// reads the exact same shape out of a `SendMessage` tool_result, to know whether the mark
/// it just tried to deliver actually went.
pub(crate) fn said_in(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

/// Anything `claude` complains about on the way down.
///
/// Its stderr used to go to `Stdio::null()`, so a missing library, a refused login or a
/// flag this build does not know became a toolbar that simply never answered. Forwarded as
/// trouble the page can show, because a reason somebody can act on is the whole difference.
fn complain(app: AppHandle, trouble: std::process::ChildStderr) {
    for line in BufReader::new(trouble).lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let _ = app.emit_to(
            crate::colai::OVERLAY_LABEL,
            "colai:trouble",
            json!({ "said": briefly(line) }),
        );
    }
}

/// The conversation the toolbar was started from, when it was started from one.
///
/// Claude Code puts `CLAUDE_CODE_SESSION_ID` into the environment of everything the Bash
/// tool spawns, so a toolbar launched by `/colai:show` already knows which chat asked for
/// it — and that is nearly always the chat the first mark is meant for. Before this, the
/// rail opened on a picker and the most repeated act in using the toolbar was telling it
/// what it could have read.
///
/// `--in <id>` is read first because `show.md` passes it explicitly: the environment is
/// the reliable route and the flag is the visible one, and a file somebody can read is
/// worth more than a variable they cannot see. Either may be absent — a toolbar started
/// from a terminal belongs to no conversation, and says so by choosing none.
pub(crate) struct CameFrom(Mutex<Option<String>>);

impl CameFrom {
    /// What the launch said, or nothing.
    pub(crate) fn read(&self) -> Option<String> {
        self.0.lock().ok().and_then(|held| held.clone())
    }

    /// A later launch said which conversation it came from.
    ///
    /// Kept, rather than only used at startup, because a second `/colai:show` from another
    /// chat is somebody saying "this one now" — the same words, from a different room.
    /// Returns whether it actually changed, so the caller can stay quiet when it did not.
    pub(crate) fn heard(&self, said: Option<String>) -> bool {
        let Ok(mut held) = self.0.lock() else {
            return false;
        };
        if *held == said || said.is_none() {
            return false;
        }
        *held = said;
        true
    }
}

impl Default for CameFrom {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}

/// Read a launch's arguments and environment for the conversation it belongs to.
pub(crate) fn came_from(args: &[String]) -> Option<String> {
    let flagged = args
        .iter()
        .position(|word| word == "--in")
        .and_then(|at| args.get(at + 1))
        .map(String::as_str)
        .or_else(|| {
            args.iter()
                .find_map(|word| word.strip_prefix("--in="))
        });
    let said = match flagged {
        Some(said) => said.to_string(),
        None => std::env::var("CLAUDE_CODE_SESSION_ID").ok()?,
    };
    let said = said.trim();
    // A shell that substituted nothing leaves the literal text behind, and `show.md` is
    // markdown before it is a command line — so an unsubstituted placeholder arrives here
    // looking like an id. It is not one, and treating it as one would point the toolbar at
    // a conversation that does not exist.
    if said.is_empty() || said.starts_with('$') || said.starts_with('{') {
        return None;
    }
    Some(said.to_string())
}

/// Every conversation Claude Code has on this machine, newest first.
///
/// Read off disk rather than asked for, because there is nothing to ask: the transcripts
/// under `~/.claude/projects` *are* the session list, one JSONL per conversation, and the
/// Agent SDK's own `listSessions` reads the same files.
///
/// That is a private format with no promise attached to it, so everything here is optional
/// and nothing throws: a file that has changed shape costs its title, not the list.
pub(crate) fn conversations(limit: usize) -> Vec<Conversation> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let projects = home.join(".claude").join("projects");
    // A poisoned lock means a scan panicked part-way; what it left is at worst a transcript
    // read short, which the next change to that file re-reads. Not worth losing the list over.
    let mut held = scanned().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    conversations_under(&projects, limit, &mut held)
}

/*
 * The list is asked for every five seconds, and reading it used to mean reading all of it:
 * every transcript, start to finish, every line through the JSON parser — forty-odd megabytes
 * for a few dozen conversations, a fifth of a second of a worker thread each time, to learn
 * that nothing had changed.
 *
 * Transcripts only grow. Claude Code appends a line per thing that happens and rewrites
 * nothing, so what a transcript said up to the byte last read is still what it says. So each
 * one is remembered as far as it was read — how long it was, when it last changed, and what
 * it had said by then — and a call costs a directory listing and a stat per file. A file that
 * has not moved is not opened. One that has grown is read from where the last read stopped.
 * One that has shrunk, or changed without growing, or whose first bytes are no longer the
 * first bytes it had, is not the file that was read, and is read again from the top.
 */

/// How many bytes from the top of a transcript are kept to recognise it by. Its first line
/// names its session and the moment it began, so a file that still starts the same way is the
/// one that was read; one that does not was replaced, and is read again.
const SCANNED_HEAD: usize = 256;

/// Everything read so far, by transcript path, for as long as the toolbar runs.
fn scanned() -> &'static Mutex<HashMap<PathBuf, Scanned>> {
    static SCANNED: std::sync::OnceLock<Mutex<HashMap<PathBuf, Scanned>>> =
        std::sync::OnceLock::new();
    SCANNED.get_or_init(|| Mutex::new(HashMap::new()))
}

/// How far one transcript has been read, and what it had said by then.
#[derive(Default)]
struct Scanned {
    /// Bytes read, which is how long the file was at the last look.
    len: u64,
    /// When it last changed, at the last look.
    modified: Option<std::time::SystemTime>,
    /// The first bytes it had, up to `SCANNED_HEAD`.
    head: Vec<u8>,
    /// A last line not yet ended — still being written, or never going to be.
    leftover: Vec<u8>,
    gist: Gist,
}

/// What the list needs out of a transcript, as far as it has been read.
#[derive(Default, Clone)]
struct Gist {
    /// The first directory any entry recorded, which is where the conversation is had.
    cwd: Option<String>,
    /// The latest generated title.
    title: Option<String>,
    /// The latest thing somebody typed.
    prompt: Option<String>,
    /// The latest running total.
    spent: f64,
}

/// The entry types that carry what `Gist` keeps, as they appear in a line, quotes and all.
///
/// Every other line in a transcript is somebody's words or a tool's output, and most of the
/// bytes are those — so once the directory is known a line is searched for these before it
/// is parsed, and almost none are. Matched with their quotes, because the words themselves
/// may well appear in a conversation about this very code; that costs a parse, not an answer,
/// since the parsed line's type is checked again.
const GIST_KINDS: [&[u8]; 3] = [b"\"ai-title\"", b"\"last-prompt\"", b"\"cost-state\""];

impl Gist {
    /// Take in one line of a transcript.
    fn take_in(&mut self, line: &[u8]) {
        if self.cwd.is_some()
            && !GIST_KINDS
                .iter()
                .any(|kind| line.windows(kind.len()).any(|here| here == *kind))
        {
            return;
        }
        let Ok(entry) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        if self.cwd.is_none() {
            self.cwd = entry.get("cwd").and_then(Value::as_str).map(str::to_string);
        }
        match entry.get("type").and_then(Value::as_str) {
            // The generated title, which is what the session list in Claude Code shows.
            Some("ai-title") => {
                if let Some(said) = entry.get("aiTitle").and_then(Value::as_str) {
                    self.title = Some(said.to_string());
                }
            }
            // What they last asked, which beats "Untitled" for a conversation too young
            // to have been given a name.
            Some("last-prompt") => {
                if let Some(said) = entry.get("lastPrompt").and_then(Value::as_str) {
                    self.prompt = Some(said.to_string());
                }
            }
            /*
             * Claude Code keeps the running total itself, so the rail can say what a
             * conversation cost before anybody adds to it.
             *
             * A number or a string, and both are real: this was written reading `as_str`
             * only, because an inspection script had printed the value through `str()` and
             * put quotes round a number. Every cost came back zero and the field looked
             * absent rather than mistyped.
             *
             * Sessions started with `--print` — which is how the toolbar talks — carry no
             * `cost-state` at all; only interactive ones do. So zero here means "nothing
             * recorded", not "free", and the live total the toolbar keeps is the one that
             * covers the conversation it is having.
             */
            Some("cost-state") => {
                if let Some(said) = cost_in(entry.get("totalCostUSD")) {
                    self.spent = said;
                }
            }
            _ => {}
        }
    }
}

impl Scanned {
    /// Bring this up to date with the file at `path`, as `meta` describes it. `false` when it
    /// cannot be read, which drops it from the list as an unreadable file always was.
    fn catch_up(&mut self, path: &Path, meta: &fs::Metadata) -> bool {
        let len = meta.len();
        let modified = meta.modified().ok();
        let begun = self.modified.is_some();
        if begun && len == self.len && modified == self.modified {
            return true;
        }
        let Ok(mut file) = fs::File::open(path) else {
            return false;
        };
        let grown = begun && len > self.len && self.starts_as_it_did(&mut file);
        if !grown {
            *self = Scanned::default();
        }
        if file.seek(SeekFrom::Start(self.len)).is_err() {
            return false;
        }
        // Up to the length just seen and no further, so what was read and what `len` says
        // was read stay the same number however fast the file is growing.
        let mut chunk = Vec::new();
        if (&mut file).take(len - self.len).read_to_end(&mut chunk).is_err() {
            return false;
        }
        self.len += chunk.len() as u64;
        self.modified = modified;
        let room = SCANNED_HEAD.saturating_sub(self.head.len());
        self.head.extend_from_slice(&chunk[..room.min(chunk.len())]);

        self.leftover.extend_from_slice(&chunk);
        let mut start = 0;
        while let Some(nl) = self.leftover[start..].iter().position(|&byte| byte == b'\n') {
            self.gist.take_in(&self.leftover[start..start + nl + 1]);
            start += nl + 1;
        }
        self.leftover.drain(..start);
        true
    }

    /// Whether the file still begins with the bytes it began with when it was read.
    fn starts_as_it_did(&self, file: &mut fs::File) -> bool {
        let mut now = vec![0; self.head.len()];
        file.seek(SeekFrom::Start(0)).is_ok() && file.read_exact(&mut now).is_ok() && now == self.head
    }

    /// The row for this transcript.
    ///
    /// A last line with no newline yet is read as well, without being kept: a reader that
    /// went start to finish would have read it, and when its newline lands it is read again
    /// as the line it then is.
    fn conversation(&self, key: String, at: u64) -> Conversation {
        let mut whole;
        let gist = if self.leftover.is_empty() {
            &self.gist
        } else {
            whole = self.gist.clone();
            whole.take_in(&self.leftover);
            &whole
        };
        let last_prompt = gist.prompt.as_deref().and_then(|said| at_most(said, LAST_PROMPT_AT_MOST));
        let name = gist
            .title
            .as_deref()
            .or(gist.prompt.as_deref())
            .map(briefly)
            .unwrap_or_else(|| "Untitled".to_string());
        Conversation {
            session_key: key,
            name,
            cwd: gist.cwd.clone(),
            at,
            spent: gist.spent,
            last_prompt,
        }
    }
}

/// `conversations`, for any folder laid out as `~/.claude/projects` is and against any
/// memory of it — split out so a test can grow and cut transcripts of its own and compare
/// what a remembering scan says with what a fresh one does.
fn conversations_under(
    projects: &Path,
    limit: usize,
    held: &mut HashMap<PathBuf, Scanned>,
) -> Vec<Conversation> {
    let Ok(dirs) = fs::read_dir(projects) else {
        return Vec::new();
    };

    let mut found: Vec<Conversation> = Vec::new();
    let mut there: HashSet<PathBuf> = HashSet::new();
    for dir in dirs.flatten() {
        let Ok(files) = fs::read_dir(dir.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(key) = path.file_stem().and_then(|stem| stem.to_str()).map(str::to_string) else {
                continue;
            };
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            let at = meta
                .modified()
                .ok()
                .and_then(|when| when.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|since| since.as_millis() as u64)
                .unwrap_or(0);
            let scan = held.entry(path.clone()).or_default();
            if !scan.catch_up(&path, &meta) {
                held.remove(&path);
                continue;
            }
            found.push(scan.conversation(key, at));
            there.insert(path);
        }
    }
    // Forget transcripts that have gone, so a long-running toolbar does not hold on to every
    // conversation it ever listed. Only under this folder: the memory is shared with nobody
    // else in the app, but a test points this at folders of its own.
    held.retain(|path, _| !path.starts_with(projects) || there.contains(path));

    found.sort_by(|a, b| b.at.cmp(&a.at));
    found.truncate(limit);
    found
}

/// What the toolbar is allowed to do without asking.
///
/// `acceptEdits` by default, for the reason given in `Session::start`: it is the mode in
/// which the product works at all, and it is confined to the conversation's own directory.
/// `COLAI_PERMISSION` narrows it — `default` refuses every edit too — and anything the CLI
/// does not recognise is refused rather than passed along, because a typo here would
/// otherwise become a silently more permissive session.
fn permission_asked() -> String {
    const KNOWN: [&str; 3] = ["acceptEdits", "default", "plan"];
    match std::env::var("COLAI_PERMISSION") {
        Ok(asked) if KNOWN.contains(&asked.as_str()) => asked,
        Ok(asked) if !asked.is_empty() => {
            eprintln!("[colai] COLAI_PERMISSION={asked} is not one of {KNOWN:?}; using acceptEdits.");
            "acceptEdits".to_string()
        }
        _ => "acceptEdits".to_string(),
    }
}

/// Where a conversation is being had.
///
/// The cwd Claude Code recorded for it, which is the project it is about. `None` for a
/// conversation that has not started, or one whose transcript never said.
///
/// Read from the one transcript, and only as far as the first entry that says: this was a
/// scan of every conversation on the machine to find one of them, on every send and every
/// keystroke after an `@`. The first entry with a `cwd` is the answer the list gives too.
pub(crate) fn where_it_is_had(key: Option<&str>) -> Option<String> {
    let key = key?;
    // A key is a file name the list once read, never a path. This answers what may be read
    // and sent, so one that tries to climb out of `projects` is answered with nothing.
    if key.is_empty() || key.starts_with('.') || key.contains(['/', '\\', ':']) {
        return None;
    }
    let file = fs::File::open(transcript_of(key)?).ok()?;
    let mut lines = BufReader::new(file);
    let mut line = Vec::new();
    loop {
        line.clear();
        match lines.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return None,
            Ok(_) => {}
        }
        let Ok(entry) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        if let Some(cwd) = entry.get("cwd").and_then(Value::as_str) {
            return Some(cwd.to_string());
        }
    }
}

/// The folders a message may carry a file out of.
///
/// The Gateway was asked this and answered with every catalog, host and session it knew.
/// Here it is one folder: the one this conversation is being had in. That is a narrower
/// gate than the Gateway's, deliberately — a file outside the project the agent is working
/// in is one nobody asked to send.
pub(crate) fn work_roots(key: Option<&str>) -> Vec<String> {
    where_it_is_had(key).into_iter().collect()
}

/// What a recorded cost says, whichever way it was written.
///
/// Its own function so the two shapes can be tested. See the note at the call site: this
/// was written reading strings only, every cost came back zero, and the field looked
/// missing rather than mistyped.
fn cost_in(said: Option<&Value>) -> Option<f64> {
    let said = said?;
    said.as_f64()
        .or_else(|| said.as_str().and_then(|said| said.parse().ok()))
        .filter(|cost| cost.is_finite() && *cost >= 0.0)
}

/// What was said in a conversation, oldest first.
///
/// Read out of the transcript, because on this host the transcript is the only record —
/// the Gateway kept history and could be asked for it; Claude Code writes a JSONL and that
/// is that. Both sides of the conversation, because the panel shows both.
///
/// Tolerant by construction: a private format with no promise attached, so an entry that
/// has changed shape is skipped rather than fatal.
pub(crate) fn what_was_said(key: &str, most: usize) -> Vec<(String, String, bool, Option<i64>)> {
    what_was_said_and_changed(key, most).into_iter().map(|(turn, _)| turn).collect()
}

/// What was said, as `what_was_said`, with each prompt carrying the file edits Claude made
/// answering it — so a conversation can be rewound to any prompt in it, not only the last.
pub(crate) fn what_was_said_and_changed(key: &str, most: usize) -> Vec<(Turn, Vec<Change>)> {
    let Some(path) = transcript_of(key) else {
        return Vec::new();
    };
    let Ok(file) = fs::File::open(&path) else {
        return Vec::new();
    };
    turns_and_changes_in(BufReader::new(file).lines().map_while(Result::ok), most)
}

/// One turn as `what_was_said` gives it: id, words, whether it is the person's, and when.
pub(crate) type Turn = (String, String, bool, Option<i64>);

/// The turns in a run of transcript lines — the pure half of `what_was_said`, so the
/// filtering and merging can be tested without a transcript on disk.
///
/// Two things make a raw transcript unreadable as a conversation, and both are undone here:
///
/// - Most `user` entries were not typed by anybody. A tool's result comes back as a `user`
///   entry, and so do slash-command echoes, hook output, background-task notices and the
///   summary a compacted session starts with. Only what a person said — or a colai mark
///   handed in from another session, which is a person's words delivered by a relay — counts.
/// - One answer is written as several `assistant` entries, one per stretch of prose between
///   tool calls. Those are joined back into the one turn they were, so the panel shows an
///   answer rather than its fragments.
#[cfg(test)]
fn turns_in(lines: impl Iterator<Item = String>, most: usize) -> Vec<Turn> {
    turns_and_changes_in(lines, most).into_iter().map(|(turn, _)| turn).collect()
}

/// `turns_in`, with each prompt carrying the file edits Claude made answering it.
///
/// An answer's edits are its tool calls, which sit in `assistant` entries that often have no
/// prose at all — so they are gathered before the prose filter, not after it, and they go to
/// the last prompt rather than to the turn being merged, because the person rewinds to what
/// they asked, not to what was answered. A call whose result came back an error changed
/// nothing and is dropped: its result is a later entry, so the errors are collected on the
/// way through and taken out at the end.
fn turns_and_changes_in(
    lines: impl Iterator<Item = String>,
    most: usize,
) -> Vec<(Turn, Vec<Change>)> {
    let mut said: Vec<(Turn, Vec<(String, Change)>)> = Vec::new();
    let mut failed: HashSet<String> = HashSet::new();
    for line in lines {
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let mine = match entry.get("type").and_then(Value::as_str) {
            Some("user") => true,
            Some("assistant") => false,
            _ => continue,
        };
        if mine {
            failed.extend(calls_that_failed(&entry));
        }
        if an_aside(&entry) {
            continue;
        }
        if !mine {
            let made = changes_made(&entry);
            if !made.is_empty() {
                if let Some((_, edits)) = said.iter_mut().rev().find(|(turn, _)| turn.2) {
                    edits.extend(made);
                }
            }
        }
        let Some(words) = (if mine { typed_by_somebody(&entry) } else { answered(&entry) }) else {
            continue;
        };
        if words.trim().is_empty() {
            continue;
        }
        // The next stretch of an answer already underway: one turn, not two.
        if !mine {
            if let Some(((_, before, false, _), _)) = said.last_mut() {
                before.push_str("\n\n");
                before.push_str(words.trim());
                continue;
            }
        }
        let id = entry
            .get("uuid")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        said.push(((id, words.trim().to_string(), mine, None), Vec::new()));
    }
    // Newest kept when there are more than the panel asked for, oldest first on the way
    // out — the order it was said in.
    if said.len() > most {
        said.drain(..said.len() - most);
    }
    said.into_iter()
        .map(|(turn, edits)| {
            let kept = edits
                .into_iter()
                .filter(|(call, _)| !failed.contains(call))
                .map(|(_, change)| change)
                .collect();
            (turn, kept)
        })
        .collect()
}

/// The file edits in an `assistant` entry, oldest first, each with the id of the call that
/// made it. A `MultiEdit` is opened out into its single swaps, so each one can be put back.
fn changes_made(entry: &Value) -> Vec<(String, Change)> {
    let Some(Value::Array(parts)) = entry.pointer("/message/content") else {
        return Vec::new();
    };
    let mut made = Vec::new();
    for part in parts {
        if part.get("type").and_then(Value::as_str) != Some("tool_use") {
            continue;
        }
        let call = part.get("id").and_then(Value::as_str).unwrap_or_default();
        let input = part.get("input");
        let text = |from: Option<&Value>, key: &str| {
            from.and_then(|from| from.get(key)).and_then(Value::as_str).map(str::to_string)
        };
        let path = text(input, "file_path").or_else(|| text(input, "notebook_path"));
        let Some(path) = path.filter(|path| !path.is_empty()) else {
            continue;
        };
        let swap = |old: Option<String>, new: Option<String>| Change {
            path: path.clone(),
            kind: "edit".to_string(),
            old: Some(old.unwrap_or_default()),
            new: Some(new.unwrap_or_default()),
        };
        match part.get("name").and_then(Value::as_str) {
            Some("Edit") => {
                made.push((call.to_string(), swap(text(input, "old_string"), text(input, "new_string"))));
            }
            Some("MultiEdit") => {
                let swaps = input.and_then(|input| input.get("edits")).and_then(Value::as_array);
                for one in swaps.into_iter().flatten() {
                    let one = Some(one);
                    made.push((call.to_string(), swap(text(one, "old_string"), text(one, "new_string"))));
                }
            }
            // A whole file, or a notebook cell: no prior copy, and no body sent to a page.
            Some("Write") | Some("NotebookEdit") => made.push((
                call.to_string(),
                Change { path: path.clone(), kind: "write".to_string(), old: None, new: None },
            )),
            _ => {}
        }
    }
    made
}

/// The ids of the tool calls whose results, in this `user` entry, came back as errors.
fn calls_that_failed(entry: &Value) -> Vec<String> {
    let Some(Value::Array(parts)) = entry.pointer("/message/content") else {
        return Vec::new();
    };
    parts
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("tool_result"))
        .filter(|part| part.get("is_error").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|part| part.get("tool_use_id").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

/// The prose of an `assistant` entry — its text blocks, with the tool calls left out.
fn answered(entry: &Value) -> Option<String> {
    Some(words_in(entry.get("message")?.get("content")))
}

/// What a person said in a `user` entry, or `None` when nobody typed it.
///
/// The same line the mirror draws between a person's turn and the machinery around it,
/// extended for what only a reader of the whole transcript meets: a tool result (the most
/// common `user` entry by far), a compacted session's summary, an entry whose `origin` names
/// something other than a person, and the tagged echoes slash commands and hooks leave.
fn typed_by_somebody(entry: &Value) -> Option<String> {
    if entry.get("isCompactSummary").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let content = entry.get("message")?.get("content");
    if let Some(Value::Array(parts)) = content {
        if parts
            .iter()
            .any(|part| part.get("type").and_then(Value::as_str) == Some("tool_result"))
        {
            return None;
        }
    }
    match entry.pointer("/origin/kind").and_then(Value::as_str) {
        None | Some("human") => {}
        // A mark colai handed to a chat somebody has open arrives as a message from another
        // session, wrapped in a preamble addressed to the agent. The body is what was sent —
        // the person's own message — so that is what is shown.
        Some("peer") => {
            if let Some(body) = entry.pointer("/origin/body").and_then(Value::as_str) {
                return Some(body.to_string());
            }
        }
        Some(_) => return None,
    }
    let words = words_in(content);
    let start = words.trim_start();
    if start.starts_with("[Request interrupted") || machine_tagged(start) {
        return None;
    }
    Some(words)
}

/// Whether text opens with one of the tags Claude Code wraps its own output in, rather than
/// with anything a person would type.
fn machine_tagged(start: &str) -> bool {
    const TAGS: [&str; 12] = [
        "command-name", "command-message", "command-args",
        "local-command-stdout", "local-command-stderr", "local-command-caveat",
        "bash-input", "bash-stdout", "bash-stderr",
        "user-prompt-submit-hook", "system-reminder", "task-notification",
    ];
    let Some(rest) = start.strip_prefix('<') else {
        return false;
    };
    TAGS.iter().any(|tag| {
        rest.strip_prefix(tag)
            .is_some_and(|after| after.starts_with('>') || after.starts_with(' '))
    })
}

/// The text of a message, whichever way its content was written.
fn words_in(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(said)) => said.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

/// Which file holds a conversation. Searched rather than computed: the directory name is
/// the project path with its separators replaced, and reversing that is guesswork where
/// looking is not.
fn transcript_of(key: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let projects = home.join(".claude").join("projects");
    for dir in fs::read_dir(&projects).ok()?.flatten() {
        let here = dir.path().join(format!("{key}.jsonl"));
        if here.exists() {
            return Some(here);
        }
    }
    None
}

/// A title the rail has room for. Cut on a word, because a name that stops mid-word reads
/// as a bug rather than as a name that was too long.
fn briefly(said: &str) -> String {
    let said = said.split_whitespace().collect::<Vec<_>>().join(" ");
    if said.chars().count() <= 60 {
        return said;
    }
    let cut: String = said.chars().take(60).collect();
    match cut.rsplit_once(' ') {
        Some((head, _)) if head.chars().count() > 20 => format!("{head}…"),
        _ => format!("{cut}…"),
    }
}

/// A prompt kept as it was typed — line breaks and all, since the panel draws it pre-wrapped —
/// but no longer than `most` characters, cut with an ellipsis. `None` for one that is only
/// whitespace, which is nothing to show.
fn at_most(said: &str, most: usize) -> Option<String> {
    let said = said.trim();
    if said.is_empty() {
        return None;
    }
    if said.chars().count() <= most {
        return Some(said.to_string());
    }
    let cut: String = said.chars().take(most).collect();
    Some(format!("{}…", cut.trim_end()))
}

#[cfg(test)]
mod tests {
    use super::{at_most, briefly, cost_in, permission_asked, turns_and_changes_in, turns_in};
    use crate::wire::Change;
    use serde_json::{json, Value};

    /// An answer that only calls tools — the usual shape of an entry that edits a file.
    fn calls(id: &str, tools: &[Value]) -> Value {
        json!({"type": "assistant", "uuid": id, "message": {"role": "assistant", "content": tools}})
    }

    fn edit_call(call: &str, path: &str, old: &str, new: &str) -> Value {
        json!({"type": "tool_use", "id": call, "name": "Edit",
            "input": {"file_path": path, "old_string": old, "new_string": new}})
    }

    fn failed(call: &str) -> Value {
        json!({"type": "user", "message": {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": call, "is_error": true, "content": "String not found"},
        ]}})
    }

    fn swapped(path: &str, old: &str, new: &str) -> Change {
        Change { path: path.into(), kind: "edit".into(), old: Some(old.into()), new: Some(new.into()) }
    }

    #[test]
    fn a_prompt_carries_its_edits_in_the_order_they_were_made() {
        let said = turns_and_changes_in(
            transcript(&[
                typed("u1", "make the button red"),
                answer("a1", "On it."),
                calls("a2", &[edit_call("e1", "/p/a.css", "blue", "red")]),
                tool_result("r1"),
                calls("a3", &[json!({"type": "tool_use", "id": "m1", "name": "MultiEdit", "input": {
                    "file_path": "/p/b.css",
                    "edits": [{"old_string": "x", "new_string": "y"}, {"old_string": "1", "new_string": "2"}],
                }})]),
                tool_result("r2"),
                calls("a4", &[json!({"type": "tool_use", "id": "w1", "name": "Write",
                    "input": {"file_path": "/p/c.txt", "content": "a whole file"}})]),
                answer("a5", "Done."),
            ]),
            40,
        );
        assert_eq!(said.len(), 2, "{said:?}");
        assert_eq!(
            said[0].1,
            [
                swapped("/p/a.css", "blue", "red"),
                swapped("/p/b.css", "x", "y"),
                swapped("/p/b.css", "1", "2"),
                Change { path: "/p/c.txt".into(), kind: "write".into(), old: None, new: None },
            ]
        );
        let shipped = serde_json::to_value(&said[0].1[3]).unwrap();
        assert_eq!(shipped, json!({"path": "/p/c.txt", "kind": "write"}), "no file body goes out");
        assert!(said[1].1.is_empty(), "an answer carries no edits");
    }

    #[test]
    fn an_edit_that_failed_is_not_offered_back() {
        let said = turns_and_changes_in(
            transcript(&[
                typed("u1", "fix it"),
                calls("a1", &[edit_call("e1", "/p/a.rs", "nope", "yes")]),
                failed("e1"),
                calls("a2", &[edit_call("e2", "/p/a.rs", "old", "new")]),
                tool_result("r2"),
                answer("a3", "Fixed."),
            ]),
            40,
        );
        assert_eq!(said[0].1, [swapped("/p/a.rs", "old", "new")]);
    }

    #[test]
    fn each_prompt_keeps_its_own_edits() {
        let said = turns_and_changes_in(
            transcript(&[
                typed("u1", "first"),
                answer("a1", "One."),
                calls("a2", &[edit_call("e1", "/p/a", "1", "2")]),
                typed("u2", "second"),
                calls("a3", &[edit_call("e2", "/p/b", "3", "4")]),
                answer("a4", "Two."),
                calls("a5", &[edit_call("e3", "/p/c", "5", "6")]),
            ]),
            40,
        );
        let mine: Vec<_> = said.iter().filter(|(turn, _)| turn.2).collect();
        assert_eq!(mine.len(), 2);
        assert_eq!(mine[0].1, [swapped("/p/a", "1", "2")]);
        assert_eq!(mine[1].1, [swapped("/p/b", "3", "4"), swapped("/p/c", "5", "6")]);
        assert!(said.iter().filter(|(turn, _)| !turn.2).all(|(_, edits)| edits.is_empty()));
    }

    /// A transcript, one JSON line per entry, the way `what_was_said` reads it off disk.
    fn transcript(entries: &[Value]) -> impl Iterator<Item = String> {
        entries.iter().map(Value::to_string).collect::<Vec<_>>().into_iter()
    }

    fn typed(id: &str, words: &str) -> Value {
        json!({"type": "user", "uuid": id, "message": {"role": "user", "content": words}})
    }

    fn answer(id: &str, words: &str) -> Value {
        json!({"type": "assistant", "uuid": id, "message": {"role": "assistant", "content": [
            {"type": "text", "text": words},
            {"type": "tool_use", "id": format!("t-{id}"), "name": "Read", "input": {}},
        ]}})
    }

    fn tool_result(id: &str) -> Value {
        json!({"type": "user", "uuid": id, "message": {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "t", "content": "file contents"},
        ]}})
    }

    #[test]
    fn the_fragments_of_one_answer_are_one_turn() {
        // An answer is written as one entry per stretch of prose between tool calls, with a
        // tool result in between each. Shown raw that is four turns for one reply.
        let said = turns_in(
            transcript(&[
                typed("u1", "make the button red"),
                answer("a1", "Looking at the stylesheet."),
                tool_result("r1"),
                answer("a2", "Found it."),
                tool_result("r2"),
                answer("a3", "Done — the button is red."),
            ]),
            40,
        );
        assert_eq!(said.len(), 2, "{said:?}");
        assert_eq!(said[0].1, "make the button red");
        assert!(said[0].2);
        assert_eq!(said[1].0, "a1", "the merged turn keeps its first entry's id");
        assert_eq!(said[1].1, "Looking at the stylesheet.\n\nFound it.\n\nDone — the button is red.");
        assert!(!said[1].2);
    }

    #[test]
    fn nothing_nobody_typed_is_shown_as_theirs() {
        let said = turns_in(
            transcript(&[
                tool_result("r0"),
                json!({"type": "user", "isMeta": true, "message": {"content": "<local-command-caveat>Caveat</local-command-caveat>"}}),
                json!({"type": "user", "message": {"content": "<command-name>/plan</command-name>"}}),
                json!({"type": "user", "message": {"content": "<local-command-stdout>Enabled plan mode</local-command-stdout>"}}),
                json!({"type": "user", "origin": {"kind": "task-notification"}, "message": {"content": "<task-notification>done</task-notification>"}}),
                json!({"type": "user", "isCompactSummary": true, "message": {"content": "This session is being continued"}}),
                json!({"type": "user", "message": {"content": [{"type": "text", "text": "[Request interrupted by user]"}]}}),
                json!({"type": "user", "isSidechain": true, "message": {"content": "a subagent's brief"}}),
                typed("u1", "what is <div> for?"),
            ]),
            40,
        );
        assert_eq!(said.len(), 1, "{said:?}");
        assert_eq!(said[0].1, "what is <div> for?");
    }

    #[test]
    fn a_mark_handed_in_from_colai_is_shown_as_the_message_that_was_sent() {
        let said = turns_in(
            transcript(&[json!({
                "type": "user", "uuid": "p1",
                "origin": {"kind": "peer", "body": "The person marked something.\n\nMake it red."},
                "message": {"content": "Another Claude session sent a message:\n<cross-session-message>…"},
            })]),
            40,
        );
        assert_eq!(said.len(), 1);
        assert_eq!(said[0].1, "The person marked something.\n\nMake it red.");
        assert!(said[0].2);
    }

    #[test]
    fn turns_come_back_in_the_order_they_were_said_newest_kept() {
        let said = turns_in(
            transcript(&[
                typed("u1", "one"),
                answer("a1", "two"),
                tool_result("r1"),
                typed("u2", "three"),
                answer("a2", "four"),
                answer("a3", "five"),
            ]),
            3,
        );
        let words: Vec<&str> = said.iter().map(|(_, w, _, _)| w.as_str()).collect();
        assert_eq!(words, ["two", "three", "four\n\nfive"]);
    }

    #[test]
    fn a_last_prompt_is_kept_whole_up_to_its_cap() {
        assert_eq!(at_most("  two\nlines  ", 400).as_deref(), Some("two\nlines"));
        assert_eq!(at_most("   ", 400), None);
        let long = at_most(&"word ".repeat(200), 400).expect("a prompt");
        assert!(long.ends_with('…'));
        assert!(long.chars().count() <= 401);
    }

    #[test]
    fn an_unknown_permission_is_refused_rather_than_passed_along() {
        /*
         * Measured behaviour this protects, from probing the real CLI:
         *
         *   acceptEdits  write inside the working directory   succeeds
         *   acceptEdits  write outside it                     denied
         *   acceptEdits  Bash                                 blocked
         *
         * So the mode decides how much of somebody's machine is reachable without being
         * asked, and a typo that fell through to a broader one would widen that silently.
         */
        std::env::set_var("COLAI_PERMISSION", "bypassPermissions");
        assert_eq!(permission_asked(), "acceptEdits", "an unknown mode must not be honoured");
        std::env::set_var("COLAI_PERMISSION", "default");
        assert_eq!(permission_asked(), "default", "a stricter mode must be honoured");
        std::env::remove_var("COLAI_PERMISSION");
        assert_eq!(permission_asked(), "acceptEdits");
    }

    #[test]
    fn a_cost_is_read_whether_it_is_a_number_or_a_string() {
        // Both occur in real transcripts. Reading only one of them made every session
        // free, which is the worst way for a cost display to be wrong.
        assert_eq!(cost_in(Some(&json!(0.3672968))), Some(0.3672968));
        assert_eq!(cost_in(Some(&json!("0.3672968"))), Some(0.3672968));
    }

    #[test]
    fn nonsense_in_that_field_is_no_cost_rather_than_a_wrong_one() {
        // A private format with no promise attached: better to show nothing than to show
        // a number that came from a shape nobody expected.
        assert_eq!(cost_in(None), None);
        assert_eq!(cost_in(Some(&json!("free"))), None);
        assert_eq!(cost_in(Some(&json!(-1.0))), None);
        assert_eq!(cost_in(Some(&json!(f64::INFINITY))), None);
    }


    #[test]
    fn a_short_title_is_left_alone() {
        assert_eq!(briefly("Ubuntu screen recording"), "Ubuntu screen recording");
    }

    #[test]
    fn a_long_title_is_cut_on_a_word() {
        let said = briefly(&"alpha beta gamma delta epsilon zeta eta theta iota kappa".repeat(3));
        assert!(said.ends_with('…'));
        assert!(said.chars().count() <= 61);
        // Not mid-word: whatever is before the ellipsis is a whole word.
        assert!(!said.trim_end_matches('…').ends_with(' '));
    }

    #[test]
    fn whitespace_in_a_prompt_is_flattened() {
        // A `last-prompt` carries whatever somebody typed, newlines included, and a name
        // with a line break in it breaks the row it is drawn in.
        assert_eq!(briefly("two\n\nlines   here"), "two lines here");
    }
}

#[cfg(test)]
mod reading {
    use super::conversations;

    /// Not a unit test: it reads this machine's own transcripts, so it asserts the shape
    /// of what comes back rather than any particular conversation. Ignored by default —
    /// a machine with no Claude Code history would fail it for the wrong reason.
    #[test]
    #[ignore = "reads ~/.claude/projects on this machine"]
    fn it_reads_the_conversations_on_this_machine() {
        let found = conversations(8);
        assert!(!found.is_empty(), "no conversations found");
        for one in &found {
            assert!(!one.session_key.is_empty());
            assert!(!one.name.is_empty());
            assert!(one.spent >= 0.0);
            println!(
                "  {:>9.4}  {:<22} {}",
                one.spent,
                one.cwd.as_deref().unwrap_or("?").rsplit('/').next().unwrap_or("?"),
                one.name,
            );
        }
        // Newest first, which is the order the rail draws them in.
        assert!(found.windows(2).all(|pair| pair[0].at >= pair[1].at));
    }
}

#[cfg(test)]
mod transcripts {
    use super::{conversations, what_was_said};

    #[test]
    #[ignore = "reads ~/.claude/projects on this machine"]
    fn it_reads_back_what_was_said_in_a_real_conversation() {
        let newest = conversations(1).pop().expect("a conversation");
        let said = what_was_said(&newest.session_key, 40);
        assert!(!said.is_empty(), "no turns read from {}", newest.name);
        // Both sides, not only one: the panel shows the conversation, not a monologue.
        // Both sides. Most `user` entries in an agentic session are tool results with no
        // text and are skipped, so a narrow window can be all assistant — 6 was, which is
        // why this asks for more rather than asserting on the last few.
        assert!(said.iter().any(|(_, _, mine, _)| *mine), "nothing of theirs");
        println!("  read {} turns, {} theirs", said.len(), said.iter().filter(|(_,_,m,_)| *m).count());
        for (_, words, mine, _) in said.iter().take(6) {
            let who = if *mine { "them" } else { "claude" };
            println!("  {who:>6}: {}", words.replace('\n', " ").chars().take(64).collect::<String>());
        }
    }
}

#[cfg(test)]
mod which_conversation {
    use super::*;

    fn asked(args: &[&str], env: Option<&str>) -> Option<String> {
        let _held = crate::hold_the_environment();
        match env {
            Some(said) => std::env::set_var("CLAUDE_CODE_SESSION_ID", said),
            None => std::env::remove_var("CLAUDE_CODE_SESSION_ID"),
        }
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let found = came_from(&owned);
        std::env::remove_var("CLAUDE_CODE_SESSION_ID");
        found
    }

    #[test]
    fn the_environment_is_enough() {
        // What Claude Code actually sets, on everything the Bash tool spawns.
        assert_eq!(asked(&["show"], Some("c94530b7-4075")), Some("c94530b7-4075".into()));
    }

    #[test]
    fn a_flag_beats_the_environment() {
        // `show.md` passes it explicitly, and a file somebody can read should win over a
        // variable they cannot see.
        assert_eq!(
            asked(&["show", "--in", "from-the-flag"], Some("from-the-env")),
            Some("from-the-flag".into())
        );
        assert_eq!(
            asked(&["show", "--in=from-the-flag"], Some("from-the-env")),
            Some("from-the-flag".into())
        );
    }

    #[test]
    fn a_terminal_belongs_to_no_conversation() {
        // Somebody who typed the binary's name gets the picker, which is the honest answer.
        assert_eq!(asked(&["show"], None), None);
    }

    #[test]
    fn a_placeholder_that_was_never_substituted_is_not_an_id() {
        /*
         * `show.md` is markdown before it is a command line, and `${CLAUDE_SESSION_ID}` is
         * substituted by Claude Code rather than by a shell. Anything that reaches the
         * binary without that substitution having happened arrives looking like an id and
         * is not one — pointing the toolbar at it would select a conversation that cannot
         * exist, and the rail would sit on a receiver nothing can be sent to.
         */
        for never in ["${CLAUDE_SESSION_ID}", "$CLAUDE_SESSION_ID", "{{session}}", "", "   "] {
            assert_eq!(asked(&["show", "--in", never], None), None, "{never}");
        }
    }

    #[test]
    fn a_flag_with_nothing_after_it_falls_back_rather_than_panicking() {
        assert_eq!(asked(&["show", "--in"], Some("from-the-env")), Some("from-the-env".into()));
        assert_eq!(asked(&["show", "--in"], None), None);
    }

    #[test]
    fn hearing_the_same_conversation_twice_is_not_news() {
        // The event it would emit retargets the rail over somebody's choice, so a repeat
        // must be silent — `/colai:show` twice in one chat should not move anything.
        let from = CameFrom::default();
        assert!(from.heard(Some("one".into())));
        assert!(!from.heard(Some("one".into())));
        assert!(from.heard(Some("two".into())));
        // And a launch from outside any conversation must not clear a chat already chosen:
        // running the binary from a terminal is not a statement about where marks go.
        assert!(!from.heard(None));
        assert_eq!(from.read(), Some("two".into()));
    }
}

/*
 * Who else is holding this conversation.
 *
 * Claude Code writes a small record per running session to `~/.claude/sessions/<pid>.json`
 * — the conversation it has open, how it was started, and where to reach it. That registry
 * is how a session lists its peers, and it is the only way to answer the question this
 * file has to ask before it resumes anything: is somebody already in this conversation?
 *
 * It matters because the toolbar does not talk to a running session. It starts its own
 * `claude --resume <id>`, which on the same id as a live chat is a second process reading
 * and appending to one transcript. Both write. Nothing arbitrates. And the visible symptom
 * is not corruption but confusion: a mark sent to "this conversation" is answered by the
 * other process, in the toolbar's Work panel, while the chat the person is actually looking
 * at says nothing at all. Which is exactly what happened, and what this exists to stop.
 *
 * The registry is a private format with no promise attached, so everything here is optional
 * and nothing throws: a record that has changed shape costs one session from the answer,
 * never the answer itself.
 */

/// What Claude Code says is running, asked of Claude Code.
///
/// `claude agents --json` is a documented command and reports interactive sessions and
/// background ones together, each with its pid, its conversation and the name it answers to.
/// That name is the address `SendMessage` takes, which is the whole reason this is needed:
/// a mark is addressed to a conversation and delivered to a name.
///
/// About 150ms, because it is a whole CLI starting up. Called when a mark is sent and not on
/// a timer.
pub(crate) fn as_claude_lists_them() -> Vec<Value> {
    let mut run = std::process::Command::new("claude");
    run.args(["agents", "--json"]).stderr(std::process::Stdio::null());
    // No console window flashing up every time a mark is sent (this is called on send).
    no_console_window(&mut run);
    let Some(said) = run.output().ok() else {
        return Vec::new();
    };
    serde_json::from_slice::<Value>(&said.stdout)
        .ok()
        .and_then(|listed| listed.as_array().cloned())
        .unwrap_or_default()
}

/// The name a live conversation answers to, if it is live.
///
/// `None` covers both "no such conversation" and "not running", and the caller wants the same
/// thing in either case: a mark cannot be handed to a chat nobody is in.
pub(crate) fn what_that_chat_is_called(key: &str) -> Option<String> {
    as_claude_lists_them().into_iter().find_map(|one| {
        (one.get("sessionId").and_then(Value::as_str) == Some(key)
            && one.get("kind").and_then(Value::as_str) == Some("interactive"))
        .then(|| one.get("name").and_then(Value::as_str))
        .flatten()
        .map(str::to_string)
    })
}

/// A Claude Code that is running right now.
#[derive(Debug, Clone)]
pub(crate) struct LiveSession {
    // Set from the registry and printed by the `who_is_running_right_now` probe, which is
    // an ignored test; nothing in a normal run reads it, so it is silenced rather than
    // dropped from the struct that models a live session.
    #[allow(dead_code)]
    pub pid: i32,
    pub session_id: String,
    /// What started it. `cli` is a person at a terminal; `sdk-cli` is one the toolbar ran.
    pub entrypoint: String,
    /// What it calls itself, for saying which chat is in the way.
    pub name: Option<String>,
}

impl LiveSession {
    /// Whether this is somebody's own session rather than one the toolbar started.
    ///
    /// The distinction is `entrypoint`, not `kind` — both say `interactive`, which is what
    /// made this confusing to read the first time. A toolbar child is `sdk-cli`; a person's
    /// terminal is `cli`. Resuming a `cli` session is the collision. Resuming one of our own
    /// is not, because it is ours and we are the only one sending to it.
    pub fn is_somebodys_own(&self) -> bool {
        self.entrypoint == "cli"
    }
}

/// Every session the registry currently claims is running.
pub(crate) fn live_sessions() -> Vec<LiveSession> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(home.join(".claude").join("sessions")) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        // The `.key` files beside these hold a credential. Nothing here reads one: the
        // question is who is running, and the answer is entirely in the `.json`.
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(said) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(said): Result<Value, _> = serde_json::from_str(&said) else {
            continue;
        };
        let (Some(pid), Some(session_id)) = (
            said.get("pid").and_then(Value::as_i64),
            said.get("sessionId").and_then(Value::as_str),
        ) else {
            continue;
        };
        /*
         * A record outlives a crash, so the pid is checked rather than believed — the same
         * reason the plugin checks the toolbar's own pidfile instead of trusting it.
         *
         * Existence only, not identity. The record carries `procStart` for exactly the
         * reuse case, and comparing it would be stricter. It is not done here because the
         * two answers differ only when the system has handed this pid to something else
         * since, and then this says "a chat is open" when none is — which costs a refusal
         * and a sentence, where being wrong the other way costs two agents on one
         * transcript. The cheap mistake is the one to make.
         */
        if !still_running(pid as i32) {
            continue;
        }
        found.push(LiveSession {
            pid: pid as i32,
            session_id: session_id.to_string(),
            entrypoint: said
                .get("entrypoint")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            name: said
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string),
        });
    }
    found
}

/// Whether a pid is a process that exists.
#[cfg(target_os = "linux")]
fn still_running(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    Path::new(&format!("/proc/{pid}")).exists()
}

/// The same question on Windows, where there is no `/proc`: open the process and ask its
/// exit code. A handle we can open whose code is still `STILL_ACTIVE` is a live process;
/// no handle, or an exited one, is not. Without this the toolbar took every conversation to
/// be gone and never recognised one already open in somebody's own Claude Code.
#[cfg(target_os = "windows")]
fn still_running(pid: i32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    if pid <= 0 {
        return false;
    }
    const STILL_ACTIVE: u32 = 259;
    // SAFETY: the handle is opened and closed here and escapes nowhere; the exit code is a
    // plain out-parameter.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid as u32);
        if handle.is_null() {
            return false;
        }
        let mut code: u32 = 0;
        let read = GetExitCodeProcess(handle, &mut code) != 0;
        CloseHandle(handle);
        read && code == STILL_ACTIVE
    }
}

/// And on a Mac, which has no `/proc` either: the oldest question Unix has for it.
///
/// `kill` with signal 0 sends nothing and only checks whether it could have. Success is a
/// live process; `EPERM` is a live process that belongs to somebody else, which is still
/// running and still counts; `ESRCH` — no such process — is the only answer that means
/// gone.
#[cfg(target_os = "macos")]
fn still_running(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 delivers nothing; this is a pure existence check on a pid.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn still_running(pid: i32) -> bool {
    let _ = pid;
    false
}

/// Somebody's own Claude Code holding this conversation, if there is one.
///
/// Our own pid is excluded nowhere because it cannot appear: the toolbar is not a Claude
/// Code session and writes no record. Children we started are excluded by `entrypoint`.
pub(crate) fn already_open_in_a_chat(key: &str) -> Option<LiveSession> {
    live_sessions()
        .into_iter()
        .find(|one| one.session_id == key && one.is_somebodys_own())
}

#[cfg(test)]
mod who_else_is_in_here {
    use super::*;

    /// A registry of our own, since the real one is whatever this machine is doing.
    fn registry(records: &[(&str, i32, &str, &str)]) -> tempdir::Held {
        let home = tempdir::make();
        let dir = home.path().join(".claude").join("sessions");
        fs::create_dir_all(&dir).expect("a sessions directory");
        for (id, pid, entrypoint, name) in records {
            let said = json!({
                "pid": pid, "sessionId": id, "kind": "interactive",
                "entrypoint": entrypoint, "name": name,
            });
            fs::write(dir.join(format!("{pid}.json")), said.to_string()).expect("a record");
        }
        home
    }

    /// This process, which is certainly running, so a record naming it is a live one.
    fn us() -> i32 {
        std::process::id() as i32
    }

    #[test]
    fn a_chat_somebody_is_sitting_in_is_found() {
        let _home = registry(&[("abc", us(), "cli", "the-one-they-are-typing-in")]);
        let held = already_open_in_a_chat("abc").expect("the live chat");
        assert_eq!(held.name.as_deref(), Some("the-one-they-are-typing-in"));
    }

    #[test]
    fn a_child_the_toolbar_started_is_not_in_the_way() {
        /*
         * The distinction that made this hard to read: both kinds say `kind: interactive`,
         * and only `entrypoint` separates them. A toolbar child is `sdk-cli`. Treating one
         * of those as a collision would make the toolbar refuse to talk to its own agent,
         * which is the only thing it can talk to.
         */
        let _home = registry(&[("abc", us(), "sdk-cli", "colai-a7")]);
        assert!(already_open_in_a_chat("abc").is_none());
    }

    #[test]
    fn a_record_left_behind_by_a_crash_is_not_a_running_chat() {
        /*
         * A pid above the kernel's ceiling, because it is the only one that can be written
         * down here and be certain to name nothing. The first attempt used 1 — which is
         * init, exists on every Linux, and made the test fail for the right reason.
         */
        let _home = registry(&[("abc", beyond_any_pid(), "cli", "long-gone")]);
        assert!(already_open_in_a_chat("abc").is_none());
    }

    fn beyond_any_pid() -> i32 {
        fs::read_to_string("/proc/sys/kernel/pid_max")
            .ok()
            .and_then(|said| said.trim().parse::<i32>().ok())
            .and_then(|most| most.checked_add(1))
            .unwrap_or(i32::MAX)
    }

    #[test]
    fn another_conversation_is_not_this_one() {
        let _home = registry(&[("other", us(), "cli", "elsewhere")]);
        assert!(already_open_in_a_chat("abc").is_none());
    }

    #[test]
    fn no_registry_at_all_is_no_collision() {
        let _home = tempdir::make();
        assert!(already_open_in_a_chat("abc").is_none());
    }

    /// A home directory of our own, put back on the way out.
    ///
    /// `HOME` is process-wide, so these run one at a time and each restores what it found.
    mod tempdir {
        use std::path::{Path, PathBuf};
        use std::sync::MutexGuard;

        pub struct Held {
            at: PathBuf,
            was: Option<std::ffi::OsString>,
            _held: MutexGuard<'static, ()>,
        }

        impl Held {
            pub fn path(&self) -> &Path {
                &self.at
            }
        }

        impl Drop for Held {
            fn drop(&mut self) {
                match &self.was {
                    Some(was) => std::env::set_var("HOME", was),
                    None => std::env::remove_var("HOME"),
                }
                let _ = std::fs::remove_dir_all(&self.at);
            }
        }

        pub fn make() -> Held {
            let held = crate::hold_the_environment();
            let at = std::env::temp_dir().join(format!("colai-who-{}", uniquely()));
            std::fs::create_dir_all(&at).expect("a home");
            let was = std::env::var_os("HOME");
            std::env::set_var("HOME", &at);
            Held { at, was, _held: held }
        }

        fn uniquely() -> u128 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or(0)
        }
    }
}

#[cfg(test)]
mod against_this_machine {
    use super::*;

    /// What the registry on this machine actually says, run by hand.
    ///
    /// Ignored by default for the same reason `transcripts` is: it reads the real
    /// `~/.claude/sessions`, so what it finds depends on what is running. Run it when
    /// changing the reader — a registry whose shape has moved reads as "nothing is live",
    /// which is silently the dangerous answer, since the guard then permits every resume.
    ///
    ///     cargo test -- --ignored who_is_running_right_now --nocapture
    #[test]
    #[ignore = "reads ~/.claude/sessions on this machine"]
    fn who_is_running_right_now() {
        let live = live_sessions();
        for one in &live {
            println!(
                "pid {:>7}  {:<8}  {:<34}  {}  {}",
                one.pid,
                one.entrypoint,
                one.name.as_deref().unwrap_or("-"),
                &one.session_id[..8],
                if one.is_somebodys_own() { "← a chat somebody is in" } else { "(ours)" },
            );
        }
        assert!(
            !live.is_empty(),
            "no live session found at all — this test is itself running inside one, so the \
             reader has stopped understanding the registry"
        );
    }
}

#[cfg(test)]
mod what_a_send_costs {
    use super::*;

    /// How long `where_it_is_had` takes on this machine's real transcripts.
    ///
    /// Ignored by default because it reads `~/.claude/projects` and the answer depends on how
    /// much conversation is on the disk. Run it when changing how the cwd is found:
    ///
    ///     cargo test -- --ignored what_one_send_spends_finding_a_directory --nocapture
    #[test]
    #[ignore = "reads ~/.claude/projects on this machine"]
    fn what_one_send_spends_finding_a_directory() {
        let newest = conversations(1).pop().expect("a conversation on this machine");
        let began = std::time::Instant::now();
        let found = where_it_is_had(Some(&newest.session_key));
        let took = began.elapsed();
        println!("  where_it_is_had: {took:?} -> {found:?}");

        // The same answer the list gives, which is where it used to be looked up.
        let listed = conversations(400);
        let row = listed.iter().find(|one| one.session_key == newest.session_key).unwrap();
        assert_eq!(found, row.cwd);

        // The list as the rail asks for it every five seconds: the first call of a run reads
        // every transcript, every call after only what was appended.
        let began = std::time::Instant::now();
        let listed = conversations_under(
            &PathBuf::from(std::env::var_os("HOME").unwrap()).join(".claude").join("projects"),
            400,
            &mut HashMap::new(),
        );
        println!("  conversations(400), nothing remembered: {:?} for {} rows", began.elapsed(), listed.len());
        for _ in 0..3 {
            let began = std::time::Instant::now();
            let again = conversations(400);
            println!("  conversations(400), remembered: {:?} for {} rows", began.elapsed(), again.len());
        }
    }
}

#[cfg(test)]
mod remembering_the_list {
    use super::*;

    /// A folder laid out as `~/.claude/projects` is, removed when the test is done with it.
    struct Projects(PathBuf);

    impl Projects {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("colai-scan-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("a-project")).unwrap();
            Projects(root)
        }

        fn transcript(&self, key: &str) -> PathBuf {
            self.0.join("a-project").join(format!("{key}.jsonl"))
        }
    }

    impl Drop for Projects {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn append(path: &Path, text: &str) {
        let mut file = fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }

    fn line(entry: Value) -> String {
        format!("{entry}\n")
    }

    /// What a scan with nothing remembered says, which is what the old reader said.
    fn fresh(projects: &Projects) -> Vec<Conversation> {
        conversations_under(&projects.0, 400, &mut HashMap::new())
    }

    #[test]
    fn a_remembering_scan_says_what_a_fresh_one_does() {
        let projects = Projects::new("grow");
        let mut held = HashMap::new();
        let one = projects.transcript("one");
        let two = projects.transcript("two");

        append(&one, &line(json!({"type": "user", "cwd": "/work/first", "message": {"content": "hi"}})));
        append(&two, &line(json!({"type": "summary"})));
        let seen = conversations_under(&projects.0, 400, &mut held);
        assert_eq!(seen, fresh(&projects));
        assert_eq!(seen.len(), 2);

        // Appended to: a title, a later cwd that must not win, a prompt, a cost written as
        // a string — and a last line still being written, with no newline yet.
        append(&one, &line(json!({"type": "ai-title", "aiTitle": "Fixing the rail"})));
        append(&one, &line(json!({"type": "user", "cwd": "/work/second"})));
        append(&one, &line(json!({"type": "last-prompt", "lastPrompt": "and the menu"})));
        append(&one, &line(json!({"type": "cost-state", "totalCostUSD": "0.25"})));
        append(&one, "{\"type\":\"cost-state\",\"totalCostUSD\":0.5}");
        append(&two, &line(json!({"type": "last-prompt", "lastPrompt": "only asked"})));
        let seen = conversations_under(&projects.0, 400, &mut held);
        assert_eq!(seen, fresh(&projects));
        let first = seen.iter().find(|one| one.session_key == "one").unwrap();
        assert_eq!(first.name, "Fixing the rail");
        assert_eq!(first.cwd.as_deref(), Some("/work/first"));
        assert_eq!(first.last_prompt.as_deref(), Some("and the menu"));
        assert_eq!(first.spent, 0.5, "an unfinished last line is read, as a full read would");
        let second = seen.iter().find(|one| one.session_key == "two").unwrap();
        assert_eq!(second.name, "only asked");
        assert_eq!(second.cwd, None);

        // The unfinished line finishes, split across two writes, and a newer cost follows.
        append(&one, "\n");
        append(&one, &line(json!({"type": "cost-state", "totalCostUSD": 0.75})));
        let seen = conversations_under(&projects.0, 400, &mut held);
        assert_eq!(seen, fresh(&projects));
        assert_eq!(seen.iter().find(|one| one.session_key == "one").unwrap().spent, 0.75);

        // Nothing changed: the same answer again, from memory.
        assert_eq!(conversations_under(&projects.0, 400, &mut held), seen);
    }

    #[test]
    fn a_cut_or_replaced_transcript_is_read_again() {
        let projects = Projects::new("cut");
        let mut held = HashMap::new();
        let one = projects.transcript("one");
        append(&one, &line(json!({"type": "user", "cwd": "/work/before"})));
        append(&one, &line(json!({"type": "ai-title", "aiTitle": "The long version of it"})));
        conversations_under(&projects.0, 400, &mut held);

        // Cut shorter, to something else entirely.
        fs::write(&one, line(json!({"type": "user", "cwd": "/work/after"}))).unwrap();
        let seen = conversations_under(&projects.0, 400, &mut held);
        assert_eq!(seen, fresh(&projects));
        assert_eq!(seen[0].cwd.as_deref(), Some("/work/after"));
        assert_eq!(seen[0].name, "Untitled");

        // Replaced by a longer file with another beginning: grown, but not the file that was read.
        fs::write(
            &one,
            line(json!({"type": "user", "cwd": "/work/elsewhere", "pad": "x".repeat(300)}))
                + &line(json!({"type": "ai-title", "aiTitle": "Another conversation"})),
        )
        .unwrap();
        let seen = conversations_under(&projects.0, 400, &mut held);
        assert_eq!(seen, fresh(&projects));
        assert_eq!(seen[0].cwd.as_deref(), Some("/work/elsewhere"));
        assert_eq!(seen[0].name, "Another conversation");

        // Removed: gone from the list, and from memory.
        fs::remove_file(&one).unwrap();
        assert!(conversations_under(&projects.0, 400, &mut held).is_empty());
        assert!(held.is_empty());
    }
}
