//! Groups FFT bins into frequency bands with smoothing and falling peak caps.

use super::analysis::FFT_SIZE;

/// Tilt applied so a typical mix (which loses energy with frequency) looks
/// roughly flat: dB added per octave above 1 kHz (subtracted below).
const TILT_DB_PER_OCTAVE: f32 = 3.0;
const TILT_PIVOT_HZ: f32 = 1000.0;
/// Level that maps to 0.0 (empty) and 1.0 (full) before sensitivity is applied.
const FLOOR_DB: f32 = -70.0;
const CEILING_DB: f32 = -12.0;
/// Rise time constant; short so transients show immediately.
const ATTACK_SECS: f32 = 0.01;
/// How long a peak cap sits still before falling.
const PEAK_HOLD_SECS: f32 = 0.5;
/// Peak caps fall with this acceleration (full heights per second²).
const PEAK_GRAVITY: f32 = 3.0;
/// Power floor so log10 of silence stays finite.
const MIN_POWER: f32 = 1e-12;

/// User-tunable response.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandSettings {
    /// Added to every level before mapping to 0..1 (sensitivity).
    pub gain_db: f32,
    /// Fall time constant in seconds: higher is smoother and slower.
    pub decay_secs: f32,
}

impl Default for BandSettings {
    fn default() -> Self {
        Self {
            gain_db: 0.0,
            decay_secs: 0.15,
        }
    }
}

/// How one band reads its power from the spectrum.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Source {
    /// Mean power of bins `start..end`.
    Bins { start: usize, end: usize },
    /// Band is narrower than a bin: interpolate at its centre.
    Interpolate { bin: usize, frac: f32 },
}

/// A set of bands between fixed frequency edges.
#[derive(Debug, Clone)]
pub struct Bands {
    /// `count + 1` ascending edges in Hz.
    edges: Vec<f32>,
    /// Per-band tilt in dB, from each band's centre frequency.
    tilt_db: Vec<f32>,
    /// Cached bin mapping and the sample rate it was built for.
    sources: Vec<Source>,
    sources_rate: u32,
    values: Vec<f32>,
    peaks: Vec<PeakCap>,
}

#[derive(Debug, Clone, Copy, Default)]
struct PeakCap {
    level: f32,
    hold: f32,
    velocity: f32,
}

impl Bands {
    /// `count` bands spaced evenly in pitch between `min_hz` and `max_hz`.
    pub fn log_spaced(count: usize, min_hz: f32, max_hz: f32) -> Self {
        assert!(count > 0 && min_hz > 0.0 && max_hz > min_hz);
        let ratio = max_hz / min_hz;
        let edges = (0..=count)
            .map(|i| min_hz * ratio.powf(i as f32 / count as f32))
            .collect();
        Self::from_edges(edges)
    }

    /// Bands between consecutive `edges` (ascending, in Hz).
    pub fn from_edges(edges: Vec<f32>) -> Self {
        assert!(edges.len() >= 2 && edges.windows(2).all(|w| w[0] < w[1]));
        let count = edges.len() - 1;
        let tilt_db = edges
            .windows(2)
            .map(|w| TILT_DB_PER_OCTAVE * ((w[0] * w[1]).sqrt() / TILT_PIVOT_HZ).log2())
            .collect();
        Self {
            edges,
            tilt_db,
            sources: Vec::new(),
            sources_rate: 0,
            values: vec![0.0; count],
            peaks: vec![PeakCap::default(); count],
        }
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// The `len() + 1` band edges in Hz, ascending.
    pub fn edges(&self) -> &[f32] {
        &self.edges
    }

    /// Smoothed levels, 0..1, lowest band first.
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    /// Peak cap per band, 0..1. Always >= the matching value.
    pub fn peaks(&self) -> impl ExactSizeIterator<Item = f32> + '_ {
        self.peaks.iter().map(|p| p.level)
    }

    /// Advances by `dt` seconds using a power spectrum of `FFT_SIZE / 2 + 1` bins.
    pub fn update(&mut self, power: &[f32], sample_rate: u32, settings: &BandSettings, dt: f32) {
        if self.sources_rate != sample_rate {
            self.sources = map_to_bins(&self.edges, sample_rate, power.len());
            self.sources_rate = sample_rate;
        }
        let attack = smoothing_coef(ATTACK_SECS, dt);
        let decay = smoothing_coef(settings.decay_secs, dt);

        for i in 0..self.len() {
            let db = 10.0 * read_power(power, self.sources[i]).max(MIN_POWER).log10();
            let target = normalize(db + self.tilt_db[i] + settings.gain_db);
            let value = &mut self.values[i];
            let coef = if target > *value { attack } else { decay };
            *value += (target - *value) * coef;
            self.peaks[i].update(*value, dt);
        }
    }
}

impl PeakCap {
    fn update(&mut self, value: f32, dt: f32) {
        if value >= self.level {
            *self = Self {
                level: value,
                hold: PEAK_HOLD_SECS,
                velocity: 0.0,
            };
        } else if self.hold > 0.0 {
            self.hold -= dt;
        } else {
            self.velocity += PEAK_GRAVITY * dt;
            self.level = (self.level - self.velocity * dt).max(value);
        }
    }
}

/// Fraction to move toward a target this frame for an exponential time constant.
pub fn smoothing_coef(time_constant: f32, dt: f32) -> f32 {
    if time_constant <= 0.0 {
        return 1.0;
    }
    1.0 - (-dt / time_constant).exp()
}

fn normalize(db: f32) -> f32 {
    ((db - FLOOR_DB) / (CEILING_DB - FLOOR_DB)).clamp(0.0, 1.0)
}

fn map_to_bins(edges: &[f32], sample_rate: u32, bins: usize) -> Vec<Source> {
    let hz_per_bin = sample_rate as f32 / FFT_SIZE as f32;
    let last = bins - 1;
    edges
        .windows(2)
        .map(|w| {
            // Bins whose centre lies in [lo, hi).
            let start = ((w[0] / hz_per_bin).ceil() as usize).min(last);
            let end = ((w[1] / hz_per_bin).ceil() as usize).min(bins);
            if end > start {
                Source::Bins { start, end }
            } else {
                let centre = (w[0] * w[1]).sqrt() / hz_per_bin;
                let bin = (centre.floor() as usize).min(last - 1);
                Source::Interpolate {
                    bin,
                    frac: (centre - bin as f32).clamp(0.0, 1.0),
                }
            }
        })
        .collect()
}

fn read_power(power: &[f32], source: Source) -> f32 {
    match source {
        Source::Bins { start, end } => power[start..end].iter().sum::<f32>() / (end - start) as f32,
        Source::Interpolate { bin, frac } => power[bin] + (power[bin + 1] - power[bin]) * frac,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::analysis::SPECTRUM_BINS;

    const RATE: u32 = 48_000;
    const DT: f32 = 1.0 / 60.0;

    fn hz_per_bin() -> f32 {
        RATE as f32 / FFT_SIZE as f32
    }

    /// A spectrum with a single loud bin at `hz`, silence elsewhere.
    fn tone(hz: f32, power: f32) -> Vec<f32> {
        let mut p = vec![0.0; SPECTRUM_BINS];
        p[(hz / hz_per_bin()).round() as usize] = power;
        p
    }

    fn settle(bands: &mut Bands, power: &[f32], secs: f32) {
        for _ in 0..(secs / DT) as usize {
            bands.update(power, RATE, &BandSettings::default(), DT);
        }
    }

    #[test]
    fn log_spaced_edges_cover_the_range() {
        let b = Bands::log_spaced(32, 20.0, 20_000.0);
        assert_eq!(b.len(), 32);
        assert!((b.edges[0] - 20.0).abs() < 1e-3);
        assert!((b.edges[32] - 20_000.0).abs() < 1.0);
        // Equal ratio between neighbours = evenly spaced in pitch.
        let r0 = b.edges[1] / b.edges[0];
        let r1 = b.edges[20] / b.edges[19];
        assert!((r0 - r1).abs() < 1e-3);
    }

    #[test]
    fn a_tone_lights_only_its_own_band() {
        let mut b = Bands::log_spaced(16, 20.0, 20_000.0);
        settle(&mut b, &tone(1000.0, 1.0), 0.5);
        let loudest = b
            .values()
            .iter()
            .enumerate()
            .max_by(|x, y| x.1.total_cmp(y.1))
            .unwrap()
            .0;
        assert!(b.edges[loudest] <= 1000.0 && 1000.0 < b.edges[loudest + 1]);
        assert!(b.values()[loudest] > 0.9);
        assert_eq!(b.values()[0], 0.0);
    }

    #[test]
    fn narrow_low_bands_interpolate_instead_of_reading_nothing() {
        // 128 bands from 20 Hz: the lowest are far narrower than a 23 Hz bin.
        let b = Bands::log_spaced(128, 20.0, 20_000.0);
        let sources = map_to_bins(&b.edges, RATE, SPECTRUM_BINS);
        assert!(matches!(sources[0], Source::Interpolate { .. }));
        assert!(matches!(sources[127], Source::Bins { .. }));

        let flat = vec![1e-4; SPECTRUM_BINS];
        let mut b = b;
        settle(&mut b, &flat, 0.5);
        assert!(b.values()[0] > 0.0);
    }

    #[test]
    fn rises_fast_and_falls_slowly() {
        let mut b = Bands::log_spaced(4, 20.0, 20_000.0);
        let loud = vec![1.0; SPECTRUM_BINS];
        let silent = vec![0.0; SPECTRUM_BINS];

        b.update(&loud, RATE, &BandSettings::default(), DT);
        assert!(b.values()[2] > 0.8, "attack too slow: {}", b.values()[2]);

        settle(&mut b, &loud, 0.2);
        b.update(&silent, RATE, &BandSettings::default(), DT);
        assert!(b.values()[2] > 0.8, "decay too fast: {}", b.values()[2]);

        settle(&mut b, &silent, 2.0);
        assert!(b.values()[2] < 0.01);
    }

    #[test]
    fn peak_caps_hold_then_fall_but_never_below_the_bar() {
        let mut b = Bands::log_spaced(4, 20.0, 20_000.0);
        settle(&mut b, &vec![1.0; SPECTRUM_BINS], 0.2);
        let top = b.peaks().nth(2).unwrap();

        let silent = vec![0.0; SPECTRUM_BINS];
        settle(&mut b, &silent, PEAK_HOLD_SECS * 0.8);
        assert_eq!(b.peaks().nth(2).unwrap(), top, "should still be holding");

        settle(&mut b, &silent, 1.5);
        assert!(b.peaks().nth(2).unwrap() < top);
        for (v, p) in b.values().iter().zip(b.peaks()) {
            assert!(p >= *v);
        }
    }

    #[test]
    fn gain_raises_levels() {
        let quiet = vec![1e-6; SPECTRUM_BINS];
        let run = |gain_db| {
            let mut b = Bands::log_spaced(4, 20.0, 20_000.0);
            let s = BandSettings {
                gain_db,
                ..Default::default()
            };
            for _ in 0..60 {
                b.update(&quiet, RATE, &s, DT);
            }
            b.values()[2]
        };
        assert!(run(12.0) > run(0.0));
    }

    #[test]
    fn rebuilds_bin_mapping_when_sample_rate_changes() {
        let mut b = Bands::log_spaced(8, 20.0, 20_000.0);
        let p = vec![0.0; SPECTRUM_BINS];
        b.update(&p, 48_000, &BandSettings::default(), DT);
        let at_48k = b.sources.clone();
        b.update(&p, 44_100, &BandSettings::default(), DT);
        assert_ne!(b.sources, at_48k);
    }

    #[test]
    fn custom_edges_make_bass_mid_treble() {
        let mut b = Bands::from_edges(vec![20.0, 250.0, 4000.0, 16_000.0]);
        settle(&mut b, &tone(100.0, 1.0), 0.3);
        assert!(b.values()[0] > 0.0);
        assert_eq!(b.values()[1], 0.0);
        assert_eq!(b.values()[2], 0.0);
    }
}
