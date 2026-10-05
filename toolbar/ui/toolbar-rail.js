// The rail: the keys, the menus that hang off them, and who a mark can be sent to.
//
// Everything a person points at directly. The rail is the toolbar's whole face, so it
// lives apart from the layers it draws over — what a key does when it is pressed is a
// different question from what a mark looks like once it is made.
//
// The receiver list is here too, because on this surface it is part of the rail: the
// conversation key is a control on it, and the menu it opens is what makes the rest of this
// mean anything. Sending is elsewhere; choosing is here.

const GLYPHS = {
  pointer: '<path d="M5 3.5l14.5 7.2-6.3 1.6-2.3 6.1z"/><path d="M12.4 12.3l5.6 5.7"/>',
  pointAt:
    '<path d="M12 21s-6-5.6-6-10.4a6 6 0 0 1 12 0C18 15.4 12 21 12 21z"/><circle cx="12" cy="10.5" r="2.2" fill="currentColor" stroke="none"/>',
  draw: '<path d="M3 20.5c3-6 6-8 8.5-8 2 0 2 2.5 0 3.5-2.5 1.2-1.5 4 1 3 4-1.5 5-6 8.5-10.5"/><circle cx="21" cy="8.5" r="1.8" fill="currentColor" stroke="none"/>',
  arrow: '<path d="M4.5 19.5L19 5"/><path d="M11.5 5H19v7.5"/>',
  line: '<path d="M4.5 19.5L19.5 4.5"/>',
  highlight:
    '<path d="M4 20.5h16" stroke-width="3.4" opacity="0.45"/><path d="M8.5 15.5l6.6-9.6 3.6 2.6-6.6 9.6z"/><path d="M8.5 15.5l3.6 2.6"/>',
  shape: '<rect x="3" y="3" width="11" height="11" rx="1.5"/><circle cx="15.5" cy="15.5" r="5.5"/>',
  design:
    '<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M3 9h18M9 21V9"/><circle cx="15" cy="15" r="1.4" fill="currentColor" stroke="none"/>',
  undo: '<path d="M3.5 7v6h6"/><path d="M20.5 17a8.5 8.5 0 0 0-14.3-6.2L3.5 13"/>',
  redo: '<path d="M20.5 7v6h-6"/><path d="M3.5 17a8.5 8.5 0 0 1 14.3-6.2L20.5 13"/>',
  box: '<rect x="3" y="3" width="18" height="18" rx="2"/>',
  circle: '<circle cx="12" cy="12" r="9"/>',
  wireframe:
    '<rect x="3" y="3" width="18" height="18" rx="2" stroke-dasharray="3 2.5"/><path d="M7 8h10M7 12h6M7 16h8"/>',
  component:
    '<rect x="2.5" y="2.5" width="8.5" height="8.5" rx="1.6"/><rect x="13" y="13" width="8.5" height="8.5" rx="1.6"/><path d="M11 6.75h4.25a2 2 0 0 1 2 2V13"/>',
  system:
    '<rect x="3" y="3" width="7.5" height="7.5" rx="1.6"/><rect x="13.5" y="3" width="7.5" height="7.5" rx="1.6"/><rect x="3" y="13.5" width="7.5" height="7.5" rx="1.6" fill="currentColor" stroke="none"/><rect x="13.5" y="13.5" width="7.5" height="7.5" rx="1.6"/>',
  // The design questions the toolbar PRD asks of a region: inspect is a magnifier, matching
  // to a token is a target, and a redline is a dimension line with end ticks.
  inspect: '<circle cx="10.5" cy="10.5" r="6.5"/><path d="M15.2 15.2L21 21"/>',
  match: '<circle cx="12" cy="12" r="8"/><circle cx="12" cy="12" r="3.4"/>',
  redline: '<path d="M4 7v10M20 7v10"/><path d="M4 12h16"/><path d="M7 9l-3 3 3 3M17 9l3 3-3 3"/>',
  screenshot:
    '<path d="M4 8V6a2 2 0 0 1 2-2h2M16 4h2a2 2 0 0 1 2 2v2M20 16v2a2 2 0 0 1-2 2h-2M8 20H6a2 2 0 0 1-2-2v-2"/>',
  // Review: an eye over a card. What is about to change, seen before it does.
  review:
    '<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M7 12c1.5-2 3.2-3 5-3s3.5 1 5 3c-1.5 2-3.2 3-5 3s-3.5-1-5-3z"/><circle cx="12" cy="12" r="1.4" fill="currentColor" stroke="none"/>',
  // Context map: a small graph of connected nodes — slices of context, joined.
  contextMap:
    '<circle cx="6" cy="7" r="2"/><circle cx="17.5" cy="6" r="2"/><circle cx="12" cy="17.5" r="2"/><path d="M7.7 8.5l3.1 7.2M15.7 7.4l-2.8 8.3M8 6.7l7.5-.5"/>',
  send: '<path d="M21 3L10.5 13.5"/><path d="M21 3l-6.8 18-3.7-7.5L3 9.8z"/>',
  // Scheduled tasks: a clock. The hands point to the top-right, the way a clock face is drawn
  // when it is standing in for "later" rather than telling a particular time.
  clock: '<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3.4 2"/>',
  close: '<path d="M7 7l10 10M17 7L7 17"/>',
  folder:
    '<path d="M3 7.5a2 2 0 0 1 2-2h3.6l2 2.4H19a2 2 0 0 1 2 2v7.6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
  stop: '<rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" stroke="none"/>',
  measure: '<path d="M4 6v12M20 6v12M4 12h16"/><path d="M8.5 9l-3 3 3 3M15.5 9l3 3-3 3"/>',
  record:
    '<rect x="2.5" y="5" width="14" height="14" rx="2.5"/><path d="M16.5 10.2l5-2.7v9l-5-2.7z"/>',
  colour:
    '<path d="M12 3.5s6 6.4 6 10.1a6 6 0 0 1-12 0C6 9.9 12 3.5 12 3.5z"/><path d="M8.6 14.4a3.4 3.4 0 0 0 3.4 3.2"/>',
  // Git, drawn the way git draws itself: commits are nodes and branches are the lines
  // between them. Six that read as one family at seventeen pixels, which is what a rail
  // this size can carry.
  gitBranch:
    '<circle cx="6.5" cy="5.5" r="2.3"/><circle cx="6.5" cy="18.5" r="2.3"/><circle cx="17.5" cy="8.5" r="2.3"/><path d="M6.5 7.8v8.4"/><path d="M17.5 10.8c0 3.6-3.1 4.7-6.7 5.3"/>',
  gitCommit: '<circle cx="12" cy="12" r="3.6"/><path d="M2.5 12h5.9M15.6 12h5.9"/>',
  gitAdd:
    '<path d="M13.5 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8.5z"/><path d="M13.5 3v5.5H19"/><path d="M12 12.5v5M9.5 15h5"/>',
  gitIgnore:
    '<path d="M13.5 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8.5z"/><path d="M13.5 3v5.5H19"/><path d="M8.8 17.2l6.4-7.4"/>',
  gitPush: '<path d="M4 4h16"/><path d="M12 20.5V8.2"/><path d="M7.2 13L12 8.2l4.8 4.8"/>',
  gitRebase:
    '<circle cx="6.5" cy="5.5" r="2.2"/><circle cx="6.5" cy="18.5" r="2.2"/><circle cx="17.5" cy="12" r="2.2"/><path d="M6.5 7.7v8.6"/><path d="M8.7 5.5h3.6a3.5 3.5 0 0 1 3.5 3.5v.9"/>',
  // The read side of git the toolbar PRD asks for. Same node-and-line family: blame is which
  // commit a line came from (the middle node filled, a line out to what was marked); diff is
  // a plus over a minus; a pull request is a branch merging with an arrow; history is time.
  gitBlame:
    '<circle cx="7" cy="6" r="2.1"/><circle cx="7" cy="12" r="2.1" fill="currentColor"/><circle cx="7" cy="18" r="2.1"/><path d="M7 8.1v1.8M7 14.1v1.8"/><path d="M10.6 12H19"/>',
  gitDiff: '<path d="M6 8h5M8.5 5.5v5"/><path d="M13 16h5"/>',
  gitPr: '<circle cx="6.5" cy="6" r="2.2"/><circle cx="6.5" cy="18" r="2.2"/><circle cx="17.5" cy="18" r="2.2"/><path d="M6.5 8.2v7.6"/><path d="M17.5 15.8V11a3 3 0 0 0-3-3h-3.4"/><path d="M12.7 5.6l-2 2.4 2.4 1z" fill="currentColor" stroke="none"/>',
  gitHistory: '<circle cx="12" cy="12" r="8"/><path d="M12 7.6V12l3 1.8"/>',
  more: '<circle cx="6" cy="6" r="1.7" fill="currentColor" stroke="none"/><circle cx="12" cy="6" r="1.7" fill="currentColor" stroke="none"/><circle cx="18" cy="6" r="1.7" fill="currentColor" stroke="none"/><circle cx="6" cy="12" r="1.7" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1.7" fill="currentColor" stroke="none"/><circle cx="18" cy="12" r="1.7" fill="currentColor" stroke="none"/><circle cx="6" cy="18" r="1.7" fill="currentColor" stroke="none"/><circle cx="12" cy="18" r="1.7" fill="currentColor" stroke="none"/><circle cx="18" cy="18" r="1.7" fill="currentColor" stroke="none"/>',
};

function icon(name, size) {
  return `<svg width="${size || 17}" height="${size || 17}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${GLYPHS[name]}</svg>`;
}

/*
 * colai's own mark — the jellyfish — lives in `toolbar-jelly.js` now: `buildJelly(size)` builds the
 * SVG and a requestAnimationFrame loop swims it the way the ColaiAd announce does (procedural
 * tentacles, a pulsing dome, a blink), grey at rest and colai green while the agent works. The home
 * key mounts it below; `startJelly()` (from toolbar.js) starts the loop.
 */

/* ── the rail ───────────────────────────────────────────────────────────── */

const buttons = {};

/**
 * One mark, one meaning: a corner dot says this key opens a menu.
 *
 * Every key with something behind it wears it and behaves the same way — press it, a
 * list opens, pick from the list. Box and circle work that way, so do the kinds of design,
 * and so do the pens and the recording lengths. The toolbar PRD (§2) draws that mark as a
 * small dot in the key's bottom-right corner rather than a caret — dim at rest, full while
 * the menu is open — and it sits out of the flow, so a key is the same width whether it has
 * a menu or not and does not resize when the menu opens.
 */
const MENU = "menu";

function key(id, title, glyph, onClick, mark) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "key";
  button.title = title;
  button.setAttribute("aria-label", title);
  button.innerHTML = icon(glyph) + (mark === MENU ? '<span class="menu-dot" aria-hidden="true"></span>' : "");
  button.addEventListener("click", onClick);
  buttons[id] = button;
  return button;
}

function buildRail() {
  const dividers = el.rail.querySelectorAll("[data-divider]");

  // The six groups the PRD (§3) lays the rail out in, each after its own divider: select,
  // mark-up, make, utilities, history, work. One divider per group rather than the three the
  // rail carried before, so the separators fall where the meaning changes — pointing tools apart
  // from marking tools apart from the things that make something of a mark.

  // Select: choose what to point at.
  const select = document.createDocumentFragment();
  select.append(
    key("pointer", "Pointer · V", "pointer", () => use("pointer")),
    key("pointAt", "Point at · P", "pointAt", () => use("pointAt")),
  );
  dividers[0].after(select);

  // Mark up: draw on it, or box it.
  const markUp = document.createDocumentFragment();
  markUp.append(
    key("draw", "Draw · D", "draw", () => flyout("draw"), MENU),
    key("shape", "Box / circle · S", "shape", () => flyout("shape"), MENU),
  );
  dividers[1].after(markUp);

  // Make: turn a region into something — a design, a git question, a context.
  const make = document.createDocumentFragment();
  make.append(
    key("design", "Design", "design", () => flyout("design"), MENU),
    key("git", "Git", "gitBranch", () => flyout("git"), MENU),
  );
  // The Context map, last in the make group. Always built; its feature flag (state.contextMap,
  // toggled from settings) gates visibility in render() — a hidden key is display:none, so it
  // still reserves no space when off (PRD §9), and the flag can flip live without a rebuild.
  make.append(key("context", "Context map · K", "contextMap", () => flyout("context"), MENU));
  dividers[2].after(make);

  // Utilities: the exact tools, and the key that folds them out of the way.
  //
  // Folding happens on the rail itself: the exact tools close up where they stand and the rail
  // gets shorter, rather than moving into a menu. A menu would be a second place to go looking
  // for a tool, and the whole reason to fold anything is that a rail is easier to read when it
  // is shorter — not that somewhere else is a better home for them.
  const utilities = document.createDocumentFragment();
  utilities.append(
    key("exact", "Measure, colour, record, screenshot…", "more", toggleTucked, MENU),
    key("measure", "Measure · M", "measure", () => use("measure")),
    key("colour", "Colour · C", "colour", () => use("colour")),
    key("record", "Record · R", "record", () => flyout("record"), MENU),
    // Under the recorder, because that is what somebody is choosing between when they reach for
    // either: a moving picture of this, or a still one.
    key("screenshot", "Screenshot", "screenshot", () => use("screenshot")),
  );
  dividers[3].after(utilities);

  // History: step the marks back and forward.
  const history = document.createDocumentFragment();
  history.append(
    key("undo", "Undo · Ctrl Z", "undo", undo),
    key("redo", "Redo · Ctrl Shift Z", "redo", redo),
  );
  dividers[4].after(history);

  const chat = document.createElement("button");
  chat.type = "button";
  chat.className = "key chat-key";
  chat.title = "Who receives this?";
  chat.innerHTML =
    '<span class="running-dots"></span><span class="chat-name"><span class="chat-who"></span><span class="chat-running"></span></span><span class="ask-badge" aria-hidden="true">?</span><span class="menu-dot" aria-hidden="true"></span>';
  /*
   * Two things on one key.
   *
   * The badge only exists while an agent is waiting on an answer, and pressing it brings
   * the question back rather than opening the receiver list — which is the thing somebody
   * pressing a question mark means. Read off the event rather than given its own button:
   * a button inside a button is not markup a browser will keep, and the badge has to sit
   * inside this one to come out of it.
   */
  chat.addEventListener("click", (event) => {
    if (event.target.closest(".ask-badge")) {
      showAsked();
      return;
    }
    flyout("chat");
  });
  buttons.chat = chat;

  const send = document.createElement("button");
  send.type = "button";
  send.className = "key send-key";
  send.title = "Work — say what you want done";
  send.innerHTML =
    icon("send") +
    '<span class="send-many"></span><span class="send-unread" role="img" hidden></span><span class="menu-dot" aria-hidden="true"></span>';
  // The Work window rather than a flyout of its own: one place where work is assembled,
  // and the same place it is reviewed afterwards.
  send.addEventListener("click", toggleWork);
  // Sending is what this key does; scheduling the same thing is what it is also for. A
  // right click, the same as every other key with more behind it — "more about this
  // key" rather than another key.
  //
  // Nothing behind the right click any more. It opened a menu of two — send now, or
  // schedule it — and scheduling was OpenClaw's, which ran agents in the background.
  // Claude Code is a session somebody is sitting in front of, so the menu would have had
  // one item on it, which is a menu that exists to be dismissed.
  buttons.send = send;

  /*
   * How things are going, wearing colai's own face.
   *
   * It used to be the way back to OpenClaw, and opening that application is what it did.
   * There is no second application here — Claude Code is the terminal already in front of
   * the person — so the key keeps the only job it had that still means something: it is the
   * light. Green while something runs, and the mark turns while it does.
   */
  const home = document.createElement("button");
  home.type = "button";
  home.className = "key home-key";
  home.title = "colai";
  home.setAttribute("aria-label", "colai");
  home.replaceChildren(buildJelly(21));
  /*
   * colai's own key opens the plugin's settings.
   *
   * It is the one key always on the rail, named after the application, so it is where "about
   * this plugin" belongs. It opens a settings panel on the shared shell — the toggles that
   * configure the plugin, and a "How to use" section that keeps the grip / fold / `/` / `@`
   * hints the first-run card used to carry, so nothing became undiscoverable when that card
   * went. A toggle: the same press that opened it closes it.
   */
  home.addEventListener("click", () => {
    if (state.panel === "settings") closeSurface();
    else openSettings();
    render();
  });
  buttons.settings = home;

  // Scheduled tasks, to the left of Work: the same marks and the same sort of ask, but run later
  // and on a cadence rather than now. A key that opens the scheduler surface, the way colai's own
  // key opens settings — press it again to close it.
  const clock = key("schedule", "Scheduled tasks", "clock", () => {
    if (state.panel === "schedule") closeSurface();
    else openSchedule();
    render();
  });

  // Only ever on the rail while something is running, and beside the key that says so.
  // Stopping is the one thing here that destroys work rather than describing it, so it
  // is never a key somebody can press by reflex looking for something else.
  const stop = key("stop", "Stop the agent", "stop", () => void stopReceiving());
  stop.classList.add("stop-key");
  // Folded rather than hidden, and folded from the start: `render` decides from here on,
  // and `hidden` set once here would have outlived it — nothing clears it any more.
  stop.dataset.folded = "true";

  /*
   * Close: the end of the rail, and the end of the toolbar.
   *
   * The toolbar outlives the session that started it — closing a terminal leaves it on screen,
   * which is the point — so being done with it needs a key, not a command somebody has to know.
   * One press. It was two, the first arming it, and a close button that needs pressing twice
   * reads as one that did not work.
   */
  const quit = key("quit", "Close Colai", "close", () => void invoke("colai_quit").catch(() => {}));
  quit.classList.add("quit-key");

  // Work: schedule it for later, say what you want done now, who is on it, stop it, and the light
  // that shows how it goes. The clock sits to the left of Work — later and now, side by side.
  dividers[5].after(clock, send, chat, stop, home, quit);

  row(el.flyShape, "box", "Box", "box", "B");
  row(el.flyShape, "circle", "Circle", "circle", "O");
  // Every kind of design on the menu, not one row called "Design" with the choice
  // hidden in the popup that opens afterwards. What somebody is after — a wireframe, a
  // a component, a whole design system — is the thing they came for, and a menu
  // that does not name it is a menu they conclude cannot do it.
  for (const [id, kind] of Object.entries(DESIGNS)) {
    designRow(el.flyDesign, id, kind);
  }
  // Same rule, same reason: every command named on the menu. Somebody who wants to know
  // whether this toolbar can rebase should be able to find out by opening a menu rather
  // than by marking something and hoping.
  for (const [id, kind] of Object.entries(GITS)) {
    gitRow(el.flyGit, id, kind);
  }
  for (const seconds of RECORD_LENGTHS) lengthRow(el.flyRecord, seconds);
  for (const [id, pen] of Object.entries(PENS)) penRow(el.flyDraw, id, pen);
  // The Context map's own dropdown, built the same way. Always built; the key it hangs off is
  // what the feature flag hides.
  buildContextMenu(el.flyContext);
}

/** One of the lengths a recording can be, on the menu the record key opens. */
// `lengthRow`, not `length`: these scripts share one global scope, and a top-level
// `function length` overwrites `window.length`. Legal, and exactly the collision the
// no-modules arrangement is most likely to produce.
function lengthRow(into, seconds) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "row";
  button.setAttribute("role", "menuitem");
  button.dataset.seconds = String(seconds);
  button.innerHTML = icon("record", 14) + `<span>${seconds} seconds</span>`;
  button.addEventListener("click", () => {
    state.recordFor = seconds;
    // Chosen from the record menu, so the choice is also the tool: nobody opens this to
    // set a number and then goes looking for the key they just right-clicked.
    use("record");
  });
  into.append(button);
}

/** The tools that fold away together, in the order they sit on the rail. */
const EXACT = ["measure", "colour", "record", "screenshot"];

/** Fold them shut, or open them out, and remember which. */
function toggleTucked() {
  state.tucked = !state.tucked;
  state.open = null;
  remember();
  render();
  followTheFold();
}

/**
 * Keep the clickable region on the rail while the rail is still changing size.
 *
 * The shape is measured from the drawn rectangle, and for the fifth of a second the
 * keys are opening or closing the drawn rectangle is a different size every frame.
 * Measured once at the start, the toolbar spends that fifth of a second answering the
 * pointer where it used to be — which is silent, and indistinguishable from a dead
 * button.
 */
let following = 0;
function followTheFold() {
  cancelAnimationFrame(following);
  // A few frames past the end: six keys finish six transitions at slightly different
  // moments, and the last one is not reliably the one that settles the width.
  const until = Date.now() + FOLD_TIME + 60;
  const again = () => {
    clamp();
    if (Date.now() < until) following = requestAnimationFrame(again);
  };
  again();
}

/**
 * Stop what is running.
 *
 * What is being received, not everything on the machine.
 *
 * The rail's key used to stop every run it knew about, which was safe only because it knew
 * about so little — sends from this toolbar and nothing else. Now that it can see what the
 * Gateway is running, "everything" would include agents somebody started in a terminal or
 * from the Control UI, and one key that kills those is a key nobody can press with
 * confidence. Rows in the Work panel keep their own Stop for anything else.
 */
async function stopReceiving() {
  const running = runsBeingReceived();
  if (running.length === 0) return;
  state.runs = state.runs.filter(
    (run) => !running.some((one) => one.sessionKey === run.sessionKey),
  );
  render();
  const stopped = [];
  for (const run of running) {
    try {
      await invoke("colai_stop", { sessionKey: run.sessionKey });
      stopped.push(run.who || run.sessionKey);
    } catch (error) {
      state.trouble = `Could not stop ${run.who || "that run"} — ${error && error.message ? error.message : String(error)}`;
    }
  }
  // Said, not assumed. A stop that produced no answer looks exactly like a stop that did
  // not happen, and somebody who pressed it needs to know which.
  if (stopped.length) say(`Stopped ${stopped.join(", ")}.`, "receipt");
  render();
}

/**
 * The runs belonging to whoever is receiving.
 *
 * The panel is already filtered to them, so its own list is the answer — a run whose
 * conversation is not on screen is not one this key is about.
 */
function runsBeingReceived() {
  const shown = new Set(state.history.map((entry) => entry.sessionKey));
  return state.runs.filter((run) => shown.has(run.sessionKey));
}

/** One pen on the menu the drawing key opens. */
function penRow(into, id, pen) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "row";
  button.setAttribute("role", "menuitem");
  button.dataset.pen = id;
  button.innerHTML = icon(pen.glyph, 14) + `<span>${pen.label}</span>`;
  button.addEventListener("click", () => {
    state.pen = id;
    // Picking a pen is also picking the tool. Nobody opens this to set a pen and then
    // goes looking for the key they just right-clicked.
    use("draw");
  });
  into.append(button);
}

/**
 * One kind of design on the menu.
 *
 * Picking it settles what the next mark will ask for and puts the tool in your hand in
 * the same click. The kind can still be changed in the popup afterwards — that is where
 * somebody who marked first and decided later goes — but nobody should have to mark
 * something to find out whether this toolbar can build them a design system.
 */
function designRow(into, id, kind) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "row";
  button.setAttribute("role", "menuitem");
  button.dataset.tool = "design";
  button.dataset.kind = id;
  button.innerHTML = icon(kind.glyph, 14) + `<span>${kind.label}</span>`;
  button.addEventListener("click", () => {
    state.designKind = id;
    use("design");
  });
  into.append(button);
}

/**
 * One git command on the git key's menu.
 *
 * Picking one settles what the next mark asks for and puts the tool in your hand, the
 * way `designRow` does.
 */
function gitRow(into, id, kind) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "row";
  button.setAttribute("role", "menuitem");
  button.dataset.tool = "git";
  button.dataset.kind = id;
  button.innerHTML = icon(kind.glyph, 14) + `<span>${kind.label}</span>`;
  button.addEventListener("click", () => {
    state.gitKind = id;
    use("git");
  });
  into.append(button);
}

// Every row here sits inside a `role="menu"`, so each one is an item of it. Without
// this the menus announced a count of nothing and arrow keys had no list to walk.
function row(into, tool, label, glyph, press) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "row";
  button.setAttribute("role", "menuitem");
  button.dataset.tool = tool;
  button.innerHTML =
    icon(glyph, 14) +
    `<span>${label}</span>` +
    (press ? `<span class="row-key">${press}</span>` : "");
  button.addEventListener("click", () => use(tool));
  into.append(button);
}

/* ── using a tool, and the one decision that matters ─────────────────────── */

/**
 * A tool that only marks always works — reading is universal. A tool that changes
 * something is refused unless a connector owns the surface, and refused *here*, before
 * anything is dispatched, so there is no path where a write is attempted and then
 * apologised for.
 */
function use(tool) {
  /*
   * "shape" is a key rather than a tool. It carries Box and Circle, so asking for the pair
   * means the neighbour of whichever is in hand — Box from anything else, Circle from Box,
   * and back to Box from Circle. Resolved here rather than at the keyboard, so that `S` and
   * any other way of asking for "the shape tool" cannot come to different answers.
   */
  const asked = tool === "shape" ? SHAPES[(SHAPES.indexOf(state.tool) + 1) % SHAPES.length] : tool;
  // Putting the tool away takes the marks off the screen, so they are shown going where
  // they went. Only on the way out: picking a tool up puts them back, and a flight then
  // would be describing the opposite of what happened.
  const away = asked === "pointer" && state.tool !== "pointer";
  state.tool = asked;
  state.open = null;
  if (away) flyToWork(state.marks);
  render();
}

/**
 * The conversation currently marked as receiving.
 *
 * Returned as one shape so everything downstream — the rail's label, the popup, the mark
 * beside a row — names the receiver the same way. It used to ask which of three kinds it
 * was first; there is one kind on this host, so there is nothing to ask.
 */
function receiver() {
  if (state.receiving.id === null) return null;
  const session = state.sessions.find((row) => row.key === state.receiving.id);
  if (session) return { name: session.title };
  // Off the list rather than gone. The remembered name is what was true when it was
  // picked, which is a better answer than pretending nobody is receiving.
  return state.receiving.name ? { name: state.receiving.name } : null;
}

/** How many conversations there are to hand something to. */
function talking() {
  return state.sessions.length;
}

function flyout(which) {
  state.open = state.open === which ? null : which;
  // The other half of the rule `toggleWork` already keeps. Both hang off the rail now,
  // and two of them opening from the same key would sit on top of each other.
  if (state.open) {
    state.work.open = false;
    // And the scheduled-tasks panel, which hangs off the rail the same way — so opening the
    // receiver picker (or any menu) does not leave it sitting open underneath.
    if (state.panel === "schedule" && typeof closeSurface === "function") closeSurface();
    // Scheduling has fields and every menu has a way out that is a key. This window is
    // a dock, so the keyboard has to be asked for; opening one of the toolbar's own
    // panels is somebody asking for this window. See `reachTheKeyboard`.
    reachTheKeyboard();
  }
  render();
  // Asked when the menu opens rather than polled: the answer only matters when somebody
  // is looking at it, and a conversation's title and status move while it runs, so a
  // list kept warm in the background would be a list that is quietly wrong.
  // Both menus show who could receive, and both are worth a fresh look at what is in
  // front: the answer is different by the time somebody opens one.
  if (state.open === "chat" || state.open === "send") void learnFront().then(loadWho);
}

/**
 * Which conversations this machine has, so a region has somewhere to go.
 *
 * The application's own lists, not a second idea of them — the toolbar should never
 * disagree with the window behind it about what is running. A failure leaves them alone
 * and says so on the rail rather than emptying them, because "nobody there" and "could
 * not ask" are different facts and only one of them is the user's problem.
 */
/**
 * How often the toolbar asks what every agent is doing.
 *
 * A light that lags is worse than no light, and a light that costs a round trip a
 * second is a toolbar somebody turns off. The Gateway caches this list, so what is
 * being paid for is one small frame on an already-open socket.
 *
 * Asked on a timer rather than pushed, deliberately. The pushed events this toolbar
 * already receives arrive only for the sessions it subscribed to — which is exactly the
 * set this light is not about. A subscription wide enough would have to be re-made on
 * every reconnect, and a light that silently stops after a dropped connection is the
 * failure this is meant to prevent.
 */
const WATCH_EVERY = 5000;

/**
 * What `colai_at_work` answers on this host: nothing running, nothing waiting, nobody known.
 *
 * The Rust command returns its `AtWork::default()`, field for field, and this is that value
 * written down — so the light reads exactly what it read when it asked. Frozen, because it is
 * shared by every tick and `state.atWork` holds it.
 */
const AT_WORK_NONE = Object.freeze({
  running: 0,
  waiting: 0,
  trouble: 0,
  working: Object.freeze([]),
  troubled: Object.freeze([]),
  known: Object.freeze([]),
});

/**
 * Ask, and only redraw if the answer changed.
 *
 * Every tick would otherwise rebuild the rail and re-measure the clickable region five
 * times a minute for a picture that is usually identical.
 */
async function watchEverything() {
  /*
   * The receivers, if this machine still does not know who it may talk to.
   *
   * The Gateway connects a moment after the app does, so the first ask usually lands
   * before anything can answer it. This used to be three `setTimeout`s at 1.5, 4 and 9
   * seconds — all of which expired before the Rust side's own fifteen-second budget for
   * starting the Gateway had even finished on a cold machine, and after that nothing
   * asked again for the rest of the session. This ticker already runs forever, which is
   * the property the retry actually needed.
   */
  //
  // Or while a launch named a conversation the list does not have yet. A session that has
  // said nothing has no transcript on disk, so the one that started the toolbar is usually
  // missing from the first list — and nothing else reloads it until somebody opens the menu.
  if (state.whoTrouble || mustFollow) {
    void loadWho();
  }
  // And the conversations, which move without this toolbar: an agent answers, somebody
  // works in the Control UI, a session is started from a terminal. One round trip on the
  // same ticker that already runs forever.
  void loadWork();
  try {
    /*
     * Not asked, for now. `colai_at_work` is still registered, but on this host it answers
     * the same empty picture every time — the only work is this toolbar's own conversation,
     * which it already knows about first-hand (`state.runs`). A round trip every five seconds
     * for a constant is a round trip for nothing, so the constant stands in for the answer
     * until the command has something of its own to say. Everything below still runs on it:
     * the first tick records it, and the rest keep the line beside the mark honest.
     */
    const work = AT_WORK_NONE;
    const before = state.atWork;
    // Which, as well as how many. Two runs where one ends as another begins leaves every
    // count where it was, and the panel would go on showing a spinner over the one that
    // finished.
    const same = (was, now) => (was || []).join("\u0000") === (now || []).join("\u0000");
    if (
      before &&
      before.running === work.running &&
      before.waiting === work.waiting &&
      before.trouble === work.trouble &&
      same(before.working, work.working) &&
      same(before.troubled, work.troubled)
    ) {
      // Nothing about who is working has moved — but the line beside the mark can go
      // stale on its own clock. An agent silent inside one long tool call is the same
      // news five seconds later, and the sentence about it stops being true without
      // anything here changing. Redrawn only when what it would say has actually
      // changed, so this stays a comparison rather than a five-second repaint.
      const shown = el.doing.hidden ? null : el.doing.textContent;
      if (doingSaid(state.doing, work, Date.now()) !== shown) scheduleRender();
      return;
    }
    state.atWork = work;
    // A run the Gateway calls failed. Recorded on the exchange, because the run leaves
    // `state.runs` either way and without this a failure and a clean finish become the
    // same thing the moment it ends. This used to arrive as a `session.error` frame;
    // the Gateway does not broadcast one, so it is read from the same list that says
    // what is working.
    for (const key of work.troubled || []) {
      const entry = state.history.find((one) => one.sessionKey === key);
      if (entry) {
        entry.failed = true;
        workChanged();
      }
    }
    scheduleRender();
  } catch {
    // A Gateway that cannot be asked is not news about agents. The light holds what it
    // last knew rather than flickering off every time the socket blinks — and the
    // conversations menu is where "could not ask" is already said out loud.
  }
}

async function loadWho() {
  /*
   * One ask now, and it used to be five under a single `Promise.all` and a single `catch`
   * — so any one rejection threw away the four answers that had arrived. Some of the five
   * had nothing whatever to do with choosing who receives, and a host with no component
   * catalogue therefore replaced the whole conversation list with "Could not reach the
   * Gateway" while the Gateway was answering perfectly well about conversations. The four
   * that were not about conversations are gone; this is the one that is.
   *
   * A refusal leaves what was already known alone. "Nobody there" and "could not ask" are
   * different facts, and emptying the list is how the second gets told as the first. On the
   * very first load there is nothing to keep, so an ask that fails degrades to the empty
   * list it started as — which is the honest answer before anything has been heard.
   */
  try {
    state.sessions = (await invoke("colai_sessions", { receiving: state.receiving.id })) || [];
    state.whoTrouble = null;
  } catch (trouble) {
    // Said rather than swallowed, and it replaces the list — which is a fair trade for
    // news about the list itself. Retried on the ticker, where an ask that quietly failed
    // gets another go.
    state.whoTrouble = trouble && trouble.message ? trouble.message : String(trouble);
  }
  try {
    // The conversation the toolbar was opened from, when it was opened from one.
    //
    // `/colai:show` runs inside a chat, and Claude Code puts that chat's id in the
    // environment of everything it spawns — so the toolbar already knows the answer to
    // the question the picker was about to ask. Telling it what it could have read was
    // the most repeated act in using it.
    //
    // Only while nobody has picked. Somebody who chose a receiver has said something
    // more recent than the launch did, and a list that reloads every few seconds must
    // not keep overruling them.
    followTheChatWeCameFrom();
    // Nothing else stands in. The Gateway named a default agent and the rail fell back to
    // it; here there is no agent to be default, and an empty receiver is a real answer —
    // `colai_send` reads it as "start a new conversation".
  } catch (error) {
    // Reachable only on an answer shaped like nothing this expects — a list that came back
    // as something other than a list. Said rather than swallowed: a rail that cannot work
    // out who receives has to say so, and this is the line that says it.
    state.whoTrouble = error && error.message ? error.message : String(error);
  }
  render();
}

/** The chat id the toolbar was launched from, as last heard. Null until something says. */
let cameFrom = null;

/**
 * A launch named a conversation and the rail is not on it yet.
 *
 * Needed because the two halves arrive in either order and neither can wait for the other:
 * the id comes back from one command, the list of conversations from another, and a chat
 * young enough to have launched the toolbar may not be in the list at all for a moment. So
 * "we have been told, and have not managed it" is its own state rather than something
 * inferred — inferring it is what broke this the first time, when following was recorded as
 * a choice and the flag that recorded it then blocked the retry.
 */
let mustFollow = false;

/**
 * Point the receiver at the conversation the toolbar was opened from.
 *
 * Does nothing until that conversation is actually in the list. The transcript is written
 * by Claude Code and a chat that has not said anything yet has no file on disk to be read
 * back — so a brand new session can be the one that launched us and still be absent for a
 * moment. Being quiet and trying again on the next reload is the right answer; inventing a
 * row for it would put a receiver in the rail that cannot be sent to.
 */
function pointAtTheChatWeCameFrom() {
  if (!cameFrom) return false;
  if (state.receiving.id === cameFrom) return true;
  const chat = state.sessions.find((row) => row.key === cameFrom);
  if (!chat) return false;
  state.receiving = { id: chat.key, name: chat.title };
  return true;
}

/**
 * Point at the launching conversation if we should and if we can.
 *
 * Two guards, and they are different questions. `mustFollow` is a launch that has spoken
 * and not yet been obeyed, and it overrules everything. `state.picked` is somebody having
 * chosen a receiver by hand, which the list refreshing itself must never quietly undo.
 */
function followTheChatWeCameFrom() {
  if (!mustFollow && state.picked) return;
  if (!pointAtTheChatWeCameFrom()) return;
  mustFollow = false;
  // Recorded as a choice now that it is one, so the next reload leaves it alone.
  state.picked = true;
}

/**
 * A launch said which conversation the toolbar now belongs to.
 *
 * This overrules a choice, which a reload deliberately does not. The difference is who is
 * speaking: a reload is a list refreshing itself, and this is somebody running
 * `/colai:show` inside another chat, which is as clear a statement of where the next mark
 * should go as picking from the menu is.
 */
function heardWhichChat(chat) {
  cameFrom = chat || null;
  if (!cameFrom) return;
  mustFollow = true;
  followTheChatWeCameFrom();
  if (mustFollow) {
    // Not in the list yet. Ask for the list again rather than leaving the rail on the chat
    // before it — a `/colai:show` from a new conversation is exactly the case where the
    // transcript is younger than the last reload.
    void loadWho();
    return;
  }
  render();
}

function receive(id, name) {
  state.picked = true;
  // The name travels with the id. The menu shows recent conversations, so the list this
  // came out of may have been reloaded from under the id by the time somebody sends, and
  // a receiver with no name is a control that has quietly gone blank.
  state.receiving = { id, name };
  state.open = null;
  void loadWho();
}

function undo() {
  const last = state.marks.pop();
  if (!last) return;
  state.undone.push(last);
  render();
}

function redo() {
  const back = state.undone.pop();
  if (!back) return;
  state.marks.push(back);
  render();
}

function placeFlyout(node, vertical, from) {
  node.style.cssText = "";
  const along = vertical ? "top" : "left";
  node.style[along] = `${from}px`;
  // The side the flyout opens toward, taken from the dock: a right-docked rail opens its menus to
  // the left, a bottom-docked one opens them upward, and so on. `opposite` is where it flips to when
  // that side has no room.
  const gap = vertical ? "10px" : "8px";
  const prefer = vertical
    ? state.dock === "right" ? "right" : "left"
    : state.dock === "top" ? "top" : "bottom";
  const opposite = { left: "right", right: "left", top: "bottom", bottom: "top" }[prefer];
  const toward = (side) => {
    node.style.left = node.style.right = node.style.top = node.style.bottom = "";
    node.style[along] = `${from}px`;
    node.style[side] = `calc(100% + ${gap})`;
  };
  toward(prefer);
  if (node.hidden) return;
  // Measured only once it is placed and filled: a menu's length depends on how many
  // conversations are in it, which is not known until it is drawn.
  const fitted = within(node, vertical ? "y" : "x", from);
  if (fitted !== from) node.style[along] = `${fitted}px`;
  // And the direction it opens: the dock's side is only right while there is room on it. Against the
  // top of the screen a menu that opens upward is cut off, so it flips down; against the right edge
  // it flips left. Whichever side shows more of the menu wins when neither can show all of it — a
  // cut-off menu is the one thing this must never leave, wherever the toolbar has been put.
  const awayAxis = vertical ? "x" : "y";
  const spill = () => overflow(node, awayAxis);
  const onPrefer = spill();
  if (onPrefer > 0) {
    toward(opposite);
    if (fitted !== from) node.style[along] = `${fitted}px`;
    if (spill() > onPrefer) {
      // The opposite side is no better — go back to the side that showed the most.
      toward(prefer);
      if (fitted !== from) node.style[along] = `${fitted}px`;
    }
  }
}

/**
 * How far a node spills past the usable screen on one axis, near end plus far end, zero if it is
 * wholly inside. The companion to `within`: that one clamps the axis a menu runs *along*, this one
 * measures the axis it opens *away* on, so `placeFlyout` can flip to the side with more room.
 */
function overflow(node, axis) {
  const box = node.getBoundingClientRect();
  const room = usable(screenAt(state.screens, { x: box.left, y: box.top }));
  const near = axis === "x" ? room.left + EDGE : room.top + EDGE;
  const far = axis === "x" ? room.right - EDGE : room.bottom - EDGE;
  const head = axis === "x" ? box.left : box.top;
  const tail = axis === "x" ? box.right : box.bottom;
  return Math.max(0, near - head) + Math.max(0, tail - far);
}

/**
 * Where a flyout has to sit to stay on the screen.
 *
 * Menus open at a fixed offset from the top of the rail, which is fine in the middle of
 * a screen and wrong at the end of one: a rail docked low with a full list of
 * conversations opened it straight off the bottom, and the rows nearest the bottom were
 * simply unreachable. Nudged back by however much it overhangs, and the near edge wins
 * when a menu is too long to fit either way — the top of a list is where reading starts.
 */
function within(node, axis, at) {
  const box = node.getBoundingClientRect();
  const room = usable(screenAt(state.screens, { x: box.left, y: box.top }));
  const near = axis === "y" ? room.top + EDGE : room.left + EDGE;
  const far = axis === "y" ? room.bottom - EDGE : room.right - EDGE;
  const head = axis === "y" ? box.top : box.left;
  const tail = axis === "y" ? box.bottom : box.right;
  let shift = Math.min(0, far - tail);
  if (head + shift < near) shift = near - head;
  return Math.round(at + shift);
}
