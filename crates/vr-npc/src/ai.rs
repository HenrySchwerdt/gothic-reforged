//! Bevy-free melee combat AI for VR opponents.
//!
//! A humanoid with a physical one-handed weapon fights the player like this:
//!
//! * keeps a fighting distance (approaches, backs off, circles while waiting);
//! * attacks only while holding one of the [`AttackSlots`] (at most 1-2 attackers at a
//!   time, the others circle);
//! * every attack is **telegraphed**: a clear wind-up pose on the side the blow comes from
//!   ([`AttackDir`]) held long enough to read, then a fast strike along an arc with the edge
//!   leading, then a recovery;
//! * **blocks** an incoming player weapon by putting its own blade across the threat's path
//!   (right, left or high);
//! * **recoils** when its own attack was blocked (short) or parried (long).
//!
//! The output is an [`NpcIntent`]: where the body goes and faces, and the world pose the
//! weapon grip should take. The ragdoll plays it physically.
use glam::{Quat, Vec3};
use gothic_vr::Pose;
use serde::{Deserialize, Serialize};
use std::f32::consts::{FRAC_PI_2, PI};

/// Where a blow comes from (NPC's point of view).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttackDir {
    FromRight,
    FromLeft,
    Overhead,
    Thrust,
}

impl AttackDir {
    pub const ALL: [AttackDir; 4] = [
        AttackDir::FromRight,
        AttackDir::FromLeft,
        AttackDir::Overhead,
        AttackDir::Thrust,
    ];
}

/// Guard used to block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardSide {
    Right,
    Left,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Phase {
    /// Ready stance, managing distance.
    Ready,
    /// Wind-up before a strike (seconds elapsed).
    Telegraph(AttackDir, f32),
    Strike(AttackDir, f32),
    Recover(AttackDir, f32),
    Block(GuardSide),
    /// Own attack was blocked or parried (seconds left).
    Recoil(f32),
    /// Off balance / down: no actions.
    Disabled,
}

/// Timings and ranges.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AiSettings {
    pub telegraph_time: f32,
    pub strike_time: f32,
    pub recover_time: f32,
    pub min_range: f32,
    pub max_range: f32,
    pub approach_speed: f32,
    pub retreat_speed: f32,
    pub circle_speed: f32,
    /// Random pause between attacks (min, max) seconds.
    pub attack_interval: (f32, f32),
    /// A player weapon point closer than this and closing faster than `threat_speed` is
    /// blocked.
    pub threat_range: f32,
    pub threat_speed: f32,
    pub turn_rate: f32,
    /// Always use this attack (training mode, tests); `None` picks at random.
    pub attack_pattern: Option<AttackDir>,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            telegraph_time: 0.55,
            strike_time: 0.3,
            recover_time: 0.55,
            min_range: 1.15,
            max_range: 1.75,
            approach_speed: 1.4,
            retreat_speed: 1.0,
            circle_speed: 0.45,
            attack_interval: (0.8, 1.8),
            threat_range: 1.4,
            threat_speed: 2.0,
            turn_rate: 4.0,
            attack_pattern: None,
        }
    }
}

/// Global attack tokens: at most `max` NPCs attack at once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttackSlots {
    pub max: usize,
    holders: Vec<u64>,
}

impl Default for AttackSlots {
    fn default() -> Self {
        Self::new(2)
    }
}

impl AttackSlots {
    pub fn new(max: usize) -> Self {
        Self {
            max: max.max(1),
            holders: Vec::new(),
        }
    }

    pub fn request(&mut self, id: u64) -> bool {
        if self.holders.contains(&id) {
            return true;
        }
        if self.holders.len() < self.max {
            self.holders.push(id);
            true
        } else {
            false
        }
    }

    pub fn release(&mut self, id: u64) {
        self.holders.retain(|h| *h != id);
    }

    pub fn holders(&self) -> &[u64] {
        &self.holders
    }
}

/// A player weapon point that may hit the NPC.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Threat {
    pub point: Vec3,
    pub velocity: Vec3,
}

/// What the AI sees this tick.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Perception {
    /// Player feet position.
    pub player: Vec3,
    pub threat: Option<Threat>,
    /// The body is balanced and can act.
    pub can_act: bool,
}

/// What the body should do.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NpcIntent {
    /// Feet centre.
    pub root: Vec3,
    /// Facing yaw (Bevy convention).
    pub yaw: f32,
    /// Weapon grip pose (world).
    pub weapon: Pose,
    pub phase: Phase,
}

/// An order from an outside decision maker (the original game AI via [`crate::exec`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Order {
    /// Strike this way as soon as range and an attack slot allow.
    Attack(AttackDir),
    /// Hold the guard up (the original `Parade`).
    Guard,
}

/// Per-NPC combat brain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CombatAi {
    pub settings: AiSettings,
    /// `true`: picks its own attacks (the VR-05 standalone opponent). `false`: attacks only on
    /// an [`Order`], so the original AI decides when and with what.
    pub autonomous: bool,
    /// The pending order (consumed when the attack starts; `Guard` holds until replaced).
    pub order: Option<Order>,
    pub root: Vec3,
    pub yaw: f32,
    pub phase: Phase,
    /// Seconds until the next attack may start.
    pub cooldown: f32,
    circle_sign: f32,
    rng: u64,
    /// Attacks started so far (for tests / telemetry).
    pub attacks: u32,
}

/// Weapon grip pose in the NPC's body frame (origin at the feet, facing -Z).
pub fn local_weapon_pose(phase: Phase, s: &AiSettings) -> Pose {
    let shoulder = Vec3::new(0.2, 1.4, 0.0);
    let horizontal = |phi: f32, roll: f32, y: f32| {
        let r = Quat::from_rotation_y(phi);
        Pose::new(
            shoulder + Vec3::new(0.0, y, 0.0) + r * Vec3::new(0.0, 0.0, -0.42),
            r * Quat::from_rotation_z(roll),
        )
    };
    let vertical = |theta: f32| {
        let r = Quat::from_rotation_x(theta);
        Pose::new(
            Vec3::new(0.12, 1.45, 0.0) + r * Vec3::new(0.0, 0.0, -0.42),
            r * Quat::from_rotation_z(PI),
        )
    };
    let ease = |k: f32| {
        let k = k.clamp(0.0, 1.0);
        k * k * (3.0 - 2.0 * k)
    };
    // (start, end) of each strike, as functions of the strike parameter.
    let arc = |dir: AttackDir, k: f32| -> Pose {
        match dir {
            AttackDir::FromRight => horizontal(
                (-100f32).to_radians() + 170f32.to_radians() * k,
                FRAC_PI_2,
                -0.1,
            ),
            AttackDir::FromLeft => horizontal(
                100f32.to_radians() - 170f32.to_radians() * k,
                -FRAC_PI_2,
                -0.1,
            ),
            AttackDir::Overhead => vertical(120f32.to_radians() - 140f32.to_radians() * k),
            AttackDir::Thrust => Pose::new(
                Vec3::new(0.22, 1.25, 0.05).lerp(Vec3::new(0.1, 1.3, -0.62), k),
                Quat::IDENTITY,
            ),
        }
    };
    let guard = Pose::new(Vec3::new(0.25, 1.1, -0.38), Quat::from_rotation_x(1.0));
    match phase {
        Phase::Ready | Phase::Disabled => guard,
        Phase::Telegraph(dir, t) => {
            let k = ease(t / (s.telegraph_time * 0.6));
            let w = arc(dir, 0.0);
            Pose::new(
                guard.position.lerp(w.position, k),
                guard.rotation.slerp(w.rotation, k),
            )
        }
        Phase::Strike(dir, t) => arc(dir, ease(t / s.strike_time)),
        Phase::Recover(dir, t) => {
            let k = ease(t / s.recover_time);
            let end = arc(dir, 1.0);
            Pose::new(
                end.position.lerp(guard.position, k),
                end.rotation.slerp(guard.rotation, k),
            )
        }
        Phase::Recoil(_) => Pose::new(Vec3::new(0.35, 1.3, 0.05), Quat::from_rotation_x(1.3)),
        Phase::Block(GuardSide::Right) => {
            Pose::new(Vec3::new(0.3, 1.0, -0.4), Quat::from_rotation_x(FRAC_PI_2))
        }
        Phase::Block(GuardSide::Left) => Pose::new(
            Vec3::new(-0.15, 1.0, -0.4),
            Quat::from_rotation_x(FRAC_PI_2),
        ),
        Phase::Block(GuardSide::High) => Pose::new(
            Vec3::new(0.3, 1.6, -0.35),
            Quat::from_rotation_y(FRAC_PI_2) * Quat::from_rotation_x(0.1),
        ),
    }
}

fn wrap(a: f32) -> f32 {
    (a + PI).rem_euclid(std::f32::consts::TAU) - PI
}

impl CombatAi {
    pub fn new(root: Vec3, yaw: f32, seed: u64) -> Self {
        Self {
            settings: AiSettings::default(),
            autonomous: true,
            order: None,
            root,
            yaw,
            phase: Phase::Ready,
            cooldown: 1.0,
            circle_sign: if seed.is_multiple_of(2) { 1.0 } else { -1.0 },
            rng: seed
                .wrapping_mul(2862933555777941757)
                .wrapping_add(3037000493),
            attacks: 0,
        }
    }

    fn rand(&mut self) -> f32 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Body frame (feet, yaw).
    pub fn frame(&self) -> Pose {
        Pose::new(self.root, Quat::from_rotation_y(self.yaw))
    }

    /// The NPC's own attack was blocked or parried.
    pub fn recoil(&mut self, seconds: f32) {
        if !matches!(self.phase, Phase::Disabled) {
            self.phase = Phase::Recoil(seconds.max(0.0));
        }
    }

    fn guard_side(&self, t: &Threat) -> Option<GuardSide> {
        let s = &self.settings;
        let chest = self.root + Vec3::Y * 1.2;
        let to = chest - t.point;
        if to.length() > s.threat_range {
            return None;
        }
        let closing = t.velocity.dot(to.normalize_or_zero());
        if closing < s.threat_speed {
            return None;
        }
        let local = self.frame().inverse().transform_point(t.point);
        Some(if local.y > 1.55 && t.velocity.y < -1.0 {
            GuardSide::High
        } else if local.x >= 0.0 {
            GuardSide::Right
        } else {
            GuardSide::Left
        })
    }

    /// One AI tick.
    pub fn update(
        &mut self,
        id: u64,
        per: &Perception,
        slots: &mut AttackSlots,
        dt: f32,
    ) -> NpcIntent {
        let s = self.settings;
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        if !per.can_act {
            if !matches!(self.phase, Phase::Disabled) {
                slots.release(id);
            }
            self.phase = Phase::Disabled;
        } else if self.phase == Phase::Disabled {
            self.phase = Phase::Ready;
            self.cooldown = s.attack_interval.0;
        }

        // Face the player.
        let to = per.player - self.root;
        let flat = Vec3::new(to.x, 0.0, to.z);
        let dist = flat.length();
        if dist > 1e-3 && self.phase != Phase::Disabled {
            let want = (-flat.x).atan2(-flat.z);
            let turn = wrap(want - self.yaw);
            let max = s.turn_rate * dt;
            self.yaw = wrap(self.yaw + turn.clamp(-max, max));
        }
        let dir = if dist > 1e-3 {
            flat / dist
        } else {
            Vec3::NEG_Z
        };

        self.cooldown -= dt;
        let threat = per.threat.and_then(|t| self.guard_side(&t));
        self.phase = match self.phase {
            Phase::Disabled => Phase::Disabled,
            Phase::Recoil(left) if left - dt > 0.0 => Phase::Recoil(left - dt),
            Phase::Recoil(_) => {
                slots.release(id);
                self.cooldown = s.attack_interval.0;
                Phase::Ready
            }
            Phase::Telegraph(d, t) if t + dt < s.telegraph_time => Phase::Telegraph(d, t + dt),
            Phase::Telegraph(d, _) => Phase::Strike(d, 0.0),
            Phase::Strike(d, t) if t + dt < s.strike_time => Phase::Strike(d, t + dt),
            Phase::Strike(d, _) => Phase::Recover(d, 0.0),
            Phase::Recover(d, t) if t + dt < s.recover_time => Phase::Recover(d, t + dt),
            Phase::Recover(_, _) => {
                slots.release(id);
                let (a, b) = s.attack_interval;
                self.cooldown = a + (b - a) * self.rand();
                Phase::Ready
            }
            Phase::Ready | Phase::Block(_) => {
                let in_range = dist <= s.max_range + 0.1 && dist >= s.min_range - 0.2;
                if let Some(side) = threat {
                    Phase::Block(side)
                } else if self.order == Some(Order::Guard) {
                    Phase::Block(GuardSide::Right)
                } else if !self.autonomous {
                    match self.order {
                        Some(Order::Attack(d)) if in_range && slots.request(id) => {
                            self.order = None;
                            self.attacks += 1;
                            Phase::Telegraph(d, 0.0)
                        }
                        _ => Phase::Ready,
                    }
                } else if self.cooldown <= 0.0 && in_range && slots.request(id) {
                    let k = ((self.rand() * 4.0) as usize).min(3);
                    self.attacks += 1;
                    Phase::Telegraph(s.attack_pattern.unwrap_or(AttackDir::ALL[k]), 0.0)
                } else {
                    Phase::Ready
                }
            }
        };

        // Footwork.
        let mut step = Vec3::ZERO;
        match self.phase {
            Phase::Ready | Phase::Block(_) => {
                if dist > s.max_range {
                    step = dir * s.approach_speed;
                } else if dist < s.min_range {
                    step = -dir * s.retreat_speed;
                } else if self.phase == Phase::Ready {
                    step = Vec3::new(-dir.z, 0.0, dir.x) * s.circle_speed * self.circle_sign;
                }
            }
            Phase::Strike(AttackDir::Thrust, _) => step = dir * 0.8,
            Phase::Recoil(_) => step = -dir * s.retreat_speed * 0.6,
            _ => {}
        }
        if step.is_finite() {
            self.root += step * dt;
        }
        let weapon = self.frame().mul(&local_weapon_pose(self.phase, &s));
        NpcIntent {
            root: self.root,
            yaw: self.yaw,
            weapon,
            phase: self.phase,
        }
    }
}
