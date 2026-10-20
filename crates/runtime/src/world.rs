//! `WorldQuery` for the game host: actor positions live here (the ECS reads them back for
//! presentation), movement is straight-line, line of sight is the original's "outdoors, no
//! check" rule.
use gothic_sim::types::ActorId;
use gothic_sim::world::{Pos, WorldQuery};
use std::collections::BTreeMap;

/// Actor placement shared between the core and the presentation. Units: centimetres,
/// `y` up; the yaw convention is [`Pos::yaw_to`].
#[derive(Debug, Clone, Default)]
pub struct PlayWorld {
    pub actors: BTreeMap<ActorId, (Pos, f32)>,
}

impl WorldQuery for PlayWorld {
    fn position(&self, id: ActorId) -> Option<Pos> {
        self.actors.get(&id).map(|a| a.0)
    }
    fn facing(&self, id: ActorId) -> f32 {
        self.actors.get(&id).map_or(0.0, |a| a.1)
    }
    fn line_of_sight(&self, _from: Pos, _to: Pos) -> bool {
        true
    }
    fn move_towards(&mut self, id: ActorId, target: Pos, max_dist: f32) -> f32 {
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
