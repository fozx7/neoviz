//! The winit event loop handler: creates the window and routes events to the
//! [`Session`].

mod session;

use std::{path::PathBuf, sync::Arc, time::Instant};

use anyhow::{Context, Result, anyhow, bail};
use session::{Outcome, Session};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition},
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::ActiveEventLoop,
    monitor::MonitorHandle,
    window::{Fullscreen, Window, WindowId, WindowLevel},
};

use crate::{
    Args,
    config::Settings,
    input::{Action, DoubleClickDetector, action_for_key},
    render::gpu::Gpu,
    viz,
};

const DEFAULT_WINDOW_SIZE: LogicalSize<f64> = LogicalSize::new(1280.0, 720.0);
/// Offset from the chosen monitor's top-left corner when opening windowed.
const WINDOW_MONITOR_OFFSET: i32 = 64;

pub struct App {
    args: Args,
    /// Settings to start with (from disk, with command-line overrides applied).
    settings: Settings,
    /// What's on disk now, so unchanged settings aren't rewritten.
    saved: Settings,
    config_path: Option<PathBuf>,
    session: Option<Session>,
    double_click: DoubleClickDetector,
    error: Option<anyhow::Error>,
}

impl App {
    pub fn new(args: Args, saved: Settings, config_path: Option<PathBuf>) -> Self {
        let mut settings = saved.clone();
        // Command-line choices act like picking them in the settings panel.
        if args.device.is_some() {
            settings.device = args.device.clone();
        }
        Self {
            args,
            settings,
            saved,
            config_path,
            session: None,
            double_click: DoubleClickDetector::default(),
            error: None,
        }
    }

    /// The error that stopped the event loop, if any.
    pub fn exit_result(self) -> Result<()> {
        self.error.map_or(Ok(()), Err)
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, err: anyhow::Error) {
        log::error!("{err:#}");
        self.error = Some(err);
        event_loop.exit();
    }

    fn start(&self, event_loop: &ActiveEventLoop) -> Result<Session> {
        let monitor = pick_monitor(event_loop, self.args.monitor)?;
        let mut attrs = Window::default_attributes()
            .with_title("neoViz")
            .with_inner_size(DEFAULT_WINDOW_SIZE)
            .with_decorations(!self.settings.borderless);
        if self.settings.always_on_top {
            attrs = attrs.with_window_level(WindowLevel::AlwaysOnTop);
        }
        if let Some(m) = &monitor {
            let p = m.position();
            attrs = attrs.with_position(PhysicalPosition::new(
                p.x + WINDOW_MONITOR_OFFSET,
                p.y + WINDOW_MONITOR_OFFSET,
            ));
        }
        if self.args.fullscreen {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(monitor)));
        }

        let window = Arc::new(event_loop.create_window(attrs).context("creating window")?);
        let gpu = pollster::block_on(Gpu::new(window.clone()))?;
        let visualizers = viz::create_all(gpu.device(), gpu.size());
        let names: Vec<&str> = visualizers.iter().map(|v| v.name()).collect();
        let start = match (&self.args.visualizer, &self.settings.visualizer) {
            // An explicit --visualizer must exist; a remembered one may have been renamed.
            (Some(wanted), _) => find_by_name(&names, wanted)?,
            (None, Some(last)) => find_by_name(&names, last).unwrap_or(0),
            (None, None) => 0,
        };
        Session::new(
            window,
            gpu,
            visualizers,
            start,
            self.settings.clone(),
            self.saved.clone(),
            self.config_path.clone(),
        )
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.session.is_some() {
            return;
        }
        match self.start(event_loop) {
            Ok(session) => self.session = Some(session),
            Err(err) => self.fail(event_loop, err),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(session) = &mut self.session else {
            return;
        };
        let ui_used = session.ui_event(&event);

        let action = match &event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return;
            }
            WindowEvent::Resized(size) => {
                session.resize(*size);
                return;
            }
            WindowEvent::RedrawRequested => {
                session.redraw();
                return;
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && !event.repeat =>
            {
                action_for_key(&event.logical_key)
                    // While typing in the UI, only the keys that close it still work.
                    .filter(|a| {
                        !session.ui_wants_keyboard()
                            || matches!(a, Action::ToggleSettings | Action::Escape)
                    })
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } if !ui_used => self
                .double_click
                .click(Instant::now())
                .then_some(Action::ToggleFullscreen),
            _ => None,
        };
        if let Some(action) = action
            && session.apply(action) == Outcome::Quit
        {
            event_loop.exit();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(session) = &mut self.session {
            session.save();
        }
    }
}

/// Resolves `--monitor N` to a monitor handle. `None` means "let the OS decide".
fn pick_monitor(
    event_loop: &ActiveEventLoop,
    index: Option<usize>,
) -> Result<Option<MonitorHandle>> {
    let Some(index) = index else {
        return Ok(event_loop.primary_monitor());
    };
    let monitors: Vec<_> = event_loop.available_monitors().collect();
    monitors.get(index).cloned().map(Some).ok_or_else(|| {
        anyhow!(
            "--monitor {index} is out of range: {} monitor(s) found (0-based)",
            monitors.len()
        )
    })
}

/// Index of the visualizer called `wanted` (case-insensitive).
fn find_by_name(names: &[&str], wanted: &str) -> Result<usize> {
    if let Some(i) = names.iter().position(|n| n.eq_ignore_ascii_case(wanted)) {
        return Ok(i);
    }
    bail!(
        "no visualizer named {wanted:?}; available: {}",
        names.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_visualizers_case_insensitively() {
        let names = ["Bars & Peaks", "Ring Scope"];
        assert_eq!(find_by_name(&names, "ring scope").unwrap(), 1);
    }

    #[test]
    fn unknown_visualizer_lists_the_options() {
        let err = find_by_name(&["Bars & Peaks", "Ring Scope"], "nope").unwrap_err();
        assert!(err.to_string().contains("Bars & Peaks, Ring Scope"));
    }
}
