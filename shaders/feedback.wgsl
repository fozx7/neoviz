// Feedback warp: redraws the previous frame zoomed, rotated and faded.
// Also used (with identity transform, no fade) to blit the result out.

struct Params {
    zoom: f32,      // > 1 drifts outward, < 1 inward
    rotation: f32,  // radians this frame
    fade: f32,      // brightness multiplier this frame
    _pad: f32,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var prev: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

@fragment
fn fs(in: FullscreenOut) -> @location(0) vec4f {
    let dims = vec2f(textureDimensions(prev));
    let aspect = dims.x / dims.y;

    // Work in aspect-correct coordinates centred on the screen so rotation is round.
    var q = in.uv - 0.5;
    q.x *= aspect;
    let c = cos(-p.rotation);
    let s = sin(-p.rotation);
    q = vec2f(c * q.x - s * q.y, s * q.x + c * q.y) / p.zoom;
    q.x /= aspect;
    let uv = q + 0.5;

    // Outside the old frame there's nothing to pull in.
    if (any(uv < vec2f(0.0)) || any(uv > vec2f(1.0))) {
        return vec4f(0.0, 0.0, 0.0, 1.0);
    }
    return vec4f(textureSample(prev, samp, uv).rgb * p.fade, 1.0);
}
