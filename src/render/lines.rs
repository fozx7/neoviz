//! Draws glowing polylines (waveforms, scopes) into an HDR target.

use super::common::{HDR_FORMAT, Uniforms, begin_pass, entry};

const POINT_BYTES: u64 = 2 * size_of::<f32>() as u64;

/// Line appearance. Widths are fractions of the target height so the look is
/// the same at any resolution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineStyle {
    pub color: [f32; 3],
    pub core_width: f32,
    pub glow_width: f32,
}

pub struct LineRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    params: Uniforms<8>,
    points: wgpu::Buffer,
    max_points: usize,
    point_count: usize,
    bytes: Vec<u8>,
}

impl LineRenderer {
    pub fn new(device: &wgpu::Device, max_points: usize) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lines"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../shaders/lines.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lines"),
            entries: &[entry::uniform(0)],
        });
        let params = Uniforms::new(device, "lines params");
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lines"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: params.buffer.as_entire_binding(),
            }],
        });
        let points = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("line points"),
            size: max_points as u64 * POINT_BYTES,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Segment i reads point i from slot 0 and point i+1 from slot 1, which
        // is the same buffer bound one point later.
        let p0 = [wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 0,
            shader_location: 0,
        }];
        let p1 = [wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 0,
            shader_location: 1,
        }];
        let buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: POINT_BYTES,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &p0,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: POINT_BYTES,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &p1,
            }),
        ];

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lines"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let max_blend = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Max,
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lines"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: max_blend,
                        alpha: max_blend,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
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
            points,
            max_points,
            point_count: 0,
            bytes: Vec::new(),
        }
    }

    /// Uploads the polyline (in aspect units: y -1..1, x -aspect..aspect) and style.
    pub fn set(
        &mut self,
        queue: &wgpu::Queue,
        points: &[[f32; 2]],
        style: &LineStyle,
        size: (u32, u32),
    ) {
        let points = &points[..points.len().min(self.max_points)];
        self.point_count = points.len();
        self.bytes.clear();
        self.bytes
            .extend(points.iter().flatten().flat_map(|v| v.to_ne_bytes()));
        queue.write_buffer(&self.points, 0, &self.bytes);

        let h = size.1 as f32;
        let [r, g, b] = style.color;
        self.params.write(
            queue,
            [
                size.0 as f32,
                h,
                style.core_width * h,
                style.glow_width * h,
                r,
                g,
                b,
                1.0,
            ],
        );
    }

    /// Draws the uploaded polyline over the existing contents of `target`.
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        if self.point_count < 2 {
            return;
        }
        let bytes = self.point_count as u64 * POINT_BYTES;
        let mut pass = begin_pass(encoder, "lines", target, false);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.points.slice(..bytes - POINT_BYTES));
        pass.set_vertex_buffer(1, self.points.slice(POINT_BYTES..bytes));
        pass.draw(0..6, 0..(self.point_count - 1) as u32);
    }
}
