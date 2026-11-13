//! Turns the visualizer output into the final frame:
//! crossfade → bloom → scanlines → screen.
//!
//! Visualizers render into two offscreen slots: [`INCOMING`] holds the current
//! visualizer, [`OUTGOING`] the one fading out during a crossfade.

use super::common::{
    HDR_FORMAT, Target, Uniforms, draw_fullscreen, entry, fullscreen_pipeline, linear_sampler,
};

pub const OUTGOING: usize = 0;
pub const INCOMING: usize = 1;

/// Bloom works at this fraction of the screen resolution.
const BLOOM_DOWNSCALE: u32 = 4;
const BLOOM_THRESHOLD: f32 = 0.6;
const BLOOM_KNEE: f32 = 0.3;
const BLOOM_STRENGTH: f32 = 0.8;

/// Post-processing switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostFx {
    pub bloom: bool,
    pub scanlines: bool,
}

impl Default for PostFx {
    fn default() -> Self {
        Self {
            bloom: true,
            scanlines: false,
        }
    }
}

/// Size-dependent resources, rebuilt on resize.
struct Frame {
    slots: [Target; 2],
    scene: Target,
    /// Ping-pong pair at bloom resolution; the result ends up in `bloom[0]`.
    bloom: [Target; 2],
    bloom_size: (u32, u32),
    mix_group: wgpu::BindGroup,
    bright_group: wgpu::BindGroup,
    blur_h_group: wgpu::BindGroup,
    blur_v_group: wgpu::BindGroup,
    final_group: wgpu::BindGroup,
}

/// Size-independent GPU objects.
struct Pipelines {
    one_texture: wgpu::BindGroupLayout,
    two_textures: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    mix: wgpu::RenderPipeline,
    bright: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    final_pass: wgpu::RenderPipeline,
    // One buffer per pass: all passes are recorded before the queue writes land.
    mix_params: Uniforms<4>,
    bright_params: Uniforms<4>,
    blur_h_params: Uniforms<4>,
    blur_v_params: Uniforms<4>,
    final_params: Uniforms<4>,
}

pub struct Compositor {
    p: Pipelines,
    frame: Frame,
}

impl Compositor {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        size: (u32, u32),
    ) -> Self {
        let p = Pipelines::new(device, surface_format);
        let frame = p.make_frame(device, size);
        Self { p, frame }
    }

    pub fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        self.frame = self.p.make_frame(device, size);
    }

    pub fn slot(&self, index: usize) -> &wgpu::TextureView {
        &self.frame.slots[index].view
    }

    /// Blends `mix_amount` (0 = outgoing only, 1 = incoming only) of the two
    /// slots, applies `post`, scales it by `brightness` (0 = black, 1 = as
    /// drawn), and writes the result to `target`.
    pub fn draw(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        mix_amount: f32,
        post: PostFx,
        brightness: f32,
    ) {
        let (p, f) = (&self.p, &self.frame);
        p.mix_params.write(queue, [mix_amount, 0.0, 0.0, 0.0]);
        draw_fullscreen(encoder, "mix", &f.scene.view, true, &p.mix, &[&f.mix_group]);

        if post.bloom {
            let (w, h) = f.bloom_size;
            p.bright_params
                .write(queue, [BLOOM_THRESHOLD, BLOOM_KNEE, 0.0, 0.0]);
            p.blur_h_params
                .write(queue, [1.0 / w as f32, 0.0, 0.0, 0.0]);
            p.blur_v_params
                .write(queue, [0.0, 1.0 / h as f32, 0.0, 0.0]);
            draw_fullscreen(
                encoder,
                "bloom bright",
                &f.bloom[0].view,
                true,
                &p.bright,
                &[&f.bright_group],
            );
            draw_fullscreen(
                encoder,
                "bloom blur h",
                &f.bloom[1].view,
                true,
                &p.blur,
                &[&f.blur_h_group],
            );
            draw_fullscreen(
                encoder,
                "bloom blur v",
                &f.bloom[0].view,
                true,
                &p.blur,
                &[&f.blur_v_group],
            );
        }

        let strength = if post.bloom { BLOOM_STRENGTH } else { 0.0 };
        let scanlines = f32::from(u8::from(post.scanlines));
        p.final_params
            .write(queue, [strength, scanlines, brightness, 0.0]);
        draw_fullscreen(
            encoder,
            "final",
            target,
            true,
            &p.final_pass,
            &[&f.final_group],
        );
    }
}

impl Pipelines {
    fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let one_texture = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post one texture"),
            entries: &[
                entry::uniform(0),
                entry::texture(1, true),
                entry::sampler(2),
            ],
        });
        let two_textures = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post two textures"),
            entries: &[
                entry::uniform(0),
                entry::texture(1, true),
                entry::texture(2, true),
                entry::sampler(3),
            ],
        });
        let pipe = |label, source, layout, format| {
            fullscreen_pipeline(device, label, source, &[layout], format, None)
        };
        Self {
            mix: pipe(
                "mix",
                include_str!("../../shaders/post/mix.wgsl"),
                &two_textures,
                HDR_FORMAT,
            ),
            bright: pipe(
                "bloom bright",
                include_str!("../../shaders/post/bright.wgsl"),
                &one_texture,
                HDR_FORMAT,
            ),
            blur: pipe(
                "bloom blur",
                include_str!("../../shaders/post/blur.wgsl"),
                &one_texture,
                HDR_FORMAT,
            ),
            final_pass: pipe(
                "final",
                include_str!("../../shaders/post/final.wgsl"),
                &two_textures,
                surface_format,
            ),
            sampler: linear_sampler(device),
            mix_params: Uniforms::new(device, "mix params"),
            bright_params: Uniforms::new(device, "bright params"),
            blur_h_params: Uniforms::new(device, "blur h params"),
            blur_v_params: Uniforms::new(device, "blur v params"),
            final_params: Uniforms::new(device, "final params"),
            one_texture,
            two_textures,
        }
    }

    fn make_frame(&self, device: &wgpu::Device, size: (u32, u32)) -> Frame {
        let bloom_size = (
            (size.0 / BLOOM_DOWNSCALE).max(1),
            (size.1 / BLOOM_DOWNSCALE).max(1),
        );
        let slots = [
            Target::new(device, "outgoing slot", size),
            Target::new(device, "incoming slot", size),
        ];
        let scene = Target::new(device, "scene", size);
        let bloom = [
            Target::new(device, "bloom a", bloom_size),
            Target::new(device, "bloom b", bloom_size),
        ];
        let one = |params: &Uniforms<4>, tex: &Target| {
            self.group(device, &self.one_texture, params, &[&tex.view])
        };
        let two = |params: &Uniforms<4>, a: &Target, b: &Target| {
            self.group(device, &self.two_textures, params, &[&a.view, &b.view])
        };
        Frame {
            mix_group: two(&self.mix_params, &slots[OUTGOING], &slots[INCOMING]),
            bright_group: one(&self.bright_params, &scene),
            blur_h_group: one(&self.blur_h_params, &bloom[0]),
            blur_v_group: one(&self.blur_v_params, &bloom[1]),
            final_group: two(&self.final_params, &scene, &bloom[0]),
            slots,
            scene,
            bloom,
            bloom_size,
        }
    }

    /// Bind group of `params` at 0, then each texture, then the sampler.
    fn group(
        &self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        params: &Uniforms<4>,
        textures: &[&wgpu::TextureView],
    ) -> wgpu::BindGroup {
        let entries: Vec<wgpu::BindGroupEntry> = std::iter::once(wgpu::BindGroupEntry {
            binding: 0,
            resource: params.buffer.as_entire_binding(),
        })
        .chain(
            textures
                .iter()
                .enumerate()
                .map(|(i, view)| wgpu::BindGroupEntry {
                    binding: i as u32 + 1,
                    resource: wgpu::BindingResource::TextureView(view),
                }),
        )
        .chain(std::iter::once(wgpu::BindGroupEntry {
            binding: textures.len() as u32 + 1,
            resource: wgpu::BindingResource::Sampler(&self.sampler),
        }))
        .collect();
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post"),
            layout,
            entries: &entries,
        })
    }
}
