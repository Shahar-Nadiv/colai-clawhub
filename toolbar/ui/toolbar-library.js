// Choosing a component, or a design system, out of somebody's catalogue.
//
// A window rather than a flyout, and the only one in this toolbar. Every other menu here
// is a list of words, where the thing being chosen *is* its name — a pen, a length, a
// conversation. A component is not: nobody recognises "Aurora card" from the words, they
// recognise it from the picture. So this needs room for pictures, and room is what makes
// it a window.
//
// Nothing here holds an account. The search runs through the agent's own catalogue tool,
// which means whatever somebody connected in OpenClaw is what they see, and colai stores
// none of it. When nothing is connected the window says so and says what to do about it,
// which is the whole reason it opens rather than the chip greying out: a disabled chip
// teaches nothing.

/** How long the window waits after a keystroke before asking. */
const LIBRARY_SETTLE = 300;

let asking = 0;

/**
 * Open the library for one mark, and remember which mark it is for.
 *
 * The mark rather than a copy of its kind: what gets chosen is written back onto it, and
 * a window that had only been told "a component" could not put the answer anywhere.
 */
function openLibrary(mark) {
  state.library = {
    markId: mark.id,
    kind: kindIdOf(mark),
    query: "",
    mine: true,
    loading: true,
    found: null,
    trouble: null,
  };
  state.open = null;
  render();
  // The library exists to be searched, so the cursor starts in the search. It opened with
  // focus nowhere, which for a panel whose whole content is a list of results meant the
  // first keystroke went to the page — where, on this surface, a letter is a tool
  // shortcut.
  const find = el.library.querySelector(".library-find");
  if (find) find.focus({ preventScroll: true });
  void lookInLibrary();
}

function closeLibrary() {
  state.library = null;
  render();
}

/**
 * Ask the catalogue, and ignore an answer that arrived too late to be about the question.
 *
 * Typing produces one of these every few hundred milliseconds, and they come back out of
 * order — an answer to "pri" landing after the answer to "pricing" would replace the
 * results with worse ones while somebody watched.
 */
async function lookInLibrary() {
  const open = state.library;
  if (!open) return;
  const mine = open.mine;
  const asked = ++asking;
  open.loading = true;
  render();
  try {
    const found = await invoke("colai_library_search", {
      kind: open.kind,
      query: open.query,
      mine,
      agentId: state.receiving.kind === "agent" ? state.receiving.id : null,
      sessionKey: state.receiving.kind === "agent" ? null : state.receiving.id,
    });
    if (asked !== asking || !state.library) return;
    // An answer with nothing in it is not a crash to show somebody. Reading straight
    // through it put "undefined is not an object" in the window where the results go.
    state.library.found = found || { library: null, cards: [], connect: true, trouble: null };
    state.library.trouble = state.library.found.trouble || null;
  } catch (error) {
    if (asked !== asking || !state.library) return;
    state.library.found = null;
    state.library.trouble = error && error.message ? error.message : String(error);
  }
  if (state.library) state.library.loading = false;
  render();
}

let settling = null;
function lookLater() {
  if (settling !== null) clearTimeout(settling);
  settling = setTimeout(() => void lookInLibrary(), LIBRARY_SETTLE);
}

/** Take the choice back to the mark that asked for it, and close. */
function chooseFromLibrary(card) {
  const open = state.library;
  if (!open) return;
  const mark = state.marks.find((one) => one.id === open.markId);
  if (mark) {
    mark.source = "library";
    // Everything the message will need, kept on the mark rather than looked up again at
    // send time — the window will be closed by then and the search that found it gone.
    mark.fromLibrary = {
      id: card.id,
      name: card.name,
      library: (open.found && open.found.library) || "the library",
      url: card.url || null,
      install: card.install || null,
      preview: card.preview || null,
    };
  }
  state.library = null;
  // Back to the mark it belongs to, so the choice lands somewhere somebody can see it.
  if (mark) state.popup = mark.id;
  render();
}

/** What the window is currently showing, so an unchanged render leaves it alone. */
let shown = "";

function drawLibrary() {
  const open = state.library;
  el.library.hidden = open === null;
  if (!open) {
    shown = "";
    return;
  }
  /*
   * Rebuilt only when what it says changed.
   *
   * Everything in here is thrown away and made again on every render, and the rail
   * re-renders for reasons that have nothing to do with this window — an agent replying,
   * the five-second look at what every agent is doing. A render landing between somebody
   * pressing the close button and letting go replaces that button with a new one, and a
   * click needs both halves on the *same* element: so the press did nothing, silently,
   * and only sometimes. Leaving the DOM alone when nothing changed is what makes the
   * button survive long enough to be clicked.
   */
  const asItStands = JSON.stringify([
    open.kind,
    open.mine,
    open.loading,
    open.trouble,
    open.found && open.found.connect,
    open.found && open.found.library,
    open.found && open.found.cards.map((card) => card.id),
  ]);
  if (asItStands === shown) {
    // Still placed: the screen it belongs on can change under it when the rail moves.
    placeLibrary();
    return;
  }
  shown = asItStands;
  const kind = DESIGNS[open.kind] || DESIGNS[DESIGN_FIRST];

  const head = document.createElement("div");
  head.className = "library-head";
  const title = document.createElement("p");
  title.className = "agents-title";
  title.textContent = `${kind.label} — from a library`;
  head.append(title);
  const shut = document.createElement("button");
  shut.type = "button";
  shut.className = "popup-shut";
  shut.title = "Close";
  shut.setAttribute("aria-label", "Close the library");
  shut.textContent = "×";
  shut.addEventListener("click", closeLibrary);
  head.append(shut);

  const find = document.createElement("input");
  find.className = "popup-note library-find";
  find.dataset.field = "library-find";
  find.type = "text";
  find.value = open.query;
  find.placeholder =
    open.kind === "system" ? "A theme — dark, violet…" : "A component — pricing table…";
  find.setAttribute("aria-label", "Search the library");
  find.addEventListener("input", () => {
    open.query = find.value;
    lookLater();
  });

  // Two places to look, and yours first. Somebody who has collected components has
  // already done the choosing once, and making them search a public catalogue for a
  // thing they bookmarked is asking them to do it again.
  const where = document.createElement("div");
  where.className = "mode-row";
  for (const [mine, label] of [
    [true, "Yours"],
    [false, "Browse"],
  ]) {
    const chip = document.createElement("button");
    chip.type = "button";
    chip.className = "chip";
    chip.setAttribute("aria-pressed", String(open.mine === mine));
    chip.textContent = label;
    chip.addEventListener("click", () => {
      open.mine = mine;
      void lookInLibrary();
    });
    where.append(chip);
  }

  const rows = [head, find, where];
  const found = open.found;
  if (open.loading) {
    rows.push(saying("Looking…"));
  } else if (found && found.connect) {
    // The one state this window exists to handle well. Not an error: nothing is broken,
    // there is simply nothing connected yet, and the only useful thing to say is where
    // to go and connect one.
    // Named exactly, and the server's name matters: a tool is addressed as
    // `mcp__<server>__…`, so what somebody types in that field is what the toolbar has
    // to look under. Saying "add an MCP server" and leaving them to pick a name is how
    // this ends up genuinely connected and still reported as missing.
    const known = state.libraries || [];
    const told = known.length
      ? known
          .map(
            (one) =>
              `${one.label} — add it in OpenClaw under Settings → MCP, and name the server “${one.called}”.`,
          )
          .join(" ")
      : "Add one in OpenClaw under Settings → MCP.";
    rows.push(saying(`No component library is connected to this agent yet. ${told}`));
  } else if (open.trouble) {
    rows.push(saying(`Could not look — ${open.trouble}`));
  } else if (!found || found.cards.length === 0) {
    rows.push(saying(open.mine ? "Nothing saved here yet." : "Nothing matched."));
  } else {
    const grid = document.createElement("div");
    grid.className = "library-grid scrolls";
    for (const card of found.cards) grid.append(cardIn(card));
    rows.push(grid);
  }
  el.library.replaceChildren(...rows);
  placeLibrary();
}

/**
 * Put the window on the screen the mark is on, in the middle of it.
 *
 * Measured after it is filled, because how tall it is depends on how many results came
 * back — and a window placed from a guessed height sits off centre by however wrong the
 * guess was.
 */
function placeLibrary() {
  const open = state.library;
  if (!open) return;
  const mark = state.marks.find((one) => one.id === open.markId);
  // The middle of what was marked. A mark with no region — a pin — still has a point,
  // and a mark that has gone leaves the rail, which is at least somewhere somebody is.
  const at =
    mark && mark.region
      ? {
          x: (mark.region.box.x + mark.region.box.w / 2) * window.innerWidth,
          y: (mark.region.box.y + mark.region.box.h / 2) * window.innerHeight,
        }
      : state.at || { x: 0, y: 0 };
  const put = centredIn(usable(screenAt(state.screens, at)), el.library.getBoundingClientRect());
  el.library.style.left = `${put.x}px`;
  el.library.style.top = `${put.y}px`;
}

/** One thing to choose, as a picture with its name under it. */
/**
 * An address safe to fetch a picture from, or nothing.
 *
 * `https:` only. `file:` would read the user's own disk into the page, and plain `http:`
 * announces every keystroke of a library search to anybody on the network between here
 * and the catalogue.
 *
 * *Which host* is no longer decided here. It is decided twice, above and below this: Rust
 * drops any preview that is not on the host its catalogue keeps pictures on
 * (`from_the_catalogue`), and the page's content policy names the same host, so the
 * browser refuses one that somehow arrives anyway. This is left as the scheme check it
 * always was — the third of three, and the cheapest.
 */
function httpsOnly(address) {
  if (typeof address !== "string" || !address) return null;
  try {
    return new URL(address).protocol === "https:" ? address : null;
  } catch {
    // Not an address at all.
    return null;
  }
}

function cardIn(card) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "library-card";
  button.title = card.blurb || card.name;

  const shot = document.createElement("div");
  shot.className = "library-shot";
  /*
   * A preview is an address from a catalogue server, so it is checked before it is
   * fetched.
   *
   * Loading it is an outbound request from the toolbar to a third party, and it hands
   * attacker-chosen bytes to whatever image decoder the user's WebKitGTK happens to be.
   * The address used to be taken as given apart from its scheme, which meant a catalogue
   * that had been compromised could name any host at all and have every toolbar that
   * opened this panel go and fetch from it — a beacon, carrying each user's address and
   * browser fingerprint, chosen by somebody else.
   *
   * It cannot now: the host is pinned to the one its catalogue keeps pictures on, in Rust
   * where the card is parsed and again in the content policy the page is served under.
   * What is left is the request to that host, which is the cost of showing its pictures
   * at all.
   */
  const preview = httpsOnly(card.preview);
  if (preview) {
    const picture = document.createElement("img");
    picture.src = preview;
    picture.alt = "";
    picture.loading = "lazy";
    // Nothing about which card was looked at travels to the catalogue's server.
    picture.referrerPolicy = "no-referrer";
    picture.crossOrigin = "anonymous";
    // A catalogue's picture may be gone, and a broken-image glyph in a grid of previews
    // reads as a broken toolbar. The empty frame reads as a thing with no picture.
    picture.addEventListener("error", () => picture.remove());
    shot.append(picture);
  }
  button.append(shot);

  const name = document.createElement("span");
  name.className = "library-name";
  name.textContent = card.name;
  button.append(name);
  if (card.author) {
    const by = document.createElement("span");
    by.className = "row-under";
    by.textContent = card.author;
    button.append(by);
  }
  button.addEventListener("click", () => chooseFromLibrary(card));
  return button;
}
