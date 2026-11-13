//! Colour helpers shared by visualizers.

/// HSV (hue wraps, saturation and value 0..1) to linear RGB.
pub fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = h.rem_euclid(1.0) * 6.0;
    let sector = h.floor();
    let f = h - sector;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match sector as u8 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5)
    }

    #[test]
    fn primaries() {
        assert!(approx(hsv(0.0, 1.0, 1.0), [1.0, 0.0, 0.0]));
        assert!(approx(hsv(1.0 / 3.0, 1.0, 1.0), [0.0, 1.0, 0.0]));
        assert!(approx(hsv(2.0 / 3.0, 1.0, 1.0), [0.0, 0.0, 1.0]));
    }

    #[test]
    fn hue_wraps() {
        assert!(approx(hsv(1.25, 0.5, 0.8), hsv(0.25, 0.5, 0.8)));
        assert!(approx(hsv(-0.75, 0.5, 0.8), hsv(0.25, 0.5, 0.8)));
    }

    #[test]
    fn zero_saturation_is_grey() {
        assert!(approx(hsv(0.42, 0.0, 0.5), [0.5, 0.5, 0.5]));
    }
}
