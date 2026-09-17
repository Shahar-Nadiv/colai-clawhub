//! Handing what was marked to whoever is receiving it.
//!
//! One kind of receiver: a conversation. OpenClaw offered three — an agent, a session it
//! was holding, and a conversation held in another agent entirely — because it ran many
//! agents on somebody's behalf. Claude Code has one Claude, so the only choice worth
//! making is which conversation to carry on, and an empty one means start a new one.
//!
//! The message itself is composed in the page, not here. What an agent reads is a
//! product decision that belongs next to the marks somebody made, and it is a pure
//! function there with tests on it. This module's job is to resolve a receiver, attach
//! the pictures, and say what happened.

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::colai_capture::MarkShots;
use crate::wire::{
    ChatAttachment, CronAdd, CronAdded, Point, Rewound,
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
    /// The name of the prompt just sent, which is what "put it back to before I asked" is
    /// addressed by. Nothing else knows it: it is minted at the moment of sending.
    pub prompt: String,
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
    /// The chat this was left for, when it was left rather than sent.
    ///
    /// `None` is the ordinary case: the toolbar's own agent has it and the answer will
    /// come back to the Work panel. `Some(name)` means the conversation is open in a
    /// terminal, so the mark is waiting for that session's next message — and the rail has
    /// to say so, because "sent" and "will arrive when you next type there" are different
    /// promises and only one of them is true.
    pub handed_to: Option<String>,
}

/// Send the marked work to whoever was chosen.
#[tauri::command]
pub(crate) async fn colai_send(
    // Here to reach the main thread. Laying a recording out as one picture is drawing,
    // and drawing may only happen there — see `attach`.
    app: AppHandle,
    session: State<'_, crate::session::Session>,
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
    /*
     * Which conversation this belongs to.
     *
     * OpenClaw had three kinds of receiver — an agent, a session, a thread in somebody
     * else's project — because it ran many agents and the toolbar had to say which. Claude
     * Code has one Claude and a pile of conversations, so there is one kind: a session,
     * and an empty id means start a new one.
     */
    let key = (!receiver.id.trim().is_empty()).then(|| receiver.id.clone());

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
    /*
     * Model and effort are not applied here.
     *
     * The Gateway kept them on the conversation and had a method to patch it. Claude Code
     * takes the model when the process starts and keeps its own thinking setting, so there
     * is nothing to patch mid-conversation — and a call that silently did nothing would be
     * worse than not making it. The rail's pickers are removed on this host rather than
     * left as controls that move nothing.
     */
    let mut settings_trouble: Option<String> = None;
    let _ = (&model, &thinking_level);
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
        // Where this conversation is being had, which is what decides what may be read.
        // The Gateway was asked; here it is the cwd Claude Code recorded for the session.
        let roots = crate::session::work_roots(key.as_deref());
        let roots: Vec<std::path::PathBuf> = roots.into_iter().map(Into::into).collect();
        let brought = crate::colai_files::carry(&files, &roots);
        attachments.extend(brought.travelling);
        refused = brought.refused;
    }
    let carried = attachments.len() - pictures;
    /*
     * Pictures travel; named files do not.
     *
     * A screenshot exists nowhere but in memory, so it has to go as bytes. A file the
     * person named is already on disk and Claude Code can open it — the message says where
     * it is, and sending a copy would mean the agent reading one of two things that are
     * supposed to be the same file.
     */
    let images: Vec<String> = attachments
        .iter()
        .filter(|one| one.mime_type.starts_with("image/"))
        .map(|one| one.content.clone())
        .collect();
    /*
     * A conversation somebody is sitting in is handed over, not taken over.
     *
     * `session.send` starts a `claude --resume` of its own, and on a live chat that is a
     * second process on one transcript — answering in the Work panel while the window the
     * person is actually looking at says nothing. So when the receiver is a chat that is
     * open in a terminal, the mark is left where that session's own hook will find it, and
     * it arrives there on their next message.
     */
    if let Some(held) = key.as_deref().and_then(crate::session::already_open_in_a_chat) {
        // Where that conversation is being had, which is the only place its session may
        // read from — so it is where the pictures have to land.
        let theirs = crate::session::where_it_is_had(Some(held.session_id.as_str()));
        let staged =
            crate::outbox::stage(held.session_id.as_str(), theirs.as_deref(), &message, &images)?;
        return Ok(Sent {
            session_key: held.session_id,
            run_id: String::new(),
            prompt: String::new(),
            pictures: staged.pictures,
            carried,
            refused,
            // Nothing to watch for: the answer will appear in their terminal, not here.
            watching: false,
            settings_trouble,
            handed_to: held.name,
        });
    }

    let cwd = crate::session::where_it_is_had(key.as_deref());
    let prompt = session.send(&app, key.clone(), message, images, cwd)?;
    let sent = Sent {
        session_key: key.unwrap_or_default(),
        // The Gateway gave a run id to correlate against. Nothing here does: one
        // conversation, one turn at a time, and the reply carries the session it is for.
        run_id: String::new(),
        prompt,
        pictures,
        carried,
        refused,
        // See the note below: there is no second channel to miss.
        watching: true,
        settings_trouble,
        // Answered here, by the toolbar's own agent, which is the ordinary case.
        handed_to: None,
    };
    // Only once it has landed. A failed send that had already forgotten its pictures
    // would leave the marks in the tray with nothing behind them.
    //
    // And its failure is not the send's. The message is delivered by this point, so
    // returning an error here would tell somebody their send failed and invite them to
    // send it twice.
    if let Err(why) = shots.forget(&mark_ids) {
        eprintln!("[colai] the pictures could not be released after sending: {why}");
    }
    /*
     * Nothing to subscribe to.
     *
     * The Gateway delivered a conversation's messages only to subscribers, so a send was
     * followed by a request to listen and `watching` said whether that worked. Here the
     * answer comes back down the pipe the message went up — there is no second channel to
     * miss, so `watching` is true whenever anything was sent at all.
     */
    Ok(sent)
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
    #[allow(unused_variables)] receiver: Receiver,
    #[allow(unused_variables)] message: String,
    #[allow(unused_variables)] schedule: CronAdd,
) -> Result<CronAdded, String> {
    /*
     * No scheduler on this host.
     *
     * OpenClaw ran agents in the background and could be told to run one later; Claude
     * Code is a session somebody is sitting in front of. Rather than invent a scheduler
     * inside a toolbar, this says so — and the rail's automation control is removed on
     * this host rather than left as a button that explains itself only after being pressed.
     */
    Err("Scheduling is an OpenClaw feature; Claude Code has no scheduler.".to_string())
}

/// Where a conversation could be taken back to.
///

/// What was said in a conversation, both halves of it.
///
/// The Work panel keeps its own record of what was sent from this toolbar, and that
/// record survives a restart — but the answers do not, because they arrive long after
/// the send and often while the toolbar is not running. This is where they come back
/// from: the Gateway has the transcript, so the panel asks for it rather than being the
/// only thing that ever knew.
#[tauri::command]
pub(crate) async fn colai_said(session_key: String) -> Result<Vec<Point>, String> {
    // Off the transcript, which on this host is the only record there is.
    Ok(crate::session::what_was_said(&session_key, POINTS_AT_MOST as usize)
        .into_iter()
        .map(|(id, said, mine, at)| Point { id, said, at, mine })
        .collect())
}

/// How far back a conversation offers to go.
///
/// Not the whole transcript. What somebody is looking for is a message they remember
/// sending in the last few minutes, and a list long enough to scroll is a list nobody
/// reads to the end of — while the answer itself is a transcript coming down a socket.
const POINTS_AT_MOST: u32 = 40;

/// Take a conversation back to one of its own prompts.
///

/// Stop a run that is underway.
///
/// The only thing on this rail that destroys work rather than describing it. It says
/// what it stopped rather than going quiet, because a stop that produced no answer is
/// indistinguishable from a stop that did not happen — and somebody who pressed it needs
/// to know which.
#[tauri::command]
pub(crate) async fn colai_stop(
    session: State<'_, crate::session::Session>,
    #[allow(unused_variables)] session_key: String,
) -> Result<(), String> {
    /*
     * Stop the turn, not the process.
     *
     * This used to kill the child, which does stop it and also throws away the result frame
     * — and with it the cost of everything that had already happened, so a conversation
     * somebody interrupted under-reported what it had spent. `interrupt` ends the turn and
     * leaves the conversation standing.
     *
     * Killing remains the fallback: a child that has stopped reading its own pipe cannot be
     * asked to do anything, and stop has to work then most of all.
     */
    if session.stop_turn().is_err() {
        session.interrupt();
    }
    Ok(())
}

/// Answer the question the rail is showing.
///
/// The turn is stopped until this returns — `can_use_tool` is not news, it is a question,
/// and Claude Code is waiting on the other end of the pipe.
#[tauri::command]
pub(crate) async fn colai_answer(
    session: State<'_, crate::session::Session>,
    id: String,
    allow: bool,
    message: Option<String>,
) -> Result<(), String> {
    // Whatever somebody typed as a reason goes to Claude, so it travels through the same
    // fence as everything else that is not ours: it is about to become part of a prompt.
    let said = crate::session::plainly(message.as_deref().unwrap_or_default());
    session.answer(&id, allow, &said)
}

/// Change what may happen without being asked, from now on.
#[tauri::command]
pub(crate) async fn colai_allow_now(
    session: State<'_, crate::session::Session>,
    mode: String,
) -> Result<(), String> {
    session.allow_now(&mode)
}

/// Put every file back to how it was before the given prompt.
///
/// With `dry_run` it reports what would change and changes nothing, which is what the
/// confirmation shows — rewinding also reverts anything a person edited by hand while the
/// agent was working, and those are not the agent's to put back.
#[tauri::command]
pub(crate) async fn colai_undo(
    session: State<'_, crate::session::Session>,
    prompt: String,
    dry_run: bool,
) -> Result<(), String> {
    session.undo_since(&prompt, dry_run)
}

/// Start listening to a conversation this toolbar did not start.
///
/// Sending already subscribes to what it sent. This is the other way in: the Work panel
/// lists every conversation the Gateway holds, and opening one there should mean its
/// replies keep arriving — otherwise it shows whatever was true at the instant it was
/// opened while its own pill goes on saying "Working".
#[tauri::command]
pub(crate) async fn colai_watch(#[allow(unused_variables)] session_key: String) -> Result<bool, String> {
    /*
     * Nothing to subscribe to, so nothing to fail.
     *
     * The Gateway delivered a conversation's messages only to subscribers, and this asked
     * to be one. Here the answer comes back down the same pipe the message went up: there
     * is no second channel, and therefore none to miss.
     */
    Ok(true)
}

/// Stop listening to a conversation.
///
/// Called when the overlay is put away and when what was being waited on is dismissed.
/// A subscription the Gateway is holding for a window that has gone is a socket kept
/// open for nobody.
#[tauri::command]
pub(crate) async fn colai_unwatch(#[allow(unused_variables)] session_key: String) -> Result<(), String> {
    // See `colai_watch`: there is nothing to stop listening to.
    Ok(())
}

/// Open a new conversation where the work is.
///
/// Seeded with what was marked, so the first thing the new session sees is the reason
/// it exists rather than an empty prompt somebody then has to explain themselves into.
#[tauri::command]
pub(crate) async fn colai_start_here(
    #[allow(unused_variables)] asked: StartHere,
) -> Result<(), String> {
    /*
     * The Gateway could open a terminal on a project and start a conversation in it.
     * Nothing here can: Claude Code is started by the person, in the directory they mean,
     * and a toolbar spawning terminals on their behalf is a different product.
     *
     * A conversation started *by the toolbar* simply has no key yet — see `colai_send`,
     * where an empty receiver id means exactly that.
     */
    Err("Start a conversation by running claude where you want it.".to_string())
}
