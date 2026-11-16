// Burning World: a desert planet seen from orbit as firestorms sweep across
// it. Walls of flame advance over the sand, leaving charred ground with
// glowing embers and drifting smoke; the atmosphere glows orange at the edge
// and embers rise off into space.
//
// The flames grow with the bass, the spectrum is laid from pole to pole
// (bass at the south), and beats make the whole firestorm flare.
//
// p.extra.x = fire clock (the fronts' advance), p.extra.y = planet spin,
// p.extra.z = loudness, smoothed.

const PLANET_CENTRE: vec2f = vec2f(0.45, -1.5);
const PLANET_RADIUS: f32 = 1.55;
const AXIAL_TILT: f32 = 0.35;
const SUN_DIR: vec3f = vec3f(-0.62, 0.5, 0.6);
const SAND_DARK: vec3f = vec3f(0.4, 0.19, 0.07);
const SAND_LIGHT: vec3f = vec3f(0.85, 0.5, 0.22);
const CHAR: vec3f = vec3f(0.035, 0.025, 0.02);
const SMOKE: vec3f = vec3f(0.06, 0.05, 0.05);
const AIR_GLOW: vec3f = vec3f(1.0, 0.42, 0.12);
// How many fire fronts circle the planet at once.
const FRONTS: f32 = 2.5;

fn rot_y(a: f32) -> mat3x3f {
    let c = cos(a);
    let s = sin(a);
    return mat3x3f(c, 0.0, -s, 0.0, 1.0, 0.0, s, 0.0, c);
}

fn rot_z(a: f32) -> mat3x3f {
    let c = cos(a);
    let s = sin(a);
    return mat3x3f(c, s, 0.0, -s, c, 0.0, 0.0, 0.0, 1.0);
}

// Flame colour for heat 0..~2: deep red through orange to yellow-white.
fn flame(h: f32) -> vec3f {
    let red = vec3f(0.6, 0.06, 0.01);
    let orange = vec3f(1.0, 0.38, 0.05);
    let yellow = vec3f(1.0, 0.8, 0.35);
    var c = mix(red, orange, smoothstep(0.0, 0.5, h));
    c = mix(c, yellow, smoothstep(0.5, 1.2, h));
    return c * h;
}

// The planet's surface at point `s` (unit sphere, in the planet's own frame),
// with normal `n` (camera frame) for lighting.
fn surface(s: vec3f, n: vec3f) -> vec3f {
    let t = p.extra.x;
    let sun = max(dot(n, normalize(SUN_DIR)), 0.0);

    // Sand: dunes and darker rock.
    let dunes = fbm3(s * 4.0 + 1.5 * fbm3(s * 2.0));
    let sand = mix(SAND_DARK, SAND_LIGHT, dunes) * (0.08 + 1.1 * sun);

    // Fire fronts: bands of a noisy "fuel" field moving with the fire clock.
    // `phase` is how long ago the front passed: 0 = burning now.
    // Fine noise makes the fronts ragged rather than smooth lines.
    let fuel = fbm3(s * 1.8 + 5.0);
    let ragged = 0.06 * fbm3(s * 12.0 + vec3f(0.0, t * 2.0, 0.0));
    let phase = fract(fuel * FRONTS + s.x * 0.6 + ragged - t);
    let flicker = fbm3(s * 14.0 + vec3f(0.0, t * 6.0, t * 3.0));
    let music = 0.9 + 1.0 * band_at(0.5 + 0.5 * s.y) + 1.0 * p.flash;
    // The flames: a bright wall at the front, with a lower burn behind it.
    let front = (exp(-phase * 35.0) + 0.25 * exp(-phase * 8.0))
        * (0.5 + flicker) * music * (1.0 + 0.8 * p.bass);

    // Behind the front: charred ground with embers, cooling, until the sand
    // drifts back over it.
    let burnt = smoothstep(0.0, 0.03, phase) * (1.0 - smoothstep(0.55, 0.95, phase));
    let cooling = 1.0 - smoothstep(0.02, 0.5, phase);
    let embers = pow(fbm3(s * 22.0 + t * 0.4), 3.0) * 3.0 * cooling * (0.6 + p.bass);
    var col = mix(sand, CHAR * (0.3 + sun), burnt);
    col += flame(embers) * burnt * 0.6;

    // Smoke trails the front, darkening the lit ground and glowing from below.
    let smoke_shape = fbm3(s * 5.0 + vec3f(t * 0.8, 0.0, -t * 0.5));
    let smoke = smoothstep(0.35, 0.75, smoke_shape) * (1.0 - smoothstep(0.0, 0.35, phase));
    col = mix(col, SMOKE * (0.3 + sun) + flame(0.4) * 0.15, smoke * 0.75);

    return col + flame(front * 2.0) * (1.0 - 0.4 * smoke);
}

// Stars, a distant sun, and embers rising off the planet.
fn space(uv: vec2f, above: f32) -> vec3f {
    let cell = floor(uv * 160.0);
    let h = hash22(cell);
    var col = vec3f(0.004, 0.003, 0.008);
    if (h.x > 0.985) {
        let d = length(fract(uv * 160.0) - 0.5);
        let twinkle = 0.7 + 0.3 * sin(p.time * (1.5 + 3.0 * h.y) + h.y * 40.0);
        col += vec3f(0.8, 0.85, 1.0) * smoothstep(0.3, 0.0, d) * (h.x - 0.985) * 60.0 * twinkle;
    }
    let sun_dist = length(uv - vec2f(-1.45, 0.85));
    col += vec3f(1.0, 0.85, 0.6) * (exp(-sun_dist * 9.0) * 0.5 + exp(-sun_dist * 40.0) * 2.0);

    // Embers: drifting upwards, fading as they rise away from the surface.
    let rise = uv * 22.0 + vec2f(0.4 * sin(uv.y * 3.0 + p.time * 0.3), -p.extra.x * 6.0);
    let ember_cell = floor(rise);
    let e = hash22(ember_cell + 3.0);
    if (e.x > 0.93) {
        let pos = 0.2 + 0.6 * hash22(ember_cell + 7.0);
        let d = length(fract(rise) - pos);
        let fade = exp(-above * 3.0) * (0.5 + p.bass + p.flash);
        col += flame(1.0) * smoothstep(0.08, 0.0, d) * fade * 1.5;
    }
    return col;
}

@fragment
fn fs(in: FullscreenOut) -> @location(0) vec4f {
    let uv = centered(in.uv);
    let d = (uv - PLANET_CENTRE) / PLANET_RADIUS;
    let r2 = dot(d, d);
    let r = sqrt(r2);
    let fire_glow = 0.6 + 0.8 * p.extra.z + 1.2 * p.flash;

    var col: vec3f;
    if (r2 < 1.0) {
        let n = vec3f(d, sqrt(1.0 - r2));
        let s = rot_y(p.extra.y) * (rot_z(AXIAL_TILT) * n);
        col = surface(s, n);
        // Looking through more air towards the edge: the glow of the fires.
        let edge = pow(1.0 - n.z, 3.0);
        col = mix(col, AIR_GLOW * fire_glow * 0.6, edge * 0.7);
    } else {
        col = space(uv, r - 1.0);
    }
    // The atmosphere's halo just outside the planet.
    let halo = exp(-max(r - 1.0, 0.0) * 30.0) * smoothstep(0.97, 1.0, r);
    col += AIR_GLOW * halo * fire_glow * 0.9;

    let vignette = 1.0 - 0.35 * dot(in.uv - 0.5, in.uv - 0.5) * 2.0;
    return vec4f(col * vignette, 1.0);
}
