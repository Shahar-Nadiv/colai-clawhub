// One overlay shell for the big panels.
//
// The bespoke surfaces the PRD names — Work, Agents and the Context map — are meant to share
// one way of appearing: the same enter (a fade and a small rise and scale, 180ms), the same
// two ways out (Escape, and a press anywhere off the panel), and a focus trap while one is up
// so Tab does not wander onto the rail behind it. This is that shared shell.
//
// It is deliberately additive. `state.panel` is a new field, separate from `state.open` (the
// small flyouts that hang off a rail key) and from `state.surface` (the front window a mark is
// about) — so a surface can be built on the shell without first moving the panels that already
// work onto it. The Context map is the first thing built on it; Work and Agents can follow when
// there is reason to, and until then lose nothing by staying as they are.
//
// A classic script sharing one global scope with the rest of the toolbar. It reads `el` and
// `state` from toolbar.js, and the screen helpers (`screenAt`, `usable`, `EDGE`) from the files
// loaded before it.

/**
 * Bring a surface up: name it so the rest of the page knows what is open, show its node,
 * centre it on the screen the rail is on, play the enter once, and trap the keyboard in it.
 */
function openSurface(name, node) {
  state.panel = name;
  if (!node) return;
  node.hidden = false;
  placeSurface(node);
  // The enter, once. Taken off on the next frame so a redraw that leaves the panel where it
  // was does not replay it — the animation is for arriving, not for every render.
  node.classList.add("overlay-enter");
  requestAnimationFrame(() => node.classList.remove("overlay-enter"));
  trapFocus(node);
}

/** Take the surface down. The caller renders; this only says it is gone. */
function closeSurface() {
  state.panel = null;
}

/**
 * Let a surface be dragged by a handle (its header). Modelled on the rail's own drag
 * (`listenForDrag`): pointerdown on the handle, then window pointermove/pointerup, moving the node by
 * `left`/`top` clamped onto the screen it is on. `onDrop({left, top})` is called at the end so the
 * caller can remember where it was left. A press on a button inside the handle (the close ×, a tab) is
 * left alone, so those still click.
 */
function makeDraggable(node, handle, onDrop) {
  if (!node || !handle) return;
  handle.addEventListener("pointerdown", (event) => {
    if (event.target && event.target.closest && event.target.closest("button")) return;
    event.preventDefault();
    const box = node.getBoundingClientRect();
    const offX = event.clientX - box.left;
    const offY = event.clientY - box.top;
    const width = box.width;
    const height = box.height;
    let last = null;
    const move = (ev) => {
      const room = usable(screenAt(state.screens, { x: ev.clientX, y: ev.clientY }));
      let left = ev.clientX - offX;
      let top = ev.clientY - offY;
      left = Math.max(room.left + EDGE, Math.min(left, room.right - width - EDGE));
      top = Math.max(room.top + EDGE, Math.min(top, room.bottom - height - EDGE));
      node.style.position = "fixed";
      node.style.left = `${Math.round(left)}px`;
      node.style.top = `${Math.round(top)}px`;
      last = { left: Math.round(left), top: Math.round(top) };
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      if (last && typeof onDrop === "function") onDrop(last);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  });
}

/**
 * Centre a surface on the screen the rail is on, pulled back from any edge it would overhang.
 *
 * The panels the shell carries are large and fixed (the Context map is 1120x560), so they are
 * centred rather than anchored to a key the way the small flyouts are. The clamp is the same
 * idea as `within`: never let an edge cut the panel off, and when the panel is larger than the
 * room, the near edge wins so its top-left stays reachable.
 */
function placeSurface(node) {
  const at = state.at || { x: 0, y: 0 };
  const room = usable(screenAt(state.screens, at));
  const box = node.getBoundingClientRect();
  const width = box.width || node.offsetWidth || 0;
  const height = box.height || node.offsetHeight || 0;
  const middleLeft = room.left + (room.right - room.left - width) / 2;
  const middleTop = room.top + (room.bottom - room.top - height) / 2;
  const left = Math.max(room.left + EDGE, Math.min(middleLeft, room.right - width - EDGE));
  const top = Math.max(room.top + EDGE, Math.min(middleTop, room.bottom - height - EDGE));
  node.style.position = "fixed";
  node.style.left = `${Math.round(left)}px`;
  node.style.top = `${Math.round(top)}px`;
}

/**
 * Keep Tab inside the open surface.
 *
 * The overlay is a dock the window manager keeps above everything, and the rail behind an open
 * panel is scenery — tabbing onto it is tabbing into furniture. So Tab cycles within the panel,
 * wrapping at each end. Installed once per node (guarded), because a surface is opened many
 * times over its life and a listener added each time would fire many times per key.
 */
function trapFocus(node) {
  if (!node || node.dataset.trapped === "yes") return;
  node.dataset.trapped = "yes";
  node.addEventListener("keydown", (event) => {
    if (event.key !== "Tab") return;
    const focusable = [
      ...node.querySelectorAll(
        'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
      ),
    ].filter((one) => one.offsetParent !== null || one === node.ownerDocument.activeElement);
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = node.ownerDocument.activeElement;
    if (event.shiftKey && active === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  });
}
