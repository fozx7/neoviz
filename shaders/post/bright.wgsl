// Bloom step 1: keep only the bright parts, downsampled to the bloom size.

struct Params {
    threshold: f32, // brightness where bloom starts
    knee: f32,      // softness of the threshold
    _pad0: f32,
    _pad1: f32,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

@fragment
fn fs(in: FullscreenOut) -> @location(0) vec4f {
    // Four bilinear taps cover a 4x4 block of the full-res source, which
    // avoids shimmering when downsampling by 4.
    let texel = 1.0 / vec2f(textureDimensions(src));
    var c = textureSample(src, samp, in.uv + texel * vec2f(-1.0, -1.0)).rgb;
    c += textureSample(src, samp, in.uv + texel * vec2f(1.0, -1.0)).rgb;
    c += textureSample(src, samp, in.uv + texel * vec2f(-1.0, 1.0)).rgb;
    c += textureSample(src, samp, in.uv + texel * vec2f(1.0, 1.0)).rgb;
    c *= 0.25;

    let brightness = max(c.r, max(c.g, c.b));
    var soft = clamp(brightness - p.threshold + p.knee, 0.0, 2.0 * p.knee);
    soft = soft * soft / (4.0 * p.knee + 1e-4);
    let weight = max(soft, brightness - p.threshold) / max(brightness, 1e-4);
    return vec4f(c * weight, 1.0);
}
