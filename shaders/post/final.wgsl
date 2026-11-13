// Final pass: scene + bloom, optional CRT scanlines and vignette, to the screen.

struct Params {
    bloom_strength: f32,
    scanlines: f32,   // 1.0 = on
    _pad0: f32,
    _pad1: f32,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var scene: texture_2d<f32>;
@group(0) @binding(2) var bloom: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;

// Roughly 270 visible scanlines whatever the resolution, but never finer than 3px.
const SCANLINES: f32 = 270.0;
const MIN_PERIOD_PX: f32 = 3.0;

@fragment
fn fs(in: FullscreenOut) -> @location(0) vec4f {
    var col = textureSample(scene, samp, in.uv).rgb;
    col += textureSample(bloom, samp, in.uv).rgb * p.bloom_strength;

    if (p.scanlines > 0.5) {
        let height = f32(textureDimensions(scene).y);
        let period = max(MIN_PERIOD_PX, round(height / SCANLINES));
        let line = 0.5 + 0.5 * cos(6.2831853 * in.pos.y / period);
        col *= mix(0.65, 1.05, line);
        let d = in.uv - 0.5;
        col *= 1.0 - dot(d, d) * 0.7;
    }
    return vec4f(clamp(col, vec3f(0.0), vec3f(1.0)), 1.0);
}
