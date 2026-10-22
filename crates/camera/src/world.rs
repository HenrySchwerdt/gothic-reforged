//! Collision queries the camera needs from the host.
use glam::Vec3;

/// Ray queries used for the camera boom, dialog shot validation and the first-person wall push.
///
/// Positions and lengths are in world units (cm). `dir` is a unit vector.
pub trait CameraWorld {
    /// Distance to the nearest blocker along the ray, or `None` within `len`.
    fn ray_nearest(&self, from: Vec3, dir: Vec3, len: f32) -> Option<f32>;

    /// Push a point out of nearby walls so it keeps `margin` distance from them (the first-person
    /// wall distance, `MaintainWallDistance`, GameDll:0x1002aa10). The default probes the six axes.
    fn push_out(&self, p: Vec3, margin: f32) -> Vec3 {
        let mut out = p;
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            for sign in [1.0, -1.0] {
                let dir = axis * sign;
                if let Some(d) = self.ray_nearest(p, dir, margin) {
                    out -= dir * (margin - d);
                }
            }
        }
        out
    }
}

/// A world without obstacles.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoWorld;

impl CameraWorld for NoWorld {
    fn ray_nearest(&self, _from: Vec3, _dir: Vec3, _len: f32) -> Option<f32> {
        None
    }
}

impl CameraWorld for gothic_physics::CollisionWorld {
    fn ray_nearest(&self, from: Vec3, dir: Vec3, len: f32) -> Option<f32> {
        self.raycast_accelerated(from, dir, len)
    }
}

#[cfg(test)]
mod tests {
    use super::CameraWorld;
    use glam::Vec3;
    use gothic_physics::{CollisionWorld, Tri};

    #[test]
    fn collision_world_camera_ray_hits_expected_distance_and_respects_range() {
        let mut world = CollisionWorld::new();
        let origin = Vec3::new(-5_000.0, 1_000.0, -5_000.0);
        let dir = Vec3::new(1.0, -0.1, 1.0).normalize();
        let center = origin + dir * 5_000.0;
        let right = dir.cross(Vec3::Y).normalize();
        let up = dir.cross(right).normalize();
        let triangle: Tri = [
            center - right * 100.0 - up * 100.0,
            center + right * 100.0 - up * 100.0,
            center + up * 200.0,
        ];
        world.insert_chunk(1, vec![triangle]);

        let hit = world
            .ray_nearest(origin, dir, 10_000.0)
            .expect("camera ray should hit the fixture triangle");
        assert!((hit - 5_000.0).abs() < 0.01, "unexpected hit distance {hit}");
        assert_eq!(world.ray_nearest(origin, dir, 4_990.0), None);
        assert_eq!(world.ray_nearest(origin, -dir, 10_000.0), None);
    }
}
