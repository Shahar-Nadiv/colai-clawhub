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

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

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

/// One conversation on this machine, for the rail to choose between.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Conversation {
    pub session_key: String,
    pub name: String,
    pub cwd: Option<String>,
    /// Milliseconds since the epoch, from the transcript's own mtime.
    pub at: u64,
    /// What it has cost so far, which Claude Code records in the transcript itself.
    pub spent: f64,
}

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
    spent: Mutex<f64>,
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
        let path = std::env::var("PATH").ok()?;
        for dir in path.split(':').filter(|dir| !dir.is_empty()) {
            let here = Path::new(dir).join("claude");
            if here.exists() {
                // Resolved, because what is on PATH is usually a shim.
                return Some(fs::canonicalize(&here).unwrap_or(here));
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
            spent: Mutex::new(0.0),
        }
    }

    /// What this conversation has cost since the toolbar started talking to it.
    pub fn spent(&self) -> f64 {
        *self.spent.lock().expect("spent")
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
            *self.spent.lock().expect("spent") = 0.0;
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
            run.args(["--resume", key]);
        }
        if let Some(cwd) = cwd.as_deref().filter(|cwd| Path::new(cwd).is_dir()) {
            // Where the conversation was had. It decides which files the agent can reach,
            // so resuming somewhere else would quietly change what the answer is about.
            run.current_dir(cwd);
        }
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

/// Everything `claude` says, turned into the events the page already draws.
fn listen(app: AppHandle, hearing: std::process::ChildStdout) {
    for line in BufReader::new(hearing).lines() {
        let Ok(line) = line else { break };
        let Ok(frame) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        for (name, body) in what_it_said(&frame) {
            let _ = app.emit_to(crate::colai::OVERLAY_LABEL, name, body);
        }
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
    let Ok(dirs) = fs::read_dir(&projects) else {
        return Vec::new();
    };

    let mut found: Vec<Conversation> = Vec::new();
    for dir in dirs.flatten() {
        let Ok(files) = fs::read_dir(dir.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Some(one) = read_conversation(&path) {
                found.push(one);
            }
        }
    }
    found.sort_by(|a, b| b.at.cmp(&a.at));
    found.truncate(limit);
    found
}

fn read_conversation(path: &Path) -> Option<Conversation> {
    let key = path.file_stem()?.to_str()?.to_string();
    let at = fs::metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|when| when.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0);

    let file = fs::File::open(path).ok()?;
    let mut named: HashMap<&str, String> = HashMap::new();
    let mut cwd = None;
    let mut spent = 0.0;

    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if cwd.is_none() {
            cwd = entry
                .get("cwd")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        match entry.get("type").and_then(Value::as_str) {
            // The generated title, which is what the session list in Claude Code shows.
            Some("ai-title") => {
                if let Some(said) = entry.get("aiTitle").and_then(Value::as_str) {
                    named.insert("title", said.to_string());
                }
            }
            // What they last asked, which beats "Untitled" for a conversation too young
            // to have been given a name.
            Some("last-prompt") => {
                if let Some(said) = entry.get("lastPrompt").and_then(Value::as_str) {
                    named.insert("prompt", said.to_string());
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
                    spent = said;
                }
            }
            _ => {}
        }
    }

    let name = named
        .remove("title")
        .or_else(|| named.remove("prompt"))
        .map(|said| briefly(&said))
        .unwrap_or_else(|| "Untitled".to_string());

    Some(Conversation {
        session_key: key,
        name,
        cwd,
        at,
        spent,
    })
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
pub(crate) fn where_it_is_had(key: Option<&str>) -> Option<String> {
    let key = key?;
    conversations(400)
        .into_iter()
        .find(|one| one.session_key == key)
        .and_then(|one| one.cwd)
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
    let Some(path) = transcript_of(key) else {
        return Vec::new();
    };
    let Ok(file) = fs::File::open(&path) else {
        return Vec::new();
    };
    let mut said = Vec::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let mine = match entry.get("type").and_then(Value::as_str) {
            Some("user") => true,
            Some("assistant") => false,
            _ => continue,
        };
        let Some(message) = entry.get("message") else {
            continue;
        };
        let words = words_in(message.get("content"));
        if words.trim().is_empty() {
            continue;
        }
        let id = entry
            .get("uuid")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        said.push((id, words, mine, None));
    }
    // Newest kept when there are more than the panel asked for, oldest first on the way
    // out — the order it was said in.
    if said.len() > most {
        said.drain(..said.len() - most);
    }
    said
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

#[cfg(test)]
mod tests {
    use super::{briefly, cost_in, permission_asked};
    use serde_json::json;

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
    let Some(said) = std::process::Command::new("claude")
        .args(["agents", "--json"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
    else {
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
fn still_running(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    Path::new(&format!("/proc/{pid}")).exists()
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
        use std::sync::{Mutex, MutexGuard};

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

        let began = std::time::Instant::now();
        let listed = conversations(400);
        println!("  conversations(400): {:?} for {} rows", began.elapsed(), listed.len());
    }
}
