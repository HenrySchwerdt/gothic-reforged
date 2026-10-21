//! Bevy-free PD controllers that make a physical hand (and whatever it holds) chase the
//! tracked controller pose, plus the strength model that limits them.
//!
//! The hand is a dynamic rigid body. Each physics step it receives a force/torque computed
//! from the pose error, *clamped by the player's strength*. A held object hangs on the hand
//! through a joint, so the controller sees the combined mass: light weapons track almost
//! perfectly, heavy ones lag behind fast motions and sag when the strength cannot carry them.
//!
//! The controller is the implicit ("stable") PD form, which stays stable for stiff gains at
//! VR physics rates: `a = (kp*(x_t - x - v*dt) + kd*(v_t - v)) / (1 + kd*dt + kp*dt^2)`.
use glam::{Mat3, Quat, Vec3};
use serde::{Deserialize, Serialize};

/// Gains expressed as a natural frequency and damping ratio (independent of mass).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PdGains {
    pub frequency_hz: f32,
    pub damping_ratio: f32,
}

impl PdGains {
    pub const HAND_LINEAR: PdGains = PdGains {
        frequency_hz: 12.0,
        damping_ratio: 1.0,
    };
    pub const HAND_ANGULAR: PdGains = PdGains {
        frequency_hz: 10.0,
        damping_ratio: 1.0,
    };

    pub fn kp(&self) -> f32 {
        let w = std::f32::consts::TAU * self.frequency_hz.max(0.0);
        w * w
    }

    pub fn kd(&self) -> f32 {
        2.0 * self.damping_ratio.max(0.0) * std::f32::consts::TAU * self.frequency_hz.max(0.0)
    }
}

/// Force/torque limits of one arm.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ArmLimits {
    /// Newtons.
    pub max_force: f32,
    /// Newton-metres.
    pub max_torque: f32,
}

/// Arm limits from the character's strength attribute (Gothic `STR`, typically 10..200).
///
/// Calibrated so a starting character (STR 10) swings a 1.5 kg sword freely and holds about
/// 12 kg at arm's length against gravity, and a strong one (STR 100+) handles heavy two-handers
/// briskly. The exact mapping to the original's requirements is a local integration concern.
pub fn arm_limits(strength: f32) -> ArmLimits {
    let s = if strength.is_finite() {
        strength.clamp(0.0, 300.0)
    } else {
        0.0
    };
    ArmLimits {
        max_force: 120.0 + 2.5 * s,
        max_torque: 18.0 + 0.4 * s,
    }
}

/// Force (N) that drives a body of `mass` toward `target_pos`/`target_vel`, including
/// feed-forward against `gravity` for the body weight, clamped to `max_force`.
#[allow(clippy::too_many_arguments)]
pub fn linear_force(
    pos: Vec3,
    vel: Vec3,
    target_pos: Vec3,
    target_vel: Vec3,
    mass: f32,
    gravity: Vec3,
    gains: PdGains,
    max_force: f32,
    dt: f32,
) -> Vec3 {
    if !(pos.is_finite() && vel.is_finite() && target_pos.is_finite() && target_vel.is_finite())
        || mass <= 0.0
        || dt <= 0.0
    {
        return Vec3::ZERO;
    }
    let (kp, kd) = (gains.kp(), gains.kd());
    let accel = (kp * (target_pos - pos - vel * dt) + kd * (target_vel - vel))
        / (1.0 + kd * dt + kp * dt * dt);
    let f = (accel - gravity) * mass;
    f.clamp_length_max(max_force.max(0.0))
}

/// Torque (N*m) that turns a body with the world-frame inertia tensor `inertia` (about the
/// point it rotates around) toward `target`, clamped.
#[allow(clippy::too_many_arguments)]
pub fn angular_torque(
    rot: Quat,
    ang_vel: Vec3,
    target: Quat,
    target_ang_vel: Vec3,
    inertia: Mat3,
    gains: PdGains,
    max_torque: f32,
    dt: f32,
) -> Vec3 {
    if !(rot.is_finite() && ang_vel.is_finite() && target.is_finite() && inertia.is_finite())
        || dt <= 0.0
    {
        return Vec3::ZERO;
    }
    let err = rotation_error(rot, target);
    let (kp, kd) = (gains.kp(), gains.kd());
    let alpha = (kp * (err - ang_vel * dt) + kd * (target_ang_vel - ang_vel))
        / (1.0 + kd * dt + kp * dt * dt);
    (inertia * alpha).clamp_length_max(max_torque.max(0.0))
}

/// World-frame inertia tensor about `pivot` of a body with the local (centre of mass) tensor
/// `local`, orientation `rotation`, `mass` and centre of mass `com` (parallel axis theorem).
pub fn inertia_about(local: Mat3, rotation: Quat, mass: f32, com: Vec3, pivot: Vec3) -> Mat3 {
    let r = Mat3::from_quat(rotation);
    let d = com - pivot;
    let outer = Mat3::from_cols(d * d.x, d * d.y, d * d.z);
    r * local * r.transpose() + (Mat3::IDENTITY * d.length_squared() - outer) * mass
}

/// Shortest rotation from `from` to `to` as a scaled axis (radians).
pub fn rotation_error(from: Quat, to: Quat) -> Vec3 {
    let mut d = to * from.inverse();
    if d.w < 0.0 {
        d = -d;
    }
    let (axis, angle) = d.normalize().to_axis_angle();
    if angle.is_finite() && axis.is_finite() {
        axis * angle
    } else {
        Vec3::ZERO
    }
}

/// Angular velocity that turns `from` into `to` over `dt`.
pub fn angular_velocity_between(from: Quat, to: Quat, dt: f32) -> Vec3 {
    if dt <= 0.0 {
        return Vec3::ZERO;
    }
    rotation_error(from, to) / dt
}

/// A point mass driven by [`linear_force`] under gravity: the reference model the tests use
/// (and that documents the expected lag), integrated with semi-implicit Euler.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointHand {
    pub pos: Vec3,
    pub vel: Vec3,
    pub mass: f32,
}

impl PointHand {
    pub fn step(
        &mut self,
        target_pos: Vec3,
        target_vel: Vec3,
        gravity: Vec3,
        gains: PdGains,
        limits: ArmLimits,
        dt: f32,
    ) {
        let f = linear_force(
            self.pos,
            self.vel,
            target_pos,
            target_vel,
            self.mass,
            gravity,
            gains,
            limits.max_force,
            dt,
        );
        self.vel += (f / self.mass + gravity) * dt;
        self.pos += self.vel * dt;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 90.0;
    const G: Vec3 = Vec3::new(0.0, -9.81, 0.0);

    /// Track a fast horizontal swing (1.2 m amplitude, 1.5 Hz) and return the worst lag.
    fn swing_lag(mass: f32, strength: f32) -> f32 {
        let mut h = PointHand {
            pos: Vec3::new(0.0, 1.2, -0.4),
            vel: Vec3::ZERO,
            mass,
        };
        let limits = arm_limits(strength);
        let mut worst: f32 = 0.0;
        let w = std::f32::consts::TAU * 1.5;
        for i in 0..270 {
            let t = i as f32 * DT;
            let target = Vec3::new(0.6 * (w * t).sin(), 1.2, -0.4);
            let tv = Vec3::new(0.6 * w * (w * t).cos(), 0.0, 0.0);
            h.step(target, tv, G, PdGains::HAND_LINEAR, limits, DT);
            if i > 90 {
                worst = worst.max(h.pos.distance(target));
            }
        }
        worst
    }

    #[test]
    fn light_tracks_heavy_lags_strength_helps() {
        let dagger = swing_lag(0.4 + 0.5, 10.0);
        let sword = swing_lag(1.5 + 0.5, 10.0);
        let two_hander = swing_lag(4.5 + 0.5, 10.0);
        let strong_two_hander = swing_lag(4.5 + 0.5, 150.0);
        assert!(dagger < 0.03, "dagger lag {dagger}");
        assert!(sword < two_hander, "{sword} {two_hander}");
        assert!(two_hander > 0.15, "heavy weapon lags {two_hander}");
        assert!(strong_two_hander < two_hander * 0.6, "{strong_two_hander}");
    }

    #[test]
    fn settles_without_overshoot() {
        let mut h = PointHand {
            pos: Vec3::ZERO,
            vel: Vec3::ZERO,
            mass: 1.0,
        };
        let target = Vec3::new(0.3, 0.0, 0.0);
        let mut max_x: f32 = 0.0;
        for _ in 0..180 {
            h.step(
                target,
                Vec3::ZERO,
                Vec3::ZERO,
                PdGains::HAND_LINEAR,
                arm_limits(50.0),
                DT,
            );
            max_x = max_x.max(h.pos.x);
        }
        assert!((h.pos.x - 0.3).abs() < 1e-3);
        assert!(max_x < 0.3 + 0.005, "overshoot {max_x}");
    }

    #[test]
    fn too_heavy_sags() {
        // 40 kg is beyond what STR 10 can hold up (120 + 25 = 145 N < 392 N).
        let mut h = PointHand {
            pos: Vec3::new(0.0, 1.2, -0.5),
            vel: Vec3::ZERO,
            mass: 40.0,
        };
        for _ in 0..90 {
            h.step(
                Vec3::new(0.0, 1.2, -0.5),
                Vec3::ZERO,
                G,
                PdGains::HAND_LINEAR,
                arm_limits(10.0),
                DT,
            );
        }
        assert!(h.pos.y < 0.5, "sags {}", h.pos.y);
        // Same weight, strength 200 (620 N): held.
        let mut h = PointHand {
            pos: Vec3::new(0.0, 1.2, -0.5),
            vel: Vec3::ZERO,
            mass: 40.0,
        };
        for _ in 0..90 {
            h.step(
                Vec3::new(0.0, 1.2, -0.5),
                Vec3::ZERO,
                G,
                PdGains::HAND_LINEAR,
                arm_limits(200.0),
                DT,
            );
        }
        assert!((h.pos.y - 1.2).abs() < 0.02, "held {}", h.pos.y);
    }

    #[test]
    fn force_is_clamped_and_finite() {
        let f = linear_force(
            Vec3::ZERO,
            Vec3::ZERO,
            Vec3::splat(100.0),
            Vec3::ZERO,
            1.0,
            G,
            PdGains::HAND_LINEAR,
            50.0,
            DT,
        );
        assert!((f.length() - 50.0).abs() < 1e-3);
        let bad = linear_force(
            Vec3::NAN,
            Vec3::ZERO,
            Vec3::ZERO,
            Vec3::ZERO,
            1.0,
            G,
            PdGains::HAND_LINEAR,
            50.0,
            DT,
        );
        assert_eq!(bad, Vec3::ZERO);
    }

    #[test]
    fn parallel_axis() {
        // Point mass 2 kg at 0.5 m on X: inertia about the pivot is 0.5 about Y and Z.
        let i = inertia_about(Mat3::ZERO, Quat::IDENTITY, 2.0, Vec3::X * 0.5, Vec3::ZERO);
        assert!((i.y_axis.y - 0.5).abs() < 1e-6 && (i.z_axis.z - 0.5).abs() < 1e-6);
        assert!(i.x_axis.x.abs() < 1e-6, "no inertia about the lever axis");
        // A rotated rod keeps its long-axis inertia small.
        let rod = Mat3::from_diagonal(Vec3::new(0.1, 0.1, 0.001));
        let i = inertia_about(
            rod,
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            1.0,
            Vec3::ZERO,
            Vec3::ZERO,
        );
        assert!((i.x_axis.x - 0.001).abs() < 1e-6, "{i}");
    }

    #[test]
    fn angular_torque_turns_the_short_way() {
        let from = Quat::IDENTITY;
        let to = Quat::from_rotation_y(-0.5);
        let t = angular_torque(
            from,
            Vec3::ZERO,
            to,
            Vec3::ZERO,
            Mat3::IDENTITY * 0.01,
            PdGains::HAND_ANGULAR,
            100.0,
            DT,
        );
        assert!(t.y < 0.0);
        let e = rotation_error(Quat::from_rotation_y(3.0), Quat::from_rotation_y(-3.0));
        assert!((e.y - (std::f32::consts::TAU - 6.0)).abs() < 1e-3, "{e}");
        let w = angular_velocity_between(from, to, 0.5);
        assert!((w.y + 1.0).abs() < 1e-4);
    }
}
