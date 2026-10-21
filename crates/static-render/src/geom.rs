//! Bounding boxes, planes and frusta (right-handed, Bevy conventions).
use glam::{Affine3A, Mat4, Vec3, Vec3A, Vec4};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb {
        min: Vec3::splat(f32::INFINITY),
        max: Vec3::splat(f32::NEG_INFINITY),
    };

    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    pub fn from_center_half(center: Vec3, half: Vec3) -> Self {
        Self::new(center - half, center + half)
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn half_extents(&self) -> Vec3 {
        (self.max - self.min) * 0.5
    }

    pub fn union(&self, o: &Aabb) -> Aabb {
        Aabb::new(self.min.min(o.min), self.max.max(o.max))
    }

    pub fn is_empty(&self) -> bool {
        self.min.cmpgt(self.max).any()
    }

    /// World bounds of this local box under `t` (exact for affine transforms).
    pub fn transformed(&self, t: &Affine3A) -> Aabb {
        let c = t.transform_point3a(Vec3A::from(self.center()));
        let h = Vec3A::from(self.half_extents());
        let m = t.matrix3;
        let e = m.x_axis.abs() * h.x + m.y_axis.abs() * h.y + m.z_axis.abs() * h.z;
        Aabb::new((c - e).into(), (c + e).into())
    }

    /// Distance from `p` to the box (0 inside).
    pub fn distance(&self, p: Vec3) -> f32 {
        (self.min - p).max(p - self.max).max(Vec3::ZERO).length()
    }

    pub fn contains_point(&self, p: Vec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }
}

/// `n . p + d >= 0` is inside.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    pub n: Vec3,
    pub d: f32,
}

impl Plane {
    fn from_vec4(v: Vec4) -> Self {
        let n = v.truncate();
        let len = n.length().max(1e-20);
        Self {
            n: n / len,
            d: v.w / len,
        }
    }

    pub fn distance(&self, p: Vec3) -> f32 {
        self.n.dot(p) + self.d
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Containment {
    Outside,
    Intersects,
    Inside,
}

/// Convex volume of up to 6 planes.
#[derive(Debug, Clone, PartialEq)]
pub struct Frustum {
    pub planes: Vec<Plane>,
}

impl Frustum {
    /// Planes of a `clip_from_world` matrix (wgpu depth range 0..1; reversed-Z works too
    /// because both depth planes are taken from the `w ± z` / `z` rows symmetrically).
    /// `far`: also clip at the far plane (off for Bevy's infinite reversed-Z projections).
    pub fn from_clip_from_world(m: &Mat4, far: bool) -> Self {
        let r = [m.row(0), m.row(1), m.row(2), m.row(3)];
        let mut planes = vec![
            Plane::from_vec4(r[3] + r[0]),
            Plane::from_vec4(r[3] - r[0]),
            Plane::from_vec4(r[3] + r[1]),
            Plane::from_vec4(r[3] - r[1]),
            Plane::from_vec4(r[2]),
        ];
        if far {
            planes.push(Plane::from_vec4(r[3] - r[2]));
        }
        Self { planes }
    }

    /// Box-shaped volume (e.g. an orthographic shadow cascade) given by planes directly.
    pub fn from_planes(planes: Vec<Plane>) -> Self {
        Self { planes }
    }

    pub fn test_aabb(&self, b: &Aabb) -> Containment {
        let c = b.center();
        let h = b.half_extents();
        let mut inside = true;
        for p in &self.planes {
            let r = h.x * p.n.x.abs() + h.y * p.n.y.abs() + h.z * p.n.z.abs();
            let s = p.distance(c);
            if s < -r {
                return Containment::Outside;
            }
            if s < r {
                inside = false;
            }
        }
        if inside {
            Containment::Inside
        } else {
            Containment::Intersects
        }
    }

    pub fn intersects_aabb(&self, b: &Aabb) -> bool {
        self.test_aabb(b) != Containment::Outside
    }
}

/// The volume that can cast shadows into a shadow cascade covering the view slice
/// `near..far` of a camera: the slice's corners in light space, boxed, and extended
/// `caster_reach` towards the light (casters outside the slice still throw shadows into it).
pub fn cascade_caster_volume(
    world_from_view: &Mat4,
    fov_y: f32,
    aspect: f32,
    near: f32,
    far: f32,
    light_dir: Vec3,
    caster_reach: f32,
) -> Frustum {
    let to_light = (-light_dir).normalize_or(Vec3::Y);
    // Light basis: z towards the light.
    let z = to_light;
    let x = if z.y.abs() < 0.99 {
        Vec3::Y.cross(z)
    } else {
        Vec3::X.cross(z)
    }
    .normalize();
    let y = z.cross(x);
    let (ty, tx) = ((fov_y * 0.5).tan(), (fov_y * 0.5).tan() * aspect);
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    for d in [near, far] {
        for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let p = world_from_view.transform_point3(Vec3::new(sx * tx * d, sy * ty * d, -d));
            let l = Vec3::new(p.dot(x), p.dot(y), p.dot(z));
            lo = lo.min(l);
            hi = hi.max(l);
        }
    }
    hi.z += caster_reach;
    let planes = vec![
        Plane { n: x, d: -lo.x },
        Plane { n: -x, d: hi.x },
        Plane { n: y, d: -lo.y },
        Plane { n: -y, d: hi.y },
        Plane { n: z, d: -lo.z },
        Plane { n: -z, d: hi.z },
    ];
    Frustum::from_planes(planes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    #[test]
    fn transformed_aabb_is_tight_for_rotations() {
        let b = Aabb::new(Vec3::splat(-1.0), Vec3::splat(1.0));
        let t = Affine3A::from_rotation_translation(
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_4),
            Vec3::new(10.0, 0.0, 0.0),
        );
        let w = b.transformed(&t);
        let s = 2f32.sqrt();
        assert!((w.max - Vec3::new(10.0 + s, 1.0, s)).abs().max_element() < 1e-5);
        assert!((w.min - Vec3::new(10.0 - s, -1.0, -s)).abs().max_element() < 1e-5);
    }

    #[test]
    fn perspective_frustum_classifies_boxes() {
        let view = glam::camera::rh::view::look_at_mat4(Vec3::ZERO, -Vec3::Z, Vec3::Y);
        let proj =
            glam::camera::rh::proj::directx::perspective(60f32.to_radians(), 1.0, 0.1, 100.0);
        let f = Frustum::from_clip_from_world(&(proj * view), true);
        let unit = |c: Vec3| Aabb::from_center_half(c, Vec3::splat(0.5));
        assert_eq!(
            f.test_aabb(&unit(Vec3::new(0.0, 0.0, -10.0))),
            Containment::Inside
        );
        assert_eq!(
            f.test_aabb(&unit(Vec3::new(0.0, 0.0, 10.0))),
            Containment::Outside
        );
        assert_eq!(
            f.test_aabb(&unit(Vec3::new(50.0, 0.0, -10.0))),
            Containment::Outside
        );
        assert_eq!(
            f.test_aabb(&unit(Vec3::new(0.0, 0.0, -200.0))),
            Containment::Outside
        );
        assert_eq!(
            f.test_aabb(&unit(Vec3::new(0.0, 0.0, -100.0))),
            Containment::Intersects
        );
        // Reversed-Z infinite projection (Bevy's default): no far plane.
        let inf = glam::camera::rh::proj::directx::perspective_infinite_reverse(
            60f32.to_radians(),
            1.0,
            0.1,
        );
        let f = Frustum::from_clip_from_world(&(inf * view), false);
        assert!(f.intersects_aabb(&unit(Vec3::new(0.0, 0.0, -5000.0))));
        assert!(!f.intersects_aabb(&unit(Vec3::new(0.0, 0.0, 5.0))));
    }

    #[test]
    fn cascade_volume_reaches_towards_the_light() {
        let world_from_view = Mat4::IDENTITY; // camera at origin looking -z
        let light = Vec3::new(0.0, -1.0, 0.0); // straight down
        let v = cascade_caster_volume(&world_from_view, 1.0, 1.0, 0.1, 20.0, light, 50.0);
        let unit = |c: Vec3| Aabb::from_center_half(c, Vec3::splat(0.5));
        assert!(v.intersects_aabb(&unit(Vec3::new(0.0, 0.0, -10.0))));
        assert!(
            v.intersects_aabb(&unit(Vec3::new(0.0, 40.0, -10.0))),
            "caster above"
        );
        assert!(
            !v.intersects_aabb(&unit(Vec3::new(0.0, 0.0, -40.0))),
            "beyond the slice"
        );
        assert!(
            !v.intersects_aabb(&unit(Vec3::new(0.0, 0.0, 30.0))),
            "behind the camera"
        );
    }
}
