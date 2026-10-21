//! Billboard geometry of the particles (`eCParticle_PS::UpdateVertices`, Engine:0x10153630):
//! facing modes, spin, sub-image UVs and the vertex colour. Pure functions, no rendering.
use crate::params::{EmitterParams, Facing};
use crate::sim::{Emitter, Particle};
use bevy::math::{Mat3, Quat, Vec3};

/// Camera basis in the same space as the emitters.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub pos: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    /// Viewing direction (away from the eye).
    pub forward: Vec3,
    /// Tangents of the half field of view (horizontal, vertical); particles outside the
    /// frustum (with their size as margin) are not built. `None` keeps everything.
    pub tan_half: Option<(f32, f32)>,
}

impl View {
    pub fn new(pos: Vec3, rotation: Quat) -> Self {
        Self {
            pos,
            right: rotation * Vec3::X,
            up: rotation * Vec3::Y,
            forward: rotation * Vec3::NEG_Z,
            tan_half: None,
        }
    }

    /// True when a sphere (centre, radius in cm) can touch the view frustum.
    pub fn sees(&self, centre: Vec3, radius: f32) -> bool {
        let to = centre - self.pos;
        let depth = to.dot(self.forward);
        if depth + radius < 0.0 {
            return false;
        }
        let Some((tx, ty)) = self.tan_half else {
            return true;
        };
        let reach = depth.max(0.0) + radius;
        to.dot(self.right).abs() <= reach * tx + radius
            && to.dot(self.up).abs() <= reach * ty + radius
    }
}

/// A camera-sorted, ready-to-draw particle quad.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    /// Corners: top-left, top-right, bottom-right, bottom-left (cm).
    pub corners: [Vec3; 4],
    /// UV rectangle of the first sub-image (`u0 v0 u1 v1`).
    pub uv_a: [f32; 4],
    /// UV rectangle of the second sub-image and its blend weight (0 = only the first).
    pub uv_b: [f32; 4],
    pub blend: f32,
    pub color: [f32; 4],
}

/// UV rectangle `[u0, v0, u1, v1]` of sub-image `index` (column-major unless `row_major`,
/// exactly as the vertex fill of `UpdateVertices`).
pub fn frame_rect(p: &EmitterParams, index: i32) -> [f32; 4] {
    let cols = p.u_subdivisions + 1;
    let rows = p.v_subdivisions + 1;
    if cols == 1 && rows == 1 {
        return [0.0, 0.0, 1.0, 1.0];
    }
    let index = index.max(0);
    let (col, row) = if p.row_major {
        (index % cols, index / cols)
    } else {
        (index / rows, index % rows)
    };
    let (du, dv) = (1.0 / cols as f32, 1.0 / rows as f32);
    [
        du * col as f32,
        dv * row as f32,
        du * (col + 1) as f32,
        dv * (row + 1) as f32,
    ]
}

/// The quad's axes for a facing mode: `(right, up, normal)`, all unit length.
fn axes(p: &EmitterParams, q: &Particle, view: &View, rot: Mat3) -> (Vec3, Vec3, Vec3) {
    let motion = q.vel * q.vel_scale;
    let normal_from = |n: Vec3, along: Option<Vec3>| -> (Vec3, Vec3, Vec3) {
        let n = n.normalize_or_zero();
        let n = if n == Vec3::ZERO { Vec3::Y } else { n };
        let up = match along {
            Some(m) => {
                let proj = m - n * m.dot(n);
                if proj.length_squared() > 1e-8 {
                    proj.normalize()
                } else {
                    n.any_orthonormal_vector()
                }
            }
            None => n.any_orthonormal_vector(),
        };
        let right = up.cross(n).normalize_or_zero();
        (right, up, n)
    };
    match p.facing {
        Facing::Camera => (view.right, view.up, -view.forward),
        Facing::AlongMovementFacingCamera => {
            // Up follows the movement projected onto the screen plane; the quad faces the eye.
            let n = -view.forward;
            let proj = motion - n * motion.dot(n);
            if proj.length_squared() < 1e-8 {
                return (view.right, view.up, n);
            }
            let up = proj.normalize();
            (up.cross(n).normalize_or_zero(), up, n)
        }
        Facing::SpecifiedNormal => {
            let n = if p.rotate_with_entity {
                rot * Vec3::from_array(p.projection_normal)
            } else {
                Vec3::from_array(p.projection_normal)
            };
            normal_from(n, None)
        }
        Facing::AlongMovementFacingNormal => {
            let n = if p.rotate_with_entity {
                rot * Vec3::from_array(p.projection_normal)
            } else {
                Vec3::from_array(p.projection_normal)
            };
            normal_from(n, Some(motion))
        }
        Facing::PerpendicularToMovement => {
            if motion.length_squared() < 1e-8 {
                (view.right, view.up, -view.forward)
            } else {
                normal_from(motion, None)
            }
        }
    }
}

/// Build the quad of one particle. `to_world` positions relative emitters.
pub fn particle_quad(
    e: &Emitter,
    q: &Particle,
    view: &View,
    relative: Option<&bevy::math::Mat4>,
) -> Quad {
    let p = &*e.params;
    let rot = Mat3::from_mat4(e.xf);
    let (mut right, mut up, mut n) = axes(p, q, view, rot);
    // Spin about the quad's own axes (x: right, y: up, z: normal), `UpdateVertices`.
    if q.rot.x != 0.0 {
        let r = Quat::from_axis_angle(right, q.rot.x);
        up = r * up;
        n = r * n;
    }
    if q.rot.y != 0.0 {
        let r = Quat::from_axis_angle(up, q.rot.y);
        right = r * right;
        n = r * n;
    }
    if q.rot.z != 0.0 {
        let r = Quat::from_axis_angle(n, q.rot.z);
        right = r * right;
        up = r * up;
    }
    let size = q.size();
    let pivot = Vec3::from_array(p.spin_pivot);
    // Corner offsets: (pivot +- 1) * size on the two quad axes.
    let (hx, hy) = (size.x, size.y);
    let off = |sx: f32, sy: f32| right * ((pivot.x + sx) * hx) + up * ((pivot.y + sy) * hy);
    let center = match relative {
        Some(m) => m.transform_point3(q.pos),
        None => q.pos,
    };
    let corners = [
        center + off(-1.0, 1.0),
        center + off(1.0, 1.0),
        center + off(1.0, -1.0),
        center + off(-1.0, -1.0),
    ];
    let frames = p.frame_count();
    let f = q.frame.max(0.0);
    let a = f.floor() as i32;
    let (uv_a, uv_b, blend) = if frames <= 1 {
        ([0.0, 0.0, 1.0, 1.0], [0.0, 0.0, 1.0, 1.0], 0.0)
    } else {
        let a = a.min(frames - 1);
        let b = (f.ceil() as i32).min(frames - 1);
        let w = if p.blend_subdivisions {
            f - f.floor()
        } else {
            0.0
        };
        (frame_rect(p, a), frame_rect(p, b), w)
    };
    let mut color = [0.0; 4];
    for (c, o) in color.iter_mut().enumerate() {
        *o = (q.color[c] * q.color_factor[c]).clamp(0.0, 1.0);
    }
    Quad {
        corners,
        uv_a,
        uv_b,
        blend,
        color,
    }
}

/// Append the quads of an emitter, back to front (`DepthSortParticles`).
pub fn emitter_quads(e: &Emitter, view: &View, out: &mut Vec<Quad>) {
    let relative = e.params.relative.then_some(e.xf);
    // Sort the cheap (depth, index) keys; the quads are built once, in drawing order.
    let mut keys: Vec<(f32, u32)> = e
        .particles
        .iter()
        .enumerate()
        .filter(|(_, q)| !q.dead)
        // Fully faded particles draw nothing.
        .filter(|(_, q)| q.color[3] * q.color_factor[3] > 0.002)
        .filter_map(|(i, q)| {
            let at = match &relative {
                Some(m) => m.transform_point3(q.pos),
                None => q.pos,
            };
            let size = q.size();
            // Spin can turn the quad corner outward: use the diagonal as the radius.
            let radius = size.x.max(size.y) * std::f32::consts::SQRT_2;
            view.sees(at, radius)
                .then(|| ((at - view.pos).dot(view.forward), i as u32))
        })
        .collect();
    keys.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
    out.reserve(keys.len());
    for (_, i) in keys {
        out.push(particle_quad(
            e,
            &e.particles[i as usize],
            view,
            relative.as_ref(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::Range1;
    use crate::sim::Emitter;
    use bevy::math::Mat4;
    use std::sync::Arc;

    fn grid() -> EmitterParams {
        EmitterParams {
            u_subdivisions: 5,
            v_subdivisions: 1,
            ..EmitterParams::default()
        }
    }

    #[test]
    fn frames_run_down_the_columns_unless_row_major() {
        let mut p = grid();
        // 6 columns, 2 rows: frame 1 is the second row of the first column.
        assert_eq!(frame_rect(&p, 0), [0.0, 0.0, 1.0 / 6.0, 0.5]);
        assert_eq!(frame_rect(&p, 1), [0.0, 0.5, 1.0 / 6.0, 1.0]);
        assert_eq!(frame_rect(&p, 2)[0], 1.0 / 6.0);
        p.row_major = true;
        assert_eq!(frame_rect(&p, 1)[0], 1.0 / 6.0);
        assert_eq!(frame_rect(&p, 6)[1], 0.5);
    }

    #[test]
    fn camera_facing_quad_spans_twice_the_size() {
        let p = EmitterParams {
            lifetime: Range1::new(10.0, 10.0),
            start_size: crate::params::Range3::splat(20.0),
            ..EmitterParams::default()
        };
        let mut e = Emitter::new(Arc::new(p), 1);
        e.set_transform(Mat4::IDENTITY);
        e.update(0.2);
        let view = View::new(Vec3::new(0.0, 0.0, 500.0), Quat::IDENTITY);
        let mut quads = Vec::new();
        emitter_quads(&e, &view, &mut quads);
        assert!(!quads.is_empty());
        let q = quads[0];
        assert!(((q.corners[1] - q.corners[0]).length() - 40.0).abs() < 1e-3);
        assert!(((q.corners[0] - q.corners[3]).length() - 40.0).abs() < 1e-3);
    }

    #[test]
    fn the_frustum_test_drops_particles_behind_and_far_to_the_side() {
        let mut v = View::new(Vec3::ZERO, Quat::IDENTITY); // looks down -z
        v.tan_half = Some((1.0, 0.6));
        assert!(v.sees(Vec3::new(0.0, 0.0, -500.0), 10.0));
        assert!(!v.sees(Vec3::new(0.0, 0.0, 500.0), 10.0), "behind the eye");
        assert!(
            !v.sees(Vec3::new(2000.0, 0.0, -500.0), 10.0),
            "far to the side"
        );
        assert!(
            v.sees(Vec3::new(2000.0, 0.0, -500.0), 1600.0),
            "a huge quad reaches in"
        );
        v.tan_half = None;
        assert!(v.sees(Vec3::new(2000.0, 0.0, -500.0), 10.0));
    }

    #[test]
    fn colour_multiplies_curve_and_fade() {
        let p = EmitterParams::default();
        let mut e = Emitter::new(Arc::new(p), 1);
        e.set_transform(Mat4::IDENTITY);
        e.update(0.5);
        let mut q = e.particles[0].clone();
        q.color = [1.0, 0.5, 0.0, 1.0];
        q.color_factor = [0.5, 0.5, 1.0, 0.5];
        let quad = particle_quad(&e, &q, &View::new(Vec3::ZERO, Quat::IDENTITY), None);
        assert_eq!(quad.color, [0.5, 0.25, 0.0, 0.5]);
    }

    #[test]
    fn blend_weight_is_the_fractional_frame() {
        let mut p = grid();
        p.blend_subdivisions = true;
        let mut e = Emitter::new(Arc::new(p), 1);
        e.set_transform(Mat4::IDENTITY);
        e.update(0.5);
        let mut q = e.particles[0].clone();
        q.frame = 2.25;
        let quad = particle_quad(&e, &q, &View::new(Vec3::ZERO, Quat::IDENTITY), None);
        assert!((quad.blend - 0.25).abs() < 1e-6);
        assert_eq!(quad.uv_a, frame_rect(&e.params, 2));
        assert_eq!(quad.uv_b, frame_rect(&e.params, 3));
    }
}
