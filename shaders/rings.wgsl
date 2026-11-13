// Battery Rings: concentric pulsing rings (one per band group) and rotating
// spokes, drawn over a zooming, rotating feedback trail.
//
// p.extra.y = hue offset (p.extra.x is unused here).

const RINGS: i32 = 8;
const SPOKES: f32 = 12.0;

@fragment
fn fs(in: FullscreenOut) -> @location(0) vec4f {
    let q = centered(in.uv);
    let r = length(q);
    let a = atan2(q.y, q.x);
    let pixel = 2.0 / p.resolution.y; // one pixel in aspect units
    let hue = p.extra.y;

    var col = vec3f(0.0);
    for (var k = 0; k < RINGS; k++) {
        let fk = f32(k);
        let v = band_at(0.03 + fk / f32(RINGS) * 0.9);
        let radius = 0.12 + fk * 0.1 + v * 0.07 + p.flash * 0.05;
        let width = pixel * (1.5 + 4.0 * v);
        let glow = exp(-abs(r - radius) / width);
        col += hsv(hue + fk * 0.07, 0.75, 1.0) * glow * (0.15 + 0.9 * v);
    }

    // Spokes: distance (along the arc) to the nearest spoke.
    let turn = (a + p.time * 0.4) / TAU * SPOKES;
    let arc = abs(fract(turn + 0.5) - 0.5) * TAU / SPOKES * r;
    let spoke_fade = smoothstep(0.08, 0.2, r) * (1.0 - smoothstep(0.7, 1.1, r));
    col += hsv(hue + 0.5, 0.5, 1.0) * exp(-arc / (pixel * 2.0)) * spoke_fade * (0.08 + 0.8 * p.treble);

    // Core that swells with bass and beats.
    col += hsv(hue + 0.1, 0.4, 1.0) * exp(-r * (12.0 - 7.0 * p.flash)) * (0.2 + p.bass);
    return vec4f(col, 1.0);
}
