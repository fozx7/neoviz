//! Feedback buffer: each frame starts from the previous one, zoomed, rotated
//! and faded, which leaves the trails behind moving shapes that give the
//! old-school look.
//!
//! Per frame: [`Feedback::warp`], draw new content onto [`Feedback::current`],
//! then [`Feedback::finish`] to copy it out and swap buffers.

use super::common::{
    HDR_FORMAT, Target, Uniforms, draw_fullscreen, entry, fullscreen_pipeline, linear_sampler,
};

/// How the previous frame is carried into the next.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Warp {
    /// Scale applied per second (> 1 drifts outward).
    pub zoom_per_sec: f32,
    /// Rotation in radians per second.
    pub rotation_per_sec: f32,
    /// Seconds for the trail to fade to ~37% brightness.
    pub fade_secs: f32,
}

impl Warp {
    /// The per-frame uniform values for a frame lasting `dt` seconds.
    fn per_frame(&self, dt: f32) -> [f32; 4] {
        let fade = if self.fade_secs > 0.0 {
            (-dt / self.fade_secs).exp()
        } else {
            0.0
        };
        [
            self.zoom_per_sec.powf(dt),
            self.rotation_per_sec * dt,
            fade,
            0.0,
        ]
    }
}

const IDENTITY: [f32; 4] = [1.0, 0.0, 1.0, 0.0];

pub struct Feedback {
    /// Ping-pong buffers; `current` indexes the one being drawn this frame.
    buffers: [Target; 2],
    current: usize,
    /// Draws the source buffer transformed; with identity params it's a plain copy.
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    warp_params: Uniforms<4>,
    blit_params: Uniforms<4>,
    /// `*_groups[i]` read buffer `i` with the warp or blit params.
    warp_groups: [wgpu::BindGroup; 2],
    blit_groups: [wgpu::BindGroup; 2],
}

impl Feedback {
    pub fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("feedback"),
            entries: &[
                entry::uniform(0),
                entry::texture(1, true),
                entry::sampler(2),
            ],
        });
        let pipeline = fullscreen_pipeline(
            device,
            "feedback",
            include_str!("../../shaders/feedback.wgsl"),
            &[&layout],
            HDR_FORMAT,
            None,
        );
        let sampler = linear_sampler(device);
        let warp_params = Uniforms::new(device, "feedback warp params");
        let blit_params = Uniforms::new(device, "feedback blit params");
        let buffers = make_buffers(device, size);
        let warp_groups = make_groups(device, &layout, &warp_params, &buffers, &sampler);
        let blit_groups = make_groups(device, &layout, &blit_params, &buffers, &sampler);
        Self {
            buffers,
            current: 0,
            pipeline,
            layout,
            sampler,
            warp_params,
            blit_params,
            warp_groups,
            blit_groups,
        }
    }

    /// Recreates the buffers at the new size (this clears the trails).
    pub fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        self.buffers = make_buffers(device, size);
        self.warp_groups = make_groups(
            device,
            &self.layout,
            &self.warp_params,
            &self.buffers,
            &self.sampler,
        );
        self.blit_groups = make_groups(
            device,
            &self.layout,
            &self.blit_params,
            &self.buffers,
            &self.sampler,
        );
    }

    /// Fills the current buffer with the previous frame, transformed by `warp`.
    pub fn warp(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        warp: &Warp,
        dt: f32,
    ) {
        self.warp_params.write(queue, warp.per_frame(dt));
        let previous = 1 - self.current;
        draw_fullscreen(
            encoder,
            "feedback warp",
            &self.buffers[self.current].view,
            true,
            &self.pipeline,
            &[&self.warp_groups[previous]],
        );
    }

    /// The buffer to draw this frame's new content onto (after [`Feedback::warp`]).
    pub fn current(&self) -> &wgpu::TextureView {
        &self.buffers[self.current].view
    }

    /// Copies this frame to `target` and makes it the "previous" frame for next time.
    pub fn finish(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
    ) {
        self.blit_params.write(queue, IDENTITY);
        draw_fullscreen(
            encoder,
            "feedback blit",
            target,
            true,
            &self.pipeline,
            &[&self.blit_groups[self.current]],
        );
        self.current = 1 - self.current;
    }
}

fn make_buffers(device: &wgpu::Device, size: (u32, u32)) -> [Target; 2] {
    [
        Target::new(device, "feedback a", size),
        Target::new(device, "feedback b", size),
    ]
}

fn make_groups(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    params: &Uniforms<4>,
    buffers: &[Target; 2],
    sampler: &wgpu::Sampler,
) -> [wgpu::BindGroup; 2] {
    std::array::from_fn(|i| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("feedback"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&buffers[i].view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warp_is_frame_rate_independent() {
        let w = Warp {
            zoom_per_sec: 1.5,
            rotation_per_sec: 0.6,
            fade_secs: 0.4,
        };
        let one = w.per_frame(1.0 / 30.0);
        let half = w.per_frame(1.0 / 60.0);
        // Two 60 fps frames compound to one 30 fps frame.
        assert!((half[0] * half[0] - one[0]).abs() < 1e-5);
        assert!((half[1] * 2.0 - one[1]).abs() < 1e-6);
        assert!((half[2] * half[2] - one[2]).abs() < 1e-5);
    }

    #[test]
    fn zero_fade_time_clears_each_frame() {
        let w = Warp {
            zoom_per_sec: 1.0,
            rotation_per_sec: 0.0,
            fade_secs: 0.0,
        };
        assert_eq!(w.per_frame(1.0 / 60.0)[2], 0.0);
    }
}
