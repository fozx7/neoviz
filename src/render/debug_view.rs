//! Debug view of the raw analysis: waveform on top, spectrum below.

use crate::audio::analysis::{Analyzer, FFT_SIZE, SPECTRUM_BINS, WAVEFORM_LEN};

/// Bottom of the spectrum display in dBFS.
const DISPLAY_DB_MIN: f32 = -90.0;
const WAVEFORM_ROW: u32 = 0;
const SPECTRUM_ROW: u32 = 1;
const DATA_WIDTH: u32 = SPECTRUM_BINS as u32;
const PARAMS_FLOATS: usize = 8;

pub struct DebugView {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    params: wgpu::Buffer,
    data: wgpu::Texture,
    /// Reused staging space for converting f32 slices to bytes.
    bytes: Vec<u8>,
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
                height: 2,
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
        }
    }

    /// Uploads this frame's analysis and screen size.
    pub fn update(
        &mut self,
        queue: &wgpu::Queue,
        analyzer: &Analyzer,
        sample_rate: u32,
        size: (u32, u32),
    ) {
        let params: [f32; PARAMS_FLOATS] = [
            size.0 as f32,
            size.1 as f32,
            sample_rate as f32,
            FFT_SIZE as f32,
            WAVEFORM_LEN as f32,
            SPECTRUM_BINS as f32,
            DISPLAY_DB_MIN,
            0.0,
        ];
        queue.write_buffer(&self.params, 0, to_bytes(&mut self.bytes, &params));
        self.write_row(queue, WAVEFORM_ROW, analyzer.waveform());
        self.write_row(queue, SPECTRUM_ROW, analyzer.spectrum_db());
    }

    fn write_row(&mut self, queue: &wgpu::Queue, row: u32, values: &[f32]) {
        let width = values.len() as u32;
        debug_assert!(width <= DATA_WIDTH);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.data,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: row, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            to_bytes(&mut self.bytes, values),
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

fn to_bytes<'a>(buf: &'a mut Vec<u8>, values: &[f32]) -> &'a [u8] {
    buf.clear();
    buf.extend(values.iter().flat_map(|v| v.to_ne_bytes()));
    buf
}
