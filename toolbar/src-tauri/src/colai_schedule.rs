//! Scheduled tasks — colai's own small scheduler for recurring and one-off work.
//!
//! Claude Code's scheduled tasks (claude.ai/scheduled-task) are cloud routines: there is no local
//! API or CLI to read or create them from a plugin, and that page is authenticated. colai, though,
//! is a persistent app, so it can own the local version of the same idea — a task is a prompt (and,
//! when it was scheduled from a mark, the picture of what was pointed at), a working directory, and
//! a cadence. The tasks live on disk under `~/.claude/colai/schedule.json`; `watch` runs a thread
//! that fires the ones that have come due.
//!
//! A fired task runs in its own short-lived `claude --print` — standalone, never the shared session
//! the person is talking to, so a task coming due can never kill a turn somebody is in the middle of.
//! It runs with the same confinement an unattended run should have: edits are accepted inside the
//! task's working directory, nothing outside it, and no shell — and with no permission prompts, since
//! nobody is watching to answer one.
//!
//! Wall-clock alignment ("every day at 9am") is computed in the page, which knows the person's own
//! timezone, and arrives here as `next_run` (epoch ms). This side only advances a recurring task to
//! its next occurrence by a whole interval — so there is no timezone math on disk, and the advance is
//! a pure, tested function.

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};

use crate::colai_capture::MarkShots;

/// How often a task repeats. `kind` is all this side needs; `label` is the page's own words for it,
/// kept only to show back ("Daily at 9:00 AM"). `once` never repeats.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Cadence {
    /// "once" | "hourly" | "daily" | "weekly".
    pub kind: String,
    /// The page's human phrasing, shown back as-is. Empty is fine.
    #[serde(default)]
    pub label: String,
}

/// One scheduled task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Task {
    pub id: String,
    pub prompt: String,
    /// Base64 PNGs captured from the marks this task was scheduled with, sent with it each time it
    /// fires. Empty for a task scheduled from the composer without pointing at anything.
    #[serde(default)]
    pub images: Vec<String>,
    /// Where it runs. A task with no directory runs wherever a fresh conversation would, with the
    /// stricter `default` permission — see `fire`.
    #[serde(default)]
    pub cwd: Option<String>,
    pub cadence: Cadence,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub created_at: u64,
    #[serde(default)]
    pub last_run: Option<u64>,
    /// When it next comes due, epoch ms. Computed by the page at creation (local time), advanced by a
    /// whole interval here after each recurring fire.
    pub next_run: u64,
}

fn yes() -> bool {
    true
}

/// The file every task lives in: `{ "tasks": [...] }`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    tasks: Vec<Task>,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// `~/.claude/colai`, created if missing. `None` only when there is no home at all.
fn colai_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    let dir = home.join(".claude").join("colai");
    let _ = fs::create_dir_all(&dir);
    Some(dir)
}

fn store_path() -> Option<PathBuf> {
    colai_dir().map(|dir| dir.join("schedule.json"))
}

/// Read the store, defensively: a missing or malformed file is an empty store, never an error.
fn load() -> Store {
    store_path()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Write the store without a torn file: to a temp beside it, then rename over.
fn save(store: &Store) -> Result<(), String> {
    let path = store_path().ok_or("could not find your home directory")?;
    let text = serde_json::to_string_pretty(store).map_err(|e| format!("could not encode the schedule: {e}"))?;
    let tmp = path.with_extension("json.colai-tmp");
    fs::write(&tmp, text).map_err(|e| format!("could not write the schedule: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("could not replace the schedule: {e}"))
}

/// How long one repeat of this cadence is, or `None` for a one-off.
fn interval_ms(kind: &str) -> Option<u64> {
    match kind {
        "hourly" => Some(3_600_000),
        "daily" => Some(86_400_000),
        "weekly" => Some(604_800_000),
        _ => None,
    }
}

/// Advance a due time past `now` by whole intervals.
///
/// A whole number of steps, not a single `+ step`, so a task the machine was asleep through does not
/// then fire once a tick to catch up — it skips straight to the next real occurrence.
fn advance(next_run: u64, now: u64, step: u64) -> u64 {
    if step == 0 {
        return next_run;
    }
    let mut when = next_run;
    while when <= now {
        when = when.saturating_add(step);
    }
    when
}

/// Is this task due to fire now?
fn is_due(task: &Task, now: u64) -> bool {
    task.enabled && now >= task.next_run
}

/// Where a task stands after it fires: its new `last_run`, its new `next_run`, and whether it is
/// still enabled (a one-off disables itself; a recurring task advances to its next occurrence).
fn after_fire(task: &Task, now: u64) -> (u64, u64, bool) {
    match interval_ms(&task.cadence.kind) {
        Some(step) => (now, advance(task.next_run, now, step), true),
        None => (now, task.next_run, false),
    }
}

/* ── the commands the page calls ─────────────────────────────────────────── */

/// Every scheduled task, newest first.
#[tauri::command]
pub(crate) async fn colai_schedule_list() -> Result<Vec<Task>, String> {
    let mut tasks = load().tasks;
    tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(tasks)
}

/// Schedule a new task. The pictures are pulled from the marks (if any) and stored with it, so it
/// carries them every time it fires even after a restart.
#[tauri::command]
pub(crate) async fn colai_schedule_add(
    shots: State<'_, MarkShots>,
    prompt: String,
    next_run: u64,
    cadence: Cadence,
    cwd: Option<String>,
    mark_ids: Option<Vec<String>>,
) -> Result<Task, String> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("A scheduled task needs something to do.".to_string());
    }
    let images = pictures_for(&shots, &mark_ids.unwrap_or_default())?;
    let now = now_ms();
    let task = Task {
        id: format!("sch-{now}"),
        prompt,
        images,
        cwd: cwd.filter(|c| !c.trim().is_empty()),
        cadence,
        enabled: true,
        created_at: now,
        last_run: None,
        // Never in the past: a task scheduled for a moment that has already slipped by fires on the
        // next tick rather than "now", which is the honest reading of "every day at 9" set at 9:01.
        next_run: next_run.max(now),
    };
    let mut store = load();
    store.tasks.push(task.clone());
    save(&store)?;
    Ok(task)
}

/// Forget a task.
#[tauri::command]
pub(crate) async fn colai_schedule_remove(id: String) -> Result<(), String> {
    let mut store = load();
    let before = store.tasks.len();
    store.tasks.retain(|t| t.id != id);
    if store.tasks.len() == before {
        return Ok(());
    }
    save(&store)
}

/// Pause or resume a task without forgetting it.
#[tauri::command]
pub(crate) async fn colai_schedule_toggle(id: String, enabled: bool) -> Result<(), String> {
    let mut store = load();
    let mut found = false;
    for task in store.tasks.iter_mut() {
        if task.id == id {
            task.enabled = enabled;
            found = true;
        }
    }
    if !found {
        return Err("that task is no longer scheduled".to_string());
    }
    save(&store)
}

/// Run a task right now, out of turn, without touching its schedule. For the "run now" button.
#[tauri::command]
pub(crate) async fn colai_schedule_run_now(app: AppHandle, id: String) -> Result<(), String> {
    let task = load().tasks.into_iter().find(|t| t.id == id).ok_or("that task is no longer scheduled")?;
    let said = fire(&task)?;
    announce(&app, &task, said.as_deref());
    // Record that it ran, but leave the schedule alone — this was out of turn.
    let mut store = load();
    for one in store.tasks.iter_mut() {
        if one.id == id {
            one.last_run = Some(now_ms());
        }
    }
    let _ = save(&store);
    Ok(())
}

/// Open Claude Code's scheduled-tasks page in the browser, for the cloud routines colai cannot read.
#[tauri::command]
pub(crate) async fn colai_schedule_open_web(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url("https://claude.ai/scheduled-task", None::<&str>)
        .map_err(|e| format!("could not open the page: {e}"))
}

/* ── the scheduler itself ────────────────────────────────────────────────── */

/// How often to look for tasks that have come due. Scheduled work is not to-the-second, and a task
/// set for 9:00 firing a few seconds after is indistinguishable from one that fired on the dot.
const EVERY: Duration = Duration::from_secs(20);

/// Watch for due tasks, forever, on a thread of its own.
pub(crate) fn watch(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(EVERY);
        tick(&app);
    });
}

/// One pass: fire every task that has come due, then write back where each one now stands.
///
/// The schedule is advanced and saved *before* the task is fired, so a fire that crashes the task's
/// own child can never leave a recurring task stuck re-firing the same due time in a loop.
fn tick(app: &AppHandle) {
    let now = now_ms();
    let store = load();
    let due: Vec<Task> = store.tasks.iter().filter(|t| is_due(t, now)).cloned().collect();
    if due.is_empty() {
        return;
    }
    // Advance the schedule first, and persist it.
    let mut next = load();
    for one in next.tasks.iter_mut() {
        if due.iter().any(|d| d.id == one.id) {
            let (last, when, still) = after_fire(one, now);
            one.last_run = Some(last);
            one.next_run = when;
            one.enabled = still;
        }
    }
    let _ = save(&next);
    // Then fire each, one at a time.
    for task in due {
        match fire(&task) {
            Ok(said) => announce(app, &task, said.as_deref()),
            Err(why) => {
                eprintln!("[colai] a scheduled task could not run: {why}");
                let _ = app.emit_to(
                    crate::colai::OVERLAY_LABEL,
                    "colai:scheduled",
                    serde_json::json!({ "reason": "trouble", "id": task.id, "prompt": task.prompt, "said": why }),
                );
            }
        }
    }
}

/// Tell the page a task fired, so it can note it and refresh the list's "last run".
fn announce(app: &AppHandle, task: &Task, said: Option<&str>) {
    let _ = app.emit_to(
        crate::colai::OVERLAY_LABEL,
        "colai:scheduled",
        serde_json::json!({ "reason": "fired", "id": task.id, "prompt": task.prompt, "said": said }),
    );
}

/// The base64 PNGs for these marks, pulled from the in-memory store at scheduling time.
fn pictures_for(shots: &MarkShots, mark_ids: &[String]) -> Result<Vec<String>, String> {
    if mark_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for picked in shots.pick(mark_ids)? {
        for frame in picked.frames {
            out.push(base64::engine::general_purpose::STANDARD.encode(frame));
        }
    }
    Ok(out)
}

/// Well-known directories just inside home that hold credentials or configuration, and so are
/// too sensitive to hand unattended `acceptEdits` to. The names are the same on every platform
/// (`~/.ssh` and `%USERPROFILE%\.ssh` alike), so joining them onto home covers Unix and Windows.
const SENSITIVE_DIRS: [&str; 5] = [".ssh", ".aws", ".gnupg", ".config", ".claude"];

/// Whether a directory is too broad to accept edits in unattended — the home directory itself,
/// a filesystem root, or one of the well-known credential/config directories under home
/// (`~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.config`, `~/.claude`). A task pointed at one of these
/// still runs, but under the stricter `default` mode: `acceptEdits` across the whole of
/// somebody's home — or across their keys, cloud credentials or Claude's own config — is no
/// confinement at all, and nobody is watching to catch it reaching where it should not.
fn too_broad(cwd: &str) -> bool {
    let path = std::path::Path::new(cwd);
    // A filesystem root has no parent — `/` on Unix, a drive root like `C:\` on Windows.
    if path.parent().is_none() {
        return true;
    }
    let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
    else {
        return false;
    };
    // Home itself.
    if path == home {
        return true;
    }
    // A sensitive directory anchored just inside home. Comparing against `home.join(name)` matches
    // the canonical path suffix regardless of separator or a trailing slash (`Path` equality is by
    // component), so it holds cross-platform without catching an unrelated `.config` elsewhere.
    SENSITIVE_DIRS.iter().any(|name| path == home.join(name))
}

/// Run a task in its own short-lived `claude`, and return its answer if one came back.
///
/// Standalone on purpose — never the shared session the person is talking to — so a task coming due
/// cannot interrupt a turn they are in. Unattended, so there are no permission prompts: edits are
/// accepted inside the task's own directory and nothing else, and without a directory the stricter
/// `default` applies, exactly as a fresh conversation gets.
fn fire(task: &Task) -> Result<Option<String>, String> {
    let claude = crate::session::Session::discover().ok_or("no `claude` on PATH to run the task")?;
    let mut run = Command::new(&claude);
    run.args([
        "--print",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
    ]);
    let in_a_dir = task.cwd.as_deref().map(|c| std::path::Path::new(c).is_dir()).unwrap_or(false);
    // A directory this broad — the home directory, or a filesystem root — is too much to hand
    // unattended edits to: `acceptEdits` there is edits anywhere somebody keeps anything. The
    // confinement only means something when it confines, so the broad case drops to `default`.
    let confined = in_a_dir && !task.cwd.as_deref().map(too_broad).unwrap_or(false);
    run.args(["--permission-mode", if confined { "acceptEdits" } else { "default" }]);
    if in_a_dir {
        run.current_dir(task.cwd.as_deref().unwrap());
    }
    crate::session::no_console_window(&mut run);
    let mut child = run
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start claude: {e}"))?;

    // The pictures first, the sentence last — it refers to them.
    let mut content: Vec<serde_json::Value> = task
        .images
        .iter()
        .map(|data| {
            serde_json::json!({
                "type": "image",
                "source": {"type": "base64", "media_type": "image/png", "data": data},
            })
        })
        .collect();
    content.push(serde_json::json!({"type": "text", "text": task.prompt}));
    let frame = serde_json::json!({
        "type": "user",
        "uuid": format!("colai-sched-{}", now_ms()),
        "origin": {"kind": "human"},
        "message": {"role": "user", "content": content},
    });
    {
        let mut stdin = child.stdin.take().ok_or("could not reach claude's input")?;
        writeln!(stdin, "{frame}").map_err(|e| format!("could not send the task: {e}"))?;
        // Dropping stdin closes it, which is how `--print` knows the input is complete and runs.
    }
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut out);
    }
    let _ = child.wait();
    Ok(final_text(&out))
}

/// The agent's final answer, dug out of a `stream-json` run's output.
///
/// The last line is a `{"type":"result", ...}` frame carrying the whole answer as `result`; failing
/// that (an interrupted run), the text of the last assistant message. `None` when neither is there.
fn final_text(out: &str) -> Option<String> {
    let mut answer: Option<String> = None;
    for line in out.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        match value.get("type").and_then(|t| t.as_str()) {
            Some("result") => {
                if let Some(text) = value.get("result").and_then(|r| r.as_str()) {
                    return Some(text.trim().to_string());
                }
            }
            Some("assistant") => {
                if let Some(text) = value
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_array())
                    .map(|parts| {
                        parts
                            .iter()
                            .filter_map(|p| (p.get("type").and_then(|t| t.as_str()) == Some("text")).then(|| p.get("text").and_then(|t| t.as_str())).flatten())
                            .collect::<Vec<_>>()
                            .join("")
                    })
                    .filter(|s| !s.trim().is_empty())
                {
                    answer = Some(text.trim().to_string());
                }
            }
            _ => {}
        }
    }
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_task(kind: &str, next_run: u64, enabled: bool) -> Task {
        Task {
            id: "sch-1".into(),
            prompt: "do the thing".into(),
            images: vec![],
            cwd: None,
            cadence: Cadence { kind: kind.into(), label: String::new() },
            enabled,
            created_at: 0,
            last_run: None,
            next_run,
        }
    }

    #[test]
    fn a_task_is_due_only_when_enabled_and_its_time_has_come() {
        assert!(is_due(&a_task("daily", 100, true), 100), "due at exactly its time");
        assert!(is_due(&a_task("daily", 100, true), 500), "due once its time is past");
        assert!(!is_due(&a_task("daily", 100, true), 50), "not due before its time");
        assert!(!is_due(&a_task("daily", 100, false), 500), "a paused task is never due");
    }

    #[test]
    fn a_oneoff_disables_itself_after_firing_and_a_recurring_task_advances() {
        // A one-off: last_run set, next_run unchanged, and no longer enabled.
        let once = a_task("once", 100, true);
        let (last, next, still) = after_fire(&once, 150);
        assert_eq!(last, 150);
        assert_eq!(next, 100, "a one-off does not reschedule");
        assert!(!still, "a one-off switches itself off after it runs");

        // A daily task advances by exactly one day past its due time.
        let daily = a_task("daily", 100, true);
        let (_, next, still) = after_fire(&daily, 150);
        assert_eq!(next, 100 + 86_400_000, "daily moves on a day");
        assert!(still, "a recurring task stays on");
    }

    #[test]
    fn a_task_the_machine_slept_through_skips_to_its_next_real_occurrence() {
        // Three days asleep: it does not fire three times to catch up — it jumps to the next day
        // still in the future.
        let step = 86_400_000u64;
        let now = step * 3 + 5;
        assert_eq!(advance(step, now, step), step * 4, "skips straight past every missed day");
    }

    #[test]
    fn the_answer_is_read_from_the_result_frame() {
        let out = r#"{"type":"system","subtype":"init"}
{"type":"assistant","message":{"content":[{"type":"text","text":"working on it"}]}}
{"type":"result","result":"all done"}"#;
        assert_eq!(final_text(out).as_deref(), Some("all done"));
    }

    #[test]
    fn with_no_result_frame_the_last_assistant_text_is_used() {
        let out = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"a first thought"}]}}
{"type":"assistant","message":{"content":[{"type":"text","text":"the final word"}]}}"#;
        assert_eq!(final_text(out).as_deref(), Some("the final word"), "the last assistant text stands in");
    }

    #[test]
    fn nothing_parseable_yields_no_answer() {
        assert_eq!(final_text("not json at all\n\n"), None);
    }

    #[test]
    fn home_or_a_root_is_too_broad_but_a_directory_inside_it_is_not() {
        let _held = crate::hold_the_environment();
        let before = std::env::var_os("HOME");
        std::env::set_var("HOME", "/home/someone");

        // A filesystem root, whatever the platform calls it.
        assert!(too_broad("/"), "a filesystem root is too broad");
        #[cfg(windows)]
        assert!(too_broad("C:\\"), "a drive root is too broad");

        // The home directory itself, with or without a trailing slash.
        assert!(too_broad("/home/someone"), "home itself is too broad");
        assert!(too_broad("/home/someone/"), "a trailing slash is still home");

        // A well-known credential/config directory just inside home, likewise.
        assert!(too_broad("/home/someone/.ssh"), "~/.ssh holds keys — too broad");
        assert!(too_broad("/home/someone/.ssh/"), "a trailing slash is still ~/.ssh");
        assert!(too_broad("/home/someone/.config"), "~/.config is too broad");

        // A project somewhere inside home is exactly the confined case this leaves alone.
        assert!(!too_broad("/home/someone/Desktop/colai"), "a directory under home is fine");
        assert!(!too_broad("/home/someone/.ssh/keys/project"), "a directory under ~/.ssh is confinable");
        assert!(!too_broad("/srv/build"), "an ordinary directory is fine");
        assert!(!too_broad("/srv/.config"), "a .config outside home is not the sensitive one");

        match before {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }
}
