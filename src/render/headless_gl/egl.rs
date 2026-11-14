//! Linux backend: an EGL pbuffer on a hardware GPU.
//!
//! With several EGL vendors installed, the default display can be Mesa's
//! software renderer, so the device is picked explicitly.

use std::ffi::{CStr, c_char, c_void};

use anyhow::{Context, Result, anyhow};
use khronos_egl as egl;

const PLATFORM_DEVICE_EXT: egl::Enum = 0x313F;
const SOFTWARE_DEVICE_EXT: &str = "EGL_MESA_device_software";
const MAX_DEVICES: usize = 16;

type QueryDevicesExt = unsafe extern "C" fn(i32, *mut *mut c_void, *mut i32) -> egl::Boolean;
type QueryDeviceStringExt = unsafe extern "C" fn(*mut c_void, egl::Int) -> *const c_char;

pub struct Backend {
    egl: egl::Instance<egl::Static>,
    display: egl::Display,
    config: egl::Config,
    context: egl::Context,
    surface: egl::Surface,
}

impl Backend {
    /// Creates an OpenGL 3.3 core context on a `size` pbuffer, current on
    /// the calling thread.
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
        let backend = Self {
            egl,
            display,
            config,
            context,
            surface,
        };
        backend.make_current()?;
        Ok(backend)
    }

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

    /// Replaces the pbuffer with one of `size`, leaving the context current on it.
    pub fn resize(&mut self, size: (u32, u32)) -> Result<()> {
        let surface = create_pbuffer(&self.egl, self.display, self.config, size)?;
        let old = std::mem::replace(&mut self.surface, surface);
        let current = self.make_current();
        let _ = self.egl.destroy_surface(self.display, old);
        current
    }

    pub fn proc_address(&self, name: &str) -> *const c_void {
        self.egl
            .get_proc_address(name)
            .map_or(std::ptr::null(), |f| f as *const c_void)
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
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
