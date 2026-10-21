//! The original navigation graph: authored zones (walkable polygons) joined by authored
//! paths (corridors), searched with a best-first search over the corridor connection points.
//!
//! Port of the data model and queries of `gCNavZone_PS`, `gCNavPath_PS` and `gCNavigationMap`
//! (Game.dll): point-in-zone / point-on-path tests with the vertical tolerance band, zone
//! and path cost functions, net (connected component) indices, `GetZone` and `FindWay`.
//!
//! Search model [V structure, I weights]: the nodes are the points where a corridor meets
//! its two zones. Moving inside a zone costs the straight distance (`gCNavZone_PS::GetCosts`),
//! moving along a corridor costs the arc length between the two points (`gCNavPath_PS::GetCosts`).
//! The route found is then expanded into waypoints; inside a concave zone a leg that would
//! leave the polygon is routed around its corners (the original does this with evade points,
//! `gCNavZone_PS::GetEvade`).
use crate::scene::{EntityId, NavScene, transform};
use glam::{Vec2, Vec3};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// `gCNavigationMap::FindWay`: "DEADLOCK PREVENTION (3 sec.)", the clock is read every 1000
/// expanded nodes.
pub const DEADLOCK_PREVENTION: Duration = Duration::from_secs(3);
const DEADLOCK_CHECK_EVERY: u32 = 1000;
/// Corner offset used when a leg is routed around a concave zone, cm.
const CORNER_INSET: f32 = 45.0;
const GRID_CELL: f32 = 1500.0;
/// How far outside a zone a corridor end may lie and still belong to it, cm.
const END_SLACK: f32 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AreaId {
    Zone(u32),
    Path(u32),
}

/// Why `FindWay` gave up. `code` is the value the original stores in the navigation property
/// set (`gCNavigation_PS+0x21c`) where it is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindWayError {
    /// Start point is in no zone or path (code 7).
    InvalidStartZone,
    /// Destination point is in no zone or path.
    InvalidDestinationZone,
    /// Start and destination lie in different nets (code 8).
    DifferentNets,
    /// Same net but the search ran out of nodes.
    NoWay,
    /// The search took longer than [`DEADLOCK_PREVENTION`].
    Deadlock,
}

impl FindWayError {
    pub fn code(self) -> u32 {
        match self {
            Self::InvalidStartZone => 7,
            Self::DifferentNets => 8,
            Self::InvalidDestinationZone => 9,
            Self::NoWay => 10,
            Self::Deadlock => 11,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct NavRoute {
    pub points: Vec<Vec3>,
    /// Horizontal length, cm.
    pub length: f32,
    /// Zones and corridors the route passes, in order.
    pub areas: Vec<AreaId>,
}

#[derive(Debug)]
pub struct Zone {
    pub id: EntityId,
    pub name: String,
    pts: Vec<Vec2>,
    ys: Vec<f32>,
    pub min_y: f32,
    pub max_y: f32,
    pub top_tolerance: f32,
    pub bottom_tolerance: f32,
    lo: Vec2,
    hi: Vec2,
    pub net: u32,
    conns: Vec<u32>,
    corners: OnceLock<Corners>,
}

#[derive(Debug)]
struct Corners {
    pos: Vec<Vec2>,
    y: Vec<f32>,
    adj: Vec<Vec<(u16, f32)>>,
}

#[derive(Debug)]
pub struct Corridor {
    pub id: EntityId,
    pub name: String,
    pts: Vec<Vec3>,
    radii: Vec<f32>,
    /// Arc length at each vertex.
    cum: Vec<f32>,
    pub unlimited_height: bool,
    pub zone_a: u32,
    pub zone_b: u32,
    pub min_y: f32,
    pub max_y: f32,
    pub top_tolerance: f32,
    pub bottom_tolerance: f32,
    lo: Vec2,
    hi: Vec2,
    a_conn: u32,
    b_conn: u32,
}

#[derive(Debug, Clone, Copy)]
struct Conn {
    pos: Vec3,
    path: u32,
    zone: u32,
    /// Arc position on the corridor.
    arc: f32,
}

/// The navigation graph of a loaded world.
#[derive(Debug, Default)]
pub struct NavGraph {
    zones: Vec<Zone>,
    paths: Vec<Corridor>,
    conns: Vec<Conn>,
    grid: HashMap<(i32, i32), Vec<AreaId>>,
    by_id: HashMap<EntityId, AreaId>,
    /// Paths dropped because a zone id was not found.
    pub dangling_paths: usize,
}

/// 2D crossing-number test. The original sums the signed angles to the polygon vertices
/// (`Test3DPointInZone`); both give the same answer for simple polygons.
pub fn point_in_polygon(p: Vec2, poly: &[Vec2]) -> bool {
    let mut inside = false;
    let mut j = poly.len().wrapping_sub(1);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn dist_to_segment(p: Vec2, a: Vec2, b: Vec2) -> (f32, f32) {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 < 1.0e-6 {
        0.0
    } else {
        ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
    };
    ((a + ab * t - p).length(), t)
}

fn dist_to_polygon_edge(p: Vec2, poly: &[Vec2]) -> f32 {
    let mut best = f32::MAX;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        best = best.min(dist_to_segment(p, poly[j], poly[i]).0);
        j = i;
    }
    best
}

impl Zone {
    fn contains_xz(&self, p: Vec2) -> bool {
        p.x >= self.lo.x
            && p.x <= self.hi.x
            && p.y >= self.lo.y
            && p.y <= self.hi.y
            && point_in_polygon(p, &self.pts)
    }

    /// Height of the ceiling of the zone volume (`GetZoneTopHeight`): highest vertex plus the
    /// top tolerance.
    pub fn top(&self) -> f32 {
        self.max_y + self.top_tolerance
    }

    /// Floor of the zone volume (`GetZoneBottomHeight`).
    pub fn bottom(&self) -> f32 {
        self.min_y - self.bottom_tolerance
    }

    /// `Test3DPointInZone` with the vertical band.
    pub fn contains(&self, p: Vec3) -> bool {
        self.contains_xz(Vec2::new(p.x, p.z)) && p.y <= self.top() && p.y >= self.bottom()
    }

    /// Outline in the host plane (x, z).
    pub fn outline(&self) -> &[Vec2] {
        &self.pts
    }

    fn vertical_gap(&self, y: f32) -> f32 {
        (y - self.top()).max(self.bottom() - y).max(0.0)
    }

    /// Is the straight leg between two points inside the polygon? A leg is inside when it
    /// crosses no polygon edge (touching within a couple of centimetres does not count: corridor
    /// connection points sit on the boundary) and its middle is inside.
    fn leg_inside(&self, a: Vec2, b: Vec2) -> bool {
        const EPS: f32 = 2.0;
        let ab = b - a;
        let len = ab.length();
        if len < 1.0e-3 {
            return true;
        }
        let side =
            |p: Vec2, q: Vec2, r: Vec2| (q - p).perp_dot(r - p) / (q - p).length().max(1.0e-6);
        let mut j = self.pts.len() - 1;
        for i in 0..self.pts.len() {
            let (c, d) = (self.pts[j], self.pts[i]);
            j = i;
            let (s1, s2) = (side(a, b, c), side(a, b, d));
            if (s1 > EPS && s2 < -EPS) || (s1 < -EPS && s2 > EPS) {
                let (s3, s4) = (side(c, d, a), side(c, d, b));
                if (s3 > EPS && s4 < -EPS) || (s3 < -EPS && s4 > EPS) {
                    return false;
                }
            }
        }
        let mid = (a + b) * 0.5;
        point_in_polygon(mid, &self.pts) || dist_to_polygon_edge(mid, &self.pts) < 5.0
    }

    fn corners(&self) -> &Corners {
        self.corners.get_or_init(|| {
            let n = self.pts.len();
            let mut pos = Vec::with_capacity(n);
            let mut y = Vec::with_capacity(n);
            for i in 0..n {
                let prev = self.pts[(i + n - 1) % n];
                let next = self.pts[(i + 1) % n];
                let v = self.pts[i];
                let d1 = (v - prev).normalize_or_zero();
                let d2 = (next - v).normalize_or_zero();
                let bis = (d2 - d1).normalize_or_zero();
                let mut p = v + bis * CORNER_INSET;
                if !point_in_polygon(p, &self.pts) {
                    p = v - bis * CORNER_INSET;
                }
                if !point_in_polygon(p, &self.pts) {
                    p = v;
                }
                pos.push(p);
                y.push(self.ys[i]);
            }
            let mut adj = vec![Vec::new(); n];
            for i in 0..n {
                for j in (i + 1)..n {
                    if self.leg_inside(pos[i], pos[j]) {
                        let d = pos[i].distance(pos[j]);
                        adj[i].push((j as u16, d));
                        adj[j].push((i as u16, d));
                    }
                }
            }
            Corners { pos, y, adj }
        })
    }

    /// Waypoints between two points inside the zone (both included). A straight leg when it
    /// stays inside the polygon, otherwise the shortest way around the corners.
    fn route(&self, a: Vec3, b: Vec3) -> Vec<Vec3> {
        let (pa, pb) = (Vec2::new(a.x, a.z), Vec2::new(b.x, b.z));
        if self.leg_inside(pa, pb) {
            return vec![a, b];
        }
        let c = self.corners();
        let n = c.pos.len();
        let seen_a: Vec<usize> = (0..n).filter(|&i| self.leg_inside(pa, c.pos[i])).collect();
        let seen_b: Vec<usize> = (0..n).filter(|&i| self.leg_inside(pb, c.pos[i])).collect();
        if seen_a.is_empty() || seen_b.is_empty() {
            return vec![a, b];
        }
        // Dijkstra over the corners; the best entry corner is found by seeding all of them.
        let mut dist = vec![f32::MAX; n];
        let mut prev = vec![usize::MAX; n];
        let mut heap = BinaryHeap::new();
        for &i in &seen_a {
            dist[i] = pa.distance(c.pos[i]);
            heap.push(Item(dist[i], i as u32));
        }
        while let Some(Item(d, u)) = heap.pop() {
            let u = u as usize;
            if d > dist[u] {
                continue;
            }
            for &(v, w) in &c.adj[u] {
                let v = v as usize;
                let nd = d + w;
                if nd < dist[v] {
                    dist[v] = nd;
                    prev[v] = u;
                    heap.push(Item(nd, v as u32));
                }
            }
        }
        let Some((&last, _)) = seen_b
            .iter()
            .map(|i| (i, dist[*i] + c.pos[*i].distance(pb)))
            .filter(|(_, d)| *d < f32::MAX / 2.0)
            .min_by(|x, y| x.1.total_cmp(&y.1))
        else {
            return vec![a, b];
        };
        let mut chain = vec![last];
        while prev[*chain.last().unwrap_or(&last)] != usize::MAX {
            chain.push(prev[*chain.last().unwrap_or(&last)]);
        }
        chain.reverse();
        let mut out = vec![a];
        out.extend(
            chain
                .iter()
                .map(|&i| Vec3::new(c.pos[i].x, c.y[i], c.pos[i].y)),
        );
        out.push(b);
        out
    }
}

/// Min-heap item.
#[derive(Debug, Clone, Copy)]
struct Item(f32, u32);
impl PartialEq for Item {
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0
    }
}
impl Eq for Item {}
impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Item {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.total_cmp(&self.0)
    }
}

impl Corridor {
    pub fn len(&self) -> f32 {
        self.cum.last().copied().unwrap_or(0.0)
    }

    pub fn is_empty(&self) -> bool {
        self.pts.len() < 2
    }

    /// Nearest arc position of the polyline to a point, and the horizontal distance to it.
    fn project(&self, p: Vec3) -> (f32, f32) {
        let q = Vec2::new(p.x, p.z);
        let mut best = (f32::MAX, 0.0);
        for i in 0..self.pts.len().saturating_sub(1) {
            let (a, b) = (self.pts[i], self.pts[i + 1]);
            let (d, t) = dist_to_segment(q, Vec2::new(a.x, a.z), Vec2::new(b.x, b.z));
            if d < best.0 {
                let seg = self.cum[i + 1] - self.cum[i];
                best = (d, self.cum[i] + seg * t);
            }
        }
        (best.1, best.0)
    }

    /// Arc position where the corridor leaves `zone`, walking from its start (`from_start`) or
    /// its end. An end that is not inside the zone is its own crossing.
    fn exit_arc(&self, zone: &Zone, from_start: bool) -> f32 {
        let len = self.len();
        let at = |t: f32| if from_start { t } else { len - t };
        let inside = |t: f32| {
            let p = self.at(at(t));
            zone.contains_xz(Vec2::new(p.x, p.z))
        };
        if !inside(0.0) {
            return at(0.0);
        }
        let mut t = 0.0;
        while t < len {
            let next = (t + 20.0).min(len);
            if !inside(next) {
                return at(t);
            }
            t = next;
        }
        at(len)
    }

    /// Point of the corridor at an arc position.
    pub fn at(&self, arc: f32) -> Vec3 {
        let arc = arc.clamp(0.0, self.len());
        let i = self
            .cum
            .partition_point(|&c| c <= arc)
            .saturating_sub(1)
            .min(self.pts.len() - 2);
        let seg = (self.cum[i + 1] - self.cum[i]).max(1.0e-4);
        self.pts[i].lerp(self.pts[i + 1], (arc - self.cum[i]) / seg)
    }

    /// `Test3DPointOnPath`: inside the radius tube of a segment and inside the vertical band.
    pub fn contains(&self, p: Vec3) -> bool {
        self.band_gap(p.y) <= 0.0 && self.horizontal_gap(p) <= 0.0
    }

    fn horizontal_gap(&self, p: Vec3) -> f32 {
        let q = Vec2::new(p.x, p.z);
        if q.x < self.lo.x || q.x > self.hi.x || q.y < self.lo.y || q.y > self.hi.y {
            return f32::MAX;
        }
        let mut best = f32::MAX;
        for i in 0..self.pts.len().saturating_sub(1) {
            let (a, b) = (self.pts[i], self.pts[i + 1]);
            let (d, t) = dist_to_segment(q, Vec2::new(a.x, a.z), Vec2::new(b.x, b.z));
            let r = self.radii[i] * (1.0 - t) + self.radii[i + 1] * t;
            best = best.min(d - r);
        }
        best
    }

    /// `CalcAbsHeightDiffFlagAndPrioFlag`: how far the height is outside the band (0 inside).
    fn band_gap(&self, y: f32) -> f32 {
        if self.unlimited_height {
            return 0.0;
        }
        let top = self.max_y + self.top_tolerance;
        let bottom = self.min_y - self.bottom_tolerance;
        (y - top).max(bottom - y).max(0.0)
    }
}

impl NavGraph {
    /// Build the graph. `map` converts a point from the document (Gothic) space to the host
    /// space; it must be an isometry (rotation, translation, mirror), the same one the host
    /// applies to character positions.
    pub fn build(scene: &NavScene, map: &dyn Fn(Vec3) -> Vec3) -> Self {
        let mut g = NavGraph::default();
        for z in &scene.zones {
            if z.points.len() < 3 {
                continue;
            }
            let world: Vec<Vec3> = z
                .points
                .iter()
                .map(|p| map(transform(&z.matrix, *p)))
                .collect();
            let pts: Vec<Vec2> = world.iter().map(|p| Vec2::new(p.x, p.z)).collect();
            let ys: Vec<f32> = world.iter().map(|p| p.y).collect();
            let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
            for p in &pts {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
            let idx = g.zones.len() as u32;
            g.by_id.insert(z.id, AreaId::Zone(idx));
            g.zones.push(Zone {
                id: z.id,
                name: z.name.clone(),
                min_y: ys.iter().copied().fold(f32::MAX, f32::min),
                max_y: ys.iter().copied().fold(f32::MIN, f32::max),
                pts,
                ys,
                top_tolerance: z.top_tolerance,
                bottom_tolerance: z.bottom_tolerance,
                lo,
                hi,
                net: 0,
                conns: Vec::new(),
                corners: OnceLock::new(),
            });
        }
        g.index();
        for p in &scene.paths {
            if p.points.len() < 2 {
                continue;
            }
            let pts: Vec<Vec3> = p
                .points
                .iter()
                .map(|q| map(transform(&p.matrix, *q)))
                .collect();
            // The authored zone ids are often stale or unset (the engine relinks while
            // compiling the navigation scene); fall back to the zones containing the ends.
            let by_id = |id: Option<EntityId>| {
                id.and_then(|i| match g.by_id.get(&i) {
                    Some(AreaId::Zone(z)) => Some(*z),
                    _ => None,
                })
            };
            let (ida, idb) = (by_id(p.zone_a), by_id(p.zone_b));
            let first = pts[0];
            let last = pts[pts.len() - 1];
            let (Some(za), Some(zb)) = (
                ida.or_else(|| g.zone_near(first, END_SLACK)),
                idb.or_else(|| g.zone_near(last, END_SLACK)),
            ) else {
                g.dangling_paths += 1;
                continue;
            };
            if za == zb {
                g.dangling_paths += 1;
                continue;
            }
            let radii: Vec<f32> = (0..pts.len())
                .map(|i| p.radii.get(i).or(p.radii.last()).copied().unwrap_or(20.0))
                .collect();
            let mut cum = vec![0.0];
            for w in pts.windows(2) {
                let l = Vec2::new(w[1].x - w[0].x, w[1].z - w[0].z).length();
                cum.push(cum.last().copied().unwrap_or(0.0) + l);
            }
            let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
            let rmax = radii.iter().copied().fold(0.0, f32::max);
            for q in &pts {
                lo = lo.min(Vec2::new(q.x, q.z) - Vec2::splat(rmax));
                hi = hi.max(Vec2::new(q.x, q.z) + Vec2::splat(rmax));
            }
            let idx = g.paths.len() as u32;
            let mut corridor = Corridor {
                id: p.id,
                name: p.name.clone(),
                min_y: pts.iter().map(|q| q.y).fold(f32::MAX, f32::min),
                max_y: pts.iter().map(|q| q.y).fold(f32::MIN, f32::max),
                pts,
                radii,
                cum,
                unlimited_height: p.unlimited_height,
                zone_a: za,
                zone_b: zb,
                top_tolerance: p.top_tolerance,
                bottom_tolerance: p.bottom_tolerance,
                lo,
                hi,
                a_conn: 0,
                b_conn: 0,
            };
            // Connection points: the authored intersection centres when the authored zone id
            // was valid, else where the corridor leaves the zone. Snapped onto the corridor so
            // that costs along it are exact.
            let arc_a = match ida {
                Some(_) if p.a_center != [0.0; 3] => {
                    corridor.project(map(transform(&p.matrix, p.a_center))).0
                }
                _ => corridor.exit_arc(&g.zones[za as usize], true),
            };
            let arc_b = match idb {
                Some(_) if p.b_center != [0.0; 3] => {
                    corridor.project(map(transform(&p.matrix, p.b_center))).0
                }
                _ => corridor.exit_arc(&g.zones[zb as usize], false),
            };
            corridor.a_conn = g.conns.len() as u32;
            g.conns.push(Conn {
                pos: corridor.at(arc_a),
                path: idx,
                zone: za,
                arc: arc_a,
            });
            corridor.b_conn = g.conns.len() as u32;
            g.conns.push(Conn {
                pos: corridor.at(arc_b),
                path: idx,
                zone: zb,
                arc: arc_b,
            });
            g.zones[za as usize].conns.push(corridor.a_conn);
            g.zones[zb as usize].conns.push(corridor.b_conn);
            g.by_id.insert(p.id, AreaId::Path(idx));
            g.paths.push(corridor);
        }
        g.assign_nets();
        g.index();
        g
    }

    /// `DetectNetIndices`: connected components of zones joined by corridors.
    fn assign_nets(&mut self) {
        let n = self.zones.len();
        let mut parent: Vec<usize> = (0..n).collect();
        fn find(parent: &mut [usize], mut x: usize) -> usize {
            while parent[x] != x {
                parent[x] = parent[parent[x]];
                x = parent[x];
            }
            x
        }
        for p in &self.paths {
            let (a, b) = (
                find(&mut parent, p.zone_a as usize),
                find(&mut parent, p.zone_b as usize),
            );
            if a != b {
                parent[a] = b;
            }
        }
        let mut ids: HashMap<usize, u32> = HashMap::new();
        for i in 0..n {
            let r = find(&mut parent, i);
            let next = ids.len() as u32;
            self.zones[i].net = *ids.entry(r).or_insert(next);
        }
    }

    fn index(&mut self) {
        let cell = |v: f32| (v / GRID_CELL).floor() as i32;
        let put = |lo: Vec2, hi: Vec2, id: AreaId, grid: &mut HashMap<(i32, i32), Vec<AreaId>>| {
            for x in cell(lo.x)..=cell(hi.x) {
                for z in cell(lo.y)..=cell(hi.y) {
                    grid.entry((x, z)).or_default().push(id);
                }
            }
        };
        let mut grid = HashMap::new();
        for (i, z) in self.zones.iter().enumerate() {
            put(z.lo, z.hi, AreaId::Zone(i as u32), &mut grid);
        }
        for (i, p) in self.paths.iter().enumerate() {
            put(p.lo, p.hi, AreaId::Path(i as u32), &mut grid);
        }
        self.grid = grid;
    }

    /// Build the corner visibility graphs of concave zones now instead of on first use.
    pub fn warm(&self) {
        for z in &self.zones {
            if z.pts.len() > 4 {
                z.corners();
            }
        }
    }

    pub fn zone_count(&self) -> usize {
        self.zones.len()
    }

    pub fn path_count(&self) -> usize {
        self.paths.len()
    }

    pub fn zone(&self, i: u32) -> Option<&Zone> {
        self.zones.get(i as usize)
    }

    pub fn corridor(&self, i: u32) -> Option<&Corridor> {
        self.paths.get(i as usize)
    }

    /// Area of an authored entity id (`LastUseableNavigationZoneID`, `ZoneAEntityID`, ...).
    pub fn area_of_entity(&self, id: &EntityId) -> Option<AreaId> {
        self.by_id.get(id).copied()
    }

    pub fn net_of(&self, a: AreaId) -> Option<u32> {
        match a {
            AreaId::Zone(i) => self.zones.get(i as usize).map(|z| z.net),
            AreaId::Path(i) => self
                .paths
                .get(i as usize)
                .map(|p| self.zones[p.zone_a as usize].net),
        }
    }

    /// `GetZone`: the area a point belongs to. A zone containing the point wins over a
    /// corridor; among candidates the one whose vertical band is closest wins.
    pub fn locate(&self, p: Vec3) -> Option<AreaId> {
        let key = (
            (p.x / GRID_CELL).floor() as i32,
            (p.z / GRID_CELL).floor() as i32,
        );
        let cands = self.grid.get(&key)?;
        let mut best: Option<(f32, u8, AreaId)> = None;
        for &a in cands {
            let (rank, gap) = match a {
                AreaId::Zone(i) => {
                    let z = &self.zones[i as usize];
                    if !z.contains(p) {
                        continue;
                    }
                    // Closer to the zone's mean height first.
                    (0u8, (p.y - (z.min_y + z.max_y) * 0.5).abs())
                }
                AreaId::Path(i) => {
                    let c = &self.paths[i as usize];
                    if !c.contains(p) {
                        continue;
                    }
                    (1u8, c.band_gap(p.y))
                }
            };
            if best.is_none_or(|(g, r, _)| (rank, gap) < (r, g)) {
                best = Some((gap, rank, a));
            }
        }
        best.map(|b| b.2)
    }

    /// Like [`NavGraph::locate`] but a point within `slack` cm outside a zone counts as in it
    /// (characters standing on a polygon edge, interaction points at walls).
    pub fn locate_near(&self, p: Vec3, slack: f32) -> Option<AreaId> {
        if let Some(a) = self.locate(p) {
            return Some(a);
        }
        let key = (
            (p.x / GRID_CELL).floor() as i32,
            (p.z / GRID_CELL).floor() as i32,
        );
        let q = Vec2::new(p.x, p.z);
        let mut best: Option<(f32, AreaId)> = None;
        for dx in -1..=1 {
            for dz in -1..=1 {
                let Some(list) = self.grid.get(&(key.0 + dx, key.1 + dz)) else {
                    continue;
                };
                for &a in list {
                    let d = match a {
                        AreaId::Zone(i) => {
                            let z = &self.zones[i as usize];
                            if z.vertical_gap(p.y) > 0.0 {
                                continue;
                            }
                            dist_to_polygon_edge(q, &z.pts)
                        }
                        AreaId::Path(i) => {
                            let c = &self.paths[i as usize];
                            if c.band_gap(p.y) > 0.0 {
                                continue;
                            }
                            c.horizontal_gap(p).max(0.0)
                        }
                    };
                    if d <= slack && best.is_none_or(|(b, _)| d < b) {
                        best = Some((d, a));
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    /// Zone containing the point, or within `slack` of its edge.
    fn zone_near(&self, p: Vec3, slack: f32) -> Option<u32> {
        let key = (
            (p.x / GRID_CELL).floor() as i32,
            (p.z / GRID_CELL).floor() as i32,
        );
        let q = Vec2::new(p.x, p.z);
        let mut best: Option<(f32, u32)> = None;
        for dx in -1..=1 {
            for dz in -1..=1 {
                for a in self
                    .grid
                    .get(&(key.0 + dx, key.1 + dz))
                    .into_iter()
                    .flatten()
                {
                    let AreaId::Zone(i) = *a else { continue };
                    let z = &self.zones[i as usize];
                    if z.vertical_gap(p.y) > 0.0 {
                        continue;
                    }
                    let d = if point_in_polygon(q, &z.pts) {
                        0.0
                    } else {
                        dist_to_polygon_edge(q, &z.pts)
                    };
                    if d <= slack && best.is_none_or(|(b, _)| d < b) {
                        best = Some((d, i));
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    fn area_net(&self, a: AreaId) -> u32 {
        self.net_of(a).unwrap_or(u32::MAX)
    }

    /// `gCNavigationMap::FindWay`: route between two points with the default deadlock limit.
    pub fn find_way(&self, from: Vec3, to: Vec3) -> Result<NavRoute, FindWayError> {
        self.find_way_with(from, to, DEADLOCK_PREVENTION, 1.0)
    }

    /// `find_way` with an explicit time limit and heuristic weight (1.0 is exact).
    pub fn find_way_with(
        &self,
        from: Vec3,
        to: Vec3,
        limit: Duration,
        weight: f32,
    ) -> Result<NavRoute, FindWayError> {
        const SLACK: f32 = 60.0;
        let sa = self
            .locate_near(from, SLACK)
            .ok_or(FindWayError::InvalidStartZone)?;
        let ga = self
            .locate_near(to, SLACK)
            .ok_or(FindWayError::InvalidDestinationZone)?;
        if self.area_net(sa) != self.area_net(ga) {
            return Err(FindWayError::DifferentNets);
        }
        // Direct case: both ends in the same area.
        if sa == ga {
            let pts = self.leg_in_area(sa, from, to);
            return Ok(self.finish(pts, vec![sa]));
        }
        // Nodes: connection points, then the virtual start and goal.
        let n = self.conns.len();
        let (start, goal) = (n, n + 1);
        let start_edges = self.edges_to_area(sa, from);
        let goal_edges = self.edges_to_area(ga, to);
        let flat = |a: Vec3, b: Vec3| Vec2::new(a.x - b.x, a.z - b.z).length();
        let h = |pos: Vec3| flat(pos, to) * weight;
        let mut dist = vec![f32::MAX; n + 2];
        let mut prev = vec![usize::MAX; n + 2];
        let mut closed = vec![false; n + 2];
        let mut heap = BinaryHeap::new();
        dist[start] = 0.0;
        heap.push(Item(h(from), start as u32));
        let began = Instant::now();
        let mut steps = 0u32;
        while let Some(Item(_, u)) = heap.pop() {
            let u = u as usize;
            if closed[u] {
                continue;
            }
            closed[u] = true;
            if u == goal {
                break;
            }
            steps += 1;
            if steps.is_multiple_of(DEADLOCK_CHECK_EVERY) && began.elapsed() >= limit {
                return Err(FindWayError::Deadlock);
            }
            let du = dist[u];
            let mut relax = |v: usize, w: f32, heap: &mut BinaryHeap<Item>| {
                let nd = du + w;
                if nd < dist[v] {
                    dist[v] = nd;
                    prev[v] = u;
                    let pos = if v == goal { to } else { self.conns[v].pos };
                    heap.push(Item(nd + h(pos), v as u32));
                }
            };
            if u == start {
                for &(v, w) in &start_edges {
                    relax(v, w, &mut heap);
                }
                continue;
            }
            let c = self.conns[u];
            // Along the corridor to its other end.
            let p = &self.paths[c.path as usize];
            let other = if p.a_conn as usize == u {
                p.b_conn
            } else {
                p.a_conn
            } as usize;
            relax(other, (self.conns[other].arc - c.arc).abs(), &mut heap);
            // Inside the zone to every other connection point of it.
            for &v in &self.zones[c.zone as usize].conns {
                if v as usize != u {
                    relax(
                        v as usize,
                        c.pos.distance(self.conns[v as usize].pos),
                        &mut heap,
                    );
                }
            }
            for &(v, w) in &goal_edges {
                if v == u {
                    relax(goal, w, &mut heap);
                }
            }
        }
        if dist[goal] == f32::MAX {
            return Err(FindWayError::NoWay);
        }
        let mut chain = vec![goal];
        while *chain.last().unwrap_or(&start) != start {
            chain.push(prev[*chain.last().unwrap_or(&start)]);
        }
        chain.reverse();
        // Expand every hop into waypoints inside the area it happens in.
        let pos_of = |k: usize| match k {
            _ if k == start => from,
            _ if k == goal => to,
            _ => self.conns[k].pos,
        };
        let mut pts = vec![from];
        let mut areas: Vec<AreaId> = Vec::new();
        for w in chain.windows(2) {
            let (x, y) = (w[0], w[1]);
            let area = if x == start {
                sa
            } else if y == goal {
                ga
            } else if self.conns[x].path == self.conns[y].path {
                AreaId::Path(self.conns[x].path)
            } else {
                AreaId::Zone(self.conns[x].zone)
            };
            if areas.last() != Some(&area) {
                areas.push(area);
            }
            let leg = self.leg_in_area(area, pos_of(x), pos_of(y));
            pts.extend(leg.into_iter().skip(1));
        }
        Ok(self.finish(pts, areas))
    }

    /// Cost edges from the virtual start / goal into the connection points of its area.
    fn edges_to_area(&self, a: AreaId, p: Vec3) -> Vec<(usize, f32)> {
        match a {
            AreaId::Zone(i) => self.zones[i as usize]
                .conns
                .iter()
                .map(|&c| (c as usize, p.distance(self.conns[c as usize].pos)))
                .collect(),
            AreaId::Path(i) => {
                let c = &self.paths[i as usize];
                let (arc, _) = c.project(p);
                [c.a_conn, c.b_conn]
                    .iter()
                    .map(|&k| (k as usize, (self.conns[k as usize].arc - arc).abs()))
                    .collect()
            }
        }
    }

    /// Waypoints of a straight move inside one area.
    fn leg_in_area(&self, a: AreaId, from: Vec3, to: Vec3) -> Vec<Vec3> {
        match a {
            AreaId::Zone(i) => self.zones[i as usize].route(from, to),
            AreaId::Path(i) => {
                let c = &self.paths[i as usize];
                let (a0, _) = c.project(from);
                let (a1, _) = c.project(to);
                let mut v = vec![from];
                let (lo, hi) = (a0.min(a1), a0.max(a1));
                let mut mids: Vec<Vec3> = c
                    .pts
                    .iter()
                    .enumerate()
                    .filter(|(k, _)| c.cum[*k] > lo && c.cum[*k] < hi)
                    .map(|(_, p)| *p)
                    .collect();
                if a0 > a1 {
                    mids.reverse();
                }
                v.extend(mids);
                v.push(to);
                v
            }
        }
    }

    fn finish(&self, mut pts: Vec<Vec3>, areas: Vec<AreaId>) -> NavRoute {
        pts.dedup_by(|a, b| a.distance_squared(*b) < 1.0);
        let length = pts
            .windows(2)
            .map(|w| Vec2::new(w[1].x - w[0].x, w[1].z - w[0].z).length())
            .sum();
        NavRoute {
            points: pts,
            length,
            areas,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{PathRecord, ZoneRecord};

    const IDENT: [[f32; 4]; 4] = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];

    fn id(n: u8) -> EntityId {
        let mut i = [0u8; 20];
        i[0] = n;
        i
    }

    fn square(n: u8, x0: f32, z0: f32, w: f32) -> ZoneRecord {
        ZoneRecord {
            id: id(n),
            name: format!("Z{n}"),
            matrix: IDENT,
            points: vec![
                [x0, 0.0, z0],
                [x0 + w, 0.0, z0],
                [x0 + w, 0.0, z0 + w],
                [x0, 0.0, z0 + w],
            ],
            radius: w,
            ccw: true,
            top_tolerance: 150.0,
            bottom_tolerance: 100.0,
        }
    }

    fn corridor(n: u8, a: u8, b: u8, from: [f32; 3], to: [f32; 3]) -> PathRecord {
        PathRecord {
            id: id(n),
            name: format!("P{n}"),
            matrix: IDENT,
            points: vec![from, to],
            radii: vec![20.0, 20.0],
            unlimited_height: false,
            zone_a: Some(id(a)),
            zone_b: Some(id(b)),
            a_center: from,
            b_center: to,
            top_tolerance: 150.0,
            bottom_tolerance: 100.0,
        }
    }

    /// Three squares in a row joined by corridors, plus an island with no corridor.
    fn scene() -> NavScene {
        let mut s = NavScene::default();
        s.zones.push(square(1, 0.0, 0.0, 1000.0));
        s.zones.push(square(2, 1500.0, 0.0, 1000.0));
        s.zones.push(square(3, 3000.0, 0.0, 1000.0));
        s.zones.push(square(4, 0.0, 5000.0, 500.0));
        s.paths.push(corridor(
            11,
            1,
            2,
            [900.0, 0.0, 500.0],
            [1600.0, 0.0, 500.0],
        ));
        s.paths.push(corridor(
            12,
            2,
            3,
            [2400.0, 0.0, 500.0],
            [3100.0, 0.0, 500.0],
        ));
        s
    }

    fn graph() -> NavGraph {
        NavGraph::build(&scene(), &|p| p)
    }

    #[test]
    fn zone_and_path_point_tests_use_the_vertical_band() {
        let g = graph();
        assert_eq!(
            g.locate(Vec3::new(500.0, 0.0, 500.0)),
            Some(AreaId::Zone(0))
        );
        // Band is [min - 100, max + 150] around y = 0.
        assert!(g.locate(Vec3::new(500.0, 140.0, 500.0)).is_some());
        assert!(g.locate(Vec3::new(500.0, 160.0, 500.0)).is_none());
        assert!(g.locate(Vec3::new(500.0, -110.0, 500.0)).is_none());
        // On the corridor between the squares.
        assert_eq!(
            g.locate(Vec3::new(1250.0, 0.0, 510.0)),
            Some(AreaId::Path(0))
        );
        assert!(g.locate(Vec3::new(1250.0, 0.0, 600.0)).is_none());
        // Within slack of the zone edge.
        assert!(g.locate_near(Vec3::new(1020.0, 0.0, 300.0), 60.0).is_some());
    }

    #[test]
    fn nets_group_connected_zones() {
        let g = graph();
        assert_eq!(g.zone(0).unwrap().net, g.zone(2).unwrap().net);
        assert_ne!(g.zone(0).unwrap().net, g.zone(3).unwrap().net);
        let r = g.find_way(Vec3::new(100.0, 0.0, 100.0), Vec3::new(200.0, 0.0, 5200.0));
        assert_eq!(r.unwrap_err(), FindWayError::DifferentNets);
        let r = g.find_way(Vec3::new(100.0, 0.0, 100.0), Vec3::new(-5000.0, 0.0, 0.0));
        assert_eq!(r.unwrap_err(), FindWayError::InvalidDestinationZone);
        assert_eq!(FindWayError::DifferentNets.code(), 8);
        assert_eq!(FindWayError::InvalidStartZone.code(), 7);
    }

    #[test]
    fn route_crosses_corridors_in_order() {
        let g = graph();
        let r = g
            .find_way(Vec3::new(100.0, 0.0, 100.0), Vec3::new(3900.0, 0.0, 900.0))
            .unwrap();
        assert_eq!(
            r.areas,
            vec![
                AreaId::Zone(0),
                AreaId::Path(0),
                AreaId::Zone(1),
                AreaId::Path(1),
                AreaId::Zone(2)
            ]
        );
        // Corridor ends are waypoints.
        for want in [900.0, 1600.0, 2400.0, 3100.0] {
            assert!(
                r.points
                    .iter()
                    .any(|p| (p.x - want).abs() < 1.0 && (p.z - 500.0).abs() < 1.0),
                "missing corridor end at x={want}: {:?}",
                r.points
            );
        }
        let direct = Vec2::new(3800.0, 800.0).length();
        assert!(
            r.length >= direct && r.length < direct * 1.15,
            "{}",
            r.length
        );
        // And back again.
        let back = g
            .find_way(Vec3::new(3900.0, 0.0, 900.0), Vec3::new(100.0, 0.0, 100.0))
            .unwrap();
        assert!((back.length - r.length).abs() < 5.0);
    }

    #[test]
    fn same_zone_is_a_straight_line_and_concave_zones_go_around_the_corner() {
        let g = graph();
        let r = g
            .find_way(Vec3::new(100.0, 0.0, 100.0), Vec3::new(900.0, 0.0, 800.0))
            .unwrap();
        assert_eq!(r.points.len(), 2);

        // An L-shaped zone: going from one arm to the other must bend at the inner corner.
        let mut s = NavScene::default();
        s.zones.push(ZoneRecord {
            points: vec![
                [0.0, 0.0, 0.0],
                [2000.0, 0.0, 0.0],
                [2000.0, 0.0, 500.0],
                [500.0, 0.0, 500.0],
                [500.0, 0.0, 2000.0],
                [0.0, 0.0, 2000.0],
            ],
            ..square(1, 0.0, 0.0, 10.0)
        });
        let g = NavGraph::build(&s, &|p| p);
        let r = g
            .find_way(Vec3::new(1800.0, 0.0, 250.0), Vec3::new(250.0, 0.0, 1800.0))
            .unwrap();
        assert!(r.points.len() >= 3, "{:?}", r.points);
        let straight = Vec2::new(1550.0, 1550.0).length();
        assert!(r.length > straight * 1.02);
        let z = g.zone(0).unwrap();
        for w in r.points.windows(2) {
            assert!(z.leg_inside(Vec2::new(w[0].x, w[0].z), Vec2::new(w[1].x, w[1].z)));
        }
    }

    #[test]
    fn start_on_a_corridor_works() {
        let g = graph();
        let r = g
            .find_way(Vec3::new(1250.0, 0.0, 505.0), Vec3::new(2000.0, 0.0, 500.0))
            .unwrap();
        assert_eq!(r.areas.first(), Some(&AreaId::Path(0)));
        assert!(r.length > 700.0 && r.length < 900.0, "{}", r.length);
    }

    #[test]
    fn deadlock_limit_aborts() {
        let g = graph();
        let r = g.find_way_with(
            Vec3::new(100.0, 0.0, 100.0),
            Vec3::new(3900.0, 0.0, 900.0),
            Duration::ZERO,
            1.0,
        );
        // The graph is tiny: it finishes before the first 1000-node clock check.
        assert!(r.is_ok());
    }

    #[test]
    fn mapping_mirrors_the_world() {
        let g = NavGraph::build(&scene(), &|p| Vec3::new(-p.x, p.y, p.z));
        let r = g
            .find_way(
                Vec3::new(-100.0, 0.0, 100.0),
                Vec3::new(-3900.0, 0.0, 900.0),
            )
            .unwrap();
        assert_eq!(r.areas.len(), 5);
    }
}
