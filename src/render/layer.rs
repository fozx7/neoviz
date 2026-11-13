//! A full-screen shader fed the standard per-frame audio uniforms and band
//! data (see `shaders/layer_header.wgsl`). Most visualizers are one of these,
//! optionally drawn over a feedback buffer.

use super::common::{
    DataTexture, HDR_FORMAT, Uniforms, draw_fullscreen, entry, fullscreen_pipeline,
};

const HEADER: &str = include_str!("../../shaders/layer_header.wgsl");
/// Widest band row a layer can receive.
const MAX_BANDS: u32 = 64;
const PARAM_FLOATS: usize = 16;

/// Per-frame inputs for a layer shader.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LayerParams {
    pub size: (u32, u32),
    pub time: f32,
    pub dt: f32,
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub rms: f32,
    pub flash: f32,
    pub variant: f32,
    pub beat: bool,
    pub extra: [f32; 4],
}

impl LayerParams {
    fn to_array(self, band_count: usize) -> [f32; PARAM_FLOATS] {
        let [e0, e1, e2, e3] = self.extra;
        [
            self.size.0 as f32,
            self.size.1 as f32,
            self.time,
            self.dt,
            self.bass,
            self.mid,
            self.treble,
            self.rms,
            self.flash,
            band_count as f32,
            self.variant,
            f32::from(u8::from(self.beat)),
            e0,
            e1,
            e2,
            e3,
        ]
    }
}

/// How a layer combines with what's already in the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blend {
    /// Overwrite (the layer draws the whole frame).
    Replace,
    /// Mix by the shader's output alpha (soft washes).
    Alpha,
    /// Keep the brighter of old and new, per channel. Over a feedback buffer
    /// this draws at full strength without piling up into a white-out.
    Max,
}

impl Blend {
    fn state(self) -> Option<wgpu::BlendState> {
        match self {
            Self::Replace => None,
            Self::Alpha => Some(wgpu::BlendState::ALPHA_BLENDING),
            Self::Max => {
                let max = wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Max,
                };
                Some(wgpu::BlendState {
                    color: max,
                    alpha: max,
                })
            }
        }
    }
}

pub struct ShaderLayer {
    label: &'static str,
    blend: Blend,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    params: Uniforms<PARAM_FLOATS>,
    data: DataTexture,
}

impl ShaderLayer {
    /// `source` must define `fn fs(in: FullscreenOut) -> @location(0) vec4f`
    /// and may use everything in the layer header.
    pub fn new(device: &wgpu::Device, label: &'static str, source: &str, blend: Blend) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries: &[entry::uniform(0), entry::texture(1, false)],
        });
        let pipeline = fullscreen_pipeline(
            device,
            label,
            &format!("{HEADER}\n{source}"),
            &[&layout],
            HDR_FORMAT,
            blend.state(),
        );
        let params = Uniforms::new(device, label);
        let data = DataTexture::new(device, label, MAX_BANDS, 2);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
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
            label,
            blend,
            pipeline,
            bind_group,
            params,
            data,
        }
    }

    /// Uploads this frame's parameters plus band levels and peaks.
    pub fn set(&mut self, queue: &wgpu::Queue, params: &LayerParams, bands: &[f32], peaks: &[f32]) {
        self.params.write(queue, params.to_array(bands.len()));
        self.data.write_row(queue, 0, bands.iter().copied());
        self.data.write_row(queue, 1, peaks.iter().copied());
    }

    /// Draws into `target`. [`Blend::Replace`] layers clear it first.
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let clear = self.blend == Blend::Replace;
        draw_fullscreen(
            encoder,
            self.label,
            target,
            clear,
            &self.pipeline,
            &[&self.bind_group],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_pack_in_header_order() {
        let p = LayerParams {
            size: (1920, 1080),
            time: 2.0,
            dt: 0.5,
            bass: 0.1,
            mid: 0.2,
            treble: 0.3,
            rms: 0.4,
            flash: 0.6,
            variant: 2.0,
            beat: true,
            extra: [7.0, 8.0, 9.0, 10.0],
        };
        assert_eq!(
            p.to_array(64),
            [
                1920.0, 1080.0, 2.0, 0.5, 0.1, 0.2, 0.3, 0.4, 0.6, 64.0, 2.0, 1.0, 7.0, 8.0, 9.0,
                10.0
            ]
        );
    }
}
