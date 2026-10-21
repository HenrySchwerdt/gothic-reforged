//! Synthetic greybox level (no game content): boxes in metres that become both render meshes
//! (Bevy plugin) and the centimetre [`CollisionWorld`] the character controller walks on.
//!
//! The default layout has the things comfort locomotion must handle: flat floor, a staircase,
//! a walkable ramp, a too-steep ramp, a wall, a raised platform (teleport target) and pillars.
use crate::CM_PER_M;
use glam::{Quat, Vec3};
use gothic_physics::{CollisionWorld, Tri};

/// An oriented box (metres).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GreyboxBox {
    pub name: &'static str,
    pub center: Vec3,
    pub half_extents: Vec3,
    pub rotation: Quat,
    /// sRGB colour for the debug render.
    pub color: [f32; 3],
}

impl GreyboxBox {
    pub fn new(name: &'static str, center: Vec3, half_extents: Vec3) -> Self {
        Self {
            name,
            center,
            half_extents,
            rotation: Quat::IDENTITY,
            color: [0.55, 0.55, 0.58],
        }
    }

    pub fn rotated(mut self, rotation: Quat) -> Self {
        self.rotation = rotation;
        self
    }

    pub fn colored(mut self, color: [f32; 3]) -> Self {
        self.color = color;
        self
    }

    /// Eight corners in world metres.
    pub fn corners(&self) -> [Vec3; 8] {
        let h = self.half_extents;
        let mut out = [Vec3::ZERO; 8];
        for (i, c) in out.iter_mut().enumerate() {
            let s = Vec3::new(
                if i & 1 == 0 { -1.0 } else { 1.0 },
                if i & 2 == 0 { -1.0 } else { 1.0 },
                if i & 4 == 0 { -1.0 } else { 1.0 },
            );
            *c = self.center + self.rotation * (h * s);
        }
        out
    }

    /// Twelve outward-wound triangles in world centimetres.
    pub fn triangles_cm(&self) -> Vec<Tri> {
        let c = self.corners().map(|p| p * CM_PER_M);
        let center = self.center * CM_PER_M;
        // Corner index bits: x=1, y=2, z=4; each face as a quad loop.
        const FACES: [[usize; 4]; 6] = [
            [0, 2, 6, 4],
            [1, 5, 7, 3],
            [0, 4, 5, 1],
            [2, 3, 7, 6],
            [0, 1, 3, 2],
            [4, 6, 7, 5],
        ];
        let mut out = Vec::with_capacity(12);
        for f in FACES {
            let mut q = f.map(|i| c[i]);
            let n = (q[1] - q[0]).cross(q[2] - q[0]);
            let mid = (q[0] + q[1] + q[2] + q[3]) / 4.0;
            if n.dot(mid - center) < 0.0 {
                q.reverse();
            }
            out.push([q[0], q[1], q[2]]);
            out.push([q[0], q[2], q[3]]);
        }
        out
    }
}

/// Number of steps in the default staircase.
pub const STAIR_STEPS: usize = 8;
/// Rise of one stair step (m); below the controller's 0.65 m step height.
pub const STAIR_RISE: f32 = 0.18;
/// Run of one stair step (m).
pub const STAIR_RUN: f32 = 0.32;

/// The default greybox layout. Player starts at the origin looking down -Z.
pub fn default_layout() -> Vec<GreyboxBox> {
    let mut boxes = vec![
        GreyboxBox::new(
            "floor",
            Vec3::new(0.0, -0.25, 0.0),
            Vec3::new(30.0, 0.25, 30.0),
        )
        .colored([0.42, 0.44, 0.40]),
        // A wall to the west for lean-into-wall and blocking tests.
        GreyboxBox::new(
            "wall",
            Vec3::new(-4.0, 1.5, -3.0),
            Vec3::new(0.15, 1.5, 3.0),
        )
        .colored([0.6, 0.5, 0.45]),
        // Raised platform north-east (teleport target), reached by the stairs.
        GreyboxBox::new(
            "platform",
            Vec3::new(4.0, STAIR_RISE * STAIR_STEPS as f32 / 2.0, -10.0),
            Vec3::new(2.0, STAIR_RISE * STAIR_STEPS as f32 / 2.0, 2.0),
        )
        .colored([0.5, 0.55, 0.65]),
    ];
    // Stairs going -Z from z = -2.5, ending at the platform's south edge (z = -8).
    for i in 0..STAIR_STEPS {
        let top = STAIR_RISE * (i + 1) as f32;
        let z = -2.5 - STAIR_RUN * (i as f32 + 0.5);
        boxes.push(
            GreyboxBox::new(
                "stair",
                Vec3::new(4.0, top / 2.0, z),
                Vec3::new(1.0, top / 2.0, STAIR_RUN / 2.0),
            )
            .colored([0.62, 0.6, 0.5]),
        );
    }
    let stair_end = -2.5 - STAIR_RUN * STAIR_STEPS as f32;
    // Fill between the last stair and the platform.
    let gap = (stair_end - -8.0).abs();
    if gap > 0.01 {
        let top = STAIR_RISE * STAIR_STEPS as f32;
        boxes.push(
            GreyboxBox::new(
                "landing",
                Vec3::new(4.0, top / 2.0, (stair_end + -8.0) / 2.0),
                Vec3::new(1.0, top / 2.0, gap / 2.0),
            )
            .colored([0.62, 0.6, 0.5]),
        );
    }
    boxes.extend([
        ramp("ramp_25", Vec3::new(9.0, 0.0, -2.0), 25.0, 6.0).colored([0.5, 0.62, 0.5]),
        ramp("ramp_60", Vec3::new(13.0, 0.0, -2.0), 60.0, 3.0).colored([0.7, 0.4, 0.4]),
        GreyboxBox::new(
            "pillar",
            Vec3::new(-2.0, 1.25, -9.0),
            Vec3::new(0.3, 1.25, 0.3),
        ),
        GreyboxBox::new(
            "pillar",
            Vec3::new(2.0, 1.25, -14.0),
            Vec3::new(0.3, 1.25, 0.3),
        ),
    ]);
    boxes
}

/// A thin slab rising toward -Z at `deg` from `foot` (centre of its low edge) over `length` m.
pub fn ramp(name: &'static str, foot: Vec3, deg: f32, length: f32) -> GreyboxBox {
    let a = deg.to_radians();
    let thick = 0.2;
    let dir = Vec3::new(0.0, a.sin(), -a.cos());
    let up = Vec3::new(0.0, a.cos(), a.sin());
    let center = foot + dir * (length / 2.0) - up * (thick / 2.0);
    GreyboxBox::new(name, center, Vec3::new(1.0, thick / 2.0, length / 2.0))
        .rotated(Quat::from_rotation_x(a))
}

/// Collision world (centimetres) of a layout, one chunk per box.
pub fn collision_world(boxes: &[GreyboxBox]) -> CollisionWorld {
    let mut w = CollisionWorld::new();
    for (i, b) in boxes.iter().enumerate() {
        w.insert_chunk(i as u64, b.triangles_cm());
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_triangles_face_outward() {
        let b = GreyboxBox::new("t", Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.5, 1.0, 2.0));
        let center = b.center * CM_PER_M;
        for t in b.triangles_cm() {
            let n = (t[1] - t[0]).cross(t[2] - t[0]);
            let mid = (t[0] + t[1] + t[2]) / 3.0;
            assert!(n.dot(mid - center) > 0.0, "inward face {t:?}");
        }
    }

    #[test]
    fn ramp_starts_at_its_foot_and_has_its_slope() {
        let r = ramp("r", Vec3::ZERO, 25.0, 6.0);
        let w = collision_world(&[r]);
        let probe = |z: f32| {
            w.ground_below(Vec3::new(0.0, 1000.0, z * CM_PER_M), 2000.0)
                .unwrap()
        };
        let h1 = probe(-1.0);
        let h2 = probe(-3.0);
        let slope = ((h2 - h1) / 200.0).atan().to_degrees();
        assert!((slope - 25.0).abs() < 0.5, "{slope}");
        assert!(h1 > 0.0 && h1 < 60.0);
    }

    #[test]
    fn layout_has_stairs_up_to_the_platform() {
        let w = collision_world(&default_layout());
        let top = w
            .ground_below(Vec3::new(400.0, 1000.0, -1000.0), 2000.0)
            .unwrap();
        assert!((top - STAIR_RISE * STAIR_STEPS as f32 * CM_PER_M).abs() < 0.5);
        let floor = w.ground_below(Vec3::new(0.0, 100.0, 0.0), 500.0).unwrap();
        assert!(floor.abs() < 0.5);
    }
}
