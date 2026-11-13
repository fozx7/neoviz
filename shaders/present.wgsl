// Copies one slice of the finished canvas to a window. Normally the slice is
// the whole canvas; in span mode it's the part that monitor shows.

struct Params {
    offset: vec2f, // top-left of the slice, 0..1 of the canvas
    scale: vec2f,  // size of the slice, 0..1 of the canvas
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var canvas: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

@fragment
fn fs(in: FullscreenOut) -> @location(0) vec4f {
    return vec4f(textureSample(canvas, samp, p.offset + in.uv * p.scale).rgb, 1.0);
}
