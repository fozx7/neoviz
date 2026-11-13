//! App state and the winit event loop handler.

use std::{sync::Arc, time::Instant};

use anyhow::{Context, Result, anyhow};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition},
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::ActiveEventLoop,
    monitor::MonitorHandle,
    window::{Fullscreen, Window, WindowId},
};

use crate::{
    Args,
    audio::{Capture, analysis::Analyzer},
    input::{Action, DoubleClickDetector, action_for_key},
    render::{debug_view::DebugView, gpu::Gpu},
};

const WINDOW_TITLE: &str = "neoViz";
const DEFAULT_WINDOW_SIZE: LogicalSize<f64> = LogicalSize::new(1280.0, 720.0);
/// Offset from the chosen monitor's top-left corner when opening windowed.
const WINDOW_MONITOR_OFFSET: i32 = 64;

pub struct App {
    args: Args,
    running: Option<Running>,
    double_click: DoubleClickDetector,
    error: Option<anyhow::Error>,
}

struct Running {
    window: Arc<Window>,
    gpu: Gpu,
    capture: Capture,
    analyzer: Analyzer,
    debug_view: DebugView,
}

impl Running {
    fn redraw(&mut self) {
        self.analyzer.ingest(&mut self.capture.consumer);
        self.analyzer.analyze();
        self.debug_view.update(
            self.gpu.queue(),
            &self.analyzer,
            self.capture.sample_rate(),
            self.gpu.size(),
        );
        let debug_view = &self.debug_view;
        self.gpu
            .render(|encoder, target| debug_view.draw(encoder, target));
        self.window.request_redraw();
    }
}

impl App {
    pub fn new(args: Args) -> Self {
        Self {
            args,
            running: None,
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

    fn start(&self, event_loop: &ActiveEventLoop) -> Result<Running> {
        let monitor = pick_monitor(event_loop, self.args.monitor)?;
        let mut attrs = Window::default_attributes()
            .with_title(WINDOW_TITLE)
            .with_inner_size(DEFAULT_WINDOW_SIZE);
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
        let debug_view = DebugView::new(gpu.device(), gpu.surface_format());
        let capture = Capture::start(self.args.device.clone())?;
        window.request_redraw();
        Ok(Running {
            window,
            gpu,
            capture,
            analyzer: Analyzer::default(),
            debug_view,
        })
    }

    fn apply(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        let Some(running) = &self.running else { return };
        let window = &running.window;
        match action {
            Action::ToggleFullscreen => {
                let next = match window.fullscreen() {
                    Some(_) => None,
                    None => Some(Fullscreen::Borderless(window.current_monitor())),
                };
                window.set_fullscreen(next);
            }
            Action::Escape if window.fullscreen().is_some() => window.set_fullscreen(None),
            Action::Escape => event_loop.exit(),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.running.is_some() {
            return;
        }
        match self.start(event_loop) {
            Ok(running) => self.running = Some(running),
            Err(err) => self.fail(event_loop, err),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = &mut self.running {
                    r.gpu.resize(size);
                }
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && !event.repeat =>
            {
                if let Some(action) = action_for_key(&event.logical_key) {
                    self.apply(action, event_loop);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if self.double_click.click(Instant::now()) {
                    self.apply(Action::ToggleFullscreen, event_loop);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(r) = &mut self.running {
                    r.redraw();
                }
            }
            _ => {}
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
