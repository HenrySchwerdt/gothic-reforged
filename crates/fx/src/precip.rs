//! Rain and snow: the particle system of `eCPrecipitation_PS` (Engine.dll:0x10292c00..).
//!
//! The emitter lives in a cube around the camera (`PrecipitationCubeSize`). A set of spawn points
//! (`MaxSpawnPoints`, 500 in the world) remembers where the ground (or a roof) lies below it, found
//! once with a ray (`Update`, 0x10293190, `TraceRayNearestHit`); particles spawn at a random spawn
//! point near the top of the cube, fall along `Direction` with a random speed in `SpeedMinMax`,
//! and when they reach the stored ground height become a splash (`SplashDuration`, growing by
//! `SplashSizeScale` per second, tile `SplashTextureTile`) or vanish. A particle leaving the cube
//! dies. Drawing: camera facing quads whose up axis follows the fall direction projected on the
//! screen, stretched by `1 + |projected direction| * DirectionScale` (rain streaks); snow
//! flakes wobble with `JitterPower` / `JitterSpeed`.
//!
//! Units are centimetres, Y up, in the same (origin-relative) space as the camera.
use crate::{
    draw::{Quad, View},
    rng::Rng,
};
use bevy::math::{Vec2, Vec3};
use gothic_environment::precip::PrecipParams;

/// Host query: the height of the first surface below `(x, z)`, searching downward from `from_y`
/// for at most `depth` centimetres.
pub trait Ground {
    fn ground_y(&mut self, x: f32, z: f32, from_y: f32, depth: f32) -> Option<f32>;
}

/// Open ground at a constant height (tests, headless runs).
pub struct FlatGround(pub f32);

impl Ground for FlatGround {
    fn ground_y(&mut self, _x: f32, _z: f32, from_y: f32, depth: f32) -> Option<f32> {
        (self.0 <= from_y && self.0 >= from_y - depth).then_some(self.0)
    }
}

#[derive(Clone, Copy, Debug)]
struct SpawnPoint {
    x: f32,
    z: f32,
    /// Ground height below the point once the ray has been cast; `Some(None)` = open sky.
    ground: Option<Option<f32>>,
}

#[derive(Clone, Copy, Debug)]
struct Flake {
    pos: Vec3,
    speed: f32,
    ground: Option<f32>,
    /// Remaining splash time; 0 = still falling.
    splash: f32,
    size: Vec2,
    tile: u32,
    phase: f32,
}

/// Rays cast per frame to resolve spawn points.
const RAYS_PER_FRAME: usize = 32;

/// The simulated precipitation.
pub struct Precipitation {
    pub params: PrecipParams,
    spawn: Vec<SpawnPoint>,
    drops: Vec<Flake>,
    rng: Rng,
    carry: f32,
    time: f32,
}

impl Precipitation {
    pub fn new(params: PrecipParams, seed: u64) -> Self {
        Self {
            params,
            spawn: Vec::new(),
            drops: Vec::new(),
            rng: Rng::new(seed),
            carry: 0.0,
            time: 0.0,
        }
    }

    /// Live particles (`GetNumParticlesAlive`).
    pub fn alive(&self) -> usize {
        self.drops.len()
    }

    fn direction(&self) -> Vec3 {
        Vec3::from_array(self.params.direction).normalize_or(Vec3::NEG_Y)
    }

    fn cube_contains(&self, centre: Vec3, p: Vec3, scale: f32) -> bool {
        let h = self.params.cube_size * 0.5 * scale;
        (p.x - centre.x).abs() <= h && (p.y - centre.y).abs() <= h && (p.z - centre.z).abs() <= h
    }

    fn random_spawn_position(&mut self, centre: Vec3) -> (f32, f32) {
        let h = self.params.cube_size * 0.5;
        (
            centre.x + (self.rng.unit() * 2.0 - 1.0) * h,
            centre.z + (self.rng.unit() * 2.0 - 1.0) * h,
        )
    }

    /// Advance by `dt` seconds with the camera at `cam`.
    pub fn step(&mut self, dt: f32, cam: Vec3, ground: &mut impl Ground) {
        self.time += dt;
        let want = self.params.max_spawn_points as usize;
        let half = self.params.cube_size * 0.5;

        // Spawn points: recycle those that left the cube, resolve a few per frame.
        while self.spawn.len() < want {
            let (x, z) = self.random_spawn_position(cam);
            self.spawn.push(SpawnPoint { x, z, ground: None });
        }
        self.spawn.truncate(want);
        for i in 0..self.spawn.len() {
            let sp = self.spawn[i];
            if (sp.x - cam.x).abs() > half || (sp.z - cam.z).abs() > half {
                let (x, z) = self.random_spawn_position(cam);
                self.spawn[i] = SpawnPoint { x, z, ground: None };
            }
        }
        let mut budget = RAYS_PER_FRAME;
        for sp in self.spawn.iter_mut().filter(|s| s.ground.is_none()) {
            if budget == 0 {
                break;
            }
            budget -= 1;
            sp.ground =
                Some(ground.ground_y(sp.x, sp.z, cam.y + half, self.params.cube_size * 1.5));
        }

        // Existing particles.
        let dir = self.direction();
        let p = self.params.clone();
        let cube_centre = cam;
        let splash_tile = p.splash_tile;
        let mut keep = Vec::with_capacity(self.drops.len());
        for mut d in std::mem::take(&mut self.drops) {
            if d.splash > 0.0 {
                d.splash -= dt;
                d.size += Vec2::from_array(p.splash_size_scale) * dt;
                if d.splash > 0.0 && d.size.x > 0.0 && d.size.y > 0.0 {
                    keep.push(d);
                }
                continue;
            }
            d.pos += dir * d.speed * dt;
            if let Some(g) = d.ground
                && d.pos.y <= g
            {
                if p.splash_duration > 0.0 {
                    d.pos.y = g;
                    d.splash = p.splash_duration;
                    d.tile = splash_tile;
                    keep.push(d);
                }
                continue;
            }
            if !self.cube_contains(cube_centre, d.pos, 1.0) {
                continue;
            }
            keep.push(d);
        }
        self.drops = keep;

        // New particles at the configured rate.
        self.carry += self.params.particles_per_second * dt;
        let mut spawn_now = self.carry.floor() as usize;
        self.carry -= spawn_now as f32;
        let room = (self.params.max_particles as usize).saturating_sub(self.drops.len());
        spawn_now = spawn_now.min(room);
        if self.spawn.is_empty() {
            return;
        }
        let dizzer = self.params.spawn_point_dizzer;
        for _ in 0..spawn_now {
            let sp =
                self.spawn[(self.rng.unit() * self.spawn.len() as f32) as usize % self.spawn.len()];
            let Some(ground_at) = sp.ground else {
                continue;
            };
            let top = cam.y + half - self.rng.unit() * half * 0.25;
            let pos = Vec3::new(
                sp.x + (self.rng.unit() * 2.0 - 1.0) * dizzer,
                top,
                sp.z + (self.rng.unit() * 2.0 - 1.0) * dizzer,
            );
            // A roof (or the ground) above the spawn height cannot be reached from the sky.
            if ground_at.is_some_and(|g| g >= pos.y) {
                continue;
            }
            let [lo, hi] = self.params.speed_min_max;
            self.drops.push(Flake {
                pos,
                speed: lo + (hi - lo) * self.rng.unit(),
                ground: ground_at,
                splash: 0.0,
                size: Vec2::from_array(self.params.size),
                tile: self.params.current_tile,
                phase: self.rng.unit() * std::f32::consts::TAU,
            });
        }
    }

    /// Billboards of all particles for a camera (`UpdateVertices` of the precipitation).
    pub fn quads(&self, view: &View) -> Vec<Quad> {
        let dir = self.direction();
        // Fall direction in view space (x right, y up).
        let dv = Vec2::new(dir.dot(view.right), dir.dot(view.up));
        let stretch = dv.length() * self.params.direction_scale + 1.0;
        // right = cross((0,0,1), d) = (-d.y, d.x); up = cross((0,0,1), right).
        let r = Vec2::new(-dv.y, dv.x).normalize_or(Vec2::X);
        let u = Vec2::new(-r.y, r.x);
        let right_w = view.right * r.x + view.up * r.y;
        let up_w = view.right * u.x + view.up * u.y;
        let [cols, rows] = self.params.tile_count;
        let wobble = self.params.jitter_power * 0.1;
        let speed = self.params.jitter_speed * 1000.0;
        let mut out = Vec::with_capacity(self.drops.len());
        for d in &self.drops {
            let mut c = d.pos;
            if d.splash <= 0.0 && wobble > 0.0 && speed > 0.0 {
                let a = self.time * speed + d.phase;
                c.x += a.sin() * wobble;
                c.z += a.cos() * wobble;
            }
            if !view.sees(c, d.size.max_element() * stretch) {
                continue;
            }
            let k = if d.splash > 0.0 { 1.0 } else { stretch };
            let rv = right_w * d.size.x;
            let uv = up_w * d.size.y * k;
            let (u0, v0) = (
                (d.tile % cols) as f32 / cols as f32,
                (d.tile / cols) as f32 / rows as f32,
            );
            out.push(Quad {
                corners: [c - rv + uv, c + rv + uv, c + rv - uv, c - rv - uv],
                uv_a: [u0, v0, u0 + 1.0 / cols as f32, v0 + 1.0 / rows as f32],
                uv_b: [0.0; 4],
                blend: 0.0,
                color: [1.0; 4],
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Quat;

    fn rain() -> PrecipParams {
        PrecipParams {
            particles_per_second: 3100.0,
            size: [1.0, 1.0],
            direction_scale: 14.0,
            current_tile: 2,
            splash_duration: 0.4,
            splash_tile: 3,
            splash_size_scale: [25.0, 25.0],
            ..PrecipParams::default()
        }
    }

    fn run(p: &mut Precipitation, secs: f32, cam: Vec3, ground: &mut impl Ground) {
        let steps = (secs * 60.0) as usize;
        for _ in 0..steps {
            p.step(1.0 / 60.0, cam, ground);
        }
    }

    #[test]
    fn no_rate_no_particles() {
        let mut p = Precipitation::new(PrecipParams::default(), 1);
        run(&mut p, 2.0, Vec3::ZERO, &mut FlatGround(-500.0));
        assert_eq!(p.alive(), 0);
    }

    #[test]
    fn rain_fills_the_cube_up_to_the_budget_and_splashes() {
        let mut p = Precipitation::new(rain(), 3);
        let mut g = FlatGround(-300.0);
        run(&mut p, 3.0, Vec3::ZERO, &mut g);
        assert!(p.alive() > 500 && p.alive() <= 3000, "alive {}", p.alive());
        // Some particles are splashes on the ground.
        let view = View::new(Vec3::ZERO, Quat::IDENTITY);
        let quads = p.quads(&view);
        assert!(!quads.is_empty());
        assert!(
            quads.iter().any(|q| q.uv_a[0] == 0.5 && q.uv_a[1] == 0.5),
            "splash tile 3"
        );
        assert!(
            quads.iter().any(|q| q.uv_a[0] == 0.0 && q.uv_a[1] == 0.5),
            "rain tile 2"
        );
    }

    #[test]
    fn particles_stay_inside_the_cube_and_above_the_ground() {
        let mut p = Precipitation::new(rain(), 9);
        let mut g = FlatGround(-300.0);
        run(&mut p, 2.0, Vec3::ZERO, &mut g);
        for d in &p.drops {
            assert!(p.cube_contains(Vec3::ZERO, d.pos, 1.01), "{:?}", d.pos);
            assert!(d.pos.y >= -300.0 - 1e-3);
        }
    }

    #[test]
    fn a_roof_keeps_the_rain_off() {
        // A roof just below the top of the cube shields almost every spawn point.
        let mut open = Precipitation::new(rain(), 5);
        run(&mut open, 2.0, Vec3::ZERO, &mut FlatGround(-300.0));
        let mut roofed = Precipitation::new(rain(), 5);
        run(&mut roofed, 2.0, Vec3::ZERO, &mut FlatGround(499.0));
        assert!(
            roofed.alive() * 20 < open.alive(),
            "{} vs {}",
            roofed.alive(),
            open.alive()
        );
    }

    #[test]
    fn rain_streaks_are_stretched_snow_is_not() {
        let mut p = Precipitation::new(rain(), 11);
        run(&mut p, 1.0, Vec3::ZERO, &mut FlatGround(-300.0));
        let view = View::new(Vec3::ZERO, Quat::IDENTITY);
        let q = p
            .quads(&view)
            .into_iter()
            .find(|q| q.uv_a[0] == 0.0 && q.uv_a[1] == 0.5)
            .expect("a falling drop");
        let height = (q.corners[0] - q.corners[3]).length();
        let width = (q.corners[0] - q.corners[1]).length();
        assert!(
            (height / width - 15.0).abs() < 0.1,
            "stretch {}",
            height / width
        );
    }

    #[test]
    fn same_seed_same_rain() {
        let go = || {
            let mut p = Precipitation::new(rain(), 21);
            run(&mut p, 1.0, Vec3::ZERO, &mut FlatGround(-300.0));
            (p.alive(), p.drops.first().map(|d| d.pos))
        };
        assert_eq!(go(), go());
    }
}
