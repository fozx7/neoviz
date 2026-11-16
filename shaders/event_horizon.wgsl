// Event Horizon: a black hole seen from just above its accretion disk. Each
// pixel's light ray is bent around the hole, so the far side of the disk
// shows wrapped over and under the shadow, the galaxies behind it (a real
// Webb deep field) are lensed into arcs around it, and a thin photon ring
// outlines the shadow.
//
// Units: the event horizon has radius 1. The disk's rings follow the
// spectrum (bass outside, treble inside), it spins faster with the mids, and
// each beat sends a ripple outward through it.
//
// p.extra.x = disk spin clock, p.extra.y = camera drift clock,
// p.extra.z = loudness, smoothed (the overall glow).

const R_IN: f32 = 2.6;        // inner edge of the disk (just outside the last stable orbit)
const R_OUT: f32 = 11.0;
const CAM_DIST: f32 = 24.0;
const FOV: f32 = 0.42;        // half the view's height at unit distance
const MAX_STEPS: i32 = 240;
const ESCAPE_R: f32 = 40.0;
const PHOTON_SPHERE: f32 = 1.5;
// The backdrop photo: how much of it spans one radian of sky (so how big it
// looks), how bright it's shown, and how far its centre wanders.
const SKY_SCALE: f32 = 0.55;
const SKY_EXPOSURE: f32 = 0.9;
const SKY_PAN: f32 = 0.06;

// The backdrop (see ImageTexture): a deep-field photo, mirrored at its edges.
@group(1) @binding(0) var sky_image: texture_2d<f32>;
@group(1) @binding(1) var sky_sampler: sampler;

fn rot2(a: f32) -> mat2x2f {
    let c = cos(a);
    let s = sin(a);
    return mat2x2f(c, s, -s, c);
}

// Disk colour by temperature: deep orange at the rim to white-hot inside.
fn heat_colour(t: f32) -> vec3f {
    let ember = vec3f(0.55, 0.12, 0.02);
    let amber = vec3f(1.0, 0.48, 0.12);
    let gold = vec3f(1.0, 0.8, 0.5);
    let white = vec3f(1.0, 0.96, 0.9);
    var c = mix(ember, amber, smoothstep(0.0, 0.35, t));
    c = mix(c, gold, smoothstep(0.3, 0.75, t));
    return mix(c, white, smoothstep(0.7, 1.2, t));
}

// Light the disk gives off where a ray crosses its plane at `hit`, travelling
// along `ray`. Returns premultiplied colour and opacity.
fn disk(hit: vec3f, ray: vec3f) -> vec4f {
    let r = length(hit.xz);
    if (r < R_IN || r > R_OUT) {
        return vec4f(0.0);
    }
    let x = (r - R_IN) / (R_OUT - R_IN); // 0 inner edge .. 1 outer edge

    // Inner orbits spin faster (Kepler), which shears the gas into streaks.
    let spin = p.extra.x * 7.0 * pow(r, -1.5);
    let q = rot2(spin) * hit.xz / r; // a point on the unit circle
    let gas = fbm(vec2f(r * 4.0, 0.0) + q * 1.3);
    let fine = noise(vec2f(r * 13.0, 3.0) + q * 4.5);
    let density = 0.3 + 1.1 * gas * gas + 0.35 * fine;

    // The spectrum, laid out across the disk: treble inside, bass outside.
    let level = band_at(1.0 - x);
    let music = 0.5 + 1.4 * level + 0.7 * p.extra.z;

    // A beat's ripple runs outward as the flash fades.
    let ripple_r = R_IN + (1.0 - p.flash) * (R_OUT - R_IN);
    let ripple = p.flash * exp(-pow((r - ripple_r) * 1.4, 2.0));

    // Gas spinning towards us looks brighter and hotter than gas moving away.
    let v = sqrt(0.5 / r);
    let tangent = normalize(vec3f(-hit.z, 0.0, hit.x));
    let towards = dot(tangent, -ray);
    let doppler = sqrt(1.0 - v * v) / (1.0 - v * towards);
    let beaming = mix(1.0, pow(doppler, 3.0), 0.6);
    // Light climbing out of the well near the hole loses energy.
    let climb = sqrt(max(1.0 - 1.0 / r, 0.0));

    let heat = pow(R_IN / r, 1.6);
    let colour = heat_colour(heat * doppler * 1.1 + 0.4 * ripple);
    let edge = smoothstep(0.0, 0.06, x) * smoothstep(1.0, 0.55, x);
    let glow = (2.4 * heat + 0.1) * density * music * beaming * climb + 1.8 * ripple;
    let alpha = clamp(edge * (0.3 + 0.55 * density), 0.0, 1.0);
    return vec4f(colour * glow * edge, alpha);
}

// The sky in direction `d` (world space), seen by a camera with axes
// `right`, `up` and `forward`. The photo is laid around the view direction
// by angle (an azimuthal projection), which stays smooth even for light the
// hole bends round from behind the camera. `blur` adds mip levels where the
// lensing squeezes the photo, so it doesn't shimmer.
fn sky(d: vec3f, right: vec3f, up: vec3f, forward: vec3f, blur: f32) -> vec3f {
    let local = vec3f(dot(d, right), dot(d, up), dot(d, forward));
    let angle = acos(clamp(local.z, -1.0, 1.0));
    let across = normalize(local.xy + vec2f(1e-6, 0.0));
    let drift = SKY_PAN * vec2f(sin(p.extra.y * 1.3), cos(p.extra.y * 0.9));
    let uv = vec2f(0.5) + drift + vec2f(across.x, -across.y) * angle * SKY_SCALE;
    // About one photo pixel per screen pixel when unlensed.
    let texels = f32(textureDimensions(sky_image).y) * SKY_SCALE * 2.0 * FOV / p.resolution.y;
    let lod = max(log2(texels), 0.0) + blur;
    let photo = textureSampleLevel(sky_image, sky_sampler, uv, lod).rgb;
    return photo * SKY_EXPOSURE * (1.0 + 0.15 * p.treble + 0.2 * p.flash);
}

@fragment
fn fs(in: FullscreenOut) -> @location(0) vec4f {
    // The camera drifts slowly around the hole, bobbing just above the disk.
    let drift = p.extra.y;
    let yaw = drift * 3.0;
    let elevation = 0.13 + 0.05 * sin(drift * 5.0);
    let cam = CAM_DIST * vec3f(cos(elevation) * sin(yaw), sin(elevation), cos(elevation) * cos(yaw));
    let forward = normalize(-cam);
    let right = normalize(cross(forward, vec3f(0.0, 1.0, 0.0)));
    let up = cross(right, forward);
    let view = rot2(0.06 * sin(drift * 3.7)) * centered(in.uv);

    var pos = cam;
    var vel = normalize(forward + FOV * (view.x * right + view.y * up));
    // Bending light around the hole: this pull keeps the ray on the path
    // general relativity gives it, with h2 its (conserved) angular momentum².
    let c = cross(pos, vel);
    let h2 = dot(c, c);

    var col = vec3f(0.0);
    var alpha = 0.0;
    var closest = 1e9;
    var captured = false;
    for (var i = 0; i < MAX_STEPS; i++) {
        let r = length(pos);
        let step = clamp(0.09 * (r - 0.8), 0.02, 1.5);
        vel += -1.5 * h2 * pos / pow(r, 5.0) * step;
        let next = pos + vel * step;
        if (pos.y * next.y < 0.0) {
            let hit = mix(pos, next, pos.y / (pos.y - next.y));
            let d = disk(hit, normalize(vel));
            col += (1.0 - alpha) * d.rgb;
            alpha += (1.0 - alpha) * d.a;
        }
        pos = next;
        let nr = length(pos);
        closest = min(closest, nr);
        if (nr < 1.0) {
            captured = true;
            break;
        }
        if ((nr > ESCAPE_R && dot(pos, vel) > 0.0) || alpha > 0.98) {
            break;
        }
    }

    if (!captured) {
        // Rays that passed close to the photon sphere are squeezed hard.
        let squeeze = 3.0 * exp(-(closest - PHOTON_SPHERE) * 1.5);
        col += (1.0 - alpha) * sky(normalize(vel), right, up, forward, squeeze);
    }
    // Light that skimmed the photon sphere: the thin bright ring.
    let ring = exp(-pow((closest - PHOTON_SPHERE) * 9.0, 2.0));
    col += (1.0 - alpha) * vec3f(1.0, 0.78, 0.5) * ring * (0.5 + 0.8 * p.bass + 0.6 * p.flash);

    let vignette = 1.0 - 0.35 * dot(in.uv - 0.5, in.uv - 0.5) * 2.0;
    return vec4f(col * vignette, 1.0);
}
