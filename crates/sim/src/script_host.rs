//! Adapter that lets the `gothic-script` port drive the simulation: [`SimHost`] implements
//! every host trait (`ScriptHost`) over a `&mut Sim<W>`.
//!
//! Entities: actor `n` is `EntityId(n + 1)`; interact points, anchors, enclaves, items and zones
//! live in [`ScriptTables`] (stored in [`Sim::script`](crate::sim::Sim)) with ids from
//! `1 << 24` upwards. Everything the script reads that the simulation already models comes from
//! the actors, enclaves and the game state; the routine bookkeeping the simulation has no
//! counterpart for (task, state, step counter, callbacks, destinations, ...) lives in
//! [`NpcScript`]. Geometry and line of sight go through the simulation's [`WorldQuery`].
//!
//! AI commands queued by script states are collected in [`ScriptTables::commands`]; executing
//! them (walking, animations) is the job of the game layer, which drains the queue.
//!
//! Approximations (all `[I]`): zones are the world's security/outdoor classes (two actors are in
//! "the same zone" when both classes match), the player is never "standing", sprinting is
//! unknown, spells carry no deity.

use std::collections::BTreeMap;

use gothic_script::config::{Difficulty as SDifficulty, EngineSetup, Variants as SVariants};
use gothic_script::enums as se;
use gothic_script::host::{
    AiCommand, Attribute, EntityApi, EntityId, InventoryApi, NpcApi, Pool, Pos as SPos, Protection,
    ScriptCtx, StatsApi, WorldApi,
};
use gothic_script::routine::state::{Callback, State};
use gothic_script::routine::time::RoutineTime;

use crate::actor::{Actor, FightVsPlayer};
use crate::config::Difficulty;
use crate::crime::EnclaveStatus as CEnclaveStatus;
use crate::item::Use;
use crate::sim::Sim;
use crate::types::{ActorId, AiMode, Alignment, Crime, DamageType};
use crate::world::{Pos, WorldQuery, ZoneSecurity, in_fov};

const KIND_SHIFT: u32 = 24;
const KIND_POINT: u32 = 1;
const KIND_ANCHOR: u32 = 2;
const KIND_ENCLAVE: u32 = 3;
const KIND_ITEM: u32 = 4;
const KIND_ZONE: u32 = 5;

fn table_id(kind: u32, index: usize) -> EntityId {
    EntityId((kind << KIND_SHIFT) | (index as u32 + 1))
}

fn table_index(e: EntityId, kind: u32) -> Option<usize> {
    if e.0 >> KIND_SHIFT == kind {
        Some((e.0 & ((1 << KIND_SHIFT) - 1)) as usize - 1)
    } else {
        None
    }
}

/// Script-side routine state of one actor.
#[derive(Debug, Clone)]
pub struct NpcScript {
    /// Current task (`SetTask`).
    pub task: Option<State>,
    /// Current state (`SetState`).
    pub state: Option<State>,
    /// Step counter of the top state frame.
    pub progress: u32,
    /// Task callback.
    pub task_cb: Callback,
    /// Local callback.
    pub local_cb: Callback,
    /// Seconds in the current state.
    pub state_time: f32,
    /// Seconds in the current task.
    pub task_time: f32,
    /// `PSRoutine.TaskPosition`.
    pub task_position: u32,
    /// Current destination point.
    pub dest: EntityId,
    /// Arrived at the destination.
    pub on_dest: bool,
    /// Spawn position remembered by ambient creatures.
    pub start_pos: Option<SPos>,
    /// Working, relaxing and sleeping point (`[work, relax, sleep]`).
    pub routine_points: [EntityId; 3],
    /// Scanner results: nearest free object per use type.
    pub use_objects: BTreeMap<se::UseType, EntityId>,
    /// Waiting companion.
    pub waiting: bool,
    /// Party member type (the simulation only knows "summoned").
    pub member_type: se::PartyMemberType,
    /// Dialog `MobEnabled`.
    pub mob_enabled: bool,
    /// Dialog `PartyEnabled`.
    pub party_enabled: bool,
    /// `CombatState`.
    pub combat_state: u32,
    /// `PSNpc.Ransacked`.
    pub ransacked: bool,
    /// `TimeStampTransformed`.
    pub time_stamp_transformed: u32,
    /// Guard status and point.
    pub guard_status: se::GuardStatus,
    /// Guard point entity.
    pub guard_point: EntityId,
    /// AI mode the simulation cannot express (`Sleep`, `PlayerParty`).
    pub extra_mode: Option<se::AiMode>,
    /// Action the simulation cannot express.
    pub extra_action: se::Action,
    /// Pose the simulation cannot express.
    pub extra_pose: Option<se::AniState>,
    /// Status-effect bits (`HasStatusEffects`).
    pub status: u32,
    /// Primary pose id (1 or 2 while holding weapons).
    pub primary_pose: u32,
    /// Routine name set by script.
    pub routine: String,
    /// Secondary target.
    pub alt_target: EntityId,
    /// `StatePosition` of the current move.
    pub state_position: u32,
    /// Last damage written to the receiver.
    pub damage_amount: i32,
    /// Seconds of play time of the last fight.
    pub last_fight_ts: f32,
    /// Result of the last fight against the player (`Friendly` has no simulation twin).
    pub last_fight_friendly: bool,
    /// Last attack reason against the player.
    pub last_player_ar: se::AttackReason,
    /// Status of the NPC's last attacker.
    pub last_attacker: EntityId,
}

impl Default for NpcScript {
    fn default() -> Self {
        Self {
            task: None,
            state: None,
            progress: 0,
            task_cb: Callback::None,
            local_cb: Callback::None,
            state_time: 0.0,
            task_time: 0.0,
            task_position: 0,
            dest: EntityId::NONE,
            on_dest: false,
            start_pos: None,
            routine_points: [EntityId::NONE; 3],
            use_objects: BTreeMap::new(),
            waiting: false,
            member_type: se::PartyMemberType::None,
            mob_enabled: false,
            party_enabled: true,
            combat_state: 0,
            ransacked: false,
            time_stamp_transformed: 0,
            guard_status: se::GuardStatus::Inactive,
            guard_point: EntityId::NONE,
            extra_mode: None,
            extra_action: se::Action::None,
            extra_pose: None,
            status: 0,
            primary_pose: 0,
            routine: String::new(),
            alt_target: EntityId::NONE,
            state_position: 0,
            damage_amount: 0,
            last_fight_ts: 0.0,
            last_fight_friendly: false,
            last_player_ar: se::AttackReason::None,
            last_attacker: EntityId::NONE,
        }
    }
}

#[derive(Debug, Clone)]
struct PointRec {
    use_type: se::UseType,
    anchor: EntityId,
    user: EntityId,
    owner: EntityId,
    pos: Pos,
}

#[derive(Debug, Clone)]
struct AnchorRec {
    name: String,
    kind: se::AnchorType,
    max_users: u32,
    users: u32,
    points: Vec<EntityId>,
}

#[derive(Debug, Clone, Default)]
struct EnclaveExtra {
    raid: bool,
    revolution: bool,
    switched: bool,
    counts: BTreeMap<se::PlayerCrime, i32>,
}

#[derive(Debug, Clone)]
struct ItemRec {
    name: String,
    dropped: bool,
    pos: Pos,
}

/// All script-only state of a simulation.
#[derive(Debug, Clone, Default)]
pub struct ScriptTables {
    npcs: BTreeMap<u32, NpcScript>,
    points: Vec<PointRec>,
    anchors: Vec<AnchorRec>,
    enclave_names: Vec<String>,
    enclave_extra: BTreeMap<String, EnclaveExtra>,
    items: Vec<ItemRec>,
    zones: Vec<(ZoneSecurity, bool)>,
    roaming: BTreeMap<String, Vec<EntityId>>,
    political_crimes: BTreeMap<se::Alignment, i32>,
    wanted: Vec<se::Alignment>,
    god: bool,
    /// Behaviour variants of the script port (default: the shipped game).
    pub variants: Option<SVariants>,
    /// AI commands queued by script states since the last drain.
    pub commands: Vec<(ActorId, AiCommand)>,
    /// Game messages `(key, name, kind)`.
    pub messages: Vec<(String, String, u32)>,
    /// `(victim, killed)` quest-manager notifications.
    pub felled: Vec<(ActorId, bool)>,
    /// Items dropped by `DropItemsFromSlot`.
    pub dropped: Vec<(ActorId, se::Slot)>,
    /// Number of `StopAIGoto` calls.
    pub stopped_goto: u32,
}

impl ScriptTables {
    /// Add an anchor and return its entity.
    pub fn add_anchor(&mut self, name: &str, kind: se::AnchorType, max_users: u32) -> EntityId {
        self.anchors.push(AnchorRec {
            name: name.into(),
            kind,
            max_users,
            users: 0,
            points: Vec::new(),
        });
        table_id(KIND_ANCHOR, self.anchors.len() - 1)
    }

    /// Add an interact point (optionally on an anchor) and return its entity.
    pub fn add_point(&mut self, use_type: se::UseType, anchor: EntityId, pos: Pos) -> EntityId {
        self.points.push(PointRec {
            use_type,
            anchor,
            user: EntityId::NONE,
            owner: EntityId::NONE,
            pos,
        });
        let id = table_id(KIND_POINT, self.points.len() - 1);
        if let Some(i) = table_index(anchor, KIND_ANCHOR) {
            self.anchors[i].points.push(id);
        }
        id
    }

    /// Add a loose item entity.
    pub fn add_item(&mut self, name: &str, pos: Pos) -> EntityId {
        self.items.push(ItemRec {
            name: name.into(),
            dropped: false,
            pos,
        });
        table_id(KIND_ITEM, self.items.len() - 1)
    }

    /// Mutable script state of an actor.
    pub fn npc_mut(&mut self, id: ActorId) -> &mut NpcScript {
        self.npcs.entry(id.0).or_default()
    }

    /// Script state of an actor (default if never touched).
    pub fn npc(&self, id: ActorId) -> NpcScript {
        self.npcs.get(&id.0).cloned().unwrap_or_default()
    }

    /// Set an actor's working/relaxing/sleeping point.
    pub fn set_routine_point(&mut self, id: ActorId, time: RoutineTime, point: EntityId) {
        self.npc_mut(id).routine_points[time_slot(time)] = point;
    }

    /// Register the scanner result for an actor.
    pub fn set_use_object(&mut self, id: ActorId, use_type: se::UseType, object: EntityId) {
        self.npc_mut(id).use_objects.insert(use_type, object);
    }

    /// Define the roaming points of an anchor name.
    pub fn set_roaming_points(&mut self, name: &str, points: &[EntityId]) {
        self.roaming.insert(name.into(), points.to_vec());
    }

    /// Drain the queued AI commands.
    pub fn take_commands(&mut self) -> Vec<(ActorId, AiCommand)> {
        std::mem::take(&mut self.commands)
    }

    fn enclave_id(&mut self, name: &str) -> EntityId {
        if let Some(i) = self
            .enclave_names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(name))
        {
            return table_id(KIND_ENCLAVE, i);
        }
        self.enclave_names.push(name.into());
        table_id(KIND_ENCLAVE, self.enclave_names.len() - 1)
    }

    fn zone_id(&mut self, sec: ZoneSecurity, outdoor: bool) -> EntityId {
        if let Some(i) = self.zones.iter().position(|z| *z == (sec, outdoor)) {
            return table_id(KIND_ZONE, i);
        }
        self.zones.push((sec, outdoor));
        table_id(KIND_ZONE, self.zones.len() - 1)
    }
}

const fn time_slot(t: RoutineTime) -> usize {
    match t {
        RoutineTime::Work => 0,
        RoutineTime::Relax => 1,
        RoutineTime::Sleep => 2,
    }
}

fn actor_of(e: EntityId) -> Option<ActorId> {
    if e.0 == 0 || e.0 >> KIND_SHIFT != 0 {
        None
    } else {
        Some(ActorId(e.0 - 1))
    }
}

fn entity_of(a: ActorId) -> EntityId {
    EntityId(a.0 + 1)
}

fn opt_entity(a: Option<ActorId>) -> EntityId {
    a.map_or(EntityId::NONE, entity_of)
}

/// Convert by variant name between two enums that share their naming.
fn by_name<T: std::fmt::Debug, U>(v: T, f: impl Fn(&str) -> Option<U>) -> Option<U> {
    f(&format!("{v:?}"))
}

fn s_ai_mode(m: AiMode) -> se::AiMode {
    by_name(m, se::AiMode::from_name).unwrap_or(se::AiMode::Routine)
}

fn to_pos(p: Pos) -> SPos {
    SPos::new(p.x, p.y, p.z)
}

fn from_pos(p: SPos) -> Pos {
    Pos::new(p.x, p.y, p.z)
}

fn s_use(name: &str) -> se::UseType {
    match name {
        "1H" => se::UseType::OneH,
        "2H" => se::UseType::TwoH,
        other => se::UseType::from_name(other).unwrap_or(se::UseType::None),
    }
}

fn s_use_kind(u: Use) -> se::UseType {
    by_name(u, se::UseType::from_name).unwrap_or(se::UseType::None)
}

fn protection_type(p: Protection) -> DamageType {
    match p {
        Protection::Blades => DamageType::Blade,
        Protection::Impact => DamageType::Impact,
        Protection::Missile => DamageType::Missile,
        Protection::Fire => DamageType::Fire,
        Protection::Ice => DamageType::Ice,
        Protection::Lightning => DamageType::Lightning,
    }
}

fn s_crime(c: Crime) -> se::PlayerCrime {
    by_name(c, se::PlayerCrime::from_name).unwrap_or(se::PlayerCrime::None)
}

fn crime_s(c: se::PlayerCrime) -> Crime {
    match c {
        se::PlayerCrime::Livestock => Crime::Livestock,
        se::PlayerCrime::Attack => Crime::Attack,
        se::PlayerCrime::Theft => Crime::Theft,
        se::PlayerCrime::Murder => Crime::Murder,
        _ => Crime::None,
    }
}

fn s_alignment(a: Alignment) -> se::Alignment {
    by_name(a, se::Alignment::from_name).unwrap_or(se::Alignment::None)
}

fn alignment_s(a: se::Alignment) -> Alignment {
    Alignment::from_name(a.name())
}

fn s_reason(r: crate::types::AttackReason) -> se::AttackReason {
    by_name(r, se::AttackReason::from_name).unwrap_or(se::AttackReason::None)
}

fn reason_s(r: se::AttackReason) -> crate::types::AttackReason {
    use crate::types::AttackReason as R;
    match r {
        se::AttackReason::Livestock => R::Livestock,
        se::AttackReason::StopThief => R::StopThief,
        se::AttackReason::Theft => R::Theft,
        se::AttackReason::UseInteractObject => R::UseInteractObject,
        se::AttackReason::ReactToWeapon => R::ReactToWeapon,
        se::AttackReason::StopFight => R::StopFight,
        se::AttackReason::ReactToDamage => R::ReactToDamage,
        se::AttackReason::GateGuard => R::GateGuard,
        se::AttackReason::Intruder => R::Intruder,
        se::AttackReason::Enemy => R::Enemy,
        se::AttackReason::Murder => R::Murder,
        se::AttackReason::Prey => R::Prey,
        se::AttackReason::Duel => R::Duel,
        se::AttackReason::Arena => R::Arena,
        se::AttackReason::Revolution => R::Revolution,
        se::AttackReason::Kill => R::Kill,
        _ => R::None,
    }
}

fn s_fight(f: FightVsPlayer) -> se::LastFight {
    match f {
        FightVsPlayer::None => se::LastFight::None,
        FightVsPlayer::Running => se::LastFight::Running,
        FightVsPlayer::Cancel => se::LastFight::Cancel,
        FightVsPlayer::Lost => se::LastFight::Lost,
        FightVsPlayer::Won => se::LastFight::Won,
    }
}

fn fight_s(f: se::LastFight) -> FightVsPlayer {
    match f {
        se::LastFight::Running => FightVsPlayer::Running,
        se::LastFight::Cancel => FightVsPlayer::Cancel,
        se::LastFight::Lost => FightVsPlayer::Lost,
        se::LastFight::Won => FightVsPlayer::Won,
        _ => FightVsPlayer::None,
    }
}

/// The adapter: script host traits over a simulation.
pub struct SimHost<'a, W: WorldQuery> {
    /// The simulation being driven.
    pub sim: &'a mut Sim<W>,
    /// `Self` of the running script call.
    pub self_entity: EntityId,
    /// `Other` of the running script call.
    pub other_entity: EntityId,
}

impl<'a, W: WorldQuery> SimHost<'a, W> {
    /// Wrap a simulation.
    pub fn new(sim: &'a mut Sim<W>) -> Self {
        Self {
            sim,
            self_entity: EntityId::NONE,
            other_entity: EntityId::NONE,
        }
    }

    fn actor(&self, e: EntityId) -> Option<&Actor> {
        actor_of(e).and_then(|a| self.sim.actors.get(a.0 as usize))
    }

    fn actor_mut(&mut self, e: EntityId) -> Option<&mut Actor> {
        actor_of(e).and_then(|a| self.sim.actors.get_mut(a.0 as usize))
    }

    fn script(&self, e: EntityId) -> NpcScript {
        actor_of(e)
            .map(|a| self.sim.script.npc(a))
            .unwrap_or_default()
    }

    fn script_mut(&mut self, e: EntityId) -> Option<&mut NpcScript> {
        actor_of(e).map(|a| self.sim.script.npc_mut(a))
    }

    fn entity_pos(&self, e: EntityId) -> Pos {
        if let Some(a) = actor_of(e) {
            return self.sim.world.position(a).unwrap_or_default();
        }
        if let Some(i) = table_index(e, KIND_POINT) {
            return self.sim.script.points[i].pos;
        }
        if let Some(i) = table_index(e, KIND_ITEM) {
            return self.sim.script.items[i].pos;
        }
        Pos::default()
    }

    fn enclave_name(&self, e: EntityId) -> Option<String> {
        table_index(e, KIND_ENCLAVE).and_then(|i| self.sim.script.enclave_names.get(i).cloned())
    }

    fn enclave_extra_mut(&mut self, e: EntityId) -> Option<&mut EnclaveExtra> {
        let name = self.enclave_name(e)?;
        Some(
            self.sim
                .script
                .enclave_extra
                .entry(name.to_ascii_lowercase())
                .or_default(),
        )
    }

    fn enclave_extra(&self, e: EntityId) -> EnclaveExtra {
        self.enclave_name(e)
            .and_then(|n| {
                self.sim
                    .script
                    .enclave_extra
                    .get(&n.to_ascii_lowercase())
                    .cloned()
            })
            .unwrap_or_default()
    }

    fn equipped(&self, e: EntityId, slot: se::Slot) -> Option<&crate::item::ItemDef> {
        let a = self.actor(e)?;
        match slot {
            se::Slot::RightHand => a.equipment.right.as_ref(),
            se::Slot::LeftHand | se::Slot::EquipLeftHand => a.equipment.left.as_ref(),
            se::Slot::Armor | se::Slot::Body => a.equipment.body.as_ref(),
            _ => None,
        }
    }
}

impl<W: WorldQuery> ScriptCtx for SimHost<'_, W> {
    fn self_entity(&self) -> EntityId {
        self.self_entity
    }
    fn other_entity(&self) -> EntityId {
        self.other_entity
    }
    fn setup(&self) -> EngineSetup {
        let c = &self.sim.cfg;
        EngineSetup {
            extended_content: c.extended_content,
            ai_mode: c.ai_mode,
            min_hit_duration: c.min_hit_duration,
            xp_modifier: c.xp_modifier,
            quick_loot: true,
        }
    }
    fn variants(&self) -> SVariants {
        let mut v = self.sim.script.variants.unwrap_or_default();
        v.current_level_adds_raw = self.sim.cfg.current_level_adds_raw;
        v
    }
    fn difficulty(&self) -> SDifficulty {
        match self.sim.cfg.difficulty {
            Difficulty::Easy => SDifficulty::Easy,
            Difficulty::Normal => SDifficulty::Normal,
            Difficulty::Hard => SDifficulty::Hard,
        }
    }
    fn random(&mut self, n: i32) -> i32 {
        self.sim.rng.below(n)
    }
    fn playing_time(&self) -> f32 {
        (self.sim.tick * u64::from(self.sim.cfg.tick_ms)) as f32 / 1000.0
    }
    fn world_hour(&self) -> f32 {
        self.sim.clock.hour_of_day()
    }
    fn queue_ai(&mut self, npc: EntityId, cmd: AiCommand) {
        if let Some(a) = actor_of(npc) {
            self.sim.script.commands.push((a, cmd));
        }
    }
    fn print_game_message(&mut self, key: &str, name: &str, kind: u32) {
        self.sim
            .script
            .messages
            .push((key.into(), name.into(), kind));
    }
}

impl<W: WorldQuery> EntityApi for SimHost<'_, W> {
    fn player(&self) -> EntityId {
        opt_entity(self.sim.hero)
    }
    fn exists(&self, e: EntityId) -> bool {
        if let Some(a) = actor_of(e) {
            return (a.0 as usize) < self.sim.actors.len();
        }
        table_index(e, KIND_POINT).is_some_and(|i| i < self.sim.script.points.len())
            || table_index(e, KIND_ANCHOR).is_some_and(|i| i < self.sim.script.anchors.len())
            || table_index(e, KIND_ENCLAVE).is_some_and(|i| i < self.sim.script.enclave_names.len())
            || table_index(e, KIND_ITEM).is_some_and(|i| i < self.sim.script.items.len())
            || table_index(e, KIND_ZONE).is_some_and(|i| i < self.sim.script.zones.len())
    }
    fn name(&self, e: EntityId) -> String {
        if let Some(a) = self.actor(e) {
            return a.name.clone();
        }
        if let Some(i) = table_index(e, KIND_ANCHOR) {
            return self
                .sim
                .script
                .anchors
                .get(i)
                .map(|a| a.name.clone())
                .unwrap_or_default();
        }
        if let Some(i) = table_index(e, KIND_ITEM) {
            return self
                .sim
                .script
                .items
                .get(i)
                .map(|a| a.name.clone())
                .unwrap_or_default();
        }
        self.enclave_name(e).unwrap_or_default()
    }
    fn position(&self, e: EntityId) -> SPos {
        to_pos(self.entity_pos(e))
    }
    fn zone(&self, e: EntityId) -> EntityId {
        // Zones are the world's security and outdoor classes (see the module docs). The table
        // is append-only, so a `&self` lookup that misses returns the first matching record.
        let pos = self.entity_pos(e);
        let key = (
            self.sim.world.zone_security(pos),
            self.sim.world.is_outdoors(pos),
        );
        self.sim
            .script
            .zones
            .iter()
            .position(|z| *z == key)
            .map_or(EntityId::NONE, |i| table_id(KIND_ZONE, i))
    }
    fn is_in_fov(&self, observer: EntityId, target: EntityId) -> bool {
        match actor_of(observer) {
            Some(o) => in_fov(
                &self.sim.world,
                o,
                self.entity_pos(target),
                self.sim.cfg.fov_degrees,
            ),
            None => false,
        }
    }
    fn is_sprinting(&self, _e: EntityId) -> bool {
        false
    }
    fn cheat_god(&self) -> bool {
        self.sim.script.god
    }
    fn zone_owner(&self, _zone: EntityId) -> EntityId {
        EntityId::NONE
    }
    fn zone_security(&self, zone: EntityId) -> se::SecurityLevel {
        match table_index(zone, KIND_ZONE).and_then(|i| self.sim.script.zones.get(i)) {
            Some((ZoneSecurity::Public, _)) => se::SecurityLevel::Public,
            Some((ZoneSecurity::Enclave, _)) => se::SecurityLevel::Enclave,
            Some((ZoneSecurity::Political, _)) => se::SecurityLevel::Political,
            _ => se::SecurityLevel::None,
        }
    }
    fn roaming_points_by_name(&self, anchor_name: &str) -> Vec<EntityId> {
        self.sim
            .script
            .roaming
            .get(anchor_name)
            .cloned()
            .unwrap_or_default()
    }
    fn next_patrol_point(&self, current: EntityId) -> EntityId {
        let Some(i) = table_index(current, KIND_POINT) else {
            return EntityId::NONE;
        };
        let anchor = self.sim.script.points[i].anchor;
        let Some(a) = table_index(anchor, KIND_ANCHOR).and_then(|a| self.sim.script.anchors.get(a))
        else {
            return EntityId::NONE;
        };
        let at = a.points.iter().position(|&p| p == current).unwrap_or(0);
        a.points
            .get((at + 1) % a.points.len().max(1))
            .copied()
            .unwrap_or(EntityId::NONE)
    }
    fn random_navigation_point(&mut self, center: SPos, radius: f32) -> SPos {
        let angle = self.sim.rng.below(360) as f32 * std::f32::consts::PI / 180.0;
        SPos::new(
            center.x + radius * angle.cos(),
            center.y,
            center.z + radius * angle.sin(),
        )
    }
}

impl<W: WorldQuery> NpcApi for SimHost<'_, W> {
    fn species(&self, npc: EntityId) -> se::Species {
        self.actor(npc)
            .map_or(se::Species::Human, |a| a.species.to_script())
    }
    fn npc_class(&self, npc: EntityId) -> se::NpcClass {
        self.actor(npc)
            .and_then(|a| by_name(a.class, se::NpcClass::from_name))
            .unwrap_or(se::NpcClass::None)
    }
    fn npc_type(&self, npc: EntityId) -> se::NpcType {
        self.actor(npc)
            .and_then(|a| by_name(a.npc_type, se::NpcType::from_name))
            .unwrap_or(se::NpcType::Personal)
    }
    fn alignment(&self, npc: EntityId) -> se::Alignment {
        self.actor(npc)
            .map_or(se::Alignment::None, |a| s_alignment(a.alignment))
    }
    fn guard_status(&self, npc: EntityId) -> se::GuardStatus {
        self.script(npc).guard_status
    }
    fn guard_point(&self, npc: EntityId) -> EntityId {
        self.script(npc).guard_point
    }
    fn level_raw(&self, npc: EntityId) -> u32 {
        match self.actor(npc) {
            Some(a) if a.is_player => self.sim.state.level.max(1),
            Some(a) => a.level.max(0) as u32,
            None => 1,
        }
    }
    fn level_max_raw(&self, npc: EntityId) -> u32 {
        match self.actor(npc) {
            Some(a) if a.is_player => self.sim.state.level.max(1),
            Some(a) => a.level_max.max(0) as u32,
            None => 1,
        }
    }
    fn is_transformed(&self, npc: EntityId) -> bool {
        self.actor(npc).is_some_and(|a| a.transformed)
    }
    fn has_status(&self, npc: EntityId, mask: u32) -> bool {
        let sim_bits = self.actor(npc).map_or(0, |a| a.status);
        (self.script(npc).status | sim_bits) & mask != 0
    }
    fn attack_reason(&self, npc: EntityId) -> se::AttackReason {
        self.actor(npc)
            .map_or(se::AttackReason::None, |a| s_reason(a.attack_reason))
    }
    fn last_player_attack_reason(&self, npc: EntityId) -> se::AttackReason {
        self.script(npc).last_player_ar
    }
    fn last_fight_timestamp(&self, npc: EntityId) -> f32 {
        self.script(npc).last_fight_ts
    }
    fn set_last_fight_timestamp(&mut self, npc: EntityId, t: f32) {
        if let Some(s) = self.script_mut(npc) {
            s.last_fight_ts = t;
        }
    }
    fn last_fight_vs_player(&self, npc: EntityId) -> se::LastFight {
        if self.script(npc).last_fight_friendly {
            return se::LastFight::Friendly;
        }
        self.actor(npc)
            .map_or(se::LastFight::None, |a| s_fight(a.last_fight_vs_player))
    }
    fn ransacked(&self, npc: EntityId) -> bool {
        self.script(npc).ransacked
    }
    fn set_ransacked(&mut self, npc: EntityId, v: bool) {
        if let Some(s) = self.script_mut(npc) {
            s.ransacked = v;
        }
    }
    fn current_target(&self, npc: EntityId) -> EntityId {
        opt_entity(self.actor(npc).and_then(|a| a.target))
    }
    fn set_current_target(&mut self, npc: EntityId, target: EntityId) {
        if let Some(a) = self.actor_mut(npc) {
            a.target = actor_of(target);
        }
    }
    fn enclave(&self, npc: EntityId) -> EntityId {
        let Some(name) = self.actor(npc).and_then(|a| a.enclave.clone()) else {
            return EntityId::NONE;
        };
        self.sim
            .script
            .enclave_names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(&name))
            .map_or(EntityId::NONE, |i| table_id(KIND_ENCLAVE, i))
    }
    fn set_attack_reason(&mut self, npc: EntityId, reason: se::AttackReason) {
        if let Some(a) = self.actor_mut(npc) {
            a.attack_reason = reason_s(reason);
        }
    }
    fn set_last_player_attack_reason(&mut self, npc: EntityId, reason: se::AttackReason) {
        if let Some(s) = self.script_mut(npc) {
            s.last_player_ar = reason;
        }
    }
    fn set_last_fight_vs_player(&mut self, npc: EntityId, result: se::LastFight) {
        if let Some(s) = self.script_mut(npc) {
            s.last_fight_friendly = result == se::LastFight::Friendly;
        }
        if let Some(a) = self.actor_mut(npc) {
            a.last_fight_vs_player = fight_s(result);
        }
    }
    fn last_player_crime(&self, npc: EntityId) -> se::PlayerCrime {
        self.actor(npc)
            .map_or(se::PlayerCrime::None, |a| s_crime(a.last_player_crime))
    }
    fn set_last_player_crime(&mut self, npc: EntityId, crime: se::PlayerCrime) {
        if let Some(a) = self.actor_mut(npc) {
            a.last_player_crime = crime_s(crime);
        }
    }
    fn set_alignment(&mut self, npc: EntityId, alignment: se::Alignment) {
        if let Some(a) = self.actor_mut(npc) {
            a.alignment = alignment_s(alignment);
        }
    }
    fn combat_state(&self, npc: EntityId) -> u32 {
        self.script(npc).combat_state
    }
    fn set_combat_state(&mut self, npc: EntityId, state: u32) {
        if let Some(s) = self.script_mut(npc) {
            s.combat_state = state;
        }
    }
    fn stop_ai_goto(&mut self, _npc: EntityId) {
        self.sim.script.stopped_goto += 1;
    }
    fn task_time(&self, npc: EntityId) -> f32 {
        self.script(npc).task_time
    }
    fn set_task_time(&mut self, npc: EntityId, t: f32) {
        if let Some(s) = self.script_mut(npc) {
            s.task_time = t;
        }
    }
    fn set_ani_state(&mut self, npc: EntityId, state: se::AniState) {
        let mapped = by_name(state, |n| match n {
            "Stand" => Some(crate::types::AniState::Stand),
            "Sneak" => Some(crate::types::AniState::Sneak),
            "Parade" => Some(crate::types::AniState::Parade),
            "SitGround" => Some(crate::types::AniState::SitGround),
            "SitStool" => Some(crate::types::AniState::SitStool),
            "SitBench" => Some(crate::types::AniState::SitBench),
            "SitThrone" => Some(crate::types::AniState::SitThrone),
            "SitKnockDown" => Some(crate::types::AniState::SitKnockDown),
            "LieKnockDown" => Some(crate::types::AniState::LieKnockDown),
            "Swim" => Some(crate::types::AniState::Swim),
            "Dive" => Some(crate::types::AniState::Dive),
            _ => None,
        });
        match mapped {
            Some(m) => {
                if let Some(a) = self.actor_mut(npc) {
                    a.ani_state = m;
                }
                if let Some(s) = self.script_mut(npc) {
                    s.extra_pose = None;
                }
            }
            None => {
                if let Some(s) = self.script_mut(npc) {
                    s.extra_pose = Some(state);
                }
            }
        }
    }
    fn set_alternative_target(&mut self, npc: EntityId, target: EntityId) {
        if let Some(s) = self.script_mut(npc) {
            s.alt_target = target;
        }
    }
    fn set_collision_group(&mut self, _npc: EntityId, _group: u32) {}
    fn set_can_be_pushed(&mut self, _npc: EntityId, _v: bool) {}
    fn reset_character(&mut self, _npc: EntityId) {}
    fn set_routine_name(&mut self, npc: EntityId, routine: &str) {
        if let Some(a) = self.actor_mut(npc) {
            a.routine_name = routine.into();
        }
    }
    fn dialog_mob_enabled(&self, npc: EntityId) -> bool {
        self.script(npc).mob_enabled
    }
    fn current_attacker(&self, npc: EntityId) -> EntityId {
        opt_entity(self.actor(npc).and_then(|a| a.current_attacker))
    }
    fn set_current_attacker(&mut self, npc: EntityId, attacker: EntityId) {
        if let Some(a) = self.actor_mut(npc) {
            a.current_attacker = actor_of(attacker);
        }
    }
    fn last_attacker(&self, npc: EntityId) -> EntityId {
        opt_entity(self.actor(npc).and_then(|a| a.last_attacker))
    }
    fn set_last_attacker(&mut self, npc: EntityId, attacker: EntityId) {
        if let Some(a) = self.actor_mut(npc) {
            a.last_attacker = actor_of(attacker);
        }
    }
    fn set_last_hit_timestamp(&mut self, npc: EntityId, _t: u32) {
        if let Some(a) = self.actor_mut(npc) {
            a.brain.last_hit_taken_ms = None;
        }
    }
    fn set_current_action(&mut self, npc: EntityId, action: se::Action) {
        let mapped = by_name(action, |n| match n {
            "None" => Some(crate::types::Action::None),
            "Attack" => Some(crate::types::Action::Attack),
            "PowerAttack" => Some(crate::types::Action::PowerAttack),
            "QuickAttack" => Some(crate::types::Action::QuickAttack),
            "QuickAttackR" => Some(crate::types::Action::QuickAttackR),
            "QuickAttackL" => Some(crate::types::Action::QuickAttackL),
            "SimpleWhirl" => Some(crate::types::Action::SimpleWhirl),
            "TurnLeft" => Some(crate::types::Action::TurnLeft),
            "TurnRight" => Some(crate::types::Action::TurnRight),
            "SprintAttack" => Some(crate::types::Action::SprintAttack),
            "WhirlAttack" => Some(crate::types::Action::WhirlAttack),
            "PierceAttack" => Some(crate::types::Action::PierceAttack),
            "JumpAttack" => Some(crate::types::Action::JumpAttack),
            "RamAttack" => Some(crate::types::Action::RamAttack),
            "HackAttack" => Some(crate::types::Action::HackAttack),
            "FinishingAttack" => Some(crate::types::Action::FinishingAttack),
            "Parade" => Some(crate::types::Action::Parade),
            "GetUpAttack" => Some(crate::types::Action::GetUpAttack),
            "GetUpParade" => Some(crate::types::Action::GetUpParade),
            _ => None,
        });
        if let Some(a) = self.actor_mut(npc) {
            a.action = mapped.unwrap_or(crate::types::Action::None);
        }
        if let Some(s) = self.script_mut(npc) {
            s.extra_action = if mapped.is_some() {
                se::Action::None
            } else {
                action
            };
        }
    }
    fn state_position(&self, npc: EntityId) -> u32 {
        self.actor(npc)
            .map_or(0, |a| a.state_position.max(0) as u32)
    }
    fn defeated_by_player(&self, npc: EntityId) -> bool {
        self.actor(npc).is_some_and(|a| a.defeated_by_player)
    }
    fn set_defeated_by_player(&mut self, npc: EntityId, v: bool) {
        if let Some(a) = self.actor_mut(npc) {
            a.defeated_by_player = v;
        }
    }
    fn is_immortal(&self, npc: EntityId) -> bool {
        self.actor(npc).is_some_and(|a| a.immortal)
    }
    fn set_status_bits(&mut self, npc: EntityId, mask: u32, on: bool) {
        if let Some(s) = self.script_mut(npc) {
            if on {
                s.status |= mask;
            } else {
                s.status &= !mask;
            }
        }
        if let Some(a) = self.actor_mut(npc) {
            if on {
                a.status |= mask;
            } else {
                a.status &= !mask;
            }
        }
    }
    fn set_damage_amount(&mut self, npc: EntityId, amount: i32) {
        if let Some(s) = self.script_mut(npc) {
            s.damage_amount = amount;
        }
    }
    fn time_stamp_transformed(&self, npc: EntityId) -> u32 {
        self.script(npc).time_stamp_transformed
    }
    fn set_time_stamp_transformed(&mut self, npc: EntityId, t: u32) {
        if let Some(s) = self.script_mut(npc) {
            s.time_stamp_transformed = t;
        }
    }
    fn primary_pose(&self, npc: EntityId) -> u32 {
        self.script(npc).primary_pose
    }
    fn arena_active(&self, npc: EntityId) -> bool {
        self.actor(npc).is_some_and(|a| a.ai_mode == AiMode::Arena)
    }
    fn has_svm_sample(&self, _npc: EntityId, _family: &str) -> bool {
        false
    }
    fn in_dialog_with_player(&self, npc: EntityId) -> bool {
        self.sim.gui_open && self.sim.dialog_partner == actor_of(npc)
    }
    fn find_smalltalk_partner(&self, _npc: EntityId) -> EntityId {
        EntityId::NONE
    }
    fn revolution_attitude(&self, _npc: EntityId, _other: EntityId) -> se::Attitude {
        se::Attitude::None
    }
    fn enclave_status(&self, enclave: EntityId) -> se::EnclaveStatus {
        let Some(name) = self.enclave_name(enclave) else {
            return se::EnclaveStatus::Routine;
        };
        match self.sim.enclaves.get(&name).map(|e| e.status) {
            Some(CEnclaveStatus::Attack) => se::EnclaveStatus::Attack,
            Some(CEnclaveStatus::Flee) => se::EnclaveStatus::Flee,
            _ => se::EnclaveStatus::Routine,
        }
    }
    fn enclave_alignment(&self, enclave: EntityId) -> se::Alignment {
        self.enclave_name(enclave)
            .and_then(|n| self.sim.enclaves.get(&n).map(|e| s_alignment(e.alignment)))
            .unwrap_or(se::Alignment::None)
    }
    fn enclave_switched(&self, enclave: EntityId) -> bool {
        self.enclave_extra(enclave).switched
    }
    fn ai_mode(&self, npc: EntityId) -> se::AiMode {
        if let Some(m) = self.script(npc).extra_mode {
            return m;
        }
        self.actor(npc)
            .map_or(se::AiMode::Routine, |a| s_ai_mode(a.ai_mode))
    }
    fn set_ai_mode(&mut self, npc: EntityId, mode: se::AiMode) {
        let mapped = match mode {
            se::AiMode::Routine => Some(AiMode::Routine),
            se::AiMode::Reaction => Some(AiMode::Reaction),
            se::AiMode::Combat => Some(AiMode::Combat),
            se::AiMode::Flee => Some(AiMode::Flee),
            se::AiMode::Arena => Some(AiMode::Arena),
            se::AiMode::Talk => Some(AiMode::Talk),
            se::AiMode::Down => Some(AiMode::Down),
            se::AiMode::Dead => Some(AiMode::Dead),
            se::AiMode::Abandoned => Some(AiMode::Abandoned),
            _ => None,
        };
        if let Some(s) = self.script_mut(npc) {
            s.extra_mode = if mapped.is_some() { None } else { Some(mode) };
        }
        if let Some(a) = self.actor_mut(npc) {
            a.ai_mode = mapped.unwrap_or(AiMode::Routine);
        }
    }
    fn ani_state(&self, npc: EntityId) -> se::AniState {
        if let Some(p) = self.script(npc).extra_pose {
            return p;
        }
        self.actor(npc)
            .and_then(|a| by_name(a.ani_state, se::AniState::from_name))
            .unwrap_or(se::AniState::Stand)
    }
    fn current_action(&self, npc: EntityId) -> se::Action {
        let extra = self.script(npc).extra_action;
        if extra != se::Action::None {
            return extra;
        }
        self.actor(npc)
            .and_then(|a| by_name(a.action, se::Action::from_name))
            .unwrap_or(se::Action::None)
    }
    fn current_task(&self, npc: EntityId) -> Option<State> {
        self.script(npc).task
    }
    fn set_task(&mut self, npc: EntityId, task: State) {
        if let Some(s) = self.script_mut(npc) {
            s.task = Some(task);
            s.state = Some(task);
            s.progress = 0;
            s.task_position = 0;
            s.state_time = 0.0;
            s.task_time = 0.0;
        }
    }
    fn current_state(&self, npc: EntityId) -> Option<State> {
        self.script(npc).state
    }
    fn set_state(&mut self, npc: EntityId, state: State) {
        if let Some(s) = self.script_mut(npc) {
            s.state = Some(state);
            s.progress = 0;
            s.state_time = 0.0;
        }
    }
    fn set_task_callback(&mut self, npc: EntityId, cb: Callback) {
        if let Some(s) = self.script_mut(npc) {
            s.task_cb = cb;
        }
    }
    fn set_local_callback(&mut self, npc: EntityId, cb: Callback) {
        if let Some(s) = self.script_mut(npc) {
            s.local_cb = cb;
        }
    }
    fn state_time(&self, npc: EntityId) -> f32 {
        self.script(npc).state_time
    }
    fn set_state_time(&mut self, npc: EntityId, t: f32) {
        if let Some(s) = self.script_mut(npc) {
            s.state_time = t;
        }
    }
    fn task_position(&self, npc: EntityId) -> u32 {
        self.script(npc).task_position
    }
    fn set_task_position(&mut self, npc: EntityId, pos: u32) {
        if let Some(s) = self.script_mut(npc) {
            s.task_position = pos;
        }
    }
    fn state_progress(&self, npc: EntityId) -> u32 {
        self.script(npc).progress
    }
    fn set_state_progress(&mut self, npc: EntityId, progress: u32) {
        if let Some(s) = self.script_mut(npc) {
            s.progress = progress;
        }
    }
    fn full_stop(&mut self, npc: EntityId) {
        if let Some(a) = actor_of(npc) {
            self.sim.script.commands.retain(|(who, _)| *who != a);
        }
    }
    fn destination_point(&self, npc: EntityId) -> EntityId {
        self.script(npc).dest
    }
    fn set_destination_point(&mut self, npc: EntityId, point: EntityId) {
        if let Some(s) = self.script_mut(npc) {
            s.dest = point;
        }
    }
    fn is_on_destination(&self, npc: EntityId) -> bool {
        self.script(npc).on_dest
    }
    fn set_on_destination(&mut self, npc: EntityId, v: bool) {
        if let Some(s) = self.script_mut(npc) {
            s.on_dest = v;
        }
    }
    fn is_travelling(&self, _npc: EntityId) -> bool {
        false
    }
    fn in_processing_range(&self, npc: EntityId) -> bool {
        // The simulation processes every actor within 100 m of the hero.
        let Some(h) = self.sim.hero else { return true };
        actor_of(npc).is_none_or(|a| self.sim.dist(a, h) < 10_000.0)
    }
    fn start_position(&self, npc: EntityId) -> Option<SPos> {
        self.script(npc).start_pos
    }
    fn set_start_position(&mut self, npc: EntityId, pos: SPos) {
        if let Some(s) = self.script_mut(npc) {
            s.start_pos = Some(pos);
        }
    }
    fn routine_point(&self, npc: EntityId, time: RoutineTime) -> EntityId {
        self.script(npc).routine_points[time_slot(time)]
    }
    fn point_use_type(&self, point: EntityId) -> se::UseType {
        if let Some(i) = table_index(point, KIND_POINT) {
            return self
                .sim
                .script
                .points
                .get(i)
                .map_or(se::UseType::None, |p| p.use_type);
        }
        // Items carry the use type of their template.
        table_index(point, KIND_ITEM)
            .and_then(|i| self.sim.script.items.get(i))
            .and_then(|it| self.sim.items.get(&it.name))
            .and_then(|d| d.use_type.as_deref())
            .map_or(se::UseType::None, s_use)
    }
    fn point_anchor(&self, point: EntityId) -> EntityId {
        table_index(point, KIND_POINT)
            .and_then(|i| self.sim.script.points.get(i))
            .map_or(EntityId::NONE, |p| p.anchor)
    }
    fn point_user(&self, point: EntityId) -> EntityId {
        table_index(point, KIND_POINT)
            .and_then(|i| self.sim.script.points.get(i))
            .map_or(EntityId::NONE, |p| p.user)
    }
    fn set_point_user(&mut self, point: EntityId, user: EntityId) {
        if let Some(p) =
            table_index(point, KIND_POINT).and_then(|i| self.sim.script.points.get_mut(i))
        {
            p.user = user;
        }
    }
    fn point_owner(&self, point: EntityId) -> EntityId {
        table_index(point, KIND_POINT)
            .and_then(|i| self.sim.script.points.get(i))
            .map_or(EntityId::NONE, |p| p.owner)
    }
    fn anchor_type(&self, anchor: EntityId) -> se::AnchorType {
        table_index(anchor, KIND_ANCHOR)
            .and_then(|i| self.sim.script.anchors.get(i))
            .map_or(se::AnchorType::Local, |a| a.kind)
    }
    fn anchor_max_users(&self, anchor: EntityId) -> u32 {
        table_index(anchor, KIND_ANCHOR)
            .and_then(|i| self.sim.script.anchors.get(i))
            .map_or(0, |a| a.max_users)
    }
    fn set_anchor_max_users(&mut self, anchor: EntityId, v: u32) {
        if let Some(a) =
            table_index(anchor, KIND_ANCHOR).and_then(|i| self.sim.script.anchors.get_mut(i))
        {
            a.max_users = v;
        }
    }
    fn anchor_user_count(&self, anchor: EntityId) -> u32 {
        table_index(anchor, KIND_ANCHOR)
            .and_then(|i| self.sim.script.anchors.get(i))
            .map_or(0, |a| a.users)
    }
    fn set_anchor_user_count(&mut self, anchor: EntityId, v: u32) {
        if let Some(a) =
            table_index(anchor, KIND_ANCHOR).and_then(|i| self.sim.script.anchors.get_mut(i))
        {
            a.users = v;
        }
    }
    fn anchor_points(&self, anchor: EntityId) -> Vec<EntityId> {
        table_index(anchor, KIND_ANCHOR)
            .and_then(|i| self.sim.script.anchors.get(i))
            .map(|a| a.points.clone())
            .unwrap_or_default()
    }
    fn find_use_object(&self, npc: EntityId, use_type: se::UseType) -> EntityId {
        self.script(npc)
            .use_objects
            .get(&use_type)
            .copied()
            .unwrap_or(EntityId::NONE)
    }
    fn party_leader(&self, npc: EntityId) -> EntityId {
        opt_entity(self.actor(npc).and_then(|a| a.party_leader))
    }
    fn set_party_leader(&mut self, npc: EntityId, leader: EntityId) {
        if let Some(a) = self.actor_mut(npc) {
            a.party_leader = actor_of(leader);
        }
    }
    fn party_waiting(&self, npc: EntityId) -> bool {
        self.script(npc).waiting
    }
    fn set_party_waiting(&mut self, npc: EntityId, v: bool) {
        if let Some(s) = self.script_mut(npc) {
            s.waiting = v;
        }
    }
    fn party_member_type(&self, npc: EntityId) -> se::PartyMemberType {
        if self.actor(npc).is_some_and(|a| a.summoned) {
            return se::PartyMemberType::Summoned;
        }
        self.script(npc).member_type
    }
    fn set_party_member_type(&mut self, npc: EntityId, t: se::PartyMemberType) {
        if let Some(s) = self.script_mut(npc) {
            s.member_type = t;
        }
        if let Some(a) = self.actor_mut(npc) {
            a.summoned = t == se::PartyMemberType::Summoned;
        }
    }
    fn party_members(&self, leader: EntityId) -> Vec<EntityId> {
        let Some(l) = actor_of(leader) else {
            return Vec::new();
        };
        self.sim
            .actors
            .iter()
            .filter(|a| a.party_leader == Some(l) && a.id != l)
            .map(|a| entity_of(a.id))
            .collect()
    }
    fn set_dialog_party_enabled(&mut self, npc: EntityId, v: bool) {
        if let Some(s) = self.script_mut(npc) {
            s.party_enabled = v;
        }
    }
    fn standing_in_water(&self, npc: EntityId) -> bool {
        self.actor(npc).is_some_and(|a| {
            matches!(
                a.ani_state,
                crate::types::AniState::Swim | crate::types::AniState::Dive
            )
        })
    }
}

impl<W: WorldQuery> InventoryApi for SimHost<'_, W> {
    fn has_items(&self, e: EntityId, template: &str, count: u32) -> bool {
        self.actor(e).is_some_and(|a| {
            a.inventory
                .iter()
                .any(|(k, n)| k.eq_ignore_ascii_case(template) && *n >= count)
        })
    }
    fn assure_items(&mut self, e: EntityId, template: &str, _flags: u32, count: u32) -> i32 {
        let Some(a) = self.actor_mut(e) else {
            return -1;
        };
        let key = a
            .inventory
            .keys()
            .find(|k| k.eq_ignore_ascii_case(template))
            .cloned()
            .unwrap_or_else(|| template.to_string());
        let n = a.inventory.entry(key.clone()).or_insert(0);
        if *n < count {
            *n = count;
        }
        a.inventory
            .keys()
            .position(|k| *k == key)
            .map_or(-1, |i| i as i32)
    }
    fn find_stack_index(&self, e: EntityId, slot: se::Slot) -> i32 {
        if self.equipped(e, slot).is_some() {
            slot as i32 + 1
        } else {
            -1
        }
    }
    fn unequip_stack(&mut self, e: EntityId, stack: i32) {
        if stack < 0 {
            return;
        }
        if let Some(a) = self.actor_mut(e) {
            if stack == se::Slot::RightHand as i32 + 1 {
                a.equipment.right = None;
            } else if stack == se::Slot::LeftHand as i32 + 1 {
                a.equipment.left = None;
            }
        }
    }
    fn hold_stacks(&mut self, _e: EntityId, _right: i32, _left: i32) {}
    fn drop_slot(&mut self, e: EntityId, slot: se::Slot) {
        if let Some(a) = actor_of(e) {
            self.sim.script.dropped.push((a, slot));
        }
        if let Some(a) = self.actor_mut(e) {
            match slot {
                se::Slot::RightHand => a.equipment.right = None,
                se::Slot::LeftHand | se::Slot::EquipLeftHand => a.equipment.left = None,
                _ => {}
            }
        }
    }
    fn delete_unequipped_stacks(&mut self, e: EntityId) {
        if let Some(a) = self.actor_mut(e) {
            a.inventory.clear();
        }
    }
    fn generate_plunder(&mut self, _e: EntityId) {}
    fn slot_use_type(&self, e: EntityId, slot: se::Slot) -> se::UseType {
        self.equipped(e, slot)
            .map_or(se::UseType::None, |i| s_use_kind(i.use_kind()))
    }
    fn stack_use_type(&self, _e: EntityId, _stack: i32) -> se::UseType {
        se::UseType::None
    }
    fn is_skill_active(&self, e: EntityId, perk: &str) -> bool {
        self.actor(e).is_some_and(|a| a.has_perk(perk))
    }
    fn slot_item_category(&self, e: EntityId, slot: se::Slot) -> u32 {
        self.equipped(e, slot)
            .map_or(0, |i| u32::from(!i.category.is_empty()))
    }
    fn slot_item_quality(&self, e: EntityId, slot: se::Slot) -> u32 {
        self.equipped(e, slot)
            .map_or(0, |i| i.quality.max(0) as u32)
    }
    fn slot_item_name(&self, e: EntityId, slot: se::Slot) -> String {
        self.equipped(e, slot)
            .map(|i| i.name.clone())
            .unwrap_or_default()
    }
    fn chest_is_robe(&self, e: EntityId) -> bool {
        self.actor(e).is_some_and(|a| a.equipment.wears_robe())
    }
    fn chest_protection_modifier(&self, e: EntityId, p: Protection) -> i32 {
        self.actor(e)
            .and_then(|a| a.equipment.body.as_ref())
            .map_or(0, |b| b.protection(protection_type(p)))
    }
    fn slot_spell_name(&self, e: EntityId, slot: se::Slot) -> String {
        self.equipped(e, slot)
            .and_then(|i| i.spell.clone())
            .unwrap_or_default()
    }
    fn slot_spell_deity(&self, _e: EntityId, _slot: se::Slot) -> se::SpellDeity {
        se::SpellDeity::None
    }
}

impl<W: WorldQuery> StatsApi for SimHost<'_, W> {
    fn pm_attribute(&self, attr: Attribute) -> i32 {
        let Some(h) = self.sim.hero else { return 0 };
        let a = self.sim.actor(h);
        let cfg = &self.sim.cfg;
        match attr {
            Attribute::Strength => a.strength(cfg),
            Attribute::Dexterity => a.dexterity(cfg),
            Attribute::Intelligence => a.intelligence(cfg),
            Attribute::Alchemy => a.base.alchemy,
            Attribute::Smithing => a.base.smithing,
            Attribute::Theft => a.theft(cfg),
        }
    }
    fn pm_set_attribute(&mut self, attr: Attribute, v: i32) {
        let Some(h) = self.sim.hero else { return };
        let b = &mut self.sim.actor_mut(h).base;
        match attr {
            Attribute::Strength => b.str = v,
            Attribute::Dexterity => b.dex = v,
            Attribute::Intelligence => b.int = v,
            Attribute::Alchemy => b.alchemy = v,
            Attribute::Smithing => b.smithing = v,
            Attribute::Theft => b.theft = v,
        }
    }
    fn pm_add_attribute(&mut self, attr: Attribute, delta: i32) {
        let Some(h) = self.sim.hero else { return };
        let b = &mut self.sim.actor_mut(h).base;
        match attr {
            Attribute::Strength => b.str += delta,
            Attribute::Dexterity => b.dex += delta,
            Attribute::Intelligence => b.int += delta,
            Attribute::Alchemy => b.alchemy += delta,
            Attribute::Smithing => b.smithing += delta,
            Attribute::Theft => b.theft += delta,
        }
    }
    fn pm_protection(&self, p: Protection) -> i32 {
        self.sim.hero.map_or(0, |h| {
            self.sim
                .actor(h)
                .equipment
                .protection_sum(protection_type(p))
        })
    }
    fn pm_set_protection(&mut self, _p: Protection, _v: i32) {}
    fn pm_add_protection(&mut self, _p: Protection, _delta: i32) {}
    fn pm_pool(&self, pool: Pool) -> i32 {
        self.sim.hero.map_or(0, |h| {
            let a = self.sim.actor(h);
            match pool {
                Pool::Hit => a.hp,
                Pool::Mana => a.mana,
                Pool::Stamina => a.stamina,
            }
        })
    }
    fn pm_pool_max(&self, pool: Pool) -> i32 {
        self.sim.hero.map_or(0, |h| {
            let a = self.sim.actor(h);
            match pool {
                Pool::Hit => a.hp_max(&self.sim.cfg),
                Pool::Mana => a.mana_max(),
                Pool::Stamina => a.stamina_max(&self.sim.cfg),
            }
        })
    }
    fn pm_set_pool(&mut self, pool: Pool, v: i32) {
        if let Some(h) = self.sim.hero {
            let a = self.sim.actor_mut(h);
            match pool {
                Pool::Hit => a.hp = v,
                Pool::Mana => a.mana = v,
                Pool::Stamina => a.stamina = v,
            }
        }
    }
    fn pm_set_pool_max(&mut self, pool: Pool, v: i32) {
        if let Some(h) = self.sim.hero {
            let b = &mut self.sim.actor_mut(h).base;
            match pool {
                Pool::Hit => b.hp_max = v,
                Pool::Mana => b.mana_max = v,
                Pool::Stamina => b.stamina_max = v,
            }
        }
    }
    fn pm_add_pool(&mut self, pool: Pool, delta: i32) {
        let max = self.pm_pool_max(pool);
        let now = self.pm_pool(pool);
        self.pm_set_pool(pool, (now + delta).clamp(0, max.max(0)));
    }
    fn pm_add_pool_max(&mut self, pool: Pool, delta: i32) {
        let max = self.pm_pool_max(pool);
        self.pm_set_pool_max(pool, max + delta);
    }
    fn chapter(&self) -> u32 {
        self.sim.chapter
    }
    fn political_fame(&self, alignment: se::Alignment) -> i32 {
        self.sim
            .state
            .fame
            .get(&format!("pol:{}", alignment_s(alignment).tag()))
            .copied()
            .unwrap_or(0)
    }
    fn enclave_player_fame(&self, enclave: EntityId) -> i32 {
        self.enclave_name(enclave).map_or(0, |n| {
            self.sim
                .state
                .fame
                .get(&format!("enc:{}", n.to_ascii_lowercase()))
                .copied()
                .unwrap_or(0)
        })
    }
    fn player_xp(&self) -> i64 {
        self.sim.state.xp as i64
    }
    fn set_player_xp(&mut self, xp: i64) {
        self.sim.state.xp = xp.max(0) as u64;
    }
    fn set_player_level(&mut self, level: u32) {
        self.sim.state.level = level;
        if let Some(h) = self.sim.hero {
            self.sim.actor_mut(h).level = level as i32;
        }
    }
    fn add_learning_points(&mut self, n: i32) {
        let lp = i64::from(self.sim.state.learning_points) + i64::from(n);
        self.sim.state.learning_points = lp.max(0) as u32;
    }
    fn player_level(&self) -> u32 {
        self.sim.state.level.max(1)
    }
    fn has_damage_receiver(&self, e: EntityId) -> bool {
        self.actor(e).is_some()
    }
    fn dr_pool(&self, e: EntityId, pool: Pool) -> i32 {
        self.actor(e).map_or(0, |a| match pool {
            Pool::Hit => a.hp,
            Pool::Mana => a.mana,
            Pool::Stamina => a.stamina,
        })
    }
    fn dr_pool_max(&self, e: EntityId, pool: Pool) -> i32 {
        self.actor(e).map_or(0, |a| match pool {
            Pool::Hit => a.hp_max(&self.sim.cfg),
            Pool::Mana => a.mana_max(),
            Pool::Stamina => a.stamina_max(&self.sim.cfg),
        })
    }
    fn dr_set_pool(&mut self, e: EntityId, pool: Pool, v: i32) {
        if let Some(a) = self.actor_mut(e) {
            match pool {
                Pool::Hit => a.hp = v,
                Pool::Mana => a.mana = v,
                Pool::Stamina => a.stamina = v,
            }
        }
    }
    fn dr_set_pool_max(&mut self, e: EntityId, pool: Pool, v: i32) {
        if let Some(a) = self.actor_mut(e) {
            match pool {
                Pool::Hit => a.base.hp_max = v,
                Pool::Mana => a.base.mana_max = v,
                Pool::Stamina => a.base.stamina_max = v,
            }
        }
    }
}

impl<W: WorldQuery> WorldApi for SimHost<'_, W> {
    fn all_npcs(&self) -> Vec<EntityId> {
        self.sim
            .actors
            .iter()
            .filter(|a| !a.is_player)
            .map(|a| entity_of(a.id))
            .collect()
    }
    fn distance(&self, a: EntityId, b: EntityId) -> f32 {
        self.entity_pos(a).dist(self.entity_pos(b))
    }
    fn has_free_los(&self, a: EntityId, b: EntityId) -> bool {
        self.sim
            .world
            .line_of_sight(self.entity_pos(a), self.entity_pos(b))
    }
    fn zone_is_outdoor(&self, zone: EntityId) -> bool {
        table_index(zone, KIND_ZONE)
            .and_then(|i| self.sim.script.zones.get(i))
            .is_some_and(|z| z.1)
    }
    fn is_standing(&self, _e: EntityId) -> bool {
        false
    }
    fn find_npc(&self, name: &str) -> EntityId {
        opt_entity(self.sim.find(name))
    }
    fn find_enclave(&self, name: &str) -> EntityId {
        self.sim
            .script
            .enclave_names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(name))
            .map_or(EntityId::NONE, |i| table_id(KIND_ENCLAVE, i))
    }
    fn quest_open(&self, name: &str) -> bool {
        self.sim
            .state
            .quests
            .get(name)
            .is_some_and(|q| q.status == Some(gothic_gameplay::model::QuestStatus::Running))
    }
    fn game_event_set(&self, name: &str) -> bool {
        self.sim.state.events.contains(name)
    }
    fn quest_succeeded(&self, name: &str) -> bool {
        self.sim
            .state
            .quests
            .get(name)
            .is_some_and(|q| q.status == Some(gothic_gameplay::model::QuestStatus::Success))
    }
    fn item_is_dropped(&self, item: EntityId) -> bool {
        table_index(item, KIND_ITEM)
            .and_then(|i| self.sim.script.items.get(i))
            .is_some_and(|i| i.dropped)
    }
    fn enclave_known_crime(&self, enclave: EntityId) -> se::PlayerCrime {
        self.enclave_name(enclave)
            .and_then(|n| self.sim.enclaves.get(&n))
            .map_or(se::PlayerCrime::None, |e| s_crime(e.known_crime))
    }
    fn set_enclave_known_crime(&mut self, enclave: EntityId, crime: se::PlayerCrime) {
        if let Some(e) = self
            .enclave_name(enclave)
            .and_then(|n| self.sim.enclaves.get_mut(&n))
        {
            e.known_crime = crime_s(crime);
        }
    }
    fn set_enclave_player_fame(&mut self, enclave: EntityId, fame: i32) {
        if let Some(n) = self.enclave_name(enclave) {
            self.sim
                .state
                .fame
                .insert(format!("enc:{}", n.to_ascii_lowercase()), fame);
        }
    }
    fn enclave_crime_count(&self, enclave: EntityId, crime: se::PlayerCrime) -> i32 {
        if let Some(e) = self
            .enclave_name(enclave)
            .and_then(|n| self.sim.enclaves.get(&n))
        {
            match crime {
                se::PlayerCrime::Theft => return e.theft_count as i32,
                se::PlayerCrime::Murder => return e.murder_count as i32,
                _ => {}
            }
        }
        self.enclave_extra(enclave)
            .counts
            .get(&crime)
            .copied()
            .unwrap_or(0)
    }
    fn set_enclave_crime_count(&mut self, enclave: EntityId, crime: se::PlayerCrime, n: i32) {
        if let Some(e) = self
            .enclave_name(enclave)
            .and_then(|nm| self.sim.enclaves.get_mut(&nm))
        {
            match crime {
                se::PlayerCrime::Theft => {
                    e.theft_count = n.max(0) as u32;
                    return;
                }
                se::PlayerCrime::Murder => {
                    e.murder_count = n.max(0) as u32;
                    return;
                }
                _ => {}
            }
        }
        if let Some(x) = self.enclave_extra_mut(enclave) {
            x.counts.insert(crime, n);
        }
    }
    fn clear_enclave_suspect_comment(&mut self, enclave: EntityId, _crime: se::PlayerCrime) {
        if let Some(e) = self
            .enclave_name(enclave)
            .and_then(|n| self.sim.enclaves.get_mut(&n))
        {
            e.suspect_comment = false;
        }
    }
    fn enclave_raid(&self, enclave: EntityId) -> bool {
        self.enclave_extra(enclave).raid
    }
    fn set_enclave_raid(&mut self, enclave: EntityId, v: bool) {
        if let Some(x) = self.enclave_extra_mut(enclave) {
            x.raid = v;
        }
    }
    fn enclave_revolution(&self, enclave: EntityId) -> bool {
        self.enclave_extra(enclave).revolution
    }
    fn set_enclave_revolution(&mut self, enclave: EntityId, v: bool) {
        if let Some(x) = self.enclave_extra_mut(enclave) {
            x.revolution = v;
        }
    }
    fn set_enclave_status_raw(&mut self, enclave: EntityId, status: se::EnclaveStatus) {
        if let Some(e) = self
            .enclave_name(enclave)
            .and_then(|n| self.sim.enclaves.get_mut(&n))
        {
            e.status = match status {
                se::EnclaveStatus::Attack => CEnclaveStatus::Attack,
                se::EnclaveStatus::Flee => CEnclaveStatus::Flee,
                se::EnclaveStatus::Routine => CEnclaveStatus::Routine,
            };
        }
    }
    fn enclave_members(&self, enclave: EntityId) -> Vec<EntityId> {
        let Some(name) = self.enclave_name(enclave) else {
            return Vec::new();
        };
        self.sim
            .actors
            .iter()
            .filter(|a| {
                a.enclave
                    .as_deref()
                    .is_some_and(|e| e.eq_ignore_ascii_case(&name))
            })
            .map(|a| entity_of(a.id))
            .collect()
    }
    fn political_crime_count(&self, alignment: se::Alignment) -> i32 {
        self.sim
            .script
            .political_crimes
            .get(&alignment)
            .copied()
            .unwrap_or(0)
    }
    fn set_political_crime_count(&mut self, alignment: se::Alignment, n: i32) {
        self.sim.script.political_crimes.insert(alignment, n);
    }
    fn political_player_crime(&self, alignment: se::Alignment) -> bool {
        self.sim.script.wanted.contains(&alignment)
    }
    fn is_swimming(&self, e: EntityId) -> bool {
        self.standing_in_water(e)
    }
    fn gui_page_open(&self) -> bool {
        self.sim.gui_open
    }
    fn on_npc_felled(&mut self, victim: EntityId, _killer: EntityId, killed: bool) {
        if let Some(a) = actor_of(victim) {
            self.sim.script.felled.push((a, killed));
        }
    }
    fn on_enclave_state_changed(&mut self, _enclave: EntityId) {}
}

impl<W: WorldQuery> SimHost<'_, W> {
    /// The entity handle of an actor.
    pub fn entity(&self, a: ActorId) -> EntityId {
        entity_of(a)
    }

    /// Entity of an enclave by name, registering it for the script on first use.
    pub fn enclave_entity(&mut self, name: &str) -> EntityId {
        self.sim.script.enclave_id(name)
    }

    /// Zone entity for the world's classes at an actor (registers it on first use).
    pub fn zone_entity(&mut self, a: ActorId) -> EntityId {
        let pos = self.sim.world.position(a).unwrap_or_default();
        let key = (
            self.sim.world.zone_security(pos),
            self.sim.world.is_outdoors(pos),
        );
        self.sim.script.zone_id(key.0, key.1)
    }

    /// Convert a script position.
    pub fn to_world(p: SPos) -> Pos {
        from_pos(p)
    }

    /// Drain the queued AI commands.
    pub fn take_commands(&mut self) -> Vec<(ActorId, AiCommand)> {
        self.sim.script.take_commands()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SimConfig;
    use crate::item::ItemDb;
    use crate::types::Species;
    use crate::world::Grid2dWorld;
    use gothic_script::damage::{HitEvent, assess_hit, give_xp};
    use gothic_script::routine::select::select_routine_task;
    use gothic_script::stats;

    fn sim() -> Sim<Grid2dWorld> {
        let mut s = Sim::new(SimConfig::default(), Grid2dWorld::new(), ItemDb::default());
        s.spawn_hero(Pos::flat(0.0, 0.0));
        s
    }

    #[test]
    fn stats_match_the_simulation() {
        let mut s = sim();
        let orc = s.spawn(
            Actor::npc(ActorId(0), "Orc", Species::Orc, 5, 30),
            Pos::flat(100.0, 0.0),
        );
        let cfg = s.cfg.clone();
        let a = s.actor(orc).clone();
        let host = SimHost::new(&mut s);
        let e = host.entity(orc);
        assert_eq!(
            stats::attribute(&host, e, Attribute::Strength),
            a.strength(&cfg)
        );
        assert_eq!(
            stats::attribute(&host, e, Attribute::Dexterity),
            a.dexterity(&cfg)
        );
        assert_eq!(stats::attribute(&host, e, Attribute::Theft), a.theft(&cfg));
        assert_eq!(stats::pool_max(&host, e, Pool::Hit), a.hp_max(&cfg));
        assert_eq!(
            stats::pool_max(&host, e, Pool::Stamina),
            a.stamina_max(&cfg)
        );
        for ty in [
            DamageType::Blade,
            DamageType::Impact,
            DamageType::Missile,
            DamageType::Fire,
            DamageType::Ice,
            DamageType::Lightning,
        ] {
            let prot = match ty {
                DamageType::Blade => Protection::Blades,
                DamageType::Impact => Protection::Impact,
                DamageType::Missile => Protection::Missile,
                DamageType::Fire => Protection::Fire,
                DamageType::Ice => Protection::Ice,
                _ => Protection::Lightning,
            };
            assert_eq!(
                stats::protection(&host, e, prot),
                a.protection(&cfg, ty),
                "{ty:?}"
            );
        }
        // The hero reads player memory: strength 100 from the new-game attributes.
        let h = host.player();
        assert_eq!(stats::attribute(&host, h, Attribute::Strength), 100);
    }

    fn duel_world() -> (Sim<Grid2dWorld>, ActorId, ActorId) {
        let mut s = sim();
        let hero = s.hero.unwrap();
        let orc = {
            let mut o = Actor::npc(ActorId(0), "Orc", Species::Orc, 5, 30);
            o.alignment = Alignment::Orc;
            s.spawn(o, Pos::flat(100.0, 0.0))
        };
        s.equip_def(
            hero,
            crate::item::ItemDef::weapon("It_Sword", "1H", "Blade", 50),
        )
        .expect("hero may wield the sword");
        (s, hero, orc)
    }

    #[test]
    fn script_damage_agrees_with_the_simulation() {
        // Differential check: the same hit through `resolve_hit` and through the ported
        // `AssessHit`, hero -> orc and orc -> hero, for every move multiplier.
        use crate::combat::{Attack, resolve_hit};
        use crate::types::Action as A;
        for (hero_hits, act, act_s) in [
            (true, A::Attack, se::Action::Attack),
            (true, A::QuickAttack, se::Action::QuickAttack),
            (true, A::HackAttack, se::Action::HackAttack),
            (true, A::PowerAttack, se::Action::PowerAttack),
            (false, A::Attack, se::Action::Attack),
            (false, A::QuickAttack, se::Action::QuickAttack),
            (false, A::HackAttack, se::Action::HackAttack),
        ] {
            let (mut reference_sim, hero, orc) = duel_world();
            let (att_id, vic_id) = if hero_hits { (hero, orc) } else { (orc, hero) };
            reference_sim.actor_mut(att_id).action = act;
            reference_sim.actor_mut(vic_id).hp = 100_000;
            let (att, mut vic) = (
                reference_sim.actor(att_id).clone(),
                reference_sim.actor(vic_id).clone(),
            );
            let attack = Attack::melee(&att);
            let mut rng = reference_sim.rng.clone();
            let reference = resolve_hit(&reference_sim.cfg, &mut rng, &att, &mut vic, &attack);

            let (mut port, hero, orc) = duel_world();
            let (att_id, vic_id) = if hero_hits { (hero, orc) } else { (orc, hero) };
            port.actor_mut(att_id).action = act;
            port.actor_mut(vic_id).hp = 100_000;
            let mut host = SimHost::new(&mut port);
            let (a, v) = (host.entity(att_id), host.entity(vic_id));
            host.set_current_action(a, act_s);
            let mut ev = HitEvent::melee(v, a, se::DamageType::Blade, attack.amount);
            ev.weapon_name = attack.entity_name.clone();
            ev.hit_mult = attack.hit_mult;
            let out = assess_hit(&mut host, &ev);
            assert_eq!(
                out.damage, reference.damage,
                "hero_hits={hero_hits} {act_s:?}: port {} vs sim {}",
                out.damage, reference.damage
            );
        }
    }

    #[test]
    fn xp_matches_the_simulation_rule() {
        let mut s = sim();
        let hero = s.hero.unwrap();
        let man = s.spawn(
            Actor::npc(ActorId(0), "Bandit", Species::Human, 12, 12),
            Pos::flat(50.0, 0.0),
        );
        let before = s.state.xp;
        let mut host = SimHost::new(&mut s);
        let (h, m) = (host.entity(hero), host.entity(man));
        assert_eq!(give_xp(&mut host, m, h, -1), 120);
        assert_eq!(s.state.xp, before + 120);
    }

    #[test]
    fn routine_tasks_run_on_the_simulation() {
        let mut s = sim();
        let smith = s.spawn(
            Actor::npc(ActorId(0), "Smith", Species::Human, 5, 5),
            Pos::flat(500.0, 0.0),
        );
        let anchor = s.script.add_anchor("FORGE", se::AnchorType::Local, 0);
        let point = s
            .script
            .add_point(se::UseType::Anvil, anchor, Pos::flat(520.0, 0.0));
        s.script.npc_mut(smith).dest = point;
        let mut host = SimHost::new(&mut s);
        let e = host.entity(smith);
        assert_eq!(select_routine_task(&mut host, e), State::Smith);
        assert_eq!(host.current_task(e), Some(State::Smith));
        // The state machine queues AI commands for the game layer.
        host.set_on_destination(e, true);
        assert!(host.is_on_destination(e));
    }
}
