//! Fading the picture to black when the music stops, and back in when it
//! returns (and at launch).
//!
//! "Silent" already means a couple of seconds below the silence threshold
//! (see `AudioFrame::silent`), so short gaps between songs don't trigger it.

/// How long the picture takes to fade out once the audio has gone silent.
pub const FADE_OUT_SECS: f32 = 3.0;
/// How long it takes to come back when sound returns. Quicker than the fade
/// out, so the music never plays to a dark screen for long.
pub const FADE_IN_SECS: f32 = 0.75;

/// Picture brightness from 0 (black) to 1 (full), eased at both ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SilenceFade {
    /// Linear progress 0..1; [`Self::brightness`] eases it.
    level: f32,
}

impl Default for SilenceFade {
    /// Starts black, so the first frames fade in.
    fn default() -> Self {
        Self { level: 0.0 }
    }
}

impl SilenceFade {
    /// Call once per frame. With `enabled` off the picture stays at full.
    pub fn update(&mut self, enabled: bool, silent: bool, dt: f32) {
        let fading_out = enabled && silent;
        self.level = if fading_out {
            (self.level - dt / FADE_OUT_SECS).max(0.0)
        } else {
            (self.level + dt / FADE_IN_SECS).min(1.0)
        };
    }

    /// The brightness to multiply the picture by.
    pub fn brightness(&self) -> f32 {
        let t = self.level;
        t * t * (3.0 - 2.0 * t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(fade: &mut SilenceFade, enabled: bool, silent: bool, secs: f32) {
        let dt = 1.0 / 60.0;
        for _ in 0..(secs / dt).round() as usize {
            fade.update(enabled, silent, dt);
        }
    }

    #[test]
    fn fades_in_at_launch() {
        let mut fade = SilenceFade::default();
        assert_eq!(fade.brightness(), 0.0);
        run(&mut fade, true, false, FADE_IN_SECS);
        assert!(fade.brightness() > 0.99);
    }

    #[test]
    fn fades_out_gradually_on_silence() {
        let mut fade = SilenceFade::default();
        run(&mut fade, true, false, 1.0);
        run(&mut fade, true, true, FADE_OUT_SECS / 2.0);
        let halfway = fade.brightness();
        assert!(halfway > 0.2 && halfway < 0.8, "halfway was {halfway}");
        run(&mut fade, true, true, FADE_OUT_SECS);
        assert_eq!(fade.brightness(), 0.0);
    }

    #[test]
    fn sound_returning_fades_back_in() {
        let mut fade = SilenceFade::default();
        run(&mut fade, true, true, 10.0);
        run(&mut fade, true, false, FADE_IN_SECS);
        assert!(fade.brightness() > 0.99);
    }

    #[test]
    fn disabled_stays_bright_in_silence() {
        let mut fade = SilenceFade::default();
        run(&mut fade, false, true, 10.0);
        assert!(fade.brightness() > 0.99);
    }
}
