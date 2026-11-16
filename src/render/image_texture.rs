//! A picture (JPEG) as a mipmapped texture, for visualizers that draw one,
//! e.g. a real space image behind Event Horizon.

use anyhow::Context;
use zune_jpeg::{
    JpegDecoder,
    zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions},
};

use super::common::entry;

const CHANNELS: usize = 4;

pub struct ImageTexture {
    pub layout: wgpu::BindGroupLayout,
    pub bind_group: wgpu::BindGroup,
}

impl ImageTexture {
    /// Decodes `jpeg` and uploads it with a full mip chain, so it stays
    /// smooth however small it's drawn. The bind group has the texture at
    /// binding 0 and a linear, mirror-repeating sampler at binding 1.
    pub fn from_jpeg(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &str,
        jpeg: &[u8],
    ) -> anyhow::Result<Self> {
        let (width, height, pixels) = decode_rgba(jpeg).with_context(|| format!("decoding {label}"))?;
        let levels = mip_chain(width, height, pixels);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, (w, h, data)) in levels.iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w * CHANNELS as u32),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: *w,
                    height: *h,
                    depth_or_array_layers: 1,
                },
            );
        }
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some(label),
            address_mode_u: wgpu::AddressMode::MirrorRepeat,
            address_mode_v: wgpu::AddressMode::MirrorRepeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries: &[entry::texture(0, true), entry::sampler(1)],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        Ok(Self { layout, bind_group })
    }
}

/// Width, height and RGBA pixels of a JPEG.
fn decode_rgba(jpeg: &[u8]) -> anyhow::Result<(u32, u32, Vec<u8>)> {
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(jpeg), options);
    let pixels = decoder.decode().map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let info = decoder.info().context("no image header")?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    anyhow::ensure!(
        pixels.len() == width as usize * height as usize * CHANNELS,
        "decoded {} bytes for a {width}x{height} image",
        pixels.len()
    );
    Ok((width, height, pixels))
}

/// The image and each smaller level down to 1x1, as (width, height, pixels).
fn mip_chain(width: u32, height: u32, pixels: Vec<u8>) -> Vec<(u32, u32, Vec<u8>)> {
    let mut levels = vec![(width, height, pixels)];
    while let Some((w, h, data)) = levels.last().filter(|(w, h, _)| *w > 1 || *h > 1) {
        let next = half_size(*w, *h, data);
        levels.push(next);
    }
    levels
}

/// Halves an RGBA image by averaging each 2x2 block (odd edges repeat).
fn half_size(width: u32, height: u32, pixels: &[u8]) -> (u32, u32, Vec<u8>) {
    let (w, h) = (width as usize, height as usize);
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let at = |x: usize, y: usize, c: usize| u32::from(pixels[(y.min(h - 1) * w + x.min(w - 1)) * CHANNELS + c]);
    let mut out = Vec::with_capacity(nw * nh * CHANNELS);
    for y in 0..nh {
        for x in 0..nw {
            for c in 0..CHANNELS {
                let sum = at(2 * x, 2 * y, c)
                    + at(2 * x + 1, 2 * y, c)
                    + at(2 * x, 2 * y + 1, c)
                    + at(2 * x + 1, 2 * y + 1, c);
                out.push(((sum + 2) / 4) as u8);
            }
        }
    }
    (nw as u32, nh as u32, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halving_averages_each_block() {
        // 2x2: one white pixel, three black.
        let px = [255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255];
        let (w, h, out) = half_size(2, 2, &px);
        assert_eq!((w, h), (1, 1));
        assert_eq!(out, [64, 64, 64, 255]);
    }

    #[test]
    fn odd_sizes_round_down_without_reading_past_the_edge() {
        let px = vec![100; 3 * CHANNELS];
        let (w, h, out) = half_size(3, 1, &px);
        assert_eq!((w, h), (1, 1));
        assert_eq!(out, [100; 4]);
    }

    #[test]
    fn mip_chain_goes_down_to_one_pixel() {
        let levels = mip_chain(8, 2, vec![0; 8 * 2 * CHANNELS]);
        let sizes: Vec<_> = levels.iter().map(|(w, h, _)| (*w, *h)).collect();
        assert_eq!(sizes, [(8, 2), (4, 1), (2, 1), (1, 1)]);
    }

    #[test]
    fn the_bundled_sky_decodes() {
        let jpeg = include_bytes!("../../assets/sky/webb-first-deep-field.jpg");
        let (w, h, px) = decode_rgba(jpeg).unwrap();
        assert_eq!((w, h), (3011, 3072));
        assert_eq!(px.len(), (w * h) as usize * CHANNELS);
    }
}
