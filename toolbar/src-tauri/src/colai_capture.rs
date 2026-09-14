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

use std::sync::Mutex;

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
/// Bounded here rather than at the send, because the Gateway refuses an image over 6MB
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
pub(crate) struct MarkShots(Mutex<Vec<Shot>>);

/// How many pictures are held before the oldest is dropped.
const SHOTS_KEPT: usize = 40;
/// And how much they may weigh together, whichever runs out first.
const SHOTS_WEIGH: usize = 48 * 1024 * 1024;

impl MarkShots {
    pub(crate) fn keep(&self, shot: Shot) -> Result<(), String> {
        let mut held = self.held()?;
        held.retain(|kept| kept.id != shot.id);
        held.push(shot);
        while held.len() > SHOTS_KEPT
            || (held.len() > 1 && held.iter().map(Shot::weighs).sum::<usize>() > SHOTS_WEIGH)
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

    fn held(&self) -> Result<std::sync::MutexGuard<'_, Vec<Shot>>, String> {
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

    // GDK belongs to the main thread. The page has already made itself invisible and is
    // waiting on this, so the hop is the only thing between the two.
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
    // The size of the picture that actually goes, which is not the size of the region
    // asked for: a capture wider than an agent will take is shrunk on the way out, and
    // telling the Gateway the region's size would describe an image nobody has.
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
        let (done, wait) = std::sync::mpsc::channel();
        let (within, drawn, accent) = (within.clone(), drawn.clone(), accent.clone());
        app.run_on_main_thread(move || {
            let _ = done.send(picture_of(
                (crop.x + at.x, crop.y + at.y, crop.width, crop.height),
                &within,
                drawn.as_deref(),
                &accent,
                sampled,
            ));
        })
        .map_err(|error| format!("Could not reach the display: {error}"))?;
        let (png, sampled_hex, size) = wait
            .recv()
            .map_err(|_| "The display did not answer.".to_string())??;
        sent = size;
        // The colour is whatever was there when the first frame was taken. Asking again
        // on every frame would answer with whichever one happened to be last.
        if taken == 0 {
            hex = sampled_hex;
        }
        frames.push(png);
    }

    let first = frames
        .first()
        .ok_or_else(|| "Nothing was photographed.".to_string())?;
    let thumb = thumbnail(first)?;
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

/// Forget the pictures for marks that were undone or have been sent.
#[tauri::command]
pub(crate) fn colai_forget_marks(
    shots: tauri::State<'_, MarkShots>,
    ids: Vec<String>,
) -> Result<(), String> {
    shots.forget(&ids)
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
pub(crate) fn contact_sheet(frames: &[Vec<u8>], accent: &str) -> Result<Sheet, String> {
    sheet_of(frames, accent)
}

/// One mark's pictures, and where it sat in the list that asked for them.
pub(crate) struct Picked {
    pub asked_at: usize,
    pub id: String,
    pub frames: Vec<Vec<u8>>,
    pub width: i32,
    pub height: i32,
}

/// A contact sheet and the size it came out at.
///
/// The size travels with it because the attachment needs it. It used to be sent as
/// `(0, 0)` — a real picture described as having no dimensions, which is the kind of
/// sentinel that reads as "unknown" to everything downstream.
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
fn sheet_of(frames: &[Vec<u8>], accent: &str) -> Result<Sheet, String> {
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

#[cfg(not(target_os = "linux"))]
fn picture_of(
    _at: (i32, i32, i32, i32),
    _within: &[(f64, f64)],
    _drawn: Option<&str>,
    _accent: &str,
    _sampled: Option<(f64, f64)>,
) -> Result<(Vec<u8>, Option<String>, (i32, i32)), String> {
    Err("Marking the screen is only built for Linux so far.".to_string())
}

#[cfg(not(target_os = "linux"))]
fn thumbnail(_png: &[u8]) -> Result<String, String> {
    Err("Marking the screen is only built for Linux so far.".to_string())
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
}
