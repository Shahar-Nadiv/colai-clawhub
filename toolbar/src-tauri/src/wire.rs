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
// Several of these are filled in less than they used to be — nothing asks to go back —
// and the fields that are no longer filled are noted where they sit rather than removed,
// because the page still reads them and a missing field is a silent `undefined` where an
// empty one is a drawn nothing.
//
// Scheduling's shapes are not among them. `CronAdd` and its schedule, target, wake mode
// and payload enums described a Gateway that ran agents in the background, and nothing on
// this host can be asked to run a turn later — so they are deleted rather than kept empty:
// a type nobody can construct teaches the next reader that the feature is merely switched
// off somewhere, and sends them looking for the switch.

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

