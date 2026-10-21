//! Full-body IK avatar from three tracked points (head + hands) on a generic humanoid skeleton.
//!
//! Bevy-free and deterministic. The skeleton is generic on purpose: a later retarget maps
//! [`Joint`]s onto Gothic's bone names. Solver per frame ([`BodyIk::solve`]):
//!
//! * **neck** hangs below/behind the eyes in the head frame;
//! * **body yaw** follows the head only outside a dead band (looking around does not twist
//!   the hips), pulled toward where the hands are;
//! * **pelvis** sits a torso length below the neck; when the head is lower than that (crouch,
//!   bow) the pelvis drops to a minimum and the rest becomes forward spine bend;
//! * **spine/chest** are placed on a bent curve pelvis -> neck;
//! * **arms**: analytic two-bone IK shoulder -> hand with an elbow pole (down/out/back);
//! * **legs**: procedural stepping. Each foot stays planted until it is too far from its
//!   rest spot under the hip (or the body turned), then steps along an arc; one foot at a
//!   time; feet follow a ground-height callback; knees by two-bone IK with a forward pole.
//!
//! Units: metres, world space, Y up.
use crate::input::Pose;
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Joint {
    Pelvis,
    Spine,
    Chest,
    Neck,
    Head,
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
    AnkleL,
    AnkleR,
}

pub const JOINT_COUNT: usize = 17;

impl Joint {
    pub const ALL: [Joint; JOINT_COUNT] = [
        Joint::Pelvis,
        Joint::Spine,
        Joint::Chest,
        Joint::Neck,
        Joint::Head,
        Joint::ShoulderL,
        Joint::ShoulderR,
        Joint::ElbowL,
        Joint::ElbowR,
        Joint::HandL,
        Joint::HandR,
        Joint::HipL,
        Joint::HipR,
        Joint::KneeL,
        Joint::KneeR,
        Joint::AnkleL,
        Joint::AnkleR,
    ];

    pub fn index(self) -> usize {
        self as usize
    }
}

/// Bones as (parent joint, child joint) for drawing / retargeting.
pub const BONES: [(Joint, Joint); 16] = [
    (Joint::Pelvis, Joint::Spine),
    (Joint::Spine, Joint::Chest),
    (Joint::Chest, Joint::Neck),
    (Joint::Neck, Joint::Head),
    (Joint::Chest, Joint::ShoulderL),
    (Joint::Chest, Joint::ShoulderR),
    (Joint::ShoulderL, Joint::ElbowL),
    (Joint::ShoulderR, Joint::ElbowR),
    (Joint::ElbowL, Joint::HandL),
    (Joint::ElbowR, Joint::HandR),
    (Joint::Pelvis, Joint::HipL),
    (Joint::Pelvis, Joint::HipR),
    (Joint::HipL, Joint::KneeL),
    (Joint::HipR, Joint::KneeR),
    (Joint::KneeL, Joint::AnkleL),
    (Joint::KneeR, Joint::AnkleR),
];

/// Segment lengths of the generic humanoid (metres).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Proportions {
    /// Eyes -> neck base, in the head frame (down, back).
    pub eye_to_neck: Vec3,
    /// Neck -> eye-level head joint (for drawing).
    pub head: f32,
    /// Pelvis -> neck.
    pub torso: f32,
    pub shoulder_half_width: f32,
    /// Shoulders sit this far below the neck along the spine.
    pub shoulder_drop: f32,
    /// How far the shoulder may protract toward a far hand.
    pub clavicle_reach: f32,
    pub upper_arm: f32,
    pub forearm: f32,
    pub hip_half_width: f32,
    pub thigh: f32,
    pub shin: f32,
    /// Ankle height above the ground.
    pub ankle_height: f32,
}

impl Proportions {
    /// Average proportions scaled to an eye height.
    pub fn for_eye_height(eye: f32) -> Self {
        let k = eye / 1.7;
        Self {
            eye_to_neck: Vec3::new(0.0, -0.13, 0.08) * k,
            head: 0.15 * k,
            torso: 0.58 * k,
            shoulder_half_width: 0.19 * k,
            shoulder_drop: 0.10 * k,
            clavicle_reach: 0.07 * k,
            upper_arm: 0.31 * k,
            forearm: 0.29 * k,
            hip_half_width: 0.10 * k,
            thigh: 0.48 * k,
            shin: 0.46 * k,
            ankle_height: 0.08 * k,
        }
    }

    pub fn leg_length(&self) -> f32 {
        self.thigh + self.shin + self.ankle_height
    }

    pub fn arm_length(&self) -> f32 {
        self.upper_arm + self.forearm
    }
}

/// Tunables of the solver.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BodySettings {
    /// Head may turn this far (radians) before the body follows.
    pub yaw_dead_band: f32,
    /// Body yaw follow rate, 1/s.
    pub yaw_follow: f32,
    /// A planted foot further than this from its rest spot starts a step.
    pub step_distance: f32,
    pub step_duration: f32,
    pub step_height: f32,
    /// Body turned this far (radians) since the foot planted also triggers a step.
    pub step_turn: f32,
}

impl Default for BodySettings {
    fn default() -> Self {
        Self {
            yaw_dead_band: 0.6,
            yaw_follow: 6.0,
            step_distance: 0.22,
            step_duration: 0.22,
            step_height: 0.08,
            step_turn: 0.7,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct Foot {
    planted: Vec3,
    planted_yaw: f32,
    /// Step in flight: (from, to, elapsed).
    step: Option<(Vec3, Vec3, f32)>,
}

/// Solved joint positions plus the body frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BodyPose {
    pub joints: [Vec3; JOINT_COUNT],
    /// Yaw of the hips (Bevy convention).
    pub body_yaw: f32,
    /// Hands reached their targets (false when clamped by arm length).
    pub reached: [bool; 2],
}

impl BodyPose {
    pub fn get(&self, j: Joint) -> Vec3 {
        self.joints[j.index()]
    }

    /// World rotation of a bone: its +Y along parent -> child, twisted toward the body's
    /// forward (a stable frame for retargeting and for capsule hitzones).
    pub fn bone_rotation(&self, bone: (Joint, Joint)) -> Quat {
        let dir = (self.get(bone.1) - self.get(bone.0)).normalize_or_zero();
        if dir == Vec3::ZERO {
            return Quat::from_rotation_y(self.body_yaw);
        }
        let fwd = Quat::from_rotation_y(self.body_yaw) * Vec3::NEG_Z;
        let side = dir.cross(fwd);
        let side = if side.length_squared() < 1e-6 {
            dir.cross(Vec3::X).normalize()
        } else {
            side.normalize()
        };
        let z = side.cross(dir).normalize();
        Quat::from_mat3(&glam::Mat3::from_cols(side, dir, z)).normalize()
    }

    pub fn is_finite(&self) -> bool {
        self.joints.iter().all(|j| j.is_finite()) && self.body_yaw.is_finite()
    }
}

/// Stateful solver (body yaw and feet persist between frames).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BodyIk {
    pub proportions: Proportions,
    pub settings: BodySettings,
    body_yaw: Option<f32>,
    feet: Option<[Foot; 2]>,
}

/// Two-bone IK: elbow/knee position for root `a`, target `t`, lengths `l1`, `l2` and a pole
/// direction. Returns (joint, effector, reached).
pub fn two_bone(a: Vec3, t: Vec3, l1: f32, l2: f32, pole: Vec3) -> (Vec3, Vec3, bool) {
    let to = t - a;
    let dist = to.length();
    let max = (l1 + l2) * 0.9999;
    let min = (l1 - l2).abs() + 1e-4;
    let dir = if dist > 1e-6 { to / dist } else { Vec3::NEG_Y };
    let d = dist.clamp(min, max);
    let reached = (dist - d).abs() < 1e-4;
    let end = a + dir * d;
    // Law of cosines for the angle at `a`.
    let cos_a = ((l1 * l1 + d * d - l2 * l2) / (2.0 * l1 * d)).clamp(-1.0, 1.0);
    let along = l1 * cos_a;
    let off = l1 * (1.0 - cos_a * cos_a).max(0.0).sqrt();
    let mut bend = pole - dir * pole.dot(dir);
    if bend.length_squared() < 1e-8 {
        bend = dir.any_orthonormal_vector();
    }
    let joint = a + dir * along + bend.normalize() * off;
    (joint, end, reached)
}

fn wrap(a: f32) -> f32 {
    let t = std::f32::consts::TAU;
    let mut a = a % t;
    if a > std::f32::consts::PI {
        a -= t;
    } else if a < -std::f32::consts::PI {
        a += t;
    }
    a
}

impl BodyIk {
    pub fn new(proportions: Proportions) -> Self {
        Self {
            proportions,
            settings: BodySettings::default(),
            body_yaw: None,
            feet: None,
        }
    }

    pub fn body_yaw(&self) -> Option<f32> {
        self.body_yaw
    }

    /// Feet currently mid-step.
    pub fn stepping(&self) -> [bool; 2] {
        self.feet
            .map_or([false; 2], |f| [f[0].step.is_some(), f[1].step.is_some()])
    }

    /// Solve one frame. `hands` are `[left, right]` grip poses (`None` = untracked: the arm
    /// hangs). `ground(x, z)` returns the floor height there.
    pub fn solve(
        &mut self,
        head: &Pose,
        hands: [Option<Pose>; 2],
        ground: impl Fn(f32, f32) -> f32,
        dt: f32,
    ) -> BodyPose {
        let p = self.proportions;
        let s = self.settings;
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        let head_pos = head.position;
        let neck = head.transform_point(p.eye_to_neck);

        // Body yaw with dead band, biased toward the hands.
        let head_yaw = head.yaw();
        let mut want = head_yaw;
        let tracked: Vec<Vec3> = hands.iter().flatten().map(|h| h.position).collect();
        if !tracked.is_empty() {
            let mid = tracked.iter().copied().sum::<Vec3>() / tracked.len() as f32;
            let to = Vec3::new(mid.x - neck.x, 0.0, mid.z - neck.z);
            if to.length() > 0.15 {
                let hy = (-to.x).atan2(-to.z);
                want = head_yaw + wrap(hy - head_yaw) * 0.3;
            }
        }
        let yaw = match self.body_yaw {
            None => want,
            Some(y) => {
                let err = wrap(want - y);
                let excess = err.abs() - s.yaw_dead_band;
                let mut y = y;
                if excess > 0.0 {
                    y += err.signum() * excess; // never further than the dead band
                }
                // Then drift toward the wished yaw while moving the head.
                let drift = wrap(want - y) * (s.yaw_follow * dt * 0.25).min(1.0);
                wrap(y + drift)
            }
        };
        self.body_yaw = Some(yaw);
        let rot = Quat::from_rotation_y(yaw);
        let fwd = rot * Vec3::NEG_Z;
        let right = rot * Vec3::X;

        // Pelvis.
        let floor = ground(neck.x, neck.z);
        let leg = p.leg_length();
        let pelvis_y = (neck.y - p.torso).clamp(floor + 0.45 * leg, floor + 0.98 * leg);
        let dy = neck.y - pelvis_y;
        let (pelvis, bend) = if dy < p.torso {
            let h = (p.torso * p.torso - dy * dy).max(0.0).sqrt();
            (
                Vec3::new(neck.x, pelvis_y, neck.z) - fwd * h,
                (h / p.torso).clamp(0.0, 1.0),
            )
        } else {
            (neck - Vec3::Y * p.torso, 0.0)
        };
        let spine_dir = (neck - pelvis).normalize_or_zero();
        // Bent spine: the mid points bulge backward a little when bowing.
        let bulge = -fwd * 0.04 * bend;
        let spine = pelvis + (neck - pelvis) * 0.35 + bulge;
        let chest = pelvis + (neck - pelvis) * 0.7 + bulge * 0.5;
        let head_joint = neck + (head_pos - neck).normalize_or_zero() * p.head;

        // Arms.
        let shoulder_base = neck - spine_dir * p.shoulder_drop;
        let mut joints = [Vec3::ZERO; JOINT_COUNT];
        let mut reached = [true; 2];
        for (i, (sj, ej, hj)) in [
            (Joint::ShoulderL, Joint::ElbowL, Joint::HandL),
            (Joint::ShoulderR, Joint::ElbowR, Joint::HandR),
        ]
        .into_iter()
        .enumerate()
        {
            let sign = if i == 0 { -1.0 } else { 1.0 };
            let mut shoulder = shoulder_base + right * (sign * p.shoulder_half_width);
            // Clavicle: the shoulder protracts a few centimetres toward a far target.
            if let Some(h) = hands[i] {
                let to = h.position - shoulder;
                let excess = to.length() - p.arm_length() * 0.98;
                if excess > 0.0 {
                    shoulder += to.normalize_or_zero() * excess.min(p.clavicle_reach);
                }
            }
            let target = hands[i].map_or(
                shoulder - spine_dir * (p.arm_length() * 0.95) + fwd * 0.05,
                |h| h.position,
            );
            let pole = -spine_dir + right * (sign * 0.6) - fwd * 0.4;
            let (elbow, hand, ok) = two_bone(shoulder, target, p.upper_arm, p.forearm, pole);
            reached[i] = ok || hands[i].is_none();
            joints[sj.index()] = shoulder;
            joints[ej.index()] = elbow;
            joints[hj.index()] = hand;
        }

        // Legs with procedural stepping.
        let hips = [
            pelvis - right * p.hip_half_width,
            pelvis + right * p.hip_half_width,
        ];
        // Feet support the head, not the pelvis: bowing or crouching shifts the hips back
        // over planted feet instead of making the feet step.
        let support = Vec3::new(neck.x, 0.0, neck.z) - fwd * 0.05;
        let rest = |i: usize| {
            let side = if i == 0 { -1.0 } else { 1.0 };
            let h = support + right * (side * p.hip_half_width);
            Vec3::new(h.x, ground(h.x, h.z) + p.ankle_height, h.z)
        };
        let mut feet = self.feet.unwrap_or_else(|| {
            [0, 1].map(|i| Foot {
                planted: rest(i),
                planted_yaw: yaw,
                step: None,
            })
        });
        let any_stepping = feet.iter().any(|f| f.step.is_some());
        // The foot furthest from its rest spot gets to step first.
        let mut order = [0usize, 1];
        order.sort_by(|&a, &b| {
            rest(b)
                .distance(feet[b].planted)
                .total_cmp(&rest(a).distance(feet[a].planted))
        });
        let mut started = any_stepping;
        for i in order {
            let f = &mut feet[i];
            if let Some((_, to, t)) = &mut f.step {
                *t += dt;
                // Retarget toward the moving rest spot.
                *to = rest(i);
                if *t >= s.step_duration {
                    f.planted = *to;
                    f.planted_yaw = yaw;
                    f.step = None;
                }
                continue;
            }
            let r = rest(i);
            let far =
                Vec3::new(r.x - f.planted.x, 0.0, r.z - f.planted.z).length() > s.step_distance;
            let turned = wrap(yaw - f.planted_yaw).abs() > s.step_turn;
            let fell = (r.y - f.planted.y).abs() > 0.3;
            if fell {
                f.planted = r;
                f.planted_yaw = yaw;
            } else if (far || turned) && !started {
                f.step = Some((f.planted, r, 0.0));
                started = true;
            }
        }
        for (i, (hj, kj, aj)) in [
            (Joint::HipL, Joint::KneeL, Joint::AnkleL),
            (Joint::HipR, Joint::KneeR, Joint::AnkleR),
        ]
        .into_iter()
        .enumerate()
        {
            let f = feet[i];
            let ankle = match f.step {
                Some((from, to, t)) => {
                    let k = (t / s.step_duration).clamp(0.0, 1.0);
                    let e = k * k * (3.0 - 2.0 * k);
                    from.lerp(to, e) + Vec3::Y * (s.step_height * (std::f32::consts::PI * k).sin())
                }
                None => f.planted,
            };
            let pole = fwd + Vec3::Y * 0.1;
            let (knee, ankle, _) = two_bone(hips[i], ankle, p.thigh, p.shin, pole);
            joints[hj.index()] = hips[i];
            joints[kj.index()] = knee;
            joints[aj.index()] = ankle;
        }
        self.feet = Some(feet);

        joints[Joint::Pelvis.index()] = pelvis;
        joints[Joint::Spine.index()] = spine;
        joints[Joint::Chest.index()] = chest;
        joints[Joint::Neck.index()] = neck;
        joints[Joint::Head.index()] = head_joint;
        BodyPose {
            joints,
            body_yaw: yaw,
            reached,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_bone_preserves_lengths_and_bends_toward_pole() {
        let (j, e, ok) = two_bone(
            Vec3::ZERO,
            Vec3::new(0.0, -0.4, -0.2),
            0.3,
            0.27,
            Vec3::NEG_Y,
        );
        assert!(ok);
        assert!((j.length() - 0.3).abs() < 1e-4);
        assert!((j.distance(e) - 0.27).abs() < 1e-4);
        // Out of reach: clamped, straight, not reached.
        let (j, e, ok) = two_bone(Vec3::ZERO, Vec3::new(5.0, 0.0, 0.0), 0.3, 0.27, Vec3::NEG_Y);
        assert!(!ok);
        assert!((e.length() - 0.57).abs() < 1e-3);
        assert!((j.length() - 0.3).abs() < 1e-4);
        // Degenerate target at the root stays finite.
        let (j, e, _) = two_bone(Vec3::ZERO, Vec3::ZERO, 0.3, 0.27, Vec3::ZERO);
        assert!(j.is_finite() && e.is_finite());
    }
}
