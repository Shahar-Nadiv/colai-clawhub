//! What a mark *is*, and which pixels it turns out to be about.
//!
//! A mark arrives from the page as fractions of a screen: a shape, some points, and a
//! tool that says what was meant by them. Before anything can be photographed, three
//! questions have to be answered about it — where to crop, where its points fall inside
//! that crop, and whether the shape gets drawn back onto the picture. That is all this
//! is, and none of it needs a display, which is why it can be tested and why it lives
//! apart from the module that reaches for one.
//!
//! **The mark is drawn back on, sometimes.** A box or a circle survives being cropped —
//! the crop *is* the annotation. A pin has no size at all and a freehand stroke is a
//! shape rather than an area, so cropping either one produces a picture of some pixels
//! with no indication of what about them mattered. Those take the ground around them
//! and wear the mark. And a picture whose whole meaning is another picture beside it
//! wears nothing, because the one difference an agent could be certain of would be ours.

use serde::Deserialize;

/// A point on the overlay, in fractions of it.
#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) struct Spot {
    pub x: f64,
    pub y: f64,
}

/// A rectangle on the overlay, in fractions of it.
#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) struct Frame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Region {
    pub shape: String,
    /// `box` is the page's name for it and a keyword here.
    #[serde(rename = "box")]
    pub frame: Frame,
}

/// One thing somebody marked, exactly as the page holds it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Mark {
    pub id: String,
    pub tool: String,
    #[serde(default)]
    pub region: Option<Region>,
    #[serde(default)]
    pub points: Vec<Spot>,
    /// Which pen drew it, for the one tool that has a choice of them.
    #[serde(default)]
    pub pen: Option<String>,
}

/// A rectangle of the display, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Crop {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Room left around a region so the outline drawn on it is not clipped by its own edge.
const OUTLINE_ROOM: f64 = 12.0;
/// How much of the screen a point or a stroke takes with it, as a share of the short side.
const CONTEXT_SHARE: f64 = 0.14;
/// The least context worth taking, whatever the display is.
const CONTEXT_LEAST: f64 = 140.0;

/// Which pixels a mark is about.
///
/// A region is its own rectangle, grown by `OUTLINE_ROOM` so the outline drawn on it is
/// not clipped by the crop's own edge. A point or a stroke has no rectangle, so it takes
/// a share of the screen around it — enough that the picture says where it is, which a
/// tight crop of one pixel does not.
pub(crate) fn crop_for(mark: &Mark, width: i32, height: i32) -> Option<Crop> {
    if width <= 0 || height <= 0 {
        return None;
    }
    let (w, h) = (f64::from(width), f64::from(height));
    let Some((left, top, right, bottom)) = edges_of(mark) else {
        return Some(Crop {
            x: 0,
            y: 0,
            width,
            height,
        });
    };
    let room = if mark.region.is_some() {
        OUTLINE_ROOM
    } else {
        (w.min(h) * CONTEXT_SHARE).max(CONTEXT_LEAST)
    };
    let x = (left * w - room).max(0.0);
    let y = (top * h - room).max(0.0);
    let far_x = (right * w + room).min(w);
    let far_y = (bottom * h + room).min(h);
    // A region dragged entirely off the edge leaves nothing to photograph. Saying so is
    // better than handing GDK a zero-width rectangle and reading its complaint.
    if far_x - x < 1.0 || far_y - y < 1.0 {
        return None;
    }
    Some(Crop {
        x: x as i32,
        y: y as i32,
        width: (far_x - x) as i32,
        height: (far_y - y) as i32,
    })
}

/// A mark's extent in fractions: left, top, right, bottom.
fn edges_of(mark: &Mark) -> Option<(f64, f64, f64, f64)> {
    if let Some(region) = &mark.region {
        let frame = region.frame;
        return Some((frame.x, frame.y, frame.x + frame.w, frame.y + frame.h));
    }
    let first = mark.points.first()?;
    let mut edges = (first.x, first.y, first.x, first.y);
    for spot in &mark.points {
        edges.0 = edges.0.min(spot.x);
        edges.1 = edges.1.min(spot.y);
        edges.2 = edges.2.max(spot.x);
        edges.3 = edges.3.max(spot.y);
    }
    Some(edges)
}

/// A mark's own points, moved into the crop taken for it.
///
/// The page thinks in fractions of the whole overlay and the picture is a corner of it,
/// so nothing can be drawn until the two agree. Kept separate from the drawing so the
/// arithmetic can be tested without a display.
pub(crate) fn points_within(mark: &Mark, crop: Crop, width: i32, height: i32) -> Vec<(f64, f64)> {
    let (w, h) = (f64::from(width), f64::from(height));
    let (ox, oy) = (f64::from(crop.x), f64::from(crop.y));
    match &mark.region {
        Some(region) => {
            let frame = region.frame;
            vec![
                (frame.x * w - ox, frame.y * h - oy),
                ((frame.x + frame.w) * w - ox, (frame.y + frame.h) * h - oy),
            ]
        }
        None => mark
            .points
            .iter()
            .map(|spot| (spot.x * w - ox, spot.y * h - oy))
            .collect(),
    }
}

/// What to draw over a picture so it says what was meant by it.
///
/// The screenshot tools mark nothing: the crop is the whole statement, and an outline
/// around the edge of a picture is noise.
pub(crate) fn drawn_as(mark: &Mark) -> Option<&'static str> {
    // Nothing is drawn on a picture whose whole meaning is the picture. A screenshot and
    // a design mark are the crop itself, and an outline round the edge of one is noise;
    // a recording's frames are about what changed between them, and a mark on every one
    // is the only thing that did not.
    if matches!(mark.tool.as_str(), "screenshot" | "design" | "record") {
        return None;
    }
    if mark.tool == "measure" {
        return Some("span");
    }
    // The drawing tool's four pens all arrive as a run of points; which of them it was
    // is the only thing that says whether to draw a line, put a head on it, or lay a
    // wide translucent stripe over what is underneath.
    if mark.tool == "draw" && mark.points.len() > 1 {
        return Some(match mark.pen.as_deref() {
            Some("arrow") => "arrow",
            Some("line") => "line",
            Some("highlight") => "highlight",
            _ => "stroke",
        });
    }
    match mark.region.as_ref().map(|region| region.shape.as_str()) {
        Some("ellipse") => Some("ellipse"),
        Some(_) => Some("box"),
        None if mark.points.len() > 1 => Some("stroke"),
        // A pin is drawn at a point, so a mark with no points is not a pin — it is
        // nothing to draw. Returning one anyway sent the drawing code looking for a
        // point that was not there, inside a callback that GTK calls across an
        // `extern "C"` boundary, where a panic is an abort and the whole toolbar goes.
        None if mark.points.is_empty() => None,
        None => Some("pin"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_region_is_photographed_with_room_for_its_own_outline() {
        let crop = crop_for(
            &mark("box", boxed(0.25, 0.5, 0.25, 0.25), vec![]),
            1000,
            800,
        )
        .unwrap();
        // 250..500 across and 400..600 down, widened by the room the outline needs.
        assert_eq!(
            crop,
            Crop {
                x: 238,
                y: 388,
                width: 274,
                height: 224
            }
        );
    }

    #[test]
    fn a_pin_takes_the_ground_around_it() {
        // A point has no area, so a crop of the point alone is a picture of one pixel.
        let crop = crop_for(
            &mark("pointAt", None, vec![Spot { x: 0.5, y: 0.5 }]),
            1000,
            800,
        )
        .unwrap();
        assert_eq!(crop.width, 280);
        assert_eq!(crop.height, 280);
        assert_eq!((crop.x, crop.y), (360, 260));
    }

    #[test]
    fn a_mark_against_an_edge_still_names_a_rectangle_that_exists() {
        let crop = crop_for(
            &mark("pointAt", None, vec![Spot { x: 0.0, y: 0.0 }]),
            1000,
            800,
        )
        .unwrap();
        assert_eq!((crop.x, crop.y), (0, 0));
        assert!(crop.width > 0 && crop.height > 0);
        // Dragged clean off the display there is nothing to photograph, and saying so
        // beats handing GDK a rectangle of no width.
        assert!(crop_for(&mark("box", boxed(1.5, 1.5, 0.1, 0.1), vec![]), 1000, 800).is_none());
        assert!(crop_for(&mark("box", boxed(0.1, 0.1, 0.1, 0.1), vec![]), 0, 0).is_none());
    }

    #[test]
    fn a_mark_with_nothing_in_it_photographs_the_whole_display() {
        let crop = crop_for(&mark("screenshot", None, vec![]), 1920, 1080).unwrap();
        assert_eq!(
            crop,
            Crop {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080
            }
        );
    }

    #[test]
    fn points_move_into_the_picture_taken_for_them() {
        let one = mark("pointAt", None, vec![Spot { x: 0.5, y: 0.5 }]);
        let crop = crop_for(&one, 1000, 800).unwrap();
        let within = points_within(&one, crop, 1000, 800);
        // The pin was at 500,400 on the display and the crop starts at 360,260.
        assert_eq!(within, vec![(140.0, 140.0)]);
    }

    #[test]
    fn only_the_tools_that_mean_something_are_drawn_on() {
        assert_eq!(
            drawn_as(&mark("box", boxed(0.1, 0.1, 0.2, 0.2), vec![])),
            Some("box")
        );
        assert_eq!(
            drawn_as(&mark(
                "circle",
                Some(Region {
                    shape: "ellipse".to_string(),
                    frame: Frame {
                        x: 0.1,
                        y: 0.1,
                        w: 0.2,
                        h: 0.2
                    },
                }),
                vec![]
            )),
            Some("ellipse"),
        );
        assert_eq!(
            drawn_as(&mark("pointAt", None, vec![Spot { x: 0.5, y: 0.5 }])),
            Some("pin")
        );
        assert_eq!(
            drawn_as(&mark(
                "draw",
                None,
                vec![Spot { x: 0.1, y: 0.1 }, Spot { x: 0.2, y: 0.2 }]
            )),
            Some("stroke"),
        );
        // A screenshot is the whole statement; an outline round its edge is noise. A
        // design mark is a picture somebody is going to build from, and a red box drawn
        // across it is a red box in the wireframe.
        assert_eq!(drawn_as(&mark("screenshot", None, vec![])), None);
        assert_eq!(
            drawn_as(&mark("design", boxed(0.1, 0.1, 0.2, 0.2), vec![])),
            None
        );
    }

    #[test]
    fn a_pen_decides_what_a_drawing_is() {
        // Four pens arrive as the same thing — a run of points under the drawing tool —
        // so the pen is the only thing that says whether to put a head on the line or
        // lay a wide translucent stripe over what is underneath.
        let drawn = |pen: Option<&str>| {
            let mut made = mark(
                "draw",
                None,
                vec![Spot { x: 0.1, y: 0.1 }, Spot { x: 0.4, y: 0.4 }],
            );
            made.pen = pen.map(str::to_string);
            drawn_as(&made)
        };
        assert_eq!(drawn(Some("arrow")), Some("arrow"));
        assert_eq!(drawn(Some("line")), Some("line"));
        assert_eq!(drawn(Some("highlight")), Some("highlight"));
        // Freehand, and anything this build has never heard of. A mark from a newer
        // page that names a pen this one cannot draw is still a line somebody drew.
        assert_eq!(drawn(Some("freehand")), Some("stroke"));
        assert_eq!(drawn(Some("glitter")), Some("stroke"));
        assert_eq!(drawn(None), Some("stroke"));
    }

    #[test]
    fn a_pen_cannot_turn_a_click_into_a_line() {
        // One point is a pin wherever it came from. An arrow with nothing to point
        // along would be drawn as a head sitting on its own tip.
        let mut made = mark("draw", None, vec![Spot { x: 0.2, y: 0.2 }]);
        made.pen = Some("arrow".to_string());
        assert_eq!(drawn_as(&made), Some("pin"));
    }

    #[test]
    fn a_recording_draws_nothing_over_itself() {
        // What changed between the frames is the subject. An outline on every one of
        // them is the only thing in the picture that does not move.
        let mut recorded = mark("record", boxed(0.1, 0.1, 0.3, 0.3), vec![]);
        assert_eq!(drawn_as(&recorded), None);
        recorded.tool = "screenshot".to_string();
        assert_eq!(drawn_as(&recorded), None);
    }

    fn mark(tool: &str, region: Option<Region>, points: Vec<Spot>) -> Mark {
        Mark {
            id: "m1".to_string(),
            tool: tool.to_string(),
            region,
            points,
            pen: None,
        }
    }

    fn boxed(x: f64, y: f64, w: f64, h: f64) -> Option<Region> {
        Some(Region {
            shape: "box".to_string(),
            frame: Frame { x, y, w, h },
        })
    }
}
