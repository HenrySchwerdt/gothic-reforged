//! Swept tests against the vertical capsule of a character.
//!
//! The engine hits characters through their PhysX collision shapes; the host that has no such
//! shapes can approximate them with [`Capsule`] (the hero's movement cylinder is 133.68 high with
//! a 33.16 radius, `PHYSICS_MOVEMENT.md`).
use glam::Vec3;

/// A vertical capsule: the cylinder of a standing character with rounded ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Capsule {
    /// Centre of the bottom (the feet).
    pub base: Vec3,
    /// Total height, cm.
    pub height: f32,
    /// Radius, cm.
    pub radius: f32,
}

impl Capsule {
    /// Where on the body a point is: `0` feet, `1` top.
    pub fn height_fraction(&self, p: Vec3) -> f32 {
        if self.height <= f32::EPSILON {
            return 0.0;
        }
        ((p.y - self.base.y) / self.height).clamp(0.0, 1.0)
    }

    /// Axis end points of the capsule (the sphere centres).
    fn axis(&self) -> (Vec3, Vec3) {
        let r = self.radius.min(self.height * 0.5);
        (
            self.base + Vec3::Y * r,
            self.base + Vec3::Y * (self.height - r).max(r),
        )
    }
}

/// First parameter `t` in `0..=1` at which the segment `from -> to`, swollen by `extra` (the
/// projectile's own radius), touches the capsule; `Some(0)` when it starts inside.
pub fn sweep(from: Vec3, to: Vec3, cap: &Capsule, extra: f32) -> Option<f32> {
    let r = cap.radius.min(cap.height * 0.5) + extra.max(0.0);
    let (a, b) = cap.axis();
    let d = to - from;
    let mut best: Option<f32> = None;
    let mut take = |t: f32| {
        if (0.0..=1.0).contains(&t) {
            best = Some(best.map_or(t, |c: f32| c.min(t)));
        }
    };

    // Starts inside the capsule.
    if point_segment_distance_sq(from, a, b) <= r * r {
        return Some(0.0);
    }

    // The cylinder between the two sphere centres.
    let o = Vec3::new(from.x - a.x, 0.0, from.z - a.z);
    let dh = Vec3::new(d.x, 0.0, d.z);
    let qa = dh.length_squared();
    if qa > 1e-9 {
        let qb = 2.0 * o.dot(dh);
        let qc = o.length_squared() - r * r;
        let disc = qb * qb - 4.0 * qa * qc;
        if disc >= 0.0 {
            let t = (-qb - disc.sqrt()) / (2.0 * qa);
            let y = from.y + d.y * t;
            if y >= a.y && y <= b.y {
                take(t);
            }
        }
    }
    // The two end spheres.
    for c in [a, b] {
        let m = from - c;
        let qa = d.length_squared();
        if qa <= 1e-9 {
            continue;
        }
        let qb = 2.0 * m.dot(d);
        let qc = m.length_squared() - r * r;
        let disc = qb * qb - 4.0 * qa * qc;
        if disc >= 0.0 {
            take((-qb - disc.sqrt()) / (2.0 * qa));
        }
    }
    best
}

fn point_segment_distance_sq(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 <= 1e-9 {
        0.0
    } else {
        ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
    };
    (a + ab * t - p).length_squared()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hero() -> Capsule {
        Capsule {
            base: Vec3::ZERO,
            height: 133.68,
            radius: 33.16,
        }
    }

    #[test]
    fn a_shot_at_the_chest_hits_the_front_of_the_capsule() {
        let t = sweep(
            Vec3::new(-500.0, 100.0, 0.0),
            Vec3::new(500.0, 100.0, 0.0),
            &hero(),
            0.0,
        )
        .expect("hit");
        let x = -500.0 + 1000.0 * t;
        assert!((x + 33.16).abs() < 0.01, "x = {x}");
    }

    #[test]
    fn a_shot_over_the_head_misses() {
        assert!(
            sweep(
                Vec3::new(-500.0, 200.0, 0.0),
                Vec3::new(500.0, 200.0, 0.0),
                &hero(),
                0.0
            )
            .is_none()
        );
    }

    #[test]
    fn a_shot_to_the_side_misses_but_the_projectile_radius_widens_the_body() {
        let from = Vec3::new(-500.0, 100.0, 36.0);
        let to = Vec3::new(500.0, 100.0, 36.0);
        assert!(sweep(from, to, &hero(), 0.0).is_none());
        assert!(sweep(from, to, &hero(), 5.0).is_some());
    }

    #[test]
    fn starting_inside_hits_at_once_and_short_segments_stop_short() {
        assert_eq!(
            sweep(
                Vec3::new(0.0, 50.0, 0.0),
                Vec3::new(10.0, 50.0, 0.0),
                &hero(),
                0.0
            ),
            Some(0.0)
        );
        assert!(
            sweep(
                Vec3::new(-500.0, 100.0, 0.0),
                Vec3::new(-400.0, 100.0, 0.0),
                &hero(),
                0.0
            )
            .is_none()
        );
    }

    #[test]
    fn a_vertical_shot_hits_the_top_cap() {
        let t = sweep(
            Vec3::new(0.0, 400.0, 0.0),
            Vec3::new(0.0, 0.0, 0.0),
            &hero(),
            0.0,
        )
        .expect("hit");
        assert!((400.0 - 400.0 * t - 133.68).abs() < 0.01);
    }

    #[test]
    fn height_fraction_tells_a_head_shot() {
        let c = hero();
        assert!(c.height_fraction(Vec3::new(0.0, 125.0, 0.0)) > 0.9);
        assert!(c.height_fraction(Vec3::new(0.0, 60.0, 0.0)) < 0.5);
    }
}
