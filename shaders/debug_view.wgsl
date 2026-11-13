// Debug view: waveform (top half) and log-frequency spectrum (bottom half).
//
// `data` row 0 holds the waveform (-1..1), row 1 the spectrum in dBFS.

struct Params {
    resolution: vec2f,
    sample_rate: f32,
    fft_size: f32,
    wave_len: f32,
    bins: f32,
    db_min: f32, // bottom of the spectrum display, e.g. -90 dB
    _pad: f32,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var data: texture_2d<f32>;

const MIN_HZ: f32 = 20.0;
const MAX_SPAN: i32 = 256; // cap on samples/bins scanned per pixel column
const BG: vec3f = vec3f(0.015, 0.015, 0.03);
const GRID: vec3f = vec3f(0.07, 0.07, 0.11);
const WAVE: vec3f = vec3f(0.3, 1.0, 0.6);
const SPEC_LOW: vec3f = vec3f(0.1, 0.3, 1.0);
const SPEC_HIGH: vec3f = vec3f(1.0, 0.35, 0.8);

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    // One oversized triangle covering the screen.
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn wave(i: i32) -> f32 {
    return textureLoad(data, vec2i(clamp(i, 0, i32(p.wave_len) - 1), 0), 0).r;
}

fn spec(i: i32) -> f32 {
    return textureLoad(data, vec2i(clamp(i, 0, i32(p.bins) - 1), 1), 0).r;
}

// Antialiased coverage for a pixel `d` pixels away from a 1.5px-wide line.
fn line(d: f32) -> f32 {
    return 1.0 - smoothstep(0.75, 1.75, d);
}

fn draw_wave(px: vec2f, w: f32, h: f32) -> vec3f {
    let center = h * 0.5;
    let amp = h * 0.45;
    let scale = (p.wave_len - 1.0) / w;
    let i0 = i32(floor(px.x * scale));
    let i1 = min(i32(ceil((px.x + 1.0) * scale)), i0 + MAX_SPAN);

    // Span every sample this column covers so steep edges stay connected.
    var lo = wave(i0);
    var hi = lo;
    for (var j = i0 + 1; j <= i1; j++) {
        let v = wave(j);
        lo = min(lo, v);
        hi = max(hi, v);
    }
    let y_top = center - hi * amp;
    let y_bottom = center - lo * amp;
    let d = max(max(y_top - px.y, px.y - y_bottom), 0.0);

    var col = BG;
    col = mix(col, GRID, line(abs(px.y - center)));
    return mix(col, WAVE, line(d));
}

// Fractional FFT bin at horizontal pixel `x` on a log axis from MIN_HZ to Nyquist.
fn bin_at(x: f32, w: f32) -> f32 {
    let nyquist = p.sample_rate * 0.5;
    let hz = MIN_HZ * pow(nyquist / MIN_HZ, x / w);
    return hz / p.sample_rate * p.fft_size;
}

fn x_of_hz(hz: f32, w: f32) -> f32 {
    let nyquist = p.sample_rate * 0.5;
    return w * log(hz / MIN_HZ) / log(nyquist / MIN_HZ);
}

fn draw_spectrum(px: vec2f, w: f32, h: f32) -> vec3f {
    let b0 = bin_at(px.x, w);
    let b1 = bin_at(px.x + 1.0, w);

    var level: f32;
    if (b1 - b0 < 1.0) {
        // Low frequencies: several pixels per bin, so interpolate.
        let i = i32(floor(b0));
        level = mix(spec(i), spec(i + 1), fract(b0));
    } else {
        // High frequencies: several bins per pixel, so take the loudest.
        let i0 = i32(floor(b0));
        let i1 = min(i32(ceil(b1)), i0 + MAX_SPAN);
        level = spec(i0);
        for (var j = i0 + 1; j <= i1; j++) {
            level = max(level, spec(j));
        }
    }

    let norm = clamp((level - p.db_min) / -p.db_min, 0.0, 1.0);
    let height_frac = 1.0 - px.y / h; // 0 at bottom, 1 at top

    var col = BG;
    // Horizontal grid every 20 dB.
    let db_here = p.db_min * (1.0 - height_frac);
    let db_step = abs(db_here - 20.0 * round(db_here / 20.0)) * h / -p.db_min;
    col = mix(col, GRID, line(db_step));
    // Vertical grid at 100 Hz, 1 kHz, 10 kHz.
    for (var hz = 100.0; hz < p.sample_rate * 0.5; hz *= 10.0) {
        col = mix(col, GRID, line(abs(px.x - x_of_hz(hz, w))));
    }

    if (height_frac <= norm) {
        col = mix(SPEC_LOW, SPEC_HIGH, height_frac);
    }
    return col;
}

@fragment
fn fs(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    let w = p.resolution.x;
    let half_h = p.resolution.y * 0.5;
    var col: vec3f;
    if (pos.y < half_h) {
        col = draw_wave(pos.xy, w, half_h);
    } else {
        col = draw_spectrum(vec2f(pos.x, pos.y - half_h), w, half_h);
    }
    col = mix(col, vec3f(0.25), line(abs(pos.y - half_h)));
    return vec4f(col, 1.0);
}
