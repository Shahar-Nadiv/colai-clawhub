//! Handing what was marked to whoever is receiving it.
//!
//! The toolbar offers three kinds of receiver — an agent, a conversation the Gateway is
//! holding, and a conversation held in another agent entirely — and they look like three
//! different things on the menu. They are not three different sends. Every one of them
//! resolves to a session key, and from there a message with pictures attached goes out
//! the same way for all of them.
//!
//! The message itself is composed in the page, not here. What an agent reads is a
//! product decision that belongs next to the marks somebody made, and it is a pure
//! function there with tests on it. This module's job is to resolve a receiver, attach
//! the pictures, and say what happened.

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::colai_capture::MarkShots;
use crate::gateway_ws::{
    ChatAttachment, ChatRoutingTarget, CronAdd, CronAdded, GatewayClient, Point, Rewound,
    StartHere, ThreadLocator,
};

/// Who is getting this, as the page knows them.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Receiver {
    /// `agent`, `session`, or `thread`.
    pub kind: String,
    pub id: String,
    /// Only a thread has one, and a thread cannot be reached without it.
    #[serde(default)]
    pub locator: Option<ThreadLocator>,
}

/// What the receipt gets to say.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Sent {
    pub session_key: String,
    pub run_id: String,
    /// How many pictures actually went. A mark whose picture has aged out of the store
    /// is still described in the message; it just arrives without its picture, and the
    /// receipt should not claim otherwise.
    pub pictures: usize,
    /// How many of the files somebody brought in actually went with it.
    pub carried: usize,
    /// Files the message named that did not travel, and why they are worth saying: the
    /// agent has been told they are attached, so silence here is a conversation about a
    /// file nobody sent.
    pub refused: Vec<String>,
    /// Whether the reply will find its way back to the screen.
    ///
    /// Said rather than swallowed. A subscription that quietly failed leaves a mark
    /// waiting on an answer that is never coming, which looks exactly like an agent
    /// thinking about it — the worst of both, since the answer did arrive, just
    /// somewhere else.
    pub watching: bool,
    /// Why the model or the effort did not take, when they did not.
    ///
    /// The commonest reason is the ordinary one: a first send to an agent has no
    /// conversation yet, so there was nothing to set it on. Said rather than swallowed —
    /// a setting that appears to have applied and did not is how somebody spends an hour
    /// wondering why the answers look the same.
    pub settings_trouble: Option<String>,
}

/// Send the marked work to whoever was chosen.
#[tauri::command]
pub(crate) async fn colai_send(
    // Here to reach the main thread. Laying a recording out as one picture is drawing,
    // and drawing may only happen there — see `attach`.
    app: AppHandle,
    gateway: State<'_, GatewayClient>,
    shots: State<'_, MarkShots>,
    receiver: Receiver,
    message: String,
    // Which marks would rather arrive as one contact sheet than as a run of frames.
    sheets: Option<Vec<String>>,
    accent: Option<String>,
    // What goes with the message. Optional because nothing is a real answer for both:
    // a reply to something an agent said carries neither, and requiring them turned
    // that into "invalid args: missing required key `files`" the first time somebody
    // pressed Accept — a sentence about this function's shape, put in front of somebody
    // who was agreeing with a suggestion.
    mark_ids: Option<Vec<String>>,
    files: Option<Vec<String>>,
    // How this conversation should answer, when somebody has chosen. Optional both
    // ways: nobody choosing is not the same as choosing the default.
    model: Option<String>,
    thinking_level: Option<String>,
) -> Result<Sent, String> {
    let message = message.trim().to_string();
    if message.is_empty() {
        return Err("There is nothing to send.".to_string());
    }
    let mark_ids = mark_ids.unwrap_or_default();
    let target = resolve(&gateway, &receiver).await?;

    /*
     * How this should be answered, before it is asked.
     *
     * Model and effort are settings on the conversation rather than fields on a message,
     * so they are applied to the conversation this is about to go to. Only when one is
     * set: an unset pair is somebody who has not chosen, not somebody choosing "default".
     *
     * A send to an agent with no conversation yet has nothing to patch and this fails.
     * The message still goes — sending is what was asked for — and the failure is carried
     * back rather than swallowed, because a setting that silently did not apply is worse
     * than one that visibly did not. The next send lands it, the conversation now existing.
     */
    let mut settings_trouble = None;
    if model.is_some() || thinking_level.is_some() {
        if let Err(trouble) = gateway
            .set_answering(
                &target.session_key,
                target.agent_id.clone(),
                model,
                thinking_level,
            )
            .await
        {
            settings_trouble = Some(trouble);
        }
    }
    let (mut attachments, sheet_trouble) = attach(
        &app,
        &shots,
        &mark_ids,
        &sheets.unwrap_or_default(),
        &accent.unwrap_or_else(|| "#ff5c5c".to_string()),
    )?;
    if let Some(why) = sheet_trouble {
        settings_trouble.get_or_insert(why);
    }
    let pictures = attachments.len();
    // After the pictures, in the order the message describes them. The message has
    // already decided which of these travel and which are only named; anything in this
    // list is one that travels.
    // What may be read is decided against the folders the Gateway says are worked in,
    // never against a list the page supplied.
    //
    // Asked only when there is something to ask about. `work_roots` enumerates every
    // catalog, host and session the Gateway knows, and it was doing that on every send —
    // a Gateway round trip in front of every message, to decide what may be read out of
    // an empty list of files. The gate is unchanged: with nothing to carry there is
    // nothing for it to let through.
    let files = files.unwrap_or_default();
    let mut refused: Vec<String> = Vec::new();
    if !files.is_empty() {
        let roots = crate::colai_receivers::work_roots(&gateway).await;
        let brought = crate::colai_files::carry(&files, &roots);
        attachments.extend(brought.travelling);
        refused = brought.refused;
    }
    let carried = attachments.len() - pictures;
    let sent = gateway
        .chat_send_to(
            target,
            message,
            attachments,
            // Every send is its own send. The key exists so a transport retry cannot
            // double-post, not to collapse two deliberate sends of the same marks.
            &uuid::Uuid::new_v4().to_string(),
        )
        .await?;
    // Only once it has landed. A failed send that had already forgotten its pictures
    // would leave the marks in the tray with nothing behind them.
    //
    // And its failure is not the send's. The message is delivered by this point, so
    // returning an error here would tell somebody their send failed and invite them to
    // send it twice.
    if let Err(why) = shots.forget(&mark_ids) {
        eprintln!("[colai] the pictures could not be released after sending: {why}");
    }
    // Listening is not worth failing the send over — the message has already landed,
    // and all that is lost is the answer coming back to the screen rather than to the
    // conversation. Worth saying, though, which is what `watching` is for.
    let watching = gateway
        .watch_session(&sent.target.session_key, true)
        .await
        .is_ok();
    Ok(Sent {
        session_key: sent.target.session_key,
        run_id: sent.run_id,
        pictures,
        carried,
        refused,
        watching,
        settings_trouble,
    })
}

/// Which session a receiver turns out to be.
///
/// A thread is the only one that costs anything: continuing it is what makes a
/// conversation held elsewhere into one the Gateway can speak to, and it is a real
/// handover. The page asks before it ever gets here.
async fn resolve(
    gateway: &GatewayClient,
    receiver: &Receiver,
) -> Result<ChatRoutingTarget, String> {
    match receiver.kind.as_str() {
        "agent" => gateway.target_for_agent(&receiver.id).await,
        "session" => Ok(ChatRoutingTarget {
            session_key: receiver.id.clone(),
            // A canonical session key already says whose it is; naming the agent again
            // is rejected.
            agent_id: None,
        }),
        "thread" => {
            let locator = receiver
                .locator
                .clone()
                .ok_or_else(|| "That conversation cannot be reached from here.".to_string())?;
            let session_key = gateway.catalog_continue(locator).await?;
            Ok(ChatRoutingTarget {
                session_key,
                agent_id: None,
            })
        }
        other => Err(format!("There is no way to send to a {other}.")),
    }
}

/// Lay a recording out as one picture, on the thread allowed to draw.
///
/// GDK may only be used from the thread that started it, and it does not decline when it
/// is not: it aborts the process it is on. This runs inside `colai_send`, which is an
/// async command and therefore on a tokio worker, so calling the drawing directly killed
/// that worker mid-send. Tauri does not catch a panic across the command boundary, so the
/// promise on the page never settled, its `finally` never ran, and `state.sending` stayed
/// true for the life of the toolbar — one recording sent, and Send never worked again.
///
/// The frame capture beside it has always gone through here. Only the contact sheet did
/// not, because it is assembled from bytes already in hand and did not look like drawing.
#[cfg(target_os = "linux")]
fn sheet_on_the_main_thread(
    app: &AppHandle,
    frames: &[Vec<u8>],
    accent: &str,
) -> Result<crate::colai_capture::Sheet, String> {
    let (done, wait) = std::sync::mpsc::channel();
    let (frames, accent) = (frames.to_vec(), accent.to_string());
    app.run_on_main_thread(move || {
        let _ = done.send(crate::colai_capture::contact_sheet(&frames, &accent));
    })
    .map_err(|error| format!("Could not reach the display: {error}"))?;
    wait.recv()
        .map_err(|_| "The display did not answer.".to_string())?
}

/// The pictures for these marks, named in the order the message describes them.
///
/// The names matter: the message says "mark 2" and the agent has to be able to tell
/// which picture that is. A mark that photographed once keeps the plain name it always
/// had; a recording numbers its frames after it, so a set of six is a sequence rather
/// than six unrelated pictures of the same corner of a screen.
fn attach(
    app: &AppHandle,
    shots: &MarkShots,
    mark_ids: &[String],
    sheets: &[String],
    accent: &str,
) -> Result<(Vec<ChatAttachment>, Option<String>), String> {
    let mut carried = Vec::new();
    let mut trouble: Option<String> = None;
    for picked in shots.pick(mark_ids)?.into_iter() {
        // The position it was asked for at, not the position it survived at: a mark whose
        // shot aged out leaves a gap, and closing it would rename everything after it.
        let numbered = picked.asked_at + 1;
        let one = |png: Vec<u8>, name: String, size: (i32, i32)| ChatAttachment {
            kind: "image".to_string(),
            mime_type: "image/png".to_string(),
            file_name: name,
            content: base64::engine::general_purpose::STANDARD.encode(png),
            width: size.0,
            height: size.1,
        };
        // A run that would rather arrive as one picture. Eight frames of a screen cost
        // about fifteen thousand image tokens sent separately and under a thousand laid
        // out in a grid — and the grid reads better, because the sequence is visible
        // instead of having to be reassembled from eight unrelated pictures.
        //
        // Which marks want it is the page's call, not this function's: what deserves a
        // sheet is a question about what somebody meant, and this end only knows bytes.
        if picked.frames.len() > 1 && sheets.contains(&picked.id) {
            #[cfg(target_os = "linux")]
            match sheet_on_the_main_thread(app, &picked.frames, accent) {
                Ok(sheet) => {
                    carried.push(one(
                        sheet.png,
                        format!("mark-{numbered}.png"),
                        (sheet.width, sheet.height),
                    ));
                    continue;
                }
                // Falling through sends every frame on its own, which costs about fifteen
                // times the image tokens and reads worse. Somebody asked for a sheet and
                // is getting something else, so it is said rather than absorbed.
                Err(why) => {
                    trouble.get_or_insert(format!("A recording could not be laid out as one picture, so its frames were sent separately: {why}"));
                }
            }
        }
        let many = picked.frames.len() > 1;
        let size = (picked.width, picked.height);
        for (frame, png) in picked.frames.into_iter().enumerate() {
            let name = if many {
                format!("mark-{numbered}-{}.png", frame + 1)
            } else {
                format!("mark-{numbered}.png")
            };
            carried.push(one(png, name, size));
        }
    }
    Ok((carried, trouble))
}

/// Make an automation out of what was marked.
///
/// The same request, on a schedule, and the schedule is the Gateway's own — a job made
/// here is a job the Control UI can list, edit and stop, rather than a second idea of
/// what a recurring task is.
///
/// It carries words and no pictures, which is not a shortcut: a scheduled job takes a
/// message and nothing else. The page composes the message knowing that, and says so
/// where somebody can read it before agreeing to it.
#[tauri::command]
pub(crate) async fn colai_automate(
    gateway: State<'_, GatewayClient>,
    receiver: Receiver,
    asked: CronAdd,
) -> Result<CronAdded, String> {
    if asked.name.trim().is_empty() {
        return Err("An automation needs a name.".to_string());
    }
    // Resolved the same way a send is, so "who receives this" means one thing on this
    // surface. A thread is the only kind that costs anything, and the page has already
    // asked before it gets here.
    let target = resolve(&gateway, &receiver).await?;
    // Where the job belongs, said the way the receiver said it. An agent names an
    // agent; a conversation names its session and lets the Gateway work out whose it
    // is. Left off entirely, the job would run against whatever default the Gateway
    // picks — quietly somewhere other than where it was set up.
    gateway
        .cron_add(CronAdd {
            agent_id: target.agent_id.clone(),
            session_key: target.agent_id.is_none().then_some(target.session_key),
            ..asked
        })
        .await
}

/// Where a conversation could be taken back to.
///
/// Only what the operator said. An answer is not somewhere to go back to — rewinding to
/// one would discard the prompt that produced it and leave the conversation asking a
/// question nobody had asked.
#[tauri::command]
pub(crate) async fn colai_points(
    gateway: State<'_, GatewayClient>,
    session_key: String,
) -> Result<Vec<Point>, String> {
    Ok(gateway
        .chat_history(&session_key, POINTS_AT_MOST)
        .await?
        .into_iter()
        .filter(|point| point.mine)
        .collect())
}

/// What was said in a conversation, both halves of it.
///
/// The Work panel keeps its own record of what was sent from this toolbar, and that
/// record survives a restart — but the answers do not, because they arrive long after
/// the send and often while the toolbar is not running. This is where they come back
/// from: the Gateway has the transcript, so the panel asks for it rather than being the
/// only thing that ever knew.
#[tauri::command]
pub(crate) async fn colai_said(
    gateway: State<'_, GatewayClient>,
    session_key: String,
) -> Result<Vec<Point>, String> {
    gateway.chat_history(&session_key, POINTS_AT_MOST).await
}

/// How far back a conversation offers to go.
///
/// Not the whole transcript. What somebody is looking for is a message they remember
/// sending in the last few minutes, and a list long enough to scroll is a list nobody
/// reads to the end of — while the answer itself is a transcript coming down a socket.
const POINTS_AT_MOST: u32 = 40;

/// Take a conversation back to one of its own prompts.
///
/// The first thing on this surface that discards work. It says what it did.
#[tauri::command]
pub(crate) async fn colai_rewind(
    gateway: State<'_, GatewayClient>,
    session_key: String,
    entry_id: String,
) -> Result<Rewound, String> {
    if entry_id.trim().is_empty() {
        return Err("There is no prompt to go back to.".to_string());
    }
    gateway.sessions_rewind(&session_key, &entry_id).await
}

/// Stop a run that is underway.
///
/// The only thing on this rail that destroys work rather than describing it. It says
/// what it stopped rather than going quiet, because a stop that produced no answer is
/// indistinguishable from a stop that did not happen — and somebody who pressed it needs
/// to know which.
#[tauri::command]
pub(crate) async fn colai_stop(
    gateway: State<'_, GatewayClient>,
    session_key: String,
) -> Result<(), String> {
    gateway.chat_abort(&session_key).await
}

/// Start listening to a conversation this toolbar did not start.
///
/// Sending already subscribes to what it sent. This is the other way in: the Work panel
/// lists every conversation the Gateway holds, and opening one there should mean its
/// replies keep arriving — otherwise it shows whatever was true at the instant it was
/// opened while its own pill goes on saying "Working".
#[tauri::command]
pub(crate) async fn colai_watch(
    gateway: State<'_, GatewayClient>,
    session_key: String,
) -> Result<(), String> {
    gateway.watch_session(&session_key, true).await
}

/// Stop listening to a conversation.
///
/// Called when the overlay is put away and when what was being waited on is dismissed.
/// A subscription the Gateway is holding for a window that has gone is a socket kept
/// open for nobody.
#[tauri::command]
pub(crate) async fn colai_unwatch(
    gateway: State<'_, GatewayClient>,
    session_key: String,
) -> Result<(), String> {
    gateway.watch_session(&session_key, false).await
}

/// Open a new conversation where the work is.
///
/// Seeded with what was marked, so the first thing the new session sees is the reason
/// it exists rather than an empty prompt somebody then has to explain themselves into.
#[tauri::command]
pub(crate) async fn colai_start_here(
    gateway: State<'_, GatewayClient>,
    asked: StartHere,
) -> Result<(), String> {
    gateway.start_here(asked).await
}
