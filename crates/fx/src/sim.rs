//! Particle simulation: spawn, per-tick update and emission of one `eCParticle_PS`.
//!
//! The engine steps every emitter with a fixed tick (`TicksPerSecond`, default 25) fed by an
//! accumulator (`eCParticle_PS::DoProcessV1`, Engine:0x1014fcc0); each tick calls
//! `ProcessParticles` (Engine:0x1014ffd0) which updates the live particles and then emits new
//! ones. All vectors are centimetres in the world space the emitter transform is given in.
use crate::params::{EmitterParams, Range3, VelocityFrom};
use crate::rng::Rng;
use crate::scale::{color_scale, float_scale, looped, vector_scale};
use bevy::math::{Mat3, Mat4, Vec3};
use std::sync::Arc;

/// One live particle (`eCParticle2`, 0xb4 bytes in the engine).
#[derive(Clone, Debug)]
pub struct Particle {
    pub pos: Vec3,
    pub prev: Vec3,
    pub vel: Vec3,
    /// Velocity factor curve value (multiplies the velocity when moving).
    pub vel_scale: Vec3,
    pub rev_center: Vec3,
    pub rev_speed: Vec3,
    /// Current spin angles (radians) around the quad's right, up and normal axes.
    pub rot: Vec3,
    /// Spin speed (turns per second) per axis.
    pub spin: Vec3,
    pub size0: Vec3,
    pub size_factor: Vec3,
    pub color: [f32; 4],
    pub color_factor: [f32; 4],
    pub age: f32,
    pub life: f32,
    /// Fractional sub-image index.
    pub frame: f32,
    pub dead: bool,
}

impl Particle {
    /// Normalised age 0..1.
    pub fn t(&self) -> f32 {
        if self.life != 0.0 {
            self.age / self.life
        } else {
            0.0
        }
    }

    /// Current half extents (cm).
    pub fn size(&self) -> Vec3 {
        self.size0 * self.size_factor
    }
}

/// Port of eCParticle_PS::CalcColorFactor (Engine:0x1014f790): the fade-in/out colour
/// multiplier from the particle age (seconds) and life.
pub fn color_factor(p: &EmitterParams, age: f32, life: f32) -> [f32; 4] {
    let mut fin = [1.0; 4];
    let mut fout = [1.0; 4];
    if p.fade_in && age < p.fade_in_end {
        let x = if p.fade_in_end > 0.0 {
            1.0 - (age / p.fade_in_end).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let f = p.fade_in_factor;
        fin = [
            1.0 - x * (1.0 - f[0]),
            1.0 - (1.0 - f[1]) * x,
            1.0 - (1.0 - f[2]) * x,
            1.0 - x * f[3],
        ];
    }
    if p.fade_out && p.fade_out_start <= age {
        let x = if p.fade_out_start < life {
            ((age - p.fade_out_start) / (life - p.fade_out_start)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let f = p.fade_out_factor;
        fout = [
            1.0 - (1.0 - f[0]) * x,
            1.0 - (1.0 - f[1]) * x,
            1.0 - (1.0 - f[2]) * x,
            1.0 - x * f[3],
        ];
    }
    [
        fout[0] * fin[0],
        fout[1] * fin[1],
        fout[2] * fin[2],
        fout[3] * fin[3],
    ]
}

/// Port of eCParticle_PS::CalcFrame (Engine:0x1014f4e0): the (fractional) sub-image at life
/// fraction `t`.
pub fn calc_frame(p: &EmitterParams, t: f32) -> f32 {
    if p.use_subdivision_scale {
        return float_scale(&p.curves.frame, looped(t, p.subdivision_scale_repeats));
    }
    let count = p.frame_count();
    let start = p.subdivision_start.max(0);
    let end = p.subdivision_end.min(count - 1);
    if end == start {
        return start as f32;
    }
    ((end - start + 1) as f32 * t + start as f32).min(end as f32)
}

fn v3(a: [f32; 3]) -> Vec3 {
    Vec3::from_array(a)
}

/// A running emitter.
#[derive(Clone, Debug)]
pub struct Emitter {
    pub params: Arc<EmitterParams>,
    pub particles: Vec<Particle>,
    rng: Rng,
    /// Emitter transform of this frame / the previous tick (spawn positions interpolate).
    pub xf: Mat4,
    prev_origin: Vec3,
    spawn_accum: f32,
    /// Particles queued by `trigger` (`+0x334`), emitted at `TriggerParticlesPerSecond`.
    burst_left: f32,
    delay: f32,
    time_accum: f32,
    warmed: bool,
    finished: bool,
    first: bool,
    stopped: bool,
}

impl Emitter {
    pub fn new(params: Arc<EmitterParams>, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let delay = params.initial_delay.sample(&mut rng).max(0.0);
        Self {
            particles: Vec::with_capacity(params.max_particles),
            params,
            rng,
            xf: Mat4::IDENTITY,
            prev_origin: Vec3::ZERO,
            spawn_accum: 0.0,
            burst_left: 0.0,
            delay,
            time_accum: 0.0,
            warmed: false,
            finished: false,
            first: true,
            stopped: false,
        }
    }

    pub fn origin(&self) -> Vec3 {
        self.xf.w_axis.truncate()
    }

    /// True once an auto-kill emitter has lost all its particles, or a stopped emitter has
    /// let its last particle die.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Port of eCParticle_PS::Reset (Engine:0x101521f0): drop the particles and start over (the emitter restarts
    /// with its warm-up when it is next processed).
    pub fn reset(&mut self) {
        self.particles.clear();
        self.spawn_accum = 0.0;
        self.burst_left = 0.0;
        self.time_accum = 0.0;
        self.warmed = false;
    }

    /// Stop emitting; live particles run out (the engine's `Decay` of a spawned effect entity).
    pub fn stop(&mut self) {
        self.stopped = true;
        self.burst_left = 0.0;
        if self.particles.is_empty() {
            self.finished = true;
        }
    }

    /// Port of eCParticle_PS::Trigger (Engine:0x1014ef20): queue `count` extra particles.
    pub fn trigger(&mut self, count: f32) {
        self.burst_left += count.max(0.0);
    }

    /// Move the emitter (call before `update`).
    pub fn set_transform(&mut self, xf: Mat4) {
        if self.first {
            self.prev_origin = xf.w_axis.truncate();
            self.first = false;
        }
        self.xf = xf;
    }

    /// Port of eCParticle_PS::DoProcessV1 (Engine:0x1014fcc0): advance by `dt` seconds of frame
    /// time with the fixed-tick accumulator.
    pub fn update(&mut self, dt: f32) {
        if !self.params.enabled || self.finished {
            return;
        }
        if !self.warmed {
            self.warmed = true;
            self.prev_origin = self.origin();
            self.warm_up();
        }
        if self.delay > 0.0 {
            self.delay -= dt;
            if self.delay >= 0.0 {
                return;
            }
            self.time_accum -= self.delay;
            self.delay = 0.0;
        } else {
            self.time_accum += dt;
        }
        let step = if self.params.ticks_per_second > 0.0 {
            1.0 / self.params.ticks_per_second
        } else {
            self.time_accum.max(1.0e-3)
        };
        let n = ((self.time_accum / step) as usize).min(8);
        // Frames longer than eight ticks drop the excess time instead of spiralling.
        if self.time_accum / step > 8.0 {
            self.time_accum = step * 8.0;
        }
        for k in 0..n {
            self.process(step, k as f32 / n as f32, (k + 1) as f32 / n as f32);
            self.time_accum -= step;
        }
        self.prev_origin = self.origin();
    }

    fn warm_up(&mut self) {
        let p = &self.params;
        let total = p.relative_warmup_time * p.max_life();
        if total <= 0.0 || p.warmup_ticks_per_second <= 0.0 {
            return;
        }
        let step = 1.0 / p.warmup_ticks_per_second;
        let n = (total / step).ceil() as usize;
        for _ in 0..n.min(512) {
            self.process(step, 0.0, 1.0);
        }
    }

    /// Port of eCParticle_PS::ProcessParticles (Engine:0x1014ffd0): one fixed tick. `t0..t1`
    /// is the span of the frame this tick covers (spawn times are spread over it).
    pub fn process(&mut self, dt: f32, t0: f32, t1: f32) {
        let p = Arc::clone(&self.params);
        let mut dead = 0;
        for i in 0..self.particles.len() {
            self.tick_particle(i, dt);
            if self.particles[i].dead {
                dead += 1;
            }
        }
        let had = self.particles.len();
        if p.respawn_dead {
            self.particles.retain(|q| !q.dead);
        }
        let capacity = p.max_particles.saturating_sub(self.particles.len());
        if self.stopped {
            self.particles.retain(|q| !q.dead);
            if self.particles.is_empty() {
                self.finished = true;
            }
            return;
        }
        if p.automatic_spawning {
            self.spawn_accum = 0.0;
            self.burst_left = 0.0;
            for _ in 0..capacity {
                self.spawn(t1);
            }
        } else {
            self.spawn_accum += dt * p.particles_per_second;
            if self.burst_left > 0.0 {
                let take = (self.burst_left).min(dt * 100.0).max(1.0);
                self.spawn_accum += take;
                self.burst_left = (self.burst_left - take).max(0.0);
            }
            let n = (self.spawn_accum as usize).min(capacity);
            for k in 1..=n {
                self.spawn((k as f32 / n as f32) * (t1 - t0) + t0);
            }
            self.spawn_accum -= n as f32;
        }
        // All particles dead: auto kill ends the emitter (the engine kills the entity).
        let all_dead = if p.respawn_dead {
            false
        } else {
            had != 0 && dead == had
        };
        if all_dead && p.auto_kill {
            self.finished = true;
        }
    }

    /// Port of the particle part of eCParticle_PS::TickParticles (Engine:0x10155800).
    fn tick_particle(&mut self, i: usize, dt: f32) {
        let p = Arc::clone(&self.params);
        let q = &mut self.particles[i];
        if q.dead {
            return;
        }
        q.age += dt;
        if q.age > q.life {
            q.dead = true;
            return;
        }
        let t = q.t();
        q.prev = q.pos;
        // x += v * scale * dt, then the constant acceleration term and the velocity update.
        q.pos += q.vel * q.vel_scale * dt;
        let acc = v3(p.acceleration);
        q.pos += acc * (dt * dt * 0.5);
        q.vel += acc * dt;
        for c in 0..3 {
            let m = p.max_velocity[c];
            if m != 0.0 {
                q.vel[c] = q.vel[c].clamp(-m, m);
            }
        }
        // Velocity loss: a random per-axis fraction of the velocity per second.
        if p.velocity_loss != Range3::default() {
            let loss = p.velocity_loss.sample(&mut self.rng);
            q.vel -= q.vel * Vec3::from_array(loss) * dt;
        }
        q.vel_scale = if p.use_velocity_scale {
            v3(vector_scale(
                &p.curves.velocity,
                looped(t, p.velocity_scale_repeats),
            ))
        } else {
            Vec3::ONE
        };
        if p.use_revolution {
            let factor = if p.use_revolution_scale {
                v3(vector_scale(
                    &p.curves.revolution,
                    looped(t, p.revolution_scale_repeats),
                ))
            } else {
                Vec3::ONE
            };
            let turns = q.rev_speed * factor * dt * std::f32::consts::TAU;
            let rot = Mat3::from_euler(bevy::math::EulerRot::XYZ, turns.x, turns.y, turns.z);
            q.pos = rot * (q.pos - q.rev_center) + q.rev_center;
        }
        if p.spin {
            q.rot += q.spin * (std::f32::consts::TAU * dt);
        }
        q.size_factor = if p.use_size_scale {
            v3(vector_scale(
                &p.curves.size,
                looped(t, p.size_scale_repeats),
            ))
        } else {
            Vec3::ONE
        };
        if !p.random_subdivision {
            q.frame = calc_frame(&p, t);
        }
        q.color = calc_color(&p, t);
        q.color_factor = color_factor(&p, q.age, q.life);
    }

    /// Port of eCParticle_PS::Spawn (Engine:0x10150530). `frac` places the spawn inside the
    /// frame so a moving emitter leaves a continuous trail.
    fn spawn(&mut self, frac: f32) {
        let p = Arc::clone(&self.params);
        let life = p.lifetime.sample(&mut self.rng);
        let age = p.initial_age.sample(&mut self.rng);
        let rng = &mut self.rng;
        let origin_now = self.xf.w_axis.truncate();
        let origin = self.prev_origin.lerp(origin_now, frac);
        let rot3 = Mat3::from_mat4(self.xf);
        let mut local = if p.start_shape == 1 {
            v3(rng.direction()) * p.start_sphere_radius.sample(rng)
        } else {
            v3(p.start_box.sample(rng))
        };
        local += v3(p.start_offset);
        if p.rotate_with_entity {
            local = rot3 * local;
        }
        let mut pos = local;
        let mut vel = v3(p.start_velocity.sample(rng));
        match p.velocity_from {
            VelocityFrom::StartPositionAndOwner => {
                vel = -local.normalize_or_zero() * vel.length();
            }
            VelocityFrom::OwnerAndStartPosition => {
                vel = local.normalize_or_zero() * vel.length();
                pos -= local;
            }
            VelocityFrom::None => {
                if p.rotate_with_entity {
                    vel = rot3 * vel;
                }
            }
        }
        if !p.relative {
            pos += origin;
        }
        let (rev_speed, rev_center) = if p.use_revolution {
            let speed = v3(p.revolutions_per_second.sample(rng));
            let mut center = v3(p.revolution_center.sample(rng));
            if !p.relative {
                center += origin;
            }
            (speed, center)
        } else {
            (Vec3::ZERO, Vec3::ZERO)
        };
        let rot = v3(p.start_spin.sample(rng));
        let mut spin = v3(p.spins_per_second.sample(rng));
        for c in 0..3 {
            // SpinDirection: each axis flips with this probability.
            if p.spin_direction <= rng.unit() {
                spin[c] = -spin[c];
            }
        }
        let mut size0 = v3(p.start_size.sample(rng));
        if p.uniform_size {
            size0.y = size0.x;
            size0.z = size0.x;
        }
        let t = if life != 0.0 { age / life } else { 0.0 };
        let size_factor = if p.use_size_scale {
            v3(vector_scale(
                &p.curves.size,
                looped(t, p.size_scale_repeats),
            ))
        } else {
            Vec3::ONE
        };
        let frame = if p.random_subdivision {
            let count = p.frame_count();
            let lo = p.subdivision_start.max(0);
            let hi = p.subdivision_end.min(count - 1);
            if hi <= lo {
                lo as f32
            } else {
                (lo + (rng.unit() * (hi - lo + 1) as f32) as i32).min(hi) as f32
            }
        } else {
            calc_frame(&p, t)
        };
        let particle = Particle {
            pos,
            prev: pos,
            vel,
            vel_scale: Vec3::ONE,
            rev_center,
            rev_speed,
            rot,
            spin,
            size0,
            size_factor,
            color: calc_color(&p, t),
            color_factor: color_factor(&p, age, life),
            age,
            life,
            frame,
            // A particle that starts older than its life is dead on arrival.
            dead: age > life,
        };
        self.particles.push(particle);
    }

    /// Number of live particles.
    pub fn live(&self) -> usize {
        self.particles.iter().filter(|q| !q.dead).count()
    }
}

/// Port of eCParticle_PS::CalcColor (Engine:0x1014f5d0): the colour-scale curve.
pub fn calc_color(p: &EmitterParams, t: f32) -> [f32; 4] {
    if p.use_color_scale {
        color_scale(&p.curves.color, looped(t, p.color_scale_repeats))
    } else {
        [1.0; 4]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{Range1, Range3};

    fn torch() -> EmitterParams {
        EmitterParams {
            max_particles: 100,
            particles_per_second: 100.0,
            lifetime: Range1::new(0.5, 0.5),
            start_shape: 1,
            start_sphere_radius: Range1::new(5.0, 5.0),
            start_velocity: Range3 {
                min: [-1.0, -3.0, -1.0],
                max: [1.0, 3.0, 1.0],
            },
            acceleration: [0.0, 400.0, 0.0],
            start_size: Range3 {
                min: [15.0; 3],
                max: [30.0; 3],
            },
            ..EmitterParams::default()
        }
    }

    #[test]
    fn steady_state_holds_rate_times_life_particles() {
        let mut e = Emitter::new(Arc::new(torch()), 1);
        e.set_transform(Mat4::IDENTITY);
        for _ in 0..120 {
            e.update(1.0 / 60.0);
        }
        // 100 per second with 0.5 s life: about 50 alive.
        let n = e.live();
        assert!((46..=54).contains(&n), "live {n}");
    }

    #[test]
    fn acceleration_lifts_particles() {
        let mut e = Emitter::new(Arc::new(torch()), 2);
        e.set_transform(Mat4::IDENTITY);
        for _ in 0..30 {
            e.update(1.0 / 60.0);
        }
        let old = e
            .particles
            .iter()
            .filter(|q| q.age > 0.3)
            .map(|q| q.pos.y)
            .fold(0.0_f32, f32::max);
        // 0.5 * 400 * 0.3^2 = 18 cm of lift at least for the oldest particles.
        assert!(old > 10.0, "lift {old}");
    }

    #[test]
    fn one_shot_emitters_finish_when_everything_died() {
        let p = EmitterParams {
            automatic_spawning: true,
            respawn_dead: false,
            auto_kill: true,
            max_particles: 10,
            lifetime: Range1::new(0.2, 0.2),
            ..EmitterParams::default()
        };
        let mut e = Emitter::new(Arc::new(p), 3);
        e.set_transform(Mat4::IDENTITY);
        assert!(!e.is_finished());
        for _ in 0..60 {
            e.update(1.0 / 25.0);
        }
        assert!(e.is_finished());
    }

    #[test]
    fn fade_in_starts_transparent() {
        let p = EmitterParams {
            fade_in: true,
            fade_in_end: 0.5,
            fade_in_factor: [1.0, 1.0, 1.0, 1.0],
            ..EmitterParams::default()
        };
        assert_eq!(color_factor(&p, 0.0, 2.0)[3], 0.0);
        assert_eq!(color_factor(&p, 0.5, 2.0)[3], 1.0);
        let mid = color_factor(&p, 0.25, 2.0)[3];
        assert!((mid - 0.5).abs() < 1e-6);
    }

    #[test]
    fn fade_out_ends_transparent() {
        let p = EmitterParams {
            fade_out: true,
            fade_out_start: 1.0,
            fade_out_factor: [1.0, 1.0, 1.0, 1.0],
            ..EmitterParams::default()
        };
        assert_eq!(color_factor(&p, 0.5, 2.0)[3], 1.0);
        assert_eq!(color_factor(&p, 2.0, 2.0)[3], 0.0);
    }

    #[test]
    fn frame_runs_over_the_life_without_scale() {
        let p = EmitterParams {
            u_subdivisions: 5,
            v_subdivisions: 1,
            subdivision_start: 0,
            subdivision_end: 11,
            ..EmitterParams::default()
        };
        assert_eq!(calc_frame(&p, 0.0), 0.0);
        assert_eq!(calc_frame(&p, 0.5), 6.0);
        assert_eq!(calc_frame(&p, 1.0), 11.0);
    }

    #[test]
    fn moving_emitter_leaves_a_trail() {
        let p = EmitterParams {
            max_particles: 100,
            particles_per_second: 1000.0,
            lifetime: Range1::new(5.0, 5.0),
            ..EmitterParams::default()
        };
        let mut e = Emitter::new(Arc::new(p), 4);
        e.set_transform(Mat4::IDENTITY);
        e.update(0.0);
        e.set_transform(Mat4::from_translation(Vec3::new(100.0, 0.0, 0.0)));
        e.update(0.2);
        let xs: Vec<f32> = e.particles.iter().map(|q| q.pos.x).collect();
        assert!(xs.iter().any(|x| *x > 50.0) && xs.iter().any(|x| *x < 50.0));
    }
}
