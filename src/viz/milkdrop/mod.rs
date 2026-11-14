//! MilkDrop presets via libprojectM.
//!
//! projectM only speaks OpenGL, so it renders on its own thread into a hidden
//! OpenGL context (see [`worker`]); finished frames are uploaded as a wgpu
//! texture. That makes MilkDrop an ordinary visualizer: crossfades, bloom and
//! the overlay all work on top of it.

mod deck;
#[cfg(windows)]
mod glew;
mod presets;
mod worker;

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use anyhow::Result;

use crate::{audio::analysis::AudioFrame, config::Settings};

use super::Visualizer;
use deck::{Blit, Deck};
use presets::{Playlist, display_name};
use worker::Event;

pub const NAME: &str = "MilkDrop";
/// projectM renders at most this tall; larger windows are scaled up, which
/// keeps the per-frame copy cheap at 4K.
const MAX_RENDER_HEIGHT: u32 = 1080;
/// Stop auto-skipping failed presets after this many in a row.
const MAX_SKIPS_IN_A_ROW: u32 = 10;
/// Crossfade length when the preset timer moves on.
const AUTO_FADE_SECS: f32 = 2.0;
/// Crossfade length for `[` / `]` and the first preset.
const MANUAL_FADE_SECS: f32 = 0.5;
const PRESETS_HINT: &str = "No preset folder set. Get presets (e.g. github.com/projectM-visualizer/presets-cream-of-the-crop) and choose the folder below.";

/// Two decks take turns: the live one is on screen while the other loads
/// the next preset in the background (preloading). A change crossfades to
/// the other deck once its preset is ready, so a slow-loading preset never
/// stalls the picture.
pub struct MilkDrop {
    blit: Blit,
    decks: [Deck; 2],
    /// Index of the deck on screen.
    live: usize,
    /// A change waiting for the other deck to finish loading: its fade length.
    pending_fade: Option<f32>,
    /// Crossfade from the other deck to the live one, in progress.
    fade: Option<Fade>,
    /// Presets projectM couldn't load, skipped for the rest of the session.
    bad: HashSet<PathBuf>,
    playlist: Playlist,
    preset_dir: String,
    auto_change: bool,
    preset_secs: f32,
    since_preset: f32,
    /// Locked by the user (`Space`): no automatic preset changes.
    locked: bool,
    /// The Motion speed setting, undone: presets keep their own pace and
    /// the timer counts real seconds.
    speed: f32,
    gain: f32,
    status: Option<String>,
    /// Text for the next toast (preset changes, load failures).
    message: Option<String>,
    /// Consecutive presets that failed to load.
    failures_in_a_row: u32,
}

impl MilkDrop {
    pub fn new(device: &wgpu::Device, size: (u32, u32)) -> Result<Self> {
        let render_size = render_size(size);
        let defaults = Settings::default();
        let blit = Blit::new(device);
        let decks = [
            Deck::start(&blit, render_size)?,
            Deck::start(&blit, render_size)?,
        ];
        Ok(Self {
            blit,
            decks,
            live: 0,
            pending_fade: None,
            fade: None,
            bad: HashSet::new(),
            playlist: Playlist::new(Vec::new(), defaults.preset_shuffle, 1),
            preset_dir: String::new(),
            auto_change: defaults.preset_auto,
            preset_secs: defaults.preset_secs,
            since_preset: 0.0,
            locked: false,
            speed: defaults.speed,
            gain: 1.0,
            status: Some(PRESETS_HINT.to_owned()),
            message: None,
            failures_in_a_row: 0,
        })
    }

    fn standby(&self) -> usize {
        1 - self.live
    }

    fn rescan(&mut self, shuffle: bool) {
        self.playlist = Playlist::new(Vec::new(), shuffle, seed());
        self.bad.clear();
        self.pending_fade = None;
        self.finish_fade();
        self.decks[self.standby()].clear();
        if self.preset_dir.trim().is_empty() {
            self.status = Some(PRESETS_HINT.to_owned());
            return;
        }
        let home = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf());
        let dir = presets::expand_home(&self.preset_dir, home.as_deref());
        match presets::scan(&dir) {
            Ok(found) if found.is_empty() => {
                self.status = Some(format!("No .milk presets found in {}", dir.display()));
            }
            Ok(found) => {
                log::info!("{} MilkDrop presets in {}", found.len(), dir.display());
                self.status = Some(format!("{} presets in {}", found.len(), dir.display()));
                self.playlist = Playlist::new(found, shuffle, seed());
                self.failures_in_a_row = 0;
                self.change(1, MANUAL_FADE_SECS);
            }
            Err(e) => self.status = Some(format!("Can't read preset folder: {e:#}")),
        }
    }

    /// Moves through the playlist and switches to that preset (as soon as
    /// it's loaded). Returns its name.
    fn change(&mut self, direction: isize, fade_secs: f32) -> Option<String> {
        // The other deck is needed now, even if it's still fading out. (It
        // then preloads the next preset, which is reused if that's the one.)
        self.finish_fade();
        let path = step_skipping(&mut self.playlist, direction, &self.bad)?;
        let name = display_name(&path);
        log::info!("MilkDrop preset: {name}");
        self.since_preset = 0.0;
        self.decks[self.standby()].load(&path);
        self.pending_fade = Some(fade_secs);
        self.start_pending_switch();
        Some(name)
    }

    /// Puts the other deck on screen if a change is waiting and it's ready.
    fn start_pending_switch(&mut self) {
        let Some(secs) = self.pending_fade else {
            return;
        };
        if !self.decks[self.standby()].is_ready() {
            return;
        }
        self.pending_fade = None;
        self.live = self.standby();
        self.fade = Some(Fade::new(secs));
        log::debug!("MilkDrop: deck {} on screen, {secs} s crossfade", self.live);
    }

    /// Ends any crossfade, and has the deck that went off screen preload
    /// the preset that comes next.
    fn finish_fade(&mut self) {
        if self.fade.take().is_some() {
            self.preload();
        }
    }

    fn preload(&mut self) {
        let mut ahead = self.playlist.clone();
        if let Some(next) = step_skipping(&mut ahead, 1, &self.bad) {
            let standby = self.standby();
            log::debug!(
                "MilkDrop: deck {standby} preloading {}",
                display_name(&next)
            );
            self.decks[standby].load(&next);
        }
    }

    /// Sends this frame's new audio, interleaved stereo, to the deck(s) on screen.
    fn feed_audio(&mut self, audio: &AudioFrame) {
        let start = audio.left.len() - audio.new_frames;
        let gain = self.gain;
        let pcm: Vec<f32> = audio.left[start..]
            .iter()
            .zip(&audio.right[start..])
            .flat_map(|(l, r)| [(l * gain).clamp(-1.0, 1.0), (r * gain).clamp(-1.0, 1.0)])
            .collect();
        if self.fade.is_some() {
            self.decks[self.standby()].send_audio(pcm.clone());
        }
        self.decks[self.live].send_audio(pcm);
    }

    fn handle_events(&mut self) {
        for index in 0..self.decks.len() {
            while let Some(event) = self.decks[index].next_event() {
                match event {
                    Event::Loaded { path } => {
                        if self.decks[index].preset() == Some(path.as_path()) {
                            self.failures_in_a_row = 0;
                        }
                    }
                    Event::LoadFailed { path } => self.on_load_failed(index, &path),
                }
            }
        }
        self.start_pending_switch();
    }

    /// Skips a preset projectM couldn't load, unless many have failed in a
    /// row (e.g. a folder of incompatible presets), to avoid looping forever.
    fn on_load_failed(&mut self, deck: usize, path: &Path) {
        let name = display_name(path);
        log::warn!("MilkDrop couldn't load preset {name}");
        self.bad.insert(path.to_path_buf());
        if self.decks[deck].preset() != Some(path) {
            return; // superseded by a later request
        }
        self.decks[deck].clear();
        self.failures_in_a_row += 1;
        if self.failures_in_a_row >= MAX_SKIPS_IN_A_ROW {
            self.pending_fade = None;
            self.message = Some(format!(
                "{MAX_SKIPS_IN_A_ROW} presets in a row failed to load; stopped skipping"
            ));
            return;
        }
        match self.pending_fade {
            // A change was waiting on it: move on to the one after.
            Some(secs) => {
                let next = self.change(1, secs);
                self.message = Some(match next {
                    Some(next) => {
                        format!("Skipped {name} (projectM can't load it)\n{NAME}: {next}")
                    }
                    None => format!("Couldn't load preset {name}"),
                });
            }
            // Only a preload: quietly preload the next one instead.
            None => self.preload(),
        }
    }
}

impl Visualizer for MilkDrop {
    fn name(&self) -> &str {
        NAME
    }

    fn configure(&mut self, settings: &Settings, rescan: bool) {
        self.gain = 10f32.powf(settings.sensitivity_db / 20.0);
        self.speed = settings.speed;
        self.auto_change = settings.preset_auto;
        self.preset_secs = settings.preset_secs;
        self.preset_dir = settings.preset_dir.clone();
        self.playlist.set_shuffle(settings.preset_shuffle);
        // Only on request: the folder field changes on every keystroke.
        if rescan {
            self.rescan(settings.preset_shuffle);
        }
    }

    fn update(&mut self, audio: &AudioFrame, motion_dt: f32) {
        let dt = real_seconds(motion_dt, self.speed);
        self.feed_audio(audio);
        self.handle_events();
        // The fade starts once the new preset's first frame is up.
        if let Some(fade) = &mut self.fade
            && self.decks[self.live].has_frame()
            && fade.advance(dt)
        {
            self.finish_fade();
        }
        self.since_preset += dt;
        if should_auto_change(
            self.auto_change && !self.locked,
            self.since_preset,
            self.preset_secs,
            self.playlist.len(),
        ) {
            self.failures_in_a_row = 0;
            if let Some(name) = self.change(1, AUTO_FADE_SECS) {
                self.message = Some(format!("{NAME}: {name}"));
            }
        }
    }

    fn step_preset(&mut self, direction: isize) -> Option<String> {
        self.failures_in_a_row = 0;
        self.change(direction, MANUAL_FADE_SECS)
    }

    fn set_locked(&mut self, locked: bool) {
        self.locked = locked;
        // A full interval on the current preset after unlocking, not an instant change.
        self.since_preset = 0.0;
    }

    fn status(&self) -> Option<String> {
        self.status.clone()
    }

    fn take_message(&mut self) -> Option<String> {
        self.message.take()
    }

    fn render(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        let size = render_size(size);
        let (live, other) = (self.live, self.standby());
        self.decks[live].render(&self.blit, queue, size);
        let Some(fade) = &self.fade else {
            self.blit.draw(encoder, target, &self.decks[live], None);
            return;
        };
        // Mid-crossfade: both decks run, the old one underneath.
        self.decks[other].render(&self.blit, queue, size);
        let amount = if self.decks[live].has_frame() {
            fade.amount()
        } else {
            0.0
        };
        self.blit.draw(
            encoder,
            target,
            &self.decks[other],
            Some((&self.decks[live], amount)),
        );
    }
}

/// A crossfade's progress.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Fade {
    secs: f32,
    /// 0..1 through the fade.
    progress: f32,
}

impl Fade {
    fn new(secs: f32) -> Self {
        Self {
            secs: secs.max(f32::EPSILON),
            progress: 0.0,
        }
    }

    /// Moves on by `dt` seconds; true once finished.
    fn advance(&mut self, dt: f32) -> bool {
        self.progress = (self.progress + dt / self.secs).min(1.0);
        self.progress >= 1.0
    }

    /// How much of the new preset shows: eased, 0..1.
    fn amount(&self) -> f32 {
        let t = self.progress;
        t * t * (3.0 - 2.0 * t)
    }
}

/// Undoes the Motion speed setting applied to `motion_dt`.
fn real_seconds(motion_dt: f32, speed: f32) -> f32 {
    if speed > 0.0 {
        motion_dt / speed
    } else {
        motion_dt
    }
}

/// Steps `playlist` in `direction`, passing over presets in `bad`. None if
/// it's empty or everything is bad.
fn step_skipping(
    playlist: &mut Playlist,
    direction: isize,
    bad: &HashSet<PathBuf>,
) -> Option<PathBuf> {
    for _ in 0..playlist.len() {
        let path = playlist.step(direction)?;
        if !bad.contains(path) {
            return Some(path.to_path_buf());
        }
    }
    None
}

/// The size projectM renders at for an output of `size`: the same, but no
/// taller than [`MAX_RENDER_HEIGHT`].
fn render_size(size: (u32, u32)) -> (u32, u32) {
    let (w, h) = (size.0.max(1), size.1.max(1));
    if h <= MAX_RENDER_HEIGHT {
        return (w, h);
    }
    let scale = MAX_RENDER_HEIGHT as f32 / h as f32;
    (
        ((w as f32 * scale).round() as u32).max(1),
        MAX_RENDER_HEIGHT,
    )
}

/// Whether the preset timer should move on: auto-change `enabled` (on and
/// not locked), the interval is up, and there's another preset to go to.
fn should_auto_change(enabled: bool, since_preset: f32, preset_secs: f32, presets: usize) -> bool {
    enabled && since_preset >= preset_secs && presets > 1
}

fn seed() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.subsec_nanos())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_size_passes_small_outputs_through() {
        assert_eq!(render_size((1280, 720)), (1280, 720));
        assert_eq!(render_size((1920, 1080)), (1920, 1080));
    }

    #[test]
    fn render_size_caps_4k_at_1080p_keeping_aspect() {
        assert_eq!(render_size((3840, 2160)), (1920, 1080));
        assert_eq!(render_size((5120, 1440)), (3840, 1080));
    }

    #[test]
    fn auto_change_waits_for_the_interval() {
        assert!(!should_auto_change(true, 29.9, 30.0, 5));
        assert!(should_auto_change(true, 30.0, 30.0, 5));
    }

    #[test]
    fn auto_change_never_fires_when_disabled_or_locked() {
        assert!(!should_auto_change(false, 999.0, 30.0, 5));
    }

    #[test]
    fn auto_change_needs_another_preset() {
        assert!(!should_auto_change(true, 999.0, 30.0, 1));
    }

    #[test]
    fn fade_finishes_after_its_length_with_eased_amounts() {
        let mut fade = Fade::new(2.0);
        assert_eq!(fade.amount(), 0.0);
        assert!(!fade.advance(1.0));
        assert!((fade.amount() - 0.5).abs() < 1e-6);
        assert!(fade.advance(1.0));
        assert_eq!(fade.amount(), 1.0);
    }

    #[test]
    fn the_timer_counts_real_seconds_whatever_the_motion_speed() {
        assert_eq!(real_seconds(0.2, 2.0), 0.1);
        assert_eq!(real_seconds(0.05, 0.25), 0.2);
        assert_eq!(real_seconds(0.1, 0.0), 0.1);
    }

    #[test]
    fn a_zero_length_fade_finishes_at_once() {
        assert!(Fade::new(0.0).advance(1.0 / 60.0));
    }

    #[test]
    fn stepping_passes_over_bad_presets() {
        let paths: Vec<PathBuf> = ["a", "b", "c"].iter().map(PathBuf::from).collect();
        let mut playlist = Playlist::new(paths, false, 1);
        let first = step_skipping(&mut playlist, 1, &HashSet::new()).unwrap();
        let bad: HashSet<PathBuf> = paths_after(&first, 1).into_iter().collect();
        let next = step_skipping(&mut playlist, 1, &bad).unwrap();
        assert_eq!(next, paths_after(&first, 2)[0]);
    }

    #[test]
    fn stepping_gives_none_when_every_preset_is_bad() {
        let paths: Vec<PathBuf> = ["a", "b"].iter().map(PathBuf::from).collect();
        let bad: HashSet<PathBuf> = paths.iter().cloned().collect();
        let mut playlist = Playlist::new(paths, false, 1);
        assert_eq!(step_skipping(&mut playlist, 1, &bad), None);
    }

    /// The preset `n` places after `from` in the sorted list a, b, c.
    fn paths_after(from: &Path, n: usize) -> Vec<PathBuf> {
        let names = ["a", "b", "c"];
        let at = names.iter().position(|&x| Path::new(x) == from).unwrap();
        vec![PathBuf::from(names[(at + n) % names.len()])]
    }

    #[test]
    fn render_size_never_hits_zero() {
        assert_eq!(render_size((0, 0)), (1, 1));
    }
}
