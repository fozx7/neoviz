// Shared header for full-screen visualizer shaders (prepended by ShaderLayer).
//
// Provides the per-frame audio uniforms `p`, the band data texture, and colour
// helpers. Must match `LayerParams::to_array` in src/render/layer.rs.

struct Params {
    resolution: vec2f,
    time: f32,
    dt: f32,
    bass: f32,       // 0..1, smoothed
    mid: f32,
    treble: f32,
    rms: f32,        // after sensitivity
    flash: f32,      // beat flash, 0..1, decaying
    band_count: f32,
    variant: f32,    // visualizer-specific
    beat: f32,       // 1.0 on the frame a beat lands
    extra: vec4f,    // visualizer-specific
}

@group(0) @binding(0) var<uniform> p: Params;
// Row 0: band levels (0..1, bass first). Row 1: band peak caps.
@group(0) @binding(1) var data: texture_2d<f32>;

const PI: f32 = 3.14159265;
const TAU: f32 = 6.28318531;

fn band(i: i32) -> f32 {
    return textureLoad(data, vec2i(clamp(i, 0, i32(p.band_count) - 1), 0), 0).r;
}

fn band_peak(i: i32) -> f32 {
    return textureLoad(data, vec2i(clamp(i, 0, i32(p.band_count) - 1), 1), 0).r;
}

// Band level at position t (0 = lowest band, 1 = highest), interpolated.
fn band_at(t: f32) -> f32 {
    let f = clamp(t, 0.0, 1.0) * (p.band_count - 1.0);
    let i = i32(floor(f));
    return mix(band(i), band(i + 1), fract(f));
}

fn hsv(h: f32, s: f32, v: f32) -> vec3f {
    let k = vec3f(1.0, 2.0 / 3.0, 1.0 / 3.0);
    let q = abs(fract(vec3f(h) + k) * 6.0 - 3.0);
    return v * mix(vec3f(1.0), clamp(q - 1.0, vec3f(0.0), vec3f(1.0)), s);
}

// Pixel position centred on the screen, y up, in "aspect units"
// (y -1..1, x -aspect..aspect).
fn centered(uv: vec2f) -> vec2f {
    let aspect = p.resolution.x / p.resolution.y;
    return vec2f((uv.x * 2.0 - 1.0) * aspect, 1.0 - uv.y * 2.0);
}

fn hash21(q: vec2f) -> f32 {
    let h = dot(q, vec2f(127.1, 311.7));
    return fract(sin(h) * 43758.5453);
}

// Smooth value noise, 0..1.
fn noise(q: vec2f) -> f32 {
    let i = floor(q);
    let f = fract(q);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2f(1.0, 0.0));
    let c = hash21(i + vec2f(0.0, 1.0));
    let d = hash21(i + vec2f(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Four octaves of noise, 0..~1.
fn fbm(q: vec2f) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var x = q;
    for (var i = 0; i < 4; i++) {
        sum += amp * noise(x);
        x = x * 2.03 + vec2f(1.7, 9.2);
        amp *= 0.5;
    }
    return sum;
}
