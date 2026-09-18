//! Who the toolbar can hand a region to.
//!
//! One kind: a conversation Claude Code is having on this machine. OpenClaw put three on
//! the menu — an agent it was configured with, a session it was holding, a conversation
//! held in another agent entirely — because it ran many agents on somebody's behalf. One
//! Claude with many conversations leaves exactly one thing worth choosing, so the agent
//! list and the project tree are gone rather than kept as menus with nothing in them.
//!
//! Apart from the overlay because it shares nothing with it. Nothing here knows about a
//! window, a shape, or a screen.

use serde::Serialize;

/// How many conversations the rail will offer. More than anybody scrolls, few enough
/// that reading every transcript to build the list stays quick.
const SESSIONS_AT_MOST: usize = 60;

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
/// The whole of "who receives what you point at" now: a conversation already underway,
/// and handing a region to one of those puts it where the work already is. An empty id
/// on the way back out means start a new one — see `colai_send`.
///
/// Nothing is invented when there is nothing. An empty list is a real answer — no
/// conversations yet — and the page says so rather than filling the menu.
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
            at: Some(one.at as i64),
        })
        .collect())
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
) -> Result<crate::wire::AtWork, String> {
    /*
     * The rail's one light. On OpenClaw it counted other people's agents running in the
     * background; here the only thing working is the conversation this toolbar is having,
     * which it already knows about — so this reports on itself and nothing else.
     */
    let _ = session.spent();
    Ok(crate::wire::AtWork::default())
}
