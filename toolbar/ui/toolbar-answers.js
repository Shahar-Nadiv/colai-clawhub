// What comes back, and what it is worth.
//
// The half of this surface that makes it a conversation rather than an outbox. It used
// to arrive as a pin on the region somebody marked and open beside it; now every mark
// is in the Work window and the reply is read there, so what is left here is reading a
// turn out of a message and sending a verdict back.
//
// The pin went with the marks. Once the screen is meant to be quiet, a circle left on it
// is the thing being complained about, not the exception to it.

function spokenBy(message) {
  if (!message || message.role !== "assistant") return null;
  const content = message.content;
  if (typeof content === "string") return content.trim() || null;
  if (!Array.isArray(content)) return null;
  const words = content
    .filter((part) => part && part.type === "text" && typeof part.text === "string")
    .map((part) => part.text.trim())
    .filter(Boolean)
    .join("\n\n");
  return words || null;
}

/**
 * Say something back.
 *
 * The conversation continues rather than ending. It used to close the pin on the way
 * out, which was right when the only two things sayable were "yes" and "no" and wrong
 * the moment somebody could answer a question — an answer usually gets a reply, and a
 * pin that vanished as you sent one would take the reply with it.
 */
async function verdict(answer, said) {
  const words = (said || "").trim();
  if (answer.saying || !words) return;
  answer.saying = true;
  render();
  try {
    await invoke("colai_send", {
      receiver: { kind: "session", id: answer.sessionKey, locator: null },
      message: words,
    });
    answer.saying = false;
    answer.saying_text = "";
    answer.turns = [...(answer.turns || []), { said: words, mine: true }];
    // And it is working again, on this.
    state.runs = [
      ...state.runs.filter((run) => run.sessionKey !== answer.sessionKey),
      { sessionKey: answer.sessionKey, who: answer.who, heard: Date.now() },
    ];
    render();
  } catch (error) {
    answer.saying = false;
    state.trouble = `Could not reply — ${error && error.message ? error.message : String(error)}`;
    render();
  }
}

/** Stop waiting on a conversation, and stop the Gateway talking to nobody. */
async function forgetAnswer(answer) {
  state.answers = state.answers.filter((held) => held !== answer);
  render();
  const still = state.answers.some((held) => held.sessionKey === answer.sessionKey);
  if (!still) await invoke("colai_unwatch", { sessionKey: answer.sessionKey }).catch(() => {});
}

/* ── the question, and the answers to it ─────────────────────────────────── */

/*
 * An agent that has asked something has stopped, and until this the only way to unstick
 * it from the toolbar was to open the Work window and type. Which is the wrong shape for
 * the thing that has happened: a question is an interruption, and an interruption that
 * has to be gone looking for is one that waits until somebody happens to look.
 *
 * So it comes out of the agents key — the one that says who is talking — with the
 * agent's own options on buttons. `choicesIn` reads those out of what it wrote; where it
 * cannot, the box is still there, because most questions are not multiple choice.
 */

/** Say one of the things the agent offered, and stop showing the question. */
async function answerAsked(answer, reply) {
  /*
   * Put aside before the send rather than after it.
   *
   * `verdict` waits on the Gateway, and a popup that stayed up through that would be a
   * question still visibly waiting on an answer that has already been given — and
   * clickable twice. Once the reply lands it is appended to the turns and `askedOf` stops
   * finding it, so this only covers the second or two in between; if the send fails,
   * `verdict` says so in the trouble line, which is where everything that fails to reach
   * the Gateway is said.
   */
  hideAsked();
  await verdict(answer, reply);
}

/** Not now. The badge on the agents key stays, and it opens again from there. */
function hideAsked() {
  const asked = askedOf(state.answers);
  if (!asked) return;
  state.pushedAside = { sessionKey: asked.answer.sessionKey, said: asked.said };
  render();
}

/** What the agents key's badge does: bring the question back. */
function showAsked() {
  state.pushedAside = null;
  render();
}

/**
 * The question on screen right now, if there is one.
 *
 * Which conversation is asking is a fact about the conversations; whether this one has
 * been waved away is a fact about the person. Both are needed and only the second is
 * remembered — the words as well as the session, so a second question on a conversation
 * whose first was dismissed opens on its own.
 */
function askedShowing() {
  const asked = askedOf(state.answers);
  if (!asked) return null;
  const aside = state.pushedAside;
  if (aside && aside.sessionKey === asked.answer.sessionKey && aside.said === asked.said) {
    return null;
  }
  return asked;
}

/**
 * "May I?" — the one thing on this rail that stops the world until it is answered.
 *
 * Drawn in the same place as the agent's questions because it is the same kind of moment,
 * and drawn from `askedFor`, which turns the request into the change itself rather than a
 * sentence about it.
 *
 * Returns true when it drew, so `drawAsk` knows to leave the panel alone: a permission
 * request outranks a question, because Claude Code is blocked on this one.
 */
function drawApproval() {
  // The confirmation takes the card over while it is up: it is a question about something
  // irreversible, and leaving the approval visible behind it offers two answers at once.
  if (state.undoing) return drawUndoAsk();
  const asking = state.asking;
  if (!asking) return false;
  const shown = askedFor(asking);
  if (!shown) return false;

  const bits = [];

  const head = document.createElement("div");
  head.className = "ask-head";
  const who = document.createElement("span");
  who.className = "ask-who";
  who.textContent = shown.where ? `${shown.tool} · ${shown.where}` : shown.tool;
  /*
   * The mode, where it matters most.
   *
   * Being asked is exactly the moment somebody forms an opinion about how often they want
   * to be asked, so the answer to that is here rather than in a settings panel they would
   * have to go looking for. It cycles, because there are four and a menu for four things
   * you press at most twice is more ceremony than the choice deserves.
   *
   * It applies to the NEXT tool, not this one — this one is already waiting on an answer,
   * and quietly re-deciding the question in front of somebody would be a worse surprise
   * than any it saves.
   */
  const mode = document.createElement("button");
  mode.type = "button";
  mode.className = "ask-mode";
  mode.dataset.act = "true";
  mode.textContent = MODE_SAID[state.allowing] || MODE_SAID.default;
  mode.title = "What may happen without being asked, from the next one on";
  mode.addEventListener("click", allowNext);
  head.append(who, mode);
  bits.push(head);

  if (shown.said) {
    const said = document.createElement("p");
    said.className = "appr-said";
    said.textContent = shown.said;
    bits.push(said);
  }

  if (shown.lines.length > 0) {
    const diff = document.createElement("div");
    diff.className = "ask-diff";
    for (const line of shown.lines) {
      const row = document.createElement("div");
      row.className = "ask-diff-row";
      row.dataset.sign = line.sign;
      const sign = document.createElement("span");
      sign.className = "ask-diff-sign";
      sign.textContent = line.sign;
      const text = document.createElement("span");
      text.className = "ask-diff-text";
      // textContent throughout: this is the model's words and a file's contents, and the
      // one surface in colai where being wrong about that ends in a write.
      text.textContent = line.said;
      row.append(sign, text);
      diff.append(row);
    }
    bits.push(diff);
    if (shown.whole) {
      const more = document.createElement("p");
      more.className = "ask-more";
      more.textContent = "…more than shown here.";
      bits.push(more);
    }
    if (shown.everywhere) {
      const all = document.createElement("p");
      all.className = "ask-more";
      all.textContent = "Every occurrence in the file, not only this one.";
      bits.push(all);
    }
  }

  /*
   * Undoing the whole prompt, which is a different thing from skipping one edit.
   *
   * Skipping refuses this call and lets the turn carry on, so anything already written
   * earlier in the turn stays written. This stops the turn and puts every file back to how
   * it was before the prompt — including anything a person edited by hand while the agent
   * was working, which is why it asks first and names what it would touch.
   */
  if (state.prompt) {
    const undo = document.createElement("button");
    undo.type = "button";
    undo.className = "ask-undo";
    undo.textContent = "Undo everything since my prompt";
    undo.addEventListener("click", askUndo);
    bits.push(undo);
  }

  const row = document.createElement("div");
  row.className = "appr-choices";
  // Ordered so the safe answer is the easy one when Claude Code says it should be.
  const choices = asking.defaultNo
    ? [["Skip this", false], ["Approve", true]]
    : [["Approve", true], ["Skip this", false]];
  for (const [label, allow] of choices) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "appr-choice";
    button.dataset.yes = String(allow);
    button.textContent = label;
    button.addEventListener("click", () => answerAsking(allow));
    row.append(button);
  }
  bits.push(row);

  el.flyAsk.hidden = false;
  el.flyAsk.replaceChildren(...bits);
  return true;
}

/** What a rewind would take back, named before anything is taken. */
function drawUndoAsk() {
  const undoing = state.undoing;
  const files = undoing.files;

  const head = document.createElement("div");
  head.className = "ask-head";
  const who = document.createElement("span");
  who.className = "ask-who";
  who.textContent = "Put the files back";
  head.append(who);

  const said = document.createElement("p");
  said.className = "appr-said";
  said.textContent =
    files === null
      ? "Working out what would change…"
      : files.length === 0
        ? "Nothing has been written since your prompt, so there is nothing to put back."
        : `${files.length} file${files.length === 1 ? "" : "s"} would go back to how they were before your prompt. Anything you edited yourself while this was working goes back too.`;

  const bits = [head, said];

  if (files && files.length > 0) {
    const list = document.createElement("div");
    list.className = "ask-diff";
    for (const file of files.slice(0, 20)) {
      const row = document.createElement("div");
      row.className = "ask-diff-row";
      row.textContent = withoutHome(String(file && file.path ? file.path : file));
      list.append(row);
    }
    bits.push(list);
  }

  const row = document.createElement("div");
  row.className = "appr-choices";
  const back = document.createElement("button");
  back.type = "button";
  back.className = "appr-choice";
  back.textContent = "Leave it";
  back.addEventListener("click", () => {
    state.undoing = null;
    render();
  });
  row.append(back);
  if (files && files.length > 0) {
    const go = document.createElement("button");
    go.type = "button";
    go.className = "appr-choice";
    go.dataset.yes = "true";
    go.textContent = `Put ${files.length} back`;
    go.addEventListener("click", doUndo);
    row.append(go);
  }
  bits.push(row);

  el.flyAsk.hidden = false;
  el.flyAsk.replaceChildren(...bits);
  return true;
}

/**
 * Ask what putting the files back would change, without changing anything.
 *
 * The dry run is the whole reason this is two presses rather than one: a rewind reverts
 * every file the agent touched during the turn, and cannot tell which of those somebody
 * edited themselves while it worked. Naming them first is the difference between undoing a
 * change and losing an afternoon.
 */
function askUndo() {
  if (!state.prompt) return;
  state.undoing = { asked: true, files: null };
  render();
  void invoke("colai_undo", { prompt: state.prompt, dryRun: true }).catch((trouble) => {
    state.undoing = null;
    state.trouble = `That could not be undone — ${String(trouble)}`;
    render();
  });
}

/** Stop the turn, then actually put them back. */
function doUndo() {
  if (!state.prompt) return;
  var prompt = state.prompt;
  state.undoing = null;
  state.asking = null;
  render();
  // Stopped first. Rewinding under an agent that is still writing is a race with a
  // filesystem, and the agent loses track of what it thinks it has done.
  void invoke("colai_stop", { sessionKey: state.sessionKey || "" })
    .catch(() => {})
    .then(function () {
      return invoke("colai_undo", { prompt: prompt, dryRun: false });
    })
    .catch(function (trouble) {
      state.trouble = `That could not be undone — ${String(trouble)}`;
      render();
    });
}

/** Move to the next mode along, and tell Claude Code so it holds for the next tool. */
function allowNext() {
  const order = Object.keys(MODES_ALLOWING);
  const at = order.indexOf(state.allowing);
  const next = order[(at + 1) % order.length];
  state.allowing = next;
  render();
  void invoke("colai_allow_now", { mode: next }).catch((trouble) => {
    // Put back what it actually is. A chip that says one thing while the session is in
    // another is worse than no chip: it is a promise about what will happen next.
    state.trouble = `That mode did not take — ${String(trouble)}`;
    state.allowing = "default";
    render();
  });
}

/** Send the answer, and stop showing the question whatever happens to it. */
function answerAsking(allow) {
  const asking = state.asking;
  if (!asking || !asking.id) return;
  state.asking = null;
  render();
  void invoke("colai_answer", { id: asking.id, allow, message: allow ? "" : "You turned this down." })
    .catch((trouble) => {
      // The turn is stopped on the other end. Saying nothing here would leave somebody
      // watching a conversation that never moves, with no idea they are the reason.
      state.trouble = `That answer did not reach Claude Code — ${String(trouble)}`;
      render();
    });
}

function drawAsk() {
  if (drawApproval()) return;
  const showing = askedShowing();
  el.flyAsk.hidden = showing === null;
  if (!showing) {
    el.flyAsk.replaceChildren();
    return;
  }
  const { answer, said: asking } = showing;

  const head = document.createElement("div");
  head.className = "ask-head";
  const who = document.createElement("span");
  who.className = "ask-who";
  who.textContent = `${answer.who} is asking`;
  const shut = document.createElement("button");
  shut.type = "button";
  shut.className = "ask-shut";
  shut.title = "Not now";
  shut.setAttribute("aria-label", "Not now");
  shut.textContent = "×";
  shut.addEventListener("click", hideAsked);
  head.append(who, shut);

  const bits = [head];

  // The question without the options under it, because the options are about to be the
  // buttons. Empty when the agent wrote nothing but a list, and then the buttons are the
  // whole of it.
  const said = questionIn(asking);
  if (said) {
    const line = document.createElement("p");
    line.className = "ask-said";
    line.textContent = said;
    bits.push(line);
  }

  const choices = choicesIn(asking);
  if (choices.length) {
    const row = document.createElement("div");
    row.className = "ask-choices";
    for (const choice of choices) {
      const one = document.createElement("button");
      one.type = "button";
      one.className = "ask-choice";
      one.textContent = choice.label;
      // The whole line, including whatever the agent explained after the dash. The button
      // is read in a glance; the agent is not, and the shortened version would be a
      // different answer.
      one.title = choice.reply;
      one.disabled = Boolean(answer.saying);
      one.addEventListener("click", () => void answerAsked(answer, choice.reply));
      row.append(one);
    }
    bits.push(row);
  }

  /*
   * And a box, always.
   *
   * Not only for the questions with no readable options in them: an agent offering three
   * things is often offering the wrong three, and a popup that could only ever say one
   * of them would make the toolbar worse at answering than the chat it is standing in
   * for. It shares `saying_text` with the Work window's reply box, so a half-written
   * answer is the same half-written answer in both.
   */
  const own = document.createElement("div");
  own.className = "ask-own";
  const field = document.createElement("textarea");
  field.className = "popup-note ask-write";
  field.dataset.field = `say:${answer.sessionKey}`;
  field.rows = 2;
  field.placeholder = choices.length ? "…or say something else" : "Answer them…";
  field.value = answer.saying_text || "";
  /*
   * `/` and `@` here too, because this box is a prompt like any other.
   *
   * What goes in it is handed to the conversation exactly as typed, so a `/` command in a
   * reply is a command Claude Code runs and an `@` path is a file it reads — the same two
   * keystrokes doing the same two things they do in a terminal. They worked only in the
   * composer, which meant the one box that opens by itself, while an agent is stopped and
   * waiting, was the one where the keys somebody reached for did nothing.
   */
  const box = document.createElement("div");
  box.className = "ask-box";
  const menu = document.createElement("div");
  menu.className = "ask-menu";
  menu.hidden = true;
  box.append(field, menu);
  // The same holder the Work panel's reply box uses for this answer. They are never on
  // screen together — opening one closes the other — and they already share the half-written
  // words, so a list half chosen in one is the same list in the other.
  completes(field, menu, replying(askingOn(answer), (said) => (answer.saying_text = said)));
  const go = document.createElement("button");
  go.type = "button";
  go.className = "ask-send";
  go.dataset.lead = "true";
  go.disabled = Boolean(answer.saying) || !(answer.saying_text || "").trim();
  go.textContent = answer.saying ? "Sending…" : "Reply";
  go.addEventListener("click", () => void answerAsked(answer, answer.saying_text || ""));
  // Not a render: this panel redraws whole, and rebuilding the field on every keystroke
  // takes the cursor with it. The button beside it is the only thing that has to follow.
  field.addEventListener("input", () => {
    answer.saying_text = field.value;
    go.disabled = Boolean(answer.saying) || !field.value.trim();
  });
  own.append(box, go);
  bits.push(own);

  el.flyAsk.replaceChildren(...bits);
}
