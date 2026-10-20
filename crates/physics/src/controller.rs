//! Character controller following the rules of the original `gCCharacterMovement_PS`.
//!
//! The original does not use PhysX' character controller. It drives a rigid-body capsule with
//! its own mode machine (standing / sneaking / walking / running / sprinting, jump, slide,
//! fall, swim) and traces rays to find the ground (`PutToGround`). This module reproduces the
//! observable rules:
//!
//! * ground contact within `StepHeight` above or below the feet keeps the character walking;
//!   a larger drop starts the fall state (V, `CheckChangedMovementStates`);
//! * obstacles lower than `StepHeight` do not block (they are climbed), higher ones do;
//! * speed accelerates with `MoveAcceleration` up to `ForwardSpeedMax * gait modifier`, scaled
//!   on slopes by the angle between `SteepGroundAngleMin` and `SteepGroundAngleMax`
//!   (uphill slower, downhill faster by `WalkDownSpeedScale`); steeper ground is not walkable
//!   and makes the character slide at up to `SlideSpeedMax`;
//! * a jump leaves the ground with `JumpStartUpVelocity`, turns into a fall at the apex,
//!   gravity is the scene's 981 cm/s^2 and the vertical speed is clamped at -1500 cm/s.
//!
//! Source tags are in [`crate::MovementProfile`]; the derivation is in
//! `docs/research/PHYSICS_MOVEMENT.md`.
use crate::profile::{MovementProfile, fall_damage};
use crate::{CollisionWorld, Tri, segment_triangle};
use glam::{Vec2, Vec3};

/// Gait chosen by the player or the AI (`gECharMovementMode` 2..5 in the original).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Gait {
    Sneak,
    Walk,
    #[default]
    Run,
    Sprint,
}

impl Gait {
    /// Speed modifier applied to `ForwardSpeedMax` (V: `SetSpeedByMode`).
    pub fn modifier(self, p: &MovementProfile) -> f32 {
        match self {
            Gait::Sneak => p.sneak_mod,
            Gait::Walk => p.slow_mod,
            Gait::Run => 1.0,
            Gait::Sprint => p.fast_mod,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MoveMode {
    #[default]
    Ground,
    /// Ground steeper than `SteepGroundAngleMax` (original mode 0xc).
    Slide,
    /// Rising after take-off (original mode 6).
    Jump,
    /// Falling (original mode 0xd).
    Fall,
}

/// Result of touching ground after being airborne.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Landing {
    /// Seconds spent in the fall state (the clock `fall_damage` uses).
    pub fall_time: f32,
    /// Downward speed at impact, cm/s.
    pub impact_speed: f32,
    /// `fall_damage(fall_time)`; scripts apply perks on top.
    pub damage: i32,
}

/// What happened during one [`CharacterController::step`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StepReport {
    pub landed: Option<Landing>,
    pub took_off: bool,
    pub started_sliding: bool,
}

#[derive(Debug, Clone, Copy)]
struct Ground {
    y: f32,
    normal: Vec3,
}

impl Ground {
    fn angle_deg(&self) -> f32 {
        self.normal.y.clamp(-1.0, 1.0).acos().to_degrees()
    }
}

#[derive(Debug, Clone)]
pub struct CharacterController {
    pub config: MovementProfile,
    /// Feet position.
    pub position: Vec3,
    pub vertical_speed: f32,
    pub grounded: bool,
    pub mode: MoveMode,
    /// Horizontal velocity of the last step, cm/s.
    pub velocity: Vec3,
    /// Local facing used for the strafe/backward speed limits; `None` = always forward.
    pub facing: Option<Vec2>,
    /// Seconds in the fall state so far (0 when not falling).
    pub fall_time: f32,
    /// Smoothed slope speed scale (1 on level ground).
    pub slope_scale: f32,
    /// Normal of the ground below (up when airborne).
    pub ground_normal: Vec3,
    pub last_landing: Option<Landing>,
    speed: f32,
    slide_speed: f32,
}

impl CharacterController {
    pub fn new(config: MovementProfile, position: Vec3) -> Self {
        Self {
            config,
            position,
            vertical_speed: 0.0,
            grounded: false,
            mode: MoveMode::Fall,
            velocity: Vec3::ZERO,
            facing: None,
            fall_time: 0.0,
            slope_scale: 1.0,
            ground_normal: Vec3::Y,
            last_landing: None,
            speed: 0.0,
            slide_speed: 0.0,
        }
    }

    /// Place the character (teleport) and let it fall to the ground.
    pub fn teleport(&mut self, position: Vec3) {
        self.position = position;
        self.vertical_speed = 0.0;
        self.grounded = false;
        self.mode = MoveMode::Fall;
        self.velocity = Vec3::ZERO;
        self.speed = 0.0;
        self.fall_time = 0.0;
    }

    /// Capsule segment endpoints for a lateral test: the lower sphere starts `clear` above the
    /// feet.
    fn segment(&self, feet: Vec3, clear: f32) -> (Vec3, Vec3) {
        let r = self.config.radius;
        let lo = feet + Vec3::Y * (clear + r);
        let hi = feet + Vec3::Y * (self.config.height - r).max(clear + r);
        (lo, hi)
    }

    /// Push the capsule out of geometry. `clear` raises the lower sphere (step offset). With
    /// `lateral` the push is horizontal only (walls); otherwise full. Returns
    /// (touched a wall, touched a ceiling).
    fn depenetrate(
        &self,
        world: &CollisionWorld,
        feet: &mut Vec3,
        clear: f32,
        lateral: bool,
    ) -> (bool, bool) {
        let r = self.config.radius;
        let mut tris: Vec<Tri> = Vec::new();
        let (mut wall, mut ceiling) = (false, false);
        for _ in 0..4 {
            let (a, b) = self.segment(*feet, clear);
            world.query(
                a.min(b) - Vec3::splat(r + 1.0),
                a.max(b) + Vec3::splat(r + 1.0),
                &mut tris,
            );
            let mut moved = false;
            for t in &tris {
                let (a, b) = self.segment(*feet, clear);
                let (sp, tp, crossing) = segment_triangle(a, b, t);
                let dist = sp.distance(tp);
                if !crossing && dist >= r {
                    continue;
                }
                let mut n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
                if n == Vec3::ZERO {
                    continue;
                }
                let mid = (a + b) * 0.5;
                if crossing && (mid - t[0]).dot(n) < 0.0 {
                    n = -n;
                }
                let (dir, depth) = if crossing {
                    (n, r)
                } else {
                    ((sp - tp) / dist.max(1.0e-6), r - dist)
                };
                if dir.y < -0.7 {
                    ceiling = true;
                    if lateral {
                        continue;
                    }
                }
                let push = if lateral {
                    let h = Vec3::new(dir.x, 0.0, dir.z);
                    let hl = h.length();
                    if hl < 0.25 {
                        continue; // floor-like contact: the ground sensor owns the height
                    }
                    wall = true;
                    h / hl * (depth / hl).min(depth * 3.0)
                } else {
                    if dir.y.abs() < 0.7 {
                        wall = true;
                    }
                    dir * depth
                };
                *feet += push;
                moved = true;
            }
            if !moved {
                break;
            }
        }
        (wall, ceiling)
    }

    /// Ground below `feet`: highest surface within the footprint that is at most `step_height`
    /// above the feet (surfaces above are ceilings or obstacles) and at most `depth` below them.
    fn sense(&self, world: &CollisionWorld, feet: Vec3, top: f32, depth: f32) -> Option<Ground> {
        let r = self.config.radius * 0.55;
        let offs = [
            Vec3::ZERO,
            Vec3::new(r, 0.0, 0.0),
            Vec3::new(-r, 0.0, 0.0),
            Vec3::new(0.0, 0.0, r),
            Vec3::new(0.0, 0.0, -r),
        ];
        let mut hits: [Option<(f32, Vec3)>; 5] = [None; 5];
        let mut best = f32::NEG_INFINITY;
        for (i, o) in offs.iter().enumerate() {
            let origin = Vec3::new(feet.x, feet.y + top, feet.z) + *o;
            if let Some((d, n)) = world.raycast_hit(origin, Vec3::NEG_Y, top + depth) {
                let y = origin.y - d;
                hits[i] = Some((y, n));
                best = best.max(y);
            }
        }
        if !best.is_finite() {
            return None;
        }
        // Average the normals of the contacts near the top: single terrain triangles are noisy.
        let mut n = Vec3::ZERO;
        for (y, normal) in hits.iter().flatten() {
            if best - y < 12.0 {
                n += *normal;
            }
        }
        let n = n.normalize_or_zero();
        Some(Ground {
            y: best,
            normal: if n == Vec3::ZERO { Vec3::Y } else { n },
        })
    }

    /// Slope speed scale for walking along `dir` (unit, horizontal) on `ground`.
    /// V: `CalcGroundSlopeTransScale` (Game.dll). Uphill: 1 below `SteepGroundAngleMin`, falling
    /// linearly to 0 at `SteepGroundAngleMax`; downhill: 1 rising to `1 + WalkDownSpeedScale`
    /// (`2 * WalkDownSpeedScale` past the maximum).
    pub fn slope_target_scale(&self, ground_angle_deg: f32, downhill: bool) -> f32 {
        let c = &self.config;
        let a = ground_angle_deg;
        if downhill {
            if a > c.steep_max_deg {
                2.0 * c.walk_down_scale
            } else if a > c.steep_min_deg {
                1.0 + (a - c.steep_min_deg) / (c.steep_max_deg - c.steep_min_deg).max(1.0e-3)
                    * c.walk_down_scale
            } else {
                1.0
            }
        } else if a > c.steep_max_deg {
            0.0
        } else if a > c.steep_min_deg {
            1.0 - (a - c.steep_min_deg) / (c.steep_max_deg - c.steep_min_deg).max(1.0e-3)
        } else {
            1.0
        }
        .max(c.min_slope_scale)
    }

    /// Advance with a desired horizontal velocity `wish` (cm/s: direction and top speed; the
    /// speed is clamped to the directional maximum of the profile).
    pub fn step(&mut self, world: &CollisionWorld, wish: Vec3, jump: bool, dt: f32) -> StepReport {
        let dt = dt.clamp(0.0, 0.1);
        let travel = wish.length() * dt + self.vertical_speed.abs().max(self.slide_speed) * dt;
        let steps = ((travel / (self.config.radius * 0.4)).ceil() as usize).clamp(1, 12);
        let sub = dt / steps as f32;
        let mut report = StepReport::default();
        let mut jump = jump;
        for _ in 0..steps {
            self.sub_step(world, wish, std::mem::take(&mut jump), sub, &mut report);
        }
        report
    }

    /// Advance with a unit direction and a gait.
    pub fn step_gait(
        &mut self,
        world: &CollisionWorld,
        dir: Vec3,
        gait: Gait,
        jump: bool,
        dt: f32,
    ) -> StepReport {
        let d = Vec3::new(dir.x, 0.0, dir.z).normalize_or_zero();
        let speed = if d == Vec3::ZERO {
            0.0
        } else {
            self.directional_max(d) * gait.modifier(&self.config)
        };
        self.step(world, d * speed, jump, dt)
    }

    /// `ForwardSpeedMax`-style limit for a world direction given the facing.
    fn directional_max(&self, dir: Vec3) -> f32 {
        let Some(f) = self.facing else {
            return self.config.forward_max;
        };
        let fwd = Vec2::new(f.x, f.y).normalize_or_zero();
        let right = Vec2::new(-fwd.y, fwd.x);
        let d = Vec2::new(dir.x, dir.z);
        self.config.max_speed_of_dir(d.dot(right), d.dot(fwd))
    }

    fn sub_step(
        &mut self,
        world: &CollisionWorld,
        wish: Vec3,
        jump: bool,
        dt: f32,
        report: &mut StepReport,
    ) {
        let c = self.config;
        let wish_h = Vec3::new(wish.x, 0.0, wish.z);
        let want = wish_h.length();
        let dir = if want > 1.0e-4 {
            wish_h / want
        } else {
            Vec3::ZERO
        };
        match self.mode {
            MoveMode::Ground => {
                if jump {
                    self.take_off(report);
                    return self.air_step(world, dir, want, dt, report);
                }
                self.ground_step(world, dir, want, dt, report);
            }
            MoveMode::Slide => self.slide_step(world, dt, report),
            MoveMode::Jump | MoveMode::Fall => self.air_step(world, dir, want, dt, report),
        }
        self.vertical_speed = if self.grounded {
            0.0
        } else {
            self.vertical_speed.max(-c.terminal_velocity)
        };
    }

    fn take_off(&mut self, report: &mut StepReport) {
        self.vertical_speed = self.config.jump_velocity;
        self.grounded = false;
        self.mode = MoveMode::Jump;
        self.fall_time = 0.0;
        report.took_off = true;
    }

    fn start_fall(&mut self, report: &mut StepReport) {
        self.grounded = false;
        self.mode = MoveMode::Fall;
        self.fall_time = 0.0;
        self.vertical_speed = 0.0;
        report.took_off = true;
    }

    fn ground_step(
        &mut self,
        world: &CollisionWorld,
        dir: Vec3,
        want: f32,
        dt: f32,
        report: &mut StepReport,
    ) {
        let c = self.config;
        let start = self.position;
        let cur = self.sense(world, start, c.step_height, c.step_height + 1.0);
        let ground = cur.unwrap_or(Ground {
            y: start.y,
            normal: Vec3::Y,
        });
        self.ground_normal = ground.normal;
        // Speed: accelerate up to the (slope scaled) maximum; no input stops at once (V: the
        // original adds no velocity without input).
        if want <= 1.0e-4 {
            self.speed = 0.0;
        } else {
            let downhill =
                Vec2::new(dir.x, dir.z).dot(Vec2::new(ground.normal.x, ground.normal.z)) >= 0.0;
            let target = self.slope_target_scale(ground.angle_deg(), downhill);
            if target <= 0.0 {
                self.slope_scale = 0.0;
            } else {
                let k = 1.0 - (-dt / c.slope_trans_inertia.max(1.0e-3)).exp();
                self.slope_scale += (target - self.slope_scale) * k;
            }
            let max = want.min(self.directional_max(dir) * c.fast_mod) * self.slope_scale;
            self.speed = (self.speed + c.accel * dt).min(max);
        }
        self.velocity = dir * self.speed;
        let mut moved = false;
        if self.speed > 1.0e-3 {
            moved = self.try_move(world, dir * self.speed * dt, start);
        }
        // Keep to the ground (PutToGround).
        let p = self.position;
        match self.sense(world, p, c.step_height, c.step_height + 1.0) {
            None => self.start_fall(report),
            Some(g) => {
                let rate = c.ground_follow_speed * dt;
                let dy = g.y - p.y;
                self.position.y += dy.clamp(-rate, rate);
                self.ground_normal = g.normal;
                if g.angle_deg() > c.steep_max_deg + 0.5 && !moved {
                    self.begin_slide(g, report);
                } else if g.angle_deg() > c.steep_max_deg + 0.5 {
                    // Moved onto steep ground: undo (V: uphill scale is 0 past the maximum).
                    self.position = start;
                    self.speed = 0.0;
                    self.velocity = Vec3::ZERO;
                }
            }
        }
    }

    /// Horizontal move with wall push-out; slides along the wall when blocked. Returns
    /// whether the character moved.
    fn try_move(&mut self, world: &CollisionWorld, delta: Vec3, start: Vec3) -> bool {
        let clear = self.config.step_height;
        let mut p = start + delta;
        self.depenetrate(world, &mut p, clear, true);
        let ground_ok = |me: &Self, p: Vec3| -> bool {
            me.sense(world, p, me.config.step_height, me.config.step_height + 1.0)
                .is_none_or(|g| g.angle_deg() <= me.config.steep_max_deg + 0.5)
        };
        if ground_ok(self, p) {
            self.position = Vec3::new(p.x, self.position.y, p.z);
            return (p - start).length_squared() > 1.0e-6;
        }
        // Steep ground ahead: slide along it instead of walking up.
        let back = start + delta;
        if let Some(g) = self.sense(
            world,
            back,
            self.config.step_height,
            self.config.step_height + 1.0,
        ) {
            let nh = Vec3::new(g.normal.x, 0.0, g.normal.z);
            let l2 = nh.length_squared();
            if l2 > 1.0e-6 {
                let tangent = delta - nh * (delta.dot(nh) / l2);
                let mut q = start + tangent;
                self.depenetrate(world, &mut q, clear, true);
                if ground_ok(self, q) {
                    self.position = Vec3::new(q.x, self.position.y, q.z);
                    return (q - start).length_squared() > 1.0e-6;
                }
            }
        }
        false
    }

    fn begin_slide(&mut self, g: Ground, report: &mut StepReport) {
        self.mode = MoveMode::Slide;
        self.ground_normal = g.normal;
        self.slide_speed = self.speed;
        report.started_sliding = true;
    }

    fn slide_step(&mut self, world: &CollisionWorld, dt: f32, report: &mut StepReport) {
        let c = self.config;
        let start = self.position;
        let Some(g) = self.sense(world, start, c.step_height, c.step_height + 1.0) else {
            self.start_fall(report);
            return;
        };
        self.ground_normal = g.normal;
        if g.angle_deg() <= c.steep_max_deg - 1.0 {
            self.mode = MoveMode::Ground;
            self.speed = 0.0;
            return;
        }
        let down = Vec3::new(g.normal.x, 0.0, g.normal.z).normalize_or_zero();
        let k = 1.0 - (-dt / c.slide_inertia.max(1.0e-3)).exp();
        self.slide_speed += (c.slide_speed_max - self.slide_speed) * k;
        self.velocity = down * self.slide_speed;
        let mut p = start + self.velocity * dt;
        self.depenetrate(world, &mut p, c.step_height, true);
        self.position = Vec3::new(p.x, self.position.y, p.z);
        match self.sense(world, self.position, c.step_height, c.step_height + 1.0) {
            None => self.start_fall(report),
            Some(g2) => {
                let rate = c.slide_speed_max.max(c.ground_follow_speed) * dt;
                self.position.y += (g2.y - self.position.y).clamp(-rate, rate);
                self.ground_normal = g2.normal;
            }
        }
    }

    fn air_step(
        &mut self,
        world: &CollisionWorld,
        dir: Vec3,
        want: f32,
        dt: f32,
        report: &mut StepReport,
    ) {
        let c = self.config;
        // Steering: same acceleration, scaled by the steer factor (V: SetSpeedByMode maps the
        // jump / fall modes to JumpSteerScaleFactor / FallSteerScaleFactor).
        let steer = if self.mode == MoveMode::Jump {
            c.jump_steer
        } else {
            c.fall_steer
        };
        let target = dir * (want.min(self.directional_max(dir) * c.fast_mod) * steer);
        let diff = target - self.velocity;
        let max_d = c.accel * dt;
        self.velocity += if diff.length() > max_d {
            diff / diff.length() * max_d
        } else {
            diff
        };
        self.velocity.y = 0.0;
        self.speed = self.velocity.length();

        let before = self.position;
        self.vertical_speed = (self.vertical_speed - c.gravity * dt).max(-c.terminal_velocity);
        let mut p = before + self.velocity * dt + Vec3::Y * (self.vertical_speed * dt);
        let (_, ceiling) = self.depenetrate(world, &mut p, 12.0, false);
        if ceiling && self.vertical_speed > 0.0 {
            self.vertical_speed = 0.0;
        }
        self.position = p;
        if self.mode == MoveMode::Jump && self.vertical_speed < 0.5 {
            // V: the jump state ends at the apex (vertical speed below 0.5 cm/s).
            self.mode = MoveMode::Fall;
            self.fall_time = 0.0;
        }
        if self.mode == MoveMode::Fall {
            self.fall_time += dt;
        }
        if self.vertical_speed <= 0.0 {
            let top = (before.y - p.y).max(0.0) + 6.0;
            let probe = Vec3::new(p.x, p.y, p.z);
            if let Some(g) = self.sense(world, probe, top, 1.0)
                && g.y >= p.y - 0.5
            {
                self.land(g, report);
            }
        }
    }

    fn land(&mut self, g: Ground, report: &mut StepReport) {
        let landing = Landing {
            fall_time: self.fall_time,
            impact_speed: -self.vertical_speed,
            damage: fall_damage(self.fall_time),
        };
        self.position.y = g.y;
        self.vertical_speed = 0.0;
        self.grounded = true;
        self.ground_normal = g.normal;
        self.fall_time = 0.0;
        self.last_landing = Some(landing);
        report.landed = Some(landing);
        if g.angle_deg() > self.config.steep_max_deg + 0.5 {
            self.begin_slide(g, report);
        } else {
            self.mode = MoveMode::Ground;
            self.speed = self.velocity.length();
        }
    }

    /// Move by an external displacement (character pushing) respecting walls; stays on the
    /// ground when it was on the ground.
    pub fn nudge(&mut self, world: &CollisionWorld, delta: Vec3) {
        let start = self.position;
        let mut p = start + Vec3::new(delta.x, 0.0, delta.z);
        self.depenetrate(world, &mut p, self.config.step_height, true);
        self.position = Vec3::new(p.x, start.y, p.z);
        if self.grounded
            && let Some(g) = self.sense(
                world,
                self.position,
                self.config.step_height,
                self.config.step_height + 1.0,
            )
        {
            self.position.y = g.y;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn quad(y: f32, half: f32) -> Vec<Tri> {
        let a = Vec3::new(-half, y, -half);
        let b = Vec3::new(half, y, -half);
        let c = Vec3::new(half, y, half);
        let d = Vec3::new(-half, y, half);
        vec![[a, c, b], [a, d, c]]
    }

    fn wall(x: f32, height: f32) -> Vec<Tri> {
        let a = Vec3::new(x, -100.0, -500.0);
        let b = Vec3::new(x, height, -500.0);
        let c = Vec3::new(x, height, 500.0);
        let d = Vec3::new(x, -100.0, 500.0);
        vec![[a, b, c], [a, c, d]]
    }

    /// Box with top at `top` between x0..x1 spanning z -500..500 (a ledge / step).
    fn ledge(x0: f32, x1: f32, top: f32) -> Vec<Tri> {
        let (zn, zp) = (-500.0, 500.0);
        let a = Vec3::new(x0, top, zn);
        let b = Vec3::new(x1, top, zn);
        let c = Vec3::new(x1, top, zp);
        let d = Vec3::new(x0, top, zp);
        let lo = Vec3::new(x0, -100.0, zn);
        let lo2 = Vec3::new(x0, -100.0, zp);
        vec![[a, c, b], [a, d, c], [lo, a, d], [lo, d, lo2]]
    }

    fn ramp(deg: f32, len: f32) -> Vec<Tri> {
        let s = deg.to_radians().tan();
        let p = |x: f32, z: f32| Vec3::new(x, x.max(0.0) * s, z);
        vec![
            [p(-2000.0, -800.0), p(0.0, 800.0), p(0.0, -800.0)],
            [p(-2000.0, -800.0), p(-2000.0, 800.0), p(0.0, 800.0)],
            [p(0.0, -800.0), p(len, 800.0), p(len, -800.0)],
            [p(0.0, -800.0), p(0.0, 800.0), p(len, 800.0)],
        ]
    }

    fn run(c: &mut CharacterController, w: &CollisionWorld, wish: Vec3, secs: f32) {
        for _ in 0..(secs / DT) as usize {
            c.step(w, wish, false, DT);
        }
    }

    fn controller(at: Vec3) -> CharacterController {
        CharacterController::new(MovementProfile::hero(), at)
    }

    fn flat() -> CollisionWorld {
        let mut w = CollisionWorld::new();
        w.insert_chunk(1, quad(0.0, 6000.0));
        w
    }

    #[test]
    fn falls_and_rests_on_floor() {
        let w = flat();
        let mut c = controller(Vec3::new(0.0, 300.0, 0.0));
        run(&mut c, &w, Vec3::ZERO, 3.0);
        assert!(c.grounded && c.mode == MoveMode::Ground);
        assert!(c.position.y.abs() < 1.0, "{}", c.position.y);
    }

    #[test]
    fn run_speed_matches_forward_speed_max() {
        let w = flat();
        let mut c = controller(Vec3::new(0.0, 0.0, 0.0));
        run(&mut c, &w, Vec3::ZERO, 0.5);
        let x0 = c.position.x;
        run(&mut c, &w, Vec3::X * 400.0, 2.0);
        // 0.4 s of acceleration at 1000 cm/s^2, then 400 cm/s.
        let travelled = c.position.x - x0;
        let expected = 0.5 * 400.0 * 0.4 + 400.0 * 1.6;
        assert!(
            (travelled - expected).abs() < 25.0,
            "{travelled} vs {expected}"
        );
        assert!((c.velocity.length() - 400.0).abs() < 1.0);
    }

    #[test]
    fn sprint_cannot_exceed_fast_modifier() {
        let w = flat();
        let mut c = controller(Vec3::ZERO);
        run(&mut c, &w, Vec3::ZERO, 0.5);
        run(&mut c, &w, Vec3::X * 5000.0, 2.0);
        assert!(
            (c.velocity.length() - 600.0).abs() < 1.0,
            "{}",
            c.velocity.length()
        );
    }

    #[test]
    fn wall_blocks_horizontal_motion() {
        let mut w = flat();
        w.insert_chunk(2, wall(300.0, 500.0));
        let mut c = controller(Vec3::ZERO);
        run(&mut c, &w, Vec3::X * 400.0, 3.0);
        assert!(
            c.position.x <= 300.0 - c.config.radius + 2.0,
            "x={}",
            c.position.x
        );
        assert!(c.position.x > 200.0);
    }

    #[test]
    fn steps_up_ledge_below_step_height_but_not_above() {
        let mut w = flat();
        w.insert_chunk(2, ledge(200.0, 2000.0, 60.0));
        let mut c = controller(Vec3::ZERO);
        run(&mut c, &w, Vec3::X * 400.0, 2.0);
        assert!(c.position.x > 500.0, "x={}", c.position.x);
        assert!((c.position.y - 60.0).abs() < 1.5, "y={}", c.position.y);

        let mut w = flat();
        w.insert_chunk(2, ledge(200.0, 2000.0, 80.0));
        let mut c = controller(Vec3::ZERO);
        run(&mut c, &w, Vec3::X * 400.0, 2.0);
        assert!(
            c.position.x < 200.0,
            "blocked by an 80 cm ledge, x={}",
            c.position.x
        );
    }

    #[test]
    fn walks_off_small_drop_but_falls_from_big_one() {
        // Ground at 0, ledge top at 60 over x < 0: stepping down keeps contact.
        let mut w = CollisionWorld::new();
        w.insert_chunk(1, quad(0.0, 6000.0));
        w.insert_chunk(2, ledge(-2000.0, 100.0, 60.0));
        let mut c = controller(Vec3::new(-500.0, 60.0, 0.0));
        run(&mut c, &w, Vec3::ZERO, 0.3);
        run(&mut c, &w, Vec3::X * 400.0, 1.0);
        assert!(c.grounded && c.mode == MoveMode::Ground, "{:?}", c.mode);
        // A 150 cm drop starts a fall.
        let mut w = CollisionWorld::new();
        w.insert_chunk(1, quad(0.0, 6000.0));
        w.insert_chunk(2, ledge(-2000.0, 100.0, 150.0));
        let mut c = controller(Vec3::new(-500.0, 150.0, 0.0));
        run(&mut c, &w, Vec3::ZERO, 0.3);
        let mut fell = false;
        for _ in 0..180 {
            c.step(&w, Vec3::X * 400.0, false, DT);
            fell |= c.mode == MoveMode::Fall;
        }
        assert!(fell);
        assert!(c.grounded && c.position.y.abs() < 1.0);
    }

    #[test]
    fn walkable_slope_is_slower_uphill_and_steep_slope_slides() {
        // 35 degrees: between SteepGroundAngleMin (20) and Max (49): uphill speed scale ~0.48.
        let mut w = CollisionWorld::new();
        w.insert_chunk(1, ramp(35.0, 3000.0));
        let mut c = controller(Vec3::new(-300.0, 0.0, 0.0));
        run(&mut c, &w, Vec3::ZERO, 0.5);
        run(&mut c, &w, Vec3::X * 400.0, 1.5);
        assert!(c.mode == MoveMode::Ground);
        assert!(c.position.y > 100.0, "climbed: y={}", c.position.y);
        let v = c.velocity.length();
        assert!(v > 120.0 && v < 280.0, "uphill speed {v}");

        // 55 degrees: not walkable; standing on it slides down at up to 800 cm/s.
        let mut w = CollisionWorld::new();
        w.insert_chunk(1, ramp(55.0, 3000.0));
        let mut c = controller(Vec3::new(300.0, 520.0, 0.0));
        run(&mut c, &w, Vec3::ZERO, 2.5);
        assert!(
            c.mode == MoveMode::Slide || c.position.x < 0.0,
            "{:?}",
            c.mode
        );
        assert!(c.position.x < 100.0, "slid down, x={}", c.position.x);
    }

    #[test]
    fn cannot_walk_up_slope_steeper_than_limit() {
        let mut w = CollisionWorld::new();
        w.insert_chunk(1, ramp(60.0, 3000.0));
        let mut c = controller(Vec3::new(-400.0, 0.0, 0.0));
        run(&mut c, &w, Vec3::ZERO, 0.5);
        run(&mut c, &w, Vec3::X * 400.0, 3.0);
        assert!(c.position.y < 70.0, "y={}", c.position.y);
    }

    #[test]
    fn jump_height_and_air_time_follow_gravity() {
        let w = flat();
        let mut c = controller(Vec3::ZERO);
        run(&mut c, &w, Vec3::ZERO, 0.5);
        c.step(&w, Vec3::ZERO, true, DT);
        let (mut peak, mut t, mut landing) = (0.0f32, DT, None);
        while t < 3.0 {
            let r = c.step(&w, Vec3::ZERO, false, DT);
            peak = peak.max(c.position.y);
            t += DT;
            if r.landed.is_some() {
                landing = r.landed;
                break;
            }
        }
        // v^2 / 2g = 600^2 / 1962 = 183.5 cm; airtime 2 v / g = 1.22 s.
        assert!((peak - 183.5).abs() < 6.0, "peak {peak}");
        assert!((t - 1.223).abs() < 0.08, "airtime {t}");
        let l = landing.expect("landed");
        assert_eq!(l.damage, 0);
    }

    #[test]
    fn long_fall_reports_damage_and_terminal_velocity() {
        let w = flat();
        let mut c = controller(Vec3::new(0.0, 6000.0, 0.0));
        let mut min_v = 0.0f32;
        let mut landing = None;
        for _ in 0..600 {
            let r = c.step(&w, Vec3::ZERO, false, DT);
            min_v = min_v.min(c.vertical_speed);
            if r.landed.is_some() {
                landing = r.landed;
                break;
            }
        }
        assert!((min_v + 1500.0).abs() < 1.0, "terminal {min_v}");
        let l = landing.expect("landed");
        assert!(l.fall_time > 3.0 && l.damage > 500, "{l:?}");
    }

    #[test]
    fn thin_wall_is_not_tunnelled_at_sprint() {
        let mut w = flat();
        w.insert_chunk(2, wall(400.0, 400.0));
        let mut c = controller(Vec3::ZERO);
        for _ in 0..200 {
            c.step(&w, Vec3::X * 600.0, false, 0.05);
        }
        assert!(c.position.x < 400.0);
    }

    #[test]
    fn ceiling_stops_a_jump() {
        let mut w = flat();
        w.insert_chunk(3, quad(220.0, 600.0));
        let mut c = controller(Vec3::ZERO);
        run(&mut c, &w, Vec3::ZERO, 0.5);
        c.step(&w, Vec3::ZERO, true, DT);
        let mut peak = 0.0f32;
        for _ in 0..90 {
            c.step(&w, Vec3::ZERO, false, DT);
            peak = peak.max(c.position.y);
        }
        assert!(
            peak + c.config.height <= 225.0,
            "head above ceiling: {peak}"
        );
    }
}
