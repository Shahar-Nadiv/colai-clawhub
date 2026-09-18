// The two surfaces somebody types into: the popup on a mark, and the composer.
//
// The popup opens where they were looking and is about one mark; the composer hangs off
// the rail and is about everything waiting to go. They are the same conversation at two
// sizes, which is why they share a receiver, a mode, and the files coming along — and
// why they live in one file rather than growing two ideas of what a send is.
//
// The agent picker is here too: what it draws is the list those two choose from, and
// the choosing is the point of both of them.

/**
 * Ask the desktop where this document should go.
 *
 * Relative to the project the marked window was working in, when the folder is inside
 * it. The destination is read by an agent that may not be on this machine, so an
 * absolute path from this one is an instruction only this machine could follow — Rust
 * decides that, since it is the side that knows what was actually chosen.
 *
 * Cancelling leaves the field exactly as it was, which is why nothing is written until
 * an answer comes back.
 */
async function chooseHome(mark) {
  try {
    const within = (mark.where && mark.where.cwd) || null;
    const chosen = await invoke("colai_pick_folder", { within });
    if (!chosen) return;
    mark.dest = chosen.said;
    mark.destTyped = true;
    render();
  } catch (error) {
    state.trouble = `Could not open the file chooser — ${
      error && error.message ? error.message : String(error)
    }`;
    render();
  }
}

/**
 * The one line of a mark's address that fits beside its thumbnail.
 *
 * The most specific thing known, because that is the thing worth checking: a URL beats
 * a file, a file beats a directory, a directory beats an application name. The whole
 * address goes in the message; this is only enough to see it was picked up right.
 */
function placeSaid(mark) {
  const where = mark.where;
  if (!where || !where.app) return null;
  if (where.url) return where.url;
  const place = placeOf(where);
  if (place.file) return place.file;
  if (place.path) return place.path;
  if (where.cwd) return where.cwd;
  return where.app;
}

function drawPopup() {
  const mark = state.marks.find((held) => held.id === state.popup);
  if (!mark) {
    el.popup.hidden = true;
    return;
  }
  el.popup.hidden = false;
  const rows = [];

  const head = document.createElement("div");
  head.className = "popup-head";
  if (mark.thumb) {
    const shot = document.createElement("img");
    shot.className = "popup-shot";
    shot.src = mark.thumb;
    shot.alt = "";
    head.append(shot);
  }
  const named = document.createElement("div");
  named.className = "popup-named";
  const what = document.createElement("strong");
  what.textContent = labelOf(mark);
  const size = document.createElement("span");
  size.className = "popup-size";
  // What the mark knows, when it knows something exact. A colour you cannot see is a
  // colour you have to send to somebody else to find out.
  const detail = detailOf(mark);
  size.textContent = mark.trouble ? mark.trouble : detail || mark.shot || "taking a picture…";
  if (mark.hex) {
    const swatch = document.createElement("span");
    swatch.className = "popup-swatch";
    swatch.style.background = mark.hex;
    size.prepend(swatch);
  }
  named.append(what, size);
  // Where it was captured, on the mark rather than in a log. This is the half of a mark
  // an agent will act on, and somebody should be able to see it was picked up correctly
  // before they send it — not find out afterwards that the address was wrong.
  const at = placeSaid(mark);
  if (at) {
    const place = document.createElement("span");
    place.className = "popup-size popup-place";
    place.textContent = at;
    place.title = at;
    named.append(place);
  }
  head.append(named);

  // The visible way out, beside the two ways that are not. A dialog with only "Keep"
  // and "Send" makes dismissing it look like a choice somebody has to make.
  const shut = document.createElement("button");
  shut.type = "button";
  shut.className = "popup-shut";
  shut.title = "Discard this mark · Esc";
  shut.setAttribute("aria-label", "Discard this mark");
  shut.textContent = "\u00d7";
  shut.addEventListener("click", () => cancelMark(mark.id));
  head.append(shut);
  rows.push(head);

  const note = document.createElement("textarea");
  note.className = "popup-note";
  // Named so a redraw can find it again and put the cursor back. Named apart from the
  // composer's note for the same mark, because both can be on screen at once and a
  // cursor restored into the wrong one of the two is its own bug.
  note.dataset.field = `popup-note:${mark.id}`;
  note.rows = 2;
  /*
   * And it says so, because this is where somebody is standing when they first want them.
   *
   * This box is the one that opens by itself, on the thing they just pointed at, and it is
   * the first field most people ever type into — so a placeholder that named neither
   * keystroke left `/` and `@` to be discovered in a panel they had not opened yet.
   */
  note.placeholder = "What about it? / for a command, @ for a file";
  note.value = mark.note || "";
  note.addEventListener("input", () => {
    mark.note = note.value;
  });
  /*
   * The same `/` and `@` as the composer, on the popup's own field.
   *
   * They were the composer's alone, which made the toolbar's own headline act — point at
   * something, say what you want — the one place the keystrokes did not work. A mark's
   * popup and the composer are the same conversation at two sizes and already share the
   * mode and the files; the menu is the third thing they should never have disagreed about.
   *
   * Wrapped in its own `.ask-box` rather than reaching for a menu somewhere else on the
   * page: the box is what the menu is positioned against, so each field carries the one it
   * opens and two of them cannot end up drawing into the same element.
   */
  const noteBox = document.createElement("div");
  noteBox.className = "ask-box";
  const noteMenu = document.createElement("div");
  noteMenu.className = "ask-menu";
  noteMenu.hidden = true;
  noteBox.append(note, noteMenu);
  completes(note, noteMenu, composing(askingOn(mark), (said) => (mark.note = said)));
  rows.push(noteBox);

  if (mark.tool === "design") {
    // Which of the three this is. Chips rather than a menu: they are three ways of
    // reading the same picture, and seeing them side by side is what tells somebody
    // that a wireframe and a component are different questions.
    const kinds = document.createElement("div");
    kinds.className = "mode-row design-row";
    for (const [id, kind] of Object.entries(DESIGNS)) {
      const chip = document.createElement("button");
      chip.type = "button";
      chip.className = "chip";
      chip.setAttribute("aria-pressed", String((mark.design || DESIGN_FIRST) === id));
      chip.textContent = kind.chip || kind.label;
      chip.addEventListener("click", () => {
        mark.design = id;
        // The home moves with the kind, unless somebody has typed over it. A design
        // system left pointing at the path a wireframe suggested is the sort of wrong
        // that only shows up in a pull request.
        if (!mark.destTyped) mark.dest = kind.home || "";
        render();
      });
      kinds.append(chip);
    }
    rows.push(kinds);

    const kind = kindOf(mark);
    const line = document.createElement("div");
    line.className = "popup-dest-line";
    const where = document.createElement("input");
    where.className = "popup-note popup-dest";
    where.dataset.field = `dest:${mark.id}`;
    where.type = "text";
    where.value = mark.dest || "";
    // A component has no home to suggest, because only the repository knows where its
    // own components go. Empty is the honest answer, and the placeholder says so
    // rather than leaving a blank box that looks unfinished.
    where.placeholder = kind.home || "wherever this project keeps them";
    where.setAttribute("aria-label", `Where the ${kind.label.toLowerCase()} goes`);
    where.addEventListener("input", () => {
      mark.dest = where.value;
      mark.destTyped = true;
    });
    line.append(where);

    // Beside the field rather than instead of it. A destination can be a folder that
    // does not exist yet, or one in a checkout that is not on this machine at all, and
    // both are things somebody types. The chooser is for the ordinary case — this
    // project, a folder they would otherwise recall from memory and mistype.
    const browse = document.createElement("button");
    browse.type = "button";
    browse.className = "popup-browse";
    browse.title = "Choose a folder";
    browse.setAttribute("aria-label", "Choose a folder");
    browse.innerHTML = icon("folder", 14);
    browse.addEventListener("click", () => void chooseHome(mark));
    line.append(browse);
    rows.push(line);
  }

  const foot = document.createElement("div");
  foot.className = "popup-foot";
  const to = document.createElement("button");
  to.type = "button";
  to.className = "popup-to";
  /*
   * The name in a span of its own, which is what lets it be shortened.
   *
   * It was the button's own text, and a conversation is named after whatever somebody first
   * said in it — so a receiver called `claude-code-plugin-integration` is one unbreakable
   * word. A flex item will not shrink below the width of its longest word however small its
   * `min-width` is, and text that is not in an element of its own cannot be given an
   * ellipsis, so this row grew past the card and pushed Send out through the right-hand
   * edge of the popup. The name is the only part of the row that can afford to lose
   * characters; the two buttons are the point of the row.
   */
  const whom = document.createElement("span");
  whom.className = "popup-to-name";
  whom.textContent = state.receiving.name || "Choose who receives";
  to.append(whom);
  to.title = "Change who receives this";
  to.addEventListener("click", () => {
    state.popup = null;
    flyout("chat");
  });
  const keep = document.createElement("button");
  keep.type = "button";
  keep.className = "popup-do";
  keep.textContent = "Keep";
  keep.addEventListener("click", () => {
    state.popup = null;
    render();
  });
  const now = document.createElement("button");
  now.type = "button";
  now.className = "popup-do popup-go";
  now.disabled = state.sending;
  now.textContent = state.sending ? "Sending…" : "Send now";
  now.addEventListener("click", () => void sendMarks([mark.id]));
  foot.append(to, keep, now);
  rows.push(foot);

  el.popup.replaceChildren(...rows);
  placePopup(mark);
}

/**
 * Put the popup beside the mark it is about, and inside the screen.
 *
 * Beside rather than on top: covering the thing somebody just pointed at, while asking
 * them what they meant by it, is the one place this must not open.
 */
function placePopup(mark) {
  // A whole-display capture has no corner to sit beside, so it opens in the middle
  // rather than at Math.max of nothing, which is negative infinity and the top-left.
  const edges = mark.region
    ? {
        right: (mark.region.box.x + mark.region.box.w) * window.innerWidth,
        bottom: (mark.region.box.y + mark.region.box.h) * window.innerHeight,
      }
    : mark.points.length
      ? {
          right: Math.max(...mark.points.map((spot) => spot.x)) * window.innerWidth,
          bottom: Math.max(...mark.points.map((spot) => spot.y)) * window.innerHeight,
        }
      : { right: window.innerWidth / 2, bottom: window.innerHeight / 2 };
  el.popup.style.left = "0px";
  el.popup.style.top = "0px";
  const box = el.popup.getBoundingClientRect();
  // On the screen the mark is on: a popup for something marked on the second display
  // belongs there, not pinned inside the first one's edges.
  const room = usable(screenAt(state.screens, { x: edges.right, y: edges.bottom }));
  const left = Math.min(
    Math.max(edges.right + 14, room.left + EDGE),
    room.right - box.width - EDGE,
  );
  const top = Math.min(
    Math.max(edges.bottom + 14, room.top + EDGE),
    room.bottom - box.height - EDGE,
  );
  el.popup.style.left = `${Math.round(left)}px`;
  el.popup.style.top = `${Math.round(top)}px`;
}

/** The composer: everything marked so far, and what to do with the ticked ones. */
/**
 * The files and folders coming along, and the two ways to add one.
 *
 * Under the note rather than above it, because what somebody types is the point and a
 * list of attachments that pushes it off the menu is a file manager with a text box in
 * it. Each one says whether it is travelling or only being named — the difference is
 * the difference between an agent that can see the thing and one that has to go and
 * open it, and finding that out after sending is finding it out too late.
 */
/**
 * How the ask should be taken, as one control rather than four chips.
 *
 * Four chips spent a row of the composer saying three things nobody had chosen. The
 * mode is one decision with one answer, which is a dropdown — and the same decision is
 * reachable with `/` in the field, for anyone who would rather not leave the keyboard.
 */
function modePick() {
  const box = document.createElement("div");
  box.className = "mode-pick";

  const now = MODES[state.mode] || MODES.plan;
  const open = document.createElement("button");
  open.type = "button";
  open.className = "mode-key";
  open.setAttribute("aria-haspopup", "true");
  open.setAttribute("aria-expanded", "false");
  open.title = now.says;
  const named = document.createElement("span");
  named.textContent = now.label;
  const mark = document.createElement("span");
  mark.className = "caret";
  mark.textContent = "▾";
  open.append(named, mark);

  // The same menu `/` opens in the field, because it is the same choice — one look for
  // it, whichever way somebody reaches it. It was the platform's own `<select>`: a grey
  // slab that ignored every token on the page and could not show what a mode does.
  const menu = document.createElement("div");
  menu.className = "ask-menu mode-menu";
  menu.hidden = true;
  for (const [id, mode] of Object.entries(MODES)) {
    const one = document.createElement("button");
    one.type = "button";
    one.className = "ask-menu-row";
    one.dataset.on = String(id === state.mode);
    const name = document.createElement("span");
    name.className = "ask-menu-name";
    name.textContent = mode.label;
    const says = document.createElement("span");
    says.className = "ask-menu-says";
    says.textContent = mode.says;
    one.title = mode.says;
    one.append(name, says);
    one.addEventListener("click", () => {
      state.mode = id;
      render();
    });
    menu.append(one);
  }

  const shut = () => {
    menu.hidden = true;
    open.setAttribute("aria-expanded", "false");
  };
  open.addEventListener("click", () => {
    menu.hidden = !menu.hidden;
    open.setAttribute("aria-expanded", String(!menu.hidden));
  });
  // A menu that only closes by choosing something is a menu somebody is stuck in.
  menu.addEventListener("focusout", (event) => {
    if (!box.contains(event.relatedTarget)) shut();
  });
  open.addEventListener("keydown", (event) => {
    if (event.key === "Escape") shut();
  });

  box.append(open, menu);
  return box;
}

/**
 * Send.
 *
 * Its own function because the field has to be handed it before the foot is built, and
 * because what it says depends on what is going — which is a decision, not a label.
 */
function sendButton() {
  const going = chosenMarks();
  const says = state.sending
    ? "Sending…"
    : going.length
      ? `Send ${counted(going.length, "mark")}`
      : "Send";

  const go = document.createElement("button");
  go.type = "button";
  // The arrow, always: what is going is already listed directly above it. It spelled the
  // words out instead while the receiver could be a conversation held in another agent,
  // because sending there adopted it and an arrow cannot say that. Nothing is adoptable.
  go.className = "popup-do popup-go compose-send";
  go.textContent = state.sending ? "…" : "↑";
  go.title = says;
  go.setAttribute("aria-label", says);
  go.disabled = !canSend(going);
  go.addEventListener("click", () => void sendMarks(chosenMarks().map((mark) => mark.id)));
  return go;
}

/** Whether there is anything to send, asked in one place so the key and the arrow agree. */
function canSend(going) {
  if (state.sending) return false;
  return going.length > 0 || state.files.length > 0 || Boolean(state.text.trim());
}

/**
 * The ask, and the menu that opens inside it.
 *
 * `/` at the start of a word offers Claude Code's commands, and colai's own modes until
 * there are any. Picking a mode sets it and takes the word back out, because the mode is
 * how the ask should be read and not part of the ask; picking a command leaves it where it
 * was typed, because a command is the agent's to read. `completes` holds all of that — this
 * field is one of four with the same menu in it — and what is left here is the two things
 * only the composer does: the words are `state.text`, and Ctrl+Enter sends them.
 */
function askField(go) {
  const box = document.createElement("div");
  box.className = "ask-box";

  const text = document.createElement("textarea");
  text.className = "popup-note";
  // Named so a redraw can find it again and put the cursor back. See `whatIsBeingTyped`.
  text.dataset.field = "ask";
  text.rows = 2;
  // The two keystrokes live here now rather than in a labelled row above the field.
  // `/` has a control beside it to be found from; `@` has nothing anywhere else, so if
  // this line does not name it nobody ever finds it. It is the largest empty space in
  // the composer and it is exactly where somebody is about to type.
  const marked = chosenMarks().length;
  // A commit's field is not an ask, it is the message — passed through to git exactly as
  // typed. Saying "say what you want done" over a box whose contents become a permanent
  // line in somebody's history is the field lying about what it is for.
  const marks = chosenMarks();
  text.placeholder = isCommitting(marks)
    ? "The commit message…"
    : marked
      ? `Say what you want done with ${counted(marked, "mark")}… / for a command, @ for a file`
      : "Say what you want done… / for a command, @ for a file";
  text.value = state.text;

  const menu = document.createElement("div");
  menu.className = "ask-menu";
  menu.hidden = true;

  /*
   * Which mark is open, if either. `/` answers from a table and `@` answers from disk,
   * but where the menu goes and how it is driven is the same question both times.
   *
   * Held in `state` rather than in this closure, because this whole field is rebuilt by
   * every render — and a render happens on a five-second refresh, on any reply arriving,
   * and whenever a window moves under the toolbar. So an open `/` or `@` list closed
   * itself, mid-choice, because something unrelated happened somewhere else. Reading a
   * list of files from disk and then throwing it away before the person could pick one
   * is the same class of bug as the caret this field already lost once.
   */
  const asking = state.ask;

  text.addEventListener("input", () => {
    state.text = text.value;
    // Typing does not redraw the composer — a render on every keystroke would rebuild
    // the field and take the caret with it — so the button draw produced would still be
    // refusing after the first word. With nothing marked, that button is the only way
    // out of the composer, and it was dead: a whole sentence typed, and nothing to
    // press. The words are the composer's own subject; marks are extra.
    go.disabled = !canSend(chosenMarks());
  });
  /*
   * Attached before `completes`, so that on Enter this listener is the one that runs first
   * and can see the menu still open. The other way round, the menu would have taken the
   * highlighted row and closed itself, and this would then read a hidden menu and send the
   * message — one keystroke doing both jobs.
   */
  text.addEventListener("keydown", (event) => {
    // Send from the keyboard, unless the `/` or `@` menu is open — there Enter is
    // already answering a question, and stealing it would send whatever half-typed
    // word the menu was offering to complete.
    if (menu.hidden && event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      const going = chosenMarks();
      if (canSend(going)) void sendMarks(going.map((one) => one.id));
    }
  });
  completes(text, menu, composing(asking, (said) => (state.text = said)));

  box.append(text, menu);
  return box;
}

/* ── `/` and `@`, in every box there is to type in ────────────────────────────
 *
 * Four fields on this page ask somebody for words: the composer, the note on a mark's
 * popup, the reply to an agent's question, and the reply beside an answer in the Work
 * panel. The menu was built inside the first of them, so the two keystrokes anybody who
 * uses Claude Code already knows worked in one box and did nothing in the other three —
 * which is worse than not having them at all. A keystroke is learned about the toolbar,
 * not about one panel of it, and one that silently does nothing reads as broken rather
 * than absent. Marking something on screen and saying what you want about it is the
 * whole of what colai is for, and that box was the one without the menu.
 *
 * So the machinery lives here once and is handed a field, a menu to draw into, and `how`
 * — which is the only part that differs. What differs is never how the menu behaves but
 * what a chosen row *means*, and that is a fact about the surface: the composer and a
 * mark's popup are assembling a message colai composes, so a mode is a mode and a file is
 * an attachment; the two reply boxes hand their words to a conversation exactly as typed,
 * so the only thing that can carry a command out of them is the words themselves.
 */

/**
 * What picking a row does where colai is composing the message — the composer, and the
 * popup on a mark.
 *
 * Those two share a receiver, a mode and the files coming along (see the top of this
 * file), so a mode chosen in either sets it for both and a file named in either travels
 * with the send. `keep` is where that field's words live, which is the one thing even
 * these two do not share: one is `state.text` and the other is the note on a mark.
 */
function composing(asking, keep) {
  return {
    asking,
    keep,
    took: (mark, chosen) => {
      if (mark === "@") {
        // Described rather than assumed. Whether a file travels with the message or is
        // only named depends on how big it is, and a size invented here as zero would
        // make everything look small enough to carry. `bringFiles` is the one door every
        // file comes through, so a path chosen with `@` lands the same way a dropped one
        // does.
        void invoke("colai_describe_files", { paths: [chosen.path] })
          .then((described) => bringFiles(described))
          .catch(() => {});
        return null;
      }
      /*
       * A Claude Code command is not colai's to set, so it stays in the words.
       *
       * It used to be written into `state.mode`, which holds one of four modes — so
       * picking `/review` left the mode unreadable, `summaryFor` read it back as Plan,
       * and the command somebody had deliberately chosen was never sent at all. The words
       * are the part of the message the agent reads as its own, which is where a command
       * belongs.
       */
      if (chosen.command) return chosen.label;
      state.mode = chosen.id;
      return null;
    },
  };
}

/**
 * And what it does where the words *are* the message — the two reply boxes.
 *
 * A reply goes to the conversation exactly as typed (see `verdict`), so Claude Code is the
 * thing that reads it: `/review` has to still be in the box when it arrives, and so does
 * `@src/thing.ts`. Nothing here sets a mode, because a reply is not a message colai
 * composes — and when there is no command list yet and the modes stand in, the mode's own
 * sentence is what goes in, since saying it is the only way to ask for it from here.
 */
function replying(asking, keep) {
  return {
    asking,
    keep,
    took: (mark, chosen) =>
      mark === "@" ? chosen.path : chosen.command ? chosen.label : chosen.says,
  };
}

/** One field's menu state, kept on the thing that field is about and made on first use. */
function askingOn(holder) {
  /*
   * On the mark or the answer rather than in one place keyed by name, because that is the
   * thing which outlives the field: every one of these boxes is rebuilt by its panel's
   * next render, and a mark and an answer both survive that. One holder each, so two boxes
   * open at once — a popup over the Work panel — cannot share a highlighted row, which
   * would be a choice landing in whichever of them was not being looked at.
   */
  if (!holder.asking) holder.asking = { mark: null, showing: [], picked: 0 };
  return holder.asking;
}

/**
 * How tall the menu can get, which is the stylesheet's `max-height` restated.
 *
 * Only used to decide which way it opens, so being a few pixels out costs nothing — but
 * the two numbers mean the same thing and are worth keeping side by side.
 */
const ASK_MENU_TALL = 190;

/**
 * Give one field the `/` and `@` menu, drawn into one menu element beside it.
 *
 * `how` is the surface's three answers: `asking`, the state this menu keeps between
 * renders; `keep`, where this field's words are held; and `took`, what picking a row means
 * — see `composing` and `replying`, which are the only two answers there are.
 */
function completes(field, menu, how) {
  const asking = how.asking;
  /*
   * `asked` stays local: it is a sequence number for in-flight lookups, and a rebuilt
   * field has no in-flight lookups of its own to disambiguate.
   */
  let asked = 0;

  const close = () => {
    asking.mark = null;
    asking.showing = [];
    asking.picked = 0;
    menu.hidden = true;
    menu.replaceChildren();
  };

  const take = (chosen) => {
    if (!chosen) return close();
    const token = tokenAt(field.value, field.selectionStart, asking.mark);
    if (!token) return close();
    // What this surface does with the row, and what it leaves behind in the box: `took`
    // answers with the words to complete the token into, or null when the choice was not
    // words at all — a mode colai will read, or a file it will carry.
    const typed = how.took(asking.mark, chosen);
    const left =
      typed === null ? withoutToken(field.value, token) : insteadOf(field.value, token, typed);
    how.keep(left.text);
    field.value = left.text;
    // Set here rather than after the redraw, because the redraw is what reads it: the
    // caret is noted off whatever holds it, and this box holds it until `render` runs.
    // It used to be followed by a `text.focus()`, which by then was addressed to a box
    // that had already been replaced and so put the cursor precisely nowhere.
    field.setSelectionRange(left.caret, left.caret);
    close();
    render();
  };

  const draw = () => {
    asking.picked = Math.min(asking.picked, Math.max(0, asking.showing.length - 1));
    menu.hidden = asking.showing.length === 0;
    /*
     * Which way it opens, measured rather than assumed.
     *
     * The menu hangs off the field it belongs to, and the four fields sit in four
     * different places: the composer's is at the bottom of a panel with the whole panel
     * above it, a mark's popup can open anywhere on the screen with its note near the
     * top of a short card, and a reply in the Work panel lives inside a list that
     * scrolls — where anything drawn past the top of the list is clipped by it. Upward is
     * right for the first and wrong for the others, so the room above is measured: within
     * the scrolling box when there is one, and within the window when there is not.
     */
    const above = field.getBoundingClientRect().top;
    const holder = field.closest(".scrolls");
    const ceiling = holder ? holder.getBoundingClientRect().top : 0;
    menu.dataset.under = String(above - ceiling < ASK_MENU_TALL);
    menu.replaceChildren(
      /*
       * `asking.showing`, not a bare `showing`.
       *
       * These scripts share one global scope, and `toolbar-mark.js` declares a function of
       * that name — so the bare word resolved to it, and a function has no `.map`. Every
       * other line in this closure already says `asking.showing`; this one did not, and had
       * not since the first commit, because nothing ever typed `/` into the box.
       */
      ...asking.showing.map((row, at) => {
        const one = document.createElement("button");
        one.type = "button";
        one.className = "ask-menu-row";
        one.dataset.on = String(at === asking.picked);
        const name = document.createElement("span");
        name.className = "ask-menu-name";
        name.textContent = row.label ?? row.shown;
        const says = document.createElement("span");
        says.className = "ask-menu-says";
        says.textContent = row.says ?? row.path;
        // Clipped to one line so several fit; the whole of it stays reachable.
        one.title = row.says ?? row.path;
        one.append(name, says);
        // Pressed rather than clicked: a click would blur the field first and close the
        // menu out from under the press.
        one.addEventListener("mousedown", (event) => {
          event.preventDefault();
          take(row);
        });
        return one;
      }),
    );
  };

  const look = () => {
    const slash = tokenAt(field.value, field.selectionStart, "/");
    const at = tokenAt(field.value, field.selectionStart, "@");
    // Whichever was typed later is the one being typed now.
    const token = !slash ? at : !at ? slash : slash.from > at.from ? slash : at;
    if (!token) return close();
    asking.mark = token === slash ? "/" : "@";

    if (asking.mark === "/") {
      /*
       * Claude Code's commands when they are known, colai's modes until then.
       *
       * The real list comes on the init frame at the start of every turn, so before a
       * conversation has begun there is nothing to offer — and a `/` that answers with
       * nothing is worse than one that answers with what this toolbar can still do.
       */
      const commands = commandsMatching(token.word, state.commands, state.terminalOnly);
      asking.showing = commands.length > 0 ? commands : modesMatching(token.word);
      if (asking.showing.length === 0) return close();
      return draw();
    }
    // Asked of the machine, so the answer arrives after the keystroke that wanted it.
    // Each ask is numbered and a late one is dropped: without that, a slow search for
    // `sr` lands after a fast one for `src` and replaces the right answer with a stale
    // one — the menu flickering backwards as somebody types.
    const mine = ++asked;
    void invoke("colai_search_files", { query: token.word })
      .then((rows) => {
        if (mine !== asked || asking.mark !== "@") return;
        asking.showing = rows || [];
        if (asking.showing.length === 0) return close();
        draw();
      })
      .catch(() => close());
  };

  field.addEventListener("input", look);
  field.addEventListener("click", look);
  field.addEventListener("blur", close);
  field.addEventListener("keydown", (event) => {
    if (menu.hidden) return;
    if (event.key === "Escape") {
      event.preventDefault();
      // And no further. This listener is on the field, so the event goes on to reach the
      // window — where Escape closes the work panel a composer lives in, and throws away
      // the mark a popup is about. Shutting a suggestion list would have shut the whole
      // panel around it and taken what was being typed with it.
      event.stopPropagation();
      return close();
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      asking.picked =
        (asking.picked + (event.key === "ArrowDown" ? 1 : asking.showing.length - 1)) %
        asking.showing.length;
      return look();
    }
    // Enter takes the highlighted one. Tab too, because a menu that only answers to one
    // key is a menu half the people using it never get out of.
    if (event.key === "Enter" || event.key === "Tab") {
      event.preventDefault();
      take(asking.showing[asking.picked]);
    }
  });

  // Put back on screen, not merely remembered. The state above survives the rebuild; the
  // element does not, so a list that was open has to be drawn again or hoisting it would
  // only have moved where the disappearance happens.
  if (asking.showing.length > 0) draw();
}

function fileRows() {
  const rows = [];
  for (const file of carrying(state.files)) {
    const row = document.createElement("div");
    row.className = "row file-row";
    row.dataset.carried = String(file.carried);
    const said = document.createElement("span");
    said.className = "agent-name";
    said.textContent = file.name;
    said.title = file.path;
    // One label, not two. A size on the left and "named · 41 MB" on the right is the
    // same fact twice, and the half worth reading first is what is going to happen to
    // it — so the fate leads and the reason follows it.
    const how = document.createElement("span");
    how.className = "row-key";
    how.textContent = file.carried
      ? `attached · ${sizeOf(file.bytes)}`
      : file.why === "no room left"
        ? `named · no room left, ${sizeOf(file.bytes)}`
        : `named · ${file.why}`;
    const drop = document.createElement("button");
    drop.type = "button";
    drop.className = "file-drop";
    drop.title = `Leave ${file.name} out`;
    drop.setAttribute("aria-label", `Leave ${file.name} out`);
    drop.textContent = "✕";
    drop.addEventListener("click", () => {
      state.files = state.files.filter((had) => had.path !== file.path);
      render();
    });
    row.append(said, how, drop);
    rows.push(row);
  }

  return rows;
}

/**
 * Adding a file.
 *
 * Two chips and a sentence used to own a line of a composer that has only a few, so this
 * was folded into the send row — where, with the receiver and the mode beside it, there
 * was no longer room for a name: "Claude Code" came out as "Claude …". It shares a quiet
 * line with Schedule now. Files still arrive by dropping them anywhere on the panel and
 * by typing `@`; this is the third way, and the rarest, so it is the smallest.
 */
function fileAdd() {
  const add = document.createElement("button");
  add.type = "button";
  add.className = "compose-later";
  add.textContent = "Files…";
  add.title = "Choose files, or drop them anywhere on this panel";
  add.addEventListener("click", (event) => void pickFiles(event.altKey));
  return add;
}

/** Ask the desktop for files, and keep whatever comes back that is not already here. */
async function pickFiles(folders) {
  try {
    bringFiles(await invoke("colai_pick_files", { folders }));
  } catch (error) {
    state.trouble = `Could not open the file chooser — ${error && error.message ? error.message : String(error)}`;
    render();
  }
}

/**
 * Take paths into the send, without taking any of them twice.
 *
 * By path, because the same file dropped twice is the same file, and a list that shows
 * it twice would also encode it twice into the message.
 */
function bringFiles(brought) {
  if (!brought) return;
  const chosen = brought.chosen || [];
  // What could not be described, named. Three files dropped and two appearing is a
  // toolbar that lost one without saying so, and the one it lost is the one somebody
  // most wants to ask about — a broken link, an unreadable mount.
  const refused = brought.refused || [];
  if (refused.length) {
    state.trouble = `Could not read ${refused.join(", ")} — ${
      refused.length === 1 ? "it was" : "they were"
    } left out.`;
  }
  if (!chosen.length) {
    if (refused.length) render();
    return;
  }
  const had = new Set(state.files.map((file) => file.path));
  state.files = [...state.files, ...chosen.filter((file) => !had.has(file.path))];
  // Opened, because a file dropped onto a closed toolbar has nowhere visible to land,
  // and something that vanishes on arrival reads as a drop that failed.
  //
  // It said `state.open = "send"`, and there is no "send" flyout — the panels are shape,
  // design, git, how, automate, row, points, draw, record and agents. So the comment
  // above described exactly what did not happen: the file was accepted, nothing opened,
  // and the drop looked like it had failed. The composer lives in the work panel, which
  // is where the file now actually appears.
  openWork();
}

/** One line of prose in a menu that has nothing else to show. */
function saying(words) {
  const line = document.createElement("p");
  line.className = "chat-empty";
  line.textContent = words;
  return line;
}
function drawComposer(into) {
  const rows = [];
  // Made first, though it is drawn last: the field below has to keep it in step, and a
  // button that does not exist yet cannot be kept in step with anything.
  const go = sendButton();
  // No section heading and no "nothing marked yet": the panel is already called Work,
  // and the field's own placeholder says what to do with an empty composer. Two labels
  // for one thing is how a compose box grows to eight rows of chrome.
  for (const mark of state.marks) {
    const row = document.createElement("div");
    row.className = "row mark-row";

    // The tick, the picture and the name toggle together, because they are all the same
    // question — is this one going? The note beside them is not, which is why it sits
    // outside the label rather than inside it: a click meant for the words somebody is
    // about to change must not untick the mark they are changing them on.
    const pick = document.createElement("label");
    pick.className = "mark-pick";
    const tick = document.createElement("input");
    tick.type = "checkbox";
    tick.className = "mark-tick";
    tick.checked = Boolean(mark.chosen);
    tick.addEventListener("change", () => {
      mark.chosen = tick.checked;
      render();
    });
    const shot = document.createElement("span");
    shot.className = "mark-shot";
    if (mark.thumb) {
      const picture = document.createElement("img");
      picture.src = mark.thumb;
      picture.alt = "";
      shot.append(picture);
    }
    // Numbered the way the glass numbers it and the way the message will, so all three
    // agree about which one is being talked about.
    const number = document.createElement("span");
    number.className = "mark-number";
    const called = numberOf(state.marks, mark);
    number.textContent = called === null ? "" : String(called);
    const said = document.createElement("span");
    said.className = "agent-name";
    // Named the way the message will name it, so what somebody ticks in the tray and
    // what the agent reads are the same word.
    said.textContent = labelOf(mark);
    said.title = said.textContent;
    pick.append(tick, shot, number, said);

    // Its note, here as well as in the popup. Marks travel in groups now, and the popup
    // reaches whichever one is newest — going back to change what you wrote on the first
    // of four meant discarding three and starting again.
    const note = document.createElement("input");
    note.type = "text";
    note.className = "mark-note";
    note.dataset.field = `note:${mark.id}`;
    note.value = mark.note || "";
    note.placeholder = "What about it?";
    note.setAttribute("aria-label", `What about ${said.textContent}`);
    note.addEventListener("input", () => {
      mark.note = note.value;
    });

    row.append(pick, note);
    rows.push(row);
  }

  rows.push(askField(go));

  rows.push(...fileRows());

  const foot = document.createElement("div");
  foot.className = "popup-foot";
  const to = document.createElement("button");
  to.type = "button";
  to.className = "popup-to";
  // A dot, a name and a chevron: who this is going to, whether they are up, and that
  // the name can be changed. The name alone read as a caption nobody could click.
  const lit = document.createElement("span");
  lit.className = "popup-to-lit";
  lit.dataset.up = String(Boolean(state.receiving.name));
  const named = document.createElement("span");
  named.className = "popup-to-name";
  named.textContent = state.receiving.name || "Choose who receives";
  const stack = document.createElement("span");
  stack.className = "popup-to-stack";
  stack.append(named);
  const mark = document.createElement("span");
  mark.className = "caret";
  mark.textContent = "▾";
  to.append(lit, stack, mark);
  to.title = "Choose who receives this, and how they answer";
  to.addEventListener("click", () => flyout("chat"));
  // The keystroke, said once beside the key it belongs to. A send key nobody is told
  // about is a send key nobody uses.
  const key = document.createElement("span");
  key.className = "compose-key";
  key.textContent = "Ctrl ↵";
  key.title = "Ctrl+Enter sends";

  /*
   * And the two keystrokes inside the field, which had nowhere permanent to be said.
   *
   * They were named in the placeholder — the one piece of text guaranteed to be gone by
   * the time anybody could use them, because it disappears on the first character typed.
   * So the two things that make this field more than a text box were advertised only to
   * people who had not started using it yet.
   *
   * Buttons rather than labels: somebody who has just learned that `/` exists should be
   * able to press the thing that told them so.
   */
  const inField = document.createElement("span");
  inField.className = "compose-keys";
  for (const [mark, what] of [
    ["/", "Choose how this is read — plan, review, commit…"],
    ["@", "Bring in a file by name"],
  ]) {
    const chip = document.createElement("button");
    chip.type = "button";
    chip.className = "compose-key compose-key-do";
    chip.textContent = mark;
    chip.title = what;
    chip.setAttribute("aria-label", what);
    chip.addEventListener("click", () => {
      // Typed into the field rather than acted on here, so one piece of code decides what
      // these mean: the field's own key handler, which already knows.
      const field = document.querySelector('[data-field="ask"]');
      if (!field) return;
      const at = field.selectionStart ?? field.value.length;
      // On a word boundary, because that is the only place the menus open. Appended to
      // the end of a word it would insert a character and do nothing else.
      const before = field.value.slice(0, at);
      const spacer = before.length === 0 || /\s$/.test(before) ? "" : " ";
      field.value = `${before}${spacer}${mark}${field.value.slice(at)}`;
      state.text = field.value;
      field.focus({ preventScroll: true });
      const now = at + spacer.length + 1;
      field.setSelectionRange(now, now);
      field.dispatchEvent(new Event("input", { bubbles: true }));
    });
    inField.append(chip);
  }

  // Bringing a file in, on a line of its own. It shared this row with "Schedule…" until
  // that button was removed — it opened a dialog whose Create always failed, because there
  // is no scheduler on this host to create anything with.
  const extras = document.createElement("div");
  extras.className = "compose-more";
  extras.append(fileAdd());
  rows.push(extras);

  // Who and how on the left, what happens to it on the right. `to` takes whatever room
  // the rest leaves, so only a genuinely long agent name shortens.
  const gap = document.createElement("span");
  gap.className = "compose-gap";
  // Who, how it should be taken, and which model takes it — the three facts about the
  // answer, in the order they were asked for. Then what happens to it, on the right.
  foot.append(to, modePick(), gap, inField, key, go);
  rows.push(foot);

  into.replaceChildren(...rows);
}

/**
 * Agents and conversations, as rows somebody picks from.
 *
 * Three states and they are genuinely different: could not ask, nothing there, and a
 * list. Collapsing the first two into "nobody yet" would blame the person for a Gateway
 * that is not answering.
 *
 * Two headed groups rather than one flat list, because picking an agent and picking a
 * conversation are different choices — one starts something, the other joins it.
 */
function drawWho() {
  if (state.whoTrouble) {
    const said = document.createElement("p");
    said.className = "chat-empty";
    said.textContent = `Could not reach the Gateway — ${state.whoTrouble}`;
    el.chatRows.replaceChildren(said);
    return;
  }
  if (talking() === 0) {
    const empty = document.createElement("p");
    empty.className = "chat-empty";
    empty.textContent =
      "Nothing yet. Start a conversation with Claude Code and it appears here.";
    el.chatRows.replaceChildren(empty);
    return;
  }

  /*
   * One list, and no heading over it.
   *
   * There were three: an "Agents" group, the conversations, and a tree of projects with
   * the threads each held. The agent list came back empty on this host — one Claude means
   * one entry, which is not a choice — and the project tree was OpenClaw's second axis,
   * conversations belonging to somebody else's checkout. Claude Code's own conversations
   * already carry the directory they were had in, which `colai_sessions` puts under each
   * name, so the tree would have been this same list drawn twice.
   */
  const rows = state.sessions.map((session) =>
    whoRow({
      face: session.title.slice(0, 1).toUpperCase(),
      name: session.title,
      receiving: state.receiving.id === session.key,
      onPick: () => receive(session.key, session.title),
      about: {
        id: session.key,
        name: session.title,
        sessionKey: session.key,
      },
    }),
  );
  el.chatRows.replaceChildren(...rows);
}

function whoRow({ face, name, receiving, onPick, about }) {
  const row = document.createElement("button");
  row.type = "button";
  row.className = "row";
  row.setAttribute("role", "menuitem");
  row.setAttribute("aria-pressed", String(receiving));

  const avatar = document.createElement("span");
  avatar.className = "chat-avatar-dot";
  avatar.textContent = face;

  const label = document.createElement("span");
  label.className = "agent-name";
  label.textContent = name;
  label.title = name;

  row.append(avatar, label);
  // Marked when it is the one receiving. It used to lose that mark to a "Running" or
  // "Unread" note off the row itself, and those came back hardcoded false — one
  // conversation is live at a time and the toolbar is the thing having it.
  if (receiving) {
    const tail = document.createElement("span");
    tail.className = "row-key";
    tail.textContent = "Receiving";
    row.append(tail);
  }
  row.addEventListener("click", onPick);
  /*
   * No menu on a row any more.
   *
   * It held one item — Rewind — and rewind is Claude Code's own `/rewind`, in the session
   * the person is looking at. A second way to do it from a toolbar would be a second idea
   * of where a conversation currently is, and the two would disagree the first time
   * somebody used both. With nothing left on it, the dots and the right click opened an
   * empty menu.
   */
  return row;
}
