// Making a mark, and turning it into a picture.
//
// The gesture, what it produces, and the photograph taken of it — one path from a hand
// on the glass to an image an agent can be given. Kept together because they are one
// motion: the shape somebody drags decides what is cropped, and the crop is what the
// mark turns out to have meant.
//
// Nothing here listens while the file loads. `listenForMarking` is called from the
// page's own `start`, so the order of the script tags is not a dependency anybody has
// to know about.

/* ── drawing ─────────────────────────────────────────────────────────────── */

let gesture = null;
let liveFrame = 0;

function fractionOf(event) {
  const box = el.capture.getBoundingClientRect();
  if (!box.width || !box.height) return { x: 0, y: 0 };
  const clamp = (value) => Math.min(1, Math.max(0, value));
  return {
    x: clamp((event.clientX - box.left) / box.width),
    y: clamp((event.clientY - box.top) / box.height),
  };
}

/**
 * Start listening for the gesture that makes a mark.
 *
 * Registered from `start` rather than while this file loads, so nothing here depends on
 * the order the page's scripts happen to be in — the same reason the dock listens the
 * way it does, and what lets this half of the toolbar live in a file of its own.
 */
function listenForMarking() {
  el.capture.addEventListener("pointerdown", startGesture);
  el.capture.addEventListener("pointermove", extendGesture);
  el.capture.addEventListener("pointerup", release);
  el.capture.addEventListener("pointercancel", release);
}

function startGesture(event) {
  // Only the primary button. A right-click over somebody's desktop belongs to whatever
  // is underneath, and swallowing it is how an overlay earns a reputation.
  if (event.button !== 0) return;
  event.preventDefault();
  // The library is open over everything, and a press outside it means "not this one".
  // Without this the glass underneath takes the press and starts drawing a new mark —
  // so pressing away from the window did not close it, it quietly began another mark
  // behind it, and the only ways out were a close button on the far screen and a key
  // nobody had been told about.
  if (state.library !== null) {
    closeLibrary();
    return;
  }
  // And the Work panel, for the same reason: a press on the glass is a press somewhere
  // that is not the panel. It closes rather than swallowing the press — what was being
  // marked is still being marked, and what was typed is still in the composer when it
  // opens again.
  if (state.work.open) {
    state.work.open = false;
    render();
  }
  // A press off the popup. The catcher covers the whole desk and the popup is stacked
  // above it, so this only ever fires outside.
  //
  // For a tool somebody is annotating with, that means "keep that one, here is another":
  // the mark is already photographed and already in the tray, so the popup simply closes
  // and a new gesture begins under the same press. For everything else it still means
  // never mind, which is the third way out the popup was given.
  if (state.popup !== null) {
    if (!KEEPS_MARKING.includes(state.tool)) {
      cancelMark(state.popup);
      return;
    }
    state.popup = null;
    render();
  }
  if (event.target.setPointerCapture) event.target.setPointerCapture(event.pointerId);

  const point = fractionOf(event);
  const kind = kindFor(state.tool, state.pen);

  // A pin is the click itself; there is nothing to drag out, and waiting for the
  // release would make a tool that should feel instant feel unsure.
  if (!kind) {
    addMark({ tool: state.tool, region: null, points: [point] });
    return;
  }
  gesture = { kind, points: [point], screen: screenSize() };
  drawLive();
}

function extendGesture(event) {
  if (!gesture) return;
  const point = fractionOf(event);
  // The two pens that keep every point the hand passed through. An arrow or a line
  // keeps two, because a line somebody drew wobbling is not a line they meant.
  if (gesture.kind === "stroke" || gesture.kind === "highlight") {
    // A 120Hz pointer emits points a fraction of a pixel apart; keeping them all makes
    // a path attribute nothing can read and every frame has to re-parse.
    const last = gesture.points[gesture.points.length - 1];
    if (Math.hypot(point.x - last.x, point.y - last.y) < 0.002) return;
    gesture.points.push(point);
    return;
  }
  gesture.points[1] = point;
}

function release() {
  const finished = gesture;
  gesture = null;
  cancelAnimationFrame(liveFrame);
  const live = document.getElementById("live");
  if (live) live.remove();
  const liveSpan = document.getElementById("live-span");
  if (liveSpan) liveSpan.remove();
  if (!finished || finished.points.length === 0) return;

  const box = boxOf(finished.points);
  // A press that went nowhere is a click, not a region. Without this every stray click
  // becomes a zero-sized mark that is invisible, un-hittable, and still counted.
  if (!PATHS.includes(finished.kind) && box.w < 0.004 && box.h < 0.004) {
    const meant = CLICK_MEANS[state.tool];
    // The tools that photograph: not dragging one out is how somebody asks for the whole
    // screen, and refusing that as a slip would leave the simplest thing the toolbar does
    // with no way to ask for it.
    if (meant === "display") {
      addMark({ tool: state.tool, region: null, points: [] });
      return;
    }
    // And the tools where a click is a place. No region, one point — the same shape a
    // `pointAt` mark has, so it draws as a numbered pin and carries a coordinate without
    // a line of new drawing code.
    if (meant === "point") {
      addMark({ tool: state.tool, region: null, points: [finished.points[0]] });
      return;
    }
    return;
  }

  const made = {
    tool: state.tool,
    region: PATHS.includes(finished.kind)
      ? null
      : { shape: finished.kind === "ellipse" ? "ellipse" : "box", box },
    points: finished.points,
  };
  // Which pen drew it. Carried on the mark rather than read from the toolbar later:
  // the pen can be changed while a mark is still waiting in the tray, and a mark should
  // not quietly become a different drawing because somebody picked up a highlighter.
  if (state.tool === "draw") made.pen = state.pen;
  // Settled once, here, rather than recomputed wherever it happens to be needed: the
  // rail can move to a screen of another size, and the answer is about the pixels that
  // were under the hand at the time.
  if (finished.kind === "span") made.px = spanOf(finished.points, screenNow());
  addMark(made);
}

/**
 * The live shape, drawn straight into one attribute rather than through a re-render.
 *
 * A still hand produces the same path string every frame, so comparing it is far
 * cheaper than writing an attribute the browser then has to re-parse.
 */
function drawLive() {
  let previous = "";
  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.id = "live";
  path.setAttribute("stroke", "var(--accent)");
  path.setAttribute("stroke-width", "2");
  path.setAttribute("stroke-dasharray", "6 4");
  path.setAttribute("vector-effect", "non-scaling-stroke");
  el.marks.append(path);

  let label = null;
  const step = () => {
    liveFrame = requestAnimationFrame(step);
    if (!gesture) return;
    const d = pathFor(gesture);
    if (d === previous) return;
    previous = d;
    path.setAttribute("d", d);
    // Measuring without seeing the number while you drag is not measuring, it is
    // guessing and then being told.
    if (gesture.kind === "span" && gesture.points.length > 1) {
      if (label) label.remove();
      label = spanLabel(gesture.points, `${spanOf(gesture.points, screenNow())}px`);
      label.id = "live-span";
      el.pins.append(label);
    }
    path.setAttribute(
      "fill",
      PATHS.includes(gesture.kind) ? "none" : "color-mix(in srgb, var(--accent) 13%, transparent)",
    );
    // A highlighter shows its real width while it is being drawn, or somebody finds out
    // how much it covered only after letting go.
    path.setAttribute("stroke-width", gesture.kind === "highlight" ? String(HIGHLIGHT_WIDE) : "2");
    path.setAttribute("stroke-opacity", gesture.kind === "highlight" ? "0.32" : "1");
  };
  step();
}

function addMark(mark) {
  mark.id = `mark-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
  mark.note = "";
  // In the next send unless somebody says otherwise. Marking something and then having
  // to go and tick it is a step nobody asked for.
  mark.chosen = true;
  // A design mark starts as a wireframe with the kind's own home in it. Somebody can
  // change both in the popup; starting empty would mean typing a path they had no
  // reason to know before they could ask for the commonest thing on the menu.
  if (mark.tool === "design") {
    mark.design = state.designKind;
    mark.dest = (DESIGNS[state.designKind] || DESIGNS[DESIGN_FIRST]).home;
  }
  // Which git command this mark is asking for. The repository is not settled here: the
  // address is read after the picture is taken, so it is stamped on in `shoot` where
  // the rest of the address is.
  if (mark.tool === "git") mark.git = state.gitKind;
  // Until there is a photograph of what it is about. See `drawMarks`.
  mark.shooting = true;
  state.marks.push(mark);
  // A new mark ends the redo trail: what was undone is no longer what comes next.
  state.undone = [];
  render();
  void shoot(mark);
}

/**
 * Photograph what a mark is about, then ask what to do with it.
 *
 * The page makes itself invisible first. The overlay is transparent, so a page that
 * draws nothing composites to nothing and the picture comes out without the toolbar in
 * it — which hiding the window would also achieve, at the cost of the window manager
 * handing the keyboard somewhere else the moment it came back.
 *
 * Two frames and a moment: the compositor has to have presented the empty overlay
 * before the pixels underneath it are read, and a frame callback only says the page has
 * drawn, not that anybody has seen it.
 */
async function shoot(mark) {
  await photograph(mark);
  state.popup = mark.id;
  render();
  // The note is a text field and one way out is a key, and neither works while the
  // window manager treats this window as scenery.
  reachTheKeyboard();
}

/**
 * Take the picture, and nothing else.
 *
 * Split from `shoot` because the popup and the keyboard grab are what a person wants
 * when they have just marked something, and a picture is worth taking without them.
 */
/*
 * Getting out of the picture, without the toolbar disappearing to do it.
 *
 * A capture is a photograph of the desktop, and this window is on the desktop — so
 * anything it is drawing lands in the shot. It used to solve that by hiding the whole
 * body: every layer, the rail included, for two frames and forty milliseconds, on every
 * single mark. That is the blink. The rail is almost never inside the region being
 * photographed, and it was being taken off the screen anyway.
 *
 * So each layer is asked whether it is actually in shot. The marks are, by definition —
 * they are drawn on the thing being photographed. The rail, a popup, a toast usually are
 * not, and stay exactly where they are.
 *
 * `visibility` rather than `display` or `opacity`, as before: it is the one property a
 * child can turn back on underneath a hidden ancestor, which is how the recording frame
 * stays visible while everything around it goes.
 */

/** The layers that could show up in a photograph, in the order they are stacked. */
const IN_SHOT = ["rail-wrap", "marks", "pins", "popup", "library", "toasts", "flights"];

/*
 * How far past the mark the picture actually reaches.
 *
 * These three are `OUTLINE_ROOM`, `CONTEXT_SHARE` and `CONTEXT_LEAST` in
 * `src-tauri/src/colai_marks.rs`, which is where the crop is really decided. They are
 * repeated here because this side has to know what will be in the photograph *before*
 * asking for it, and a guess is not good enough in one direction: too small and the
 * toolbar is left standing in somebody's screenshot.
 *
 * A region is its own rectangle plus room for the outline drawn on it. Anything else — a
 * point, a stroke, a measurement — is photographed with a wide margin of context around
 * it, because a dot with nothing around it tells an agent nothing. On a 1080-tall screen
 * that margin is about 151px, not a rounding error.
 *
 * `colai_marks.rs` works in physical pixels and this works in CSS pixels. Using the same
 * numbers therefore over-estimates on a HiDPI screen, which is the direction to be wrong
 * in. The test beside this holds the two files to the same figures.
 */
const OUTLINE_ROOM = 12;
const CONTEXT_SHARE = 0.14;
const CONTEXT_LEAST = 140;

/** Where on the page this mark is about to photograph, in CSS pixels. */
function shotBox(mark) {
  const across = window.innerWidth;
  const down = window.innerHeight;
  const box = mark.region
    ? mark.region.box
    : mark.points && mark.points.length
      ? boxOf(mark.points)
      : null;
  // No box is a whole-display capture, and everything is in that.
  if (!box) return null;
  const room = mark.region
    ? OUTLINE_ROOM
    : Math.max(Math.min(across, down) * CONTEXT_SHARE, CONTEXT_LEAST);
  return {
    left: box.x * across - room,
    top: box.y * down - room,
    right: (box.x + box.w) * across + room,
    bottom: (box.y + box.h) * down + room,
  };
}

function overlaps(rect, shot) {
  return (
    rect.left < shot.right &&
    rect.right > shot.left &&
    rect.top < shot.bottom &&
    rect.bottom > shot.top
  );
}

/** Take the layers that are in shot off the screen, and say which they were. */
function hideFromTheShot(mark) {
  const shot = shotBox(mark);
  const putAway = [];
  for (const id of IN_SHOT) {
    const layer = document.getElementById(id);
    if (!layer || layer.hidden) continue;
    // No box means no way to tell, which is the one case worth being timid about: a
    // whole-display capture has no region, and everything is in it.
    if (shot && id !== "marks" && !overlaps(layer.getBoundingClientRect(), shot)) continue;
    layer.style.visibility = "hidden";
    putAway.push(layer);
  }
  return putAway;
}

async function photograph(mark) {
  // What is in front *now*. A mark is about the window somebody is looking at, and
  // reading that once when the app started answered a question about a different
  // afternoon.
  await learnFront();
  // The address goes on the mark rather than being read at send time. Two marks made in
  // two applications are two addresses, and one global read at the end would label both
  // with whichever happened to be last — confidently, and wrongly.
  mark.where = await showing(state.surface, mark);
  mark.spot = spotIn(mark, mark.where, screenSize());
  // And what it is attached to, which is the same address said as something to draw
  // from. `region.box` stays what it always was — where this was on the screen at the
  // moment it was marked — because the picture was cropped from it and the message
  // describes it. `inside` is that place said in the window's own terms, so drawing can
  // ask where the window is now instead of where the desktop was then.
  mark.on = anchorOf(mark.where);
  // The repository this is about, worked out from the address the mark just took. Done
  // here rather than at send time for the same reason the address is: two marks made in
  // two checkouts are two repositories, and one read at the end would label both with
  // whichever window happened to be last.
  if (mark.tool === "git") mark.repo = repoFor(mark.where);
  if (mark.on) {
    const screen = screenSize();
    mark.inside = {
      box: mark.region ? intoWindow(mark.region.box, mark.on.at, screen) : null,
      points: (mark.points || []).map((point) => intoWindow(point, mark.on.at, screen)),
    };
  }
  const putAway = hideFromTheShot(mark);
  // Said out loud, because going invisible makes somebody else's window the front one
  // and the rule that closes panels on that would otherwise fire on every mark.
  state.capturing = true;
  try {
    await new Promise((drawn) => requestAnimationFrame(() => requestAnimationFrame(drawn)));
    await new Promise((waited) => setTimeout(waited, 40));
    // A recording is the one capture long enough to be waited through, so it says so
    // while it happens — and gives the desktop back while it does, because a recording
    // of somebody being unable to click anything is not what they were pointing at.
    if (mark.tool === "record") startRecording(mark);
    const taken = await invoke("colai_capture_mark", {
      mark,
      accent: accentNow(),
      seconds: state.recordFor,
    });
    if (taken.hex) mark.hex = taken.hex;
    mark.frames = taken.frames;
    mark.seconds = taken.seconds;
    mark.thumb = taken.thumb;
    mark.shot = `${taken.width}\u00d7${taken.height}`;
  } catch (error) {
    // The mark still exists and can still be described; it simply arrives without a
    // picture. Saying which is better than a popup that looks broken.
    mark.trouble = error && error.message ? error.message : String(error);
  } finally {
    stopRecording();
    state.capturing = false;
    for (const layer of putAway) layer.style.visibility = "";
    // Drawn from here on, picture or no picture: a capture that failed still leaves a
    // mark somebody made and can describe.
    delete mark.shooting;
  }
}

/**
 * Which application is in front, and which project that makes this about.
 *
 * Asked again rather than remembered from startup: the window in front is the one
 * thing on this desktop guaranteed to have changed since.
 */
async function learnFront() {
  try {
    const front = await invoke("colai_frontmost");
    // No connector until the surface registry exists. Stated rather than implied: this
    // is what a write is refused on, and defaulting it otherwise would make the refusal
    // meaningless. The title comes with it, because that is where an editor puts the
    // name of the repository it has open.
    // Everything the desktop and the kernel would say, kept whole. `connector` is
    // still null because no surface registry exists yet, and stating it is what makes
    // the write gate's refusal mean something.
    state.surface = front ? { ...front, connector: null } : null;
  } catch {
    state.surface = null;
  }
  state.inFront = projectInFront(state.projects, state.surface);
}

/** The colour the app is themed in, if it has told us one. */
function accentNow() {
  const said = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim();
  return said || undefined;
}

function drawMarks() {
  const live = document.getElementById("live");
  const drawn = [];
  // Where the marks belong now: the window each was made on, where that window is at
  // this moment. A mark whose application is not the one in front is not drawn at all —
  // a dot left at the same pixels over somebody else's tab is the toolbar lying about
  // what it is pointing at.
  const front = state.front;
  const screen = screenSize();
  // Whether marks are on the screen at all: only while a tool is out.
  const look = { tool: state.tool };
  for (const held of state.marks) {
    // Not while it is being photographed. It used to be drawn the instant it was made
    // and then hidden for its own capture, so a mark appeared, vanished for the length
    // of the shot, and came back — the flash somebody sees the moment they finish a
    // gesture. Drawn once, when there is a picture of what it is about.
    if (held.shooting) continue;
    if (!showingNow(held, front, look)) continue;
    const mark = asDrawn(held, front, screen);
    // A span has no region — it is two points and the distance between them — so the
    // shape cannot be read off the mark the way a box's can. Without this the number
    // was drawn and the line it measures was not.
    const kind =
      DRAWS[mark.tool] === "span"
        ? "span"
        : mark.tool === "draw"
          ? kindFor("draw", mark.pen)
          : mark.region && mark.region.shape;
    if (!kind) continue;
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    // The size of the layer, because an arrowhead built in the unit square it is
    // stretched from comes out as a different shape on every screen.
    path.setAttribute("d", pathFor({ kind, points: mark.points, screen: screenSize() }));
    path.setAttribute(
      "fill",
      PATHS.includes(kind) ? "none" : "color-mix(in srgb, var(--accent) 13%, transparent)",
    );
    path.setAttribute("stroke", "var(--accent)");
    // A highlighter is a wide translucent stripe rather than a line: it is meant to sit
    // over words and leave them readable, which a solid stroke does not.
    path.setAttribute("stroke-width", kind === "highlight" ? String(HIGHLIGHT_WIDE) : "2");
    if (kind === "highlight") path.setAttribute("stroke-opacity", "0.32");
    path.setAttribute("stroke-linecap", "round");
    path.setAttribute("stroke-linejoin", "round");
    path.setAttribute("vector-effect", "non-scaling-stroke");
    drawn.push(path);
  }
  // The live path belongs to the gesture, not to this list, so it is put back rather
  // than swept away mid-drag.
  el.marks.replaceChildren(...drawn);
  if (live) el.marks.append(live);

  // Every waiting mark wears its number, and it is the number the message will give it.
  // One mark on screen needed none of this; four of them are unreadable without it.
  const drawnPins = state.marks
    .map((held) => {
      if (!showingNow(held, front, look)) return null;
      const mark = asDrawn(held, front, screen);
      const spot = badgeAt(mark);
      // Numbered from the mark itself, not from the copy: the number has to be the one
      // the message will give it, and the copy is not in the list the message counts.
      const number = numberOf(state.marks, held);
      if (!spot || number === null) return null;
      const pin = document.createElement("span");
      pin.className = "pin";
      pin.dataset.pointing = String(mark.tool === "pointAt");
      pin.style.left = `${spot.x * 100}%`;
      pin.style.top = `${spot.y * 100}%`;
      pin.textContent = String(number);
      return pin;
    })
    .filter(Boolean);
  // A measurement's whole point is its number, so the number is on the screen and not
  // only in the message. Written in HTML rather than into the marks layer, which is a
  // unit square stretched to the display and would stretch the text with it.
  for (const held of state.marks) {
    if (held.tool !== "measure" || typeof held.px !== "number") continue;
    if (!showingNow(held, front, look)) continue;
    // The label rides with its line, so it is placed from the drawn copy too. The number
    // it says does not change: a measurement is of the thing, not of the screen it
    // happens to be on, and a window somebody resized did not re-measure anything.
    drawnPins.push(spanLabel(asDrawn(held, front, screen).points, `${held.px}px`));
  }
  el.pins.replaceChildren(...drawnPins);
}

/**
 * The only thing the rail says on its own, and it only says it when something is wrong.
 *
 * There used to be a line here after every action, naming the tool, the window in front
 * and who was receiving. All three are already on the rail, so it was the toolbar
 * reading its own state back — the kind of thing somebody notices once and then never
 * reads again, while it sits over their work.
 */
/**
 * What an agent actually said, out of a message that may be many things.
 *
 * A turn carries tool calls, reasoning and images as well as words. Only the words are
 * an answer, and only from the other side — the echo of the question going in is not a
 * reply to it.
 */
/**
 * The address, with whatever the desktop is willing to add to it.
 *
 * The cheap half is already in hand and cannot fail. This asks for the half that can —
 * a page's real URL — which costs a quarter of a second at worst.
 *
 * Asked once per window, ever. Which windows can answer is decided by asking them
 * rather than by a list of browser names, but a window that exposes no document will
 * not start exposing one, so the second reading of an editor or a terminal is a quarter
 * of a second spent learning what the first one already knew. Only the *no* is
 * remembered: a browser's URL changes with every tab, and a remembered one would be a
 * confident answer about a page somebody left.
 */
async function showing(where, mark) {
  if (!where) return where;
  if (state.mute.has(where.id)) return where;
  const at = middleOf([mark]);
  const seen = await invoke("colai_showing", {
    x: Math.round(at.x * window.innerWidth),
    y: Math.round(at.y * window.innerHeight),
  }).catch(() => null);
  if (seen && seen.url) return { ...where, url: seen.url };
  state.mute.add(where.id);
  return where;
}

/**
 * Where a mark's number sits: on a pin, or at the top-left corner of anything with a
 * shape. Nothing for a mark that has neither, which is a screenshot of the whole display
 * — a number in the corner of the desktop labels the desktop.
 */
function badgeAt(mark) {
  if (mark.region) return { x: mark.region.box.x, y: mark.region.box.y };
  if (mark.points && mark.points.length) return mark.points[0];
  return null;
}

/** How big the layer the marks are stretched over actually is, in pixels. */
function screenSize() {
  return { width: window.innerWidth, height: window.innerHeight };
}

function screenNow() {
  return { width: window.innerWidth, height: window.innerHeight };
}

function spanLabel(points, said) {
  const [from, to] = [points[0], points[points.length - 1]];
  const label = document.createElement("span");
  label.className = "span-label";
  label.style.left = `${((from.x + to.x) / 2) * 100}%`;
  label.style.top = `${((from.y + to.y) / 2) * 100}%`;
  label.textContent = said;
  return label;
}

/**
 * The answers, waiting or arrived, on the regions they are about.
 *
 * Each one is measured for the input shape on its own rather than as a group: a pin at
 * one corner of the screen and a pin at the other would otherwise claim everything
 * between them, and everything between them is somebody's desktop.
 */
