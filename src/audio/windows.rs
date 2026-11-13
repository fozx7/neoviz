//! Windows capture: WASAPI loopback on the default render device, via cpal.
//!
//! cpal opens an *output* device as an input stream in loopback mode. WASAPI
//! doesn't move a stream when the default device changes (e.g. headphones
//! plugged in); cpal reports that as an error, and we reopen on the new default.

use std::{
    sync::{
        Arc,
        mpsc::{self, RecvTimeoutError},
    },
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::{CaptureContext, RETRY_DELAY};

/// How often the supervisor checks for a stop request while the stream runs.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

pub fn run(ctx: Arc<CaptureContext>) {
    while !ctx.should_stop() {
        if let Err(err) = capture_until_stopped(&ctx) {
            log::warn!("audio capture: {err:#}; retrying in {RETRY_DELAY:?}");
            ctx.sleep(RETRY_DELAY);
        }
    }
}

/// Captures until a stop is requested (`Ok`) or the stream fails (`Err`).
fn capture_until_stopped(ctx: &Arc<CaptureContext>) -> Result<()> {
    let host = cpal::default_host();
    let device = find_device(&host, ctx.device())?;
    let name = device.to_string();

    // Output devices are captured in loopback mode; input devices (mic, line-in) directly.
    let config = if device.supports_output() {
        device.default_output_config()
    } else {
        device.default_input_config()
    }
    .with_context(|| format!("querying the format of {name}"))?;
    if config.sample_format() != cpal::SampleFormat::F32 {
        bail!(
            "{name} uses {:?} samples; only f32 is supported",
            config.sample_format()
        );
    }
    let channels = usize::from(config.channels());
    let sample_rate = config.sample_rate();

    let (err_tx, err_rx) = mpsc::channel();
    let cb_ctx = ctx.clone();
    let stream = device
        .build_input_stream::<f32, _, _>(
            config.config(),
            move |data: &[f32], _| cb_ctx.push_interleaved(data, channels),
            move |err| {
                let _ = err_tx.send(err);
            },
            None,
        )
        .with_context(|| format!("opening {name}"))?;
    stream.play().with_context(|| format!("starting {name}"))?;

    ctx.set_sample_rate(sample_rate);
    log::info!("capturing from {name} at {sample_rate} Hz, {channels} ch");

    loop {
        if ctx.should_stop() {
            return Ok(());
        }
        match err_rx.recv_timeout(POLL_INTERVAL) {
            Ok(err) if is_transient(err.kind()) => log::debug!("audio stream: {err}"),
            Ok(err) => return Err(anyhow!("{name}: {err}")),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => bail!("{name}: stream closed"),
        }
    }
}

/// Errors that don't stop the stream.
fn is_transient(kind: cpal::ErrorKind) -> bool {
    matches!(
        kind,
        cpal::ErrorKind::Xrun | cpal::ErrorKind::RealtimeDenied | cpal::ErrorKind::DeviceChanged
    )
}

/// The named device, or the default output device when `name` is `None`.
fn find_device(host: &cpal::Host, name: Option<&str>) -> Result<cpal::Device> {
    let Some(name) = name else {
        return host
            .default_output_device()
            .context("no default output device");
    };
    host.devices()
        .context("listing audio devices")?
        .find(|d| d.to_string() == name)
        .with_context(|| format!("no audio device named {name:?}"))
}
