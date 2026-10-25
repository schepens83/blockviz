//! A scrollbar for the scroll containers gpui leaves bare: a thin track on the
//! right edge that follows a `ScrollHandle`, a thumb to drag, and a click on
//! the track to jump there. The geometry is plain numbers, tested alone.

use crate::theme::Ui;
use gpui::{canvas, fill, point, px, size, Bounds, Corners, DispatchPhase, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollHandle, Styled};
use std::cell::Cell;
use std::rc::Rc;

/// How wide the strip along the edge is, and the thumb inside it.
pub(crate) const STRIP: f32 = 10.0;
const THUMB: f32 = 6.0;
/// The shortest the thumb gets, so it stays easy to grab in a very long file.
const MIN_THUMB: f32 = 24.0;

/// Where the thumb sits in a track: its top and height, or None when the content fits.
/// `offset` is how far the content is scrolled (0 at the top).
pub(crate) fn thumb(viewport: f32, content: f32, offset: f32, track: f32, min_thumb: f32) -> Option<(f32, f32)> {
    if content <= viewport || viewport <= 0.0 || track <= 0.0 {
        return None;
    }
    let height = (track * viewport / content).max(min_thumb).min(track);
    let top = (track - height) * (offset / (content - viewport)).clamp(0.0, 1.0);
    Some((top, height))
}

/// How far to scroll for the thumb's top at `top` in the track: the inverse of `thumb`, kept in range.
pub(crate) fn offset_for(top: f32, viewport: f32, content: f32, track: f32, min_thumb: f32) -> f32 {
    let Some((_, height)) = thumb(viewport, content, 0.0, track, min_thumb) else { return 0.0 };
    let room = track - height;
    if room <= 0.0 {
        return 0.0;
    }
    (top / room).clamp(0.0, 1.0) * (content - viewport)
}

/// A scrollbar's state, shared by the frames that draw it: the handle it follows, whether the pointer is over it, and a drag's grip.
#[derive(Clone)]
pub(crate) struct Scrollbar {
    pub handle: ScrollHandle,
    hover: Rc<Cell<bool>>,
    /// While dragging, how far below the thumb's top the pointer holds it.
    grip: Rc<Cell<Option<f32>>>,
}

impl Scrollbar {
    pub fn new() -> Self {
        Scrollbar { handle: ScrollHandle::new(), hover: Rc::default(), grip: Rc::default() }
    }

    /// Whether the thumb is being dragged.
    #[cfg(test)]
    pub fn dragging(&self) -> bool {
        self.grip.get().is_some()
    }

    /// (viewport, content, offset) now, from what the container last laid out.
    fn metrics(handle: &ScrollHandle) -> (f32, f32, f32) {
        let viewport: f32 = handle.bounds().size.height.into();
        let max: f32 = handle.max_offset().height.into();
        let offset: f32 = handle.offset().y.into();
        (viewport, viewport + max, -offset)
    }

    /// The strip to lay over the container's right edge (which must be positioned).
    pub fn overlay(&self, ui: &Ui) -> impl gpui::IntoElement {
        let (bar, ui) = (self.clone(), ui.clone());
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let (viewport, content, offset) = Self::metrics(&bar.handle);
                let Some((top, height)) = thumb(viewport, content, offset, bounds.size.height.into(), MIN_THUMB) else { return };
                let active = bar.hover.get() || bar.grip.get().is_some();
                let strip = Bounds::new(point(bounds.right() - px(STRIP), bounds.top()), size(px(STRIP), bounds.size.height));
                if active {
                    window.paint_quad(fill(strip, ui.hover));
                }
                let rect = Bounds::new(point(bounds.right() - px((STRIP + THUMB) / 2.0), bounds.top() + px(top)), size(px(THUMB), px(height)));
                window.paint_quad(gpui::quad(rect, Corners::all(px(THUMB / 2.0)), if active { ui.faint } else { ui.line_strong }, 0.0, gpui::transparent_black(), gpui::BorderStyle::default()));

                let at = move |bar: &Scrollbar, y: Pixels, grip: f32| {
                    let (viewport, content, _) = Self::metrics(&bar.handle);
                    let top: f32 = (y - bounds.top()).into();
                    let to = offset_for(top - grip, viewport, content, bounds.size.height.into(), MIN_THUMB);
                    bar.handle.set_offset(point(bar.handle.offset().x, px(-to)));
                };
                let over = move |p: gpui::Point<Pixels>| strip.contains(&p);
                let b = bar.clone();
                window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble || e.button != MouseButton::Left || !over(e.position) {
                        return;
                    }
                    let (viewport, content, offset) = Self::metrics(&b.handle);
                    let Some((top, height)) = thumb(viewport, content, offset, bounds.size.height.into(), MIN_THUMB) else { return };
                    let y: f32 = (e.position.y - bounds.top()).into();
                    // On the thumb: hold it where it was grabbed. On the track: jump so the thumb centers there, and carry on dragging from its middle.
                    let grip = if y >= top && y <= top + height { y - top } else { height / 2.0 };
                    b.grip.set(Some(grip));
                    at(&b, e.position.y, grip);
                    window.refresh();
                    cx.stop_propagation();
                });
                let b = bar.clone();
                window.on_mouse_event(move |e: &MouseMoveEvent, _, window, _| {
                    match b.grip.get() {
                        // A release the window never saw ends the drag.
                        Some(_) if e.pressed_button != Some(MouseButton::Left) => {
                            b.grip.set(None);
                            window.refresh();
                        }
                        Some(grip) => {
                            at(&b, e.position.y, grip);
                            window.refresh();
                        }
                        None => {
                            if b.hover.replace(over(e.position)) != over(e.position) {
                                window.refresh();
                            }
                        }
                    }
                });
                let b = bar.clone();
                window.on_mouse_event(move |e: &MouseUpEvent, _, window, _| {
                    if e.button == MouseButton::Left && b.grip.replace(None).is_some() {
                        b.hover.set(over(e.position));
                        window.refresh();
                    }
                });
            },
        )
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .w(px(STRIP))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_that_fits_has_no_thumb() {
        assert_eq!(thumb(100.0, 100.0, 0.0, 100.0, 24.0), None);
        assert_eq!(thumb(100.0, 60.0, 0.0, 100.0, 24.0), None);
        assert_eq!(thumb(0.0, 60.0, 0.0, 100.0, 24.0), None);
    }

    #[test]
    fn the_thumb_is_the_share_of_the_content_in_view() {
        assert_eq!(thumb(100.0, 400.0, 0.0, 100.0, 24.0), Some((0.0, 25.0)));
    }

    #[test]
    fn the_thumb_moves_from_the_top_to_the_bottom_of_the_track() {
        assert_eq!(thumb(100.0, 400.0, 300.0, 100.0, 24.0), Some((75.0, 25.0)));
        assert_eq!(thumb(100.0, 400.0, 150.0, 100.0, 24.0), Some((37.5, 25.0)));
        // Overscrolled either way, it stays on the track.
        assert_eq!(thumb(100.0, 400.0, 900.0, 100.0, 24.0), Some((75.0, 25.0)));
        assert_eq!(thumb(100.0, 400.0, -20.0, 100.0, 24.0), Some((0.0, 25.0)));
    }

    #[test]
    fn a_long_file_does_not_shrink_the_thumb_below_the_minimum() {
        assert_eq!(thumb(100.0, 100_000.0, 0.0, 100.0, 24.0), Some((0.0, 24.0)));
        let (top, h) = thumb(100.0, 100_000.0, 99_900.0, 100.0, 24.0).unwrap();
        assert_eq!((top + h, h), (100.0, 24.0));
    }

    #[test]
    fn a_track_shorter_than_the_minimum_is_filled() {
        assert_eq!(thumb(10.0, 40.0, 0.0, 20.0, 24.0), Some((0.0, 20.0)));
    }

    #[test]
    fn offset_for_inverts_thumb() {
        for offset in [0.0, 37.0, 150.0, 299.0, 300.0] {
            let (top, _) = thumb(100.0, 400.0, offset, 100.0, 24.0).unwrap();
            assert!((offset_for(top, 100.0, 400.0, 100.0, 24.0) - offset).abs() < 1e-3);
        }
        // With the minimum in play too.
        let (top, _) = thumb(100.0, 100_000.0, 50_000.0, 100.0, 24.0).unwrap();
        assert!((offset_for(top, 100.0, 100_000.0, 100.0, 24.0) - 50_000.0).abs() < 1.0);
    }

    #[test]
    fn offset_for_stays_in_range() {
        assert_eq!(offset_for(-30.0, 100.0, 400.0, 100.0, 24.0), 0.0);
        assert_eq!(offset_for(500.0, 100.0, 400.0, 100.0, 24.0), 300.0);
        assert_eq!(offset_for(10.0, 100.0, 80.0, 100.0, 24.0), 0.0);
    }
}
