//! Energy-based beat detection on the bass band.
//!
//! A beat is a frame whose bass energy is rising and well above its recent
//! average. Requiring a rise means a sustained loud note fires once, at its
//! start, rather than repeatedly while the average catches up.

use super::bands::smoothing_coef;

/// Energy must exceed the running average by this factor to count as a beat.
const THRESHOLD_RATIO: f32 = 1.5;
/// Ratio at which intensity reaches 1.0.
const MAX_RATIO: f32 = 4.0;
/// Weakest beat intensity reported, so even marginal beats are visible.
const MIN_INTENSITY: f32 = 0.2;
/// Time constant of the running average ("recent" ≈ the last second).
const AVERAGE_SECS: f32 = 1.0;
/// Minimum gap between beats (caps detection at 240 BPM).
const MIN_INTERVAL_SECS: f32 = 0.25;
/// Mean bass power below this (~-60 dBFS) is treated as silence.
const ENERGY_FLOOR: f32 = 1e-6;
/// Energy must be at least this much above the previous frame's to count as rising.
const MIN_RISE: f32 = 1.05;

#[derive(Debug, Clone)]
pub struct BeatDetector {
    average: Option<f32>,
    previous: f32,
    since_beat: f32,
}

impl Default for BeatDetector {
    fn default() -> Self {
        Self {
            average: None,
            previous: 0.0,
            since_beat: MIN_INTERVAL_SECS,
        }
    }
}

impl BeatDetector {
    /// Feeds this frame's bass energy (linear power). Returns the beat
    /// intensity (0.2..=1) if this frame is a beat.
    pub fn update(&mut self, energy: f32, dt: f32) -> Option<f32> {
        self.since_beat += dt;
        let previous = std::mem::replace(&mut self.previous, energy);
        let Some(average) = self.average else {
            self.average = Some(energy);
            return None;
        };

        let ratio = energy / average.max(ENERGY_FLOOR);
        let is_beat = energy > ENERGY_FLOOR
            && energy > previous * MIN_RISE
            && ratio > THRESHOLD_RATIO
            && self.since_beat >= MIN_INTERVAL_SECS;

        // Update after the comparison so a spike doesn't raise its own bar.
        self.average = Some(average + (energy - average) * smoothing_coef(AVERAGE_SECS, dt));

        if !is_beat {
            return None;
        }
        self.since_beat = 0.0;
        let t = ((ratio - THRESHOLD_RATIO) / (MAX_RATIO - THRESHOLD_RATIO)).clamp(0.0, 1.0);
        Some(MIN_INTENSITY + (1.0 - MIN_INTENSITY) * t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn run(d: &mut BeatDetector, energy: f32, secs: f32) -> usize {
        (0..(secs / DT) as usize)
            .filter(|_| d.update(energy, DT).is_some())
            .count()
    }

    #[test]
    fn steady_energy_has_no_beats() {
        let mut d = BeatDetector::default();
        assert_eq!(run(&mut d, 0.01, 3.0), 0);
    }

    #[test]
    fn a_spike_over_steady_energy_is_a_beat() {
        let mut d = BeatDetector::default();
        run(&mut d, 0.01, 2.0);
        let intensity = d.update(0.03, DT).expect("3x the average should be a beat");
        assert!(intensity > MIN_INTENSITY && intensity < 1.0);
    }

    #[test]
    fn huge_spikes_saturate_intensity() {
        let mut d = BeatDetector::default();
        run(&mut d, 0.01, 2.0);
        assert_eq!(d.update(1.0, DT), Some(1.0));
    }

    #[test]
    fn sustained_spike_counts_once() {
        let mut d = BeatDetector::default();
        run(&mut d, 0.01, 2.0);
        assert_eq!(run(&mut d, 0.05, MIN_INTERVAL_SECS * 0.9), 1);
    }

    #[test]
    fn regular_pulses_at_120_bpm_are_all_detected() {
        let mut d = BeatDetector::default();
        run(&mut d, 0.01, 2.0);
        let mut beats = 0;
        for _ in 0..8 {
            beats += run(&mut d, 0.05, 0.1);
            beats += run(&mut d, 0.01, 0.4);
        }
        assert_eq!(beats, 8);
    }

    #[test]
    fn sound_starting_after_silence_beats_once() {
        let mut d = BeatDetector::default();
        run(&mut d, 0.0, 1.0);
        // Ramp up over a few frames, as the FFT window fills, then hold.
        let ramp = [0.002, 0.006, 0.01]
            .iter()
            .filter(|&&e| d.update(e, DT).is_some())
            .count();
        assert_eq!(ramp + run(&mut d, 0.01, 2.0), 1);
    }

    #[test]
    fn silence_never_beats() {
        let mut d = BeatDetector::default();
        run(&mut d, 0.0, 1.0);
        assert_eq!(d.update(ENERGY_FLOOR * 0.5, DT), None);
    }
}
