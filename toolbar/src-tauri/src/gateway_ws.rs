use crate::gateway_device_identity::{
    GatewayAuth, GatewayDeviceIdentity, GatewayDeviceIdentityStore, CLIENT_DEVICE_FAMILY,
    CLIENT_ID, CLIENT_MODE, CLIENT_PLATFORM, CLIENT_ROLE, CLIENT_SCOPES,
};
use futures_util::{SinkExt, StreamExt};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error as RustlsError, SignatureScheme};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fmt;
use std::io::ErrorKind;
#[cfg(any(target_os = "linux", test))]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;
#[cfg(any(target_os = "linux", test))]
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::{Error as TungsteniteError, Message};
use tokio_tungstenite::{
    connect_async, connect_async_tls_with_config, Connector, MaybeTlsStream, WebSocketStream,
};
use uuid::Uuid;

const AGENT_KIND_CLIENT_CAPABILITY: &str = "agent-kind";
const GATEWAY_STATE_EVENT: &str = "colai:gateway";
/// What the toolbar hears when a session it is watching says something.
const REPLY_EVENT: &str = "colai:reply";
/// And what it hears while that session is still working, one tool call at a time.
const DOING_EVENT: &str = "colai:doing";
const GATEWAY_DEVICE_IDENTITY_FILE: &str = "quickchat-gateway-device.json";
const AGENTS_CACHE_TTL: Duration = Duration::from_secs(60);
/// How many conversations the toolbar's picker asks for.
///
/// A menu, not a session browser: the dashboard behind it is where somebody goes to
/// find an old conversation, and an unbounded ask would make opening the menu cost more
/// the longer the machine has been used.
const SESSIONS_SHOWN: u32 = 12;
/// How many conversations the toolbar asks each external agent for.
///
/// Larger than the Gateway's own, and deliberately: a coding agent accumulates a thread
/// per task, so a person's day is two dozen of them, and a picker that showed half of
/// what the window behind it lists is the bug this whole surface exists to fix. Still
/// bounded — the menu scrolls, it does not grow forever.
const THREADS_SHOWN: u32 = 25;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(35);
const DRIVER_TICK: Duration = Duration::from_secs(1);
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(30);
const PAIRING_REQUIRED_DETAIL_CODE: &str = "PAIRING_REQUIRED";
const AUTH_TOKEN_MISSING_DETAIL_CODE: &str = "AUTH_TOKEN_MISSING";
const AUTH_PASSWORD_MISSING_DETAIL_CODE: &str = "AUTH_PASSWORD_MISSING";
const AUTH_DEVICE_TOKEN_MISMATCH_DETAIL_CODE: &str = "AUTH_DEVICE_TOKEN_MISMATCH";
const TLS_PIN_MISMATCH_ERROR: &str = "Gateway TLS certificate fingerprint mismatch";

// Mirrors packages/gateway-protocol/src/version.ts. The Gateway rejects other ranges.
const MIN_PROTOCOL_VERSION: u32 = 4;
const MAX_PROTOCOL_VERSION: u32 = 4;
const INLINE_WIDGETS_CLIENT_CAPABILITY: &str = "inline-widgets";

#[derive(Clone)]
pub struct GatewayWsConfig {
    ws_url: String,
    token: Option<String>,
    password: Option<String>,
    tls_fingerprint: Option<String>,
}

impl GatewayWsConfig {
    pub fn new(
        ws_url: String,
        token: Option<String>,
        password: Option<String>,
        tls_fingerprint: Option<String>,
    ) -> Self {
        Self {
            ws_url,
            token,
            password,
            tls_fingerprint,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TlsTrustDecision {
    SystemRoots,
    Pinned([u8; 32]),
}

fn tls_trust_decision(fingerprint: Option<&str>) -> Result<TlsTrustDecision, String> {
    fingerprint
        .map(parse_tls_fingerprint)
        .transpose()
        .map(|fingerprint| {
            fingerprint.map_or(TlsTrustDecision::SystemRoots, TlsTrustDecision::Pinned)
        })
}

fn parse_tls_fingerprint(raw: &str) -> Result<[u8; 32], String> {
    let value = raw.trim();
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Gateway TLS fingerprint must be 64 hexadecimal characters.".to_string());
    }
    let mut fingerprint = [0_u8; 32];
    for (index, byte) in fingerprint.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| "Gateway TLS fingerprint is invalid.".to_string())?;
    }
    Ok(fingerprint)
}

fn pinned_fingerprint_matches(expected: &[u8; 32], certificate_der: &[u8]) -> bool {
    let observed: [u8; 32] = Sha256::digest(certificate_der).into();
    bool::from(expected.as_slice().ct_eq(observed.as_slice()))
}

struct GatewayTlsPinVerifier {
    expected: [u8; 32],
    supported_algorithms: WebPkiSupportedAlgorithms,
}

impl fmt::Debug for GatewayTlsPinVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayTlsPinVerifier")
            .finish_non_exhaustive()
    }
}

impl ServerCertVerifier for GatewayTlsPinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        // The local CLI authenticates this exact leaf-certificate hash before handing it to the
        // app. A present pin replaces CA/hostname trust, matching OpenClawKit; the signature
        // methods below still prove the peer owns the certificate's private key.
        if pinned_fingerprint_matches(&self.expected, end_entity.as_ref()) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(RustlsError::General(TLS_PIN_MISMATCH_ERROR.to_string()))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls12_signature(message, cert, signature, &self.supported_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls13_signature(message, cert, signature, &self.supported_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.supported_algorithms.supported_schemes()
    }
}

fn pinned_tls_connector(expected: [u8; 32]) -> Result<Connector, String> {
    let provider = rustls::crypto::ring::default_provider();
    let verifier = GatewayTlsPinVerifier {
        expected,
        supported_algorithms: provider.signature_verification_algorithms,
    };
    let config = ClientConfig::builder_with_provider(Arc::new(provider))
        .with_safe_default_protocol_versions()
        .map_err(|error| format!("Could not configure Gateway TLS: {error}"))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Ok(Connector::Rustls(Arc::new(config)))
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GatewayAgentIdentity {
    pub name: Option<String>,
    pub emoji: Option<String>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct GatewayAgentSummary {
    pub id: String,
    pub kind: Option<String>,
    pub name: Option<String>,
    pub identity: Option<GatewayAgentIdentity>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentsListResult {
    pub default_id: String,
    pub main_key: String,
    pub scope: String,
    pub agents: Vec<GatewayAgentSummary>,
}

/// One conversation the Gateway is holding, as much of it as the toolbar needs.
///
/// A deliberately narrow read of a very wide row: the toolbar names a session, says
/// whose it is and whether it is busy. Everything else on the row belongs to the
/// dashboard, and reading it here would be a second, competing idea of a session.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GatewaySessionSummary {
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
/// Only what the page draws. The Gateway's own entry carries a great deal more — context
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

/// One stop on the effort slider.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelLevel {
    pub id: String,
    pub label: String,
}

/// Read the models out of a `chat.metadata` reply, keeping only what is drawn.
fn models_in(payload: &Value) -> Vec<ModelChoice> {
    payload
        .get("models")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let id = row.get("id").and_then(Value::as_str)?.trim();
                    if id.is_empty() {
                        return None;
                    }
                    let named = |key: &str| {
                        row.get(key)
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|said| !said.is_empty())
                            .map(str::to_string)
                    };
                    Some(ModelChoice {
                        id: id.to_string(),
                        // A model with no display name is named by its id, which is
                        // still something somebody can recognise.
                        name: named("name").unwrap_or_else(|| id.to_string()),
                        provider: named("provider").unwrap_or_default(),
                        // Absent means usable: the field is only sent when there is
                        // something to say, and treating silence as "unavailable" would
                        // empty the list on every Gateway that does not send it.
                        available: row
                            .get("available")
                            .and_then(Value::as_bool)
                            .unwrap_or(true),
                        why_not: named("unavailableReason"),
                        levels: row
                            .get("thinkingLevels")
                            .and_then(Value::as_array)
                            .map(|levels| {
                                levels
                                    .iter()
                                    .filter_map(|level| {
                                        let id = level.get("id").and_then(Value::as_str)?.trim();
                                        if id.is_empty() {
                                            return None;
                                        }
                                        let label = level
                                            .get("label")
                                            .and_then(Value::as_str)
                                            .map(str::trim)
                                            .filter(|said| !said.is_empty())
                                            .unwrap_or(id);
                                        Some(ModelLevel {
                                            id: id.to_string(),
                                            label: label.to_string(),
                                        })
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                        level_default: named("thinkingDefault"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// What every agent on this Gateway adds up to, for something that can only say one
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
    /// minute. The Gateway knows which session is which; this stops throwing it away.
    pub working: Vec<String>,
    /// The same, for sessions that recently fell over.
    pub troubled: Vec<String>,
    /// Every session the Gateway listed, whatever it is doing.
    ///
    /// This is what tells "finished" from "never heard of". Not every run the toolbar
    /// starts reaches this list — an adopted conversation, or one that has not
    /// registered yet, is missing from it — and those two have to be treated
    /// differently: one is over, the other has not begun.
    pub known: Vec<String>,
}

/// A conversation held by an agent the Gateway knows about but does not own — a Claude
/// Code thread, and whatever else registers a catalog later.
///
/// These are not Gateway sessions and the difference is not pedantic: the store can be
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

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CatalogHost {
    pub host_id: String,
    #[serde(default)]
    pub sessions: Vec<CatalogThread>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionCatalog {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub hosts: Vec<CatalogHost>,
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

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CatalogContinueResult {
    pub session_key: String,
}

/// One turn of a conversation, as the Gateway recorded it.
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

/// The prompts in a transcript, read the way the Gateway actually sends them.
///
/// Written against `chat.history` as it is served — `src/gateway/session-transcript-message.ts`
/// projects each stored event onto the message itself and stamps the event's own id into
/// `__openclaw`, which is the id `sessions.rewind` takes back. An earlier version of this
/// guessed at `entryId` and friends, found nothing, and left the panel empty on a
/// conversation full of prompts; so every name below is one this code has been shown.
///
/// The failure mode is still the point: an entry whose id it cannot find produces
/// nothing rather than a guess, because the toolbar would otherwise offer to discard
/// work at an address it made up.
fn points_in(payload: &Value) -> Vec<Point> {
    let Some(rows) = payload.get("messages").and_then(Value::as_array) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            // A page carries the message itself; a delta wraps it in an envelope.
            let inner = row.get("message").unwrap_or(row);
            let meta = inner.get("__openclaw");
            let role = inner
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or_default();
            // Everything a person or an agent actually said. Tool calls and system
            // scaffolding are not turns of a conversation and read as noise in a panel.
            if role != "user" && role != "assistant" {
                return None;
            }
            let id = meta
                .and_then(|meta| meta.get("id"))
                .or_else(|| row.get("messageId"))
                .and_then(Value::as_str)?;
            // Typed but not yet a turn. The Gateway lists these separately and there is
            // nothing behind them to go back to.
            if id.starts_with(PENDING_PROMPT) {
                return None;
            }
            Some(Point {
                mine: role == "user",
                said: said_in(inner).unwrap_or_default(),
                at: meta
                    .and_then(|meta| meta.get("recordTimestampMs"))
                    .or_else(|| inner.get("timestamp"))
                    .and_then(when_in),
                id: id.to_string(),
            })
        })
        .collect()
}

/// Input the Gateway is holding rather than a turn it has recorded.
const PENDING_PROMPT: &str = "pending:";

/// When something happened, whether it arrived as milliseconds or as a date.
///
/// Both are real: a projected page carries `recordTimestampMs`, and a stored message
/// carries an ISO string the moment it comes from anywhere else.
fn when_in(value: &Value) -> Option<i64> {
    if let Some(millis) = value.as_i64() {
        return Some(millis);
    }
    let text = value.as_str()?;
    let (date, rest) = text.split_once('T')?;
    let mut date = date.split('-');
    let year: i64 = date.next()?.parse().ok()?;
    let month: i64 = date.next()?.parse().ok()?;
    let day: i64 = date.next()?.parse().ok()?;
    let clock = rest.trim_end_matches('Z');
    let mut clock = clock.split(':');
    let hour: i64 = clock.next()?.parse().ok()?;
    let minute: i64 = clock.next()?.parse().ok()?;
    let second: f64 = clock.next()?.parse().ok()?;
    // Days since the epoch by Howard Hinnant's civil-day algorithm, which is exact for
    // every date this will ever see and saves taking a date library for one field.
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some(((days * 86_400 + hour * 3_600 + minute * 60) * 1_000) + (second * 1_000.0) as i64)
}

/// How long after a run falls over the toolbar still calls it news.
///
/// A session that failed an hour ago is simply a session, and a red light about it is a
/// warning about nothing — which teaches somebody to stop reading the light. Long
/// enough to be seen if you were away from the desk, short enough to still be about now.
const TROUBLE_RECENT: i64 = 10 * 60 * 1000;

/// What the sessions add up to: how many are working, and how many just fell over.
///
/// Pure, and separate from the request that fetches them, because every interesting
/// decision is here — which statuses count as work, which count as trouble, and when
/// trouble stops being news.
pub(crate) fn at_work_of(sessions: &[GatewaySessionSummary], now: i64) -> AtWork {
    let mut counted = AtWork::default();
    for row in sessions {
        counted.known.push(row.key.clone());
        match row.status.as_deref() {
            Some("running" | "queued") => {
                counted.running += 1;
                counted.working.push(row.key.clone());
            }
            // Killed is not trouble. Somebody stopped it on purpose, and a red light
            // over a deliberate act is the toolbar arguing with the person using it.
            Some("failed" | "timeout") => {
                let last = row.last_activity_at.or(row.updated_at).unwrap_or(0);
                if now - last < TROUBLE_RECENT {
                    counted.trouble += 1;
                    counted.troubled.push(row.key.clone());
                }
            }
            _ => {}
        }
    }
    counted
}

/// What a tool said, and whether it was there to say anything.
///
/// `missing` is the useful half. A tool the agent does not have is not a failure to
/// report — it is the answer to "is a library connected", and the surface that asked
/// says something helpful about it rather than showing an error.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ToolAnswer {
    pub output: Value,
    pub missing: bool,
    pub trouble: Option<String>,
}

/// Read one `tools.invoke` reply, keeping the difference between the three outcomes.
fn answer_in(payload: &Value) -> ToolAnswer {
    if payload.get("ok").and_then(Value::as_bool) == Some(true) {
        return ToolAnswer {
            output: payload.get("output").cloned().unwrap_or(Value::Null),
            missing: false,
            trouble: None,
        };
    }
    let error = payload.get("error");
    let code = error
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    ToolAnswer {
        output: Value::Null,
        missing: code == "not_found",
        trouble: (code != "not_found").then(|| {
            error
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("The tool could not be called.")
                .to_string()
        }),
    }
}

/// How many approvals are waiting, out of whatever shape the list arrives in.
///
/// Only the count is wanted: the icon says "something is waiting on you" and the
/// Control UI is where somebody goes to see what. Reading no further than that is also
/// what keeps this from breaking when the record around it grows a field.
fn waiting_in(payload: &Value) -> u32 {
    let rows = payload
        .as_array()
        .or_else(|| payload.get("approvals").and_then(Value::as_array))
        .or_else(|| payload.get("items").and_then(Value::as_array));
    rows.map_or(0, |rows| rows.len() as u32)
}

/// What a prompt said, in the words somebody would recognise it by.
///
/// `content` is a plain string on every user message this has been shown, and a list of
/// parts on the ones the Gateway composes; both are read because both are served.
///
/// An attachment sent without words arrives as markup rather than as nothing, so it is
/// stripped here. Otherwise the panel offers a row of HTML to choose between, which is
/// worse than admitting there were no words.
fn said_in(message: &Value) -> Option<String> {
    let said = match message.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" "),
        _ => return None,
    };
    let words = without_markup(&said);
    (!words.is_empty()).then_some(words)
}

/// The same words with any tags taken out of them.
fn without_markup(said: &str) -> String {
    let mut words = String::with_capacity(said.len());
    let mut inside = false;
    for letter in said.chars() {
        match letter {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => words.push(letter),
            _ => {}
        }
    }
    words.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// An automation, as the toolbar asks for one.
///
/// The Gateway's own `cron.add` shape, narrowed to what a panel on an overlay offers:
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

/// What came back, as much of it as is worth saying.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CronAdded {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Deserialize)]
pub(crate) struct SessionsCatalogListResult {
    #[serde(default)]
    pub catalogs: Vec<SessionCatalog>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct SessionsListResult {
    #[serde(default)]
    pub sessions: Vec<GatewaySessionSummary>,
}

#[derive(Clone)]
struct CachedAgents {
    fetched_at: Instant,
    result: AgentsListResult,
}

/// The conversation list, for as long as it is certainly still true.
struct CachedSessions {
    fetched_at: Instant,
    result: SessionsListResult,
}

/// How long two callers asking the same question count as one asking.
///
/// The panel and the rail both want the conversation list, and they want it in the same
/// breath: `loadWork` asks, and `colai_at_work` asks again microseconds later for a count
/// derived from the same rows. That was two identical round trips every five seconds
/// forever — around half of everything this toolbar pulled while nobody was touching it.
///
/// Deliberately far shorter than the tick that drives them. This is not a cache of the
/// conversation list; it is a way of noticing that one question was asked twice.
const SESSIONS_ARE_FRESH_FOR: Duration = Duration::from_millis(750);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatSendParams {
    session_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_id: Option<String>,
    message: String,
    idempotency_key: String,
    /// Omitted entirely when there is nothing to carry, because an empty list is a
    /// different claim from no list at all.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    attachments: Vec<ChatAttachment>,
}

/// A picture carried alongside a message.
///
/// The Gateway takes base64 in `content` and does its own normalizing from there, so
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatSendAck {
    run_id: String,
    status: String,
    #[serde(default)]
    error: Option<Value>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatRoutingTarget {
    pub(crate) session_key: String,
    pub(crate) agent_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatSendResult {
    #[serde(flatten)]
    pub(crate) target: ChatRoutingTarget,
    pub(crate) run_id: String,
}

enum GatewayRequest {
    /// Which models this agent could answer with, and what each one can be asked for.
    ChatMetadata {
        agent_id: Option<String>,
    },
    /// A per-conversation setting, changed. Only the two the toolbar offers.
    SessionsPatch {
        key: String,
        agent_id: Option<String>,
        model: Option<String>,
        thinking_level: Option<String>,
    },
    AgentsList,
    SessionsList,
    SessionsCatalogList,
    SessionsCatalogContinue(ThreadLocator),
    WatchSession {
        key: String,
        watching: bool,
    },
    StartHere(StartHere),
    CronAdd(CronAdd),
    ChatAbort {
        key: String,
    },
    ChatHistory {
        key: String,
        limit: u32,
    },
    SessionsRewind {
        key: String,
        entry_id: String,
    },
    /// Everything waiting on a person right now, across every agent.
    ApprovalsPending,
    /// One of the agent's own tools, called as the agent.
    ToolsInvoke {
        name: String,
        args: Value,
        agent_id: Option<String>,
        session_key: Option<String>,
    },
    ChatSend(ChatSendParams),
}

enum GatewayResponse {
    Models(Vec<ModelChoice>),
    Patched,
    AgentsList(AgentsListResult),
    SessionsList(SessionsListResult),
    SessionsCatalogList(SessionsCatalogListResult),
    SessionsCatalogContinue(CatalogContinueResult),
    CronAdd(CronAdded),
    History(Vec<Point>),
    Pending(u32),
    ToolOutput(ToolAnswer),
    Rewound(Rewound),
    ChatSend(ChatSendAck),
    /// It worked and there is nothing to read. A request whose whole answer is "yes".
    Done,
}

enum DriverCommand {
    Request {
        request: GatewayRequest,
        budget: Option<Duration>,
        reply: oneshot::Sender<Result<GatewayResponse, String>>,
    },
    Reconfigure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GatewayConnectionState {
    Down = 0,
    Up = 1,
    PairingRequired = 2,
    CredentialRequired = 3,
    TlsFailure = 4,
}

impl GatewayConnectionState {
    fn from_u64(value: u64) -> Self {
        match value {
            1 => Self::Up,
            2 => Self::PairingRequired,
            3 => Self::CredentialRequired,
            4 => Self::TlsFailure,
            _ => Self::Down,
        }
    }

    fn event_name(self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::Up => "up",
            Self::PairingRequired => "pairing-required",
            Self::CredentialRequired => "credential-required",
            Self::TlsFailure => "tls-failure",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ConnectErrorDetails {
    code: Option<String>,
    device_id: Option<String>,
    remediation_hint: Option<String>,
    retryable: Option<bool>,
    pause_reconnect: Option<bool>,
}

impl ConnectErrorDetails {
    fn from_value(value: Option<&Value>) -> Self {
        let Some(value) = value else {
            return Self::default();
        };
        Self {
            code: connect_detail_text(value.get("code"), 80),
            device_id: connect_detail_text(value.get("deviceId"), 128),
            remediation_hint: connect_detail_text(value.get("remediationHint"), 240),
            retryable: value.get("retryable").and_then(Value::as_bool),
            pause_reconnect: value.get("pauseReconnect").and_then(Value::as_bool),
        }
    }
}

struct RequestFailure {
    message: String,
    disconnect: bool,
    connect_details: ConnectErrorDetails,
    connect_state: Option<GatewayConnectionState>,
    tls_failure: bool,
}

impl RequestFailure {
    fn transport(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            disconnect: true,
            connect_details: ConnectErrorDetails::default(),
            connect_state: None,
            tls_failure: false,
        }
    }

    fn tls(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            disconnect: true,
            connect_details: ConnectErrorDetails::default(),
            connect_state: None,
            tls_failure: true,
        }
    }

    fn method_with_details(message: impl Into<String>, details: Option<&Value>) -> Self {
        Self {
            message: message.into(),
            disconnect: false,
            connect_details: ConnectErrorDetails::from_value(details),
            connect_state: None,
            tls_failure: false,
        }
    }

    fn classify_connect(mut self, auth: &GatewayAuth) -> Self {
        self.connect_state =
            classify_connect_failure(self.connect_details.code.as_deref(), !auth.is_none());
        self
    }
}

struct GatewayClientInner {
    config: Mutex<Option<GatewayWsConfig>>,
    config_generation: AtomicU64,
    commands: Mutex<Option<mpsc::Sender<DriverCommand>>>,
    agents_cache: Mutex<Option<CachedAgents>>,
    identity: Mutex<Option<GatewayDeviceIdentityStore>>,
    /// What the Gateway said this connection may do, from the last handshake.
    scopes: Mutex<Vec<String>>,
    sessions_cache: Mutex<Option<CachedSessions>>,
    /// The sessions this toolbar is listening to.
    ///
    /// A subscription lives on the connection, not on the account, so every one of them
    /// dies with the socket. Nothing here used to remember them, and the handshake replays
    /// only `connect` and `agents.list` — so after any drop the toolbar stayed connected,
    /// looked healthy, and never heard another word from any conversation. A Gateway
    /// restart is ordinary; this made every one of them silently final.
    watching: Mutex<std::collections::BTreeSet<String>>,
    connection_notice: Mutex<Option<String>>,
    connection_state: AtomicU64,
    reconnect_paused: AtomicBool,
    running: AtomicBool,
}

#[derive(Clone)]
pub struct GatewayClient {
    inner: Arc<GatewayClientInner>,
}

impl GatewayClient {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(GatewayClientInner {
                config: Mutex::new(None),
                config_generation: AtomicU64::new(0),
                commands: Mutex::new(None),
                scopes: Mutex::new(Vec::new()),
                sessions_cache: Mutex::new(None),
                watching: Mutex::new(std::collections::BTreeSet::new()),
                agents_cache: Mutex::new(None),
                identity: Mutex::new(None),
                connection_notice: Mutex::new(None),
                connection_state: AtomicU64::new(GatewayConnectionState::Down as u64),
                reconnect_paused: AtomicBool::new(false),
                running: AtomicBool::new(false),
            }),
        }
    }

    pub fn configure(&self, app: &AppHandle, config: GatewayWsConfig) {
        *self
            .inner
            .config
            .lock()
            .expect("gateway config mutex poisoned") = Some(config);
        *self
            .inner
            .agents_cache
            .lock()
            .expect("gateway agents cache mutex poisoned") = None;
        self.inner.config_generation.fetch_add(1, Ordering::SeqCst);
        self.inner.reconnect_paused.store(false, Ordering::SeqCst);
        self.set_connection_state(app, GatewayConnectionState::Down, None);
        if let Some(commands) = self
            .inner
            .commands
            .lock()
            .expect("gateway command mutex poisoned")
            .as_ref()
        {
            let _ = commands.try_send(DriverCommand::Reconfigure);
        }
    }

    pub fn activate(&self, app: AppHandle) {
        if self.inner.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let (commands, receiver) = mpsc::channel(16);
        *self
            .inner
            .commands
            .lock()
            .expect("gateway command mutex poisoned") = Some(commands);
        let client = self.clone();
        tauri::async_runtime::spawn(async move {
            client.run_driver(app, receiver).await;
        });
    }

    /// The models this agent could answer with.
    ///
    /// Each one carries its own thinking levels, so which efforts are on offer is the
    /// model's answer rather than a list held here that would drift the first time a
    /// provider changed one.
    pub async fn models_for(&self, agent_id: Option<String>) -> Result<Vec<ModelChoice>, String> {
        match self.request(GatewayRequest::ChatMetadata { agent_id }).await? {
            GatewayResponse::Models(models) => Ok(models),
            _ => Err("The Gateway answered something else.".to_string()),
        }
    }

    /// Set the model and the effort on one conversation.
    ///
    /// Both optional and both sent only when set: a patch that carried nulls would be
    /// the toolbar clearing settings it was never asked about.
    pub async fn set_answering(
        &self,
        key: &str,
        agent_id: Option<String>,
        model: Option<String>,
        thinking_level: Option<String>,
    ) -> Result<(), String> {
        self.request(GatewayRequest::SessionsPatch {
            key: key.to_string(),
            agent_id,
            model,
            thinking_level,
        })
        .await
        .map(|_| ())
    }

    pub async fn agents_list(&self) -> Result<AgentsListResult, String> {
        if !self.is_connected() {
            return Err("Gateway unreachable — retrying".to_string());
        }
        let cached = {
            self.inner
                .agents_cache
                .lock()
                .map_err(|_| "Gateway agent cache is unavailable.".to_string())?
                .as_ref()
                .filter(|cached| cached.fetched_at.elapsed() < AGENTS_CACHE_TTL)
                .map(|cached| cached.result.clone())
        };
        if let Some(result) = cached {
            return Ok(result);
        }
        let response = self.request(GatewayRequest::AgentsList).await?;
        let GatewayResponse::AgentsList(result) = response else {
            return Err("Gateway returned the wrong response for agents.list.".to_string());
        };
        self.cache_agents(result.clone());
        Ok(result)
    }

    /// The conversations somebody could hand a region to.
    ///
    /// Uncached, unlike the agent list: agents are configuration and change when
    /// somebody edits them, while a session's title and status change as it runs, and a
    /// picker showing a minute-old answer would be worse than one that waits.
    pub async fn sessions_list(&self) -> Result<SessionsListResult, String> {
        if !self.is_connected() {
            return Err("Gateway unreachable — retrying".to_string());
        }
        // Answered from the last one if it is younger than a blink. See
        // `SESSIONS_ARE_FRESH_FOR`: the two callers that want this want it together, and
        // asking twice cost a duplicate of the largest message the toolbar receives.
        if let Ok(held) = self.inner.sessions_cache.lock() {
            if let Some(cached) = held.as_ref() {
                if cached.fetched_at.elapsed() < SESSIONS_ARE_FRESH_FOR {
                    return Ok(cached.result.clone());
                }
            }
        }
        let response = self.request(GatewayRequest::SessionsList).await?;
        let GatewayResponse::SessionsList(result) = response else {
            return Err("Gateway returned the wrong response for sessions.list.".to_string());
        };
        if let Ok(mut held) = self.inner.sessions_cache.lock() {
            *held = Some(CachedSessions {
                fetched_at: Instant::now(),
                result: result.clone(),
            });
        }
        Ok(result)
    }

    /// The conversations somebody is holding in another agent altogether.
    pub async fn sessions_catalog_list(&self) -> Result<SessionsCatalogListResult, String> {
        if !self.is_connected() {
            return Err("Gateway unreachable — retrying".to_string());
        }
        let response = self.request(GatewayRequest::SessionsCatalogList).await?;
        let GatewayResponse::SessionsCatalogList(result) = response else {
            return Err(
                "Gateway returned the wrong response for sessions.catalog.list.".to_string(),
            );
        };
        Ok(result)
    }

    /// Ask an external agent to open a new conversation in a directory.
    ///
    /// The other half of routing: when what is in front has no conversation worth
    /// joining, the answer is a new one where the work is, rather than an agent that
    /// has to be told where the work is.
    /// The points in a conversation somebody could go back to.
    pub async fn chat_history(&self, key: &str, limit: u32) -> Result<Vec<Point>, String> {
        match self
            .request(GatewayRequest::ChatHistory {
                key: key.to_string(),
                limit,
            })
            .await?
        {
            GatewayResponse::History(points) => Ok(points),
            _ => Err("The Gateway answered something else.".to_string()),
        }
    }

    /// Call one of the agent's own tools and hand back what it said.
    ///
    /// The toolbar has no credentials of its own and wants none: whatever library is
    /// connected is connected to the agent, and this borrows it rather than duplicating
    /// it. `tools.invoke` refusing with `not_found` is how the toolbar learns nothing is
    /// connected, which is a fact worth having rather than an error to report.
    pub async fn invoke_tool(
        &self,
        name: &str,
        args: Value,
        agent_id: Option<String>,
        session_key: Option<String>,
    ) -> Result<ToolAnswer, String> {
        let GatewayResponse::ToolOutput(answer) = self
            .request(GatewayRequest::ToolsInvoke {
                name: name.to_string(),
                args,
                agent_id,
                session_key,
            })
            .await?
        else {
            return Err("Gateway returned the wrong response for tools.invoke.".to_string());
        };
        Ok(answer)
    }

    /// How many things are waiting on a person, across every agent.
    pub async fn approvals_pending(&self) -> Result<u32, String> {
        let GatewayResponse::Pending(count) =
            self.request(GatewayRequest::ApprovalsPending).await?
        else {
            return Err("Gateway returned the wrong response for exec.approval.list.".to_string());
        };
        Ok(count)
    }

    /// Cut a conversation back to one of its own prompts.
    pub async fn sessions_rewind(&self, key: &str, entry_id: &str) -> Result<Rewound, String> {
        match self
            .request(GatewayRequest::SessionsRewind {
                key: key.to_string(),
                entry_id: entry_id.to_string(),
            })
            .await?
        {
            GatewayResponse::Rewound(back) => Ok(back),
            _ => Err("The Gateway answered something else.".to_string()),
        }
    }

    /// Stop a run that is underway.
    ///
    /// The one thing this toolbar does that destroys work rather than describing it, so
    /// what it stopped is said out loud rather than assumed — the caller reports it.
    pub async fn chat_abort(&self, key: &str) -> Result<(), String> {
        self.request(GatewayRequest::ChatAbort {
            key: key.to_string(),
        })
        .await
        .map(|_| ())
    }

    /// Make an automation: the same request, on a schedule the Gateway keeps.
    pub async fn cron_add(&self, asked: CronAdd) -> Result<CronAdded, String> {
        match self.request(GatewayRequest::CronAdd(asked)).await? {
            GatewayResponse::CronAdd(made) => Ok(made),
            _ => Err("The Gateway answered something else.".to_string()),
        }
    }

    pub async fn start_here(&self, asked: StartHere) -> Result<(), String> {
        self.request(GatewayRequest::StartHere(asked))
            .await
            .map(|_| ())
    }

    /// Hear what a session says from now on, or stop hearing it.
    ///
    /// A subscription is a thing the server holds open, so letting go is not optional
    /// housekeeping: an overlay that is put away while still listening leaves the
    /// Gateway talking to nobody.
    pub async fn watch_session(&self, key: &str, watching: bool) -> Result<(), String> {
        self.request(GatewayRequest::WatchSession {
            key: key.to_string(),
            watching,
        })
        .await
        .map(|_| ())?;
        // Written down only once the Gateway has agreed, so the set says what is actually
        // subscribed rather than what was asked for.
        if let Ok(mut held) = self.inner.watching.lock() {
            if watching {
                held.insert(key.to_string());
            } else {
                held.remove(key);
            }
        }
        Ok(())
    }

    /// The sessions a fresh connection has to be told about again.
    fn watched_sessions(&self) -> Vec<String> {
        self.inner
            .watching
            .lock()
            .map(|held| held.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Adopt a conversation held elsewhere, and learn the session key it now answers to.
    ///
    /// This is a real handover: the thread stops being something a terminal drives and
    /// becomes a Gateway session. Whoever calls this has already said so out loud.
    pub async fn catalog_continue(&self, locator: ThreadLocator) -> Result<String, String> {
        let response = self
            .request(GatewayRequest::SessionsCatalogContinue(locator))
            .await?;
        let GatewayResponse::SessionsCatalogContinue(result) = response else {
            return Err(
                "Gateway returned the wrong response for sessions.catalog.continue.".to_string(),
            );
        };
        Ok(result.session_key)
    }

    /// Where a message to this agent should land, by the app's own routing rules.
    pub async fn target_for_agent(&self, agent_id: &str) -> Result<ChatRoutingTarget, String> {
        let catalog = self.agents_list().await?;
        Ok(routing_target(&catalog.scope, agent_id, &catalog.main_key))
    }

    /// Send to a session key that is already known, with pictures attached.
    ///
    /// Beside `chat_send` rather than replacing it: Quick Chat picks an agent and lets
    /// routing work the key out, while the toolbar has resolved a receiver to an exact
    /// session and must not have that decision made again underneath it.
    pub async fn chat_send_to(
        &self,
        target: ChatRoutingTarget,
        message: String,
        attachments: Vec<ChatAttachment>,
        idempotency_key: &str,
    ) -> Result<ChatSendResult, String> {
        let response = self
            .request(GatewayRequest::ChatSend(ChatSendParams {
                session_key: target.session_key.clone(),
                agent_id: target.agent_id.clone(),
                message,
                idempotency_key: idempotency_key.to_string(),
                attachments,
            }))
            .await?;
        let GatewayResponse::ChatSend(ack) = response else {
            return Err("Gateway returned the wrong response for chat.send.".to_string());
        };
        classify_chat_ack(&ack)?;
        Ok(ChatSendResult {
            target,
            run_id: ack.run_id,
        })
    }

    pub fn resume_reconnect(&self) {
        if let Some(commands) = self
            .inner
            .commands
            .lock()
            .expect("gateway command mutex poisoned")
            .as_ref()
        {
            let _ = commands.try_send(DriverCommand::Reconfigure);
        }
    }

    pub fn resume_paused_reconnect(&self) {
        if self.inner.reconnect_paused.load(Ordering::SeqCst) {
            self.resume_reconnect();
        }
    }


    async fn request(&self, request: GatewayRequest) -> Result<GatewayResponse, String> {
        self.request_with_budget(request, None).await
    }

    async fn request_with_budget(
        &self,
        request: GatewayRequest,
        budget: Option<Duration>,
    ) -> Result<GatewayResponse, String> {
        if !self.is_connected() {
            return Err("Gateway unreachable — retrying".to_string());
        }
        let commands = self
            .inner
            .commands
            .lock()
            .map_err(|_| "Gateway command queue is unavailable.".to_string())?
            .clone()
            .ok_or_else(|| "Gateway unreachable — retrying".to_string())?;
        let (reply, response) = oneshot::channel();
        commands
            .send(DriverCommand::Request {
                request,
                budget,
                reply,
            })
            .await
            .map_err(|_| "Gateway unreachable — retrying".to_string())?;
        tokio::time::timeout(COMMAND_TIMEOUT, response)
            .await
            .map_err(|_| "Gateway request timed out.".to_string())?
            .map_err(|_| "Gateway connection closed before the request completed.".to_string())?
    }

    async fn run_driver(&self, app: AppHandle, mut receiver: mpsc::Receiver<DriverCommand>) {
        let mut reconnect_attempt = 0_u32;
        loop {
            if !gateway_surface_open(&app) {
                self.inner.reconnect_paused.store(false, Ordering::SeqCst);
                self.set_connection_state(&app, GatewayConnectionState::Down, None);
                tokio::time::sleep(DRIVER_TICK).await;
                reconnect_attempt = 0;
                continue;
            }
            let config = self
                .inner
                .config
                .lock()
                .expect("gateway config mutex poisoned")
                .clone();
            let Some(config) = config else {
                self.inner.reconnect_paused.store(false, Ordering::SeqCst);
                self.set_connection_state(&app, GatewayConnectionState::Down, None);
                tokio::time::sleep(DRIVER_TICK).await;
                continue;
            };
            while let Ok(command) = receiver.try_recv() {
                reject_disconnected_command(command);
            }
            let generation = self.inner.config_generation.load(Ordering::SeqCst);
            let connection_result = self
                .connect_and_serve(&app, &config, generation, &mut receiver)
                .await;
            let reached_hello = self.is_connected();
            let failure = connection_result.as_ref().err();
            let disconnected_state = failure
                .and_then(|failure| failure.connect_state)
                .or_else(|| {
                    failure
                        .is_some_and(|failure| failure.tls_failure)
                        .then_some(GatewayConnectionState::TlsFailure)
                })
                .unwrap_or(GatewayConnectionState::Down);
            let pause_reconnect = failure
                .map(|failure| should_pause_reconnect(&failure.connect_details))
                .unwrap_or(false);
            let notice = failure.and_then(|failure| {
                connection_notice(
                    disconnected_state,
                    &failure.connect_details,
                    pause_reconnect,
                )
            });
            self.inner
                .reconnect_paused
                .store(pause_reconnect, Ordering::SeqCst);
            self.set_connection_state(&app, disconnected_state, notice);
            if pause_reconnect {
                // Server retry policy is authoritative: explicit pauseReconnect or retryable=false
                // waits for a fresh user summon instead of burning the capped backoff loop.
                loop {
                    let Some(command) = receiver.recv().await else {
                        return;
                    };
                    match command {
                        DriverCommand::Reconfigure => break,
                        command => reject_disconnected_command(command),
                    }
                }
                self.inner.reconnect_paused.store(false, Ordering::SeqCst);
                reconnect_attempt = 0;
                continue;
            }
            reconnect_attempt = if reached_hello {
                1
            } else {
                reconnect_attempt.saturating_add(1)
            };
            if connection_result.is_ok() {
                reconnect_attempt = 1;
            }
            if !gateway_surface_open(&app) {
                continue;
            }
            let delay = reconnect_backoff(reconnect_attempt);
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                command = receiver.recv() => {
                    if let Some(command) = command {
                        reject_disconnected_command(command);
                    }
                }
            }
        }
    }

    async fn connect_and_serve(
        &self,
        app: &AppHandle,
        config: &GatewayWsConfig,
        generation: u64,
        receiver: &mut mpsc::Receiver<DriverCommand>,
    ) -> Result<(), RequestFailure> {
        let (identity, auth) = self.identity_and_auth(app, config)?;
        let mut socket = tokio::time::timeout(CONNECT_TIMEOUT, connect_gateway_socket(config))
            .await
            .map_err(|_| RequestFailure::transport("Gateway connection timed out."))??;
        let challenge = wait_for_connect_challenge(&mut socket).await?;
        // Native child WebViews use platform HTTP trust and cannot bind the optional
        // WebSocket leaf pin, so pinned Gateway connections remain capability-free.
        let inline_widgets_available = config
            .tls_fingerprint
            .as_deref()
            .is_none_or(|value| value.trim().is_empty());
        let params = connect_params(
            &identity,
            &auth,
            &challenge.nonce,
            challenge.issued_at_ms,
            inline_widgets_available,
        )
        .map_err(RequestFailure::transport)?;
        let config_changed = AtomicBool::new(false);
        let dispatch = |frame: &Value| {
            dispatch_session_message(app, frame);
            dispatch_tool_start(app, frame);
            if frame.get("type").and_then(Value::as_str) == Some("event")
                && frame.get("event").and_then(Value::as_str) == Some("config.changed")
            {
                config_changed.store(true, Ordering::SeqCst);
            }
        };
        let hello =
            match request_on_socket(&mut socket, "connect", params, REQUEST_TIMEOUT, &dispatch)
                .await
            {
                Ok(hello) => hello,
                Err(failure) => {
                    let failure = failure.classify_connect(&auth);
                    if should_clear_stored_device_token(&failure, &auth) {
                        self.clear_device_token(&config.ws_url)?;
                    }
                    return Err(failure);
                }
            };
        drop(auth);
        let hello = validate_hello(hello).map_err(RequestFailure::transport)?;
        if let Some(device_token) = hello.device_token.as_deref() {
            self.persist_device_token(&config.ws_url, device_token)?;
        }
        if let Ok(mut held) = self.inner.scopes.lock() {
            *held = hello.scopes.clone();
        }
        let agents = request_agents_list(&mut socket, REQUEST_TIMEOUT, &dispatch).await?;
        if self.inner.config_generation.load(Ordering::SeqCst) != generation {
            return Ok(());
        }
        self.cache_agents(agents);
        // Every subscription this toolbar had went down with the last socket. Re-made
        // here, before anyone is told the connection is up, so there is no window in
        // which the page believes it is listening and is not.
        //
        // Failures are not fatal: a session that has since been deleted must not stop the
        // rest from being restored, and one that cannot be re-watched is no worse off than
        // it was a moment ago.
        for key in self.watched_sessions() {
            let asked = request_on_socket(
                &mut socket,
                "sessions.messages.subscribe",
                json!({ "key": key }),
                REQUEST_TIMEOUT,
                &dispatch,
            )
            .await;
            if asked.is_err() {
                if let Ok(mut held) = self.inner.watching.lock() {
                    held.remove(&key);
                }
            }
        }
        /*
         * And the session events, which is a second subscription and not the same one.
         *
         * Messages and events are two audiences on this Gateway, and tool activity is
         * addressed to whichever of them the run thinks is watching: to message
         * subscribers as `agent` when nothing else is looking, and to event subscribers
         * as `session.tool` when a Control UI is. A run started from this toolbar counts
         * as the second case — `controlUiVisible` defaults to true and nothing here says
         * otherwise — so subscribing only to messages meant the pill beside the crab
         * could say that work was happening and never once say what it was.
         *
         * Asked for once and unscoped: the audience is the connection, not a session, and
         * the toolbar wants every conversation for the same reason the light does — the
         * agent worth being told about is usually the one you are not looking at.
         *
         * A failure is not fatal. It costs the line beside the crab, not the toolbar.
         */
        if request_on_socket(
            &mut socket,
            "sessions.subscribe",
            json!({}),
            REQUEST_TIMEOUT,
            &dispatch,
        )
        .await
        .is_err()
        {
            eprintln!("[colai] the Gateway would not send session events; the toolbar can say that an agent is working but not what it is doing.");
        }
        self.set_connection_state(app, GatewayConnectionState::Up, None);
        let mut last_gateway_activity = Instant::now();

        loop {
            if self.inner.config_generation.load(Ordering::SeqCst) != generation
                || !gateway_surface_open(app)
            {
                return Ok(());
            }
            if config_changed.swap(false, Ordering::SeqCst) {
                // The Gateway's config moved under us. Nothing here reads it any more —
                // the accent is the page's own CSS and the canvas surface is gone — but
                // the flag still has to be cleared, and the moment is still activity.
                last_gateway_activity = Instant::now();
            }
            tokio::select! {
                command = receiver.recv() => {
                    let Some(command) = command else {
                        return Ok(());
                    };
                    match command {
                        DriverCommand::Reconfigure => return Ok(()),
                        DriverCommand::Request { request, budget, reply } => {
                            let result = perform_request(&mut socket, request, budget, &dispatch).await;
                            last_gateway_activity = Instant::now();
                            match result {
                                Ok(response) => {
                                    let _ = reply.send(Ok(response));
                                }
                                Err(failure) => {
                                    let disconnect = failure.disconnect;
                                    let message = failure.message;
                                    let _ = reply.send(Err(message.clone()));
                                    if disconnect {
                                        return Err(RequestFailure::transport(message));
                                    }
                                }
                            }
                        }
                    }
                }
                incoming = socket.next() => {
                    handle_idle_message(&dispatch, &mut socket, incoming).await?;
                    last_gateway_activity = Instant::now();
                }
                _ = tokio::time::sleep(DRIVER_TICK) => {
                    // hello-ok owns the heartbeat cadence. Reconnect after two missed ticks so a
                    // half-open transport cannot leave Quick Chat showing a false connected state.
                    if last_gateway_activity.elapsed() > hello.tick_watch_timeout {
                        return Err(RequestFailure::transport("Gateway tick timeout."));
                    }
                }
            }
        }
    }

    fn identity_and_auth(
        &self,
        app: &AppHandle,
        config: &GatewayWsConfig,
    ) -> Result<(GatewayDeviceIdentity, GatewayAuth), RequestFailure> {
        let mut store =
            self.inner.identity.lock().map_err(|_| {
                RequestFailure::transport("Gateway device identity is unavailable.")
            })?;
        if store.is_none() {
            let path = app
                .path()
                .app_config_dir()
                .map_err(|error| {
                    RequestFailure::transport(format!(
                        "Could not resolve Gateway device identity path: {error}"
                    ))
                })?
                .join(GATEWAY_DEVICE_IDENTITY_FILE);
            *store = Some(
                GatewayDeviceIdentityStore::load_or_create(path)
                    .map_err(RequestFailure::transport)?,
            );
        }
        let store = store.as_ref().expect("gateway identity initialized");
        Ok((
            store.identity(),
            store.select_auth(
                &config.ws_url,
                config.token.as_deref(),
                config.password.as_deref(),
            ),
        ))
    }

    fn persist_device_token(
        &self,
        gateway: &str,
        device_token: &str,
    ) -> Result<(), RequestFailure> {
        let mut store =
            self.inner.identity.lock().map_err(|_| {
                RequestFailure::transport("Gateway device identity is unavailable.")
            })?;
        store
            .as_mut()
            .ok_or_else(|| RequestFailure::transport("Gateway device identity is unavailable."))?
            .persist_device_token(gateway, device_token)
            .map_err(RequestFailure::transport)
    }

    fn clear_device_token(&self, gateway: &str) -> Result<(), RequestFailure> {
        let mut store =
            self.inner.identity.lock().map_err(|_| {
                RequestFailure::transport("Gateway device identity is unavailable.")
            })?;
        store
            .as_mut()
            .ok_or_else(|| RequestFailure::transport("Gateway device identity is unavailable."))?
            .clear_device_token(gateway)
            .map_err(RequestFailure::transport)
    }

    fn cache_agents(&self, result: AgentsListResult) {
        *self
            .inner
            .agents_cache
            .lock()
            .expect("gateway agents cache mutex poisoned") = Some(CachedAgents {
            fetched_at: Instant::now(),
            result,
        });
    }

    /// What this connection is allowed to do, as the Gateway last reported it.
    ///
    /// Empty before the first handshake, and empty is not "everything" — a toolbar that
    /// offered an action because it had not heard yet would offer one that fails.
    pub fn scopes(&self) -> Vec<String> {
        self.inner
            .scopes
            .lock()
            .map(|held| held.clone())
            .unwrap_or_default()
    }

    fn is_connected(&self) -> bool {
        self.connection_state() == GatewayConnectionState::Up
    }

    fn connection_state(&self) -> GatewayConnectionState {
        GatewayConnectionState::from_u64(self.inner.connection_state.load(Ordering::SeqCst))
    }

    fn set_connection_state(
        &self,
        app: &AppHandle,
        state: GatewayConnectionState,
        notice: Option<String>,
    ) {
        if state != GatewayConnectionState::Up {
            *self
                .inner
                .agents_cache
                .lock()
                .expect("gateway agents cache mutex poisoned") = None;
        }
        let notice_changed = {
            let mut current = self
                .inner
                .connection_notice
                .lock()
                .expect("gateway connection notice mutex poisoned");
            if *current == notice {
                false
            } else {
                *current = notice.clone();
                true
            }
        };
        let state_changed = self
            .inner
            .connection_state
            .swap(state as u64, Ordering::SeqCst)
            != state as u64;
        if !state_changed && !notice_changed {
            return;
        }
        self.emit_connection_state(app, state, notice);
    }

    fn emit_connection_state(
        &self,
        app: &AppHandle,
        state: GatewayConnectionState,
        notice: Option<String>,
    ) {
        let _ = app.emit_to(
            crate::colai::OVERLAY_LABEL,
            GATEWAY_STATE_EVENT,
            GatewayStateEvent::new(state, notice),
        );
    }
}

/// What the page is told when the connection changes.
///
/// The notice is the Gateway's own words where it has any; the page has a sentence per
/// state for when it does not. Nothing else travels — this used to carry a canvas
/// surface URL and a user accent that no page ever read.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GatewayStateEvent {
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    notice: Option<String>,
}

impl GatewayStateEvent {
    fn new(state: GatewayConnectionState, notice: Option<String>) -> Self {
        Self {
            state: state.event_name(),
            notice,
        }
    }
}

fn reject_disconnected_command(command: DriverCommand) {
    if let DriverCommand::Request { reply, .. } = command {
        let _ = reply.send(Err("Gateway unreachable — retrying".to_string()));
    }
}

/// Whether an always-on-top surface that talks to the Gateway is open.
///
/// The connection is owned by the overlays, not by one of them: Quick Chat sends
/// messages through it and the toolbar asks it who can receive a region. Gating on Quick
/// Chat alone left the toolbar permanently reporting an unreachable Gateway while the
/// dashboard behind it was connected.
/// Whether there is anything on screen that would use a Gateway.
///
/// The overlay is made on the first summon and kept from then on, so this is false only
/// before the toolbar has ever been shown — a copy started by `openclaw colai hide`, for
/// instance. There is nothing to answer to until there is, and the driver idles.
///
/// This used to ask the same question twice and `||` the answers together, which was what
/// remained of the desktop app's `quickchat || colai` test after Quick Chat was left
/// behind.
fn gateway_surface_open(app: &AppHandle) -> bool {
    app.get_webview_window(crate::colai::OVERLAY_LABEL).is_some()
}

fn routing_target(scope: &str, selected_agent_id: &str, main_key: &str) -> ChatRoutingTarget {
    if scope.trim().eq_ignore_ascii_case("global") {
        ChatRoutingTarget {
            session_key: "global".to_string(),
            agent_id: Some(selected_agent_id.to_string()),
        }
    } else {
        ChatRoutingTarget {
            session_key: format!("agent:{selected_agent_id}:{main_key}"),
            // Canonical agent keys already encode ownership; a redundant agentId is rejected.
            agent_id: None,
        }
    }
}

fn connect_detail_text(value: Option<&Value>, max_chars: usize) -> Option<String> {
    let normalized = value
        .and_then(Value::as_str)?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        return None;
    }
    Some(normalized.chars().take(max_chars).collect())
}

fn classify_connect_failure(
    detail_code: Option<&str>,
    has_local_credential: bool,
) -> Option<GatewayConnectionState> {
    if detail_code == Some(PAIRING_REQUIRED_DETAIL_CODE) {
        return Some(GatewayConnectionState::PairingRequired);
    }
    // A retained device token can fail because the Gateway now requires shared credentials.
    // Mismatch errors remain credential-aware so configured auth keeps its existing recovery path.
    let credential_required = detail_code.is_some_and(|code| {
        code == AUTH_TOKEN_MISSING_DETAIL_CODE
            || code == AUTH_PASSWORD_MISSING_DETAIL_CODE
            || (!has_local_credential && code.starts_with("AUTH_") && code.ends_with("_MISMATCH"))
    });
    credential_required.then_some(GatewayConnectionState::CredentialRequired)
}

fn should_pause_reconnect(details: &ConnectErrorDetails) -> bool {
    details.pause_reconnect == Some(true) || details.retryable == Some(false)
}

fn short_device_id(device_id: &str) -> Option<String> {
    let short = device_id
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .take(8)
        .collect::<String>();
    (!short.is_empty()).then_some(short)
}

fn connection_notice(
    state: GatewayConnectionState,
    details: &ConnectErrorDetails,
    reconnect_paused: bool,
) -> Option<String> {
    let fallback = match state {
        GatewayConnectionState::PairingRequired => "Approve this device in the dashboard (Nodes)",
        GatewayConnectionState::CredentialRequired => {
            "Gateway requires a credential — open the dashboard on the gateway host"
        }
        _ if reconnect_paused => "Gateway connection paused — reopen Quick Chat to retry",
        _ => return None,
    };
    // The Gateway owns recovery semantics and can give more precise operator guidance than this
    // client. Keep only its bounded plain-text hint, then add the safe pairing identifier.
    let mut notice = details
        .remediation_hint
        .clone()
        .unwrap_or_else(|| fallback.to_string());
    if state == GatewayConnectionState::PairingRequired {
        if let Some(device_id) = details.device_id.as_deref().and_then(short_device_id) {
            notice.push_str(" · Device ");
            notice.push_str(&device_id);
        }
    }
    Some(notice)
}

fn reconnect_backoff(attempt: u32) -> Duration {
    let shift = attempt.saturating_sub(1).min(5);
    Duration::from_secs((1_u64 << shift).min(MAX_RECONNECT_DELAY.as_secs()))
}

fn should_clear_stored_device_token(failure: &RequestFailure, auth: &GatewayAuth) -> bool {
    matches!(auth, GatewayAuth::DeviceToken(_))
        && failure.connect_details.code.as_deref() == Some(AUTH_DEVICE_TOKEN_MISMATCH_DETAIL_CODE)
}

fn connect_params(
    identity: &GatewayDeviceIdentity,
    auth: &GatewayAuth,
    nonce: &str,
    signed_at_ms: u64,
    inline_widgets_available: bool,
) -> Result<Value, String> {
    let mut client_caps = vec![AGENT_KIND_CLIENT_CAPABILITY];
    if inline_widgets_available {
        client_caps.push(INLINE_WIDGETS_CLIENT_CAPABILITY);
    }
    let mut params = json!({
        "minProtocol": MIN_PROTOCOL_VERSION,
        "maxProtocol": MAX_PROTOCOL_VERSION,
        "client": {
            "id": CLIENT_ID,
            "version": env!("CARGO_PKG_VERSION"),
            "platform": CLIENT_PLATFORM,
            "mode": CLIENT_MODE,
            "deviceFamily": CLIENT_DEVICE_FAMILY
        },
        "caps": client_caps,
        "commands": [],
        "permissions": {},
        "role": CLIENT_ROLE,
        "scopes": CLIENT_SCOPES
    });
    if let Some(auth) = auth.json() {
        params["auth"] = auth;
    }
    params["device"] = identity.signed_device(auth, nonce, signed_at_ms)?;
    Ok(params)
}

fn request_frame(id: &str, method: &str, params: Value) -> Value {
    json!({
        "type": "req",
        "id": id,
        "method": method,
        "params": params
    })
}

#[derive(Debug, PartialEq, Eq)]
struct ConnectChallenge {
    nonce: String,
    issued_at_ms: u64,
}

fn parse_connect_challenge(value: &Value) -> Result<ConnectChallenge, RequestFailure> {
    let nonce = value
        .get("payload")
        .and_then(|payload| payload.get("nonce"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|nonce| !nonce.is_empty());
    let issued_at_ms = value
        .get("payload")
        .and_then(|payload| payload.get("ts"))
        .and_then(Value::as_u64)
        .ok_or_else(|| RequestFailure::transport("Gateway challenge timestamp was invalid."))?;
    nonce
        .map(|nonce| ConnectChallenge {
            nonce: nonce.to_owned(),
            issued_at_ms,
        })
        .ok_or_else(|| RequestFailure::transport("Gateway challenge omitted nonce."))
}

async fn wait_for_connect_challenge(
    socket: &mut GatewaySocket,
) -> Result<ConnectChallenge, RequestFailure> {
    tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
        loop {
            let value = next_json(socket).await?;
            if value.get("type").and_then(Value::as_str) == Some("event")
                && value.get("event").and_then(Value::as_str) == Some("connect.challenge")
            {
                return parse_connect_challenge(&value);
            }
        }
    })
    .await
    .map_err(|_| RequestFailure::transport("Gateway connect challenge timed out."))?
}

async fn request_on_socket<F>(
    socket: &mut GatewaySocket,
    method: &str,
    params: Value,
    budget: Duration,
    dispatch: &F,
) -> Result<Value, RequestFailure>
where
    F: Fn(&Value),
{
    let id = Uuid::new_v4().to_string();
    let encoded = serde_json::to_string(&request_frame(&id, method, params)).map_err(|error| {
        RequestFailure::transport(format!("Could not encode {method}: {error}"))
    })?;
    socket
        .send(Message::Text(encoded.into()))
        .await
        .map_err(|error| RequestFailure::transport(format!("Could not send {method}: {error}")))?;

    tokio::time::timeout(budget, async {
        loop {
            let value = next_json(socket).await?;
            dispatch(&value);
            if value.get("type").and_then(Value::as_str) != Some("res")
                || value.get("id").and_then(Value::as_str) != Some(id.as_str())
            {
                continue;
            }
            if value.get("ok").and_then(Value::as_bool) == Some(true) {
                return Ok(value.get("payload").cloned().unwrap_or(Value::Null));
            }
            let message = value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("Gateway request failed.");
            let details = value
                .get("error")
                .and_then(|error| error.get("details"))
                .filter(|details| details.is_object());
            return Err(RequestFailure::method_with_details(message, details));
        }
    })
    .await
    .map_err(|_| RequestFailure::transport(format!("Gateway {method} request timed out.")))?
}

async fn perform_request<F>(
    socket: &mut GatewaySocket,
    request: GatewayRequest,
    budget: Option<Duration>,
    dispatch: &F,
) -> Result<GatewayResponse, RequestFailure>
where
    F: Fn(&Value),
{
    let budget = budget.unwrap_or(REQUEST_TIMEOUT);
    match request {
        GatewayRequest::AgentsList => request_agents_list(socket, budget, dispatch)
            .await
            .map(GatewayResponse::AgentsList),
        GatewayRequest::SessionsList => request_sessions_list(socket, budget, dispatch)
            .await
            .map(GatewayResponse::SessionsList),
        GatewayRequest::SessionsCatalogList => {
            let payload = request_on_socket(
                socket,
                "sessions.catalog.list",
                json!({ "limitPerHost": THREADS_SHOWN }),
                budget,
                dispatch,
            )
            .await?;
            serde_json::from_value(payload)
                .map(GatewayResponse::SessionsCatalogList)
                .map_err(|error| {
                    RequestFailure::transport(format!(
                        "Invalid sessions.catalog.list response: {error}"
                    ))
                })
        }
        GatewayRequest::WatchSession { key, watching } => {
            let method = if watching {
                "sessions.messages.subscribe"
            } else {
                "sessions.messages.unsubscribe"
            };
            request_on_socket(socket, method, json!({ "key": key }), budget, dispatch)
                .await
                .map(|_| GatewayResponse::Done)
        }
        GatewayRequest::StartHere(asked) => {
            let params = serde_json::to_value(asked).map_err(|error| {
                RequestFailure::transport(format!(
                    "Could not encode sessions.catalog.startTerminal: {error}"
                ))
            })?;
            request_on_socket(
                socket,
                "sessions.catalog.startTerminal",
                params,
                budget,
                dispatch,
            )
            .await
            .map(|_| GatewayResponse::Done)
        }
        GatewayRequest::SessionsCatalogContinue(locator) => {
            let params = serde_json::to_value(locator).map_err(|error| {
                RequestFailure::transport(format!(
                    "Could not encode sessions.catalog.continue: {error}"
                ))
            })?;
            let payload = request_on_socket(
                socket,
                "sessions.catalog.continue",
                params,
                budget,
                dispatch,
            )
            .await?;
            serde_json::from_value(payload)
                .map(GatewayResponse::SessionsCatalogContinue)
                .map_err(|error| {
                    RequestFailure::transport(format!(
                        "Invalid sessions.catalog.continue response: {error}"
                    ))
                })
        }
        GatewayRequest::ChatMetadata { agent_id } => {
            let mut params = serde_json::Map::new();
            if let Some(agent) = agent_id {
                params.insert("agentId".to_string(), Value::from(agent.clone()));
            }
            let payload = request_on_socket(
                socket,
                "chat.metadata",
                Value::Object(params),
                budget,
                dispatch,
            )
            .await?;
            Ok(GatewayResponse::Models(models_in(&payload)))
        }
        GatewayRequest::SessionsPatch {
            key,
            agent_id,
            model,
            thinking_level,
        } => {
            let mut params = serde_json::Map::new();
            params.insert("key".to_string(), Value::from(key.clone()));
            if let Some(agent) = agent_id {
                params.insert("agentId".to_string(), Value::from(agent.clone()));
            }
            // Only what was asked for. A key present with a null is "clear this", which
            // is a different instruction from "leave it alone".
            if let Some(model) = model {
                params.insert("model".to_string(), Value::from(model.clone()));
            }
            if let Some(level) = thinking_level {
                params.insert("thinkingLevel".to_string(), Value::from(level.clone()));
            }
            request_on_socket(socket, "sessions.patch", Value::Object(params), budget, dispatch)
                .await
                .map(|_| GatewayResponse::Patched)
        }
        GatewayRequest::ChatAbort { key } => request_on_socket(
            socket,
            "chat.abort",
            serde_json::json!({ "sessionKey": key }),
            budget,
            dispatch,
        )
        .await
        .map(|_| GatewayResponse::Done),
        GatewayRequest::ChatHistory { key, limit } => {
            let payload = request_on_socket(
                socket,
                "chat.history",
                serde_json::json!({ "sessionKey": key, "limit": limit }),
                budget,
                dispatch,
            )
            .await?;
            Ok(GatewayResponse::History(points_in(&payload)))
        }
        GatewayRequest::ToolsInvoke {
            name,
            args,
            agent_id,
            session_key,
        } => {
            let mut params = serde_json::json!({ "name": name, "args": args });
            if let Some(map) = params.as_object_mut() {
                if let Some(agent) = agent_id {
                    map.insert("agentId".to_string(), Value::from(agent));
                }
                if let Some(key) = session_key {
                    map.insert("sessionKey".to_string(), Value::from(key));
                }
            }
            let payload =
                request_on_socket(socket, "tools.invoke", params, budget, dispatch).await?;
            Ok(GatewayResponse::ToolOutput(answer_in(&payload)))
        }
        GatewayRequest::ApprovalsPending => {
            let payload = request_on_socket(
                socket,
                "exec.approval.list",
                serde_json::json!({}),
                budget,
                dispatch,
            )
            .await?;
            Ok(GatewayResponse::Pending(waiting_in(&payload)))
        }
        GatewayRequest::SessionsRewind { key, entry_id } => {
            let payload = request_on_socket(
                socket,
                "sessions.rewind",
                serde_json::json!({ "sessionKey": key, "entryId": entry_id }),
                budget,
                dispatch,
            )
            .await?;
            Ok(GatewayResponse::Rewound(
                serde_json::from_value(payload).unwrap_or_default(),
            ))
        }
        GatewayRequest::CronAdd(asked) => {
            let params = serde_json::to_value(asked).map_err(|error| {
                RequestFailure::transport(format!("Could not encode cron.add: {error}"))
            })?;
            let payload = request_on_socket(socket, "cron.add", params, budget, dispatch).await?;
            // Read loosely on purpose. Creating the job is the outcome; its name and id
            // are only there so the receipt can say which one, and a shape this does not
            // recognise must not turn a job that was made into an error saying it was not.
            Ok(GatewayResponse::CronAdd(
                serde_json::from_value(payload).unwrap_or_default(),
            ))
        }
        GatewayRequest::ChatSend(params) => {
            let params = serde_json::to_value(params).map_err(|error| {
                RequestFailure::transport(format!("Could not encode chat.send: {error}"))
            })?;
            let payload = request_on_socket(socket, "chat.send", params, budget, dispatch).await?;
            serde_json::from_value(payload)
                .map(GatewayResponse::ChatSend)
                .map_err(|error| {
                    RequestFailure::transport(format!("Invalid chat.send response: {error}"))
                })
        }

    }
}

async fn request_agents_list<F>(
    socket: &mut GatewaySocket,
    budget: Duration,
    dispatch: &F,
) -> Result<AgentsListResult, RequestFailure>
where
    F: Fn(&Value),
{
    let payload = request_on_socket(socket, "agents.list", json!({}), budget, dispatch).await?;
    serde_json::from_value(payload).map_err(|error| {
        RequestFailure::transport(format!("Invalid agents.list response: {error}"))
    })
}

/// The same rows the dashboard's own session list shows.
///
/// The filters are the Control UI roster's, not a second selection: a toolbar that
/// disagreed with the window behind it about which conversations exist would be worse
/// than one that showed none. Bounded, because this runs every time the menu opens, and
/// titles are projected so a session that was never named still reads as itself.
async fn request_sessions_list<F>(
    socket: &mut GatewaySocket,
    budget: Duration,
    dispatch: &F,
) -> Result<SessionsListResult, RequestFailure>
where
    F: Fn(&Value),
{
    let payload = request_on_socket(
        socket,
        "sessions.list",
        json!({
            "limit": SESSIONS_SHOWN,
            "sortBy": "lastInteractionAt",
            "includeGlobal": true,
            "includeUnknown": true,
            "configuredAgentsOnly": true,
            "includeDerivedTitles": true,
            "includeLastMessage": true,
            "archived": false,
        }),
        budget,
        dispatch,
    )
    .await?;
    serde_json::from_value(payload).map_err(|error| {
        RequestFailure::transport(format!("Invalid sessions.list response: {error}"))
    })
}

struct ValidatedHello {
    device_token: Option<String>,
    tick_watch_timeout: Duration,
    scopes: Vec<String>,
}

impl ValidatedHello {
    fn new(device_token: Option<String>, tick_watch_timeout: Duration, scopes: Vec<String>) -> Self {
        Self {
            scopes,
            device_token,
            tick_watch_timeout,
        }
    }
}

fn validate_hello(payload: Value) -> Result<ValidatedHello, String> {
    #[derive(Deserialize)]
    struct HelloFeatures {
        methods: Vec<String>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct HelloOk {
        #[serde(rename = "type")]
        kind: String,
        protocol: u32,
        features: HelloFeatures,
        auth: HelloAuth,
        policy: Option<HelloPolicy>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct HelloAuth {
        device_token: Option<String>,
        /// What this connection is allowed to do, as the Gateway itself reports it.
        ///
        /// Read rather than assumed. Several things worth offering — rewinding a
        /// conversation among them — need a scope a desktop token may simply not have,
        /// and a menu item that always fails is worse than one that is not there. This
        /// is how the toolbar knows which of the two to show.
        #[serde(default)]
        scopes: Vec<String>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct HelloPolicy {
        tick_interval_ms: Option<u64>,
    }
    let hello: HelloOk = serde_json::from_value(payload)
        .map_err(|error| format!("Invalid Gateway hello response: {error}"))?;
    if hello.kind != "hello-ok" || hello.protocol != MAX_PROTOCOL_VERSION {
        return Err("Gateway negotiated an unsupported protocol.".to_string());
    }
    for required in ["agents.list", "chat.send"] {
        if !hello
            .features
            .methods
            .iter()
            .any(|method| method == required)
        {
            return Err(format!(
                "Gateway does not advertise required method {required}."
            ));
        }
    }
    let tick_interval_ms = hello
        .policy
        .and_then(|policy| policy.tick_interval_ms)
        .unwrap_or(30_000)
        .max(1);
    let issued_device_auth = hello.auth.device_token;
    let scopes = hello.auth.scopes;
    Ok(ValidatedHello::new(
        issued_device_auth,
        Duration::from_millis(tick_interval_ms).saturating_mul(2),
        scopes,
    ))
}

fn classify_chat_ack(ack: &ChatSendAck) -> Result<(), String> {
    match ack.status.trim().to_ascii_lowercase().as_str() {
        "ok" | "started" | "in_flight" => Ok(()),
        "error" | "timeout" => Err(ack_error_message(ack)),
        status => Err(format!(
            "Gateway returned unexpected chat.send status \"{status}\"."
        )),
    }
}

fn ack_error_message(ack: &ChatSendAck) -> String {
    ack.message
        .clone()
        .or_else(|| {
            ack.error
                .as_ref()
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            ack.error
                .as_ref()
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| format!("Gateway chat.send {}.", ack.status))
}

type GatewaySocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect_gateway_socket(config: &GatewayWsConfig) -> Result<GatewaySocket, RequestFailure> {
    let trust =
        tls_trust_decision(config.tls_fingerprint.as_deref()).map_err(RequestFailure::tls)?;
    let result = match trust {
        TlsTrustDecision::SystemRoots => connect_async(config.ws_url.as_str()).await,
        TlsTrustDecision::Pinned(expected) => {
            if !config.ws_url.starts_with("wss://") {
                return Err(RequestFailure::tls(
                    "Gateway TLS fingerprint requires a wss:// URL.",
                ));
            }
            let connector = pinned_tls_connector(expected).map_err(RequestFailure::tls)?;
            connect_async_tls_with_config(config.ws_url.as_str(), None, false, Some(connector))
                .await
        }
    };
    result
        .map(|(socket, _)| socket)
        .map_err(|error| connect_failure(config, error))
}

fn connect_failure(config: &GatewayWsConfig, error: TungsteniteError) -> RequestFailure {
    let message = format!("Gateway connection failed: {error}");
    if is_tls_connect_failure(&config.ws_url, &error) {
        RequestFailure::tls(message)
    } else {
        RequestFailure::transport(message)
    }
}

fn is_tls_connect_failure(ws_url: &str, error: &TungsteniteError) -> bool {
    if !ws_url.starts_with("wss://") {
        return false;
    }
    error.to_string().contains(TLS_PIN_MISMATCH_ERROR)
        || matches!(error, TungsteniteError::Tls(_))
        || matches!(error, TungsteniteError::Io(io_error) if io_error.kind() == ErrorKind::InvalidData)
}

async fn next_json(socket: &mut GatewaySocket) -> Result<Value, RequestFailure> {
    loop {
        let message = socket
            .next()
            .await
            .ok_or_else(|| RequestFailure::transport("Gateway connection closed."))?
            .map_err(|error| {
                RequestFailure::transport(format!("Gateway connection failed: {error}"))
            })?;
        match message {
            Message::Text(text) => {
                return serde_json::from_str(text.as_ref()).map_err(|error| {
                    RequestFailure::transport(format!("Gateway sent invalid JSON: {error}"))
                });
            }
            Message::Ping(payload) => {
                socket.send(Message::Pong(payload)).await.map_err(|error| {
                    RequestFailure::transport(format!("Could not answer Gateway ping: {error}"))
                })?
            }
            Message::Close(_) => {
                return Err(RequestFailure::transport("Gateway connection closed."));
            }
            _ => {}
        }
    }
}

async fn handle_idle_message<F>(
    dispatch: &F,
    socket: &mut GatewaySocket,
    incoming: Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
) -> Result<(), RequestFailure>
where
    F: Fn(&Value),
{
    let message = incoming
        .ok_or_else(|| RequestFailure::transport("Gateway connection closed."))?
        .map_err(|error| {
            RequestFailure::transport(format!("Gateway connection failed: {error}"))
        })?;
    match message {
        Message::Text(text) => {
            if let Ok(value) = serde_json::from_str::<Value>(text.as_ref()) {
                dispatch(&value);
            }
            Ok(())
        }
        Message::Ping(payload) => socket.send(Message::Pong(payload)).await.map_err(|error| {
            RequestFailure::transport(format!("Could not answer Gateway ping: {error}"))
        }),
        Message::Close(_) => Err(RequestFailure::transport("Gateway connection closed.")),
        _ => Ok(()),
    }
}

/// A message from a session the toolbar is watching, sent to the overlay.
///
/// The same pushed frames Quick Chat reads, filtered differently. Emitted raw, because
/// deciding which reply belongs to which mark is the page's business and it already
/// knows what it sent where.
fn dispatch_session_message<R: tauri::Runtime>(app: &AppHandle<R>, frame: &Value) {
    if frame.get("type").and_then(Value::as_str) != Some("event")
        || frame.get("event").and_then(Value::as_str) != Some("session.message")
    {
        return;
    }
    if let Some(payload) = frame.get("payload") {
        let _ = app.emit_to(crate::colai::OVERLAY_LABEL, REPLY_EVENT, payload.clone());
    }
}

/// A tool the agent has just picked up, sent to the overlay so it can say so.
///
/// Two event names for one fact, because which one arrives depends on something the
/// toolbar has no say in. The Gateway sends tool lifecycle to session-message
/// subscribers — which is what the toolbar is — under `agent`, but only while no Control
/// UI is watching that session; when one is, the same payload goes to session-event
/// subscribers under `session.tool` instead. Listening for one of them would mean a
/// toolbar that goes quiet whenever somebody happens to have the dashboard open.
///
/// Only the start of a call. A result says a thing is finished, and a line that named
/// what just stopped happening would be a status one step behind the work.
///
/// Sent narrow rather than raw, unlike a reply: the page needs the tool's name and the
/// arguments it was called with, and a tool result can carry a whole file in it.
fn worth_saying(frame: &Value) -> bool {
    if frame.get("type").and_then(Value::as_str) != Some("event") {
        return false;
    }
    if !matches!(
        frame.get("event").and_then(Value::as_str),
        Some("agent") | Some("session.tool")
    ) {
        return false;
    }
    let Some(payload) = frame.get("payload") else {
        return false;
    };
    if payload.get("stream").and_then(Value::as_str) != Some("tool") {
        return false;
    }
    let Some(data) = payload.get("data") else {
        return false;
    };
    data.get("phase").and_then(Value::as_str) == Some("start")
        && data.get("name").and_then(Value::as_str).is_some()
}

fn dispatch_tool_start<R: tauri::Runtime>(app: &AppHandle<R>, frame: &Value) {
    if !worth_saying(frame) {
        return;
    }
    let payload = frame.get("payload").expect("worth_saying checked the payload");
    let data = payload.get("data").expect("worth_saying checked the data");
    let name = data
        .get("name")
        .and_then(Value::as_str)
        .expect("worth_saying checked the name");
    let said = serde_json::json!({
        "sessionKey": payload.get("sessionKey").and_then(Value::as_str),
        "name": name,
        "args": data.get("args").cloned().unwrap_or(Value::Null),
    });
    let _ = app.emit_to(crate::colai::OVERLAY_LABEL, DOING_EVENT, said);
}


#[cfg(test)]
mod tests {
    use super::*;

    /// What counts as news about a tool, and what does not.
    ///
    /// The rules rather than the emit, which needs an app handle and a window. Every one
    /// of these was a way for the pill to be wrong: the wrong event name and it hears
    /// nothing while a dashboard is open, the wrong phase and it names what has just
    /// stopped happening, and a payload with no name is a line that says "Using".
    #[test]
    fn only_a_tool_being_picked_up_is_news() {
        let says = |event: &str, stream: &str, phase: &str, name: Option<&str>| {
            let frame = serde_json::json!({
                "type": "event",
                "event": event,
                "payload": {
                    "stream": stream,
                    "sessionKey": "agent:main:main",
                    "data": { "phase": phase, "name": name, "args": { "file_path": "a.css" } },
                },
            });
            worth_saying(&frame)
        };

        // Both names, because which one arrives is the Gateway's business: tool lifecycle
        // reaches session-message subscribers as `agent`, and session-event subscribers as
        // `session.tool` when a Control UI is watching the same conversation.
        assert!(says("agent", "tool", "start", Some("read")));
        assert!(says("session.tool", "tool", "start", Some("read")));

        assert!(!says("agent", "text", "start", Some("read")), "not every stream is a tool");
        assert!(!says("agent", "tool", "result", Some("read")), "a result is a thing finished");
        assert!(!says("agent", "tool", "start", None), "a call with no name says nothing");
        assert!(!says("chat", "tool", "start", Some("read")), "some other event entirely");
    }

    /// Two callers asking in the same breath is one question, not a cache.
    ///
    /// The guard is on the size of the window rather than on the behaviour, because the
    /// behaviour needs a Gateway. It is the number that matters: this exists to notice
    /// that `loadWork` and `colai_at_work` want the same list microseconds apart, and
    /// anything approaching the five-second refresh would stop being that and start being
    /// a stale conversation list shown to somebody watching an agent work.
    #[test]
    fn the_sessions_window_is_a_blink_not_a_cache() {
        assert!(
            SESSIONS_ARE_FRESH_FOR < Duration::from_secs(1),
            "long enough to serve a stale list to the panel that refreshes every 5s"
        );
        assert!(
            SESSIONS_ARE_FRESH_FOR >= Duration::from_millis(100),
            "too short to catch the second of two calls made together"
        );
    }

    /// A subscription belongs to a socket, so a new socket has to be told again.
    ///
    /// This is the half that can be tested without a Gateway: that the client remembers
    /// what it is listening to, forgets what it has let go of, and hands a fresh
    /// connection the list to re-make. `connect_and_serve` replays exactly this list.
    #[test]
    fn what_is_watched_survives_the_socket_that_carried_it() {
        let client = GatewayClient::new();
        assert!(
            client.watched_sessions().is_empty(),
            "nothing is watched before anything is watched"
        );

        // Recorded the way `watch_session` records it once the Gateway has agreed.
        for key in ["agent:main:one", "agent:main:two"] {
            client
                .inner
                .watching
                .lock()
                .expect("watch set")
                .insert(key.to_string());
        }
        assert_eq!(
            client.watched_sessions(),
            vec!["agent:main:one".to_string(), "agent:main:two".to_string()],
            "both are handed to the next connection"
        );

        client
            .inner
            .watching
            .lock()
            .expect("watch set")
            .remove("agent:main:one");
        assert_eq!(
            client.watched_sessions(),
            vec!["agent:main:two".to_string()],
            "letting go of one does not resurrect it on the next connect"
        );
    }

    #[tokio::test]
    async fn budgeted_driver_request_releases_the_serial_queue() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind websocket fixture");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept websocket fixture");
            let mut socket = tokio_tungstenite::accept_async(stream)
                .await
                .expect("accept websocket handshake");
            let _request = socket.next().await.expect("request frame");
            std::future::pending::<()>().await;
        });
        let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}"))
            .await
            .expect("connect websocket fixture");
        let (commands, mut receiver) = mpsc::channel(2);
        let (reply, response) = oneshot::channel();
        commands
            .send(DriverCommand::Request {
                request: GatewayRequest::AgentsList,
                // Any budget short enough that the test does not sit through the
                // default fifteen seconds. It borrowed the suspend timeout, which was
                // named for a thing this build does not do.
                budget: Some(Duration::from_secs(3)),
                reply,
            })
            .await
            .expect("queue budgeted request");
        commands
            .send(DriverCommand::Reconfigure)
            .await
            .expect("queue reconnect");

        let started = Instant::now();
        let command = receiver.recv().await.expect("budgeted request");
        let DriverCommand::Request {
            request,
            budget,
            reply,
        } = command
        else {
            panic!("expected request command");
        };
        let failure = match perform_request(&mut socket, request, budget, &|_| {}).await {
            Ok(_) => panic!("hung request should time out"),
            Err(failure) => failure,
        };
        let elapsed = started.elapsed();
        assert!(failure.disconnect, "timeout must recycle the socket");
        let _ = reply.send(Err(failure.message));

        assert!(matches!(
            tokio::time::timeout(Duration::from_millis(250), receiver.recv())
                .await
                .expect("serial queue remained blocked"),
            Some(DriverCommand::Reconfigure)
        ));
        assert!(
            elapsed >= Duration::from_millis(2_750),
            "elapsed: {elapsed:?}"
        );
        assert!(elapsed < Duration::from_secs(4), "elapsed: {elapsed:?}");
        let reply = response.await.expect("driver reply");
        match reply {
            Ok(_) => panic!("expected timeout reply"),
            Err(error) => assert!(error.contains("agents.list request timed out")),
        }
        server.abort();
    }

    #[test]
    fn routing_matches_macos_quick_chat_contract() {
        assert_eq!(
            routing_target("global", "work", "main"),
            ChatRoutingTarget {
                session_key: "global".to_string(),
                agent_id: Some("work".to_string()),
            }
        );
        assert_eq!(
            routing_target("per-sender", "work", "main"),
            ChatRoutingTarget {
                session_key: "agent:work:main".to_string(),
                agent_id: None,
            }
        );
        assert_eq!(
            serde_json::to_value(routing_target("global", "work", "main"))
                .expect("serialized routing target"),
            json!({ "sessionKey": "global", "agentId": "work" })
        );
    }

    #[test]
    fn agents_list_result_uses_gateway_routing_and_render_fields() {
        let result = serde_json::from_value::<AgentsListResult>(json!({
            "defaultId": "main",
            "mainKey": "main",
            "scope": "per-sender",
            "agents": [{
                "id": "main",
                "name": "Main",
                "identity": {
                    "name": "Molty",
                    "emoji": "🦞",
                    "avatarUrl": "data:image/png;base64,AA=="
                }
            }]
        }))
        .expect("agents.list result");

        assert_eq!(result.default_id, "main");
        assert_eq!(result.main_key, "main");
        assert_eq!(result.scope, "per-sender");
    }

    #[test]
    fn chat_ack_acceptance_is_explicit() {
        for status in ["ok", "started", "in_flight"] {
            assert!(classify_chat_ack(&ChatSendAck {
                run_id: "run-1".to_string(),
                status: status.to_string(),
                error: None,
                message: None,
            })
            .is_ok());
        }
        for status in ["error", "timeout", "queued"] {
            assert!(classify_chat_ack(&ChatSendAck {
                run_id: "run-1".to_string(),
                status: status.to_string(),
                error: Some(json!({ "message": "not accepted" })),
                message: None,
            })
            .is_err());
        }
    }

    #[test]
    fn tls_trust_decision_uses_system_roots_or_an_exact_pin() {
        assert_eq!(
            tls_trust_decision(None).expect("system trust"),
            TlsTrustDecision::SystemRoots
        );
        assert_eq!(
            tls_trust_decision(Some(&"ab".repeat(32))).expect("pinned trust"),
            TlsTrustDecision::Pinned([0xab; 32])
        );
        assert!(tls_trust_decision(Some("sha256:abc")).is_err());

        let certificate = b"fixture gateway leaf certificate";
        let expected: [u8; 32] = Sha256::digest(certificate).into();
        assert!(pinned_fingerprint_matches(&expected, certificate));
        assert!(!pinned_fingerprint_matches(
            &expected,
            b"different gateway leaf certificate"
        ));
    }

    #[test]
    fn tls_failures_have_a_distinct_connectivity_state() {
        let tls_error = TungsteniteError::Io(std::io::Error::new(
            ErrorKind::InvalidData,
            TLS_PIN_MISMATCH_ERROR,
        ));
        assert!(is_tls_connect_failure("wss://127.0.0.1:18789", &tls_error));
        assert!(!is_tls_connect_failure("ws://127.0.0.1:18789", &tls_error));
        assert_eq!(
            GatewayConnectionState::TlsFailure.event_name(),
            "tls-failure"
        );
    }

    #[test]
    fn reconnect_backoff_is_exponential_and_capped() {
        assert_eq!(reconnect_backoff(1), Duration::from_secs(1));
        assert_eq!(reconnect_backoff(2), Duration::from_secs(2));
        assert_eq!(reconnect_backoff(5), Duration::from_secs(16));
        assert_eq!(reconnect_backoff(6), MAX_RECONNECT_DELAY);
        assert_eq!(reconnect_backoff(100), MAX_RECONNECT_DELAY);
    }

    /// One page of `chat.history` in the shape the Gateway serves it.
    ///
    /// Copied from a real transcript on this machine rather than composed: the id under
    /// `__openclaw`, the timestamp beside it, `content` as a plain string. The previous
    /// version of these tests agreed with a parser that read none of this, and both were
    /// wrong together — which is what an invented fixture buys.
    fn a_page() -> Value {
        json!({"messages": [
            {
                "role": "user",
                "content": "ok go on to the next stage",
                "timestamp": 1_788_695_901_289i64,
                "idempotencyKey": "run-1",
                "__openclaw": {
                    "senderIsOwner": true,
                    "id": "9c720ea7-a79d-46fe-845a-0552b181818d",
                    "recordTimestampMs": 1_788_695_901_361i64,
                    "seq": 161
                }
            },
            {
                "role": "assistant",
                "content": "done",
                "__openclaw": {"id": "69c5a8fb-d8c3-4ccf-8745-6d042b011aef", "seq": 162}
            }
        ]})
    }

    fn a_session(status: &str, last: i64) -> GatewaySessionSummary {
        named_session("main:1", status, last)
    }

    fn named_session(key: &str, status: &str, last: i64) -> GatewaySessionSummary {
        GatewaySessionSummary {
            key: key.to_string(),
            agent_id: None,
            label: None,
            display_name: None,
            derived_title: None,
            last_message_preview: None,
            status: Some(status.to_string()),
            unread: None,
            last_activity_at: Some(last),
            updated_at: None,
        }
    }

    #[test]
    fn work_underway_is_counted_wherever_it_is_happening() {
        let now = 1_700_000_000_000;
        let counted = at_work_of(
            &[
                a_session("running", now),
                a_session("queued", now),
                a_session("done", now),
            ],
            now,
        );
        assert_eq!(
            counted.running, 2,
            "queued work has not started but is work"
        );
        assert_eq!(counted.trouble, 0);
    }

    #[test]
    fn which_session_is_working_is_kept_not_just_how_many() {
        /*
         * The counts answer "is anything happening", which is all the rail's one light
         * needs. The Work panel asks whether *this* run has finished, and a count cannot
         * answer it — so the toolbar inferred an ending from the total reaching zero,
         * with a timeout under it for when the total was about somebody else's agent.
         * A finished agent therefore read as running for the best part of a minute.
         */
        let now = 1_700_000_000_000;
        let counted = at_work_of(
            &[
                named_session("agent:main:one", "running", now),
                named_session("agent:main:two", "queued", now),
                named_session("agent:main:three", "done", now),
                named_session("agent:main:four", "failed", now),
            ],
            now,
        );
        assert_eq!(
            counted.working,
            vec!["agent:main:one".to_string(), "agent:main:two".to_string()],
            "queued work has not started but is work, and both are named"
        );
        assert_eq!(counted.troubled, vec!["agent:main:four".to_string()]);
        // A session the Gateway lists as finished is named by neither, which is how the
        // page tells "done" from "never heard of it".
        assert!(!counted.working.iter().any(|key| key == "agent:main:three"));
        // But it is named as known, which is the difference between a run that is over
        // and one the Gateway has never heard of.
        assert!(counted.known.iter().any(|key| key == "agent:main:three"));
        assert_eq!(counted.known.len(), 4, "every listed session, whatever it is doing");
    }

    #[test]
    fn trouble_that_is_history_names_nobody() {
        // The list and the count have to agree, or the panel marks a run failed on the
        // strength of something the light has already stopped mentioning.
        let now = 1_700_000_000_000;
        let old = now - 60 * 60 * 1000;
        let counted = at_work_of(&[named_session("agent:main:one", "failed", old)], now);
        assert_eq!(counted.trouble, 0);
        assert!(counted.troubled.is_empty());
    }

    #[test]
    fn a_run_that_was_stopped_on_purpose_is_not_trouble() {
        // A red light over a deliberate act is the toolbar arguing with the person who
        // pressed stop.
        let now = 1_700_000_000_000;
        assert_eq!(at_work_of(&[a_session("killed", now)], now).trouble, 0);
    }

    #[test]
    fn a_failure_is_news_until_it_is_history() {
        let now = 1_700_000_000_000;
        assert_eq!(at_work_of(&[a_session("failed", now)], now).trouble, 1);
        assert_eq!(at_work_of(&[a_session("timeout", now)], now).trouble, 1);
        // An hour later it is simply a session, and a warning about it is a warning
        // about nothing — which is how somebody learns to stop reading the light.
        let old = now - 60 * 60 * 1000;
        assert_eq!(at_work_of(&[a_session("failed", old)], now).trouble, 0);
    }

    #[test]
    fn a_failure_with_no_time_on_it_is_not_treated_as_just_now() {
        // Missing means unknown, and unknown is not an emergency.
        let mut row = a_session("failed", 0);
        row.last_activity_at = None;
        assert_eq!(at_work_of(&[row], 1_700_000_000_000).trouble, 0);
    }

    #[test]
    fn what_is_waiting_is_counted_out_of_whatever_shape_it_arrives_in() {
        assert_eq!(waiting_in(&json!([{ "id": "a" }, { "id": "b" }])), 2);
        assert_eq!(waiting_in(&json!({ "approvals": [{ "id": "a" }] })), 1);
        assert_eq!(waiting_in(&json!({ "items": [] })), 0);
        // Nothing recognisable is nothing waiting, not a light nobody can turn off.
        assert_eq!(waiting_in(&json!({ "other": 3 })), 0);
    }

    #[test]
    fn a_page_gives_up_both_halves_of_the_conversation_and_says_which_is_which() {
        /*
         * Both, tagged. Two surfaces read this list and want different parts of it: the
         * Work panel is showing a conversation, and a conversation with the answers taken
         * out is a list of things somebody said into a void; rewind takes only the ones
         * that are `mine`, because rewinding to an answer would discard the prompt that
         * produced it.
         */
        let points = points_in(&a_page());
        assert_eq!(points.len(), 2, "the prompt and the answer");

        let asked = points.iter().find(|point| point.mine).expect("a prompt");
        assert_eq!(asked.id, "9c720ea7-a79d-46fe-845a-0552b181818d");
        assert_eq!(asked.said, "ok go on to the next stage");
        assert_eq!(asked.at, Some(1_788_695_901_361));

        assert_eq!(
            points.iter().filter(|point| !point.mine).count(),
            1,
            "and the answer, marked as not the operator's"
        );
    }

    #[test]
    fn a_delta_envelope_is_read_as_well_as_a_page() {
        // The streaming path wraps the message and stamps the id beside it.
        let points = points_in(&json!({"messages": [{
            "sessionKey": "main:1",
            "messageId": "e1",
            "message": {"role": "user", "content": "fix the header gap"}
        }]}));
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].id, "e1");
        assert_eq!(points[0].said, "fix the header gap");
        assert!(points[0].mine);
    }

    #[test]
    fn a_transcript_it_cannot_read_produces_nothing_rather_than_a_guess() {
        // The failure mode is the point. A useless answer that is true beats a plausible
        // one that is not, for something whose next step discards work.
        assert!(points_in(&json!({})).is_empty());
        assert!(points_in(&json!({"messages": []})).is_empty());
        // An entry with no id cannot be rewound to, so it is not offered.
        assert!(points_in(&json!({"messages": [{"role": "user", "content": "hello"}]})).is_empty());
        // Tool calls and scaffolding are not turns of a conversation.
        assert!(points_in(&json!({"messages": [{
            "role": "tool",
            "content": "{}",
            "__openclaw": {"id": "t1"}
        }]}))
        .is_empty());
        // Input the Gateway is holding is not yet a turn to go back to.
        assert!(points_in(&json!({"messages": [{
            "role": "user",
            "content": "queued",
            "__openclaw": {"id": "pending:abc"}
        }]}))
        .is_empty());
    }

    #[test]
    fn a_prompt_that_was_only_an_attachment_says_so_rather_than_showing_markup() {
        // Sending a picture with no words stores the markup that displays it. Offering
        // that as a row to choose between is worse than admitting there were no words.
        let points = points_in(&json!({"messages": [{
            "role": "user",
            "content": "<img class=\"image\" src=\"http://127.0.0.1:18789/x.png\" />",
            "__openclaw": {"id": "e1"}
        }]}));
        assert_eq!(points.len(), 1, "it still has a place in the transcript");
        assert_eq!(points[0].said, "");
    }

    #[test]
    fn a_date_is_read_as_well_as_a_number() {
        // Stored events carry ISO strings; projected pages carry milliseconds.
        assert_eq!(
            when_in(&json!(1_788_695_901_361i64)),
            Some(1_788_695_901_361)
        );
        assert_eq!(
            when_in(&json!("2026-09-06T11:58:21.361Z")),
            Some(1_788_695_901_361)
        );
        assert_eq!(when_in(&json!("1970-01-01T00:00:00.000Z")), Some(0));
        assert_eq!(when_in(&json!("not a date")), None);
    }

    #[test]
    fn connect_frame_matches_gateway_schema() {
        let directory = std::env::temp_dir().join(format!(
            "openclaw-linux-connect-frame-test-{}",
            Uuid::new_v4()
        ));
        let store = GatewayDeviceIdentityStore::load_or_create(directory.join("identity.json"))
            .expect("device identity");
        let params = connect_params(
            &store.identity(),
            &GatewayAuth::SharedToken("secret".to_string()),
            "fixture-nonce",
            1_800_000_000_000,
            true,
        )
        .expect("connect params");
        let frame = request_frame("connect-1", "connect", params);

        assert_eq!(frame["type"], "req");
        assert_eq!(frame["id"], "connect-1");
        assert_eq!(frame["method"], "connect");
        assert_eq!(frame["params"]["minProtocol"], MIN_PROTOCOL_VERSION);
        assert_eq!(frame["params"]["maxProtocol"], MAX_PROTOCOL_VERSION);
        assert_eq!(
            frame["params"]["caps"],
            json!([
                AGENT_KIND_CLIENT_CAPABILITY,
                INLINE_WIDGETS_CLIENT_CAPABILITY
            ])
        );
        assert_eq!(frame["params"]["client"]["id"], CLIENT_ID);
        assert_eq!(
            frame["params"]["client"]["deviceFamily"],
            CLIENT_DEVICE_FAMILY
        );
        assert_eq!(frame["params"]["auth"], json!({ "token": "secret" }));
        assert_eq!(frame["params"]["device"]["nonce"], "fixture-nonce");
        assert_eq!(frame["params"]["device"]["signedAt"], 1_800_000_000_000_u64);
        assert_eq!(
            frame["params"]["device"]["id"]
                .as_str()
                .expect("device id")
                .len(),
            64
        );
        assert!(frame["params"]["device"]["publicKey"]
            .as_str()
            .is_some_and(|value| !value.contains('=')));
        assert!(frame["params"]["device"]["signature"]
            .as_str()
            .is_some_and(|value| !value.contains('=')));

        let pinned_params = connect_params(
            &store.identity(),
            &GatewayAuth::SharedToken("secret".to_string()),
            "fixture-nonce",
            1_800_000_000_000,
            false,
        )
        .expect("pinned connect params");
        // Pinning only withdraws inline widgets; agent-kind is unconditional.
        assert_eq!(pinned_params["caps"], json!([AGENT_KIND_CLIENT_CAPABILITY]));
        std::fs::remove_dir_all(directory).expect("remove connect fixture");
    }

    #[test]
    fn connect_challenge_uses_gateway_timestamp() {
        let Ok(challenge) = parse_connect_challenge(&json!({
            "payload": {
                "nonce": " fixture-nonce ",
                "ts": 1_700_000_000_123_u64
            }
        })) else {
            panic!("expected valid challenge");
        };

        assert_eq!(
            challenge,
            ConnectChallenge {
                nonce: "fixture-nonce".to_string(),
                issued_at_ms: 1_700_000_000_123,
            }
        );
        assert!(parse_connect_challenge(&json!({
            "payload": { "nonce": "missing-time" }
        }))
        .is_err());
        assert!(parse_connect_challenge(&json!({
            "payload": { "nonce": "fixture-nonce", "ts": "1700000000123" }
        }))
        .is_err());
    }

    #[test]
    fn hello_tick_policy_sets_two_interval_watchdog() {
        let hello = validate_hello(json!({
            "type": "hello-ok",
            "protocol": MAX_PROTOCOL_VERSION,
            "features": { "methods": ["agents.list", "chat.send"] },
            "auth": { "deviceToken": "test-device-token" },
            "policy": { "tickIntervalMs": 1_250 },
            "pluginSurfaceUrls": {
                "canvas": "https://gateway.example/__openclaw__/cap/fixture-capability"
            }
        }))
        .expect("valid hello");

        assert_eq!(hello.device_token.as_deref(), Some("test-device-token"));
        assert_eq!(hello.tick_watch_timeout, Duration::from_millis(2_500));
    }

    #[test]
    fn gateway_state_event_says_the_state_and_the_gateway_s_own_words() {
        // The page shows the notice when there is one and its own sentence when there is
        // not, so both shapes have to survive serialisation.
        let spoken = serde_json::to_value(GatewayStateEvent::new(
            GatewayConnectionState::PairingRequired,
            Some("Pair this device first.".to_string()),
        ))
        .expect("serialize gateway state");
        assert_eq!(spoken["state"], "pairing-required");
        assert_eq!(spoken["notice"], "Pair this device first.");

        let silent = serde_json::to_value(GatewayStateEvent::new(GatewayConnectionState::Up, None))
            .expect("serialize gateway state");
        assert_eq!(silent["state"], "up");
        assert!(silent.get("notice").is_none());
    }

    #[test]
    fn connect_classification_separates_pairing_and_missing_credentials() {
        assert_eq!(
            classify_connect_failure(Some(PAIRING_REQUIRED_DETAIL_CODE), true),
            Some(GatewayConnectionState::PairingRequired)
        );
        assert_eq!(
            classify_connect_failure(Some(AUTH_TOKEN_MISSING_DETAIL_CODE), false),
            Some(GatewayConnectionState::CredentialRequired)
        );
        assert_eq!(
            classify_connect_failure(Some("AUTH_TOKEN_MISMATCH"), false),
            Some(GatewayConnectionState::CredentialRequired)
        );
        assert_eq!(
            classify_connect_failure(Some("AUTH_TOKEN_MISMATCH"), true),
            None
        );
        assert_eq!(
            GatewayConnectionState::CredentialRequired.event_name(),
            "credential-required"
        );

        let pairing_details = json!({ "code": PAIRING_REQUIRED_DETAIL_CODE });
        let pending =
            RequestFailure::method_with_details("pairing required", Some(&pairing_details))
                .classify_connect(&GatewayAuth::SharedToken("bootstrap".to_string()));
        assert_eq!(
            pending.connect_state,
            Some(GatewayConnectionState::PairingRequired)
        );

        let missing_details = json!({ "code": AUTH_TOKEN_MISSING_DETAIL_CODE });
        let missing_auth_failure =
            RequestFailure::method_with_details("token missing", Some(&missing_details))
                .classify_connect(&GatewayAuth::None);
        assert_eq!(
            missing_auth_failure.connect_state,
            Some(GatewayConnectionState::CredentialRequired)
        );

        let mismatch_details = json!({ "code": "AUTH_TOKEN_MISMATCH" });
        let mismatch_without_auth =
            RequestFailure::method_with_details("token mismatch", Some(&mismatch_details))
                .classify_connect(&GatewayAuth::None);
        assert_eq!(
            mismatch_without_auth.connect_state,
            Some(GatewayConnectionState::CredentialRequired)
        );
        let mismatch_with_auth =
            RequestFailure::method_with_details("token mismatch", Some(&mismatch_details))
                .classify_connect(&GatewayAuth::SharedToken("configured".to_string()));
        assert_eq!(mismatch_with_auth.connect_state, None);

        let stale_device_details = json!({ "code": AUTH_DEVICE_TOKEN_MISMATCH_DETAIL_CODE });
        let stale_device_auth = RequestFailure::method_with_details(
            "device token mismatch",
            Some(&stale_device_details),
        )
        .classify_connect(&GatewayAuth::DeviceToken("stale".to_string()));
        assert_eq!(stale_device_auth.connect_state, None);
        assert!(should_clear_stored_device_token(
            &stale_device_auth,
            &GatewayAuth::DeviceToken("stale".to_string())
        ));
    }

    #[test]
    fn missing_gateway_credentials_override_retained_device_auth() {
        for detail_code in [
            AUTH_TOKEN_MISSING_DETAIL_CODE,
            AUTH_PASSWORD_MISSING_DETAIL_CODE,
        ] {
            let details = json!({
                "code": detail_code,
                "retryable": false,
                "pauseReconnect": true
            });
            let auth = GatewayAuth::DeviceToken("retained-device-token".to_string());
            let failure = RequestFailure::method_with_details("credential missing", Some(&details))
                .classify_connect(&auth);

            assert_eq!(
                failure.connect_state,
                Some(GatewayConnectionState::CredentialRequired)
            );
            assert!(should_pause_reconnect(&failure.connect_details));
            assert!(!should_clear_stored_device_token(&failure, &auth));
            let state = failure.connect_state.expect("classified state");
            let notice = connection_notice(state, &failure.connect_details, true);
            assert_eq!(
                notice.as_deref(),
                Some("Gateway requires a credential — open the dashboard on the gateway host")
            );
            assert_eq!(
                serde_json::to_value(GatewayStateEvent::new(state, notice))
                    .expect("serialize credential-required state"),
                json!({
                    "state": "credential-required",
                    "notice": "Gateway requires a credential — open the dashboard on the gateway host"
                })
            );
        }
    }

    #[tokio::test]
    async fn reopening_quick_chat_resumes_only_a_paused_reconnect() {
        let client = GatewayClient::new();
        let (commands, mut receiver) = mpsc::channel(2);
        *client
            .inner
            .commands
            .lock()
            .expect("gateway command mutex poisoned") = Some(commands);

        client.resume_paused_reconnect();
        assert!(
            tokio::time::timeout(Duration::from_millis(25), receiver.recv())
                .await
                .is_err()
        );

        client.inner.reconnect_paused.store(true, Ordering::SeqCst);
        client.resume_paused_reconnect();
        assert!(matches!(
            receiver.recv().await,
            Some(DriverCommand::Reconfigure)
        ));
    }

    #[test]
    fn reconnect_pause_requires_explicit_server_policy() {
        let pause_details = json!({ "pauseReconnect": true });
        let paused = RequestFailure::method_with_details("pause", Some(&pause_details));
        assert!(should_pause_reconnect(&paused.connect_details));

        let terminal_details = json!({ "retryable": false });
        let terminal = RequestFailure::method_with_details("terminal", Some(&terminal_details));
        assert!(should_pause_reconnect(&terminal.connect_details));

        let retry_details = json!({ "retryable": true, "pauseReconnect": false });
        let retry = RequestFailure::method_with_details("retry", Some(&retry_details));
        assert!(!should_pause_reconnect(&retry.connect_details));
        assert!(!should_pause_reconnect(
            &RequestFailure::transport("transport").connect_details
        ));
    }

    #[test]
    fn connection_notices_prefer_server_guidance_and_shorten_device_ids() {
        let details = ConnectErrorDetails::from_value(Some(&json!({
            "remediationHint": "Use the Nodes approval queue.",
            "deviceId": "abcdef1234567890"
        })));
        assert_eq!(
            connection_notice(GatewayConnectionState::PairingRequired, &details, true).as_deref(),
            Some("Use the Nodes approval queue. · Device abcdef12")
        );
        assert_eq!(
            connection_notice(
                GatewayConnectionState::CredentialRequired,
                &ConnectErrorDetails::default(),
                true,
            )
            .as_deref(),
            Some("Gateway requires a credential — open the dashboard on the gateway host")
        );
        assert_eq!(
            connection_notice(
                GatewayConnectionState::Down,
                &ConnectErrorDetails::from_value(Some(&json!({
                    "remediationHint": "Replace the configured credential."
                }))),
                true,
            )
            .as_deref(),
            Some("Replace the configured credential.")
        );
    }

    #[test]
    fn chat_send_frame_matches_gateway_schema() {
        let params = ChatSendParams {
            session_key: "agent:work:main".to_string(),
            agent_id: None,
            message: "hello".to_string(),
            idempotency_key: "idempotency-1".to_string(),
            attachments: Vec::new(),
        };
        assert_eq!(
            request_frame(
                "chat-1",
                "chat.send",
                serde_json::to_value(params).expect("chat params")
            ),
            json!({
                "type": "req",
                "id": "chat-1",
                "method": "chat.send",
                "params": {
                    "sessionKey": "agent:work:main",
                    "message": "hello",
                    "idempotencyKey": "idempotency-1"
                }
            })
        );
    }

    #[test]
    fn chat_send_result_flattens_route_and_ack_run_id() {
        let result = ChatSendResult {
            target: routing_target("global", "work", "main"),
            run_id: "run-1".to_string(),
        };
        assert_eq!(
            serde_json::to_value(result).expect("serialized chat send result"),
            json!({ "sessionKey": "global", "agentId": "work", "runId": "run-1" })
        );
    }
}
