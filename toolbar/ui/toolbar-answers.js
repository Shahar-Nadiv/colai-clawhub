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

function drawAsk() {
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
  own.append(field, go);
  bits.push(own);

  el.flyAsk.replaceChildren(...bits);
}
