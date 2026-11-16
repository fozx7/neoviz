//! Visualizers that draw a whole scene in one shader, each pass from scratch:
//! Event Horizon (a black hole with a lensed accretion disk), Oil on Ice
//! (iridescent oil slicks drifting over dark ice), Burning World (a desert
//! planet swept by firestorms), Deep Nebula (light behind a band of dust) and
//! Windswept Field (gusts rolling through long grass).

use crate::{
    audio::{analysis::AudioFrame, bands::smoothing_coef},
    render::{
        image_texture::ImageTexture,
        layer::{Blend, LayerParams, ShaderLayer},
    },
};

use super::{
    Visualizer,
    common::{BeatFlash, layer_params},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    EventHorizon,
    OilOnIce,
    BurningWorld,
    DeepNebula,
    WindsweptField,
}

/// The full COSMOS field from ESO's VISTA telescope (UltraVISTA), behind
/// Event Horizon: thousands of faint, distant galaxies.
const DEEP_FIELD: &[u8] = include_bytes!("../../assets/sky/cosmos-ultravista.jpg");
/// Its credit line, which must be shown with it (CC BY 4.0).
const DEEP_FIELD_CREDIT: &str =
    "Background: ESO/UltraVISTA team. Acknowledgement: TERAPIX/CNRS/INSU/CASU";

impl Scene {
    pub const ALL: [Self; 5] = [
        Self::EventHorizon,
        Self::OilOnIce,
        Self::BurningWorld,
        Self::DeepNebula,
        Self::WindsweptField,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::EventHorizon => "Event Horizon",
            Self::OilOnIce => "Oil on Ice",
            Self::BurningWorld => "Burning World",
            Self::DeepNebula => "Deep Nebula",
            Self::WindsweptField => "Windswept Field",
        }
    }

    fn shader(self) -> &'static str {
        match self {
            Self::EventHorizon => include_str!("../../shaders/event_horizon.wgsl"),
            Self::OilOnIce => include_str!("../../shaders/oil_on_ice.wgsl"),
            Self::BurningWorld => include_str!("../../shaders/burning_world.wgsl"),
            Self::DeepNebula => include_str!("../../shaders/deep_nebula.wgsl"),
            Self::WindsweptField => include_str!("../../shaders/windswept_field.wgsl"),
        }
    }

    /// A picture the shader samples, if it uses one.
    fn image(self) -> Option<&'static [u8]> {
        match self {
            Self::EventHorizon => Some(DEEP_FIELD),
            _ => None,
        }
    }

    /// The credit line that must be shown with the scene's picture.
    fn credit(self) -> Option<&'static str> {
        self.image().map(|_| DEEP_FIELD_CREDIT)
    }

    /// How long a beat's pulse takes to fade.
    fn flash_decay_secs(self) -> f32 {
        match self {
            Self::EventHorizon => 0.6,
            Self::OilOnIce | Self::BurningWorld => 0.4,
            Self::DeepNebula | Self::WindsweptField => 0.9,
        }
    }

    /// How fast each of the scene's two clocks runs for this audio: the main
    /// motion (the disk's spin, the oil's drift, the fire's advance, the
    /// gas's flow) and a slow one (the camera, the light, the planet's spin).
    fn clock_rates(self, audio: &AudioFrame) -> (f32, f32) {
        match self {
            Self::EventHorizon => (0.35 + 0.9 * audio.mid + 0.5 * audio.bass, 0.02),
            Self::OilOnIce => (0.03 + 0.15 * audio.mid + 0.09 * audio.bass, 0.02),
            Self::BurningWorld => (0.012 + 0.03 * audio.bass, 0.01),
            Self::DeepNebula => (0.03 + 0.08 * audio.mid, 0.025),
            Self::WindsweptField => (0.2 + 0.35 * audio.mid + 0.25 * audio.bass, 0.004),
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
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, scene: Scene) -> anyhow::Result<Self> {
        let (name, shader) = (scene.name(), scene.shader());
        let layer = match scene.image() {
            Some(jpeg) => {
                let image = ImageTexture::from_jpeg(device, queue, name, jpeg)?;
                ShaderLayer::with_image(device, name, shader, Blend::Replace, image)
            }
            None => ShaderLayer::new(device, name, shader, Blend::Replace),
        };
        Ok(Self {
            scene,
            layer,
            params: LayerParams::default(),
            bands: Vec::new(),
            time: 0.0,
            motion: 0.0,
            slow: 0.0,
            swell: 0.0,
            flash: BeatFlash::new(scene.flash_decay_secs()),
        })
    }
}

impl Visualizer for SceneViz {
    fn name(&self) -> &str {
        self.scene.name()
    }

    fn credit(&self) -> Option<&str> {
        self.scene.credit()
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
