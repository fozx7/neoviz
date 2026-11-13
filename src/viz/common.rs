//! Helpers shared by visualizers.

use crate::{
    audio::{analysis::AudioFrame, bands::smoothing_coef},
    render::layer::LayerParams,
};

/// A brightness pulse that jumps on each beat and fades out.
#[derive(Debug, Clone, Copy)]
pub struct BeatFlash {
    value: f32,
    decay_secs: f32,
}

impl BeatFlash {
    pub fn new(decay_secs: f32) -> Self {
        Self {
            value: 0.0,
            decay_secs,
        }
    }

    /// Advances by `dt` and returns the new brightness (0..1).
    pub fn update(&mut self, audio: &AudioFrame, dt: f32) -> f32 {
        self.value *= 1.0 - smoothing_coef(self.decay_secs, dt);
        if audio.beat {
            self.value = self.value.max(audio.beat_intensity);
        }
        self.value
    }

    pub fn value(&self) -> f32 {
        self.value
    }
}

/// Scales a waveform so its recent peak reaches a target height: quiet
/// passages stay visible, loud ones don't clip. Rises instantly, relaxes slowly.
#[derive(Debug, Clone, Copy)]
pub struct AutoScale {
    level: f32,
}

/// Quietest peak that gets boosted to full height (avoids blowing up noise).
const AUTO_SCALE_MIN_LEVEL: f32 = 0.05;
const AUTO_SCALE_DECAY_SECS: f32 = 1.5;

impl Default for AutoScale {
    fn default() -> Self {
        Self {
            level: AUTO_SCALE_MIN_LEVEL,
        }
    }
}

impl AutoScale {
    /// Feeds this frame's waveform; returns the factor that maps its recent
    /// peak to `target`.
    pub fn update(&mut self, wave: &[f32], target: f32, dt: f32) -> f32 {
        let peak = wave.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        self.level = peak.max(self.level * (1.0 - smoothing_coef(AUTO_SCALE_DECAY_SECS, dt)));
        target / self.level.max(AUTO_SCALE_MIN_LEVEL)
    }
}

/// Standard layer uniforms from this frame's audio.
pub fn layer_params(
    audio: &AudioFrame,
    size: (u32, u32),
    time: f32,
    dt: f32,
    flash: f32,
) -> LayerParams {
    LayerParams {
        size,
        time,
        dt,
        bass: audio.bass,
        mid: audio.mid,
        treble: audio.treble,
        rms: audio.rms,
        flash,
        beat: audio.beat,
        ..Default::default()
    }
}

/// Small, fast, deterministic random numbers (xorshift32). Visual effects
/// don't need anything stronger, and this avoids a dependency.
#[derive(Debug, Clone)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform in `0.0..1.0`.
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Uniform in `lo..hi`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_stays_in_range_and_varies() {
        let mut r = Rng::new(42);
        let xs: Vec<f32> = (0..1000).map(|_| r.next_f32()).collect();
        assert!(xs.iter().all(|x| (0.0..1.0).contains(x)));
        let mean = xs.iter().sum::<f32>() / xs.len() as f32;
        assert!((mean - 0.5).abs() < 0.05, "mean {mean}");
    }

    #[test]
    fn rng_range_respects_bounds() {
        let mut r = Rng::new(7);
        assert!(
            (0..1000)
                .map(|_| r.range(-2.0, 3.0))
                .all(|x| (-2.0..3.0).contains(&x))
        );
    }

    #[test]
    fn auto_scale_maps_the_peak_to_the_target() {
        let mut a = AutoScale::default();
        assert!((a.update(&[0.5, -0.2], 0.75, 0.016) - 1.5).abs() < 1e-5);
    }

    #[test]
    fn auto_scale_relaxes_slowly_after_loud_audio() {
        let mut a = AutoScale::default();
        a.update(&[1.0], 1.0, 0.016);
        let right_after = a.update(&[0.1], 1.0, 0.016);
        assert!(right_after < 1.1, "should still be near the loud level");
        for _ in 0..600 {
            a.update(&[0.1], 1.0, 0.016);
        }
        assert!((a.update(&[0.1], 1.0, 0.016) - 10.0).abs() < 0.1);
    }

    #[test]
    fn auto_scale_does_not_amplify_silence_without_limit() {
        let mut a = AutoScale::default();
        assert_eq!(a.update(&[0.0], 1.0, 0.016), 1.0 / AUTO_SCALE_MIN_LEVEL);
    }

    #[test]
    fn zero_seed_still_produces_numbers() {
        let mut r = Rng::new(0);
        assert_ne!(r.next_u32(), 0);
    }
}
