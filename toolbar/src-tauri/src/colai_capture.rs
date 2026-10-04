//! Turning a mark into a picture somebody can look at.
//!
//! The toolbar's whole promise is that you point at something and an agent sees what you
//! meant. A region drawn on glass is not that: it is a rectangle in fractions of a
//! screen, and it means nothing to anyone who was not looking at the screen. This is
//! where it becomes pixels.
//!
//! Two decisions shape everything here.
//!
//! **The mark is drawn back on.** A box or a circle survives being cropped — the crop is
//! the annotation. A pin has no size at all and a freehand stroke is a shape rather than
//! an area, so cropping either one produces a picture of some pixels with no indication
//! of what about them mattered. Those take the ground around them and wear the mark.
//!
//! **The bytes stay here.** A screenshot is a megabyte and the page has no use for it: it
//! shows a thumbnail and knows the mark's name. Sending the full image to the page and
//! back again would put that megabyte through the IPC boundary twice for nothing.

use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::colai_marks::{crop_for, drawn_as, points_within, Mark};
use tauri::{AppHandle, Manager};
/// An arrowhead: how far back along its own line the barbs sit, and how far out.
///
/// A share of the arrow rather than a fixed size, so a short arrow does not arrive as a
/// head with a stub behind it — bounded at both ends, because a share of a very long
/// arrow is a head the size of a window.
const ARROW_HEAD: f64 = 0.22;
const ARROW_LEAST: f64 = 12.0;
const ARROW_MOST: f64 = 42.0;
const ARROW_WIDE: f64 = 0.42;

/// How wide a highlighter lays down, and how much of the screen shows through it.
const HIGHLIGHT_WIDE: f64 = 22.0;
const HIGHLIGHT_THROUGH: f64 = 0.32;

const THUMB_EDGE: i32 = 180;
/// The widest edge a picture keeps before it is shrunk.
///
/// Bounded here rather than at the send, because the model refuses an image over 6MB
/// and a refusal at that point loses the whole batch for one oversized capture.
///
/// The number is a measurement, not a guess. A whole 1920-wide screen sent at 1600 costs
/// about 1,920 image tokens and at 1,100 about 900 — and the question is only whether the
/// smaller one is still readable. Tested against real rendered interface text at ten and
/// a half pixels, scaled by exactly that ratio: every word survived, including monospace
/// file sizes and the smallest labels on the toolbar's own composer.
///
/// Twelve hundred rather than the eleven that was measured, because one sample of one
/// interface in one theme is thin evidence for a ceiling that applies to every screen
/// anybody points at. It still costs a little over half what sixteen hundred did.
const SHOT_EDGE: i32 = 1200;

/// The pixels a mark is about.
///
/// A box or a circle is its own answer. A pin and a stroke are not — a pin has no area
/// whatsoever — so those take the ground around them, because a crop of nothing tells an
/// agent nothing. A mark with neither is a whole-display capture, which is what the
/// screenshot tool asks for when it is clicked rather than dragged.
///
/// Everything is clamped to the display: a mark made against an edge still has to name a
/// rectangle that exists.
pub(crate) struct Shot {
    pub id: String,
    pub frames: Vec<Vec<u8>>,
    pub width: i32,
    pub height: i32,
}

/// The most frames a recording will ever take.
///
/// A cap rather than a rate, because the length is somebody's to choose and the number
/// of images is not: every frame is a picture an agent has to be sent and pay for, and
/// fifty of them is not a recording, it is a bill. A longer recording spreads the same
/// handful of frames further apart — which is the honest trade, and the one a person
/// would make if asked.
const RECORD_FRAMES: usize = 8;
/// The shortest gap between frames, so a brief recording still samples quickly enough
/// to catch something that flickers.
const RECORD_CLOSEST: u64 = 250;

/// How far apart a recording's frames fall, for a length in seconds.
///
/// Across the gaps, not the frames: eight photographs have seven gaps between them, and
/// dividing by eight would leave the last frame an interval short of the length
/// somebody asked for — a fifteen-second recording that actually covered thirteen.
fn record_every(seconds: f64) -> std::time::Duration {
    let across = (seconds.max(0.25) * 1000.0) / (RECORD_FRAMES - 1) as f64;
    std::time::Duration::from_millis((across as u64).max(RECORD_CLOSEST))
}

impl Shot {
    fn weighs(&self) -> usize {
        self.frames.iter().map(Vec::len).sum()
    }
}

/// The pictures taken for marks nobody has sent yet.
///
/// Bounded on both counts that can run away: how many are held, and how much they weigh
/// together. An afternoon of marking should not become the reason the machine swaps.
#[derive(Default)]
pub(crate) struct MarkShots(Mutex<Vec<Held>>);

/// A shot as the store keeps it: each frame behind its own `Arc`, so a send can be handed them
/// without a copy.
///
/// Shared rather than moved out, because the store has to keep them until the send has
/// landed — a send that fails leaves the marks in the tray, and they must still have their
/// pictures behind them when it is tried again. Shared rather than cloned, because a
/// recording is eight full screenshots and a clone of it was a second copy of every byte,
/// made at the moment the base64 copy of every byte was about to be made as well.
struct Held {
    id: String,
    frames: Vec<Arc<[u8]>>,
    width: i32,
    height: i32,
    weighs: usize,
}

/// How many pictures are held before the oldest is dropped.
const SHOTS_KEPT: usize = 40;
/// And how much they may weigh together, whichever runs out first.
const SHOTS_WEIGH: usize = 48 * 1024 * 1024;

impl MarkShots {
    pub(crate) fn keep(&self, shot: Shot) -> Result<(), String> {
        let mut held = self.held()?;
        held.retain(|kept| kept.id != shot.id);
        held.push(Held {
            weighs: shot.weighs(),
            id: shot.id,
            frames: shot.frames.into_iter().map(Arc::from).collect(),
            width: shot.width,
            height: shot.height,
        });
        while held.len() > SHOTS_KEPT
            || (held.len() > 1 && held.iter().map(|kept| kept.weighs).sum::<usize>() > SHOTS_WEIGH)
        {
            held.remove(0);
        }
        Ok(())
    }

    /// The pictures for these marks, in the order asked for, skipping any already gone.
    /// The shots for these ids, each carrying the position it was asked for at.
    ///
    /// The position travels because a shot can have aged out — `SHOTS_KEPT` is forty and
    /// `SHOTS_WEIGH` is 48 MB, and a handful of recordings evicts earlier marks that are
    /// still ticked in the composer. Numbering the survivors instead would mean the
    /// message says "2. Arrow (mark-2.png)" while `mark-2.png` is mark three's picture.
    pub(crate) fn pick(&self, ids: &[String]) -> Result<Vec<Picked>, String> {
        let held = self.held()?;
        Ok(ids
            .iter()
            .enumerate()
            .filter_map(|(asked_at, id)| {
                held.iter().find(|shot| &shot.id == id).map(|shot| Picked {
                    asked_at,
                    id: shot.id.clone(),
                    // A clone of the `Arc`s, not of the pictures behind them.
                    frames: shot.frames.clone(),
                    width: shot.width,
                    height: shot.height,
                })
            })
            .collect())
    }

    pub(crate) fn forget(&self, ids: &[String]) -> Result<(), String> {
        self.held()?.retain(|shot| !ids.contains(&shot.id));
        Ok(())
    }

    fn held(&self) -> Result<std::sync::MutexGuard<'_, Vec<Held>>, String> {
        self.0
            .lock()
            .map_err(|_| "The pictures taken for your marks are unavailable.".to_string())
    }
}

/// What the page learns about a picture: enough to show it, never the picture itself.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Taken {
    pub id: String,
    /// A small PNG as a data URL, sized for a row in a list.
    pub thumb: String,
    pub width: i32,
    pub height: i32,
    /// What colour was under the point, for the one tool that asks.
    pub hex: Option<String>,
    /// How many pictures were taken. One for everything but a recording.
    pub frames: usize,
    /// How long they cover, which is the count times the gap and not what was asked
    /// for: a very short recording still samples at the fastest rate it has.
    pub seconds: f64,
}

/// Whether the recording running right now has been asked to stop.
///
/// A recording is the one thing this toolbar does that somebody has to wait out. Fifteen
/// seconds is a long time to watch a countdown you started by mistake, and there was no
/// way to end it: Escape reached a page that was mid-capture and could do nothing with
/// it, so the only way out was to wait or to kill the toolbar.
///
/// One flag rather than a channel, because there is only ever one recording — the page
/// makes itself invisible and blocks until the frames are in, so a second cannot start.
static CUT_SHORT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Stop the recording that is running, keeping the frames it already has.
///
/// Not a cancel: what has been photographed is what somebody was recording, and throwing
/// it away would make the button that ends a recording early indistinguishable from one
/// that abandons it.
#[tauri::command]
pub(crate) fn colai_cut_recording() {
    CUT_SHORT.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Photograph what a mark is about.
///
/// The page makes itself invisible before calling this and visible again after, which is
/// why nothing here touches the window: hiding an always-on-top window and showing it
/// again asks the window manager for a favour, and it usually answers by taking the
/// keyboard away from whatever somebody was typing into. A transparent window composites
/// to nothing, so the picture comes out clean without the overlay ever changing state.
#[tauri::command]
pub(crate) async fn colai_capture_mark(
    app: AppHandle,
    mark: Mark,
    accent: Option<String>,
    // How long a recording should cover. Ignored by every other tool.
    seconds: Option<f64>,
) -> Result<Taken, String> {
    let window = app
        .get_webview_window(crate::colai::OVERLAY_LABEL)
        .ok_or_else(|| "The toolbar is not open.".to_string())?;
    let size = window
        .inner_size()
        .map_err(|error| format!("Could not measure the overlay: {error}"))?;
    let at = window
        .outer_position()
        .map_err(|error| format!("Could not find the overlay: {error}"))?;
    let width = size.width as i32;
    let height = size.height as i32;
    let crop = crop_for(&mark, width, height)
        .ok_or_else(|| "There is nothing inside that mark to photograph.".to_string())?;
    let within = points_within(&mark, crop, width, height);
    let drawn = drawn_as(&mark).map(str::to_string);
    // The Control UI's own accent, and the third place this colour used to be written
    // out by hand. The page always sends one now, so this is the answer for a caller
    // that does not — not a second opinion about what colai looks like.
    let accent = accent.unwrap_or_else(|| "#ff5c5c".to_string());

    // Which thread each frame is taken on is the platform's business — see `take_frame`.
    // Where to read a colour from, if this mark is asking for one: the point it was
    // made at, in the picture's own coordinates.
    let sampled = (mark.tool == "colour")
        .then(|| within.first().copied())
        .flatten();
    // A recording is the same photograph taken several times. Nothing else about it is
    // special, which is why it costs a loop rather than a second way of taking pictures.
    let every = record_every(seconds.unwrap_or(2.0));
    let wanted = if mark.tool == "record" {
        RECORD_FRAMES
    } else {
        1
    };
    let mut frames: Vec<Vec<u8>> = Vec::with_capacity(wanted);
    let mut hex = None;
    // The first frame's thumbnail, where the platform could make it from pixels already
    // in hand. Linux leaves it empty and it is made from the PNG below, as it always was.
    let mut thumb = None;
    // The size of the picture that actually goes, which is not the size of the region
    // asked for: a capture wider than an agent will take is shrunk on the way out, and
    // claiming the region's size would describe an image nobody has.
    let mut sent = (crop.width, crop.height);
    // Cleared here rather than when the recording ends, so a stop that arrives after the
    // last frame cannot cut the next recording short before it has taken one.
    CUT_SHORT.store(false, std::sync::atomic::Ordering::SeqCst);
    for taken in 0..wanted {
        if taken > 0 {
            // Asked between frames, which is where the waiting is. At least one frame is
            // always taken: a recording that returns nothing is a failure, and this is a
            // person saying "that is enough", not "that was a mistake".
            if CUT_SHORT.swap(false, std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(every).await;
        }
        let frame = take_frame(
            &app,
            (crop.x + at.x, crop.y + at.y, crop.width, crop.height),
            (within.clone(), drawn.clone(), accent.clone()),
            sampled,
            taken == 0,
        )
        .await?;
        sent = frame.size;
        // The colour is whatever was there when the first frame was taken. Asking again
        // on every frame would answer with whichever one happened to be last.
        if taken == 0 {
            hex = frame.hex;
            thumb = frame.thumb;
        }
        frames.push(frame.png);
    }

    let first = frames
        .first()
        .ok_or_else(|| "Nothing was photographed.".to_string())?;
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let thumb = {
        let _ = first;
        thumb.ok_or_else(|| "Nothing was photographed.".to_string())?
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let thumb = match thumb {
        Some(thumb) => thumb,
        None => thumbnail(first)?,
    };
    let shots = app.state::<MarkShots>();
    let held = frames.len();
    shots.keep(Shot {
        id: mark.id.clone(),
        frames,
        width: sent.0,
        height: sent.1,
    })?;
    Ok(Taken {
        id: mark.id,
        thumb,
        width: sent.0,
        height: sent.1,
        hex,
        frames: held,
        // What the frames actually cover, which is the gaps between them. Said rather
        // than the length that was asked for, because the floor on the interval can
        // stretch a very short recording past it, and an agent timing a change off this
        // number has to be able to trust it.
        seconds: if wanted > 1 {
            ((wanted - 1) as f64 * every.as_millis() as f64) / 1000.0
        } else {
            0.0
        },
    })
}

/// One photograph, finished: the PNG that is kept, the colour asked about, the size it
/// came out at, and — for the first frame, where the platform can make it from pixels
/// already in hand — the thumbnail the page shows.
struct Frame {
    png: Vec<u8>,
    hex: Option<String>,
    size: (i32, i32),
    thumb: Option<String>,
}

/// What is drawn on a frame: the mark's points within the crop, its shape, its colour.
type Ink = (Vec<(f64, f64)>, Option<String>, String);

/// Take one frame on the thread GDK insists on.
///
/// GDK belongs to the main thread, so the grab, the drawing and the encode all hop there,
/// and the thumbnail is made from the PNG afterwards. The platforms with no capture at all
/// come through here too, to be told so by their `picture_of`.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
async fn take_frame(
    app: &AppHandle,
    region: (i32, i32, i32, i32),
    ink: Ink,
    sampled: Option<(f64, f64)>,
    _thumb: bool,
) -> Result<Frame, String> {
    let (done, wait) = std::sync::mpsc::channel();
    let (within, drawn, accent) = ink;
    app.run_on_main_thread(move || {
        let _ = done.send(picture_of(region, &within, drawn.as_deref(), &accent, sampled));
    })
    .map_err(|error| format!("Could not reach the display: {error}"))?;
    let (png, hex, size) = wait
        .recv()
        .map_err(|_| "The display did not answer.".to_string())??;
    Ok(Frame {
        png,
        hex,
        size,
        thumb: None,
    })
}

/// Take one frame on Windows, nowhere near the main thread.
///
/// GDI does not care which thread asks it for the screen, so nothing here needs the main
/// thread at all — and the main thread is the one that paints the toolbar and answers
/// every click. The shrink and the PNG encode are the expensive half, sixty to a hundred
/// and eighty milliseconds a frame on a full screen and eight times that for a recording,
/// and they used to run there: the whole toolbar stalled for each one. Now the grab, the
/// drawing, the shrink and the encode run on a blocking worker, and the thumbnail is made
/// from the shrunk pixels rather than by decoding the PNG that was just encoded from them.
#[cfg(target_os = "windows")]
async fn take_frame(
    _app: &AppHandle,
    region: (i32, i32, i32, i32),
    ink: Ink,
    sampled: Option<(f64, f64)>,
    thumb: bool,
) -> Result<Frame, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (x, y, width, height) = region;
        finished(win_capture::grab(x, y, width, height)?, &ink, sampled, thumb)
    })
    .await
    .map_err(|_| "The photograph was abandoned before it was finished.".to_string())?
}

/// Take one frame on macOS: the grab where it always was, everything after it elsewhere.
///
/// The grab stays on the main thread, because asking for Screen Recording access can put
/// a system prompt on screen and that is AppKit's to do from the main thread. Once the
/// pixels are in hand, though, the drawing, the shrink and the encode are pure Rust with
/// no reason to hold up the toolbar — so they go to a blocking worker, as on Windows.
#[cfg(target_os = "macos")]
async fn take_frame(
    app: &AppHandle,
    region: (i32, i32, i32, i32),
    ink: Ink,
    sampled: Option<(f64, f64)>,
    thumb: bool,
) -> Result<Frame, String> {
    let (done, wait) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let (x, y, width, height) = region;
        let _ = done.send(mac_capture::grab(x, y, width, height));
    })
    .map_err(|error| format!("Could not reach the display: {error}"))?;
    let shot = wait
        .recv()
        .map_err(|_| "The display did not answer.".to_string())??;
    tauri::async_runtime::spawn_blocking(move || finished(shot, &ink, sampled, thumb))
        .await
        .map_err(|_| "The photograph was abandoned before it was finished.".to_string())?
}

#[cfg(target_os = "linux")]
fn picture_of(
    at: (i32, i32, i32, i32),
    within: &[(f64, f64)],
    drawn: Option<&str>,
    accent: &str,
    sampled: Option<(f64, f64)>,
) -> Result<(Vec<u8>, Option<String>, (i32, i32)), String> {
    use gdk::cairo;
    use gdk::prelude::*;

    let (x, y, width, height) = at;
    let root = gdk::Screen::default()
        .and_then(|screen| screen.root_window())
        .ok_or_else(|| "There is no display to photograph.".to_string())?;
    let taken = root
        .pixbuf(x, y, width, height)
        .ok_or_else(|| "The display would not give up that region.".to_string())?;

    // Read before anything is drawn on it. A mark painted over the very pixel being
    // asked about would answer with the colour of the mark.
    let hex = sampled.and_then(|(x, y)| pixel_at(&taken, x, y));

    let Some(drawn) = drawn else {
        let small = shrunk(&taken)?;
        return Ok((encode(&small)?, hex, (small.width(), small.height())));
    };

    let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, width, height)
        .map_err(|error| format!("Could not prepare the picture: {error}"))?;
    let ink = cairo::Context::new(&surface)
        .map_err(|error| format!("Could not draw on the picture: {error}"))?;
    ink.set_source_pixbuf(&taken, 0.0, 0.0);
    ink.paint()
        .map_err(|error| format!("Could not copy the region: {error}"))?;
    draw_mark(&ink, drawn, within, accent)?;
    drop(ink);

    let marked = gdk::pixbuf_get_from_surface(&surface, 0, 0, width, height)
        .ok_or_else(|| "Could not read the marked picture back.".to_string())?;
    let small = shrunk(&marked)?;
    Ok((encode(&small)?, hex, (small.width(), small.height())))
}

/// The colour of one pixel, as the six digits somebody would paste into a stylesheet.
///
/// Straight out of the picture already taken rather than a second look at the screen:
/// two reads a moment apart can disagree, and the answer has to be the colour that is
/// in the image the agent is being shown.
#[cfg(target_os = "linux")]
fn pixel_at(pixbuf: &gdk::gdk_pixbuf::Pixbuf, x: f64, y: f64) -> Option<String> {
    let (across, down) = (x.round() as i32, y.round() as i32);
    if across < 0 || down < 0 || across >= pixbuf.width() || down >= pixbuf.height() {
        return None;
    }
    let channels = pixbuf.n_channels();
    if channels < 3 {
        return None;
    }
    let bytes = pixbuf.read_pixel_bytes();
    let at = (down * pixbuf.rowstride() + across * channels) as usize;
    let pixel = bytes.get(at..at + 3)?;
    Some(format!("#{:02x}{:02x}{:02x}", pixel[0], pixel[1], pixel[2]))
}

/// Put the mark on the picture, twice.
///
/// A single stroke in one colour disappears against whatever it happens to land on — a
/// red circle over a red button is not an annotation. The dark pass underneath is what
/// makes the bright one legible on any background, which is the same trick the rail uses
/// to sit over an unknown desktop.
#[cfg(target_os = "linux")]
fn draw_mark(
    ink: &gdk::cairo::Context,
    drawn: &str,
    within: &[(f64, f64)],
    accent: &str,
) -> Result<(), String> {
    /*
     * Nothing to draw on is not a failure, it is nothing to draw.
     *
     * Every arm below reaches for `within[0]`, and the box and ellipse arms for
     * `within[1]`. This runs inside a closure GTK calls across an `extern "C"` boundary,
     * where an index panic does not unwind into a `Result` — it aborts, and the whole
     * toolbar leaves the screen mid-gesture. So the shapes that need two points are only
     * attempted when there are two.
     */
    let enough = match drawn {
        "box" | "ellipse" => within.len() >= 2,
        _ => !within.is_empty(),
    };
    if !enough {
        return Ok(());
    }
    let (red, green, blue) = colour_of(accent);
    let trace = |ink: &gdk::cairo::Context| match drawn {
        "box" => {
            let [(left, top), (right, bottom)] = [within[0], within[1]];
            ink.rectangle(left, top, right - left, bottom - top);
        }
        "ellipse" => {
            let [(left, top), (right, bottom)] = [within[0], within[1]];
            let (rx, ry) = ((right - left) / 2.0, (bottom - top) / 2.0);
            ink.save().ok();
            ink.translate(left + rx, top + ry);
            ink.scale(rx.max(0.5), ry.max(0.5));
            ink.arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU);
            ink.restore().ok();
        }
        "pin" => {
            let (x, y) = within[0];
            ink.arc(x, y, 13.0, 0.0, std::f64::consts::TAU);
        }
        "arrow" => {
            // Shaft and head in one path, so the dark outline behind it in the picture
            // follows both. A head with its own floating outline is worse than none.
            let [(x0, y0), (x1, y1)] = [within[0], within[within.len() - 1]];
            let (dx, dy) = (x1 - x0, y1 - y0);
            let long = dx.hypot(dy).max(1.0);
            let back = (long * ARROW_HEAD).clamp(ARROW_LEAST, ARROW_MOST);
            let (ux, uy) = (dx / long, dy / long);
            let (bx, by) = (x1 - ux * back, y1 - uy * back);
            let (sx, sy) = (-uy * back * ARROW_WIDE, ux * back * ARROW_WIDE);
            ink.move_to(x0, y0);
            ink.line_to(x1, y1);
            ink.move_to(bx + sx, by + sy);
            ink.line_to(x1, y1);
            ink.line_to(bx - sx, by - sy);
        }
        "line" => {
            let [(x0, y0), (x1, y1)] = [within[0], within[within.len() - 1]];
            ink.move_to(x0, y0);
            ink.line_to(x1, y1);
        }
        "span" => {
            // Here the ticks are worth drawing: this is real pixels, not the unit
            // square the page draws into, so square to the line really is square.
            let [(x0, y0), (x1, y1)] = [within[0], within[within.len() - 1]];
            let (dx, dy) = (x1 - x0, y1 - y0);
            let long = dx.hypot(dy).max(1.0);
            let (tx, ty) = (-dy / long * 7.0, dx / long * 7.0);
            ink.move_to(x0, y0);
            ink.line_to(x1, y1);
            ink.move_to(x0 - tx, y0 - ty);
            ink.line_to(x0 + tx, y0 + ty);
            ink.move_to(x1 - tx, y1 - ty);
            ink.line_to(x1 + tx, y1 + ty);
        }
        _ => {
            for (at, (x, y)) in within.iter().enumerate() {
                if at == 0 {
                    ink.move_to(*x, *y);
                } else {
                    ink.line_to(*x, *y);
                }
            }
        }
    };

    ink.set_line_cap(gdk::cairo::LineCap::Round);
    ink.set_line_join(gdk::cairo::LineJoin::Round);
    // A highlighter is not a line with an outline round it. It is meant to sit over
    // words and leave them readable, so it goes on once, wide and translucent, with no
    // dark halo — the halo is what makes every other mark legible against a busy screen
    // and is exactly what would make this one opaque.
    if drawn == "highlight" {
        ink.set_source_rgba(red, green, blue, HIGHLIGHT_THROUGH);
        ink.set_line_width(HIGHLIGHT_WIDE);
        trace(ink);
        return ink
            .stroke()
            .map_err(|error| format!("Could not draw the mark: {error}"));
    }
    ink.set_source_rgba(0.0, 0.0, 0.0, 0.45);
    ink.set_line_width(7.0);
    trace(ink);
    ink.stroke()
        .map_err(|error| format!("Could not outline the mark: {error}"))?;
    ink.set_source_rgb(red, green, blue);
    ink.set_line_width(3.5);
    trace(ink);
    ink.stroke()
        .map_err(|error| format!("Could not draw the mark: {error}"))?;
    Ok(())
}

/// `#rrggbb` as cairo takes it, falling back to the toolbar's own red.
fn colour_of(accent: &str) -> (f64, f64, f64) {
    let digits = accent.trim().trim_start_matches('#');
    if digits.len() == 6 {
        if let Ok(packed) = u32::from_str_radix(digits, 16) {
            return (
                f64::from((packed >> 16) & 0xff) / 255.0,
                f64::from((packed >> 8) & 0xff) / 255.0,
                f64::from(packed & 0xff) / 255.0,
            );
        }
    }
    (1.0, 0.42, 0.42)
}

#[cfg(target_os = "linux")]
fn encode(pixbuf: &gdk::gdk_pixbuf::Pixbuf) -> Result<Vec<u8>, String> {
    pixbuf
        .save_to_bufferv("png", &[])
        .map_err(|error| format!("Could not encode the picture: {error}"))
}

/// How wide a contact sheet is, and how many frames run across it.
///
/// The whole reason this exists. Eight frames of a full screen, sent as eight pictures,
/// cost about fifteen thousand image tokens — two hundred times what the entire message
/// around them costs, for a mark somebody made in one drag. The same eight laid out in a
/// grid cost under a thousand, and a model reads them *better*: the sequence is visible
/// at a glance instead of having to be reconstructed from eight unrelated images.
const SHEET_EDGE: i32 = 1600;
const SHEET_ACROSS: usize = 4;
/// The strip along the top of each cell that carries its number.
const SHEET_LABEL: f64 = 22.0;

/// A run of frames, laid out as one numbered picture, for whoever is sending it.
#[cfg(target_os = "linux")]
pub(crate) fn contact_sheet(frames: &[Arc<[u8]>], accent: &str) -> Result<Sheet, String> {
    sheet_of(frames, accent)
}

/// One mark's pictures, and where it sat in the list that asked for them.
///
/// The frames are the store's own, shared — see `Held` for why they are not a copy.
pub(crate) struct Picked {
    pub asked_at: usize,
    pub id: String,
    pub frames: Vec<Arc<[u8]>>,
    pub width: i32,
    pub height: i32,
}

/// A contact sheet and the size it came out at.
///
/// The size travels with it because the attachment needs it. It used to be sent as
/// `(0, 0)` — a real picture described as having no dimensions, which is the kind of
/// sentinel that reads as "unknown" to everything downstream.
#[cfg(target_os = "linux")]
pub(crate) struct Sheet {
    pub png: Vec<u8>,
    pub width: i32,
    pub height: i32,
}

/// A run of frames, laid out as one numbered picture.
///
/// Numbered because the message says "frame 3" and that has to mean something; laid out
/// left to right and top row first, which is the order the message states rather than
/// the order anybody should have to infer.
#[cfg(target_os = "linux")]
fn sheet_of(frames: &[Arc<[u8]>], accent: &str) -> Result<Sheet, String> {
    use gdk::cairo;
    use gdk::prelude::*;

    let cells: Vec<gdk::gdk_pixbuf::Pixbuf> = frames
        .iter()
        .map(|png| decode(png))
        .collect::<Result<_, _>>()?;
    let first = cells
        .first()
        .ok_or_else(|| "There are no frames.".to_string())?;
    let (wide, high) = (first.width().max(1), first.height().max(1));

    let grid = sheet_grid(cells.len(), wide, high);
    let (across, cell_high, scale, label) = (grid.across, grid.cell_high, grid.scale, grid.label);
    let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, grid.width, grid.height)
        .map_err(|error| format!("Could not prepare the sheet: {error}"))?;
    let ink = cairo::Context::new(&surface)
        .map_err(|error| format!("Could not draw the sheet: {error}"))?;
    ink.scale(scale, scale);
    // The ground between cells, so a frame with a pale edge is still a frame with an
    // edge rather than the one beside it.
    ink.set_source_rgb(0.08, 0.08, 0.09);
    ink.paint()
        .map_err(|error| format!("Could not lay the sheet's ground: {error}"))?;

    let (red, green, blue) = colour_of(accent);
    ink.select_font_face(
        "sans-serif",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Bold,
    );
    ink.set_font_size(label * 0.72);
    for (at, cell) in cells.iter().enumerate() {
        let x = (at % across) as f64 * f64::from(wide);
        let y = (at / across) as f64 * cell_high;
        ink.set_source_rgb(red, green, blue);
        ink.move_to(x + label * 0.3, y + label * 0.76);
        ink.show_text(&format!("{}", at + 1))
            .map_err(|error| format!("Could not number a frame: {error}"))?;
        ink.set_source_pixbuf(cell, x, y + label);
        ink.paint()
            .map_err(|error| format!("Could not place a frame: {error}"))?;
    }
    drop(ink);

    let sheet = gdk::pixbuf_get_from_surface(&surface, 0, 0, surface.width(), surface.height())
        .ok_or_else(|| "Could not read the sheet back.".to_string())?;
    Ok(Sheet {
        width: sheet.width(),
        height: sheet.height(),
        png: encode(&sheet)?,
    })
}

/// How a run of frames is laid out, before anything is drawn.
///
/// Its own function because it is the half that can be wrong quietly: a sheet whose cells
/// overlap, or one that comes out bigger than the pictures it replaced, has spent the
/// saving and produced a worse image than it started with. Arithmetic can be tested
/// anywhere; the drawing needs a display.
// Only drawn on Linux, but arithmetic the tests exercise on every platform, so it is
// silenced rather than cfg-gated away from the test build.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SheetGrid {
    pub across: usize,
    pub down: usize,
    pub cell_high: f64,
    /// The strip above each frame, in the sheet's own units — already enlarged so that
    /// it comes out the same size on every sheet once the scale has been applied.
    pub label: f64,
    pub scale: f64,
    pub width: i32,
    pub height: i32,
}

#[allow(dead_code)]
pub(crate) fn sheet_grid(count: usize, wide: i32, high: i32) -> SheetGrid {
    let across = SHEET_ACROSS.min(count.max(1));
    let down = count.max(1).div_ceil(across);
    let (wide, high) = (f64::from(wide.max(1)), f64::from(high.max(1)));
    // The scale comes from the frames alone, and the label is added afterwards at a size
    // that survives it. Sized with the frames instead, a number on a sheet of eight full
    // screens lands at four pixels — drawn, paid for, and unreadable, which is the one
    // outcome worse than not numbering them at all.
    let scale = (f64::from(SHEET_EDGE) / (across as f64 * wide).max(down as f64 * high)).min(1.0);
    let label = SHEET_LABEL / scale;
    let cell_high = high + label;
    let full = (across as f64 * wide, down as f64 * cell_high);
    SheetGrid {
        across,
        down,
        cell_high,
        label,
        scale,
        width: ((full.0 * scale) as i32).max(1),
        height: ((full.1 * scale) as i32).max(1),
    }
}

/// A PNG, back as pixels.
#[cfg(target_os = "linux")]
fn decode(png: &[u8]) -> Result<gdk::gdk_pixbuf::Pixbuf, String> {
    use gdk::gdk_pixbuf::PixbufLoader;
    use gdk::prelude::PixbufLoaderExt;

    let loader = PixbufLoader::new();
    loader
        .write(png)
        .map_err(|error| format!("Could not read a frame back: {error}"))?;
    loader
        .close()
        .map_err(|error| format!("Could not finish reading a frame: {error}"))?;
    loader
        .pixbuf()
        .ok_or_else(|| "A frame came back empty.".to_string())
}

/// A picture brought under the size an agent will accept, or left alone if it already is.
#[cfg(target_os = "linux")]
fn shrunk(pixbuf: &gdk::gdk_pixbuf::Pixbuf) -> Result<gdk::gdk_pixbuf::Pixbuf, String> {
    let (width, height) = (pixbuf.width().max(1), pixbuf.height().max(1));
    let longest = width.max(height);
    if longest <= SHOT_EDGE {
        return Ok(pixbuf.clone());
    }
    let scale = f64::from(SHOT_EDGE) / f64::from(longest);
    pixbuf
        .scale_simple(
            ((f64::from(width) * scale) as i32).max(1),
            ((f64::from(height) * scale) as i32).max(1),
            gdk::gdk_pixbuf::InterpType::Bilinear,
        )
        .ok_or_else(|| "Could not shrink the picture to a size an agent will take.".to_string())
}

/// A small copy of a picture, as a data URL the page can put straight in an `img`.
#[cfg(target_os = "linux")]
fn thumbnail(png: &[u8]) -> Result<String, String> {
    use base64::Engine as _;
    use gdk::gdk_pixbuf::{InterpType, Pixbuf, PixbufLoader};
    use gdk::prelude::PixbufLoaderExt;

    let loader = PixbufLoader::new();
    loader
        .write(png)
        .map_err(|error| format!("Could not read the picture back: {error}"))?;
    loader
        .close()
        .map_err(|error| format!("Could not finish reading the picture: {error}"))?;
    let full: Pixbuf = loader
        .pixbuf()
        .ok_or_else(|| "The picture came back empty.".to_string())?;
    let (width, height) = (full.width().max(1), full.height().max(1));
    let scale = f64::from(THUMB_EDGE) / f64::from(width.max(height));
    let small = if scale < 1.0 {
        full.scale_simple(
            ((f64::from(width) * scale) as i32).max(1),
            ((f64::from(height) * scale) as i32).max(1),
            InterpType::Bilinear,
        )
        .ok_or_else(|| "Could not shrink the picture.".to_string())?
    } else {
        full
    };
    let bytes = encode(&small)?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn picture_of(
    _at: (i32, i32, i32, i32),
    _within: &[(f64, f64)],
    _drawn: Option<&str>,
    _accent: &str,
    _sampled: Option<(f64, f64)>,
) -> Result<(Vec<u8>, Option<String>, (i32, i32)), String> {
    Err("Marking the screen is only built for Linux, Windows and macOS so far.".to_string())
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn thumbnail(_png: &[u8]) -> Result<String, String> {
    Err("Marking the screen is only built for Linux, Windows and macOS so far.".to_string())
}

/// Everything below is the Windows and macOS capture path: GDI or CoreGraphics for the
/// pixels, `tiny-skia` for the mark drawn onto them, and the `image` crate for the PNG and
/// the shrink. It stands in for the gdk-pixbuf and cairo functions above — `finished` for
/// `picture_of`, then `draw_mark_win`, `thumbnail`, `pixel_at`, `shrunk`, `encode` — so
/// the caller in `take` does not know or care which platform answered.
///
/// Only the grab differs between the two. Once the screen is an `RgbaImage` of the size asked
/// for, every later step is pure Rust and the same code on either, which is why a mark looks
/// the same whichever of them photographed it — and why none of it has to run on the main
/// thread. `draw_mark_win` keeps the name it had when Windows was the only one drawing this way.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn finished(
    shot: image::RgbaImage,
    ink: &Ink,
    sampled: Option<(f64, f64)>,
    thumb: bool,
) -> Result<Frame, String> {
    let (within, drawn, accent) = ink;
    // The sampled colour is read before anything is drawn over it — the same order the X11
    // path keeps, so the answer is the pixel that was there, not a mark's colour.
    let hex = sampled.and_then(|(px, py)| pixel_at(&shot, px, py));
    let shot = match drawn {
        Some(drawn) => draw_mark_win(shot, drawn, within, accent)?,
        None => shot,
    };
    let small = shrunk(shot);
    let thumb = if thumb { Some(thumbnail(&small)?) } else { None };
    Ok(Frame {
        png: encode(&small)?,
        hex,
        size: (small.width() as i32, small.height() as i32),
        thumb,
    })
}

/// Put the mark on the picture, the same way and the same two passes the cairo version does.
///
/// A dark stroke underneath and the bright one over it, so the mark is legible on any
/// background — except the highlighter, which is meant to sit over words and leave them
/// readable, so it goes on once, wide and translucent, with no halo. The geometry of every
/// shape is copied from `draw_mark` above, point for point, so a mark looks the same
/// whichever platform photographed it.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn draw_mark_win(
    shot: image::RgbaImage,
    drawn: &str,
    within: &[(f64, f64)],
    accent: &str,
) -> Result<image::RgbaImage, String> {
    use tiny_skia::{Color, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

    // The same guard as the cairo path: shapes that need two points are only drawn when
    // there are two, and an empty mark draws nothing. Returning the untouched capture is
    // right — the picture is still the ground the mark was made over.
    let enough = match drawn {
        "box" | "ellipse" => within.len() >= 2,
        _ => !within.is_empty(),
    };
    if !enough {
        return Ok(shot);
    }

    let (width, height) = (shot.width(), shot.height());
    let mut pixmap = Pixmap::new(width, height)
        .ok_or_else(|| "Could not prepare the picture to draw on.".to_string())?;
    // The capture is opaque straight RGBA, and tiny-skia stores premultiplied RGBA — the
    // same bytes while alpha is 255. So the capture copies straight in and, because the
    // ground stays opaque under the blend, reads straight back out after.
    pixmap.data_mut().copy_from_slice(shot.as_raw());

    let f = |v: f64| v as f32;
    let mut pb = PathBuilder::new();
    match drawn {
        "box" => {
            let [(left, top), (right, bottom)] = [within[0], within[1]];
            let rect = Rect::from_ltrb(
                f(left.min(right)),
                f(top.min(bottom)),
                f(left.max(right)),
                f(top.max(bottom)),
            )
            .ok_or_else(|| "The box has no area.".to_string())?;
            pb.push_rect(rect);
        }
        "ellipse" => {
            let [(left, top), (right, bottom)] = [within[0], within[1]];
            let rect = Rect::from_ltrb(
                f(left.min(right)),
                f(top.min(bottom)),
                f(left.max(right)),
                f(top.max(bottom)),
            )
            .ok_or_else(|| "The ellipse has no area.".to_string())?;
            pb.push_oval(rect);
        }
        "pin" => {
            let (x, y) = within[0];
            pb.push_circle(f(x), f(y), 13.0);
        }
        "arrow" => {
            // Shaft and head in one path, so the dark outline follows both — a head with
            // its own floating outline is worse than none.
            let [(x0, y0), (x1, y1)] = [within[0], within[within.len() - 1]];
            let (dx, dy) = (x1 - x0, y1 - y0);
            let long = dx.hypot(dy).max(1.0);
            let back = (long * ARROW_HEAD).clamp(ARROW_LEAST, ARROW_MOST);
            let (ux, uy) = (dx / long, dy / long);
            let (bx, by) = (x1 - ux * back, y1 - uy * back);
            let (sx, sy) = (-uy * back * ARROW_WIDE, ux * back * ARROW_WIDE);
            pb.move_to(f(x0), f(y0));
            pb.line_to(f(x1), f(y1));
            pb.move_to(f(bx + sx), f(by + sy));
            pb.line_to(f(x1), f(y1));
            pb.line_to(f(bx - sx), f(by - sy));
        }
        "line" => {
            let [(x0, y0), (x1, y1)] = [within[0], within[within.len() - 1]];
            pb.move_to(f(x0), f(y0));
            pb.line_to(f(x1), f(y1));
        }
        "span" => {
            let [(x0, y0), (x1, y1)] = [within[0], within[within.len() - 1]];
            let (dx, dy) = (x1 - x0, y1 - y0);
            let long = dx.hypot(dy).max(1.0);
            let (tx, ty) = (-dy / long * 7.0, dx / long * 7.0);
            pb.move_to(f(x0), f(y0));
            pb.line_to(f(x1), f(y1));
            pb.move_to(f(x0 - tx), f(y0 - ty));
            pb.line_to(f(x0 + tx), f(y0 + ty));
            pb.move_to(f(x1 - tx), f(y1 - ty));
            pb.line_to(f(x1 + tx), f(y1 + ty));
        }
        _ => {
            for (at, (x, y)) in within.iter().enumerate() {
                if at == 0 {
                    pb.move_to(f(*x), f(*y));
                } else {
                    pb.line_to(f(*x), f(*y));
                }
            }
        }
    }
    let path = pb
        .finish()
        .ok_or_else(|| "The mark did not describe a path.".to_string())?;

    let (red, green, blue) = colour_of(accent);
    let to8 = |c: f64| (c * 255.0).round() as u8;
    let stroke_with = |pixmap: &mut Pixmap, color: Color, wide: f32| {
        let mut paint = Paint::default();
        paint.set_color(color);
        paint.anti_alias = true;
        let stroke = Stroke {
            width: wide,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    };

    if drawn == "highlight" {
        stroke_with(
            &mut pixmap,
            Color::from_rgba8(to8(red), to8(green), to8(blue), to8(HIGHLIGHT_THROUGH)),
            HIGHLIGHT_WIDE as f32,
        );
    } else {
        stroke_with(&mut pixmap, Color::from_rgba8(0, 0, 0, 115), 7.0);
        stroke_with(
            &mut pixmap,
            Color::from_rgba8(to8(red), to8(green), to8(blue), 255),
            3.5,
        );
    }

    image::RgbaImage::from_raw(width, height, pixmap.data().to_vec())
        .ok_or_else(|| "The drawn picture did not fit its own size.".to_string())
}

/// The colour of one pixel, as the six digits somebody would paste into a stylesheet —
/// read straight out of the picture already taken, for the reason the X11 one gives.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn pixel_at(shot: &image::RgbaImage, x: f64, y: f64) -> Option<String> {
    let (across, down) = (x.round() as i32, y.round() as i32);
    if across < 0 || down < 0 || across >= shot.width() as i32 || down >= shot.height() as i32 {
        return None;
    }
    let pixel = shot.get_pixel(across as u32, down as u32);
    Some(format!("#{:02x}{:02x}{:02x}", pixel[0], pixel[1], pixel[2]))
}

/// A picture brought under the size an agent will accept, or left alone if it already is.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn shrunk(shot: image::RgbaImage) -> image::RgbaImage {
    let (width, height) = (shot.width().max(1), shot.height().max(1));
    let longest = width.max(height) as i32;
    if longest <= SHOT_EDGE {
        return shot;
    }
    let scale = f64::from(SHOT_EDGE) / f64::from(longest);
    let to = |side: u32| ((f64::from(side) * scale) as u32).max(1);
    // Lanczos over bilinear: the ceiling exists to keep interface text legible after the
    // shrink, and Lanczos is the filter that keeps small text readable rather than muddy.
    image::imageops::resize(&shot, to(width), to(height), image::imageops::FilterType::Lanczos3)
}

/// Pixels to PNG.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn encode(shot: &image::RgbaImage) -> Result<Vec<u8>, String> {
    use image::ImageEncoder as _;
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            shot.as_raw(),
            shot.width(),
            shot.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| format!("Could not encode the picture: {error}"))?;
    Ok(png)
}

/// A PNG, back as pixels. Only the round-trip test reads one back: the capture itself
/// never decodes what it has just encoded.
#[cfg(all(test, target_os = "windows"))]
fn decode(png: &[u8]) -> Result<image::RgbaImage, String> {
    image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .map_err(|error| format!("Could not read a frame back: {error}"))
        .map(|frame| frame.to_rgba8())
}

/// A small copy of a picture, as a data URL the page can put straight in an `img`.
///
/// Made from the shrunk pixels that are about to be encoded, not from the PNG once it
/// has been: decoding a picture this process encoded a moment ago is a second full pass
/// over a megabyte to recover what was already in hand. The pixels are the same either
/// way — PNG is lossless — so the thumbnail is too.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn thumbnail(full: &image::RgbaImage) -> Result<String, String> {
    use base64::Engine as _;
    let (width, height) = (full.width().max(1), full.height().max(1));
    let scale = f64::from(THUMB_EDGE) / f64::from(width.max(height));
    let png = if scale < 1.0 {
        let to = |side: u32| ((f64::from(side) * scale) as u32).max(1);
        encode(&image::imageops::resize(
            full,
            to(width),
            to(height),
            image::imageops::FilterType::Lanczos3,
        ))?
    } else {
        encode(full)?
    };
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    ))
}

/// The screen, into an image, through GDI.
///
/// A screen DC, a memory DC to blit into, a bitmap to hold the pixels, then `GetDIBits` to
/// read them out as 32-bit top-down rows — which arrive as BGRA and are turned into the
/// RGBA the `image` crate wants. Single-origin desktops are covered; a monitor placed to
/// the left of the primary (negative virtual coordinates) is a refinement left for later.
#[cfg(target_os = "windows")]
mod win_capture {
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS, SRCCOPY,
    };

    pub(super) fn grab(
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> Result<image::RgbaImage, String> {
        if width <= 0 || height <= 0 {
            return Err("There is no region to photograph.".to_string());
        }
        // SAFETY: every GDI handle created here is released before this returns, on both the
        // success and the failure paths, and none of them escapes the function.
        unsafe {
            let screen = GetDC(std::ptr::null_mut());
            if screen.is_null() {
                return Err("Could not open the screen to photograph it.".to_string());
            }
            let mem = CreateCompatibleDC(screen);
            let bitmap = CreateCompatibleBitmap(screen, width, height);
            if mem.is_null() || bitmap.is_null() {
                if !bitmap.is_null() {
                    DeleteObject(bitmap.cast());
                }
                if !mem.is_null() {
                    DeleteDC(mem);
                }
                ReleaseDC(std::ptr::null_mut(), screen);
                return Err("Could not prepare a surface to photograph onto.".to_string());
            }
            let prev = SelectObject(mem, bitmap.cast());
            let copied = BitBlt(mem, 0, 0, width, height, screen, x, y, SRCCOPY);

            // Top-down (negative height) 32-bit rows, so the buffer is BGRA from the
            // top-left corner — the layout the conversion below assumes.
            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = width;
            info.bmiHeader.biHeight = -height;
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            info.bmiHeader.biCompression = BI_RGB as u32;
            let mut buf = vec![0u8; width as usize * height as usize * 4];
            let rows = GetDIBits(
                mem,
                bitmap,
                0,
                height as u32,
                buf.as_mut_ptr().cast(),
                &mut info,
                DIB_RGB_COLORS,
            );

            SelectObject(mem, prev);
            DeleteObject(bitmap.cast());
            DeleteDC(mem);
            ReleaseDC(std::ptr::null_mut(), screen);

            if copied == 0 || rows == 0 {
                return Err("The screen would not give up that region.".to_string());
            }
            // GDI hands back BGRX; the image wants RGBA, opaque.
            for pixel in buf.chunks_exact_mut(4) {
                pixel.swap(0, 2);
                pixel[3] = 255;
            }
            image::RgbaImage::from_raw(width as u32, height as u32, buf)
                .ok_or_else(|| "The photographed region did not fit its own size.".to_string())
        }
    }
}

/// The screen, into an image, through CoreGraphics.
///
/// Three things make a Mac different from the GDI path above, and each has its own answer.
///
/// **Units.** Everything handed to `picture_of` is in Tauri's "physical" pixels, which on a
/// Mac are points times the display's backing scale. CoreGraphics names a region of the
/// screen in points. So the region is divided by the scale on the way in, and the picture
/// that comes back — at the panel's own resolution, which on a Retina display is twice the
/// points in each direction — is drawn into a canvas exactly the physical size asked for.
/// That last step is what keeps the mark's points, which are physical and relative to the
/// crop, landing on the pixels they were made over even if the two scales ever disagree.
///
/// **Colour.** The canvas is sRGB, so drawing into it converts from whatever the display
/// uses — Display P3 on most recent Macs. The pipette answers with a hex somebody will paste
/// into a stylesheet, and a stylesheet's hex means sRGB; the display's raw numbers would be a
/// colour that looks subtly wrong once it is used.
///
/// **Permission.** macOS will not show one app another app's windows without Screen
/// Recording access. Without it the call does not fail: it hands back the wallpaper and the
/// menu bar, which a mark would then be drawn over as if that were the screen. So access is
/// asked about first, and a missing one is a sentence that says where to grant it rather
/// than a picture of nothing. Asking also files the app in that list, so the switch is there
/// to turn on.
///
/// The overlay covers the main display only (see `desk_monitors` in `colai.rs`), whose
/// top-left corner is the origin of the global space this is asked in, so its own position
/// is the `(0, 0)` that the caller already adds.
#[cfg(target_os = "macos")]
mod mac_capture {
    use core_graphics::access::ScreenCaptureAccess;
    use core_graphics::base::{kCGBitmapByteOrder32Big, kCGImageAlphaPremultipliedLast};
    use core_graphics::color_space::{kCGColorSpaceSRGB, CGColorSpace};
    use core_graphics::context::{CGContext, CGInterpolationQuality};
    use core_graphics::display::CGDisplay;
    use core_graphics::geometry::{CGPoint, CGRect, CGSize};
    use core_graphics::window::{
        create_image, kCGNullWindowID, kCGWindowImageDefault, kCGWindowListOptionOnScreenOnly,
    };

    /// What somebody is told when the Mac will not show us the screen.
    const NOT_ALLOWED: &str = "colai is not allowed to photograph the screen. Turn it on in System Settings → Privacy & Security → Screen Recording (for the app colai was started from, such as your terminal), then quit and reopen the toolbar.";

    pub(super) fn grab(
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> Result<image::RgbaImage, String> {
        if width <= 0 || height <= 0 {
            return Err("There is no region to photograph.".to_string());
        }
        let access = ScreenCaptureAccess;
        if !access.preflight() {
            // The first time, this shows the system's own prompt; after that it returns at
            // once. Either way, the grant only takes effect for a process started after it,
            // which is why the sentence says to reopen.
            let _ = access.request();
            return Err(NOT_ALLOWED.to_string());
        }

        let scale = backing_scale();
        let points = CGRect::new(
            &CGPoint::new(f64::from(x) / scale, f64::from(y) / scale),
            &CGSize::new(f64::from(width) / scale, f64::from(height) / scale),
        );
        // Every window on screen, composited as it is shown. The overlay is among them,
        // but the page has already made itself invisible, and a transparent window adds
        // nothing to a composite — the same reasoning the GDI path relies on.
        let shot = create_image(
            points,
            kCGWindowListOptionOnScreenOnly,
            kCGNullWindowID,
            kCGWindowImageDefault,
        )
        .ok_or_else(|| NOT_ALLOWED.to_string())?;
        if shot.width() == 0 || shot.height() == 0 {
            return Err(NOT_ALLOWED.to_string());
        }

        let (wide, high) = (width as usize, height as usize);
        // SAFETY: `kCGColorSpaceSRGB` is an immutable CFString constant exported by
        // CoreGraphics; reading the pointer is all that happens here.
        let space = CGColorSpace::create_with_name(unsafe { kCGColorSpaceSRGB })
            .unwrap_or_else(CGColorSpace::create_device_rgb);
        // Eight bits a channel, rows packed tight, laid out R, G, B, A in memory — exactly
        // the bytes `RgbaImage` holds, so nothing is shuffled afterwards.
        let mut canvas = CGContext::create_bitmap_context(
            None,
            wide,
            high,
            8,
            wide * 4,
            &space,
            kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big,
        );
        let whole = CGRect::new(
            &CGPoint::new(0.0, 0.0),
            &CGSize::new(wide as f64, high as f64),
        );
        // An opaque ground first, so anything the composite left see-through comes out
        // black rather than as holes in a picture that is meant to be opaque.
        canvas.set_rgb_fill_color(0.0, 0.0, 0.0, 1.0);
        canvas.fill_rect(whole);
        // High quality, because on a scale that is not a whole number the picture is
        // resampled here, and interface text is what has to survive it.
        canvas.set_interpolation_quality(CGInterpolationQuality::CGInterpolationQualityHigh);
        canvas.draw_image(whole, &shot);

        // The bitmap's first row in memory is the top of the picture, whatever way up
        // CoreGraphics counts its own coordinates, so rows copy straight across.
        let stride = canvas.bytes_per_row();
        let data = canvas.data();
        let mut buf = Vec::with_capacity(wide * high * 4);
        for row in 0..high {
            let start = row * stride;
            buf.extend_from_slice(&data[start..start + wide * 4]);
        }
        // Premultiplied over an opaque ground is already straight colour; the alpha is
        // set rather than trusted, as the GDI path does.
        for pixel in buf.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        image::RgbaImage::from_raw(width as u32, height as u32, buf)
            .ok_or_else(|| "The photographed region did not fit its own size.".to_string())
    }

    /// How many physical pixels a point is on the main display — what AppKit calls the
    /// backing scale factor, and what Tauri multiplied by to produce the region's numbers.
    ///
    /// Read from the display mode rather than the window, because this runs with nothing
    /// but the region in hand. The mode's pixel width over its point width is the backing
    /// scale for every mode macOS offers, including the "looks like" scaled ones, which are
    /// drawn at twice their points and then fitted to the panel.
    fn backing_scale() -> f64 {
        CGDisplay::main()
            .display_mode()
            .and_then(|mode| {
                let (pixels, points) = (mode.pixel_width(), mode.width());
                (pixels > 0 && points > 0).then(|| pixels as f64 / points as f64)
            })
            .unwrap_or(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_store_drops_the_oldest_rather_than_growing_forever() {
        let shots = MarkShots::default();
        for at in 0..SHOTS_KEPT + 5 {
            shots
                .keep(Shot {
                    id: format!("m{at}"),
                    frames: vec![vec![0; 16]],
                    width: 10,
                    height: 10,
                })
                .unwrap();
        }
        let held = shots.0.lock().unwrap();
        assert_eq!(held.len(), SHOTS_KEPT);
        assert_eq!(held[0].id, "m5");
    }

    #[test]
    fn keeping_a_mark_twice_replaces_its_picture_rather_than_doubling_it() {
        let shots = MarkShots::default();
        for _ in 0..2 {
            shots
                .keep(Shot {
                    id: "m1".to_string(),
                    frames: vec![vec![7; 4]],
                    width: 1,
                    height: 1,
                })
                .unwrap();
        }
        assert_eq!(shots.0.lock().unwrap().len(), 1);
        assert_eq!(shots.pick(&["m1".to_string()]).unwrap().len(), 1);
        shots.forget(&["m1".to_string()]).unwrap();
        assert!(shots.pick(&["m1".to_string()]).unwrap().is_empty());
    }

    #[test]
    fn a_picked_shot_shares_the_stores_frames_rather_than_copying_them() {
        // A recording is eight screenshots. Picking it for a send used to copy every one;
        // now the send and the store hold the same bytes, and the store still has them
        // afterwards in case the send fails and is tried again.
        let shots = MarkShots::default();
        shots
            .keep(Shot {
                id: "m1".to_string(),
                frames: vec![vec![1; 64]; RECORD_FRAMES],
                width: 8,
                height: 8,
            })
            .unwrap();
        let once = shots.pick(&["m1".to_string()]).unwrap().remove(0);
        let again = shots.pick(&["m1".to_string()]).unwrap().remove(0);
        assert!(once.frames.iter().zip(&again.frames).all(|(a, b)| Arc::ptr_eq(a, b)));
        assert_eq!(once.frames.len(), RECORD_FRAMES);
    }

    #[test]
    fn a_run_of_frames_comes_out_the_size_of_one_picture() {
        // The whole point of the sheet. Eight frames of a screen cost about fifteen
        // thousand image tokens sent one by one; laid out in a grid they have to come out
        // around the size of a single picture, or nothing has been saved.
        let grid = sheet_grid(8, 1600, 900);
        assert_eq!((grid.across, grid.down), (4, 2));
        assert!(grid.width <= SHEET_EDGE && grid.height <= SHEET_EDGE);
        // A single 1600x900 picture is about 1.9 million pixels. The sheet of eight has
        // to be in that neighbourhood rather than eight times it.
        let pixels = i64::from(grid.width) * i64::from(grid.height);
        assert!(
            pixels < 1_600 * 900,
            "a sheet of eight came out at {pixels} pixels"
        );
    }

    #[test]
    fn a_sheet_gives_every_frame_room_for_its_own_number() {
        // The message says "frame 3" and that has to mean something, so each cell carries
        // a strip above it. A cell exactly as tall as its frame would put the number on
        // the picture.
        let grid = sheet_grid(4, 400, 300);
        assert!(grid.cell_high > 300.0);
        assert_eq!(grid.cell_high, 300.0 + grid.label);
    }

    #[test]
    fn a_frame_number_is_the_same_size_however_much_the_sheet_shrank() {
        // Sized with the frames, a number on a sheet of eight full screens lands at four
        // pixels: drawn, paid for, and unreadable. It is enlarged by exactly as much as
        // the sheet is about to be reduced.
        for (count, wide, high) in [(8, 1600, 900), (2, 300, 200), (5, 900, 600)] {
            let grid = sheet_grid(count, wide, high);
            let on_screen = grid.label * grid.scale;
            assert!(
                (on_screen - SHEET_LABEL).abs() < 0.001,
                "{count} frames of {wide}x{high} numbered at {on_screen}px"
            );
        }
    }

    #[test]
    fn a_short_run_does_not_leave_a_row_of_nothing() {
        // Two frames are two cells side by side, not two in a row of four with two holes.
        assert_eq!(
            (sheet_grid(2, 400, 300).across, sheet_grid(2, 400, 300).down),
            (2, 1)
        );
        assert_eq!(
            (sheet_grid(1, 400, 300).across, sheet_grid(1, 400, 300).down),
            (1, 1)
        );
        // And five wrap onto a second row rather than running off the first.
        assert_eq!(
            (sheet_grid(5, 400, 300).across, sheet_grid(5, 400, 300).down),
            (4, 2)
        );
    }

    #[test]
    fn a_sheet_never_grows_a_small_run() {
        // Scaling is a ceiling, not a target: two small frames stay their own size rather
        // than being blown up to fill a sheet nobody asked for.
        let grid = sheet_grid(2, 200, 150);
        assert_eq!(grid.scale, 1.0);
        assert_eq!(grid.width, 400);
    }

    #[test]
    fn a_longer_recording_spreads_the_same_frames_rather_than_adding_more() {
        // Fifty images is not a recording, it is a bill. The same eight frames spread
        // across whatever length was picked, and spread across the gaps between them so
        // the last one lands on the length rather than an interval short of it.
        // Within a few milliseconds, because the interval is whole milliseconds and
        // seven of them rarely divide a length exactly. The message rounds to a tenth
        // of a second, so this is under the resolution anybody reads it at.
        let covers =
            |seconds: f64| (RECORD_FRAMES - 1) as f64 * record_every(seconds).as_secs_f64();
        assert!((covers(2.0) - 2.0).abs() < 0.01, "{}", covers(2.0));
        assert!((covers(15.0) - 15.0).abs() < 0.01, "{}", covers(15.0));
        assert_eq!(record_every(2.0).as_millis(), 285);
        assert_eq!(record_every(15.0).as_millis(), 2142);
        // And nothing samples faster than the floor, however short the ask — so a very
        // brief recording covers more than it was asked for rather than blurring past
        // the thing it was pointed at.
        assert_eq!(record_every(0.1).as_millis(), 250);
    }

    #[test]
    fn a_run_of_frames_is_weighed_by_all_of_it() {
        // The budget is about memory, and a recording is six images under one name. A
        // store that counted the first frame would hold six times what it thinks.
        let run = Shot {
            id: "m1".to_string(),
            frames: vec![vec![0; 100]; RECORD_FRAMES],
            width: 10,
            height: 10,
        };
        assert_eq!(run.weighs(), 100 * RECORD_FRAMES);
    }

    #[test]
    fn an_accent_that_makes_no_sense_falls_back_to_the_toolbars_own() {
        assert_eq!(colour_of("#000000"), (0.0, 0.0, 0.0));
        assert_eq!(colour_of("ffffff"), (1.0, 1.0, 1.0));
        for nonsense in ["", "#12", "#gggggg", "rebeccapurple"] {
            assert_eq!(colour_of(nonsense), (1.0, 0.42, 0.42));
        }
    }

    /// The whole Windows capture pipeline, end to end: grab a real corner of the screen,
    /// encode it, read it back, and check it survived at the size asked for. This is the
    /// one part with no test on the arithmetic to fall back on — if GDI, the BGRA→RGBA
    /// swap, or the PNG round trip is wrong, marks arrive blank or the wrong size, and
    /// nothing above would catch it.
    #[cfg(target_os = "windows")]
    #[test]
    fn a_grabbed_region_survives_the_round_trip_at_its_own_size() {
        let shot = win_capture::grab(0, 0, 32, 24).expect("grab a corner of the screen");
        assert_eq!((shot.width(), shot.height()), (32, 24));
        // Alpha is forced opaque, so every pixel is fully drawn rather than see-through.
        assert!(shot.pixels().all(|p| p[3] == 255));

        let png = encode(&shot).expect("encode the capture");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "the bytes are a PNG");

        let back = decode(&png).expect("read the capture back");
        assert_eq!((back.width(), back.height()), (32, 24));

        // And the thumbnail is a data URL the page can drop straight into an `img`.
        let thumb = thumbnail(&back).expect("thumbnail the capture");
        assert!(thumb.starts_with("data:image/png;base64,"));
    }

    /// A frame finished off the main thread comes back whole: a PNG at the size it says,
    /// and a thumbnail only when one was asked for — the first frame of a recording, never
    /// the seven after it.
    #[cfg(target_os = "windows")]
    #[test]
    fn a_finished_frame_carries_its_thumbnail_only_when_asked() {
        let ink: Ink = (vec![(4.0, 4.0), (28.0, 20.0)], Some("box".to_string()), "#ff00ff".to_string());
        let first = finished(win_capture::grab(0, 0, 32, 24).expect("grab"), &ink, Some((1.0, 1.0)), true)
            .expect("finish the first frame");
        assert_eq!(first.size, (32, 24));
        assert_eq!(&first.png[..8], b"\x89PNG\r\n\x1a\n");
        assert!(first.hex.is_some(), "the sampled colour was read");
        assert!(first.thumb.expect("a thumbnail").starts_with("data:image/png;base64,"));
        let later = finished(win_capture::grab(0, 0, 32, 24).expect("grab"), &ink, None, false)
            .expect("finish a later frame");
        assert!(later.thumb.is_none());
    }

    /// Drawing a mark onto a capture keeps its size and changes its pixels — the accent
    /// stroke actually lands, rather than the picture coming back the ground it started as.
    #[cfg(target_os = "windows")]
    #[test]
    fn a_mark_drawn_on_a_capture_lands_on_it() {
        let shot = win_capture::grab(0, 0, 64, 48).expect("grab a corner of the screen");
        let before = shot.as_raw().to_vec();
        // A box across most of the region, in a colour unlikely to already be there.
        let within = [(6.0, 6.0), (58.0, 42.0)];
        let drawn = draw_mark_win(shot, "box", &within, "#ff00ff").expect("draw the box");
        assert_eq!((drawn.width(), drawn.height()), (64, 48));
        assert!(drawn.as_raw() != &before[..], "the mark changed no pixels");
        assert!(drawn.pixels().all(|p| p[3] == 255), "the picture stayed opaque");
        // An empty mark leaves the capture as it was rather than failing.
        let plain = win_capture::grab(0, 0, 8, 8).expect("grab a corner");
        let plain_before = plain.as_raw().to_vec();
        let untouched = draw_mark_win(plain, "pin", &[], "#ff0000").expect("nothing to draw");
        assert_eq!(untouched.as_raw(), &plain_before[..]);
    }
}
