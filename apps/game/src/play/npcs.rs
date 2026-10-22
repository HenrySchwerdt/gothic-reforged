//! Ardea NPCs: world data -> core actors + assembled animated characters.
use super::anim::{CombatStance, Fallen, OverlayQueue, play_fall};
use super::data::{Roll, seed_of};
use super::objects::WorldObjects;
use super::{Cand, DocState, NpcActor, NpcExtra, Roster, angle_delta, sim_to_bevy_yaw};
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

/// A character can be shown: humans wear a Body item; whole-body creatures (wolves, boars,
/// seagulls ...) carry only the skeleton resource of their `eCVisualAnimation_PS` and no body
/// parts.
fn is_candidate(e: &gothic_sim::data::NpcRecord, looks: &Looks) -> bool {
    // The player's own template entity is the controlled hero (spawned by `spawn_hero`);
    // spawning it as a roster character would put a second body on top of him.
    !e.name.eq_ignore_ascii_case("PC_Hero")
        && (e
            .inventory
            .equipped
            .iter()
            .any(|i| i.slot.eq_ignore_ascii_case("Body"))
            || looks
                .map
                .get(&look_key(&e.document, &e.name, e.position))
                .is_some_and(is_creature))
}

/// Lower-case document paths of all characters the world can show.
pub fn candidate_docs(file: &NpcFile, looks: &Looks) -> std::collections::BTreeSet<String> {
    file.entities
        .iter()
        .filter(|e| is_candidate(e, looks))
        .map(|e| e.document.to_ascii_lowercase())
        .collect()
}

/// Every character of the world as a candidate; which of them are shown is decided each frame
/// by the processing range and the tile residency (`scan_candidates`).
///
/// Rule [V data, E20]: the layer a character is stored in is a sector layer; its root's Enabled
/// flag is the sector's initial state (the `*_npc_02` layer of a city carries Enabled = false and
/// holds the post-liberation population). An entity that is stored disabled inside an enabled
/// layer is not instantiated either.
pub fn select_roster(file: &NpcFile, looks: &Looks, flags: &super::layers::LayerFlags) -> Roster {
    let origin = Origin(ORIGIN);
    let mut r = Roster::default();
    let mut doc_index: std::collections::HashMap<String, u16> = std::collections::HashMap::new();
    let mut skipped = 0;
    for e in &file.entities {
        if !is_candidate(e, looks) {
            continue;
        }
        let doc = e.document.to_ascii_lowercase();
        if flags.entity_disabled(&doc, &e.name) {
            skipped += 1;
            continue;
        }
        let di = *doc_index.entry(doc.clone()).or_insert_with(|| {
            r.docs.push(DocState {
                sector: super::layers::sector_name(&doc),
                enabled: flags.root_enabled(&doc),
                path: doc.clone(),
            });
            (r.docs.len() - 1) as u16
        });
        r.display.insert(
            e.name.clone(),
            e.display_name.clone().unwrap_or_else(|| e.name.clone()),
        );
        r.cands.push(Cand {
            record: e.clone(),
            pos_cm: origin.point(Vec3::from(e.position)),
            look: looks
                .map
                .get(&look_key(&e.document, &e.name, e.position))
                .cloned(),
            doc: di,
            visual: None,
            actor: None,
            failed: false,
        });
    }
    separate(&mut r.cands, &r.docs);
    for (i, c) in r.cands.iter().enumerate() {
        r.grid
            .entry(grid_cell(c.pos_cm))
            .or_default()
            .push(i as u32);
    }
    let on = r
        .cands
        .iter()
        .filter(|c| r.docs[c.doc as usize].enabled)
        .count();
    eprintln!(
        "play: {} characters in {} layers ({} in enabled layers, {} stored disabled skipped)",
        r.cands.len(),
        r.docs.len(),
        on,
        skipped
    );
    r
}

/// Cell of the candidate grid (cm).
const GRID: f32 = 2_500.0;
fn grid_cell(p: Vec3) -> (i32, i32) {
    ((p.x / GRID).floor() as i32, (p.z / GRID).floor() as i32)
}

/// The original placements belong to different story states and overlap when shown together;
/// push neighbours apart to a minimum spacing (per grid cell, enabled layers only).
fn separate(list: &mut [Cand], docs: &[DocState]) {
    const MIN: f32 = 110.0;
    let mut buckets: std::collections::HashMap<(i32, i32), Vec<usize>> = Default::default();
    for (i, c) in list.iter().enumerate() {
        if docs[c.doc as usize].enabled {
            buckets.entry(grid_cell(c.pos_cm)).or_default().push(i);
        }
    }
    for idx in buckets.values().filter(|v| v.len() > 1) {
        for _ in 0..24 {
            let mut moved = false;
            for a in 0..idx.len() {
                for b in (a + 1)..idx.len() {
                    let (i, j) = (idx[a], idx[b]);
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
}

pub fn register(app: &mut App) {
    app.add_systems(
        Update,
        (scan_candidates, spawn_npcs)
            .chain()
            .after(crate::walk::spawn_pending),
    )
    .add_systems(
        Update,
        (sync_npcs, super::anim::overlay_system)
            .chain()
            .after(update_camera),
    );
    super::npc_moves::register(app);
    app.add_systems(Update, los_service);
}

/// Answers the line-of-sight questions the simulation asked (`eCEntity::IsFreeLOS`): a ray from
/// eye to eye against the level geometry.
fn los_service(core: Res<CoreRes>, collision: Res<Collision>) {
    /// Height of the eyes above the feet in cm [I: the captured NPC capsule is 129 + 2 x 35].
    const EYE: f32 = 150.0;
    let world = &core.0.sim.world;
    for (from, to) in world.take_los_queries() {
        let a = Vec3::new(from.x, from.y + EYE, from.z);
        let b = Vec3::new(to.x, to.y + EYE, to.z);
        let d = b - a;
        let len = d.length();
        let free = len < 1.0 || collision.0.raycast(a, d / len, len).is_none();
        world.answer_los(from, to, free);
    }
}

/// Turn rate of a character in radians per second: `TurnSpeedMax` 200 degrees
/// (`gothic_physics::MovementProfile::human_npc`).
const NPC_TURN_RATE: f32 = 200.0 * std::f32::consts::PI / 180.0;

/// Per-NPC presentation state.
#[derive(Component)]
pub struct NpcMotion {
    pub last_cm: Vec3,
    pub yaw: f32,
    pub ground_y: f32,
    pub ground_at: Vec2,
    pub velocity: Vec3,
    pub fallen: bool,
    /// 0..1: the body stands side-on while a bow is aimed (see `sync_npcs`).
    pub aim: f32,
}

/// A whole-body creature: the skeleton resource is the mesh and no body parts are listed.
fn is_creature(look: &NpcLook) -> bool {
    look.visual.parts.is_empty() && look.visual.skeleton.is_some()
}

/// Character description from the entity's visual data; `Err` names what did not resolve.
fn assemble(lib: &ActorLibrary, rec: &Cand) -> Result<NpcSpec, String> {
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

/// Characters wanted right now (inside the range box of the hero, layer enabled) that are not
/// shown yet. Readiness checks wait for these.
pub fn pending_in_ring(roster: &Roster, _stream: &Stream) -> usize {
    roster.queue.len()
}

/// Half extent of the box around the hero inside which characters are shown: the entity
/// region of interest (`Entity.ROI`, 4000) of the processing range.
const SPAWN_RANGE: f32 = 4_000.0;
/// Characters leave the world presentation beyond this (the 250 hysteresis of `eCEntityAdmin`).
const DESPAWN_RANGE: f32 = 4_250.0;
/// Seconds between candidate scans.
const SCAN_EVERY: f32 = 0.2;

fn box_distance(a: Vec3, b: Vec3) -> f32 {
    (a.x - b.x).abs().max((a.z - b.z).abs())
}

impl Cand {
    /// Where the character is now: the core's actor once it exists, else its home.
    fn pos_now(&self, core: &CoreRes) -> Vec3 {
        self.actor
            .and_then(|a| core.0.sim.world.actors.get(&a))
            .map_or(self.pos_cm, |(p, _)| Vec3::new(p.x, p.y, p.z))
    }
}

/// Finds the characters to show (`Roster::queue`, nearest first) and removes the ones that left
/// the range, whose layer was switched off, or that do not exist at this story state.
/// The core keeps the actor (position, health, inventory); only the presentation goes.
#[allow(clippy::too_many_arguments)]
fn scan_candidates(
    time: Res<Time>,
    mut roster: ResMut<Roster>,
    player: Res<crate::walk::Player>,
    core: Res<CoreRes>,
    state: Res<super::PlayState>,
    mut events: MessageReader<gothic_runtime::CoreEvent>,
    mut commands: Commands,
) {
    let mut dirty = false;
    for e in events.read() {
        if let gothic_runtime::Event::SectorSwitched { sector, enabled } = &e.0 {
            eprintln!(
                "play: sector {sector} switched {}",
                if *enabled { "on" } else { "off" }
            );
            roster.sector_switches.push((sector.clone(), *enabled));
            dirty = true;
        }
    }
    if dirty {
        let flags: Vec<bool> = roster
            .docs
            .iter()
            .map(|d| core.0.document_enabled(&d.path))
            .collect();
        for (d, on) in roster.docs.iter_mut().zip(flags) {
            d.enabled = on;
        }
    }
    let now = time.elapsed_secs();
    if !dirty && now - roster.last_scan < SCAN_EVERY {
        return;
    }
    roster.last_scan = now;
    let t0 = std::time::Instant::now();
    let hero = player.ctrl.position;
    // Leaving the world.
    let mut gone: Vec<u32> = Vec::new();
    for &i in &roster.live {
        let c = &roster.cands[i as usize];
        let busy = c.actor.is_some() && c.actor == state.dialog_npc;
        let far = box_distance(c.pos_now(&core), hero) > DESPAWN_RANGE;
        if !busy && (far || !roster.docs[c.doc as usize].enabled) {
            gone.push(i);
        }
    }
    for i in gone {
        let c = &mut roster.cands[i as usize];
        if let Some(e) = c.visual.take() {
            commands.entity(e).despawn();
        }
        let actor = c.actor;
        if let Some(a) = actor {
            roster.by_actor.remove(&a);
        }
        roster.live.retain(|x| *x != i);
        roster.despawned += 1;
    }
    // Entering it.
    let (cx, cz) = grid_cell(hero);
    let reach = (SPAWN_RANGE / GRID).ceil() as i32 + 1;
    let mut want: Vec<(f32, u32)> = Vec::new();
    for gx in (cx - reach)..=(cx + reach) {
        for gz in (cz - reach)..=(cz + reach) {
            let Some(list) = roster.grid.get(&(gx, gz)) else {
                continue;
            };
            for &i in list {
                let c = &roster.cands[i as usize];
                if c.visual.is_some() || c.failed || !roster.docs[c.doc as usize].enabled {
                    continue;
                }
                let d = box_distance(c.pos_now(&core), hero);
                if d <= SPAWN_RANGE {
                    want.push((d, i));
                }
            }
        }
    }
    want.sort_by(|a, b| a.0.total_cmp(&b.0));
    roster.queue = want.into_iter().map(|w| w.1).collect();
    roster.total = roster.spawned + roster.queue.len();
    roster.cost.scan.add(t0.elapsed());
}

/// Spawns queued characters whose ground collision is resident, within a time budget.
#[allow(clippy::too_many_arguments)]
fn spawn_npcs(
    time: Res<Time>,
    mut roster: ResMut<Roster>,
    stream: Res<Stream>,
    collision: Res<Collision>,
    mut spawner: ActorSpawner,
    mut commands: MessageWriter<CoreCommand>,
    objects: Res<WorldObjects>,
    core: Res<CoreRes>,
) {
    let started = std::time::Instant::now();
    let queued = !roster.queue.is_empty();
    // A body is built on the main thread (mesh, skeleton and clip decode on a cold cache costs
    // milliseconds). After an expensive one the next waits, so the cost spreads over frames
    // instead of landing as one hitch: the spawner never takes more than a third of the time.
    let now = time.elapsed_secs();
    if now < roster.spawn_not_before {
        return;
    }
    let mut budget = 6;
    let mut qi = 0;
    while qi < roster.queue.len() && budget > 0 && started.elapsed().as_micros() < 3_000 {
        let ci = roster.queue[qi] as usize;
        let home = roster.cands[ci].pos_now(&core);
        let cell = stream.settings.cell_at(home);
        if !collision.0.has_chunk(tile_chunk_id((cell.x, cell.z))) {
            qi += 1;
            continue;
        }
        roster.queue.remove(qi);
        // Snap to the surface nearest the original height: a small window first, then the
        // first hit below the original height (never the topmost hit, which can be a roof).
        let from = Vec3::new(home.x, home.y + 50.0, home.z);
        // Flyers (gulls, flocks) hover far above the surface: with a resident tile and nothing
        // within reach below, they stay at their original height instead of waiting forever.
        let ground = collision
            .0
            .ground_below(from, 100.0)
            .or_else(|| collision.0.ground_below(from, 800.0))
            .unwrap_or(home.y);
        budget -= 1;
        let cand = &roster.cands[ci];
        let known = cand.actor;
        let mut spec = match assemble(&spawner.library, cand) {
            Ok(spec) => spec,
            Err(why) => {
                eprintln!("play: appearance of {} unresolved: {why}", cand.record.name);
                roster.cands[ci].failed = true;
                continue;
            }
        };
        let idx = roster.spawned;
        spec = spec.with_start_time(hash01(idx, 4) * 2.0);
        let gear = gear_of(&objects, &core, &spawner.library, &cand.record);
        for w in &gear.weapons {
            spec = spec.with_weapon(w.clone());
        }
        let yaw = match known.and_then(|a| core.0.sim.world.actors.get(&a)) {
            Some((_, sim_yaw)) => sim_to_bevy_yaw(*sim_yaw),
            None => hash01(idx, 7) * std::f32::consts::TAU,
        };
        let pos = Vec3::new(home.x, ground, home.z);
        let tf = Transform {
            translation: pos * CM,
            rotation: Quat::from_rotation_y(yaw),
            scale: Vec3::splat(CM),
        };
        let t_spawn = std::time::Instant::now();
        let spawned = match spawner.spawn(&spec, tf) {
            Ok(e) => Ok(e),
            Err(e) => {
                // A weapon whose slot this skeleton lacks: spawn the character without it.
                eprintln!(
                    "play: {} spawn with weapons failed ({e:#}), retrying bare",
                    cand.record.name
                );
                spec.weapons.clear();
                spawner.spawn(&spec, tf)
            }
        };
        let built = t_spawn.elapsed();
        let entity = match spawned {
            Ok(e) => e,
            Err(e) => {
                eprintln!("play: spawn {} failed: {e:#}", cand.record.name);
                roster.cands[ci].failed = true;
                continue;
            }
        };
        let creature = cand.look.as_ref().is_some_and(is_creature);
        let stance = if creature {
            Stance::Unarmed
        } else if Species::from_name(&cand.record.species) == Species::Orc && gear.equip.is_empty()
        {
            Stance::TwoHand
        } else {
            gear.stance
        };
        let look = cand.look.as_ref();
        let extra = NpcExtra {
            voice: look.map(|l| l.voice.clone()).unwrap_or_default(),
            work: look.and_then(|l| l.routine.working_points.first().cloned()),
            relax: look.and_then(|l| l.routine.relaxing_points.first().cloned()),
            sleep: look.and_then(|l| l.routine.sleeping_points.first().cloned()),
        };
        let record = cand.record.clone();
        let mut ec = spawner.commands.entity(entity);
        ec.insert((
            extra,
            OverlayQueue::default(),
            CombatStance(stance),
            NpcMotion {
                last_cm: pos,
                yaw,
                ground_y: ground,
                ground_at: Vec2::new(pos.x, pos.z),
                velocity: Vec3::ZERO,
                fallen: false,
                aim: 0.0,
            },
        ));
        roster.cost.build.add(built);
        roster.cands[ci].visual = Some(entity);
        roster.live.push(ci as u32);
        roster.spawned += 1;
        match known {
            Some(actor) => {
                // The core still has this character (health, inventory, quest state): bind the
                // new body to it instead of creating a second actor.
                ec.insert(NpcActor(actor));
                roster.by_actor.insert(actor, entity);
            }
            None => {
                ec.insert(NpcGear {
                    equip: gear.equip,
                    give: gear.give,
                });
                roster.binds.push_back((entity, ci as u32));
                commands.write(CoreCommand(Command::SpawnNpc {
                    record: Box::new(record),
                    at: Pos::new(pos.x, ground, pos.z),
                    yaw: super::bevy_to_sim_yaw(yaw),
                }));
            }
        }
    }
    roster.total = roster.spawned + roster.queue.len();
    if queued {
        roster.cost.spawn.add(started.elapsed());
        let spent = started.elapsed().as_secs_f32();
        if spent > 0.003 {
            roster.spawn_not_before = now + spent * 2.0;
        }
    }
}

/// Core actor state -> entity transform, locomotion input and fall poses.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_npcs(
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
        &mut CombatStance,
    )>,
) {
    let dt = time.delta_secs().clamp(1.0e-3, 0.1);
    for (entity, npc, mut tf, mut m, input, mut animator, mut cs) in &mut q {
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
        // `TurnSpeedMax` of the NPC templates (200 degrees per second).
        let step = (NPC_TURN_RATE * dt).min(delta.abs()) * delta.signum();
        m.yaw += step;
        // Write only on change so idle characters keep their transform un-flagged (transform
        // propagation, the joint writer and the renderer all skip unchanged entities).
        let translation = Vec3::new(p.x, m.ground_y, p.z) * CM;
        if tf.translation != translation {
            tf.translation = translation;
        }
        // The aim clips leave the body side-on with the left arm along the entity's -X: while a
        // ranged weapon aims the model turns a quarter so that arm points at the target, as the
        // hero's does (`hero.rs`, eased in and out).
        let aiming = actor.drawn
            && actor.target.is_some()
            && m.velocity.length() < 60.0
            && matches!(
                actor.equipment.weapon_mode(),
                gothic_sim::item::WeaponMode::Ranged
            );
        m.aim += ((if aiming { 1.0 } else { 0.0 }) - m.aim) * (dt * 10.0).min(1.0);
        let rotation = Quat::from_rotation_y(m.yaw + std::f32::consts::FRAC_PI_2 * m.aim);
        if tf.rotation != rotation {
            tf.rotation = rotation;
        }
        if tf.scale != Vec3::splat(CM) {
            tf.scale = Vec3::splat(CM);
        }
        if let Some(mut input) = input {
            // The hands decide the stance: a fighter that switched to the bow stands as an
            // archer (`_AI_SelectWeapon`); the stance of the spawn is the fallback for
            // characters whose weapons the simulation does not hold as items (natural weapons).
            let stance = if actor.drawn {
                match super::hero::stance_of(&actor.equipment) {
                    Stance::Unarmed => cs.0,
                    held => {
                        // The overlays of shots and casts follow what the hands hold now.
                        if cs.0 != held {
                            cs.0 = held;
                        }
                        held
                    }
                }
            } else {
                Stance::Unarmed
            };
            let turn_error = if m.velocity.length() < 30.0 {
                delta - step
            } else {
                0.0
            };
            // The guard of `_AI_Parade` stays up as a pose until the next instruction.
            let parade = actor.ani_state == gothic_sim::types::AniState::Parade;
            if input.velocity != m.velocity
                || !input.grounded
                || input.stance != stance
                || input.turn_error != turn_error
                || input.parade != parade
            {
                input.velocity = m.velocity;
                input.grounded = true;
                input.stance = stance;
                input.turn_error = turn_error;
                input.parade = parade;
            }
        }
    }
}
