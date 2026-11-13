//! Visualizers and switching between them.
//!
//! Adding a visualizer: implement [`Visualizer`] in its own file (plus a
//! shader if it needs one) and add it to [`create_all`].

mod bars;
mod color;
mod scope;

use crate::audio::analysis::AudioFrame;

/// How long a crossfade between visualizers lasts.
pub const CROSSFADE_SECS: f32 = 1.0;

pub trait Visualizer {
    fn name(&self) -> &str;

    /// Called when the output size changes, with the new size in pixels.
    fn resize(&mut self, _device: &wgpu::Device, _size: (u32, u32)) {}

    /// Advances animation state by `dt` seconds using this frame's audio.
    fn update(&mut self, audio: &AudioFrame, dt: f32);

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

/// Every built-in visualizer, in rotation order.
pub fn create_all(device: &wgpu::Device, size: (u32, u32)) -> Vec<Box<dyn Visualizer>> {
    use bars::{Bars, BarsVariant};
    use scope::{Scope, ScopeVariant};
    vec![
        Box::new(Bars::new(device, BarsVariant::Solid)),
        Box::new(Scope::new(device, size, ScopeVariant::Line)),
        Box::new(Bars::new(device, BarsVariant::Led)),
        Box::new(Scope::new(device, size, ScopeVariant::Ring)),
        Box::new(Bars::new(device, BarsVariant::Mirror)),
        Box::new(Scope::new(device, size, ScopeVariant::StereoXy)),
    ]
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

    pub fn next(&mut self) {
        self.select((self.current + 1) % self.count);
    }

    pub fn previous(&mut self) {
        self.select((self.current + self.count - 1) % self.count);
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

    #[test]
    fn next_and_previous_wrap() {
        let mut s = Switcher::new(3, 2);
        s.next();
        assert_eq!(s.current(), 0);
        s.previous();
        s.previous();
        assert_eq!(s.current(), 1);
    }

    #[test]
    fn switching_crossfades_over_one_second() {
        let mut s = Switcher::new(3, 0);
        s.next();
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
        s.next(); // 0 -> 1
        s.advance(0.3);
        s.next(); // 1 -> 2
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
        s.next();
        assert_eq!(s.outgoing(), None);
    }
}
