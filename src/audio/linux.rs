//! Linux capture: records the monitor source of the default sink through
//! PulseAudio. Works on PipeWire systems via `pipewire-pulse`.

use std::sync::Arc;

use anyhow::{Result, anyhow};
use libpulse_binding::{
    def::BufferAttr,
    sample::{Format, Spec},
    stream::Direction,
};
use libpulse_simple_binding::Simple;

use super::{CHANNELS, CaptureContext, RETRY_DELAY};

/// Pulse's alias for "the monitor of whatever the default output is".
const DEFAULT_MONITOR: &str = "@DEFAULT_MONITOR@";
const SAMPLE_RATE: u32 = 48_000;
/// Frames per read: ~10 ms at 48 kHz, which bounds capture latency.
const FRAMES_PER_READ: usize = 512;
const BYTES_PER_SAMPLE: usize = size_of::<f32>();

pub fn run(ctx: Arc<CaptureContext>) {
    while !ctx.should_stop() {
        if let Err(err) = capture_until_stopped(&ctx) {
            log::warn!("audio capture: {err:#}; retrying in {RETRY_DELAY:?}");
            ctx.sleep(RETRY_DELAY);
        }
    }
}

/// Captures until a stop is requested (`Ok`) or the stream fails (`Err`).
fn capture_until_stopped(ctx: &CaptureContext) -> Result<()> {
    let device = ctx.device().unwrap_or(DEFAULT_MONITOR);
    let spec = Spec {
        format: Format::FLOAT32NE,
        rate: SAMPLE_RATE,
        channels: CHANNELS as u8,
    };
    let read_bytes = FRAMES_PER_READ * CHANNELS * BYTES_PER_SAMPLE;
    // u32::MAX = "server default" for the playback-only fields. `fragsize` is what
    // matters for recording: it's how much the server batches before sending.
    let attr = BufferAttr {
        maxlength: u32::MAX,
        tlength: u32::MAX,
        prebuf: u32::MAX,
        minreq: u32::MAX,
        fragsize: read_bytes as u32,
    };
    let stream = Simple::new(
        None,
        "neoViz",
        Direction::Record,
        Some(device),
        "visualizer input",
        &spec,
        None,
        Some(&attr),
    )
    .map_err(|e| anyhow!("connecting to PulseAudio source {device}: {e}"))?;

    ctx.set_sample_rate(SAMPLE_RATE);
    log::info!("capturing from PulseAudio source {device} at {SAMPLE_RATE} Hz");

    let mut bytes = vec![0u8; read_bytes];
    let mut samples = vec![0f32; FRAMES_PER_READ * CHANNELS];
    while !ctx.should_stop() {
        stream
            .read(&mut bytes)
            .map_err(|e| anyhow!("reading from PulseAudio source {device}: {e}"))?;
        let (raw_samples, _) = bytes.as_chunks::<BYTES_PER_SAMPLE>();
        for (sample, raw) in samples.iter_mut().zip(raw_samples) {
            *sample = f32::from_ne_bytes(*raw);
        }
        ctx.push_interleaved(&samples, CHANNELS);
    }
    Ok(())
}
