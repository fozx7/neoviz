//! Visualizers and switching between them.
//!
//! Adding a visualizer: implement [`Visualizer`] in its own file (plus a
//! shader if it needs one) and add it to [`create_all`].

mod bars;
mod color;
mod common;
mod layered;
mod lightshow;
#[cfg(milkdrop)]
mod milkdrop;
mod particles;
mod scope;
mod starfield;

use crate::{audio::analysis::AudioFrame, config::Settings};

pub use common::Rng;

/// How long a crossfade between visualizers lasts.
pub const CROSSFADE_SECS: f32 = 1.0;

pub trait Visualizer {
    fn name(&self) -> &str;

    /// Called when the output size changes, with the new size in pixels.
    fn resize(&mut self, _device: &wgpu::Device, _size: (u32, u32)) {}

    /// Advances animation state by `dt` seconds using this frame's audio.
    fn update(&mut self, audio: &AudioFrame, dt: f32);

    /// Picks up settings at startup and whenever they change. `rescan` asks
    /// for external resources (e.g. preset folders) to be reloaded.
    fn configure(&mut self, _settings: &Settings, _rescan: bool) {}

    /// Steps through this visualizer's own presets, if it has any. Returns
    /// text to show (e.g. the new preset's name).
    fn step_preset(&mut self, _direction: isize) -> Option<String> {
        None
    }

    /// The user locked (`Space`) or unlocked the current visualizer. While
    /// locked, a visualizer with its own presets must not change them on a timer.
    fn set_locked(&mut self, _locked: bool) {}

    /// A message to show as a toast (e.g. "switched to preset X"), taken once.
    fn take_message(&mut self) -> Option<String> {
        None
    }

    /// A status line for the settings panel, if there's something to say.
    fn status(&self) -> Option<String> {
        None
    }

    /// Records commands that draw the current state into `target`
    /// (an [`HDR_FORMAT`](crate::render::common::HDR_FORMAT) texture of `size`),
    /// replacing whatever it held.
    fn render(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
    );
}

/// Every built-in visualizer, in rotation order: the seven main ones first,
/// then the alternate variants.
pub fn create_all(device: &wgpu::Device, size: (u32, u32)) -> Vec<Box<dyn Visualizer>> {
    use bars::{Bars, BarsVariant};
    use layered::{Kind, Layered};
    use scope::{Scope, ScopeVariant};
    let mut all: Vec<Box<dyn Visualizer>> = vec![
        Box::new(Bars::new(device, BarsVariant::Solid)),
        Box::new(Scope::new(device, size, ScopeVariant::Line)),
        Box::new(Layered::new(device, size, Kind::AmbientSwirl)),
        Box::new(Layered::new(device, size, Kind::BatteryRings)),
        Box::new(particles::Particles::new(device, size)),
        Box::new(lightshow::Lightshow::new(device, size)),
        Box::new(starfield::Starfield::new(device, size)),
    ];
    #[cfg(milkdrop)]
    match milkdrop::MilkDrop::new(device, size) {
        Ok(m) => all.push(Box::new(m)),
        Err(e) => log::error!("MilkDrop unavailable: {e:#}"),
    }
    all.extend([
        Box::new(Bars::new(device, BarsVariant::Led)) as Box<dyn Visualizer>,
        Box::new(Scope::new(device, size, ScopeVariant::Ring)),
        Box::new(Bars::new(device, BarsVariant::Mirror)),
        Box::new(Scope::new(device, size, ScopeVariant::StereoXy)),
    ]);
    all
}

/// Tracks which visualizer is showing and any crossfade in progress.
#[derive(Debug, Clone, PartialEq)]
pub struct Switcher {
    count: usize,
    current: usize,
    fade: Option<Fade>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Fade {
    from: usize,
    /// 0..1 through the crossfade.
    progress: f32,
}

impl Switcher {
    pub fn new(count: usize, start: usize) -> Self {
        assert!(count > 0 && start < count);
        Self {
            count,
            current: start,
            fade: None,
        }
    }

    pub fn current(&self) -> usize {
        self.current
    }

    /// The visualizer fading out, if a crossfade is running.
    pub fn outgoing(&self) -> Option<usize> {
        self.fade.map(|f| f.from)
    }

    /// How much of the current visualizer to show (0..1), eased.
    pub fn mix(&self) -> f32 {
        self.fade.map_or(1.0, |f| smoothstep(f.progress))
    }

    /// Moves `direction` (+1 next, -1 previous) to the nearest visualizer in
    /// rotation. `in_rotation[i]` says whether visualizer `i` is included;
    /// if none are, all are.
    pub fn step(&mut self, direction: isize, in_rotation: &[bool]) {
        let n = self.count as isize;
        let target = (1..n)
            .map(|k| (self.current as isize + direction.signum() * k).rem_euclid(n) as usize)
            .find(|&i| is_included(in_rotation, i));
        if let Some(i) = target {
            self.select(i);
        }
    }

    /// Switches to a random visualizer in rotation other than the current one.
    pub fn random(&mut self, in_rotation: &[bool], rng: &mut Rng) {
        let choices: Vec<usize> = (0..self.count)
            .filter(|&i| i != self.current && is_included(in_rotation, i))
            .collect();
        if !choices.is_empty() {
            self.select(choices[rng.next_u32() as usize % choices.len()]);
        }
    }

    /// Starts a crossfade to `index`. Switching mid-fade fades from whatever
    /// was the target, so at most two visualizers are ever live.
    pub fn select(&mut self, index: usize) {
        if index == self.current || index >= self.count {
            return;
        }
        self.fade = Some(Fade {
            from: self.current,
            progress: 0.0,
        });
        self.current = index;
    }

    pub fn advance(&mut self, dt: f32) {
        if let Some(fade) = &mut self.fade {
            fade.progress += dt / CROSSFADE_SECS;
            if fade.progress >= 1.0 {
                self.fade = None;
            }
        }
    }
}

fn is_included(in_rotation: &[bool], index: usize) -> bool {
    !in_rotation.iter().any(|&on| on) || in_rotation.get(index).copied().unwrap_or(false)
}

/// Beat intensity that counts as "strong" for switching early.
pub const STRONG_BEAT: f32 = 0.8;

/// Decides when random mode switches visualizer.
#[derive(Debug, Clone, Default)]
pub struct AutoSwitch {
    elapsed: f32,
}

impl AutoSwitch {
    /// Restarts the countdown (e.g. after a manual switch).
    pub fn reset(&mut self) {
        self.elapsed = 0.0;
    }

    /// Advances by `dt`. Returns true when it's time to switch: after
    /// `interval` seconds, or (with `on_beat`) on a strong beat once at least
    /// half the interval has passed.
    pub fn tick(&mut self, dt: f32, interval: f32, on_beat: bool, beat: Option<f32>) -> bool {
        self.elapsed += dt;
        let strong_beat = on_beat
            && self.elapsed >= interval * 0.5
            && beat.is_some_and(|intensity| intensity >= STRONG_BEAT);
        let due = self.elapsed >= interval || strong_beat;
        if due {
            self.elapsed = 0.0;
        }
        due
    }
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_settled() {
        let s = Switcher::new(3, 1);
        assert_eq!(s.current(), 1);
        assert_eq!(s.outgoing(), None);
        assert_eq!(s.mix(), 1.0);
    }

    const ALL: [bool; 0] = [];

    #[test]
    fn next_and_previous_wrap() {
        let mut s = Switcher::new(3, 2);
        s.step(1, &ALL);
        assert_eq!(s.current(), 0);
        s.step(-1, &ALL);
        s.step(-1, &ALL);
        assert_eq!(s.current(), 1);
    }

    #[test]
    fn stepping_skips_visualizers_out_of_rotation() {
        let mut s = Switcher::new(4, 0);
        let rotation = [true, false, false, true];
        s.step(1, &rotation);
        assert_eq!(s.current(), 3);
        s.step(1, &rotation);
        assert_eq!(s.current(), 0);
    }

    #[test]
    fn empty_rotation_means_everything() {
        let mut s = Switcher::new(3, 0);
        s.step(1, &[false, false, false]);
        assert_eq!(s.current(), 1);
    }

    #[test]
    fn random_picks_another_one_in_rotation() {
        let mut rng = Rng::new(3);
        let rotation = [true, false, true, true];
        for _ in 0..50 {
            let mut s = Switcher::new(4, 0);
            s.random(&rotation, &mut rng);
            assert!(s.current() == 2 || s.current() == 3);
        }
    }

    #[test]
    fn random_with_nothing_else_stays_put() {
        let mut s = Switcher::new(3, 1);
        s.random(&[false, true, false], &mut Rng::new(1));
        assert_eq!(s.current(), 1);
        assert_eq!(s.outgoing(), None);
    }

    #[test]
    fn switching_crossfades_over_one_second() {
        let mut s = Switcher::new(3, 0);
        s.step(1, &ALL);
        assert_eq!(s.outgoing(), Some(0));
        assert_eq!(s.mix(), 0.0);

        s.advance(CROSSFADE_SECS / 2.0);
        assert!((s.mix() - 0.5).abs() < 1e-6);

        s.advance(CROSSFADE_SECS / 2.0 + 0.01);
        assert_eq!(s.outgoing(), None);
        assert_eq!(s.mix(), 1.0);
    }

    #[test]
    fn switching_mid_fade_restarts_from_the_previous_target() {
        let mut s = Switcher::new(4, 0);
        s.step(1, &ALL); // 0 -> 1
        s.advance(0.3);
        s.step(1, &ALL); // 1 -> 2
        assert_eq!(s.current(), 2);
        assert_eq!(s.outgoing(), Some(1));
        assert_eq!(s.mix(), 0.0);
    }

    #[test]
    fn selecting_the_current_one_does_nothing() {
        let mut s = Switcher::new(2, 0);
        s.select(0);
        s.select(7);
        assert_eq!(s.outgoing(), None);
    }

    #[test]
    fn single_visualizer_never_fades() {
        let mut s = Switcher::new(1, 0);
        s.step(1, &ALL);
        assert_eq!(s.outgoing(), None);
    }

    #[test]
    fn auto_switch_fires_after_the_interval() {
        let mut a = AutoSwitch::default();
        assert!(!a.tick(9.0, 10.0, false, None));
        assert!(a.tick(1.0, 10.0, false, None));
        assert!(!a.tick(1.0, 10.0, false, None), "countdown restarts");
    }

    #[test]
    fn strong_beats_switch_early_but_not_too_early() {
        let mut a = AutoSwitch::default();
        assert!(
            !a.tick(2.0, 10.0, true, Some(1.0)),
            "before half the interval"
        );
        assert!(
            !a.tick(4.0, 10.0, true, Some(STRONG_BEAT - 0.1)),
            "weak beat"
        );
        assert!(a.tick(0.0, 10.0, true, Some(STRONG_BEAT)));
    }

    #[test]
    fn beats_are_ignored_unless_enabled() {
        let mut a = AutoSwitch::default();
        assert!(!a.tick(6.0, 10.0, false, Some(1.0)));
    }
}
