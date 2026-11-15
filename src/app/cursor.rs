//! Hides the mouse cursor over full-screen and span windows once it's been
//! still for a moment, like a video player. Moving it brings it back, and it
//! always shows while the settings overlay is open (that's clicked with it).

/// Seconds the cursor must be still before it's hidden.
const HIDE_AFTER_SECS: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorHider {
    still_secs: f32,
    visible: bool,
}

impl Default for CursorHider {
    fn default() -> Self {
        Self {
            still_secs: 0.0,
            visible: true,
        }
    }
}

impl CursorHider {
    /// Whether the cursor should currently show (for newly opened windows).
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// The cursor moved over one of our windows.
    pub fn moved(&mut self) {
        self.still_secs = 0.0;
    }

    /// Advances by `dt` seconds. `immersive` is full screen or span mode.
    /// Returns the new visibility when it changes.
    pub fn update(&mut self, dt: f32, immersive: bool, overlay_open: bool) -> Option<bool> {
        self.still_secs += dt;
        let visible = !immersive || overlay_open || self.still_secs < HIDE_AFTER_SECS;
        (visible != self.visible).then(|| {
            self.visible = visible;
            visible
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hides_after_being_still_in_full_screen() {
        let mut c = CursorHider::default();
        assert_eq!(c.update(1.0, true, false), None);
        assert_eq!(c.update(1.0, true, false), Some(false));
        assert_eq!(c.update(1.0, true, false), None);
    }

    #[test]
    fn moving_shows_it_again_and_restarts_the_wait() {
        let mut c = CursorHider::default();
        c.update(3.0, true, false);
        c.moved();
        assert_eq!(c.update(0.1, true, false), Some(true));
        assert_eq!(c.update(1.0, true, false), None);
        assert_eq!(c.update(1.0, true, false), Some(false));
    }

    #[test]
    fn never_hides_in_a_window() {
        let mut c = CursorHider::default();
        assert_eq!(c.update(60.0, false, false), None);
    }

    #[test]
    fn shows_while_the_overlay_is_open() {
        let mut c = CursorHider::default();
        c.update(3.0, true, false);
        assert_eq!(c.update(0.1, true, true), Some(true));
        assert_eq!(c.update(10.0, true, true), None);
    }

    #[test]
    fn leaving_full_screen_shows_it_at_once() {
        let mut c = CursorHider::default();
        c.update(3.0, true, false);
        assert_eq!(c.update(0.1, false, false), Some(true));
    }
}
