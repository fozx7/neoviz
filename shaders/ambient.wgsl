// Ambient Swirl: soft, flowing colour fields (domain-warped noise) washed over
// a slowly zooming, rotating feedback buffer.
//
// p.extra.x = flow time (advances faster with bass), p.extra.y = hue offset.

@fragment
fn fs(in: FullscreenOut) -> @location(0) vec4f {
    let q = centered(in.uv) * 1.4;
    let t = p.extra.x;

    // Two rounds of domain warping give the marbled, flowing look.
    let w1 = vec2f(
        fbm(q + vec2f(t * 0.30, -t * 0.20)),
        fbm(q + vec2f(5.2, 1.3) - vec2f(t * 0.25)),
    );
    let w2 = vec2f(
        fbm(q + 3.0 * w1 + vec2f(1.7, 9.2) + vec2f(t * 0.15)),
        fbm(q + 3.0 * w1 + vec2f(8.3, 2.8) - vec2f(t * 0.10)),
    );
    let v = fbm(q + 4.0 * w2);

    let hue = p.extra.y + v * 0.5 + length(w2) * 0.3;
    let brightness = 0.2 + 1.1 * v * v * (0.6 + 0.9 * p.bass) + 0.4 * p.flash;
    let col = hsv(hue, 0.75 - 0.3 * p.treble, brightness);
    // Low alpha: each frame only nudges the feedback, so colours drift smoothly.
    return vec4f(col, 0.10 + 0.25 * p.flash);
}
