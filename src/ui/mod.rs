//! On-screen UI via egui: the settings overlay and the heads-up text
//! (visualizer name, FPS, capture errors), drawn over the visualizers.

pub mod debug_hud;
pub mod fps_hud;
pub mod overlay;

use winit::window::Window;

use crate::render::common::begin_pass;

/// How long a toast (e.g. the visualizer name) stays on screen.
pub const TOAST_SECS: f32 = 2.0;
const TOAST_FADE_IN: f32 = 0.2;
const TOAST_FADE_OUT: f32 = 0.6;

/// A short message that fades in, holds, and fades out.
#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub text: String,
    age: f32,
}

impl Toast {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            age: 0.0,
        }
    }

    /// Advances by `dt`; returns false once the toast has finished.
    pub fn tick(&mut self, dt: f32) -> bool {
        self.age += dt;
        self.age < TOAST_SECS
    }

    /// Opacity now, 0..1.
    pub fn alpha(&self) -> f32 {
        let a = self.age;
        if a < TOAST_FADE_IN {
            a / TOAST_FADE_IN
        } else if a < TOAST_SECS - TOAST_FADE_OUT {
            1.0
        } else {
            ((TOAST_SECS - a) / TOAST_FADE_OUT).max(0.0)
        }
    }
}

/// Tessellated UI waiting to be painted.
struct Prepared {
    jobs: Vec<egui::ClippedPrimitive>,
    pixels_per_point: f32,
}

pub struct Ui {
    ctx: egui::Context,
    state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    prepared: Option<Prepared>,
    /// Texture changes not yet uploaded. Kept across frames that don't get
    /// painted (e.g. while minimised): egui requires every delta be applied.
    textures: egui::TexturesDelta,
}

impl Ui {
    pub fn new(window: &Window, device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let ctx = egui::Context::default();
        ctx.set_visuals(egui::Visuals::dark());
        let state = egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            None,
            Some(device.limits().max_texture_dimension_2d as usize),
        );
        let renderer = egui_wgpu::Renderer::new(device, format, Default::default());
        Self {
            ctx,
            state,
            renderer,
            prepared: None,
            textures: Default::default(),
        }
    }

    /// Feeds a window event to egui. Returns true if egui used it (so the app
    /// shouldn't also act on it, e.g. a click on a button).
    pub fn on_window_event(&mut self, window: &Window, event: &winit::event::WindowEvent) -> bool {
        self.state.on_window_event(window, event).consumed
    }

    /// True while a text field or similar has keyboard focus.
    pub fn wants_keyboard(&self) -> bool {
        self.ctx.egui_wants_keyboard_input()
    }

    /// Builds this frame's UI with `build`.
    pub fn run(&mut self, window: &Window, build: impl FnMut(&egui::Context)) {
        let input = self.state.take_egui_input(window);
        let output = self.ctx.run_pass(input, build);
        self.state
            .handle_platform_output(window, output.platform_output);
        let jobs = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        self.textures.append(output.textures_delta);
        self.prepared = Some(Prepared {
            jobs,
            pixels_per_point: output.pixels_per_point,
        });
    }

    /// Draws the UI built by [`Ui::run`] over `target`.
    pub fn paint(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        let Some(p) = self.prepared.take() else {
            return;
        };
        let mut textures = std::mem::take(&mut self.textures);
        for (id, deltas) in &textures.set {
            for delta in deltas {
                self.renderer.update_texture(device, queue, *id, delta);
            }
        }
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [size.0, size.1],
            pixels_per_point: p.pixels_per_point,
        };
        self.renderer
            .update_buffers(device, queue, encoder, &p.jobs, &screen);
        {
            let mut pass = begin_pass(encoder, "egui", target, false).forget_lifetime();
            self.renderer.render(&mut pass, &p.jobs, &screen);
        }
        for id in &textures.free {
            self.renderer.free_texture(id);
        }
        // Everything is applied; tell epaint so its drop check doesn't fire.
        textures.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_fades_in_holds_and_fades_out() {
        let mut t = Toast::new("hi");
        assert_eq!(t.alpha(), 0.0);
        t.tick(TOAST_FADE_IN / 2.0);
        assert!((t.alpha() - 0.5).abs() < 1e-5);
        t.tick(0.5);
        assert_eq!(t.alpha(), 1.0);
        t.tick(TOAST_SECS - TOAST_FADE_IN / 2.0 - 0.5 - TOAST_FADE_OUT / 2.0);
        assert!((t.alpha() - 0.5).abs() < 1e-3);
    }

    #[test]
    fn toast_finishes_after_its_lifetime() {
        let mut t = Toast::new("hi");
        assert!(t.tick(TOAST_SECS - 0.01));
        assert!(!t.tick(0.02));
        assert_eq!(t.alpha(), 0.0);
    }
}
