//! Developer aid: saves one rendered frame to a PNG and quits, for checking
//! a visualizer's look without a screenshot tool (which would grab whatever
//! window is in front).
//!
//! `NEOVIZ_CAPTURE=frame.png neoviz --visualizer "..."` saves the frame after
//! `NEOVIZ_CAPTURE_AFTER` seconds (default 6). Unset, this does nothing.

use std::path::PathBuf;

use anyhow::Context;

const DEFAULT_AFTER_SECS: f32 = 6.0;
/// wgpu requires each copied row to start on this many bytes.
const ROW_ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

pub struct FrameCapture {
    path: PathBuf,
    after: f32,
    elapsed: f32,
}

impl FrameCapture {
    pub fn from_env() -> Option<Self> {
        let path = PathBuf::from(std::env::var_os("NEOVIZ_CAPTURE")?);
        let after = std::env::var("NEOVIZ_CAPTURE_AFTER")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_AFTER_SECS);
        Some(Self {
            path,
            after,
            elapsed: 0.0,
        })
    }

    /// Advances by `dt`; true once it's time to save.
    pub fn due(&mut self, dt: f32) -> bool {
        self.elapsed += dt;
        self.elapsed >= self.after
    }

    /// Copies `view`'s texture (8-bit RGBA or BGRA) to the PNG.
    pub fn save(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
    ) -> anyhow::Result<()> {
        let texture = view.texture();
        let (width, height) = (texture.width(), texture.height());
        let bgra = matches!(
            texture.format(),
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let row = (width * 4).div_ceil(ROW_ALIGN) * ROW_ALIGN;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame capture"),
            size: u64::from(row * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
            },
            texture.size(),
        );
        queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .context("waiting for the frame")?;

        let mapped = slice
            .get_mapped_range()
            .map_err(|e| anyhow::anyhow!("reading the frame: {e:?}"))?;
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for line in mapped.chunks(row as usize) {
            for &[a, b, c, _] in line[..(width * 4) as usize].as_chunks::<4>().0 {
                pixels.extend(if bgra { [c, b, a, 255] } else { [a, b, c, 255] });
            }
        }
        drop(mapped);

        let file = std::fs::File::create(&self.path)
            .with_context(|| format!("creating {}", self.path.display()))?;
        let mut png = png::Encoder::new(std::io::BufWriter::new(file), width, height);
        png.set_color(png::ColorType::Rgba);
        png.set_depth(png::BitDepth::Eight);
        png.write_header()?.write_image_data(&pixels)?;
        log::info!("saved a frame to {}", self.path.display());
        Ok(())
    }
}
