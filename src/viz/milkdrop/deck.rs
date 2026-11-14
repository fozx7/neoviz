//! A "deck": one projectM instance (on its own worker thread) and the wgpu
//! texture its frames are uploaded into. MilkDrop keeps two, so one can load
//! the next preset while the other is on screen.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::render::common::{HDR_FORMAT, begin_pass, entry, fullscreen_pipeline, linear_sampler};

use super::worker::{Command, Event, Worker};

pub struct Deck {
    worker: Worker,
    frame: FrameTexture,
    /// The preset this deck has loaded, or is loading.
    preset: Option<PathBuf>,
    /// Whether `preset` has finished loading.
    ready: bool,
    /// Whether the texture holds a frame of `preset` yet.
    has_frame: bool,
}

impl Deck {
    pub fn start(blit: &Blit, size: (u32, u32)) -> Result<Self> {
        Ok(Self {
            worker: Worker::start(size)?,
            frame: FrameTexture::new(blit, size),
            preset: None,
            ready: false,
            has_frame: false,
        })
    }

    pub fn preset(&self) -> Option<&Path> {
        self.preset.as_deref()
    }

    /// Whether a preset is loaded and ready to show.
    pub fn is_ready(&self) -> bool {
        self.ready && self.preset.is_some()
    }

    pub fn has_frame(&self) -> bool {
        self.has_frame
    }

    /// Starts loading `path`, unless it's already loaded or loading.
    pub fn load(&mut self, path: &Path) {
        if self.preset() == Some(path) {
            return;
        }
        self.worker.send(Command::Load(path.to_path_buf()));
        self.preset = Some(path.to_path_buf());
        self.ready = false;
        self.has_frame = false;
    }

    /// Forgets the preset, e.g. after a rescan.
    pub fn clear(&mut self) {
        self.preset = None;
        self.ready = false;
    }

    /// The next event from the worker, after noting a finished load.
    pub fn next_event(&mut self) -> Option<Event> {
        let event = self.worker.try_event()?;
        if let Event::Loaded { path } = &event
            && self.preset.as_ref() == Some(path)
        {
            self.ready = true;
            // A frame waiting now was rendered before the load; drop it.
            // The worker sends `Loaded` after its last frame of the old preset.
            if let Some(stale) = self.worker.take_frame() {
                self.worker.send(Command::Recycle(stale.pixels));
            }
        }
        Some(event)
    }

    pub fn send_audio(&self, pcm: Vec<f32>) {
        self.worker.send(Command::Audio(pcm));
    }

    /// Asks for a frame at `size` and uploads the newest one that's arrived.
    pub fn render(&mut self, blit: &Blit, queue: &wgpu::Queue, size: (u32, u32)) {
        self.worker.send(Command::Render(size));
        // Frames arrive a little behind; until a new one does, the last stays up.
        if let Some(frame) = self.worker.take_frame() {
            // Until the load is confirmed, frames may be of the old preset.
            if self.ready {
                self.frame.write(blit, queue, &frame.pixels, frame.size);
                self.has_frame = true;
            }
            self.worker.send(Command::Recycle(frame.pixels));
        }
    }
}

/// The texture a deck's frames are uploaded into.
struct FrameTexture {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    size: (u32, u32),
}

impl FrameTexture {
    fn new(blit: &Blit, size: (u32, u32)) -> Self {
        let (texture, bind_group) = blit.make_texture(size);
        Self {
            texture,
            bind_group,
            size,
        }
    }

    /// Uploads one RGBA8 frame, resizing the texture if the frame size changed.
    fn write(&mut self, blit: &Blit, queue: &wgpu::Queue, pixels: &[u8], size: (u32, u32)) {
        if pixels.len() != size.0 as usize * size.1 as usize * 4 {
            return;
        }
        if size != self.size {
            (self.texture, self.bind_group) = blit.make_texture(size);
            self.size = size;
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.0 * 4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
        );
    }
}

/// Draws deck frames into our target, optionally crossfading two of them.
pub struct Blit {
    device: wgpu::Device,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    opaque: wgpu::RenderPipeline,
    /// Mixes over what's there by the blend constant: src·c + dst·(1−c).
    mix: wgpu::RenderPipeline,
}

impl Blit {
    pub fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("milkdrop"),
            entries: &[entry::texture(0, true), entry::sampler(1)],
        });
        let source = include_str!("../../../shaders/milkdrop_blit.wgsl");
        let opaque = fullscreen_pipeline(device, "milkdrop", source, &[&layout], HDR_FORMAT, None);
        let by_constant = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Constant,
            dst_factor: wgpu::BlendFactor::OneMinusConstant,
            operation: wgpu::BlendOperation::Add,
        };
        let mix = fullscreen_pipeline(
            device,
            "milkdrop mix",
            source,
            &[&layout],
            HDR_FORMAT,
            Some(wgpu::BlendState {
                color: by_constant,
                alpha: by_constant,
            }),
        );
        Self {
            device: device.clone(),
            sampler: linear_sampler(device),
            layout,
            opaque,
            mix,
        }
    }

    /// Draws `base`, then `over` mixed in by `amount` (0..1) if given.
    pub fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        base: &Deck,
        over: Option<(&Deck, f32)>,
    ) {
        let mut pass = begin_pass(encoder, "milkdrop", target, true);
        pass.set_pipeline(&self.opaque);
        pass.set_bind_group(0, &base.frame.bind_group, &[]);
        pass.draw(0..3, 0..1);
        if let Some((deck, amount)) = over {
            let c = f64::from(amount.clamp(0.0, 1.0));
            pass.set_pipeline(&self.mix);
            pass.set_blend_constant(wgpu::Color {
                r: c,
                g: c,
                b: c,
                a: c,
            });
            pass.set_bind_group(0, &deck.frame.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    fn make_texture(&self, size: (u32, u32)) -> (wgpu::Texture, wgpu::BindGroup) {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("milkdrop frame"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // projectM writes display-ready (sRGB-encoded) colour.
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("milkdrop"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        (texture, bind_group)
    }
}
