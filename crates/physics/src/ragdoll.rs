//! Ragdoll for dead / knocked-out characters.
//!
//! The original builds PhysX ragdolls through EMotionFX (`eCNovodexPhysicsController`,
//! `eCVisualAnimation_PS::SetRagDollEnabled`), with spherical joints (`eCSphericalJointDesc`,
//! swing / twist limits) on bodies in collision group `RagDoll` (`eECollisionGroup` 17). The
//! per-bone joint limits live in the actor's physics setup, which is not decoded yet, so this
//! is a position-based (verlet) stand-in: a stick skeleton of point masses with distance
//! constraints, minimum-distance "joint limit" constraints and sphere collision against the
//! world. Gravity, the sleep threshold and the damping come from the original scene / rigid
//! body values (`PHYSICS_MOVEMENT.md`); body proportions are a humanoid average (T).
use crate::{CollisionWorld, Tri, closest_point_on_triangle};
use glam::Vec3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Joint {
    Head,
    Neck,
    Chest,
    Pelvis,
    ShoulderL,
    ShoulderR,
    ElbowL,
    ElbowR,
    HandL,
    HandR,
    HipL,
    HipR,
    KneeL,
    KneeR,
    FootL,
    FootR,
}

const COUNT: usize = 16;

// Segments (a, b, length as a fraction of body height) of a humanoid.
const BONES: [(Joint, Joint, f32); 17] = [
    (Joint::Head, Joint::Neck, 0.09),
    (Joint::Neck, Joint::Chest, 0.12),
    (Joint::Chest, Joint::Pelvis, 0.20),
    (Joint::Neck, Joint::ShoulderL, 0.11),
    (Joint::Neck, Joint::ShoulderR, 0.11),
    (Joint::ShoulderL, Joint::ElbowL, 0.17),
    (Joint::ShoulderR, Joint::ElbowR, 0.17),
    (Joint::ElbowL, Joint::HandL, 0.15),
    (Joint::ElbowR, Joint::HandR, 0.15),
    (Joint::Pelvis, Joint::HipL, 0.05),
    (Joint::Pelvis, Joint::HipR, 0.05),
    (Joint::HipL, Joint::KneeL, 0.23),
    (Joint::HipR, Joint::KneeR, 0.23),
    (Joint::KneeL, Joint::FootL, 0.23),
    (Joint::KneeR, Joint::FootR, 0.23),
    (Joint::ShoulderL, Joint::ShoulderR, 0.22),
    (Joint::HipL, Joint::HipR, 0.10),
];

// Soft "joint limit" pairs: minimum distances that stop limbs folding through the body.
const LIMITS: [(Joint, Joint, f32); 6] = [
    (Joint::Head, Joint::Chest, 0.15),
    (Joint::Chest, Joint::HipL, 0.14),
    (Joint::Chest, Joint::HipR, 0.14),
    (Joint::HipL, Joint::FootL, 0.20),
    (Joint::HipR, Joint::FootR, 0.20),
    (Joint::ShoulderL, Joint::HandL, 0.16),
];

#[derive(Debug, Clone)]
pub struct Ragdoll {
    pos: [Vec3; COUNT],
    prev: [Vec3; COUNT],
    rest: Vec<(usize, usize, f32)>,
    mins: Vec<(usize, usize, f32)>,
    /// Collision radius of every point (limb thickness), cm.
    pub radius: f32,
    /// Gravity, cm/s^2. V: scene `GravityVector`.
    pub gravity: f32,
    /// Velocity damping per second. V: `eCRigidBody_PS.AngularDamping` 0.1 of the hero body
    /// (linear damping 0).
    pub damping: f32,
    /// Ground friction per contact (fraction of tangential velocity removed). T.
    pub friction: f32,
    asleep: bool,
    still_time: f32,
}

impl Ragdoll {
    /// Standing humanoid of `height` cm with its feet at `feet`, facing `facing` (horizontal),
    /// moving with `velocity` (inherits the character's speed on death).
    pub fn humanoid(feet: Vec3, facing: Vec3, height: f32, velocity: Vec3) -> Self {
        let f = Vec3::new(facing.x, 0.0, facing.z).normalize_or_zero();
        let f = if f == Vec3::ZERO { Vec3::X } else { f };
        let side = Vec3::Y.cross(f);
        let h = height;
        let at = |y: f32, s: f32| feet + Vec3::Y * (y * h) + side * (s * h);
        let mut pos = [Vec3::ZERO; COUNT];
        pos[Joint::Head as usize] = at(0.93, 0.0);
        pos[Joint::Neck as usize] = at(0.84, 0.0);
        pos[Joint::Chest as usize] = at(0.72, 0.0);
        pos[Joint::Pelvis as usize] = at(0.52, 0.0);
        pos[Joint::ShoulderL as usize] = at(0.82, 0.11);
        pos[Joint::ShoulderR as usize] = at(0.82, -0.11);
        pos[Joint::ElbowL as usize] = at(0.65, 0.12);
        pos[Joint::ElbowR as usize] = at(0.65, -0.12);
        pos[Joint::HandL as usize] = at(0.50, 0.12);
        pos[Joint::HandR as usize] = at(0.50, -0.12);
        pos[Joint::HipL as usize] = at(0.50, 0.05);
        pos[Joint::HipR as usize] = at(0.50, -0.05);
        pos[Joint::KneeL as usize] = at(0.27, 0.05);
        pos[Joint::KneeR as usize] = at(0.27, -0.05);
        pos[Joint::FootL as usize] = at(0.04, 0.05);
        pos[Joint::FootR as usize] = at(0.04, -0.05);
        let rest = BONES
            .iter()
            .map(|&(a, b, fr)| (a as usize, b as usize, fr * h))
            .collect();
        let mins = LIMITS
            .iter()
            .map(|&(a, b, fr)| (a as usize, b as usize, fr * h))
            .collect();
        // The first step is taken at 1/60 s: seed `prev` so the body starts with `velocity`.
        let mut prev = pos;
        for p in &mut prev {
            *p -= velocity / 60.0;
        }
        Self {
            pos,
            prev,
            rest,
            mins,
            radius: 7.0,
            gravity: 981.0,
            damping: 0.1,
            friction: 0.35,
            asleep: false,
            still_time: 0.0,
        }
    }

    pub fn joint(&self, j: Joint) -> Vec3 {
        self.pos[j as usize]
    }

    pub fn joints(&self) -> &[Vec3; COUNT] {
        &self.pos
    }

    pub fn is_asleep(&self) -> bool {
        self.asleep
    }

    /// Lowest and highest point (to see how flat the body lies).
    pub fn vertical_extent(&self) -> (f32, f32) {
        self.pos.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
            (lo.min(p.y), hi.max(p.y))
        })
    }

    /// Apply an impulse (cm/s change of velocity) to one joint, e.g. a hit.
    pub fn impulse(&mut self, joint: Joint, dv: Vec3, dt: f32) {
        self.prev[joint as usize] -= dv * dt;
        self.asleep = false;
    }

    /// Advance by `dt` (call with a fixed step <= 1/30 s, the scene's simulation rate).
    pub fn step(&mut self, world: &CollisionWorld, dt: f32) {
        if self.asleep || dt <= 0.0 {
            return;
        }
        let damp = (1.0 - self.damping * dt).clamp(0.0, 1.0);
        let mut moved2 = 0.0f32;
        for i in 0..COUNT {
            let v = (self.pos[i] - self.prev[i]) * damp;
            self.prev[i] = self.pos[i];
            self.pos[i] += v + Vec3::NEG_Y * self.gravity * dt * dt;
        }
        let mut tris: Vec<Tri> = Vec::new();
        for _ in 0..6 {
            for &(a, b, len) in &self.rest {
                let d = self.pos[b] - self.pos[a];
                let l = d.length().max(1.0e-4);
                let corr = d * ((l - len) / l * 0.5);
                self.pos[a] += corr;
                self.pos[b] -= corr;
            }
            for &(a, b, min) in &self.mins {
                let d = self.pos[b] - self.pos[a];
                let l = d.length().max(1.0e-4);
                if l < min {
                    let corr = d * ((l - min) / l * 0.5);
                    self.pos[a] += corr;
                    self.pos[b] -= corr;
                }
            }
            for i in 0..COUNT {
                self.collide(world, i, &mut tris);
            }
        }
        for i in 0..COUNT {
            moved2 = moved2.max((self.pos[i] - self.prev[i]).length_squared() / (dt * dt));
        }
        // V: DefaultSleepLinVeloSquared 0.0225 (cm/s)^2 in the scene; the point cloud needs a
        // little more slack (5 cm/s) because the constraint solver jitters.
        if moved2 < 25.0 {
            self.still_time += dt;
            if self.still_time > 1.0 {
                self.asleep = true;
            }
        } else {
            self.still_time = 0.0;
        }
    }

    fn collide(&mut self, world: &CollisionWorld, i: usize, tris: &mut Vec<Tri>) {
        let r = self.radius;
        let p = self.pos[i];
        world.query(p - Vec3::splat(r + 1.0), p + Vec3::splat(r + 1.0), tris);
        for t in tris.iter() {
            let c = closest_point_on_triangle(self.pos[i], t);
            let d = self.pos[i] - c;
            let l = d.length();
            let tn = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
            if tn == Vec3::ZERO {
                continue;
            }
            // Face the triangle towards the side the point came from, so a point that got
            // past a one-sided floor is pushed back out and not deeper in.
            let side = if (self.prev[i] - t[0]).dot(tn) >= 0.0 {
                1.0
            } else {
                -1.0
            };
            let ng = tn * side;
            let signed = (self.pos[i] - t[0]).dot(ng);
            if l >= r && !(signed < 0.0 && l < 40.0) {
                continue;
            }
            let n = if signed >= 0.0 && l > 1.0e-4 {
                d / l
            } else {
                ng
            };
            self.pos[i] = c + n * r;
            // Friction: remove part of the tangential motion this step.
            let v = self.pos[i] - self.prev[i];
            let vt = v - n * v.dot(n);
            self.prev[i] += vt * self.friction;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor(y: f32) -> CollisionWorld {
        let h = 5000.0;
        let mut w = CollisionWorld::new();
        w.insert_chunk(
            1,
            vec![
                [
                    Vec3::new(-h, y, -h),
                    Vec3::new(h, y, h),
                    Vec3::new(h, y, -h),
                ],
                [
                    Vec3::new(-h, y, -h),
                    Vec3::new(-h, y, h),
                    Vec3::new(h, y, h),
                ],
            ],
        );
        w
    }

    #[test]
    fn dead_body_falls_lies_down_and_sleeps() {
        let w = floor(0.0);
        let mut r = Ragdoll::humanoid(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::X,
            185.0,
            Vec3::new(200.0, 0.0, 0.0),
        );
        r.impulse(Joint::Head, Vec3::new(150.0, 0.0, 0.0), 1.0 / 30.0);
        for _ in 0..600 {
            r.step(&w, 1.0 / 30.0);
        }
        let (lo, hi) = r.vertical_extent();
        assert!(lo > -1.0, "nothing below the floor: {lo}");
        assert!(hi < 40.0, "lies flat: top {hi}");
        assert!(r.is_asleep());
    }

    #[test]
    fn bones_keep_their_length() {
        let w = floor(0.0);
        let mut r = Ragdoll::humanoid(Vec3::new(0.0, 100.0, 0.0), Vec3::X, 185.0, Vec3::ZERO);
        for _ in 0..90 {
            r.step(&w, 1.0 / 30.0);
        }
        let l = (r.joint(Joint::KneeL) - r.joint(Joint::FootL)).length();
        assert!((l - 0.23 * 185.0).abs() < 4.0, "shin {l}");
    }
}
