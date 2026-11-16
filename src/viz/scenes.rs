//! Visualizers that draw a whole scene in one shader, each pass from scratch:
//! Event Horizon (a black hole with a lensed accretion disk) and Oil on Ice
//! (iridescent oil slicks drifting over dark ice).

use crate::{
    audio::{analysis::AudioFrame, bands::smoothing_coef},
    render::layer::{Blend, LayerParams, ShaderLayer},
};

use super::{
    Visualizer,
    common::{BeatFlash, layer_params},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    EventHorizon,
    OilOnIce,
}

impl Scene {
    fn name(self) -> &'static str {
        match self {
            Self::EventHorizon => "Event Horizon",
            Self::OilOnIce => "Oil on Ice",
        }
    }

    fn shader(self) -> &'static str {
        match self {
            Self::EventHorizon => include_str!("../../shaders/event_horizon.wgsl"),
            Self::OilOnIce => include_str!("../../shaders/oil_on_ice.wgsl"),
        }
    }

    /// How long a beat's pulse takes to fade.
    fn flash_decay_secs(self) -> f32 {
        match self {
            Self::EventHorizon => 0.6,
            Self::OilOnIce => 0.4,
        }
    }

    /// How fast each of the scene's two clocks runs for this audio: the main
    /// motion (the disk's spin, the oil's drift) and a slow one (the camera,
    /// the light).
    fn clock_rates(self, audio: &AudioFrame) -> (f32, f32) {
        match self {
            Self::EventHorizon => (0.35 + 0.9 * audio.mid + 0.5 * audio.bass, 0.02),
            Self::OilOnIce => (0.05 + 0.25 * audio.mid + 0.15 * audio.bass, 0.03),
        }
    }
}

/// How quickly the smoothed loudness follows the music.
const SWELL_SECS: f32 = 0.4;

pub struct SceneViz {
    scene: Scene,
    layer: ShaderLayer,
    params: LayerParams,
    bands: Vec<f32>,
    time: f32,
    /// The two clocks from [`Scene::clock_rates`].
    motion: f32,
    slow: f32,
    /// Loudness smoothed over [`SWELL_SECS`], for slow swells.
    swell: f32,
    flash: BeatFlash,
}

impl SceneViz {
    pub fn new(device: &wgpu::Device, scene: Scene) -> Self {
        Self {
            scene,
            layer: ShaderLayer::new(device, scene.name(), scene.shader(), Blend::Replace),
            params: LayerParams::default(),
            bands: Vec::new(),
            time: 0.0,
            motion: 0.0,
            slow: 0.0,
            swell: 0.0,
            flash: BeatFlash::new(scene.flash_decay_secs()),
        }
    }
}

impl Visualizer for SceneViz {
    fn name(&self) -> &str {
        self.scene.name()
    }

    fn update(&mut self, audio: &AudioFrame, dt: f32) {
        self.time += dt;
        let (motion_rate, slow_rate) = self.scene.clock_rates(audio);
        self.motion += dt * motion_rate;
        self.slow += dt * slow_rate;
        let loudness = (audio.bass + audio.mid + audio.treble) / 3.0;
        self.swell += (loudness - self.swell) * smoothing_coef(SWELL_SECS, dt);
        let flash = self.flash.update(audio, dt);
        self.bands.clear();
        self.bands.extend_from_slice(audio.bands.values());
        self.params = LayerParams {
            extra: [self.motion, self.slow, self.swell, 0.0],
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
        self.layer.set(queue, &self.params, &self.bands, &[]);
        self.layer.draw(encoder, target);
    }
}
