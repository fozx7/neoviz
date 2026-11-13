//! Bars & Peaks: a classic spectrum analyser with falling peak caps.

use crate::{
    audio::{analysis::AudioFrame, bands::smoothing_coef},
    render::common::{
        DataTexture, HDR_FORMAT, Uniforms, draw_fullscreen, entry, fullscreen_pipeline,
    },
};

use super::Visualizer;

/// Bands drawn by the LED variant; chunky segments read better with fewer bars.
const LED_BANDS: usize = 32;
/// Widest band set any variant draws.
const MAX_BANDS: u32 = 64;
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

    /// Value of the shader's `variant` uniform.
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
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    params: Uniforms<8>,
    data: DataTexture,
    values: Vec<f32>,
    peaks: Vec<f32>,
    time: f32,
    flash: f32,
}

impl Bars {
    pub fn new(device: &wgpu::Device, variant: BarsVariant) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bars"),
            entries: &[entry::uniform(0), entry::texture(1, false)],
        });
        let pipeline = fullscreen_pipeline(
            device,
            "bars",
            include_str!("../../shaders/bars.wgsl"),
            &[&layout],
            HDR_FORMAT,
            None,
        );
        let params = Uniforms::new(device, "bars params");
        let data = DataTexture::new(device, "bars data", MAX_BANDS, 2);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bars"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&data.view),
                },
            ],
        });
        Self {
            variant,
            pipeline,
            bind_group,
            params,
            data,
            values: Vec::new(),
            peaks: Vec::new(),
            time: 0.0,
            flash: 0.0,
        }
    }
}

impl Visualizer for Bars {
    fn name(&self) -> &str {
        self.variant.name()
    }

    fn update(&mut self, audio: &AudioFrame, dt: f32) {
        self.time += dt;
        self.flash *= 1.0 - smoothing_coef(FLASH_DECAY_SECS, dt);
        if audio.beat {
            self.flash = self.flash.max(audio.beat_intensity);
        }

        let count = match self.variant {
            BarsVariant::Led => LED_BANDS,
            BarsVariant::Solid | BarsVariant::Mirror => audio.bands.len(),
        };
        let peaks: Vec<f32> = audio.bands.peaks().collect();
        group_max(audio.bands.values(), count, &mut self.values);
        group_max(&peaks, count, &mut self.peaks);
    }

    fn render(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        self.params.write(
            queue,
            [
                size.0 as f32,
                size.1 as f32,
                self.values.len() as f32,
                self.variant.shader_id(),
                self.time,
                self.flash,
                0.0,
                0.0,
            ],
        );
        self.data.write_row(queue, 0, self.values.iter().copied());
        self.data.write_row(queue, 1, self.peaks.iter().copied());
        draw_fullscreen(
            encoder,
            "bars",
            target,
            true,
            &self.pipeline,
            &[&self.bind_group],
        );
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
