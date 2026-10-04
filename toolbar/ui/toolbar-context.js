// The Context map.
//
// Saved slices of conversations on the left, the sessions they can be dropped into on the right.
// A context is dropped onto a session as a chip; the chips on a session sum into one token pill;
// Open resolves the drop into a manifest and hands it to that session as its first turn, through
// the same send path everything else uses. Nothing commits until Open — the chips a person has
// dropped live in `state.staged` and are the page's alone until then.
//
// Local-first. The store (colai_context.rs) is on this machine; the Team filter is empty here
// because there is no sync, and says so rather than pretending to be loading. It sits on the
// shared overlay shell (toolbar-shell.js), so it opens, centres, dismisses and traps focus the
// way every big panel does.
//
// A classic script sharing one global scope with the rest; reads `el` and `state` from
// toolbar.js and the shell functions from toolbar-shell.js.

/** The most chips one session will carry, so a drop stays a slice and not a second history. */
const STAGE_CAP = 5;

/** The rows the Context-map key's dropdown offers. */
function buildContextMenu(into) {
  const rows = [
    ["Open context map", "contextMap", openContextMap],
    ["Save this session as context…", "screenshot", saveThisSession],
    ["Clear staged", "undo", clearStaged],
  ];
  for (const [label, glyph, act] of rows) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "row";
    button.setAttribute("role", "menuitem");
    button.innerHTML = icon(glyph, 14) + `<span>${label}</span>`;
    button.addEventListener("click", () => {
      state.open = null;
      act();
    });
    into.append(button);
  }
}

/** Load the local set for the current filter, then render whatever is open. */
function loadContexts() {
  void invoke("colai_contexts_list", { scope: state.contextScope })
    .then((found) => {
      state.contexts = Array.isArray(found) ? found : [];
      render();
    })
    .catch((trouble) => {
      state.trouble = `Could not read your contexts — ${trouble && trouble.message ? trouble.message : String(trouble)}`;
      render();
    });
}

/** Bring the map up on the shell, and fetch what goes in it. */
function openContextMap() {
  openSurface("context", el.context);
  loadContexts();
  render();
}

/** Snapshot the conversation being received as a new context, then refresh the map. */
function saveThisSession() {
  const who = state.receiving;
  if (!who || !who.id) {
    state.trouble = "Choose a conversation first — a context is saved from one.";
    render();
    return;
  }
  void invoke("colai_context_save_session", {
    sessionKey: who.id,
    title: who.name || "Untitled context",
    project: null,
  })
    .then(() => {
      say("Saved this session as a context.", "receipt");
      loadContexts();
    })
    .catch((trouble) => {
      state.trouble = `Could not save that — ${trouble && trouble.message ? trouble.message : String(trouble)}`;
      render();
    });
}

/** Drop every staged chip. Staging is the page's own, so this is a state reset; the command
 *  exists for symmetry and is fired without waiting on it. */
function clearStaged() {
  state.staged = {};
  void invoke("colai_context_clear_staged").catch(() => {});
  render();
}

/** Drop a context onto a session, capped, without repeats. */
function stageContext(sessionKey, contextId) {
  if (!sessionKey || !contextId) return;
  const on = state.staged[sessionKey] || [];
  if (on.includes(contextId) || on.length >= STAGE_CAP) return;
  state.staged = { ...state.staged, [sessionKey]: [...on, contextId] };
  render();
}

/** Take one chip back off a session. */
function unstageContext(sessionKey, contextId) {
  const on = (state.staged[sessionKey] || []).filter((id) => id !== contextId);
  state.staged = { ...state.staged, [sessionKey]: on };
  render();
}

/** The tokens a session's staged chips carry, a shared snapshot counted once — the same rule the
 *  backend's resolve uses, mirrored here so the pill can update without a round trip. */
function stagedTokens(sessionKey) {
  const ids = state.staged[sessionKey] || [];
  const seen = new Set();
  let tokens = 0;
  for (const id of ids) {
    const context = state.contexts.find((one) => one.id === id);
    if (!context) continue;
    if (seen.has(context.payloadRef)) continue;
    seen.add(context.payloadRef);
    tokens += context.tokens || 0;
  }
  return tokens;
}

/** Resolve a session's drop into a manifest and hand it to that session as a first turn. */
function openStaged(sessionKey) {
  const ids = state.staged[sessionKey] || [];
  if (ids.length === 0) return;
  void invoke("colai_context_resolve", { ids })
    .then((manifest) => {
      const said = manifestSaid(manifest);
      // Through the ordinary send path: the manifest is the session's first turn. Nothing about
      // the sources is mutated — this is a message about them, not a move of them.
      return invoke("colai_send", {
        receiver: { id: sessionKey },
        message: said,
        markIds: [],
      }).then(() => {
        state.staged = { ...state.staged, [sessionKey]: [] };
        closeSurface();
        say(`Opened ${ids.length} context${ids.length === 1 ? "" : "s"} into that session.`, "receipt");
        render();
      });
    })
    .catch((trouble) => {
      state.trouble = `Could not open those — ${trouble && trouble.message ? trouble.message : String(trouble)}`;
      render();
    });
}

/** The manifest as the text a session receives: what each context is, whose, from where, and how
 *  big — and a plain note when the drop is over the window rather than a silent trim. */
function manifestSaid(manifest) {
  if (!manifest || !Array.isArray(manifest.contexts)) return "Context map: nothing resolved.";
  const lines = manifest.contexts.map(
    (one) => `- ${one.title} (${one.owner}, ${one.machine}) — ~${one.tokens} tokens`,
  );
  const head = `Context map — ${manifest.contexts.length} context${manifest.contexts.length === 1 ? "" : "s"}, ~${manifest.tokens} tokens:`;
  const foot = manifest.overflow
    ? `\n\nThis is larger than the ${manifest.window}-token window — open fewer, or expect the session to drop the oldest.`
    : "";
  return `${head}\n${lines.join("\n")}${foot}`;
}

/**
 * Draw the panel. Header (title, staged total, the Mine/Team/All filter, Save), then the two
 * columns: the saved contexts as draggable cards, and the sessions as drop rows.
 */
function drawContext() {
  el.context.hidden = state.panel !== "context";
  if (state.panel !== "context") return;

  const head = document.createElement("div");
  head.className = "ctx-head";
  const title = document.createElement("span");
  title.className = "ctx-title";
  title.textContent = "Context map";
  const scale = document.createElement("span");
  scale.className = "ctx-scale";
  const staged = Object.values(state.staged).reduce((sum, ids) => sum + ids.length, 0);
  scale.textContent = staged > 0 ? `${staged} staged` : `${state.contexts.length} saved`;

  const tabs = document.createElement("div");
  tabs.className = "ctx-tabs";
  for (const [scope, label] of [
    ["mine", "Mine"],
    ["team", "Team"],
    ["machines", "All machines"],
  ]) {
    const tab = document.createElement("button");
    tab.type = "button";
    tab.className = "ctx-tab";
    tab.dataset.on = String(state.contextScope === scope);
    tab.dataset.scope = scope;
    tab.textContent = label;
    tab.addEventListener("click", () => {
      state.contextScope = scope;
      loadContexts();
    });
    tabs.append(tab);
  }
  const save = document.createElement("button");
  save.type = "button";
  save.className = "ctx-save";
  save.textContent = "Save this session";
  save.addEventListener("click", saveThisSession);
  head.append(title, scale, tabs, save);

  const body = document.createElement("div");
  body.className = "ctx-body";
  body.append(drawContextMap(), drawContextSessions());

  el.context.replaceChildren(head, body);
}

/** The left column: saved contexts as cards. Team, here, is the empty state, said plainly. */
function drawContextMap() {
  const map = document.createElement("div");
  map.className = "ctx-map";

  if (state.contextScope === "team") {
    const empty = document.createElement("p");
    empty.className = "ctx-empty";
    empty.textContent = "Nobody on your team has shared a context yet.";
    map.append(empty);
    return map;
  }

  const search = (state.contextSearch || "").trim().toLowerCase();
  const shown = state.contexts.filter(
    (one) => !search || (one.title || "").toLowerCase().includes(search) || (one.project || "").toLowerCase().includes(search),
  );
  if (shown.length === 0) {
    const empty = document.createElement("p");
    empty.className = "ctx-empty";
    empty.textContent = "No saved contexts yet. Save this session to make one.";
    map.append(empty);
    return map;
  }

  for (const context of shown) {
    const card = document.createElement("button");
    card.type = "button";
    card.className = "ctx-card";
    card.dataset.context = context.id;
    card.draggable = true;
    const name = document.createElement("span");
    name.className = "ctx-card-title";
    name.textContent = context.title;
    const meta = document.createElement("span");
    meta.className = "ctx-card-meta";
    // The machine legend: this machine wears the running colour, so a future other-machine card
    // reads as elsewhere without a word.
    meta.textContent = `${context.machine} · ~${context.tokens} tokens`;
    meta.dataset.here = String(context.isLocal === true);
    card.append(name, meta);
    // Drag to a session, and a click as the keyboard-and-linkedom path onto the current receiver.
    card.addEventListener("dragstart", (event) => {
      if (event.dataTransfer) event.dataTransfer.setData("text/colai-context", context.id);
      state.dragging = context.id;
    });
    card.addEventListener("click", () => {
      const to = state.receiving && state.receiving.id;
      if (to) stageContext(to, context.id);
    });
    map.append(card);
  }
  return map;
}

/** The right column: the sessions a drop can land on, with their chips and summed pill. */
function drawContextSessions() {
  const list = document.createElement("div");
  list.className = "ctx-sessions";

  // The current receiver first — the session this map was opened from is the likeliest target.
  const sessions = [...(state.sessions || [])];
  const receiving = state.receiving && state.receiving.id;
  sessions.sort((a, b) => (a.key === receiving ? -1 : b.key === receiving ? 1 : 0));

  if (sessions.length === 0) {
    const empty = document.createElement("p");
    empty.className = "ctx-empty";
    empty.textContent = "No sessions yet. Start a conversation with Claude Code.";
    list.append(empty);
    return list;
  }

  for (const session of sessions) {
    const row = document.createElement("div");
    row.className = "ctx-session";
    row.dataset.session = session.key;
    const name = document.createElement("div");
    name.className = "ctx-session-name";
    name.textContent = session.title;
    row.append(name);

    const chips = document.createElement("div");
    chips.className = "ctx-chips";
    const ids = state.staged[session.key] || [];
    for (const id of ids) {
      const context = state.contexts.find((one) => one.id === id);
      const chip = document.createElement("span");
      chip.className = "ctx-chip";
      chip.textContent = context ? context.title : id;
      const drop = document.createElement("button");
      drop.type = "button";
      drop.className = "ctx-chip-x";
      drop.textContent = "×";
      drop.setAttribute("aria-label", "Remove");
      drop.addEventListener("click", () => unstageContext(session.key, id));
      chip.append(drop);
      chips.append(chip);
    }
    if (ids.length > 0) {
      const pill = document.createElement("span");
      pill.className = "ctx-pill";
      pill.textContent = `~${stagedTokens(session.key)} tokens`;
      chips.append(pill);
    }
    row.append(chips);

    // The drop target.
    row.addEventListener("dragover", (event) => event.preventDefault());
    row.addEventListener("drop", (event) => {
      event.preventDefault();
      const id = (event.dataTransfer && event.dataTransfer.getData("text/colai-context")) || state.dragging;
      state.dragging = null;
      stageContext(session.key, id);
    });

    if (ids.length > 0) {
      const open = document.createElement("button");
      open.type = "button";
      open.className = "ctx-open";
      open.textContent = "Open ⌘↵";
      open.addEventListener("click", () => openStaged(session.key));
      row.append(open);
    }
    list.append(row);
  }
  return list;
}
