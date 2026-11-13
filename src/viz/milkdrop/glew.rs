//! On Windows, projectM calls OpenGL through GLEW (linked statically with it)
//! but leaves initializing GLEW to the host app.

use anyhow::{Result, bail};
use glow::HasContext;

const GLEW_OK: u32 = 0;
/// Bound on draining stale GL errors (a lost context reports one forever).
const MAX_STALE_ERRORS: usize = 16;

unsafe extern "system" {
    /// Lets GLEW load entry points on core-profile contexts.
    static mut glewExperimental: u8;
    fn glewInit() -> u32;
}

/// Loads GLEW's function pointers for the current context. Call on the
/// thread that owns it, after it's made current and before creating projectM.
pub fn init(gl: &glow::Context) -> Result<()> {
    // SAFETY: GLEW's globals are only touched here, once, on the GL thread,
    // with its context current.
    let status = unsafe {
        glewExperimental = 1;
        glewInit()
    };
    if status != GLEW_OK {
        bail!("glewInit failed (GLEW error {status})");
    }
    // glewInit can leave a harmless GL_INVALID_ENUM behind on core profiles;
    // clear it so projectM's own error checks don't trip on it.
    for _ in 0..MAX_STALE_ERRORS {
        // SAFETY: a plain query on the current context.
        if unsafe { gl.get_error() } == glow::NO_ERROR {
            break;
        }
    }
    Ok(())
}
