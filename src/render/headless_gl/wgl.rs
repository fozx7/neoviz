//! Windows backend: a WGL pbuffer (WGL_ARB_pbuffer).
//!
//! projectM always draws to framebuffer 0, so a framebuffer object won't do,
//! and a hidden window's own framebuffer isn't reliably readable. A pbuffer
//! is an offscreen drawable with its own framebuffer 0, like EGL's on Linux.
//!
//! WGL can only load the pbuffer and core-context entry points through an
//! existing context, so a hidden window with a legacy context is created
//! first and thrown away once the real context is current.

use std::{
    ffi::{CString, c_void},
    ptr::{null, null_mut},
};

use anyhow::{Context, Result, anyhow, bail};
use windows_sys::Win32::{
    Foundation::{HMODULE, HWND},
    Graphics::{
        Gdi::{GetDC, HDC, ReleaseDC},
        OpenGL::{
            ChoosePixelFormat, HGLRC, PFD_DOUBLEBUFFER, PFD_DRAW_TO_WINDOW, PFD_MAIN_PLANE,
            PFD_SUPPORT_OPENGL, PFD_TYPE_RGBA, PIXELFORMATDESCRIPTOR, SetPixelFormat,
            wglCreateContext, wglDeleteContext, wglGetCurrentContext, wglGetProcAddress,
            wglMakeCurrent,
        },
    },
    System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
    UI::WindowsAndMessaging::{
        CS_OWNDC, CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW, WNDCLASSEXW,
        WS_OVERLAPPEDWINDOW,
    },
};

// WGL_ARB_pixel_format
const WGL_DRAW_TO_PBUFFER_ARB: i32 = 0x202D;
const WGL_SUPPORT_OPENGL_ARB: i32 = 0x2010;
const WGL_ACCELERATION_ARB: i32 = 0x2003;
const WGL_FULL_ACCELERATION_ARB: i32 = 0x2027;
const WGL_PIXEL_TYPE_ARB: i32 = 0x2013;
const WGL_TYPE_RGBA_ARB: i32 = 0x202B;
const WGL_RED_BITS_ARB: i32 = 0x2015;
const WGL_GREEN_BITS_ARB: i32 = 0x2017;
const WGL_BLUE_BITS_ARB: i32 = 0x2019;
const WGL_ALPHA_BITS_ARB: i32 = 0x201B;
// WGL_ARB_create_context(_profile)
const WGL_CONTEXT_MAJOR_VERSION_ARB: i32 = 0x2091;
const WGL_CONTEXT_MINOR_VERSION_ARB: i32 = 0x2092;
const WGL_CONTEXT_PROFILE_MASK_ARB: i32 = 0x9126;
const WGL_CONTEXT_CORE_PROFILE_BIT_ARB: i32 = 0x0001;

type Hpbuffer = *mut c_void;
type ChoosePixelFormatArb =
    unsafe extern "system" fn(HDC, *const i32, *const f32, u32, *mut i32, *mut u32) -> i32;
type CreateContextAttribsArb = unsafe extern "system" fn(HDC, HGLRC, *const i32) -> HGLRC;
type CreatePbufferArb = unsafe extern "system" fn(HDC, i32, i32, i32, *const i32) -> Hpbuffer;
type GetPbufferDcArb = unsafe extern "system" fn(Hpbuffer) -> HDC;
type ReleasePbufferDcArb = unsafe extern "system" fn(Hpbuffer, HDC) -> i32;
type DestroyPbufferArb = unsafe extern "system" fn(Hpbuffer) -> i32;

pub struct Backend {
    arb: Arb,
    format: i32,
    context: HGLRC,
    pbuffer: Pbuffer,
    /// For GL 1.1 functions, which `wglGetProcAddress` doesn't return.
    opengl32: HMODULE,
}

impl Backend {
    /// Creates an OpenGL 3.3 core context on a `size` pbuffer, current on
    /// the calling thread.
    pub fn new(size: (u32, u32)) -> Result<Self> {
        let bootstrap = Bootstrap::new()?;
        let arb = Arb::load()?;
        let format = choose_pbuffer_format(&arb, bootstrap.dc)?;
        let pbuffer = Pbuffer::new(&arb, bootstrap.dc, format, size)?;

        #[rustfmt::skip]
        let attrs = [
            WGL_CONTEXT_MAJOR_VERSION_ARB, 3,
            WGL_CONTEXT_MINOR_VERSION_ARB, 3,
            WGL_CONTEXT_PROFILE_MASK_ARB, WGL_CONTEXT_CORE_PROFILE_BIT_ARB,
            0,
        ];
        // SAFETY: a valid DC with a pixel format, and a zero-terminated list.
        let context =
            unsafe { (arb.create_context_attribs)(pbuffer.dc, null_mut(), attrs.as_ptr()) };
        if context.is_null() {
            pbuffer.destroy(&arb);
            bail!(
                "wglCreateContextAttribsARB (OpenGL 3.3 core) failed: {}",
                last_error()
            );
        }
        let opengl32 = module_handle("opengl32.dll")?;
        let backend = Self {
            arb,
            format,
            context,
            pbuffer,
            opengl32,
        };
        backend.make_current()?;
        drop(bootstrap);
        Ok(backend)
    }

    pub fn make_current(&self) -> Result<()> {
        // SAFETY: the DC and context belong to this backend and are alive.
        if unsafe { wglMakeCurrent(self.pbuffer.dc, self.context) } == 0 {
            return Err(anyhow!("wglMakeCurrent failed: {}", last_error()));
        }
        Ok(())
    }

    /// Replaces the pbuffer with one of `size`, leaving the context current on it.
    pub fn resize(&mut self, size: (u32, u32)) -> Result<()> {
        let fresh = Pbuffer::new(&self.arb, self.pbuffer.dc, self.format, size)?;
        let old = std::mem::replace(&mut self.pbuffer, fresh);
        let current = self.make_current();
        old.destroy(&self.arb);
        current
    }

    pub fn proc_address(&self, name: &str) -> *const c_void {
        let Ok(name) = CString::new(name) else {
            return null();
        };
        // SAFETY: a NUL-terminated name; a context is current on this thread.
        let wgl = unsafe { wglGetProcAddress(name.as_ptr().cast()) };
        // Some drivers return small sentinel values instead of null on failure.
        if let Some(f) = wgl
            && !matches!(f as usize, 1..=3 | usize::MAX)
        {
            return f as *const c_void;
        }
        // SAFETY: `opengl32` is loaded for the life of the process.
        unsafe { GetProcAddress(self.opengl32, name.as_ptr().cast()) }
            .map_or(null(), |f| f as *const c_void)
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        // SAFETY: releasing and deleting this backend's own objects.
        unsafe {
            wglMakeCurrent(null_mut(), null_mut());
            wglDeleteContext(self.context);
        }
        self.pbuffer.destroy(&self.arb);
    }
}

/// The WGL extension entry points used here.
struct Arb {
    choose_pixel_format: ChoosePixelFormatArb,
    create_context_attribs: CreateContextAttribsArb,
    create_pbuffer: CreatePbufferArb,
    get_pbuffer_dc: GetPbufferDcArb,
    release_pbuffer_dc: ReleasePbufferDcArb,
    destroy_pbuffer: DestroyPbufferArb,
}

impl Arb {
    /// Needs a context current on this thread (the bootstrap one).
    fn load() -> Result<Self> {
        // SAFETY: each type matches the function's signature in the WGL_ARB specs.
        unsafe {
            Ok(Self {
                choose_pixel_format: wgl_function("wglChoosePixelFormatARB")?,
                create_context_attribs: wgl_function("wglCreateContextAttribsARB")?,
                create_pbuffer: wgl_function("wglCreatePbufferARB")?,
                get_pbuffer_dc: wgl_function("wglGetPbufferDCARB")?,
                release_pbuffer_dc: wgl_function("wglReleasePbufferDCARB")?,
                destroy_pbuffer: wgl_function("wglDestroyPbufferARB")?,
            })
        }
    }
}

/// Looks up a WGL extension function.
///
/// # Safety
/// `F` must be the `unsafe extern "system" fn` type of `name`'s signature,
/// and a context must be current on this thread.
unsafe fn wgl_function<F: Copy>(name: &str) -> Result<F> {
    let c_name = CString::new(name)?;
    // SAFETY: a NUL-terminated name; the caller guarantees a current context.
    let f = unsafe { wglGetProcAddress(c_name.as_ptr().cast()) }
        .with_context(|| format!("{name} isn't supported by the graphics driver"))?;
    // SAFETY: function pointers are all the same size; the caller
    // guarantees `F` is the right signature.
    Ok(unsafe { std::mem::transmute_copy::<_, F>(&f) })
}

/// An accelerated RGBA8 pixel format that can draw to a pbuffer.
fn choose_pbuffer_format(arb: &Arb, dc: HDC) -> Result<i32> {
    #[rustfmt::skip]
    let attrs = [
        WGL_DRAW_TO_PBUFFER_ARB, 1,
        WGL_SUPPORT_OPENGL_ARB, 1,
        WGL_ACCELERATION_ARB, WGL_FULL_ACCELERATION_ARB,
        WGL_PIXEL_TYPE_ARB, WGL_TYPE_RGBA_ARB,
        WGL_RED_BITS_ARB, 8, WGL_GREEN_BITS_ARB, 8, WGL_BLUE_BITS_ARB, 8, WGL_ALPHA_BITS_ARB, 8,
        0,
    ];
    let (mut format, mut count) = (0, 0);
    // SAFETY: a valid DC, a zero-terminated list, and room for one format.
    let ok = unsafe {
        (arb.choose_pixel_format)(dc, attrs.as_ptr(), null(), 1, &mut format, &mut count)
    };
    if ok == 0 || count == 0 {
        bail!("no accelerated pbuffer pixel format: {}", last_error());
    }
    Ok(format)
}

/// An offscreen pbuffer and its device context.
struct Pbuffer {
    handle: Hpbuffer,
    dc: HDC,
}

impl Pbuffer {
    /// `device_dc` is any DC on the same GPU (used only to pick the device).
    fn new(arb: &Arb, device_dc: HDC, format: i32, size: (u32, u32)) -> Result<Self> {
        let (w, h) = (size.0.max(1) as i32, size.1.max(1) as i32);
        let attrs = [0];
        // SAFETY: a valid DC and pixel format, and a zero-terminated list.
        let handle = unsafe { (arb.create_pbuffer)(device_dc, format, w, h, attrs.as_ptr()) };
        if handle.is_null() {
            bail!("wglCreatePbufferARB ({w}x{h}) failed: {}", last_error());
        }
        // SAFETY: `handle` is the pbuffer just created.
        let dc = unsafe { (arb.get_pbuffer_dc)(handle) };
        if dc.is_null() {
            // SAFETY: as above; it's not used again.
            unsafe { (arb.destroy_pbuffer)(handle) };
            bail!("wglGetPbufferDCARB failed: {}", last_error());
        }
        Ok(Self { handle, dc })
    }

    fn destroy(&self, arb: &Arb) {
        // SAFETY: this pbuffer's own handle and DC; callers don't use them again.
        unsafe {
            (arb.release_pbuffer_dc)(self.handle, self.dc);
            (arb.destroy_pbuffer)(self.handle);
        }
    }
}

/// A hidden window with a legacy OpenGL context, current while it lives.
struct Bootstrap {
    window: HWND,
    dc: HDC,
    context: HGLRC,
}

impl Bootstrap {
    fn new() -> Result<Self> {
        let class = wide("neoviz-gl");
        let instance = module_handle_self();
        let window_class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_OWNDC,
            lpfnWndProc: Some(DefWindowProcW),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class.as_ptr(),
            hIconSm: null_mut(),
        };
        // SAFETY: a fully initialized class description. Fails harmlessly if
        // the class is already registered (e.g. MilkDrop restarted), and any
        // real problem shows up in CreateWindowExW below.
        unsafe { RegisterClassExW(&window_class) };

        // SAFETY: a registered class; the window is never shown.
        let window = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                1,
                1,
                null_mut(),
                null_mut(),
                instance,
                null(),
            )
        };
        if window.is_null() {
            bail!(
                "creating a hidden window for OpenGL failed: {}",
                last_error()
            );
        }
        let mut bootstrap = Self {
            window,
            // SAFETY: a window we own.
            dc: unsafe { GetDC(window) },
            context: null_mut(),
        };
        bootstrap.create_legacy_context()?;
        Ok(bootstrap)
    }

    fn create_legacy_context(&mut self) -> Result<()> {
        let pfd = PIXELFORMATDESCRIPTOR {
            nSize: size_of::<PIXELFORMATDESCRIPTOR>() as u16,
            nVersion: 1,
            dwFlags: PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER,
            iPixelType: PFD_TYPE_RGBA,
            cColorBits: 32,
            cAlphaBits: 8,
            iLayerType: PFD_MAIN_PLANE as u8,
            ..Default::default()
        };
        // SAFETY: the window's own DC and a fully initialized descriptor.
        unsafe {
            let format = ChoosePixelFormat(self.dc, &pfd);
            if format == 0 || SetPixelFormat(self.dc, format, &pfd) == 0 {
                bail!("no OpenGL pixel format for the window: {}", last_error());
            }
            self.context = wglCreateContext(self.dc);
            if self.context.is_null() {
                bail!("wglCreateContext failed: {}", last_error());
            }
            if wglMakeCurrent(self.dc, self.context) == 0 {
                bail!("wglMakeCurrent (bootstrap) failed: {}", last_error());
            }
        }
        Ok(())
    }
}

impl Drop for Bootstrap {
    fn drop(&mut self) {
        // SAFETY: tearing down this struct's own window, DC and context.
        unsafe {
            if !self.context.is_null() {
                if wglGetCurrentContext() == self.context {
                    wglMakeCurrent(null_mut(), null_mut());
                }
                wglDeleteContext(self.context);
            }
            ReleaseDC(self.window, self.dc);
            DestroyWindow(self.window);
        }
    }
}

fn module_handle(name: &str) -> Result<HMODULE> {
    let name_w = wide(name);
    // SAFETY: a NUL-terminated UTF-16 name.
    let module = unsafe { GetModuleHandleW(name_w.as_ptr()) };
    if module.is_null() {
        bail!("{name} isn't loaded: {}", last_error());
    }
    Ok(module)
}

/// This executable's module handle.
fn module_handle_self() -> HMODULE {
    // SAFETY: null asks for the calling process's own module, which always exists.
    unsafe { GetModuleHandleW(null()) }
}

/// A NUL-terminated UTF-16 copy of `s`.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error() -> std::io::Error {
    std::io::Error::last_os_error()
}
