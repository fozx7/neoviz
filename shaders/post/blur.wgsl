// Bloom step 2: one direction of a 9-tap Gaussian blur, using 5 bilinear taps.

struct Params {
    step: vec2f, // one texel along the blur direction, in uv units
    _pad: vec2f,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

@fragment
fn fs(in: FullscreenOut) -> @location(0) vec4f {
    let offsets = vec2f(1.3846153846, 3.2307692308);
    let weights = vec3f(0.2270270270, 0.3162162162, 0.0702702703);
    var c = textureSample(src, samp, in.uv).rgb * weights.x;
    c += textureSample(src, samp, in.uv + p.step * offsets.x).rgb * weights.y;
    c += textureSample(src, samp, in.uv - p.step * offsets.x).rgb * weights.y;
    c += textureSample(src, samp, in.uv + p.step * offsets.y).rgb * weights.z;
    c += textureSample(src, samp, in.uv - p.step * offsets.y).rgb * weights.z;
    return vec4f(c, 1.0);
}
