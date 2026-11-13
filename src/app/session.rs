//! Everything that exists while the window is open: GPU, audio, visualizers,
//! UI and settings, plus the per-frame loop.

use std::{path::PathBuf, sync::Arc, time::Instant};

use winit::window::{Fullscreen, Window, WindowLevel};

use crate::{
    audio::{
        self, Capture, DeviceInfo,
        analysis::{Analyzer, GAIN_LIMIT_DB, GAIN_STEP_DB},
        bands::BandSettings,
    },
    config::{self, Settings},
    input::Action,
    render::{
        compositor::{Compositor, INCOMING, OUTGOING, PostFx},
        debug_view::DebugView,
        gpu::Gpu,
    },
    ui::{
        Toast, Ui,
        overlay::{self, Hud, PanelInfo},
    },
    viz::{AutoSwitch, Rng, Switcher, Visualizer},
};

const WINDOW_TITLE: &str = "neoViz";
/// Longest frame time fed to the analysis, in seconds.
const MAX_FRAME_DT: f32 = 0.1;
/// How often changed settings are written to disk.
const SAVE_INTERVAL_SECS: f32 = 1.0;
/// Smoothing for the FPS counter.
const FPS_SMOOTHING: f32 = 0.05;

/// What the event loop should do after an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Continue,
    Quit,
}

pub struct Session {
    pub window: Arc<Window>,
    gpu: Gpu,
    capture: Capture,
    analyzer: Analyzer,
    visualizers: Vec<Box<dyn Visualizer>>,
    names: Vec<String>,
    switcher: Switcher,
    auto_switch: AutoSwitch,
    rng: Rng,
    compositor: Compositor,
    debug_view: DebugView,
    ui: Ui,
    settings: Settings,
    /// What's on disk, to know when to save.
    saved: Settings,
    config_path: Option<PathBuf>,
    since_save: f32,
    devices: Vec<DeviceInfo>,
    device_list_error: Option<String>,
    show_debug: bool,
    show_settings: bool,
    locked: bool,
    toast: Option<Toast>,
    fps: f32,
    last_frame: Instant,
}

impl Session {
    pub fn new(
        window: Arc<Window>,
        gpu: Gpu,
        visualizers: Vec<Box<dyn Visualizer>>,
        start: usize,
        settings: Settings,
        saved: Settings,
        config_path: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let size = gpu.size();
        let mut visualizers = visualizers;
        for v in &mut visualizers {
            v.configure(&settings, true);
        }
        let names: Vec<String> = visualizers.iter().map(|v| v.name().to_owned()).collect();
        let session = Self {
            capture: Capture::start(settings.device.clone())?,
            compositor: Compositor::new(gpu.device(), gpu.surface_format(), size),
            debug_view: DebugView::new(gpu.device(), gpu.surface_format()),
            ui: Ui::new(&window, gpu.device(), gpu.ui_format()),
            switcher: Switcher::new(visualizers.len(), start),
            toast: Some(Toast::new(names[start].clone())),
            analyzer: Analyzer::default(),
            auto_switch: AutoSwitch::default(),
            rng: Rng::new(seed_from_clock()),
            visualizers,
            names,
            settings,
            saved,
            config_path,
            since_save: 0.0,
            devices: Vec::new(),
            device_list_error: None,
            show_debug: false,
            show_settings: false,
            locked: false,
            fps: 60.0,
            last_frame: Instant::now(),
            window,
            gpu,
        };
        session.update_title();
        session.window.request_redraw();
        Ok(session)
    }

    /// Lets the UI see a window event first. Returns true if it used it.
    pub fn ui_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        self.ui.on_window_event(&self.window, event)
    }

    pub fn ui_wants_keyboard(&self) -> bool {
        self.ui.wants_keyboard()
    }

    pub fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        self.gpu.resize(size);
        let size = self.gpu.size();
        self.compositor.resize(self.gpu.device(), size);
        for v in &mut self.visualizers {
            v.resize(self.gpu.device(), size);
        }
    }

    pub fn apply(&mut self, action: Action) -> Outcome {
        match action {
            Action::SensitivityUp | Action::SensitivityDown => {
                let step = if action == Action::SensitivityUp {
                    GAIN_STEP_DB
                } else {
                    -GAIN_STEP_DB
                };
                let gain =
                    (self.settings.sensitivity_db + step).clamp(-GAIN_LIMIT_DB, GAIN_LIMIT_DB);
                self.settings.sensitivity_db = gain;
                self.toast = Some(Toast::new(format!("Sensitivity {gain:+} dB")));
            }
            Action::NextVisualizer | Action::PreviousVisualizer => {
                let direction = if action == Action::NextVisualizer {
                    1
                } else {
                    -1
                };
                self.switcher.step(direction, &self.rotation());
                self.on_switched();
            }
            Action::ToggleRandom => {
                self.settings.random_mode = !self.settings.random_mode;
                self.auto_switch.reset();
                self.toast = Some(Toast::new(format!(
                    "Random mode {}",
                    on_off(self.settings.random_mode)
                )));
            }
            Action::ToggleLock => self.toggle_lock(),
            Action::NextPreset | Action::PreviousPreset => {
                let direction = if action == Action::NextPreset { 1 } else { -1 };
                let current = &mut self.visualizers[self.switcher.current()];
                if let Some(preset) = current.step_preset(direction) {
                    self.toast = Some(Toast::new(format!("{}: {preset}", current.name())));
                }
            }
            Action::ToggleSettings => self.set_settings_open(!self.show_settings),
            Action::ToggleDebugView => self.show_debug = !self.show_debug,
            Action::ToggleBloom => {
                self.settings.bloom = !self.settings.bloom;
                self.toast = Some(Toast::new(format!("Bloom {}", on_off(self.settings.bloom))));
            }
            Action::ToggleScanlines => {
                self.settings.scanlines = !self.settings.scanlines;
                self.toast = Some(Toast::new(format!(
                    "Scanlines {}",
                    on_off(self.settings.scanlines)
                )));
            }
            Action::ToggleFullscreen => {
                let next = match self.window.fullscreen() {
                    Some(_) => None,
                    None => Some(Fullscreen::Borderless(self.window.current_monitor())),
                };
                self.window.set_fullscreen(next);
            }
            Action::Escape if self.show_settings => self.set_settings_open(false),
            Action::Escape if self.window.fullscreen().is_some() => {
                self.window.set_fullscreen(None)
            }
            Action::Escape => return Outcome::Quit,
        }
        Outcome::Continue
    }

    pub fn redraw(&mut self) {
        let now = Instant::now();
        // Clamp so a stall (e.g. dragging the window) doesn't cause a huge jump.
        let dt = (now - self.last_frame).as_secs_f32().min(MAX_FRAME_DT);
        self.last_frame = now;
        if dt > 0.0 {
            self.fps += (1.0 / dt - self.fps) * FPS_SMOOTHING;
        }

        self.analyzer.settings = BandSettings {
            gain_db: self.settings.sensitivity_db,
            decay_secs: self.settings.smoothing_secs,
        };
        let sample_rate = self.capture.sample_rate();
        self.analyzer.ingest(&mut self.capture.consumer);
        self.analyzer.analyze(sample_rate, dt);
        let beat = {
            let audio = self.analyzer.frame();
            audio.beat.then_some(audio.beat_intensity)
        };
        if self.settings.random_mode
            && !self.locked
            && self.auto_switch.tick(
                dt,
                self.settings.auto_switch_secs,
                self.settings.switch_on_beat,
                beat,
            )
        {
            self.switcher.random(&self.rotation(), &mut self.rng);
            self.on_switched();
        }
        if let Some(message) = self.visualizers[self.switcher.current()].take_message() {
            self.toast = Some(Toast::new(message));
        }
        if let Some(toast) = &mut self.toast
            && !toast.tick(dt)
        {
            self.toast = None;
        }

        self.build_ui();

        let audio = self.analyzer.frame();
        let size = self.gpu.size();
        if self.show_debug {
            self.debug_view
                .update(self.gpu.queue(), &audio, sample_rate, size, dt);
        } else {
            self.switcher.advance(dt);
            let current = self.switcher.current();
            for index in std::iter::once(current).chain(self.switcher.outgoing()) {
                self.visualizers[index].update(&audio, dt);
            }
        }
        self.draw(size);
        self.save_if_changed(dt);
        self.window.request_redraw();
    }

    fn draw(&mut self, size: (u32, u32)) {
        let current = self.switcher.current();
        let outgoing = self.switcher.outgoing();
        let mix = self.switcher.mix();
        let post = PostFx {
            bloom: self.settings.bloom,
            scanlines: self.settings.scanlines,
        };
        let show_debug = self.show_debug;
        let (visualizers, compositor, debug_view, ui) = (
            &mut self.visualizers,
            &self.compositor,
            &self.debug_view,
            &mut self.ui,
        );
        self.gpu.render(|device, queue, encoder, targets| {
            if show_debug {
                debug_view.draw(encoder, targets.scene);
            } else {
                if let Some(index) = outgoing {
                    visualizers[index].render(queue, encoder, compositor.slot(OUTGOING), size);
                }
                visualizers[current].render(queue, encoder, compositor.slot(INCOMING), size);
                compositor.draw(queue, encoder, targets.scene, mix, post);
            }
            ui.paint(device, queue, encoder, targets.ui, size);
        });
    }

    /// Lays out the overlay and HUD, then acts on anything the user changed.
    fn build_ui(&mut self) {
        let before = self.settings.clone();
        let capture_error = self.capture.error();
        let names: Vec<&str> = self.names.iter().map(String::as_str).collect();
        let info = PanelInfo {
            visualizers: &names,
            current: self.switcher.current(),
            devices: &self.devices,
            device_list_error: self.device_list_error.as_deref(),
            capture_error: capture_error.as_deref(),
            locked: self.locked,
            config_path: self.config_path.as_deref(),
            statuses: self
                .visualizers
                .iter()
                .filter_map(|v| v.status().map(|s| (v.name().to_owned(), s)))
                .collect(),
        };
        let hud = Hud {
            toast: self.toast.as_ref(),
            fps: self.settings.show_fps.then_some(self.fps),
            capture_error: capture_error.as_deref(),
        };
        let mut requests = Default::default();
        let mut open = self.show_settings;
        let settings = &mut self.settings;
        self.ui.run(&self.window, |ctx| {
            if open {
                requests = overlay::settings_panel(ctx, settings, &info, &mut open);
            }
            overlay::draw_hud(ctx, &hud);
        });
        self.show_settings = open;

        if let Some(index) = requests.select {
            self.switcher.select(index);
            self.on_switched();
        }
        if requests.refresh_devices {
            self.refresh_devices();
        }
        if requests.toggle_lock {
            self.toggle_lock();
        }
        self.apply_changes(&before);
        if self.settings != before || requests.rescan_presets {
            for v in &mut self.visualizers {
                v.configure(&self.settings, requests.rescan_presets);
            }
        }
    }

    /// Applies settings that need more than being read each frame.
    fn apply_changes(&mut self, before: &Settings) {
        let s = &self.settings;
        if s.device != before.device {
            match Capture::start(s.device.clone()) {
                Ok(capture) => {
                    self.capture = capture;
                    let label = s.device.as_deref().unwrap_or("default output");
                    log::info!("capture device: {label}");
                }
                Err(e) => log::error!("restarting capture: {e:#}"),
            }
        }
        if s.always_on_top != before.always_on_top || s.borderless != before.borderless {
            self.apply_window_settings();
        }
        if s.random_mode != before.random_mode {
            self.auto_switch.reset();
        }
    }

    pub fn apply_window_settings(&self) {
        self.window
            .set_window_level(if self.settings.always_on_top {
                WindowLevel::AlwaysOnTop
            } else {
                WindowLevel::Normal
            });
        self.window.set_decorations(!self.settings.borderless);
    }

    fn set_settings_open(&mut self, open: bool) {
        if open && !self.show_settings {
            self.refresh_devices();
        }
        self.show_settings = open;
    }

    fn toggle_lock(&mut self) {
        self.locked = !self.locked;
        let text = if self.locked { "Locked" } else { "Unlocked" };
        self.toast = Some(Toast::new(text));
    }

    fn refresh_devices(&mut self) {
        match audio::list_devices() {
            Ok(devices) => {
                self.devices = devices;
                self.device_list_error = None;
            }
            Err(e) => self.device_list_error = Some(format!("{e:#}")),
        }
    }

    fn rotation(&self) -> Vec<bool> {
        self.names
            .iter()
            .map(|n| self.settings.is_in_rotation(n))
            .collect()
    }

    fn on_switched(&mut self) {
        let name = &self.names[self.switcher.current()];
        self.settings.visualizer = Some(name.clone());
        self.toast = Some(Toast::new(name.clone()));
        self.auto_switch.reset();
        self.show_debug = false;
        self.update_title();
    }

    fn update_title(&self) {
        let name = &self.names[self.switcher.current()];
        self.window.set_title(&format!("{WINDOW_TITLE} · {name}"));
    }

    fn save_if_changed(&mut self, dt: f32) {
        self.since_save += dt;
        if self.since_save >= SAVE_INTERVAL_SECS {
            self.since_save = 0.0;
            self.save();
        }
    }

    /// Writes settings to disk if they changed since the last save.
    pub fn save(&mut self) {
        let Some(path) = &self.config_path else {
            return;
        };
        if self.settings == self.saved {
            return;
        }
        match config::save(&self.settings, path) {
            Ok(()) => self.saved = self.settings.clone(),
            Err(e) => log::warn!("saving settings: {e:#}"),
        }
    }
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

fn seed_from_clock() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.subsec_nanos() ^ d.as_secs() as u32)
}
