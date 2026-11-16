//! Maps raw input to app actions. Kept free of window/GPU state so it is unit-testable.

use std::time::{Duration, Instant};

use winit::keyboard::{Key, NamedKey};

/// Maximum gap between two clicks for them to count as a double-click.
const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    ToggleFullscreen,
    /// Leave full screen if in it, otherwise quit.
    Escape,
    SensitivityUp,
    SensitivityDown,
    NextVisualizer,
    PreviousVisualizer,
    ToggleDebugView,
    ToggleBloom,
    ToggleScanlines,
    ToggleRandom,
    /// Pause random switching on the current visualizer.
    ToggleLock,
    ToggleSettings,
    NextPreset,
    PreviousPreset,
    /// Spread across every monitor (double-tap F).
    EnterSpan,
}

/// Returns the action bound to a key press, if any.
pub fn action_for_key(key: &Key) -> Option<Action> {
    match key {
        Key::Named(NamedKey::Escape) => Some(Action::Escape),
        Key::Named(NamedKey::ArrowUp) => Some(Action::SensitivityUp),
        Key::Named(NamedKey::ArrowDown) => Some(Action::SensitivityDown),
        Key::Named(NamedKey::ArrowRight) => Some(Action::NextVisualizer),
        Key::Named(NamedKey::ArrowLeft) => Some(Action::PreviousVisualizer),
        Key::Character(c) if c.eq_ignore_ascii_case("d") => Some(Action::ToggleDebugView),
        Key::Character(c) if c.eq_ignore_ascii_case("b") => Some(Action::ToggleBloom),
        Key::Character(c) if c.eq_ignore_ascii_case("c") => Some(Action::ToggleScanlines),
        Key::Character(c) if c.eq_ignore_ascii_case("r") => Some(Action::ToggleRandom),
        Key::Named(NamedKey::Space) => Some(Action::ToggleLock),
        Key::Named(NamedKey::Tab) => Some(Action::ToggleSettings),
        Key::Character(c) if c == "]" => Some(Action::NextPreset),
        Key::Character(c) if c == "[" => Some(Action::PreviousPreset),
        Key::Character(c) if c.eq_ignore_ascii_case("f") => Some(Action::ToggleFullscreen),
        _ => None,
    }
}

/// Detects double-clicks from a stream of left-button presses.
#[derive(Debug, Default)]
pub struct DoubleClickDetector {
    last_click: Option<Instant>,
}

impl DoubleClickDetector {
    /// Records a click at `now` and returns true if it completes a double-click.
    pub fn click(&mut self, now: Instant) -> bool {
        let is_double = self
            .last_click
            .is_some_and(|prev| now.saturating_duration_since(prev) <= DOUBLE_CLICK_WINDOW);
        // A completed double-click resets, so a triple-click isn't two double-clicks.
        self.last_click = if is_double { None } else { Some(now) };
        is_double
    }

    /// Forgets any pending first click.
    pub fn reset(&mut self) {
        self.last_click = None;
    }
}

/// How far (in pixels) the mouse must move with the button held before it
/// starts moving the window. Less than this is still a click.
const DRAG_THRESHOLD_PX: f64 = 6.0;

/// Moving the window by dragging anywhere in it: watches the left button and
/// the cursor, and says when a press has turned into a drag.
#[derive(Debug, Default)]
pub struct DragToMove {
    cursor: Option<(f64, f64)>,
    pressed_at: Option<(f64, f64)>,
}

impl DragToMove {
    /// The left button went down on the visuals (not on the settings panel).
    pub fn pressed(&mut self) {
        self.pressed_at = self.cursor;
    }

    pub fn released(&mut self) {
        self.pressed_at = None;
    }

    /// The cursor moved to `pos`. Returns true once when a press becomes a
    /// drag, at which point the window should start moving.
    pub fn moved(&mut self, pos: (f64, f64)) -> bool {
        self.cursor = Some(pos);
        let Some(start) = self.pressed_at else {
            return false;
        };
        let far = (pos.0 - start.0).hypot(pos.1 - start.1) >= DRAG_THRESHOLD_PX;
        if far {
            self.pressed_at = None;
        }
        far
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_toggles_fullscreen_in_either_case() {
        assert_eq!(
            action_for_key(&Key::Character("f".into())),
            Some(Action::ToggleFullscreen)
        );
        assert_eq!(
            action_for_key(&Key::Character("F".into())),
            Some(Action::ToggleFullscreen)
        );
    }

    #[test]
    fn escape_maps_to_escape() {
        assert_eq!(
            action_for_key(&Key::Named(NamedKey::Escape)),
            Some(Action::Escape)
        );
    }

    #[test]
    fn arrows_adjust_sensitivity() {
        assert_eq!(
            action_for_key(&Key::Named(NamedKey::ArrowUp)),
            Some(Action::SensitivityUp)
        );
        assert_eq!(
            action_for_key(&Key::Named(NamedKey::ArrowDown)),
            Some(Action::SensitivityDown)
        );
    }

    #[test]
    fn left_right_switch_visualizers_and_d_toggles_debug() {
        assert_eq!(
            action_for_key(&Key::Named(NamedKey::ArrowRight)),
            Some(Action::NextVisualizer)
        );
        assert_eq!(
            action_for_key(&Key::Named(NamedKey::ArrowLeft)),
            Some(Action::PreviousVisualizer)
        );
        assert_eq!(
            action_for_key(&Key::Character("D".into())),
            Some(Action::ToggleDebugView)
        );
    }

    #[test]
    fn b_and_c_toggle_post_effects() {
        assert_eq!(
            action_for_key(&Key::Character("b".into())),
            Some(Action::ToggleBloom)
        );
        assert_eq!(
            action_for_key(&Key::Character("C".into())),
            Some(Action::ToggleScanlines)
        );
    }

    #[test]
    fn random_lock_and_settings_keys() {
        assert_eq!(
            action_for_key(&Key::Character("R".into())),
            Some(Action::ToggleRandom)
        );
        assert_eq!(
            action_for_key(&Key::Named(NamedKey::Space)),
            Some(Action::ToggleLock)
        );
        assert_eq!(
            action_for_key(&Key::Named(NamedKey::Tab)),
            Some(Action::ToggleSettings)
        );
    }

    #[test]
    fn brackets_step_presets() {
        assert_eq!(
            action_for_key(&Key::Character("]".into())),
            Some(Action::NextPreset)
        );
        assert_eq!(
            action_for_key(&Key::Character("[".into())),
            Some(Action::PreviousPreset)
        );
    }

    #[test]
    fn unbound_keys_do_nothing() {
        assert_eq!(action_for_key(&Key::Character("q".into())), None);
        assert_eq!(action_for_key(&Key::Named(NamedKey::Enter)), None);
    }

    #[test]
    fn two_quick_clicks_are_a_double_click() {
        let mut d = DoubleClickDetector::default();
        let t0 = Instant::now();
        assert!(!d.click(t0));
        assert!(d.click(t0 + Duration::from_millis(200)));
    }

    #[test]
    fn slow_clicks_are_not_a_double_click() {
        let mut d = DoubleClickDetector::default();
        let t0 = Instant::now();
        assert!(!d.click(t0));
        assert!(!d.click(t0 + Duration::from_millis(600)));
    }

    #[test]
    fn triple_click_yields_one_double_click() {
        let mut d = DoubleClickDetector::default();
        let t0 = Instant::now();
        assert!(!d.click(t0));
        assert!(d.click(t0 + Duration::from_millis(100)));
        assert!(!d.click(t0 + Duration::from_millis(200)));
    }

    #[test]
    fn dragging_past_the_threshold_starts_a_move_once() {
        let mut d = DragToMove::default();
        d.moved((100.0, 100.0));
        d.pressed();
        assert!(!d.moved((103.0, 102.0)), "a small wobble is still a click");
        assert!(d.moved((110.0, 100.0)));
        assert!(!d.moved((140.0, 100.0)), "only starts the move once");
    }

    #[test]
    fn moving_without_the_button_held_does_nothing() {
        let mut d = DragToMove::default();
        d.moved((0.0, 0.0));
        assert!(!d.moved((500.0, 500.0)));
        d.pressed();
        d.released();
        assert!(!d.moved((0.0, 0.0)));
    }

    #[test]
    fn a_press_before_any_cursor_position_is_ignored() {
        let mut d = DragToMove::default();
        d.pressed();
        assert!(!d.moved((300.0, 300.0)));
    }
}
