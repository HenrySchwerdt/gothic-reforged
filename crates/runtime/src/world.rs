//! `WorldQuery` for the game host: actor positions live here (the ECS reads them back for
//! presentation), movement follows the navmesh (A* paths, local avoidance, no overlap with other
//! characters) when one is attached and falls back to straight lines where the mesh does not
//! know the ground; line of sight is asked of the host's collision world (a frame later, free
//! until answered) and the original's "outdoors, no check" rule stays for perception.
use glam::{Vec2, Vec3};
use gothic_nav::{PathFollower, SharedNav, avoid};
use gothic_sim::types::ActorId;
use gothic_sim::world::{Pos, WorldQuery};
use std::collections::BTreeMap;

/// Character radius used for avoidance and separation, cm. V: human NPC capsule radius
/// 34-35.3 in the templates (`PHYSICS_MOVEMENT.md`).
const AGENT_RADIUS: f32 = 35.3;

#[derive(Debug, Clone)]
struct Follow {
    goal: Vec3,
    /// Where the last step left the actor; a different position means it was moved elsewhere.
    expected: Vec3,
    follower: PathFollower,
}

/// Cells of the line-of-sight cache in cm.
const LOS_CELL: f32 = 100.0;

/// A free-line query the sim asked and the host has not answered yet.
pub type LosQuery = (Pos, Pos);

#[derive(Debug, Default)]
struct LosInner {
    /// Answers by quantised end points: `true` is a free line.
    answers: BTreeMap<[i32; 6], bool>,
    /// Questions waiting for the host's collision world.
    pending: Vec<LosQuery>,
}

/// `eCEntity::IsFreeLOS` as the sim sees it: the game host answers each question with a ray cast
/// against the level geometry one frame later ([`PlayWorld::take_los_queries`],
/// [`PlayWorld::answer_los`]); until then a line counts as free. Interior mutability because the
/// sim asks through `&self`.
#[derive(Debug, Default)]
pub struct LosCache(std::sync::Mutex<LosInner>);

impl Clone for LosCache {
    fn clone(&self) -> Self {
        let g = self.0.lock().expect("los cache");
        Self(std::sync::Mutex::new(LosInner {
            answers: g.answers.clone(),
            pending: g.pending.clone(),
        }))
    }
}

fn los_key(from: Pos, to: Pos) -> [i32; 6] {
    let q = |v: f32| (v / LOS_CELL).floor() as i32;
    [q(from.x), q(from.y), q(from.z), q(to.x), q(to.y), q(to.z)]
}

/// Actor placement shared between the core and the presentation. Units: centimetres,
/// `y` up; the yaw convention is [`Pos::yaw_to`].
#[derive(Debug, Clone, Default)]
pub struct PlayWorld {
    pub actors: BTreeMap<ActorId, (Pos, f32)>,
    /// Navigation mesh shared with the host (grown from the collision world).
    pub nav: Option<SharedNav>,
    follows: BTreeMap<ActorId, Follow>,
    /// Line-of-sight answers of the host's collision world.
    pub los: LosCache,
}

impl PlayWorld {
    /// The questions asked since the last call (the host answers them with
    /// [`PlayWorld::answer_los`]).
    pub fn take_los_queries(&self) -> Vec<LosQuery> {
        std::mem::take(&mut self.los.0.lock().expect("los cache").pending)
    }

    /// The host's answer for a question: `free` is whether the segment hits no geometry.
    pub fn answer_los(&self, from: Pos, to: Pos, free: bool) {
        let mut g = self.los.0.lock().expect("los cache");
        if g.answers.len() > 4096 {
            g.answers.clear();
        }
        g.answers.insert(los_key(from, to), free);
    }

    fn straight(&mut self, id: ActorId, target: Pos, max_dist: f32) -> f32 {
        let Some((pos, yaw)) = self.actors.get_mut(&id) else {
            return 0.0;
        };
        let d = pos.dist_xz(target);
        if d < 1.0e-3 {
            return 0.0;
        }
        let step = d.min(max_dist);
        *yaw = pos.yaw_to(target);
        pos.x += (target.x - pos.x) / d * step;
        pos.z += (target.z - pos.z) / d * step;
        step
    }

    /// One path-following step with local avoidance.
    fn navigate(&mut self, nav: &SharedNav, id: ActorId, target: Pos, max_dist: f32) -> f32 {
        let Some(&(pos, _)) = self.actors.get(&id) else {
            return 0.0;
        };
        let from = Vec3::new(pos.x, pos.y, pos.z);
        let goal = Vec3::new(target.x, target.y, target.z);
        let stale = self.follows.get(&id).is_none_or(|f| {
            f.goal.distance(goal) > 100.0
                || f.expected.distance(from) > 60.0
                || f.follower.next >= f.follower.path.len()
        });
        if stale {
            self.follows.remove(&id);
            if nav.covers(from, goal)
                && let Some(path) = nav.path(from, goal)
            {
                self.follows.insert(
                    id,
                    Follow {
                        goal,
                        expected: from,
                        follower: PathFollower::new(path, goal),
                    },
                );
            }
        }
        let Some(mut f) = self.follows.remove(&id) else {
            return self.straight(id, target, max_dist);
        };
        // Direction to the next waypoint, bent around nearby characters.
        let here = Vec2::new(pos.x, pos.z);
        let way = f
            .follower
            .path
            .get(f.follower.next)
            .copied()
            .unwrap_or(goal);
        let desired = Vec2::new(way.x, way.z) - here;
        let neighbours: Vec<(Vec2, f32, Vec2)> = self
            .actors
            .iter()
            .filter(|(k, _)| **k != id)
            .filter_map(|(_, (p, _))| {
                let q = Vec2::new(p.x, p.z);
                (q.distance(here) < 500.0).then_some((q, AGENT_RADIUS, Vec2::ZERO))
            })
            .collect();
        // Speed in cm per tick: the horizon is 1.2 m of walking.
        let horizon = 120.0 / max_dist.max(1.0);
        let dir = avoid(here, desired, max_dist, AGENT_RADIUS, &neighbours, horizon);
        let straight_ahead = dir.dot(desired.normalize_or_zero()) > 0.999;
        let step = if straight_ahead {
            max_dist.min(desired.length())
        } else {
            max_dist
        };
        let mut new = here + dir * step;
        // Hard separation: never end a step inside another character.
        for (q, r, _) in &neighbours {
            let d = new.distance(*q);
            let min = AGENT_RADIUS + r;
            if d < min && d > 1.0e-3 {
                new += (new - *q) / d * (min - d);
            }
        }
        if (Vec2::new(way.x, way.z) - new).length() < 30.0 {
            f.follower.next += 1;
        }
        let y = nav.height_at(new.x, new.y).unwrap_or(pos.y);
        let moved = new.distance(here);
        if let Some((p, yaw)) = self.actors.get_mut(&id) {
            p.x = new.x;
            p.z = new.y;
            p.y = y;
            if moved > 1.0e-3 {
                *yaw = (new.y - here.y).atan2(new.x - here.x);
            }
            f.expected = Vec3::new(p.x, p.y, p.z);
        }
        self.follows.insert(id, f);
        moved
    }
}

impl WorldQuery for PlayWorld {
    fn position(&self, id: ActorId) -> Option<Pos> {
        self.actors.get(&id).map(|a| a.0)
    }
    fn facing(&self, id: ActorId) -> f32 {
        self.actors.get(&id).map_or(0.0, |a| a.1)
    }
    fn line_of_sight(&self, from: Pos, to: Pos) -> bool {
        let key = los_key(from, to);
        let mut g = self.los.0.lock().expect("los cache");
        if let Some(free) = g.answers.get(&key) {
            return *free;
        }
        if g.pending.len() < 64 {
            g.pending.push((from, to));
        }
        true
    }
    fn move_towards(&mut self, id: ActorId, target: Pos, max_dist: f32) -> f32 {
        match self.nav.clone() {
            Some(nav) => self.navigate(&nav, id, target, max_dist),
            None => self.straight(id, target, max_dist),
        }
    }
    fn path_length(&self, from: Pos, to: Pos) -> Option<f32> {
        let straight = Some(from.dist_xz(to));
        let Some(nav) = &self.nav else {
            return straight;
        };
        let (a, b) = (
            Vec3::new(from.x, from.y, from.z),
            Vec3::new(to.x, to.y, to.z),
        );
        if !nav.covers(a, b) {
            return straight;
        }
        nav.path_length(a, b)
    }
    fn face_towards(&mut self, id: ActorId, target: Pos) {
        if let Some((pos, yaw)) = self.actors.get_mut(&id)
            && pos.dist_xz(target) > 1.0
        {
            *yaw = pos.yaw_to(target);
        }
    }
    fn teleport(&mut self, id: ActorId, pos: Pos) {
        let yaw = self.actors.get(&id).map_or(0.0, |a| a.1);
        self.actors.insert(id, (pos, yaw));
    }
    fn is_outdoors(&self, _at: Pos) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gothic_nav::{NavMesh, NavParams};
    use gothic_physics::CollisionWorld;

    fn walled_world() -> (CollisionWorld, SharedNav) {
        let mut w = CollisionWorld::new();
        let h = 3000.0;
        let (a, b, c, d) = (
            Vec3::new(-h, 0.0, -h),
            Vec3::new(h, 0.0, -h),
            Vec3::new(h, 0.0, h),
            Vec3::new(-h, 0.0, h),
        );
        w.insert_chunk(1, vec![[a, c, b], [a, d, c]]);
        // Wall at x = 0 from z = -1500 to z = 600 (gap beyond 600).
        let (p, q, r, s) = (
            Vec3::new(0.0, 0.0, -1500.0),
            Vec3::new(0.0, 300.0, -1500.0),
            Vec3::new(0.0, 300.0, 600.0),
            Vec3::new(0.0, 0.0, 600.0),
        );
        w.insert_chunk(2, vec![[p, q, r], [p, r, s]]);
        let mut mesh = NavMesh::new(NavParams::default());
        mesh.bake_disc(&w, Vec3::new(-500.0, 0.0, 0.0), 2200.0);
        (w, SharedNav::new(mesh))
    }

    #[test]
    fn actors_walk_around_a_wall_and_do_not_overlap() {
        let (_w, nav) = walled_world();
        let mut world = PlayWorld {
            nav: Some(nav),
            ..PlayWorld::default()
        };
        let (a, b) = (ActorId(1), ActorId(2));
        world.teleport(a, Pos::new(-500.0, 0.0, 0.0));
        world.teleport(b, Pos::new(-500.0, 0.0, 60.0));
        let goal = Pos::new(500.0, 0.0, 0.0);
        let goals = [
            (a, Pos::new(500.0, 0.0, -50.0)),
            (b, Pos::new(500.0, 0.0, 50.0)),
        ];
        let len = world.path_length(Pos::new(-500.0, 0.0, 0.0), goal).unwrap();
        assert!(len > 1600.0, "detour around the wall, got {len}");
        let mut min_gap = f32::MAX;
        for _ in 0..400 {
            for (id, g) in goals {
                world.move_towards(id, g, 8.0);
            }
            let (pa, pb) = (world.position(a).unwrap(), world.position(b).unwrap());
            min_gap = min_gap.min(pa.dist_xz(pb));
            // Never inside the wall.
            for p in [pa, pb] {
                assert!(
                    !(p.x.abs() < 25.0 && p.z < 600.0),
                    "actor inside the wall at {p:?}"
                );
            }
        }
        for (id, g) in goals {
            assert!(world.position(id).unwrap().dist_xz(g) < 40.0);
        }
        assert!(min_gap > 60.0, "characters overlapped: {min_gap}");
    }

    #[test]
    fn unknown_ground_falls_back_to_straight_lines() {
        let nav = SharedNav::new(NavMesh::new(NavParams::default()));
        let mut world = PlayWorld {
            nav: Some(nav),
            ..PlayWorld::default()
        };
        let a = ActorId(1);
        world.teleport(a, Pos::new(0.0, 0.0, 0.0));
        let goal = Pos::new(1000.0, 0.0, 0.0);
        assert_eq!(
            world.path_length(Pos::new(0.0, 0.0, 0.0), goal),
            Some(1000.0)
        );
        assert_eq!(world.move_towards(a, goal, 10.0), 10.0);
        assert!((world.position(a).unwrap().x - 10.0).abs() < 1e-3);
    }
}
