//! Frame-rate figures for the FPS counter, over one-second windows.
//!
//! An average alone hides dropped frames (59 smooth frames and one doubled
//! still read "60 fps"), so the worst frame time is reported too.

/// Length of each measuring window, in seconds.
const WINDOW_SECS: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameStats {
    /// Figures from the last full window: (frames per second, worst frame ms).
    report: (f32, f32),
    elapsed: f32,
    frames: u32,
    worst: f32,
}

impl Default for FrameStats {
    fn default() -> Self {
        Self {
            report: (0.0, 0.0),
            elapsed: 0.0,
            frames: 0,
            worst: 0.0,
        }
    }
}

impl FrameStats {
    /// Records a frame that took `dt` seconds.
    pub fn record(&mut self, dt: f32) {
        self.elapsed += dt;
        self.frames += 1;
        self.worst = self.worst.max(dt);
        if self.elapsed >= WINDOW_SECS {
            *self = Self {
                report: (self.frames as f32 / self.elapsed, self.worst * 1000.0),
                ..Self::default()
            };
        }
    }

    /// (frames per second, worst frame time in ms) over the last second.
    pub fn report(&self) -> (f32, f32) {
        self.report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_rate_and_worst_frame_after_a_second() {
        let mut stats = FrameStats::default();
        for _ in 0..59 {
            stats.record(1.0 / 60.0);
        }
        stats.record(2.0 / 60.0);
        let (fps, worst_ms) = stats.report();
        assert!((fps - 60.0 / 61.0 * 60.0).abs() < 0.1, "fps {fps}");
        assert!((worst_ms - 33.3).abs() < 0.1, "worst {worst_ms}");
    }

    #[test]
    fn keeps_the_previous_report_until_the_window_fills() {
        let mut stats = FrameStats::default();
        // 61, not 60: sixty f32 sixtieths sum to just under a second.
        for _ in 0..61 {
            stats.record(1.0 / 60.0);
        }
        let before = stats.report();
        stats.record(0.05);
        assert_eq!(stats.report(), before);
    }

    #[test]
    fn a_new_window_starts_fresh() {
        let mut stats = FrameStats::default();
        stats.record(0.5);
        stats.record(0.5);
        for _ in 0..61 {
            stats.record(1.0 / 60.0);
        }
        let (_, worst_ms) = stats.report();
        assert!((worst_ms - 16.7).abs() < 0.1, "worst {worst_ms}");
    }
}
