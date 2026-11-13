//! Runs projectM on its own thread, so slow preset loads (some take a third
//! of a second to compile their shaders) never freeze the app or delay input.
//!
//! The main thread sends commands; the worker handles everything queued,
//! renders at most once per batch, and leaves the newest frame in a shared
//! slot for the main thread to pick up.

use std::{
    cell::Cell,
    ffi::{CString, c_char, c_void},
    panic::AssertUnwindSafe,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread::JoinHandle,
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use projectm::core::ProjectM;

use crate::render::headless_gl::HeadlessGl;

const RENDER_FPS: u32 = 60;
/// How long to wait for the worker to set up OpenGL and projectM.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

pub enum Command {
    /// Interleaved stereo samples, -1..1.
    Audio(Vec<f32>),
    Load {
        path: PathBuf,
        smooth: bool,
    },
    /// Render a frame at this size.
    Render((u32, u32)),
    /// Hand a pixel buffer back for reuse.
    Recycle(Vec<u8>),
}

pub enum Event {
    /// projectM couldn't load this preset (under 1% of the cream-of-the-crop
    /// pack: shader syntax it can't translate) and kept the previous one.
    LoadFailed { path: PathBuf },
}

/// A rendered frame: RGBA8, bottom row first.
pub struct Frame {
    pub pixels: Vec<u8>,
    pub size: (u32, u32),
}

pub struct Worker {
    commands: Option<Sender<Command>>,
    events: Receiver<Event>,
    frame: Arc<Mutex<Option<Frame>>>,
    thread: Option<JoinHandle<()>>,
    /// Set once the thread is found dead, so that's only logged once.
    reported_stop: Cell<bool>,
}

impl Worker {
    /// Starts the thread and waits until projectM is ready (or failed).
    pub fn start(size: (u32, u32)) -> Result<Self> {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let frame = Arc::new(Mutex::new(None));
        let slot = frame.clone();
        let thread = std::thread::Builder::new()
            .name("milkdrop".into())
            .spawn(move || run(size, cmd_rx, event_tx, slot, ready_tx))
            .context("spawning the MilkDrop thread")?;
        ready_rx
            .recv_timeout(STARTUP_TIMEOUT)
            .map_err(|_| anyhow!("MilkDrop didn't start in time"))??;
        Ok(Self {
            commands: Some(cmd_tx),
            events: event_rx,
            frame,
            thread: Some(thread),
            reported_stop: Cell::new(false),
        })
    }

    pub fn send(&self, command: Command) {
        if let Some(tx) = &self.commands
            && tx.send(command).is_err()
            && !self.reported_stop.replace(true)
        {
            log::error!("MilkDrop thread has stopped; MilkDrop will stay frozen");
        }
    }

    /// The newest finished frame, if one arrived since the last call.
    pub fn take_frame(&self) -> Option<Frame> {
        self.frame.lock().ok()?.take()
    }

    pub fn try_event(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Closing the channel tells the thread to finish.
        self.commands.take();
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            log::error!("MilkDrop thread panicked");
        }
    }
}

fn run(
    size: (u32, u32),
    commands: Receiver<Command>,
    events: Sender<Event>,
    slot: Arc<Mutex<Option<Frame>>>,
    ready: mpsc::SyncSender<Result<()>>,
) {
    let mut gl = match HeadlessGl::new(size) {
        Ok(gl) => gl,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    #[cfg(windows)]
    if let Err(e) = super::glew::init(&gl.gl) {
        let _ = ready.send(Err(e));
        return;
    }
    // projectM needs its GL context current (on this thread) when created.
    let pm = ProjectM::create();
    pm.set_window_size(size.0 as usize, size.1 as usize);
    pm.set_fps(RENDER_FPS);
    // We choose presets ourselves; stop projectM switching on its own.
    pm.set_preset_locked(true);
    pm.set_hard_cut_enabled(false);
    let load_failed = install_failure_hook(&pm);
    let max_floats = (ProjectM::pcm_get_max_samples() as usize & !1).max(2);
    let _ = ready.send(Ok(()));

    let mut spare: Vec<Vec<u8>> = Vec::new();
    // Block for the next command, then take everything else already queued:
    // audio and loads are applied in order, but only the last render request
    // is drawn, so a slow preset load doesn't build up a backlog of frames.
    while let Ok(first) = commands.recv() {
        let mut render = None;
        for command in std::iter::once(first).chain(commands.try_iter()) {
            match command {
                Command::Audio(pcm) => {
                    for chunk in pcm.chunks(max_floats) {
                        pm.pcm_add_float(chunk, 2);
                    }
                }
                Command::Load { path, smooth } => {
                    // The callback fires during this call if the load fails.
                    load_failed.set(false);
                    load_preset(&pm, &path, smooth);
                    if load_failed.get() {
                        let _ = events.send(Event::LoadFailed { path });
                    }
                }
                Command::Render(size) => render = Some(size),
                Command::Recycle(buffer) => spare.push(buffer),
            }
        }
        if let Some(size) = render {
            render_frame(&pm, &mut gl, size, &slot, &mut spare);
        }
    }
    // projectM must be destroyed while its context is still alive, and
    // before the flag its callback points at.
    drop(pm);
    drop(load_failed);
}

/// Loads a preset file through the raw API.
///
/// The `projectm` crate's `load_preset_file` passes the Rust string without
/// a terminating NUL, so projectM reads past the end of the path and loads
/// fail at random (it saw extensions like ".milk\x01"). A `CString` fixes it,
/// and also handles paths that aren't valid UTF-8.
fn load_preset(pm: &ProjectM, path: &Path, smooth: bool) {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
        log::warn!("skipping preset path with a NUL byte: {}", path.display());
        return;
    };
    let handle = *pm.get_instance().borrow();
    // SAFETY: `handle` is live, and `c_path` is NUL-terminated and outlives the call.
    unsafe { projectm_sys::projectm_load_preset_file(handle, c_path.as_ptr(), smooth) };
}

/// Registers projectM's "preset failed to load" callback through the raw API,
/// returning the flag it sets.
///
/// The callback deliberately ignores the filename and message projectM passes:
/// in projectM 4.1.3 the filename pointer is often invalid by the time it's
/// called (we saw garbage bytes), and the message is always just
/// "std::exception". Reading them crashed the `projectm` crate's own wrapper,
/// which also panics inside the C callback and so aborts the process. The
/// worker knows which preset it was loading, so a flag is all it needs.
fn install_failure_hook(pm: &ProjectM) -> Box<Cell<bool>> {
    let flag = Box::new(Cell::new(false));
    let handle = *pm.get_instance().borrow();
    let user_data = std::ptr::from_ref::<Cell<bool>>(&flag)
        .cast_mut()
        .cast::<c_void>();
    // SAFETY: `handle` is a live projectM instance; `user_data` points at the
    // boxed flag, which the caller keeps alive until projectM is destroyed.
    // projectM only calls back from inside our own calls on this thread.
    unsafe {
        projectm_sys::projectm_set_preset_switch_failed_event_callback(
            handle,
            Some(on_load_failed),
            user_data,
        );
    }
    flag
}

/// Called by projectM (from C++) when a preset fails to load. Never unwinds,
/// and never reads the (unreliable) string arguments.
unsafe extern "C" fn on_load_failed(
    _preset_filename: *const c_char,
    _message: *const c_char,
    user_data: *mut c_void,
) {
    let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
        if user_data.is_null() {
            return;
        }
        // SAFETY: `user_data` is the boxed flag from `install_failure_hook`,
        // used only on the worker thread that's calling into projectM.
        unsafe { &*user_data.cast::<Cell<bool>>() }.set(true);
    }));
}

fn render_frame(
    pm: &ProjectM,
    gl: &mut HeadlessGl,
    size: (u32, u32),
    slot: &Mutex<Option<Frame>>,
    spare: &mut Vec<Vec<u8>>,
) {
    if let Err(e) = gl.resize(size) {
        log::warn!("MilkDrop: {e:#}");
        return;
    }
    let size = gl.size();
    pm.set_window_size(size.0 as usize, size.1 as usize);
    pm.render_frame();
    let mut pixels = spare.pop().unwrap_or_default();
    if !gl.read_previous_frame(&mut pixels) {
        spare.push(pixels);
        return;
    }
    let Ok(mut slot) = slot.lock() else {
        return;
    };
    // Replace any frame the main thread hasn't collected; reuse its buffer.
    if let Some(stale) = slot.replace(Frame { pixels, size }) {
        spare.push(stale.pixels);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_callback_sets_the_flag_without_reading_the_strings() {
        let flag = Box::new(Cell::new(false));
        let user_data = std::ptr::from_ref::<Cell<bool>>(&flag)
            .cast_mut()
            .cast::<c_void>();
        // Dangling, non-NUL-terminated "strings", like projectM sometimes passes.
        let garbage = [0xFFu8, 0xFE, 0x41];
        let ptr = garbage.as_ptr().cast::<c_char>();
        unsafe { on_load_failed(ptr, ptr, user_data) };
        assert!(flag.get());
    }

    #[test]
    fn failure_callback_ignores_null_user_data() {
        unsafe { on_load_failed(std::ptr::null(), std::ptr::null(), std::ptr::null_mut()) };
    }
}
