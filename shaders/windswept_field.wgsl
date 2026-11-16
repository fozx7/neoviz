// Windswept Field: an open field of long grass under a wide sky, with the
// wind rolling through it in gusts. Each blade bends with the gust passing
// over it, and bent grass turns paler as it catches the light, so the gusts
// show as silvery waves sweeping across the field.
//
// The field is drawn as rows of blades at increasing distance (nearest
// first), each row smaller and hazier, down to a distant line of hills.
// Gusts strengthen with the bass and the music's loudness, beats push a
// stronger gust through, and seeds drifting on the wind glint with the treble.
//
// p.extra.x = wind clock (the gusts' travel), p.extra.y = cloud clock,
// p.extra.z = loudness, smoothed.

const HORIZON: f32 = 0.22;     // screen height of the horizon (y up, -1..1)
const CAM_HEIGHT: f32 = 1.1;   // eye height above the ground
const NEAREST: f32 = 0.72;     // distance of the nearest row (its roots are below the screen)
const ROW_GROWTH: f32 = 1.12;  // each row this much further than the last
const ROWS: i32 = 34;
const BLADE_HEIGHT: f32 = 0.95;
const BLADE_WIDTH: f32 = 0.022;
const BLADE_SPACING: f32 = 0.03;

const SKY_TOP: vec3f = vec3f(0.18, 0.36, 0.75);
const SKY_LOW: vec3f = vec3f(0.62, 0.76, 0.9);
const SUN_DIR: vec2f = vec2f(0.85, 0.62);
const SUN: vec3f = vec3f(1.0, 0.9, 0.7);
const HILLS: vec3f = vec3f(0.2, 0.34, 0.24);
const HILLS_FAR: vec3f = vec3f(0.38, 0.5, 0.56);
const UNDERGROWTH: vec3f = vec3f(0.02, 0.06, 0.012);
const HAZE: vec3f = vec3f(0.55, 0.66, 0.68);
const ROOT: vec3f = vec3f(0.015, 0.05, 0.01);
const GREEN: vec3f = vec3f(0.09, 0.3, 0.035);
const YELLOW_GREEN: vec3f = vec3f(0.3, 0.45, 0.07);
const SHEEN: vec3f = vec3f(0.62, 0.72, 0.4);

// How strongly the wind is blowing right now, overall.
fn wind_strength() -> f32 {
    return 0.35 + 0.5 * p.extra.z + 0.45 * p.bass + 0.5 * p.flash;
}

// The gust at ground position (x, z): 0 = calm, ~1 = a strong gust. Gusts
// are broad patches of noise travelling with the wind (left to right).
fn gust(x: f32, z: f32) -> f32 {
    let t = p.extra.x;
    let g = smooth_noise(vec2f(x * 0.45 - t * 1.4, z * 0.5 + 0.3 * t));
    return smoothstep(0.3, 0.85, g) * wind_strength();
}

// How far a blade at (x, z) bends: its tip moves this fraction of its height
// to the right. A gentle sway everywhere, plus the gusts.
fn bend(x: f32, z: f32, phase: f32) -> f32 {
    let sway = 0.12 * sin(p.extra.x * 2.2 + x * 0.9 + phase * 6.0);
    return 0.12 + sway + 0.75 * gust(x, z);
}

// One row of blades at distance `z`: the colour and coverage (premultiplied)
// of whatever blade covers screen point `q`.
fn row(q: vec2f, z: f32, index: f32, pixel: f32) -> vec4f {
    // Blades sit a little in front of or behind the row's line (so rows
    // don't show as stripes); skip pixels no blade of this row can reach.
    let spread = 0.5 * (ROW_GROWTH - 1.0);
    let lowest = HORIZON - CAM_HEIGHT / (z * (1.0 - spread));
    let highest = HORIZON + (BLADE_HEIGHT - CAM_HEIGHT) / (z * (1.0 - spread));
    if (q.y < lowest || q.y > highest) {
        return vec4f(0.0);
    }
    let cell = floor(q.x * z / BLADE_SPACING);
    var best = vec4f(0.0);
    // Bent blades lean right, so they can reach in from cells to the left.
    for (var k = -3; k <= 1; k++) {
        let c = cell + f32(k);
        let h = hash22(vec2f(c, index * 37.0));
        // This blade's own distance, scale and root line.
        let zb = z * (1.0 + (hash22(vec2f(c, index * 37.0 + 11.0)).x - 0.5) * 2.0 * spread);
        let s = 1.0 / zb;
        let ground = HORIZON - CAM_HEIGHT * s;
        let x = q.x * zb;
        let px_world = pixel * zb;
        let root = (c + h.x) * BLADE_SPACING;
        let height = BLADE_HEIGHT * (0.55 + 0.45 * h.y);
        let lean = bend(root, z, h.x) + (h.y - 0.5) * 0.25;
        // How far up the blade this pixel is (0 root .. 1 tip). Bending
        // lowers the tip a little.
        let t = (q.y - ground) / (height * s * (1.0 - 0.25 * abs(lean)));
        if (t < 0.0 || t > 1.0) {
            continue;
        }
        let centre = root + lean * height * pow(t, 1.7);
        let half_width = 0.5 * BLADE_WIDTH * (1.0 - 0.92 * t);
        let coverage = clamp(0.5 - (abs(x - centre) - half_width) / px_world, 0.0, 1.0);
        if (coverage > best.a) {
            let base = mix(GREEN, YELLOW_GREEN, h.x * h.y);
            var colour = mix(ROOT, base, smoothstep(0.0, 0.6, t));
            // Bent grass shows its paler side to the light.
            let catch_light = clamp(lean - 0.2, 0.0, 1.0) * t;
            colour = mix(colour, SHEEN, 0.75 * catch_light);
            // Far rows fade into the haze.
            let fog = 1.0 - exp(-(zb - NEAREST) * 0.06);
            colour = mix(colour, HAZE, fog);
            best = vec4f(colour * coverage, coverage);
        }
    }
    return best;
}

fn sky(q: vec2f) -> vec3f {
    let height = clamp((q.y - HORIZON) / (1.0 - HORIZON), 0.0, 1.0);
    var col = mix(SKY_LOW, SKY_TOP, pow(height, 0.7));
    let sun_dist = length(q - SUN_DIR * vec2f(1.6, 1.0));
    col += SUN * (exp(-sun_dist * 6.0) * 0.6 + exp(-sun_dist * 1.5) * 0.12);
    // Clouds, flattened towards the horizon and drifting with the wind.
    let flat_q = vec2f(q.x / (height + 0.25), 1.0 / (height + 0.25));
    let clouds = smooth_fbm(flat_q * vec2f(0.8, 1.6) + vec2f(-p.extra.y * 3.0, 0.0));
    let cover = smoothstep(0.45, 0.75, clouds) * smoothstep(0.0, 0.15, height);
    col = mix(col, vec3f(0.95, 0.96, 0.98) * (0.85 + 0.15 * height), cover * 0.85);
    return col;
}

// Seeds drifting on the wind, glinting in the sun with the treble.
fn seeds(q: vec2f) -> vec3f {
    let drift = q * 9.0 + vec2f(-p.extra.x * 4.0, 0.6 * sin(q.x * 2.0 + p.time * 0.4));
    let cell = floor(drift);
    let h = hash22(cell + 11.0);
    if (h.x < 0.94) {
        return vec3f(0.0);
    }
    let pos = 0.2 + 0.6 * hash22(cell + 23.0);
    let d = length(fract(drift) - pos);
    let glint = 0.3 + 1.2 * p.treble + 0.6 * p.flash;
    // Only low over the field, not up in the sky.
    let low = 1.0 - smoothstep(HORIZON - 0.1, HORIZON + 0.25, q.y);
    return vec3f(1.0, 0.95, 0.8) * smoothstep(0.05, 0.0, d) * glint * low;
}

@fragment
fn fs(in: FullscreenOut) -> @location(0) vec4f {
    let q = centered(in.uv);
    let pixel = 2.0 / p.resolution.y;

    // Behind the grass: sky, distant hills, and the far field.
    var col: vec3f;
    // Two ranges of rolling hills: a pale far one, a greener near one.
    let far_hills = HORIZON + 0.02 + 0.09 * pow(smooth_fbm(vec2f(q.x * 0.7 + 8.0, 0.5)), 1.5);
    let near_hills = HORIZON + 0.005 + 0.05 * pow(smooth_fbm(vec2f(q.x * 1.1 + 3.0, 2.5)), 1.3);
    if (q.y > far_hills) {
        col = sky(q);
    } else if (q.y > near_hills) {
        col = mix(HILLS_FAR, SKY_LOW, 0.35);
    } else if (q.y > HORIZON) {
        col = mix(HILLS, HAZE, 0.4 + 0.3 * smoothstep(HORIZON, near_hills, q.y));
    } else {
        // Between and beyond the blades: dense undergrowth, fading into the
        // haze with distance, with the gusts passing over the far field as
        // paler bands.
        let z = CAM_HEIGHT / max(HORIZON - q.y, 1e-3);
        let far_gust = clamp(gust(q.x * z, z) - 0.2, 0.0, 1.0);
        let far = smoothstep(4.0, 20.0, z);
        col = mix(UNDERGROWTH, mix(GREEN, SHEEN, 0.4 * far_gust), far);
        col = mix(col, HAZE, 1.0 - exp(-(z - NEAREST) * 0.06));
    }

    // The grass, nearest row first, stopping once a pixel is covered.
    var grass = vec4f(0.0);
    var z = NEAREST;
    for (var i = 0; i < ROWS; i++) {
        let r = row(q, z, f32(i), pixel);
        grass += (1.0 - grass.a) * r;
        if (grass.a > 0.99) {
            break;
        }
        z *= ROW_GROWTH;
    }
    col = col * (1.0 - grass.a) + grass.rgb;

    col += seeds(q);
    let vignette = 1.0 - 0.3 * dot(in.uv - 0.5, in.uv - 0.5) * 2.0;
    return vec4f(col * vignette, 1.0);
}
