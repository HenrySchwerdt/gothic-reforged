//! Minimal collision world and capsule character controller.
//!
//! Triangles are held in streamable chunks and indexed by a uniform XZ grid. The
//! controller is a push-out ("depenetration") capsule with step-up, ground snap and
//! slope limit. Units are the caller's; defaults assume 1 unit = 1 cm (Gothic 3), Y up.
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

    fn query(&self, lo: Vec3, hi: Vec3, out: &mut Vec<Tri>) {
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

    /// Highest surface below `from` within `depth` (vertical ray).
    pub fn ground_below(&self, from: Vec3, depth: f32) -> Option<f32> {
        self.raycast(from, Vec3::NEG_Y, depth).map(|d| from.y - d)
    }
}

fn ray_triangle(o: Vec3, d: Vec3, t: &Tri) -> Option<f32> {
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
fn segment_triangle(a: Vec3, b: Vec3, t: &Tri) -> (Vec3, Vec3, bool) {
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

#[derive(Debug, Clone, Copy)]
pub struct ControllerConfig {
    pub radius: f32,
    pub height: f32,
    pub step_height: f32,
    pub gravity: f32,
    /// Minimum surface normal Y that counts as walkable ground.
    pub walkable_normal_y: f32,
    pub jump_speed: f32,
}

impl Default for ControllerConfig {
    /// Centimetre units: 175 cm tall, 35 cm radius.
    fn default() -> Self {
        Self {
            radius: 35.0,
            height: 175.0,
            step_height: 45.0,
            gravity: 981.0,
            walkable_normal_y: 0.65,
            jump_speed: 380.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CharacterController {
    pub config: ControllerConfig,
    /// Feet position.
    pub position: Vec3,
    pub vertical_speed: f32,
    pub grounded: bool,
}

impl CharacterController {
    pub fn new(config: ControllerConfig, position: Vec3) -> Self {
        Self {
            config,
            position,
            vertical_speed: 0.0,
            grounded: false,
        }
    }

    fn segment(&self, feet: Vec3) -> (Vec3, Vec3) {
        let r = self.config.radius;
        (
            feet + Vec3::Y * r,
            feet + Vec3::Y * (self.config.height - r).max(r),
        )
    }

    /// Push the capsule out of nearby geometry. Returns (hit walkable ground, hit ceiling).
    fn resolve(
        &self,
        world: &CollisionWorld,
        feet: &mut Vec3,
        vertical_only_ground: bool,
        edge_ground: bool,
    ) -> (bool, bool) {
        let r = self.config.radius;
        let mut tris = Vec::new();
        let mut ground = false;
        let mut ceiling = false;
        for _ in 0..4 {
            let (a, b) = self.segment(*feet);
            let lo = a.min(b) - Vec3::splat(r + 1.0);
            let hi = a.max(b) + Vec3::splat(r + 1.0);
            world.query(lo, hi, &mut tris);
            let mut moved = false;
            for t in &tris {
                let (a, b) = self.segment(*feet);
                let (sp, tp, crossing) = segment_triangle(a, b, t);
                let dist = sp.distance(tp);
                if !crossing && dist >= r {
                    continue;
                }
                let mut n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
                if n == Vec3::ZERO {
                    continue;
                }
                let mid = (a + b) * 0.5;
                if (mid - t[0]).dot(n) < 0.0 && crossing {
                    n = -n;
                }
                let (push_dir, depth) = if crossing {
                    (n, r)
                } else {
                    ((sp - tp) / dist.max(1e-6), r - dist)
                };
                let rounded_edge = edge_ground
                    && tp.y < a.y
                    && push_dir.y > 0.2
                    && push_dir.y < self.config.walkable_normal_y;
                let walkable = rounded_edge || push_dir.y >= self.config.walkable_normal_y;
                let push = if walkable && vertical_only_ground {
                    // Resting on a slope: lift straight up so the capsule does not creep downhill.
                    Vec3::Y * (depth / push_dir.y.max(0.3)).min(self.config.step_height)
                } else if push_dir.y < self.config.walkable_normal_y && push_dir.y > -0.7 {
                    // Wall or steep slope: push horizontally only.
                    let h = Vec3::new(push_dir.x, 0.0, push_dir.z);
                    let hl = h.length().max(0.3);
                    h.normalize_or_zero() * (depth / hl).min(depth * 3.0)
                } else {
                    push_dir * depth
                };
                *feet += push;
                moved = true;
                if walkable {
                    ground = true;
                }
                if push_dir.y < -0.7 {
                    ceiling = true;
                }
            }
            if !moved {
                break;
            }
        }
        (ground, ceiling)
    }

    /// Height of a walkable, flat-topped ledge just ahead (within the step height), if any.
    fn step_up_height(&self, world: &CollisionWorld, start: Vec3, dir: Vec3) -> Option<f32> {
        let r = self.config.radius;
        let h = self.config.step_height;
        let probe = |ahead: f32| {
            let o = start + dir * ahead + Vec3::Y * (h + 2.0);
            world.ground_below(o, h + 2.0 + 1.0)
        };
        let h1 = probe(r + 3.0)?;
        let h2 = probe(r + 13.0)?;
        let rise = h1 - start.y;
        (rise > 1.0 && rise <= h && (h2 - h1).abs() < 8.0).then_some(h1 + 1.0)
    }

    /// Advance by `dt` seconds with the desired horizontal velocity (units/s).
    pub fn step(&mut self, world: &CollisionWorld, wish: Vec3, jump: bool, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);
        // Sub-step so a fast mover never skips past a wall thinner than the capsule radius.
        let travel = wish.length() * dt + self.vertical_speed.abs() * dt;
        let steps = ((travel / (self.config.radius * 0.4)).ceil() as usize).clamp(1, 12);
        let sub = dt / steps as f32;
        for i in 0..steps {
            self.sub_step(world, wish, jump && i == 0 && self.grounded, sub);
        }
    }

    fn sub_step(&mut self, world: &CollisionWorld, wish: Vec3, jump: bool, dt: f32) {
        let step_h = self.config.step_height;
        let h_move = Vec3::new(wish.x, 0.0, wish.z) * dt;
        if jump {
            self.vertical_speed = self.config.jump_speed;
            self.grounded = false;
        }
        if self.grounded && self.vertical_speed <= 0.0 {
            let start = self.position;
            let mut p = start + h_move;
            self.resolve(world, &mut p, true, false);
            let wanted = h_move.length();
            if wanted > 1e-4 {
                let dir = h_move / wanted;
                let achieved = (p - start).dot(dir);
                if achieved < wanted * 0.5
                    && let Some(top) = self.step_up_height(world, start, dir)
                {
                    let raised = Vec3::new(start.x, top, start.z);
                    p = raised + h_move;
                    self.resolve(world, &mut p, true, false);
                }
            }
            // Snap down so walking downhill or off a small ledge keeps ground contact.
            let mut q = p;
            let mut ground = false;
            for _ in 0..((step_h + 6.0) / 4.0).ceil() as usize {
                q.y -= 4.0;
                ground = self.resolve(world, &mut q, true, true).0;
                if ground {
                    break;
                }
            }
            if ground {
                self.position = q;
                self.vertical_speed = 0.0;
            } else {
                self.position = p;
                self.grounded = false;
            }
        } else {
            self.vertical_speed -= self.config.gravity * dt;
            self.vertical_speed = self.vertical_speed.max(-3000.0);
            let mut p = self.position + h_move + Vec3::Y * (self.vertical_speed * dt);
            let (ground, ceiling) = self.resolve(world, &mut p, true, false);
            self.position = p;
            if ground && self.vertical_speed <= 0.0 {
                self.grounded = true;
                self.vertical_speed = 0.0;
            } else {
                self.grounded = false;
                if ceiling && self.vertical_speed > 0.0 {
                    self.vertical_speed = 0.0;
                }
            }
        }
    }
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
    fn wall(x: f32) -> Vec<Tri> {
        let a = Vec3::new(x, -100.0, -500.0);
        let b = Vec3::new(x, 500.0, -500.0);
        let c = Vec3::new(x, 500.0, 500.0);
        let d = Vec3::new(x, -100.0, 500.0);
        vec![[a, b, c], [a, c, d]]
    }
    fn controller(at: Vec3) -> CharacterController {
        CharacterController::new(ControllerConfig::default(), at)
    }

    #[test]
    fn falls_and_rests_on_floor() {
        let mut w = CollisionWorld::new();
        w.insert_chunk(1, quad(0.0, 2000.0));
        let mut c = controller(Vec3::new(0.0, 300.0, 0.0));
        for _ in 0..240 {
            c.step(&w, Vec3::ZERO, false, 1.0 / 60.0);
        }
        assert!(c.grounded);
        assert!(c.position.y.abs() < 3.0, "{}", c.position.y);
    }

    #[test]
    fn wall_blocks_horizontal_motion() {
        let mut w = CollisionWorld::new();
        w.insert_chunk(1, quad(0.0, 3000.0));
        w.insert_chunk(2, wall(300.0));
        let mut c = controller(Vec3::new(0.0, 1.0, 0.0));
        for _ in 0..300 {
            c.step(&w, Vec3::new(400.0, 0.0, 0.0), false, 1.0 / 60.0);
        }
        let r = c.config.radius;
        assert!(c.position.x <= 300.0 - r + 2.0, "x={}", c.position.x);
        assert!(c.position.x > 200.0);
    }

    #[test]
    fn steps_up_small_ledge() {
        let mut w = CollisionWorld::new();
        w.insert_chunk(1, quad(0.0, 3000.0));
        let top = 30.0;
        let (x0, x1) = (200.0, 2000.0);
        let a = Vec3::new(x0, top, -500.0);
        let b = Vec3::new(x1, top, -500.0);
        let c2 = Vec3::new(x1, top, 500.0);
        let d = Vec3::new(x0, top, 500.0);
        let lo = Vec3::new(x0, 0.0, -500.0);
        let lo2 = Vec3::new(x0, 0.0, 500.0);
        w.insert_chunk(2, vec![[a, c2, b], [a, d, c2], [lo, a, d], [lo, d, lo2]]);
        let mut c = controller(Vec3::new(0.0, 1.0, 0.0));
        for _ in 0..240 {
            c.step(&w, Vec3::new(300.0, 0.0, 0.0), false, 1.0 / 60.0);
        }
        assert!(c.position.x > 300.0, "x={}", c.position.x);
        assert!((c.position.y - top).abs() < 4.0, "y={}", c.position.y);
    }

    #[test]
    fn walks_up_gentle_slope_and_raycast_works() {
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
        let g = w
            .ground_below(Vec3::new(1000.0, 5000.0, 0.0), 10000.0)
            .unwrap();
        assert!((g - 1000.0 * slope).abs() < 0.1);
        let mut c = controller(Vec3::new(0.0, 2.0, 0.0));
        for _ in 0..180 {
            c.step(&w, Vec3::new(300.0, 0.0, 0.0), false, 1.0 / 60.0);
        }
        assert!(c.position.x > 400.0, "x={}", c.position.x);
        assert!(c.grounded);
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
}
