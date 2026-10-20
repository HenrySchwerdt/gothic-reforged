//! Collision world, Gothic-3 character movement and small physics helpers.
//!
//! * [`CollisionWorld`]: streamable triangle chunks behind a uniform XZ grid (ray, capsule
//!   and ground queries).
//! * [`controller`]: the character controller, a re-implementation of the game's own
//!   `gCCharacterMovement_PS` rules (see `docs/research/PHYSICS_MOVEMENT.md`), parameterised
//!   by a [`MovementProfile`] read from the template data.
//! * [`crowd`]: character-versus-character soft collision. [`ragdoll`]: verlet ragdoll.
//!
//! Units are centimetres (Gothic 3), Y up.
pub mod controller;
pub mod crowd;
pub mod movement;
pub mod profile;
pub mod ragdoll;
pub mod relax;

pub use controller::{CharacterController, Gait, Landing, MoveCommand, MoveMode, StepReport};
pub use movement::{ModeChange, MovementMode};
pub use profile::{ControllerConfig, MovementProfile, fall_damage};

use glam::Vec3;
use std::collections::HashMap;

pub type Tri = [Vec3; 3];

const GRID: f32 = 400.0;

#[derive(Default)]
struct Chunk {
    tris: Vec<Tri>,
    cells: Vec<(i32, i32)>,
}

/// Chunked triangle soup with a uniform XZ grid for neighbourhood queries.
#[derive(Default)]
pub struct CollisionWorld {
    chunks: HashMap<u64, Chunk>,
    grid: HashMap<(i32, i32), Vec<(u64, u32)>>,
}

fn cell(v: f32) -> i32 {
    (v / GRID).floor() as i32
}

impl CollisionWorld {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn triangle_count(&self) -> usize {
        self.chunks.values().map(|c| c.tris.len()).sum()
    }
    pub fn has_chunk(&self, id: u64) -> bool {
        self.chunks.contains_key(&id)
    }

    /// Insert (or replace) a chunk. Degenerate/non-finite triangles are dropped.
    pub fn insert_chunk(&mut self, id: u64, tris: Vec<Tri>) {
        self.remove_chunk(id);
        let tris: Vec<Tri> = tris
            .into_iter()
            .filter(|t| {
                t.iter().all(|p| p.is_finite())
                    && (t[1] - t[0]).cross(t[2] - t[0]).length_squared() > 1e-6
            })
            .collect();
        let mut cells = Vec::new();
        for (i, t) in tris.iter().enumerate() {
            let lo = t[0].min(t[1]).min(t[2]);
            let hi = t[0].max(t[1]).max(t[2]);
            // A pathological triangle must not flood the grid.
            if (hi.x - lo.x) > GRID * 64.0 || (hi.z - lo.z) > GRID * 64.0 {
                continue;
            }
            for cx in cell(lo.x)..=cell(hi.x) {
                for cz in cell(lo.z)..=cell(hi.z) {
                    self.grid.entry((cx, cz)).or_default().push((id, i as u32));
                    cells.push((cx, cz));
                }
            }
        }
        cells.sort_unstable();
        cells.dedup();
        self.chunks.insert(id, Chunk { tris, cells });
    }

    pub fn remove_chunk(&mut self, id: u64) {
        let Some(chunk) = self.chunks.remove(&id) else {
            return;
        };
        for c in chunk.cells {
            if let Some(list) = self.grid.get_mut(&c) {
                list.retain(|(cid, _)| *cid != id);
                if list.is_empty() {
                    self.grid.remove(&c);
                }
            }
        }
    }

    /// Triangles whose bounding box overlaps the box `lo..hi`.
    pub fn query(&self, lo: Vec3, hi: Vec3, out: &mut Vec<Tri>) {
        out.clear();
        let mut seen: Vec<(u64, u32)> = Vec::new();
        for cx in cell(lo.x)..=cell(hi.x) {
            for cz in cell(lo.z)..=cell(hi.z) {
                if let Some(list) = self.grid.get(&(cx, cz)) {
                    seen.extend_from_slice(list);
                }
            }
        }
        seen.sort_unstable();
        seen.dedup();
        for (c, i) in seen {
            let t = self.chunks[&c].tris[i as usize];
            let tlo = t[0].min(t[1]).min(t[2]);
            let thi = t[0].max(t[1]).max(t[2]);
            if tlo.cmple(hi).all() && thi.cmpge(lo).all() {
                out.push(t);
            }
        }
    }

    /// Nearest hit distance along a normalised direction (double-sided).
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<f32> {
        let end = origin + dir * max;
        let mut tris = Vec::new();
        self.query(origin.min(end) - 1.0, origin.max(end) + 1.0, &mut tris);
        tris.iter()
            .filter_map(|t| ray_triangle(origin, dir, t))
            .filter(|d| *d <= max)
            .min_by(f32::total_cmp)
    }

    /// Does the capsule (segment `a`-`b`, radius `r`) touch any triangle?
    pub fn capsule_blocked(&self, a: Vec3, b: Vec3, r: f32) -> bool {
        let mut tris = Vec::new();
        self.query(
            a.min(b) - Vec3::splat(r + 1.0),
            a.max(b) + Vec3::splat(r + 1.0),
            &mut tris,
        );
        tris.iter().any(|t| {
            let (sp, tp, crossing) = segment_triangle(a, b, t);
            crossing || sp.distance_squared(tp) < r * r
        })
    }

    /// Like [`Self::capsule_blocked`] for two radii at once (`small <= big`) with one query:
    /// returns (blocked at `small`, blocked at `big`).
    pub fn capsule_blocked2(&self, a: Vec3, b: Vec3, small: f32, big: f32) -> (bool, bool) {
        let mut tris = Vec::new();
        self.query(
            a.min(b) - Vec3::splat(big + 1.0),
            a.max(b) + Vec3::splat(big + 1.0),
            &mut tris,
        );
        let mut nearest = f32::MAX;
        for t in &tris {
            let (sp, tp, crossing) = segment_triangle(a, b, t);
            nearest = nearest.min(if crossing { 0.0 } else { sp.distance(tp) });
        }
        (nearest < small, nearest < big)
    }

    /// Nearest hit along a normalised direction with the (up-facing) triangle normal.
    pub fn raycast_hit(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<(f32, Vec3)> {
        let end = origin + dir * max;
        let mut tris = Vec::new();
        self.query(origin.min(end) - 1.0, origin.max(end) + 1.0, &mut tris);
        let mut best: Option<(f32, Vec3)> = None;
        for t in &tris {
            if let Some(d) = ray_triangle(origin, dir, t)
                && d <= max
                && best.is_none_or(|b| d < b.0)
            {
                let mut n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
                if n.dot(dir) > 0.0 {
                    n = -n;
                }
                best = Some((d, n));
            }
        }
        best
    }

    /// Highest surface below `from` within `depth` (vertical ray).
    pub fn ground_below(&self, from: Vec3, depth: f32) -> Option<f32> {
        self.raycast(from, Vec3::NEG_Y, depth).map(|d| from.y - d)
    }
}

pub(crate) fn ray_triangle(o: Vec3, d: Vec3, t: &Tri) -> Option<f32> {
    let e1 = t[1] - t[0];
    let e2 = t[2] - t[0];
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - t[0];
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let dist = e2.dot(q) * inv;
    (dist >= 0.0).then_some(dist)
}

pub fn closest_point_on_triangle(p: Vec3, t: &Tri) -> Vec3 {
    let (a, b, c) = (t[0], t[1], t[2]);
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return b + (c - b) * w;
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// Closest points between segments p1-q1 and p2-q2 (point on first, point on second).
fn segment_segment(p1: Vec3, q1: Vec3, p2: Vec3, q2: Vec3) -> (Vec3, Vec3) {
    let d1 = q1 - p1;
    let d2 = q2 - p2;
    let r = p1 - p2;
    let a = d1.dot(d1);
    let e = d2.dot(d2);
    let f = d2.dot(r);
    let (s, t);
    if a <= 1e-9 && e <= 1e-9 {
        return (p1, p2);
    }
    if a <= 1e-9 {
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = d1.dot(r);
        if e <= 1e-9 {
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = d1.dot(d2);
            let denom = a * e - b * b;
            let mut s0 = if denom > 1e-9 {
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
    (p1 + d1 * s, p2 + d2 * t)
}

/// Closest pair (point on segment, point on triangle) and whether the segment crosses the triangle.
pub(crate) fn segment_triangle(a: Vec3, b: Vec3, t: &Tri) -> (Vec3, Vec3, bool) {
    let d = b - a;
    let len = d.length();
    if len > 1e-6
        && let Some(dist) = ray_triangle(a, d / len, t)
        && dist <= len
    {
        let p = a + d / len * dist;
        return (p, p, true);
    }
    let mut best = (a, closest_point_on_triangle(a, t));
    let mut best_d = best.0.distance_squared(best.1);
    let cb = closest_point_on_triangle(b, t);
    if b.distance_squared(cb) < best_d {
        best = (b, cb);
        best_d = b.distance_squared(cb);
    }
    for i in 0..3 {
        let (s, e) = segment_segment(a, b, t[i], t[(i + 1) % 3]);
        let dd = s.distance_squared(e);
        if dd < best_d {
            best = (s, e);
            best_d = dd;
        }
    }
    (best.0, best.1, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(y: f32, half: f32) -> Vec<Tri> {
        let a = Vec3::new(-half, y, -half);
        let b = Vec3::new(half, y, -half);
        let c = Vec3::new(half, y, half);
        let d = Vec3::new(-half, y, half);
        vec![[a, c, b], [a, d, c]]
    }

    #[test]
    fn chunk_removal_drops_collision() {
        let mut w = CollisionWorld::new();
        w.insert_chunk(1, quad(0.0, 1000.0));
        assert!(w.ground_below(Vec3::new(0.0, 100.0, 0.0), 500.0).is_some());
        w.remove_chunk(1);
        assert!(w.ground_below(Vec3::new(0.0, 100.0, 0.0), 500.0).is_none());
        assert_eq!(w.triangle_count(), 0);
    }

    #[test]
    fn raycast_hit_returns_facing_normal() {
        let mut w = CollisionWorld::new();
        let slope = 20f32.to_radians().tan();
        let p = |x: f32, z: f32| Vec3::new(x, x * slope, z);
        w.insert_chunk(
            1,
            vec![
                [p(-2000.0, -500.0), p(2000.0, 500.0), p(2000.0, -500.0)],
                [p(-2000.0, -500.0), p(-2000.0, 500.0), p(2000.0, 500.0)],
            ],
        );
        let (d, n) = w
            .raycast_hit(Vec3::new(1000.0, 5000.0, 0.0), Vec3::NEG_Y, 10_000.0)
            .unwrap();
        assert!((5000.0 - d - 1000.0 * slope).abs() < 0.1);
        assert!(n.y > 0.0 && (n.y - 20f32.to_radians().cos()).abs() < 1e-3);
    }
}
