// Deep Nebula: drifting through a painterly nebula. Layers of coloured gas
// (teal, magenta, gold) at different depths slide past at different speeds,
// dark dust lanes cut across them, and bright stars shine through.
//
// Bass lights the big clouds at the back, the mids the middle layer, the
// treble the fine wisps in front (and makes stars twinkle). Each beat sends
// a wave of light out through the gas from the bright star at the centre.
//
// p.extra.x = gas flow clock, p.extra.y = drift clock (the camera's pan),
// p.extra.z = loudness, smoothed.

const DEEP: vec3f = vec3f(0.008, 0.01, 0.03);
const DEEP_GLOW: vec3f = vec3f(0.04, 0.02, 0.07);
const TEAL: vec3f = vec3f(0.1, 0.55, 0.65);
const MAGENTA: vec3f = vec3f(0.7, 0.16, 0.5);
const GOLD: vec3f = vec3f(1.0, 0.6, 0.25);
const DUST_RIM: vec3f = vec3f(0.9, 0.45, 0.2);
const CORE_POS: vec2f = vec2f(0.25, 0.1);

// One layer of gas: domain-warped noise. Returns density, 0..~1.
fn gas(q: vec2f, t: f32, seed: f32) -> f32 {
    let warp = vec2f(
        smooth_fbm(q + vec2f(seed, 0.3 * t)),
        smooth_fbm(q + vec2f(4.7 + seed, 1.9) - 0.25 * t),
    );
    let v = smooth_fbm(q + 2.4 * warp + vec2f(0.1 * t, seed));
    return smoothstep(0.42, 0.85, v);
}

// Stars at one depth: faint dots, and with `big` a few bright ones with
// diffraction spikes.
fn stars(q: vec2f, density: f32, big: bool) -> vec3f {
    let cell = floor(q);
    let h = hash22(cell + 17.0);
    if (h.x < 1.0 - density) {
        return vec3f(0.0);
    }
    let centre = 0.2 + 0.6 * hash22(cell + 31.0);
    let d = fract(q) - centre;
    let bright = (h.x - (1.0 - density)) / density;
    let twinkle = 1.0 + (0.2 + 0.8 * p.treble) * 0.35 * sin(p.time * (1.0 + 4.0 * h.y) + h.y * 50.0);
    let tint = mix(vec3f(0.7, 0.8, 1.0), vec3f(1.0, 0.85, 0.7), h.y);
    var light = smoothstep(0.06, 0.0, length(d)) * bright;
    if (big && bright > 0.6) {
        let spikes = exp(-abs(d.x) * 120.0) * exp(-abs(d.y) * 9.0)
            + exp(-abs(d.y) * 120.0) * exp(-abs(d.x) * 9.0);
        light += (0.02 / (dot(d, d) * 400.0 + 0.02) * 0.6 + spikes) * bright;
    }
    return tint * light * twinkle;
}

@fragment
fn fs(in: FullscreenOut) -> @location(0) vec4f {
    let uv = centered(in.uv);
    let t = p.extra.x;
    // The camera pans slowly; nearer layers slide past faster.
    let pan = vec2f(p.extra.y * 0.9, 0.25 * sin(p.extra.y * 1.3));

    var col = mix(DEEP, DEEP_GLOW, smoothstep(-1.0, 1.0, uv.y + 0.4 * uv.x));
    col += stars(uv * 90.0 + pan * 9.0, 0.05, false) * 0.6;

    // A beat's wave of light, travelling out from the core as the flash fades.
    let from_core = length(uv - CORE_POS);
    let wave = p.flash * exp(-pow((from_core - (1.0 - p.flash) * 2.0) * 3.0, 2.0));
    let lit = 1.0 + 1.5 * wave + 0.4 * p.extra.z;

    // Back: big, soft teal clouds (bass).
    let back = gas(uv * 0.9 + pan * 0.3, t * 0.6, 0.0);
    col += TEAL * back * back * (0.25 + 0.9 * p.bass) * lit;

    // Middle: magenta and gold, mixed by a slow colour field (mids).
    let mid_q = uv * 1.5 + pan * 0.6;
    let middle = gas(mid_q, t, 7.3);
    let hue = smooth_fbm(mid_q * 0.5 + 3.0);
    let mid_colour = mix(MAGENTA, GOLD, smoothstep(0.35, 0.7, hue));
    col += mid_colour * middle * (0.2 + 0.9 * p.mid) * lit;

    // The bright star at the core, lighting the gas around it.
    let core = exp(-from_core * 4.0);
    col += GOLD * core * (0.5 + middle) * (0.4 + 0.8 * p.bass + 1.2 * p.flash);

    // Dust lanes: dark bands of dust that block the glow, with lit edges.
    let dust_q = uv * 1.2 + pan * 0.8 + vec2f(0.05 * t, 0.0);
    let dust = smoothstep(0.48, 0.6, smooth_fbm(dust_q + 1.5 * smooth_fbm(dust_q * 0.7 + 9.0)));
    let dust_edge = dust * (1.0 - dust) * 4.0;
    col = col * (1.0 - 0.92 * dust) + DUST_RIM * dust_edge * (middle + back) * 0.3 * lit;

    // Front: fine bright wisps (treble), and nearer stars.
    let front_q = uv * 3.0 + pan * 1.4;
    let wisps = gas(front_q, t * 1.6, 13.1);
    col += mix(TEAL, vec3f(0.8, 0.9, 1.0), 0.5) * wisps * wisps * (0.06 + 0.5 * p.treble) * lit;
    col += stars(uv * 22.0 + pan * 30.0, 0.04, true) * (1.0 - 0.7 * dust);

    let vignette = 1.0 - 0.45 * dot(in.uv - 0.5, in.uv - 0.5) * 2.0;
    return vec4f(col * vignette, 1.0);
}
