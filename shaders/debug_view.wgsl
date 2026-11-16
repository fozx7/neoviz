// Debug view, top to bottom:
//   1. waveform
//   2. raw log-frequency spectrum (dBFS)
//   3. smoothed bands with peak caps, plus RMS / bass / mid / treble meters.
//      The panel background flashes on beats and dims when silent.
//
// `data` rows: 0 waveform (-1..1), 1 spectrum (dBFS), 2 bands (0..1),
// 3 band peaks (0..1), 4 levels [rms, bass, mid, treble, peak].

struct Params {
    resolution: vec2f,
    sample_rate: f32,
    fft_size: f32,
    wave_len: f32,
    bins: f32,
    db_min: f32,     // bottom of the spectrum display, e.g. -90 dB
    band_count: f32,
    flash: f32,      // beat flash, 0..1
    silent: f32,     // 1.0 when silent
    // Layout (set from Rust, which draws the labels over it):
    wave_end: f32,   // fraction of the height where the waveform panel ends
    spec_end: f32,   // ... and the spectrum panel
    bars_width: f32, // fraction of the bands panel's width for the bars
    label_strip: f32, // pixels under the bars kept clear for labels
    min_hz: f32,     // left edge of the spectrum's log axis
    _pad: f32,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var data: texture_2d<f32>;

const ROW_WAVE: i32 = 0;
const ROW_SPEC: i32 = 1;
const ROW_BANDS: i32 = 2;
const ROW_PEAKS: i32 = 3;
const ROW_LEVELS: i32 = 4;

const MAX_SPAN: i32 = 256; // cap on samples/bins scanned per pixel column
const BG: vec3f = vec3f(0.015, 0.015, 0.03);
const GRID: vec3f = vec3f(0.07, 0.07, 0.11);
const DIVIDER: vec3f = vec3f(0.25, 0.25, 0.25);
const WAVE: vec3f = vec3f(0.3, 1.0, 0.6);
const SPEC_LOW: vec3f = vec3f(0.1, 0.3, 1.0);
const SPEC_HIGH: vec3f = vec3f(1.0, 0.35, 0.8);
const BAR_LOW: vec3f = vec3f(0.1, 0.8, 0.3);
const BAR_HIGH: vec3f = vec3f(1.0, 0.2, 0.1);
const PEAK_CAP: vec3f = vec3f(1.0, 1.0, 1.0);
const FLASH: vec3f = vec3f(0.35, 0.2, 0.05);
const SILENT_TINT: vec3f = vec3f(0.04, 0.04, 0.08);

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    // One oversized triangle covering the screen.
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn load(row: i32, i: i32, len: f32) -> f32 {
    return textureLoad(data, vec2i(clamp(i, 0, i32(len) - 1), row), 0).r;
}

// Antialiased coverage for a pixel `d` pixels away from a 1.5px-wide line.
fn line(d: f32) -> f32 {
    return 1.0 - smoothstep(0.75, 1.75, d);
}

// --- Panel 1: waveform -------------------------------------------------------

fn draw_wave(px: vec2f, size: vec2f) -> vec3f {
    let center = size.y * 0.5;
    let amp = size.y * 0.45;
    let scale = (p.wave_len - 1.0) / size.x;
    let i0 = i32(floor(px.x * scale));
    let i1 = min(i32(ceil((px.x + 1.0) * scale)), i0 + MAX_SPAN);

    // Span every sample this column covers so steep edges stay connected.
    var lo = load(ROW_WAVE, i0, p.wave_len);
    var hi = lo;
    for (var j = i0 + 1; j <= i1; j++) {
        let v = load(ROW_WAVE, j, p.wave_len);
        lo = min(lo, v);
        hi = max(hi, v);
    }
    let y_top = center - hi * amp;
    let y_bottom = center - lo * amp;
    let d = max(max(y_top - px.y, px.y - y_bottom), 0.0);

    var col = mix(BG, GRID, line(abs(px.y - center)));
    return mix(col, WAVE, line(d));
}

// --- Panel 2: raw spectrum ---------------------------------------------------

// Fractional FFT bin at horizontal pixel `x` on a log axis from MIN_HZ to Nyquist.
fn bin_at(x: f32, w: f32) -> f32 {
    let nyquist = p.sample_rate * 0.5;
    let hz = p.min_hz * pow(nyquist / p.min_hz, x / w);
    return hz / p.sample_rate * p.fft_size;
}

fn x_of_hz(hz: f32, w: f32) -> f32 {
    let nyquist = p.sample_rate * 0.5;
    return w * log(hz / p.min_hz) / log(nyquist / p.min_hz);
}

fn draw_spectrum(px: vec2f, size: vec2f) -> vec3f {
    let b0 = bin_at(px.x, size.x);
    let b1 = bin_at(px.x + 1.0, size.x);

    var level: f32;
    if (b1 - b0 < 1.0) {
        // Low frequencies: several pixels per bin, so interpolate.
        let i = i32(floor(b0));
        level = mix(load(ROW_SPEC, i, p.bins), load(ROW_SPEC, i + 1, p.bins), fract(b0));
    } else {
        // High frequencies: several bins per pixel, so take the loudest.
        let i0 = i32(floor(b0));
        let i1 = min(i32(ceil(b1)), i0 + MAX_SPAN);
        level = load(ROW_SPEC, i0, p.bins);
        for (var j = i0 + 1; j <= i1; j++) {
            level = max(level, load(ROW_SPEC, j, p.bins));
        }
    }

    let norm = clamp((level - p.db_min) / -p.db_min, 0.0, 1.0);
    let height_frac = 1.0 - px.y / size.y; // 0 at bottom, 1 at top

    var col = BG;
    // Horizontal grid every 20 dB.
    let db_here = p.db_min * (1.0 - height_frac);
    let db_step = abs(db_here - 20.0 * round(db_here / 20.0)) * size.y / -p.db_min;
    col = mix(col, GRID, line(db_step));
    // Vertical grid at 100 Hz, 1 kHz, 10 kHz.
    for (var hz = 100.0; hz < p.sample_rate * 0.5; hz *= 10.0) {
        col = mix(col, GRID, line(abs(px.x - x_of_hz(hz, size.x))));
    }

    if (height_frac <= norm) {
        col = mix(SPEC_LOW, SPEC_HIGH, height_frac);
    }
    return col;
}

// --- Panel 3: bands and level meters -----------------------------------------

// A vertical bar filling `slot` (x range in pixels) up to `value`, with a 1px gap.
fn bar(px: vec2f, height: f32, slot: vec2f, value: f32, low: vec3f, high: vec3f, bg: vec3f) -> vec3f {
    let gap = select(0.0, 1.0, slot.y - slot.x > 3.0);
    if (px.x < slot.x + gap || px.x >= slot.y) {
        return bg;
    }
    let height_frac = 1.0 - px.y / height;
    if (height_frac <= value) {
        return mix(low, high, height_frac);
    }
    return bg;
}

fn draw_bands(px: vec2f, size: vec2f) -> vec3f {
    var bg = BG + FLASH * p.flash;
    if (p.silent > 0.5) {
        bg = SILENT_TINT;
    }

    // The strip along the bottom holds the labels.
    let bars_h = max(size.y - p.label_strip, 1.0);
    if (px.y >= bars_h) {
        return BG;
    }

    let bars_w = size.x * p.bars_width;
    if (px.x < bars_w) {
        let slot_w = bars_w / p.band_count;
        let i = i32(floor(px.x / slot_w));
        let slot = vec2f(f32(i) * slot_w, f32(i + 1) * slot_w);
        var col = bar(px, bars_h, slot, load(ROW_BANDS, i, p.band_count), BAR_LOW, BAR_HIGH, bg);

        // Peak cap: a 2px line at the peak height.
        let peak_y = (1.0 - load(ROW_PEAKS, i, p.band_count)) * bars_h;
        if (px.x >= slot.x + 1.0 && abs(px.y - peak_y) < 1.0) {
            col = PEAK_CAP;
        }
        return col;
    }

    // Four meters: RMS, bass, mid, treble.
    let meter_w = (size.x - bars_w) / 4.0;
    let m = i32(floor((px.x - bars_w) / meter_w));
    let slot = vec2f(bars_w + f32(m) * meter_w + 4.0, bars_w + f32(m + 1) * meter_w - 4.0);
    let value = load(ROW_LEVELS, m, 5.0);
    var colors = array<vec3f, 4>(
        vec3f(0.9, 0.9, 0.9),
        vec3f(1.0, 0.3, 0.2),
        vec3f(0.3, 1.0, 0.3),
        vec3f(0.3, 0.5, 1.0),
    );
    let c = colors[clamp(m, 0, 3)];
    var col = bar(px, bars_h, slot, value, c * 0.5, c, bg);

    // The RMS meter also marks the sample peak.
    let peak_y = (1.0 - clamp(load(ROW_LEVELS, 4, 5.0), 0.0, 1.0)) * bars_h;
    if (m == 0 && px.x >= slot.x && px.x < slot.y && abs(px.y - peak_y) < 1.0) {
        col = PEAK_CAP;
    }
    return col;
}

@fragment
fn fs(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    let w = p.resolution.x;
    let h = p.resolution.y;
    let wave_end = h * p.wave_end;
    let spec_end = h * p.spec_end;

    var col: vec3f;
    if (pos.y < wave_end) {
        col = draw_wave(pos.xy, vec2f(w, wave_end));
    } else if (pos.y < spec_end) {
        col = draw_spectrum(vec2f(pos.x, pos.y - wave_end), vec2f(w, spec_end - wave_end));
    } else {
        col = draw_bands(vec2f(pos.x, pos.y - spec_end), vec2f(w, h - spec_end));
    }
    col = mix(col, DIVIDER, line(abs(pos.y - wave_end)));
    col = mix(col, DIVIDER, line(abs(pos.y - spec_end)));
    return vec4f(col, 1.0);
}
