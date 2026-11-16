// Deep Nebula: a glowing nebula behind a band of dark dust. Three lights
// hidden behind the dust (one each for the bass, mids and treble) drift
// slowly and swell with their part of the music, so light bursts out above
// and below the band, catches its edges, and streams out through the gaps
// in rays. The glow fades from white-gold through peach and pink to a deep
// blue-violet sky.
//
// p.extra.x = dust flow clock, p.extra.y = light drift clock,
// p.extra.z = loudness, smoothed.

const SKY_EDGE: vec3f = vec3f(0.012, 0.016, 0.06);
const SKY_MID: vec3f = vec3f(0.1, 0.05, 0.17);
const HAZE: vec3f = vec3f(0.75, 0.38, 0.6);
const DUST_DARK: vec3f = vec3f(0.025, 0.01, 0.018);
const DUST_LIT: vec3f = vec3f(0.9, 0.38, 0.22);
// Colours of the bass, mid and treble lights.
const BASS_LIGHT: vec3f = vec3f(1.0, 0.62, 0.3);
const MID_LIGHT: vec3f = vec3f(1.0, 0.82, 0.62);
const TREBLE_LIGHT: vec3f = vec3f(1.0, 0.62, 0.82);
// Samples taken towards the lights for the rays through the dust.
const RAY_STEPS: i32 = 14;
const RAY_REACH: f32 = 0.6;   // how far towards the lights the rays look
const RAY_DECAY: f32 = 0.92;

// Where the band of dust runs: its centre line wanders a little.
fn dust_band(q: vec2f) -> f32 {
    let centre = 0.05 * sin(q.x * 1.3 + 0.5) - 0.03 * q.x;
    let width = 0.17 + 0.07 * sin(q.x * 2.1 + 1.0);
    return exp(-pow((q.y - centre) / width, 2.0));
}

// Dust density, 0..1: the band broken up into clumps and wisps.
fn dust(q: vec2f, t: f32) -> f32 {
    let warp = vec2f(
        smooth_fbm(q * 1.8 + vec2f(0.2 * t, 0.0)),
        smooth_fbm(q * 1.8 + vec2f(5.2, -0.15 * t)),
    );
    // Big clumps, plus fine wispy detail.
    let clumps = smooth_fbm(q * 2.6 + 1.4 * warp)
        + 0.18 * (smooth_fbm(q * 9.0 + 3.0 * warp) - 0.5);
    let b = dust_band(q);
    // Thick in the middle of the band, ragged at its edges, with stray
    // clouds trailing off to the sides.
    let core = b * smoothstep(0.3, 0.62, clumps + 0.22 * b);
    let strays = (1.0 - b) * 0.6 * smoothstep(0.58, 0.75, clumps) * exp(-abs(q.y) * 2.5);
    return clamp(core + strays, 0.0, 1.0);
}

// A cheaper dust estimate for the rays (they sample it many times).
fn dust_lite(q: vec2f, t: f32) -> f32 {
    let b = dust_band(q);
    return b * smoothstep(0.3, 0.62, smooth_fbm(q * 2.6 + vec2f(0.1 * t, 0.0)) + 0.22 * b);
}

struct Light {
    pos: vec2f,
    colour: vec3f,
    power: f32,
}

// A point on a flat ellipse around the centre (the dust band runs across
// it, so a light on this path passes behind the band and out the other side),
// with a slight wobble so the path never quite repeats.
fn orbit(angle: f32, radius: vec2f) -> vec2f {
    let wobble = 1.0 + 0.12 * sin(angle * 2.3 + 1.0);
    return vec2f(cos(angle), sin(angle)) * radius * wobble;
}

// The three lights, each slowly circling behind the dust on its own path
// (the treble one the opposite way), about once every 1.5 to 2 minutes.
fn lights() -> array<Light, 3> {
    let a = p.extra.y * TAU;
    let swell = 1.0 + 0.4 * p.extra.z + 0.8 * p.flash;
    return array<Light, 3>(
        Light(orbit(a * 0.5, vec2f(0.55, 0.24)),
              BASS_LIGHT, (0.35 + 1.6 * p.bass) * swell),
        Light(orbit(a * 0.37 + 2.1, vec2f(0.72, 0.3)),
              MID_LIGHT, (0.3 + 1.4 * p.mid) * swell),
        Light(orbit(-a * 0.43 + 4.2, vec2f(0.48, 0.2)),
              TREBLE_LIGHT, (0.25 + 1.6 * p.treble) * swell),
    );
}

// Light reaching `q` from the lights, before the dust blocks any of it:
// a bright core and a wide soft halo for each.
fn glow(q: vec2f, ls: array<Light, 3>) -> vec3f {
    var sum = vec3f(0.0);
    for (var i = 0; i < 3; i++) {
        let d = length(q - ls[i].pos);
        let shape = 3.0 * exp(-d * d * 22.0) + 0.5 * exp(-d * 4.5);
        sum += ls[i].colour * shape * ls[i].power;
    }
    return sum;
}

// Hot light looks whiter: push bright glow towards white.
fn whiten(c: vec3f) -> vec3f {
    let lum = dot(c, vec3f(0.3, 0.5, 0.2));
    return mix(c, vec3f(lum), smoothstep(0.8, 2.5, lum) * 0.6);
}

fn stars(q: vec2f) -> vec3f {
    let g = q * 70.0;
    let cell = floor(g);
    let h = hash22(cell + 17.0);
    if (h.x < 0.92) {
        return vec3f(0.0);
    }
    let d = fract(g) - (0.2 + 0.6 * hash22(cell + 31.0));
    let bright = (h.x - 0.92) / 0.08;
    let twinkle = 1.0 + (0.2 + 0.8 * p.treble) * 0.4 * sin(p.time * (1.0 + 4.0 * h.y) + h.y * 50.0);
    let tint = mix(vec3f(0.7, 0.8, 1.0), vec3f(1.0, 0.75, 0.8), h.y);
    var light = smoothstep(0.07, 0.0, length(d)) * bright;
    if (bright > 0.85) {
        let spikes = exp(-abs(d.x) * 90.0) * exp(-abs(d.y) * 7.0)
            + exp(-abs(d.y) * 90.0) * exp(-abs(d.x) * 7.0);
        light += spikes * bright;
    }
    return tint * light * twinkle;
}

@fragment
fn fs(in: FullscreenOut) -> @location(0) vec4f {
    let uv = centered(in.uv);
    let t = p.extra.x;
    // A very slow sway, so the scene breathes without wandering off.
    let q = uv * 0.85 + 0.04 * vec2f(sin(p.extra.y * 2.0), cos(p.extra.y * 1.5));
    let ls = lights();

    // The sky: deep blue-violet, warming towards the middle.
    let centre_dist = length(q * vec2f(0.6, 1.0));
    var col = mix(SKY_MID, SKY_EDGE, smoothstep(0.1, 1.2, centre_dist));
    col += stars(uv);

    // Glow from the lights, partly scattered in front of the dust as haze.
    let light_here = glow(q, ls);
    let thickness = dust(q, t);
    let through = exp(-thickness * 4.5);
    col += HAZE * 0.05 * dot(light_here, vec3f(0.33)) * (1.0 + p.extra.z);
    col += whiten(light_here) * through;

    // Rays: walk towards the lights' centre, adding up light that got past
    // the dust along the way.
    var centre = vec2f(0.0);
    var total = 0.0;
    for (var i = 0; i < 3; i++) {
        centre += ls[i].pos * ls[i].power;
        total += ls[i].power;
    }
    centre /= max(total, 1e-3);
    let step = (centre - q) * RAY_REACH / f32(RAY_STEPS);
    var ray_q = q;
    var weight = 1.0;
    var rays = vec3f(0.0);
    for (var i = 0; i < RAY_STEPS; i++) {
        ray_q += step;
        let open = exp(-dust_lite(ray_q, t) * 4.0);
        rays += glow(ray_q, ls) * open * weight;
        weight *= RAY_DECAY;
    }
    col += rays * (0.06 + 0.04 * p.flash) * (1.0 - 0.5 * thickness);

    // The dust itself: dark, but its thin edges glow with the light behind.
    let edge = thickness * (1.0 - thickness) * 4.0;
    let backlight = dot(light_here, vec3f(0.33));
    let dust_colour = DUST_DARK + DUST_LIT * edge * backlight * 0.6;
    col = mix(col, dust_colour, smoothstep(0.0, 0.6, thickness) * 0.95);

    let vignette = 1.0 - 0.4 * dot(in.uv - 0.5, in.uv - 0.5) * 2.0;
    return vec4f(col * vignette, 1.0);
}
