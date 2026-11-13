//! Draws many glowing streaks (or dots) in one call: particles, stars.

use super::common::{HDR_FORMAT, Uniforms, begin_pass, entry};

/// One capsule from `from` to `to` (aspect units). Equal ends draw a dot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Streak {
    pub from: [f32; 2],
    pub to: [f32; 2],
    /// Linear RGB; may exceed 1.0 (HDR).
    pub color: [f32; 3],
    /// Core radius as a fraction of the target height.
    pub width: f32,
}

const FLOATS_PER_STREAK: usize = 8;
const STREAK_BYTES: u64 = (FLOATS_PER_STREAK * size_of::<f32>()) as u64;

pub struct StreakRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    params: Uniforms<4>,
    instances: wgpu::Buffer,
    capacity: usize,
    count: usize,
    bytes: Vec<u8>,
}

impl StreakRenderer {
    pub fn new(device: &wgpu::Device, capacity: usize) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("streaks"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../shaders/streaks.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("streaks"),
            entries: &[entry::uniform(0)],
        });
        let params = Uniforms::new(device, "streaks params");
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("streaks"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: params.buffer.as_entire_binding(),
            }],
        });
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("streak instances"),
            size: capacity as u64 * STREAK_BYTES,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let attributes = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4];
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: STREAK_BYTES,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &attributes,
        })];
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("streaks"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let additive = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("streaks"),
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
                        color: additive,
                        alpha: wgpu::BlendComponent::OVER,
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
            instances,
            capacity,
            count: 0,
            bytes: Vec::new(),
        }
    }

    /// Uploads this frame's streaks (beyond capacity are dropped).
    pub fn set(&mut self, queue: &wgpu::Queue, streaks: &[Streak], size: (u32, u32)) {
        let streaks = &streaks[..streaks.len().min(self.capacity)];
        self.count = streaks.len();
        self.bytes.clear();
        for s in streaks {
            let floats = [
                s.from[0], s.from[1], s.to[0], s.to[1], s.color[0], s.color[1], s.color[2], s.width,
            ];
            self.bytes
                .extend(floats.iter().flat_map(|v| v.to_ne_bytes()));
        }
        if !self.bytes.is_empty() {
            queue.write_buffer(&self.instances, 0, &self.bytes);
        }
        self.params
            .write(queue, [size.0 as f32, size.1 as f32, 0.0, 0.0]);
    }

    /// Draws the uploaded streaks into `target`, clearing it first if `clear`.
    pub fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clear: bool,
    ) {
        let mut pass = begin_pass(encoder, "streaks", target, clear);
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..self.count as u64 * STREAK_BYTES));
        pass.draw(0..6, 0..self.count as u32);
    }
}
