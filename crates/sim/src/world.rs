//! World abstraction. The simulation never owns geometry: positions, line of sight and
//! pathing come from a [`WorldQuery`] that the 3D game implements later (physics, navmesh).
//! [`Grid2dWorld`] is a small deterministic implementation for tests and headless runs.
//!
//! Units are the game's centimetres; the horizontal plane is `x`/`z`, `y` is up [CA 10.1
//! "vertical offset above 1000"].
use crate::types::ActorId;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pos {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Pos {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
    /// Horizontal position on the ground plane (`y = 0`).
    pub fn flat(x: f32, z: f32) -> Self {
        Self { x, y: 0.0, z }
    }
    pub fn dist_xz(&self, o: Pos) -> f32 {
        ((self.x - o.x).powi(2) + (self.z - o.z).powi(2)).sqrt()
    }
    pub fn dist(&self, o: Pos) -> f32 {
        ((self.x - o.x).powi(2) + (self.y - o.y).powi(2) + (self.z - o.z).powi(2)).sqrt()
    }
    /// Yaw (radians) of the horizontal direction towards `o`; `0` looks along `+x`.
    pub fn yaw_to(&self, o: Pos) -> f32 {
        (o.z - self.z).atan2(o.x - self.x)
    }
}

/// Security level of the zone an actor stands in [CA 10.2 `AssessPlayerZone`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZoneSecurity {
    #[default]
    None,
    Public,
    Enclave,
    Political,
}

/// Everything the simulation asks of the world. Movement is applied through the world so
/// the game can route it through its physics / navigation.
pub trait WorldQuery {
    fn position(&self, id: ActorId) -> Option<Pos>;
    /// Facing as yaw in radians (see [`Pos::yaw_to`]).
    fn facing(&self, id: ActorId) -> f32;
    /// Free line of sight between two points.
    fn line_of_sight(&self, from: Pos, to: Pos) -> bool;
    /// Walk `id` towards `target` along a path by at most `max_dist`; returns the distance
    /// actually moved and turns the actor to face its movement direction.
    fn move_towards(&mut self, id: ActorId, target: Pos, max_dist: f32) -> f32;
    fn face_towards(&mut self, id: ActorId, target: Pos);
    fn teleport(&mut self, id: ActorId, pos: Pos);
    /// Length of the walkable path, `None` when unreachable. Defaults to the straight line.
    fn path_length(&self, from: Pos, to: Pos) -> Option<f32> {
        Some(from.dist_xz(to))
    }
    fn zone_security(&self, _at: Pos) -> ZoneSecurity {
        ZoneSecurity::None
    }
    /// LOS is skipped outdoors in the same zone [CA 10.1].
    fn is_outdoors(&self, _at: Pos) -> bool {
        false
    }
    /// Height of a flat floor at `(x, z)` for projectiles that fall to the ground; `None` when
    /// the world's own geometry ([`WorldQuery::surface_hit`]) answers.
    fn ground_height(&self, _x: f32, _z: f32) -> Option<f32> {
        None
    }
    /// First static contact of the segment `from -> to` (a projectile's sweep). The default
    /// finds it by bisecting [`WorldQuery::line_of_sight`]; worlds with real geometry override
    /// it with a ray cast that also knows the surface normal and material.
    fn surface_hit(&self, from: Pos, to: Pos) -> Option<SurfaceContact> {
        if self.line_of_sight(from, to) {
            return None;
        }
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..12 {
            let mid = (lo + hi) * 0.5;
            let p = Pos::new(
                from.x + (to.x - from.x) * mid,
                from.y + (to.y - from.y) * mid,
                from.z + (to.z - from.z) * mid,
            );
            if self.line_of_sight(from, p) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let d = [to.x - from.x, to.y - from.y, to.z - from.z];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6);
        Some(SurfaceContact {
            t: hi,
            normal: [-d[0] / len, -d[1] / len, -d[2] / len],
            material: 5,
        })
    }
}

/// A projectile's contact with the level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceContact {
    /// Position along the swept segment, `0..=1`.
    pub t: f32,
    /// Surface normal towards the side the projectile came from.
    pub normal: [f32; 3],
    /// `eEShapeMaterial` value (5 = earth when the world does not know).
    pub material: u8,
}

/// Is `target` inside the horizontal field of view of `observer` (degrees, total angle)?
pub fn in_fov(world: &dyn WorldQuery, observer: ActorId, target: Pos, fov_degrees: f32) -> bool {
    let Some(p) = world.position(observer) else {
        return false;
    };
    if p.dist_xz(target) < 1.0 {
        return true;
    }
    let mut d = (p.yaw_to(target) - world.facing(observer)).abs() % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d = std::f32::consts::TAU - d;
    }
    d <= fov_degrees.to_radians() / 2.0
}

/// Axis-aligned obstacle rectangle on the ground plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub min_x: f32,
    pub min_z: f32,
    pub max_x: f32,
    pub max_z: f32,
}

impl Rect {
    pub fn new(min_x: f32, min_z: f32, max_x: f32, max_z: f32) -> Self {
        Self {
            min_x,
            min_z,
            max_x,
            max_z,
        }
    }
    fn contains(&self, x: f32, z: f32) -> bool {
        x >= self.min_x && x <= self.max_x && z >= self.min_z && z <= self.max_z
    }
}

const CELL: f32 = 100.0;

/// Deterministic 2D test world: rectangular walls, BFS pathing on a 100-unit grid.
#[derive(Debug, Clone, Default)]
pub struct Grid2dWorld {
    actors: BTreeMap<ActorId, (Pos, f32)>,
    walls: Vec<Rect>,
    zones: Vec<(Rect, ZoneSecurity)>,
    /// Treat everything as outdoors (skips LOS like the original does outdoors).
    pub outdoors: bool,
}

impl Grid2dWorld {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add_wall(&mut self, r: Rect) -> &mut Self {
        self.walls.push(r);
        self
    }
    pub fn add_zone(&mut self, r: Rect, s: ZoneSecurity) -> &mut Self {
        self.zones.push((r, s));
        self
    }
    pub fn place(&mut self, id: ActorId, pos: Pos) {
        self.actors.insert(id, (pos, 0.0));
    }
    pub fn set_facing(&mut self, id: ActorId, yaw: f32) {
        if let Some(e) = self.actors.get_mut(&id) {
            e.1 = yaw;
        }
    }

    fn blocked(&self, x: f32, z: f32) -> bool {
        self.walls.iter().any(|w| w.contains(x, z))
    }

    fn segment_free(&self, a: Pos, b: Pos) -> bool {
        let len = a.dist_xz(b);
        let steps = (len / 25.0).ceil().max(1.0) as i32;
        (0..=steps).all(|i| {
            let t = i as f32 / steps as f32;
            !self.blocked(a.x + (b.x - a.x) * t, a.z + (b.z - a.z) * t)
        })
    }

    fn cell(p: Pos) -> (i32, i32) {
        ((p.x / CELL).floor() as i32, (p.z / CELL).floor() as i32)
    }
    fn center(c: (i32, i32)) -> Pos {
        Pos::flat((c.0 as f32 + 0.5) * CELL, (c.1 as f32 + 0.5) * CELL)
    }

    /// BFS over free cells; returns the cell path from `from` to `to` (inclusive).
    fn bfs(&self, from: Pos, to: Pos) -> Option<Vec<(i32, i32)>> {
        let (s, g) = (Self::cell(from), Self::cell(to));
        let pad = 30;
        let (lo_x, hi_x) = (s.0.min(g.0) - pad, s.0.max(g.0) + pad);
        let (lo_z, hi_z) = (s.1.min(g.1) - pad, s.1.max(g.1) + pad);
        let mut prev: BTreeMap<(i32, i32), (i32, i32)> = BTreeMap::new();
        let mut seen: BTreeSet<(i32, i32)> = BTreeSet::new();
        let mut q = VecDeque::new();
        seen.insert(s);
        q.push_back(s);
        while let Some(c) = q.pop_front() {
            if c == g {
                let mut path = vec![c];
                let mut cur = c;
                while let Some(&p) = prev.get(&cur) {
                    path.push(p);
                    cur = p;
                }
                path.reverse();
                return Some(path);
            }
            for dx in -1..=1 {
                for dz in -1..=1 {
                    if dx == 0 && dz == 0 {
                        continue;
                    }
                    let n = (c.0 + dx, c.1 + dz);
                    if n.0 < lo_x || n.0 > hi_x || n.1 < lo_z || n.1 > hi_z || seen.contains(&n) {
                        continue;
                    }
                    let ctr = Self::center(n);
                    if n != g && self.blocked(ctr.x, ctr.z) {
                        continue;
                    }
                    // No diagonal corner cutting.
                    if dx != 0 && dz != 0 {
                        let a = Self::center((c.0 + dx, c.1));
                        let b = Self::center((c.0, c.1 + dz));
                        if self.blocked(a.x, a.z) || self.blocked(b.x, b.z) {
                            continue;
                        }
                    }
                    seen.insert(n);
                    prev.insert(n, c);
                    q.push_back(n);
                }
            }
        }
        None
    }

    /// Next waypoint towards `target`: the target itself when visible, else the farthest
    /// visible cell centre on the BFS path.
    fn waypoint(&self, from: Pos, target: Pos) -> Option<Pos> {
        if self.segment_free(from, target) {
            return Some(target);
        }
        let path = self.bfs(from, target)?;
        let mut best = None;
        for c in path.iter().skip(1) {
            let p = Self::center(*c);
            if self.segment_free(from, p) {
                best = Some(p);
            } else if best.is_some() {
                break;
            }
        }
        best.or_else(|| path.get(1).map(|c| Self::center(*c)))
    }
}

impl WorldQuery for Grid2dWorld {
    fn ground_height(&self, _x: f32, _z: f32) -> Option<f32> {
        Some(0.0)
    }
    fn position(&self, id: ActorId) -> Option<Pos> {
        self.actors.get(&id).map(|e| e.0)
    }
    fn facing(&self, id: ActorId) -> f32 {
        self.actors.get(&id).map_or(0.0, |e| e.1)
    }
    fn line_of_sight(&self, from: Pos, to: Pos) -> bool {
        self.segment_free(from, to)
    }
    fn move_towards(&mut self, id: ActorId, target: Pos, max_dist: f32) -> f32 {
        let Some(&(pos, _)) = self.actors.get(&id) else {
            return 0.0;
        };
        let Some(wp) = self.waypoint(pos, target) else {
            return 0.0;
        };
        let d = pos.dist_xz(wp);
        if d < 0.001 {
            return 0.0;
        }
        let step = d.min(max_dist);
        let t = step / d;
        let np = Pos::new(
            pos.x + (wp.x - pos.x) * t,
            pos.y,
            pos.z + (wp.z - pos.z) * t,
        );
        let yaw = pos.yaw_to(wp);
        self.actors.insert(id, (np, yaw));
        step
    }
    fn face_towards(&mut self, id: ActorId, target: Pos) {
        if let Some(e) = self.actors.get_mut(&id)
            && e.0.dist_xz(target) > 0.001
        {
            e.1 = e.0.yaw_to(target);
        }
    }
    fn teleport(&mut self, id: ActorId, pos: Pos) {
        let yaw = self.actors.get(&id).map_or(0.0, |e| e.1);
        self.actors.insert(id, (pos, yaw));
    }
    fn path_length(&self, from: Pos, to: Pos) -> Option<f32> {
        if self.segment_free(from, to) {
            return Some(from.dist_xz(to));
        }
        let p = self.bfs(from, to)?;
        Some(p.len().saturating_sub(1) as f32 * CELL)
    }
    fn zone_security(&self, at: Pos) -> ZoneSecurity {
        self.zones
            .iter()
            .find(|(r, _)| r.contains(at.x, at.z))
            .map_or(ZoneSecurity::None, |z| z.1)
    }
    fn is_outdoors(&self, _at: Pos) -> bool {
        self.outdoors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_blocked_by_wall_and_open_otherwise() {
        let mut w = Grid2dWorld::new();
        w.add_wall(Rect::new(450.0, -500.0, 550.0, 500.0));
        assert!(!w.line_of_sight(Pos::flat(0.0, 0.0), Pos::flat(1000.0, 0.0)));
        assert!(w.line_of_sight(Pos::flat(0.0, 0.0), Pos::flat(400.0, 100.0)));
    }

    #[test]
    fn path_goes_around_a_wall() {
        let mut w = Grid2dWorld::new();
        w.add_wall(Rect::new(450.0, -500.0, 550.0, 500.0));
        let a = ActorId(1);
        w.place(a, Pos::flat(0.0, 0.0));
        let target = Pos::flat(1000.0, 0.0);
        let len = w.path_length(Pos::flat(0.0, 0.0), target).unwrap();
        assert!(len > 1100.0, "detour expected, got {len}");
        let mut moved = 0.0;
        for _ in 0..400 {
            moved += w.move_towards(a, target, 50.0);
            if w.position(a).unwrap().dist_xz(target) < 1.0 {
                break;
            }
        }
        assert!(w.position(a).unwrap().dist_xz(target) < 1.0);
        assert!(moved > 1100.0);
    }

    #[test]
    fn fov_uses_facing() {
        let mut w = Grid2dWorld::new();
        let a = ActorId(1);
        w.place(a, Pos::flat(0.0, 0.0));
        w.set_facing(a, 0.0);
        assert!(in_fov(&w, a, Pos::flat(500.0, 100.0), 120.0));
        assert!(!in_fov(&w, a, Pos::flat(-500.0, 0.0), 120.0));
        w.face_towards(a, Pos::flat(-500.0, 0.0));
        assert!(in_fov(&w, a, Pos::flat(-500.0, 0.0), 120.0));
    }

    #[test]
    fn unreachable_target_has_no_path() {
        let mut w = Grid2dWorld::new();
        // Closed box around the goal.
        w.add_wall(Rect::new(900.0, -300.0, 1000.0, 300.0));
        w.add_wall(Rect::new(1000.0, 200.0, 1500.0, 300.0));
        w.add_wall(Rect::new(1000.0, -300.0, 1500.0, -200.0));
        w.add_wall(Rect::new(1400.0, -300.0, 1500.0, 300.0));
        // Bounded search (pad 30 cells) still finds a way only if open; here fully enclosed.
        assert!(
            w.path_length(Pos::flat(0.0, 0.0), Pos::flat(1200.0, 0.0))
                .is_none()
        );
    }
}
