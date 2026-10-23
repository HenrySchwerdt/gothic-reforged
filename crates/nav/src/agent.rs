//! Per-character navigation state (`gCNavigation_PS`) and approach offsets (`gCNavOffset_PS`).
//!
//! `NavAgent` tracks which zone or corridor a character stands in (raising the enter / leave
//! notifications the original forwards to scripts as `OnEnterArea` / `OnLeaveArea`), remembers
//! the last position that was navigable (`LastUseableNavigationPosition` and zone id, used
//! when a character is found outside every area), and follows a route of waypoints with a
//! stuck watchdog.
use crate::graph::{AreaId, NavGraph};
use crate::scene::{Matrix, OffsetRecord, transform};
use glam::{Quat, Vec2, Vec3};

/// Seconds without progress before the route is recomputed [I: the original's only timing
/// constant in the search is the 3 s deadlock limit; the movement watchdog is this port's].
pub const STUCK_SECONDS: f32 = 3.0;
/// Progress smaller than this counts as standing still, cm.
pub const STUCK_DISTANCE: f32 = 20.0;
/// Re-plans before the character is put back on its last navigable position.
pub const MAX_REPLANS: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavEvent {
    EnterArea(AreaId),
    LeaveArea(AreaId),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StuckAction {
    /// Keep walking.
    None,
    /// Compute the route again from the current position.
    Replan,
    /// Replanning did not help: put the character here (last useable navigation position).
    Reset(Vec3),
}

#[derive(Debug, Clone, Default)]
pub struct NavAgent {
    /// Area the character is in now.
    pub area: Option<AreaId>,
    /// Last position that was inside an area, with that area.
    pub last_useable: Option<(Vec3, AreaId)>,
    route: Vec<Vec3>,
    next: usize,
    goal: Option<Vec3>,
    idle: f32,
    anchor: Vec3,
    replans: u32,
}

impl NavAgent {
    /// Seed the last useable position from authored data (`LastUseableNavigationPosition`).
    pub fn with_last_useable(pos: Vec3, area: AreaId) -> Self {
        Self {
            last_useable: Some((pos, area)),
            area: Some(area),
            ..Self::default()
        }
    }

    /// `OnEnterProcessingRange` / per-frame zone tracking. Returns the area transitions.
    pub fn track(&mut self, graph: &NavGraph, pos: Vec3) -> Vec<NavEvent> {
        let now = graph.locate_near(pos, 30.0);
        let mut events = Vec::new();
        if now != self.area {
            if let Some(old) = self.area {
                events.push(NavEvent::LeaveArea(old));
            }
            if let Some(new) = now {
                events.push(NavEvent::EnterArea(new));
            }
            self.area = now;
        }
        if let Some(a) = now {
            self.last_useable = Some((pos, a));
        }
        events
    }

    /// Start following a route (waypoints from [`NavGraph::find_way`] or the grid).
    pub fn begin(&mut self, route: Vec<Vec3>, goal: Vec3) {
        self.next = usize::from(route.len() > 1);
        self.anchor = route.first().copied().unwrap_or(goal);
        self.route = route;
        self.goal = Some(goal);
        self.idle = 0.0;
    }

    pub fn clear(&mut self) {
        self.route.clear();
        self.goal = None;
        self.next = 0;
        self.idle = 0.0;
        self.replans = 0;
    }

    pub fn goal(&self) -> Option<Vec3> {
        self.goal
    }

    pub fn finished(&self) -> bool {
        self.goal.is_some() && self.next >= self.route.len()
    }

    /// Current waypoint; waypoints closer than `reach` (horizontal) are consumed.
    pub fn waypoint(&mut self, pos: Vec3, reach: f32) -> Option<Vec3> {
        while let Some(w) = self.route.get(self.next) {
            let d = Vec2::new(w.x - pos.x, w.z - pos.z).length();
            if d > reach {
                return Some(*w);
            }
            self.next += 1;
        }
        None
    }

    /// Stuck watchdog: call once per tick with the elapsed time.
    pub fn watchdog(&mut self, pos: Vec3, dt: f32) -> StuckAction {
        if self.goal.is_none() || self.finished() {
            return StuckAction::None;
        }
        if Vec2::new(pos.x - self.anchor.x, pos.z - self.anchor.z).length() > STUCK_DISTANCE {
            self.anchor = pos;
            self.idle = 0.0;
            return StuckAction::None;
        }
        self.idle += dt;
        if self.idle < STUCK_SECONDS {
            return StuckAction::None;
        }
        self.idle = 0.0;
        self.replans += 1;
        if self.replans > MAX_REPLANS {
            self.replans = 0;
            if let Some((p, _)) = self.last_useable {
                return StuckAction::Reset(p);
            }
        }
        StuckAction::Replan
    }
}

/// Where and how a character approaches an interaction object.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OffsetChoice {
    pub index: usize,
    /// Standing position (host space).
    pub position: Vec3,
    /// Horizontal facing, towards the object (unit, host plane x / z).
    pub facing: Vec2,
    /// `gEDirection` of the approach animation.
    pub direction: i32,
}

/// A `gCNavOffset_PS` placed in the world.
#[derive(Debug, Clone)]
pub struct NavOffset {
    pub name: String,
    poses: Vec<(Vec3, Vec2)>,
    directions: Vec<i32>,
    pub circle: bool,
}

impl NavOffset {
    /// Place the poses with the entity matrix, then convert to host space with `map`.
    pub fn from_record(rec: &OffsetRecord, map: &dyn Fn(Vec3) -> Vec3) -> Self {
        let rot_of = |m: &Matrix, v: Vec3| {
            // Row-vector convention: image of a direction is v.x*row0 + v.y*row1 + v.z*row2.
            Vec3::new(m[0][0], m[0][1], m[0][2]) * v.x
                + Vec3::new(m[1][0], m[1][1], m[1][2]) * v.y
                + Vec3::new(m[2][0], m[2][1], m[2][2]) * v.z
        };
        let poses = rec
            .poses
            .iter()
            .map(|p| {
                let world = transform(&rec.matrix, [p[0], p[1], p[2]]);
                // The pose looks along its local +z; the character stands facing the object,
                // the half turn `RotateAxisY` of `DetectFinalTargetAsOffset` [U on the axis].
                let q = Quat::from_xyzw(p[3], p[4], p[5], p[6]);
                let fwd = -rot_of(&rec.matrix, q * Vec3::Z);
                let (a, b) = (map(world), map(world + fwd));
                let d = Vec2::new(b.x - a.x, b.z - a.z).normalize_or_zero();
                (a, d)
            })
            .collect();
        Self {
            name: rec.name.clone(),
            poses,
            directions: rec.directions.clone(),
            circle: rec.circle,
        }
    }

    pub fn pose_count(&self) -> usize {
        self.poses.len()
    }

    /// Port of `gCNavigationMap::DetectFinalTargetAsOffset`: among the poses that lie inside
    /// the zone the character is in, take the one nearest to the character.
    pub fn choose(&self, graph: &NavGraph, zone: AreaId, from: Vec3) -> Option<OffsetChoice> {
        gothic_portcov::hit!("choose");
        let mut best: Option<(f32, usize)> = None;
        for (i, (p, _)) in self.poses.iter().enumerate() {
            let inside = match zone {
                AreaId::Zone(z) => graph.zone(z).is_some_and(|z| {
                    crate::graph::point_in_polygon(Vec2::new(p.x, p.z), z.outline())
                }),
                AreaId::Path(c) => graph.corridor(c).is_some_and(|c| c.contains(*p)),
            };
            if !inside {
                continue;
            }
            let d = Vec2::new(p.x - from.x, p.z - from.z).length_squared();
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| self.choice(i))
    }

    /// The pose `index` without a zone test (character already inside, or no graph).
    pub fn choice(&self, index: usize) -> OffsetChoice {
        let (position, facing) = self.poses[index];
        OffsetChoice {
            index,
            position,
            facing,
            direction: self.direction(index),
        }
    }

    /// Animation direction of a pose: the authored entry, else forward (1); when the list
    /// exists but is shorter, left (3) and right (4) of its first entry swap.
    pub fn direction(&self, index: usize) -> i32 {
        if let Some(d) = self.directions.get(index) {
            return *d;
        }
        match self.directions.first() {
            Some(3) => 4,
            Some(4) => 3,
            _ => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NavScene;
    use crate::scene::ZoneRecord;

    const IDENT: Matrix = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];

    fn graph() -> NavGraph {
        let mut s = NavScene::default();
        let zone = |n: u8, x0: f32| {
            let mut id = [0u8; 20];
            id[0] = n;
            ZoneRecord {
                id,
                name: String::new(),
                matrix: IDENT,
                points: vec![
                    [x0, 0.0, 0.0],
                    [x0 + 1000.0, 0.0, 0.0],
                    [x0 + 1000.0, 0.0, 1000.0],
                    [x0, 0.0, 1000.0],
                ],
                radius: 0.0,
                ccw: true,
                top_tolerance: 150.0,
                bottom_tolerance: 100.0,
            }
        };
        s.zones.push(zone(1, 0.0));
        s.zones.push(zone(2, 1500.0));
        NavGraph::build(&s, &|p| p)
    }

    #[test]
    fn tracking_raises_enter_and_leave_and_remembers_the_last_ground() {
        let g = graph();
        let mut a = NavAgent::default();
        let ev = a.track(&g, Vec3::new(500.0, 0.0, 500.0));
        assert_eq!(ev, vec![NavEvent::EnterArea(AreaId::Zone(0))]);
        assert!(a.track(&g, Vec3::new(600.0, 0.0, 500.0)).is_empty());
        // Between the zones: no area. The last useable position stays.
        let ev = a.track(&g, Vec3::new(1250.0, 0.0, 500.0));
        assert_eq!(ev, vec![NavEvent::LeaveArea(AreaId::Zone(0))]);
        assert_eq!(a.last_useable.map(|l| l.0.x), Some(600.0));
        let ev = a.track(&g, Vec3::new(2000.0, 0.0, 500.0));
        assert_eq!(ev, vec![NavEvent::EnterArea(AreaId::Zone(1))]);
    }

    #[test]
    fn watchdog_replans_then_resets() {
        let mut a = NavAgent::with_last_useable(Vec3::new(5.0, 0.0, 5.0), AreaId::Zone(0));
        let goal = Vec3::new(1000.0, 0.0, 0.0);
        a.begin(vec![Vec3::ZERO, goal], goal);
        let pos = Vec3::ZERO;
        assert_eq!(a.waypoint(pos, 10.0), Some(goal));
        let mut actions = Vec::new();
        for _ in 0..(5 * 4 * 10) {
            match a.watchdog(pos, 0.25) {
                StuckAction::None => {}
                other => actions.push(other),
            }
        }
        assert_eq!(&actions[..3], &[StuckAction::Replan; 3]);
        assert_eq!(actions[3], StuckAction::Reset(Vec3::new(5.0, 0.0, 5.0)));
        // Moving resets the timer.
        let mut b = NavAgent::default();
        b.begin(vec![Vec3::ZERO, goal], goal);
        for i in 0..100 {
            assert_eq!(
                b.watchdog(Vec3::new(i as f32 * 10.0, 0.0, 0.0), 0.25),
                StuckAction::None
            );
        }
    }

    #[test]
    fn waypoints_are_consumed_by_reach() {
        let mut a = NavAgent::default();
        let goal = Vec3::new(300.0, 0.0, 0.0);
        a.begin(vec![Vec3::ZERO, Vec3::new(100.0, 0.0, 0.0), goal], goal);
        assert_eq!(a.waypoint(Vec3::ZERO, 20.0).map(|w| w.x), Some(100.0));
        assert_eq!(
            a.waypoint(Vec3::new(95.0, 0.0, 0.0), 20.0).map(|w| w.x),
            Some(300.0)
        );
        assert!(a.waypoint(Vec3::new(295.0, 0.0, 0.0), 20.0).is_none());
        assert!(a.finished());
    }

    #[test]
    fn offset_picks_the_nearest_pose_inside_the_zone() {
        let g = graph();
        let rec = OffsetRecord {
            id: [0; 20],
            name: "Campfire".into(),
            matrix: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [500.0, 0.0, 500.0, 1.0],
            ],
            circle: true,
            // Two poses: one in zone 0 (x = 500 + 100), one far to the right in zone 1.
            poses: vec![
                [100.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                [1500.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            ],
            directions: vec![3],
        };
        let off = NavOffset::from_record(&rec, &|p| p);
        let c = off
            .choose(&g, AreaId::Zone(0), Vec3::new(900.0, 0.0, 900.0))
            .unwrap();
        assert_eq!(c.index, 0);
        assert_eq!(c.position, Vec3::new(600.0, 0.0, 500.0));
        // Facing the object: identity pose looks along +z, the character faces -z.
        assert!((c.facing - Vec2::new(0.0, -1.0)).length() < 1.0e-5);
        assert_eq!(c.direction, 3);
        // Beyond the authored list left and right swap.
        assert_eq!(off.direction(1), 4);
        // Only the second pose is inside zone 1.
        let c = off.choose(&g, AreaId::Zone(1), Vec3::ZERO).unwrap();
        assert_eq!(c.index, 1);
    }
}
