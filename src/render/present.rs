//! The canvas every frame is rendered into, and copying slices of it to the
//! windows that show it.

use super::common::{
    Target, Uniforms, draw_fullscreen, entry, fullscreen_pipeline, linear_sampler,
};

/// One window's slice of the canvas, as `[x, y, width, height]` in 0..1.
pub type Slice = [f32; 4];
pub const WHOLE: Slice = [0.0, 0.0, 1.0, 1.0];

pub struct Canvas {
    target: Target,
    size: (u32, u32),
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// One uniform buffer and bind group per window (all written each frame,
    /// so they can't share a buffer).
    outputs: Vec<(Uniforms<4>, wgpu::BindGroup)>,
}

impl Canvas {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, size: (u32, u32)) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("present"),
            entries: &[
                entry::uniform(0),
                entry::texture(1, true),
                entry::sampler(2),
            ],
        });
        let pipeline = fullscreen_pipeline(
            device,
            "present",
            include_str!("../../shaders/present.wgsl"),
            &[&layout],
            format,
            None,
        );
        Self {
            target: Target::with_format(device, "canvas", size, format),
            size,
            format,
            pipeline,
            layout,
            sampler: linear_sampler(device),
            outputs: Vec::new(),
        }
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Where the frame is drawn before being copied to windows.
    pub fn view(&self) -> &wgpu::TextureView {
        &self.target.view
    }

    pub fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if size == self.size {
            return;
        }
        self.target = Target::with_format(device, "canvas", size, self.format);
        self.size = size;
        // Bind groups point at the old texture.
        self.outputs.clear();
    }

    /// Copies `slice` of the canvas to `target`, the frame of window `index`.
    pub fn present(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        index: usize,
        slice: Slice,
        target: &wgpu::TextureView,
    ) {
        while self.outputs.len() <= index {
            let params = Uniforms::new(device, "present params");
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("present"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: params.buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&self.target.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.outputs.push((params, group));
        }
        let (params, group) = &self.outputs[index];
        params.write(queue, slice);
        draw_fullscreen(encoder, "present", target, true, &self.pipeline, &[group]);
    }
}
