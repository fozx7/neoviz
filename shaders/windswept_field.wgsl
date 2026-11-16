// Windswept Field: a moor of long, fine grass under a heavy overcast sky,
// with the wind rolling through it in gusts. Blades bend with the gust
// passing over them and turn paler where they're pressed flat, so the gusts
// show as pale waves sweeping across the field.
//
// The view looks out over the field from a high angle: rows of blades at
// increasing distance (nearest first), each row smaller and hazier, up to a
// low horizon.
//
// The grass only follows slow changes in the music, so it never twitches:
// the gusts grow and travel faster with the bass and loudness, both smoothed
// over about half a second.
//
// p.extra.x = wind clock (the gusts' travel), p.extra.y = cloud clock,
// p.extra.z = loudness, smoothed; p.extra.w = bass, smoothed.

const HORIZON: f32 = 0.5;      // screen height of the horizon (y up, -1..1)
const CAM_HEIGHT: f32 = 2.2;   // eye height above the ground
const NEAREST: f32 = 1.2;      // distance of the nearest row (roots below the screen)
const ROW_GROWTH: f32 = 1.085; // each row this much further than the last
const ROWS: i32 = 44;
const BLADE_HEIGHT: f32 = 1.5;
const BLADE_WIDTH: f32 = 0.013;
const BLADE_SPACING: f32 = 0.009;

// --- Colours: an overcast moor -----------------------------------------------
const SKY_LOW: vec3f = vec3f(0.36, 0.41, 0.4);
const SKY_TOP: vec3f = vec3f(0.12, 0.15, 0.16);
const CLOUD_DARK: vec3f = vec3f(0.1, 0.12, 0.13);
const CLOUD_LIGHT: vec3f = vec3f(0.5, 0.55, 0.53);
const HAZE: vec3f = vec3f(0.3, 0.36, 0.33);
const UNDERGROWTH: vec3f = vec3f(0.035, 0.07, 0.035);
const GREEN: vec3f = vec3f(0.07, 0.15, 0.06);
const SAGE: vec3f = vec3f(0.17, 0.25, 0.13);
const SHEEN: vec3f = vec3f(0.38, 0.46, 0.36);

// How strongly the wind is blowing right now, overall.
fn wind_strength() -> f32 {
    return 0.45 + 0.4 * p.extra.z + 0.6 * p.extra.w;
}

// The gust at ground position (x, z): 0 = calm, ~1+ = a strong gust. Gusts
// are broad patches of noise travelling with the wind (left to right).
fn gust(x: f32, z: f32) -> f32 {
    let t = p.extra.x;
    let g = smooth_noise(vec2f(x * 0.45 - t * 1.4, z * 0.5 + 0.3 * t));
    return smoothstep(0.25, 0.85, g) * wind_strength();
}

// How far a blade at (x, z) bends: its tip moves this fraction of its height
// downwind. A gentle sway everywhere, plus the gusts.
fn bend(x: f32, z: f32, phase: f32) -> f32 {
    let sway = 0.12 * sin(p.extra.x * 2.2 + x * 0.9 + phase * 6.0);
    return 0.25 + sway + 0.9 * gust(x, z);
}

// A blade's colour `t` of the way up it (0 root .. 1 tip). `h` varies it
// per blade, `clump` (0..1) by patch of the field, and `flattened` (0..1)
// is how hard the wind is pressing it over.
fn grass_colour(t: f32, h: vec2f, clump: f32, flattened: f32) -> vec3f {
    let base = mix(GREEN, SAGE, clamp(0.25 * h.x + 0.75 * clump, 0.0, 1.0));
    // Blades differ a lot in brightness, and tips catch more light.
    let tone = (0.55 + 0.9 * h.y * h.y) * (0.8 + 0.5 * smoothstep(0.5, 1.0, t));
    let colour = mix(UNDERGROWTH, base * tone, smoothstep(0.0, 0.5, t));
    // Grass pressed flat shows its paler side.
    return mix(colour, SHEEN, 0.9 * clamp(flattened, 0.0, 1.0) * t);
}

// Patches of lighter and darker grass across the field (0..1).
fn clumps(x: f32, z: f32) -> f32 {
    return smoothstep(0.25, 0.75, smooth_noise(vec2f(x * 0.9, z * 0.9) + 7.0));
}

// One row of blades at distance `z`: the colour and coverage (premultiplied)
// of whatever blade covers screen point `q`.
fn row(q: vec2f, z: f32, index: f32, pixel: f32) -> vec4f {
    // Blades sit a little in front of or behind the row's line (so rows
    // don't show as stripes); skip pixels no blade of this row can reach.
    let spread = 1.5 * (ROW_GROWTH - 1.0);
    let lowest = HORIZON - CAM_HEIGHT / (z * (1.0 - spread));
    let highest = HORIZON + (BLADE_HEIGHT - CAM_HEIGHT) / (z * (1.0 - spread));
    if (q.y < lowest || q.y > highest) {
        return vec4f(0.0);
    }
    let cell = floor(q.x * z / BLADE_SPACING);
    var best = vec4f(0.0);
    // Bent blades lean right, so they can reach in from cells to the left.
    for (var k = -8; k <= 1; k++) {
        let c = cell + f32(k);
        let h = hash22(vec2f(c, index * 37.0));
        let zb = z * (1.0 + (hash22(vec2f(c, index * 37.0 + 11.0)).x - 0.5) * 2.0 * spread);
        let s = 1.0 / zb;
        let ground = HORIZON - CAM_HEIGHT * s;
        let x = q.x * zb;
        let root = (c + h.x) * BLADE_SPACING;
        let height = BLADE_HEIGHT * (0.5 + 0.5 * h.y);
        let lean = bend(root, zb, h.x) + (h.y - 0.5) * 0.3;
        // How far up the blade this pixel is (0 root .. 1 tip). Bending
        // lowers the tip.
        let t = (q.y - ground) / (height * s * (1.0 - 0.3 * clamp(abs(lean), 0.0, 1.2)));
        if (t < 0.0 || t > 1.0) {
            continue;
        }
        let centre = root + lean * height * pow(t, 1.6);
        let half_width = 0.5 * BLADE_WIDTH * (1.0 - 0.75 * t);
        let coverage = clamp(0.5 - (abs(x - centre) - half_width) / (pixel * zb), 0.0, 1.0);
        if (coverage > best.a) {
            var colour = grass_colour(t, h, clumps(root, zb), (lean - 0.4) * 1.3);
            colour = mix(colour, HAZE, 1.0 - exp(-(zb - NEAREST) * 0.03));
            best = vec4f(colour * coverage, coverage);
        }
    }
    return best;
}

// A heavy overcast sky: masses of cloud, darker overhead.
fn sky(q: vec2f) -> vec3f {
    let height = clamp((q.y - HORIZON) / (1.0 - HORIZON), 0.0, 1.0);
    let col = mix(SKY_LOW, SKY_TOP, pow(height, 0.8));
    // Clouds, flattened towards the horizon and drifting with the wind.
    let flat_q = vec2f(q.x / (height + 0.7), 1.0 / (height + 0.7));
    let drift = vec2f(-p.extra.y * 3.0, 0.0);
    let masses = smooth_fbm(flat_q * vec2f(1.1, 1.6) + drift);
    let detail = smooth_fbm(flat_q * vec2f(3.0, 4.0) + drift * 1.3 + 4.0);
    let light = smoothstep(0.3, 0.75, masses + 0.25 * (detail - 0.5));
    let cloud = mix(CLOUD_DARK, CLOUD_LIGHT, light) * (1.0 - 0.45 * height);
    return mix(col, cloud, 0.85);
}

fn looking_out(q: vec2f, pixel: f32) -> vec3f {
    // Behind the grass: sky, the far edge of the moor, and the far field.
    var col: vec3f;
    let rise = HORIZON + 0.012 + 0.03 * smooth_fbm(vec2f(q.x * 0.8 + 8.0, 0.5));
    if (q.y > rise) {
        col = sky(q);
    } else if (q.y > HORIZON) {
        col = mix(HAZE, SKY_LOW, 0.3);
    } else {
        // Between and beyond the blades: dense undergrowth, fading into the
        // haze with distance; the far field rolls gently and shows the gusts
        // as paler bands.
        let z = CAM_HEIGHT / max(HORIZON - q.y, 1e-3);
        let far_gust = clamp(gust(q.x * z, z) - 0.3, 0.0, 1.0);
        let rolling = smooth_fbm(vec2f(q.x * z * 0.12, z * 0.25));
        let far = smoothstep(4.0, 25.0, z);
        let field = mix(mix(GREEN, SAGE, rolling), SHEEN, 0.5 * far_gust) * (0.75 + 0.5 * rolling);
        col = mix(UNDERGROWTH, field, far);
        col = mix(col, HAZE, 1.0 - exp(-(z - NEAREST) * 0.03));
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
    return col * (1.0 - grass.a) + grass.rgb;
}

@fragment
fn fs(in: FullscreenOut) -> @location(0) vec4f {
    let q = centered(in.uv);
    let pixel = 2.0 / p.resolution.y;
    let col = looking_out(q, pixel);
    let vignette = 1.0 - 0.35 * dot(in.uv - 0.5, in.uv - 0.5) * 2.0;
    return vec4f(col * vignette, 1.0);
}
