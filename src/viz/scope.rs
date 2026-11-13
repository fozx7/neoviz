//! Oscilloscope Wave: a glowing waveform with feedback trails.

use std::f32::consts::TAU;

use crate::{
    audio::{analysis::AudioFrame, bands::smoothing_coef},
    render::{
        feedback::{Feedback, Warp},
        lines::{LineRenderer, LineStyle},
    },
};

use super::{Visualizer, color::hsv};

/// Samples searched for a rising zero crossing to start the line on, so a
/// steady tone stands still instead of jittering sideways.
const TRIGGER_SEARCH: usize = 256;
/// Samples drawn by the line variant after the trigger point.
const LINE_SAMPLES: usize = 768;
/// The waveform is scaled so its recent peak reaches this height (y is -1..1).
const TARGET_HEIGHT: f32 = 0.75;
/// Quietest peak the auto-scaling will boost to full height (avoids blowing up noise).
const MIN_LEVEL: f32 = 0.05;
/// How fast the auto-scaling relaxes after a loud passage.
const LEVEL_DECAY_SECS: f32 = 1.5;
const FLASH_DECAY_SECS: f32 = 0.2;
/// Hue drift in full colour-wheel turns per second.
const HUE_SPEED: f32 = 0.02;
const RING_RADIUS: f32 = 0.5;
const RING_AMPLITUDE: f32 = 0.3;
/// Fraction of the screen width the line variant spans.
const LINE_WIDTH: f32 = 0.95;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeVariant {
    /// Classic horizontal trace.
    Line,
    /// The waveform wrapped around a circle.
    Ring,
    /// Left vs right as a goniometer (Lissajous): mono is a vertical line.
    StereoXy,
}

impl ScopeVariant {
    fn name(self) -> &'static str {
        match self {
            Self::Line => "Oscilloscope",
            Self::Ring => "Ring Scope",
            Self::StereoXy => "Stereo X/Y",
        }
    }

    fn warp(self) -> Warp {
        match self {
            Self::Line => Warp {
                zoom_per_sec: 1.3,
                rotation_per_sec: 0.0,
                fade_secs: 0.12,
            },
            Self::Ring => Warp {
                zoom_per_sec: 1.8,
                rotation_per_sec: 0.4,
                fade_secs: 0.35,
            },
            Self::StereoXy => Warp {
                zoom_per_sec: 0.85,
                rotation_per_sec: -0.2,
                fade_secs: 0.4,
            },
        }
    }

    /// Starting hue, so each variant has its own colour.
    fn base_hue(self) -> f32 {
        match self {
            Self::Line => 0.35,
            Self::Ring => 0.55,
            Self::StereoXy => 0.85,
        }
    }
}

pub struct Scope {
    variant: ScopeVariant,
    lines: LineRenderer,
    feedback: Feedback,
    points: Vec<[f32; 2]>,
    aspect: f32,
    /// Recent waveform peak, for auto-scaling.
    level: f32,
    time: f32,
    flash: f32,
    dt: f32,
}

impl Scope {
    pub fn new(device: &wgpu::Device, size: (u32, u32), variant: ScopeVariant) -> Self {
        Self {
            variant,
            // Ring closes the loop with one extra point.
            lines: LineRenderer::new(device, crate::audio::analysis::WAVEFORM_LEN + 1),
            feedback: Feedback::new(device, size),
            points: Vec::new(),
            aspect: aspect(size),
            level: MIN_LEVEL,
            time: 0.0,
            flash: 0.0,
            dt: 0.0,
        }
    }

    fn style(&self) -> LineStyle {
        let hue = self.variant.base_hue() + self.time * HUE_SPEED;
        let brightness = 0.8 + 0.7 * self.flash;
        LineStyle {
            color: hsv(hue, 0.65, brightness),
            core_width: 0.0025,
            glow_width: 0.012,
        }
    }
}

impl Visualizer for Scope {
    fn name(&self) -> &str {
        self.variant.name()
    }

    fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        self.feedback.resize(device, size);
        self.aspect = aspect(size);
    }

    fn update(&mut self, audio: &AudioFrame, dt: f32) {
        self.dt = dt;
        self.time += dt;
        self.flash *= 1.0 - smoothing_coef(FLASH_DECAY_SECS, dt);
        if audio.beat {
            self.flash = self.flash.max(audio.beat_intensity);
        }

        let peak = audio.waveform.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        self.level = peak.max(self.level * (1.0 - smoothing_coef(LEVEL_DECAY_SECS, dt)));
        let scale = TARGET_HEIGHT / self.level.max(MIN_LEVEL);

        self.points.clear();
        match self.variant {
            ScopeVariant::Line => line_points(audio.waveform, scale, self.aspect, &mut self.points),
            ScopeVariant::Ring => ring_points(audio.waveform, scale, &mut self.points),
            ScopeVariant::StereoXy => xy_points(audio.left, audio.right, scale, &mut self.points),
        }
    }

    fn render(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        self.feedback
            .warp(queue, encoder, &self.variant.warp(), self.dt);
        self.lines.set(queue, &self.points, &self.style(), size);
        self.lines.draw(encoder, self.feedback.current());
        self.feedback.finish(queue, encoder, target);
    }
}

fn aspect(size: (u32, u32)) -> f32 {
    size.0.max(1) as f32 / size.1.max(1) as f32
}

/// Index of the first rising zero crossing in `samples[..search]`, or 0.
fn find_trigger(samples: &[f32], search: usize) -> usize {
    let search = search.min(samples.len());
    (1..search)
        .find(|&i| samples[i - 1] < 0.0 && samples[i] >= 0.0)
        .unwrap_or(0)
}

fn line_points(wave: &[f32], scale: f32, aspect: f32, out: &mut Vec<[f32; 2]>) {
    let start = find_trigger(wave, TRIGGER_SEARCH);
    let shown = &wave[start..(start + LINE_SAMPLES).min(wave.len())];
    let half_width = aspect * LINE_WIDTH;
    let last = (shown.len().max(2) - 1) as f32;
    out.extend(shown.iter().enumerate().map(|(i, s)| {
        let x = -half_width + 2.0 * half_width * i as f32 / last;
        [x, (s * scale).clamp(-1.0, 1.0)]
    }));
}

fn ring_points(wave: &[f32], scale: f32, out: &mut Vec<[f32; 2]>) {
    let n = wave.len() as f32;
    out.extend(wave.iter().enumerate().map(|(i, s)| {
        let angle = TAU * i as f32 / n;
        let r = RING_RADIUS + RING_AMPLITUDE * (s * scale / TARGET_HEIGHT).clamp(-1.0, 1.0);
        [r * angle.cos(), r * angle.sin()]
    }));
    if let Some(&first) = out.first() {
        out.push(first);
    }
}

/// Goniometer: x = side (L - R), y = mid (L + R), each scaled by 1/√2.
fn xy_points(left: &[f32], right: &[f32], scale: f32, out: &mut Vec<[f32; 2]>) {
    let k = scale * std::f32::consts::FRAC_1_SQRT_2;
    out.extend(left.iter().zip(right).map(|(l, r)| {
        [
            ((l - r) * k).clamp(-1.0, 1.0),
            ((l + r) * k).clamp(-1.0, 1.0),
        ]
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_finds_the_first_rising_zero_crossing() {
        let wave = [0.5, 0.1, -0.2, -0.4, 0.0, 0.3, -0.1, 0.2];
        assert_eq!(find_trigger(&wave, 8), 4);
    }

    #[test]
    fn trigger_ignores_falling_crossings() {
        assert_eq!(find_trigger(&[0.5, -0.5, -0.2], 3), 0);
    }

    #[test]
    fn trigger_only_searches_the_window() {
        let wave = [0.1, 0.2, 0.3, -0.1, 0.1];
        assert_eq!(find_trigger(&wave, 3), 0);
        assert_eq!(find_trigger(&wave, 5), 4);
    }

    #[test]
    fn line_spans_the_width_and_starts_at_the_trigger() {
        let wave: Vec<f32> = (0..1024).map(|i| (i as f32 * 0.05).sin()).collect();
        let mut pts = Vec::new();
        line_points(&wave, 1.0, 2.0, &mut pts);
        assert_eq!(pts.len(), LINE_SAMPLES);
        assert!((pts[0][0] + 2.0 * LINE_WIDTH).abs() < 1e-5);
        assert!((pts.last().unwrap()[0] - 2.0 * LINE_WIDTH).abs() < 1e-5);
        assert!(pts[0][1].abs() < 0.05, "should start near zero");
    }

    #[test]
    fn ring_is_closed() {
        let wave = vec![0.2; 64];
        let mut pts = Vec::new();
        ring_points(&wave, 1.0, &mut pts);
        assert_eq!(pts.len(), 65);
        assert_eq!(pts[0], pts[64]);
    }

    #[test]
    fn mono_is_a_vertical_line_on_the_goniometer() {
        let mono = [0.1, -0.3, 0.5];
        let mut pts = Vec::new();
        xy_points(&mono, &mono, 1.0, &mut pts);
        assert!(pts.iter().all(|p| p[0] == 0.0));
    }

    #[test]
    fn opposite_channels_are_horizontal() {
        let mut pts = Vec::new();
        xy_points(&[0.4], &[-0.4], 1.0, &mut pts);
        assert_eq!(pts[0][1], 0.0);
        assert!(pts[0][0] > 0.0);
    }

    #[test]
    fn points_are_clamped_to_the_screen() {
        let mut pts = Vec::new();
        line_points(&[10.0; 1024], 1.0, 1.0, &mut pts);
        assert!(pts.iter().all(|p| p[1] <= 1.0));
    }
}
