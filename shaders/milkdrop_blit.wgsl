// Copies projectM's frame into our HDR target. The source rows are
// bottom-up (OpenGL), so flip vertically. Sampling the sRGB texture
// linearises it, and the final pass re-encodes, so colours match projectM.

@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

@fragment
fn fs(in: FullscreenOut) -> @location(0) vec4f {
    return vec4f(textureSample(frame, samp, vec2f(in.uv.x, 1.0 - in.uv.y)).rgb, 1.0);
}
