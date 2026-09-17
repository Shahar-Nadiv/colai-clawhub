// What `/` should offer, asked of Claude Code before anybody presses it.
//
// The rail's completion menu is driven by `state.commands`, which is filled from the
// `system/init` frame of a `claude` the toolbar started. That worked while every mark ran
// through the toolbar's own agent. It stopped working when marks began going straight into
// live conversations through `relay.rs`, because then the session child may never start at
// all — so the list stayed empty and `/` fell back to offering colai's four modes.
//
// Four, where Claude Code on this machine has a hundred and one. Reported as exactly that:
// "the / sign allows only 4 modes, where in claude chat there are a lot more options".
//
// The awkward part is that `init` does not arrive until the first message is sent. A child
// started and left alone emits its hook frames and then blocks on stdin forever; the
// session preamble is part of a turn, not of starting up. So this sends one, and then does
// not let the turn happen:
//
//     init after 0.92s with 101 commands — killing now
//     new transcripts written: 0
//
// Measured. The preamble is written before the model is called, so reading it and killing
// the child costs no tokens, completes no request and leaves nothing on disk. The list is
// free; it just has to be asked for rather than waited for.
//
// Cached against `claude --version`, so this happens once per Claude Code rather than once
// per launch, and refreshes itself the day Claude Code updates and the list changes.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

/// What the page hears when the list is known.
pub(crate) const COMMANDS_EVENT: &str = "colai:commands";

/// How long to wait for the preamble before giving up on it.
///
/// It takes about a second. Ten is the point past which something is wrong — a `claude` that
/// wants a login, a machine under load — and the right answer then is the four modes and no
/// fuss, rather than a toolbar that will not open.
const UNTIL_IT_SAYS: std::time::Duration = std::time::Duration::from_secs(10);

/// The commands Claude Code offers, and the ones it says belong to a terminal.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WhatSlashOffers {
    pub all: Vec<String>,
    /// Commands that only mean something in a terminal. The rail must not offer these: it
    /// is not a terminal, and pressing one here would do nothing.
    pub terminal_only: Vec<String>,
}

/// Ask Claude Code what `/` should offer, and tell the page.
///
/// Spawned rather than awaited, because this costs a second and nothing on screen depends on
/// it: the menu works from the moment the list arrives, and before that `/` still offers the
/// modes. A toolbar that waited a second longer to appear in order to know its own menu would
/// be paying the wrong thing for it.
pub(crate) fn learn_what_slash_offers(app: &AppHandle, claude: PathBuf) {
    let app = app.clone();
    std::thread::spawn(move || {
        let found = match remembered(&claude) {
            Some(known) => known,
            None => match ask_claude(&claude) {
                Some(asked) => {
                    remember(&claude, &asked);
                    asked
                }
                None => {
                    eprintln!("[colai] could not read the command list; `/` will offer the modes.");
                    return;
                }
            },
        };
        eprintln!("[colai] `/` offers {} of Claude Code's commands.", found.all.len());
        let said = json!({ "slashCommands": found.all, "terminalOnly": found.terminal_only });
        if let Err(trouble) = app.emit_to(crate::colai::OVERLAY_LABEL, COMMANDS_EVENT, said) {
            eprintln!("[colai] could not hand the command list to the page: {trouble}");
        }
    });
}

/// Start a `claude`, send it one message, read the preamble, and stop it before the turn.
///
/// The message is a single character and is never answered. What is wanted is the frame that
/// comes before the answer.
fn ask_claude(claude: &Path) -> Option<WhatSlashOffers> {
    use std::io::{BufRead, BufReader, Write};

    let mut child = Command::new(claude)
        .args([
            "--print",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let mut saying = child.stdin.take()?;
    let asked = json!({
        "type": "user",
        "message": { "role": "user", "content": [{ "type": "text", "text": "x" }]},
    });
    let _ = writeln!(saying, "{asked}").and_then(|()| saying.flush());

    let mut hearing = BufReader::new(child.stdout.take()?);
    let began = std::time::Instant::now();
    let mut found = None;
    while began.elapsed() < UNTIL_IT_SAYS {
        let mut line = String::new();
        match hearing.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let Ok(frame): Result<Value, _> = serde_json::from_str(&line) else {
            continue;
        };
        let kind = frame.get("type").and_then(Value::as_str);
        if kind == Some("system") && frame.get("subtype").and_then(Value::as_str) == Some("init") {
            found = Some(WhatSlashOffers {
                all: named(frame.get("slash_commands")),
                terminal_only: named(frame.get("terminal_slash_commands")),
            });
            break;
        }
        /*
         * If the model has started speaking, the preamble is behind us and the turn is
         * happening — which is the one thing this is meant to avoid. Stop reading and stop
         * the child; the modes are a fine answer and a spent turn is not.
         */
        if kind == Some("assistant") || kind == Some("result") {
            break;
        }
    }

    // Before the turn completes, which is what makes this free.
    let _ = child.kill();
    let _ = child.wait();
    found.filter(|found| !found.all.is_empty())
}

fn named(said: Option<&Value>) -> Vec<String> {
    said.and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Where the answer is kept, and what it is keyed on.
///
/// The version, because the list changes when Claude Code does — a plugin installed, a
/// command added — and a cache that outlived the thing it described would have `/` offering
/// commands that no longer exist, which is worse than offering too few.
fn kept_at(claude: &Path) -> Option<PathBuf> {
    let version = Command::new(claude)
        .arg("--version")
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let version: String = String::from_utf8_lossy(&version.stdout)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.')
        .take(40)
        .collect();
    if version.is_empty() {
        return None;
    }
    let base = match std::env::var_os("XDG_CACHE_HOME") {
        Some(said) if !said.is_empty() => PathBuf::from(said),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".cache"),
    };
    Some(base.join("colai").join(format!("slash-{version}.json")))
}

fn remembered(claude: &Path) -> Option<WhatSlashOffers> {
    let said = std::fs::read_to_string(kept_at(claude)?).ok()?;
    let said: Value = serde_json::from_str(&said).ok()?;
    let found = WhatSlashOffers {
        all: named(said.get("all")),
        terminal_only: named(said.get("terminalOnly")),
    };
    (!found.all.is_empty()).then_some(found)
}

fn remember(claude: &Path, found: &WhatSlashOffers) {
    let Some(at) = kept_at(claude) else {
        return;
    };
    if let Some(within) = at.parent() {
        let _ = std::fs::create_dir_all(within);
    }
    let said = json!({ "all": found.all, "terminalOnly": found.terminal_only });
    let _ = std::fs::write(at, said.to_string());
}

#[cfg(test)]
mod what_slash_offers {
    use super::*;

    #[test]
    fn a_list_survives_being_written_and_read_back() {
        let held = crate::hold_the_environment();
        let home = std::env::temp_dir().join(format!("colai-slash-{}", std::process::id()));
        std::fs::create_dir_all(&home).expect("a cache");
        std::env::remove_var("XDG_CACHE_HOME");
        std::env::set_var("HOME", &home);

        // `claude --version` has to run for the key, so this uses the real one if present.
        let claude = crate::session::Session::discover();
        if let Some(claude) = claude {
            let found = WhatSlashOffers {
                all: vec!["review".into(), "commit".into()],
                terminal_only: vec!["doctor".into()],
            };
            remember(&claude, &found);
            assert_eq!(remembered(&claude), Some(found));
        }
        let _ = std::fs::remove_dir_all(&home);
        drop(held);
    }

    #[test]
    fn an_empty_answer_is_not_remembered_as_an_answer() {
        /*
         * A `claude` that could not start, or one interrupted before the preamble, gives
         * nothing — and caching nothing would mean `/` offered the four modes for the life
         * of that Claude Code version, with no way to notice. Better to ask again next time.
         */
        let held = crate::hold_the_environment();
        let home = std::env::temp_dir().join(format!("colai-slash-empty-{}", std::process::id()));
        std::fs::create_dir_all(&home).expect("a cache");
        std::env::remove_var("XDG_CACHE_HOME");
        std::env::set_var("HOME", &home);
        if let Some(claude) = crate::session::Session::discover() {
            remember(&claude, &WhatSlashOffers { all: vec![], terminal_only: vec![] });
            assert_eq!(remembered(&claude), None);
        }
        let _ = std::fs::remove_dir_all(&home);
        drop(held);
    }

    #[test]
    fn the_terminals_own_commands_are_kept_apart() {
        // The rail is not a terminal. `commandsMatching` in the page drops these, and it can
        // only do that if they arrive as their own list rather than mixed in.
        let said = json!({
            "slash_commands": ["review", "commit", "doctor"],
            "terminal_slash_commands": ["doctor"],
        });
        assert_eq!(named(said.get("slash_commands")).len(), 3);
        assert_eq!(named(said.get("terminal_slash_commands")), vec!["doctor".to_string()]);
    }

    #[test]
    fn nothing_where_a_list_was_expected_is_an_empty_list_rather_than_a_panic() {
        // The frame is a private shape with no promise attached; a field that changes name
        // should cost the menu, not the toolbar.
        assert!(named(None).is_empty());
        assert!(named(Some(&json!("not a list"))).is_empty());
        assert!(named(Some(&json!([1, 2, 3]))).is_empty());
    }
}
