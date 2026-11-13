// Bars & Peaks: a classic spectrum analyser with falling peak caps.
// Variants: 0 solid (with floor reflection), 1 segmented LED, 2 mirrored.
//
// `data` row 0 = band levels (0..1), row 1 = peak caps (0..1).

struct Params {
    resolution: vec2f,
    band_count: f32,
    variant: f32,
    time: f32,
    flash: f32, // beat flash, 0..1
    _pad: vec2f,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var data: texture_2d<f32>;

const SIDE_MARGIN: f32 = 0.04;
const BAR_GAP: f32 = 0.22;        // fraction of each slot left empty
const BASELINE: f32 = 0.80;       // solid/LED: bars stand on this line (0 = top)
const BAR_HEIGHT: f32 = 0.72;     // solid/LED: height of a full bar
const REFLECTION: f32 = 0.18;     // solid: depth of the floor reflection
const LED_SEGMENTS: f32 = 24.0;
const LED_GAP: f32 = 0.3;         // fraction of each segment left dark
const MIRROR_HALF: f32 = 0.42;    // mirrored: half-height of a full bar
const CAP: f32 = 0.006;           // peak cap thickness (fraction of height)

fn level(row: i32, i: i32) -> f32 {
    return textureLoad(data, vec2i(clamp(i, 0, i32(p.band_count) - 1), row), 0).r;
}

fn hsv(h: f32, s: f32, v: f32) -> vec3f {
    let k = vec3f(1.0, 2.0 / 3.0, 1.0 / 3.0);
    let q = abs(fract(vec3f(h) + k) * 6.0 - 3.0);
    return v * mix(vec3f(1.0), clamp(q - 1.0, vec3f(0.0), vec3f(1.0)), s);
}

// Green at the bottom, through yellow, to red at the top.
fn heat(t: f32) -> vec3f {
    let green = vec3f(0.1, 0.9, 0.2);
    let yellow = vec3f(1.0, 0.85, 0.1);
    let red = vec3f(1.0, 0.15, 0.1);
    return mix(mix(green, yellow, smoothstep(0.0, 0.6, t)), red, smoothstep(0.6, 1.0, t));
}

fn background(uv: vec2f) -> vec3f {
    let base = mix(vec3f(0.0, 0.0, 0.015), vec3f(0.03, 0.02, 0.07), uv.y);
    return base + vec3f(0.07, 0.035, 0.0) * p.flash;
}

// Which bar this pixel column belongs to, and whether it's inside the bar
// (not in the gap). Returns (index, inside as 0/1).
fn slot(x: f32) -> vec2f {
    let u = (x - SIDE_MARGIN) / (1.0 - 2.0 * SIDE_MARGIN);
    if (u < 0.0 || u >= 1.0) {
        return vec2f(-1.0, 0.0);
    }
    let f = u * p.band_count;
    let within = fract(f);
    let inside = select(0.0, 1.0, within > BAR_GAP * 0.5 && within < 1.0 - BAR_GAP * 0.5);
    return vec2f(floor(f), inside);
}

fn solid(uv: vec2f, i: i32) -> vec3f {
    let v = level(0, i);
    let peak = level(1, i);
    let h = (BASELINE - uv.y) / BAR_HEIGHT; // height above the baseline, 0..1

    if (h >= 0.0) {
        if (abs(h - peak) * BAR_HEIGHT < CAP && peak > 0.01) {
            return vec3f(1.0, 0.95, 0.85);
        }
        if (h <= v) {
            return heat(h) * (1.0 + 0.3 * p.flash);
        }
        return background(uv);
    }

    // Floor reflection: the bar mirrored below the baseline, fading out.
    let depth = (uv.y - BASELINE) / REFLECTION;
    let mirrored = -h;
    if (depth < 1.0 && mirrored <= v) {
        return mix(background(uv), heat(mirrored), 0.25 * (1.0 - depth));
    }
    return background(uv);
}

fn led(uv: vec2f, i: i32) -> vec3f {
    let v = level(0, i);
    let peak = level(1, i);
    let h = (BASELINE - uv.y) / BAR_HEIGHT;
    if (h < 0.0 || h > 1.0) {
        return background(uv);
    }
    let seg = floor(h * LED_SEGMENTS);
    if (fract(h * LED_SEGMENTS) < LED_GAP) {
        return background(uv);
    }
    let seg_top = (seg + 1.0) / LED_SEGMENTS;
    let color = heat(seg / (LED_SEGMENTS - 1.0));
    if (seg == floor(min(peak, 0.999) * LED_SEGMENTS) && peak > 0.02) {
        return mix(color, vec3f(1.0), 0.5);
    }
    if (seg_top <= v + 0.5 / LED_SEGMENTS) {
        return color * (1.0 + 0.3 * p.flash);
    }
    return background(uv) + color * 0.06; // unlit segment
}

fn mirror(uv: vec2f, i: i32) -> vec3f {
    let v = level(0, i);
    let peak = level(1, i);
    let d = abs(uv.y - 0.5) / MIRROR_HALF; // 0 at the centre line, 1 at full height
    let hue = p.time * 0.04 + f32(i) / p.band_count * 0.35;

    if (abs(d - peak) * MIRROR_HALF < CAP && peak > 0.01) {
        return hsv(hue + 0.1, 0.3, 1.0);
    }
    if (d <= v) {
        return hsv(hue, 0.85, 0.35 + 0.65 * d) * (1.0 + 0.3 * p.flash);
    }
    return background(uv);
}

@fragment
fn fs(in: FullscreenOut) -> @location(0) vec4f {
    let uv = in.uv; // y down: 0 at top
    let s = slot(uv.x);
    if (s.y < 0.5) {
        return vec4f(background(uv), 1.0);
    }
    let i = i32(s.x);
    var col: vec3f;
    switch (i32(p.variant)) {
        case 1: { col = led(uv, i); }
        case 2: { col = mirror(uv, i); }
        default: { col = solid(uv, i); }
    }
    return vec4f(col, 1.0);
}
