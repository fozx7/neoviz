//! System audio capture.
//!
//! Each OS backend runs on its own thread, captures loopback audio, and pushes
//! interleaved stereo `f32` samples into a lock-free ring buffer. The render
//! thread drains that buffer each frame via [`Capture::consumer`].
//!
//! Backends are selected with `#[cfg]` and expose a single
//! `run(Arc<CaptureContext>)` function that captures until asked to stop,
//! reconnecting on errors (device unplugged, default device changed, server
//! restarted). A macOS backend can be added the same way later.

pub mod analysis;
pub mod bands;
mod beat;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};

/// Samples are always delivered as interleaved stereo (L, R, L, R, ...).
pub const CHANNELS: usize = 2;
/// Ring buffer capacity in samples: ~1.3 s of stereo audio at 48 kHz.
const RING_CAPACITY: usize = 1 << 17;
/// Assumed until the backend reports the real rate.
const DEFAULT_SAMPLE_RATE: u32 = 48_000;
/// How long a backend waits before reconnecting after an error.
pub(crate) const RETRY_DELAY: Duration = Duration::from_secs(1);

/// State shared between the capture thread and the app.
pub struct CaptureContext {
    /// Device to capture from; `None` means the default output (loopback).
    device: Option<String>,
    stop: AtomicBool,
    sample_rate: AtomicU32,
    /// Only the capture callback locks this, and only one stream is live at a
    /// time, so `try_lock` never contends in practice.
    producer: Mutex<rtrb::Producer<f32>>,
    /// Why capture isn't working right now, for showing on screen.
    error: Mutex<Option<String>>,
}

/// A device that can be captured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// What `--device` / the settings file expect.
    pub name: String,
    /// Human-readable label.
    pub description: String,
    /// True for an output captured in loopback (vs a mic or line-in).
    pub is_loopback: bool,
}

/// Devices available for capture, loopback outputs first.
pub fn list_devices() -> Result<Vec<DeviceInfo>> {
    let mut devices = backend::list_devices()?;
    devices.sort_by_key(|d| !d.is_loopback);
    Ok(devices)
}

impl CaptureContext {
    pub fn device(&self) -> Option<&str> {
        self.device.as_deref()
    }

    pub fn should_stop(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    pub fn set_sample_rate(&self, rate: u32) {
        self.sample_rate.store(rate, Ordering::Relaxed);
    }

    /// Converts `channels`-wide interleaved audio to stereo and queues it.
    /// Drops samples if the ring buffer is full (e.g. the window is minimised).
    pub fn push_interleaved(&self, data: &[f32], channels: usize) {
        let Ok(mut producer) = self.producer.try_lock() else {
            return;
        };
        push_as_stereo(&mut producer, data, channels);
    }

    /// Records why capture failed (shown on screen until it recovers).
    pub fn set_error(&self, message: String) {
        if let Ok(mut e) = self.error.lock() {
            *e = Some(message);
        }
    }

    pub fn clear_error(&self) {
        if let Ok(mut e) = self.error.lock() {
            *e = None;
        }
    }

    /// Sleeps for `duration`, waking early if a stop is requested.
    pub fn sleep(&self, duration: Duration) {
        let deadline = Instant::now() + duration;
        while !self.should_stop() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// A running capture. Stops and joins the capture thread on drop.
pub struct Capture {
    pub consumer: rtrb::Consumer<f32>,
    ctx: Arc<CaptureContext>,
    thread: Option<JoinHandle<()>>,
}

impl Capture {
    /// Starts capturing from `device`, or the default output's loopback if `None`.
    pub fn start(device: Option<String>) -> Result<Self> {
        let (producer, consumer) = rtrb::RingBuffer::new(RING_CAPACITY);
        let ctx = Arc::new(CaptureContext {
            device,
            stop: AtomicBool::new(false),
            sample_rate: AtomicU32::new(DEFAULT_SAMPLE_RATE),
            producer: Mutex::new(producer),
            error: Mutex::new(None),
        });
        let thread_ctx = ctx.clone();
        let thread = std::thread::Builder::new()
            .name("audio-capture".into())
            .spawn(move || backend::run(thread_ctx))
            .context("spawning audio capture thread")?;
        Ok(Self {
            consumer,
            ctx,
            thread: Some(thread),
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.ctx.sample_rate.load(Ordering::Relaxed)
    }

    /// Why capture currently isn't working, if it isn't.
    pub fn error(&self) -> Option<String> {
        self.ctx.error.lock().ok().and_then(|e| e.clone())
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.ctx.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            log::error!("audio capture thread panicked");
        }
    }
}

#[cfg(target_os = "linux")]
use linux as backend;
#[cfg(windows)]
use windows as backend;

#[cfg(not(any(target_os = "linux", windows)))]
mod backend {
    pub fn run(ctx: std::sync::Arc<super::CaptureContext>) {
        ctx.set_error("audio capture is not implemented on this OS yet".into());
    }

    pub fn list_devices() -> anyhow::Result<Vec<super::DeviceInfo>> {
        Ok(Vec::new())
    }
}

/// Writes interleaved `channels`-wide frames to `producer` as stereo:
/// mono is duplicated to both sides, extra channels beyond L/R are dropped.
fn push_as_stereo(producer: &mut rtrb::Producer<f32>, data: &[f32], channels: usize) {
    if channels == 0 {
        return;
    }
    for frame in data.chunks_exact(channels) {
        if producer.slots() < CHANNELS {
            return;
        }
        let (l, r) = match frame {
            [mono] => (*mono, *mono),
            [l, r, ..] => (*l, *r),
            [] => unreachable!("chunks_exact yields non-empty chunks"),
        };
        // Both pushes succeed: we checked for two free slots above.
        let _ = producer.push(l);
        let _ = producer.push(r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(consumer: &mut rtrb::Consumer<f32>) -> Vec<f32> {
        std::iter::from_fn(|| consumer.pop().ok()).collect()
    }

    #[test]
    fn stereo_passes_through() {
        let (mut p, mut c) = rtrb::RingBuffer::new(16);
        push_as_stereo(&mut p, &[1.0, 2.0, 3.0, 4.0], 2);
        assert_eq!(drain(&mut c), [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn mono_is_duplicated() {
        let (mut p, mut c) = rtrb::RingBuffer::new(16);
        push_as_stereo(&mut p, &[0.5, -0.5], 1);
        assert_eq!(drain(&mut c), [0.5, 0.5, -0.5, -0.5]);
    }

    #[test]
    fn surround_keeps_front_left_and_right() {
        let (mut p, mut c) = rtrb::RingBuffer::new(16);
        // Two 6-channel frames.
        push_as_stereo(
            &mut p,
            &[1.0, 2.0, 9.0, 9.0, 9.0, 9.0, 3.0, 4.0, 9.0, 9.0, 9.0, 9.0],
            6,
        );
        assert_eq!(drain(&mut c), [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn full_buffer_drops_whole_frames() {
        let (mut p, mut c) = rtrb::RingBuffer::new(3);
        push_as_stereo(&mut p, &[1.0, 2.0, 3.0, 4.0], 2);
        // Only one stereo frame fits; never a lone left sample.
        assert_eq!(drain(&mut c), [1.0, 2.0]);
    }

    #[test]
    fn zero_channels_is_ignored() {
        let (mut p, mut c) = rtrb::RingBuffer::new(4);
        push_as_stereo(&mut p, &[1.0], 0);
        assert!(drain(&mut c).is_empty());
    }
}
