//! Who the toolbar can hand a region to.
//!
//! Three kinds that look like three things on the menu and are one thing underneath:
//! an agent this machine is configured with, a conversation the Gateway is holding, and
//! a conversation held in another agent entirely. Every one of them resolves to a
//! session key, which is why sending has one path and not three.
//!
//! Apart from the overlay because it shares nothing with it. Nothing here knows about a
//! window, a shape, or a screen.

use serde::Serialize;

/// How many conversations the rail will offer. More than anybody scrolls, few enough
/// that reading every transcript to build the list stays quick.
const SESSIONS_AT_MOST: usize = 60;

/// One agent the toolbar can hand a region to.
///
/// The same list the rest of the application uses, not a second idea of what an agent
/// is. `receiving` is the toolbar's own state on top of it: several agents may be
/// available, and exactly one gets what you point at.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolbarAgent {
    pub id: String,
    pub name: String,
    pub emoji: Option<String>,
    pub receiving: bool,
}

/// The agents this machine actually has.
///
/// Read from the Gateway rather than kept here, so the toolbar never disagrees with the
/// window behind it about what is running. The Gateway caches the list itself, so asking
/// on every summon is cheap.
///
/// System agents are left out, the way Quick Chat leaves them out: they are machinery
/// rather than somebody you would hand a region to.
/// What this connection may do, so the toolbar can offer only what will work.
///
/// Asked rather than assumed. Rewinding a conversation needs a scope a desktop token may
/// simply not carry, and a menu item that always fails is worse than one that is absent —
/// it teaches somebody the toolbar is broken.
#[tauri::command]
pub(crate) fn colai_allowed(
    gateway: tauri::State<'_, crate::gateway_ws::GatewayClient>,
) -> Vec<String> {
    gateway.scopes()
}

#[tauri::command]
pub(crate) async fn colai_agents(
    #[allow(unused_variables)] receiving: Option<String>,
) -> Result<Vec<ToolbarAgent>, String> {
    /*
     * One Claude, so no list to choose from.
     *
     * OpenClaw ran many agents on somebody's behalf and the toolbar had to say which one
     * was getting the mark. Claude Code is one assistant with many conversations, so the
     * thing worth choosing is *which conversation* — that is `colai_sessions` — and an
     * agent picker here would be a control with exactly one entry in it.
     */
    Ok(Vec::new())
}

/// One conversation on the toolbar's menu.
///
/// `title` is settled here rather than in the page, because deciding what a nameless
/// session is called is a judgement about the data and not about layout, and the rail
/// and the menu must never disagree about what a row is called.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolbarSession {
    pub key: String,
    pub title: String,
    pub agent_id: Option<String>,
    pub busy: bool,
    pub unread: bool,
    pub receiving: bool,
    /// The last thing said in it, which is what the Work panel writes under the name.
    ///
    /// Carried here rather than fetched per conversation: a panel of forty rows would
    /// otherwise be forty transcripts down a socket to draw forty single lines. The
    /// transcript itself is asked for when somebody opens one.
    pub preview: Option<String>,
    /// When it last did anything, for ordering the panel and for saying "4m".
    ///
    /// Activity first and the record's own timestamp second: a conversation nobody has
    /// spoken in since Tuesday is a Tuesday conversation, whatever was written to it since.
    pub at: Option<i64>,
}

/// The conversations this machine is holding.
///
/// The other half of "who receives what you point at": an agent is who could answer,
/// a session is a conversation already underway, and handing a region to one of those
/// puts it where the work already is.
///
/// Nothing is invented when the Gateway has nothing. An empty list is a real answer —
/// no conversations yet — and the page says so rather than filling the menu.
#[tauri::command]
pub(crate) async fn colai_sessions(
    receiving: Option<String>,
) -> Result<Vec<ToolbarSession>, String> {
    /*
     * Every conversation Claude Code has on this machine, newest first.
     *
     * This is what the receiver list becomes here, and it is read off disk rather than
     * asked for: the transcripts under `~/.claude/projects` are the session list.
     */
    let chosen = receiving.as_deref();
    Ok(crate::session::conversations(SESSIONS_AT_MOST)
        .into_iter()
        .map(|one| ToolbarSession {
            receiving: chosen == Some(one.session_key.as_str()),
            key: one.session_key,
            title: one.name,
            // The project it is being had in, which is the thing that tells two
            // conversations with similar names apart.
            preview: one
                .cwd
                .as_deref()
                .and_then(|cwd| cwd.rsplit('/').next())
                .map(str::to_string),
            agent_id: None,
            // Nothing here is busy or unread: one conversation is live at a time and the
            // toolbar is the thing having it, so it already knows.
            busy: false,
            unread: false,
            at: Some(one.at as i64),
        })
        .collect())
}

/// Which models this agent could answer with.
///
/// Asked when the picker opens rather than kept warm. The same rule the conversations
/// menu already follows: the answer only matters when somebody is looking at it, and a
/// catalogue held in the background is a catalogue that is quietly wrong the moment
/// somebody signs into a provider.
#[tauri::command]
pub(crate) async fn colai_models(
    #[allow(unused_variables)] agent_id: Option<String>,
) -> Result<Vec<crate::gateway_ws::ModelChoice>, String> {
    /*
     * Claude Code chooses its own model, and changing it mid-conversation is its own
     * `/model`. The Gateway kept the choice on the conversation and had a method to patch
     * it; there is nothing equivalent to patch here, so rather than offer a picker that
     * moves nothing this is empty and the rail draws no control.
     */
    Ok(Vec::new())
}

/// What every agent on this Gateway is doing, for the light on the toolbar.
///
/// Deliberately not scoped to the conversation somebody is on. The whole point is the
/// agent you are *not* looking at: work you started in one conversation and left,
/// running while you point at something in another. A light that only knew about the
/// selected agent would go out exactly when it was worth watching.
///
/// Approvals are asked for separately because they are the one thing a session's own
/// status cannot say. "Running" covers both an agent thinking and an agent stopped dead
/// waiting for somebody to say yes, and those are not the same news.
///
/// A Gateway that cannot answer one half still answers the other: nothing waiting is
/// reported as nothing waiting rather than failing the whole light, since the running
/// count is the half somebody watches most.
#[tauri::command]
pub(crate) async fn colai_at_work(
    session: tauri::State<'_, crate::session::Session>,
) -> Result<crate::gateway_ws::AtWork, String> {
    /*
     * The rail's one light. On OpenClaw it counted other people's agents running in the
     * background; here the only thing working is the conversation this toolbar is having,
     * which it already knows about — so this reports on itself and nothing else.
     */
    let _ = session.spent();
    Ok(crate::gateway_ws::AtWork::default())
}

/// What to call a conversation, in the order a person would.
///
/// The name somebody gave it, then the name the Gateway shows in its own list, then the
/// title projected from the first message, and only then the routing key — which is an
/// address rather than a name, and appears when a session genuinely has nothing else.
fn session_title(row: &crate::gateway_ws::GatewaySessionSummary) -> String {
    [
        row.label.as_deref(),
        row.display_name.as_deref(),
        row.derived_title.as_deref(),
        row.last_message_preview.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .find(|name| !name.is_empty())
    .map(|name| name.to_string())
    .unwrap_or_else(|| row.key.clone())
}

/// One conversation held in another agent, on the toolbar's menu.
///
/// `where_at` is the branch it sits on, which is what tells two threads of the same
/// name apart — the same task picked up twice on different branches is ordinary.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolbarThread {
    pub id: String,
    pub title: String,
    pub where_at: Option<String>,
    pub receiving: bool,
    /// Carried so the thread can be continued later. An id alone names nothing: a
    /// conversation is addressed by its catalog, its host and its thread together.
    pub locator: crate::gateway_ws::ThreadLocator,
}

/// The folder a set of those conversations belongs to.
///
/// `label` is `None` for threads with no project at all: they are listed loose rather
/// than filed under an invented folder name, the same way the dashboard does it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolbarProject {
    pub key: String,
    pub holder: String,
    pub label: Option<String>,
    pub path: Option<String>,
    pub threads: Vec<ToolbarThread>,
}

/// The conversations somebody has open somewhere other than the Gateway.
///
/// A machine can have an empty Gateway session store and two dozen of these — which is
/// The directories somebody actually works in, as the Gateway reports them.
///
/// This is the list that decides what `@` may reach and what may be read into a send,
/// so it deliberately comes from the Gateway rather than from the page. A page that
/// could name its own roots could name `/`, and then the gate in front of the file
/// system would be a gate the thing being gated holds the key to.
///
/// Archived threads count: the folder is still somewhere they work, even if that
/// conversation is finished.
pub(crate) async fn work_roots(
    gateway: &crate::gateway_ws::GatewayClient,
) -> Vec<std::path::PathBuf> {
    let Ok(listed) = gateway.sessions_catalog_list().await else {
        // No answer is not permission. An empty list refuses everything, which is the
        // safe direction for a question about what may be read.
        return Vec::new();
    };
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    for catalog in &listed.catalogs {
        for host in &catalog.hosts {
            for thread in &host.sessions {
                let Some(root) = thread.cwd.as_deref().and_then(project_root) else {
                    continue;
                };
                let root = std::path::PathBuf::from(root);
                if !worth_reading(&root) {
                    continue;
                }
                if !roots.contains(&root) {
                    roots.push(root);
                }
            }
        }
    }
    roots
}

/// Whether a directory is narrow enough to be a root at all.
///
/// The comment above says a page that could name its own roots could name `/`. Everything
/// in that sentence is also true of the Gateway, which is where these come from — a
/// conversation reported with `cwd: "/"` would hand the file gate a root that contains
/// every other root, and the gate in front of the file system would be answering a
/// question that had already been decided.
///
/// The test is depth rather than a list of forbidden places, because a list is a thing to
/// keep up to date and depth is a property. Two named components is the shallowest a
/// working directory is ever legitimately at — `/home/someone`, `/opt/thing`, `/srv/app` —
/// and everything this rules out (`/`, `/home`, `/usr`, `/etc`, `/var`, `/mnt`) is a place
/// nobody runs an agent from.
///
/// Deliberately not "inside `$HOME`". `carry` gates dragged and dialog-picked files
/// against these same roots, so a home-only rule would refuse a file somebody chose
/// themselves out of `/mnt/work` — a real loss, to close a hole that depth closes anyway.
fn worth_reading(root: &std::path::Path) -> bool {
    if !root.is_absolute() {
        return false;
    }
    root.components()
        .filter(|part| matches!(part, std::path::Component::Normal(_)))
        .count()
        >= 2
}

/// The conversations already under way, so a mark can join one instead of starting over.
///
/// Archived threads are left out; they are history rather than somewhere to send a
/// region.
///
/// Grouped by project here rather than in the page, and by the dashboard's own rule, so
/// the two surfaces never disagree about which folder a conversation belongs to.
#[tauri::command]
pub(crate) async fn colai_threads(
    #[allow(unused_variables)] receiving: Option<String>,
) -> Result<Vec<ToolbarProject>, String> {
    /*
     * Projects were OpenClaw's second axis: conversations belonging to somebody else's
     * checkout, adoptable into yours. Claude Code's conversations already carry the
     * directory they were had in, and `colai_sessions` says so under each name — so a
     * separate tree of projects would be the same list drawn twice.
     */
    Ok(Vec::new())
}

/// Give every folder a name that tells it apart from the others on the menu.
///
/// Two checkouts of the same repository, or two unrelated folders ending in the same
/// word, are ordinary — and two rows both reading "colai" make the menu useless. Each
/// duplicate takes one more parent segment until it stands alone, which is the least
/// path that answers "which one".
fn name_projects(projects: &mut [ToolbarProject]) {
    let mut depth: Vec<usize> = vec![1; projects.len()];
    loop {
        let labels: Vec<Option<String>> = projects
            .iter()
            .enumerate()
            .map(|(at, project)| project.path.as_deref().map(|path| tail_of(path, depth[at])))
            .collect();
        let mut clashing = false;
        for at in 0..projects.len() {
            let Some(mine) = labels[at].as_deref() else {
                continue;
            };
            let shared = labels
                .iter()
                .enumerate()
                .any(|(other, label)| other != at && label.as_deref() == Some(mine));
            // Only grow while there is more path to grow into. Two identical paths
            // cannot be told apart, and looping over them would hang the menu.
            if shared && segments(projects[at].path.as_deref().unwrap_or("")) > depth[at] {
                depth[at] += 1;
                clashing = true;
            }
        }
        if !clashing {
            for (at, project) in projects.iter_mut().enumerate() {
                project.label = labels[at].clone();
            }
            return;
        }
    }
}

/// The last `many` segments of a path, as somebody would say them.
fn tail_of(path: &str, many: usize) -> String {
    let parts: Vec<&str> = path
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect();
    let from = parts.len().saturating_sub(many.max(1));
    let tail = parts[from..].join("/");
    if tail.is_empty() {
        path.to_string()
    } else {
        tail
    }
}

fn segments(path: &str) -> usize {
    path.split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .count()
}

/// What to call a thread: its own name, then the folder it runs in, then its id.
fn thread_title(thread: &crate::gateway_ws::CatalogThread) -> String {
    named(thread.name.as_deref())
        .or_else(|| thread.cwd.as_deref().map(checkout_name))
        .unwrap_or_else(|| thread.thread_id.clone())
}

/// The checkout a working directory belongs to, or `None` when it names no project.
///
/// A worktree under `.claude/worktrees/<name>` folds into the repository it came from,
/// because a person thinks of those as one project and the dashboard already groups them
/// that way. The first marker wins, which is the outermost repository.
fn project_root(cwd: &str) -> Option<String> {
    let trimmed = named(Some(cwd))?;
    let trimmed = trimmed.trim_end_matches(['/', '\\']);
    for marker in ["/.claude/worktrees/", "\\.claude\\worktrees\\"] {
        let Some(at) = trimmed.find(marker) else {
            continue;
        };
        // A worktree needs a name after the marker; the folder itself is not one.
        if trimmed[at + marker.len()..]
            .starts_with(|character| character == '/' || character == '\\')
        {
            continue;
        }
        let origin = &trimmed[..at];
        return (!origin.is_empty()).then(|| origin.to_string());
    }
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// The name a checkout goes by: its last path segment.
fn checkout_name(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .find(|segment| !segment.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn named(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::worth_reading;
    use std::path::Path;

    #[test]
    fn a_root_that_contains_every_other_root_is_not_a_root() {
        // What a hostile or broken Gateway would have to say to defeat the file gate.
        assert!(!worth_reading(Path::new("/")));
        assert!(!worth_reading(Path::new("/home")));
        assert!(!worth_reading(Path::new("/usr")));
        assert!(!worth_reading(Path::new("/etc")));
        assert!(!worth_reading(Path::new("/var")));
    }

    #[test]
    fn somewhere_somebody_actually_works_is() {
        assert!(worth_reading(Path::new("/home/someone")));
        assert!(worth_reading(Path::new("/home/someone/Desktop/colai")));
        // Not everybody keeps their work under their home, and `carry` gates dragged
        // files against these roots — so outside home stays readable.
        assert!(worth_reading(Path::new("/opt/thing")));
        assert!(worth_reading(Path::new("/mnt/work/app")));
    }

    #[test]
    fn a_relative_path_is_not_a_root() {
        // Nothing downstream resolves these, so a relative root would mean whatever the
        // process happened to be standing in.
        assert!(!worth_reading(Path::new("work")));
        assert!(!worth_reading(Path::new("../work")));
    }

    use super::*;
    use crate::gateway_ws::GatewaySessionSummary;

    fn row(key: &str) -> GatewaySessionSummary {
        GatewaySessionSummary {
            key: key.to_string(),
            agent_id: None,
            label: None,
            display_name: None,
            derived_title: None,
            last_message_preview: None,
            status: None,
            last_activity_at: None,
            updated_at: None,
            unread: None,
        }
    }

    fn thread(id: &str) -> crate::gateway_ws::CatalogThread {
        crate::gateway_ws::CatalogThread {
            thread_id: id.to_string(),
            agent_id: None,
            name: None,
            cwd: None,
            git_branch: None,
            archived: None,
        }
    }

    #[test]
    fn a_thread_falls_back_to_the_folder_it_runs_in() {
        let mut unnamed = thread("c94530b7");
        unnamed.cwd = Some("/home/someone/Desktop/colai/".to_string());
        assert_eq!(thread_title(&unnamed), "colai");
        // Nothing at all leaves the id, which at least identifies the row.
        assert_eq!(thread_title(&thread("c94530b7")), "c94530b7");
    }

    #[test]
    fn a_worktree_belongs_to_the_repository_it_came_from() {
        // Otherwise a week of worktrees becomes a week of one-conversation projects.
        assert_eq!(
            project_root("/home/someone/colai/.claude/worktrees/fix-the-rail").as_deref(),
            Some("/home/someone/colai"),
        );
        assert_eq!(
            project_root("/home/someone/colai/").as_deref(),
            Some("/home/someone/colai"),
        );
        assert_eq!(checkout_name("/home/someone/colai"), "colai");
    }

    fn project(path: &str) -> ToolbarProject {
        ToolbarProject {
            key: path.to_string(),
            holder: "Claude Code".to_string(),
            label: Some(checkout_name(path)),
            path: Some(path.to_string()),
            threads: Vec::new(),
        }
    }

    #[test]
    fn folders_that_share_a_name_take_enough_path_to_tell_them_apart() {
        let mut projects = [
            project("/home/someone/Desktop/colai"),
            project("/home/someone/code/colai"),
            project("/home/someone/code/arc"),
        ];
        name_projects(&mut projects);
        assert_eq!(projects[0].label.as_deref(), Some("Desktop/colai"));
        assert_eq!(projects[1].label.as_deref(), Some("code/colai"));
        // The one that was already unique keeps its short name.
        assert_eq!(projects[2].label.as_deref(), Some("arc"));
    }

    #[test]
    fn folders_that_cannot_be_told_apart_stop_growing() {
        // Identical paths have no distinguishing segment left; growing forever would
        // hang the menu rather than admit that.
        let mut same = [
            project("/home/someone/colai"),
            project("/home/someone/colai"),
        ];
        name_projects(&mut same);
        assert_eq!(same[0].label.as_deref(), Some("home/someone/colai"));
        assert_eq!(same[1].label.as_deref(), Some("home/someone/colai"));
    }

    #[test]
    fn a_directory_that_names_no_project_is_left_ungrouped() {
        // A worktree with no origin above it, and a path that is only separators: both
        // are filed loose rather than under an invented folder.
        assert_eq!(project_root("/.claude/worktrees/orphan"), None);
        assert_eq!(project_root("///"), None);
        assert_eq!(project_root("   "), None);
    }

    #[test]
    fn a_session_is_called_what_a_person_called_it() {
        let mut named = row("agent:main:whatsapp:123");
        named.label = Some("Kitchen rebuild".to_string());
        named.display_name = Some("+1 555".to_string());
        named.derived_title = Some("help me with".to_string());
        assert_eq!(session_title(&named), "Kitchen rebuild");
    }

    #[test]
    fn an_unnamed_session_falls_back_through_what_it_has() {
        let mut projected = row("agent:main:cli:42");
        // A blank label is not a name; skipping it is the difference between a menu of
        // conversations and a menu of empty rows.
        projected.label = Some("   ".to_string());
        projected.derived_title = Some("Rewrite the invoice parser".to_string());
        assert_eq!(session_title(&projected), "Rewrite the invoice parser");

        // Nothing at all leaves the routing key, which is an address rather than a name
        // — shown because a row nobody can identify is worse than an ugly one.
        assert_eq!(
            session_title(&row("agent:main:cli:42")),
            "agent:main:cli:42"
        );
    }
}
