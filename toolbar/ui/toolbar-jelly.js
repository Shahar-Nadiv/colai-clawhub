// colai's mark — the jellyfish, animated the way it is in the ColaiAd announce.
//
// The rail used to carry a static SVG of the jellyfish with a CSS "walk" played across its
// tentacles only while the agent worked. This replaces that with the ad's own living jellyfish:
// the body is the logo, but the five tentacles are drawn procedurally every frame so the creature
// swims, the dome pulses, and it blinks. Ported from the ad's `jelly.jsx` (same maths, same
// 64-unit space), rendered here as plain SVG DOM updated on a throttled interval rather
// than re-rendered by React.
//
// It swims at rest — settling into a still pose once nothing has happened for a while — and
// the "is anything working" signal rides on top: at rest the
// mark is the rail's calm grey; while the agent works it turns colai green and swims a little
// livelier. A desktop that asked for reduced motion gets a still pose, with the colour alone
// carrying the state (toolbar PRD §4.13).
//
// A classic script sharing one global scope; `buildJelly(size)` returns the SVG element the rail
// mounts, and `startJelly()` (called once from toolbar.js) begins the loop.

const JELLY_NS = "http://www.w3.org/2000/svg";
const JELLY_BASES = [17, 24.5, 32, 39.5, 47];
/** Grey at rest, colai green while working. */
const JELLY_REST = "#b9b9be";
const JELLY_WORK = "#22c55e";
const JELLY_EYE = "#f4f4f5";
/** Every live jellyfish on screen, ticked together by the one loop. */
const jellies = [];

/** A namespaced SVG element with attributes. */
function jellyEl(tag, attrs) {
  const node = document.createElementNS(JELLY_NS, tag);
  for (const key in attrs) node.setAttribute(key, attrs[key]);
  return node;
}

/** Catmull-Rom-ish smoothing through the tentacle points, as a path `d` (from jelly.jsx). */
function jellySmooth(pts) {
  const r2 = (v) => Math.round(v * 100) / 100;
  let d = `M${r2(pts[0][0])} ${r2(pts[0][1])}`;
  for (let i = 0; i < pts.length - 1; i++) {
    const p0 = pts[i - 1] || pts[i];
    const p1 = pts[i];
    const p2 = pts[i + 1];
    const p3 = pts[i + 2] || p2;
    d +=
      ` C${r2(p1[0] + (p2[0] - p0[0]) / 6)} ${r2(p1[1] + (p2[1] - p0[1]) / 6)}` +
      ` ${r2(p2[0] - (p3[0] - p1[0]) / 6)} ${r2(p2[1] - (p3[1] - p1[1]) / 6)}` +
      ` ${r2(p2[0])} ${r2(p2[1])}`;
  }
  return d;
}

/** Build the jellyfish SVG, register it with the loop, paint a first frame, and return it. */
function buildJelly(size) {
  const s = size || 21;
  const svg = jellyEl("svg", { width: s, height: s, viewBox: "0 0 64 64", "aria-hidden": "true" });
  svg.style.display = "block";
  svg.style.overflow = "visible";
  const limbs = jellyEl("g", {
    fill: "none",
    "stroke-width": "3.8",
    "stroke-linecap": "round",
    "stroke-linejoin": "round",
  });
  const tentacles = [];
  for (let i = 0; i < JELLY_BASES.length; i++) {
    const path = jellyEl("path", {});
    limbs.appendChild(path);
    tentacles.push(path);
  }
  const face = jellyEl("g", {});
  const body = jellyEl("path", {
    d: "M8 29C8 14.5 18.5 5 32 5C45.5 5 56 14.5 56 29C56 33 53.5 36 49.5 36L14.5 36C10.5 36 8 33 8 29Z",
  });
  const eyeL = jellyEl("ellipse", { cx: "24.2", cy: "25", rx: "4", ry: "4.4" });
  const eyeR = jellyEl("ellipse", { cx: "39.8", cy: "25", rx: "4", ry: "4.4" });
  const pupilL = jellyEl("circle", { cx: "22.6", cy: "23.5", r: "1.9" });
  const pupilR = jellyEl("circle", { cx: "38.2", cy: "23.5", r: "1.9" });
  face.append(body, eyeL, eyeR, pupilL, pupilR);
  svg.append(limbs, face);
  const ref = { svg, limbs, tentacles, face, body, eyeL, eyeR, pupilL, pupilR, seed: Math.random() * 3 };
  jellies.push(ref);
  tickJelly(ref, 0, jellyWorking(), jellyStill());
  return svg;
}

/** One frame: swim the tentacles, pulse the dome, blink, and paint the state colour. */
function tickJelly(ref, t, working, still) {
  const N = 10;
  const LEN = 26;
  const amp = still ? 0 : working ? 1.35 : 1;
  const pulse = still ? 0 : Math.sin(t * 4.2);
  for (let i = 0; i < JELLY_BASES.length; i++) {
    const bx = JELLY_BASES[i];
    const pts = [];
    for (let k = 0; k <= N; k++) {
      const u = k / N;
      const rest = Math.sin(u * 4.6 + i * 2.2) * 2.6 * u;
      const wob = amp * u * 3.4 * Math.sin(t * 3.6 + i * 0.9 - u * 3.2);
      const fx = bx + rest * (1 - amp * 0.6) + wob;
      const fy = 35.5 + u * LEN * (1 - 0.07 * pulse);
      pts.push([fx, fy]);
    }
    ref.tentacles[i].setAttribute("d", jellySmooth(pts));
  }
  const by = 1 + 0.045 * pulse;
  const bxs = 1 - 0.03 * pulse;
  ref.face.setAttribute("transform", `translate(32 36) scale(${bxs.toFixed(3)} ${by.toFixed(3)}) translate(-32 -36)`);

  const ph = still ? 1 : (t + ref.seed * 1.7) % 3.4;
  const blink = !still && ph < 0.18 ? Math.sin((ph / 0.18) * Math.PI) : 0;
  const open = Math.max(0.08, 1 - 0.92 * blink);
  const ry = (4.4 * open).toFixed(2);
  ref.eyeL.setAttribute("ry", ry);
  ref.eyeR.setAttribute("ry", ry);
  const showPupils = open > 0.3;
  ref.pupilL.style.display = showPupils ? "" : "none";
  ref.pupilR.style.display = showPupils ? "" : "none";

  const color = working ? JELLY_WORK : JELLY_REST;
  ref.limbs.setAttribute("stroke", color);
  ref.body.setAttribute("fill", color);
  ref.pupilL.setAttribute("fill", color);
  ref.pupilR.setAttribute("fill", color);
  ref.eyeL.setAttribute("fill", JELLY_EYE);
  ref.eyeR.setAttribute("fill", JELLY_EYE);
}

/** Whether the agent is working — the one signal the mark colours itself by. */
function jellyWorking() {
  try {
    return !!(typeof buttons !== "undefined" && buttons.settings && buttons.settings.dataset.walking === "true");
  } catch {
    return false;
  }
}

/** Whether this desktop asked for less movement. */
function jellyStill() {
  try {
    return Boolean(window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  } catch {
    return false;
  }
}

let jellyRunning = false;
/** The one interval driving the mark (a frame interval, or the slow watch while paused). */
let jellyTimer = null;
/** The period `jellyTimer` was set with, in ms. */
let jellyPeriod = 0;
/** Frames actually painted since start — what the tests count to prove a pause pauses. */
let jellyFrames = 0;
/** When the swim began, so `t` keeps flowing across a pause. */
let jellyBegan = 0;

/** ~30fps while the agent works (the lively swim), ~20fps at rest — indistinguishable at 21px. */
const JELLY_WORK_MS = 33;
const JELLY_REST_MS = 50;
/** The reduced-motion repaint, and the watch for "back on screen" while paused. */
const JELLY_SLOW_MS = 500;

/** The clock, in ms — `performance.now` where it exists, else wall time. */
function jellyNow() {
  try {
    if (typeof performance !== "undefined" && performance.now) return performance.now();
  } catch {
    /* fall through */
  }
  return Date.now();
}

/**
 * Whether nobody can see the mark right now: the rail is put away, or the window is hidden or
 * minimised. Nothing is painted then — not even the still pose.
 *
 * A merely unfocused window is NOT a pause: the toolbar floats over whatever you are working in
 * and is unfocused nearly all the time, so pausing on blur would freeze the swim for good.
 */
function jellyPaused() {
  try {
    if (typeof state !== "undefined" && state && state.away) return true;
  } catch {
    /* no state yet */
  }
  try {
    if (typeof document !== "undefined" && (document.hidden === true || document.visibilityState === "hidden")) {
      return true;
    }
  } catch {
    /* fall through */
  }
  return false;
}

/** Paint every jellyfish once, at the current moment. */
function jellyPaint() {
  const working = jellyWorking();
  const still = jellyStill();
  const t = still ? 0 : (jellyNow() - jellyBegan) / 1000;
  for (const ref of jellies) tickJelly(ref, t, working, still);
  jellyFrames++;
  return { working, still };
}

/** (Re)arm the one interval at `ms`, unless it already runs at that period. */
function jellyArm(ms) {
  if (typeof setInterval !== "function") return;
  if (jellyTimer !== null && jellyPeriod === ms) return;
  if (jellyTimer !== null && typeof clearInterval === "function") clearInterval(jellyTimer);
  jellyPeriod = ms;
  jellyTimer = setInterval(jellyStep, ms);
}

/**
 * How long the mark swims on with nothing happening before it settles into the still pose.
 *
 * An idle toolbar is most of a day, and a jellyfish swimming for nobody is a repaint every
 * fifty milliseconds for nothing. Fifteen seconds is long enough that the swim is still there
 * after anything you did, and short enough that it is not the whole afternoon.
 */
const JELLY_SETTLE_MS = 15000;
/** The last moment something worth swimming for happened: work, a render, a pointer, a return. */
let jellyStirred = 0;
/** Settled: the still pose is painted and no interval runs at all until something stirs it. */
let jellySettled = false;

/**
 * Lay the mark down: paint the still pose once and stop the interval altogether.
 *
 * Not the slow watch — there is nothing to watch for. Everything that should wake it (a render,
 * the pointer reaching the rail, the window coming back) calls `jellyWake` itself.
 */
function jellySettle(working) {
  for (const ref of jellies) tickJelly(ref, 0, working, true);
  jellyFrames++;
  if (jellyTimer !== null && typeof clearInterval === "function") clearInterval(jellyTimer);
  jellyTimer = null;
  jellyPeriod = 0;
  jellySettled = true;
}

/**
 * One tick. Paused: paint nothing and drop to the slow watch. Otherwise paint, and keep the period
 * matched to the mood — lively while working, calmer at rest, slow when reduced motion holds it still.
 * At rest for long enough, with nothing stirring it, it settles and the interval stops.
 */
function jellyStep() {
  jellySettled = false;
  if (jellyPaused()) {
    jellyArm(JELLY_SLOW_MS);
    return;
  }
  const { working, still } = jellyPaint();
  // Work keeps it awake for as long as it lasts; the quiet is counted from when it ends.
  if (working) jellyStirred = jellyNow();
  else if (jellyNow() - jellyStirred >= JELLY_SETTLE_MS) {
    jellySettle(working);
    return;
  }
  jellyArm(still ? JELLY_SLOW_MS : working ? JELLY_WORK_MS : JELLY_REST_MS);
}

/**
 * Back on screen, the rail unfolded, a render, the pointer over the rail: resume now rather than
 * waiting out the slow watch — or, when the mark had settled, at all. Cheap enough to call from
 * every render: beyond noting the moment, it does nothing unless the loop is paused or settled.
 */
function jellyWake() {
  jellyStirred = jellyNow();
  if (!jellyRunning || jellyPaused()) return;
  if (jellySettled) {
    jellyStep();
    return;
  }
  if (jellyPeriod === JELLY_SLOW_MS && !jellyStill()) jellyStep();
}

/**
 * Whether a node is part of a jellyfish — what the rail's shape observer ignores, since the swim
 * rewrites these paths every frame and none of it moves the rail's outline.
 */
function jellyOwns(node) {
  if (!node) return false;
  for (const ref of jellies) {
    if (ref.svg === node || (typeof ref.svg.contains === "function" && ref.svg.contains(node))) return true;
  }
  return false;
}

/**
 * Start the one loop that swims every jellyfish. Idempotent.
 *
 * Driven by `setInterval` rather than requestAnimationFrame, which keeps the loop out of harnesses
 * that stub rAF as a zero-delay recursion (which would spin forever). Repainting the SVG costs real
 * CPU, so it runs at ~30fps working / ~20fps at rest, and paints nothing at all while the rail is
 * put away or the window is hidden — a slow watch (and the visibility/focus events) resumes it.
 * After a quiet spell at rest it settles into the still pose and stops ticking entirely
 * (`JELLY_SETTLE_MS`); work, a render or the pointer on the rail sets it going again.
 * A first frame is always painted, so the mark looks right even where the interval never ticks.
 */
function startJelly() {
  if (jellyRunning) return;
  jellyRunning = true;
  jellyBegan = jellyNow();
  jellyStirred = jellyBegan;
  try {
    if (typeof document !== "undefined" && document.addEventListener) {
      document.addEventListener("visibilitychange", jellyWake);
    }
    if (typeof window !== "undefined" && window.addEventListener) window.addEventListener("focus", jellyWake);
    // The pointer reaching the rail is somebody looking at it, so a settled mark swims again.
    const rail = typeof document !== "undefined" && document.getElementById ? document.getElementById("rail-wrap") : null;
    if (rail && rail.addEventListener) rail.addEventListener("pointerenter", jellyWake);
  } catch {
    /* the slow watch still resumes it */
  }
  jellyStep();
}
