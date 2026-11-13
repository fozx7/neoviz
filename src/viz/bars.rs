//! Bars & Peaks: a classic spectrum analyser with falling peak caps.

use crate::{
    audio::analysis::AudioFrame,
    render::layer::{Blend, LayerParams, ShaderLayer},
};

use super::{
    Visualizer,
    common::{BeatFlash, layer_params},
};

/// Bands drawn by the LED variant; chunky segments read better with fewer bars.
const LED_BANDS: usize = 32;
const FLASH_DECAY_SECS: f32 = 0.15;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarsVariant {
    Solid,
    Led,
    Mirror,
}

impl BarsVariant {
    fn name(self) -> &'static str {
        match self {
            Self::Solid => "Bars & Peaks",
            Self::Led => "LED Bars",
            Self::Mirror => "Mirror Bars",
        }
    }

    /// Value of the shader's `p.variant`.
    fn shader_id(self) -> f32 {
        match self {
            Self::Solid => 0.0,
            Self::Led => 1.0,
            Self::Mirror => 2.0,
        }
    }
}

pub struct Bars {
    variant: BarsVariant,
    layer: ShaderLayer,
    params: LayerParams,
    values: Vec<f32>,
    peaks: Vec<f32>,
    scratch: Vec<f32>,
    time: f32,
    flash: BeatFlash,
}

impl Bars {
    pub fn new(device: &wgpu::Device, variant: BarsVariant) -> Self {
        Self {
            variant,
            layer: ShaderLayer::new(
                device,
                "bars",
                include_str!("../../shaders/bars.wgsl"),
                Blend::Replace,
            ),
            params: LayerParams::default(),
            values: Vec::new(),
            peaks: Vec::new(),
            scratch: Vec::new(),
            time: 0.0,
            flash: BeatFlash::new(FLASH_DECAY_SECS),
        }
    }
}

impl Visualizer for Bars {
    fn name(&self) -> &str {
        self.variant.name()
    }

    fn update(&mut self, audio: &AudioFrame, dt: f32) {
        self.time += dt;
        let flash = self.flash.update(audio, dt);
        self.params = LayerParams {
            variant: self.variant.shader_id(),
            ..layer_params(audio, self.params.size, self.time, dt, flash)
        };

        let count = match self.variant {
            BarsVariant::Led => LED_BANDS,
            BarsVariant::Solid | BarsVariant::Mirror => audio.bands.len(),
        };
        group_max(audio.bands.values(), count, &mut self.values);
        self.scratch.clear();
        self.scratch.extend(audio.bands.peaks());
        group_max(&self.scratch, count, &mut self.peaks);
    }

    fn render(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        self.params.size = size;
        self.layer
            .set(queue, &self.params, &self.values, &self.peaks);
        self.layer.draw(encoder, target);
    }
}

/// Reduces `values` to `count` groups, each the max of its members, into `out`.
fn group_max(values: &[f32], count: usize, out: &mut Vec<f32>) {
    out.clear();
    if values.is_empty() || count == 0 {
        return;
    }
    let count = count.min(values.len());
    out.extend((0..count).map(|g| {
        let start = g * values.len() / count;
        let end = (g + 1) * values.len() / count;
        values[start..end].iter().copied().fold(0.0, f32::max)
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grouped(values: &[f32], count: usize) -> Vec<f32> {
        let mut out = Vec::new();
        group_max(values, count, &mut out);
        out
    }

    #[test]
    fn same_count_passes_through() {
        assert_eq!(grouped(&[0.1, 0.5, 0.3], 3), [0.1, 0.5, 0.3]);
    }

    #[test]
    fn halving_takes_the_max_of_each_pair() {
        assert_eq!(grouped(&[0.1, 0.5, 0.3, 0.2], 2), [0.5, 0.3]);
    }

    #[test]
    fn never_produces_more_groups_than_values() {
        assert_eq!(grouped(&[0.4, 0.6], 8), [0.4, 0.6]);
    }

    #[test]
    fn empty_input_gives_empty_output() {
        assert!(grouped(&[], 4).is_empty());
    }
}
