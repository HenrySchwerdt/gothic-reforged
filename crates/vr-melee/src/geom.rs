//! Small geometry helpers (capsules as segments with a radius).
use glam::Vec3;

/// Closest points between segments `p1-q1` and `p2-q2`: (point on first, point on second,
/// parameter on first, parameter on second).
pub fn segment_segment(p1: Vec3, q1: Vec3, p2: Vec3, q2: Vec3) -> (Vec3, Vec3, f32, f32) {
    let d1 = q1 - p1;
    let d2 = q2 - p2;
    let r = p1 - p2;
    let a = d1.dot(d1);
    let e = d2.dot(d2);
    let f = d2.dot(r);
    let (s, t);
    if a <= 1e-12 && e <= 1e-12 {
        return (p1, p2, 0.0, 0.0);
    }
    if a <= 1e-12 {
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = d1.dot(r);
        if e <= 1e-12 {
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = d1.dot(d2);
            let denom = a * e - b * b;
            let mut s0 = if denom > 1e-12 {
                ((b * f - c * e) / denom).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut t0 = (b * s0 + f) / e;
            if t0 < 0.0 {
                t0 = 0.0;
                s0 = (-c / a).clamp(0.0, 1.0);
            } else if t0 > 1.0 {
                t0 = 1.0;
                s0 = ((b - c) / a).clamp(0.0, 1.0);
            }
            s = s0;
            t = t0;
        }
    }
    (p1 + d1 * s, p2 + d2 * t, s, t)
}

/// Do two capsules overlap? Returns the contact point (between the closest points) and the
/// separating normal pointing from the second toward the first.
pub fn capsule_overlap(
    a0: Vec3,
    a1: Vec3,
    ra: f32,
    b0: Vec3,
    b1: Vec3,
    rb: f32,
) -> Option<(Vec3, Vec3, f32)> {
    let (pa, pb, s, _) = segment_segment(a0, a1, b0, b1);
    let d = pa - pb;
    let dist = d.length();
    if dist > ra + rb {
        return None;
    }
    let n = if dist > 1e-6 { d / dist } else { Vec3::Y };
    // Point on the surface of the second capsule toward the first.
    let point = pb + n * rb.min(dist);
    Some((point, n, s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossing_segments() {
        let (a, b, s, t) = segment_segment(
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, -1.0, 0.5),
            Vec3::new(0.0, 1.0, 0.5),
        );
        assert!(a.distance(Vec3::ZERO) < 1e-6 && b.distance(Vec3::new(0.0, 0.0, 0.5)) < 1e-6);
        assert!((s - 0.5).abs() < 1e-6 && (t - 0.5).abs() < 1e-6);
    }

    #[test]
    fn capsules() {
        let hit = capsule_overlap(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            0.1,
            Vec3::new(0.5, 0.15, -1.0),
            Vec3::new(0.5, 0.15, 1.0),
            0.1,
        )
        .unwrap();
        assert!(hit.1.y < -0.99);
        assert!(
            capsule_overlap(Vec3::ZERO, Vec3::X, 0.1, Vec3::Y, Vec3::Y + Vec3::X, 0.1).is_none()
        );
    }
}
