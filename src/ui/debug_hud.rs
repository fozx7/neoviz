//! Labels drawn over the analysis debug view (D): what each panel shows,
//! frequency and level scales, the loudest frequency, and every bar's Hz.

use std::f32::consts::FRAC_PI_2;

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, epaint::TextShape};

use crate::{
    audio::{
        analysis::{FFT_SIZE, LEVEL_EDGES, WAVEFORM_LEN},
        loudest::{Peak, note},
    },
    render::debug_view::{
        BARS_WIDTH, DISPLAY_DB_MIN, LABEL_STRIP_POINTS, SPEC_END, SPECTRUM_MIN_HZ, WAVE_END,
    },
};

use super::overlay::shadowed_text;

pub struct DebugLabels {
    pub sample_rate: u32,
    /// The bands' edges in Hz (one more than there are bars).
    pub band_edges: Vec<f32>,
    pub loudest: Option<Peak>,
}

const HEADING_SIZE: f32 = 14.0;
const LABEL_SIZE: f32 = 11.0;
const PAD: f32 = 6.0;
const TEXT: Color32 = Color32::from_gray(210);
const DIM: Color32 = Color32::from_gray(150);
const LOUDEST: Color32 = Color32::from_rgb(255, 210, 80);
/// Frequencies marked along the spectrum's axis.
const AXIS_HZ: [f32; 10] = [20.0, 50.0, 100.0, 200.0, 500.0, 1e3, 2e3, 5e3, 10e3, 20e3];
/// Level lines in the spectrum (the shader draws one every 20 dB).
const AXIS_DB_STEP: f32 = 20.0;

pub fn draw(painter: &Painter, screen: Rect, labels: &DebugLabels) {
    let wave_end = screen.top() + screen.height() * WAVE_END;
    let spec_end = screen.top() + screen.height() * SPEC_END;
    let wave = Rect::from_x_y_ranges(screen.x_range(), screen.top()..=wave_end);
    let spectrum = Rect::from_x_y_ranges(screen.x_range(), wave_end..=spec_end);
    let bands = Rect::from_x_y_ranges(screen.x_range(), spec_end..=screen.bottom());

    let rate = labels.sample_rate as f32;
    heading(
        painter,
        wave,
        &format!(
            "Waveform · last {WAVEFORM_LEN} samples ({:.0} ms) · {} capture",
            WAVEFORM_LEN as f32 / rate * 1000.0,
            hz_long(rate)
        ),
    );
    draw_spectrum(painter, spectrum, labels);
    draw_bands(painter, bands, &labels.band_edges);
}

fn draw_spectrum(painter: &Painter, area: Rect, labels: &DebugLabels) {
    let nyquist = labels.sample_rate as f32 * 0.5;
    heading(
        painter,
        area,
        &format!(
            "Spectrum · {} – {} · {FFT_SIZE}-point FFT, {:.1} Hz per bin",
            hz_long(SPECTRUM_MIN_HZ),
            hz_long(nyquist),
            labels.sample_rate as f32 / FFT_SIZE as f32
        ),
    );
    let x_of = |hz: f32| {
        area.left() + area.width() * (hz / SPECTRUM_MIN_HZ).ln() / (nyquist / SPECTRUM_MIN_HZ).ln()
    };
    for hz in AXIS_HZ.into_iter().filter(|&hz| hz <= nyquist) {
        let anchor = if hz == SPECTRUM_MIN_HZ {
            Align2::LEFT_BOTTOM
        } else {
            Align2::CENTER_BOTTOM
        };
        let pos = Pos2::new(x_of(hz).max(area.left() + PAD), area.bottom() - 2.0);
        label(painter, pos, anchor, &hz_short(hz), DIM);
    }
    let mut db = -AXIS_DB_STEP;
    while db > DISPLAY_DB_MIN {
        let y = area.bottom() - area.height() * (1.0 - db / DISPLAY_DB_MIN);
        label(
            painter,
            Pos2::new(area.left() + PAD, y),
            Align2::LEFT_BOTTOM,
            &format!("{db:.0} dB"),
            DIM,
        );
        db -= AXIS_DB_STEP;
    }

    let text = match labels.loudest {
        Some(peak) => {
            let x = x_of(peak.hz);
            painter.line_segment(
                [Pos2::new(x, area.top()), Pos2::new(x, area.bottom())],
                Stroke::new(1.0, LOUDEST.gamma_multiply(0.6)),
            );
            format!("Loudest: {}", describe_peak(peak))
        }
        None => "Loudest: (quiet)".to_owned(),
    };
    let pos = Pos2::new(area.right() - PAD, area.top() + PAD);
    shadowed_text(
        painter,
        pos,
        Align2::RIGHT_TOP,
        &text,
        HEADING_SIZE,
        LOUDEST,
        1.0,
    );
}

fn draw_bands(painter: &Painter, area: Rect, edges: &[f32]) {
    let (Some(&lo), Some(&hi)) = (edges.first(), edges.last()) else {
        return;
    };
    let count = edges.len() - 1;
    heading(
        painter,
        area,
        &format!(
            "{count} bands · {} – {} · {:.2} octave each · bar labels are centre Hz",
            hz_long(lo),
            hz_long(hi),
            (hi / lo).log2() / count as f32
        ),
    );
    let strip_bottom = area.bottom() - 3.0;
    let bars_w = area.width() * BARS_WIDTH;
    let slot_w = bars_w / count as f32;
    let font = FontId::proportional(LABEL_SIZE);
    // Labels run upwards, so each needs a line height of room across.
    let line_h = painter
        .layout_no_wrap("0".into(), font.clone(), TEXT)
        .size()
        .y;
    let stride = ((line_h + 1.0) / slot_w).ceil().max(1.0) as usize;
    for (i, w) in edges.windows(2).enumerate().step_by(stride) {
        let centre_x = area.left() + (i as f32 + 0.5) * slot_w;
        let text = hz_short((w[0] * w[1]).sqrt());
        let galley = painter.layout_no_wrap(text, font.clone(), TEXT);
        let pos = Pos2::new(centre_x - line_h * 0.5, strip_bottom);
        painter.add(TextShape::new(pos, galley, TEXT).with_angle(-FRAC_PI_2));
    }

    let meters = [
        ("RMS".to_owned(), "all".to_owned()),
        ("Bass".to_owned(), range(LEVEL_EDGES[0], LEVEL_EDGES[1])),
        ("Mid".to_owned(), range(LEVEL_EDGES[1], LEVEL_EDGES[2])),
        ("Treble".to_owned(), range(LEVEL_EDGES[2], LEVEL_EDGES[3])),
    ];
    let meter_w = (area.width() - bars_w) / meters.len() as f32;
    let strip_top = area.bottom() - LABEL_STRIP_POINTS;
    for (m, (name, span)) in meters.iter().enumerate() {
        let x = area.left() + bars_w + (m as f32 + 0.5) * meter_w;
        label(
            painter,
            Pos2::new(x, strip_top + 2.0),
            Align2::CENTER_TOP,
            name,
            TEXT,
        );
        label(
            painter,
            Pos2::new(x, strip_bottom),
            Align2::CENTER_BOTTOM,
            span,
            DIM,
        );
    }
}

fn heading(painter: &Painter, area: Rect, text: &str) {
    let pos = Pos2::new(area.left() + PAD, area.top() + PAD);
    shadowed_text(
        painter,
        pos,
        Align2::LEFT_TOP,
        text,
        HEADING_SIZE,
        TEXT,
        1.0,
    );
}

fn label(painter: &Painter, pos: Pos2, anchor: Align2, text: &str, colour: Color32) {
    shadowed_text(painter, pos, anchor, text, LABEL_SIZE, colour, 1.0);
}

fn range(lo: f32, hi: f32) -> String {
    format!("{}–{}", hz_short(lo), hz_short(hi))
}

/// e.g. "110 Hz · A2 +3¢ · -18 dB".
pub fn describe_peak(peak: Peak) -> String {
    let (name, cents) = note(peak.hz);
    format!(
        "{} · {name} {cents:+}¢ · {:.0} dB",
        hz_long(peak.hz),
        peak.db
    )
}

/// Compact Hz for axis labels: "47", "350", "1.2k", "16k".
pub fn hz_short(hz: f32) -> String {
    if hz < 999.5 {
        format!("{hz:.0}")
    } else if hz < 9_950.0 {
        trim_zero(format!("{:.1}", hz / 1000.0)) + "k"
    } else {
        format!("{:.0}k", hz / 1000.0)
    }
}

/// Hz with units: "47 Hz", "1.25 kHz", "16 kHz".
pub fn hz_long(hz: f32) -> String {
    if hz < 999.5 {
        format!("{hz:.0} Hz")
    } else {
        trim_zero(trim_zero(format!("{:.2}", hz / 1000.0))) + " kHz"
    }
}

/// Drops one trailing "0" after a decimal point, and the point if bare.
fn trim_zero(s: String) -> String {
    if !s.contains('.') {
        return s;
    }
    let s = s.strip_suffix('0').unwrap_or(&s);
    s.strip_suffix('.').unwrap_or(s).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_hz_labels_stay_compact() {
        assert_eq!(hz_short(20.0), "20");
        assert_eq!(hz_short(47.3), "47");
        assert_eq!(hz_short(999.0), "999");
        assert_eq!(hz_short(1000.0), "1k");
        assert_eq!(hz_short(1240.0), "1.2k");
        assert_eq!(hz_short(9_990.0), "10k");
        assert_eq!(hz_short(16_000.0), "16k");
    }

    #[test]
    fn long_hz_labels_have_units() {
        assert_eq!(hz_long(110.2), "110 Hz");
        assert_eq!(hz_long(1250.0), "1.25 kHz");
        assert_eq!(hz_long(1200.0), "1.2 kHz");
        assert_eq!(hz_long(16_000.0), "16 kHz");
        assert_eq!(hz_long(48_000.0), "48 kHz");
        assert_eq!(hz_long(44_100.0), "44.1 kHz");
    }

    #[test]
    fn describes_a_peak_with_its_note() {
        let text = describe_peak(Peak {
            hz: 440.0,
            db: -18.2,
        });
        assert_eq!(text, "440 Hz · A4 +0¢ · -18 dB");
    }
}
