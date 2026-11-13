//! Small shared building blocks: full-screen pipelines, offscreen targets,
//! data textures and uniform uploads.

/// Format of every offscreen target. Float so additive effects can exceed 1.0
/// before the compositor clamps them.
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Vertex shader drawing one oversized triangle that covers the screen.
/// Prepended to every full-screen fragment shader; entry point `vs`.
const FULLSCREEN_VS: &str = "
struct FullscreenOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f, // 0..1, origin top-left
}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> FullscreenOut {
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var out: FullscreenOut;
    out.pos = vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2f(uv.x, 1.0 - uv.y);
    return out;
}
";

/// Builds a pipeline running the fragment shader `fs` in `source` over the
/// whole target. The source may use the `FullscreenOut` input struct.
pub fn fullscreen_pipeline(
    device: &wgpu::Device,
    label: &str,
    source: &str,
    layouts: &[&wgpu::BindGroupLayout],
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(format!("{FULLSCREEN_VS}\n{source}").into()),
    });
    let layouts: Vec<_> = layouts.iter().copied().map(Some).collect();
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &layouts,
        ..Default::default()
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// Records a full-screen draw of `pipeline` into `target`, clearing it to
/// black first if `clear`, otherwise drawing over the existing contents.
pub fn draw_fullscreen(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    target: &wgpu::TextureView,
    clear: bool,
    pipeline: &wgpu::RenderPipeline,
    bind_groups: &[&wgpu::BindGroup],
) {
    let mut pass = begin_pass(encoder, label, target, clear);
    pass.set_pipeline(pipeline);
    for (i, group) in bind_groups.iter().enumerate() {
        pass.set_bind_group(i as u32, *group, &[]);
    }
    pass.draw(0..3, 0..1);
}

/// Starts a render pass on `target`, clearing it to black if `clear`.
pub fn begin_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    label: &str,
    target: &wgpu::TextureView,
    clear: bool,
) -> wgpu::RenderPass<'e> {
    let load = if clear {
        wgpu::LoadOp::Clear(wgpu::Color::BLACK)
    } else {
        wgpu::LoadOp::Load
    };
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    })
}

/// Bind group layout entries used throughout.
pub mod entry {
    pub fn uniform(binding: u32) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }
    }

    pub fn texture(binding: u32, filterable: bool) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }
    }

    pub fn sampler(binding: u32) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        }
    }
}

pub fn linear_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("linear clamp"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    })
}

/// A uniform buffer of `N` floats. Keep `N` a multiple of 4 (16-byte aligned).
pub struct Uniforms<const N: usize> {
    pub buffer: wgpu::Buffer,
}

impl<const N: usize> Uniforms<N> {
    pub fn new(device: &wgpu::Device, label: &str) -> Self {
        const {
            assert!(
                N.is_multiple_of(4),
                "uniform size must be a multiple of 16 bytes"
            )
        };
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: (N * size_of::<f32>()) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { buffer }
    }

    pub fn write(&self, queue: &wgpu::Queue, values: [f32; N]) {
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_ne_bytes()).collect();
        queue.write_buffer(&self.buffer, 0, &bytes);
    }
}

/// An offscreen render target. The view keeps its texture alive.
pub struct Target {
    pub view: wgpu::TextureView,
}

impl Target {
    /// An [`HDR_FORMAT`] target.
    pub fn new(device: &wgpu::Device, label: &str, size: (u32, u32)) -> Self {
        Self::with_format(device, label, size, HDR_FORMAT)
    }

    pub fn with_format(
        device: &wgpu::Device,
        label: &str,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size.0.max(1),
                height: size.1.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { view }
    }
}

/// Rows of floats uploaded to the GPU as an `R32Float` texture, read in shaders
/// with `textureLoad(tex, vec2i(index, row), 0).r`.
pub struct DataTexture {
    texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    width: u32,
    bytes: Vec<u8>,
}

impl DataTexture {
    pub fn new(device: &wgpu::Device, label: &str, width: u32, rows: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height: rows,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            width,
            bytes: Vec::new(),
        }
    }

    /// Writes `values` to the start of `row`. Extra values beyond the width are dropped.
    pub fn write_row(
        &mut self,
        queue: &wgpu::Queue,
        row: u32,
        values: impl IntoIterator<Item = f32>,
    ) {
        self.bytes.clear();
        self.bytes.extend(
            values
                .into_iter()
                .take(self.width as usize)
                .flat_map(f32::to_ne_bytes),
        );
        let width = (self.bytes.len() / size_of::<f32>()) as u32;
        if width == 0 {
            return;
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: row, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &self.bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * size_of::<f32>() as u32),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }
}
