//! The FPS counter in the top-right corner: just the frame rate, or (with
//! "Detailed" on) lows, frame times, where the time goes, and a graph.

use egui::{Align2, Color32, Pos2, Rect, Stroke, vec2};

use crate::app::frame_stats::{Detail, GRAPH_FRAMES, LATE_FACTOR};

use super::overlay::shadowed_text;

pub struct FpsHud {
    /// Frames per second over the last second.
    pub fps: f32,
    pub detail: Option<FpsDetail>,
}

pub struct FpsDetail {
    /// What's on screen: the visualizer, plus its preset if it has one.
    pub showing: String,
    pub figures: Detail,
    /// Recent frame times in ms, oldest first.
    pub recent_ms: Vec<f32>,
    /// Capture rate and the loudest frequency right now.
    pub audio: String,
}

const MARGIN: f32 = 12.0;
const TEXT_SIZE: f32 = 16.0;
const DETAIL_TEXT_SIZE: f32 = 14.0;
const LINE_HEIGHT: f32 = 18.0;
const GRAPH_HEIGHT: f32 = 48.0;
/// The graph's top is this many typical frames, so a doubled frame reaches
/// two thirds of the way up.
const GRAPH_SCALE_FRAMES: f32 = 3.0;
const OK_COLOUR: Color32 = Color32::LIGHT_GREEN;
const LATE_COLOUR: Color32 = Color32::from_rgb(255, 140, 60);

pub fn draw(painter: &egui::Painter, screen: Rect, hud: &FpsHud) {
    let right = screen.right() - MARGIN;
    let mut y = screen.top() + 8.0;
    let Some(detail) = &hud.detail else {
        let text = format!("{:.0} fps", hud.fps);
        line(painter, right, y, &text, TEXT_SIZE);
        return;
    };
    let d = &detail.figures;
    let lines = [
        format!(
            "{:.0} fps · 1% low {:.0} · 5% low {:.0}",
            d.fps, d.low_1, d.low_5
        ),
        format!(
            "frame {:.1} ms avg · {:.1} p99 · {:.1} max",
            d.avg_ms, d.p99_ms, d.max_ms
        ),
        format!(
            "work {:.1} ms · waiting on display {:.1} ms",
            d.work_ms, d.wait_ms
        ),
        format!("late frames {} in last {:.0} s", d.late, d.secs),
        detail.showing.clone(),
        detail.audio.clone(),
    ];
    for (i, text) in lines.iter().enumerate() {
        let size = if i == 0 { TEXT_SIZE } else { DETAIL_TEXT_SIZE };
        line(painter, right, y, text, size);
        y += LINE_HEIGHT;
    }
    graph(painter, right, y + 4.0, &detail.recent_ms, d.typical_ms);
}

fn line(painter: &egui::Painter, right: f32, y: f32, text: &str, size: f32) {
    shadowed_text(
        painter,
        Pos2::new(right, y),
        Align2::RIGHT_TOP,
        text,
        size,
        OK_COLOUR,
        1.0,
    );
}

/// One bar per frame (newest on the right), late ones in orange, with a
/// line at the typical frame time.
fn graph(painter: &egui::Painter, right: f32, top: f32, recent_ms: &[f32], typical_ms: f32) {
    let width = GRAPH_FRAMES as f32;
    let area = Rect::from_min_size(Pos2::new(right - width, top), vec2(width, GRAPH_HEIGHT));
    painter.rect_filled(area, 2.0, Color32::from_black_alpha(140));
    if typical_ms <= 0.0 {
        return;
    }
    let top_ms = typical_ms * GRAPH_SCALE_FRAMES;
    let height_of = |ms: f32| (ms / top_ms).min(1.0) * GRAPH_HEIGHT;
    let start = area.right() - recent_ms.len() as f32;
    for (i, &ms) in recent_ms.iter().enumerate() {
        let x = start + i as f32 + 0.5;
        let colour = if ms > typical_ms * LATE_FACTOR {
            LATE_COLOUR
        } else {
            OK_COLOUR.gamma_multiply(0.7)
        };
        painter.line_segment(
            [
                Pos2::new(x, area.bottom()),
                Pos2::new(x, area.bottom() - height_of(ms)),
            ],
            Stroke::new(1.0, colour),
        );
    }
    let ref_y = area.bottom() - height_of(typical_ms);
    painter.line_segment(
        [
            Pos2::new(area.left(), ref_y),
            Pos2::new(area.right(), ref_y),
        ],
        Stroke::new(1.0, Color32::from_white_alpha(110)),
    );
}
