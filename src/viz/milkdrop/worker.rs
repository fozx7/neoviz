//! Runs projectM on its own thread, so slow preset loads (some take a third
//! of a second to compile their shaders) never freeze the app or delay input.
//!
//! The main thread sends commands; the worker handles everything queued,
//! renders at most once per batch, and leaves the newest frame in a shared
//! slot for the main thread to pick up.

use std::{
    path::PathBuf,
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
    LoadFailed { path: PathBuf, reason: String },
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
        })
    }

    pub fn send(&self, command: Command) {
        if let Some(tx) = &self.commands
            && tx.send(command).is_err()
        {
            log::warn!("MilkDrop thread has stopped");
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
    // projectM needs its GL context current (on this thread) when created.
    let pm = ProjectM::create();
    pm.set_window_size(size.0 as usize, size.1 as usize);
    pm.set_fps(RENDER_FPS);
    // We choose presets ourselves; stop projectM switching on its own.
    pm.set_preset_locked(true);
    pm.set_hard_cut_enabled(false);
    pm.set_preset_switch_failed_event_callback(move |path, reason| {
        let _ = events.send(Event::LoadFailed {
            path: path.into(),
            reason,
        });
    });
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
                    pm.load_preset_file(&path.to_string_lossy(), smooth);
                }
                Command::Render(size) => render = Some(size),
                Command::Recycle(buffer) => spare.push(buffer),
            }
        }
        if let Some(size) = render {
            render_frame(&pm, &mut gl, size, &slot, &mut spare);
        }
    }
    // projectM must be destroyed while its context is still alive.
    drop(pm);
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
