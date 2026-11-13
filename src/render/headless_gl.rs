//! A hidden OpenGL 3.3 context for libraries that only speak OpenGL
//! (projectM). Its default framebuffer is an EGL pbuffer of the requested
//! size, which can be read back with `glReadPixels`.
//!
//! The context is picked on a hardware GPU: with several EGL vendors
//! installed, the default display can be Mesa's software renderer.

use std::ffi::{CStr, c_char, c_void};

use anyhow::{Context, Result, anyhow};
use glow::HasContext;
use khronos_egl as egl;

const PLATFORM_DEVICE_EXT: egl::Enum = 0x313F;
const SOFTWARE_DEVICE_EXT: &str = "EGL_MESA_device_software";
const MAX_DEVICES: usize = 16;

type QueryDevicesExt = unsafe extern "C" fn(i32, *mut *mut c_void, *mut i32) -> egl::Boolean;
type QueryDeviceStringExt = unsafe extern "C" fn(*mut c_void, egl::Int) -> *const c_char;

pub struct HeadlessGl {
    pub gl: glow::Context,
    egl: egl::Instance<egl::Static>,
    display: egl::Display,
    config: egl::Config,
    context: egl::Context,
    surface: egl::Surface,
    size: (u32, u32),
    readback: Option<Readback>,
}

impl HeadlessGl {
    pub fn new(size: (u32, u32)) -> Result<Self> {
        let egl = egl::Instance::new(egl::Static);
        let display = hardware_display(&egl)?;
        egl.bind_api(egl::OPENGL_API)
            .map_err(|e| anyhow!("eglBindAPI: {e}"))?;

        #[rustfmt::skip]
        let config_attrs = [
            egl::SURFACE_TYPE, egl::PBUFFER_BIT,
            egl::RENDERABLE_TYPE, egl::OPENGL_BIT,
            egl::RED_SIZE, 8, egl::GREEN_SIZE, 8, egl::BLUE_SIZE, 8, egl::ALPHA_SIZE, 8,
            egl::NONE,
        ];
        let config = egl
            .choose_first_config(display, &config_attrs)
            .map_err(|e| anyhow!("eglChooseConfig: {e}"))?
            .context("no suitable EGL config")?;

        #[rustfmt::skip]
        let context_attrs = [
            egl::CONTEXT_MAJOR_VERSION, 3,
            egl::CONTEXT_MINOR_VERSION, 3,
            egl::CONTEXT_OPENGL_PROFILE_MASK, egl::CONTEXT_OPENGL_CORE_PROFILE_BIT,
            egl::NONE,
        ];
        let context = egl
            .create_context(display, config, None, &context_attrs)
            .map_err(|e| anyhow!("eglCreateContext: {e}"))?;
        let surface = create_pbuffer(&egl, display, config, size)?;
        egl.make_current(display, Some(surface), Some(surface), Some(context))
            .map_err(|e| anyhow!("eglMakeCurrent: {e}"))?;

        // SAFETY: the context is current on this thread, and the loader
        // returns GL entry points (or null) for it.
        let gl = unsafe {
            glow::Context::from_loader_function(|name| {
                egl.get_proc_address(name)
                    .map_or(std::ptr::null(), |f| f as *const _)
            })
        };
        // SAFETY: plain state queries on the current context.
        let renderer = unsafe { gl.get_parameter_string(glow::RENDERER) };
        log::info!("MilkDrop OpenGL: {renderer}");

        Ok(Self {
            gl,
            egl,
            display,
            config,
            context,
            surface,
            size,
            readback: None,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Makes this context current on the calling thread.
    pub fn make_current(&self) -> Result<()> {
        self.egl
            .make_current(
                self.display,
                Some(self.surface),
                Some(self.surface),
                Some(self.context),
            )
            .map_err(|e| anyhow!("eglMakeCurrent: {e}"))
    }

    /// Recreates the pbuffer at `size` and makes the context current on it.
    pub fn resize(&mut self, size: (u32, u32)) -> Result<()> {
        if size == self.size {
            return Ok(());
        }
        let surface = create_pbuffer(&self.egl, self.display, self.config, size)?;
        self.egl
            .make_current(
                self.display,
                Some(surface),
                Some(surface),
                Some(self.context),
            )
            .map_err(|e| anyhow!("eglMakeCurrent: {e}"))?;
        let _ = self.egl.destroy_surface(self.display, self.surface);
        self.surface = surface;
        self.size = size;
        if let Some(rb) = self.readback.take() {
            rb.delete(&self.gl);
        }
        Ok(())
    }

    /// Starts copying the frame just rendered into a GPU-side buffer, and
    /// fills `out` with the *previous* frame's pixels (RGBA8, bottom row
    /// first). Returns false if there was no previous frame yet.
    ///
    /// Reading the frame directly would stall until the GPU finishes it (and
    /// NVIDIA's driver busy-waits meanwhile); one frame of delay avoids that.
    pub fn read_previous_frame(&mut self, out: &mut Vec<u8>) -> bool {
        let len = self.size.0 as usize * self.size.1 as usize * 4;
        if self.readback.as_ref().is_none_or(|r| r.len != len) {
            self.readback = Some(Readback::new(&self.gl, len));
        }
        let Some(rb) = self.readback.as_mut() else {
            return false;
        };
        let (w, h) = (self.size.0 as i32, self.size.1 as i32);
        let (now, previous) = (rb.next, 1 - rb.next);
        rb.next = previous;
        // SAFETY: the context is current; each buffer holds `len` bytes, which
        // is exactly one w*h RGBA8 frame, and `out` is resized to match.
        unsafe {
            let gl = &self.gl;
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, None);
            gl.bind_buffer(glow::PIXEL_PACK_BUFFER, Some(rb.buffers[now]));
            gl.read_pixels(
                0,
                0,
                w,
                h,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::BufferOffset(0),
            );
            rb.filled[now] = true;

            let mut got = false;
            if rb.filled[previous] {
                gl.bind_buffer(glow::PIXEL_PACK_BUFFER, Some(rb.buffers[previous]));
                let ptr =
                    gl.map_buffer_range(glow::PIXEL_PACK_BUFFER, 0, len as i32, glow::MAP_READ_BIT);
                if !ptr.is_null() {
                    out.resize(len, 0);
                    std::ptr::copy_nonoverlapping(ptr, out.as_mut_ptr(), len);
                    got = true;
                }
                gl.unmap_buffer(glow::PIXEL_PACK_BUFFER);
            }
            gl.bind_buffer(glow::PIXEL_PACK_BUFFER, None);
            got
        }
    }
}

/// Two pixel-pack buffers used in turn for asynchronous readback.
struct Readback {
    buffers: [glow::Buffer; 2],
    filled: [bool; 2],
    next: usize,
    len: usize,
}

impl Readback {
    fn new(gl: &glow::Context, len: usize) -> Self {
        // SAFETY: creating and sizing buffers on the current context.
        let buffers = std::array::from_fn(|_| unsafe {
            let b = gl.create_buffer().expect("creating a pixel buffer");
            gl.bind_buffer(glow::PIXEL_PACK_BUFFER, Some(b));
            gl.buffer_data_size(glow::PIXEL_PACK_BUFFER, len as i32, glow::STREAM_READ);
            b
        });
        // SAFETY: unbinding on the current context.
        unsafe { gl.bind_buffer(glow::PIXEL_PACK_BUFFER, None) };
        Self {
            buffers,
            filled: [false; 2],
            next: 0,
            len,
        }
    }

    fn delete(self, gl: &glow::Context) {
        for b in self.buffers {
            // SAFETY: the buffer belongs to this context and is no longer used.
            unsafe { gl.delete_buffer(b) };
        }
    }
}

impl Drop for HeadlessGl {
    fn drop(&mut self) {
        if let Some(rb) = self.readback.take()
            && self.make_current().is_ok()
        {
            rb.delete(&self.gl);
        }
        let _ = self.egl.make_current(self.display, None, None, None);
        let _ = self.egl.destroy_surface(self.display, self.surface);
        let _ = self.egl.destroy_context(self.display, self.context);
    }
}

fn create_pbuffer(
    egl: &egl::Instance<egl::Static>,
    display: egl::Display,
    config: egl::Config,
    size: (u32, u32),
) -> Result<egl::Surface> {
    let attrs = [
        egl::WIDTH,
        size.0.max(1) as egl::Int,
        egl::HEIGHT,
        size.1.max(1) as egl::Int,
        egl::NONE,
    ];
    egl.create_pbuffer_surface(display, config, &attrs)
        .map_err(|e| anyhow!("eglCreatePbufferSurface: {e}"))
}

/// An initialized EGL display on a hardware GPU, via EGL_EXT_device_enumeration.
fn hardware_display(egl: &egl::Instance<egl::Static>) -> Result<egl::Display> {
    let query_devices = egl
        .get_proc_address("eglQueryDevicesEXT")
        .context("EGL device enumeration isn't supported")?;
    let query_string = egl
        .get_proc_address("eglQueryDeviceStringEXT")
        .context("EGL device queries aren't supported")?;
    // SAFETY: signatures from the EGL_EXT_device_enumeration / _query specs.
    let (query_devices, query_string): (QueryDevicesExt, QueryDeviceStringExt) = unsafe {
        (
            std::mem::transmute::<extern "system" fn(), QueryDevicesExt>(query_devices),
            std::mem::transmute::<extern "system" fn(), QueryDeviceStringExt>(query_string),
        )
    };

    let mut devices = [std::ptr::null_mut(); MAX_DEVICES];
    let mut count = 0;
    // SAFETY: `devices` has room for MAX_DEVICES handles.
    if unsafe { query_devices(MAX_DEVICES as i32, devices.as_mut_ptr(), &mut count) } == egl::FALSE
    {
        return Err(anyhow!("eglQueryDevicesEXT failed"));
    }
    for &device in &devices[..count.clamp(0, MAX_DEVICES as i32) as usize] {
        // SAFETY: `device` came from eglQueryDevicesEXT; the result is a
        // NUL-terminated string owned by EGL, or null.
        let extensions = unsafe {
            let ptr = query_string(device, egl::EXTENSIONS);
            if ptr.is_null() {
                String::new()
            } else {
                CStr::from_ptr(ptr).to_string_lossy().into_owned()
            }
        };
        if extensions.contains(SOFTWARE_DEVICE_EXT) {
            continue;
        }
        // SAFETY: a device handle is the native display for PLATFORM_DEVICE.
        let Ok(display) =
            (unsafe { egl.get_platform_display(PLATFORM_DEVICE_EXT, device, &[egl::ATTRIB_NONE]) })
        else {
            continue;
        };
        if egl.initialize(display).is_ok() {
            return Ok(display);
        }
    }
    Err(anyhow!("no hardware GPU available to EGL"))
}
