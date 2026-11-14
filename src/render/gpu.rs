//! wgpu device setup, and the window surfaces ("displays") frames go to.
//!
//! There's one [`Gpu`] and one or more [`Display`]s: normally just the main
//! window, plus one per extra monitor in span mode.

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use winit::{dpi::PhysicalSize, event_loop::OwnedDisplayHandle, window::Window};

pub struct Gpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_format: wgpu::TextureFormat,
    ui_format: wgpu::TextureFormat,
}

/// A window and the surface it's drawn through.
pub struct Display {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

/// A frame acquired from a display, to draw into and then present.
pub struct Acquired {
    pub frame: wgpu::SurfaceTexture,
    /// The surface changed underneath us; reconfigure after presenting.
    reconfigure: bool,
}

impl Gpu {
    /// Sets up the GPU for `window`, returning it as the first display.
    ///
    /// wgpu keeps the display handle for its whole lifetime, so it gets the
    /// event loop's, not a window: holding a window would keep it open after
    /// it's meant to close (e.g. when span mode ends).
    pub async fn new(display: OwnedDisplayHandle, window: Arc<Window>) -> Result<(Self, Display)> {
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(display)),
        );
        let surface = instance
            .create_surface(window.clone())
            .context("creating window surface")?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .context("no compatible GPU adapter found")?;
        let info = adapter.get_info();
        log::info!(
            "GPU: {} ({:?}, {:?})",
            info.name,
            info.backend,
            info.device_type
        );

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("neoviz device"),
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await
            .context("requesting GPU device")?;

        let size = window.inner_size();
        let defaults = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .context("surface is not supported by this adapter")?;
        let surface_format = defaults.format;
        // egui blends in gamma space, so it wants a non-sRGB view of the frame.
        // Where the backend can't reinterpret surface formats it shares the main one.
        let plain = surface_format.remove_srgb_suffix();
        let can_reinterpret = adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS);
        let ui_format = if plain != surface_format && can_reinterpret {
            plain
        } else {
            surface_format
        };

        let gpu = Self {
            instance,
            adapter,
            device,
            queue,
            surface_format,
            ui_format,
        };
        let display = gpu.make_display(window, surface)?;
        Ok((gpu, display))
    }

    /// Creates a display for another window (e.g. a second monitor).
    pub fn add_display(&self, window: Arc<Window>) -> Result<Display> {
        let surface = self
            .instance
            .create_surface(window.clone())
            .context("creating window surface")?;
        self.make_display(window, surface)
    }

    fn make_display(
        &self,
        window: Arc<Window>,
        surface: wgpu::Surface<'static>,
    ) -> Result<Display> {
        let caps = surface.get_capabilities(&self.adapter);
        if !caps.formats.contains(&self.surface_format) {
            bail!("this window can't use the {:?} format", self.surface_format);
        }
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&self.adapter, size.width.max(1), size.height.max(1))
            .context("surface is not supported by this adapter")?;
        config.format = self.surface_format;
        // FIFO = vsync: steady frame pacing and no tearing, supported everywhere.
        config.present_mode = wgpu::PresentMode::Fifo;
        // Two frames in flight: with one, any frame that runs a little long
        // (a slow GPU, a MilkDrop readback) misses vsync and shows twice,
        // which reads as jitter. Costs one frame (~16 ms) of latency.
        config.desired_maximum_frame_latency = 2;
        if self.ui_format != self.surface_format {
            config.view_formats.push(self.ui_format);
        }
        surface.configure(&self.device, &config);
        Ok(Display {
            window,
            surface,
            config,
        })
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Format of every display's frames (usually sRGB).
    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.surface_format
    }

    /// Format of the view egui draws through (see [`Display::ui_view`]).
    pub fn ui_format(&self) -> wgpu::TextureFormat {
        self.ui_format
    }
}

impl Display {
    pub fn resize(&mut self, gpu: &Gpu, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return; // Minimised; keep the old config until we're visible again.
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&gpu.device, &self.config);
    }

    /// Surface size in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// The next frame to draw into, or `None` if there isn't one this time
    /// (minimised, timed out, or the surface needed reconfiguring).
    pub fn acquire(&mut self, gpu: &Gpu) -> Option<Acquired> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => Some(Acquired {
                frame,
                reconfigure: false,
            }),
            // Draw this one, then reconfigure so the next frame matches the surface.
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Some(Acquired {
                frame,
                reconfigure: true,
            }),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                log::warn!("surface outdated or lost; reconfiguring");
                self.surface.configure(&gpu.device, &self.config);
                None
            }
            other => {
                log::debug!("skipping frame: {other:?}");
                None
            }
        }
    }

    /// Presents a frame acquired from this display.
    pub fn present(&mut self, gpu: &Gpu, acquired: Acquired) {
        gpu.queue.present(acquired.frame);
        if acquired.reconfigure {
            self.surface.configure(&gpu.device, &self.config);
        }
    }
}

impl Acquired {
    pub fn view(&self) -> wgpu::TextureView {
        self.frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default())
    }

    /// A view in [`Gpu::ui_format`], for egui.
    pub fn ui_view(&self, gpu: &Gpu) -> wgpu::TextureView {
        self.frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor {
                format: Some(gpu.ui_format),
                ..Default::default()
            })
    }
}
