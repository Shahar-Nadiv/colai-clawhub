// The shapes that cross between the toolbar and the page.
//
// These were the OpenClaw Claude Code's wire structs, and most of them still describe exactly
// what the page draws — a conversation, a model choice, a thing that was said. They moved
// here when Claude Code client was deleted, because the page had no reason to change and
// a type that outlives its transport is not a transport type.
//
// What did change is the naming. `GatewaySessionSummary` is a `Conversation`: there is no
// Gateway on this host, and a struct named after one would send the next reader looking
// for a service that does not exist.
//
// Several of these are filled in less than they used to be — nothing here schedules
// anything, and nothing asks to go back — and the fields that are no longer filled are
// noted where they sit rather than removed, because the page still reads them and a
// missing field is a silent `undefined` where an empty one is a drawn nothing.

use serde::{Deserialize, Serialize};

/// One conversation Claude Code has on this machine, as much of it as the toolbar needs.
///
/// A deliberately narrow read of a very wide row: the toolbar names a session, says
/// whose it is and whether it is busy. Everything else on the row belongs to the
/// dashboard, and reading it here would be a second, competing idea of a session.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Conversation {
    pub key: String,
    pub agent_id: Option<String>,
    pub label: Option<String>,
    pub display_name: Option<String>,
    pub derived_title: Option<String>,
    pub last_message_preview: Option<String>,
    pub status: Option<String>,
    pub unread: Option<bool>,
    /// When this session last did anything. Only used to decide whether a failure is
    /// news or history — a run that fell over yesterday is not a warning about now.
    pub last_activity_at: Option<i64>,
    pub updated_at: Option<i64>,
}

/// One model the toolbar could answer with.
///
/// Only what the page draws. Claude Code's own entry carries a great deal more — context
/// windows, fallbacks, runtime bindings — and carrying it through would mean this struct
/// changing every time any of that did.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelChoice {
    pub id: String,
    pub name: String,
    pub provider: String,
    /// Whether it can actually be used right now, and why not when it cannot.
    ///
    /// Shown rather than hidden. A model missing because nobody has signed in is
    /// something to go and fix; a model that is simply absent from the list is something
    /// somebody concludes this toolbar cannot do.
    pub available: bool,
    pub why_not: Option<String>,
    /// The efforts this model offers, in the order it offers them.
    ///
    /// From the model rather than from a list held here: which levels exist is the
    /// provider's answer and it changes without asking us.
    pub levels: Vec<ModelLevel>,
    pub level_default: Option<String>,
}

/// What everything running adds up to, for something that can only say one
/// thing at a time.
///
/// Counts rather than a single verdict, because the toolbar has to say *which* and
/// *how many* somewhere, and a struct that decided that here would be deciding it in
/// the wrong language. The page owns the words; this owns the arithmetic.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AtWork {
    pub running: u32,
    pub waiting: u32,
    pub trouble: u32,
    /// Which sessions those are, not just how many.
    ///
    /// The counts answer "is anything happening", which is all the rail's one light
    /// needs. The Work panel asks a different question — has *this* run finished — and a
    /// count cannot answer it: the toolbar was left inferring an ending from the total
    /// reaching zero, with a timeout under it in case the total was about somebody
    /// else's agent. That made a finished agent read as running for the best part of a
    /// minute. Claude Code knows which session is which; this stops throwing it away.
    pub working: Vec<String>,
    /// The same, for sessions that recently fell over.
    pub troubled: Vec<String>,
    /// Every session Claude Code listed, whatever it is doing.
    ///
    /// This is what tells "finished" from "never heard of". Not every run the toolbar
    /// starts reaches this list — an adopted conversation, or one that has not
    /// registered yet, is missing from it — and those two have to be treated
    /// differently: one is over, the other has not begun.
    pub known: Vec<String>,
}

/// A conversation held by an agent Claude Code knows about but does not own — a Claude
/// Code thread, and whatever else registers a catalog later.
///
/// These are not Claude Code sessions and the difference is not pedantic: the store can be
/// empty while two dozen of these are open, which is exactly what a machine looks like
/// when somebody works in a coding agent all day.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CatalogThread {
    pub thread_id: String,
    /// Which agent the host says owns it, when it says.
    pub agent_id: Option<String>,
    pub name: Option<String>,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub archived: Option<bool>,
}

/// Everything needed to name one conversation held in another agent.
///
/// The toolbar has to carry all of it, not just the thread id: continuing a thread is
/// addressed by catalog, host and thread together, and an id on its own names nothing.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThreadLocator {
    pub catalog_id: String,
    pub host_id: String,
    pub thread_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}

/// Asking an external agent to open a fresh conversation in a directory.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartHere {
    pub catalog_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_id: Option<String>,
    pub agent_id: String,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_message: Option<String>,
}

/// One turn of a conversation, as Claude Code recorded it.
///
/// Both halves, because two surfaces want this list and they want different parts of it:
/// rewind offers to go back to a prompt, so it takes the ones that are `mine`; the Work
/// panel is showing a conversation, and a conversation with the answers taken out is a
/// list of things somebody said into a void.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Point {
    pub id: String,
    pub said: String,
    pub at: Option<i64>,
    /// Whether this is something the operator said, rather than something answered back.
    pub mine: bool,
}

/// What comes back from a rewind: the words that were in the composer at that point.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Rewound {
    #[serde(default)]
    pub editor_text: Option<String>,
}

/// An automation, as the toolbar asks for one.
///
/// Claude Code's own `cron.add` shape, narrowed to what a panel on an overlay offers:
/// a name, when it runs, where it runs, and what it says. Triggers, wake mode,
/// timeouts, delivery routes and tool allowances are the Control UI's Advanced fold
/// and stay there — every one of them is a decision somebody should make sitting down.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CronAdd {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_key: Option<String>,
    pub schedule: CronSchedule,
    /// `main` posts into the agent's own timeline; `isolated` runs a turn of its own.
    pub session_target: CronSessionTarget,
    pub wake_mode: CronWakeMode,
    pub payload: CronPayload,
}

/// What came back, as much of it as is worth saying.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CronAdded {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
}

/// A picture carried alongside a message.
///
/// Claude Code takes base64 in `content` and does its own normalizing from there, so
/// this is the whole contract — no upload step, no second round trip.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatAttachment {
    #[serde(rename = "type")]
    pub kind: String,
    pub mime_type: String,
    pub file_name: String,
    pub content: String,
    pub width: i32,
    pub height: i32,
}


/// One stop on the effort slider.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelLevel {
    pub id: String,
    pub label: String,
}

/// When a job runs.
///
/// Closed, and typed, because this crosses in from the WebView and comes back out as
/// persistent state on somebody's Gateway. It used to be a bare `serde_json::Value`
/// forwarded verbatim — the widest untyped hole in the whole IPC surface, next door to
/// commands that are careful about every field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum CronSchedule {
    /// Once, at a moment.
    At { at: String },
    /// On a cron expression, in a named zone or the Gateway's own.
    Cron {
        expr: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tz: Option<String>,
    },
    /// On an interval.
    Every { every_ms: u64 },
}

/// Whether a run joins the agent's own timeline or gets one of its own.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CronSessionTarget {
    Main,
    Isolated,
}

/// Whether a due job wakes the agent now or waits for the next heartbeat.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CronWakeMode {
    Now,
    Heartbeat,
}

/// What a run does. One kind today; the tag is what lets there be another.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum CronPayload {
    AgentTurn { message: String },
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CatalogHost {
    pub host_id: String,
    #[serde(default)]
    pub sessions: Vec<CatalogThread>,
}
