// The Claude Code session the toolbar is pointed at.
//
// This is what `gateway_ws.rs` was, and it is a twentieth of the size. The Gateway needed a
// WebSocket, a protocol, an ed25519 device identity, a TLS pin and a credential bootstrap
// because it was a service on a socket that had to be told who was calling. Claude Code is
// a program on this machine, and the toolbar runs it — so the trust boundary is process
// ancestry and there is nothing to authenticate.
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
        Self {
            claude,
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
    ) -> Result<(), String> {
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

        let frame = json!({
            "type": "user",
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
        live.saying
            .flush()
            .map_err(|trouble| format!("could not reach claude: {trouble}"))
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
        ]);
        if let Some(key) = &key {
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
            .stderr(Stdio::null())
            .spawn()
            .map_err(|trouble| format!("could not start claude: {trouble}"))?;

        let saying = child.stdin.take().ok_or("claude took no input")?;
        let hearing = child.stdout.take().ok_or("claude said nothing")?;
        let app = app.clone();
        std::thread::spawn(move || listen(app, hearing));

        Ok(Talking { child, saying, key })
    }
}

impl Talking {
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
    let mut spent = 0.0_f64;
    for line in BufReader::new(hearing).lines() {
        let Ok(line) = line else { break };
        let Ok(frame) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let kind = frame.get("type").and_then(Value::as_str).unwrap_or_default();
        let session = frame
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_string);

        if kind == "assistant" {
            let Some(message) = frame.get("message") else {
                continue;
            };
            // Forwarded whole. `spokenBy` in `toolbar-answers.js` reads `{role:
            // "assistant", content: [{type: "text"}]}`, which is the shape this already
            // is — re-packing it would be a second format for the same thing.
            let _ = app.emit_to(
                crate::colai::OVERLAY_LABEL,
                REPLY_EVENT,
                json!({"sessionKey": session, "message": message}),
            );
            // And what it has just picked up, for the pill on the rail. Only the start of
            // a call: a result names something that has already stopped happening.
            if let Some(content) = message.get("content").and_then(Value::as_array) {
                for block in content {
                    if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                        let _ = app.emit_to(
                            crate::colai::OVERLAY_LABEL,
                            DOING_EVENT,
                            json!({
                                "name": block.get("name"),
                                "args": block.get("input").cloned().unwrap_or(json!({})),
                                "sessionKey": session,
                            }),
                        );
                    }
                }
            }
            continue;
        }

        if kind == "result" {
            spent += frame
                .get("total_cost_usd")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            // Said out loud, because on this host every mark is a metered call against the
            // user's own account. A tool that spends somebody's money quietly is one they
            // are right to distrust.
            let _ = app.emit_to(
                crate::colai::OVERLAY_LABEL,
                SPENT_EVENT,
                json!({
                    "sessionKey": session,
                    "cost": frame.get("total_cost_usd").and_then(Value::as_f64).unwrap_or(0.0),
                    "spent": spent,
                    "outcome": frame.get("subtype"),
                }),
            );
        }
    }
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
    use super::{briefly, cost_in};
    use serde_json::json;

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
