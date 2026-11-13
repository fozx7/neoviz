//! Keeping the screen on while visualizing, like a video player does.
//!
//! On Linux this asks the desktop (org.freedesktop.ScreenSaver) not to blank
//! or lock the screen; on Windows it marks the display as required. Dropping
//! the guard withdraws the request.

use crate::config::KeepAwake;

/// In "while music plays" mode, how long silence lasts before the screen is
/// allowed to sleep again. Long enough to ride out gaps between songs.
pub const RELEASE_AFTER_SECS: f32 = 60.0;
/// How long to wait before trying again if the request failed.
const RETRY_SECS: f32 = 30.0;

/// Whether the screen should be kept on, given the mode and how long it's
/// been silent.
pub fn should_keep_on(mode: KeepAwake, silent_secs: f32) -> bool {
    match mode {
        KeepAwake::Off => false,
        KeepAwake::Always => true,
        KeepAwake::WhilePlaying => silent_secs < RELEASE_AFTER_SECS,
    }
}

#[derive(Default)]
pub struct Awake {
    guard: Option<keepawake::KeepAwake>,
    silent_secs: f32,
    retry_in: f32,
}

impl Awake {
    /// Call once per frame with whether the audio is currently silent.
    pub fn update(&mut self, mode: KeepAwake, silent: bool, dt: f32) {
        self.silent_secs = if silent { self.silent_secs + dt } else { 0.0 };
        self.retry_in = (self.retry_in - dt).max(0.0);

        let want = should_keep_on(mode, self.silent_secs);
        if !want {
            if self.guard.take().is_some() {
                log::info!("letting the screen sleep again");
            }
            return;
        }
        if self.guard.is_some() || self.retry_in > 0.0 {
            return;
        }
        match keepawake::Builder::default()
            .display(true)
            .reason("Showing music visualizations")
            .app_name("neoViz")
            .app_reverse_domain("io.github.fozx7.neoviz")
            .create()
        {
            Ok(guard) => {
                log::info!("keeping the screen on");
                self.guard = Some(guard);
            }
            Err(e) => {
                log::warn!("couldn't keep the screen on: {e}");
                self.retry_in = RETRY_SECS;
            }
        }
    }

    pub fn is_holding(&self) -> bool {
        self.guard.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_never_keeps_the_screen_on() {
        assert!(!should_keep_on(KeepAwake::Off, 0.0));
    }

    #[test]
    fn always_keeps_it_on_even_in_long_silence() {
        assert!(should_keep_on(KeepAwake::Always, 3600.0));
    }

    #[test]
    fn while_playing_rides_out_gaps_between_songs() {
        assert!(should_keep_on(KeepAwake::WhilePlaying, 0.0));
        assert!(should_keep_on(KeepAwake::WhilePlaying, 10.0));
    }

    #[test]
    fn while_playing_lets_go_after_a_long_silence() {
        assert!(!should_keep_on(KeepAwake::WhilePlaying, RELEASE_AFTER_SECS));
    }

    #[test]
    fn off_releases_without_touching_the_system() {
        let mut a = Awake::default();
        a.update(KeepAwake::Off, false, 0.016);
        assert!(!a.is_holding());
    }
}
