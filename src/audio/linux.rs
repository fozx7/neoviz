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

use super::{CHANNELS, CaptureContext, DeviceInfo, RETRY_DELAY};

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
            ctx.set_error(format!("{err:#}"));
            ctx.sleep(RETRY_DELAY);
        }
    }
}

/// Captures until a stop is requested (`Ok`) or the stream fails (`Err`).
fn capture_until_stopped(ctx: &CaptureContext) -> Result<()> {
    let device = ctx.device().unwrap_or(DEFAULT_MONITOR);
    // PipeWire silently falls back to the default source (usually the mic)
    // for an unknown name, so check it exists. If listing fails, try anyway.
    if let Some(name) = ctx.device()
        && let Ok(devices) = list_devices()
        && !devices.iter().any(|d| d.name == name)
    {
        return Err(anyhow!("no audio device named {name:?}"));
    }
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
    ctx.clear_error();
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

/// Longest we'll wait for PulseAudio to answer a device listing.
const LIST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Lists PulseAudio sources. Monitors of outputs are marked as loopback.
pub fn list_devices() -> Result<Vec<DeviceInfo>> {
    use std::{cell::RefCell, rc::Rc, time::Instant};

    use libpulse_binding::{
        callbacks::ListResult,
        context::{Context, FlagSet, State},
        mainloop::standard::Mainloop,
        operation::State as OpState,
    };

    let mut mainloop = Mainloop::new().ok_or_else(|| anyhow!("creating PulseAudio mainloop"))?;
    let mut context =
        Context::new(&mainloop, "neoViz").ok_or_else(|| anyhow!("creating PulseAudio context"))?;
    context
        .connect(None, FlagSet::NOAUTOSPAWN, None)
        .map_err(|e| anyhow!("connecting to PulseAudio: {e}"))?;

    let deadline = Instant::now() + LIST_TIMEOUT;
    let pump = |mainloop: &mut Mainloop| -> Result<()> {
        if Instant::now() > deadline {
            return Err(anyhow!("PulseAudio didn't answer in time"));
        }
        mainloop.iterate(false);
        std::thread::sleep(std::time::Duration::from_millis(2));
        Ok(())
    };

    loop {
        match context.get_state() {
            State::Ready => break,
            State::Failed | State::Terminated => {
                return Err(anyhow!("PulseAudio connection failed"));
            }
            _ => pump(&mut mainloop)?,
        }
    }

    let found = Rc::new(RefCell::new(Vec::new()));
    let sink = found.clone();
    let op = context.introspect().get_source_info_list(move |result| {
        if let ListResult::Item(info) = result {
            let name = info.name.as_deref().unwrap_or_default().to_owned();
            let description = info.description.as_deref().unwrap_or(&name).to_owned();
            sink.borrow_mut().push(DeviceInfo {
                is_loopback: info.monitor_of_sink.is_some(),
                name,
                description,
            });
        }
    });
    while op.get_state() == OpState::Running {
        pump(&mut mainloop)?;
    }
    context.disconnect();
    Ok(found.take())
}
