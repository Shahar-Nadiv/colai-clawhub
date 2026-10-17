// The person's artifacts, one key per kind: Docs, Slides, Dashboard — and a door to Claude Design.
//
// Claude Design projects are not artifacts, and nothing a plugin can call lists them, so the Design
// key does not pretend to: it opens claude.ai/design, where they live. Each shelf can also show
// every artifact — most are plain pages of no type, and would otherwise be on no shelf at all.
//
// Each key opens a scrollable popup like the tools grid: "New <kind>" first, then the person's own
// artifacts of that kind, newest first, each opening in the browser. They are listed through the
// bridge inside the person's own Claude session (see colai_artifacts.rs) — the only place signed in
// to claude.ai — and a new one is asked of the chat that is receiving, so Claude makes it with the
// whole conversation and any marks to draw on, rather than the toolbar making an empty shell.

/*
 * A preview per kind, drawn — not a picture of the artifact.
 *
 * Claude keeps each artifact's thumbnail to itself: neither the listing nor a read of one hands a
 * plugin an image, and the pages need the person's claude.ai sign-in to render. So each card
 * shows what kind of thing it is, the way Claude's own gallery reads at a glance: a slide in its
 * frame, a page of text, a chart, artboards on a canvas. Quiet ink on the card's own tint.
 */
const ARTIFACT_PREVIEWS = {
  Slides:
    '<rect x="18" y="14" width="124" height="70" rx="5" class="ap-sheet"/><rect x="32" y="30" width="58" height="7" rx="2" class="ap-ink"/><rect x="32" y="44" width="80" height="4" rx="2" class="ap-soft"/><rect x="32" y="53" width="66" height="4" rx="2" class="ap-soft"/><rect x="106" y="28" width="22" height="22" rx="4" class="ap-accent"/><circle cx="74" cy="92" r="2" class="ap-ink"/><circle cx="82" cy="92" r="2" class="ap-soft"/><circle cx="90" cy="92" r="2" class="ap-soft"/>',
  Docs:
    '<rect x="48" y="8" width="64" height="86" rx="5" class="ap-sheet"/><rect x="58" y="20" width="34" height="6" rx="2" class="ap-ink"/><rect x="58" y="34" width="44" height="3.5" rx="1.75" class="ap-soft"/><rect x="58" y="42" width="40" height="3.5" rx="1.75" class="ap-soft"/><rect x="58" y="50" width="44" height="3.5" rx="1.75" class="ap-soft"/><rect x="58" y="62" width="30" height="3.5" rx="1.75" class="ap-accent"/><rect x="58" y="70" width="42" height="3.5" rx="1.75" class="ap-soft"/><rect x="58" y="78" width="36" height="3.5" rx="1.75" class="ap-soft"/>',
  Dashboard:
    '<rect x="16" y="12" width="128" height="76" rx="5" class="ap-sheet"/><rect x="26" y="22" width="30" height="12" rx="3" class="ap-soft"/><rect x="62" y="22" width="30" height="12" rx="3" class="ap-soft"/><rect x="98" y="22" width="36" height="12" rx="3" class="ap-soft"/><rect x="28" y="62" width="9" height="18" rx="2" class="ap-soft"/><rect x="42" y="52" width="9" height="28" rx="2" class="ap-soft"/><rect x="56" y="58" width="9" height="22" rx="2" class="ap-soft"/><rect x="70" y="44" width="9" height="36" rx="2" class="ap-accent"/><path d="M90 74 L104 60 L116 66 L132 46" class="ap-line"/>',
  Design:
    '<rect x="12" y="20" width="62" height="62" rx="5" class="ap-sheet"/><rect x="84" y="12" width="64" height="44" rx="5" class="ap-sheet"/><rect x="84" y="62" width="64" height="26" rx="5" class="ap-sheet"/><circle cx="43" cy="44" r="12" class="ap-accent"/><rect x="26" y="64" width="34" height="4" rx="2" class="ap-soft"/><rect x="94" y="24" width="30" height="5" rx="2" class="ap-ink"/><rect x="94" y="34" width="44" height="3.5" rx="1.75" class="ap-soft"/><rect x="94" y="72" width="22" height="5" rx="2.5" class="ap-soft"/>',
  // Any page: a browser window, its address bar, a heading and some text.
  All:
    '<rect x="16" y="10" width="128" height="80" rx="5" class="ap-sheet"/><circle cx="26" cy="20" r="2.2" class="ap-soft"/><circle cx="33" cy="20" r="2.2" class="ap-soft"/><circle cx="40" cy="20" r="2.2" class="ap-soft"/><rect x="50" y="17" width="80" height="6" rx="3" class="ap-soft"/><rect x="28" y="36" width="56" height="7" rx="2" class="ap-ink"/><rect x="28" y="50" width="104" height="4" rx="2" class="ap-soft"/><rect x="28" y="59" width="92" height="4" rx="2" class="ap-soft"/><rect x="28" y="72" width="24" height="8" rx="4" class="ap-accent"/>',
};

function artifactPreview(kind, seed) {
  const holder = document.createElement("span");
  holder.className = "art-thumb";
  holder.dataset.kind = kind;
  holder.innerHTML = `<svg viewBox="0 0 160 100" aria-hidden="true">${ARTIFACT_PREVIEWS[kind] || ""}</svg>`;
  if (seed) varyPreview(holder, seed);
  return holder;
}

/*
 * The same drawing would make every card look like one artifact. Each is varied a little by its
 * own title — bar heights, line lengths, where the accent sits — so a shelf reads as several
 * things, and the same artifact always looks the same.
 */
function varyPreview(holder, seed) {
  let hash = 2166136261;
  for (const ch of String(seed)) hash = Math.imul(hash ^ ch.charCodeAt(0), 16777619) >>> 0;
  const next = () => {
    hash = Math.imul(hash ^ (hash >>> 15), 2246822507) >>> 0;
    return (hash % 1000) / 1000;
  };
  for (const bar of holder.querySelectorAll("rect.ap-soft, rect.ap-ink")) {
    const wide = Number(bar.getAttribute("width"));
    const tall = Number(bar.getAttribute("height"));
    if (tall > wide) {
      // A chart bar: grow or shrink it from its base.
      const base = Number(bar.getAttribute("y")) + tall;
      const grown = Math.max(6, Math.round(tall * (0.55 + next() * 0.7)));
      bar.setAttribute("height", String(grown));
      bar.setAttribute("y", String(base - grown));
    } else if (wide > 14) {
      bar.setAttribute("width", String(Math.round(wide * (0.6 + next() * 0.4))));
    }
  }
  const accent = holder.querySelector(".ap-accent");
  if (accent && accent.tagName.toLowerCase() === "rect" && next() < 0.35) accent.remove();
}

/** The kinds, in rail order, with how each is drawn and spoken of. */
const ARTIFACT_KINDS = [
  // Not a shelf: a door to Claude Design, whose projects no plugin can list.
  { id: "Design", title: "Claude Design ↗", glyph: "artDesign", door: "https://claude.ai/design" },
  // `title` is the key's name: plural, because it is a shelf of them.
  { id: "Docs", title: "Docs", glyph: "artDocs", many: "docs", one: "doc" },
  { id: "Slides", title: "Slides", glyph: "artSlides", many: "slide decks", one: "slide deck" },
  { id: "Dashboard", title: "Dashboards", glyph: "artDashboard", many: "dashboards", one: "dashboard" },
];

/** Every artifact, typed or not — the shelf's other half. */
const ALL_ARTIFACTS = { id: "All", title: "All artifacts", many: "artifacts", one: "artifact" };

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
  if (artifactsOpen() === kind) void loadArtifacts(shelfOf(kind));
}

/** What the open shelf lists: the key's kind, or everything. */
function shelfOf(kind) {
  return state.artAll ? "All" : kind;
}

/** Open a claude.ai place in the browser — Claude Design, or one artifact. */
function openDoor(url) {
  state.open = null;
  void invoke("colai_open_artifact", { url: url }).catch((trouble) => {
    state.trouble = String(trouble);
    render();
  });
  render();
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
    if (!line.startsWith("- ")) continue;
    // Parts by what they are, not where they sit: the per-type listing carries the type's
    // description between title and link (`(mine) Title — what the type is — link`), the plain
    // one an "updated" date after the link. The title is first; the link is the claude.ai part.
    const parts = line.slice(2).split(" — ").map((part) => part.trim());
    const url = parts.find((part) => /^https:\/\/claude\.ai\/\S+$/.test(part));
    if (!url) continue;
    const title = parts[0].replace(/^(?:\([^)]*\)\s*)+/, "").trim() || "Untitled";
    const dated = parts.find((part) => part.startsWith("updated "));
    rows.push({ title, url, updated: dated ? dated.slice("updated ".length) : "" });
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
  const shelf = shelfOf(kind);
  const shown = shelf === "All" ? ALL_ARTIFACTS : named;
  const listed = state.artifacts[shelf] || { items: [], loading: false, error: "" };

  const head = document.createElement("div");
  head.className = "kit-head art-head";
  // The key's kind, or all of them: a segmented switch, the way Claude's own filters read.
  const title = document.createElement("span");
  title.className = "art-seg";
  for (const [id, said] of [[kind, named.title], ["All", "All"]]) {
    const tab = document.createElement("button");
    tab.type = "button";
    tab.className = "art-seg-tab";
    tab.dataset.on = String(shelf === id);
    tab.textContent = said;
    tab.addEventListener("click", () => {
      state.artAll = id === "All";
      void loadArtifacts(shelfOf(kind));
    });
    title.append(tab);
  }
  const again = document.createElement("button");
  again.type = "button";
  again.className = "art-refresh";
  again.title = `Look up your ${shown.many} again`;
  again.setAttribute("aria-label", again.title);
  again.textContent = "↻";
  again.addEventListener("click", () => void loadArtifacts(shelf));
  head.append(title, again);

  if (state.artNew === kind) {
    el.flyArt.replaceChildren(head, ...artifactForm(kind, named));
    return;
  }

  const grid = document.createElement("div");
  grid.className = "art-grid";

  const add = document.createElement("button");
  add.type = "button";
  add.className = "art-card kit-add";
  add.title = `Ask Claude to make a new ${named.one}`;
  const plus = document.createElement("span");
  plus.className = "art-thumb art-new";
  plus.textContent = "+";
  const addMeta = document.createElement("span");
  addMeta.className = "art-meta";
  const addSaid = document.createElement("span");
  addSaid.className = "kit-name art-title";
  addSaid.textContent = `New ${named.one}`;
  const addSub = document.createElement("span");
  addSub.className = "art-sub";
  addSub.textContent = "Made in your chat";
  addMeta.append(addSaid, addSub);
  add.append(plus, addMeta);
  add.addEventListener("click", () => {
    state.artNew = kind;
    render();
  });
  grid.append(add);

  for (const item of listed.items) {
    const card = document.createElement("button");
    card.type = "button";
    card.className = "art-card";
    card.title = `Open ${item.title}`;
    const meta = document.createElement("span");
    meta.className = "art-meta";
    const name = document.createElement("span");
    name.className = "kit-name art-title";
    name.dataset.untitled = String(item.title === "Untitled");
    name.textContent = item.title;
    const sub = document.createElement("span");
    sub.className = "art-sub";
    const what = shelf === "All" ? "Artifact" : named.title;
    sub.textContent = item.updated ? `${what} · ${item.updated}` : what;
    meta.append(name, sub);
    const open = document.createElement("span");
    open.className = "art-open";
    open.setAttribute("aria-hidden", "true");
    open.textContent = "↗";
    card.append(artifactPreview(shelf, item.title + item.url), meta, open);
    card.addEventListener("click", () => openDoor(item.url));
    grid.append(card);
  }

  const bits = [head, grid];
  const note = listed.loading
    ? `Looking up your ${shown.many}…`
    : listed.error
      ? listed.error
      : listed.items.length === 0
        ? `No ${shown.many} yet. Make one with +.`
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
