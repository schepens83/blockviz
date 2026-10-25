//! Showing where the change unfolded in the list is in the diagram, whatever the zoom: the block
//! itself when it can be made out, else the block it is in, else a marker on the edge of the view
//! pointing to it.

use crate::camera::{Insets, Rect};
use crate::diff::Status;
use crate::pen::{Align, Family, Font, Pen};
use crate::render::Hit;
use crate::theme::Theme;

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

/// The marker's hit area: the arrow and its label, which sits toward the middle of the view.
pub fn marker_area(m: Marker, label_w: f64) -> Rect {
    let w = 24.0 + label_w;
    let x = if m.angle.cos() > 0.0 { m.x - w + 12.0 } else { m.x - 12.0 };
    Rect { x, y: m.y - 12.0, w, h: 24.0 }
}

const LABEL: Font = Font { px: 11.5, family: Family::Sans, weight: 600 };

/// Draws what `located` says over the diagram, in the change's color (removed ones dashed).
/// Returns the marker's hit area when the block is out of view.
pub fn draw(pen: &mut dyn Pen, theme: &Theme, located: Located, status: Status, label: &str, view: Rect) -> Option<Rect> {
    let ink = theme.change(status).ink;
    match located {
        Located::Itself(r) | Located::Within(_, r) => {
            let within = matches!(located, Located::Within(..));
            let pad = if within { 4.0 } else { 3.0 };
            pen.save();
            pen.set_fill(ink);
            pen.set_alpha(if within { 0.10 } else { 0.16 });
            pen.begin_path();
            pen.round_rect(r.x - pad, r.y - pad, r.w + 2.0 * pad, r.h + 2.0 * pad, 6.0);
            pen.fill();
            pen.set_alpha(if within { 0.75 } else { 1.0 });
            pen.set_stroke(ink);
            pen.set_line_width(if within { 2.0 } else { 3.0 });
            pen.set_round_join(true);
            if within || status == Status::Removed {
                pen.set_dash(&[7.0, 5.0]);
            }
            pen.begin_path();
            pen.round_rect(r.x - pad, r.y - pad, r.w + 2.0 * pad, r.h + 2.0 * pad, 6.0);
            pen.stroke();
            pen.restore();
            None
        }
        Located::Away(target) => {
            let m = edge_marker(view, target, 16.0)?;
            pen.save();
            pen.set_font(LABEL);
            let text = if label.chars().count() > 28 { format!("{}…", label.chars().take(27).collect::<String>()) } else { label.to_string() };
            let w = pen.measure(&text);
            let area = marker_area(m, w + 10.0);
            // The label's plate, on the side toward the middle of the view.
            pen.set_fill(theme.paper);
            pen.set_alpha(0.94);
            pen.begin_path();
            pen.round_rect(area.x, area.y, area.w, area.h, 12.0);
            pen.fill();
            pen.set_alpha(1.0);
            pen.set_stroke(ink);
            pen.set_line_width(1.5);
            pen.begin_path();
            pen.round_rect(area.x, area.y, area.w, area.h, 12.0);
            pen.stroke();
            // The arrow, pointing at the block.
            let (c, s) = (m.angle.cos(), m.angle.sin());
            let at = |u: f64, v: f64| (m.x + u * c - v * s, m.y + u * s + v * c);
            pen.set_fill(ink);
            pen.begin_path();
            let (x0, y0) = at(6.0, 0.0);
            pen.move_to(x0, y0);
            let (x1, y1) = at(-4.0, -5.0);
            pen.line_to(x1, y1);
            let (x2, y2) = at(-4.0, 5.0);
            pen.line_to(x2, y2);
            pen.close_path();
            pen.fill();
            // Its name, on the plate.
            pen.set_fill(theme.change(status).ink);
            pen.set_align(Align::Left);
            let tx = if c > 0.0 { area.x + 10.0 } else { area.x + 24.0 };
            pen.fill_text(&text, tx, area.y + 16.0);
            pen.restore();
            Some(area)
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
    fn the_markers_area_holds_the_label_toward_the_middle() {
        let right = marker_area(Marker { x: 790.0, y: 300.0, angle: 0.0 }, 80.0);
        assert!(right.contains(790.0, 300.0) && right.contains(700.0, 300.0) && !right.contains(800.0 + 20.0, 300.0));
        let left = marker_area(Marker { x: 10.0, y: 300.0, angle: std::f64::consts::PI }, 80.0);
        assert!(left.contains(10.0, 300.0) && left.contains(100.0, 300.0));
    }
}
