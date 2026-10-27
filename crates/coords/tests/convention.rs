//! Property and asymmetric-fixture tests of the Gothic -> engine conversion.
use glam::{Affine3A, Mat3, Mat4, Quat, Vec3};
use gothic_coords as gc;

/// Deterministic pseudo random numbers in `[-1, 1]` (no extra dependency).
struct Rng(u64);
impl Rng {
    fn f(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32 / (1u64 << 23) as f32) * 2.0 - 1.0
    }
    fn v(&mut self) -> Vec3 {
        Vec3::new(self.f(), self.f(), self.f()) * 500.0
    }
    fn q(&mut self) -> Quat {
        Quat::from_xyzw(self.f(), self.f(), self.f(), self.f() + 1.5).normalize()
    }
}

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-3 * (1.0 + a.length())
}

#[test]
fn every_conversion_is_an_involution() {
    let mut r = Rng(1);
    for _ in 0..200 {
        let (v, q) = (r.v(), r.q());
        assert!(close(gc::vec3(gc::vec3(v)), v));
        assert!(close(gc::axial(gc::axial(v)), v));
        assert!(gc::quat(gc::quat(q)).dot(q).abs() > 0.99999);
        assert!((gc::heading(gc::heading(0.7)) - 0.7).abs() < 1e-7);
        let m = Mat4::from_scale_rotation_translation(Vec3::new(1.0, 2.0, 3.0), q, v);
        assert!(gc::mat4(gc::mat4(m)).abs_diff_eq(m, 1e-3));
    }
}

#[test]
fn quaternion_is_the_conjugation_with_the_reflection() {
    let mut r = Rng(2);
    for _ in 0..200 {
        let (q, v) = (r.q(), r.v());
        // Rotating then converting == converting then rotating by the converted rotation.
        assert!(close(gc::quat(q) * gc::vec3(v), gc::vec3(q * v)));
        // Matrix form: M' = F M F.
        let want = gc::FLIP * Mat3::from_quat(q) * gc::FLIP;
        assert!(Mat3::from_quat(gc::quat(q)).abs_diff_eq(want, 1e-5));
        // A proper rotation stays proper.
        assert!((Mat3::from_quat(gc::quat(q)).determinant() - 1.0).abs() < 1e-4);
    }
}

#[test]
fn composition_commutes_with_the_conversion() {
    let mut r = Rng(3);
    for _ in 0..100 {
        let (a, b) = (r.q(), r.q());
        assert!((gc::quat(a * b)).dot(gc::quat(a) * gc::quat(b)).abs() > 0.9999);
        let (ma, mb) = (
            Mat4::from_rotation_translation(a, r.v()),
            Mat4::from_rotation_translation(b, r.v()),
        );
        assert!(gc::mat4(ma * mb).abs_diff_eq(gc::mat4(ma) * gc::mat4(mb), 1e-2));
        let aff = Affine3A::from_mat4(ma);
        assert!(gc::affine(aff).abs_diff_eq(Affine3A::from_mat4(gc::mat4(ma)), 1e-3));
    }
}

#[test]
fn heading_is_a_negated_yaw() {
    for a in [-3.0f32, -1.2, 0.0, 0.4, 2.9] {
        let g = Quat::from_rotation_y(a);
        assert!(gc::quat(g).dot(Quat::from_rotation_y(gc::heading(a))).abs() > 0.99999);
    }
    // The Genome heading turns +Z towards +X; in the mirrored world the same turn goes to -X.
    let gothic = Quat::from_rotation_y(0.5) * Vec3::Z;
    assert!(gothic.x > 0.0);
    let engine = Quat::from_rotation_y(gc::heading(0.5)) * Vec3::Z;
    assert!(close(engine, gc::vec3(gothic)));
}

#[test]
fn cross_products_are_axial() {
    let mut r = Rng(4);
    for _ in 0..200 {
        let (a, b) = (r.v(), r.v());
        assert!(close(gc::vec3(a).cross(gc::vec3(b)), gc::axial(a.cross(b))));
    }
}

/// A character has a left and a right hand. Gothic (left-handed): right = up x forward; engine
/// (right-handed): right = forward x up. A hand that is on the right of the Gothic character is
/// on the right of the engine character.
#[test]
fn left_hand_stays_left() {
    let mut r = Rng(5);
    for _ in 0..200 {
        let yaw = r.f() * 3.1;
        // Gothic rotation about Y turns +Z towards +X; the authored skeleton faces -X.
        let rot = Quat::from_rotation_y(yaw);
        let fwd = rot * Vec3::NEG_X;
        let up = Vec3::Y;
        let right_dir = up.cross(fwd); // left-handed rule
        let right_hand = right_dir * 30.0 + up * 120.0;
        let left_hand = -right_dir * 30.0 + up * 120.0;
        let (f2, u2) = (gc::vec3(fwd), gc::vec3(up));
        let right2 = f2.cross(u2); // right-handed rule
        assert!((gc::vec3(right_hand) - gc::vec3(up) * 120.0).dot(right2) > 0.0);
        assert!((gc::vec3(left_hand) - gc::vec3(up) * 120.0).dot(right2) < 0.0);
    }
    // The fixed pose: facing -X, the right hand at +Z; mirrored it faces +X and the right hand is
    // still at +Z.
    let (fwd, right) = (Vec3::NEG_X, Vec3::Z);
    assert!(Vec3::Y.cross(fwd).dot(right) > 0.99);
    assert!(gc::vec3(fwd).cross(Vec3::Y).dot(gc::vec3(right)) > 0.99);
}

/// A sign quad whose texture coordinate u grows to the viewer's right reads left to right in
/// both conventions.
#[test]
fn sign_text_reads_left_to_right() {
    // Gothic, left-handed: the viewer stands at -Z looking +Z; screen right = +X.
    let quad = [
        (Vec3::new(-1.0, 0.0, 0.0), 0.0f32),
        (Vec3::new(1.0, 0.0, 0.0), 1.0),
    ];
    let view_g = Vec3::Z;
    let right_g = Vec3::Y.cross(view_g);
    assert!((quad[1].0 - quad[0].0).dot(right_g) > 0.0);
    // Engine, right-handed: the same viewer looks along +Z (mirrored world, not rotated) and
    // screen right = view x up = -X.
    let view_e = gc::vec3(view_g);
    let right_e = view_e.cross(Vec3::Y);
    let (p0, p1) = (gc::vec3(quad[0].0), gc::vec3(quad[1].0));
    assert!((p1 - p0).dot(right_e) > 0.0, "u must still grow to the right");
}

/// Triangle indices stay as they are: a closed mesh keeps its inside/outside. The Gothic index
/// order of a front-facing mesh yields a negative signed volume under the plain numeric cross
/// (the faces point inward by the right-handed rule, i.e. they are clockwise); the reflection
/// makes the same index order counter-clockwise, which is Bevy's front.
#[test]
fn winding_is_kept_by_the_reflection() {
    let v = [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(10.0, 0.0, 0.0),
        Vec3::new(0.0, 10.0, 0.0),
        Vec3::new(0.0, 0.0, 10.0),
    ];
    // Orientation with negative numeric volume (faces inward by the numeric cross).
    let tris = [[0, 1, 2], [0, 3, 1], [0, 2, 3], [1, 3, 2]];
    let vol = |p: &dyn Fn(usize) -> Vec3| -> f32 {
        tris.iter()
            .map(|t| p(t[0]).dot(p(t[1]).cross(p(t[2]))))
            .sum::<f32>()
            / 6.0
    };
    let gothic = vol(&|i| v[i]);
    let engine = vol(&|i| gc::vec3(v[i]));
    assert!(gothic < 0.0 && engine > 0.0, "the reflection flips the signed volume");
    // Outward normals of the engine mesh by the CCW rule point away from the centroid.
    let c = (0..4).map(|i| gc::vec3(v[i])).sum::<Vec3>() / 4.0;
    for t in &tris {
        let (a, b, d) = (gc::vec3(v[t[0]]), gc::vec3(v[t[1]]), gc::vec3(v[t[2]]));
        let n = (b - a).cross(d - a);
        let outward = n.dot((a + b + d) / 3.0 - c) > 0.0;
        assert!(outward, "engine front faces are counter-clockwise, outward");
    }
}

/// A Gothic sword: blade along +Y, cutting edge towards +X, the flat normal `blade x edge`.
/// Held with the edge forward in the Gothic character frame (forward -X), the converted sword's
/// edge points along the converted forward, and the converted frame is a proper right-handed
/// basis only if the flat normal goes through [`gc::axial`].
#[test]
fn sword_edge_faces_forward() {
    let (blade, edge) = (Vec3::Y, Vec3::NEG_X); // edge forward = the character's forward
    let flat = blade.cross(edge);
    let fwd = Vec3::NEG_X;
    assert!(edge.dot(fwd) > 0.999);
    let (b2, e2, f2) = (gc::vec3(blade), gc::vec3(edge), gc::axial(flat));
    assert!(e2.dot(gc::vec3(fwd)) > 0.999, "edge still forward");
    // Proper frame: flat = blade x edge in the engine as well.
    assert!(close(f2, b2.cross(e2)));
    // The naive polar conversion of the flat normal would give the opposite side.
    assert!(!close(gc::vec3(flat), b2.cross(e2)));
}

#[test]
fn frame_matches_mirror_then_translate_and_round_trips() {
    let f = gc::Frame::new(Vec3::new(1000.0, 50.0, -300.0));
    let mut r = Rng(6);
    for _ in 0..100 {
        let p = r.v() * 10.0;
        let want = gc::vec3(p) - gc::vec3(f.origin);
        assert!(close(f.point(p), want));
        assert!(close(f.to_gothic(f.point(p)), p));
        let m = Mat4::from_rotation_translation(r.q(), p);
        let g = f.mat4(m);
        assert!(close(g.w_axis.truncate(), f.point(p)));
        assert!(Mat3::from_mat4(g).abs_diff_eq(Mat3::from_mat4(gc::mat4(m)), 1e-5));
    }
}

#[test]
fn yaw_conventions_agree() {
    for sim in [0.0f32, 0.7, -2.0, 3.0, 1.5707963] {
        let b = gc::sim_yaw_to_engine(sim);
        let f = Quat::from_rotation_y(b) * Vec3::NEG_Z;
        assert!((f.x - sim.cos()).abs() < 1e-5 && (f.z - sim.sin()).abs() < 1e-5);
        assert!(gc::angle_delta(gc::engine_yaw_to_sim(b), sim).abs() < 1e-4);
    }
    for yaw in [0.0f32, 1.0, -2.5] {
        let (x, z) = gc::forward_of_yaw(yaw);
        assert!(gc::angle_delta(gc::yaw_of_dir(x, z), yaw).abs() < 1e-5);
    }
}

#[test]
fn tangent_and_box_conversions() {
    assert_eq!(gc::tangent4([1.0, 2.0, 3.0], 1.0), [-1.0, 2.0, 3.0, -1.0]);
    let (lo, hi) = gc::aabb(Vec3::new(-1.0, 0.0, 0.0), Vec3::new(4.0, 2.0, 3.0));
    assert_eq!((lo, hi), (Vec3::new(-4.0, 0.0, 0.0), Vec3::new(1.0, 2.0, 3.0)));
    assert!(close(gc::position_m(Vec3::new(100.0, 200.0, 300.0)), Vec3::new(-1.0, 2.0, 3.0)));
}
