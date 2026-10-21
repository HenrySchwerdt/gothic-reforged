//! Target pose of an NPC from its [`NpcIntent`]: the full-body IK of `gothic_vr::body`
//! solved from a virtual head (above the feet, leaning into strikes) and the hands (weapon
//! grip from the AI, off hand in a guard), including procedural stepping.
use crate::ai::{NpcIntent, Phase};
use glam::{Quat, Vec3};
use gothic_vr::Pose;
use gothic_vr::body::{BodyIk, BodyPose, Proportions};

/// Eye height of the generic NPC, m.
pub const NPC_EYE_HEIGHT: f32 = 1.68;

#[derive(Debug, Clone)]
pub struct NpcAnimator {
    pub ik: BodyIk,
}

impl Default for NpcAnimator {
    fn default() -> Self {
        Self {
            ik: BodyIk::new(Proportions::for_eye_height(NPC_EYE_HEIGHT)),
        }
    }
}

impl NpcAnimator {
    /// Target pose for this tick.
    pub fn target(
        &mut self,
        intent: &NpcIntent,
        ground: impl Fn(f32, f32) -> f32,
        dt: f32,
    ) -> BodyPose {
        let rot = Quat::from_rotation_y(intent.yaw);
        let floor = ground(intent.root.x, intent.root.z);
        let lean = match intent.phase {
            Phase::Strike(..) => Vec3::new(0.0, -0.05, -0.12),
            Phase::Telegraph(..) => Vec3::new(0.0, 0.0, 0.05),
            Phase::Recoil(_) => Vec3::new(0.0, -0.03, 0.12),
            Phase::Block(_) => Vec3::new(0.0, -0.08, 0.0),
            _ => Vec3::ZERO,
        };
        let head = Pose::new(
            Vec3::new(intent.root.x, floor, intent.root.z)
                + rot * (Vec3::Y * NPC_EYE_HEIGHT + lean),
            rot,
        );
        let off_hand = Pose::new(
            Vec3::new(intent.root.x, floor, intent.root.z) + rot * Vec3::new(-0.25, 1.1, -0.25),
            rot,
        );
        self.ik
            .solve(&head, [Some(off_hand), Some(intent.weapon)], ground, dt)
    }

    /// A standing pose at `root` facing `yaw` (spawning).
    pub fn standing(root: Vec3, yaw: f32) -> BodyPose {
        let mut a = NpcAnimator::default();
        let intent = NpcIntent {
            root,
            yaw,
            weapon: Pose::new(
                root + Quat::from_rotation_y(yaw) * Vec3::new(0.25, 1.1, -0.38),
                Quat::from_rotation_y(yaw),
            ),
            phase: Phase::Ready,
        };
        a.target(&intent, |_, _| root.y, 1.0 / 90.0)
    }
}
