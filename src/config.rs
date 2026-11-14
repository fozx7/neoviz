//! User settings, persisted as TOML in the OS config directory
//! (`~/.config/neoviz/settings.toml` on Linux).

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::audio::analysis::GAIN_LIMIT_DB;

const FILE_NAME: &str = "settings.toml";
pub const SMOOTHING_RANGE: (f32, f32) = (0.03, 1.0);
pub const AUTO_SWITCH_RANGE: (f32, f32) = (5.0, 300.0);
pub const PRESET_SWITCH_RANGE: (f32, f32) = (5.0, 300.0);
pub const SPEED_RANGE: (f32, f32) = (0.25, 2.0);

/// When to stop the screen blanking or locking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeepAwake {
    Off,
    /// While there's sound (released after a minute of silence), like a video player.
    #[default]
    WhilePlaying,
    /// Whenever neoViz is open.
    Always,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Capture device; `None` = loopback of the default output. Session-only:
    /// never saved, so every launch starts on whatever is playing now.
    #[serde(skip)]
    pub device: Option<String>,
    /// Sensitivity in dB, ±[`GAIN_LIMIT_DB`].
    pub sensitivity_db: f32,
    /// How slowly levels fall, in seconds (higher = smoother).
    pub smoothing_secs: f32,
    /// Visualizer shown at startup (the last one used).
    pub visualizer: Option<String>,
    pub random_mode: bool,
    pub auto_switch_secs: f32,
    /// In random mode, also switch early on a strong beat.
    pub switch_on_beat: bool,
    /// Visualizers left out of rotation, by name.
    pub excluded: Vec<String>,
    pub bloom: bool,
    pub scanlines: bool,
    pub show_fps: bool,
    /// The FPS counter shows lows, frame times and a graph, not just the rate.
    pub fps_detail: bool,
    pub always_on_top: bool,
    pub borderless: bool,
    /// Folder searched (recursively) for MilkDrop `.milk` presets; empty = none.
    pub preset_dir: String,
    /// Change MilkDrop presets automatically every `preset_secs`.
    pub preset_auto: bool,
    /// Seconds between automatic MilkDrop preset changes.
    pub preset_secs: f32,
    /// Play MilkDrop presets in random order (else folder order).
    pub preset_shuffle: bool,
    /// Animation speed of the visualizers' own motion (rotation, colour
    /// cycling, particles); 1.0 = normal. Doesn't slow the audio response.
    pub speed: f32,
    /// Strength of the colour flash on each beat, 0..1.
    pub flash: f32,
    /// Keep the screen from blanking while visualizing.
    pub keep_awake: KeepAwake,
    /// Fade the picture to black when the audio goes silent, and back in
    /// when sound returns.
    pub fade_on_silence: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device: None,
            sensitivity_db: 0.0,
            smoothing_secs: 0.15,
            visualizer: None,
            random_mode: false,
            auto_switch_secs: 30.0,
            switch_on_beat: true,
            excluded: Vec::new(),
            bloom: true,
            scanlines: false,
            show_fps: false,
            fps_detail: false,
            always_on_top: false,
            borderless: false,
            preset_dir: String::new(),
            preset_auto: true,
            preset_secs: 30.0,
            preset_shuffle: true,
            speed: 1.0,
            flash: 1.0,
            keep_awake: KeepAwake::default(),
            fade_on_silence: true,
        }
    }
}

impl Settings {
    /// Clamps out-of-range values (e.g. from a hand-edited file) into range.
    pub fn sanitized(mut self) -> Self {
        let finite_or = |v: f32, fallback: f32| if v.is_finite() { v } else { fallback };
        let d = Self::default();
        self.sensitivity_db =
            finite_or(self.sensitivity_db, d.sensitivity_db).clamp(-GAIN_LIMIT_DB, GAIN_LIMIT_DB);
        self.smoothing_secs = finite_or(self.smoothing_secs, d.smoothing_secs)
            .clamp(SMOOTHING_RANGE.0, SMOOTHING_RANGE.1);
        self.auto_switch_secs = finite_or(self.auto_switch_secs, d.auto_switch_secs)
            .clamp(AUTO_SWITCH_RANGE.0, AUTO_SWITCH_RANGE.1);
        self.speed = finite_or(self.speed, d.speed).clamp(SPEED_RANGE.0, SPEED_RANGE.1);
        self.flash = finite_or(self.flash, d.flash).clamp(0.0, 1.0);
        self.preset_secs = finite_or(self.preset_secs, d.preset_secs)
            .clamp(PRESET_SWITCH_RANGE.0, PRESET_SWITCH_RANGE.1);
        self
    }

    pub fn is_in_rotation(&self, name: &str) -> bool {
        !self.excluded.iter().any(|n| n == name)
    }

    pub fn set_in_rotation(&mut self, name: &str, included: bool) {
        self.excluded.retain(|n| n != name);
        if !included {
            self.excluded.push(name.to_owned());
        }
    }

    fn from_toml(text: &str) -> Result<Self> {
        Ok(toml::from_str::<Self>(text)?.sanitized())
    }

    fn to_toml(&self) -> Result<String> {
        Ok(toml::to_string_pretty(self)?)
    }
}

/// Where settings live; `None` if the OS has no config directory.
pub fn default_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "neoviz").map(|d| d.config_dir().join(FILE_NAME))
}

/// Loads settings from `path`. A missing file gives defaults. An unreadable
/// one is moved aside to `settings.toml.bad` (so it isn't overwritten) and
/// defaults are used.
pub fn load(path: &Path) -> Settings {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Settings::default(),
        Err(e) => {
            log::warn!("can't read {}: {e}; using defaults", path.display());
            return Settings::default();
        }
    };
    match Settings::from_toml(&text) {
        Ok(settings) => settings,
        Err(e) => {
            let backup = path.with_extension("toml.bad");
            log::warn!(
                "{} is invalid ({e:#}); moved it to {} and using defaults",
                path.display(),
                backup.display()
            );
            if let Err(e) = fs::rename(path, &backup) {
                log::warn!("couldn't move it aside: {e}");
            }
            Settings::default()
        }
    }
}

/// Writes settings to `path` atomically (temp file + rename), creating the directory.
pub fn save(settings: &Settings, path: &Path) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, settings.to_toml()?).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("neoviz-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn round_trips_through_toml() {
        let mut s = Settings {
            sensitivity_db: 6.0,
            random_mode: true,
            ..Default::default()
        };
        s.set_in_rotation("Starfield Warp", false);
        let back = Settings::from_toml(&s.to_toml().unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let s = Settings::from_toml("bloom = false").unwrap();
        assert!(!s.bloom);
        assert_eq!(s.auto_switch_secs, Settings::default().auto_switch_secs);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let s = Settings::from_toml(
            "sensitivity_db = 99.0\nsmoothing_secs = -1.0\nauto_switch_secs = 1.0",
        )
        .unwrap();
        assert_eq!(s.sensitivity_db, GAIN_LIMIT_DB);
        assert_eq!(s.smoothing_secs, SMOOTHING_RANGE.0);
        assert_eq!(s.auto_switch_secs, AUTO_SWITCH_RANGE.0);
    }

    #[test]
    fn speed_and_flash_are_clamped() {
        let s = Settings::from_toml("speed = 10.0\nflash = -0.5").unwrap();
        assert_eq!(s.speed, SPEED_RANGE.1);
        assert_eq!(s.flash, 0.0);
        let s = Settings::from_toml("speed = 0.01\nflash = 3.0").unwrap();
        assert_eq!(s.speed, SPEED_RANGE.0);
        assert_eq!(s.flash, 1.0);
    }

    #[test]
    fn old_settings_files_get_normal_speed_and_full_flash() {
        let s = Settings::from_toml("bloom = true").unwrap();
        assert_eq!(s.speed, 1.0);
        assert_eq!(s.flash, 1.0);
    }

    #[test]
    fn keep_awake_reads_and_writes_readable_names() {
        let s = Settings::from_toml("keep_awake = \"always\"").unwrap();
        assert_eq!(s.keep_awake, KeepAwake::Always);
        let s = Settings {
            keep_awake: KeepAwake::WhilePlaying,
            ..Default::default()
        };
        assert!(
            s.to_toml()
                .unwrap()
                .contains("keep_awake = \"while_playing\"")
        );
        assert_eq!(Settings::default().keep_awake, KeepAwake::WhilePlaying);
    }

    #[test]
    fn non_finite_values_fall_back_to_defaults() {
        let s = Settings {
            sensitivity_db: f32::NAN,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(s.sensitivity_db, 0.0);
    }

    #[test]
    fn device_is_never_saved() {
        let s = Settings {
            device: Some("alsa_output.monitor".into()),
            ..Default::default()
        };
        assert!(!s.to_toml().unwrap().contains("device"));
    }

    #[test]
    fn saved_device_is_ignored_so_startup_uses_the_current_output() {
        let s = Settings::from_toml("device = \"alsa_output.monitor\"\nbloom = false").unwrap();
        assert_eq!(s.device, None);
        assert!(!s.bloom, "other settings still load");
    }

    #[test]
    fn wrong_types_are_an_error() {
        assert!(Settings::from_toml("bloom = \"yes\"").is_err());
    }

    #[test]
    fn rotation_membership_toggles() {
        let mut s = Settings::default();
        assert!(s.is_in_rotation("A"));
        s.set_in_rotation("A", false);
        s.set_in_rotation("A", false);
        assert!(!s.is_in_rotation("A"));
        assert_eq!(s.excluded.len(), 1);
        s.set_in_rotation("A", true);
        assert!(s.is_in_rotation("A"));
    }

    #[test]
    fn save_then_load() {
        let dir = temp_dir("save");
        let path = dir.join("nested").join(FILE_NAME);
        let s = Settings {
            show_fps: true,
            ..Default::default()
        };
        save(&s, &path).unwrap();
        assert_eq!(load(&path), s);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = temp_dir("missing");
        assert_eq!(load(&dir.join(FILE_NAME)), Settings::default());
    }

    #[test]
    fn broken_file_is_moved_aside() {
        let dir = temp_dir("broken");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        fs::write(&path, "this is = = not toml").unwrap();
        assert_eq!(load(&path), Settings::default());
        assert!(!path.exists());
        assert!(path.with_extension("toml.bad").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
