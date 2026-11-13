//! Turns the captured sample stream into per-frame waveform and spectrum data.

use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex, num_complex::Complex};

use super::CHANNELS;

/// FFT length in samples. 2048 @ 48 kHz gives ~23 Hz bins over ~43 ms.
pub const FFT_SIZE: usize = 2048;
/// Number of spectrum bins (DC through Nyquist).
pub const SPECTRUM_BINS: usize = FFT_SIZE / 2 + 1;
/// Number of most-recent samples exposed as the waveform.
pub const WAVEFORM_LEN: usize = 1024;
/// Magnitudes are clamped to this floor (dBFS) so silence is finite.
pub const DB_FLOOR: f32 = -100.0;

pub struct Analyzer {
    /// Most recent `FFT_SIZE` samples per channel, oldest first.
    left: Vec<f32>,
    right: Vec<f32>,
    /// Mono downmix of the history, refreshed by [`Analyzer::analyze`].
    mono: Vec<f32>,
    incoming: Vec<f32>,
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    /// Scales FFT magnitudes so a full-scale sine reads 0 dBFS.
    magnitude_scale: f32,
    fft_in: Vec<f32>,
    fft_out: Vec<Complex<f32>>,
    fft_scratch: Vec<Complex<f32>>,
    spectrum_db: Vec<f32>,
}

impl Default for Analyzer {
    fn default() -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);
        let window = hann_window(FFT_SIZE);
        let magnitude_scale = 2.0 / window.iter().sum::<f32>();
        Self {
            left: vec![0.0; FFT_SIZE],
            right: vec![0.0; FFT_SIZE],
            mono: vec![0.0; FFT_SIZE],
            incoming: Vec::with_capacity(FFT_SIZE * CHANNELS),
            fft_in: fft.make_input_vec(),
            fft_out: fft.make_output_vec(),
            fft_scratch: fft.make_scratch_vec(),
            fft,
            window,
            magnitude_scale,
            spectrum_db: vec![DB_FLOOR; SPECTRUM_BINS],
        }
    }
}

impl Analyzer {
    /// Moves every queued stereo sample into the history. Returns frames read.
    pub fn ingest(&mut self, consumer: &mut rtrb::Consumer<f32>) -> usize {
        let available = consumer.slots() - consumer.slots() % CHANNELS;
        let Ok(chunk) = consumer.read_chunk(available) else {
            return 0;
        };
        self.incoming.clear();
        self.incoming.extend(chunk);

        let frames = self.incoming.len() / CHANNELS;
        let keep = frames.min(FFT_SIZE);
        let newest = &self.incoming[(frames - keep) * CHANNELS..];
        self.left.copy_within(keep.., 0);
        self.right.copy_within(keep.., 0);
        let tail = FFT_SIZE - keep;
        for (i, [l, r]) in newest.as_chunks::<CHANNELS>().0.iter().enumerate() {
            self.left[tail + i] = *l;
            self.right[tail + i] = *r;
        }
        frames
    }

    /// Recomputes the mono waveform and the spectrum from the current history.
    pub fn analyze(&mut self) {
        for ((m, l), r) in self.mono.iter_mut().zip(&self.left).zip(&self.right) {
            *m = 0.5 * (l + r);
        }
        for ((dst, s), w) in self.fft_in.iter_mut().zip(&self.mono).zip(&self.window) {
            *dst = s * w;
        }
        self.fft
            .process_with_scratch(&mut self.fft_in, &mut self.fft_out, &mut self.fft_scratch)
            .expect("FFT buffers are sized by the planner");
        for (db, c) in self.spectrum_db.iter_mut().zip(&self.fft_out) {
            *db = amplitude_to_db(c.norm() * self.magnitude_scale);
        }
    }

    /// The latest [`WAVEFORM_LEN`] mono samples, oldest first.
    pub fn waveform(&self) -> &[f32] {
        &self.mono[FFT_SIZE - WAVEFORM_LEN..]
    }

    /// Magnitude per FFT bin in dBFS, clamped to [`DB_FLOOR`].
    pub fn spectrum_db(&self) -> &[f32] {
        &self.spectrum_db
    }
}

/// Periodic Hann window (exact for bin-centred tones).
fn hann_window(len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / len as f32).cos())
        .collect()
}

fn amplitude_to_db(amplitude: f32) -> f32 {
    (20.0 * amplitude.log10()).max(DB_FLOOR)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(analyzer: &mut Analyzer, frames: impl IntoIterator<Item = (f32, f32)>) -> usize {
        let (mut p, mut c) = rtrb::RingBuffer::new(1 << 16);
        for (l, r) in frames {
            p.push(l).unwrap();
            p.push(r).unwrap();
        }
        analyzer.ingest(&mut c)
    }

    fn argmax(values: &[f32]) -> usize {
        values
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .unwrap()
    }

    #[test]
    fn full_scale_sine_peaks_at_its_bin_at_0_dbfs() {
        let mut a = Analyzer::default();
        let bin = 100;
        let sine = (0..FFT_SIZE).map(|i| {
            let s = (std::f32::consts::TAU * bin as f32 * i as f32 / FFT_SIZE as f32).sin();
            (s, s)
        });
        feed(&mut a, sine);
        a.analyze();

        assert_eq!(argmax(a.spectrum_db()), bin);
        assert!(
            a.spectrum_db()[bin].abs() < 0.1,
            "peak {} dB",
            a.spectrum_db()[bin]
        );
    }

    #[test]
    fn silence_sits_at_the_floor() {
        let mut a = Analyzer::default();
        feed(&mut a, std::iter::repeat_n((0.0, 0.0), FFT_SIZE));
        a.analyze();
        assert!(a.spectrum_db().iter().all(|&db| db == DB_FLOOR));
    }

    #[test]
    fn waveform_is_the_mono_downmix_of_the_newest_samples() {
        let mut a = Analyzer::default();
        let frames = 3000;
        let read = feed(&mut a, (0..frames).map(|i| (i as f32, i as f32 + 2.0)));
        a.analyze();

        assert_eq!(read, frames);
        let wave = a.waveform();
        assert_eq!(wave.len(), WAVEFORM_LEN);
        assert_eq!(*wave.last().unwrap(), (frames - 1) as f32 + 1.0);
        assert_eq!(wave[0], (frames - WAVEFORM_LEN) as f32 + 1.0);
    }

    #[test]
    fn small_ingests_shift_history_left() {
        let mut a = Analyzer::default();
        feed(&mut a, [(1.0, 1.0), (2.0, 2.0)]);
        feed(&mut a, [(3.0, 3.0)]);
        a.analyze();
        let wave = a.waveform();
        assert_eq!(&wave[WAVEFORM_LEN - 3..], &[1.0, 2.0, 3.0]);
        assert_eq!(wave[WAVEFORM_LEN - 4], 0.0);
    }

    #[test]
    fn empty_buffer_reads_nothing() {
        let mut a = Analyzer::default();
        assert_eq!(feed(&mut a, []), 0);
    }
}
