mod app;
mod input;
mod render;

use clap::Parser;
use winit::event_loop::{ControlFlow, EventLoop};

/// Retro-style visualizer for whatever audio your computer is playing.
#[derive(Debug, Clone, Parser)]
#[command(version, about)]
pub struct Args {
    /// Start in full screen.
    #[arg(long)]
    pub fullscreen: bool,

    /// Monitor to open on (0-based index). Defaults to the primary monitor.
    #[arg(long, value_name = "N")]
    pub monitor: Option<usize>,
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Args::parse();

    let event_loop = EventLoop::new()?;
    // Rendering is paced by vsync (FIFO present mode), so poll continuously.
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = app::App::new(args);
    event_loop.run_app(&mut app)?;
    app.exit_result()
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
