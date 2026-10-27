//! Plain-data transforms and pose blending (no ECS types, unit-testable).
use bevy::math::{Affine3A, Quat, Vec3};

/// Translation, rotation, scale of one node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trs {
    pub t: Vec3,
    pub r: Quat,
    pub s: Vec3,
}

impl Trs {
    pub const IDENTITY: Trs = Trs {
        t: Vec3::ZERO,
        r: Quat::IDENTITY,
        s: Vec3::ONE,
    };

    #[inline]
    pub fn affine(&self) -> Affine3A {
        Affine3A::from_scale_rotation_translation(self.s, self.r, self.t)
    }

    pub fn from_affine(a: Affine3A) -> Self {
        let (s, r, t) = a.to_scale_rotation_translation();
        Trs {
            t,
            r: r.normalize(),
            s,
        }
    }

    /// Linear blend: lerp for translation and scale, shortest-arc nlerp for rotation.
    #[inline]
    pub fn blend(self, other: Trs, w: f32) -> Trs {
        Trs {
            t: self.t.lerp(other.t, w),
            r: nlerp(self.r, other.r, w),
            s: self.s.lerp(other.s, w),
        }
    }
}

/// Shortest-arc normalised linear quaternion interpolation (cheap and accurate for the
/// small angles between 25 Hz keys and for animation cross-fades).
#[inline]
pub fn nlerp(a: Quat, b: Quat, w: f32) -> Quat {
    let b = if a.dot(b) < 0.0 { -b } else { b };
    let q = a * (1.0 - w) + b * w;
    let len2 = q.length_squared();
    if len2 > 1e-12 {
        q * len2.sqrt().recip()
    } else {
        a
    }
}

/// Reflection across the YZ plane (x -> -x) applied to a quaternion: the rotation `R` becomes
/// `M R M` with `M = diag(-1, 1, 1)`, i.e. (x, y, z, w) -> (x, -y, -z, w).
#[inline]
pub fn mirror_quat_x(q: Quat) -> Quat {
    gothic_coords::quat(q)
}

impl Trs {
    /// Conjugates the transform with the X reflection (`M T M`). Applied to every local
    /// transform of a skeleton hierarchy it mirrors the whole character across the YZ plane.
    #[inline]
    pub fn mirror_x(self) -> Trs {
        Trs {
            t: gothic_coords::vec3(self.t),
            r: mirror_quat_x(self.r),
            s: self.s,
        }
    }
}

/// Sanitised quaternion from file components (x, y, z, w).
pub fn quat_from_file(q: [f32; 4]) -> Quat {
    let q = Quat::from_xyzw(q[0], q[1], q[2], q[3]);
    if q.length_squared() > 1e-12 && q.is_finite() {
        q.normalize()
    } else {
        Quat::IDENTITY
    }
}

/// Rotation angle of `q` about +Y (twist component), in radians.
pub fn yaw_of(q: Quat) -> f32 {
    let twist = Quat::from_xyzw(0.0, q.y, 0.0, q.w);
    let twist = if twist.length_squared() > 1e-12 {
        twist.normalize()
    } else {
        Quat::IDENTITY
    };
    let angle = 2.0 * twist.y.atan2(twist.w);
    if angle > std::f32::consts::PI {
        angle - std::f32::consts::TAU
    } else if angle < -std::f32::consts::PI {
        angle + std::f32::consts::TAU
    } else {
        angle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nlerp_takes_shortest_arc() {
        let a = Quat::IDENTITY;
        let b = -Quat::from_rotation_y(0.2);
        let m = nlerp(a, b, 0.5);
        assert!((yaw_of(m) - 0.1).abs() < 1e-3);
    }

    #[test]
    fn blend_endpoints() {
        let a = Trs::IDENTITY;
        let b = Trs {
            t: Vec3::new(1.0, 2.0, 3.0),
            r: Quat::from_rotation_x(1.0),
            s: Vec3::splat(2.0),
        };
        assert_eq!(a.blend(b, 0.0).t, a.t);
        assert!((a.blend(b, 1.0).t - b.t).length() < 1e-6);
        assert!(a.blend(b, 1.0).r.dot(b.r).abs() > 0.9999);
    }

    #[test]
    fn mirror_is_conjugation() {
        let m = Affine3A::from_scale(Vec3::new(-1.0, 1.0, 1.0));
        let a = Trs {
            t: Vec3::new(1.0, 2.0, 3.0),
            r: Quat::from_euler(bevy::math::EulerRot::XYZ, 0.3, -0.7, 1.1),
            s: Vec3::ONE,
        };
        let want = m * a.affine() * m;
        let got = a.mirror_x().affine();
        assert!(want.abs_diff_eq(got, 1e-5), "{want:?} vs {got:?}");
        // Mirroring twice is the identity and a chain composes under the reflection.
        assert!(a.mirror_x().mirror_x().r.dot(a.r).abs() > 0.99999);
        let chain = a.affine() * a.affine();
        let mchain = a.mirror_x().affine() * a.mirror_x().affine();
        assert!((m * chain * m).abs_diff_eq(mchain, 1e-4));
    }

    #[test]
    fn yaw_roundtrip() {
        for angle in [-3.0f32, -1.0, 0.0, 0.5, 2.5] {
            assert!((yaw_of(Quat::from_rotation_y(angle)) - angle).abs() < 1e-4);
        }
    }
}
