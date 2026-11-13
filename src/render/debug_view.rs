//! Debug view of the analysis: waveform, raw spectrum, then bands and levels.

use crate::audio::{
    analysis::{AudioFrame, FFT_SIZE, SPECTRUM_BINS, WAVEFORM_LEN},
    bands::smoothing_coef,
};

/// Bottom of the spectrum display in dBFS.
const DISPLAY_DB_MIN: f32 = -90.0;
/// How quickly the beat flash fades.
const FLASH_DECAY_SECS: f32 = 0.12;
// Rows of the data texture; must match the shader.
const WAVEFORM_ROW: u32 = 0;
const SPECTRUM_ROW: u32 = 1;
const BANDS_ROW: u32 = 2;
const PEAKS_ROW: u32 = 3;
const LEVELS_ROW: u32 = 4;
const DATA_ROWS: u32 = 5;
const DATA_WIDTH: u32 = SPECTRUM_BINS as u32;
/// Uniform size in floats (a multiple of 4 for 16-byte alignment).
const PARAMS_FLOATS: usize = 12;

pub struct DebugView {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    params: wgpu::Buffer,
    data: wgpu::Texture,
    /// Reused staging space for converting floats to bytes.
    bytes: Vec<u8>,
    /// Beat flash brightness, 0..1, fading after each beat.
    flash: f32,
}

impl DebugView {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("debug view"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../shaders/debug_view.wgsl").into()),
        });

        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("debug view params"),
            size: (PARAMS_FLOATS * size_of::<f32>()) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let data = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("debug view data"),
            size: wgpu::Extent3d {
                width: DATA_WIDTH,
                height: DATA_ROWS,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("debug view"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let data_view = data.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("debug view"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&data_view),
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("debug view"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("debug view"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(target_format.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            bind_group,
            params,
            data,
            bytes: Vec::new(),
            flash: 0.0,
        }
    }

    /// Uploads this frame's analysis and screen size.
    pub fn update(
        &mut self,
        queue: &wgpu::Queue,
        audio: &AudioFrame,
        sample_rate: u32,
        size: (u32, u32),
        dt: f32,
    ) {
        self.flash *= 1.0 - smoothing_coef(FLASH_DECAY_SECS, dt);
        if audio.beat {
            self.flash = self.flash.max(audio.beat_intensity);
        }

        let params: [f32; PARAMS_FLOATS] = [
            size.0 as f32,
            size.1 as f32,
            sample_rate as f32,
            FFT_SIZE as f32,
            WAVEFORM_LEN as f32,
            SPECTRUM_BINS as f32,
            DISPLAY_DB_MIN,
            audio.bands.len() as f32,
            self.flash,
            f32::from(u8::from(audio.silent)),
            0.0,
            0.0,
        ];
        to_bytes(&mut self.bytes, params);
        queue.write_buffer(&self.params, 0, &self.bytes);

        self.write_row(queue, WAVEFORM_ROW, audio.waveform.iter().copied());
        self.write_row(queue, SPECTRUM_ROW, audio.spectrum_db.iter().copied());
        self.write_row(queue, BANDS_ROW, audio.bands.values().iter().copied());
        self.write_row(queue, PEAKS_ROW, audio.bands.peaks());
        let levels = [audio.rms, audio.bass, audio.mid, audio.treble, audio.peak];
        self.write_row(queue, LEVELS_ROW, levels);
    }

    fn write_row(&mut self, queue: &wgpu::Queue, row: u32, values: impl IntoIterator<Item = f32>) {
        to_bytes(&mut self.bytes, values);
        let width = (self.bytes.len() / size_of::<f32>()) as u32;
        debug_assert!(width <= DATA_WIDTH);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.data,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: row, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &self.bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * size_of::<f32>() as u32),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("debug view"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn to_bytes(buf: &mut Vec<u8>, values: impl IntoIterator<Item = f32>) {
    buf.clear();
    buf.extend(values.into_iter().flat_map(f32::to_ne_bytes));
}
