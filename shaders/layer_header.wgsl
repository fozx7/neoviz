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

// --- Stable noise -------------------------------------------------------------
// Random numbers from integer hashing (PCG). The fract(sin(...)) trick above
// isn't exact across noise cells on every GPU, so anything that amplifies
// small differences (thin-film colours, sharp thresholds) shows squares.
// Use these there.
fn pcg2(v: vec2u) -> vec2u {
    var x = v * 1664525u + 1013904223u;
    x.x += x.y * 1664525u;
    x.y += x.x * 1664525u;
    x = x ^ (x >> vec2u(16u));
    x.x += x.y * 1664525u;
    x.y += x.x * 1664525u;
    return x ^ (x >> vec2u(16u));
}

// Two random numbers (0..1) for the lattice point `q` (whole numbers).
fn hash22(q: vec2f) -> vec2f {
    return vec2f(pcg2(bitcast<vec2u>(vec2i(round(q))))) / 4294967295.0;
}

// Smooth value noise and fractal noise built on `hash22`, 0..1.
fn smooth_noise(q: vec2f) -> f32 {
    let i = floor(q);
    let f = fract(q);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash22(i).x;
    let b = hash22(i + vec2f(1.0, 0.0)).x;
    let c = hash22(i + vec2f(0.0, 1.0)).x;
    let d = hash22(i + vec2f(1.0, 1.0)).x;
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn smooth_fbm(q: vec2f) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var x = q;
    for (var i = 0; i < 4; i++) {
        sum += amp * smooth_noise(x);
        x = x * 2.03 + vec2f(1.7, 9.2);
        amp *= 0.5;
    }
    return sum;
}

fn pcg3(v: vec3u) -> vec3u {
    var x = v * 1664525u + 1013904223u;
    x.x += x.y * x.z;
    x.y += x.z * x.x;
    x.z += x.x * x.y;
    x = x ^ (x >> vec3u(16u));
    x.x += x.y * x.z;
    x.y += x.z * x.x;
    x.z += x.x * x.y;
    return x;
}

// Three random numbers (0..1) for the lattice point `q` (whole numbers).
fn hash33(q: vec3f) -> vec3f {
    return vec3f(pcg3(bitcast<vec3u>(vec3i(round(q))))) / 4294967295.0;
}

// Smooth 3D value noise and fractal noise, 0..1 (for surfaces of spheres,
// which 2D noise can't wrap without a seam).
fn noise3(q: vec3f) -> f32 {
    let i = floor(q);
    let f = fract(q);
    let u = f * f * (3.0 - 2.0 * f);
    let n000 = hash33(i).x;
    let n100 = hash33(i + vec3f(1.0, 0.0, 0.0)).x;
    let n010 = hash33(i + vec3f(0.0, 1.0, 0.0)).x;
    let n110 = hash33(i + vec3f(1.0, 1.0, 0.0)).x;
    let n001 = hash33(i + vec3f(0.0, 0.0, 1.0)).x;
    let n101 = hash33(i + vec3f(1.0, 0.0, 1.0)).x;
    let n011 = hash33(i + vec3f(0.0, 1.0, 1.0)).x;
    let n111 = hash33(i + vec3f(1.0, 1.0, 1.0)).x;
    let lower = mix(mix(n000, n100, u.x), mix(n010, n110, u.x), u.y);
    let upper = mix(mix(n001, n101, u.x), mix(n011, n111, u.x), u.y);
    return mix(lower, upper, u.z);
}

fn fbm3(q: vec3f) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var x = q;
    for (var i = 0; i < 4; i++) {
        sum += amp * noise3(x);
        x = x * 2.03 + vec3f(1.7, 9.2, 4.6);
        amp *= 0.5;
    }
    return sum;
}
