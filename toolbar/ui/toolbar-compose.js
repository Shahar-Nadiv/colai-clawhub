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
  note.placeholder = "What about it?";
  note.value = mark.note || "";
  note.addEventListener("input", () => {
    mark.note = note.value;
  });
  rows.push(note);

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

    // And where its content comes from, for the two kinds that can be brought in rather
    // than copied. A second row rather than more kinds: "component, copied" and
    // "component, from a catalogue" are one thing with two sources, and making them two
    // chips in the first row would say they were two different things to build.
    if (TAKES_SOURCE.includes(kindIdOf(mark))) {
      const sources = document.createElement("div");
      sources.className = "mode-row source-row";
      for (const [id, source] of Object.entries(SOURCES)) {
        const chip = document.createElement("button");
        chip.type = "button";
        chip.className = "chip";
        chip.setAttribute("aria-pressed", String(sourceOf(mark) === id));
        chip.textContent = source.label;
        chip.addEventListener("click", () => {
          mark.source = id;
          // Opening the library is the whole point of choosing it, so choosing it opens
          // the library. Pressing it again with something already chosen goes back to
          // swap it, which is the only other thing anybody wants from that chip.
          if (id === "library") openLibrary(mark);
          else render();
        });
        sources.append(chip);
      }
      rows.push(sources);
    }

    // What was chosen, once something has been. Named on the mark itself, because the
    // library window closes and the popup is then the only place that could say whether
    // this mark is finished or still half-asked.
    const chosen = broughtIn(mark);
    if (sourceOf(mark) === "library") {
      const said = document.createElement("button");
      said.type = "button";
      said.className = chosen ? "row row-quiet chosen-row" : "row row-quiet chosen-row chosen-none";
      said.textContent = chosen
        ? `${chosen.name} — from ${chosen.library}`
        : "Choose one from the library…";
      said.addEventListener("click", () => openLibrary(mark));
      rows.push(said);
    }

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
  to.textContent = state.receiving.name || "Choose who receives";
  to.title = "Change who receives this";
  to.addEventListener("click", () => {
    state.popup = null;
    flyout("agents");
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
  now.textContent = state.sending ? "Sending…" : needsAgreeing() ? "Send and adopt" : "Send now";
  now.addEventListener("click", () => void sendMarks([mark.id]));
  foot.append(to, keep, now);
  rows.push(foot);

  // Said before it happens, not after. Continuing a conversation held in another agent
  // hands it to the Gateway, and somebody driving that thread from a terminal should
  // find that out from the toolbar rather than from the terminal.
  if (needsAgreeing()) {
    const warned = document.createElement("p");
    warned.className = "popup-warn";
    warned.textContent = `Sending continues “${state.receiving.name}” from wherever it left off.`;
    rows.splice(rows.length - 1, 0, warned);
  }

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

/** The model currently chosen, out of whatever the Gateway last said there were. */
function modelNow() {
  const known = state.models || [];
  return known.find((one) => one.id === state.model) || null;
}

/**
 * How this will be answered: which model, and how hard it should think.
 *
 * A row of its own under the field. Files and Schedule are a different question — what
 * else could happen to this message — and folding these in beside them would make one
 * row of six unrelated controls.
 */
function modelPick() {
  const chosen = modelNow();
  const open = document.createElement("button");
  open.type = "button";
  open.className = "mode-key model-key";
  open.setAttribute("aria-haspopup", "true");
  open.setAttribute("aria-expanded", "false");
  const named = document.createElement("span");
  named.className = "model-key-name";
  // Before anything has been asked for, the name of the model is not known — and saying
  // "Default" would be a claim about which one that is.
  named.textContent = chosen ? chosen.name : state.model || "Model";
  open.title = chosen ? `${chosen.name} · ${chosen.provider}` : "Choose the model";
  const mark = document.createElement("span");
  mark.className = "caret";
  mark.textContent = "▾";
  open.append(named, mark);

  const menu = document.createElement("div");
  menu.className = "ask-menu model-menu";
  menu.hidden = true;

  const shut = () => {
    state.picking = null;
    menu.hidden = true;
    open.setAttribute("aria-expanded", "false");
  };
  const fill = () => {
    menu.replaceChildren();
    if (state.modelsTrouble) {
      const said = document.createElement("p");
      said.className = "menu-empty";
      said.textContent = state.modelsTrouble;
      menu.append(said);
      return;
    }
    if (!state.models) {
      const said = document.createElement("p");
      said.className = "menu-empty";
      said.textContent = "Asking…";
      menu.append(said);
      return;
    }
    if (state.models.length === 0) {
      const said = document.createElement("p");
      said.className = "menu-empty";
      said.textContent = "Claude Code chooses its own model — use /model to change it.";
      menu.append(said);
      return;
    }
    for (const model of state.models) {
      const one = document.createElement("button");
      one.type = "button";
      one.className = "ask-menu-row";
      one.dataset.on = String(model.id === state.model);
      // Shown and disabled rather than left out. A model missing because nobody has
      // signed in is something to go and fix; one absent from the list is something
      // somebody concludes this toolbar cannot do.
      one.disabled = model.available === false;
      const name = document.createElement("span");
      name.className = "ask-menu-name";
      name.textContent = model.name;
      const says = document.createElement("span");
      says.className = "ask-menu-says";
      says.textContent =
        model.available === false
          ? `${model.provider} — ${model.whyNot || "not available"}`
          : model.provider;
      one.title = says.textContent;
      one.append(name, says);
      one.addEventListener("click", () => {
        state.model = model.id;
        // The effort belonged to the old model's stops. Kept only if the new one offers
        // it too; otherwise its own default, which is the honest answer to "what now".
        const stops = effortStops(model).map((level) => level.id);
        if (!stops.includes(state.effort || "")) state.effort = null;
        remember();
        render();
      });
      menu.append(one);
    }
  };

  /*
   * Open-ness lives in `state`, not in this closure.
   *
   * The control that holds this is rebuilt by every render, and while an agent is working
   * the rail pulses — so renders arrive two or three times a second and the menu was being
   * destroyed within a frame of being opened. It looked exactly like a button that did
   * nothing. Same fault the `/` list had, and the same cure.
   */
  /*
   * Placed against the window, not against its ancestors.
   *
   * The menu used to be an absolutely-positioned child sitting `bottom: 100%` above the
   * chip, which is fine in the composer and does not survive the move into the receiver
   * popover: that popover is `overflow: hidden`, and where exactly the menu lands depends
   * on a chain of positioned ancestors that Chrome and WebKitGTK do not lay out the same
   * way. It measured correctly in Chrome and appeared nowhere in the real toolbar.
   *
   * Fixed coordinates, worked out from the chip's own box, depend on nothing above them
   * and cannot be clipped by anything. Same approach `placePopup` already uses for the
   * mark popup, and for the same reason.
   */
  const place = () => {
    const chip = open.getBoundingClientRect();
    menu.style.position = "fixed";
    menu.style.left = "auto";
    menu.style.right = "auto";
    menu.style.bottom = "auto";
    menu.style.width = "260px";
    // Above the chip, or below it when there is no room above.
    const tall = Math.min(menu.scrollHeight || 190, 190);
    const above = chip.top - 6 - tall;
    const top = above >= 8 ? above : Math.min(chip.bottom + 6, window.innerHeight - tall - 8);
    menu.style.top = `${Math.round(Math.max(8, top))}px`;
    menu.style.left = `${Math.round(Math.min(Math.max(8, chip.left), window.innerWidth - 268))}px`;
  };

  const show = (open_) => {
    state.picking = open_ ? "model" : null;
    menu.hidden = !open_;
    open.setAttribute("aria-expanded", String(open_));
    if (!open_) return;
    fill();
    place();
    // Asked when it opens, not kept warm: a catalogue held in the background is one
    // that is quietly wrong the moment somebody signs into a provider. Re-placed when it
    // lands, because the list is a different height once it has something in it.
    void loadModels().then(() => {
      fill();
      place();
    });
  };

  open.addEventListener("click", () => show(menu.hidden));

  // Put back the way it was found, after the rebuild that threw it away.
  if (state.picking === "model") {
    menu.hidden = false;
    open.setAttribute("aria-expanded", "true");
    fill();
    // After the chip has been laid out, or its box is still the old one's.
    requestAnimationFrame(place);
  }
  menu.addEventListener("focusout", (event) => {
    if (!row.contains(event.relatedTarget)) shut();
  });
  open.addEventListener("keydown", (event) => {
    if (event.key === "Escape") shut();
  });

  const holder = document.createElement("div");
  holder.className = "mode-pick";
  holder.append(open, menu);
  return holder;
}

/**
 * How hard it should think, as a slider over the chosen model's own stops.
 *
 * On the line with Files and Schedule rather than one of its own. Three rows of controls
 * under one field read as a stack of chrome; this is the row of small things, and the
 * slider fills the width the two words beside it leave.
 */
function effortPick() {
  const chosen = modelNow();
  const stops = effortStops(chosen);
  if (stops.length > 1) {
    const effort = document.createElement("label");
    effort.className = "effort";
    const said = document.createElement("span");
    said.className = "effort-said";
    const bar = document.createElement("input");
    bar.type = "range";
    bar.className = "effort-bar";
    bar.dataset.field = "effort";
    bar.min = "0";
    bar.max = String(stops.length - 1);
    bar.step = "1";
    const at = Math.max(0, effortAt(chosen, state.effort));
    bar.value = String(at);
    said.textContent = stops[at].label;
    bar.setAttribute("aria-label", "How hard to think");
    bar.setAttribute("aria-valuetext", stops[at].label);
    bar.addEventListener("input", () => {
      const level = stops[Number(bar.value)] || stops[0];
      // Not a render: this redraws the whole panel and would take the slider out from
      // under the hand that is dragging it. Only the word beside it moves.
      said.textContent = level.label;
      bar.setAttribute("aria-valuetext", level.label);
    });
    bar.addEventListener("change", () => {
      const level = stops[Number(bar.value)] || stops[0];
      state.effort = level.id;
      remember();
    });
    effort.append(bar, said);
    return effort;
  }
  // A model that does not think in levels gets no slider: an empty one is a control
  // lying about having a choice.
  return null;
}

/** Ask the Gateway which models this receiver could answer with. */
async function loadModels() {
  try {
    const found = await invoke("colai_models", {
      agentId: state.receiving.kind === "agent" ? state.receiving.id : null,
    });
    state.models = Array.isArray(found) ? found : [];
    state.modelsTrouble = null;
  } catch (trouble) {
    // Left as it was rather than emptied: a Gateway that cannot be asked is not the same
    // as a Gateway with no models, and blanking the list would say it was.
    state.modelsTrouble = String(trouble || "Could not ask which models there are.");
  }
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
    : needsAgreeing()
      ? "Send and adopt"
      : going.length
        ? `Send ${counted(going.length, "mark")}`
        : "Send";

  const go = document.createElement("button");
  go.type = "button";
  // Adopting keeps its words. It takes a conversation over from wherever it is running,
  // which is a decision somebody should read before making, and an arrow cannot say it.
  // Everything else is the arrow: what is going is already listed directly above it.
  const spelled = needsAgreeing();
  go.className = spelled ? "popup-do popup-go" : "popup-do popup-go compose-send";
  go.textContent = spelled ? says : state.sending ? "…" : "↑";
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
 * `/` at the start of a word offers the modes. Picking one sets it and takes the word
 * back out, because the mode is how the ask should be read and not part of the ask.
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
      ? `Say what you want done with ${counted(marked, "mark")}… / for mode, @ for a file`
      : "Say what you want done… / for mode, @ for a file";
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
   *
   * `asked` stays local: it is a sequence number for in-flight lookups, and a rebuilt
   * field has no in-flight lookups of its own to disambiguate.
   */
  const asking = state.ask;
  let asked = 0;

  const close = () => {
    asking.mark = null;
    asking.showing = [];
    asking.picked = 0;
    menu.hidden = true;
    menu.replaceChildren();
  };

  const take = (chosen) => {
    const token = tokenAt(text.value, text.selectionStart, asking.mark);
    if (!token) return close();
    const left = withoutToken(text.value, token);
    if (asking.mark === "/") {
      state.mode = chosen.id;
    } else {
      // Described rather than assumed. Whether a file travels with the message or is
      // only named depends on how big it is, and a size invented here as zero would
      // make everything look small enough to carry. `bringFiles` is the one door every
      // file comes through, so a path chosen with `@` lands the same way a dropped one
      // does.
      void invoke("colai_describe_files", { paths: [chosen.path] })
        .then((described) => bringFiles(described))
        .catch(() => {});
    }
    state.text = left.text;
    text.value = left.text;
    // Set here rather than after the redraw, because the redraw is what reads it: the
    // caret is noted off whatever holds it, and this box holds it until `render` runs.
    // It used to be followed by a `text.focus()`, which by then was addressed to a box
    // that had already been replaced and so put the cursor precisely nowhere.
    text.setSelectionRange(left.caret, left.caret);
    close();
    render();
  };

  const draw = () => {
    asking.picked = Math.min(asking.picked, Math.max(0, asking.showing.length - 1));
    menu.hidden = asking.showing.length === 0;
    menu.replaceChildren(
      ...showing.map((row, at) => {
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
    const slash = tokenAt(text.value, text.selectionStart, "/");
    const at = tokenAt(text.value, text.selectionStart, "@");
    // Whichever was typed later is the one being typed now.
    const token = !slash ? at : !at ? slash : slash.from > at.from ? slash : at;
    if (!token) return close();
    asking.mark = token === slash ? "/" : "@";

    if (asking.mark === "/") {
      asking.showing = modesMatching(token.word);
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

  text.addEventListener("input", () => {
    state.text = text.value;
    // Typing does not redraw the composer — a render on every keystroke would rebuild
    // the field and take the caret with it — so the button draw produced would still be
    // refusing after the first word. With nothing marked, that button is the only way
    // out of the composer, and it was dead: a whole sentence typed, and nothing to
    // press. The words are the composer's own subject; marks are extra.
    go.disabled = !canSend(chosenMarks());
    look();
  });
  text.addEventListener("click", look);
  text.addEventListener("blur", close);
  text.addEventListener("keydown", (event) => {
    // Send from the keyboard, unless the `/` or `@` menu is open — there Enter is
    // already answering a question, and stealing it would send whatever half-typed
    // word the menu was offering to complete.
    if (menu.hidden && event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      const going = chosenMarks();
      if (canSend(going)) void sendMarks(going.map((one) => one.id));
      return;
    }
    if (menu.hidden) return;
    if (event.key === "Escape") {
      event.preventDefault();
      // And no further. This listener is on the field, so the event goes on to reach the
      // window — where Escape now closes the work panel this composer lives in. Shutting
      // a suggestion list would have shut the whole panel around it and thrown away what
      // was being typed.
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

  box.append(text, menu);
  // Put back on screen, not merely remembered. The state above survives the rebuild; the
  // element does not, so a list that was open has to be drawn again or hoisting it would
  // only have moved where the disappearance happens.
  if (asking.showing.length > 0) draw();
  return box;
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
  line.className = "agent-empty";
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

  if (needsAgreeing()) {
    const warned = document.createElement("p");
    warned.className = "popup-warn";
    warned.textContent = `Sending continues “${state.receiving.name}” from wherever it left off.`;
    rows.push(warned);
  }

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
  // What they will answer with, under their name. Moving the two controls into the
  // popover would otherwise have hidden the answer as well as the switch — and which
  // model is about to read this is worth knowing without opening anything.
  const how = document.createElement("span");
  how.className = "popup-to-how";
  const chosen = modelNow();
  const level = effortStops(chosen).find((stop) => stop.id === state.effort);
  how.textContent = [chosen ? chosen.name : state.model, level && level.label]
    .filter(Boolean)
    .join(" · ");
  const stack = document.createElement("span");
  stack.className = "popup-to-stack";
  stack.append(named, ...(how.textContent ? [how] : []));
  const mark = document.createElement("span");
  mark.className = "caret";
  mark.textContent = "▾";
  to.append(lit, stack, mark);
  to.title = "Choose who receives this, and how they answer";
  to.addEventListener("click", () => flyout("agents"));
  // What the toolbar can see, offered rather than done.
  //
  // Choosing one of these is not like choosing an agent: sending to a conversation
  // held in another agent adopts it, which hands it to the Gateway and can fail
  // outright if something is already running it. That is a decision, and a decision
  // taken on somebody's behalf because a window happened to be in front is the toolbar
  // arranging a handover nobody asked for. So it says what it sees and waits.
  if (state.inFront && state.inFront.threads.length && !state.picked) {
    const suggested = state.inFront.threads[0];
    const offer = document.createElement("button");
    offer.type = "button";
    offer.className = "row suggest-row";
    const face = document.createElement("span");
    face.className = "agent-avatar-dot";
    face.textContent = suggested.title.slice(0, 1).toUpperCase();
    const said = document.createElement("span");
    said.className = "agent-name";
    said.textContent = suggested.title;
    const why = document.createElement("span");
    why.className = "row-key";
    why.textContent = `${state.inFront.label} is in front`;
    offer.append(face, said, why);
    offer.addEventListener("click", () =>
      receive("thread", suggested.id, suggested.title, null, suggested.locator),
    );
    rows.push(offer);
  }

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
    said.className = "agent-empty";
    said.textContent = `Could not reach the Gateway — ${state.whoTrouble}`;
    el.agentRows.replaceChildren(said);
    return;
  }
  if (state.agents.length === 0 && talking() === 0) {
    const empty = document.createElement("p");
    empty.className = "agent-empty";
    empty.textContent =
      "Nothing yet. Start a conversation with Claude Code and it appears here.";
    el.agentRows.replaceChildren(empty);
    return;
  }

  const rows = [];
  if (state.agents.length) {
    rows.push(group("Agents"));
    for (const agent of state.agents) {
      rows.push(
        whoRow({
          face: agent.emoji || agent.name.slice(0, 1).toUpperCase(),
          name: agent.name,
          receiving: state.receiving.kind === "agent" && state.receiving.id === agent.id,
          onPick: () => receive("agent", agent.id, agent.name, agent.emoji),
        }),
      );
    }
  }
  // Only when there are some: a heading over nothing reads as a list that failed to
  // load, and the machine having no conversations yet is not a failure.
  if (state.sessions.length) {
    rows.push(group("Conversations"));
    for (const session of state.sessions) {
      rows.push(
        whoRow({
          face: emojiFor(session) || session.title.slice(0, 1).toUpperCase(),
          name: session.title,
          note: session.busy ? "Running" : session.unread ? "Unread" : null,
          busy: session.busy,
          receiving: state.receiving.kind === "session" && state.receiving.id === session.key,
          onPick: () => receive("session", session.key, session.title, emojiFor(session)),
          about: {
            kind: "session",
            id: session.key,
            name: session.title,
            sessionKey: session.key,
          },
        }),
      );
    }
  }
  // Grouped under whoever holds them, then under the folder each belongs to, because
  // "Claude Code" and "which project" are the two things you need before a thread's own
  // name means anything.
  const open = openedProjects();
  let holder = null;
  for (const project of state.projects) {
    if (project.holder !== holder) {
      holder = project.holder;
      rows.push(group(holder));
    }
    // A folder with no name holds threads that belong to no project; they are listed
    // where they are rather than filed under something invented.
    if (project.label === null) {
      rows.push(...project.threads.map(threadRow));
      continue;
    }
    rows.push(projectRow(project, open.has(project.key)));
    if (open.has(project.key)) rows.push(...project.threads.map(threadRow));
  }
  el.agentRows.replaceChildren(...rows);
  drawAnswerSettings();
}

/**
 * How the chosen receiver should answer: which model, and how hard it thinks.
 *
 * Here rather than in the composer because that is what they are about. Both are
 * properties of the conversation, not of the message being written — the toolbar already
 * stores them that way, beside the dock position rather than with the text — and putting
 * them on the send row said the opposite twice over: that they were part of this message,
 * and that they were worth a third of the width of the row that sends it.
 */
function drawAnswerSettings() {
  if (!el.agentAnswer) return;
  const model = modelPick();
  const effort = effortPick();
  const said = document.createElement("span");
  said.className = "agent-answer-said";
  said.textContent = "Answers with";
  el.agentAnswer.replaceChildren(said, model, ...(effort ? [effort] : []));
}

/** A folder, as a row that opens. */
function projectRow(project, open) {
  const row = document.createElement("button");
  row.type = "button";
  row.className = "row row-project";
  row.setAttribute("aria-expanded", String(open));
  if (project.path) row.title = project.path;

  const twist = document.createElement("span");
  twist.className = "twist";
  twist.textContent = "▸";
  twist.dataset.open = String(open);

  const label = document.createElement("span");
  label.className = "agent-name";
  label.textContent = project.label;

  const many = document.createElement("span");
  many.className = "row-key";
  many.textContent = String(project.threads.length);

  row.append(twist, label, many);
  row.addEventListener("click", () => toggleProject(project.key));
  return row;
}

/** A conversation inside a folder, indented under it. */
function threadRow(thread) {
  const row = whoRow({
    face: thread.title.slice(0, 1).toUpperCase(),
    name: thread.title,
    note: thread.whereAt,
    receiving: state.receiving.kind === "thread" && state.receiving.id === thread.id,
    onPick: () => receive("thread", thread.id, thread.title, null, thread.locator),
    about: {
      kind: "thread",
      id: thread.id,
      name: thread.title,
      // A thread has no session until it has been sent to; the toolbar knows which of
      // them it adopted, and that is the only kind that can be gone back through.
      sessionKey: state.adoptedKeys ? state.adoptedKeys[thread.id] : undefined,
    },
  });
  row.classList.add("row-nested");
  return row;
}

function group(label) {
  const heading = document.createElement("p");
  heading.className = "row-group";
  heading.textContent = label;
  return heading;
}

function whoRow({ face, name, note, busy, receiving, onPick, about }) {
  const row = document.createElement("button");
  row.type = "button";
  row.className = "row";
  row.setAttribute("role", "menuitem");
  row.setAttribute("aria-pressed", String(receiving));

  const avatar = document.createElement("span");
  avatar.className = "agent-avatar-dot";
  avatar.textContent = face;
  if (busy) avatar.dataset.busy = "true";

  const label = document.createElement("span");
  label.className = "agent-name";
  label.textContent = name;
  label.title = name;

  row.append(avatar, label);
  // "Receiving" wins over "Running": one is what this menu is for, the other is
  // background news, and two marks on one row would make neither readable.
  const said = receiving ? "Receiving" : note;
  if (said) {
    const tail = document.createElement("span");
    tail.className = "row-key";
    tail.textContent = said;
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
