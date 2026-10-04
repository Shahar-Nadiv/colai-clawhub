// The plugin's settings, on the shared overlay shell.
//
// colai's own key on the rail opens this. It replaced the first-run "four things worth knowing"
// card: the things that card taught — the grip, the fold key, `/` and `@` — live on here as a
// "How to use" section, so none of them became undiscoverable, and the rest of the panel is the
// toggles that actually configure the plugin.
//
// A shell surface (toolbar-shell.js), so it centres, dismisses on Escape and on a press off it,
// and traps focus like the Context map and Work — nothing here re-implements any of that.
//
// A classic script sharing one global scope with the rest; reads `el`/`state` from toolbar.js,
// the shell functions from toolbar-shell.js, and persists through `remember` (toolbar-dock.js).

/** What nobody guesses, in the order they meet it — the old tips card's content, kept. */
const TIPS = [
  ["Move it", "Drag the grip. Double-click it to put the toolbar away."],
  ["Fold it", "The fold key tucks the exact tools out of the way."],
  ["Say how", "Type / in any box for a command, or to say how the ask is read."],
  ["Bring a file", "Type @ in any box to name a file to send with it."],
];

/** What may happen without being asked — the two the toolbar can honestly offer here. */
const ALLOW_CHOICES = [
  ["default", "Ask me first"],
  ["acceptEdits", "Edit freely here"],
];

function openSettings() {
  openSurface("settings", el.settings);
  // Reopen where it was last dragged to; otherwise the shell has centred it.
  if (state.settingsAt) {
    el.settings.style.position = "fixed";
    el.settings.style.left = `${state.settingsAt.left}px`;
    el.settings.style.top = `${state.settingsAt.top}px`;
  }
  if (state.settingsTab === "claude") loadClaudeSettings();
  render();
}

/** Read Claude Code's own settings (plugins, skills, MCP) for the Claude Code tab. Cheap; on demand. */
function loadClaudeSettings() {
  if (state.claudeLoading) return;
  state.claudeLoading = true;
  void invoke("colai_claude_settings", { cwd: state.cwd || null })
    .then((got) => {
      state.claudeSettings = got || {};
      state.claudeLoading = false;
      render();
    })
    .catch(() => {
      state.claudeSettings = null;
      state.claudeLoading = false;
      render();
    });
}

/** A labelled on/off row. */
function toggleRow(label, note, on, onChange) {
  const row = document.createElement("div");
  row.className = "set-row";
  const text = document.createElement("div");
  text.className = "set-text";
  const name = document.createElement("span");
  name.className = "set-label";
  name.textContent = label;
  const said = document.createElement("span");
  said.className = "set-note";
  said.textContent = note;
  text.append(name, said);
  const toggle = document.createElement("button");
  toggle.type = "button";
  toggle.className = "set-toggle";
  toggle.setAttribute("aria-pressed", String(Boolean(on)));
  toggle.setAttribute("aria-label", label);
  toggle.addEventListener("click", () => onChange(!on));
  row.append(text, toggle);
  return row;
}

/** A labelled segmented choice. */
function choiceRow(label, note, choices, current, onPick) {
  const row = document.createElement("div");
  row.className = "set-row";
  const text = document.createElement("div");
  text.className = "set-text";
  const name = document.createElement("span");
  name.className = "set-label";
  name.textContent = label;
  const said = document.createElement("span");
  said.className = "set-note";
  said.textContent = note;
  text.append(name, said);
  const seg = document.createElement("div");
  seg.className = "set-seg";
  for (const [value, shown] of choices) {
    const pick = document.createElement("button");
    pick.type = "button";
    pick.className = "set-seg-tab";
    pick.dataset.on = String(current === value);
    pick.dataset.value = value;
    pick.textContent = shown;
    pick.addEventListener("click", () => onPick(value));
    seg.append(pick);
  }
  row.append(text, seg);
  return row;
}

function drawSettings() {
  el.settings.hidden = state.panel !== "settings";
  if (state.panel !== "settings") return;
  const head = settingsHead();
  const body = state.settingsTab === "claude" ? claudeTab() : colaiTab();
  el.settings.replaceChildren(head, body);
  // The header is the drag handle (its buttons — close, tabs — still click). Re-attached each render on
  // the fresh header; a drop remembers where it was left.
  makeDraggable(el.settings, head, (at) => {
    state.settingsAt = at;
    remember();
  });
}

/** The panel header: title + close, and the tab strip. The whole thing is the drag handle. */
function settingsHead() {
  const head = document.createElement("div");
  head.className = "set-head";
  const row = document.createElement("div");
  row.className = "set-headrow";
  const title = document.createElement("span");
  title.className = "set-title";
  title.textContent = "Settings";
  const shut = document.createElement("button");
  shut.type = "button";
  shut.className = "set-shut";
  shut.title = "Close";
  shut.setAttribute("aria-label", "Close settings");
  shut.textContent = "×";
  shut.addEventListener("click", () => {
    closeSurface();
    render();
  });
  row.append(title, shut);
  const tabs = document.createElement("div");
  tabs.className = "set-tabs";
  for (const [key, label] of [["colai", "colai"], ["claude", "Claude Code"]]) {
    const tab = document.createElement("button");
    tab.type = "button";
    tab.className = "set-tab";
    tab.dataset.on = String(state.settingsTab === key);
    tab.textContent = label;
    tab.addEventListener("click", () => {
      state.settingsTab = key;
      if (key === "claude") loadClaudeSettings();
      render();
    });
    tabs.append(tab);
  }
  head.append(row, tabs);
  return head;
}

/** colai's own settings — the toggles and the how-to. */
function colaiTab() {
  const wrap = document.createElement("div");
  wrap.className = "set-body";
  const sub = document.createElement("p");
  sub.className = "set-sub";
  sub.textContent = "Point at anything on screen and hand it to Claude Code.";

  const toggles = document.createElement("div");
  toggles.className = "set-group";
  toggles.append(
    toggleRow(
      "Review changes",
      "See each change and approve it before it is applied.",
      state.review,
      (next) => {
        state.review = next;
        remember();
        render();
      },
    ),
    choiceRow(
      "What happens without asking",
      "How edits are handled when you are not reviewing.",
      ALLOW_CHOICES,
      state.allowDefault || "acceptEdits",
      (value) => {
        state.allowDefault = value;
        remember();
        // Apply to the live session too, so the choice takes effect now and not only next send.
        void invoke("colai_allow_now", { mode: value }).catch(() => {});
        state.allowing = value;
        render();
      },
    ),
  );

  const howHead = document.createElement("p");
  howHead.className = "set-section";
  howHead.textContent = "How to use";
  const how = document.createElement("div");
  how.className = "set-group";
  for (const [what, note] of TIPS) {
    const r = document.createElement("div");
    r.className = "set-row set-how";
    const text = document.createElement("div");
    text.className = "set-text";
    const name = document.createElement("span");
    name.className = "set-label";
    name.textContent = what;
    const said = document.createElement("span");
    said.className = "set-note";
    said.textContent = note;
    text.append(name, said);
    r.append(text);
    how.append(r);
  }
  wrap.append(sub, toggles, howHead, how);
  return wrap;
}

/** What Claude Code is running with — read-only, plus the one safe toggle (plugins). */
function claudeTab() {
  const wrap = document.createElement("div");
  wrap.className = "set-body";

  const modelName = typeof state.model === "string" ? state.model : (state.model && state.model.id) || "—";
  const mode =
    (typeof MODE_SAID !== "undefined" && MODE_SAID[state.allowing]) || state.allowing || "—";
  const where = state.cwd
    ? typeof withoutHome === "function"
      ? withoutHome(state.cwd)
      : state.cwd
    : "—";
  const facts = document.createElement("div");
  facts.className = "set-group";
  facts.append(factRow("Model", modelName), factRow("Permission mode", mode), factRow("Working in", where));
  wrap.append(facts);

  const cs = state.claudeSettings;
  if (!cs) {
    const note = document.createElement("p");
    note.className = "set-note";
    note.textContent = state.claudeLoading
      ? "Reading Claude Code's settings…"
      : "Couldn't read Claude Code's settings.";
    wrap.append(note);
    return wrap;
  }

  wrap.append(
    listGroup(
      "MCP & connectors",
      (cs.mcp || []).map((s) => (s.transport && s.transport !== "stdio" ? `${s.name} · ${s.transport}` : s.name)),
      "No MCP servers configured.",
    ),
  );
  wrap.append(listGroup("Skills", (cs.skills || []).map((s) => s.name), "No skills found."));
  wrap.append(pluginGroup(cs.plugins || []));
  wrap.append(listGroup("Tools", (state.tools || []).map(String), "—"));
  wrap.append(listGroup("Slash commands", (state.commands || []).map(String), "—"));

  const foot = document.createElement("p");
  foot.className = "set-note";
  foot.textContent =
    "Read from Claude Code. Edit MCP, connectors and skills in Claude Code's /config; plugin changes here apply when Claude Code reloads.";
  wrap.append(foot);
  return wrap;
}

/** A read-only label : value row (model, mode, …). */
function factRow(label, value) {
  const row = document.createElement("div");
  row.className = "set-row set-fact";
  const l = document.createElement("span");
  l.className = "set-label";
  l.textContent = label;
  const v = document.createElement("span");
  v.className = "set-note";
  v.textContent = value;
  row.append(l, v);
  return row;
}

/** One plain line in a read-only list. */
function lineRow(text) {
  const row = document.createElement("div");
  row.className = "set-row set-line";
  const span = document.createElement("span");
  span.className = "set-label";
  span.textContent = text;
  row.append(span);
  return row;
}

/** A titled, read-only list of names (an empty note when there are none). */
function listGroup(title, items, empty) {
  const head = document.createElement("p");
  head.className = "set-section";
  head.textContent = title;
  const group = document.createElement("div");
  group.className = "set-group";
  if (!items.length) {
    if (empty) group.append(lineRow(empty));
  } else {
    for (const item of items) group.append(lineRow(item));
  }
  const frag = document.createDocumentFragment();
  frag.append(head, group);
  return frag;
}

/** Plugins, each with the one safe toggle colai can write (enabledPlugins in settings.json). */
function pluginGroup(plugins) {
  const head = document.createElement("p");
  head.className = "set-section";
  head.textContent = "Plugins";
  const group = document.createElement("div");
  group.className = "set-group";
  if (!plugins.length) {
    group.append(lineRow("No plugins installed."));
  }
  for (const plugin of plugins) {
    group.append(
      toggleRow(plugin.name, plugin.scope ? `${plugin.scope} plugin` : "plugin", plugin.enabled, (next) => {
        void invoke("colai_claude_toggle", { name: plugin.name, enable: next })
          .then(() => {
            plugin.enabled = next;
            state.trouble = `${next ? "Enabled" : "Disabled"} ${plugin.name} — applies when Claude Code reloads.`;
            render();
          })
          .catch((error) => {
            state.trouble = `Couldn't change ${plugin.name} — ${error && error.message ? error.message : String(error)}`;
            render();
          });
      }),
    );
  }
  const frag = document.createDocumentFragment();
  frag.append(head, group);
  return frag;
}
