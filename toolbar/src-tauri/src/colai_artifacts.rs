//! The person's artifacts — Design, Docs, Slides, Dashboard — for the rail's artifact keys.
//!
//! Only a session signed in to claude.ai can list them, and the toolbar has no such sign-in of its
//! own: a `claude` it starts signs in separately, and on a desktop-app machine that sign-in lapses.
//! The bridge (hooks/bridge.ts) runs inside the person's own session, so the question is put to
//! it: a file under that session's outbox, `.ask/<id>.json`, answered beside it by the Artifact
//! tool's own listing. See `answerAsks` in the bridge.

use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The core artifact types, exactly as Claude names them.
/// `All` is every artifact, typed or not.
const KINDS: &[&str] = &["Design", "Docs", "Slides", "Dashboard", "All"];

/// How long to wait for the bridge. It looks every second and a half; the rest is the listing.
const ANSWER_WITHIN: Duration = Duration::from_secs(12);

/// List the person's artifacts of one type, as the Artifact tool words the list.
///
/// `session` is the chat to ask through — the one receiving, usually; any session whose bridge is
/// listening will do, because the list is the person's, not the chat's.
#[tauri::command]
pub(crate) async fn colai_artifacts(kind: String, session: Option<String>) -> Result<String, String> {
    if !KINDS.contains(&kind.as_str()) {
        return Err(format!("not an artifact type colai lists: {kind}"));
    }
    tauri::async_runtime::spawn_blocking(move || ask(&kind, session.as_deref()))
        .await
        .map_err(|trouble| format!("asking for the artifacts failed: {trouble}"))?
}

fn ask(kind: &str, session: Option<&str>) -> Result<String, String> {
    let outbox = crate::outbox::outbox().ok_or("no place to ask from")?;
    let room = listening_room(&outbox, session).ok_or_else(|| {
        "Open a chat in Claude Desktop (or a terminal) with Colai installed, then try again — \
         your artifacts are listed through it."
            .to_string()
    })?;
    let asks = room.join(".ask");
    std::fs::create_dir_all(&asks).map_err(|trouble| format!("could not ask: {trouble}"))?;
    let id = format!(
        "{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|at| at.as_millis())
            .unwrap_or(0),
        std::process::id()
    );
    let question = asks.join(format!("{id}.json"));
    let answer = asks.join(format!("{id}.answer.json"));
    std::fs::write(&question, serde_json::json!({ "artifacts": kind }).to_string())
        .map_err(|trouble| format!("could not ask: {trouble}"))?;
    let began = Instant::now();
    let said = loop {
        if let Ok(said) = std::fs::read_to_string(&answer) {
            break Some(said);
        }
        if began.elapsed() > ANSWER_WITHIN {
            break None;
        }
        std::thread::sleep(Duration::from_millis(150));
    };
    let _ = std::fs::remove_file(&question);
    let _ = std::fs::remove_file(&answer);
    let said = said.ok_or("Claude Desktop didn't answer in time. Try again in a moment.")?;
    let said: serde_json::Value =
        serde_json::from_str(&said).map_err(|_| "the answer could not be read".to_string())?;
    if let Some(error) = said.get("error").and_then(|e| e.as_str()) {
        return Err(error.to_string());
    }
    Ok(said.get("text").and_then(|t| t.as_str()).unwrap_or_default().to_string())
}

/// The outbox room of a session whose bridge is listening: the one asked for if it is, else any.
fn listening_room(outbox: &std::path::Path, session: Option<&str>) -> Option<PathBuf> {
    if let Some(session) = session.filter(|key| !key.is_empty()) {
        if crate::outbox::bridge_listening(session) {
            return Some(outbox.join(session));
        }
    }
    std::fs::read_dir(outbox)
        .ok()?
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .find(|key| crate::outbox::bridge_listening(key))
        .map(|key| outbox.join(key))
}

/// Open one of the person's artifacts in the browser. claude.ai links only: this is the toolbar's
/// one way to open a page, and a page that could open anything would be a different power.
#[tauri::command]
pub(crate) async fn colai_open_artifact(app: tauri::AppHandle, url: String) -> Result<(), String> {
    if !url.starts_with("https://claude.ai/") {
        return Err("only claude.ai links open from here".to_string());
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|trouble| format!("could not open it: {trouble}"))
}

#[cfg(test)]
mod asking_for_artifacts {
    use super::*;

    #[test]
    fn only_the_core_kinds_are_asked_for() {
        assert!(KINDS.contains(&"Slides"));
        assert!(!KINDS.contains(&"Design System"));
    }
}
