//! A hidden OpenGL 3.3 core context for libraries that only speak OpenGL
//! (projectM). It renders into an offscreen pbuffer that stands in for the
//! default framebuffer, which can be read back with `glReadPixels`.
//!
//! Creating the context is per-OS (`egl` on Linux, `wgl` on Windows); the
//! readback is shared.

#[cfg(target_os = "linux")]
mod egl;
#[cfg(windows)]
mod wgl;

#[cfg(target_os = "linux")]
use egl::Backend;
#[cfg(windows)]
use wgl::Backend;

use anyhow::Result;
use glow::HasContext;

pub struct HeadlessGl {
    pub gl: glow::Context,
    size: (u32, u32),
    readback: Option<Readback>,
    /// Declared last so it's dropped after the readback buffers are freed.
    backend: Backend,
}

impl HeadlessGl {
    /// Creates the context with a `size` pbuffer, current on the calling thread.
    pub fn new(size: (u32, u32)) -> Result<Self> {
        let backend = Backend::new(size)?;
        // SAFETY: the context is current on this thread, and the loader
        // returns GL entry points (or null) for it.
        let gl = unsafe { glow::Context::from_loader_function(|name| backend.proc_address(name)) };
        // SAFETY: plain state queries on the current context.
        let renderer = unsafe { gl.get_parameter_string(glow::RENDERER) };
        log::info!("MilkDrop OpenGL: {renderer}");
        Ok(Self {
            gl,
            size,
            readback: None,
            backend,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Makes this context current on the calling thread.
    pub fn make_current(&self) -> Result<()> {
        self.backend.make_current()
    }

    /// Recreates the pbuffer at `size` and makes the context current on it.
    pub fn resize(&mut self, size: (u32, u32)) -> Result<()> {
        if size == self.size {
            return Ok(());
        }
        self.backend.resize(size)?;
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
    }
}
