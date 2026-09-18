// Handing what was marked to whoever is receiving it.
//
// What happens after somebody has decided. The surfaces they decided on are next door in
// toolbar-compose.js; this is the part that resolves a receiver, says what an agent
// reads, sends it, and leaves the screen ready for the answer to come back to.
//
// One send, several doors into it. A mark can go from its own popup, from the composer
// with several others, or on its own because a watched region moved while nobody was
// looking — and the last of those must leave a half-written note and a chosen tool
// exactly as it found them, which is the only thing that makes those three different.

// Deciding what to do with something you marked, and who gets it.
//
// The popup that opens on a finished mark, the tray of everything kept, the composer
// that sends a batch, and the menu of who could receive it. Apart from the page because
// it answers a different question: the rail is about pointing at things, and all of
// this is about what happens next.
//
// A classic script like the rest of the toolbar, sharing one global scope with it. It
// reads `el` and `state` from toolbar.js, which is loaded first.

/* ── what to do with what was marked ─────────────────────────────────────── */

/**
 * Put a mark back, exactly as undo would.
 *
 * There are three ways out of the popup — the cross, Escape, and a click anywhere off
 * it — and all of them mean the same thing, because somebody who wants out of a dialog
 * should not have to work out which exit destroys their work. None of them do: the mark
 * goes onto the redo trail, so a dismissal that was not meant is one keystroke from
 * being taken back.
 */
function cancelMark(id) {
  const at = state.marks.findIndex((mark) => mark.id === id);
  if (at >= 0) state.undone.push(state.marks.splice(at, 1)[0]);
  state.popup = null;
  render();
}

/**
 * The middle of everything being sent, in fractions of the overlay.
 *
 * One answer for a batch, placed over the things it was asked about. Sending three
 * marks and getting three identical pins would be three copies of one reply pretending
 * to be three answers.
 */
function middleOf(marks) {
  const spots = marks.flatMap((mark) =>
    mark.region
      ? [
          {
            x: mark.region.box.x + mark.region.box.w / 2,
            y: mark.region.box.y + mark.region.box.h / 2,
          },
        ]
      : mark.points,
  );
  if (spots.length === 0) return { x: 0.5, y: 0.5 };
  return {
    x: spots.reduce((sum, spot) => sum + spot.x, 0) / spots.length,
    y: spots.reduce((sum, spot) => sum + spot.y, 0) / spots.length,
  };
}

/** The marks that go in the next send, in the order they were made. */
function chosenMarks() {
  return state.marks.filter((mark) => mark.chosen);
}

/** Who is receiving, as the shell needs them named. */
function receiverNow() {
  const who = state.receiving;
  if (!who.id) return null;
  return { id: who.id };
}

/**
 * Send what was marked.
 *
 * The message is composed here rather than in the shell, because what an agent reads is
 * a decision about the marks somebody made and belongs beside them. The shell resolves
 * the receiver, attaches the pictures and reports what happened.
 */
/** Send marks to whoever is receiving. */
async function sendMarks(ids) {
  if (state.sending) return;
  const who = receiverNow();
  if (!who) {
    state.trouble = "Nobody is receiving. Choose a conversation first.";
    render();
    return;
  }
  const going = state.marks.filter((mark) => ids.includes(mark.id));
  // The gate, at the moment it means something. Reading is universal and changing a
  // surface is not, so a tool that writes is refused unless a connector owns what it
  // was pointed at — refused here, before anything is dispatched, so there is no path
  // where a write is attempted and then apologised for. Nothing writes yet; this is
  // what will stop the first one that does.
  const refused = going
    .map((mark) => gateFor(mark.tool, state.surface))
    .find((said) => said.blocked);
  if (refused) {
    state.trouble = refused.says;
    // Kept, rather than only announced and forgotten. Nothing ran and nothing changed,
    // but the marks and the words are still the work somebody did — and the way out is
    // to connect the surface, which they can do and then send again. The panel shows it
    // as blocked with the marks still attached; a toast that vanishes would take the
    // whole ask with it.
    state.history = [
      {
        at: Date.now(),
        who: state.receiving.name || who.id,
        sessionKey: null,
        said: state.text,
        count: going.length,
        answer: null,
        blocked: refused.says,
        // Ours, so it is written back and laid over the Gateway's list next time.
        mine: true,
        view: { open: true, shown: new Set() },
      },
      ...state.history,
    ];
    rememberWork();
    render();
    return;
  }
  // And the other way a mark can be unfinished: it asked for something out of a library
  // and never said which. Refused here for the same reason, before anything is sent.
  const waiting = unchosen(going);
  if (waiting) {
    state.trouble = waiting;
    render();
    return;
  }
  // Held before anything is dispatched: the words are cleared the moment the send lands,
  // and the record of what was sent is assembled after that.
  const said = state.text;
  state.sending = true;
  render();
  try {
    const sent = await invoke("colai_send", {
      receiver: who,
      message: summaryFor(going, state.mode, state.text, state.surface, state.files),
      markIds: ids,
      // Which of them would rather be one picture than several. Decided here rather than
      // in the capture, because it is the same decision the message states — and the two
      // must agree or the message names files that were never sent.
      sheets: going.filter(sheeted).map((mark) => mark.id),
      accent: accentNow(),
      // Only the ones that travel. What is named rather than carried is already in the
      // message as a path, and sending it twice would mean encoding a gigabyte to say
      // something the sentence above it already said.
      files: carrying(state.files)
        .filter((file) => file.carried)
        .map((file) => file.path),
    });
    /*
     * What this turn is called, so it can be put back.
     *
     * Minted in Rust at the moment of sending and known nowhere else; `rewind_files` is
     * addressed by it, so without holding it here "undo everything since my prompt" has no
     * prompt to name.
     */
    state.prompt = sent.prompt || null;

    // The message went; something it asked for may not have. This used to be the model
    // and the effort failing to apply, and neither travels any more — what is left is a
    // recording somebody asked to arrive as one contact sheet that had to go as separate
    // frames. Said rather than swallowed: they asked for one thing and got another.
    if (sent.settingsTrouble) {
      state.trouble = `Sent, but not quite as asked: ${sent.settingsTrouble}`;
    }
    /*
     * And whether everything the message named actually travelled.
     *
     * The message lists every file and picture it decided could go, and the Rust side
     * has always reported honestly how many did — but nobody read the numbers. A file
     * outside every work root, or a picture whose shot had aged out, arrived as a clean
     * send: the agent was told a log was attached, no log was attached, and it answered
     * about something it could not see.
     */
    const shortfall = [];
    if (sent.pictures < ids.length) {
      shortfall.push(`${ids.length - sent.pictures} picture(s) had already been let go`);
    }
    if (sent.refused && sent.refused.length) {
      shortfall.push(
        `${sent.refused.length} file(s) could not be read: ${sent.refused.join(", ")}`,
      );
    }
    if (shortfall.length) {
      state.trouble = `Sent, but ${shortfall.join("; ")} — so the message names more than arrived.`;
    }
    // Something is now working. Kept from here rather than waiting for the first frame
    // back: an agent that thinks for a minute before saying anything is working the
    // whole time, and a rail that only lights up once it starts talking is a rail that
    // was dark for the part somebody was wondering about.
    state.runs = [
      ...state.runs.filter((run) => run.sessionKey !== sent.sessionKey),
      { sessionKey: sent.sessionKey, who: state.receiving.name || who.id, heard: Date.now() },
    ];
    // Sending is the one moment somebody is watching for the light to come on, so it is
    // asked for rather than waited for.
    void watchEverything();
    // What went is gone; what was left unticked is still there, which is the whole
    // point of being able to untick it. Shown going, because a mark that vanishes at the
    // moment of sending is the one most likely to be read as a mark that was lost.
    flyToWork(going);
    state.marks = state.marks.filter((mark) => !ids.includes(mark.id));
    state.files = [];
    state.text = "";
    state.popup = null;
    state.open = null;
    // And put the tool away. A marking tool holds a sheet of glass over the whole desk
    // that swallows every click on it, which is what marking needs and is the opposite
    // of what somebody needs the moment they have finished. Sending is the end of the
    // gesture: what was marked has gone, and leaving the desktop deaf until they
    // thought to press Escape is not something anybody asked for.
    state.tool = "pointer";
    // Where to put the answer when it comes. The marks are about to be cleared, so the
    // place they were asking about has to be kept now or the reply has nowhere to land
    // — which was the whole trouble with this surface: you sent, and nothing ever came
    // back to the screen you were looking at.
    /*
     * Where the answer is going to appear, which is not always here.
     *
     * Three outcomes now. The toolbar's own agent has it and the reply comes back to this
     * panel — the ordinary case, and the only one that gets a pin. Or the conversation is
     * open in somebody's terminal and the mark went straight into it, which is the good
     * case and needs no warning at all: the answer will be on their screen in a moment.
     * Or the relay could not reach them and it is waiting for their next keystroke, which
     * is the one that has to be said out loud, because "sent" and "will arrive when you
     * next type there" are different promises.
     */
    // Only when the answer can actually come back. A pin waiting on a reply that will
    // never arrive here looks exactly like an agent still thinking, which is the one
    // thing it must not look like.
    const named = state.receiving.name || who.id;
    const answer = sent.watching
      ? { sessionKey: sent.sessionKey, at: middleOf(going), who: named, turns: [] }
      : null;
    if (answer) state.answers.push(answer);
    // And the same work, kept where it can be looked at afterwards.
    //
    // Built here rather than later because this is the last moment what went is still
    // known: the marks are cleared two lines down, and an entry assembled after that
    // would be an entry about pictures nobody can see any more. The answer is the same
    // object the pin holds, so a reply landing on one lands on both.
    state.history = [
      {
        at: Date.now(),
        who: named,
        sessionKey: sent.sessionKey,
        said,
        // How many went, so the panel can say "3 marks" for a send with no words. The
        // pictures themselves used to be kept here and nothing ever read them — a pile
        // of data URLs held for a number.
        count: going.length,
        // Named, so the panel can say `region` `arrow` rather than showing two grey
        // squares. Taken here because this is the last moment the marks still exist.
        marks: going.map((mark) => labelOf(mark)).filter(Boolean),
        answer,
        // Ours. The Gateway will list this conversation too, and what it cannot know —
        // which regions of which screen it was about — is laid over it from here.
        mine: true,
        // Open, and scrolled to below. What somebody just asked for is what they should
        // be looking at while it is being worked on.
        view: { open: true, shown: new Set() },
      },
      ...state.history,
    ];
    // Written now rather than on the next render: this is the moment the record changes,
    // and a restart between here and the next frame is exactly what it exists for.
    rememberWork();
    // And brought into view, once the panel has been drawn with it in.
    showLatestWork(sent.sessionKey);
    /*
     * What actually happened, in the words that are true of it.
     *
     * Three outcomes, not two. The toolbar's own agent has it and the answer comes back
     * here. Or it went somewhere colai cannot hear — the old case. Or, new: the receiver is
     * a chat open in a terminal, which colai cannot speak into, so the mark is waiting
     * there for the person's next message.
     *
     * That last one has to say so plainly. It is the difference between "sent" and "will
     * arrive when you next type", and reporting the second as the first is what makes
     * somebody sit watching a chat that is never going to say anything on its own.
     */
    state.trouble = sent.sentTo
      ? null
      : sent.handedTo
        ? `Left for ${sent.handedTo} — it arrives there the next time you send a message in that chat.`
        : sent.watching
          ? null
          : `Sent, but the reply will only be in ${state.receiving.name || who.id} — colai could not listen for it here.`;
  } catch (error) {
    state.trouble = `Could not send — ${error && error.message ? error.message : String(error)}`;
  } finally {
    state.sending = false;
    render();
  }
}

/** The popup that opens on a finished mark. */
