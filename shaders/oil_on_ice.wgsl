// Oil on Ice: slicks of oil drifting over very dark ice. The oil shows
// thin-film colours (the rainbow sheen of real oil on water): vivid where it
// thins out at the edges, darker and greyer where it pools thick. The ice
// below has faint cracks, trapped bubbles and the odd glint of frost.
//
// The slicks spread when the music swells, the bass ripples their thickness
// out from the centre (shifting the colours), and beats push the colours
// round. A soft light drifts across, catching the sheen.
//
// p.extra.x = drift clock, p.extra.y = light clock,
// p.extra.z = loudness, smoothed.

const ICE: vec3f = vec3f(0.0015, 0.003, 0.006);
const ICE_DEEP: vec3f = vec3f(0.005, 0.012, 0.02);
const CRACK: vec3f = vec3f(0.25, 0.36, 0.48);

// Distance to the nearest edge of a Voronoi cell: the cracks in the ice.
fn crack_distance(q: vec2f) -> f32 {
    let n = floor(q);
    let f = fract(q);
    var nearest = vec2f(0.0);
    var cell = vec2f(0.0);
    var best = 8.0;
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            let g = vec2f(f32(i), f32(j));
            let r = g + hash22(n + g) - f;
            let d = dot(r, r);
            if (d < best) {
                best = d;
                nearest = r;
                cell = g;
            }
        }
    }
    var edge = 8.0;
    for (var j = -2; j <= 2; j++) {
        for (var i = -2; i <= 2; i++) {
            let g = cell + vec2f(f32(i), f32(j));
            let r = g + hash22(n + g) - f;
            let between = r - nearest;
            if (dot(between, between) > 1e-5) {
                edge = min(edge, dot(0.5 * (nearest + r), normalize(between)));
            }
        }
    }
    return edge;
}

// Thin-film interference: the colour reflected by an oil film `d` microns
// thick. Thick films wash out towards grey.
fn film(d: f32) -> vec3f {
    let wavelengths = vec3f(0.65, 0.53, 0.45);
    let colour = 0.5 + 0.5 * cos(TAU * 2.0 * 1.45 * d / wavelengths + PI);
    let grey = vec3f(dot(colour, vec3f(0.333)));
    // Real films are less saturated than pure interference.
    return mix(colour, grey, 0.3 + 0.7 * smoothstep(0.9, 1.8, d));
}

fn ice(q: vec2f, light: f32) -> vec3f {
    // Depth: darker and lighter patches.
    let depth = smooth_fbm(q * 1.1 + 7.0);
    var col = mix(ICE, ICE_DEEP, depth * depth * 1.6);

    // Cracks at two scales, wobbled so they look fractured, not geometric.
    let wobble = vec2f(smooth_fbm(q * 2.0), smooth_fbm(q * 2.0 + 3.1)) - 0.5;
    let big = crack_distance(q * 1.2 + wobble * 0.8);
    let small = crack_distance(q * 3.4 + wobble * 1.5 + 11.0);
    let cracks = exp(-big * 140.0) * 0.16 + exp(-small * 200.0) * 0.05;
    col += CRACK * cracks * (0.3 + 0.7 * light);

    // Bubbles frozen in: a few small faint rings.
    let bq = q * 9.0;
    let bubble_cell = floor(bq);
    let h = hash22(bubble_cell);
    if (h.x > 0.95) {
        let centre = bubble_cell + 0.25 + 0.5 * hash22(bubble_cell + 5.3);
        let radius = 0.06 + 0.12 * h.y;
        let ring = exp(-pow((length(bq - centre) - radius) * 40.0, 2.0));
        col += vec3f(0.3, 0.4, 0.5) * ring * 0.06 * (0.3 + light);
    }

    // Frost glints that sparkle with the treble.
    let gq = q * 60.0;
    let glint_cell = floor(gq);
    let g = hash22(glint_cell);
    if (g.x > 0.995) {
        let d = length(fract(gq) - 0.5);
        let twinkle = 0.5 + 0.5 * sin(p.time * 3.0 + g.y * 40.0);
        col += vec3f(0.6, 0.75, 1.0) * smoothstep(0.25, 0.0, d) * twinkle * (0.1 + 1.2 * p.treble);
    }
    return col;
}

@fragment
fn fs(in: FullscreenOut) -> @location(0) vec4f {
    let uv = centered(in.uv);
    let q = uv * 1.3;
    let t = p.extra.x;

    // A soft light drifting slowly across the surface.
    let lt = p.extra.y * TAU;
    let light_pos = vec2f(0.9 * cos(lt * 0.7), 0.55 * sin(lt));
    let light = exp(-dot(uv - light_pos, uv - light_pos) * 0.9);

    // The oil: domain-warped noise, flowing with the drift clock.
    let warp = vec2f(
        smooth_fbm(q * 0.8 + vec2f(t, -0.7 * t)),
        smooth_fbm(q * 0.8 + vec2f(4.1, 2.3) - 0.6 * t),
    );
    let field = smooth_fbm(q * 1.1 + 2.2 * warp + vec2f(-0.3 * t, 0.2 * t));

    // Louder music spreads the slicks further.
    let threshold = 0.5 - 0.1 * p.extra.z - 0.04 * p.bass;
    let cover = smoothstep(threshold, threshold + 0.012, field);

    // Film thickness: thin at the slick's edge, building inwards, with fine
    // ripples, the spectrum radiating from the centre, and beats.
    let ripples = 0.05 * smooth_fbm(q * 4.0 + warp * 2.0 + t * 0.5);
    let spectrum = 0.15 * band_at(clamp(length(uv) / 1.6, 0.0, 1.0));
    let thickness = 0.18 + 1.3 * max(field - threshold, 0.0) + ripples + spectrum
        + 0.2 * p.flash;
    // The sheen is a reflection, so it's faint except where the light
    // catches it, and thick pools of oil are darker still.
    let pooled = mix(1.0, 0.3, smoothstep(0.08, 0.3, field - threshold));
    let sheen = film(thickness) * pooled * (0.05 + 0.45 * light + 0.12 * p.extra.z + 0.15 * p.flash);

    // Oil dims the ice below it; its edge catches a little extra light.
    let rim = cover * (1.0 - cover) * 4.0;
    var col = ice(q, light) * (1.0 - 0.8 * cover);
    col += sheen * cover + film(0.25 + ripples) * rim * 0.05;
    col += vec3f(0.5, 0.6, 0.7) * light * light * 0.012;

    let vignette = 1.0 - 0.6 * dot(in.uv - 0.5, in.uv - 0.5) * 2.0;
    return vec4f(col * vignette, 1.0);
}
