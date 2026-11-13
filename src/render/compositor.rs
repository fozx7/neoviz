//! Owns the two offscreen slots visualizers render into, and blends them onto
//! the screen. Slot [`INCOMING`] holds the current visualizer; slot
//! [`OUTGOING`] the one fading out during a crossfade.

use super::common::{
    Target, Uniforms, draw_fullscreen, entry, fullscreen_pipeline, linear_sampler,
};

pub const OUTGOING: usize = 0;
pub const INCOMING: usize = 1;

pub struct Compositor {
    slots: [Target; 2],
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    params: Uniforms<4>,
}

impl Compositor {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        size: (u32, u32),
    ) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite"),
            entries: &[
                entry::uniform(0),
                entry::texture(1, true),
                entry::texture(2, true),
                entry::sampler(3),
            ],
        });
        let pipeline = fullscreen_pipeline(
            device,
            "composite",
            include_str!("../../shaders/composite.wgsl"),
            &[&layout],
            surface_format,
            None,
        );
        let sampler = linear_sampler(device);
        let params = Uniforms::new(device, "composite params");
        let slots = make_slots(device, size);
        let bind_group = make_bind_group(device, &layout, &params, &slots, &sampler);
        Self {
            slots,
            pipeline,
            layout,
            bind_group,
            sampler,
            params,
        }
    }

    pub fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        self.slots = make_slots(device, size);
        self.bind_group = make_bind_group(
            device,
            &self.layout,
            &self.params,
            &self.slots,
            &self.sampler,
        );
    }

    pub fn slot(&self, index: usize) -> &wgpu::TextureView {
        &self.slots[index].view
    }

    /// Draws `mix_amount` (0 = outgoing only, 1 = incoming only) of the two slots to `target`.
    pub fn draw(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        mix_amount: f32,
    ) {
        self.params.write(queue, [mix_amount, 0.0, 0.0, 0.0]);
        draw_fullscreen(
            encoder,
            "composite",
            target,
            true,
            &self.pipeline,
            &[&self.bind_group],
        );
    }
}

fn make_slots(device: &wgpu::Device, size: (u32, u32)) -> [Target; 2] {
    [
        Target::new(device, "outgoing slot", size),
        Target::new(device, "incoming slot", size),
    ]
}

fn make_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    params: &Uniforms<4>,
    slots: &[Target; 2],
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("composite"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params.buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&slots[OUTGOING].view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&slots[INCOMING].view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}
