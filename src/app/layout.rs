//! Span mode geometry: one canvas covering several monitors, and the slice of
//! it each monitor shows.

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    /// This rect as a fraction of `canvas`: `[x, y, width, height]` in 0..1,
    /// for sampling the canvas texture.
    pub fn uv_in(&self, canvas: (u32, u32)) -> [f32; 4] {
        let (cw, ch) = (canvas.0.max(1) as f32, canvas.1.max(1) as f32);
        [
            self.x as f32 / cw,
            self.y as f32 / ch,
            self.w as f32 / cw,
            self.h as f32 / ch,
        ]
    }
}

/// The canvas size and each monitor's slice of it, in the given order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanLayout {
    pub canvas: (u32, u32),
    /// Relative to the canvas's top-left corner.
    pub slices: Vec<Rect>,
}

/// Lays a canvas over `monitors` (desktop coordinates): its size is their
/// bounding box, so content flows across monitors as they're arranged.
pub fn span(monitors: &[Rect]) -> SpanLayout {
    let Some(first) = monitors.first() else {
        return SpanLayout {
            canvas: (1, 1),
            slices: Vec::new(),
        };
    };
    let left = monitors.iter().map(|m| m.x).min().unwrap_or(first.x);
    let top = monitors.iter().map(|m| m.y).min().unwrap_or(first.y);
    let right = monitors
        .iter()
        .map(|m| m.x + m.w as i32)
        .max()
        .unwrap_or(left);
    let bottom = monitors
        .iter()
        .map(|m| m.y + m.h as i32)
        .max()
        .unwrap_or(top);
    SpanLayout {
        canvas: ((right - left).max(1) as u32, (bottom - top).max(1) as u32),
        slices: monitors
            .iter()
            .map(|m| Rect::new(m.x - left, m.y - top, m.w, m.h))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_by_side_monitors_make_one_wide_canvas() {
        // The layout on the dev machine: ultrawide on the left, 1440p on the right.
        let l = span(&[Rect::new(0, 0, 3440, 1440), Rect::new(3440, 0, 2560, 1440)]);
        assert_eq!(l.canvas, (6000, 1440));
        assert_eq!(l.slices[1], Rect::new(3440, 0, 2560, 1440));
    }

    #[test]
    fn negative_desktop_coordinates_are_shifted_to_zero() {
        let l = span(&[Rect::new(0, 0, 1920, 1080), Rect::new(-1920, 0, 1920, 1080)]);
        assert_eq!(l.canvas, (3840, 1080));
        assert_eq!(l.slices[0], Rect::new(1920, 0, 1920, 1080));
        assert_eq!(l.slices[1], Rect::new(0, 0, 1920, 1080));
    }

    #[test]
    fn stacked_and_offset_monitors_use_the_bounding_box() {
        let l = span(&[
            Rect::new(0, 0, 1920, 1080),
            Rect::new(300, 1080, 1280, 1024),
        ]);
        assert_eq!(l.canvas, (1920, 2104));
        assert_eq!(l.slices[1], Rect::new(300, 1080, 1280, 1024));
    }

    #[test]
    fn a_single_monitor_is_its_own_canvas() {
        let l = span(&[Rect::new(100, 50, 2560, 1440)]);
        assert_eq!(l.canvas, (2560, 1440));
        assert_eq!(l.slices, vec![Rect::new(0, 0, 2560, 1440)]);
    }

    #[test]
    fn no_monitors_gives_a_tiny_canvas() {
        assert_eq!(span(&[]).canvas, (1, 1));
    }

    #[test]
    fn uv_is_the_slice_as_a_fraction_of_the_canvas() {
        let uv = Rect::new(3000, 0, 3000, 1440).uv_in((6000, 1440));
        assert_eq!(uv, [0.5, 0.0, 0.5, 1.0]);
    }
}
