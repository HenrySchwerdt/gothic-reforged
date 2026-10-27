//! The one place where Gothic 3 data is converted into engine space.
//!
//! **Convention** (derivation and the audit of every call site: `docs/research/COORDINATES.md`):
//!
//! * Gothic 3 (Genome engine) data is **left-handed**, Y up, 1 unit = 1 cm.
//! * The engine (Bevy / glam) is **right-handed**, Y up, -Z forward, metres.
//! * The conversion is the reflection `F = diag(-1, 1, 1)`: **X is mirrored**. Everything that
//!   lives in the Gothic world (placements, meshes, skeletons, clips, collision, nav) goes through
//!   the same `F`, so a camera sees the original picture (the mirrored world is the
//!   handedness-corrected world turned by a half turn about Y).
//! * The cm -> m scale is separate ([`CM_TO_M`]), applied once at the render/physics boundary.
//!
//! Rules that follow from `F` (each has a test in `tests/convention.rs`):
//!
//! | quantity | conversion |
//! |---|---|
//! | position, direction, normal, tangent xyz (polar vectors) | `(x, y, z) -> (-x, y, z)` |
//! | axial vectors (cross products, rotation axes, a blade's "flat" normal) | `(x, y, z) -> (x, -y, -z)` |
//! | unit quaternion `(x, y, z, w)` | `(x, -y, -z, w)` (this is `F R F`) |
//! | 4x4 / affine matrix | `F M F` (conjugation; identical for row- and column-vector storage) |
//! | angle about +Y (a Gothic heading) | negated |
//! | triangle indices | **unchanged**: the reflection reverses the winding by itself |
//! | tangent handedness sign `w` | negated |
//! | uniform scale, UVs, vertex colours | unchanged |
//!
//! Nothing else may mirror, negate X or add half turns to "fix" a Gothic datum. If a datum looks
//! wrong after one pass through this crate the cause is a missing or doubled pass (or a genuinely
//! different engine convention, which is then documented next to the call with its source).
#![forbid(unsafe_code)]

use glam::{Affine3A, Mat3, Mat4, Quat, Vec3, Vec4};

/// Gothic centimetres to engine metres.
pub const CM_TO_M: f32 = 0.01;
/// Engine metres to Gothic centimetres.
pub const M_TO_CM: f32 = 100.0;

/// The reflection `F` as a matrix.
pub const FLIP: Mat3 = Mat3::from_cols(
    Vec3::new(-1.0, 0.0, 0.0),
    Vec3::new(0.0, 1.0, 0.0),
    Vec3::new(0.0, 0.0, 1.0),
);

// ---- polar vectors ------------------------------------------------------------------------------

/// A position, direction, normal or tangent (polar vector) into engine space. Self-inverse.
#[inline]
pub fn vec3(v: Vec3) -> Vec3 {
    Vec3::new(-v.x, v.y, v.z)
}

/// [`vec3`] for the `[f32; 3]` of file parsers and vertex buffers. Self-inverse.
#[inline]
pub fn arr3(v: [f32; 3]) -> [f32; 3] {
    [-v[0], v[1], v[2]]
}

/// A tangent with its handedness sign in `w`: xyz mirrored like any polar vector, the sign flips
/// (the reflection flips the orientation of the tangent frame).
#[inline]
pub fn tangent4(t: [f32; 3], sign: f32) -> [f32; 4] {
    [-t[0], t[1], t[2], -sign]
}

/// A Gothic centimetre position as engine metres.
#[inline]
pub fn position_m(v: Vec3) -> Vec3 {
    vec3(v) * CM_TO_M
}

/// An axis-aligned box (min, max) into engine space.
#[inline]
pub fn aabb(min: Vec3, max: Vec3) -> (Vec3, Vec3) {
    (
        Vec3::new(-max.x, min.y, min.z),
        Vec3::new(-min.x, max.y, max.z),
    )
}

/// An axial vector (the cross product of two polar vectors, a rotation axis, the flat normal of a
/// blade defined as `blade x edge`) into engine space. Self-inverse.
///
/// A reflection maps `a x b` to `-F (a x b)`, not to `F (a x b)`: a basis `(u, v, u x v)`
/// converted as `(F u, F v, -F (u x v))` stays right-handed, which is what a frame needs.
#[inline]
pub fn axial(v: Vec3) -> Vec3 {
    Vec3::new(v.x, -v.y, -v.z)
}

// ---- rotations -----------------------------------------------------------------------------------

/// A rotation (unit quaternion) into engine space: `F R F`. Self-inverse.
#[inline]
pub fn quat(q: Quat) -> Quat {
    Quat::from_xyzw(q.x, -q.y, -q.z, q.w)
}

/// [`quat`] for file components `(x, y, z, w)`; renormalised (files carry rounded values).
#[inline]
pub fn quat_xyzw(q: [f32; 4]) -> Quat {
    let r = quat(Quat::from_xyzw(q[0], q[1], q[2], q[3]));
    if r.length_squared() > 1e-12 {
        r.normalize()
    } else {
        Quat::IDENTITY
    }
}

/// A rotation angle about +Y (a Gothic heading) as the angle of the engine rotation about +Y that
/// does the same thing in the mirrored world. Self-inverse.
#[inline]
pub fn heading(angle: f32) -> f32 {
    -angle
}

// ---- triangle winding ----------------------------------------------------------------------------

/// Reverses the winding of every triangle of an index list (swaps the 2nd and 3rd index).
///
/// Winding is **not** uniform across the Gothic formats. Measured on the installed data (the sign
/// of `(b - a) x (c - a)` against the stored vertex normals, mean over all triangles): static
/// `.xcmsh` meshes score -1.000 (the numeric cross product points *against* the normal), skinned
/// `.xact` actor meshes score +0.997. The reflection `F` flips the sign of the numeric cross
/// product, so after it
///
/// * **static meshes keep their indices** (they become counter-clockwise = Bevy front faces),
/// * **actor meshes need this swap** (they would otherwise be back faces).
///
/// Materials with `cull_mode: None` hide the difference; anything that culls, uses a depth
/// pre-pass normal or relies on `front_facing` does not.
pub fn flip_winding(indices: &mut [u32]) {
    for t in indices.chunks_exact_mut(3) {
        t.swap(1, 2);
    }
}

// ---- matrices ------------------------------------------------------------------------------------

fn flip4() -> Mat4 {
    Mat4::from_cols(Vec4::new(-1.0, 0.0, 0.0, 0.0), Vec4::Y, Vec4::Z, Vec4::W)
}

/// A transform into engine space: `F M F`.
#[inline]
pub fn mat4(m: Mat4) -> Mat4 {
    flip4() * m * flip4()
}

/// [`mat4`] for affine transforms.
#[inline]
pub fn affine(m: Affine3A) -> Affine3A {
    let f = Affine3A::from_scale(Vec3::new(-1.0, 1.0, 1.0));
    f * m * f
}

/// A Gothic placement (translation, rotation, scale) as the engine-space parts.
#[inline]
pub fn trs(t: Vec3, r: Quat, s: Vec3) -> (Vec3, Quat, Vec3) {
    (vec3(t), quat(r), s)
}

// ---- origin-relative world frame ----------------------------------------------------------------

/// The world frame of a viewer: a Gothic-space origin (cm) subtracted so f32 stays precise, with
/// `F` applied. The result unit is centimetres (apply [`CM_TO_M`] at the boundary).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: Vec3,
}

impl Frame {
    pub const fn new(origin: Vec3) -> Self {
        Self { origin }
    }

    /// Gothic world point -> origin-relative engine point (cm).
    #[inline]
    pub fn point(&self, p: Vec3) -> Vec3 {
        Vec3::new(self.origin.x - p.x, p.y - self.origin.y, p.z - self.origin.z)
    }

    /// Origin-relative engine point (cm) -> Gothic world point.
    #[inline]
    pub fn to_gothic(&self, p: Vec3) -> Vec3 {
        Vec3::new(self.origin.x - p.x, p.y + self.origin.y, p.z + self.origin.z)
    }

    /// A Gothic placement matrix as an origin-relative engine matrix.
    pub fn mat4(&self, m: Mat4) -> Mat4 {
        let mut out = mat4(m);
        out.w_axis = self.point(m.w_axis.truncate()).extend(1.0);
        out
    }
}

// ---- facing / yaw ---------------------------------------------------------------------------------

/// Engine yaw (rotation about +Y; yaw 0 looks along -Z, positive turns left) of the horizontal
/// direction `(dx, dz)`.
#[inline]
pub fn yaw_of_dir(dx: f32, dz: f32) -> f32 {
    (-dx).atan2(-dz)
}

/// The horizontal forward direction `(x, z)` of an engine yaw.
#[inline]
pub fn forward_of_yaw(yaw: f32) -> (f32, f32) {
    (-yaw.sin(), -yaw.cos())
}

/// The horizontal forward unit vector of an engine yaw.
#[inline]
pub fn forward3(yaw: f32) -> Vec3 {
    let (x, z) = forward_of_yaw(yaw);
    Vec3::new(x, 0.0, z)
}

/// Engine yaw of a simulation yaw. The simulation works in the mirrored world and measures
/// `atan2(dz, dx)` (yaw 0 faces +X); both describe the same facing.
#[inline]
pub fn sim_yaw_to_engine(sim: f32) -> f32 {
    yaw_of_dir(sim.cos(), sim.sin())
}

/// Simulation yaw of an engine yaw. Inverse of [`sim_yaw_to_engine`].
#[inline]
pub fn engine_yaw_to_sim(yaw: f32) -> f32 {
    let (x, z) = forward_of_yaw(yaw);
    z.atan2(x)
}

/// Shortest signed angle from `from` to `to`, in `[-PI, PI]`.
#[inline]
pub fn angle_delta(from: f32, to: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let mut d = (to - from) % TAU;
    if d > PI {
        d -= TAU;
    } else if d < -PI {
        d += TAU;
    }
    d
}
