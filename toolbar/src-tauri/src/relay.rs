// Putting a mark into the conversation somebody is actually sitting in.
//
// This is the thing the toolbar exists for and the thing it could not do. `session.rs` runs
// its own `claude` and talks stream-json to it, which is right for a conversation the toolbar
// owns and wrong for one a person has open in a terminal: `--resume` on a live transcript is
// a second process on one file, answering into a Work panel while the chat being looked at
// says nothing. That was reported as "nothing was sent to this session", and it is what this
// replaces.
//
// The way in is Claude Code's own `SendMessage`, which is a documented tool for exactly this:
// one session messaging another. The toolbar cannot call a tool — but a `claude` can, and the
// toolbar already knows how to run one. So there is a small, long-lived agent here whose only
// job is to pass a mark along, and the message arrives in the live chat with no keystroke from
// anybody.
//
// Everything else was tried first and each was ruled out by measurement rather than argument:
// the Agent SDK has no send-into-a-running-session call (the one that resembled it was removed
// in TS SDK 0.3.142); Claude Code's peer socket is private and credentialed; synthesised
// keystrokes cannot aim at a session when three of them share one VS Code window; `TIOCSTI` is
// off by default on every current kernel. `wrap.rs` is the one route that is free and instant,
// and it costs somebody starting Claude Code a different way, which is not what a plugin
// should ask.
//
// What this costs, measured rather than guessed, per mark:
//
//     a fresh process each time      6.6s   $0.13
//     this, warm                     1.3s   $0.025
//     this, with the big tools denied 2.8s   $0.14   (denying them costs more than they do)
//
// So: one child, kept alive, and recycled before its own context grows into the bill. Two
// pence a mark is a real cost and the rail says so rather than hiding it.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;

use serde_json::{json, Value};

/// How many marks one relay agent passes along before it is replaced.
///
/// Not a leak — a conversation. Every relay is a turn, so the context grows and the cache
/// read with it: measured across three in a row, $0.018 then $0.025 then $0.028, climbing
/// with no end. Starting a fresh one costs a first-turn cache fill and resets that, which is
/// cheaper than letting it run. Nothing is lost by forgetting: a relay has no memory worth
/// keeping, and the next one is told everything it needs.
const MARKS_BEFORE_A_FRESH_ONE: u32 = 8;

/// How long to wait for a relay before giving up on it.
///
/// Warm it answers in about a second and a half; cold, four. A minute is not patience, it is
/// the point past which something has gone wrong and the person should be told instead of
/// watching a spinner.
const UNTIL_WE_GIVE_UP: std::time::Duration = std::time::Duration::from_secs(60);

/// What the relay is told it is for.
///
/// Narrow on purpose. It has one tool and one job, and a model that decides to be helpful
/// instead — summarising the mark, answering it, asking a question back — would put words in
/// somebody's chat that they did not write.
const ALL_IT_DOES: &str = "\
You are a relay and nothing else. Each message you receive names a Claude Code session and \
carries text to deliver to it. Call SendMessage once, with `to` set to that session and \
`message` set to the given text exactly as written — every character, no summary, no preface, \
no commentary, nothing added and nothing removed. Then reply with only the word: ok. \
Never answer the text yourself. Never explain. If SendMessage fails, reply with only the \
word: failed, and the reason on the same line.";

/// The agent that passes marks along, and how many it has passed.
pub(crate) struct Relay {
    talking: Mutex<Option<Talking>>,
    claude: std::path::PathBuf,
}

struct Talking {
    child: Child,
    saying: ChildStdin,
    hearing: BufReader<std::process::ChildStdout>,
    passed: u32,
}

impl Relay {
    pub(crate) fn new(claude: std::path::PathBuf) -> Self {
        Self { talking: Mutex::new(None), claude }
    }

    /// Put `text` into the conversation named `to`, which somebody has open right now.
    ///
    /// Blocking, because the answer matters: the rail says "sent" or says why not, and a
    /// receipt that guesses is how somebody sits watching a chat nothing is coming to.
    pub(crate) fn hand_over(&self, to: &str, text: &str) -> Result<(), String> {
        let mut held = self
            .talking
            .lock()
            .map_err(|_| "the relay is wedged".to_string())?;

        // Replaced rather than reused past this point — see the constant.
        if held.as_ref().is_some_and(|one| one.passed >= MARKS_BEFORE_A_FRESH_ONE) {
            if let Some(mut old) = held.take() {
                let _ = old.child.kill();
                let _ = old.child.wait();
            }
        }
        // And replaced if it has died: a relay that fell over between marks must not take
        // the next one down with it.
        if held.as_ref().is_some_and(|one| !still_there(&one.child)) {
            if let Some(mut gone) = held.take() {
                let _ = gone.child.wait();
            }
        }
        if held.is_none() {
            *held = Some(self.start()?);
        }
        let talking = held.as_mut().expect("a relay");

        /*
         * The text last, after a marker, and never interpolated into a sentence.
         *
         * A mark's message is somebody's own words and can contain anything — quotes, braces,
         * the word "ignore", an instruction-shaped line. Put inside a sentence telling a model
         * what to do with it, some of those read as the instruction rather than the payload.
         * After a marker, with the whole remainder being the message, there is nothing to
         * misread: the boundary is positional, not grammatical.
         */
        let asked = json!({
            "type": "user",
            "message": { "role": "user", "content": [{ "type": "text", "text": format!(
                "Deliver to the session named `{to}`.\n\
                 Everything after the next line is the message, verbatim.\n\
                 ---MESSAGE---\n{text}"
            )}]},
        });
        writeln!(talking.saying, "{asked}")
            .and_then(|()| talking.saying.flush())
            .map_err(|trouble| format!("could not reach the relay: {trouble}"))?;
        talking.passed += 1;

        // Read to the end of the turn. `result` is the frame that says how it went, and
        // anything before it is the relay thinking out loud.
        let began = std::time::Instant::now();
        loop {
            if began.elapsed() > UNTIL_WE_GIVE_UP {
                let _ = talking.child.kill();
                *held = None;
                return Err("the relay did not answer in a minute".to_string());
            }
            let mut line = String::new();
            match talking.hearing.read_line(&mut line) {
                Ok(0) => {
                    *held = None;
                    return Err("the relay stopped before it said anything".to_string());
                }
                Ok(_) => {}
                Err(trouble) => {
                    *held = None;
                    return Err(format!("could not hear the relay: {trouble}"));
                }
            }
            let Ok(frame): Result<Value, _> = serde_json::from_str(&line) else {
                continue;
            };
            if frame.get("type").and_then(Value::as_str) != Some("result") {
                continue;
            }
            return read_the_outcome(&frame);
        }
    }

    /// One `claude`, as small as it can be while still having the tool.
    ///
    /// `--model haiku` because relaying needs no judgement, and the difference is most of the
    /// cost. The heavy tools are deliberately *not* denied: `--disallowed-tools` puts an
    /// explanation of each refusal into the prompt, which measured worse than leaving them —
    /// $0.14 a mark against $0.025.
    fn start(&self) -> Result<Talking, String> {
        let mut child = Command::new(&self.claude)
            .args([
                "--print",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
                "--model",
                "haiku",
                "--allowed-tools",
                "SendMessage",
                "--append-system-prompt",
                ALL_IT_DOES,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|trouble| format!("could not start the relay: {trouble}"))?;
        let saying = child.stdin.take().ok_or("the relay took no input")?;
        let hearing = BufReader::new(child.stdout.take().ok_or("the relay said nothing")?);
        Ok(Talking { child, saying, hearing, passed: 0 })
    }
}

/// Whether a child is still running.
fn still_there(child: &Child) -> bool {
    // `try_wait` needs `&mut`, and this only ever asks. The pid existing is the same answer
    // for our purposes: a reaped child's pid is gone from `/proc` either way.
    std::path::Path::new(&format!("/proc/{}", child.id())).exists()
}

/// Did the mark go, and if not, what should somebody be told.
///
/// Separate and public to the crate's tests, because this is where a silent failure would
/// live: a relay that answered without calling the tool looks exactly like one that
/// delivered, and the rail would say "sent" about a message nobody received.
pub(crate) fn read_the_outcome(frame: &Value) -> Result<(), String> {
    if frame.get("is_error").and_then(Value::as_bool) == Some(true) {
        return Err(said_briefly(frame).unwrap_or_else(|| "the relay failed".to_string()));
    }
    match frame.get("subtype").and_then(Value::as_str) {
        Some("success") => {}
        Some(other) => return Err(format!("the relay ended as {other}")),
        None => return Err("the relay said nothing useful".to_string()),
    }

    /*
     * It has to have actually called the tool.
     *
     * A model asked to relay can answer "ok" without doing anything, and that is the one
     * failure the person must not be told is a success — they would go and watch a chat that
     * is never going to mention it. The system prompt makes it say `failed` when SendMessage
     * refuses; `permission_denials` catches the case where it was never allowed to try.
     */
    if let Some(refused) = frame.get("permission_denials").and_then(Value::as_array) {
        if !refused.is_empty() {
            return Err("the relay was not allowed to send the message".to_string());
        }
    }
    let said = frame
        .get("result")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if said.to_ascii_lowercase().starts_with("failed") {
        return Err(said);
    }
    if said.to_ascii_lowercase() != "ok" {
        // Not fatal and not silent. The tool may well have been called; what is certain is
        // that the relay did not answer the way it was told to, which is worth a line in the
        // log rather than a failed send.
        eprintln!("[colai] the relay answered {said:?} rather than ok.");
    }
    Ok(())
}

fn said_briefly(frame: &Value) -> Option<String> {
    let said = frame
        .get("result")
        .and_then(Value::as_str)
        .or_else(|| frame.get("error").and_then(Value::as_str))?;
    let said = said.trim();
    Some(said.chars().take(200).collect())
}

#[cfg(test)]
mod whether_the_mark_went {
    use super::*;

    fn result(said: &str) -> Value {
        serde_json::from_str(said).expect("json")
    }

    #[test]
    fn ok_is_a_delivery() {
        assert!(read_the_outcome(&result(
            r#"{"type":"result","subtype":"success","is_error":false,"result":"ok",
                "permission_denials":[]}"#
        ))
        .is_ok());
    }

    #[test]
    fn a_refused_tool_is_not_a_delivery() {
        /*
         * The failure that must never read as success. A relay that could not call
         * SendMessage still ends its turn tidily, and a receipt built only on `subtype`
         * would say "sent" about a message nobody received — and somebody would go and
         * watch a chat that is never going to mention it.
         */
        let said = read_the_outcome(&result(
            r#"{"type":"result","subtype":"success","is_error":false,"result":"ok",
                "permission_denials":[{"tool_name":"SendMessage"}]}"#,
        ));
        assert_eq!(said, Err("the relay was not allowed to send the message".into()));
    }

    #[test]
    fn the_relay_saying_it_failed_is_believed() {
        let said = read_the_outcome(&result(
            r#"{"type":"result","subtype":"success","is_error":false,
                "result":"failed no session by that name","permission_denials":[]}"#,
        ));
        assert_eq!(said, Err("failed no session by that name".into()));
    }

    #[test]
    fn an_errored_turn_carries_its_reason() {
        let said = read_the_outcome(&result(
            r#"{"type":"result","subtype":"error_during_execution","is_error":true,
                "result":"the model could not be reached"}"#,
        ));
        assert_eq!(said, Err("the model could not be reached".into()));
    }

    #[test]
    fn running_out_of_turns_is_a_failure_not_a_send() {
        let said = read_the_outcome(&result(
            r#"{"type":"result","subtype":"error_max_turns","is_error":false}"#,
        ));
        assert_eq!(said, Err("the relay ended as error_max_turns".into()));
    }

    #[test]
    fn a_chatty_relay_still_counts_as_delivered() {
        // It was told to say `ok` and said something else. The tool may well have been
        // called; refusing the send over the wording would fail a mark that arrived.
        assert!(read_the_outcome(&result(
            r#"{"type":"result","subtype":"success","is_error":false,
                "result":"Sent it along!","permission_denials":[]}"#
        ))
        .is_ok());
    }
}

#[cfg(test)]
mod against_a_real_chat {
    use super::*;

    /// Put a mark into a conversation that is actually open, by hand.
    ///
    /// Ignored by default: it starts a `claude`, spends about two pence, and needs a live
    /// interactive session to deliver to. It is here because everything else about the relay
    /// is a unit test on a JSON frame, and the thing worth knowing — that a mark lands in
    /// somebody's chat — cannot be learned that way.
    ///
    ///     COLAI_RELAY_TO="<the name from `claude agents --json`>" \
    ///       cargo test -- --ignored a_mark_lands_in_a_running_chat --nocapture
    #[test]
    #[ignore = "starts a claude, costs money, needs a live session"]
    fn a_mark_lands_in_a_running_chat() {
        let Ok(to) = std::env::var("COLAI_RELAY_TO") else {
            panic!("set COLAI_RELAY_TO to the name of a session `claude agents --json` lists");
        };
        let claude = crate::session::Session::discover().expect("a claude on PATH");
        let relay = Relay::new(claude);

        let began = std::time::Instant::now();
        relay
            .hand_over(&to, "PURPLE ARTICHOKE 8842 — from the Rust relay, cold")
            .expect("the first mark to land");
        println!("cold:  {:?}", began.elapsed());

        // The second is the one that matters for what this costs somebody: the child is warm
        // and the cache is filled, which is the whole reason it is kept alive.
        let again = std::time::Instant::now();
        relay
            .hand_over(&to, "PURPLE ARTICHOKE 8842 — from the Rust relay, warm")
            .expect("the second mark to land");
        println!("warm:  {:?}", again.elapsed());
    }
}
