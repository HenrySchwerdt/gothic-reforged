//! Weapon grip frames derived from the weapon mesh (no hand-tuned rotations).
//!
//! A one-handed blade is analysed in its native (left-handed) mesh space:
//! * blade axis `b`: the long axis of the mesh, pointing from the handle to the tip;
//! * guard: the slice along `b` with the widest cross-section; handle = everything behind it;
//! * grip centre: middle of the handle (between pommel and guard), on the handle's axis (principal axis of the handle
//!   vertices, used to report how well it agrees with `b`);
//! * edge direction `e`: widest direction of the blade cross-section perpendicular to `b`; flat normal `n`: the
//!   thinnest (`n = b x e`).
//!
//! The prop frame of `vr-physics` equals the OpenXR grip frame of the holding hand (see `retarget.rs::grip_anatomy`):
//! -Z = the fist axis little finger -> thumb = the direction the blade leaves the fist, -Y = along the fingers, and
//! +-X = the palm normal. [`GripFrame::visual`] therefore maps `b -> -Z`, the edge `e -> -Y` (edge forward, along the
//! fingers) and the flat normal `n -> -X` (flat side against the palm), and moves the grip centre to the origin.
use bevy::prelude::*;
use gothic_cook::mesh::MeshData;

#[derive(Clone, Copy, Debug)]
pub struct GripFrame {
    /// Blade direction (unit), native mesh space.
    pub blade: Vec3,
    /// Edge direction (unit), blade cross-section's widest axis.
    pub edge: Vec3,
    /// Flat normal (unit) = blade x edge.
    pub flat: Vec3,
    /// Middle of the handle, native mesh units (cm).
    pub center: Vec3,
    /// Principal axis of the handle vertices (unit, same sign as `blade`).
    pub handle_axis: Vec3,
    pub tip: f32,
    pub pommel: f32,
    pub guard: f32,
}

fn principal_axis(points: &[Vec3]) -> Vec3 {
    let n = points.len().max(1) as f32;
    let mean = points.iter().copied().sum::<Vec3>() / n;
    let mut c = Mat3::ZERO;
    for p in points {
        let d = *p - mean;
        c += Mat3::from_cols(d * d.x, d * d.y, d * d.z);
    }
    let mut v = Vec3::new(0.3, 0.5, 1.0).normalize();
    for _ in 0..40 {
        v = (c * v).normalize_or_zero();
        if v == Vec3::ZERO {
            return Vec3::Z;
        }
    }
    v
}

/// Analyses the weapon mesh (blade assumed to be the long axis, tip at the end with the larger extent from the origin).
pub fn analyse(md: &MeshData) -> Option<GripFrame> {
    let mut pts: Vec<Vec3> = Vec::new();
    for s in md.submeshes() {
        for v in md.submesh_vertices(s) {
            pts.push(Vec3::from(v.pos));
        }
    }
    if pts.len() < 8 {
        return None;
    }
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in &pts {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let ext = hi - lo;
    let long = if ext.x >= ext.y && ext.x >= ext.z {
        Vec3::X
    } else if ext.y >= ext.z {
        Vec3::Y
    } else {
        Vec3::Z
    };
    // Tip = the end of the long axis that is farther from the mesh origin (handle end is near the origin).
    let (a_lo, a_hi) = (lo.dot(long), hi.dot(long));
    let sign = if a_hi.abs() >= a_lo.abs() { 1.0 } else { -1.0 };
    let blade = long * sign;
    let along = |p: Vec3| p.dot(blade);
    let (pommel, tip) = if sign > 0.0 {
        (a_lo, a_hi)
    } else {
        (-a_hi, -a_lo)
    };
    // Perpendicular basis for cross-section measurements.
    let (u, w) = {
        let a = if blade.x.abs() < 0.9 {
            Vec3::X
        } else {
            Vec3::Y
        };
        let u = blade.cross(a).normalize();
        (u, blade.cross(u))
    };
    // Guard: slice (0.5 cm) with the widest cross-section within the first 60 % of the length.
    let len = tip - pommel;
    let slice = 0.5f32;
    let nslices = ((len / slice).ceil() as usize).max(1);
    let mut width = vec![0.0f32; nslices];
    let mut bounds = vec![(f32::MAX, f32::MIN, f32::MAX, f32::MIN); nslices];
    for p in &pts {
        let i = (((along(*p) - pommel) / slice) as usize).min(nslices - 1);
        let (pu, pw) = (p.dot(u), p.dot(w));
        let b = &mut bounds[i];
        b.0 = b.0.min(pu);
        b.1 = b.1.max(pu);
        b.2 = b.2.min(pw);
        b.3 = b.3.max(pw);
    }
    let mut guard_i = 0;
    for (i, b) in bounds.iter().enumerate() {
        if b.0 <= b.1 {
            width[i] = (b.1 - b.0).max(b.3 - b.2);
        }
        if i < (nslices as f32 * 0.6) as usize && width[i] > width[guard_i] {
            guard_i = i;
        }
    }
    let guard = pommel + guard_i as f32 * slice;
    // Handle vertices: behind the guard, away from the pommel ball and the guard itself.
    let handle: Vec<Vec3> = pts
        .iter()
        .copied()
        .filter(|p| along(*p) < guard - 1.0 && along(*p) > pommel + 1.5)
        .collect();
    let handle = if handle.len() >= 6 {
        handle
    } else {
        pts.iter().copied().filter(|p| along(*p) < guard).collect()
    };
    let mut handle_axis = principal_axis(&handle);
    if handle_axis.dot(blade) < 0.0 {
        handle_axis = -handle_axis;
    }
    // Centre of the handle: halfway along the blade axis between pommel and guard, on the handle's mean axis.
    let mid_along = (pommel + guard) * 0.5;
    let mean = handle.iter().copied().sum::<Vec3>() / handle.len().max(1) as f32;
    let center = mean + blade * (mid_along - along(mean));
    // Blade cross-section: vertices in the central blade zone.
    let blade_pts: Vec<(f32, f32)> = pts
        .iter()
        .filter(|p| {
            along(**p) > guard + 0.2 * (tip - guard) && along(**p) < tip - 0.15 * (tip - guard)
        })
        .map(|p| (p.dot(u), p.dot(w)))
        .collect();
    let n = blade_pts.len().max(1) as f32;
    let (mu, mw) = blade_pts
        .iter()
        .fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
    let (mu, mw) = (mu / n, mw / n);
    let (mut suu, mut sww, mut suw) = (0.0f32, 0.0f32, 0.0f32);
    for (pu, pw) in &blade_pts {
        suu += (pu - mu) * (pu - mu);
        sww += (pw - mw) * (pw - mw);
        suw += (pu - mu) * (pw - mw);
    }
    // Principal axis of the 2x2 covariance = widest direction = edge line.
    let ang = 0.5 * (2.0 * suw).atan2(suu - sww);
    let edge = (u * ang.cos() + w * ang.sin()).normalize();
    let flat = blade.cross(edge).normalize();
    Some(GripFrame {
        blade,
        edge,
        flat,
        center,
        handle_axis,
        tip,
        pommel,
        guard,
    })
}

impl GripFrame {
    /// Mesh-space frame of the gothic data mirrored across X (the engine-space convention of skeletons and clips).
    pub fn mirrored(&self) -> GripFrame {
        let m = |v: Vec3| Vec3::new(-v.x, v.y, v.z);
        GripFrame {
            blade: m(self.blade),
            edge: m(self.edge),
            flat: -m(self.flat),
            center: m(self.center),
            handle_axis: m(self.handle_axis),
            ..*self
        }
    }

    /// Prop-frame visual transform for a mesh spawned mirrored (`Ardea::spawn_static_mirrored`):
    /// the grip centre sits at the prop origin, the blade along -Z, the edge along -Y, the flat normal along -X.
    pub fn visual(&self, unit: f32) -> Transform {
        let mir = self.mirrored();
        let from = Mat3::from_cols(mir.blade, mir.edge, mir.flat);
        // Target: blade -> -Z, edge -> -Y, flat -> (-Z) x (-Y) = -X.
        let to = Mat3::from_cols(Vec3::NEG_Z, Vec3::NEG_Y, Vec3::NEG_X);
        let rot = Quat::from_mat3(&(to * from.transpose())).normalize();
        Transform {
            translation: -(rot * (mir.center * unit)),
            rotation: rot,
            scale: Vec3::splat(unit),
        }
    }
}

/// Authored per-weapon grip data (`grips.json`, embedded). Our own values; the automatic analysis only seeds defaults.
#[derive(serde::Deserialize, Clone, Debug, Default)]
pub struct GripEntry {
    /// Centre of the fist on the handle, native mesh space (cm).
    pub center: Option<[f32; 3]>,
    /// Blade direction, native mesh space.
    pub blade: Option<[f32; 3]>,
    /// Cutting-edge direction, native mesh space.
    pub edge: Option<[f32; 3]>,
    /// Closed-grip finger flexion.
    pub curl: Option<GripCurl>,
}

/// Finger flexion of the closed grip (0 open .. 1 closed).
#[derive(serde::Deserialize, Clone, Copy, Debug)]
pub struct GripCurl {
    /// Middle, ring, little finger (the thumb follows at 0.7).
    pub grip: f32,
    /// Index finger.
    pub trigger: f32,
}

fn db() -> &'static std::collections::HashMap<String, GripEntry> {
    static DB: std::sync::OnceLock<std::collections::HashMap<String, GripEntry>> = std::sync::OnceLock::new();
    DB.get_or_init(|| {
        let raw: std::collections::HashMap<String, serde_json::Value> =
            serde_json::from_str(include_str!("../grips.json")).unwrap_or_default();
        raw.into_iter()
            .filter(|(k, _)| !k.starts_with('_'))
            .filter_map(|(k, v)| serde_json::from_value(v).ok().map(|e| (k, e)))
            .collect()
    })
}

/// Authored entry of a weapon mesh.
pub fn entry(mesh: &str) -> Option<&'static GripEntry> {
    db().get(mesh)
}

/// Closed-grip finger flexion `[grip, trigger]` of a weapon (fully closed when nothing is authored).
pub fn curl(mesh: &str) -> [f32; 2] {
    entry(mesh)
        .and_then(|e| e.curl)
        .map_or([1.0, 1.0], |c| [c.grip, c.trigger])
}

/// Grip frame of a weapon mesh: the analysis, with the authored values of `grips.json` on top.
pub fn frame_for(mesh: &str, md: &MeshData) -> Option<GripFrame> {
    let mut g = analyse(md)?;
    if let Some(e) = entry(mesh) {
        if let Some(b) = e.blade {
            g.blade = Vec3::from(b).normalize_or_zero();
        }
        if let Some(d) = e.edge {
            g.edge = Vec3::from(d).normalize_or_zero();
        }
        g.flat = g.blade.cross(g.edge).normalize_or_zero();
        if let Some(c) = e.center {
            g.center = Vec3::from(c);
        }
    }
    Some(g)
}
