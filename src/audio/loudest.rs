//! Finds the loudest frequency in the spectrum, for the debug readouts.

use super::analysis::FFT_SIZE;

/// Lowest frequency considered (below this is mostly rumble and DC).
const MIN_HZ: f32 = 20.0;
/// Peaks quieter than this count as silence.
const QUIET_DB: f32 = -70.0;
/// The readout keeps the loudest peak seen over this long, so it's readable.
const HOLD_SECS: f32 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Peak {
    pub hz: f32,
    pub db: f32,
}

/// The loudest bin from 20 Hz to Nyquist in `spectrum_db`, refined between
/// bins, or `None` if everything is quiet.
pub fn loudest(spectrum_db: &[f32], sample_rate: u32) -> Option<Peak> {
    let hz_per_bin = sample_rate as f32 / FFT_SIZE as f32;
    let last = spectrum_db.len().checked_sub(1)?;
    let first = ((MIN_HZ / hz_per_bin).ceil() as usize).max(1);
    let (bin, &db) = spectrum_db
        .iter()
        .enumerate()
        .take(last)
        .skip(first)
        .max_by(|a, b| a.1.total_cmp(b.1))?;
    if db < QUIET_DB {
        return None;
    }
    // Fit a parabola through the peak and its neighbours.
    let (a, c) = (spectrum_db[bin - 1], spectrum_db[bin + 1]);
    let curve = a - 2.0 * db + c;
    let offset = if curve < 0.0 {
        (0.5 * (a - c) / curve).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    Some(Peak {
        hz: (bin as f32 + offset) * hz_per_bin,
        db: db - 0.25 * (a - c) * offset,
    })
}

/// The nearest note and how far off it is, e.g. ("A2", 3) for 110.2 Hz.
pub fn note(hz: f32) -> (String, i32) {
    const NAMES: [&str; 12] = [
        "C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B",
    ];
    let midi = 69.0 + 12.0 * (hz / 440.0).log2();
    let nearest = midi.round();
    let cents = ((midi - nearest) * 100.0).round() as i32;
    let n = nearest as i32;
    let name = format!(
        "{}{}",
        NAMES[n.rem_euclid(12) as usize],
        n.div_euclid(12) - 1
    );
    (name, cents)
}

/// The loudest peak, held for a moment so the number doesn't flicker.
#[derive(Debug, Clone, Default)]
pub struct Readout {
    shown: Option<Peak>,
    best: Option<Peak>,
    elapsed: f32,
}

impl Readout {
    pub fn shown(&self) -> Option<Peak> {
        self.shown
    }

    /// Takes this frame's peak; every [`HOLD_SECS`] shows the loudest seen.
    pub fn update(&mut self, peak: Option<Peak>, dt: f32) {
        if let Some(p) = peak
            && self.best.is_none_or(|b| p.db > b.db)
        {
            self.best = Some(p);
        }
        self.elapsed += dt;
        if self.elapsed >= HOLD_SECS {
            self.shown = self.best.take();
            self.elapsed = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::analysis::{DB_FLOOR, SPECTRUM_BINS};

    const RATE: u32 = 48_000;

    fn hz_per_bin() -> f32 {
        RATE as f32 / FFT_SIZE as f32
    }

    fn silence() -> Vec<f32> {
        vec![DB_FLOOR; SPECTRUM_BINS]
    }

    #[test]
    fn finds_a_peak_on_a_bin() {
        let mut s = silence();
        s[47] = -10.0;
        let p = loudest(&s, RATE).unwrap();
        assert!((p.hz - 47.0 * hz_per_bin()).abs() < 0.01, "{}", p.hz);
        assert_eq!(p.db, -10.0);
    }

    #[test]
    fn refines_a_peak_between_bins() {
        let mut s = silence();
        s[47] = -10.0;
        s[48] = -12.0;
        s[46] = -20.0;
        let p = loudest(&s, RATE).unwrap();
        let bin = p.hz / hz_per_bin();
        assert!(bin > 47.0 && bin < 47.5, "{bin}");
        assert!(p.db >= -10.0);
    }

    #[test]
    fn quiet_spectrum_has_no_peak() {
        let mut s = silence();
        s[100] = QUIET_DB - 1.0;
        assert_eq!(loudest(&s, RATE), None);
    }

    #[test]
    fn ignores_rumble_below_20_hz() {
        let mut s = silence();
        s[0] = 0.0;
        s[200] = -30.0;
        assert!((loudest(&s, RATE).unwrap().hz - 200.0 * hz_per_bin()).abs() < 0.01);
    }

    #[test]
    fn names_notes_with_cents() {
        assert_eq!(note(440.0), ("A4".to_owned(), 0));
        assert_eq!(note(110.0), ("A2".to_owned(), 0));
        assert_eq!(note(261.63), ("C4".to_owned(), 0));
        let (name, cents) = note(445.0);
        assert_eq!(name, "A4");
        assert!((19..=20).contains(&cents), "{cents}");
        assert_eq!(note(27.5).0, "A0");
    }

    #[test]
    fn readout_holds_the_loudest_peak_of_each_window() {
        let mut r = Readout::default();
        let quiet = Peak {
            hz: 100.0,
            db: -30.0,
        };
        let loud = Peak {
            hz: 200.0,
            db: -10.0,
        };
        r.update(Some(quiet), 0.1);
        r.update(Some(loud), 0.1);
        assert_eq!(r.shown(), None, "nothing until the window ends");
        r.update(Some(quiet), 0.1);
        assert_eq!(r.shown(), Some(loud));
        r.update(None, HOLD_SECS);
        assert_eq!(r.shown(), None, "silence clears it");
    }
}
