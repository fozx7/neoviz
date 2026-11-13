//! MilkDrop presets via libprojectM.
//!
//! projectM only speaks OpenGL, so it renders into a hidden OpenGL context
//! ([`HeadlessGl`]); each frame is read back and uploaded as a wgpu texture.
//! That makes MilkDrop an ordinary visualizer: crossfades, bloom and the
//! overlay all work on top of it.

mod presets;

use std::path::Path;

use anyhow::Result;
use projectm::core::ProjectM;

use crate::{
    audio::analysis::AudioFrame,
    config::Settings,
    render::{
        common::{HDR_FORMAT, draw_fullscreen, entry, fullscreen_pipeline, linear_sampler},
        headless_gl::HeadlessGl,
    },
};

use super::Visualizer;
use presets::{Playlist, display_name};

pub const NAME: &str = "MilkDrop";
/// projectM renders at most this tall; larger windows are scaled up, which
/// keeps the per-frame copy cheap at 4K.
const MAX_RENDER_HEIGHT: u32 = 1080;
const RENDER_FPS: u32 = 60;
const PRESETS_HINT: &str = "No preset folder set. Get presets (e.g. github.com/projectM-visualizer/presets-cream-of-the-crop) and choose the folder below.";

pub struct MilkDrop {
    // Field order matters: projectM must be dropped while its GL context exists.
    pm: ProjectM,
    gl: HeadlessGl,
    playlist: Playlist,
    preset_dir: String,
    preset_secs: f32,
    since_preset: f32,
    gain: f32,
    status: Option<String>,
    pcm: Vec<f32>,
    pixels: Vec<u8>,
    upload: Upload,
}

/// The wgpu side: the texture projectM's frame is uploaded into, and the
/// pass that draws it into our target.
struct Upload {
    device: wgpu::Device,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    size: (u32, u32),
}

impl MilkDrop {
    pub fn new(device: &wgpu::Device, size: (u32, u32)) -> Result<Self> {
        let render_size = render_size(size);
        let gl = HeadlessGl::new(render_size)?;
        // projectM needs its GL context current when it's created.
        let pm = ProjectM::create();
        pm.set_window_size(render_size.0 as usize, render_size.1 as usize);
        pm.set_fps(RENDER_FPS);
        // We choose presets ourselves; stop projectM switching on its own.
        pm.set_preset_locked(true);
        pm.set_hard_cut_enabled(false);
        Ok(Self {
            pm,
            gl,
            playlist: Playlist::new(Vec::new(), 1),
            preset_dir: String::new(),
            preset_secs: Settings::default().preset_secs,
            since_preset: 0.0,
            gain: 1.0,
            status: Some(PRESETS_HINT.to_owned()),
            pcm: Vec::new(),
            pixels: Vec::new(),
            upload: Upload::new(device, render_size),
        })
    }

    fn rescan(&mut self) {
        self.playlist = Playlist::new(Vec::new(), seed());
        if self.preset_dir.trim().is_empty() {
            self.status = Some(PRESETS_HINT.to_owned());
            return;
        }
        let dir = Path::new(self.preset_dir.trim());
        match presets::scan(dir) {
            Ok(found) if found.is_empty() => {
                self.status = Some(format!("No .milk presets found in {}", dir.display()));
            }
            Ok(found) => {
                log::info!("{} MilkDrop presets in {}", found.len(), dir.display());
                self.status = Some(format!("{} presets in {}", found.len(), dir.display()));
                self.playlist = Playlist::new(found, seed());
                self.load_random(false);
            }
            Err(e) => self.status = Some(format!("Can't read preset folder: {e:#}")),
        }
    }

    fn load_random(&mut self, smooth: bool) {
        if let Some(path) = self.playlist.random().map(Path::to_path_buf) {
            self.load(&path, smooth);
        }
    }

    fn load(&mut self, path: &Path, smooth: bool) {
        if let Err(e) = self.gl.make_current() {
            log::warn!("MilkDrop: {e:#}");
            return;
        }
        log::info!("MilkDrop preset: {}", display_name(path));
        self.pm.load_preset_file(&path.to_string_lossy(), smooth);
        self.since_preset = 0.0;
    }

    /// Hands projectM this frame's new audio, interleaved stereo.
    fn feed_audio(&mut self, audio: &AudioFrame) {
        let n = audio.new_frames;
        let start = audio.left.len() - n;
        self.pcm.clear();
        for (l, r) in audio.left[start..].iter().zip(&audio.right[start..]) {
            self.pcm.push((l * self.gain).clamp(-1.0, 1.0));
            self.pcm.push((r * self.gain).clamp(-1.0, 1.0));
        }
        // projectM takes a limited number of floats per call; keep frames whole.
        let max = (ProjectM::pcm_get_max_samples() as usize) & !1;
        for chunk in self.pcm.chunks(max.max(2)) {
            self.pm.pcm_add_float(chunk, 2);
        }
    }
}

impl Visualizer for MilkDrop {
    fn name(&self) -> &str {
        NAME
    }

    fn configure(&mut self, settings: &Settings, rescan: bool) {
        self.gain = 10f32.powf(settings.sensitivity_db / 20.0);
        self.preset_secs = settings.preset_secs;
        self.preset_dir = settings.preset_dir.clone();
        // Only on request: the folder field changes on every keystroke.
        if rescan {
            self.rescan();
        }
    }

    fn update(&mut self, audio: &AudioFrame, dt: f32) {
        self.feed_audio(audio);
        self.since_preset += dt;
        if self.since_preset >= self.preset_secs && self.playlist.len() > 1 {
            self.load_random(true);
        }
    }

    fn step_preset(&mut self, direction: isize) -> Option<String> {
        let path = self.playlist.step(direction)?.to_path_buf();
        self.load(&path, false);
        Some(display_name(&path))
    }

    fn status(&self) -> Option<String> {
        self.status.clone()
    }

    fn render(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        let wanted = render_size(size);
        let ready = self.gl.make_current().and_then(|()| self.gl.resize(wanted));
        if let Err(e) = ready {
            log::warn!("MilkDrop: {e:#}");
            return;
        }
        let actual = self.gl.size();
        self.pm
            .set_window_size(actual.0 as usize, actual.1 as usize);
        self.pm.render_frame();
        // Shows the previous frame (one frame behind); keeps the last image
        // until the first frame after a resize arrives.
        if self.gl.read_previous_frame(&mut self.pixels) {
            self.upload.write(queue, &self.pixels, actual);
        }
        draw_fullscreen(
            encoder,
            "milkdrop",
            target,
            true,
            &self.upload.pipeline,
            &[&self.upload.bind_group],
        );
    }
}

impl Upload {
    fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("milkdrop"),
            entries: &[entry::texture(0, true), entry::sampler(1)],
        });
        let pipeline = fullscreen_pipeline(
            device,
            "milkdrop",
            include_str!("../../../shaders/milkdrop_blit.wgsl"),
            &[&layout],
            HDR_FORMAT,
            None,
        );
        let sampler = linear_sampler(device);
        let (texture, bind_group) = make_texture(device, &layout, &sampler, size);
        Self {
            device: device.clone(),
            pipeline,
            layout,
            sampler,
            texture,
            bind_group,
            size,
        }
    }

    /// Uploads one RGBA8 frame, resizing the texture if the frame size changed.
    fn write(&mut self, queue: &wgpu::Queue, pixels: &[u8], size: (u32, u32)) {
        if size != self.size {
            (self.texture, self.bind_group) =
                make_texture(&self.device, &self.layout, &self.sampler, size);
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

fn make_texture(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    size: (u32, u32),
) -> (wgpu::Texture, wgpu::BindGroup) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
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
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("milkdrop"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    (texture, bind_group)
}

/// The size projectM renders at for an output of `size`: the same, but no
/// taller than [`MAX_RENDER_HEIGHT`].
fn render_size(size: (u32, u32)) -> (u32, u32) {
    let (w, h) = (size.0.max(1), size.1.max(1));
    if h <= MAX_RENDER_HEIGHT {
        return (w, h);
    }
    let scale = MAX_RENDER_HEIGHT as f32 / h as f32;
    (
        ((w as f32 * scale).round() as u32).max(1),
        MAX_RENDER_HEIGHT,
    )
}

fn seed() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.subsec_nanos())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_size_passes_small_outputs_through() {
        assert_eq!(render_size((1280, 720)), (1280, 720));
        assert_eq!(render_size((1920, 1080)), (1920, 1080));
    }

    #[test]
    fn render_size_caps_4k_at_1080p_keeping_aspect() {
        assert_eq!(render_size((3840, 2160)), (1920, 1080));
        assert_eq!(render_size((5120, 1440)), (3840, 1080));
    }

    #[test]
    fn render_size_never_hits_zero() {
        assert_eq!(render_size((0, 0)), (1, 1));
    }
}
