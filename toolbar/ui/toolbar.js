// The toolbar: point at anything on screen and hand it to an agent.
//
// The decisions live in toolbar-tools.js, which knows nothing about a browser. This is
// the page: the rail, the gestures, and what gets drawn.

/* ── browser bindings ─────────────────────────────────────────────────────── */

const tauri = window["__TAURI__"];
const invoke = tauri ? tauri.core.invoke : async () => undefined;
const listen = tauri ? tauri.event.listen : async () => () => {};

// Where the rail was left, remembered between runs.
//
// Renamed off `openclaw.toolbar.where`, which costs anybody who used the OpenClaw build
// one rail that starts in its default corner. Worth it: a key naming a product this is no
// longer part of is the kind of thing that stays wrong for years because it works.
const WHERE = "colai.toolbar.where";

const el = {
  wrap: document.getElementById("rail-wrap"),
  rail: document.getElementById("rail"),
  grip: document.getElementById("grip"),
  flyShape: document.getElementById("fly-shape"),
  flyDesign: document.getElementById("fly-design"),
  flyGit: document.getElementById("fly-git"),
  flyRecord: document.getElementById("fly-record"),
  flyDraw: document.getElementById("fly-draw"),
  flyContext: document.getElementById("fly-context"),
  work: document.getElementById("work"),
  context: document.getElementById("context"),
  response: document.getElementById("response"),
  toasts: document.getElementById("toasts"),
  flights: document.getElementById("flights"),
  flyChat: document.getElementById("fly-chat"),
  flyAsk: document.getElementById("fly-ask"),
  chatRows: document.getElementById("chat-rows"),
  trouble: document.getElementById("trouble"),
  doing: document.getElementById("doing"),
  settings: document.getElementById("settings"),
  schedule: document.getElementById("schedule"),
  marks: document.getElementById("marks"),
  pins: document.getElementById("pins"),
  recording: document.getElementById("recording"),
  recordingArea: document.getElementById("recording-area"),
  recordingLeft: document.getElementById("recording-left"),
  capture: document.getElementById("capture"),
  popup: document.getElementById("popup"),
  zoom: document.getElementById("zoom"),
};

const state = {
  tool: "pointer",
  dock: null,
  at: null,
  open: null,
  surface: null,
  // The big overlay surface currently up (null | "work" | "agents" | "context"), on the
  // shared shell in toolbar-shell.js. Distinct from `open` (small rail flyouts) and `surface`
  // (the front window a mark is about). Only the Context map uses it so far.
  panel: null,
  // The data URL of a screenshot blown up to fill the screen, or null. Opened by clicking the
  // review card's after-shot; dismissed with a click or Escape.
  zoom: null,
  // The settings panel: which tab ("colai" | "claude"), where it has been dragged to (null = centred),
  // and what Claude Code's own settings read back as (null until the Claude Code tab loads them).
  settingsTab: "colai",
  settingsAt: null,
  claudeSettings: null,
  // The scheduled-tasks panel: the tasks read back from disk, whether a read is in flight, the
  // composer's unsaved draft (so a re-render does not wipe it), and where it was dragged to.
  schedules: [],
  scheduleLoading: false,
  scheduleDraft: null,
  scheduleAt: null,
  // Every display, each with what its own desktop keeps of it. The overlay covers the
  // whole desk, so which screen a thing is on is a question that now has to be asked.
  screens: [],
  sessions: [],
  // Null while the list is good, a message while it could not be read. The two are
  // different facts and the rail says which.
  whoTrouble: null,
  /*
   * What this conversation has cost, in dollars.
   *
   * New on this host and not a detail. On OpenClaw the toolbar reached an agent somebody
   * was already paying for however they paid for it; here every mark is a metered call
   * against the user's own account. A tool that spends somebody's money without saying so
   * is one they are right to distrust, so it is on the rail rather than in a panel nobody
   * opens.
   */
  spent: 0,
  // Why the Work panel has no conversations to show, when the reason is not "none".
  workTrouble: null,
  // Who gets what you point at. An agent is somebody who could answer; a session is a
  // conversation already underway. Which of the two it is has to be carried, because
  // the same name can belong to both and a send has to know which it is addressing.
  //
  // The name is kept beside the id on purpose. The menu shows recent conversations, so
  // a session picked an hour ago can fall off it; forgetting who was receiving because
  // they scrolled out of a menu would be the toolbar losing your choice for you.
  receiving: { id: null, name: null },
  marks: [],
  undone: [],
  // The mark whose popup is open, if any. One at a time: two dialogs about two regions
  // is a conversation nobody can follow.
  popup: null,
  // How long a recording covers. Somebody's choice, not a constant.
  recordFor: RECORD_LENGTHS[0],
  // Files and folders somebody brought in, as paths this machine can still find them
  // by. Not their contents: they are on a disk that is better at holding them than
  // this page is, and are read at the moment they are sent.
  files: [],
  // Set while something is being dragged over the toolbar, so it can say it will
  // catch it.
  catching: false,
  // The recording underway, while it is underway: the region it covers and when it
  // ends. Null the rest of the time.
  recording: null,
  // The runs the toolbar believes are underway: one per session it has sent to and not
  // yet heard the end of. Kept as a list rather than a flag, because "one agent is
  // working" and "four are" are different things to be told.
  runs: [],
  // The Work window: what is waiting, what has been sent, and whether marks are drawn.
  // `scope` is how wide the work panel looks: "mine" is the receiver's conversations,
  // which is what it has always shown, and "all" is everything colai knows about.
  work: { open: false, filter: "all", scope: "mine" },
  // What has been sent, newest first. Kept for the session — surviving a restart is a
  // store, and a store is decided on purpose rather than in passing.
  history: [],
  // What is on screen saying an answer arrived. Empty nearly always.
  toasts: [],
  /*
   * Replies nobody has looked at yet, as a count per conversation (session key → n).
   *
   * Only the ones that arrive after a handed turn has settled — the "I can't yet" that turned,
   * several turns later, into the work itself. The card had stopped listening by then, so
   * without this a reply landed in a terminal nobody was watching and the toolbar said
   * nothing. Cleared by opening that conversation's row, not by opening the panel: a list
   * glanced at is not a reply read.
   */
  unread: {},
  /*
   * A question somebody has waved away, as the session it was on and the words it was.
   *
   * Only this. *Which* question is waiting is read off the conversations themselves by
   * `askedOf`, because it is a fact about them and not about the toolbar — what belongs
   * here is the one thing that is not derivable, which is that a person has already
   * looked at this one and does not want the panel over their screen.
   *
   * The words as well as the session, so a second question on the same conversation
   * opens again rather than inheriting the first one's dismissal.
   */
  pushedAside: null,
  // The window somebody is looking at, as the watcher last reported it. Null until it
  // has spoken, which is a reason to leave every mark alone rather than to hide them —
  // a window with an empty id is the other thing, and means nothing is in front.
  front: null,
  // What every agent on this Gateway is doing, which is not the same question as what
  // this toolbar started. Null until the Gateway has answered once: no light is the
  // honest state before anything is known, and a green one would be a claim.
  atWork: null,
  // The last thing heard about work in progress — a line, which conversation it came
  // from and when. What the pill beside the mark shows. Null until an agent says
  // something, which is not the same as nothing running: `doingSaid` decides between
  // those two, because only it can see what the Gateway reports.
  doing: null,
  // Windows that were asked what they are showing and had nothing to say. Asking again
  // is a quarter of a second spent learning what the last answer already said.
  mute: new Set(),
  // Which pen the drawing tool draws with. Chosen from the menu the key opens.
  pen: PEN_FIRST,
  // Which kind of design the next design mark asks for. Chosen on the menu, and
  // changeable on the mark itself afterwards.
  designKind: DESIGN_FIRST,
  // Which git command the next git mark is asking for. Chosen on the menu, and
  // changeable on the mark afterwards, exactly as the design kind is.
  gitKind: GIT_FIRST,
  // Whether the exact tools are folded shut. Open to begin with — the rail is what
  // this toolbar is, and a first look at it should be the whole thing. Remembered with
  // the dock, because it is the same kind of fact: how somebody wants this to sit.
  tucked: false,
  // Whether a picture is being taken right now.
  //
  // The overlay makes itself invisible to photograph what is behind it, which briefly
  // makes somebody else's window the front one. Without knowing that, the rule that
  // closes panels when attention moves away would fire on every single mark.
  capturing: false,
  // Whether the whole rail is put away: the grip and the claw, and nothing else.
  //
  // A second, deeper fold than `tucked`. That one hides four tools somebody rarely
  // reaches for; this one is for when the toolbar should stop being furniture on a
  // screen being worked on. The claw stays because it is the way back and because it
  // already carries the mood — a rail that is away can still say something needs you.
  away: false,
  // Whether the receiver was chosen rather than worked out. A guess may fill an empty
  // seat; it may never take one somebody has sat in.
  picked: false,
  // What the next send is for, and anything else somebody wants to say with it.
  mode: MODE_FIRST,
  text: "",
  // Set while a send is in the air, so a second click cannot post it twice.
  sending: false,
  // What went wrong, when something did. Null the rest of the time, which is the rest
  // of the time.
  /*
   * What the ask field's own menu is showing, if anything.
   *
   * Here rather than inside the field, because the field is rebuilt by every render — a
   * five-second refresh, a reply arriving, a window moving under the toolbar — and a `/`
   * or `@` list that closes itself mid-choice because something unrelated happened is
   * the same bug the caret in that field already had.
   */
  ask: { mark: null, showing: [], picked: 0 },
  // Which of the composer's small menus is open, if either. Here for the same reason
  // `ask` is: the controls that hold them are rebuilt by every render.
  picking: null,
  trouble: null,
  // What kind of thing `trouble` is, and which words it was decided for. See `say`.
  tone: "failure",
  toneFor: null,
  // What has been sent and is still owed an answer, and the answers that have arrived.
  // Kept after the marks themselves are gone, because the point of an answer is that it
  // comes back to the place the question was asked about.
  answers: [],
  /*
   * The Agent Responses surface: the typed response of each turn, and the card that shows it
   * at the mark. `turns` is keyed by turnId (the session, for now) and filled from the
   * `colai:response` event — `{ kind, state, body, change?, question?, followUps? }`.
   * `responding` is the active at-mark card: `{ turnId, at, who, said, thumb, marks }`, set on
   * send and cleared when the card is dismissed. See toolbar-response.js.
   */
  turns: {},
  responding: null,
  /*
   * The question Claude Code is blocked on, or null.
   *
   * Not a queue: it asks about one tool at a time and waits, so a second cannot arrive
   * while one is standing. Holding it in state rather than drawing it straight means the
   * rail can be brought back to it if somebody put the toolbar away mid-turn.
   */
  asking: null,
  /*
   * The prompt the live turn came from, and the confirmation for putting its files back.
   *
   * `prompt` is minted when the message is sent and is the only handle a rewind can be
   * addressed by. `undoing` holds the dry run's answer while somebody decides.
   */
  /** Claude Code's slash commands, and the subset a rail must not offer. */
  commands: [],
  terminalOnly: [],
  prompt: null,
  undoing: null,
  /** What may happen without being asked. Claude Code's own word for it. */
  allowing: "default",
  /*
   * Review changes before they are applied.
   *
   * An opt-in the person sets in the composer. When on, a send forces the session into
   * `default` so every edit stops and shows itself first — the platform is for people who
   * do not read code, so the gate leads with a picture and the diff is folded away behind
   * "See the code". `reviewCodeOpen` remembers whether they opened it; `reviewPending` is
   * set when the mode could not be applied yet (no live session) and is spent on the first
   * session frame. `reviewBefore` holds the thumbnails of what was pointed at, to show as
   * the "before".
   */
  review: false,
  reviewCodeOpen: false,
  reviewPending: false,
  reviewBefore: [],
  /*
   * The Context map: saved conversation slices you can drop into a session (see
   * toolbar-context.js). Local-first — the store is on this machine (colai_context.rs).
   * `contexts` is the loaded set, `contextScope` the segmented filter, `staged` the drops a
   * person has made but not yet Opened (page-only until Open, per the PRD). `contextMap` is the
   * feature flag: off hides the key entirely, reserving no space. It ships off — the feature is
   * dormant behind the flag, the code kept but out of the way.
   */
  contexts: [],
  contextScope: "mine",
  contextSearch: "",
  staged: {},
  contextMap: false,
  // The default for "what may happen without being asked", set in settings and persisted. It is
  // applied to each send (unless Review forces stricter); `allowing` is the live session's mode.
  allowDefault: "acceptEdits",
};

/* ── drawing the whole thing ─────────────────────────────────────────────── */

/**
 * A press on this page, outside whatever is open.
 *
 * Controls are left alone. A key on the rail already means something — picking a tool,
 * toggling the very panel this would be closing — and closing on the way down only to
 * have the click reopen it on the way up is how a button stops working.
 */
function pressedSomewhereElse(event) {
  const at = event.target;
  if (!at || typeof at.closest !== "function") return;
  // Inside the thing that is open is not outside it.
  if (state.work.open && el.work.contains(at)) return;
  if (state.open !== null && at.closest(".flyout")) return;
  // A shell surface, or the scheduled-tasks panel off the rail, is left alone when pressed inside.
  if (state.panel && (at.closest(".overlay-surface") || (el.schedule && el.schedule.contains(at)))) return;
  if (state.panel && !state.capturing) {
    closeSurface();
    render();
    return;
  }
  if (at.closest("button, input, select, textarea, label, .grip")) return;
  shutWhatIsOpen();
}

/**
 * Close whatever is hanging off the rail, because attention went elsewhere.
 *
 * A panel that stays open over a desktop somebody has moved on from is furniture. What
 * is *in* it is kept — the words in the composer live in state and are written back the
 * next time it opens — so this closes a window rather than throwing work away.
 *
 * Never while a picture is being taken. The overlay hides itself to photograph what is
 * behind it, which makes the front window somebody else's for a moment; treating that as
 * "they clicked away" would close the panel every time anybody marked anything.
 */
function shutWhatIsOpen() {
  if (state.capturing) return;
  let shut = false;
  if (state.open !== null) {
    state.open = null;
    shut = true;
  }
  if (state.work.open) {
    state.work.open = false;
    shut = true;
  }
  // One redraw for both, rather than one per thing that happened to be open.
  if (shut) render();
}

/**
 * The field somebody has their cursor in, and where in it.
 *
 * Every panel here redraws whole: `replaceChildren` throws away the box being typed in
 * and builds a new one in its place. The words survive, because they are held in state
 * and written back — but the cursor does not. It falls onto the page, and on this page
 * a single letter typed onto the page is a tool shortcut. That is the whole of the bug
 * where writing a note under a mark switched the toolbar to another tool mid-sentence.
 *
 * Each field says which one it is, so the one that had the cursor can be found again.
 * A field without a `data-field` is a field that will lose the caret, and that is now
 * the only way to lose it.
 */
function whatIsBeingTyped() {
  const had = document.activeElement;
  if (!had || !had.dataset || !had.dataset.field) return null;
  // Not every field has a caret to be asked about — a slider, a number, a date — and on
  // some of them asking is itself an error. Being in the right box matters more than
  // being at the right character, so an unanswerable caret is simply not restored.
  let from = null;
  let to = null;
  try {
    from = had.selectionStart;
    to = had.selectionEnd;
  } catch {}
  return { field: had.dataset.field, from, to };
}

/** Put the cursor back where it was, if what it was in is still there. */
function giveItBack(held) {
  if (!held) return;
  // Compared rather than matched with a selector: a field is named after the mark or the
  // session it belongs to, and those names are not ours to promise are selector-safe.
  let now = null;
  for (const field of document.querySelectorAll("[data-field]")) {
    if (field.dataset.field === held.field) {
      now = field;
      break;
    }
  }
  if (!now) return;
  // Untouched by this redraw, so it still holds the cursor and the selection it had.
  // Focusing it again would be the one thing able to disturb them.
  if (now === document.activeElement) return;
  // Without `preventScroll`, putting the cursor back is itself enough to scroll the
  // panel — which would undo the position a panel restored a moment ago.
  now.focus({ preventScroll: true });
  if (held.from === null) return;
  try {
    now.setSelectionRange(held.from, held.to);
  } catch {}
}

/**
 * A render on the next frame, however many reasons for one arrive before it.
 *
 * Every frame from the Rust side used to call `render` straight away, and a busy turn sends
 * them in bursts — a reply, a tool starting, its outcome, the cost — each one a full redraw of
 * the rail and every open panel for one picture on screen. News that arrives on its own clock
 * comes through here instead; what somebody does with the keyboard or the pointer still
 * renders at once, because a caret and a focus have to be put back in the same task.
 *
 * A frame, with a timer under it: a webview with its window hidden stops handing out frames,
 * and a render that waits for one would wait until somebody looked. Whichever comes first
 * draws; the other finds nothing to do. A synchronous `render` in between draws it too, and
 * clears the request so the frame does not draw the same picture twice.
 */
const RENDER_LATEST = 100;
let renderWanted = false;
function scheduleRender() {
  if (renderWanted) return;
  renderWanted = true;
  const flush = () => {
    if (renderWanted) render();
  };
  requestAnimationFrame(flush);
  setTimeout(flush, RENDER_LATEST);
}

/**
 * Whether `render` has already measured the shape in this task.
 *
 * The rail's mutation observer exists for changes nobody rendered — a flyout a module filled
 * on its own. Render's own writes trip it too, and shaping again straight after a render
 * that just shaped is the same measurement twice. Cleared in a microtask, so the next task's
 * mutations are heard again.
 */
let renderHasShaped = false;

function render() {
  renderWanted = false;
  // Saved before anything is replaced, and put back once everything has been. Several of
  // the panels drawn below are boxes somebody is in the middle of writing in.
  const writing = whatIsBeingTyped();
  const vertical = isVertical(state.dock);
  el.wrap.dataset.vertical = String(vertical);
  el.wrap.dataset.dock = state.dock || "";
  el.wrap.dataset.catching = String(state.catching);
  // The jellyfish pauses while the rail is put away; an unfold resumes it on this render.
  if (typeof jellyWake === "function") jellyWake();

  for (const [id, button] of Object.entries(buttons)) {
    if (id === "shape") {
      button.setAttribute(
        "aria-pressed",
        String(state.tool === "box" || state.tool === "circle" || state.open === "shape"),
      );
    } else if (id === "design") {
      button.setAttribute(
        "aria-pressed",
        String(state.tool === "design" || state.tool === "screenshot" || state.open === "design"),
      );
      button.title =
        state.tool === "design"
          ? `Design · ${(DESIGNS[state.designKind] || DESIGNS[DESIGN_FIRST]).label}`
          : "Design";
    } else if (id === "git") {
      button.setAttribute("aria-pressed", String(state.tool === "git" || state.open === "git"));
      // Which of the six is in your hand, because the key looks the same for all of
      // them and a rebase is not a thing to find out about by doing it.
      button.title =
        state.tool === "git" ? `Git · ${GITS[gitKindOf({ git: state.gitKind })].label}` : "Git";
    } else if (id === "chat") {
      button.setAttribute("aria-pressed", String(state.open === "chat"));
    } else if (id === "send") {
      button.setAttribute("aria-pressed", String(state.open === "send"));
    } else if (id === "exact") {
      // Lit while the tool in your hand is one of the folded ones, because a folded
      // rail has no other way of saying which tool is out.
      button.setAttribute("aria-pressed", String(state.tucked && EXACT.includes(state.tool)));
      button.title = state.tucked ? "Show measure, colour, record…" : "Fold these away";
      button.setAttribute("aria-expanded", String(!state.tucked));
      // One mark that turns says "this opens and closes". Two different marks would say
      // "these are two different buttons".
      button.dataset.turn = String(!state.tucked);
    } else if (id === "draw") {
      button.setAttribute("aria-pressed", String(state.tool === "draw" || state.open === "draw"));
      button.title = `Draw · ${(PENS[state.pen] || PENS[PEN_FIRST]).label} · D`;
    } else if (id === "record") {
      button.setAttribute(
        "aria-pressed",
        String(state.tool === "record" || state.open === "record"),
      );
      button.title = `Record ${state.recordFor} seconds · R`;
    } else if (TOOLS[id]) {
      button.setAttribute("aria-pressed", String(state.tool === id));
    }
    // §2: a menu key's corner dot goes full while its menu is open, dim otherwise. `state.open`
    // names the one open key; the fold key manages its own open state through `data-turn` above,
    // so it is left alone here. Scoped to keys that actually wear a dot, so `aria-expanded` never
    // lands on a plain button that controls nothing.
    if (id !== "exact" && button.querySelector(".menu-dot")) {
      button.setAttribute("aria-expanded", String(state.open === id));
    }
  }
  buttons.undo.disabled = state.marks.length === 0;
  buttons.redo.disabled = state.undone.length === 0;

  // What is actually still running, rather than what was last started. A run that has
  // gone quiet for minutes is one the toolbar has lost track of, and claiming it is
  // still working is a worse lie than never having said so.
  state.runs = runsNow(state.runs, state.atWork, Date.now(), state.history);
  const working = runningSaid(state.runs);
  buttons.chat.dataset.working = String(state.runs.length > 0);
  // Only what is being received. The key stops the conversation somebody is looking at,
  // and the panel is already filtered to it — a run whose conversation is not on screen is
  // not one this key is about.
  const stoppable = runsBeingReceived();
  // Not `hidden`, which takes a key out of the rail between two frames and makes the
  // pill change length in one jump. It folds, like the exact tools do — the same
  // animation, because it is the same thing happening: a key that is not currently
  // wanted. The rail still gets wider while an agent is running, but it grows into it.
  const canStop = stoppable.length > 0;
  buttons.stop.title = stoppable.length === 1 ? "Stop the agent" : `Stop ${stoppable.length} runs`;

  // The mascot carries what the whole Gateway is doing, including the agents somebody
  // is not looking at. `data-mood` rather than a class, so the stylesheet holds the one
  // table of what each state looks like and this holds none of it.
  // Removed rather than emptied. The stylesheet pulses on `[data-mood]` being *there*,
  // so an empty one is still a mood as far as CSS is concerned — and left the glow
  // breathing over a desktop where nothing at all was happening.
  // The jellyfish walks whenever work is underway — from the Gateway-wide snapshot (agents and
  // sessions you are not looking at) OR from a mark you just sent that is still being worked on.
  // `state.atWork` is polled, so it lags a fresh send; and a mark handed to a terminal is
  // *mirrored* off its transcript, never in that snapshot at all — so it alone left the mascot
  // still while your own agent was plainly working. `state.runs` (computed just above, kept fresh
  // by every reply heard) is what the toolbar knows first-hand about its own sends, so it makes
  // the jellyfish jiggle the moment a send starts and until the run goes quiet. Gateway trouble
  // and waiting still win: they are real states this local view cannot see.
  let mood = moodMark(state.atWork);
  if (!mood && state.runs.length > 0) mood = "working";
  if (mood) buttons.settings.dataset.mood = mood;
  else delete buttons.settings.dataset.mood;
  // The one trigger for the motion, wherever the mark is drawn. Working is the only mood
  // it turns in: a mark spinning under a red light would be the toolbar contradicting
  // itself.
  buttons.settings.dataset.walking = String(mood === "working");
  // What it is doing, and then what pressing it does. The mark is the light and it is also the
  // way into settings, so the key says both.
  buttons.settings.title = `${moodSaid(state.atWork)} · ${
    state.panel === "settings" ? "Close" : "Open"
  } settings`;
  buttons.settings.setAttribute("aria-label", buttons.settings.title);

  // And what it is doing, beside the mark that says it is doing something.
  //
  // The text is only written when it changes. This runs on every render — a keypress, a
  // pointer move over a menu — and rewriting the node each time restarts the CSS
  // transition that carries the line in, which reads as a pill flickering at whatever
  // rate the pointer happens to be moving.
  const doing = doingSaid(state.doing, state.atWork, Date.now());
  el.doing.hidden = doing === null;
  if (doing !== null && el.doing.textContent !== doing) el.doing.textContent = doing;

  const who = receiver();
  /*
   * Whether anything is waiting on an answer, which is the loudest thing this key can
   * say. It is on the key rather than only in the popup so that waving the popup away
   * does not wave away the fact: the agent is still stopped, and the badge is the way
   * back to it.
   */
  buttons.chat.dataset.asking = String(Boolean(askedOf(state.answers)));
  const mark = buttons.chat.querySelector(".running-dots");
  // An initial, because upright the name beside this is hidden and an empty mark leaves
  // the control saying nothing at all. It was the agent's emoji when there was one; a
  // conversation has no face, so the first letter of its name is the whole of it.
  mark.textContent = who ? who.name.slice(0, 1).toUpperCase() : "";
  mark.hidden = !who;
  buttons.chat.querySelector(".chat-who").textContent = who
    ? who.name
    : talking()
      ? "Session"
      : "Conversation";
  /*
   * Under the name: what is happening, or what it has cost.
   *
   * It used to count agents, which on this host is always one — and then a conversation
   * count, which the picker below already shows. What is worth the line instead is the
   * running total, because it is the one number nobody can see anywhere else.
   */
  buttons.chat.querySelector(".chat-running").textContent = state.whoTrouble
    ? "unavailable"
    : working || (state.spent > 0 ? spentSaid(state.spent) : "");

  // Which kind each tool that has kinds is currently set to. One rule rather than a
  // special case per tool: several rows share a tool and differ only in what they ask it
  // for, so "is this the current tool" lights all of them — which it did for git, where
  // choosing Stage lit Commit, Push and Rebase alongside it.
  const kindNow = { design: state.designKind, git: state.gitKind };
  for (const button of document.querySelectorAll(".row[data-tool]")) {
    const chosen =
      button.dataset.tool === state.tool &&
      (!button.dataset.kind || button.dataset.kind === kindNow[button.dataset.tool]);
    button.setAttribute("aria-pressed", String(chosen));
  }

  const waiting = chosenMarks().length;
  buttons.send.querySelector(".send-many").textContent = waiting ? String(waiting) : "";
  buttons.send.dataset.waiting = String(waiting > 0);
  // And what came back while nobody was looking — a second badge, on the other corner,
  // because "marks waiting to go" and "replies waiting to be read" are different news.
  const unread = unreadCount();
  const unreadBadge = buttons.send.querySelector(".send-unread");
  unreadBadge.hidden = unread === 0;
  unreadBadge.textContent = unread ? String(unread) : "";
  if (unread) unreadBadge.setAttribute("aria-label", unreadSaid(unread));
  else unreadBadge.removeAttribute("aria-label");

  el.flyShape.hidden = state.open !== "shape";
  el.flyDesign.hidden = state.open !== "design";
  el.flyGit.hidden = state.open !== "git";
  // Folded, the six close up where they stand rather than vanishing — the stylesheet
  // animates it and `data-folded` is what it animates between. Not `hidden`: a key that
  // disappears takes two hundred pixels of rail with it in one frame, and a toolbar
  // that changes length between two blinks reads as a glitch, not as a thing that
  // folded.
  //
  // Two reasons a key can be shut: the exact tools are tucked, or the whole rail is
  // away. The same closing, because it is the same gesture at two depths — and reusing
  // it means an away rail collapses with the animation this already got right rather
  // than a second one that would have to agree with it.
  for (const [id, button] of Object.entries(buttons)) {
    if (id === "settings") continue;
    button.dataset.folded = String(
      foldedAway() ||
        (EXACT.includes(id) && state.tucked) ||
        // Nothing to stop is the ordinary state, and the key is not there for it.
        (id === "stop" && !canStop),
    );
  }
  // Drawn shut for one frame longer than it is meant to be, on the way open: see
  // `foldedAway`. The rail's own state is unchanged — only what is on screen.
  el.wrap.dataset.away = String(foldedAway());
  // Only once the keys have finished closing do they leave the layout. Before that they
  // are still on screen, shrinking, which is the whole of the animation.
  el.wrap.dataset.awayDone = String(state.away && !stillFolding());
  // What the handle says it will do next, and what a screen reader is told the rail is.
  el.grip.setAttribute("aria-expanded", String(!state.away));
  el.grip.title = state.away
    ? "Drag toolbar · double click to bring it back"
    : "Drag toolbar · double click to put it away";
  if (state.open === "points") drawPoints();
  el.flyDraw.hidden = state.open !== "draw";
  for (const button of el.flyDraw.children) {
    button.setAttribute("aria-pressed", String(button.dataset.pen === state.pen));
  }
  el.flyRecord.hidden = state.open !== "record";
  for (const button of el.flyRecord.children) {
    button.setAttribute("aria-pressed", String(Number(button.dataset.seconds) === state.recordFor));
  }
  el.flyChat.hidden = state.open !== "chat";
  if (el.flyContext) el.flyContext.hidden = state.open !== "context";

  // Filled before it is placed. A menu is measured to decide whether it fits on the
  // screen, and measuring it empty answers a question about a different menu — which is
  // how a full list of conversations came to hang off the bottom of the display while
  // the same code, run again a moment later, put it back.

  if (state.open === "chat") drawWho();
  // Filled before it is placed, for the same reason: it is measured to decide whether it
  // fits on the screen, and a question with four options on it is not the height of one
  // with none.
  drawAsk();
  // The Work panel is a menu like the others and obeys the same rule. It did not: it was
  // placed here and filled below, so `within` measured whatever it held last time. A
  // panel opened with six exchanges in it was fitted to the screen as though it still
  // held none, and hung two hundred pixels off the bottom of the display.
  drawWork();
  // The Context map and the settings panel, when they are up on the shell. Their own modules
  // fill and hide them.
  if (typeof drawContext === "function") drawContext();
  if (typeof drawSettings === "function") drawSettings();
  if (typeof drawSchedule === "function") drawSchedule();
  // The agent's response at the mark (Agent Responses). Its own module fills and places it.
  if (typeof drawResponse === "function") drawResponse();
  drawZoom();
  // The Context-map key's feature flag gates its visibility here rather than at build time, so
  // toggling it from settings takes effect at once. Hidden is display:none — no reserved space.
  if (buttons.context) buttons.context.hidden = !state.contextMap;
  // Under the key that opened it, measured rather than guessed. Four hand-tuned
  // offsets used to stand here, and they were four chances to drift: adding the record
  // key pushed everything to its right along and left the design menu opening under a
  // key three along from the one that owns it.
  for (const [node, anchor] of [
    [el.flyShape, buttons.shape],
    [el.flyDesign, buttons.design],
    [el.flyGit, buttons.git],
    [el.flyRecord, buttons.record],
    [el.flyDraw, buttons.draw],
    [el.flyChat, buttons.chat],
    // Out of the key that says who is talking, which is where the question came from.
    [el.flyAsk, buttons.chat],
    // The work panel hangs off send, the key that opens it.
    [el.work, buttons.send],
    // The scheduled-tasks panel hangs off the clock key the same way, so it stays pinned to the
    // rail and grows away from it rather than being centred or clamped to a screen edge.
    [el.schedule, buttons.schedule],
  ]) {
    if (node && anchor) placeFlyout(node, vertical, vertical ? anchor.offsetTop : anchor.offsetLeft);
  }
  // The Context-map dropdown, only when its key is on the rail.
  if (buttons.context && el.flyContext) {
    placeFlyout(
      el.flyContext,
      vertical,
      vertical ? buttons.context.offsetTop : buttons.context.offsetLeft,
    );
  }

  drawMarks();
  drawPopup();
  drawToasts();
  drawTrouble();
  // Left mounted while a popup is open, which is how a click off the popup is heard at
  // all — the popup is stacked above it, so its own controls still get their clicks.
  el.capture.hidden = state.tool === "pointer";
  shape();
  if (!renderHasShaped) {
    renderHasShaped = true;
    queueMicrotask(() => {
      renderHasShaped = false;
    });
  }
  giveItBack(writing);
}

/**
 * What the rail's mutation observer does with a batch of changes.
 *
 * Nothing, when every change is inside the jellyfish: the swim rewrites its paths twenty or
 * thirty times a second and none of it moves the rail's outline. Nothing, when `render` has
 * already shaped in this task — its own writes are what tripped the observer. Otherwise the
 * shape is measured, which is the case this exists for: the rail changing on its own.
 */
function railChanged(records) {
  if (renderHasShaped) return;
  if (
    Array.isArray(records) &&
    records.length > 0 &&
    typeof jellyOwns === "function" &&
    records.every((record) => jellyOwns(record.target))
  ) {
    return;
  }
  shape();
}

/**
 * Redraw the marks on the next frame, however many answers arrived before it.
 *
 * The watcher looks faster than the screen refreshes while a window is being dragged, on
 * purpose — a mark that updates on its own slower clock trails the window it is drawn on.
 * But drawing once per answer would mean drawing several times over for one frame nobody
 * sees, so the answers are kept and the drawing happens once, at the rate the display
 * actually has.
 */
let redrawing = 0;
function redrawMarksSoon() {
  if (redrawing) return;
  redrawing = requestAnimationFrame(() => {
    redrawing = 0;
    drawMarks();
    // Almost always a no-op: the marks are not part of the clickable region, and this is
    // memoised on the rectangles so it only crosses to Rust when they genuinely differ.
    // Left in because a popup open over a moving window is not a no-op.
    shape();
  });
}

/**
 * The line by the rail about the last thing that happened, and its way out.
 *
 * It had none. "Stopped main." was set, drawn, and then sat beside the toolbar for the
 * rest of the session — every message here is about a moment, and none of them said so.
 * A banner that outlives the thing it is about stops being read at all.
 *
 * Timed from the words rather than from the render, because this runs on every frame and
 * restarting the clock each time would mean it never ran out.
 */
let saidLast = "";
let fadingTrouble = null;
/**
 * Say one thing beside the rail, and say what kind of thing it is.
 *
 * Twenty-five places wrote to this strip, and it gave every one of them the same red
 * border and the same five seconds: "Automation created" and "Could not send" were
 * indistinguishable at a glance and equally forgettable. Which is backwards both ways. A
 * receipt is the toolbar reading its own state back to somebody who already knows — it
 * should be quiet and it should go. A failure is something they do not otherwise know,
 * and giving it five seconds beside a rail that can be a metre from where they are
 * looking means it may as well not have been said.
 *
 * So: receipts fade, failures wait to be dismissed or replaced. The tone belongs to the
 * message rather than to the strip, so a receipt cannot inherit the tone of the failure
 * before it — which is why it is remembered against the words it was set with.
 */
function say(said, tone = "failure") {
  state.trouble = said;
  state.tone = tone;
  state.toneFor = said;
}

function drawTrouble() {
  const said = state.trouble || "";
  // Anything set without going through `say` is a failure. That is the safe default:
  // silently downgrading something nobody classified would hide exactly the messages
  // this strip exists for.
  const tone = said && state.toneFor === said ? state.tone || "failure" : "failure";

  el.trouble.hidden = !said;
  el.trouble.dataset.tone = tone;

  if (said !== saidLast) {
    const words = document.createElement("span");
    words.className = "trouble-said";
    words.textContent = said;
    // A message that waits has to have a way out, or it is not a message, it is a
    // permanent fixture on somebody's desktop.
    if (tone === "failure" && said) {
      const shut = document.createElement("button");
      shut.type = "button";
      shut.className = "trouble-shut";
      shut.title = "Dismiss";
      shut.setAttribute("aria-label", "Dismiss this message");
      shut.textContent = "\u00d7";
      shut.addEventListener("click", () => {
        state.trouble = null;
        render();
      });
      el.trouble.replaceChildren(words, shut);
    } else {
      el.trouble.replaceChildren(words);
    }
  }

  if (said === saidLast) return;
  saidLast = said;
  if (fadingTrouble !== null) clearTimeout(fadingTrouble);
  // Only receipts are on a clock. A failure stays until it is dismissed or something
  // else needs the strip.
  fadingTrouble =
    said && tone === "receipt"
      ? setTimeout(() => {
          state.trouble = null;
          render();
        }, TOAST_FOR)
      : null;
}

/**
 * Where the toolbar exists on the screen, so everywhere else belongs to the desktop.
 *
 * Only crosses to Rust when the answer changes: this runs on every render, and a round
 * trip per frame would be absurd.
 */
let shaped = "";
function shape() {
  // The popup opens over the region it is about, well away from the rail, so the two are
  // measured as two rectangles rather than one that swallows the desktop between them.
  // A recording is the one time this window has to get out of the way entirely. Every
  // tool holds a sheet of glass over the whole desk so it can catch a drag, and holding
  // it for fifteen seconds would mean nothing on the desktop could be clicked while the
  // desktop was being filmed — a recording of somebody unable to do the thing they
  // wanted recorded. Nothing here needs clicking, so nothing here is caught: the frame
  // and the countdown are pixels on the glass and the desktop is the desktop.
  const rects = state.recording
    ? []
    : state.tool !== "pointer"
      ? [{ x: 0, y: 0, width: window.innerWidth, height: window.innerHeight }]
      : [boxAround(el.wrap)];
  if (state.popup !== null && !el.popup.hidden) rects.push(boxAround(el.popup));
  if (state.work.open && !el.work.hidden) rects.push(boxAround(el.work));
  if (state.toasts.length && !el.toasts.hidden) rects.push(boxAround(el.toasts));
  // The first-run card, which has a button on it. Everything the overlay does not claim
  // belongs to the desktop, so a card left out of this one is a card nobody can dismiss.
  if (state.panel === "settings" && el.settings && !el.settings.hidden) rects.push(boxAround(el.settings));
  // The Context map, when it is up: a shell surface the pointer must reach like any panel.
  if (state.panel === "context" && el.context && !el.context.hidden) rects.push(boxAround(el.context));
  // The scheduled-tasks panel, the same way — its composer and buttons must take the pointer.
  if (state.panel === "schedule" && el.schedule && !el.schedule.hidden) rects.push(boxAround(el.schedule));
  // The response card at the mark takes the pointer like any panel, so a reply or a follow-up
  // chip is clickable rather than falling through to the app behind it.
  if (state.responding && el.response && !el.response.hidden) rects.push(boxAround(el.response));
  // The enlarged screenshot covers the screen, and a click anywhere on it closes it — so the
  // whole thing has to take the pointer, or the click to dismiss would fall through to the desktop.
  if (state.zoom && el.zoom && !el.zoom.hidden) {
    rects.push({ x: 0, y: 0, width: window.innerWidth, height: window.innerHeight });
  }
  const key = JSON.stringify(rects);
  if (key === shaped) return;
  shaped = key;
  /*
   * A shape that did not take is a sheet of glass over somebody's desk.
   *
   * This was `void invoke(...)` with nothing catching it, which on a platform with no
   * implementation means the overlay goes on swallowing every click while the page looks
   * entirely normal — the one failure here that must never be quiet. Said, and the
   * remembered key is cleared so the next render tries again rather than believing a
   * shape that was never applied.
   */
  invoke("colai_shape", { rects }).catch((error) => {
    shaped = "";
    sayFailed(
      `The toolbar could not claim its own shape — ${error && error.message ? error.message : String(error)}`,
    );
  });
}

/** Blow a screenshot up to fill the screen. A click anywhere, or Escape, closes it. */
function openZoom(src) {
  if (!src) return;
  state.zoom = src;
  render();
}

function closeZoom() {
  if (!state.zoom) return;
  state.zoom = null;
  render();
}

function drawZoom() {
  if (!el.zoom) return;
  if (!state.zoom) {
    if (!el.zoom.hidden) {
      el.zoom.hidden = true;
      el.zoom.replaceChildren();
    }
    return;
  }
  el.zoom.hidden = false;
  const img = document.createElement("img");
  img.src = state.zoom;
  img.alt = "Enlarged screenshot — click to close";
  el.zoom.replaceChildren(img);
  el.zoom.onclick = closeZoom;
}

/**
 * An element's box, widened to hold everything it draws.
 *
 * A flyout opens beside the rail and a menu hangs below it, both positioned outside
 * their parent's box — so the parent's own rectangle does not contain them, and a shape
 * measured from it leaves them visible and dead to the pointer.
 */
function boxAround(node) {
  const own = node.getBoundingClientRect();
  let [left, top, right, bottom] = [own.left, own.top, own.right, own.bottom];
  // Depth-first, so a scrolling list can be taken as a leaf. Its rows below the fold
  // still report boxes past its bottom edge, and counting them would claim a strip of
  // desktop that shows nothing — the same mistake as missing a flyout, upside down.
  const pending = [...node.children];
  while (pending.length) {
    const child = pending.pop();
    // A `hidden` subtree draws nothing, and nothing under it can either — not worth a
    // style lookup per descendant, and this runs on every render.
    if (child.hasAttribute("hidden")) continue;
    const box = child.getBoundingClientRect();
    // Both axes, read separately: the `overflow` shorthand reports nothing useful when
    // the two differ, which is exactly the case here — a list that scrolls vertically.
    const style = getComputedStyle(child);
    // An `<svg>` is measured as one box. Its paths are drawing, not layout — the jellyfish
    // alone is a dozen of them, rewritten every frame — and none reaches a pointer the
    // picture's own box does not.
    const drawing = String(child.tagName).toLowerCase() === "svg";
    if (!drawing && style.overflowX === "visible" && style.overflowY === "visible") {
      pending.push(...child.children);
    }
    if (!box.width || !box.height) continue;
    // Something that does not take the pointer claims none of the desktop. The line
    // beside the mark is the case this exists for: a sentence about work in progress,
    // hung over whatever somebody is working on, for as long as the agent runs — and
    // counting it would lay a dead strip of glass across exactly that. Its children are
    // still walked above, because `pointer-events` is inherited and a child may take it
    // back.
    if (style.pointerEvents === "none") continue;
    left = Math.min(left, box.left);
    top = Math.min(top, box.top);
    right = Math.max(right, box.right);
    bottom = Math.max(bottom, box.bottom);
  }
  return {
    x: Math.round(left - 18),
    y: Math.round(top - 18),
    width: Math.round(right - left + 36),
    height: Math.round(bottom - top + 36),
  };
}

/* ── keys ────────────────────────────────────────────────────────────────── */

/** Start listening for the single letters that pick a tool, and the ways out. */
function listenForKeys() {
  window.addEventListener("keydown", onKey);
  // A press that does reach this page, somewhere that is not the thing that is open.
  //
  // The other half of the same rule. Clicks on somebody's editor never arrive here and
  // are noticed by the front window changing instead; these are the ones that do arrive
  // — the overlay claims a rectangle around everything it drew, so the rail's own
  // background, the space beside a panel and the glass all land on this page and used to
  // land on nothing. That is why closing worked in some places and not others.
  //
  // Capture, so it is heard before whatever is underneath decides what the press meant.
  document.addEventListener("pointerdown", pressedSomewhereElse, true);
  // And the third route: this window losing the keyboard. The front watcher only speaks
  // when the front *changes*, so a press on the window that was already in front behind
  // the overlay says nothing to it — but the overlay still loses focus, and that is the
  // same fact. Cheap, and it costs nothing when the other two have already closed things.
  window.addEventListener("blur", () => shutWhatIsOpen());
}

/**
 * Arrow keys walk a menu, the way every other menu on the desktop does.
 *
 * The flyouts have always been marked up as menus and never behaved as one: Tab moved
 * through them in document order, arrow keys did nothing, and there was no way to get
 * from the last item back to the first. A menu that only answers to Tab is one that
 * keyboard users leave.
 *
 * Home and End too, because a list of eleven tools has a top worth reaching directly.
 */
function walkMenu(event) {
  const going = { ArrowDown: 1, ArrowUp: -1, Home: 0, End: 0 };
  if (!(event.key in going)) return false;
  const from = document.activeElement;
  if (!from || from.getAttribute("role") !== "menuitem") return false;
  const menu = from.closest('[role="menu"]');
  if (!menu) return false;

  const items = [...menu.querySelectorAll('[role="menuitem"]')].filter(
    (item) => !item.disabled && item.offsetParent !== null,
  );
  if (items.length === 0) return false;

  const at = items.indexOf(from);
  // Wrapping, because a menu with ends is a menu somebody gets stuck at.
  const to =
    event.key === "Home"
      ? 0
      : event.key === "End"
        ? items.length - 1
        : (at + going[event.key] + items.length) % items.length;
  event.preventDefault();
  items[to].focus({ preventScroll: true });
  return true;
}

function onKey(event) {
  if (walkMenu(event)) return;

  // An enlarged screenshot takes Escape before anything else — it is the most-recently-opened
  // thing and sits over the card it came from, so closing it should not also dismiss that card.
  if (event.key === "Escape" && state.zoom) {
    event.preventDefault();
    closeZoom();
    return;
  }

  // The response card at the mark takes Escape first, and dismisses — never declines (the turn
  // and its record in the Work log are untouched). Dormant until a card is up.
  if (event.key === "Escape" && state.responding && typeof dismissResponse === "function") {
    event.preventDefault();
    dismissResponse();
    return;
  }

  // A shell surface (Context map) takes Escape first, and closes. Dormant until one is open,
  // since `state.panel` is null otherwise — so this changes nothing for the panels that
  // predate the shell.
  if (event.key === "Escape" && state.panel) {
    event.preventDefault();
    closeSurface();
    render();
    return;
  }

  // A recording first, because it is the one thing here somebody has to wait out and the
  // only state Escape could not reach. Fifteen seconds of countdown started by mistake
  // had no way out but killing the toolbar. What was filmed so far is kept: this is
  // "that is enough", not "that was a mistake".
  if (event.key === "Escape" && state.recording) {
    event.preventDefault();
    void invoke("colai_cut_recording");
    return;
  }
  if (event.key === "Escape" && state.popup !== null) {
    cancelMark(state.popup);
    return;
  }
  // The work panel is a panel like the others and closes like one. It did not: Escape
  // fell straight past it to the line below, which silently changed the tool underneath
  // somebody who was only trying to shut a window — and left the window open.
  if (event.key === "Escape" && state.work.open) {
    state.work.open = false;
    render();
    return;
  }
  // Once everything that was open is shut, Escape stops a running agent. Placed here, after
  // the closes, so it is an escalation rather than a reflex: pressing Escape to dismiss a
  // menu never kills a run — a second press, with nothing left to close, does. Stopping is
  // the one thing on this rail that destroys work, so it is deliberately the last thing
  // Escape reaches before it simply resets the tool.
  if (event.key === "Escape" && runsBeingReceived().length > 0) {
    event.preventDefault();
    void stopReceiving();
    return;
  }
  if (event.key === "Escape") {
    use("pointer");
    void invoke("colai_release");
    return;
  }

  // Send the Work composition from the keyboard, from anywhere. The composer's own field
  // handles this while it has focus (see `askField`); this covers a press made with focus
  // on the rail or nowhere in particular — the same gesture, so it does the same thing.
  // Nothing to send yet just opens Work, so the keystroke is never a dead end.
  if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key === "Enter") {
    const writing = event.target;
    if (writing && (writing.isContentEditable || /^(INPUT|TEXTAREA)$/.test(writing.tagName))) {
      return;
    }
    event.preventDefault();
    const going = chosenMarks();
    if (canSend(going)) {
      void sendMarks(going.map((mark) => mark.id));
    } else if (!state.work.open) {
      state.work.open = true;
      render();
    }
    return;
  }

  // Undo and redo, which the rail has offered since it had an undo key and which nothing
  // implemented: every modifier was refused one line below, so the two shortcuts printed
  // on the keys did nothing at all. Ctrl rather than Meta, because this is Linux and the
  // keys now say Ctrl.
  if (event.ctrlKey && !event.altKey && !event.metaKey && event.key.toLowerCase() === "z") {
    // Not while somebody is writing: Ctrl+Z in a text box is the box's own undo, and
    // taking it would delete a mark instead of a word.
    const writing = event.target;
    if (writing && (writing.isContentEditable || /^(INPUT|TEXTAREA)$/.test(writing.tagName))) {
      return;
    }
    event.preventDefault();
    if (event.shiftKey) redo();
    else undo();
    return;
  }

  if (event.metaKey || event.ctrlKey || event.altKey) return;
  const target = event.target;
  // Never while somebody is writing: a single letter is a shortcut only when it is not
  // a character they meant to type.
  if (target && (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName)))
    return;
  // Nor while a box is open asking to be written in, whatever the page currently thinks
  // has the cursor. `render` puts the caret back into the box after every redraw, so
  // this should not come up — but if it ever does, a letter meant for the note is worth
  // dropping and is not worth changing the tool for. The popup opens over the work and
  // closes back to it with Escape, which is still the way out.
  if (state.popup !== null) return;
  // K opens the Context map, when it is enabled. A single letter, so the same writing guards
  // above keep it from firing mid-sentence.
  if (event.key.toLowerCase() === "k" && state.contextMap && typeof openContextMap === "function") {
    event.preventDefault();
    if (state.panel === "context") closeSurface();
    else openContextMap();
    render();
    return;
  }
  const tool = KEYS[event.key.toLowerCase()];
  if (tool) {
    event.preventDefault();
    use(tool);
  }
}

/* ── start ───────────────────────────────────────────────────────────────── */

/**
 * A failure the person can see.
 *
 * An overlay that throws on startup looks exactly like one that is working and has
 * nothing to say: the rail is drawn, and nothing responds. This is the same strip a
 * failed send uses, which is the whole reason it survived the line that used to sit
 * there narrating the current tool.
 */
function sayFailed(message) {
  // Also the window title, which survives a page that cannot draw and is readable from
  // outside the app while this is being worked on.
  document.title = `toolbar error: ${message}`;
  state.trouble = `The toolbar hit an error — ${message}`;
  try {
    drawTrouble();
  } catch {
    // Nothing left to do: if drawing the message also throws, saying so louder will not
    // help, and throwing from an error handler loses the original.
  }
}

/*
 * Files dragged onto the toolbar.
 *
 * Tauri's own drag-and-drop rather than the page's, because the page's gives a browser
 * File — bytes, no path, and nothing at all for a folder — where this gives the real
 * paths on disk. That is the difference between attaching a copy of something and
 * naming the thing itself, and a folder can only be named.
 *
 * The overlay is input-shaped, so a drop only lands where the toolbar actually is: the
 * rail, or the composer when it is open. Everywhere else the drag goes through to the
 * desktop, which is right — the transparent part of this window is not a window.
 */
function listenForDrops() {
  listen("tauri://drag-enter", (event) => {
    state.catching = onTheToolbar(event);
    render();
  });
  listen("tauri://drag-leave", () => {
    state.catching = false;
    render();
  });
  listen("tauri://drag-drop", onDrop);
}

async function onDrop(event) {
  state.catching = false;
  const paths = (event.payload && event.payload.paths) || [];
  // Let go over the desktop, not over the toolbar. The window is the size of the whole
  // desk, and while a marking tool is out its input shape is the whole desk too, so
  // catching every drop would mean a file dragged to somebody's desktop quietly landing
  // in a message instead. The drawn rectangles are the promise; the window is not.
  if (!paths.length || !onTheToolbar(event)) {
    render();
    return;
  }
  try {
    bringFiles(await invoke("colai_describe_files", { paths }));
  } catch (error) {
    sayFailed(
      `Could not read what was dropped — ${error && error.message ? error.message : String(error)}`,
    );
  }
}

/** Whether a drag is over the part of this window somebody can actually see. */
function onTheToolbar(event) {
  const at = event.payload && event.payload.position;
  if (!at) return false;
  // Physical pixels from the window manager, CSS pixels from the page.
  const scale = window.devicePixelRatio || 1;
  const [x, y] = [at.x / scale, at.y / scale];
  const inside = (box) =>
    x >= box.x && y >= box.y && x <= box.x + box.width && y <= box.y + box.height;
  if (inside(boxAround(el.wrap))) return true;
  return state.popup !== null && !el.popup.hidden && inside(boxAround(el.popup));
}

// WebKit's own context menu has nothing on it that belongs on a screen overlay — and
// worse, it is a native window that outlives the toolbar's shape, so it appears over the
// desktop and has to be dismissed before anything else can be clicked.
window.addEventListener("contextmenu", (event) => event.preventDefault());

window.addEventListener("error", (event) => sayFailed(event.message || String(event.error)));

/*
 * Rejections as well as errors.
 *
 * Startup is an async function, so anything it throws is a rejected promise rather than
 * an `error` event — and listening only for the latter is how a toolbar comes up drawn
 * but dead, with nothing anywhere saying why.
 */
window.addEventListener("unhandledrejection", (event) => {
  const reason = event.reason;
  sayFailed(reason && reason.message ? reason.message : String(reason));
});

/**
 * What Claude Code says `/` should offer.
 *
 * Named rather than inline so a test can hand it an answer: the listener itself cannot be
 * driven from a harness, and this is the whole of what the page does with the list.
 *
 * An empty list is not an answer. A `claude` that could not start, or a frame whose field
 * was renamed, gives nothing — and taking that as the answer would replace a short menu
 * with no menu, so `/` would stop working rather than offer less than it could. The modes
 * stay until something better arrives.
 */
function heardCommands(said) {
  if (Array.isArray(said.slashCommands) && said.slashCommands.length > 0) {
    state.commands = said.slashCommands;
  }
  if (Array.isArray(said.terminalOnly)) state.terminalOnly = said.terminalOnly;
  scheduleRender();
}

async function start() {
  buildRail();
  // Set the jellyfish swimming — buildRail has mounted it on the home key (see toolbar-jelly.js).
  if (typeof startJelly === "function") startJelly();
  try {
    state.screens = (await invoke("colai_screens")) || [];
  } catch {
    state.screens = [];
  }
  /*
   * One screen the size of the window, when the shell cannot say.
   *
   * Everything below works in screens, and none of it should have to ask whether there
   * are any. In physical pixels, like the answer it stands in for: `innerWidth` is CSS
   * pixels, so on a 2× display the plain number is half the desk, and every dock and
   * clamp decision below would be out by that factor.
   *
   * And it is said out loud. GTK failing to name a single monitor is not a normal state,
   * and a toolbar that silently invents one is a toolbar that behaves strangely for a
   * reason nobody can find.
   */
  if (state.screens.length === 0) {
    const ratio = window.devicePixelRatio || 1;
    state.screens = [
      {
        x: 0,
        y: 0,
        width: Math.round(window.innerWidth * ratio),
        height: Math.round(window.innerHeight * ratio),
        reserved: NOTHING_RESERVED,
      },
    ];
    sayFailed("The desktop did not name any monitor, so the toolbar is guessing at one.");
  }
  // Everything this page listens to, started here rather than while its scripts load.
  // A registration that runs at load makes the order of the script tags into a
  // dependency nobody can see; this way each file is a bag of declarations and the
  // page decides when it comes alive.
  listenForMarking();
  listenForKeys();
  listenForDrops();
  listenForDrag();
  recall();
  // What only this toolbar knew about those conversations — which marks travelled, what
  // was pointed at. Read before the list arrives, so it is ready to be laid over it.
  recallWork();
  place();

  await learnFront();

  // Answers, as they arrive. A session says a great deal — the question going in, the
  // work coming out — and only what an agent finally said back is an answer to what was
  // pointed at.
  // A run saying it is over, or has fallen over.
  // Which window is in front. Asked once, then listened for.
  //
  // Both halves are needed. The watcher only speaks when the answer changes, and the
  // first change it sees is at startup — before this page exists to hear it. Waiting for
  // the second one meant a toolbar opened onto a desktop nobody then switched away from
  // never learned which window was in front, so every mark stayed pinned to the screen
  // exactly as it had before any of this was written.
  void invoke("colai_in_front")
    .then((front) => {
      // Only if nothing has been heard since: an event that arrived while this was in
      // flight is newer than this answer.
      if (state.front === null) state.front = front || null;
      scheduleRender();
    })
    .catch(() => {});

  // Which conversation this toolbar belongs to. Asked once, then listened for — the same
  // shape as the front window above, and for the same reason: a `/colai:show` run inside
  // another chat while this page is already up arrives as an event, and the launch that
  // started the page happened before there was a page to tell.
  void invoke("colai_came_from")
    .then((chat) => {
      if (chat) heardWhichChat(chat);
    })
    .catch(() => {});

  void listen("colai:came-from", (event) => {
    const said = event && event.payload;
    if (said && said.chat) heardWhichChat(said.chat);
  });

  void listen("colai:front", (event) => {
    state.front = (event && event.payload) || null;
    // Somebody clicked another window, which on a desktop is what "outside" means.
    //
    // A press on an editor or on bare desktop never reaches this page: the overlay only
    // catches clicks where it drew something, which is the whole reason the desktop is
    // still usable underneath it. But the window that press landed on comes to the
    // front, and that is the same news arriving by another route.
    //
    // Whatever was in front before does not come into it. It was tempting to close only
    // when the overlay itself had been front — but the window manager may refuse this
    // window the keyboard, and then the overlay is never front and nothing ever closes.
    // The watcher only speaks when the answer changes, so a change to somebody else's
    // window is a window switch however it started.
    if (state.front && state.front.ours === false) shutWhatIsOpen();
    redrawMarksSoon();
  }).catch(() => {});

  void listen("colai:reply", (event) => {
    const payload = event && event.payload;
    if (!payload) return;
    // Anything said is a sign of life, which is what keeps the glow from timing out on
    // an agent that is working but slow.
    const run = state.runs.find((one) => one.sessionKey === payload.sessionKey);
    if (run) run.heard = Date.now();
    // What it is doing, for the pill on the rail. Before everything below, and outside
    // it: the rest of this listener is about the pin that is waiting for an answer, and
    // most working sessions have no pin — an agent picked up from the Work panel, a
    // second turn on a conversation whose pin was closed. Those are exactly the runs the
    // pill is worth having for, and reading the line after the early return below would
    // have left it silent for all of them.
    const doingNow = doingOf(payload.message);
    if (doingNow) state.doing = { said: doingNow, sessionKey: payload.sessionKey, at: Date.now() };
    // The pin for that session, whether or not it has heard something already. It used
    // to stop looking once anything had arrived, which threw away everything after the
    // first turn — and an agent says what it is doing before it says what it found.
    const waiting = state.answers.find((answer) => answer.sessionKey === payload.sessionKey);
    const said = waiting ? spokenBy(payload.message) : null;
    const turns = (waiting && waiting.turns) || [];
    // The same turn can arrive twice on a reconnect, and a pin that repeats itself reads
    // as an agent that repeated itself.
    if (waiting && said && !turns.some((turn) => !turn.mine && turn.said === said)) {
      // The first thing back is the thing worth interrupting somebody for. An agent says
      // what it is doing before it says what it found, and one send that talks four times
      // is one thing that happened.
      const spoken = turns.some((turn) => !turn.mine);
      waiting.turns = [...turns, { said, mine: false }];
      // When this conversation last said anything, which is how `askedOf` decides between
      // two that are both waiting. Kept on the answer rather than on the turn: what is
      // being ordered is conversations, and a turn list is not sorted.
      waiting.heard = Date.now();
      workChanged();
      if (!spoken) raiseToast(waiting, said);
    }
    // Nothing here about the question popup: whether this turn is one is read off the
    // turns themselves in `render`, so an agent that asks, is ignored, and then carries on
    // talking puts the panel away by saying something that is not a question.
    //
    // And drawn whether or not a pin was waiting. This used to render only past the pin's
    // early returns, so the pill's new line above sat in `state.doing` unseen until
    // something else happened to redraw — exactly the runs with no pin it was moved up for.
    scheduleRender();
  }).catch(() => {});

  /*
   * Whether the Gateway is there.
   *
   * The toolbar had no indicator at all: down, waiting to be paired and a refused
   * certificate were indistinguishable from "the menus happen to be empty". The Rust
   * side had been announcing every one of those all along and nobody was listening.
   *
   * Only the trouble is shown. "Up" is what the rest of the toolbar working already
   * says, and a banner announcing that everything is fine is a banner people learn to
   * ignore before the one that matters arrives.
   */
  /*
   * What it is doing, as it picks each tool up.
   *
   * A separate event from the reply, because it is separate news arriving on a separate
   * schedule: a reply is what the agent has decided to say and lands at the end of a
   * turn, and this lands the moment it reaches for something. Waiting for the reply
   * would mean the pill said "Thinking" for the whole of a run and then named a tool
   * once the run was over.
   */
  /*
   * What the last turn cost, and what the conversation has cost.
   *
   * Only the running total is drawn. One turn is never the thing worth knowing — a person
   * who has spent four dollars this afternoon wants to be told that, not that this
   * particular mark was six cents.
   */
  /*
   * A reply in a handed chat after its mark had already been answered.
   *
   * The mirror settles at the first end of a turn, and the agent is under no obligation to
   * be finished then — "I'm in plan mode, I can't yet" is a complete turn. Rust follows the
   * transcript on from there and says so once per finished reply; `heardLaterReply`
   * (toolbar-work.js) puts it on the row, on the card if that card is up, and on a toast.
   */
  void listen("colai:later-reply", (event) => {
    heardLaterReply(event && event.payload);
    scheduleRender();
  }).catch(() => {});

  void listen("colai:spent", (event) => {
    const said = event && event.payload;
    if (!said) return;
    state.spent = Number(said.spent) || 0;
    scheduleRender();
  }).catch(() => {});

  void listen("colai:doing", (event) => {
    const said = event && event.payload;
    if (!said || !said.name) return;
    const doing = doingTool({ name: said.name, args: said.args || {} });
    if (!doing) return;
    state.doing = {
      said: doing,
      // The handle its outcome arrives under, so this pill can be finished rather than
      // left running until something else happens to replace it.
      id: said.id || null,
      sessionKey: said.sessionKey || null,
      at: Date.now(),
    };
    // And onto the turn, as a step the response card can show as a plain-language checklist —
    // "Writing snake.html", not the agent's prose about it. This is what makes the card readable
    // for somebody who does not want to read an agent thinking out loud.
    noteStep(said);
    scheduleRender();
  }).catch(() => {});

  /*
   * How that call ended.
   *
   * The event this whole listener list was missing. A tool used to start and then simply
   * stop being mentioned — which on a rail looks identical to a tool still running, so a
   * failed edit and a slow one were the same picture. `never` says it did not run at all:
   * refused by a rule, rejected by somebody, interrupted.
   */
  void listen("colai:did", (event) => {
    const said = event && event.payload;
    if (!said) return;
    const doing = state.doing;
    // Finish the matching step on whichever turn started it, so the card's checklist can show a
    // tick (or a cross) rather than leaving the line running for ever.
    finishStep(said);
    if (doing && said.id && doing.id && doing.id !== said.id) return;
    state.doing = null;
    if (said.wrong || said.never) {
      state.trouble = troubleSaid(said);
    }
    scheduleRender();
  }).catch(() => {});

  /*
   * What Claude Code says about itself, at the start of every turn.
   *
   * It is the session id that matters here, and it fixes something that was quietly broken:
   * a reply is filed by finding the waiting send with the same `sessionKey`, but a
   * conversation the toolbar *starts* has no id — `colai_send` returns an empty one, because
   * nothing knows the name yet. Claude Code names it in this frame, moments later, and every
   * reply then arrives under a name the page has never heard, matches nothing, and is
   * dropped. So the send still waiting without an id is that conversation, and adopts it.
   */
  /*
   * Claude Code is asking whether it may do something, and is waiting.
   *
   * The rail summons itself. A question nobody can see is a conversation that hangs with no
   * sign of why — the toolbar being put away is not consent, and it is not a refusal either.
   */
  void listen("colai:asks", (event) => {
    const said = event && event.payload;
    if (!said || !said.id) return;
    state.asking = said;
    state.doing = null;
    // Each change is met folded, whatever the last one was left as. Somebody who opened the
    // code on one edit has not asked to read every edit after it.
    state.reviewCodeOpen = false;
    void invoke("colai_summon").catch(() => {});
    scheduleRender();
  }).catch(() => {});

  /*
   * What `/` should offer, straight from Claude Code and before anybody presses it.
   *
   * It used to arrive only on `colai:session`, which is the preamble of a conversation the
   * toolbar itself started — so a mark sent straight into a live chat never produced one and
   * the menu offered colai's four modes on a machine with a hundred of Claude Code's. The
   * toolbar asks at startup now; this is the answer.
   */
  void listen("colai:commands", (event) => heardCommands((event && event.payload) || {}));

  void listen("colai:session", (event) => {
    const said = event && event.payload;
    if (!said || !said.sessionKey) return;
    // What the session is actually in, which is the only honest thing to put on the chip.
    if (said.permissionMode) state.allowing = String(said.permissionMode);
    /*
     * A review that could not be set before there was a session to set it on.
     *
     * The first send with Review on starts the child in `acceptEdits` and holds the intent
     * (`reviewPending`); this is that child's opening frame, which arrives before it runs a
     * single tool, so switching to `default` here still lands ahead of the first edit. If
     * the switch itself fails, say so rather than quietly reviewing nothing.
     */
    if (state.reviewPending && said.permissionMode !== "default") {
      state.reviewPending = false;
      void invoke("colai_allow_now", { mode: "default" })
        .then(() => {
          state.allowing = "default";
          scheduleRender();
        })
        .catch(() => {
          state.trouble =
            "Review is on, but changes could not be held for approval — they may apply as the agent makes them.";
          scheduleRender();
        });
    } else if (state.reviewPending) {
      // Already where Review wants it.
      state.reviewPending = false;
    }
    // What `/` should offer, straight from Claude Code, along with the ones it marks as
    // belonging to a terminal — which a rail must not offer, since pressing them does
    // nothing here.
    if (Array.isArray(said.slashCommands)) state.commands = said.slashCommands;
    if (Array.isArray(said.terminalOnly)) state.terminalOnly = said.terminalOnly;
    // The rest of what the session says about itself — kept now for the settings panel's Claude Code
    // tab, which shows the live model, tools, capabilities and agents beside what is read off disk.
    // These used to be received and dropped.
    if (said.model != null) state.model = said.model;
    if (said.cwd != null) state.cwd = String(said.cwd);
    if (said.version != null) state.version = said.version;
    if (Array.isArray(said.tools)) state.tools = said.tools;
    if (Array.isArray(said.capabilities)) state.capabilities = said.capabilities;
    if (Array.isArray(said.agents)) state.agents = said.agents;
    const nameless = (state.answers || []).find((answer) => !answer.sessionKey);
    if (nameless) {
      nameless.sessionKey = said.sessionKey;
      workChanged();
    }
    scheduleRender();
  }).catch(() => {});

  /*
   * Whatever `claude` complained about on the way down.
   *
   * This replaces a listener for `colai:gateway`, which nothing on this host could ever
   * emit — a leftover of the Gateway build, so the trouble banner it drove was unreachable
   * and a toolbar that failed to start said nothing at all. Its stderr is the reason, and
   * it is now carried here.
   */
  /*
   * What a rewind would take back, or has.
   *
   * The dry run and the real thing come back the same way. A dry run fills the confirmation
   * in front of somebody; a real one is finished, so it is said once and the card goes.
   */
  /*
   * The typed response of a turn (Agent Responses). Keyed by turnId, it carries the current
   * kind and state and — for an Answer — the prose body. The card at the mark renders from it;
   * the change/question actions still run through the approval popup for now (a later slice folds
   * them into the card). The body is replaced by the latest answer message rather than appended,
   * until token streaming lands.
   */
  void listen("colai:response", (event) => {
    const said = event && event.payload;
    if (!said || said.turnId == null) return;
    const turnId = String(said.turnId);
    const prev = state.turns[turnId] || {};
    const next = { ...prev, ...said };
    // A change/question/done event carries no body; keep the answer prose already shown.
    if (said.body == null) next.body = prev.body;
    // "done"/"failed" settle the turn without renaming its kind.
    if ((said.state === "done" || said.state === "failed") && prev.kind) next.kind = prev.kind;
    state.turns[turnId] = next;
    // And the edits it made go onto its Work row, which outlives this card and is where a
    // "Rewind to here" reads them from until the conversation's transcript is read.
    keepTurnChanges(turnId, prev, next);
    scheduleRender();
  }).catch(() => {});

  void listen("colai:undone", (event) => {
    const said = event && event.payload;
    if (!said) return;
    const files = Array.isArray(said.files) ? said.files : [];
    // The prompt is kept: it is what the confirmation's "Put back" is addressed by, and
    // dropping it here sent an older row's undo to the latest send instead.
    if (state.undoing) state.undoing = { asked: true, files, prompt: state.undoing.prompt };
    else {
      state.trouble =
        files.length > 0
          ? `Put ${files.length} file${files.length === 1 ? "" : "s"} back.`
          : "Nothing needed putting back.";
    }
    scheduleRender();
  }).catch(() => {});

  void listen("colai:trouble", (event) => {
    const said = event && event.payload;
    if (!said || !said.said) return;
    state.trouble = String(said.said);
    // Settle the response card if a turn errored before it ever finished. A failed spawn (a bad
    // --resume, say) emits no `result` frame, so the card would otherwise shimmer on "working"
    // for ever while the trouble strip alone carried the news. Mark the active turn failed so the
    // card stops and shows it.
    if (state.responding) {
      const turn = state.turns[String(state.responding.turnId)];
      if (turn && (turn.state === "working" || turn.state === "streaming" || !turn.state)) {
        const failed = { ...turn, state: "failed", body: turn.body || said.said };
        state.turns[String(state.responding.turnId)] = failed;
        keepTurnChanges(String(state.responding.turnId), turn, failed);
      }
    }
    scheduleRender();
  }).catch(() => {});

  // A scheduled task came due and ran (or could not). Said out loud — it happened without anyone
  // pressing anything, so silence would be a change nobody could account for — and the panel's list
  // is refreshed so "last ran" catches up. The run itself answers in a fresh conversation, which
  // shows up in Work on its own poll; this is the notice that it started.
  void listen("colai:scheduled", (event) => {
    const said = (event && event.payload) || {};
    if (said.reason === "fired") {
      say(`A scheduled task ran: ${schedulePreview(said.prompt)}.`, "receipt");
    } else if (said.reason === "trouble") {
      state.trouble = `A scheduled task couldn't run — ${said.said || "unknown reason"}.`;
    }
    if (state.panel === "schedule" && typeof loadSchedules === "function") loadSchedules();
    scheduleRender();
  }).catch(() => {});

  // And every conversation OpenClaw is holding. One round trip, and it is what the Work
  // panel is a list of — the toolbar's own record is laid over it rather than being it.
  void loadWork();

  void loadWho();
  // And what every agent is doing, from now until the window closes.
  void watchEverything();
  setInterval(() => void watchEverything(), WATCH_EVERY);
  render();
  clamp();
  // A cold toolbar says nothing about where to begin, so once — and only once — a small
  // pointer beside the rail does. After clamp, so the rail it points at is already placed.
  showFirstHint();
  // The rail redraws itself when a flyout opens, and the shape has to grow to hold it.
  new MutationObserver(railChanged).observe(el.wrap, {
    childList: true,
    subtree: true,
    attributes: true,
  });
}

void start();
