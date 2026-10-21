//! Entity processing range for the simulation: mirrors every actor as a dynamic entity of the
//! world crate's scene model and asks the ported `eCEntityAdmin` which of them are inside the
//! region of interest around the camera (the hero). Actors outside are not processed; crossings
//! are reported as enter / exit events (`OnEnterProcessingRange` / `OnExitProcessingRange`).
use bevy::math::Vec3;
use gothic_sim::types::ActorId;
use gothic_world::pvs::{EntityId, PvsCell};
use gothic_world::scene::{CellSource, Scene, StaticDesc};
use std::collections::{BTreeMap, BTreeSet};

/// Half extents of the processing-range box of a character (cm): the capsule of a human.
const ACTOR_HALF: Vec3 = Vec3::new(40.0, 90.0, 40.0);
/// Far clip plane of the main camera (`fFarClippingPlane`, high quality).
const FAR_CLIP: f32 = 10_000.0;
/// Movement below this is not forwarded to the entity (avoids dirtying idle actors).
const MOVE_EPSILON: f32 = 1.0;

struct NoStatics;
impl CellSource for NoStatics {
    fn cell_entities(&self, _: PvsCell, _: u32) -> Vec<StaticDesc> {
        Vec::new()
    }
}

/// A crossing of the processing range border.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeEvent {
    pub actor: ActorId,
    pub entered: bool,
}

/// One actor as the range sees it: id, name, position (cm), dead.
pub type ActorView<'a> = (ActorId, &'a str, Vec3, bool);

/// The processing range of one core.
pub struct ProcessingRange {
    scene: Scene,
    entities: BTreeMap<ActorId, EntityId>,
    actors: BTreeMap<EntityId, ActorId>,
    /// Switch: when off every actor is processed.
    pub enabled: bool,
    last: BTreeMap<ActorId, Vec3>,
}

impl Default for ProcessingRange {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessingRange {
    pub fn new() -> Self {
        Self {
            scene: Scene::new(FAR_CLIP),
            entities: BTreeMap::new(),
            actors: BTreeMap::new(),
            enabled: true,
            last: BTreeMap::new(),
        }
    }

    /// Radius of the dynamic ROI sphere (`Entity.ROI`).
    pub fn set_roi(&mut self, radius: f32) {
        self.scene.admin.set_roi_radius(radius);
        self.scene.admin.force_full_update();
    }

    pub fn roi(&self) -> f32 {
        self.scene.admin.config().dynamic_radius
    }

    /// Update the mirror and the region around `camera`. Returns the actors that crossed the
    /// border (exits first) and the set currently inside.
    pub fn update<'a>(
        &mut self,
        camera: Vec3,
        actors: impl Iterator<Item = ActorView<'a>>,
    ) -> (Vec<RangeEvent>, BTreeSet<ActorId>) {
        for (id, name, pos, dead) in actors {
            match self.entities.get(&id) {
                None if !dead => {
                    let e = self.scene.spawn_dynamic(name, pos, ACTOR_HALF);
                    self.entities.insert(id, e);
                    self.actors.insert(e, id);
                    self.last.insert(id, pos);
                }
                Some(&e) => {
                    if dead {
                        // A dead body no longer ticks; the entity goes with the kill list.
                        self.scene.kill(e);
                    } else if self
                        .last
                        .get(&id)
                        .is_none_or(|p| p.distance(pos) > MOVE_EPSILON)
                    {
                        self.scene.move_to(e, pos);
                        self.last.insert(id, pos);
                    }
                }
                None => {}
            }
        }
        let mut events = Vec::new();
        if let Ok(update) = self.scene.update(camera, &NoStatics) {
            for (list, entered) in [
                (&update.processing.exited, false),
                (&update.processing.entered, true),
            ] {
                for e in list {
                    if let Some(&actor) = self.actors.get(e) {
                        events.push(RangeEvent { actor, entered });
                    }
                }
            }
            for gone in update.destroyed {
                if let Some(a) = self.actors.remove(&gone) {
                    self.entities.remove(&a);
                    self.last.remove(&a);
                }
            }
        }
        let inside = self
            .scene
            .admin
            .in_range()
            .iter()
            .filter_map(|e| self.actors.get(e).copied())
            .collect();
        (events, inside)
    }

    /// Is the actor currently inside?
    pub fn contains(&self, actor: ActorId) -> bool {
        self.entities
            .get(&actor)
            .and_then(|e| self.scene.tree.get(*e))
            .is_some_and(|e| e.has_entered_processing_range())
    }

    /// Number of entities that tick this frame (inside, enabled, alive).
    pub fn ticking(&self) -> usize {
        self.scene.admin.tick(&self.scene.tree, |_, _| {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Row = (u32, Vec3, bool);

    fn run(
        r: &mut ProcessingRange,
        cam: Vec3,
        list: &[Row],
    ) -> (Vec<RangeEvent>, BTreeSet<ActorId>) {
        let names: Vec<String> = list.iter().map(|(i, ..)| format!("npc{i}")).collect();
        r.update(
            cam,
            list.iter()
                .zip(&names)
                .map(|((i, p, d), n)| (ActorId(*i), n.as_str(), *p, *d)),
        )
    }

    #[test]
    fn actors_enter_and_exit_with_the_hero() {
        let mut r = ProcessingRange::new();
        let near = (1, Vec3::new(1_000.0, 0.0, 0.0), false);
        let far = (2, Vec3::new(20_000.0, 0.0, 0.0), false);
        let (ev, inside) = run(&mut r, Vec3::ZERO, &[near, far]);
        assert_eq!(
            ev,
            vec![RangeEvent {
                actor: ActorId(1),
                entered: true
            }]
        );
        assert_eq!(inside, BTreeSet::from([ActorId(1)]));
        // Walk 16 000 units toward the second actor.
        let (ev, inside) = run(&mut r, Vec3::new(17_000.0, 0.0, 0.0), &[near, far]);
        assert_eq!(
            ev,
            vec![
                RangeEvent {
                    actor: ActorId(1),
                    entered: false
                },
                RangeEvent {
                    actor: ActorId(2),
                    entered: true
                }
            ]
        );
        assert_eq!(inside, BTreeSet::from([ActorId(2)]));
        assert!(r.contains(ActorId(2)) && !r.contains(ActorId(1)));
        assert_eq!(r.ticking(), 1);
    }

    #[test]
    fn a_walking_actor_crosses_the_border_on_its_own() {
        let mut r = ProcessingRange::new();
        let mut entered_at = None;
        for step in 0..60 {
            let x = 9_000.0 - step as f32 * 100.0;
            let (ev, _) = run(&mut r, Vec3::ZERO, &[(1, Vec3::new(x, 0.0, 0.0), false)]);
            if ev.iter().any(|e| e.entered) {
                entered_at = Some(x);
                break;
            }
        }
        // ROI 4000 + hysteresis 250 + half extent 40.
        let x = entered_at.expect("entered");
        assert!((4_200.0..=4_400.0).contains(&x), "{x}");
    }

    #[test]
    fn dead_actors_leave_and_are_forgotten() {
        let mut r = ProcessingRange::new();
        let a = (1, Vec3::new(100.0, 0.0, 0.0), false);
        run(&mut r, Vec3::ZERO, &[a]);
        let (ev, inside) = run(&mut r, Vec3::ZERO, &[(1, a.1, true)]);
        assert_eq!(
            ev,
            vec![RangeEvent {
                actor: ActorId(1),
                entered: false
            }]
        );
        assert!(inside.is_empty());
        assert!(!r.contains(ActorId(1)));
    }

    #[test]
    fn roi_radius_is_configurable() {
        let mut r = ProcessingRange::new();
        assert_eq!(r.roi(), 4_000.0);
        r.set_roi(400.0);
        let (_, inside) = run(
            &mut r,
            Vec3::ZERO,
            &[(1, Vec3::new(2_000.0, 0.0, 0.0), false)],
        );
        assert!(inside.is_empty());
    }
}
