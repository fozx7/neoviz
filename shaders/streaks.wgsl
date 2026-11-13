// Glowing streaks: each instance is a soft capsule from p0 to p1 with its own
// colour and width. p0 == p1 draws a round dot. Drawn with additive blending.
//
// Positions are in aspect units (y -1..1 up, x -aspect..aspect).

struct Params {
    resolution: vec2f,
    _pad0: f32,
    _pad1: f32,
}

@group(0) @binding(0) var<uniform> p: Params;

struct VOut {
    @builtin(position) pos: vec4f,
    @location(0) local: vec2f,  // pixel offset from p0, in the segment's frame
    @location(1) length_px: f32,
    @location(2) radius_px: f32,
    @location(3) color: vec3f,
}

fn to_px(q: vec2f) -> vec2f {
    let aspect = p.resolution.x / p.resolution.y;
    return vec2f((q.x / aspect * 0.5 + 0.5) * p.resolution.x, (0.5 - q.y * 0.5) * p.resolution.y);
}

@vertex
fn vs(
    @builtin(vertex_index) vi: u32,
    @location(0) ends: vec4f,   // p0.xy, p1.xy
    @location(1) style: vec4f,  // rgb, width (fraction of height)
) -> VOut {
    let a = to_px(ends.xy);
    let b = to_px(ends.zw);
    let delta = b - a;
    let len = length(delta);
    let dir = select(vec2f(1.0, 0.0), delta / len, len > 1e-3);
    let normal = vec2f(-dir.y, dir.x);
    // Quad reaches 2.5 radii past the core so the glow tail isn't clipped.
    let r = max(style.w * p.resolution.y, 0.5);
    let reach = r * 2.5;

    var corners = array<vec2f, 6>(
        vec2f(0.0, -1.0), vec2f(1.0, -1.0), vec2f(0.0, 1.0),
        vec2f(0.0, 1.0), vec2f(1.0, -1.0), vec2f(1.0, 1.0),
    );
    let c = corners[vi];
    let along = mix(-reach, len + reach, c.x);
    let across = c.y * reach;
    let px = a + dir * along + normal * across;

    var out: VOut;
    out.pos = vec4f(px.x / p.resolution.x * 2.0 - 1.0, 1.0 - px.y / p.resolution.y * 2.0, 0.0, 1.0);
    out.local = vec2f(along, across);
    out.length_px = len;
    out.radius_px = r;
    out.color = style.rgb;
    return out;
}

@fragment
fn fs(in: VOut) -> @location(0) vec4f {
    // Distance from the segment (a capsule), in radii.
    let along = clamp(in.local.x, 0.0, in.length_px);
    let d = length(vec2f(in.local.x - along, in.local.y)) / in.radius_px;
    // Spread the energy of long streaks so fast movers aren't brighter.
    let thin = in.radius_px / max(in.radius_px, in.length_px * 0.5);
    let intensity = exp(-d * d * 2.0) * mix(0.35, 1.0, thin);
    return vec4f(in.color * intensity, 1.0);
}
