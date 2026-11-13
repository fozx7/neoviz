// Release builds on Windows are GUI apps: no console window alongside.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod audio;
mod config;
mod input;
mod render;
mod ui;
mod viz;

use clap::Parser;
use winit::event_loop::{ControlFlow, EventLoop};

/// Retro-style visualizer for whatever audio your computer is playing.
#[derive(Debug, Clone, Parser)]
#[command(version, about)]
pub struct Args {
    /// Start in full screen.
    #[arg(long)]
    pub fullscreen: bool,

    /// Start spread across every monitor (span mode; also double-tap F).
    #[arg(long)]
    pub span: bool,

    /// Monitor to open on (0-based index). Defaults to the primary monitor.
    #[arg(long, value_name = "N")]
    pub monitor: Option<usize>,

    /// Audio device to capture instead of the default output's loopback.
    /// Linux: a PulseAudio source name (see `pactl list short sources`).
    /// Windows: the device name as shown in Sound settings.
    #[arg(long, value_name = "NAME")]
    pub device: Option<String>,

    /// Visualizer to start with (case-insensitive name, e.g. "ring scope").
    #[arg(long, value_name = "NAME")]
    pub visualizer: Option<String>,

    /// Folder of MilkDrop `.milk` presets (searched recursively; remembered).
    #[arg(long, value_name = "DIR")]
    pub presets: Option<std::path::PathBuf>,

    /// Print the audio devices that can be captured, then exit.
    #[arg(long)]
    pub list_devices: bool,
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Args::parse();
    if args.list_devices {
        return print_devices();
    }

    let event_loop = EventLoop::new()?;
    // Rendering is paced by vsync (FIFO present mode), so poll continuously.
    event_loop.set_control_flow(ControlFlow::Poll);

    let config_path = config::default_path();
    let settings = config_path.as_deref().map(config::load).unwrap_or_default();
    if let Some(path) = &config_path {
        log::info!("settings: {}", path.display());
    }

    let mut app = app::App::new(args, settings, config_path);
    event_loop.run_app(&mut app)?;
    app.exit_result()
}

fn print_devices() -> anyhow::Result<()> {
    for d in audio::list_devices()? {
        let kind = if d.is_loopback { "output" } else { "input " };
        println!("{kind}  {}\n        --device \"{}\"", d.description, d.name);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Args;
    use clap::{CommandFactory, Parser};

    #[test]
    fn cli_definition_is_valid() {
        Args::command().debug_assert();
    }

    #[test]
    fn parses_fullscreen_and_monitor() {
        let args = Args::try_parse_from(["neoviz", "--fullscreen", "--monitor", "1"]).unwrap();
        assert!(args.fullscreen);
        assert_eq!(args.monitor, Some(1));
    }

    #[test]
    fn defaults_to_windowed_on_primary_monitor() {
        let args = Args::try_parse_from(["neoviz"]).unwrap();
        assert!(!args.fullscreen);
        assert_eq!(args.monitor, None);
    }
}
