//! Showing where the change unfolded in the list is in the diagram, whatever the zoom: the block
//! itself when it can be made out, else the block it is in, else a marker on the edge of the view
//! pointing to it.

use crate::camera::{Insets, Rect};
use crate::diff::Status;
use crate::pen::{Align, Family, Font, Pen};
use crate::render::Hit;
use crate::theme::{Color, Theme};

/// What to point out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Located {
    /// The block, drawn large enough to make out.
    Itself(Rect),
    /// It is too small, or inside a block not opened at this zoom: the nearest block around it that is drawn.
    Within(u32, Rect),
    /// Outside the view: where it is, on screen.
    Away(Rect),
}

/// The part of the screen the overlays leave free.
pub fn view_rect(width: f64, height: f64, i: Insets) -> Rect {
    Rect { x: i.left, y: i.top, w: (width - i.left - i.right).max(1.0), h: (height - i.top - i.bottom).max(1.0) }
}

fn meets(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
}

/// What to point out for block `target`: `ancestors` are the blocks around it, nearest first; `hits` the
/// blocks drawn this frame; `at` where the camera puts it on screen, when it knows; `view` the free part
/// of the screen. None when nothing of it is anywhere to be found.
pub fn locate(target: u32, ancestors: &[u32], hits: &[Hit], at: Option<Rect>, view: Rect) -> Option<Located> {
    let clear = |id: u32| hits.iter().find(|h| h.id == id && h.clear && meets(h.rect, view)).map(|h| h.rect);
    if let Some(r) = clear(target) {
        return Some(Located::Itself(r));
    }
    // Out of the view altogether: say which way.
    if let Some(r) = at.filter(|r| !meets(*r, view)) {
        return Some(Located::Away(r));
    }
    if let Some((id, r)) = ancestors.iter().find_map(|&a| clear(a).map(|r| (a, r))) {
        return Some(Located::Within(id, r));
    }
    at.map(Located::Away)
}

/// A marker on the edge of the view: where, and which way (radians) it points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marker {
    pub x: f64,
    pub y: f64,
    pub angle: f64,
}

/// Where the line from the middle of `view` to the middle of `target` crosses the edge of `view`, kept
/// `margin` inside it. None when the target is (partly) in view.
pub fn edge_marker(view: Rect, target: Rect, margin: f64) -> Option<Marker> {
    if meets(view, target) {
        return None;
    }
    let (cx, cy) = (view.x + view.w / 2.0, view.y + view.h / 2.0);
    let (dx, dy) = (target.x + target.w / 2.0 - cx, target.y + target.h / 2.0 - cy);
    let (hw, hh) = ((view.w / 2.0 - margin).max(0.0), (view.h / 2.0 - margin).max(0.0));
    let k = match (dx.abs() > 1e-9, dy.abs() > 1e-9) {
        (true, true) => (hw / dx.abs()).min(hh / dy.abs()),
        (true, false) => hw / dx.abs(),
        (false, true) => hh / dy.abs(),
        (false, false) => return None,
    };
    Some(Marker { x: cx + dx * k, y: cy + dy * k, angle: dy.atan2(dx) })
}

/// How the located block is shown: every number that sets how loud it is.
pub mod look {
    /// How far the spotlight's hole reaches past the block.
    pub const HOLE_PAD: f64 = 8.0;
    /// The scrim over the rest of the diagram: darker on a dark theme; lighter when only the block around is cut out.
    pub const SCRIM_LIGHT: f32 = 0.35;
    pub const SCRIM_DARK: f32 = 0.45;
    pub const SCRIM_WITHIN: f32 = 0.20;
    /// The ring: how far outside the block, how thick (dashed when the block it rings is the one around).
    pub const RING_OFFSET: f64 = 4.0;
    pub const RING_W: f64 = 4.0;
    pub const RING_W_WITHIN: f64 = 3.0;
    pub const RING_DASH: [f64; 2] = [9.0, 6.0];
    /// The glow outside the ring, and how it breathes.
    pub const GLOW_W: f64 = 10.0;
    pub const GLOW_MIN: f64 = 0.15;
    pub const GLOW_MAX: f64 = 0.4;
    pub const BREATH_SECONDS: f64 = 1.6;
    /// Pointing at a change by hovering its row: the glow stays at this strength, and nothing is dimmed.
    pub const HOVER_GLOW: f64 = 0.3;
    /// The edge marker, against its first size.
    pub const MARKER_SCALE: f64 = 1.4;
    pub const MARKER_TEXT: u32 = 0xfafafa;
}

/// How loud a block is pointed out: the change unfolded in the list (the rest dimmed, the glow breathing), or the
/// row under the pointer (a calm ring only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Located,
    Hover,
}

/// What to draw, in order, for the change unfolded (`marked`) and the one under the pointer (`hovered`), by
/// index: the same change is drawn once, in the louder style; the hovered one goes on top.
pub fn plan(marked: Option<usize>, hovered: Option<usize>) -> Vec<(usize, Style)> {
    let mut out = vec![];
    if let Some(m) = marked {
        out.push((m, Style::Located));
    }
    if let Some(h) = hovered.filter(|&h| Some(h) != marked) {
        out.push((h, Style::Hover));
    }
    out
}

/// The glow's strength `t` seconds in: it breathes between its least and its most.
pub fn glow_alpha(t: f64) -> f64 {
    let wave = 0.5 + 0.5 * (std::f64::consts::TAU * t / look::BREATH_SECONDS).sin();
    look::GLOW_MIN + (look::GLOW_MAX - look::GLOW_MIN) * wave
}

/// The four bands of `view` around `hole`, for a scrim with a window cut out of it (the hole kept inside the view).
pub fn scrim_rects(view: Rect, hole: Rect) -> [Rect; 4] {
    let x0 = hole.x.clamp(view.x, view.x + view.w);
    let x1 = (hole.x + hole.w).clamp(view.x, view.x + view.w);
    let y0 = hole.y.clamp(view.y, view.y + view.h);
    let y1 = (hole.y + hole.h).clamp(view.y, view.y + view.h);
    [
        Rect { x: view.x, y: view.y, w: view.w, h: y0 - view.y },
        Rect { x: view.x, y: y1, w: view.w, h: view.y + view.h - y1 },
        Rect { x: view.x, y: y0, w: x0 - view.x, h: y1 - y0 },
        Rect { x: x1, y: y0, w: view.x + view.w - x1, h: y1 - y0 },
    ]
}

/// The marker's hit area: the arrow and its label, which sits toward the middle of the view.
pub fn marker_area(m: Marker, label_w: f64) -> Rect {
    let k = look::MARKER_SCALE;
    let w = 24.0 * k + label_w;
    let x = if m.angle.cos() > 0.0 { m.x - w + 12.0 * k } else { m.x - 12.0 * k };
    Rect { x, y: m.y - 12.0 * k, w, h: 24.0 * k }
}

const LABEL: Font = Font { px: 11.5 * look::MARKER_SCALE, family: Family::Sans, weight: 700 };

/// Draws what `located` says over the diagram, in the change's color (removed ones dashed): the rest of the view
/// dimmed, a ring round the block and a glow breathing outside it (`t`, seconds, sets the breath). Returns the
/// marker's hit area when the block is out of view.
pub fn draw(pen: &mut dyn Pen, theme: &Theme, located: Located, status: Status, label: &str, view: Rect, t: f64, style: Style) -> Option<Rect> {
    use look::*;
    let ink = theme.change(status).ink;
    match located {
        Located::Itself(r) | Located::Within(_, r) => {
            let within = matches!(located, Located::Within(..));
            pen.save();
            // The spotlight.
            let hole = Rect { x: r.x - HOLE_PAD, y: r.y - HOLE_PAD, w: r.w + 2.0 * HOLE_PAD, h: r.h + 2.0 * HOLE_PAD };
            pen.set_fill(Color::rgba(0, 0, 0, 1.0));
            pen.set_alpha(if within { SCRIM_WITHIN } else if theme.dark { SCRIM_DARK } else { SCRIM_LIGHT } as f64);
            if style == Style::Located {
                for b in scrim_rects(view, hole) {
                    if b.w > 0.0 && b.h > 0.0 {
                        pen.fill_rect(b.x, b.y, b.w, b.h);
                    }
                }
            }
            // The ring, and the glow outside it.
            let o = RING_OFFSET;
            let ring = |pen: &mut dyn Pen, grow: f64, width: f64, alpha: f64, dashed: bool| {
                pen.set_alpha(alpha);
                pen.set_stroke(ink);
                pen.set_line_width(width);
                pen.set_round_join(true);
                pen.set_dash(if dashed { &RING_DASH } else { &[] });
                pen.begin_path();
                pen.round_rect(r.x - o - grow, r.y - o - grow, r.w + 2.0 * (o + grow), r.h + 2.0 * (o + grow), 8.0 + grow);
                pen.stroke();
            };
            let dashed = within || status == Status::Removed;
            ring(pen, RING_W / 2.0 + GLOW_W / 2.0, GLOW_W, if style == Style::Hover { HOVER_GLOW } else { glow_alpha(t) }, false);
            ring(pen, 0.0, if within { RING_W_WITHIN } else { RING_W }, 1.0, dashed);
            pen.restore();
            None
        }
        Located::Away(target) => {
            let m = edge_marker(view, target, 16.0 * MARKER_SCALE)?;
            let k = MARKER_SCALE;
            pen.save();
            pen.set_font(LABEL);
            let text = if label.chars().count() > 28 { format!("{}…", label.chars().take(27).collect::<String>()) } else { label.to_string() };
            let w = pen.measure(&text);
            let area = marker_area(m, w + 10.0 * k);
            // A shadow, then the plate in the change's color on the side toward the middle of the view.
            pen.set_fill(Color::rgba(0, 0, 0, 1.0));
            pen.set_alpha(0.25);
            pen.begin_path();
            pen.round_rect(area.x, area.y + 3.0, area.w, area.h, 12.0 * k);
            pen.fill();
            pen.set_fill(ink);
            pen.set_alpha(1.0);
            pen.begin_path();
            pen.round_rect(area.x, area.y, area.w, area.h, 12.0 * k);
            pen.fill();
            // The arrow, pointing at the block, and the name.
            let (c, s) = (m.angle.cos(), m.angle.sin());
            let at = |u: f64, v: f64| (m.x + k * (u * c - v * s), m.y + k * (u * s + v * c));
            pen.set_fill(Color::hex(MARKER_TEXT));
            pen.begin_path();
            let (x0, y0) = at(6.0, 0.0);
            pen.move_to(x0, y0);
            let (x1, y1) = at(-4.0, -5.0);
            pen.line_to(x1, y1);
            let (x2, y2) = at(-4.0, 5.0);
            pen.line_to(x2, y2);
            pen.close_path();
            pen.fill();
            pen.set_align(Align::Left);
            let tx = if c > 0.0 { area.x + 10.0 * k } else { area.x + 24.0 * k };
            pen.fill_text(&text, tx, area.y + 16.0 * k);
            pen.restore();
            // Hovering a row in the list: the pointer is there, not here; nothing to click.
            (style == Style::Located).then_some(area)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    fn hit(id: u32, r: Rect, clear: bool) -> Hit {
        Hit { id, rect: r, parent: -1, clear, face: true }
    }

    const VIEW: Rect = Rect { x: 0.0, y: 50.0, w: 800.0, h: 500.0 };

    #[test]
    fn the_block_itself_when_it_can_be_made_out() {
        let r = rect(100.0, 100.0, 200.0, 80.0);
        assert_eq!(locate(7, &[3, 1], &[hit(7, r, true), hit(3, VIEW, true)], Some(r), VIEW), Some(Located::Itself(r)));
    }

    #[test]
    fn the_nearest_block_around_it_when_it_is_too_small_or_not_opened() {
        let tiny = rect(120.0, 120.0, 3.0, 3.0);
        let room = rect(100.0, 100.0, 300.0, 200.0);
        let hits = [hit(7, tiny, false), hit(3, room, true), hit(1, VIEW, true)];
        assert_eq!(locate(7, &[3, 1], &hits, Some(tiny), VIEW), Some(Located::Within(3, room)));
        // Not drawn at all: inside a block that is still a thumbnail, or closed.
        let hits = [hit(3, room, false), hit(1, VIEW, true)];
        assert_eq!(locate(7, &[3, 1], &hits, Some(tiny), VIEW), Some(Located::Within(1, VIEW)));
        // The camera cannot say where it is.
        assert_eq!(locate(7, &[3, 1], &hits, None, VIEW), Some(Located::Within(1, VIEW)));
    }

    #[test]
    fn away_when_it_is_outside_the_view() {
        let far = rect(2000.0, 100.0, 100.0, 50.0);
        // Its container is in view, but the block is not: it is not "in" the part shown.
        assert_eq!(locate(7, &[1], &[hit(1, VIEW, true)], Some(far), VIEW), Some(Located::Away(far)));
        // Behind the panel counts as outside.
        let behind = rect(850.0, 100.0, 100.0, 50.0);
        assert!(matches!(locate(7, &[], &[], Some(behind), VIEW), Some(Located::Away(_))));
        assert_eq!(locate(7, &[], &[], None, VIEW), None);
    }

    #[test]
    fn the_marker_is_where_the_line_to_the_block_leaves_the_view() {
        let m = edge_marker(VIEW, rect(1900.0, 275.0, 100.0, 50.0), 10.0).unwrap();
        // Straight right from the middle (400, 300): the right edge, 10 inside.
        assert!((m.x - 790.0).abs() < 1e-9 && (m.y - 300.0).abs() < 1e-9 && m.angle.abs() < 1e-9, "{m:?}");
        let up = edge_marker(VIEW, rect(350.0, -900.0, 100.0, 50.0), 10.0).unwrap();
        assert!((up.y - 60.0).abs() < 1e-9 && (up.x - 400.0).abs() < 1e-9 && (up.angle + std::f64::consts::FRAC_PI_2).abs() < 1e-9, "{up:?}");
        // Diagonally it leaves by whichever edge it meets first, on the line.
        let d = edge_marker(VIEW, rect(1800.0, 1800.0, 100.0, 100.0), 10.0).unwrap();
        assert!((d.y - 540.0).abs() < 1e-9, "{d:?}");
        assert!((d.x - 400.0) / (d.y - 300.0) > 0.0);
        assert!(d.x < 790.0);
    }

    #[test]
    fn there_is_no_marker_for_a_block_in_view() {
        assert_eq!(edge_marker(VIEW, rect(700.0, 100.0, 300.0, 50.0), 10.0), None);
    }

    #[test]
    fn the_scrim_leaves_a_hole_and_covers_the_rest_of_the_view_once() {
        let hole = rect(100.0, 150.0, 200.0, 80.0);
        let bands = scrim_rects(VIEW, hole);
        let area: f64 = bands.iter().map(|b| b.w * b.h).sum();
        assert!((area - (VIEW.w * VIEW.h - hole.w * hole.h)).abs() < 1e-6);
        // None of the bands touches the hole.
        assert!(bands.iter().all(|b| b.w <= 0.0 || b.h <= 0.0 || b.x >= hole.x + hole.w || b.x + b.w <= hole.x || b.y >= hole.y + hole.h || b.y + b.h <= hole.y));
        // A hole hanging out of the view is cut to it.
        let half = scrim_rects(VIEW, rect(-50.0, 0.0, 100.0, 100.0));
        let covered: f64 = half.iter().map(|b| b.w * b.h).sum();
        assert!((covered - (VIEW.w * VIEW.h - 50.0 * 50.0)).abs() < 1e-6, "{covered}");
    }

    #[test]
    fn a_change_is_drawn_once_and_the_hovered_one_on_top() {
        assert_eq!(plan(None, None), vec![]);
        assert_eq!(plan(Some(2), None), vec![(2, Style::Located)]);
        assert_eq!(plan(None, Some(5)), vec![(5, Style::Hover)]);
        assert_eq!(plan(Some(2), Some(2)), vec![(2, Style::Located)]);
        assert_eq!(plan(Some(2), Some(5)), vec![(2, Style::Located), (5, Style::Hover)]);
    }

    #[test]
    fn the_glow_breathes_between_its_least_and_its_most() {
        let samples: Vec<f64> = (0..160).map(|i| glow_alpha(i as f64 * 0.01)).collect();
        let (lo, hi) = (samples.iter().cloned().fold(f64::MAX, f64::min), samples.iter().cloned().fold(f64::MIN, f64::max));
        assert!((lo - look::GLOW_MIN).abs() < 1e-3 && (hi - look::GLOW_MAX).abs() < 1e-3, "{lo} {hi}");
        assert!((glow_alpha(0.0) - glow_alpha(look::BREATH_SECONDS)).abs() < 1e-9);
    }

    #[test]
    fn the_markers_area_holds_the_label_toward_the_middle() {
        let right = marker_area(Marker { x: 790.0, y: 300.0, angle: 0.0 }, 80.0);
        assert!(right.contains(790.0, 300.0) && right.contains(700.0, 300.0) && !right.contains(800.0 + 20.0, 300.0));
        let left = marker_area(Marker { x: 10.0, y: 300.0, angle: std::f64::consts::PI }, 80.0);
        assert!(left.contains(10.0, 300.0) && left.contains(100.0, 300.0));
    }
}
