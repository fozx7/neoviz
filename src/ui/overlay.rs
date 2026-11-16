//! The settings panel (Tab) and the heads-up text.

use std::{path::Path, sync::OnceLock};

use egui::{Align2, Color32, FontId, Id, LayerId, Order, Pos2};

use crate::{
    audio::{DeviceInfo, analysis::GAIN_LIMIT_DB},
    config::{
        AUTO_SWITCH_RANGE, KeepAwake, PRESET_SWITCH_RANGE, SMOOTHING_RANGE, SPEED_RANGE, Settings,
    },
};

use super::{
    Toast,
    debug_hud::{self, DebugLabels},
    fps_hud::{self, FpsHud},
};

/// What the panel needs to show besides the settings themselves.
pub struct PanelInfo<'a> {
    pub visualizers: &'a [&'a str],
    pub current: usize,
    pub devices: &'a [DeviceInfo],
    pub device_list_error: Option<&'a str>,
    pub capture_error: Option<&'a str>,
    pub locked: bool,
    pub config_path: Option<&'a Path>,
    /// (visualizer name, status line) for visualizers with something to report.
    pub statuses: Vec<(String, String)>,
    /// Whether the screen is currently being kept on.
    pub keeping_awake: bool,
}

/// Things the user asked for in the panel that aren't plain settings.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PanelRequests {
    /// Switch to this visualizer now.
    pub select: Option<usize>,
    pub refresh_devices: bool,
    pub toggle_lock: bool,
    /// Reload preset folders.
    pub rescan_presets: bool,
}

/// projectM rates loudness against its own recent average, so our gain
/// barely changes MilkDrop (only the size of drawn waveforms).
const MILKDROP_LEVELS_NOTE: &str = "MilkDrop presets set their own levels, so sensitivity mostly affects the built-in visualizers.";

const LOOPBACK_LABEL: &str = "Default output (what's playing)";

/// Draws the settings window, editing `settings` in place. Sets `open` to
/// false if the user closes it.
pub fn settings_panel(
    ctx: &egui::Context,
    settings: &mut Settings,
    info: &PanelInfo,
    open: &mut bool,
) -> PanelRequests {
    let mut requests = PanelRequests::default();
    egui::Window::new("neoViz settings")
        .open(open)
        .collapsible(false)
        .resizable(false)
        .default_pos([16.0, 16.0])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                audio_section(ui, settings, info, &mut requests);
                ui.separator();
                visualizer_section(ui, settings, info, &mut requests);
                ui.separator();
                if cfg!(milkdrop) {
                    milkdrop_section(ui, settings, info, &mut requests);
                    ui.separator();
                }
                motion_section(ui, settings);
                ui.separator();
                display_section(ui, settings, info.keeping_awake);
                ui.separator();
                footer(ui, info);
            });
        });
    requests
}

fn audio_section(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    info: &PanelInfo,
    requests: &mut PanelRequests,
) {
    ui.heading("Audio");
    ui.horizontal(|ui| {
        let selected = match &settings.device {
            None => LOOPBACK_LABEL.to_owned(),
            Some(name) => info
                .devices
                .iter()
                .find(|d| &d.name == name)
                .map_or_else(|| name.clone(), |d| d.description.clone()),
        };
        egui::ComboBox::from_id_salt("device")
            .selected_text(selected)
            .width(320.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut settings.device, None, LOOPBACK_LABEL);
                for d in info.devices {
                    let kind = if d.is_loopback { "output" } else { "input" };
                    let label = format!("{} ({kind})", d.description);
                    ui.selectable_value(&mut settings.device, Some(d.name.clone()), label);
                }
            });
        if ui.button("Refresh").clicked() {
            requests.refresh_devices = true;
        }
    });
    if let Some(err) = info.device_list_error {
        ui.colored_label(Color32::LIGHT_RED, format!("Couldn't list devices: {err}"));
    }
    if let Some(err) = info.capture_error {
        ui.colored_label(Color32::LIGHT_RED, format!("Not capturing: {err}"));
    }
    ui.add(
        egui::Slider::new(&mut settings.sensitivity_db, -GAIN_LIMIT_DB..=GAIN_LIMIT_DB)
            .text("Sensitivity")
            .suffix(" dB")
            .step_by(1.0),
    )
    .on_hover_text(MILKDROP_LEVELS_NOTE);
    ui.small(MILKDROP_LEVELS_NOTE);
    ui.add(
        egui::Slider::new(
            &mut settings.smoothing_secs,
            SMOOTHING_RANGE.0..=SMOOTHING_RANGE.1,
        )
        .text("Smoothing")
        .suffix(" s")
        .logarithmic(true),
    );
}

fn visualizer_section(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    info: &PanelInfo,
    requests: &mut PanelRequests,
) {
    ui.heading("Visualizers");
    ui.horizontal(|ui| {
        ui.checkbox(&mut settings.random_mode, "Random mode");
        let mut locked = info.locked;
        if ui.checkbox(&mut locked, "Lock current").changed() {
            requests.toggle_lock = true;
        }
    });
    ui.add_enabled_ui(settings.random_mode, |ui| {
        ui.add(
            egui::Slider::new(
                &mut settings.auto_switch_secs,
                AUTO_SWITCH_RANGE.0..=AUTO_SWITCH_RANGE.1,
            )
            .text("Switch every")
            .suffix(" s")
            .logarithmic(true),
        );
        ui.checkbox(
            &mut settings.switch_on_beat,
            "Switch early on a strong beat",
        );
    });
    ui.label("Tick to include in rotation; click a name to show it:");
    for (i, name) in info.visualizers.iter().enumerate() {
        ui.horizontal(|ui| {
            let mut included = settings.is_in_rotation(name);
            if ui.checkbox(&mut included, "").changed() {
                settings.set_in_rotation(name, included);
            }
            if ui.selectable_label(i == info.current, *name).clicked() {
                requests.select = Some(i);
            }
        });
    }
}

fn milkdrop_section(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    info: &PanelInfo,
    requests: &mut PanelRequests,
) {
    ui.heading("MilkDrop");
    ui.horizontal(|ui| {
        ui.label("Preset folder");
        let field = ui.add(
            egui::TextEdit::singleline(&mut settings.preset_dir)
                .hint_text(preset_dir_hint())
                .desired_width(260.0),
        );
        // Rescan when editing finishes (Enter or clicking away), not per keystroke.
        if field.lost_focus() {
            requests.rescan_presets = true;
        }
        if ui.button("Rescan").clicked() {
            requests.rescan_presets = true;
        }
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut settings.preset_auto, "Change preset every");
        ui.add_enabled(
            settings.preset_auto,
            egui::DragValue::new(&mut settings.preset_secs)
                .range(PRESET_SWITCH_RANGE.0..=PRESET_SWITCH_RANGE.1)
                .speed(1.0)
                .suffix(" s"),
        );
    });
    ui.checkbox(&mut settings.preset_shuffle, "Shuffle (random order)");
    for (name, status) in &info.statuses {
        ui.label(format!("{name}: {status}"));
    }
    ui.small("[ / ] step through presets while MilkDrop is showing.");
}

/// An example preset folder in the OS's own path style: the user's real
/// home folder on Windows (`C:\Users\name\milkdrop-presets`), `~/...` elsewhere.
fn preset_dir_hint() -> &'static str {
    static HINT: OnceLock<String> = OnceLock::new();
    HINT.get_or_init(|| {
        if cfg!(windows) {
            directories::BaseDirs::new().map_or_else(
                || r"C:\Users\you\milkdrop-presets".to_owned(),
                |d| d.home_dir().join("milkdrop-presets").display().to_string(),
            )
        } else {
            "~/milkdrop-presets".to_owned()
        }
    })
}

fn motion_section(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.heading("Motion");
    ui.add(
        egui::Slider::new(&mut settings.speed, SPEED_RANGE.0..=SPEED_RANGE.1)
            .text("Speed")
            .suffix("×")
            .logarithmic(true),
    )
    .on_hover_text("How fast visualizers move and cycle colours on their own. MilkDrop presets keep their own pace.");
    let mut percent = settings.flash * 100.0;
    if ui
        .add(
            egui::Slider::new(&mut percent, 0.0..=100.0)
                .text("Beat flashes")
                .suffix("%"),
        )
        .on_hover_text("Brightness pulses on each beat. Lower this if it's too flashy.")
        .changed()
    {
        settings.flash = percent / 100.0;
    }
}

fn display_section(ui: &mut egui::Ui, settings: &mut Settings, keeping_awake: bool) {
    ui.heading("Display");
    ui.horizontal(|ui| {
        ui.checkbox(&mut settings.bloom, "Bloom");
        ui.checkbox(&mut settings.scanlines, "CRT scanlines");
        ui.checkbox(&mut settings.show_fps, "FPS counter");
        ui.add_enabled(
            settings.show_fps,
            egui::Checkbox::new(&mut settings.fps_detail, "Detailed"),
        )
        .on_hover_text(
            "Adds 1% / 5% lows, frame times, how long each frame spends working vs \
             waiting for the display, late (dropped) frames, and a frame-time graph.",
        );
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut settings.always_on_top, "Always on top");
        ui.checkbox(&mut settings.borderless, "Borderless window");
    });
    ui.checkbox(
        &mut settings.fade_on_silence,
        "Fade out when the music stops",
    )
    .on_hover_text(
        "Fade to black after a couple of seconds of silence, and back in when sound returns.",
    );
    ui.horizontal(|ui| {
        ui.label("Keep screen on");
        let options = [
            (KeepAwake::WhilePlaying, "While music plays"),
            (KeepAwake::Always, "Always"),
            (KeepAwake::Off, "Off"),
        ];
        let current = options
            .iter()
            .find(|(mode, _)| *mode == settings.keep_awake)
            .map_or("", |(_, label)| label);
        egui::ComboBox::from_id_salt("keep_awake")
            .selected_text(current)
            .show_ui(ui, |ui| {
                for (mode, label) in options {
                    ui.selectable_value(&mut settings.keep_awake, mode, label);
                }
            })
            .response
            .on_hover_text("Stop the screen blanking or locking, like a playing video. \"While music plays\" lets it sleep after a minute of silence.");
        if keeping_awake {
            ui.weak("(on now)");
        }
    });
}

fn footer(ui: &mut egui::Ui, info: &PanelInfo) {
    ui.small("←/→ switch · R random · Space lock · ↑/↓ sensitivity · F full screen · Tab close");
    if let Some(path) = info.config_path {
        ui.small(format!("Saved to {}", path.display()));
    }
}

/// Text drawn straight onto the screen, outside any window.
pub struct Hud<'a> {
    pub toast: Option<&'a Toast>,
    pub fps: Option<FpsHud>,
    /// Labels for the analysis debug view, while it's showing.
    pub debug: Option<DebugLabels>,
    /// A credit line the visualizer on screen needs shown (e.g. for a photo).
    pub credit: Option<&'a str>,
    pub capture_error: Option<&'a str>,
}

pub fn draw_hud(ctx: &egui::Context, hud: &Hud) {
    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::unique("hud")));
    let screen = ctx.content_rect();

    if let Some(toast) = hud.toast {
        let pos = Pos2::new(screen.center().x, screen.bottom() - screen.height() * 0.08);
        shadowed_text(
            &painter,
            pos,
            Align2::CENTER_BOTTOM,
            &toast.text,
            32.0,
            Color32::WHITE,
            toast.alpha(),
        );
    }
    if let Some(credit) = hud.credit {
        let pos = Pos2::new(screen.left() + 10.0, screen.bottom() - 8.0);
        let colour = Color32::from_gray(190);
        shadowed_text(
            &painter,
            pos,
            Align2::LEFT_BOTTOM,
            credit,
            12.0,
            colour,
            0.75,
        );
    }
    if let Some(labels) = &hud.debug {
        debug_hud::draw(&painter, screen, labels);
    }
    if let Some(fps) = &hud.fps {
        fps_hud::draw(&painter, screen, fps);
    }
    if let Some(err) = hud.capture_error {
        let pos = Pos2::new(screen.center().x, screen.top() + 16.0);
        let text = format!("No audio: {err}");
        shadowed_text(
            &painter,
            pos,
            Align2::CENTER_TOP,
            &text,
            18.0,
            Color32::LIGHT_RED,
            1.0,
        );
    }
}

pub(super) fn shadowed_text(
    painter: &egui::Painter,
    pos: Pos2,
    anchor: Align2,
    text: &str,
    size: f32,
    color: Color32,
    alpha: f32,
) {
    let font = FontId::proportional(size);
    let shadow = Color32::from_black_alpha((200.0 * alpha) as u8);
    painter.text(
        pos + egui::vec2(2.0, 2.0),
        anchor,
        text,
        font.clone(),
        shadow,
    );
    painter.text(pos, anchor, text, font, color.gamma_multiply(alpha));
}
