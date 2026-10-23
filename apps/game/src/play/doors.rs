//! Doors of the world (`gCDoor_PS` with a `gCInteraction_PS` of use type Door): the animated
//! actor of the entity (`G3_Door_01.FXA` -> `G3_Door_01.xact`), the open / close motions
//! (`OpenAni` / `CloseAni`: `G3_Door_01_Open.FXM` -> `G3_Door_01_Open.xmot`) and the solid body
//! of a closed door. The state and the lock live in the core (`DoorChanged`, `PS_Interact`);
//! this file only shows them.
use super::objects::WorldObjects;
use crate::walk::{Collision, Player};
use bevy::prelude::*;
use gothic_animation::runtime::CharacterComposition;
use gothic_animation::{ActorLibrary, ActorSpawner, Animator, NpcSpec};
use gothic_runtime::{CoreEvent, Event};
use gothic_sim::script::enums::UseType;
use std::collections::HashMap;

const CM: f32 = 0.01;
/// Doors are built within this distance of the hero and dropped beyond the second one (cm).
const SHOW: f32 = 4_500.0;
const DROP: f32 = 6_000.0;
/// Collision chunk ids of doors live above the tile chunk ids.
const CHUNK_BASE: u64 = 0xD00D_0000_0000_0000;

struct DoorVis {
    entity: Entity,
    stem: String,
    open: bool,
    chunk: Option<u64>,
}

/// Door bodies shown now, by index into [`WorldObjects::list`].
#[derive(Resource, Default)]
pub struct Doors {
    shown: HashMap<usize, DoorVis>,
    /// Open / close motions started so far (QA).
    pub played: Vec<(usize, bool)>,
}

impl Doors {
    pub fn is_shown(&self, i: usize) -> bool {
        self.shown.contains_key(&i)
    }
    pub fn is_solid(&self, i: usize) -> bool {
        self.shown.get(&i).is_some_and(|d| d.chunk.is_some())
    }
}

pub fn register(app: &mut App) {
    app.init_resource::<Doors>()
        .add_systems(Update, (door_visuals, door_state, door_bodies).chain());
}

fn door_yaw(heading: f32) -> Quat {
    Quat::from_rotation_y(-heading)
}

fn far(o: &super::objects::ObjEntry, hero: Vec3, limit: f32) -> bool {
    (o.pos_cm.x - hero.x).abs().max((o.pos_cm.z - hero.z).abs()) > limit
}

fn door_visuals(
    mut doors: ResMut<Doors>,
    objects: Res<WorldObjects>,
    player: Res<Player>,
    time: Res<Time>,
    mut spawner: ActorSpawner,
    mut collision: ResMut<Collision>,
    mut last: Local<f32>,
) {
    let now = time.elapsed_secs();
    if now - *last < 0.5 {
        return;
    }
    *last = now;
    let hero = player.ctrl.position;
    // Drop the far ones and those whose layer is gone.
    let gone: Vec<usize> = doors
        .shown
        .keys()
        .copied()
        .filter(|i| far(&objects.list[*i], hero, DROP) || !objects.layer_on(*i))
        .collect();
    for i in gone {
        if let Some(d) = doors.shown.remove(&i) {
            spawner.commands.entity(d.entity).despawn();
            if let Some(c) = d.chunk {
                collision.0.remove_chunk(c);
            }
        }
    }
    let mut budget = 2;
    for (i, o) in objects.list.iter().enumerate() {
        if budget == 0 {
            break;
        }
        if o.def.script_use() != UseType::Door
            || o.entity.is_none()
            || doors.shown.contains_key(&i)
            || !objects.layer_on(i)
            || far(o, hero, SHOW)
        {
            continue;
        }
        let Some(stem) = o.def.assets.iter().find_map(|a| {
            a.to_ascii_lowercase()
                .ends_with(".fxa")
                .then(|| a[..a.len() - 4].to_string())
        }) else {
            continue;
        };
        if !spawner.library.has_actor(&stem) {
            eprintln!("play: door {} has no actor {stem}", o.def.name);
            continue;
        }
        budget -= 1;
        // Closed: the end of the close motion.
        let mut spec = NpcSpec::new(stem.clone());
        if spawner.library.has_motion(&format!("{stem}_Close")) {
            spec = spec
                .with_motion(format!("{stem}_Close"))
                .with_start_time(1.0e3);
        }
        let tf = Transform {
            translation: o.pos_cm * CM,
            rotation: door_yaw(o.def.heading),
            scale: Vec3::splat(CM),
        };
        match spawner.spawn(&spec, tf) {
            Ok(entity) => {
                doors.shown.insert(
                    i,
                    DoorVis {
                        entity,
                        stem,
                        open: false,
                        chunk: None,
                    },
                );
            }
            Err(e) => eprintln!("play: door {}: {e:#}", o.def.name),
        }
    }
}

/// `DoorChanged` (use key, lock pick, key) -> the open / close motion.
fn door_state(
    mut events: MessageReader<CoreEvent>,
    mut doors: ResMut<Doors>,
    objects: Res<WorldObjects>,
    mut lib: ResMut<ActorLibrary>,
    mut anims: Query<&mut Animator>,
) {
    for e in events.read() {
        let Event::Player(gothic_sim::script::player::api::PlayerEvent::DoorChanged {
            object,
            open,
        }) = &e.0
        else {
            continue;
        };
        let Some(i) = objects.index_of_entity(object.0) else {
            continue;
        };
        let Some(d) = doors.shown.get_mut(&i) else {
            continue;
        };
        d.open = *open;
        let name = format!("{}_{}", d.stem, if *open { "Open" } else { "Close" });
        if let Ok(mut a) = anims.get_mut(d.entity) {
            let rig = a.rig.clone();
            if let Ok(m) = lib.motion(&rig, &name) {
                a.play(m, 0.0, 1.0, 0.0);
            }
        }
        doors.played.push((i, *open));
    }
}

/// A closed door is a solid box of its closed body; open, it is not.
fn door_bodies(
    mut doors: ResMut<Doors>,
    objects: Res<WorldObjects>,
    mut collision: ResMut<Collision>,
    comps: Query<&CharacterComposition>,
) {
    let ids: Vec<usize> = doors.shown.keys().copied().collect();
    for i in ids {
        let Some(d) = doors.shown.get_mut(&i) else {
            continue;
        };
        match (!d.open, d.chunk) {
            (true, None) => {
                let Ok(c) = comps.get(d.entity) else { continue };
                let Some(part) = c.0.parts.first() else {
                    continue;
                };
                // The part box carries the loader's padding (35 % of the size + 20 cm).
                let centre = Vec3::from(part.aabb.center);
                let half = Vec3::from(part.aabb.half_extents);
                let h = ((half * 2.0 - Vec3::splat(40.0)) / 1.7 * 0.5).max(Vec3::splat(3.0));
                let o = &objects.list[i];
                let rot = door_yaw(o.def.heading);
                let corner = |sx: f32, sy: f32, sz: f32| {
                    o.pos_cm + rot * (centre + Vec3::new(sx * h.x, sy * h.y, sz * h.z))
                };
                let v = [
                    corner(-1., -1., -1.),
                    corner(1., -1., -1.),
                    corner(1., 1., -1.),
                    corner(-1., 1., -1.),
                    corner(-1., -1., 1.),
                    corner(1., -1., 1.),
                    corner(1., 1., 1.),
                    corner(-1., 1., 1.),
                ];
                let quads = [
                    [0, 1, 2, 3],
                    [4, 7, 6, 5],
                    [0, 4, 5, 1],
                    [3, 2, 6, 7],
                    [0, 3, 7, 4],
                    [1, 5, 6, 2],
                ];
                let mut tris = Vec::new();
                for q in quads {
                    tris.push([v[q[0]], v[q[1]], v[q[2]]]);
                    tris.push([v[q[0]], v[q[2]], v[q[3]]]);
                }
                let id = CHUNK_BASE + i as u64;
                collision.0.insert_chunk(id, tris);
                d.chunk = Some(id);
            }
            (false, Some(id)) => {
                collision.0.remove_chunk(id);
                d.chunk = None;
            }
            _ => {}
        }
    }
}
