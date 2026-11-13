// Glowing polyline. Each instance is one segment (p0 -> p1), expanded into a
// quad in the vertex shader. Drawn with MAX blending, so overlapping segments
// don't add up to hot spots.
//
// Points are in "aspect units": y in -1..1 (up), x in -aspect..aspect.

struct Params {
    resolution: vec2f,
    core_px: f32,   // half-width of the solid core
    glow_px: f32,   // half-width of the soft glow around it
    color: vec4f,
}

@group(0) @binding(0) var<uniform> p: Params;

struct VOut {
    @builtin(position) pos: vec4f,
    @location(0) across_px: f32, // signed distance from the centre line
}

fn to_px(q: vec2f) -> vec2f {
    let aspect = p.resolution.x / p.resolution.y;
    return vec2f((q.x / aspect * 0.5 + 0.5) * p.resolution.x, (0.5 - q.y * 0.5) * p.resolution.y);
}

fn to_clip(px: vec2f) -> vec4f {
    return vec4f(px.x / p.resolution.x * 2.0 - 1.0, 1.0 - px.y / p.resolution.y * 2.0, 0.0, 1.0);
}

@vertex
fn vs(@builtin(vertex_index) vi: u32, @location(0) p0: vec2f, @location(1) p1: vec2f) -> VOut {
    let a = to_px(p0);
    let b = to_px(p1);
    let delta = b - a;
    let len = length(delta);
    let dir = select(vec2f(1.0, 0.0), delta / len, len > 1e-4);
    let normal = vec2f(-dir.y, dir.x);
    let w = p.glow_px;

    // Two triangles: (along, across) corners. Ends are extended by `w` so joints overlap.
    var corners = array<vec2f, 6>(
        vec2f(0.0, -1.0), vec2f(1.0, -1.0), vec2f(0.0, 1.0),
        vec2f(0.0, 1.0), vec2f(1.0, -1.0), vec2f(1.0, 1.0),
    );
    let c = corners[vi];
    let pos = mix(a, b, c.x) + dir * (c.x * 2.0 - 1.0) * w + normal * c.y * w;

    var out: VOut;
    out.pos = to_clip(pos);
    out.across_px = c.y * w;
    return out;
}

@fragment
fn fs(in: VOut) -> @location(0) vec4f {
    let d = abs(in.across_px);
    let core = 1.0 - smoothstep(p.core_px - 0.75, p.core_px + 0.75, d);
    let glow = exp(-3.0 * d / p.glow_px) * 0.6;
    return vec4f(p.color.rgb * max(core, glow), 1.0);
}
