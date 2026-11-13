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
    input::{Action, DoubleClickDetector, action_for_key},
    render::gpu::Gpu,
};

const WINDOW_TITLE: &str = "neoViz";
const DEFAULT_WINDOW_SIZE: LogicalSize<f64> = LogicalSize::new(1280.0, 720.0);
/// Offset from the chosen monitor's top-left corner when opening windowed.
const WINDOW_MONITOR_OFFSET: i32 = 64;
/// Seconds for one full trip around the colour wheel in the placeholder background.
const HUE_CYCLE_SECS: f64 = 30.0;

pub struct App {
    args: Args,
    running: Option<Running>,
    double_click: DoubleClickDetector,
    error: Option<anyhow::Error>,
}

struct Running {
    window: Arc<Window>,
    gpu: Gpu,
    started: Instant,
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
        window.request_redraw();
        Ok(Running {
            window,
            gpu,
            started: Instant::now(),
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
                    let t = r.started.elapsed().as_secs_f64();
                    r.gpu.render_clear(placeholder_color(t));
                    r.window.request_redraw();
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

/// A dim, slowly hue-cycling background so it's obvious the render loop is alive.
/// Stands in for the visualizers until they exist.
fn placeholder_color(t_secs: f64) -> wgpu::Color {
    let hue = (t_secs / HUE_CYCLE_SECS).fract();
    let (r, g, b) = hsv_to_rgb(hue, 0.6, 0.25);
    wgpu::Color { r, g, b, a: 1.0 }
}

/// HSV (all components 0..=1) to linear RGB.
fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (f64, f64, f64) {
    let sector = (h.rem_euclid(1.0) * 6.0).floor();
    let f = h.rem_euclid(1.0) * 6.0 - sector;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match sector as u8 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: (f64, f64, f64), b: (f64, f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9 && (a.2 - b.2).abs() < 1e-9
    }

    #[test]
    fn hsv_primaries() {
        assert!(approx(hsv_to_rgb(0.0, 1.0, 1.0), (1.0, 0.0, 0.0)));
        assert!(approx(hsv_to_rgb(1.0 / 3.0, 1.0, 1.0), (0.0, 1.0, 0.0)));
        assert!(approx(hsv_to_rgb(2.0 / 3.0, 1.0, 1.0), (0.0, 0.0, 1.0)));
    }

    #[test]
    fn hsv_zero_saturation_is_grey() {
        assert!(approx(hsv_to_rgb(0.42, 0.0, 0.5), (0.5, 0.5, 0.5)));
    }

    #[test]
    fn placeholder_color_wraps_each_cycle() {
        let a = placeholder_color(3.0);
        let b = placeholder_color(3.0 + HUE_CYCLE_SECS);
        assert!(approx((a.r, a.g, a.b), (b.r, b.g, b.b)));
        assert_eq!(a.a, 1.0);
    }
}
