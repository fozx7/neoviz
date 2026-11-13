//! Classic Lightshow: slowly morphing ribbons that ripple with the waveform,
//! and a waveform "bloom" ring that bursts outward on beats, all over a
//! flowing feedback trail.

use std::f32::consts::TAU;

use crate::{
    audio::analysis::{AudioFrame, WAVEFORM_LEN},
    render::{
        feedback::{Feedback, Warp},
        lines::{LineRenderer, LineStyle},
    },
};

use super::{
    Visualizer,
    color::hsv,
    common::{AutoScale, BeatFlash},
    scope::{RING_RADIUS, TARGET_HEIGHT, ring_points},
};

const RIBBONS: usize = 3;
const RIBBON_POINTS: usize = 240;
/// Below this flash level the beat ring isn't drawn.
const BLOOM_MIN_FLASH: f32 = 0.05;
const FLASH_DECAY_SECS: f32 = 0.5;
const HUE_SPEED: f32 = 0.02;
const WARP: Warp = Warp {
    zoom_per_sec: 1.2,
    rotation_per_sec: 0.25,
    fade_secs: 0.6,
};

pub struct Lightshow {
    // One renderer per line: each holds its own points for this frame.
    ribbons: [LineRenderer; RIBBONS],
    bloom: LineRenderer,
    feedback: Feedback,
    ribbon_points: [Vec<[f32; 2]>; RIBBONS],
    bloom_points: Vec<[f32; 2]>,
    auto_scale: AutoScale,
    flash: BeatFlash,
    aspect: f32,
    time: f32,
    dt: f32,
}

impl Lightshow {
    pub fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
        Self {
            ribbons: std::array::from_fn(|_| LineRenderer::new(device, RIBBON_POINTS)),
            bloom: LineRenderer::new(device, WAVEFORM_LEN + 1),
            feedback: Feedback::new(device, size),
            ribbon_points: Default::default(),
            bloom_points: Vec::new(),
            auto_scale: AutoScale::default(),
            flash: BeatFlash::new(FLASH_DECAY_SECS),
            aspect: size.0.max(1) as f32 / size.1.max(1) as f32,
            time: 0.0,
            dt: 0.0,
        }
    }

    fn hue(&self) -> f32 {
        self.time * HUE_SPEED
    }
}

impl Visualizer for Lightshow {
    fn name(&self) -> &str {
        "Classic Lightshow"
    }

    fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        self.feedback.resize(device, size);
        self.aspect = size.0.max(1) as f32 / size.1.max(1) as f32;
    }

    fn update(&mut self, audio: &AudioFrame, dt: f32) {
        self.dt = dt;
        self.time += dt;
        let flash = self.flash.update(audio, dt);
        let scale = self.auto_scale.update(audio.waveform, 1.0, dt);
        let ripple = scale * (0.15 + 0.35 * audio.mid);

        for (k, points) in self.ribbon_points.iter_mut().enumerate() {
            ribbon(k, self.time, audio.waveform, ripple, self.aspect, points);
        }

        self.bloom_points.clear();
        if flash > BLOOM_MIN_FLASH {
            // The ring starts small and expands as the flash fades.
            ring_points(
                audio.waveform,
                scale * TARGET_HEIGHT,
                &mut self.bloom_points,
            );
            let radius = 0.2 + 0.6 * (1.0 - flash);
            let expand = radius / RING_RADIUS;
            for p in &mut self.bloom_points {
                *p = [p[0] * expand, p[1] * expand];
            }
        }
    }

    fn render(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        self.feedback.warp(queue, encoder, &WARP, self.dt);
        let flash = self.flash.value();
        let hue = self.hue();
        for (k, renderer) in self.ribbons.iter_mut().enumerate() {
            let style = LineStyle {
                color: hsv(hue + k as f32 / RIBBONS as f32, 0.6, 0.7 + 0.6 * flash),
                core_width: 0.002,
                glow_width: 0.012,
            };
            renderer.set(queue, &self.ribbon_points[k], &style, size);
            renderer.draw(encoder, self.feedback.current());
        }
        if !self.bloom_points.is_empty() {
            let style = LineStyle {
                color: hsv(hue + 0.5, 0.35, 1.5 * flash),
                core_width: 0.003,
                glow_width: 0.02,
            };
            self.bloom.set(queue, &self.bloom_points, &style, size);
            self.bloom.draw(encoder, self.feedback.current());
        }
        self.feedback.finish(queue, encoder, target);
    }
}

/// Points of ribbon `k`: a slowly morphing Lissajous-style curve whose height
/// ripples with the waveform.
fn ribbon(k: usize, time: f32, wave: &[f32], ripple: f32, aspect: f32, out: &mut Vec<[f32; 2]>) {
    out.clear();
    if wave.is_empty() {
        return;
    }
    let kf = k as f32;
    let phase = time * (0.13 + kf * 0.05);
    let last = (RIBBON_POINTS - 1) as f32;
    out.extend((0..RIBBON_POINTS).map(|i| {
        let s = i as f32 / last;
        let x = (TAU * s * (1.0 + kf * 0.5) + phase * 2.0 + kf).sin() * 0.85 * aspect;
        let base_y = (TAU * s * (2.0 + kf) * 0.5 + phase * 1.3 + kf * 2.1).sin() * 0.55;
        let sample = wave[(s * (wave.len() - 1) as f32) as usize];
        [x, (base_y + sample * ripple).clamp(-1.0, 1.0)]
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ribbons_have_a_fixed_point_count_and_stay_on_screen() {
        let wave = vec![1.0; WAVEFORM_LEN];
        let mut pts = Vec::new();
        ribbon(2, 12.3, &wave, 5.0, 16.0 / 9.0, &mut pts);
        assert_eq!(pts.len(), RIBBON_POINTS);
        assert!(
            pts.iter()
                .all(|p| p[1].abs() <= 1.0 && p[0].abs() <= 16.0 / 9.0)
        );
    }

    #[test]
    fn silence_leaves_the_ribbon_unrippled() {
        let mut flat = Vec::new();
        let mut rippled = Vec::new();
        ribbon(0, 1.0, &[0.0; 64], 1.0, 1.0, &mut flat);
        ribbon(0, 1.0, &[0.3; 64], 1.0, 1.0, &mut rippled);
        assert!(flat.iter().zip(&rippled).all(|(a, b)| a[0] == b[0]));
        assert!(flat.iter().zip(&rippled).any(|(a, b)| a[1] != b[1]));
    }

    #[test]
    fn empty_waveform_draws_nothing() {
        let mut pts = vec![[0.0, 0.0]];
        ribbon(0, 0.0, &[], 1.0, 1.0, &mut pts);
        assert!(pts.is_empty());
    }
}
