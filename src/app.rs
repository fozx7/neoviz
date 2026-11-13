//! App state and the winit event loop handler.

use std::{sync::Arc, time::Instant};

use anyhow::{Context, Result, anyhow, bail};
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
    audio::{
        Capture,
        analysis::{Analyzer, GAIN_STEP_DB},
    },
    input::{Action, DoubleClickDetector, action_for_key},
    render::{
        compositor::{Compositor, INCOMING, OUTGOING},
        debug_view::DebugView,
        gpu::Gpu,
    },
    viz::{self, Switcher, Visualizer},
};

const WINDOW_TITLE: &str = "neoViz";
const DEFAULT_WINDOW_SIZE: LogicalSize<f64> = LogicalSize::new(1280.0, 720.0);
/// Offset from the chosen monitor's top-left corner when opening windowed.
const WINDOW_MONITOR_OFFSET: i32 = 64;
/// Longest frame time fed to the analysis, in seconds.
const MAX_FRAME_DT: f32 = 0.1;

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
    visualizers: Vec<Box<dyn Visualizer>>,
    switcher: Switcher,
    compositor: Compositor,
    debug_view: DebugView,
    show_debug: bool,
    last_frame: Instant,
}

impl Running {
    fn redraw(&mut self) {
        let now = Instant::now();
        // Clamp so a stall (e.g. dragging the window) doesn't cause a huge jump.
        let dt = (now - self.last_frame).as_secs_f32().min(MAX_FRAME_DT);
        self.last_frame = now;

        let sample_rate = self.capture.sample_rate();
        self.analyzer.ingest(&mut self.capture.consumer);
        self.analyzer.analyze(sample_rate, dt);
        let audio = self.analyzer.frame();
        let size = self.gpu.size();

        if self.show_debug {
            self.debug_view
                .update(self.gpu.queue(), &audio, sample_rate, size, dt);
            let debug_view = &self.debug_view;
            self.gpu
                .render(|_, encoder, target| debug_view.draw(encoder, target));
        } else {
            self.switcher.advance(dt);
            let current = self.switcher.current();
            let outgoing = self.switcher.outgoing();
            for index in std::iter::once(current).chain(outgoing) {
                self.visualizers[index].update(&audio, dt);
            }

            let (visualizers, compositor, mix) =
                (&mut self.visualizers, &self.compositor, self.switcher.mix());
            self.gpu.render(|queue, encoder, target| {
                if let Some(index) = outgoing {
                    visualizers[index].render(queue, encoder, compositor.slot(OUTGOING), size);
                }
                visualizers[current].render(queue, encoder, compositor.slot(INCOMING), size);
                compositor.draw(queue, encoder, target, mix);
            });
        }
        self.window.request_redraw();
    }

    fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        self.gpu.resize(size);
        let size = self.gpu.size();
        self.compositor.resize(self.gpu.device(), size);
        for v in &mut self.visualizers {
            v.resize(self.gpu.device(), size);
        }
    }

    fn update_title(&self) {
        let name = self.visualizers[self.switcher.current()].name();
        let mut title = format!("{WINDOW_TITLE} · {name}");
        let gain = self.analyzer.settings.gain_db;
        // Until the on-screen overlay exists, the title bar shows this.
        if gain != 0.0 {
            title.push_str(&format!(" · sensitivity {gain:+} dB"));
        }
        self.window.set_title(&title);
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
        let size = gpu.size();
        let visualizers = viz::create_all(gpu.device(), size);
        let names: Vec<&str> = visualizers.iter().map(|v| v.name()).collect();
        let start = match &self.args.visualizer {
            Some(wanted) => find_by_name(&names, wanted)?,
            None => 0,
        };
        let running = Running {
            switcher: Switcher::new(visualizers.len(), start),
            compositor: Compositor::new(gpu.device(), gpu.surface_format(), size),
            debug_view: DebugView::new(gpu.device(), gpu.surface_format()),
            capture: Capture::start(self.args.device.clone())?,
            analyzer: Analyzer::default(),
            visualizers,
            show_debug: false,
            last_frame: Instant::now(),
            window,
            gpu,
        };
        running.update_title();
        running.window.request_redraw();
        Ok(running)
    }

    fn apply(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        let Some(running) = &mut self.running else {
            return;
        };
        let window = &running.window;
        match action {
            Action::SensitivityUp | Action::SensitivityDown => {
                let step = if action == Action::SensitivityUp {
                    GAIN_STEP_DB
                } else {
                    -GAIN_STEP_DB
                };
                let gain = running.analyzer.adjust_gain(step);
                log::info!("sensitivity {gain:+} dB");
                running.update_title();
            }
            Action::NextVisualizer | Action::PreviousVisualizer => {
                if action == Action::NextVisualizer {
                    running.switcher.next();
                } else {
                    running.switcher.previous();
                }
                running.show_debug = false;
                running.update_title();
            }
            Action::ToggleDebugView => running.show_debug = !running.show_debug,
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
                    r.resize(size);
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
