// Crossfades the outgoing visualizer (a) into the incoming one (b) and writes
// the result to the screen.

struct Params {
    mix_amount: f32, // 0 = all a, 1 = all b
    // Scalars, not a vec3f: vec3f is 16-byte aligned and would make this 32 bytes.
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var a: texture_2d<f32>;
@group(0) @binding(2) var b: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;

@fragment
fn fs(in: FullscreenOut) -> @location(0) vec4f {
    let ca = textureSample(a, samp, in.uv).rgb;
    let cb = textureSample(b, samp, in.uv).rgb;
    let col = mix(ca, cb, p.mix_amount);
    return vec4f(clamp(col, vec3f(0.0), vec3f(1.0)), 1.0);
}
