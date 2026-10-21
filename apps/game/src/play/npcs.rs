//! Ardea NPCs: world data -> core actors + assembled animated characters.
use super::anim::{CombatStance, Fallen, OverlayQueue, play_fall};
use super::data::{Roll, seed_of};
use super::objects::WorldObjects;
use super::{NpcActor, NpcExtra, PendingNpc, Roster, angle_delta, sim_to_bevy_yaw};
use crate::walk::{Collision, ORIGIN, Stream, tile_chunk_id, update_camera};
use crate::world_loader::Origin;
use bevy::prelude::*;
use gothic_animation::{
    ActorLibrary, ActorSpawner, Animator, LocomotionInput, NpcSpec, Stance, WeaponSpec,
};
use gothic_formats::props::records::NpcLook;
use gothic_runtime::{Command, CoreCommand, CoreRes};
use gothic_sim::data::NpcFile;
use gothic_sim::item::Use;
use gothic_sim::types::{AiMode, Species};
use gothic_sim::world::Pos;

const CM: f32 = 0.01;
/// Original-space centre and radius (cm) of the Ardea NPC set.
const ARDEA_CENTRE: Vec2 = Vec2::new(91_000.0, -11_000.0);
const ARDEA_RADIUS: f32 = 7_000.0;

/// Appearance records of `npcs.json`, keyed by document, name and position.
pub struct Looks {
    map: std::collections::HashMap<(String, String, [u32; 3]), NpcLook>,
    hero: Option<NpcLook>,
}

impl Looks {
    pub fn pc_hero(&self) -> Option<NpcLook> {
        self.hero.clone()
    }
}

fn look_key(document: &str, name: &str, p: [f32; 3]) -> (String, String, [u32; 3]) {
    (
        document.to_string(),
        name.to_string(),
        [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()],
    )
}

/// Reads the appearance slice of every world NPC entity in `npcs.json`.
pub fn load_looks(path: &std::path::Path) -> anyhow::Result<Looks> {
    let root: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let entities: Vec<NpcLook> = root["entities"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default();
    let hero = entities.iter().find(|e| e.name == "PC_Hero").cloned();
    let map = entities
        .into_iter()
        .map(|l| (look_key(&l.document, &l.name, l.position), l))
        .collect();
    Ok(Looks { map, hero })
}

/// (document, name) of the entities whose `eCEntity` header has Enabled
/// cleared, in the world documents the candidates come from.
///
/// Rule [V data]: a disabled entity is not instantiated by the engine. The `*_npc_02` layer of
/// every city (Ardea: 133 entities, all of them) and the story layers carry Enabled = false;
/// they hold the post-liberation population and are switched on by story script. At a new game
/// only the Enabled entities exist.
fn disabled_source_ids(
    data_dir: &std::path::Path,
    docs: &std::collections::BTreeSet<&str>,
) -> std::collections::HashSet<(String, String)> {
    use gothic_formats::vfs::VirtualFileSystem;
    use gothic_formats::world_node::WorldDocument;
    let mut out = std::collections::HashSet::new();
    let Ok(mut vfs) = VirtualFileSystem::open_data_dir(data_dir) else {
        eprintln!("play: cannot open the archives for entity flags; no story filter");
        return out;
    };
    let slots: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| docs.contains(p.to_ascii_lowercase().as_str()))
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    for (path, slot) in slots {
        let Ok(bytes) = vfs.read(slot) else { continue };
        let Ok(doc) = WorldDocument::parse(&bytes) else {
            eprintln!("play: {path}: entity flags unreadable");
            continue;
        };
        for e in doc.entities.iter().filter(|e| !e.enabled) {
            out.insert((path.to_ascii_lowercase(), e.name.clone()));
        }
    }
    out
}

pub fn select_roster(file: &NpcFile, looks: &Looks, data_dir: &std::path::Path) -> Roster {
    let origin = Origin(ORIGIN);
    let mut r = Roster::default();
    // Humans wear a Body item; whole-body creatures (wolves, boars, ...) carry only the
    // skeleton-mesh resource of their `eCVisualAnimation_PS` and no body parts.
    let near = |e: &gothic_sim::data::NpcRecord| {
        let p = Vec3::from(e.position);
        Vec2::new(p.x, p.z).distance(ARDEA_CENTRE) <= ARDEA_RADIUS
            && (e
                .inventory
                .equipped
                .iter()
                .any(|i| i.slot.eq_ignore_ascii_case("Body"))
                || looks
                    .map
                    .get(&look_key(&e.document, &e.name, e.position))
                    .is_some_and(is_creature))
    };
    let docs: std::collections::BTreeSet<String> = file
        .entities
        .iter()
        .filter(|e| near(e))
        .map(|e| e.document.to_ascii_lowercase())
        .collect();
    let doc_refs = docs.iter().map(String::as_str).collect();
    let disabled = disabled_source_ids(data_dir, &doc_refs);
    let mut skipped = 0;
    for e in &file.entities {
        let p = Vec3::from(e.position);
        if !near(e) {
            continue;
        }
        if disabled.contains(&(e.document.to_ascii_lowercase(), e.name.clone())) {
            skipped += 1;
            continue;
        }
        r.display.insert(
            e.name.clone(),
            e.display_name.clone().unwrap_or_else(|| e.name.clone()),
        );
        r.pending.push(PendingNpc {
            record: e.clone(),
            pos_cm: origin.point(p),
            look: looks
                .map
                .get(&look_key(&e.document, &e.name, e.position))
                .cloned(),
        });
    }
    // Hamlar first so the showcase finds him quickly.
    r.pending.sort_by_key(|p| p.record.name != "Hamlar");
    separate(&mut r.pending);
    r.total = r.pending.len();
    eprintln!(
        "play: story state at new game: {} NPCs enabled, {skipped} disabled (eCEntity Enabled = false) skipped",
        r.total
    );
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

/// A whole-body creature: the skeleton resource is the mesh and no body parts are listed.
fn is_creature(look: &NpcLook) -> bool {
    look.visual.parts.is_empty() && look.visual.skeleton.is_some()
}

/// Character description from the entity's visual data; `Err` names what did not resolve.
fn assemble(lib: &ActorLibrary, rec: &PendingNpc) -> Result<NpcSpec, String> {
    let look = rec.look.as_ref().ok_or("no appearance record")?;
    gothic_animation::appearance::resolve(lib, look)
}

fn hash01(i: usize, salt: u64) -> f32 {
    let mut x =
        (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ salt.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 30;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 27;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

/// The weapons a character carries, from its real inventory: the stacks of its `Weaponry`
/// treasure sets and the weapon stacks of its own inventory (`gCInventory_PS`).
pub struct Gear {
    /// Templates to wear (main weapon, shield).
    pub equip: Vec<String>,
    /// Everything else the character carries (plunder, spare weapons).
    pub give: Vec<(String, u32)>,
    pub stance: Stance,
    pub weapons: Vec<WeaponSpec>,
}

/// Slots a weapon sits in when drawn / holstered (`GetBone` of the item entity, ENGINE_MAP).
fn weapon_spec(kind: Use, mesh: &str) -> WeaponSpec {
    let (drawn, holster) = match kind {
        Use::Shield => ("Slot_LeftHand_Shield", "Slot_ShieldBack"),
        Use::Bow => ("Slot_LeftHand_Weapon", "Slot_Bow"),
        Use::CrossBow => ("Slot_LeftHand_Weapon", "Slot_CrossBow"),
        Use::TwoH | Use::Axe => ("Slot_RightHand_Weapon", "Slot_AxeBack"),
        Use::Staff | Use::Halberd => ("Slot_RightHand_Weapon", "Slot_StaffBack"),
        _ => ("Slot_RightHand_Weapon", "Slot_MeleeBackRight"),
    };
    WeaponSpec {
        mesh: mesh.into(),
        drawn_slot: drawn.into(),
        holster_slot: holster.into(),
        offset: Transform::IDENTITY,
    }
}

pub fn gear_of(
    objects: &WorldObjects,
    core: &CoreRes,
    lib: &ActorLibrary,
    rec: &gothic_sim::data::NpcRecord,
) -> Gear {
    let sets = &rec.inventory.treasure_sets;
    let mut stacks: Vec<(String, u32)> = objects.treasure.weaponry(sets);
    stacks.extend(
        rec.inventory
            .stacks
            .iter()
            .filter(|s| s.amount > 0)
            .map(|s| (s.item.clone(), s.amount as u32)),
    );
    let mut roll = Roll(seed_of(&format!("{}{:?}", rec.name, rec.position)));
    let plunder = objects.treasure.plunder(sets, &mut roll);
    let kind_of = |name: &str| {
        core.0
            .sim
            .items
            .get(name)
            .map_or(Use::None, gothic_sim::item::ItemDef::use_kind)
    };
    let mut gear = Gear {
        equip: Vec::new(),
        give: Vec::new(),
        stance: Stance::OneHand,
        weapons: Vec::new(),
    };
    let mut main: Option<(String, Use)> = None;
    let mut bow: Option<(String, Use)> = None;
    let mut shield: Option<String> = None;
    for (name, n) in &stacks {
        let k = kind_of(name);
        match k {
            Use::OneH | Use::TwoH | Use::Axe | Use::Staff | Use::Halberd if main.is_none() => {
                main = Some((name.clone(), k));
            }
            Use::Bow | Use::CrossBow if bow.is_none() => bow = Some((name.clone(), k)),
            Use::Shield if shield.is_none() => shield = Some(name.clone()),
            _ => gear.give.push((name.clone(), *n)),
        }
    }
    let attach = |gear: &mut Gear, name: &str, k: Use| {
        let mesh = objects.item_mesh.get(&name.to_ascii_lowercase());
        if let Some(m) = mesh.filter(|m| lib.has_mesh(m)) {
            gear.weapons.push(weapon_spec(k, m));
        }
    };
    // A fighter wields his melee weapon; archers without one carry the bow.
    if let Some((name, k)) = main.clone().or_else(|| bow.clone()) {
        gear.stance = match k {
            Use::TwoH | Use::Axe | Use::Halberd => Stance::TwoHand,
            Use::Staff => Stance::Staff,
            Use::Bow => Stance::Bow,
            Use::CrossBow => Stance::CrossBow,
            _ => Stance::OneHand,
        };
        gear.equip.push(name.clone());
        attach(&mut gear, &name, k);
        if k == Use::OneH
            && let Some(sh) = shield.clone()
        {
            gear.equip.push(sh.clone());
            gear.stance = Stance::OneHandShield;
            attach(&mut gear, &sh, Use::Shield);
        }
    }
    if let (Some((b, k)), Some(_)) = (bow.clone(), main.clone()) {
        // The bow of a swordsman hangs on his back.
        gear.give.push((b.clone(), 1));
        attach(&mut gear, &b, k);
    }
    gear.give.extend(plunder);
    gear
}

/// Carried gear waiting for the core to know the character.
#[derive(Component)]
pub struct NpcGear {
    pub equip: Vec<String>,
    pub give: Vec<(String, u32)>,
}

/// Spawns pending NPCs whose ground collision is resident.
#[allow(clippy::too_many_arguments)]
fn spawn_npcs(
    mut roster: ResMut<Roster>,
    stream: Res<Stream>,
    collision: Res<Collision>,
    mut spawner: ActorSpawner,
    mut commands: MessageWriter<CoreCommand>,
    objects: Res<WorldObjects>,
    core: Res<CoreRes>,
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
        // Snap to the surface nearest the original height: a small window first, then the
        // first hit below the original height (never the topmost hit, which can be a roof).
        let from = Vec3::new(p.pos_cm.x, p.pos_cm.y + 50.0, p.pos_cm.z);
        let Some(ground) = collision
            .0
            .ground_below(from, 100.0)
            .or_else(|| collision.0.ground_below(from, 800.0))
        else {
            i += 1;
            continue;
        };
        let p = roster.pending.remove(i);
        budget -= 1;
        let mut spec = match assemble(&spawner.library, &p) {
            Ok(spec) => spec,
            Err(why) => {
                eprintln!("play: appearance of {} unresolved: {why}", p.record.name);
                roster.total -= 1;
                continue;
            }
        };
        let idx = roster.spawned;
        spec = spec.with_start_time(hash01(idx, 4) * 2.0);
        let gear = gear_of(&objects, &core, &spawner.library, &p.record);
        for w in &gear.weapons {
            spec = spec.with_weapon(w.clone());
        }
        let yaw = hash01(idx, 7) * std::f32::consts::TAU;
        let pos = Vec3::new(p.pos_cm.x, ground, p.pos_cm.z);
        let tf = Transform {
            translation: pos * CM,
            rotation: Quat::from_rotation_y(yaw),
            scale: Vec3::splat(CM),
        };
        let spawned = match spawner.spawn(&spec, tf) {
            Ok(e) => Ok(e),
            Err(e) => {
                // A weapon whose slot this skeleton lacks: spawn the character without it.
                eprintln!(
                    "play: {} spawn with weapons failed ({e:#}), retrying bare",
                    p.record.name
                );
                spec.weapons.clear();
                spawner.spawn(&spec, tf)
            }
        };
        let entity = match spawned {
            Ok(e) => e,
            Err(e) => {
                eprintln!("play: spawn {} failed: {e:#}", p.record.name);
                roster.total -= 1;
                continue;
            }
        };
        let creature = p.look.as_ref().is_some_and(is_creature);
        let stance = if creature {
            Stance::Unarmed
        } else if Species::from_name(&p.record.species) == Species::Orc && gear.equip.is_empty() {
                Stance::TwoHand
            } else {
                gear.stance
            };
        let look = p.look.as_ref();
        let extra = NpcExtra {
            voice: look.map(|l| l.voice.clone()).unwrap_or_default(),
            work: look.and_then(|l| l.routine.working_points.first().cloned()),
            relax: look.and_then(|l| l.routine.relaxing_points.first().cloned()),
            sleep: look.and_then(|l| l.routine.sleeping_points.first().cloned()),
        };
        spawner.commands.entity(entity).insert((
            extra,
            NpcGear {
                equip: gear.equip,
                give: gear.give,
            },
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
        // Write only on change so idle characters keep their transform un-flagged (transform
        // propagation, the joint writer and the renderer all skip unchanged entities).
        let translation = Vec3::new(p.x, m.ground_y, p.z) * CM;
        if tf.translation != translation {
            tf.translation = translation;
        }
        let rotation = Quat::from_rotation_y(m.yaw);
        if tf.rotation != rotation {
            tf.rotation = rotation;
        }
        if tf.scale != Vec3::splat(CM) {
            tf.scale = Vec3::splat(CM);
        }
        if let Some(mut input) = input {
            let stance = if actor.drawn { cs.0 } else { Stance::Unarmed };
            let turn_error = if m.velocity.length() < 30.0 {
                delta - step
            } else {
                0.0
            };
            if input.velocity != m.velocity
                || !input.grounded
                || input.stance != stance
                || input.turn_error != turn_error
            {
                input.velocity = m.velocity;
                input.grounded = true;
                input.stance = stance;
                input.turn_error = turn_error;
            }
        }
    }
}
