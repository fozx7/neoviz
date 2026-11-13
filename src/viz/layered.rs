//! Visualizers that are a single shader layer over a feedback buffer:
//! Ambient Swirl and Battery Rings.

use crate::{
    audio::analysis::AudioFrame,
    render::{
        feedback::{Feedback, Warp},
        layer::{Blend, LayerParams, ShaderLayer},
    },
};

use super::{
    Visualizer,
    common::{BeatFlash, layer_params},
};

const FLASH_DECAY_SECS: f32 = 0.25;
/// Hue drift in colour-wheel turns per second.
const HUE_SPEED: f32 = 0.015;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    AmbientSwirl,
    BatteryRings,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::AmbientSwirl => "Ambient Swirl",
            Self::BatteryRings => "Battery Rings",
        }
    }

    fn shader(self) -> &'static str {
        match self {
            Self::AmbientSwirl => include_str!("../../shaders/ambient.wgsl"),
            Self::BatteryRings => include_str!("../../shaders/rings.wgsl"),
        }
    }

    fn blend(self) -> Blend {
        match self {
            Self::AmbientSwirl => Blend::Alpha,
            Self::BatteryRings => Blend::Max,
        }
    }

    /// Feedback motion, which reacts to the music.
    fn warp(self, params: &LayerParams) -> Warp {
        match self {
            Self::AmbientSwirl => Warp {
                zoom_per_sec: 1.1 + 0.4 * params.bass,
                rotation_per_sec: 0.12 + 0.3 * params.bass,
                fade_secs: 1.5,
            },
            Self::BatteryRings => Warp {
                zoom_per_sec: 1.25 + 0.5 * params.bass,
                rotation_per_sec: 0.3 + 0.5 * params.mid,
                fade_secs: 0.25,
            },
        }
    }
}

pub struct Layered {
    kind: Kind,
    layer: ShaderLayer,
    feedback: Feedback,
    params: LayerParams,
    bands: Vec<f32>,
    time: f32,
    /// Ambient's flow clock: runs faster when the bass is up.
    flow: f32,
    flash: BeatFlash,
}

impl Layered {
    pub fn new(device: &wgpu::Device, size: (u32, u32), kind: Kind) -> Self {
        Self {
            kind,
            layer: ShaderLayer::new(device, kind.name(), kind.shader(), kind.blend()),
            feedback: Feedback::new(device, size),
            params: LayerParams::default(),
            bands: Vec::new(),
            time: 0.0,
            flow: 0.0,
            flash: BeatFlash::new(FLASH_DECAY_SECS),
        }
    }
}

impl Visualizer for Layered {
    fn name(&self) -> &str {
        self.kind.name()
    }

    fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        self.feedback.resize(device, size);
    }

    fn update(&mut self, audio: &AudioFrame, dt: f32) {
        self.time += dt;
        self.flow += dt * (0.15 + 0.8 * audio.bass);
        let flash = self.flash.update(audio, dt);
        self.bands.clear();
        self.bands.extend_from_slice(audio.bands.values());
        self.params = LayerParams {
            extra: [self.flow, self.time * HUE_SPEED, 0.0, 0.0],
            ..layer_params(audio, self.params.size, self.time, dt, flash)
        };
    }

    fn render(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        self.params.size = size;
        let warp = self.kind.warp(&self.params);
        self.feedback.warp(queue, encoder, &warp, self.params.dt);
        self.layer.set(queue, &self.params, &self.bands, &[]);
        self.layer.draw(encoder, self.feedback.current());
        self.feedback.finish(queue, encoder, target);
    }
}
