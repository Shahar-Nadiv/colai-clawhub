// The tools grid: every tool that does not need a key of its own, and the ones people make.
//
// The ⋯ key used to fold four tools in and out of the rail. It opens this instead: a scrollable
// grid with the exact tools (measure, colour, record, screenshot), every tool the person has made,
// and — first, because it is the reason the grid exists — "New tool".
//
// A made tool is a pointer and a prompt with a name and a face: how you mark (a pin, a box, a
// circle, a drawn line, the whole screen) and what Claude should do with what you marked. Using one
// is using that pointer; the mark carries the tool, and its prompt travels with the mark the way a
// git mark's command does (see `summaryFor`).

/** Where made tools are kept, in this browser's own storage beside the rail's position. */
const KIT_KEY = "colai-custom-tools";

/** How a made tool marks, by the built-in tool that does it. */
const KIT_POINTERS = [
  { id: "pointAt", label: "Point" },
  { id: "box", label: "Box" },
  { id: "circle", label: "Circle" },
  { id: "draw", label: "Draw" },
  { id: "screenshot", label: "Whole screen" },
];

/** The faces a made tool can wear without importing one: the rail's own drawings. */
const KIT_GLYPHS = [
  "pointAt", "box", "circle", "draw", "arrow", "highlight", "inspect", "match",
  "redline", "design", "component", "wireframe", "review", "gitBranch", "gitDiff", "measure",
  "colour", "screenshot", "send", "clock", "folder",
];

/** An imported icon is a picture kept in local storage; bigger than this is a photograph, not an icon. */
const KIT_ICON_MOST = 200 * 1024;

/** The exact tools, as the grid shows them. */
const KIT_BUILT_IN = [
  { id: "measure", label: "Measure", glyph: "measure" },
  { id: "colour", label: "Colour", glyph: "colour" },
  { id: "record", label: "Record", glyph: "record" },
  { id: "screenshot", label: "Screenshot", glyph: "screenshot" },
];

/** The made tools, read once at start; a missing or damaged store is an empty kit. */
function loadKit() {
  try {
    const kept = JSON.parse(window.localStorage.getItem(KIT_KEY) || "[]");
    state.kit = Array.isArray(kept) ? kept.filter(isAKitTool) : [];
  } catch {
    state.kit = [];
  }
}

function saveKit() {
  try {
    window.localStorage.setItem(KIT_KEY, JSON.stringify(state.kit));
  } catch {
    // A full or refused store: the tool still works until the toolbar restarts. Said, because a
    // tool that silently vanishes on the next start is a promise broken.
    state.trouble = "That tool could not be saved, so it lasts only until the toolbar restarts.";
  }
}

function isAKitTool(tool) {
  return Boolean(
    tool &&
      typeof tool.id === "string" &&
      typeof tool.name === "string" &&
      typeof tool.prompt === "string" &&
      KIT_POINTERS.some((one) => one.id === tool.pointer),
  );
}

/** The made tool in hand, if the tool in hand is one. */
function kitInHand() {
  if (!state.kitTool) return null;
  const tool = (state.kit || []).find((one) => one.id === state.kitTool);
  return tool && tool.pointer === state.tool ? tool : null;
}

/** Pick up a made tool: its pointer, remembered as this tool so the mark can carry it. */
function useKitTool(id) {
  const tool = (state.kit || []).find((one) => one.id === id);
  if (!tool) return;
  if (tool.pointer === "draw") state.pen = PEN_FIRST;
  use(tool.pointer);
  // After `use`, which puts any made tool down: this one is the one in hand now.
  state.kitTool = tool.id;
  state.open = null;
  render();
}

/** Open the grid, or the editor for a new or existing tool. */
function editKitTool(id) {
  const tool = id ? (state.kit || []).find((one) => one.id === id) : null;
  state.kitEditing = tool
    ? { id: tool.id, name: tool.name, pointer: tool.pointer, prompt: tool.prompt, icon: tool.icon || { glyph: "pointAt" } }
    : { id: null, name: "", pointer: "box", prompt: "", icon: { glyph: "box" } };
  state.kitTrouble = "";
  render();
}

function saveKitTool() {
  const editing = state.kitEditing;
  if (!editing) return;
  const name = editing.name.trim();
  const prompt = editing.prompt.trim();
  if (!name || !prompt) {
    state.kitTrouble = !name ? "Give the tool a name." : "Say what Claude should do with what you mark.";
    render();
    return;
  }
  const saved = {
    id: editing.id || `tool-${Date.now().toString(36)}`,
    name: name.slice(0, 40),
    pointer: editing.pointer,
    prompt: prompt.slice(0, 2000),
    icon: editing.icon,
  };
  const at = state.kit.findIndex((one) => one.id === saved.id);
  if (at >= 0) state.kit[at] = saved;
  else state.kit.push(saved);
  saveKit();
  state.kitEditing = null;
  render();
}

function deleteKitTool(id) {
  state.kit = state.kit.filter((one) => one.id !== id);
  if (state.kitTool === id) state.kitTool = null;
  saveKit();
  state.kitEditing = null;
  render();
}

/** A tool's face: one of the rail's glyphs, or the picture somebody imported. */
function kitFace(icon) {
  const face = document.createElement("span");
  face.className = "kit-face";
  if (icon && typeof icon.image === "string" && icon.image.startsWith("data:image/")) {
    const picture = document.createElement("img");
    picture.src = icon.image;
    picture.alt = "";
    face.append(picture);
  } else {
    face.innerHTML = icon && GLYPHS[icon.glyph] ? iconSvg(icon.glyph, 20) : iconSvg("pointAt", 20);
  }
  return face;
}

/** The rail's own icon drawing, sized. A name of its own so it cannot collide with `icon`. */
function iconSvg(glyph, size) {
  return icon(glyph, size);
}

/** Draw the popup: the grid, or the editor when a tool is being made or changed. */
function drawKit() {
  if (!el.flyTools) return;
  el.flyTools.hidden = state.open !== "tools";
  if (el.flyTools.hidden) return;
  el.flyTools.replaceChildren(...(state.kitEditing ? kitEditor() : kitGrid()));
}

function kitGrid() {
  const head = document.createElement("div");
  head.className = "kit-head";
  head.textContent = "Tools";

  const grid = document.createElement("div");
  grid.className = "kit-grid";

  const add = document.createElement("button");
  add.type = "button";
  add.className = "kit-cell kit-add";
  add.title = "Make a tool: a pointer, a prompt, a name and an icon";
  const plus = document.createElement("span");
  plus.className = "kit-face kit-plus";
  plus.textContent = "+";
  const addSaid = document.createElement("span");
  addSaid.className = "kit-name";
  addSaid.textContent = "New tool";
  add.append(plus, addSaid);
  add.addEventListener("click", () => editKitTool(null));
  grid.append(add);

  for (const built of KIT_BUILT_IN) {
    const cell = document.createElement("button");
    cell.type = "button";
    cell.className = "kit-cell";
    cell.setAttribute("aria-pressed", String(state.tool === built.id && !state.kitTool));
    const name = document.createElement("span");
    name.className = "kit-name";
    name.textContent = built.label;
    cell.append(kitFace({ glyph: built.glyph }), name);
    cell.addEventListener("click", () => {
      use(built.id);
    });
    grid.append(cell);
  }

  for (const tool of state.kit || []) {
    const cell = document.createElement("div");
    cell.className = "kit-cell kit-mine";
    cell.setAttribute("role", "button");
    cell.tabIndex = 0;
    cell.dataset.tool = tool.id;
    cell.setAttribute("aria-pressed", String(state.kitTool === tool.id));
    cell.title = tool.prompt;
    const name = document.createElement("span");
    name.className = "kit-name";
    name.textContent = tool.name;
    const edit = document.createElement("button");
    edit.type = "button";
    edit.className = "kit-edit";
    edit.title = `Change ${tool.name}`;
    edit.setAttribute("aria-label", `Change ${tool.name}`);
    edit.textContent = "✎";
    edit.addEventListener("click", (event) => {
      event.stopPropagation();
      editKitTool(tool.id);
    });
    /*
     * Remove: only on a tool the person made — Colai's own tools are not theirs to lose. One
     * question first, on the tile itself, because a made tool's prompt is not written down
     * anywhere else and cannot be got back.
     */
    if (state.kitRemoving === tool.id) {
      const ask = document.createElement("div");
      ask.className = "kit-confirm";
      const said = document.createElement("span");
      said.textContent = "Remove?";
      const yes = document.createElement("button");
      yes.type = "button";
      yes.className = "kit-confirm-yes";
      yes.textContent = "Remove";
      yes.addEventListener("click", (event) => {
        event.stopPropagation();
        state.kitRemoving = null;
        deleteKitTool(tool.id);
      });
      const no = document.createElement("button");
      no.type = "button";
      no.className = "kit-confirm-no";
      no.textContent = "Keep";
      no.addEventListener("click", (event) => {
        event.stopPropagation();
        state.kitRemoving = null;
        render();
      });
      ask.append(said, yes, no);
      cell.append(kitFace(tool.icon), name, ask);
      grid.append(cell);
      continue;
    }
    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "kit-remove";
    remove.title = `Remove ${tool.name}`;
    remove.setAttribute("aria-label", `Remove ${tool.name}`);
    remove.textContent = "×";
    remove.addEventListener("click", (event) => {
      event.stopPropagation();
      state.kitRemoving = tool.id;
      render();
    });
    cell.append(kitFace(tool.icon), name, edit, remove);
    cell.addEventListener("click", () => useKitTool(tool.id));
    cell.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        useKitTool(tool.id);
      }
    });
    grid.append(cell);
  }
  return [head, grid];
}

function kitEditor() {
  const editing = state.kitEditing;
  const bits = [];

  const head = document.createElement("div");
  head.className = "kit-head";
  head.textContent = editing.id ? "Change tool" : "New tool";
  bits.push(head);

  const nameLabel = document.createElement("label");
  nameLabel.className = "kit-label";
  nameLabel.textContent = "Name";
  const name = document.createElement("input");
  name.className = "kit-input";
  name.dataset.field = "kit-name";
  name.maxLength = 40;
  name.placeholder = "e.g. Explain this error";
  name.value = editing.name;
  name.addEventListener("input", () => {
    editing.name = name.value;
  });
  nameLabel.append(name);
  bits.push(nameLabel);

  const pointerLabel = document.createElement("div");
  pointerLabel.className = "kit-label";
  pointerLabel.textContent = "How you mark";
  const pointers = document.createElement("div");
  pointers.className = "kit-chips";
  for (const one of KIT_POINTERS) {
    const chip = document.createElement("button");
    chip.type = "button";
    chip.className = "kit-chip";
    chip.dataset.pointer = one.id;
    chip.setAttribute("aria-pressed", String(editing.pointer === one.id));
    chip.textContent = one.label;
    chip.addEventListener("click", () => {
      editing.pointer = one.id;
      render();
    });
    pointers.append(chip);
  }
  pointerLabel.append(pointers);
  bits.push(pointerLabel);

  const promptLabel = document.createElement("label");
  promptLabel.className = "kit-label";
  promptLabel.textContent = "Prompt";
  const prompt = document.createElement("textarea");
  prompt.className = "kit-input kit-prompt";
  prompt.dataset.field = "kit-prompt";
  prompt.rows = 3;
  prompt.placeholder = "What Claude should do with what you mark, e.g. Explain this error and fix it.";
  prompt.value = editing.prompt;
  prompt.addEventListener("input", () => {
    editing.prompt = prompt.value;
  });
  promptLabel.append(prompt);
  bits.push(promptLabel);

  const iconLabel = document.createElement("div");
  iconLabel.className = "kit-label";
  iconLabel.textContent = "Icon";
  const icons = document.createElement("div");
  icons.className = "kit-icons";
  for (const glyph of KIT_GLYPHS.filter((one) => GLYPHS[one])) {
    const pick = document.createElement("button");
    pick.type = "button";
    pick.className = "kit-icon";
    pick.dataset.glyph = glyph;
    pick.setAttribute("aria-pressed", String(editing.icon && editing.icon.glyph === glyph));
    pick.innerHTML = iconSvg(glyph, 18);
    pick.addEventListener("click", () => {
      editing.icon = { glyph };
      render();
    });
    icons.append(pick);
  }
  // Import: a picture from disk, kept as a data URL. Small only — an icon, not a photograph.
  const imported = document.createElement("label");
  imported.className = "kit-icon kit-import";
  imported.title = "Import an icon (PNG, SVG, JPG — up to 200 KB)";
  if (editing.icon && editing.icon.image) {
    imported.setAttribute("aria-pressed", "true");
    imported.append(kitFace(editing.icon));
  } else {
    imported.textContent = "Import…";
  }
  const file = document.createElement("input");
  file.type = "file";
  file.accept = "image/png,image/svg+xml,image/jpeg,image/webp,image/gif";
  file.hidden = true;
  file.addEventListener("change", () => {
    const chosen = file.files && file.files[0];
    if (!chosen) return;
    if (chosen.size > KIT_ICON_MOST) {
      state.kitTrouble = "That picture is too big for an icon — up to 200 KB.";
      render();
      return;
    }
    const reader = new FileReader();
    reader.addEventListener("load", () => {
      if (typeof reader.result === "string" && reader.result.startsWith("data:image/")) {
        editing.icon = { image: reader.result };
        state.kitTrouble = "";
      }
      render();
    });
    reader.readAsDataURL(chosen);
  });
  imported.append(file);
  icons.append(imported);
  iconLabel.append(icons);
  bits.push(iconLabel);

  if (state.kitTrouble) {
    const trouble = document.createElement("p");
    trouble.className = "kit-trouble";
    trouble.textContent = state.kitTrouble;
    bits.push(trouble);
  }

  const row = document.createElement("div");
  row.className = "kit-actions";
  if (editing.id) {
    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "kit-delete";
    remove.textContent = "Delete";
    remove.addEventListener("click", () => deleteKitTool(editing.id));
    row.append(remove);
  }
  const cancel = document.createElement("button");
  cancel.type = "button";
  cancel.className = "kit-cancel";
  cancel.textContent = "Cancel";
  cancel.addEventListener("click", () => {
    state.kitEditing = null;
    render();
  });
  const save = document.createElement("button");
  save.type = "button";
  save.className = "kit-save";
  save.textContent = editing.id ? "Save" : "Create tool";
  save.addEventListener("click", saveKitTool);
  row.append(cancel, save);
  bits.push(row);
  return bits;
}
