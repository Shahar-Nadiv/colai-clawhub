// The rail: the keys, the menus that hang off them, and who a mark can be sent to.
//
// Everything a person points at directly. The rail is the toolbar's whole face, so it
// lives apart from the layers it draws over — what a key does when it is pressed is a
// different question from what a mark looks like once it is made.
//
// The receiver list is here too, because on this surface it is part of the rail: the
// agents key is a control on it, and the menu it opens is what makes the rest of this
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
  screenshot:
    '<path d="M4 8V6a2 2 0 0 1 2-2h2M16 4h2a2 2 0 0 1 2 2v2M20 16v2a2 2 0 0 1-2 2h-2M8 20H6a2 2 0 0 1-2-2v-2"/>',
  send: '<path d="M21 3L10.5 13.5"/><path d="M21 3l-6.8 18-3.7-7.5L3 9.8z"/>',
  schedule: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5.3l3.4 2"/>',
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
  more: '<circle cx="6" cy="6" r="1.7" fill="currentColor" stroke="none"/><circle cx="12" cy="6" r="1.7" fill="currentColor" stroke="none"/><circle cx="18" cy="6" r="1.7" fill="currentColor" stroke="none"/><circle cx="6" cy="12" r="1.7" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1.7" fill="currentColor" stroke="none"/><circle cx="18" cy="12" r="1.7" fill="currentColor" stroke="none"/><circle cx="6" cy="18" r="1.7" fill="currentColor" stroke="none"/><circle cx="12" cy="18" r="1.7" fill="currentColor" stroke="none"/><circle cx="18" cy="18" r="1.7" fill="currentColor" stroke="none"/>',
};

function icon(name, size) {
  return `<svg width="${size || 17}" height="${size || 17}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${GLYPHS[name]}</svg>`;
}

/**
 * colai's own mark, for the key that says how things are going.
 *
 * Three marks have stood here. OpenClaw's crab, because the toolbar was built inside
 * OpenClaw's desktop app and the key beside it opened that app. Then a C from the old
 * wordmark, when there was no such application to open. Now the jellyfish, which is the
 * logo colai actually has.
 *
 * Always colai's red, rather than the colour of whatever key it sits on. It used to take
 * `currentColor`, which meant the home key's mood tinted it — green while working, blue
 * while waiting — and at rest it came out the rail's own near-white, which is the one thing
 * a logo should never be. The mood is not lost by this: the key still carries it as a
 * coloured ring that pulses, which says what is happening without repainting the brand.
 *
 * Drawn from `media/brand/colai-jellyfish.svg` rather than traced by hand, so the mark on
 * the rail and the mark on the icon are the same drawing. Two masked fills rather than two
 * paths, and that is what lets the eyes be holes: a hole shows whatever is behind the rail,
 * so the face reads on a dark desktop and a light one without the mark knowing which it is
 * on. Filled eyes in a fixed colour would be right on one and invisible on the other.
 *
 * Two parts, because one of them moves — see `data-walking` in the stylesheet. The crab
 * walked, the C spun, and a jellyfish drifts: the tentacles trail while something is
 * running and the dome holds still, so the mark stays a mark rather than becoming a
 * spinner that happens to be green.
 *
 * The mask ids carry the size, because two of these can be on screen at once — the rail's
 * and the pin waiting on a reply at fourteen — and duplicate ids in one document mean the
 * second mark is masked by the first one's shape.
 *
 * Twenty-one pixels in a thirty-four pixel key. The C it replaced sat at seventeen, exactly
 * half the key, and a jellyfish is a busier shape than an arc — the dome, two eyes and five
 * tentacles have to survive being drawn that small, and at seventeen they only just did.
 */
function colaiMark(size) {
  const edge = size || 21;
  const tentacles = `colai-jelly-arms-${edge}`;
  const dome = `colai-jelly-dome-${edge}`;
  return `<svg width="${edge}" height="${edge}" viewBox="0 0 64 64" aria-hidden="true">
    <mask id="${tentacles}" maskUnits="userSpaceOnUse" x="0" y="0" width="64" height="64">
      <g fill="none" stroke="#fff" stroke-width="3.8" stroke-linecap="round">
        <path d="M17 35.5C14.5 42 19 46.5 16.5 52.5C15 56.5 17.5 58.5 17 61" />
        <path d="M24.5 35.5C27 41.5 22.5 46 25 51.5C26.5 55 24 57 24.5 59.5" />
        <path d="M32 35.5C29.5 42 34.5 47.5 32 53.5C30.5 57.5 33 59.5 32 62" />
        <path d="M39.5 35.5C37 41.5 41.5 46 39 51.5C37.5 55 40 57 39.5 59.5" />
        <path d="M47 35.5C49.5 42 45 46.5 47.5 52.5C49 56.5 46.5 58.5 47 61" />
      </g>
    </mask>
    <mask id="${dome}" maskUnits="userSpaceOnUse" x="0" y="0" width="64" height="64">
      <path fill="#fff" d="M8 29C8 14.5 18.5 5 32 5C45.5 5 56 14.5 56 29C56 33 53.5 36 49.5 36L14.5 36C10.5 36 8 33 8 29Z" />
      <path fill="#000" d="M20.2 25a4 4.4 0 1 0 8 0a4 4.4 0 1 0-8 0Z M35.8 25a4 4.4 0 1 0 8 0a4 4.4 0 1 0-8 0Z" />
      <circle fill="#fff" cx="22.6" cy="23.5" r="1.9" />
      <circle fill="#fff" cx="38.2" cy="23.5" r="1.9" />
    </mask>
    <g class="colai-arms"><rect class="colai-ink" width="64" height="64" mask="url(#${tentacles})" /></g>
    <rect class="colai-ink" width="64" height="64" mask="url(#${dome})" />
  </svg>`;
}

/* ── the rail ───────────────────────────────────────────────────────────── */

const buttons = {};

/**
 * One mark, one meaning: a caret says this key opens a menu.
 *
 * Every key with something behind it wears it and behaves the same way — press it, a
 * list opens, pick from the list. Box and circle work that way, so do the kinds of
 * design, and so do the pens and the recording lengths. A second idiom for the same
 * idea is a second thing to learn for no gain, and the one that was tried here — a
 * corner wedge meaning "right-click me" — was a menu people had to be told about.
 */
const MENU = "caret";

function key(id, title, glyph, onClick, mark) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "key";
  button.title = title;
  button.setAttribute("aria-label", title);
  button.innerHTML = icon(glyph) + (mark === MENU ? '<span class="caret">▾</span>' : "");
  button.addEventListener("click", onClick);
  buttons[id] = button;
  return button;
}

function buildRail() {
  const dividers = el.rail.querySelectorAll("[data-divider]");
  const tools = document.createDocumentFragment();
  tools.append(
    key("pointer", "Pointer · V", "pointer", () => use("pointer")),
    key("pointAt", "Point at · P", "pointAt", () => use("pointAt")),
    key("draw", "Draw · D", "draw", () => flyout("draw"), MENU),
    key("shape", "Box / circle · S", "shape", () => flyout("shape"), MENU),
    key("design", "Design", "design", () => flyout("design"), MENU),
    key("git", "Git", "gitBranch", () => flyout("git"), MENU),
  );
  dividers[0].after(tools);

  const edits = document.createDocumentFragment();
  edits.append(
    key("undo", "Undo · Ctrl Z", "undo", undo),
    key("redo", "Redo · Ctrl Shift Z", "redo", redo),
  );
  dividers[1].after(edits);

  // The exact tools, and the key that folds them out of the way.
  //
  // Folding happens on the rail itself: the six close up where they stand and the rail
  // gets shorter, rather than moving into a menu. A menu would be a second place to go
  // looking for a tool, and the whole reason to fold anything is that a rail is easier
  // to read when it is shorter — not that somewhere else is a better home for them.
  const exact = document.createDocumentFragment();
  exact.append(
    key("exact", "Measure, colour, record, screenshot…", "more", toggleTucked, MENU),
    key("measure", "Measure · M", "measure", () => use("measure")),
    key("colour", "Colour · C", "colour", () => use("colour")),
    key("record", "Record · R", "record", () => flyout("record"), MENU),
    // Under the recorder, because that is what somebody is choosing between when they
    // reach for either: a moving picture of this, or a still one. It was a row at the
    // bottom of the Design menu, where nobody looking for a screenshot would think to
    // open a menu named after wireframes.
    key("screenshot", "Screenshot", "screenshot", () => use("screenshot")),
  );
  dividers[1].after(exact);

  const agents = document.createElement("button");
  agents.type = "button";
  agents.className = "key agents-key";
  agents.title = "Agents";
  agents.innerHTML =
    '<span class="running-dots"></span><span class="agents-name"><span class="agents-who"></span><span class="agents-running"></span></span><span class="ask-badge" aria-hidden="true">?</span><span class="caret">▾</span>';
  /*
   * Two things on one key.
   *
   * The badge only exists while an agent is waiting on an answer, and pressing it brings
   * the question back rather than opening the receiver list — which is the thing somebody
   * pressing a question mark means. Read off the event rather than given its own button:
   * a button inside a button is not markup a browser will keep, and the badge has to sit
   * inside this one to come out of it.
   */
  agents.addEventListener("click", (event) => {
    if (event.target.closest(".ask-badge")) {
      showAsked();
      return;
    }
    flyout("agents");
  });
  buttons.agents = agents;

  const send = document.createElement("button");
  send.type = "button";
  send.className = "key send-key";
  send.title = "Work — say what you want done";
  send.innerHTML = icon("send") + '<span class="send-many"></span><span class="caret">▾</span>';
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
  home.innerHTML = colaiMark();
  buttons.settings = home;

  // Only ever on the rail while something is running, and beside the key that says so.
  // Stopping is the one thing here that destroys work rather than describing it, so it
  // is never a key somebody can press by reflex looking for something else.
  const stop = key("stop", "Stop the agent", "stop", () => void stopReceiving());
  stop.classList.add("stop-key");
  // Folded rather than hidden, and folded from the start: `render` decides from here on,
  // and `hidden` set once here would have outlived it — nothing clears it any more.
  stop.dataset.folded = "true";

  dividers[2].after(send, agents, stop, home);

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
  // Putting the tool away takes the marks off the screen, so they are shown going where
  // they went. Only on the way out: picking a tool up puts them back, and a flight then
  // would be describing the opposite of what happened.
  const away = tool === "pointer" && state.tool !== "pointer";
  state.tool = tool;
  state.open = null;
  if (away) flyToWork(state.marks);
  render();
}

/**
 * The agent or session currently marked as receiving, whichever it is.
 *
 * Returned as one shape so everything downstream — the rail's label, the popup, the
 * mark beside a row — can name the receiver without first asking which kind it is.
 */
function receiver() {
  if (state.receiving.id === null) return null;
  if (state.receiving.kind === "thread") {
    const thread = everyThread().find((row) => row.id === state.receiving.id);
    if (thread) return { name: thread.title, emoji: null };
  } else if (state.receiving.kind === "session") {
    const session = state.sessions.find((row) => row.key === state.receiving.id);
    if (session) return { name: session.title, emoji: emojiFor(session) };
  } else {
    const agent = state.agents.find((row) => row.id === state.receiving.id);
    if (agent) return { name: agent.name, emoji: agent.emoji };
  }
  // Off the list rather than gone. The remembered name is what was true when it was
  // picked, which is a better answer than pretending nobody is receiving.
  return state.receiving.name ? { name: state.receiving.name, emoji: state.receiving.emoji } : null;
}

/**
 * How many conversations there are to hand something to.
 *
 * Gateway sessions and threads held elsewhere counted as one number, because that is
 * how somebody counts them: they are all conversations, and which process is holding
 * one is the sort of distinction a rail with room for four words should not spend.
 */
function talking() {
  return state.sessions.length + everyThread().length;
}

/** Every conversation held elsewhere, folders flattened away. */
function everyThread() {
  return state.projects.flatMap((project) => project.threads);
}

/**
 * Which folders are open, counting the ones that have no choice about it.
 *
 * A folder holding the conversation currently receiving stays open, because a menu that
 * hides the row it is describing is a menu you cannot check. And a lone folder is opened
 * too: one collapsed row is a list of nothing.
 */
function openedProjects() {
  const open = new Set(state.opened);
  if (state.projects.length === 1) open.add(state.projects[0].key);
  if (state.receiving.kind === "thread") {
    const holding = state.projects.find((project) =>
      project.threads.some((thread) => thread.id === state.receiving.id),
    );
    if (holding) open.add(holding.key);
  }
  return open;
}

function toggleProject(key) {
  if (state.opened.has(key)) state.opened.delete(key);
  else state.opened.add(key);
  render();
}

/** A session wears its agent's face, so the two lists read as one set of people. */
function emojiFor(session) {
  const agent = state.agents.find((row) => row.id === session.agentId);
  return agent ? agent.emoji : null;
}

function flyout(which) {
  state.open = state.open === which ? null : which;
  // The other half of the rule `toggleWork` already keeps. Both hang off the rail now,
  // and two of them opening from the same key would sit on top of each other.
  if (state.open) {
    state.work.open = false;
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
  if (state.open === "agents" || state.open === "send") void learnFront().then(loadWho);
}

/**
 * Who this machine can hand a region to: its agents, and the conversations underway.
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
  if (state.whoTrouble || state.allowed.length === 0) {
    void loadWho();
  }
  // And the conversations, which move without this toolbar: an agent answers, somebody
  // works in the Control UI, a session is started from a terminal. One round trip on the
  // same ticker that already runs forever.
  void loadWork();
  try {
    const work = await invoke("colai_at_work");
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
      if (doingSaid(state.doing, work, Date.now()) !== shown) render();
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
      if (entry) entry.failed = true;
    }
    render();
  } catch {
    // A Gateway that cannot be asked is not news about agents. The light holds what it
    // last knew rather than flickering off every time the socket blinks — and the
    // conversations menu is where "could not ask" is already said out loud.
  }
}

async function loadWho() {
  const pick = (kind) => (state.receiving.kind === kind ? state.receiving.id : null);
  try {
    // Asked together, because the menu shows them together: two answers a second apart
    // would let the rail claim a receiver that the list below it does not offer.
    const [agents, sessions, projects, allowed, libraries] = await Promise.all([
      invoke("colai_agents", { receiving: pick("agent") }),
      invoke("colai_sessions", { receiving: pick("session") }),
      invoke("colai_threads", { receiving: pick("thread") }),
      // What this connection may do, asked with the rest rather than once at startup.
      // The Gateway connects a moment after the app does — which is why the ask below
      // is retried — so a single question at load is answered before there is anything
      // to answer it, and a menu would spend the session saying it was not allowed.
      invoke("colai_allowed"),
      // Which catalogues this build can read, so the library window can name one rather
      // than telling somebody to go and find out.
      invoke("colai_libraries"),
    ]);
    state.allowed = allowed || [];
    state.libraries = libraries || [];
    state.agents = agents || [];
    state.sessions = sessions || [];
    state.projects = projects || [];
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
    // Still nothing, so the Gateway's own default stands in — the first thing somebody
    // marks still has somewhere to go.
    if (state.receiving.id === null) {
      const fallback = state.agents.find((agent) => agent.receiving);
      if (fallback) {
        state.receiving = {
          kind: "agent",
          id: fallback.id,
          name: fallback.name,
          emoji: fallback.emoji,
          locator: null,
        };
      }
    }
    state.whoTrouble = null;
  } catch (error) {
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
  if (state.receiving.kind === "session" && state.receiving.id === cameFrom) return true;
  const chat = state.sessions.find((row) => row.key === cameFrom);
  if (!chat) return false;
  state.receiving = {
    kind: "session",
    id: chat.key,
    name: chat.title,
    emoji: null,
    locator: null,
  };
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

function receive(kind, id, name, emoji, locator) {
  state.picked = true;
  // The locator travels with the choice. A conversation held elsewhere is addressed by
  // its catalog, host and thread together, and by the time somebody sends, the list it
  // came from may have been reloaded out from under the id.
  state.receiving = { kind, id, name, emoji: emoji || null, locator: locator || null };
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
  if (vertical) {
    node.style[state.dock === "right" ? "right" : "left"] = "calc(100% + 10px)";
  } else {
    node.style[state.dock === "top" ? "top" : "bottom"] = "calc(100% + 8px)";
  }
  if (node.hidden) return;
  // Measured only once it is placed and filled: a menu's length depends on how many
  // conversations are in it, which is not known until it is drawn.
  const fitted = within(node, vertical ? "y" : "x", from);
  if (fitted !== from) node.style[along] = `${fitted}px`;
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
