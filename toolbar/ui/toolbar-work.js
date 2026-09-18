// Where the work lives, once it has stopped living on somebody's screen.
//
// The toolbar used to put everything on the desktop: the marks while you composed, the
// answer pins afterwards. That was right when a mark was one question asked and
// answered. It stopped being right once you could make four marks, send them, get a
// reply and go back to an earlier prompt — because by then there is a body of work, and
// the only place it lived was scattered over whatever somebody was actually doing.
//
// So the screen goes quiet and this holds it: what is waiting, what has been sent, what
// came back, and the way back to an earlier prompt. One window, because the whole
// complaint was that it was in several places at once.

/**
 * Open it, close it, or the other one.
 *
 * The send key opens this rather than a flyout. There is one place where work is
 * assembled, and a second place to assemble it in would be the thing this replaces.
 */
function toggleWork() {
  state.work.open = !state.work.open;
  if (state.work.open) {
    state.open = null;
    reachTheKeyboard();
  }
  render();
}

/**
 * Ask the window manager for the keyboard, because this window is scenery until it does.
 *
 * The overlay is hinted as a dock so the shell stacks it above everything and keeps it
 * out of the switcher, and a dock is not something a window manager hands the keyboard
 * to. Only marking used to ask — which made marking a toll on writing: open the panel
 * with nothing marked, click the field, type a sentence, and every keystroke went to
 * whatever was behind the overlay. The composer is the panel's main field and it is
 * there whether anything is marked or not, so opening the panel is when to ask.
 *
 * Asked on opening rather than on every render: taking somebody's keyboard at any moment
 * other than the one they asked for this window would be the overlay behaving like an
 * application.
 */
function reachTheKeyboard() {
  void invoke("colai_take_keyboard").catch(() => {});
}

function closeWork() {
  state.work.open = false;
  render();
}

function openWork() {
  state.work.open = true;
  state.open = null;
  reachTheKeyboard();
  render();
  // Into the box, because opening this panel is almost always the first half of writing
  // in it. Nothing focused anything, so every visit began with a click somebody should
  // not have had to make — and for a keyboard user, with a tab through the whole log.
  //
  // After the render that builds it: the field does not exist until then.
  const write = el.work.querySelector(".work-write [data-field]");
  if (write) write.focus({ preventScroll: true });
}

/**
 * Keep the clock in the panel honest while somebody watches it.
 *
 * Every elapsed time is worked out when the row is drawn, and nothing redraws on its
 * own — so an exchange sent a moment ago said "just now" for as long as the panel stayed
 * open, and a run going for ten minutes still claimed to have started a moment ago. The
 * panel is a thing people leave open, which is exactly when a frozen clock is worst.
 *
 * Ticking only while it is open, and only while something might change: once every
 * exchange is old enough to be counted in minutes, a redraw a second is redrawing the
 * same words. Half a minute is finer than the smallest thing `agoSaid` says.
 */
const WORK_TICK = 30 * 1000;
let ticking = null;

function keepTime() {
  const wanted = state.work.open && state.history.length > 0;
  if (wanted === (ticking !== null)) return;
  if (!wanted) {
    clearInterval(ticking);
    ticking = null;
    return;
  }
  ticking = setInterval(moveTheClock, WORK_TICK);
}

/**
 * Move every elapsed time on, and touch nothing else.
 *
 * This used to redraw the panel. Redrawing replaces its children, which includes the
 * field somebody may be typing into — so twice a minute, mid-sentence, the caret jumped
 * to the end of what they had written. Two words on the right-hand side of a header are
 * not worth that, and they do not need it: each carries the instant it is counting from.
 */
function moveTheClock() {
  if (!state.work.open) return;
  const now = Date.now();
  for (const when of el.work.querySelectorAll(".work-when[data-at]")) {
    const at = Number(when.dataset.at);
    if (!Number.isFinite(at)) continue;
    when.textContent = briefly(at, now);
    when.title = agoSaid(at, now);
  }
}

function drawWork() {
  const open = state.work.open;
  el.work.hidden = !open;
  if (!open) {
    keepTime();
    return;
  }

  keepTime();
  const now = Date.now();
  const here = shownWork();
  const waiting = needingYou(here, state.runs);
  const shown = state.work.filter === "needs" ? waiting : here;

  // Where in the list somebody had got to. `replaceChildren` builds a new scrolling
  // element at the top, so without this the panel throws you back to the first row every
  // time anything redraws.
  const wasAt = el.work.querySelector(".work-log")?.scrollTop ?? 0;
  el.work.replaceChildren(
    workHead(waiting.length),
    ...(waiting.length && state.work.filter !== "needs" ? [waitingBanner(waiting[0])] : []),
    workLog(shown, now),
    workWrite(),
  );
  const log = el.work.querySelector(".work-log");
  if (log) log.scrollTop = wasAt;
  // The cursor is put back by `render`, which wraps this and every other panel — the
  // boxes somebody writes in are not all in here, and one panel minding only its own
  // was how a note in the mark popup kept losing the caret.
}

/**
 * The head: what is here, and a way to see only the part that is stuck.
 *
 * The count is the reason to glance at the panel at all, so it is next to the name
 * rather than discovered by scrolling.
 */
function workHead(waiting) {
  const head = document.createElement("div");
  head.className = "work-head";

  const title = document.createElement("div");
  title.className = "work-title";
  const name = document.createElement("span");
  name.className = "work-name";
  name.textContent = "Work";
  const count = document.createElement("span");
  count.className = "work-count";
  count.textContent = workCountSaid(shownWork(), state.runs);
  title.append(name, count);

  // Two tabs, and only once there is something to filter to. A "Needs you · 0" tab is a
  // control that does nothing, sitting where the eye goes first.
  const tabs = document.createElement("div");
  tabs.className = "work-tabs";
  if (waiting > 0) {
    for (const [id, label] of [
      ["all", "All"],
      ["needs", `Needs you · ${waiting}`],
    ]) {
      const tab = document.createElement("button");
      tab.type = "button";
      tab.className = "work-tab";
      tab.dataset.on = String((state.work.filter || "all") === id);
      tab.textContent = label;
      tab.addEventListener("click", () => {
        state.work.filter = id;
        render();
      });
      tabs.append(tab);
    }
  }

  /*
   * How wide the panel is looking.
   *
   * The panel has always shown only the receiver's conversations and never said so, which
   * is the difference between a filtered list and a list that has lost things. Two words
   * settle it — and make the narrow view a choice somebody can see rather than a wall.
   *
   * On this host the axis is the conversation, not the agent. There is one Claude, so
   * "this agent" would be every row there is; what somebody actually wants narrowed to is
   * the conversation they are pointed at. The filter already did that — the receiver is
   * always a session here — so only the words were wrong.
   *
   * Only worth showing once there is something outside the filter to look at.
   */
  const beyond = state.history.length - shownWork().length;
  if (beyond > 0 || state.work.scope === "all") {
    for (const [id, label, why] of [
      ["mine", "This one", "Only the conversation you are pointed at"],
      ["all", `Everything · ${state.history.length}`, "Every conversation colai can see"],
    ]) {
      const pick = document.createElement("button");
      pick.type = "button";
      pick.className = "work-tab";
      pick.dataset.on = String((state.work.scope || "mine") === id);
      pick.title = why;
      pick.textContent = label;
      pick.addEventListener("click", () => {
        state.work.scope = id;
        render();
      });
      tabs.append(pick);
    }
  }

  const shut = document.createElement("button");
  shut.type = "button";
  shut.className = "popup-shut";
  shut.title = "Close";
  shut.setAttribute("aria-label", "Close the work panel");
  shut.textContent = "×";
  shut.addEventListener("click", closeWork);

  head.append(title, tabs, shut);
  return head;
}

/**
 * One line naming the exchange that has stopped, and a way to reach it.
 *
 * The whole panel can be scrolled past; a question four exchanges down is a question
 * nobody answers. This is the only thing in the panel that jumps somewhere.
 */
function waitingBanner(entry) {
  const bar = document.createElement("div");
  bar.className = "work-banner";
  const said = document.createElement("span");
  said.textContent = `${entry.who} is waiting on an answer`;
  const jump = document.createElement("button");
  jump.type = "button";
  jump.className = "work-jump";
  jump.textContent = "Jump";
  jump.addEventListener("click", () => {
    const row = rowFor(entry.sessionKey);
    if (row) row.scrollIntoView({ block: "center", behavior: still() ? "auto" : "smooth" });
  });
  bar.append(said, jump);
  return bar;
}

/** The drawn row for a conversation, if it is on screen. */
function rowFor(sessionKey) {
  return sessionKey ? el.work.querySelector(`[data-entry="${CSS.escape(sessionKey)}"]`) : null;
}

/**
 * Put the newest piece of work in front of whoever just made it.
 *
 * After the render, because the row it scrolls to is one the render is about to build.
 */
function showLatestWork(sessionKey) {
  requestAnimationFrame(() => {
    const row = rowFor(sessionKey);
    if (row) row.scrollIntoView({ block: "nearest", behavior: still() ? "auto" : "smooth" });
  });
}

function workLog(shown, now) {
  const log = document.createElement("div");
  log.className = "work-log scrolls";
  if (shown.length === 0) {
    log.append(shownWork().length === 0 ? nothingYet() : saying("Nothing is waiting on you."));
    return log;
  }
  for (const entry of shown) log.append(entryRow(entry, now));
  return log;
}

/**
 * The first thing anyone sees, and it was one grey sentence.
 *
 * Three steps because there are three, and somebody who has just opened this has done
 * none of them. The composer stays below it: the point of the panel is still to send.
 */
function nothingYet() {
  const box = document.createElement("div");
  box.className = "work-empty";
  const title = document.createElement("p");
  title.className = "work-empty-title";
  const said = document.createElement("p");
  said.className = "work-empty-said";
  /*
   * An empty list and a list nobody answered look identical, and they are not the same
   * news at all: one means there is no work, the other means the panel cannot see it.
   * The panel is a view of OpenClaw's conversations now, so "nothing yet" is a claim
   * about OpenClaw — and it must not be made when nobody has answered.
   */
  if (state.workTrouble) {
    title.textContent = "Could not read the conversations";
    said.textContent = `${state.workTrouble} — the toolbar will keep asking.`;
    box.append(title, said);
    return box;
  }
  title.textContent = "Nothing sent yet";
  // Marking led, and led wrongly: it made pointing at the screen a step you had to take
  // before you were allowed to ask for anything. It is the toolbar's own trick, not its
  // toll — the field below sends words on their own.
  said.textContent =
    "Say what you want done and send it. Mark something on screen first when the words need a picture.";
  const steps = document.createElement("ol");
  steps.className = "work-steps";
  for (const step of [
    "Write the ask below",
    "Pick who receives it",
    "Point at, draw or box a region — only if it helps",
  ]) {
    const one = document.createElement("li");
    one.textContent = step;
    steps.append(one);
  }
  box.append(title, said, steps);
  return box;
}

/**
 * One exchange: what went, what came back, and what to do about it.
 *
 * A passage of text on a spine, not a card. Everything visible follows from the state —
 * the pill, the colour of the node, and which actions are worth offering — so there is
 * one place to change what any of them mean.
 */
function entryRow(entry, now) {
  const state_ = stateOf(entry, state.runs);
  const row = document.createElement("div");
  row.className = "work-turn";
  row.dataset.state = state_;
  /*
   * Addressed by what it is, not by when it last moved.
   *
   * This was `entry.at` — the session's last-activity time, which changes whenever the
   * conversation does and is also the sort key. Two sessions touched in the same second
   * collide, and `querySelector` silently takes the first.
   */
  row.dataset.entry = entry.sessionKey || `blocked:${entry.at}`;

  const node = document.createElement("span");
  node.className = "work-node";

  const meta = document.createElement("div");
  meta.className = "work-meta";
  const face = document.createElement("span");
  face.className = "work-face";
  face.textContent = (entry.who || "?").slice(0, 1).toUpperCase();
  face.style.setProperty("--face", String(handHue(entry.who || "")));
  const who = document.createElement("span");
  who.className = "work-who";
  who.textContent = entry.who;
  const pill = document.createElement("span");
  pill.className = "work-pill";
  pill.textContent = STATES[state_].label;
  const when = document.createElement("span");
  when.className = "work-when";
  // Carried on the element so the clock can be moved on without redrawing the panel
  // around it. See `keepTime`.
  when.dataset.at = String(entry.at);
  when.textContent = briefly(entry.at, now);
  when.title = agoSaid(entry.at, now);
  meta.append(face, who, pill, when);
  row.append(node, meta);

  // What was marked, named rather than pictured. A thumbnail says which screenshot; the
  // word says which tool, and the tool is what somebody remembers about a mark.
  if (entry.marks && entry.marks.length) {
    const marks = document.createElement("div");
    marks.className = "work-marks";
    for (const named of entry.marks) {
      const one = document.createElement("span");
      one.className = "work-mark";
      one.textContent = named;
      marks.append(one);
    }
    row.append(marks);
  }

  // What was asked, folded away. It is context for the reply below it and you wrote it,
  // so it is the one thing here you already know — but it has to stay reachable, because
  // an hour later it is the only way to know what the reply is about.
  row.append(askLine(entry));

  /*
   * The replies, and only while the row is open.
   *
   * They used to be drawn unconditionally, so the fold controlled nothing but the
   * chevron's rotation and the clamp on the ask line above it. Pressing it a second time
   * turned the arrow back and left the whole transcript on screen, with no code path
   * anywhere that removed it — which is exactly "cannot fold after expand". One flag,
   * meaning one thing.
   */
  const turns = (entry.view.open && entry.answer && entry.answer.turns) || [];
  for (const [at, turn] of turns.entries()) {
    row.append(turnSaid(entry, turn, at));
  }
  if (state_ === "blocked") {
    const why = document.createElement("p");
    why.className = "work-said";
    why.textContent = entry.blocked;
    row.append(why);
  }

  const receipt = receiptRow(entry, state_);
  if (receipt) row.append(receipt);

  // One row of buttons, not two. The reply controls and the row's own actions are the
  // same question — what do I do about this — and splitting them put Rewind on a line of
  // its own underneath No/Yes/Reply.
  const acts = actionsFor(entry, state_);
  if (state_ === "asking" && entry.answer) {
    const box = answerBox(entry.answer, acts);
    row.append(box);
  } else {
    row.append(acts);
  }
  return row;
}

/**
 * One turn of the conversation, folded if it is long.
 *
 * An agent's reply has no length anybody agreed to, and up to forty of them arrive at
 * once — one long answer used to push every other row out of the panel. So a reply is
 * clamped, and says so with a control that opens it.
 *
 * Which ones are open lives on the entry's `view`, so it survives the refresh that
 * rebuilds these rows every few seconds. Prompts are never clamped: they are short by
 * construction, and they are the thing somebody is scanning the list for.
 */
function turnSaid(entry, turn, at) {
  const line = document.createElement("p");
  line.className = "work-said";
  line.dataset.mine = String(turn.mine === true);
  line.textContent = turn.said;
  if (turn.mine === true || (turn.said || "").length < TURN_FOLDS_OVER) {
    return line;
  }
  const open = entry.view.shown.has(at);
  line.dataset.open = String(open);
  const more = document.createElement("button");
  more.type = "button";
  more.className = "work-more";
  more.textContent = open ? "Show less" : "Show more";
  more.addEventListener("click", () => {
    if (entry.view.shown.has(at)) {
      entry.view.shown.delete(at);
    } else {
      entry.view.shown.add(at);
    }
    render();
  });
  const held = document.createElement("div");
  held.className = "work-turn";
  held.append(line, more);
  return held;
}

/**
 * How long a reply has to be before it is worth folding.
 *
 * Measured in characters rather than lines because the page cannot know how many lines
 * something will take until it has laid it out, and a control that appears after a reflow
 * is a control that moves under the pointer. Roughly the six lines the clamp allows at
 * this width.
 */
const TURN_FOLDS_OVER = 420;

/** The ask, one line, opening on a press. */
function askLine(entry) {
  const said = entrySaid(entry);
  const fold = document.createElement("button");
  fold.type = "button";
  fold.className = "work-ask";
  fold.dataset.open = String(entry.view.open);
  fold.setAttribute("aria-expanded", String(entry.view.open));
  // Named, so the pair that puts focus back after a redraw covers it. Without this a
  // keyboard user loses their place on the panel every time they press the fold.
  fold.dataset.field = `ask:${entry.sessionKey || entry.at}`;
  const mark = document.createElement("span");
  mark.className = "work-ask-mark";
  mark.textContent = "›";
  const words = document.createElement("span");
  words.className = "work-ask-said";
  words.textContent = said;
  fold.append(mark, words);
  fold.addEventListener("click", () => {
    entry.view.open = !entry.view.open;
    // Opening a conversation is when its transcript is worth fetching. Almost every row
    // in this panel is one nobody opens, and drawing the list never needed one.
    if (entry.view.open) void loadTurns(entry);
    render();
  });
  return fold;
}

/**
 * What actually happened, in the agent's own terms.
 *
 * The design draws this as `code-bridge · hero.html:38 · +6 −2`, which is the right
 * thing to show and more than colai can currently know: no connector reports the file it
 * touched or how much it changed. So the row says the part that is true — who did it,
 * and through what — and the rest waits for a connector that reports it. A plausible
 * `+6 −2` that nothing measured would be worse than no row at all.
 */
function receiptRow(entry, state_) {
  if (state_ === "working" || state_ === "blocked") return null;
  const through = entry.answer && entry.answer.connector;
  if (!through && state_ !== "done") return null;
  const line = document.createElement("p");
  line.className = "work-receipt";
  const tick = document.createElement("span");
  tick.className = "work-receipt-mark";
  tick.textContent = state_ === "failed" ? "×" : "✓";
  const said = document.createElement("span");
  said.textContent = through ? `${through} · ${entry.who}` : entry.who;
  line.append(tick, said);
  // The design puts `+6 −2` at the end of this row, and only a connector that reports
  // what it wrote can fill it in. Drawn when there is a number and left out when there
  // is not, rather than shown as zeros nothing measured.
  const wrote = entry.answer && entry.answer.wrote;
  if (wrote && (wrote.more || wrote.less)) {
    const count = document.createElement("span");
    count.className = "work-receipt-count";
    const more = document.createElement("span");
    more.className = "work-receipt-more";
    more.textContent = `+${wrote.more || 0}`;
    const less = document.createElement("span");
    less.className = "work-receipt-less";
    less.textContent = `−${wrote.less || 0}`;
    count.append(more, less);
    line.append(count);
  }
  return line;
}

/** What to offer, which is a question about the state and nothing else. */
function actionsFor(entry, state_) {
  const acts = document.createElement("div");
  acts.className = "work-acts";
  const add = (label, lead, why, go) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "work-act";
    button.dataset.lead = String(Boolean(lead));
    button.textContent = label;
    if (why) button.title = why;
    button.addEventListener("click", go);
    acts.append(button);
  };

  if (state_ === "working") {
    add("Stop", false, "Stop this run", () => void stopOne(entry.sessionKey));
  }
  if (state_ === "blocked") {
    add("Discard", false, "Forget this one", () => {
      state.history = state.history.filter((one) => one !== entry);
      render();
    });
  }
  if (state_ === "failed") {
    add("Try again", true, "Send it again, unchanged", () => void resend(entry));
  }
  /*
   * No Rewind on these rows.
   *
   * Going back to an earlier prompt is Claude Code's own `/rewind`, in the session the
   * person is looking at. A second way to do it from a toolbar would be a second idea of
   * where a conversation currently is — and the two would disagree the first time somebody
   * used both.
   */
  return acts;
}

/**
 * A reply in words, because most of what an agent says back is not a proposal to accept
 * or refuse. It asks which of two things you meant, or what a value should be — and
 * neither has an answer that fits in two fixed buttons.
 */
function answerBox(answer, acts) {
  const box = document.createElement("div");
  box.className = "work-answer";
  const field = document.createElement("textarea");
  field.className = "popup-note answer-say";
  field.dataset.field = `say:${answer.sessionKey}`;
  field.rows = 2;
  field.placeholder = "Answer them…";
  field.value = answer.saying_text || "";
  /*
   * With `/` and `@` in it, like every other box on this page.
   *
   * A reply reaches the conversation exactly as typed, so a command here is a command that
   * runs and a path is a file that gets read. Its own `.ask-box` because that is what the
   * menu is positioned against, and this one sits inside a list that scrolls — see the
   * measurement in `completes`, which is what keeps the menu out of the clipped half.
   */
  const asked = document.createElement("div");
  asked.className = "ask-box";
  const menu = document.createElement("div");
  menu.className = "ask-menu";
  menu.hidden = true;
  asked.append(field, menu);
  completes(field, menu, replying(askingOn(answer), (said) => (answer.saying_text = said)));
  // The row's own actions, with the quick replies put in front of them.
  const foot = acts;
  const first = [];
  /*
   * The agent's own options, where it wrote any, and yes-or-no where it did not.
   *
   * This used to be a fixed *Yes* and *No* whatever had been asked, which answers a
   * question the agent mostly was not asking — most of what comes back is *which of
   * these three*, and neither button is one of the three. `choicesIn` reads them out of
   * what it wrote, and is the same decision the popup off the conversation key makes, so the
   * two surfaces never offer different answers to the same question.
   */
  const quicks = choicesIn(lastTurn(answer) || "").map((choice) => [
    choice.label,
    choice.reply,
    choice.reply,
  ]);
  for (const [label, words, why] of quicks.length
    ? quicks
    : [
        ["No", "Declined — that is not what I meant.", "Tell them this is not it"],
        ["Yes", "Accepted — go ahead.", "Tell them to go ahead"],
      ]) {
    const quick = document.createElement("button");
    quick.type = "button";
    quick.className = "work-act";
    quick.disabled = Boolean(answer.saying);
    quick.textContent = label;
    quick.title = why;
    quick.addEventListener("click", () => void verdict(answer, words));
    first.push(quick);
  }
  const go = document.createElement("button");
  go.type = "button";
  go.className = "work-act";
  go.dataset.lead = "true";
  go.disabled = Boolean(answer.saying) || !(answer.saying_text || "").trim();
  go.textContent = answer.saying ? "Sending…" : "Reply";
  go.addEventListener("click", () => void verdict(answer, answer.saying_text || ""));
  first.push(go);

  // Not a render. This panel redraws whole, so a render on every keystroke rebuilt the
  // field somebody was typing into and took the focus with it — sixteen characters
  // typed, one kept. The only thing that has to follow the words is the button beside
  // them, so the button is the only thing that moves.
  field.addEventListener("input", () => {
    answer.saying_text = field.value;
    go.disabled = Boolean(answer.saying) || !field.value.trim();
  });
  // No, Yes, Reply, then whatever the row already offered — the order somebody reads
  // them in, and built in one go so the loop above cannot reverse it.
  foot.prepend(...first);
  box.append(asked, foot);
  return box;
}

/** Stop one run rather than all of them, which is what a row can offer. */
async function stopOne(sessionKey) {
  if (!sessionKey) return;
  const run = state.runs.find((one) => one.sessionKey === sessionKey);
  state.runs = state.runs.filter((one) => one.sessionKey !== sessionKey);
  render();
  try {
    await invoke("colai_stop", { sessionKey });
    // Said, not assumed. A stop that produced no answer looks exactly like a stop that
    // did not happen, and somebody who pressed it needs to know which.
    say(`Stopped ${run ? run.who : "that run"}.`, "receipt");
  } catch (error) {
    state.trouble = `Could not stop ${run ? run.who : "that run"} — ${error && error.message ? error.message : String(error)}`;
  }
  render();
}

/**
 * Put a failed ask back in the composer rather than sending it again behind their back.
 *
 * A run that fell over usually fell over for a reason, and re-dispatching the same words
 * without showing them is how somebody watches the same failure twice. The words come
 * back, the cursor is in them, and Send is one press away.
 */
function resend(entry) {
  state.text = entrySaid(entry);
  state.work.filter = "all";
  render();
  const field = el.work.querySelector(".work-write .popup-note");
  if (field) field.focus();
}

/**
 * The composer, at the foot where you type.
 *
 * Below the log rather than above it, so the reply you are answering sits directly over
 * the field. It used to be a staging area at the top with history pushed beneath it,
 * which put the newest thing furthest from the words about it.
 */
function workWrite() {
  // A band across the foot of the panel, so the card inside it floats rather than
  // butting up against the log above.
  const tail = document.createElement("div");
  tail.className = "work-tail";
  const write = document.createElement("div");
  write.className = "work-write";
  drawComposer(write);
  tail.append(write);
  return tail;
}

/**
 * A mark going where marks go.
 *
 * The screen is quiet now, which is what was asked for and which has one cost: a mark
 * that simply stops being drawn reads as a mark that was lost. So it visibly *goes*
 * somewhere — to the window when it is open, to the key that opens it when it is not —
 * and where it went is taught once, by watching, without a sentence about it.
 *
 * Decoration is not the point. This is the only thing standing between "my marks
 * disappeared" and knowing where they are.
 *
 * Painted, not caught: these are inert and last a third of a second, so they are left
 * out of the clickable region entirely rather than briefly claiming a strip of desktop.
 */
const FLIGHT = 380;

function flyToWork(marks) {
  // A desktop that asked for less movement is not told about this in motion; the marks
  // are in the window either way, and the window is one press from here.
  if (still() || typeof el.flights.animate !== "function") return;
  const going = marks.filter((mark) => badgeAt(mark));
  if (going.length === 0) return;
  const screen = screenSize();
  const nest = state.work.open ? el.work : buttons.send;
  const to = nest.getBoundingClientRect();
  for (const mark of going) {
    const spot = badgeAt(asDrawn(mark, state.front, screen));
    const flying = document.createElement("span");
    flying.className = "flight";
    if (mark.thumb) {
      const picture = document.createElement("img");
      picture.src = mark.thumb;
      picture.alt = "";
      flying.append(picture);
    }
    flying.style.left = `${spot.x * screen.width}px`;
    flying.style.top = `${spot.y * screen.height}px`;
    el.flights.append(flying);
    const across = to.left + to.width / 2 - spot.x * screen.width;
    const down = to.top + to.height / 2 - spot.y * screen.height;
    const run = flying.animate(
      [
        { transform: "translate(-50%, -50%) scale(1)", opacity: 1 },
        {
          transform: `translate(calc(-50% + ${across}px), calc(-50% + ${down}px)) scale(0.25)`,
          opacity: 0,
        },
      ],
      { duration: FLIGHT, easing: "cubic-bezier(0.4, 0, 0.2, 1)" },
    );
    const drop = () => flying.remove();
    run.finished.then(drop, drop);
  }
}

/*
 * The work outlives the toolbar, because it was never the toolbar's to begin with.
 *
 * The panel used to be a record this page kept of its own sends, and nothing else. So a
 * restart emptied it, and the first version of this fix made it worse in a way that was
 * hard to see: it remembered what colai had sent, then asked the Gateway for the replies
 * *to those conversations only*. With nothing remembered there were no conversations to
 * ask about, and the panel said "nothing yet" over a Gateway full of work.
 *
 * The relationship is the other way round. **OpenClaw holds the conversations. The
 * toolbar holds the few things only it knows about them** — which marks travelled, what
 * region was pointed at, whether a send was refused before it left. So the list comes
 * from `colai_sessions`, and what is remembered here is laid over the top of it.
 */

/** What survives a restart: what only this toolbar knew, never the conversation itself. */
const WORK_REMEMBERED = "colai.toolbar.work";

/**
 * How much of that is kept.
 *
 * It is a handful of fields per send, not a transcript — the transcripts are OpenClaw's
 * and are asked for. This bounds what goes into `localStorage`, which is a few megabytes
 * for the whole origin and shared with where the rail was put.
 */
const WORK_KEPT = 200;

/** Sends this toolbar made, by session key, so they can be laid over the Gateway's list. */
let ourOwnWork = [];

/** What was sent from here, written the moment it lands rather than on the next render. */
function rememberWork() {
  ourOwnWork = state.history
    .filter((entry) => entry.mine)
    .slice(0, WORK_KEPT)
    .map((entry) => ({
      at: entry.at,
      sessionKey: entry.sessionKey,
      said: entry.said,
      count: entry.count,
      marks: entry.marks,
      blocked: entry.blocked,
    }));
  try {
    window.localStorage.setItem(WORK_REMEMBERED, JSON.stringify(ourOwnWork));
  } catch {
    // A full or refusing store is not a reason to stop working. The conversations are
    // still OpenClaw's and still listed; only the marks beside them would be missing.
  }
}

/** And read back, before anything is drawn. */
function recallWork() {
  try {
    const kept = JSON.parse(window.localStorage.getItem(WORK_REMEMBERED) || "[]");
    ourOwnWork = Array.isArray(kept) ? kept.filter((one) => one && one.sessionKey) : [];
  } catch {
    ourOwnWork = [];
  }
}

/**
 * Every conversation OpenClaw knows, newest first, with what colai knows laid over it.
 *
 * One round trip for the whole panel. `colai_sessions` carries the name, the last thing
 * said in it and when — so a row needs no transcript to be drawn, and forty rows are not
 * forty transcripts down a socket. The turns arrive when somebody opens one.
 */
async function loadWork() {
  let sessions;
  try {
    sessions = await invoke("colai_sessions", { receiving: null });
  } catch (error) {
    // Said, not swallowed: an empty panel that is empty because nobody answered looks
    // exactly like an empty panel with nothing in it.
    state.workTrouble = error && error.message ? error.message : String(error);
    render();
    return;
  }
  state.workTrouble = null;
  if (!Array.isArray(sessions)) return;

  const known = new Map(state.history.map((entry) => [entry.sessionKey, entry]));
  const now = Date.now();

  /*
   * Everything, and filtered later.
   *
   * This used to filter here and then assign the result over `state.history`, which made
   * the receiver dropdown destructive: choosing a different agent did not narrow the list,
   * it threw the rest of it away, and choosing a thread that had never been adopted threw
   * away all of it. What came back afterwards was only whatever the Gateway happened to
   * still be listing.
   *
   * The panel is a view. `whoseConversations` is now something the view asks at draw
   * time — see `shownWork` — and nothing about who is receiving reaches this list.
   */
  const listed = sessions.map((session) => {
    const had = known.get(session.key);
    // What colai did in this conversation, most recent first. Only the toolbar knows
    // it: the Gateway has the words, not which region of a screen they were about.
    const ours = ourOwnWork
      .filter((one) => one.sessionKey === session.key)
      .sort((a, b) => (b.at || 0) - (a.at || 0))[0];
    return {
      sessionKey: session.key,
      who: session.title,
      at: session.at || (had && had.at) || now,
      // The conversation's own last line. A send colai made says what colai said
      // instead, because that is the thing somebody is looking for it by.
      said: (ours && ours.said) || session.preview || "",
      count: ours ? ours.count : 0,
      marks: ours ? ours.marks : [],
      blocked: ours ? ours.blocked : undefined,
      mine: Boolean(ours) || Boolean(had && had.mine),
      // Carried whole, and everything the person did to this panel lives in it. It used
      // to be one field at a time, and the refresh dropped whichever one was added last
      // — an open row shutting itself every few seconds.
      view: (had && had.view) || freshView(),
      failed: Boolean(had && had.failed),
      answer: (had && had.answer) || null,
    };
  });

  /*
   * And what the Gateway has not caught up with.
   *
   * A conversation created a moment ago is not in `sessions.list` yet, and a send that was
   * refused never had a session key at all — both used to vanish from the panel within
   * five seconds, taking the Discard button that exists to dismiss the second one. Kept
   * until the list has had a chance to mention them.
   */
  const listedKeys = new Set(listed.map((entry) => entry.sessionKey));
  const young = state.history.filter(
    (entry) =>
      !listedKeys.has(entry.sessionKey) &&
      entry.mine &&
      // A refused send has no session key by construction, so the Gateway will never list
      // it and this window would always run out. It is the one row with a Discard button,
      // put there so somebody can dismiss it — and it was being dismissed for them after
      // a minute, which is the code taking back what its own comment promised.
      (entry.blocked || now - (entry.at || 0) < STILL_NEW),
  );

  const next = [...listed, ...young].sort((a, b) => (b.at || 0) - (a.at || 0));
  /*
   * Drawn only when something changed.
   *
   * This ran unconditionally every few seconds, and a redraw replaces the scrolling
   * element — so reading a reply four rows down meant being thrown back to the top on a
   * timer. The rail's own watcher next door has always diffed before rendering; this is
   * the same rule.
   */
  const same = sameWork(state.history, next);
  state.history = next;
  if (!same) render();
}

/** How long an entry the Gateway has not listed is kept anyway. */
const STILL_NEW = 60_000;

/**
 * The rows this panel is showing, out of everything it knows.
 *
 * Scope first, then the tab. "Mine" is what the panel has always shown and stays the
 * default — the conversation you are pointed at is nearly always the one you mean — but
 * it is now a thing somebody can widen rather than a wall they cannot see past.
 */
function shownWork() {
  const everything = state.history;
  const scoped =
    state.work.scope === "all" ? everything : everything.filter(mineToShow(everything));
  return scoped;
}

/**
 * Which of the remembered rows belong to whoever is receiving.
 *
 * `whoseConversations` answers about a conversation in the session list; the panel holds
 * its own rows, so this is the same question asked of those. A row colai itself created is
 * always ours even before the list agrees it exists, or a send would vanish for the
 * seconds between making it and the list catching up.
 */
function mineToShow() {
  const belongs = whoseConversations();
  return (entry) => belongs({ key: entry.sessionKey }) || Boolean(entry.blocked);
}

/** Everything the person did to this panel, as opposed to everything the Gateway said. */
function freshView() {
  return { open: false, shown: new Set() };
}

/** Whether two builds of the list would draw the same. */
function sameWork(was, now) {
  if (was.length !== now.length) return false;
  return was.every((entry, at) => {
    const then = now[at];
    return (
      entry.sessionKey === then.sessionKey &&
      entry.at === then.at &&
      entry.said === then.said &&
      entry.view === then.view &&
      entry.answer === then.answer
    );
  });
}

/**
 * Which conversations belong to whoever is receiving.
 *
 * The panel is about the conversation somebody has chosen in the dropdown, not about
 * everything the Gateway is holding — a mixed list is a list nobody can read. A thread is
 * matched through the session it was adopted into, which is the only moment it acquires
 * one.
 */
function whoseConversations() {
  const who = state.receiving;
  if (who.id) return (session) => session.key === who.id;
  // Nobody chosen yet. Everything would be a mixed list, so it is nothing until the rail
  // has settled on a receiver — which it does as soon as the session list is read.
  return () => false;
}

/**
 * The turns in one conversation, when somebody opens it.
 *
 * Here rather than with the list because it is a transcript per row, and almost every row
 * is one nobody opens. Fetched once and kept for as long as the toolbar runs; the reply
 * events keep it current after that.
 */
async function loadTurns(entry) {
  if (!entry || !entry.sessionKey) return;
  /*
   * Already has the words, rather than merely has an answer object.
   *
   * This used to be `if (entry.answer) return`, and a send creates an answer with an
   * empty `turns` array — truthy, and holding nothing. So the guard fired forever on
   * exactly the conversations somebody cares most about: the ones they started from this
   * toolbar never fetched their own history, ever.
   */
  if (entry.answer && entry.answer.turns && entry.answer.turns.length > 0) return;
  let turns;
  try {
    turns = await invoke("colai_said", { sessionKey: entry.sessionKey });
  } catch {
    // A conversation the Gateway will not talk about keeps the row it already had.
    return;
  }
  if (!Array.isArray(turns)) return;
  /*
   * Written to the entry that is on screen now, not the one this started with.
   *
   * The refresh rebuilds `state.history` every few seconds, and it can land inside this
   * await — leaving the transcript attached to an object nothing can reach any more. The
   * row then showed nothing, and pressing again is what appeared to fix it.
   */
  const live = state.history.find((one) => one.sessionKey === entry.sessionKey);
  if (!live) return;
  const answer = {
    sessionKey: live.sessionKey,
    who: live.who,
    turns: turns.map((turn) => ({ said: turn.said, mine: Boolean(turn.mine) })),
  };
  live.answer = answer;
  /*
   * And it goes on listening.
   *
   * Replies arrive by finding the session in `state.answers`; an answer built here was
   * never added to it, so a conversation opened from the panel was frozen at the instant
   * it was opened — while its own pill went on saying "Working". Being in that list is the
   * whole of it: `colai_watch` used to be asked alongside, and on this host the replies
   * come back down the pipe the message went up rather than to a subscriber.
   */
  if (!state.answers.some((one) => one.sessionKey === live.sessionKey)) {
    state.answers.push(answer);
  }
  render();
}
