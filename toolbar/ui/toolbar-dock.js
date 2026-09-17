// Where the rail sits, and how it turns.
//
// Dragging it, deciding which screen edge it is claiming, and dissolving between flat
// and upright. Apart from the page because it is geometry and animation rather than
// anything about marks or agents.

/* ── where the rail sits ─────────────────────────────────────────────────── */

/**
 * Listen for somebody picking the rail up.
 *
 * Registered from `start` rather than when this file loads, so the scripts can be
 * ordered by what reads best rather than by which one happens to touch the page first.
 */
function listenForDrag() {
  el.grip.addEventListener("pointerdown", (event) => {
    event.preventDefault();
    const box = railBox();
    let grabX = event.clientX - box.left;
    let grabY = event.clientY - box.top;
    let edge = state.dock;
    let last = { x: box.left, y: box.top };
    // Where the hand went down, so a tap that went nowhere can be told from a drag.
    const from = { x: event.clientX, y: event.clientY };
    let travelled = 0;

    const move = (moved) => {
      const size = railBox();
      const hand = { x: moved.clientX, y: moved.clientY };
      // The screen under the hand, which changes mid-drag the moment the rail is carried
      // across the seam between two of them. Everything below is about that screen, so it
      // has to be worked out before the rail is placed rather than after.
      const room = usable(screenAt(state.screens, hand));
      const x = Math.max(room.left, Math.min(room.right - size.width, hand.x - grabX));
      const y = Math.max(room.top, Math.min(room.bottom - size.height, hand.y - grabY));
      const now = dockFor(hand, state.screens, edge);
      if (now !== edge) {
        const turned = isVertical(now) !== isVertical(edge);
        // A rail that was 420 wide and becomes 44 wide has no sensible relationship to
        // where the hand was on it; re-grabbing near the corner is what keeps it on screen.
        if (turned) {
          grabX = 20;
          grabY = 20;
        }
        edge = now;
        state.dock = now;
        state.open = null;
        if (turned) turn(render);
        else render();
      }
      travelled = Math.max(travelled, Math.hypot(hand.x - from.x, hand.y - from.y));
      last = { x, y };
      state.at = last;
      place();
    };

    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      const size = railBox();
      const rest = { x: last.x, y: last.y };
      // The screen it was let go over, not the one it was picked up from.
      const room = usable(screenAt(state.screens, last));
      // Against the edge of the room the toolbar has, not the edge of the screen.
      if (edge === "left") rest.x = room.left + EDGE;
      if (edge === "right") rest.x = room.right - size.width - EDGE;
      if (edge === "top") rest.y = room.top + EDGE;
      if (edge === "bottom") rest.y = room.bottom - size.height - EDGE;
      state.at = rest;
      place();
      remember();
      if (travelled <= TAP_STILL) tapped();
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  });
}

/**
 * How far the hand may wander and still have been a tap.
 *
 * A few pixels, because a double click on a handle that is also a drag handle is a
 * double click somebody made while holding a mouse steady, not perfectly still.
 */
const TAP_STILL = 4;
/** How long after one tap a second one still belongs to it. */
const TAP_AGAIN = 400;

let lastTap = 0;

/**
 * Two taps on the grip put the rail away, and two more bring it back.
 *
 * Counted here rather than by listening for `dblclick`: the drag calls `preventDefault`
 * on pointerdown, which stops some engines ever synthesising one — and doing it from the
 * pointer events gets touch right for free, and cannot fire in the middle of a drag
 * because a drag is not a tap.
 */
/**
 * Whether the fold is still running.
 *
 * The keys close by animating their own width shut, and only once they have finished do
 * they stop being laid out at all — fifteen zero-width items in a pill two pixels wide
 * is an overflow, and an overflowing flex row puts them where nobody expects. Taking
 * them out of the layout at the start is what made the fold snap instead of fold: there
 * was nothing left on screen to animate.
 *
 * `display` cannot be transitioned, so the delay is kept here rather than in the
 * stylesheet, the way `turn` already stages the rail's orientation change.
 */
let folding = null;

function stillFolding() {
  return folding !== null;
}

/*
 * Opening has a stage of its own, and it is not symmetrical with closing.
 *
 * A folded-away key is `display: none`, because fifteen zero-width items in a pill two
 * pixels wide overflow it. But nothing transitions *from* `display: none` — an element
 * that starts being laid out appears at whatever size it computes to, so the keys came
 * back at full width in a single frame while the pill grew around them.
 *
 * So opening takes two frames. The first puts the keys back in the layout still folded
 * shut; the second unfolds them, and now there is a previous width to animate from.
 * Closing needs no such thing: the keys are already laid out, and it is leaving the
 * layout that has to wait, which is what `folding` above is for.
 */
let opening = false;

/** Whether the rail should be drawn shut, whether or not it is still meant to be. */
function foldedAway() {
  return state.away || opening;
}

function tapped() {
  const now = Date.now();
  const again = now - lastTap < TAP_AGAIN;
  lastTap = again ? 0 : now;
  if (!again) return;
  state.away = !state.away;
  if (folding !== null) clearTimeout(folding);
  folding = null;
  opening = false;
  if (state.away) {
    // Closing: hold them in the layout until they have finished closing.
    folding = setTimeout(() => {
      folding = null;
      render();
      followTheFold();
    }, FOLD_TIME);
  } else {
    // Opening: back into the layout this frame, still shut, and unfolded on the next —
    // so the keys have a width to grow from rather than arriving at their final one.
    opening = true;
    requestAnimationFrame(() => {
      opening = false;
      render();
      followTheFold();
    });
  }
  // Nothing hangs off a rail that is not there. The same tidy-up opening the Work panel
  // already does for flyouts, in the other direction.
  if (state.away) {
    state.open = null;
    state.work.open = false;
  }
  remember();
  render();
  followTheFold();
}

/* ── turning between flat and upright ────────────────────────────────────── */

/** How long the rail takes to settle into a new orientation. */
const TURN = 240;
const TURN_EASE = "cubic-bezier(0.22, 1, 0.36, 1)";

let turning = null;

/**
 * Change the rail's orientation and let it dissolve into the new one.
 *
 * A row of tools and a column of them are two layouts with nothing in between, so this
 * is not a morph: animating the pill's size would squeeze the flex children underneath,
 * and animating the children alone would float them outside a pill that had already
 * snapped. What happens instead is a still picture of the old rail fading out over the
 * new one fading in, which is the one thing that genuinely reads as turning.
 *
 * The state changes at once and only the appearance lags. A drag reads the dock on
 * every pointer move and must never be handed a stale one — that was the flicker.
 *
 * The input shape is re-measured on every frame, because a rail drawn at 94% covers a
 * different part of the screen than a settled one, and a control that is drawn but not
 * clickable is the one thing an overlay must never have. The ghost is measured with it
 * rather than excluded: it is inert, but it is on screen, and the shape should say so.
 */
function turn(change) {
  if (still() || typeof el.rail.animate !== "function") {
    change();
    shape();
    return;
  }
  const ghost = ghostOf(el.rail);
  change();
  if (turning) {
    for (const running of turning) running.cancel();
  }
  const leaving = ghost.animate(
    [
      { opacity: 1, transform: "scale(1)" },
      { opacity: 0, transform: "scale(0.9)" },
    ],
    { duration: TURN * 0.6, easing: "ease-in" },
  );
  const arriving = el.rail.animate(
    [
      { opacity: 0, transform: "scale(0.9)" },
      { opacity: 1, transform: "scale(1)" },
    ],
    { duration: TURN, easing: TURN_EASE },
  );
  turning = [leaving, arriving];
  const drop = () => ghost.remove();
  leaving.finished.then(drop, drop);
  const follow = () => {
    shape();
    if (arriving.playState === "running") requestAnimationFrame(follow);
  };
  requestAnimationFrame(follow);
  // A cancelled animation rejects; the turn that cancelled it is already drawing, so
  // there is nothing left to say about this one.
  arriving.finished.then(shape, () => {});
}

/**
 * A still picture of the rail, parked where the rail was.
 *
 * Inside the wrap so the shape measurement finds it, inert so it cannot be clicked, and
 * stripped of its ids so the real toolbar's elements stay the only ones with those
 * names while it is on screen.
 */
function ghostOf(rail) {
  const ghost = rail.cloneNode(true);
  const box = rail.getBoundingClientRect();
  const wrap = railBox();
  ghost.removeAttribute("id");
  for (const named of ghost.querySelectorAll("[id]")) named.removeAttribute("id");
  ghost.setAttribute("aria-hidden", "true");
  ghost.classList.add("ghost");
  ghost.style.left = `${box.left - wrap.left}px`;
  ghost.style.top = `${box.top - wrap.top}px`;
  ghost.style.width = `${box.width}px`;
  ghost.style.height = `${box.height}px`;
  el.wrap.append(ghost);
  return ghost;
}

/** Whether the desktop has asked for as little movement as possible. */
function still() {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function place() {
  if (!state.at) return;
  el.wrap.style.left = `${state.at.x}px`;
  el.wrap.style.top = `${state.at.y}px`;
}

function remember() {
  try {
    window.localStorage.setItem(
      WHERE,
      JSON.stringify({
        ...state.at,
        dock: state.dock,
        tucked: state.tucked,
        away: state.away,
        model: state.model,
        effort: state.effort,
      }),
    );
  } catch {
    // A toolbar that will not remember where it was put is worth more than one that
    // refuses to appear.
  }
}

function recall() {
  const room = usable(screenAt(state.screens, { x: 0, y: 0 }));
  try {
    const saved = window.localStorage.getItem(WHERE);
    if (saved) {
      const put = JSON.parse(saved);
      state.at = { x: put.x, y: put.y };
      state.dock = put.dock || null;
      // Only an explicit `true` folds them. A toolbar remembered from before this
      // existed has no opinion, and open is what somebody who has not said should get.
      state.tucked = put.tucked === true;
      state.away = put.away === true;
      // How somebody wants their work answered is the same kind of fact as where they
      // put the rail: chosen once, meant until changed.
      state.model = typeof put.model === "string" ? put.model : null;
      state.effort = typeof put.effort === "string" ? put.effort : null;
      return;
    }
  } catch {
    /* falls through to the default corner */
  }
  state.at = { x: room.left + 24, y: room.bottom - 120 };
}

/**
 * A remembered position, brought back inside the room that is actually available.
 *
 * The toolbar is put somewhere on one desktop and opened on another — a different
 * monitor, a dock that moved, a panel that was not there before. Without this it comes
 * back underneath the new chrome and looks broken on first sight.
 */
/**
 * How big the rail is — the bar itself, not everything hanging off it.
 *
 * The wrapper used to be the same thing, and every measurement here took it. Then the
 * work panel moved inside the wrapper to travel with the toolbar, and the wrapper became
 * four hundred pixels wide and eight hundred tall. Every rule that holds the rail on
 * screen was suddenly holding a panel-sized object: opening the panel shoved the rail
 * across the display, and folding it away shoved it back.
 *
 * Placement asks about the rail. What may be clicked is a different question, and
 * `boxAround(el.wrap)` is still the right answer to that one — the panel is clickable.
 */
/**
 * What to assume the rail is, when it cannot be measured.
 *
 * Only ever used to keep a remembered position reachable. Smaller than the real bar, so the
 * guess errs towards leaving it on screen rather than pushing it off the other edge.
 */
const RAIL_AT_LEAST = 48;

function railBox() {
  return el.rail.getBoundingClientRect();
}

function clamp() {
  if (!state.at) return;
  const size = railBox();
  /*
   * An unmeasurable rail must not mean "leave it wherever it was".
   *
   * This used to return here, and that is how a rail ends up off the side of the screen: a
   * position remembered from a wider desk, restored onto a narrower one, and the one piece
   * of code that would have pulled it back declining to run because the bar had not been
   * laid out yet — or was folded to a sliver, which is the same zero.
   *
   * Being roughly right about the width is enough. The point of this is to keep the thing
   * reachable, and a guess that is out by thirty pixels still does that; not running at all
   * does not.
   */
  const wide = size.width || RAIL_AT_LEAST;
  const tall = size.height || RAIL_AT_LEAST;
  /*
   * The room around where it thinks it is — or, if that is nowhere, the room there is.
   *
   * `screenAt` answers with the screen containing a point. A point on a monitor that has
   * since been unplugged is on no screen at all, and the answer then has to be the desk
   * that exists rather than the one that did.
   */
  const room = usable(screenAt(state.screens, state.at) || screenAt(state.screens, { x: 0, y: 0 }));
  state.at = {
    x: Math.min(Math.max(state.at.x, room.left + EDGE), Math.max(room.left + EDGE, room.right - wide - EDGE)),
    y: Math.min(Math.max(state.at.y, room.top + EDGE), Math.max(room.top + EDGE, room.bottom - tall - EDGE)),
  };
  place();
  // And tell the shell where the rail went.
  //
  // Moving without re-shaping leaves the clickable region where the rail *was*: the
  // toolbar draws in one place and answers the pointer in another, and every click on it
  // falls through to the desktop. Silent, and indistinguishable from a dead button.
  shape();
}

/* ── the first run ────────────────────────────────────────────────────────────
 *
 * Four things about this toolbar cannot be discovered by looking at it, and every one of
 * them is load-bearing: the grip moves it and double-clicking puts it away, the fold key
 * hides the exact tools, `/` chooses how a message is read, and `@` brings in a file.
 *
 * There was no teaching surface at all. `/` and `@` were named in a placeholder that
 * disappears on the first keystroke, and the other two were named nowhere. So this is one
 * pass over the things somebody would otherwise never find — shown once, not a tour, and
 * not a thing that comes back on its own.
 *
 * It does come back when asked, though, which it did not. Shown once and dismissed once was
 * the whole of it, so anybody who pressed "Got it" to clear a card off the screen before
 * reading it had thrown away the only account of four things this toolbar cannot be used
 * without — for the life of the install. The way back is colai's own key on the rail, which
 * is on screen for as long as the toolbar is: see `buildRail`.
 */

/** Where the answer to "have they seen this" is kept. */
const TIPS_SEEN = "colai.tips.seen";

/** What nobody guesses, in the order they will meet it. */
const TIPS = [
  ["Move it", "Drag the grip. Double-click it to put the toolbar away."],
  ["Fold it", "The fold key tucks the exact tools out of the way."],
  // "Any box", because that is now true: the composer, the note on a mark and both reply
  // boxes all answer to these two. It said "the box" while only one of the four did, which
  // taught the keystroke and left somebody to find out for themselves where it worked.
  ["Say how", "Type / in any box for a command, or to say how the ask is read."],
  ["Bring a file", "Type @ in any box to name a file to send with it."],
];

function tipsWereSeen() {
  try {
    return window.localStorage.getItem(TIPS_SEEN) === "yes";
  } catch {
    // A browser that will not remember is one that gets told again. Annoying beats a
    // toolbar that refuses to start over a storage permission.
    return false;
  }
}

function rememberTipsSeen() {
  try {
    window.localStorage.setItem(TIPS_SEEN, "yes");
  } catch {}
}

function drawTips() {
  if (!el.tips) return;
  el.tips.hidden = !state.tips;
  if (!state.tips) return;

  const rows = [];
  const head = document.createElement("div");
  head.className = "tips-head";
  const title = document.createElement("strong");
  title.textContent = "Four things worth knowing";
  head.append(title);
  rows.push(head);

  for (const [what, how] of TIPS) {
    const row = document.createElement("p");
    row.className = "tips-row";
    const name = document.createElement("span");
    name.className = "tips-what";
    name.textContent = what;
    const said = document.createElement("span");
    said.textContent = how;
    row.append(name, said);
    rows.push(row);
  }

  const foot = document.createElement("div");
  foot.className = "tips-foot";
  const done = document.createElement("button");
  done.type = "button";
  done.className = "popup-do popup-go";
  done.textContent = "Got it";
  done.addEventListener("click", () => {
    state.tips = false;
    rememberTipsSeen();
    render();
  });
  foot.append(done);
  rows.push(foot);

  el.tips.replaceChildren(...rows);
}
