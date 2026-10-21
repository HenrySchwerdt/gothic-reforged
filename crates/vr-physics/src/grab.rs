//! Bevy-free grab selection: which grip of which object a closing hand takes, and where on
//! the object the hand attaches.
use glam::{Quat, Vec3};
use gothic_vr::Pose;
use serde::{Deserialize, Serialize};

/// Role of a grip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GripKind {
    /// The main hand position (a free object's first hand prefers it).
    Primary,
    /// A second-hand position (only taken while another hand holds the object, or when no
    /// primary grip is in reach).
    Secondary,
    /// Any hand, any time (arrows, staffs).
    Any,
}

/// A grip on an object: a point (`a == b`) or a handle segment `a..b` the hand can take
/// anywhere along, with the hand frame's rotation relative to the object.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GripPoint {
    pub a: Vec3,
    pub b: Vec3,
    pub rotation: Quat,
    pub kind: GripKind,
    /// How close the hand must be, m.
    pub radius: f32,
}

impl GripPoint {
    /// Closest point on the grip (object frame) to `p` (object frame).
    pub fn closest(&self, p: Vec3) -> Vec3 {
        let d = self.b - self.a;
        let len2 = d.length_squared();
        if len2 < 1e-12 {
            return self.a;
        }
        self.a + d * ((p - self.a).dot(d) / len2).clamp(0.0, 1.0)
    }
}

/// A candidate object for grabbing.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a, K> {
    pub key: K,
    /// Object world pose.
    pub pose: Pose,
    pub grips: &'a [GripPoint],
    pub free_grab_radius: Option<f32>,
    /// Object-frame anchors already taken by other hands.
    pub taken: &'a [Vec3],
}

/// Where a hand attaches: the object-frame pose that the hand's grip frame should coincide with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GrabChoice<K> {
    pub key: K,
    pub anchor: Vec3,
    pub rotation: Quat,
    /// Index of the grip used, `None` for a free grab.
    pub grip: Option<usize>,
    pub distance: f32,
}

/// Minimum distance between two hands on the same handle, m.
pub const HAND_SPACING: f32 = 0.08;

/// Pick the best grab for a hand at `hand` (world) among `candidates`.
pub fn choose_grab<K: Copy>(hand: &Pose, candidates: &[Candidate<'_, K>]) -> Option<GrabChoice<K>> {
    let mut best: Option<(f32, GrabChoice<K>)> = None;
    for c in candidates {
        let inv = c.pose.inverse();
        let local = inv.transform_point(hand.position);
        let held = !c.taken.is_empty();
        for (i, g) in c.grips.iter().enumerate() {
            let allowed = match g.kind {
                GripKind::Primary => !held,
                GripKind::Secondary => true,
                GripKind::Any => true,
            };
            if !allowed {
                continue;
            }
            let mut anchor = g.closest(local);
            // Do not stack two hands on one spot: slide along the handle, or skip a point.
            if let Some(t) = c.taken.iter().find(|t| t.distance(anchor) < HAND_SPACING) {
                let dir = (g.b - g.a).normalize_or_zero();
                if dir == Vec3::ZERO {
                    continue;
                }
                let side = if (anchor - *t).dot(dir) >= 0.0 {
                    1.0
                } else {
                    -1.0
                };
                anchor = g.closest(*t + dir * side * HAND_SPACING);
                if c.taken
                    .iter()
                    .any(|t| t.distance(anchor) < HAND_SPACING - 1e-3)
                {
                    continue;
                }
            }
            let dist = anchor.distance(local);
            if dist > g.radius {
                continue;
            }
            // Secondary grips lose ties against primary ones on a free object.
            let rot_err = (c.pose.rotation * g.rotation).angle_between(hand.rotation);
            let mut score = dist + 0.02 * rot_err;
            if g.kind == GripKind::Secondary && !held {
                score += 0.05;
            }
            if best.as_ref().is_none_or(|(s, _)| score < *s) {
                best = Some((
                    score,
                    GrabChoice {
                        key: c.key,
                        anchor,
                        rotation: g.rotation,
                        grip: Some(i),
                        distance: dist,
                    },
                ));
            }
        }
        if let Some(r) = c.free_grab_radius {
            let dist = local.length();
            if dist <= r {
                let score = dist + 0.1;
                if best.as_ref().is_none_or(|(s, _)| score < *s) {
                    best = Some((
                        score,
                        GrabChoice {
                            key: c.key,
                            anchor: local,
                            rotation: inv.rotation * hand.rotation,
                            grip: None,
                            distance: dist,
                        },
                    ));
                }
            }
        }
    }
    best.map(|(_, c)| c)
}

/// World pose the object must have so that its grip frame (`anchor`, `rotation`) coincides
/// with the hand pose.
pub fn object_pose_for_hand(hand: &Pose, anchor: Vec3, rotation: Quat) -> Pose {
    let grip = Pose::new(anchor, rotation);
    hand.mul(&grip.inverse())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::props;

    fn cand<'a>(
        key: u32,
        pose: Pose,
        spec: &'a props::PropSpec,
        taken: &'a [Vec3],
    ) -> Candidate<'a, u32> {
        Candidate {
            key,
            pose,
            grips: &spec.grips,
            free_grab_radius: spec.free_grab_radius,
            taken,
        }
    }

    #[test]
    fn picks_the_primary_grip_of_the_nearest_weapon() {
        let sword = props::sword();
        let axe = props::axe();
        let hand = Pose::new(Vec3::new(0.02, 1.0, 0.0), Quat::IDENTITY);
        let c = [
            cand(
                1,
                Pose::new(Vec3::new(0.0, 1.0, 0.0), Quat::IDENTITY),
                &sword,
                &[],
            ),
            cand(
                2,
                Pose::new(Vec3::new(0.5, 1.0, 0.0), Quat::IDENTITY),
                &axe,
                &[],
            ),
        ];
        let g = choose_grab(&hand, &c).unwrap();
        assert_eq!(g.key, 1);
        assert_eq!(g.grip, Some(0));
        assert!(g.anchor.length() < 1e-6);
        // Out of reach.
        let far = Pose::new(Vec3::new(2.0, 1.0, 0.0), Quat::IDENTITY);
        assert!(choose_grab(&far, &c).is_none());
    }

    #[test]
    fn second_hand_takes_the_handle_without_stacking() {
        let sword = props::two_handed_sword();
        let taken = [Vec3::ZERO];
        let obj = Pose::new(Vec3::new(0.0, 1.0, 0.0), Quat::IDENTITY);
        // Second hand right on the first one: slides up the handle.
        let hand = Pose::new(Vec3::new(0.0, 1.0, 0.01), Quat::IDENTITY);
        let g = choose_grab(&hand, &[cand(1, obj, &sword, &taken)]).unwrap();
        assert_eq!(sword.grips[g.grip.unwrap()].kind, GripKind::Secondary);
        assert!(g.anchor.z >= HAND_SPACING - 1e-4, "{}", g.anchor);
        // Further down the handle: takes the spot under the hand.
        let hand = Pose::new(Vec3::new(0.0, 1.0, 0.2), Quat::IDENTITY);
        let g = choose_grab(&hand, &[cand(1, obj, &sword, &taken)]).unwrap();
        assert!((g.anchor.z - 0.2).abs() < 1e-4);
    }

    #[test]
    fn free_grab_keeps_relative_pose() {
        let c = props::crate_box(Vec3::splat(0.2), 4.0);
        let obj = Pose::new(Vec3::new(1.0, 0.5, 0.0), Quat::from_rotation_y(0.4));
        let hand = Pose::new(Vec3::new(1.1, 0.7, 0.05), Quat::from_rotation_z(0.3));
        let g = choose_grab(&hand, &[cand(7, obj, &c, &[])]).unwrap();
        assert_eq!(g.grip, None);
        let back = object_pose_for_hand(&hand, g.anchor, g.rotation);
        assert!(back.position.distance(obj.position) < 1e-5);
        assert!(back.rotation.angle_between(obj.rotation) < 1e-4);
    }

    #[test]
    fn grip_alignment_pose() {
        let hand = Pose::new(Vec3::new(0.3, 1.2, -0.4), Quat::from_rotation_y(1.0));
        let p = object_pose_for_hand(&hand, Vec3::new(0.0, 0.0, 0.1), Quat::IDENTITY);
        let grip_world = p.transform_point(Vec3::new(0.0, 0.0, 0.1));
        assert!(grip_world.distance(hand.position) < 1e-5);
    }
}
