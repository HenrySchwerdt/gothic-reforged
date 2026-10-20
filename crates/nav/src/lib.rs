//! Navigation for Gothic characters: a walkable-cell navmesh baked from the collision world,
//! A* over it, string pulling, path following and local avoidance.
//!
//! The original game navigates with authored nav zones (`gCNavigation_PS`, `gCNavZone_PS`,
//! `gCCollisionCircle_PS` obstacles) and a physics-driven mover; those zone shapes are not
//! decoded yet, so the walkable surface is derived from the same collision geometry the
//! character controller walks on. A cell is walkable when the controller could stand there:
//! ground within `StepHeight` of the neighbour it was reached from, ground angle below the
//! limit where the original's NPC uphill speed scale reaches its 0.3 floor, and a free
//! capsule of the agent radius above the step height. Units are cm, Y up.
//!
//! Layers (bridges over roads, floors of a house) are handled by growing the mesh from seed
//! points: each cell takes the ground reachable from its parent, so overlapping layers do not
//! mix as long as they are seeded from the right side.
use glam::{Vec2, Vec3};
use gothic_physics::{CollisionWorld, MovementProfile};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::sync::{Arc, RwLock};

pub type CellKey = (i32, i32);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NavParams {
    /// Cell edge, cm. T: 40 keeps 120 cm doorways passable on any lattice alignment and
    /// makes a mid-edge gap smaller than the agent radius.
    pub cell: f32,
    pub radius: f32,
    pub step_height: f32,
    pub height: f32,
    /// Steepest walkable ground, degrees. V-derived: the NPC uphill speed scale
    /// `1 - (a - SteepMin) / (SteepMax - SteepMin)` reaches its 0.3 floor at this angle.
    pub max_slope_deg: f32,
    /// Slope speed model used for path costs.
    pub profile: MovementProfile,
}

impl NavParams {
    pub fn for_profile(p: MovementProfile) -> Self {
        let floor = 0.3;
        let max_slope = p.steep_min_deg + (1.0 - floor) * (p.steep_max_deg - p.steep_min_deg);
        Self {
            cell: 40.0,
            radius: p.radius,
            step_height: p.step_height,
            height: p.height,
            max_slope_deg: max_slope,
            profile: p,
        }
    }
}

impl Default for NavParams {
    fn default() -> Self {
        Self::for_profile(MovementProfile::human_npc())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Cell {
    pub y: f32,
    /// Ground angle in degrees.
    pub angle: f32,
    /// Horizontal downhill direction of the ground (unit, zero on flat ground).
    pub downhill: Vec2,
    /// Clear of geometry by the agent radius plus half a cell: any point of the cell is at
    /// least the agent radius away from walls. Narrow cells are only crossed near their centre.
    pub wide: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Path {
    pub points: Vec<Vec3>,
    /// Horizontal length of the polyline, cm.
    pub length: f32,
}

#[derive(Debug, Clone)]
pub struct NavMesh {
    pub params: NavParams,
    cells: HashMap<CellKey, Cell>,
    blocked: HashSet<CellKey>,
    queue: VecDeque<CellKey>,
}

#[derive(PartialEq)]
struct Node(f32, CellKey);
impl Eq for Node {}
impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Node {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.total_cmp(&self.0).then_with(|| o.1.cmp(&self.1))
    }
}

const NEIGHBOURS: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

impl NavMesh {
    pub fn new(params: NavParams) -> Self {
        Self {
            params,
            cells: HashMap::new(),
            blocked: HashSet::new(),
            queue: VecDeque::new(),
        }
    }

    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// Cells still waiting for their neighbours to be examined.
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    pub fn key_of(&self, x: f32, z: f32) -> CellKey {
        (
            (x / self.params.cell).floor() as i32,
            (z / self.params.cell).floor() as i32,
        )
    }

    fn center(&self, k: CellKey) -> (f32, f32) {
        (
            (k.0 as f32 + 0.5) * self.params.cell,
            (k.1 as f32 + 0.5) * self.params.cell,
        )
    }

    pub fn cell_at(&self, x: f32, z: f32) -> Option<&Cell> {
        self.cells.get(&self.key_of(x, z))
    }

    pub fn is_walkable(&self, x: f32, z: f32) -> bool {
        self.cell_at(x, z).is_some()
    }

    /// Ground height of the mesh at `(x, z)` (nearest walkable cell centre).
    pub fn height_at(&self, x: f32, z: f32) -> Option<f32> {
        self.cell_at(x, z).map(|c| c.y)
    }

    /// Examine one cell reached from a neighbour at height `from_y`.
    fn classify(&self, world: &CollisionWorld, k: CellKey, from_y: f32) -> Option<Cell> {
        let p = &self.params;
        let (x, z) = self.center(k);
        let top = from_y + p.step_height + 1.0;
        let (d, n) =
            world.raycast_hit(Vec3::new(x, top, z), Vec3::NEG_Y, p.step_height * 2.0 + 2.0)?;
        let y = top - d;
        if (y - from_y).abs() > p.step_height + 0.5 {
            return None;
        }
        let angle = n.y.clamp(-1.0, 1.0).acos().to_degrees();
        if angle > p.max_slope_deg {
            return None;
        }
        let a = Vec3::new(x, y + p.step_height + p.radius, z);
        let b = Vec3::new(x, (y + p.height - p.radius).max(a.y), z);
        let (blocked, tight) = world.capsule_blocked2(a, b, p.radius, p.radius + p.cell * 0.5);
        if blocked {
            return None;
        }
        let nh = Vec2::new(n.x, n.z);
        let wide = !tight;
        Some(Cell {
            y,
            angle,
            downhill: nh.normalize_or_zero(),
            wide,
        })
    }

    /// Ground below `pos` (searching `up` above it and `down` below it) as a walkable cell.
    pub fn add_seed(&mut self, world: &CollisionWorld, pos: Vec3) -> bool {
        let k = self.key_of(pos.x, pos.z);
        if self.cells.contains_key(&k) {
            return true;
        }
        let (x, z) = self.center(k);
        for from_y in [pos.y, pos.y + 100.0, pos.y - 100.0] {
            if let Some(c) = self.classify(world, k, from_y) {
                self.cells.insert(k, c);
                self.queue.push_back(k);
                self.blocked.remove(&k);
                let _ = (x, z);
                return true;
            }
        }
        false
    }

    /// Grow the mesh from the pending cells, examining at most `budget` new cells and staying
    /// inside the disc `(centre, radius)` when given. Returns the number of new walkable cells.
    pub fn extend(
        &mut self,
        world: &CollisionWorld,
        within: Option<(Vec2, f32)>,
        mut budget: usize,
    ) -> usize {
        let mut added = 0;
        let mut deferred: Vec<CellKey> = Vec::new();
        while budget > 0 {
            let Some(k) = self.queue.pop_front() else {
                break;
            };
            let from_y = self.cells[&k].y;
            let mut defer = false;
            for (dx, dz) in NEIGHBOURS {
                let n = (k.0 + dx, k.1 + dz);
                if self.cells.contains_key(&n) || self.blocked.contains(&n) {
                    continue;
                }
                if let Some((c, r)) = within {
                    let (x, z) = self.center(n);
                    if Vec2::new(x, z).distance(c) > r {
                        // Outside the requested region: revisit when the region grows.
                        defer = true;
                        continue;
                    }
                }
                budget = budget.saturating_sub(1);
                match self.classify(world, n, from_y) {
                    Some(c) => {
                        self.cells.insert(n, c);
                        self.queue.push_back(n);
                        added += 1;
                    }
                    None => {
                        self.blocked.insert(n);
                    }
                }
            }
            if defer {
                deferred.push(k);
            }
        }
        self.queue.extend(deferred);
        added
    }

    /// Seed at `pos` and grow within `radius` until nothing more can be added.
    pub fn bake_disc(&mut self, world: &CollisionWorld, pos: Vec3, radius: f32) -> usize {
        self.add_seed(world, pos);
        let mut total = 0;
        loop {
            let n = self.extend(world, Some((Vec2::new(pos.x, pos.z), radius)), 20_000);
            total += n;
            if n == 0 {
                break;
            }
        }
        total
    }

    /// Nearest walkable cell to `(x, z)` within `max_cells` rings.
    pub fn nearest_cell(&self, x: f32, z: f32, max_cells: i32) -> Option<CellKey> {
        let k = self.key_of(x, z);
        if self.cells.contains_key(&k) {
            return Some(k);
        }
        let mut best: Option<(f32, CellKey)> = None;
        for r in 1..=max_cells {
            for dx in -r..=r {
                for dz in -r..=r {
                    if dx.abs() != r && dz.abs() != r {
                        continue;
                    }
                    let c = (k.0 + dx, k.1 + dz);
                    if self.cells.contains_key(&c) {
                        let (cx, cz) = self.center(c);
                        let d = (cx - x).hypot(cz - z);
                        if best.is_none_or(|b| d < b.0) {
                            best = Some((d, c));
                        }
                    }
                }
            }
            if best.is_some() {
                break;
            }
        }
        best.map(|b| b.1)
    }

    /// Time-like cost of walking from `a` to `b` (adjacent cells): distance divided by the
    /// slope speed scale of the original (uphill slower; downhill is not credited so the
    /// straight-line heuristic stays admissible).
    fn step_cost(&self, a: CellKey, b: CellKey) -> f32 {
        let ca = &self.cells[&a];
        let cb = &self.cells[&b];
        let dist =
            (((a.0 - b.0) as f32).powi(2) + ((a.1 - b.1) as f32).powi(2)).sqrt() * self.params.cell;
        if cb.y <= ca.y + 0.5 {
            return dist;
        }
        let dir = Vec2::new((b.0 - a.0) as f32, (b.1 - a.1) as f32).normalize_or_zero();
        let downhill = dir.dot(cb.downhill) >= 0.0;
        let scale = slope_scale(&self.params.profile, cb.angle, downhill).max(0.3);
        dist / scale.min(1.0)
    }

    /// Shortest walkable path from `from` to `to`, string-pulled. `None` when unreachable.
    pub fn find_path(&self, from: Vec3, to: Vec3) -> Option<Path> {
        let start = self.nearest_cell(from.x, from.z, 6)?;
        let goal = self.nearest_cell(to.x, to.z, 6)?;
        let cells = self.astar(start, goal)?;
        let mut pts: Vec<Vec3> = cells
            .iter()
            .map(|k| {
                let (x, z) = self.center(*k);
                Vec3::new(x, self.cells[k].y, z)
            })
            .collect();
        if let Some(first) = pts.first_mut() {
            first.x = from.x;
            first.z = from.z;
        }
        if let Some(last) = pts.last_mut()
            && self.is_walkable(to.x, to.z)
        {
            last.x = to.x;
            last.z = to.z;
        }
        let pts = self.pull_string(&pts);
        let length = pts.windows(2).map(|w| (w[1] - w[0]).xz_len()).sum();
        Some(Path {
            points: pts,
            length,
        })
    }

    fn astar(&self, start: CellKey, goal: CellKey) -> Option<Vec<CellKey>> {
        let h = |k: CellKey| {
            (((k.0 - goal.0) as f32).powi(2) + ((k.1 - goal.1) as f32).powi(2)).sqrt()
                * self.params.cell
        };
        let mut open = BinaryHeap::new();
        let mut g: HashMap<CellKey, f32> = HashMap::new();
        let mut parent: HashMap<CellKey, CellKey> = HashMap::new();
        g.insert(start, 0.0);
        open.push(Node(h(start), start));
        while let Some(Node(f, k)) = open.pop() {
            if k == goal {
                let mut out = vec![k];
                let mut cur = k;
                while let Some(p) = parent.get(&cur) {
                    out.push(*p);
                    cur = *p;
                }
                out.reverse();
                return Some(out);
            }
            let gk = g[&k];
            if f > gk + h(k) + 1.0e-3 {
                continue; // stale entry
            }
            let from_y = self.cells[&k].y;
            for (dx, dz) in NEIGHBOURS {
                let n = (k.0 + dx, k.1 + dz);
                let Some(cn) = self.cells.get(&n) else {
                    continue;
                };
                if (cn.y - from_y).abs() > self.params.step_height + 0.5 {
                    continue;
                }
                if dx != 0 && dz != 0 {
                    // No corner cutting.
                    let ok = |c: CellKey| {
                        self.cells
                            .get(&c)
                            .is_some_and(|x| (x.y - from_y).abs() <= self.params.step_height + 0.5)
                    };
                    if !ok((k.0 + dx, k.1)) || !ok((k.0, k.1 + dz)) {
                        continue;
                    }
                }
                let ng = gk + self.step_cost(k, n);
                if g.get(&n).is_none_or(|&old| ng < old) {
                    g.insert(n, ng);
                    parent.insert(n, k);
                    open.push(Node(ng + h(n), n));
                }
            }
        }
        None
    }

    /// Walkable straight line between two points: every cell the segment touches exists and
    /// consecutive cells differ by at most the step height; cells that are not `wide` may only
    /// be crossed within 10 cm of their centre, which keeps the agent radius from walls.
    pub fn line_walkable(&self, a: Vec3, b: Vec3) -> bool {
        let len = (b - a).xz_len();
        let steps = (len / 10.0).ceil().max(1.0) as usize;
        let mut prev: Option<f32> = None;
        let mut last_key = None;
        for i in 0..=steps {
            let p = a.lerp(b, i as f32 / steps as f32);
            let k = self.key_of(p.x, p.z);
            let Some(cell) = self.cells.get(&k) else {
                return false;
            };
            if !cell.wide {
                let (cx, cz) = self.center(k);
                if (p.x - cx).hypot(p.z - cz) > 10.0 {
                    return false;
                }
            }
            if last_key != Some(k) {
                if prev.is_some_and(|y| (cell.y - y).abs() > self.params.step_height * 0.6) {
                    return false;
                }
                prev = Some(cell.y);
                last_key = Some(k);
            }
        }
        true
    }

    /// String pulling: drop every waypoint that a straight walkable line can skip (the grid
    /// equivalent of the funnel algorithm over a corridor of cells).
    fn pull_string(&self, pts: &[Vec3]) -> Vec<Vec3> {
        if pts.len() <= 2 {
            return pts.to_vec();
        }
        const WINDOW: usize = 120;
        let mut out = vec![pts[0]];
        let mut i = 0;
        while i + 1 < pts.len() {
            let far = (i + WINDOW).min(pts.len() - 1);
            let mut j = far;
            while j > i + 1 && !self.line_walkable(pts[i], pts[j]) {
                j -= 1;
            }
            out.push(pts[j]);
            i = j;
        }
        out
    }
}

/// Slope speed scale of the original (`CalcGroundSlopeTransScale`), see
/// `gothic_physics::CharacterController::slope_target_scale`.
fn slope_scale(p: &MovementProfile, angle: f32, downhill: bool) -> f32 {
    gothic_physics::CharacterController::new(*p, Vec3::ZERO).slope_target_scale(angle, downhill)
}

trait XzLen {
    fn xz_len(self) -> f32;
}
impl XzLen for Vec3 {
    fn xz_len(self) -> f32 {
        self.x.hypot(self.z)
    }
}

/// Walks along a [`Path`].
#[derive(Debug, Clone, Default)]
pub struct PathFollower {
    pub path: Vec<Vec3>,
    pub next: usize,
    pub goal: Vec3,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FollowStep {
    pub position: Vec3,
    /// Horizontal direction moved (unit), zero when idle.
    pub heading: Vec2,
    pub moved: f32,
    pub arrived: bool,
}

impl PathFollower {
    pub fn new(path: Path, goal: Vec3) -> Self {
        Self {
            path: path.points,
            next: 1,
            goal,
        }
    }

    /// Move `pos` along the path by up to `dist` (cm); heights come from the mesh.
    pub fn advance(&mut self, nav: &NavMesh, pos: Vec3, mut dist: f32) -> FollowStep {
        let mut p = pos;
        let mut moved = 0.0;
        let mut heading = Vec2::ZERO;
        while dist > 1.0e-3 && self.next < self.path.len() {
            let t = self.path[self.next];
            let d = Vec2::new(t.x - p.x, t.z - p.z);
            let l = d.length();
            if l <= dist {
                p = Vec3::new(t.x, t.y, t.z);
                dist -= l;
                moved += l;
                if l > 1.0e-3 {
                    heading = d / l;
                }
                self.next += 1;
            } else {
                let dir = d / l;
                p.x += dir.x * dist;
                p.z += dir.y * dist;
                moved += dist;
                heading = dir;
                dist = 0.0;
                if let Some(y) = nav.height_at(p.x, p.z) {
                    p.y = y;
                }
            }
        }
        FollowStep {
            position: p,
            heading,
            moved,
            arrived: self.next >= self.path.len(),
        }
    }
}

/// Local avoidance: choose a step direction close to `desired` that does not run into any
/// neighbour within the next `horizon` seconds. Sampling-based velocity-obstacle search over
/// fixed heading offsets; `neighbours` are `(position, radius, velocity)`.
pub fn avoid(
    pos: Vec2,
    desired: Vec2,
    speed: f32,
    radius: f32,
    neighbours: &[(Vec2, f32, Vec2)],
    horizon: f32,
) -> Vec2 {
    let dir = desired.normalize_or_zero();
    if dir == Vec2::ZERO || neighbours.is_empty() {
        return dir;
    }
    const OFFSETS: [f32; 13] = [
        0.0, 15.0, -15.0, 30.0, -30.0, 45.0, -45.0, 60.0, -60.0, 80.0, -80.0, 110.0, -110.0,
    ];
    let mut best = (f32::MAX, dir);
    for off in OFFSETS {
        let (s, c) = off.to_radians().sin_cos();
        let cand = Vec2::new(dir.x * c - dir.y * s, dir.x * s + dir.y * c);
        let v = cand * speed;
        let mut tmin = f32::MAX;
        for (np, nr, nv) in neighbours {
            tmin = tmin.min(time_to_collision(*np - pos, *nv - v, radius + *nr));
        }
        // Penalty: imminent collisions are costly, deviation from the wanted heading less so.
        let collide = if tmin < horizon {
            (horizon - tmin) / horizon * 10.0 + 2.0
        } else {
            0.0
        };
        let cost = collide + off.abs() / 180.0;
        if cost < best.0 {
            best = (cost, cand);
        }
    }
    best.1
}

/// Time until two discs with relative position `p` and relative velocity `v` overlap (`inf`
/// when never; `0` when already overlapping).
fn time_to_collision(p: Vec2, v: Vec2, r: f32) -> f32 {
    let c = p.length_squared() - r * r;
    if c <= 0.0 {
        return 0.0;
    }
    let a = v.length_squared();
    if a < 1.0e-6 {
        return f32::MAX;
    }
    let b = p.dot(v);
    if b >= 0.0 {
        return f32::MAX;
    }
    let disc = b * b - a * c;
    if disc <= 0.0 {
        return f32::MAX;
    }
    (-b - disc.sqrt()) / a
}

/// Shared handle for the game: the mesh is grown on a worker or in a system while the
/// simulation reads paths.
#[derive(Clone)]
pub struct SharedNav(pub Arc<RwLock<NavMesh>>);

impl SharedNav {
    pub fn new(mesh: NavMesh) -> Self {
        Self(Arc::new(RwLock::new(mesh)))
    }

    pub fn path(&self, from: Vec3, to: Vec3) -> Option<Path> {
        self.0.read().ok()?.find_path(from, to)
    }

    pub fn path_length(&self, from: Vec3, to: Vec3) -> Option<f32> {
        self.path(from, to).map(|p| p.length)
    }

    /// Does the mesh know the ground near both points? When not, "no path" means "unknown"
    /// and callers should fall back to straight movement.
    pub fn covers(&self, a: Vec3, b: Vec3) -> bool {
        self.0.read().is_ok_and(|m| {
            m.nearest_cell(a.x, a.z, 4).is_some() && m.nearest_cell(b.x, b.z, 4).is_some()
        })
    }

    pub fn height_at(&self, x: f32, z: f32) -> Option<f32> {
        self.0.read().ok()?.height_at(x, z)
    }
}

impl std::fmt::Debug for SharedNav {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0.read() {
            Ok(m) => write!(f, "SharedNav({} cells)", m.cell_count()),
            Err(_) => write!(f, "SharedNav(poisoned)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gothic_physics::Tri;

    fn floor(half: f32) -> Vec<Tri> {
        let a = Vec3::new(-half, 0.0, -half);
        let b = Vec3::new(half, 0.0, -half);
        let c = Vec3::new(half, 0.0, half);
        let d = Vec3::new(-half, 0.0, half);
        vec![[a, c, b], [a, d, c]]
    }

    /// Vertical wall on the plane `x = x0` from z0 to z1, 300 cm high.
    fn wall_x(x0: f32, z0: f32, z1: f32) -> Vec<Tri> {
        let a = Vec3::new(x0, 0.0, z0);
        let b = Vec3::new(x0, 300.0, z0);
        let c = Vec3::new(x0, 300.0, z1);
        let d = Vec3::new(x0, 0.0, z1);
        vec![[a, b, c], [a, c, d]]
    }

    fn baked(world: &CollisionWorld, radius: f32) -> NavMesh {
        baked_at(world, Vec3::new(-500.0, 0.0, 0.0), radius)
    }

    fn baked_at(world: &CollisionWorld, seed: Vec3, radius: f32) -> NavMesh {
        let mut nav = NavMesh::new(NavParams::default());
        nav.bake_disc(world, seed, radius);
        nav
    }

    #[test]
    fn straight_path_on_open_ground_is_two_points() {
        let mut w = CollisionWorld::new();
        w.insert_chunk(1, floor(3000.0));
        let nav = baked(&w, 1500.0);
        let p = nav
            .find_path(Vec3::new(-800.0, 0.0, 0.0), Vec3::new(900.0, 0.0, 300.0))
            .unwrap();
        assert!(p.points.len() <= 3, "{} points", p.points.len());
        let straight = ((1700.0f32).powi(2) + 300.0f32.powi(2)).sqrt();
        assert!(
            (p.length - straight).abs() < 25.0,
            "{} vs {straight}",
            p.length
        );
    }

    #[test]
    fn path_goes_around_a_wall_through_the_gap() {
        let mut w = CollisionWorld::new();
        w.insert_chunk(1, floor(3000.0));
        // Wall at x = 0 from z = -1500 to z = 600; gap above z = 600.
        w.insert_chunk(2, wall_x(0.0, -1500.0, 600.0));
        let nav = baked(&w, 2200.0);
        let p = nav
            .find_path(Vec3::new(-500.0, 0.0, 0.0), Vec3::new(500.0, 0.0, 0.0))
            .expect("path");
        assert!(p.length > 1000.0 + 600.0, "detour {}", p.length);
        // Passes the wall end with agent clearance.
        let crossing = p
            .points
            .windows(2)
            .find(|w| w[0].x < 0.0 && w[1].x >= 0.0)
            .expect("crosses x=0");
        let z = crossing[0].z
            + (crossing[1].z - crossing[0].z) * (-crossing[0].x / (crossing[1].x - crossing[0].x));
        assert!(z > 600.0 + 30.0, "crossing at z={z}");
        // Every segment is walkable.
        for s in p.points.windows(2) {
            assert!(nav.line_walkable(s[0], s[1]));
        }
    }

    #[test]
    fn closed_room_is_unreachable() {
        let mut w = CollisionWorld::new();
        w.insert_chunk(1, floor(3000.0));
        w.insert_chunk(2, wall_x(0.0, -3000.0, 3000.0));
        let nav = baked(&w, 1800.0);
        assert!(
            nav.find_path(Vec3::new(-500.0, 0.0, 0.0), Vec3::new(500.0, 0.0, 0.0))
                .is_none()
        );
    }

    #[test]
    fn doorway_of_120cm_is_passable() {
        let mut w = CollisionWorld::new();
        w.insert_chunk(1, floor(3000.0));
        w.insert_chunk(2, wall_x(0.0, -3000.0, -60.0));
        w.insert_chunk(3, wall_x(0.0, 60.0, 3000.0));
        let nav = baked(&w, 1800.0);
        let p = nav.find_path(Vec3::new(-500.0, 0.0, 0.0), Vec3::new(500.0, 0.0, 0.0));
        assert!(p.is_some_and(|p| (p.length - 1000.0).abs() < 60.0));
    }

    #[test]
    fn steep_ground_is_not_walkable_but_gentle_is() {
        let mut w = CollisionWorld::new();
        w.insert_chunk(1, floor(3000.0));
        // 30 degree ramp east of x = 500 rising to a plateau, 60 degree cliff west.
        let ramp = |deg: f32, sign: f32| -> Vec<Tri> {
            let s = deg.to_radians().tan();
            let h = 300.0 * s;
            let x0 = 500.0 * sign;
            let x1 = (500.0 + 300.0) * sign;
            let p = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
            vec![
                [p(x0, 0.0, -400.0), p(x1, h, 400.0), p(x1, h, -400.0)],
                [p(x0, 0.0, -400.0), p(x0, 0.0, 400.0), p(x1, h, 400.0)],
            ]
        };
        w.insert_chunk(2, ramp(30.0, 1.0));
        w.insert_chunk(3, ramp(60.0, -1.0));
        let nav = baked_at(&w, Vec3::new(10.0, 0.0, 10.0), 1500.0);
        assert!(nav.is_walkable(650.0, 0.0), "30 degree ramp walkable");
        assert!(!nav.is_walkable(-560.0, 0.0), "60 degree ramp blocked");
    }

    #[test]
    fn follower_reaches_goal_and_avoidance_sidesteps() {
        let mut w = CollisionWorld::new();
        w.insert_chunk(1, floor(3000.0));
        let nav = baked(&w, 1500.0);
        let from = Vec3::new(-600.0, 0.0, 0.0);
        let to = Vec3::new(600.0, 0.0, 200.0);
        let path = nav.find_path(from, to).unwrap();
        let mut f = PathFollower::new(path, to);
        let mut pos = from;
        for _ in 0..100 {
            let s = f.advance(&nav, pos, 40.0);
            pos = s.position;
            if s.arrived {
                break;
            }
        }
        assert!(pos.distance(to) < 5.0);

        // A neighbour dead ahead: avoidance turns away from the line.
        let d = avoid(
            Vec2::ZERO,
            Vec2::X,
            150.0,
            35.0,
            &[(Vec2::new(150.0, 0.0), 35.0, Vec2::ZERO)],
            2.0,
        );
        assert!(d.y.abs() > 0.2, "{d:?}");
        // No neighbour: unchanged.
        assert_eq!(avoid(Vec2::ZERO, Vec2::X, 150.0, 35.0, &[], 2.0), Vec2::X);
    }
}
