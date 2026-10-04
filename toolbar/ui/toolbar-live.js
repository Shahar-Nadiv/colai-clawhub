// What a recording shows while it is recording.
//
// It draws on the screen the whole time it runs, and draws *outside* the pixels its
// frames are taken from — a toolbar that photographed its own marker would be answering
// a question about itself. `recordFrame` decides that clearance, against a number read
// out of the capture code that enforces it.

/* ── what a recording shows while it runs ────────────────────────────────── */

let counting = null;

/**
 * Put up the frame and the countdown, and hand the desktop back.
 *
 * Both halves matter. Without the frame a recording is fifteen seconds of a toolbar
 * that has vanished, and nobody can tell whether it is working or broken. Without
 * giving the input shape back it is fifteen seconds of a desktop that will not take a
 * click, which makes the recording a picture of somebody unable to do the thing they
 * wanted recorded.
 */
function startRecording(mark) {
  // Every record mark is dragged out, so this is a guard rather than a case: a mark
  // with no region has no area to show, and inventing one would be a lie about what is
  // in the pictures.
  if (!mark.region) return;
  state.recording = {
    box: mark.region.box,
    until: Date.now() + state.recordFor * 1000,
  };
  drawRecording();
  // The shape is settled once: what it becomes does not change while the seconds run
  // down, and asking the window manager to re-shape ten times a second for a number
  // that is only being read would be work nobody can see.
  shape();
  // Tenths, not seconds: a countdown that redraws on its own second boundary sits on
  // the wrong number for up to a second, which on a two-second recording is half of it.
  counting = setInterval(drawRecording, 100);
}

function stopRecording() {
  if (counting !== null) clearInterval(counting);
  counting = null;
  if (!state.recording) return;
  state.recording = null;
  drawRecording();
  // And the desktop stops being entirely the desktop again. Left alone, the overlay
  // would keep catching nothing while it drew the popup over the region.
  shape();
}

function drawRecording() {
  const now = state.recording;
  el.recording.hidden = now === null;
  if (!now) return;
  const screen = { width: window.innerWidth, height: window.innerHeight };
  const frame = recordFrame(now.box, screen);
  el.recordingArea.style.left = `${frame.x * 100}%`;
  el.recordingArea.style.top = `${frame.y * 100}%`;
  el.recordingArea.style.width = `${frame.w * 100}%`;
  el.recordingArea.style.height = `${frame.h * 100}%`;
  // Above the frame where there is room for it, below where there is not — a badge
  // half off the top of the screen is the one place it cannot be read.
  el.recordingArea.dataset.under = String(frame.y * screen.height < 34);
  el.recordingLeft.textContent = `${secondsLeft(now.until, Date.now())}s`;
}
