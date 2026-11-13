//! Starfield Warp: stars stream toward the viewer, faster with the music, with
//! colour flashes on beats.

use crate::{
    audio::{analysis::AudioFrame, bands::smoothing_coef},
    render::streaks::{Streak, StreakRenderer},
};

use super::{
    Visualizer,
    color::hsv,
    common::{BeatFlash, Rng},
};

const STAR_COUNT: usize = 1500;
/// Depth range: stars spawn at FAR and are recycled once past NEAR.
const FAR: f32 = 1.0;
const NEAR: f32 = 0.03;
/// Depth units per second when silent, and extra at full energy.
const BASE_SPEED: f32 = 0.12;
const ENERGY_SPEED: f32 = 1.6;
/// How quickly the speed follows the music.
const SPEED_SMOOTHING_SECS: f32 = 0.3;
/// Streak length as seconds of travel; longer = more "warp".
const STREAK_SECS: f32 = 0.06;
/// Screen scale of the perspective projection.
const FOCAL: f32 = 0.5;
const FLASH_DECAY_SECS: f32 = 0.3;
const STAR_WHITE: [f32; 3] = [0.8, 0.85, 1.0];
const HUE_SPEED: f32 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Star {
    /// Position across the field, -1..1.
    x: f32,
    y: f32,
    /// Distance from the viewer, NEAR..FAR.
    z: f32,
}

#[derive(Debug, Clone)]
struct Field {
    stars: Vec<Star>,
    rng: Rng,
    speed: f32,
}

impl Field {
    fn new(seed: u32) -> Self {
        let mut rng = Rng::new(seed);
        let stars = (0..STAR_COUNT)
            .map(|_| Star {
                x: rng.range(-1.0, 1.0),
                y: rng.range(-1.0, 1.0),
                z: rng.range(NEAR, FAR),
            })
            .collect();
        Self {
            stars,
            rng,
            speed: BASE_SPEED,
        }
    }

    /// Moves stars closer by the speed for `energy` (0..~1), recycling any
    /// that pass the viewer or leave the screen (`aspect` wide).
    fn step(&mut self, energy: f32, aspect: f32, dt: f32) {
        let target = BASE_SPEED + ENERGY_SPEED * energy.clamp(0.0, 1.5);
        self.speed += (target - self.speed) * smoothing_coef(SPEED_SMOOTHING_SECS, dt);
        let step = self.speed * dt;
        for star in &mut self.stars {
            star.z -= step;
            let [sx, sy] = project(star.x, star.y, star.z);
            if star.z < NEAR || sx.abs() > aspect + 0.1 || sy.abs() > 1.1 {
                *star = Star {
                    x: self.rng.range(-1.0, 1.0),
                    y: self.rng.range(-1.0, 1.0),
                    z: FAR,
                };
            }
        }
    }

    fn streaks(&self, tint: [f32; 3], flash: f32, out: &mut Vec<Streak>) {
        let trail = self.speed * STREAK_SECS;
        out.clear();
        out.extend(self.stars.iter().map(|s| {
            let nearness = 1.0 - (s.z - NEAR) / (FAR - NEAR);
            let brightness = nearness.powf(1.5) * (1.0 + flash);
            let color: [f32; 3] = std::array::from_fn(|i| {
                (STAR_WHITE[i] + (tint[i] - STAR_WHITE[i]) * flash) * brightness
            });
            Streak {
                from: project(s.x, s.y, s.z),
                to: project(s.x, s.y, (s.z + trail).min(FAR)),
                color,
                width: 0.001 + 0.004 * nearness,
            }
        }));
    }
}

fn project(x: f32, y: f32, z: f32) -> [f32; 2] {
    [x / z * FOCAL, y / z * FOCAL]
}

pub struct Starfield {
    field: Field,
    renderer: StreakRenderer,
    streaks: Vec<Streak>,
    flash: BeatFlash,
    aspect: f32,
    time: f32,
}

impl Starfield {
    pub fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
        Self {
            field: Field::new(0x57a2),
            renderer: StreakRenderer::new(device, STAR_COUNT),
            streaks: Vec::with_capacity(STAR_COUNT),
            flash: BeatFlash::new(FLASH_DECAY_SECS),
            aspect: aspect(size),
            time: 0.0,
        }
    }
}

impl Visualizer for Starfield {
    fn name(&self) -> &str {
        "Starfield Warp"
    }

    fn resize(&mut self, _device: &wgpu::Device, size: (u32, u32)) {
        self.aspect = aspect(size);
    }

    fn update(&mut self, audio: &AudioFrame, dt: f32) {
        self.time += dt;
        let flash = self.flash.update(audio, dt);
        self.field
            .step(audio.bass + audio.rms * 0.5, self.aspect, dt);
        let tint = hsv(self.time * HUE_SPEED, 0.8, 1.0);
        self.field.streaks(tint, flash, &mut self.streaks);
    }

    fn render(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        self.renderer.set(queue, &self.streaks, size);
        self.renderer.draw(encoder, target, true);
    }
}

fn aspect(size: (u32, u32)) -> f32 {
    size.0.max(1) as f32 / size.1.max(1) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn stars_approach_the_viewer() {
        let mut f = Field::new(1);
        f.stars = vec![Star {
            x: 0.01,
            y: 0.01,
            z: 0.5,
        }];
        f.step(0.0, 16.0 / 9.0, DT);
        assert!(f.stars[0].z < 0.5);
    }

    #[test]
    fn music_speeds_things_up() {
        let mut quiet = Field::new(1);
        let mut loud = Field::new(1);
        for _ in 0..60 {
            quiet.step(0.0, 1.0, DT);
            loud.step(1.0, 1.0, DT);
        }
        assert!(loud.speed > quiet.speed * 3.0);
    }

    #[test]
    fn passed_stars_respawn_far_away() {
        let mut f = Field::new(1);
        f.stars = vec![Star {
            x: 0.0,
            y: 0.0,
            z: NEAR + 0.0001,
        }];
        f.step(1.0, 1.0, DT);
        assert_eq!(f.stars[0].z, FAR);
    }

    #[test]
    fn stars_leaving_the_screen_respawn() {
        let mut f = Field::new(1);
        f.stars = vec![Star {
            x: 0.9,
            y: 0.0,
            z: 0.2, // projects to x = 2.25, off a 16:9 screen
        }];
        f.step(0.0, 16.0 / 9.0, DT);
        assert_eq!(f.stars[0].z, FAR);
    }

    #[test]
    fn near_stars_are_brighter_and_wider() {
        let mut f = Field::new(1);
        f.stars = vec![
            Star {
                x: 0.1,
                y: 0.1,
                z: 0.9,
            },
            Star {
                x: 0.1,
                y: 0.1,
                z: 0.1,
            },
        ];
        let mut out = Vec::new();
        f.streaks(STAR_WHITE, 0.0, &mut out);
        assert!(out[1].color[0] > out[0].color[0]);
        assert!(out[1].width > out[0].width);
    }
}
