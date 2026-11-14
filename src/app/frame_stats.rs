//! Frame-rate figures for the FPS counter.
//!
//! The simple counter is the frame rate over the last second. The detailed
//! one looks at the last [`HISTORY_SECS`]: an average alone hides dropped
//! frames (59 smooth frames and one doubled still read "60 fps"), so it also
//! reports the lows, frame-time spread, late frames, and how each frame's
//! time splits between our own work and waiting on the display.

use std::collections::VecDeque;

/// Length of the simple counter's window, in seconds.
const SIMPLE_WINDOW_SECS: f32 = 1.0;
/// How far back the detailed figures look.
const HISTORY_SECS: f32 = 10.0;
/// How often the detailed figures are recomputed (they'd flicker otherwise).
const DETAIL_EVERY_SECS: f32 = 0.5;
/// A frame counts as late when it takes this much longer than the median
/// (i.e. it missed a refresh and the previous picture stayed up).
pub const LATE_FACTOR: f32 = 1.5;
/// Frames shown in the frame-time graph.
pub const GRAPH_FRAMES: usize = 240;

/// One frame's timings, in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameTiming {
    /// Time since the previous frame started.
    pub dt: f32,
    /// Time spent on our own work (analysis, UI, recording draw commands).
    pub work: f32,
    /// Time spent blocked waiting for the display (acquiring and presenting).
    pub wait: f32,
}

/// The detailed figures, over the last [`HISTORY_SECS`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Detail {
    pub fps: f32,
    /// Frame rate over the slowest 1% / 5% of frames.
    pub low_1: f32,
    pub low_5: f32,
    /// Frame times in ms: mean, 99th percentile, worst.
    pub avg_ms: f32,
    pub p99_ms: f32,
    pub max_ms: f32,
    /// The median frame time in ms: one refresh, when keeping up.
    pub typical_ms: f32,
    /// Mean ms per frame of our own work, and of waiting on the display.
    pub work_ms: f32,
    pub wait_ms: f32,
    /// Frames that missed a refresh.
    pub late: usize,
    /// Seconds actually covered (less than [`HISTORY_SECS`] at first).
    pub secs: f32,
}

#[derive(Debug, Clone, Default)]
pub struct FrameStats {
    history: VecDeque<FrameTiming>,
    history_secs: f32,
    /// Frames per second over the last full simple window.
    fps: f32,
    window_secs: f32,
    window_frames: u32,
    detail: Detail,
    since_detail: f32,
}

impl FrameStats {
    pub fn record(&mut self, timing: FrameTiming) {
        self.window_secs += timing.dt;
        self.window_frames += 1;
        if self.window_secs >= SIMPLE_WINDOW_SECS {
            self.fps = self.window_frames as f32 / self.window_secs;
            self.window_secs = 0.0;
            self.window_frames = 0;
        }

        self.history.push_back(timing);
        self.history_secs += timing.dt;
        while self.history_secs > HISTORY_SECS
            && let Some(old) = self.history.pop_front()
        {
            self.history_secs -= old.dt;
        }

        self.since_detail += timing.dt;
        if self.since_detail >= DETAIL_EVERY_SECS {
            self.since_detail = 0.0;
            self.detail = detail(&self.history);
        }
    }

    /// Frames per second over the last second.
    pub fn fps(&self) -> f32 {
        self.fps
    }

    /// The detailed figures (refreshed twice a second).
    pub fn detail(&self) -> &Detail {
        &self.detail
    }

    /// The most recent frame times in ms, oldest first, for the graph.
    pub fn recent_ms(&self) -> Vec<f32> {
        let skip = self.history.len().saturating_sub(GRAPH_FRAMES);
        self.history
            .iter()
            .skip(skip)
            .map(|t| t.dt * 1000.0)
            .collect()
    }
}

fn detail(history: &VecDeque<FrameTiming>) -> Detail {
    let n = history.len();
    if n == 0 {
        return Detail::default();
    }
    let secs: f32 = history.iter().map(|t| t.dt).sum();
    let mut slowest_first: Vec<f32> = history.iter().map(|t| t.dt).collect();
    slowest_first.sort_by(|a, b| b.total_cmp(a));
    let median = slowest_first[n / 2];
    let mean = |f: fn(&FrameTiming) -> f32| history.iter().map(f).sum::<f32>() / n as f32;
    Detail {
        fps: rate(secs / n as f32),
        low_1: rate(mean_of_slowest(&slowest_first, 0.01)),
        low_5: rate(mean_of_slowest(&slowest_first, 0.05)),
        avg_ms: secs / n as f32 * 1000.0,
        p99_ms: slowest_first[n / 100] * 1000.0,
        max_ms: slowest_first[0] * 1000.0,
        typical_ms: median * 1000.0,
        work_ms: mean(|t| t.work) * 1000.0,
        wait_ms: mean(|t| t.wait) * 1000.0,
        late: slowest_first
            .iter()
            .take_while(|&&dt| dt > median * LATE_FACTOR)
            .count(),
        secs,
    }
}

/// Mean of the slowest `fraction` of frame times (at least one frame).
/// `slowest_first` must be sorted slowest first.
fn mean_of_slowest(slowest_first: &[f32], fraction: f32) -> f32 {
    let count = ((slowest_first.len() as f32 * fraction).ceil() as usize).max(1);
    let slice = &slowest_first[..count.min(slowest_first.len())];
    slice.iter().sum::<f32>() / slice.len() as f32
}

/// Frames per second for a frame time in seconds.
fn rate(secs: f32) -> f32 {
    if secs > 0.0 { 1.0 / secs } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: f32 = 1.0 / 60.0;

    fn steady(dt: f32) -> FrameTiming {
        FrameTiming {
            dt,
            work: 0.004,
            wait: dt - 0.004,
        }
    }

    /// Ten seconds at 60 fps, with `hitches` doubled frames spread through.
    fn run_with_hitches(hitches: usize) -> FrameStats {
        let mut stats = FrameStats::default();
        for i in 0..600 {
            let dt = if i % 60 == 30 && i / 60 < hitches {
                2.0 * FRAME
            } else {
                FRAME
            };
            stats.record(steady(dt));
        }
        stats
    }

    #[test]
    fn steady_frames_have_lows_equal_to_the_average() {
        let d = *run_with_hitches(0).detail();
        assert!((d.fps - 60.0).abs() < 0.5, "{d:?}");
        assert!((d.low_1 - 60.0).abs() < 0.5, "{d:?}");
        assert!((d.low_5 - 60.0).abs() < 0.5, "{d:?}");
        assert_eq!(d.late, 0);
    }

    #[test]
    fn a_few_doubled_frames_show_in_the_lows_and_late_count() {
        let d = *run_with_hitches(6).detail();
        // The average barely moves...
        assert!(d.fps > 59.0, "{d:?}");
        // ...but the slowest 1% (6 frames) are all the 30 fps hitches.
        assert!((d.low_1 - 30.0).abs() < 0.5, "{d:?}");
        assert!(d.low_5 > 30.0 && d.low_5 < 60.0, "{d:?}");
        assert!((d.max_ms - 33.3).abs() < 0.1, "{d:?}");
        assert_eq!(d.late, 6);
    }

    #[test]
    fn work_and_wait_are_averaged_per_frame() {
        let d = *run_with_hitches(0).detail();
        assert!((d.work_ms - 4.0).abs() < 0.01, "{d:?}");
        assert!((d.wait_ms - (FRAME * 1000.0 - 4.0)).abs() < 0.01, "{d:?}");
    }

    #[test]
    fn history_keeps_only_the_last_ten_seconds() {
        let mut stats = run_with_hitches(6);
        // Another 10 s of smooth frames pushes every hitch out.
        for _ in 0..610 {
            stats.record(steady(FRAME));
        }
        let d = *stats.detail();
        assert_eq!(d.late, 0, "{d:?}");
        assert!(d.secs <= HISTORY_SECS + FRAME, "{d:?}");
    }

    #[test]
    fn simple_rate_is_over_the_last_second() {
        let stats = run_with_hitches(0);
        assert!((stats.fps() - 60.0).abs() < 1.0);
    }

    #[test]
    fn graph_gets_the_latest_frames_in_ms() {
        let stats = run_with_hitches(0);
        let recent = stats.recent_ms();
        assert_eq!(recent.len(), GRAPH_FRAMES);
        assert!((recent[0] - 16.67).abs() < 0.01);
    }

    #[test]
    fn slowest_mean_takes_at_least_one_frame() {
        assert_eq!(mean_of_slowest(&[0.05, 0.01, 0.01], 0.01), 0.05);
    }
}
