//! Alchemy Particles: a swirling particle field that bursts on beats, drawn as
//! glowing streaks over a fading feedback trail.

use std::f32::consts::TAU;

use crate::{
    audio::analysis::AudioFrame,
    render::{
        feedback::{Feedback, Warp},
        streaks::{Streak, StreakRenderer},
    },
};

use super::{Visualizer, color::hsv, common::Rng};

const MAX_PARTICLES: usize = 6000;
/// Particles per second emitted at silence, and extra per unit of RMS.
const BASE_EMIT_RATE: f32 = 40.0;
const RMS_EMIT_RATE: f32 = 600.0;
/// Particles in a beat burst at minimum intensity, and extra at full.
const BURST_BASE: f32 = 80.0;
const BURST_EXTRA: f32 = 320.0;
const LIFE_SECS: (f32, f32) = (1.5, 3.5);
/// Particles further than this from the centre are dropped.
const MAX_RADIUS: f32 = 3.0;
const DAMPING_PER_SEC: f32 = 0.8;
/// Streak length as seconds of travel.
const STREAK_SECS: f32 = 0.04;
const STREAK_WIDTH: f32 = 0.0025;
const HUE_SPEED: f32 = 0.03;
const WARP: Warp = Warp {
    zoom_per_sec: 1.05,
    rotation_per_sec: 0.08,
    fade_secs: 0.2,
};

/// The audio the simulation reacts to (kept separate from [`AudioFrame`] so
/// the simulation can be tested on its own).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Drive {
    rms: f32,
    mid: f32,
    treble: f32,
    /// Beat intensity on beat frames.
    beat: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Particle {
    pos: [f32; 2],
    vel: [f32; 2],
    age: f32,
    life: f32,
    hue: f32,
}

#[derive(Debug, Clone)]
struct Sim {
    particles: Vec<Particle>,
    rng: Rng,
    hue: f32,
    /// Fractional particles owed from previous frames' emission.
    emit_debt: f32,
}

impl Sim {
    fn new(seed: u32) -> Self {
        Self {
            particles: Vec::with_capacity(MAX_PARTICLES),
            rng: Rng::new(seed),
            hue: 0.0,
            emit_debt: 0.0,
        }
    }

    fn step(&mut self, drive: Drive, dt: f32) {
        self.hue += HUE_SPEED * dt;

        // Steady trickle from a ring around the centre.
        self.emit_debt += (BASE_EMIT_RATE + RMS_EMIT_RATE * drive.rms) * dt;
        while self.emit_debt >= 1.0 {
            self.emit_debt -= 1.0;
            self.emit_trickle();
        }
        if let Some(intensity) = drive.beat {
            self.burst(intensity);
        }

        let swirl = 0.6 + 1.5 * drive.mid;
        let damping = (-DAMPING_PER_SEC * dt).exp();
        let jitter = 0.5 * drive.treble;
        for p in &mut self.particles {
            let [x, y] = p.pos;
            // Tangential swirl plus a gentle pull to the centre.
            let ax = -y * swirl - x * 0.3 + self.rng.range(-jitter, jitter);
            let ay = x * swirl - y * 0.3 + self.rng.range(-jitter, jitter);
            p.vel = [
                (p.vel[0] + ax * dt) * damping,
                (p.vel[1] + ay * dt) * damping,
            ];
            p.pos = [x + p.vel[0] * dt, y + p.vel[1] * dt];
            p.age += dt;
        }
        self.particles
            .retain(|p| p.age < p.life && p.pos[0].hypot(p.pos[1]) < MAX_RADIUS);
    }

    fn spawn(&mut self, pos: [f32; 2], vel: [f32; 2], hue: f32) {
        if self.particles.len() >= MAX_PARTICLES {
            return;
        }
        let life = self.rng.range(LIFE_SECS.0, LIFE_SECS.1);
        self.particles.push(Particle {
            pos,
            vel,
            age: 0.0,
            life,
            hue,
        });
    }

    fn emit_trickle(&mut self) {
        let angle = self.rng.range(0.0, TAU);
        let (s, c) = angle.sin_cos();
        let speed = self.rng.range(0.05, 0.2);
        let hue = self.hue + self.rng.range(0.0, 0.08);
        self.spawn([c * 0.15, s * 0.15], [c * speed, s * speed], hue);
    }

    fn burst(&mut self, intensity: f32) {
        let origin = [self.rng.range(-0.4, 0.4), self.rng.range(-0.4, 0.4)];
        let count = (BURST_BASE + BURST_EXTRA * intensity) as usize;
        // Each burst gets a contrasting colour.
        let hue = self.hue + 0.5 + self.rng.range(-0.1, 0.1);
        for _ in 0..count {
            let angle = self.rng.range(0.0, TAU);
            let speed = self.rng.range(0.4, 1.2) * (0.5 + intensity);
            let (s, c) = angle.sin_cos();
            let shade = hue + self.rng.range(0.0, 0.05);
            self.spawn(origin, [c * speed, s * speed], shade);
        }
    }

    fn streaks(&self, out: &mut Vec<Streak>) {
        out.clear();
        out.extend(self.particles.iter().map(|p| {
            let fade = 1.0 - p.age / p.life;
            Streak {
                from: p.pos,
                to: [
                    p.pos[0] - p.vel[0] * STREAK_SECS,
                    p.pos[1] - p.vel[1] * STREAK_SECS,
                ],
                color: hsv(p.hue, 0.7, 0.9 * fade),
                width: STREAK_WIDTH,
            }
        }));
    }
}

pub struct Particles {
    sim: Sim,
    renderer: StreakRenderer,
    feedback: Feedback,
    streaks: Vec<Streak>,
    dt: f32,
}

impl Particles {
    pub fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
        Self {
            sim: Sim::new(0x5eed),
            renderer: StreakRenderer::new(device, MAX_PARTICLES),
            feedback: Feedback::new(device, size),
            streaks: Vec::with_capacity(MAX_PARTICLES),
            dt: 0.0,
        }
    }
}

impl Visualizer for Particles {
    fn name(&self) -> &str {
        "Alchemy Particles"
    }

    fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        self.feedback.resize(device, size);
    }

    fn update(&mut self, audio: &AudioFrame, dt: f32) {
        self.dt = dt;
        let drive = Drive {
            rms: audio.rms,
            mid: audio.mid,
            treble: audio.treble,
            beat: audio.beat.then_some(audio.beat_intensity),
        };
        self.sim.step(drive, dt);
        self.sim.streaks(&mut self.streaks);
    }

    fn render(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        self.feedback.warp(queue, encoder, &WARP, self.dt);
        self.renderer.set(queue, &self.streaks, size);
        self.renderer.draw(encoder, self.feedback.current(), false);
        self.feedback.finish(queue, encoder, target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn run(sim: &mut Sim, drive: Drive, secs: f32) {
        for _ in 0..(secs / DT) as usize {
            sim.step(drive, DT);
        }
    }

    #[test]
    fn trickles_particles_even_in_silence() {
        let mut sim = Sim::new(1);
        run(&mut sim, Drive::default(), 1.0);
        let n = sim.particles.len();
        assert!((30..=50).contains(&n), "{n} particles");
    }

    #[test]
    fn beats_burst() {
        let mut sim = Sim::new(1);
        let before = sim.particles.len();
        sim.step(
            Drive {
                beat: Some(1.0),
                ..Default::default()
            },
            DT,
        );
        assert!(sim.particles.len() - before >= (BURST_BASE + BURST_EXTRA) as usize - 1);
    }

    #[test]
    fn particles_die_of_old_age() {
        let mut sim = Sim::new(1);
        sim.burst(1.0);
        assert!(!sim.particles.is_empty());
        // No more emission: drive nothing and step past the longest life.
        sim.emit_debt = f32::NEG_INFINITY;
        run(&mut sim, Drive::default(), LIFE_SECS.1 + 0.1);
        assert!(sim.particles.is_empty());
    }

    #[test]
    fn never_exceeds_the_cap() {
        let mut sim = Sim::new(1);
        for _ in 0..100 {
            sim.step(
                Drive {
                    rms: 1.0,
                    beat: Some(1.0),
                    ..Default::default()
                },
                DT,
            );
        }
        // Full, minus any that died or drifted off during the last step.
        assert!(sim.particles.len() <= MAX_PARTICLES);
        assert!(sim.particles.len() > MAX_PARTICLES * 9 / 10);
    }

    #[test]
    fn streaks_fade_with_age_and_trail_behind() {
        let mut sim = Sim::new(1);
        sim.spawn([0.0, 0.0], [1.0, 0.0], 0.0);
        sim.particles[0].age = sim.particles[0].life * 0.5;
        let mut out = Vec::new();
        sim.streaks(&mut out);
        assert!(
            out[0].to[0] < out[0].from[0],
            "trail should point backwards"
        );
        let brightness = out[0].color.iter().copied().fold(0.0, f32::max);
        assert!((brightness - 0.45).abs() < 1e-5);
    }
}
