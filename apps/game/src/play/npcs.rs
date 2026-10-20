//! Ardea NPCs: world data -> core actors + assembled animated characters.
use super::anim::{CombatStance, Fallen, OverlayQueue, play_fall};
use super::{NpcActor, PendingNpc, Roster, angle_delta, sim_to_bevy_yaw};
use crate::walk::{Collision, ORIGIN, Stream, tile_chunk_id, update_camera};
use crate::world_loader::Origin;
use bevy::prelude::*;
use gothic_animation::library::template_actor_candidates;
use gothic_animation::{
    ActorLibrary, ActorSpawner, Animator, LocomotionInput, NpcSpec, Stance, WeaponSpec,
};
use gothic_runtime::{Command, CoreCommand, CoreRes};
use gothic_sim::data::{NpcFile, NpcRecord};
use gothic_sim::types::{AiMode, Species};
use gothic_sim::world::Pos;

const CM: f32 = 0.01;
/// Original-space centre and radius (cm) of the Ardea NPC set.
const ARDEA_CENTRE: Vec2 = Vec2::new(91_000.0, -11_000.0);
const ARDEA_RADIUS: f32 = 7_000.0;

pub fn select_roster(file: &NpcFile) -> Roster {
    let origin = Origin(ORIGIN);
    let mut r = Roster::default();
    for e in &file.entities {
        let p = Vec3::from(e.position);
        if Vec2::new(p.x, p.z).distance(ARDEA_CENTRE) > ARDEA_RADIUS
            || !e
                .inventory
                .equipped
                .iter()
                .any(|i| i.slot.eq_ignore_ascii_case("Body"))
        {
            continue;
        }
        r.display.insert(
            e.name.clone(),
            e.display_name.clone().unwrap_or_else(|| e.name.clone()),
        );
        r.pending.push(PendingNpc {
            record: e.clone(),
            pos_cm: origin.point(p),
        });
    }
    // Hamlar first so the showcase finds him quickly.
    r.pending.sort_by_key(|p| p.record.name != "Hamlar");
    separate(&mut r.pending);
    r.total = r.pending.len();
    r
}

/// The original placements belong to different story states and overlap when shown together;
/// push neighbours apart to a minimum spacing (Hamlar stays where the data puts him).
fn separate(list: &mut [PendingNpc]) {
    const MIN: f32 = 110.0;
    for _ in 0..24 {
        let mut moved = false;
        for i in 0..list.len() {
            for j in (i + 1)..list.len() {
                let d = Vec2::new(
                    list[j].pos_cm.x - list[i].pos_cm.x,
                    list[j].pos_cm.z - list[i].pos_cm.z,
                );
                let len = d.length();
                if len >= MIN {
                    continue;
                }
                moved = true;
                let dir = if len < 1.0 {
                    Vec2::new(1.0, 0.3).normalize()
                } else {
                    d / len
                };
                let push = (MIN - len) * 0.5;
                let fixed_i = list[i].record.name == "Hamlar";
                let (pi, pj) = if fixed_i {
                    (0.0, 2.0 * push)
                } else {
                    (push, push)
                };
                list[i].pos_cm.x -= dir.x * pi;
                list[i].pos_cm.z -= dir.y * pi;
                list[j].pos_cm.x += dir.x * pj;
                list[j].pos_cm.z += dir.y * pj;
            }
        }
        if !moved {
            break;
        }
    }
}

pub fn register(app: &mut App) {
    app.add_systems(Update, spawn_npcs.after(crate::walk::spawn_pending))
        .add_systems(
            Update,
            (sync_npcs, super::anim::overlay_system)
                .chain()
                .after(update_camera),
        );
}

/// Per-NPC presentation state.
#[derive(Component)]
pub struct NpcMotion {
    pub last_cm: Vec3,
    pub yaw: f32,
    pub ground_y: f32,
    pub ground_at: Vec2,
    pub velocity: Vec3,
    pub fallen: bool,
}

fn part_for(lib: &ActorLibrary, template: &str) -> Option<String> {
    template_actor_candidates(template)
        .into_iter()
        .find(|c| lib.has_actor(c))
}

/// Stand-in bodies for item templates whose mesh is not installed under a derivable name
/// (the template to mesh link lives in undecoded engine data).
fn body_fallbacks(template: &str) -> &'static [&'static str] {
    let l = template.to_ascii_lowercase();
    if l.contains("cook") {
        &["G3_Fat_Body_Smith", "G3_Fat_Body_Slave"]
    } else if l.contains("babe") && l.contains("_02") {
        &["G3_Hero_Body_Babe_Myrtana_02"]
    } else if l.contains("babe") {
        &["G3_Hero_Body_Babe_Myrtana_01"]
    } else if l.contains("rebelite") {
        &["G3_Hero_Body_Ranger", "G3_Hero_Body_Mercenary"]
    } else if l.contains("rebwarrior") {
        &["G3_Hero_Body_RebBanditMed", "G3_Hero_Body_Mercenary"]
    } else if l.contains("rebbandit") || l.contains("novice") {
        &["G3_Hero_Body_RebBandit"]
    } else {
        &["G3_Hero_Body_Peasant"]
    }
}

/// Body + head (+ hair / beard) from the equipped item templates.
fn assemble(lib: &ActorLibrary, rec: &NpcRecord) -> Option<NpcSpec> {
    let mut body = None;
    let mut extras = Vec::new();
    for e in &rec.inventory.equipped {
        let is_body = e.slot.eq_ignore_ascii_case("Body");
        let part = part_for(lib, &e.template).or_else(|| {
            is_body
                .then(|| {
                    body_fallbacks(&e.template)
                        .iter()
                        .find(|n| lib.has_actor(n))
                        .map(|n| (*n).to_string())
                })
                .flatten()
        });
        let Some(part) = part else {
            continue;
        };
        if is_body {
            body = Some(part);
        } else {
            extras.push(part);
        }
    }
    let mut spec = NpcSpec::new(body?);
    // Heads first, then hair and beards.
    extras.sort_by_key(|p| {
        let l = p.to_ascii_lowercase();
        if l.contains("hair") || l.contains("beard") {
            1
        } else {
            0
        }
    });
    for p in extras {
        spec = spec.with_part(p);
    }
    Some(spec)
}

fn hash01(i: usize, salt: u64) -> f32 {
    let mut x =
        (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ salt.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 30;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 27;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

fn weapon_for(lib: &ActorLibrary, rec: &NpcRecord) -> Option<WeaponSpec> {
    let species = Species::from_name(&rec.species);
    let wanted: &[&str] = if species == Species::Orc {
        &["Weapon_2H_Orc", "Weapon_Orc", "Orc_Axe", "Orc_Sword"]
    } else if rec.guild.eq_ignore_ascii_case("Reb") && rec.level >= 10 {
        &["G3_Weapon_1H_Sword_Broad_01", "G3_Weapon_1H_Sword_01"]
    } else {
        return None;
    };
    for w in wanted {
        if let Some(n) = lib.static_mesh_names(w).into_iter().next() {
            return Some(WeaponSpec::one_hand_sword(&n));
        }
    }
    None
}

/// Spawns pending NPCs whose ground collision is resident.
#[allow(clippy::too_many_arguments)]
fn spawn_npcs(
    mut roster: ResMut<Roster>,
    stream: Res<Stream>,
    collision: Res<Collision>,
    mut spawner: ActorSpawner,
    mut commands: MessageWriter<CoreCommand>,
    warned: Local<bool>,
) {
    let mut budget = 6;
    let mut i = 0;
    while i < roster.pending.len() && budget > 0 {
        let p = &roster.pending[i];
        let cell = stream.settings.cell_at(p.pos_cm);
        if !collision.0.has_chunk(tile_chunk_id((cell.x, cell.z))) {
            i += 1;
            continue;
        }
        let from = Vec3::new(p.pos_cm.x, p.pos_cm.y + 120.0, p.pos_cm.z);
        let Some(ground) = collision.0.ground_below(from, 800.0) else {
            i += 1;
            continue;
        };
        let p = roster.pending.remove(i);
        budget -= 1;
        let Some(mut spec) = assemble(&spawner.library, &p.record) else {
            if !*warned && !p.record.inventory.equipped.is_empty() {
                eprintln!(
                    "play: cannot assemble {} ({:?})",
                    p.record.name,
                    p.record
                        .inventory
                        .equipped
                        .iter()
                        .map(|e| e.template.as_str())
                        .collect::<Vec<_>>()
                );
            }
            roster.total -= 1;
            continue;
        };
        let idx = roster.spawned;
        spec = spec.with_start_time(hash01(idx, 4) * 2.0);
        if let Some(w) = weapon_for(&spawner.library, &p.record) {
            spec = spec.with_weapon(w);
        }
        let yaw = hash01(idx, 7) * std::f32::consts::TAU;
        let pos = Vec3::new(p.pos_cm.x, ground, p.pos_cm.z);
        let tf = Transform {
            translation: pos * CM,
            rotation: Quat::from_rotation_y(yaw),
            scale: Vec3::splat(CM),
        };
        let entity = match spawner.spawn(&spec, tf) {
            Ok(e) => e,
            Err(e) => {
                // Retry with body and head only (a hair / beard part may not bind).
                let mut slim = NpcSpec::new(spec.parts[0].clone());
                if let Some(h) = spec.parts.get(1) {
                    slim = slim.with_part(h.clone());
                }
                match spawner.spawn(&slim, tf) {
                    Ok(e2) => e2,
                    Err(_) => {
                        eprintln!("play: spawn {} failed: {e:#}", p.record.name);
                        roster.total -= 1;
                        continue;
                    }
                }
            }
        };
        let stance = if Species::from_name(&p.record.species) == Species::Orc {
            Stance::TwoHand
        } else {
            Stance::OneHand
        };
        spawner.commands.entity(entity).insert((
            OverlayQueue::default(),
            CombatStance(stance),
            NpcMotion {
                last_cm: pos,
                yaw,
                ground_y: ground,
                ground_at: Vec2::new(pos.x, pos.z),
                velocity: Vec3::ZERO,
                fallen: false,
            },
        ));
        roster.binds.push_back(entity);
        roster.spawned += 1;
        commands.write(CoreCommand(Command::SpawnNpc {
            record: Box::new(p.record),
            at: Pos::new(pos.x, ground, pos.z),
            yaw: super::bevy_to_sim_yaw(yaw),
        }));
    }
}

/// Core actor state -> entity transform, locomotion input and fall poses.
#[allow(clippy::too_many_arguments)]
fn sync_npcs(
    time: Res<Time>,
    core: Res<CoreRes>,
    collision: Res<Collision>,
    mut lib: ResMut<ActorLibrary>,
    mut cmd: Commands,
    mut q: Query<(
        Entity,
        &NpcActor,
        &mut Transform,
        &mut NpcMotion,
        Option<&mut LocomotionInput>,
        &mut Animator,
        &CombatStance,
    )>,
) {
    let dt = time.delta_secs().clamp(1.0e-3, 0.1);
    for (entity, npc, mut tf, mut m, input, mut animator, cs) in &mut q {
        let actor = core.0.sim.actor(npc.0);
        let down = matches!(actor.ai_mode, AiMode::Dead | AiMode::Down) || actor.hp <= 0;
        if down {
            if !m.fallen {
                m.fallen = true;
                cmd.entity(entity)
                    .remove::<(LocomotionInput, gothic_animation::Locomotion)>()
                    .insert(Fallen);
                play_fall(&mut lib, &mut animator, cs.0);
            }
            continue;
        }
        if m.fallen {
            // Woke up: back to locomotion.
            m.fallen = false;
            cmd.entity(entity).remove::<Fallen>().insert((
                gothic_animation::Locomotion::default(),
                LocomotionInput::default(),
            ));
            continue;
        }
        let Some((p, sim_yaw)) = core.0.sim.world.actors.get(&npc.0).copied() else {
            continue;
        };
        let want = Vec3::new(p.x, 0.0, p.z);
        let moved = Vec2::new(want.x - m.last_cm.x, want.z - m.last_cm.z);
        if Vec2::new(want.x, want.z).distance(m.ground_at) > 8.0 {
            let from = Vec3::new(p.x, m.ground_y + 200.0, p.z);
            if let Some(g) = collision.0.ground_below(from, 600.0) {
                m.ground_y = g;
            }
            m.ground_at = Vec2::new(want.x, want.z);
        }
        let v = Vec3::new(moved.x / dt, 0.0, moved.y / dt);
        m.velocity = m.velocity.lerp(v, 0.5);
        m.last_cm = Vec3::new(p.x, m.ground_y, p.z);
        let target = sim_to_bevy_yaw(sim_yaw);
        let delta = angle_delta(m.yaw, target);
        let step = (7.0 * dt).min(delta.abs()) * delta.signum();
        m.yaw += step;
        tf.translation = Vec3::new(p.x, m.ground_y, p.z) * CM;
        tf.rotation = Quat::from_rotation_y(m.yaw);
        tf.scale = Vec3::splat(CM);
        if let Some(mut input) = input {
            input.velocity = m.velocity;
            input.grounded = true;
            input.stance = if actor.drawn { cs.0 } else { Stance::Unarmed };
            input.turn_error = if m.velocity.length() < 30.0 {
                delta - step
            } else {
                0.0
            };
        }
    }
}
