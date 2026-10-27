//! The port renders in a mirrored, origin-relative centimetre space (see
//! `world_loader::Origin::point`): `(x, y, z) -> (origin.x - x, y - origin.y, z - origin.z)`.
//! Emitter parameters are mirrored once ([`crate::params::EmitterParams::mirrored_x`]) and
//! every engine transform goes through [`to_view`].
use bevy::math::{Mat4, Vec3};

/// Engine world transform (columns = axes) as a transform of the mirrored view space. The local
/// frame of the entity is mirrored too, matching the mirrored emitter parameters.
pub fn to_view(engine: &Mat4, origin: Vec3) -> Mat4 {
    gothic_coords::Frame::new(origin).mat4(*engine)
}

/// A local (parent-relative) engine transform in the mirrored local frames.
pub fn to_view_local(engine: &Mat4) -> Mat4 {
    gothic_coords::mat4(*engine)
}

/// Engine position to mirrored origin-relative cm.
pub fn point_to_view(p: Vec3, origin: Vec3) -> Vec3 {
    gothic_coords::Frame::new(origin).point(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_match_the_loader_origin_map() {
        let o = Vec3::new(92_400.0, 0.0, -11_000.0);
        let p = Vec3::new(89_421.75, 5_108.0, -12_946.0);
        let m = to_view(&Mat4::from_translation(p), o);
        assert_eq!(m.w_axis.truncate(), point_to_view(p, o));
    }

    #[test]
    fn a_yaw_keeps_its_handedness_under_mirroring() {
        // An engine +x axis tilted toward +z mirrors to -x tilted toward +z.
        let rot = Mat4::from_rotation_y(0.5);
        let v = to_view(&rot, Vec3::ZERO);
        let ex = v.transform_vector3(Vec3::new(-1.0, 0.0, 0.0));
        let local = rot.transform_vector3(Vec3::X);
        assert!((ex - Vec3::new(-local.x, local.y, local.z)).length() < 1e-5);
    }
}
