//! Turns the captured sample stream into a per-frame [`AudioFrame`].

use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex, num_complex::Complex};

use super::{
    CHANNELS,
    bands::{BandSettings, Bands},
    beat::BeatDetector,
};

/// FFT length in samples. 2048 @ 48 kHz gives ~23 Hz bins over ~43 ms.
pub const FFT_SIZE: usize = 2048;
/// Number of spectrum bins (DC through Nyquist).
pub const SPECTRUM_BINS: usize = FFT_SIZE / 2 + 1;
/// Number of most-recent samples exposed as the waveform.
pub const WAVEFORM_LEN: usize = 1024;
/// Magnitudes are clamped to this floor (dBFS) so silence is finite.
pub const DB_FLOOR: f32 = -100.0;
/// Bands in the default band set handed to visualizers.
pub const DEFAULT_BANDS: usize = 64;
const BAND_MIN_HZ: f32 = 20.0;
const BAND_MAX_HZ: f32 = 16_000.0;
/// Edges for the bass / mid / treble levels, in Hz.
const LEVEL_EDGES: [f32; 4] = [20.0, 250.0, 4_000.0, 16_000.0];
/// Range the beat detector listens to (kick drums and bass lines).
const BEAT_MIN_HZ: f32 = 40.0;
const BEAT_MAX_HZ: f32 = 150.0;
/// RMS below this (~-60 dBFS) counts as silence.
const SILENCE_RMS: f32 = 0.001;
/// Seconds of continuous silence before [`AudioFrame::silent`] turns on.
const SILENCE_AFTER_SECS: f32 = 2.0;
/// Sensitivity limits and step for the up/down keys. Levels span 58 dB, so
/// each step moves the bars by about a tenth of their height.
pub const GAIN_LIMIT_DB: f32 = 24.0;
pub const GAIN_STEP_DB: f32 = 6.0;

/// Everything a visualizer gets about the audio for one frame.
#[derive(Debug, Clone, Copy)]
pub struct AudioFrame<'a> {
    /// Latest [`WAVEFORM_LEN`] mono samples (-1..1), oldest first.
    pub waveform: &'a [f32],
    /// Latest [`WAVEFORM_LEN`] samples per channel, for stereo effects.
    #[allow(dead_code, reason = "for the stereo X/Y scope in milestone 4")]
    pub left: &'a [f32],
    #[allow(dead_code, reason = "for the stereo X/Y scope in milestone 4")]
    pub right: &'a [f32],
    /// Raw magnitude per FFT bin in dBFS (no smoothing or sensitivity).
    pub spectrum_db: &'a [f32],
    /// [`DEFAULT_BANDS`] smoothed levels (0..1), evenly spaced in pitch, bass first.
    pub bands: &'a Bands,
    /// RMS and peak of the waveform, after sensitivity (0..~1).
    pub rms: f32,
    pub peak: f32,
    /// Smoothed energy (0..1) in 20–250 Hz, 250–4k Hz and 4k–16k Hz.
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    /// True on the frame a beat is detected.
    pub beat: bool,
    /// Strength of the latest beat (0.2..=1) on beat frames, else 0.
    pub beat_intensity: f32,
    /// True after [`SILENCE_AFTER_SECS`] of silence.
    pub silent: bool,
    /// Stereo frames captured since the previous analysis (the newest
    /// `new_frames` entries of `left`/`right`, capped at their length).
    #[cfg_attr(not(milkdrop), allow(dead_code))]
    pub new_frames: usize,
}

pub struct Analyzer {
    pub settings: BandSettings,
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
    /// Linear power per bin (amplitude², full-scale sine = 1.0).
    power: Vec<f32>,
    spectrum_db: Vec<f32>,
    bands: Bands,
    levels: Bands,
    beat_detector: BeatDetector,
    rms: f32,
    peak: f32,
    beat_intensity: Option<f32>,
    silence_secs: f32,
    /// Frames ingested since the last `analyze`.
    pending_frames: usize,
    new_frames: usize,
}

impl Default for Analyzer {
    fn default() -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);
        let window = hann_window(FFT_SIZE);
        let magnitude_scale = 2.0 / window.iter().sum::<f32>();
        Self {
            settings: BandSettings::default(),
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
            power: vec![0.0; SPECTRUM_BINS],
            spectrum_db: vec![DB_FLOOR; SPECTRUM_BINS],
            bands: Bands::log_spaced(DEFAULT_BANDS, BAND_MIN_HZ, BAND_MAX_HZ),
            levels: Bands::from_edges(LEVEL_EDGES.to_vec()),
            beat_detector: BeatDetector::default(),
            rms: 0.0,
            peak: 0.0,
            beat_intensity: None,
            silence_secs: 0.0,
            pending_frames: 0,
            new_frames: 0,
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
        self.pending_frames += frames;
        frames
    }

    /// Recomputes everything from the current history, advancing time by `dt` seconds.
    pub fn analyze(&mut self, sample_rate: u32, dt: f32) {
        self.new_frames = std::mem::take(&mut self.pending_frames);
        for ((m, l), r) in self.mono.iter_mut().zip(&self.left).zip(&self.right) {
            *m = 0.5 * (l + r);
        }
        self.compute_spectrum();

        self.bands
            .update(&self.power, sample_rate, &self.settings, dt);
        self.levels
            .update(&self.power, sample_rate, &self.settings, dt);

        let bass_power = mean_power(&self.power, sample_rate, BEAT_MIN_HZ, BEAT_MAX_HZ);
        self.beat_intensity = self.beat_detector.update(bass_power, dt);

        let wave = &self.mono[FFT_SIZE - WAVEFORM_LEN..];
        let raw_rms = (wave.iter().map(|s| s * s).sum::<f32>() / wave.len() as f32).sqrt();
        let raw_peak = wave.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let gain = db_to_amplitude(self.settings.gain_db);
        self.rms = raw_rms * gain;
        self.peak = raw_peak * gain;
        self.silence_secs = if raw_rms < SILENCE_RMS {
            self.silence_secs + dt
        } else {
            0.0
        };
    }

    fn compute_spectrum(&mut self) {
        for ((dst, s), w) in self.fft_in.iter_mut().zip(&self.mono).zip(&self.window) {
            *dst = s * w;
        }
        self.fft
            .process_with_scratch(&mut self.fft_in, &mut self.fft_out, &mut self.fft_scratch)
            .expect("FFT buffers are sized by the planner");
        for ((power, db), c) in self
            .power
            .iter_mut()
            .zip(&mut self.spectrum_db)
            .zip(&self.fft_out)
        {
            let amplitude = c.norm() * self.magnitude_scale;
            *power = amplitude * amplitude;
            *db = amplitude_to_db(amplitude);
        }
    }

    /// The analysis from the last [`Analyzer::analyze`] call.
    pub fn frame(&self) -> AudioFrame<'_> {
        let levels = self.levels.values();
        let start = FFT_SIZE - WAVEFORM_LEN;
        AudioFrame {
            waveform: &self.mono[start..],
            left: &self.left[start..],
            right: &self.right[start..],
            spectrum_db: &self.spectrum_db,
            bands: &self.bands,
            rms: self.rms,
            peak: self.peak,
            bass: levels[0],
            mid: levels[1],
            treble: levels[2],
            beat: self.beat_intensity.is_some(),
            beat_intensity: self.beat_intensity.unwrap_or(0.0),
            silent: self.silence_secs >= SILENCE_AFTER_SECS,
            new_frames: self.new_frames.min(WAVEFORM_LEN),
        }
    }
}

/// Mean linear power of the bins between `lo_hz` and `hi_hz`.
fn mean_power(power: &[f32], sample_rate: u32, lo_hz: f32, hi_hz: f32) -> f32 {
    let hz_per_bin = sample_rate as f32 / FFT_SIZE as f32;
    let last = power.len() - 1;
    let start = ((lo_hz / hz_per_bin).floor() as usize).min(last);
    let end = ((hi_hz / hz_per_bin).ceil() as usize).clamp(start + 1, power.len());
    power[start..end].iter().sum::<f32>() / (end - start) as f32
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

fn db_to_amplitude(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;
    const DT: f32 = 1.0 / 60.0;

    fn feed(analyzer: &mut Analyzer, frames: impl IntoIterator<Item = (f32, f32)>) -> usize {
        let (mut p, mut c) = rtrb::RingBuffer::new(1 << 16);
        for (l, r) in frames {
            p.push(l).unwrap();
            p.push(r).unwrap();
        }
        analyzer.ingest(&mut c)
    }

    /// `frames` of a sine at `hz` with the given amplitude, starting at sample `offset`.
    fn sine(
        hz: f32,
        amplitude: f32,
        offset: usize,
        frames: usize,
    ) -> impl Iterator<Item = (f32, f32)> {
        (offset..offset + frames).map(move |i| {
            let s = amplitude * (std::f32::consts::TAU * hz * i as f32 / RATE as f32).sin();
            (s, s)
        })
    }

    /// Runs `secs` of 60 fps frames; returns how many were beats.
    fn play(a: &mut Analyzer, hz: f32, amplitude: f32, secs: f32) -> usize {
        let per_frame = RATE as usize / 60;
        let mut beats = 0;
        for f in 0..(secs * 60.0) as usize {
            feed(a, sine(hz, amplitude, f * per_frame, per_frame));
            a.analyze(RATE, DT);
            beats += usize::from(a.frame().beat);
        }
        beats
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
        let hz = bin as f32 * RATE as f32 / FFT_SIZE as f32;
        feed(&mut a, sine(hz, 1.0, 0, FFT_SIZE));
        a.analyze(RATE, DT);

        let spectrum = a.frame().spectrum_db;
        assert_eq!(argmax(spectrum), bin);
        assert!(spectrum[bin].abs() < 0.1, "peak {} dB", spectrum[bin]);
    }

    #[test]
    fn silence_sits_at_the_floor() {
        let mut a = Analyzer::default();
        feed(&mut a, std::iter::repeat_n((0.0, 0.0), FFT_SIZE));
        a.analyze(RATE, DT);
        assert!(a.frame().spectrum_db.iter().all(|&db| db == DB_FLOOR));
    }

    #[test]
    fn waveform_is_the_mono_downmix_of_the_newest_samples() {
        let mut a = Analyzer::default();
        let frames = 3000;
        let read = feed(&mut a, (0..frames).map(|i| (i as f32, i as f32 + 2.0)));
        a.analyze(RATE, DT);

        assert_eq!(read, frames);
        let f = a.frame();
        assert_eq!(f.waveform.len(), WAVEFORM_LEN);
        assert_eq!(*f.waveform.last().unwrap(), (frames - 1) as f32 + 1.0);
        assert_eq!(f.waveform[0], (frames - WAVEFORM_LEN) as f32 + 1.0);
        assert_eq!(*f.right.last().unwrap(), (frames - 1) as f32 + 2.0);
    }

    #[test]
    fn small_ingests_shift_history_left() {
        let mut a = Analyzer::default();
        feed(&mut a, [(1.0, 1.0), (2.0, 2.0)]);
        feed(&mut a, [(3.0, 3.0)]);
        a.analyze(RATE, DT);
        let wave = a.frame().waveform;
        assert_eq!(&wave[WAVEFORM_LEN - 3..], &[1.0, 2.0, 3.0]);
        assert_eq!(wave[WAVEFORM_LEN - 4], 0.0);
    }

    #[test]
    fn empty_buffer_reads_nothing() {
        let mut a = Analyzer::default();
        assert_eq!(feed(&mut a, []), 0);
    }

    #[test]
    fn bass_tone_raises_bass_not_treble() {
        let mut a = Analyzer::default();
        play(&mut a, 80.0, 0.5, 0.5);
        let f = a.frame();
        assert!(f.bass > 0.5, "bass {}", f.bass);
        assert!(f.treble < 0.05, "treble {}", f.treble);
        assert!(f.bands.values()[..8].iter().any(|&v| v > 0.5));
    }

    #[test]
    fn rms_of_a_sine_is_amplitude_over_root_two() {
        let mut a = Analyzer::default();
        play(&mut a, 440.0, 0.5, 0.1);
        let f = a.frame();
        assert!((f.rms - 0.5 / 2f32.sqrt()).abs() < 0.01, "rms {}", f.rms);
        assert!((f.peak - 0.5).abs() < 0.01, "peak {}", f.peak);
    }

    #[test]
    fn gain_scales_levels() {
        let mut a = Analyzer::default();
        a.settings.gain_db = 6.0;
        play(&mut a, 440.0, 0.25, 0.1);
        assert!((a.frame().peak - 0.5).abs() < 0.01);
    }

    #[test]
    fn steady_bass_beats_once_at_its_start() {
        let mut a = Analyzer::default();
        assert_eq!(play(&mut a, 60.0, 0.05, 2.0), 1);
        assert_eq!(play(&mut a, 60.0, 0.05, 2.0), 0);
    }

    #[test]
    fn kick_after_quiet_bass_is_a_beat() {
        let mut a = Analyzer::default();
        play(&mut a, 60.0, 0.05, 2.0);
        assert_eq!(play(&mut a, 60.0, 0.5, 0.1), 1);
    }

    #[test]
    fn treble_bursts_are_not_beats() {
        let mut a = Analyzer::default();
        play(&mut a, 60.0, 0.05, 2.0);
        assert_eq!(play(&mut a, 6000.0, 0.5, 0.1), 0);
    }

    #[test]
    fn goes_silent_after_a_pause() {
        let mut a = Analyzer::default();
        play(&mut a, 440.0, 0.5, 0.5);
        assert!(!a.frame().silent);

        play(&mut a, 440.0, 0.0, SILENCE_AFTER_SECS * 0.5);
        assert!(!a.frame().silent, "too eager");
        play(&mut a, 440.0, 0.0, SILENCE_AFTER_SECS);
        assert!(a.frame().silent);

        play(&mut a, 440.0, 0.5, 0.1);
        assert!(!a.frame().silent);
        assert_eq!(a.silence_secs, 0.0);
    }

    #[test]
    fn counts_new_frames_since_the_last_analysis() {
        let mut a = Analyzer::default();
        feed(&mut a, std::iter::repeat_n((0.0, 0.0), 300));
        feed(&mut a, std::iter::repeat_n((0.0, 0.0), 500));
        a.analyze(RATE, DT);
        assert_eq!(a.frame().new_frames, 800);
        a.analyze(RATE, DT);
        assert_eq!(a.frame().new_frames, 0);
        feed(&mut a, std::iter::repeat_n((0.0, 0.0), 5000));
        a.analyze(RATE, DT);
        assert_eq!(a.frame().new_frames, WAVEFORM_LEN, "capped to what's kept");
    }
}
