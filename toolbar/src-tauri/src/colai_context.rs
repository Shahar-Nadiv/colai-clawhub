//! The Context map's store — local-first.
//!
//! A "context" is a saved slice of a conversation: what was decided and worked on, kept so it
//! can be dropped into another session later. The PRD imagines these syncing across a team and
//! across a person's machines through a Gateway; this codebase deliberately has no Gateway, so
//! the store here is **local only**. The model still carries the fields a sync backend would
//! need — `owner`, `machine`, `visibility`, `payload_ref` — but they are inert: everything is
//! owned by "you", lives on this machine, and is private. Filling them in later is what a
//! future backend does; nothing here depends on one.
//!
//! Storage is plain files under `~/.claude/colai`: one JSON per context in `contexts/`, and the
//! conversation snapshots in `payloads/`, named by a content hash so two contexts saved from the
//! same untouched transcript share one payload rather than storing it twice.
//!
//! The file logic is written as pure functions over an explicit root, with the Tauri commands as
//! thin wrappers that resolve the root from `HOME`. That keeps the tests off the process
//! environment (no `HOME` juggling, no races) and lets them run against a temp directory.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// The window budget a resolved drop is measured against, so an overflow can be reported rather
/// than silently summarised away. A fraction of a large model's context — enough to be useful,
/// small enough that piling on contexts is caught.
const WINDOW: u64 = 180_000;

/// One saved context. `#[serde(rename_all = "camelCase")]` so the page reads `updatedAt`,
/// `payloadRef`, `isLocal` — the names the PRD's model uses.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Context {
    pub id: String,
    pub title: String,
    pub project: String,
    /// Always "you" here — the sync field a backend would fill with a real identity.
    pub owner: String,
    /// This machine's name — the field that would distinguish machines once several sync.
    pub machine: String,
    pub tokens: u64,
    /// Inert: everything local is "private". Kept so the model does not have to change later.
    pub visibility: String,
    pub updated_at: i64,
    /// The content hash of the snapshot in `payloads/`, so identical snapshots share a file.
    pub payload_ref: String,
    /// True for everything this store holds; the seam a cross-machine index would vary.
    pub is_local: bool,
}

/// One turn of a saved conversation.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct Turn {
    /// "you" or "claude".
    pub who: String,
    pub text: String,
}

/// A saved conversation snapshot, stored under `payloads/<hash>.json`.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Payload {
    pub session: String,
    pub turns: Vec<Turn>,
    pub saved_at: i64,
}

/// What resolving a set of contexts produces: the drop, measured and de-duplicated.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Manifest {
    pub contexts: Vec<ManifestEntry>,
    /// Sum of the unique payloads' tokens — a snapshot counted once however many contexts name it.
    pub tokens: u64,
    pub window: u64,
    /// True when the drop would not fit the window. Reported, never summarised away.
    pub overflow: bool,
    /// How many distinct snapshots the drop actually carries.
    pub payloads: usize,
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManifestEntry {
    pub id: String,
    pub title: String,
    pub owner: String,
    pub machine: String,
    pub tokens: u64,
    pub payload_ref: String,
}

/// A rough token count: about four characters to a token. Good enough to warn on, which is all
/// the figure is for — the real count is Claude Code's to make when the drop is opened.
pub(crate) fn estimate_tokens(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4)
}

/// FNV-1a, so identical snapshots hash the same without pulling in a crypto crate. The hash only
/// has to spot "the same bytes" for de-duplication, not resist an adversary.
fn hash_hex(bytes: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// This machine's name — `COMPUTERNAME` on Windows, `HOSTNAME` elsewhere, and an honest
/// placeholder when neither is set rather than a guess dressed as fact.
fn this_machine() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "this machine".to_string())
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or(0)
}

/// A fresh id for a saved context. Two saves in the same millisecond would otherwise collide on
/// `ctx-<ms>` and the second would overwrite the first's file, so a per-process counter keeps
/// them apart.
fn fresh_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    format!("ctx-{}-{}", now_ms(), SEQ.fetch_add(1, Ordering::Relaxed))
}

/// `~` — `HOME` first, then `USERPROFILE`, because Windows sets the latter and not always the
/// former. The rest of the toolbar reads `HOME`; this adds the fallback so the store still lands
/// somewhere sensible if a Windows session never set it.
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

/// The store root, `~/.claude/colai`. Resolved from the environment for the real commands; the
/// pure functions below take a root directly so tests never touch it.
fn store_root() -> Result<PathBuf, String> {
    Ok(home()
        .ok_or("No home directory to keep contexts in (neither HOME nor USERPROFILE is set).")?
        .join(".claude")
        .join("colai"))
}

fn contexts_dir(root: &Path) -> PathBuf {
    root.join("contexts")
}
fn payloads_dir(root: &Path) -> PathBuf {
    root.join("payloads")
}

/// Save a snapshot as a context under `root`, returning the context that was written.
///
/// The payload is written under its own content hash, so saving the same untouched conversation
/// twice writes the snapshot once and points both contexts at it. The context record is small and
/// always its own file.
pub(crate) fn save_at(
    root: &Path,
    title: &str,
    project: &str,
    session: &str,
    turns: Vec<Turn>,
) -> Result<Context, String> {
    let text: String = turns.iter().map(|turn| turn.text.as_str()).collect::<Vec<_>>().join("\n");
    let tokens = estimate_tokens(&text);
    // The hash is over what makes a snapshot the *same* snapshot — the session and its turns —
    // and deliberately not over `saved_at`. Hashing the whole record would give the same
    // conversation a new hash every time it was saved, which is exactly the de-duplication this
    // is meant to do, undone by a timestamp.
    let canonical: String = std::iter::once(session.to_string())
        .chain(turns.iter().map(|turn| format!("{}\u{1f}{}", turn.who, turn.text)))
        .collect::<Vec<_>>()
        .join("\u{1e}");
    let payload_ref = hash_hex(&canonical);
    let payload = Payload {
        session: session.to_string(),
        turns,
        saved_at: now_ms(),
    };
    let body = serde_json::to_string(&payload).map_err(|why| format!("could not encode the snapshot: {why}"))?;

    let payloads = payloads_dir(root);
    fs::create_dir_all(&payloads).map_err(|why| format!("could not make the payloads folder: {why}"))?;
    let payload_path = payloads.join(format!("{payload_ref}.json"));
    // Only if it is not already there: an identical snapshot is the same file, which is the whole
    // point of hashing it.
    if !payload_path.exists() {
        fs::write(&payload_path, &body).map_err(|why| format!("could not write the snapshot: {why}"))?;
    }

    let context = Context {
        id: fresh_id(),
        title: if title.trim().is_empty() { "Untitled context".to_string() } else { title.trim().to_string() },
        project: project.to_string(),
        owner: "you".to_string(),
        machine: this_machine(),
        tokens,
        visibility: "private".to_string(),
        updated_at: now_ms(),
        payload_ref,
        is_local: true,
    };
    let contexts = contexts_dir(root);
    fs::create_dir_all(&contexts).map_err(|why| format!("could not make the contexts folder: {why}"))?;
    let record = serde_json::to_string_pretty(&context).map_err(|why| format!("could not encode the context: {why}"))?;
    fs::write(contexts.join(format!("{}.json", context.id)), record)
        .map_err(|why| format!("could not write the context: {why}"))?;
    Ok(context)
}

/// Every context under `root`, newest first. `scope` narrows it the way the panel's segmented
/// control does: "team" is always empty here (nobody has shared one, because there is no sync),
/// and every other scope returns the local set — which is all there is.
pub(crate) fn list_at(root: &Path, scope: &str) -> Vec<Context> {
    if scope == "team" {
        return Vec::new();
    }
    let mut found: Vec<Context> = Vec::new();
    if let Ok(entries) = fs::read_dir(contexts_dir(root)) {
        for entry in entries.flatten() {
            if entry.path().extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            if let Ok(body) = fs::read_to_string(entry.path()) {
                // Tolerant: a record that has changed shape is skipped, not fatal — the same
                // rule the transcript reader uses.
                if let Ok(context) = serde_json::from_str::<Context>(&body) {
                    found.push(context);
                }
            }
        }
    }
    found.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    found
}

/// Build the drop manifest for a set of context ids: what each one is, the tokens the whole drop
/// carries (a shared snapshot counted once), and whether it overflows the window. Missing ids are
/// skipped rather than failing the whole resolve.
pub(crate) fn resolve_at(root: &Path, ids: &[String]) -> Manifest {
    let dir = contexts_dir(root);
    let mut entries: Vec<ManifestEntry> = Vec::new();
    let mut seen_payloads: Vec<String> = Vec::new();
    let mut tokens: u64 = 0;
    for id in ids {
        let path = dir.join(format!("{id}.json"));
        let Ok(body) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(context) = serde_json::from_str::<Context>(&body) else {
            continue;
        };
        // A snapshot two contexts share is dropped once and its tokens counted once.
        if !seen_payloads.contains(&context.payload_ref) {
            seen_payloads.push(context.payload_ref.clone());
            tokens += context.tokens;
        }
        entries.push(ManifestEntry {
            id: context.id,
            title: context.title,
            owner: context.owner,
            machine: context.machine,
            tokens: context.tokens,
            payload_ref: context.payload_ref,
        });
    }
    Manifest {
        contexts: entries,
        tokens,
        window: WINDOW,
        overflow: tokens > WINDOW,
        payloads: seen_payloads.len(),
    }
}

/* ── the commands, thin wrappers resolving the root from the environment ─────── */

#[tauri::command]
pub(crate) fn colai_contexts_list(scope: Option<String>) -> Result<Vec<Context>, String> {
    let root = store_root()?;
    Ok(list_at(&root, scope.as_deref().unwrap_or("mine")))
}

#[tauri::command]
pub(crate) fn colai_context_save_session(
    session_key: String,
    title: String,
    project: Option<String>,
) -> Result<Context, String> {
    // The transcript is the only record on this host, so a saved context is a snapshot of it.
    // Capped, because a context is a slice worth carrying, not an entire history.
    let turns: Vec<Turn> = crate::session::what_was_said(&session_key, 400)
        .into_iter()
        .map(|(_, text, mine, _)| Turn {
            who: if mine { "you".to_string() } else { "claude".to_string() },
            text,
        })
        .collect();
    let root = store_root()?;
    save_at(&root, &title, project.as_deref().unwrap_or_default(), &session_key, turns)
}

#[tauri::command]
pub(crate) fn colai_context_resolve(ids: Vec<String>) -> Result<Manifest, String> {
    let root = store_root()?;
    Ok(resolve_at(&root, &ids))
}

/// Clearing the staged set is a page concern — the chips a person has dropped but not yet opened
/// live in the page's own state, and nothing about them is written until Open. The command exists
/// so the page has one to call and the API is whole; there is no persisted staging to remove.
#[tauri::command]
pub(crate) fn colai_context_clear_staged() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A throwaway directory that removes itself on drop. The store's functions take a root
    /// directly, so a test needs a folder, not a fiddled `HOME` — this is just that folder.
    struct TempRoot(PathBuf);
    impl TempRoot {
        fn make() -> Self {
            static COUNT: AtomicU64 = AtomicU64::new(0);
            let unique = format!(
                "colai-ctx-{}-{}",
                now_ms(),
                COUNT.fetch_add(1, Ordering::Relaxed),
            );
            TempRoot(std::env::temp_dir().join(unique))
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn turns(text: &str) -> Vec<Turn> {
        vec![
            Turn { who: "you".to_string(), text: text.to_string() },
            Turn { who: "claude".to_string(), text: "done".to_string() },
        ]
    }

    #[test]
    fn a_saved_context_lists_and_resolves() {
        let home = TempRoot::make();
        let root = home.path().join(".claude").join("colai");

        let saved = save_at(&root, "Fixing the hotkey", "colai", "sess-1", turns("the hotkey work"))
            .expect("save");
        assert_eq!(saved.owner, "you");
        assert!(saved.is_local);
        assert_eq!(saved.visibility, "private");
        assert!(saved.tokens > 0, "a non-empty snapshot has tokens");

        let listed = list_at(&root, "mine");
        assert_eq!(listed.len(), 1, "the saved context is listed");
        assert_eq!(listed[0].id, saved.id);

        let manifest = resolve_at(&root, &[saved.id.clone()]);
        assert_eq!(manifest.contexts.len(), 1);
        assert_eq!(manifest.tokens, saved.tokens);
        assert_eq!(manifest.payloads, 1);
        assert!(!manifest.overflow);
    }

    #[test]
    fn the_team_scope_is_empty_here() {
        let home = TempRoot::make();
        let root = home.path().join(".claude").join("colai");
        save_at(&root, "One", "colai", "s", turns("a")).expect("save");
        assert!(list_at(&root, "team").is_empty(), "nobody has shared a context");
        assert_eq!(list_at(&root, "mine").len(), 1);
        assert_eq!(list_at(&root, "all").len(), 1, "all-machines shows the local set");
    }

    #[test]
    fn an_identical_snapshot_is_stored_and_counted_once() {
        let home = TempRoot::make();
        let root = home.path().join(".claude").join("colai");
        // Two contexts saved from the same conversation text: same payload hash.
        let a = save_at(&root, "A", "colai", "sess", turns("same words")).expect("a");
        let b = save_at(&root, "B", "colai", "sess", turns("same words")).expect("b");
        assert_eq!(a.payload_ref, b.payload_ref, "identical snapshots share a hash");

        // One payload file on disk.
        let payloads = fs::read_dir(payloads_dir(&root)).expect("payloads").count();
        assert_eq!(payloads, 1, "the snapshot is written once");

        // Resolving both counts the shared snapshot's tokens once.
        let manifest = resolve_at(&root, &[a.id.clone(), b.id.clone()]);
        assert_eq!(manifest.contexts.len(), 2, "both contexts are in the drop");
        assert_eq!(manifest.payloads, 1, "but one snapshot");
        assert_eq!(manifest.tokens, a.tokens, "counted once, not doubled");
    }

    #[test]
    fn a_drop_past_the_window_is_reported_not_summarised() {
        let home = TempRoot::make();
        let root = home.path().join(".claude").join("colai");
        // A snapshot large enough to blow the window on its own.
        let big = "x".repeat((WINDOW as usize + 10) * 4);
        let saved = save_at(&root, "Big", "colai", "s", turns(&big)).expect("save");
        assert!(saved.tokens > WINDOW);
        let manifest = resolve_at(&root, &[saved.id]);
        assert!(manifest.overflow, "the overflow is reported");
        assert_eq!(manifest.window, WINDOW);
    }

    #[test]
    fn a_missing_id_is_skipped_not_fatal() {
        let home = TempRoot::make();
        let root = home.path().join(".claude").join("colai");
        let saved = save_at(&root, "Real", "colai", "s", turns("here")).expect("save");
        let manifest = resolve_at(&root, &["ctx-does-not-exist".to_string(), saved.id.clone()]);
        assert_eq!(manifest.contexts.len(), 1, "only the real one resolves");
        assert_eq!(manifest.contexts[0].id, saved.id);
    }
}
