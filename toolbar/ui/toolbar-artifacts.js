// The person's artifacts, one key per kind: Design, Docs, Slides, Dashboard.
//
// Each key opens a scrollable popup like the tools grid: "New <kind>" first, then the person's own
// artifacts of that kind, newest first, each opening in the browser. They are listed through the
// bridge inside the person's own Claude session (see colai_artifacts.rs) — the only place signed in
// to claude.ai — and a new one is asked of the chat that is receiving, so Claude makes it with the
// whole conversation and any marks to draw on, rather than the toolbar making an empty shell.

/** The kinds, in rail order, with how each is drawn and spoken of. */
const ARTIFACT_KINDS = [
  // `title` is the key's name: plural, because it is a shelf of them — and so "Designs" here is never
  // confused with the Design key beside it, which makes wireframes and redlines.
  { id: "Design", title: "Designs", glyph: "artDesign", many: "designs", one: "design" },
  { id: "Docs", title: "Docs", glyph: "artDocs", many: "docs", one: "doc" },
  { id: "Slides", title: "Slides", glyph: "artSlides", many: "slide decks", one: "slide deck" },
  { id: "Dashboard", title: "Dashboards", glyph: "artDashboard", many: "dashboards", one: "dashboard" },
];

function artifactKind(id) {
  return ARTIFACT_KINDS.find((one) => one.id === id) || null;
}

/** Which kind's popup is open, if one is. */
function artifactsOpen() {
  return typeof state.open === "string" && state.open.startsWith("art:") ? state.open.slice(4) : null;
}

/** Open (or close) a kind's popup, and look the list up fresh when it opens. */
function openArtifacts(kind) {
  state.artNew = null;
  flyout(`art:${kind}`);
  if (artifactsOpen() === kind) void loadArtifacts(kind);
}

async function loadArtifacts(kind) {
  const had = state.artifacts[kind];
  state.artifacts[kind] = { items: had ? had.items : [], loading: true, error: "" };
  render();
  try {
    const text = await invoke("colai_artifacts", { kind: kind, session: state.receiving.id || null });
    state.artifacts[kind] = { items: parseArtifacts(text), loading: false, error: "" };
  } catch (trouble) {
    state.artifacts[kind] = {
      items: had ? had.items : [],
      loading: false,
      error: trouble && trouble.message ? trouble.message : String(trouble),
    };
  }
  render();
}

/**
 * The Artifact tool's listing, read back into rows.
 *
 * It writes one line per artifact — `- (mine) Title — https://claude.ai/artifact/… — updated
 * 2026-09-26` — and anything else (a header, a "more not shown" note) is not a row.
 */
function parseArtifacts(text) {
  const rows = [];
  for (const line of String(text || "").split("\n")) {
    const found = line.match(/^-\s+(?:\([^)]*\)\s+)?(.+?)\s+—\s+(https:\/\/claude\.ai\/\S+)(?:\s+—\s+updated\s+(\S+))?/);
    if (found) rows.push({ title: found[1], url: found[2], updated: found[3] || "" });
  }
  return rows;
}

/** Ask the receiving chat to make a new artifact of this kind, with any marks as its material. */
function createArtifact(kind, about) {
  const named = artifactKind(kind);
  if (!named) return;
  const marks = typeof chosenMarks === "function" ? chosenMarks() : [];
  const wish = (about || "").trim();
  state.text =
    `Create a new ${kind} artifact` +
    (wish ? ` — ${wish}` : "") +
    `. Use Claude's ${kind} artifact type` +
    (marks.length ? ", and build it from what I marked." : ".");
  state.artNew = null;
  state.open = null;
  void sendMarks(marks.map((mark) => mark.id));
}

function drawArtifacts() {
  if (!el.flyArt) return;
  const kind = artifactsOpen();
  el.flyArt.hidden = !kind;
  if (!kind) return;
  const named = artifactKind(kind);
  const listed = state.artifacts[kind] || { items: [], loading: false, error: "" };

  const head = document.createElement("div");
  head.className = "kit-head art-head";
  const title = document.createElement("span");
  title.textContent = named.title;
  const again = document.createElement("button");
  again.type = "button";
  again.className = "art-refresh";
  again.title = `Look up your ${named.many} again`;
  again.setAttribute("aria-label", again.title);
  again.textContent = "↻";
  again.addEventListener("click", () => void loadArtifacts(kind));
  head.append(title, again);

  if (state.artNew === kind) {
    el.flyArt.replaceChildren(head, ...artifactForm(kind, named));
    return;
  }

  const grid = document.createElement("div");
  grid.className = "kit-grid art-grid";

  const add = document.createElement("button");
  add.type = "button";
  add.className = "kit-cell kit-add";
  add.title = `Ask Claude to make a new ${named.one}`;
  const plus = document.createElement("span");
  plus.className = "kit-face kit-plus";
  plus.textContent = "+";
  const addSaid = document.createElement("span");
  addSaid.className = "kit-name";
  addSaid.textContent = `New ${named.one}`;
  add.append(plus, addSaid);
  add.addEventListener("click", () => {
    state.artNew = kind;
    render();
  });
  grid.append(add);

  for (const item of listed.items) {
    const card = document.createElement("button");
    card.type = "button";
    card.className = "kit-cell art-card";
    card.title = item.updated ? `${item.title} · updated ${item.updated}` : item.title;
    const name = document.createElement("span");
    name.className = "kit-name";
    name.textContent = item.title;
    const when = document.createElement("span");
    when.className = "art-when";
    when.textContent = item.updated;
    card.append(kitFace({ glyph: named.glyph }), name, when);
    card.addEventListener("click", () => {
      void invoke("colai_open_artifact", { url: item.url }).catch((trouble) => {
        state.trouble = String(trouble);
        render();
      });
    });
    grid.append(card);
  }

  const bits = [head, grid];
  const note = listed.loading
    ? `Looking up your ${named.many}…`
    : listed.error
      ? listed.error
      : listed.items.length === 0
        ? `No ${named.many} yet. Make one with +.`
        : "";
  if (note) {
    const said = document.createElement("p");
    said.className = listed.error ? "art-note art-trouble" : "art-note";
    said.textContent = note;
    bits.push(said);
  }
  el.flyArt.replaceChildren(...bits);
}

function artifactForm(kind, named) {
  const label = document.createElement("label");
  label.className = "kit-label";
  label.textContent = `What should the ${named.one} be about?`;
  const about = document.createElement("textarea");
  about.className = "kit-input kit-prompt";
  about.dataset.field = "art-about";
  about.rows = 3;
  about.placeholder =
    kind === "Slides"
      ? "e.g. A 6-slide pitch for the new onboarding flow"
      : kind === "Dashboard"
        ? "e.g. Weekly signups and churn from the CSV I marked"
        : kind === "Docs"
          ? "e.g. A one-page brief for the redesign"
          : "e.g. A landing page for the launch";
  about.value = state.artAbout || "";
  about.addEventListener("input", () => {
    state.artAbout = about.value;
  });
  label.append(about);
  const marks = typeof chosenMarks === "function" ? chosenMarks().length : 0;
  const hint = document.createElement("p");
  hint.className = "art-note";
  hint.textContent = marks
    ? `Goes to your chat with ${marks} mark${marks === 1 ? "" : "s"} as material.`
    : "Goes to your chat; Claude makes it there.";
  const row = document.createElement("div");
  row.className = "kit-actions";
  const cancel = document.createElement("button");
  cancel.type = "button";
  cancel.className = "kit-cancel";
  cancel.textContent = "Cancel";
  cancel.addEventListener("click", () => {
    state.artNew = null;
    render();
  });
  const make = document.createElement("button");
  make.type = "button";
  make.className = "kit-save";
  make.textContent = `Create ${named.one}`;
  make.addEventListener("click", () => {
    const wish = state.artAbout || "";
    state.artAbout = "";
    createArtifact(kind, wish);
  });
  row.append(cancel, make);
  return [label, hint, row];
}
