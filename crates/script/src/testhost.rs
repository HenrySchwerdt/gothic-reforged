//! In-memory host used by the unit tests (and available to downstream integration tests).
//!
//! [`TestWorld`] implements every host trait over plain maps: a handful of NPCs, interact points,
//! anchors, enclaves and zones. It is deliberately dumb: AI commands are only logged, and
//! [`TestWorld::finish_commands`] "executes" them (a goto marks the NPC as arrived at an interact
//! point it targeted).

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use crate::config::{Difficulty, EngineSetup, Variants};
use crate::enums::{
    Action, AiMode, Alignment, AnchorType, AniState, AttackReason, Attitude, EnclaveStatus,
    GuardStatus, LastFight, NpcClass, NpcType, PartyMemberType, SecurityLevel, Slot, Species,
    SpellDeity, UseType,
};
use crate::host::{
    AiCommand, Attribute, EntityApi, EntityId, InventoryApi, NpcApi, Pool, Pos, Protection,
    ScriptCtx, StatsApi,
};
use crate::routine::state::{Callback, State};
use crate::routine::time::RoutineTime;

/// Player-memory store of the test world.
#[derive(Debug, Default, Clone)]
pub struct PlayerMemory {
    /// Base attributes.
    pub attributes: HashMap<Attribute, i32>,
    /// Protection values.
    pub protection: HashMap<Protection, i32>,
    /// `(current, max)` per pool.
    pub pools: HashMap<Pool, (i32, i32)>,
}

#[derive(Debug, Clone)]
struct Slotted {
    use_type: UseType,
    category: u32,
    spell: String,
    deity: SpellDeity,
}

#[derive(Debug, Clone)]
struct TestNpc {
    name: String,
    is_player: bool,
    species: Species,
    class: NpcClass,
    npc_type: NpcType,
    alignment: Alignment,
    guard_status: GuardStatus,
    guard_point: EntityId,
    level: u32,
    level_max: u32,
    transformed: bool,
    status: u32,
    attack_reason: AttackReason,
    last_player_ar: AttackReason,
    last_fight_ts: f32,
    last_fight: LastFight,
    ransacked: bool,
    target: EntityId,
    enclave: EntityId,
    ai_mode: AiMode,
    ani_state: AniState,
    task: Option<State>,
    state: Option<State>,
    task_cb: Callback,
    local_cb: Callback,
    state_time: f32,
    task_position: u32,
    progress: u32,
    dest: EntityId,
    on_dest: bool,
    start_pos: Option<Pos>,
    pos: Pos,
    zone: EntityId,
    routine_points: HashMap<RoutineTime, EntityId>,
    leader: EntityId,
    waiting: bool,
    member_type: PartyMemberType,
    dialog_party_enabled: bool,
    items: Vec<String>,
    slots: HashMap<Slot, Slotted>,
    skills: HashSet<String>,
    dr: HashMap<Pool, (i32, i32)>,
    use_objects: HashMap<UseType, EntityId>,
    revolution: Attitude,
    time_stamp_transformed: u32,
    pending: VecDeque<AiCommand>,
}

#[derive(Debug, Clone)]
struct TestPoint {
    use_type: UseType,
    anchor: EntityId,
    user: EntityId,
    owner: EntityId,
}

#[derive(Debug, Clone)]
struct TestAnchor {
    name: String,
    kind: AnchorType,
    max_users: u32,
    users: u32,
    points: Vec<EntityId>,
}

#[derive(Debug, Clone)]
struct TestEnclave {
    alignment: Alignment,
    status: EnclaveStatus,
    switched: bool,
    player_fame: i32,
}

/// The in-memory world.
#[derive(Debug)]
pub struct TestWorld {
    next_id: u32,
    player: EntityId,
    npcs: BTreeMap<EntityId, TestNpc>,
    points: BTreeMap<EntityId, TestPoint>,
    anchors: BTreeMap<EntityId, TestAnchor>,
    enclaves: BTreeMap<EntityId, TestEnclave>,
    zones: BTreeMap<EntityId, (EntityId, SecurityLevel)>,
    roaming: HashMap<String, Vec<EntityId>>,
    political_fame: HashMap<Alignment, i32>,
    smalltalk_partner: EntityId,
    rng: u64,
    /// Player memory.
    pub pm: PlayerMemory,
    /// Engine setup flags (shipped values by default).
    pub setup: EngineSetup,
    /// Behaviour variants.
    pub variants: Variants,
    /// Game difficulty.
    pub difficulty: Difficulty,
    /// Hour of the day.
    pub hour: f32,
    /// Play time in seconds.
    pub playing_time: f32,
    /// Scripted dice: consumed front to back, each `v` answers `random(n)` with `v % n`.
    pub rolls: Vec<i32>,
    /// God-mode cheat.
    pub god: bool,
    /// Is the player sprinting?
    pub player_sprinting: bool,
    /// Player chapter.
    pub chapter: u32,
    /// Every NPC is inside the processing range.
    pub in_range: bool,
    /// The dialog page is open with the NPC as partner.
    pub in_dialog: bool,
    /// An arena fight is running.
    pub arena: bool,
    /// Creatures stand in water.
    pub in_water: bool,
    /// SVM speech samples exist.
    pub has_speech: bool,
    /// Game messages `(key, name, kind)`.
    pub messages: Vec<(String, String, u32)>,
    /// Every queued AI command in order.
    pub log: Vec<AiCommand>,
    /// Number of `FullStop` calls.
    pub full_stops: u32,
    /// Stack indices passed to `unequip_stack` (ignoring `-1`).
    pub unequipped: Vec<i32>,
}

impl Default for TestWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl TestWorld {
    /// A world containing only the player (entity 1, level 1).
    pub fn new() -> Self {
        let mut w = Self {
            next_id: 1,
            player: EntityId::NONE,
            npcs: BTreeMap::new(),
            points: BTreeMap::new(),
            anchors: BTreeMap::new(),
            enclaves: BTreeMap::new(),
            zones: BTreeMap::new(),
            roaming: HashMap::new(),
            political_fame: HashMap::new(),
            smalltalk_partner: EntityId::NONE,
            rng: 0x2545_F491_4F6C_DD1D,
            pm: PlayerMemory::default(),
            setup: EngineSetup::default(),
            variants: Variants::default(),
            difficulty: Difficulty::Normal,
            hour: 12.0,
            playing_time: 0.0,
            rolls: Vec::new(),
            god: false,
            player_sprinting: false,
            chapter: 1,
            in_range: true,
            in_dialog: false,
            arena: false,
            in_water: false,
            has_speech: false,
            messages: Vec::new(),
            log: Vec::new(),
            full_stops: 0,
            unequipped: Vec::new(),
        };
        let p = w.spawn_npc("Hero", Species::Human, 1, 1);
        w.player = p;
        w.npcs.get_mut(&p).expect("player").is_player = true;
        w.pm.pools.insert(Pool::Hit, (200, 200));
        w.pm.pools.insert(Pool::Mana, (100, 100));
        w.pm.pools.insert(Pool::Stamina, (100, 100));
        w
    }

    fn alloc(&mut self) -> EntityId {
        self.next_id += 1;
        EntityId(self.next_id - 1)
    }

    fn npc(&self, e: EntityId) -> Option<&TestNpc> {
        self.npcs.get(&e)
    }

    fn npc_mut(&mut self, e: EntityId) -> Option<&mut TestNpc> {
        self.npcs.get_mut(&e)
    }

    /// Create an NPC. Hit points and stamina start at the script's own maximum formulas.
    pub fn spawn_npc(
        &mut self,
        name: &str,
        species: Species,
        level: u32,
        level_max: u32,
    ) -> EntityId {
        let id = self.alloc();
        let hp_max = (level_max as i32 * 20).max(100);
        let sta_max = (level_max as i32 * 10).max(100);
        let mut dr = HashMap::new();
        dr.insert(Pool::Hit, (hp_max, hp_max));
        dr.insert(Pool::Stamina, (sta_max, sta_max));
        self.npcs.insert(
            id,
            TestNpc {
                name: name.into(),
                is_player: false,
                species,
                class: NpcClass::Warrior,
                npc_type: NpcType::Personal,
                alignment: Alignment::None,
                guard_status: GuardStatus::Inactive,
                guard_point: EntityId::NONE,
                level,
                level_max,
                transformed: false,
                status: 0,
                attack_reason: AttackReason::None,
                last_player_ar: AttackReason::None,
                last_fight_ts: 0.0,
                last_fight: LastFight::None,
                ransacked: false,
                target: EntityId::NONE,
                enclave: EntityId::NONE,
                ai_mode: AiMode::Routine,
                ani_state: AniState::Stand,
                task: None,
                state: None,
                task_cb: Callback::None,
                local_cb: Callback::None,
                state_time: 0.0,
                task_position: 0,
                progress: 0,
                dest: EntityId::NONE,
                on_dest: false,
                start_pos: None,
                pos: Pos::default(),
                zone: EntityId::NONE,
                routine_points: HashMap::new(),
                leader: EntityId::NONE,
                waiting: false,
                member_type: PartyMemberType::None,
                dialog_party_enabled: true,
                items: Vec::new(),
                slots: HashMap::new(),
                skills: HashSet::new(),
                dr,
                use_objects: HashMap::new(),
                revolution: Attitude::None,
                time_stamp_transformed: 0,
                pending: VecDeque::new(),
            },
        );
        id
    }

    /// Add an anchor with `max_users` capacity (0 = number of points).
    pub fn add_anchor(&mut self, name: &str, kind: AnchorType, max_users: u32) -> EntityId {
        let id = self.alloc();
        self.anchors.insert(
            id,
            TestAnchor {
                name: name.into(),
                kind,
                max_users,
                users: 0,
                points: Vec::new(),
            },
        );
        id
    }

    /// Add an interact point (optionally belonging to an anchor).
    pub fn add_point(&mut self, use_type: UseType, anchor: EntityId) -> EntityId {
        let id = self.alloc();
        self.points.insert(
            id,
            TestPoint {
                use_type,
                anchor,
                user: EntityId::NONE,
                owner: EntityId::NONE,
            },
        );
        if let Some(a) = self.anchors.get_mut(&anchor) {
            a.points.push(id);
        }
        id
    }

    /// Add an enclave.
    pub fn add_enclave(&mut self, alignment: Alignment) -> EntityId {
        let id = self.alloc();
        self.enclaves.insert(
            id,
            TestEnclave {
                alignment,
                status: EnclaveStatus::Routine,
                switched: false,
                player_fame: 100,
            },
        );
        id
    }

    /// Set the player's level (`PSNpc.Level` and `LevelMax`).
    pub fn set_player_level(&mut self, level: u32) {
        let p = self.player;
        if let Some(n) = self.npc_mut(p) {
            n.level = level;
            n.level_max = n.level_max.max(level);
        }
    }

    /// Set the player's `LevelMax`.
    pub fn set_player_level_max(&mut self, level: u32) {
        let p = self.player;
        if let Some(n) = self.npc_mut(p) {
            n.level_max = level;
        }
    }

    /// Mark an NPC as transformed.
    pub fn set_transformed(&mut self, e: EntityId, v: bool) {
        if let Some(n) = self.npc_mut(e) {
            n.transformed = v;
        }
    }

    /// Give an entity an active perk.
    pub fn grant_skill(&mut self, e: EntityId, perk: &str) {
        if let Some(n) = self.npc_mut(e) {
            n.skills.insert(perk.into());
        }
    }

    /// Set the guard status.
    pub fn set_guard_status(&mut self, e: EntityId, s: GuardStatus) {
        if let Some(n) = self.npc_mut(e) {
            n.guard_status = s;
        }
    }

    /// Set guard status and guard point.
    pub fn set_guard(&mut self, e: EntityId, s: GuardStatus, point: EntityId) {
        if let Some(n) = self.npc_mut(e) {
            n.guard_status = s;
            n.guard_point = point;
        }
    }

    /// Set the political alignment.
    pub fn set_alignment(&mut self, e: EntityId, a: Alignment) {
        if let Some(n) = self.npc_mut(e) {
            n.alignment = a;
        }
    }

    /// Put an NPC into an enclave.
    pub fn set_enclave(&mut self, e: EntityId, enclave: EntityId) {
        if let Some(n) = self.npc_mut(e) {
            n.enclave = enclave;
        }
    }

    /// Set the NPC type.
    pub fn set_npc_type(&mut self, e: EntityId, t: NpcType) {
        if let Some(n) = self.npc_mut(e) {
            n.npc_type = t;
        }
    }

    /// Set the guild class.
    pub fn set_class(&mut self, e: EntityId, c: NpcClass) {
        if let Some(n) = self.npc_mut(e) {
            n.class = c;
        }
    }

    /// Set the enclave status.
    pub fn set_enclave_status(&mut self, enclave: EntityId, s: EnclaveStatus) {
        if let Some(x) = self.enclaves.get_mut(&enclave) {
            x.status = s;
        }
    }

    /// Set the enclave `Switched` flag.
    pub fn set_enclave_switched(&mut self, enclave: EntityId, v: bool) {
        if let Some(x) = self.enclaves.get_mut(&enclave) {
            x.switched = v;
        }
    }

    /// Set the enclave's own player fame.
    pub fn set_enclave_fame(&mut self, enclave: EntityId, v: i32) {
        if let Some(x) = self.enclaves.get_mut(&enclave) {
            x.player_fame = v;
        }
    }

    /// Set the player's political fame for an alignment.
    pub fn set_political_fame(&mut self, a: Alignment, v: i32) {
        self.political_fame.insert(a, v);
    }

    /// Set last-fight bookkeeping.
    pub fn set_last_fight(&mut self, e: EntityId, ts: f32, result: LastFight) {
        if let Some(n) = self.npc_mut(e) {
            n.last_fight_ts = ts;
            n.last_fight = result;
        }
    }

    /// Set `LastPlayerAR`.
    pub fn set_last_player_ar(&mut self, e: EntityId, ar: AttackReason) {
        if let Some(n) = self.npc_mut(e) {
            n.last_player_ar = ar;
        }
    }

    /// Set the attack reason.
    pub fn set_attack_reason(&mut self, e: EntityId, ar: AttackReason) {
        if let Some(n) = self.npc_mut(e) {
            n.attack_reason = ar;
        }
    }

    /// Set the current target without any side effect.
    pub fn set_target(&mut self, e: EntityId, t: EntityId) {
        if let Some(n) = self.npc_mut(e) {
            n.target = t;
        }
    }

    /// Put an entity in a zone.
    pub fn set_zone_of(&mut self, e: EntityId, zone: EntityId) {
        if let Some(n) = self.npc_mut(e) {
            n.zone = zone;
        }
    }

    /// Define a zone's owner and security level.
    pub fn set_zone(&mut self, zone: EntityId, owner: EntityId, level: SecurityLevel) {
        self.zones.insert(zone, (owner, level));
    }

    /// Fill an equipment slot.
    pub fn set_slot(&mut self, e: EntityId, slot: Slot, use_type: UseType, category: u32) {
        if let Some(n) = self.npc_mut(e) {
            n.slots.insert(
                slot,
                Slotted {
                    use_type,
                    category,
                    spell: String::new(),
                    deity: SpellDeity::None,
                },
            );
        }
    }

    /// Put weapons in both back slots (stacks 5 and 6).
    pub fn equip_back(&mut self, e: EntityId) {
        self.set_slot(e, Slot::BackLeft, UseType::OneH, 1);
        self.set_slot(e, Slot::BackRight, UseType::OneH, 1);
    }

    /// Set the reserved destination without touching counters.
    pub fn set_destination(&mut self, e: EntityId, p: EntityId) {
        if let Some(n) = self.npc_mut(e) {
            n.dest = p;
        }
    }

    /// Set the owner of an interact point.
    pub fn set_point_owner(&mut self, point: EntityId, owner: EntityId) {
        if let Some(p) = self.points.get_mut(&point) {
            p.owner = owner;
        }
    }

    /// Set party leadership bookkeeping directly.
    pub fn set_leader(&mut self, e: EntityId, leader: EntityId, t: PartyMemberType, waiting: bool) {
        if let Some(n) = self.npc_mut(e) {
            n.leader = leader;
            n.member_type = t;
            n.waiting = waiting;
        }
    }

    /// Assign the anchor of a routine time slot.
    pub fn set_routine_anchor(&mut self, e: EntityId, time: RoutineTime, anchor: EntityId) {
        if let Some(n) = self.npc_mut(e) {
            n.routine_points.insert(time, anchor);
        }
    }

    /// Define the roaming points of an anchor name.
    pub fn set_roaming_points(&mut self, name: &str, points: &[EntityId]) {
        self.roaming.insert(name.into(), points.to_vec());
    }

    /// Give an NPC an inventory item.
    pub fn give_item(&mut self, e: EntityId, item: &str) {
        if let Some(n) = self.npc_mut(e) {
            n.items.push(item.into());
        }
    }

    /// Does the NPC hold the item?
    pub fn has_item(&self, e: EntityId, item: &str) -> bool {
        self.npc(e)
            .is_some_and(|n| n.items.iter().any(|i| i == item))
    }

    /// Script `GetRevolutionAttitude`.
    pub fn set_revolution_attitude(&mut self, e: EntityId, a: Attitude) {
        if let Some(n) = self.npc_mut(e) {
            n.revolution = a;
        }
    }

    /// Set the status-effect mask.
    pub fn set_status(&mut self, e: EntityId, mask: u32) {
        if let Some(n) = self.npc_mut(e) {
            n.status = mask;
        }
    }

    /// `PSDialog.PartyEnabled`.
    pub fn dialog_party_enabled(&self, e: EntityId) -> bool {
        self.npc(e).is_some_and(|n| n.dialog_party_enabled)
    }

    /// Put the NPC on a task and state without side effects.
    pub fn set_task_directly(&mut self, e: EntityId, s: State) {
        if let Some(n) = self.npc_mut(e) {
            n.task = Some(s);
            n.state = Some(s);
            n.progress = 0;
            n.task_position = 0;
        }
    }

    /// Current task callback.
    pub fn callback(&self, e: EntityId) -> Callback {
        self.npc(e).map_or(Callback::None, |n| n.task_cb)
    }

    /// Current local callback.
    pub fn local_callback(&self, e: EntityId) -> Callback {
        self.npc(e).map_or(Callback::None, |n| n.local_cb)
    }

    /// Register the scanner result for a use type.
    pub fn set_use_object(&mut self, e: EntityId, use_type: UseType, object: EntityId) {
        if let Some(n) = self.npc_mut(e) {
            n.use_objects.insert(use_type, object);
        }
    }

    /// Define the chat partner the partner search returns.
    pub fn set_smalltalk_partner(&mut self, e: EntityId) {
        self.smalltalk_partner = e;
    }

    /// Move an entity.
    pub fn set_position(&mut self, e: EntityId, pos: Pos) {
        if let Some(n) = self.npc_mut(e) {
            n.pos = pos;
        }
    }

    /// Execute the queued AI commands of an NPC and return them. A goto to an interact point or
    /// the NPC's own destination marks it as arrived.
    pub fn finish_commands(&mut self, e: EntityId) -> Vec<AiCommand> {
        let cmds: Vec<AiCommand> = match self.npc_mut(e) {
            Some(n) => n.pending.drain(..).collect(),
            None => return Vec::new(),
        };
        for c in &cmds {
            if let AiCommand::GotoExt { target } = c
                && let Some(n) = self.npc_mut(e)
                && n.dest == *target
            {
                n.on_dest = true;
            }
        }
        cmds
    }

    fn roll(&mut self, n: i32) -> i32 {
        if n <= 0 {
            return 0;
        }
        if !self.rolls.is_empty() {
            let v = self.rolls.remove(0);
            return v.rem_euclid(n);
        }
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng % n as u64) as i32
    }
}

impl ScriptCtx for TestWorld {
    fn self_entity(&self) -> EntityId {
        self.player
    }
    fn other_entity(&self) -> EntityId {
        EntityId::NONE
    }
    fn setup(&self) -> EngineSetup {
        self.setup
    }
    fn variants(&self) -> Variants {
        self.variants
    }
    fn difficulty(&self) -> Difficulty {
        self.difficulty
    }
    fn random(&mut self, n: i32) -> i32 {
        self.roll(n)
    }
    fn playing_time(&self) -> f32 {
        self.playing_time
    }
    fn world_hour(&self) -> f32 {
        self.hour
    }
    fn queue_ai(&mut self, npc: EntityId, cmd: AiCommand) {
        self.log.push(cmd.clone());
        if let Some(n) = self.npc_mut(npc) {
            n.pending.push_back(cmd);
        }
    }
    fn print_game_message(&mut self, key: &str, name: &str, kind: u32) {
        self.messages.push((key.into(), name.into(), kind));
    }
}

impl EntityApi for TestWorld {
    fn player(&self) -> EntityId {
        self.player
    }
    fn exists(&self, e: EntityId) -> bool {
        e.is_some()
            && (self.npcs.contains_key(&e)
                || self.points.contains_key(&e)
                || self.anchors.contains_key(&e)
                || self.enclaves.contains_key(&e))
    }
    fn name(&self, e: EntityId) -> String {
        if let Some(n) = self.npc(e) {
            n.name.clone()
        } else if let Some(a) = self.anchors.get(&e) {
            a.name.clone()
        } else {
            String::new()
        }
    }
    fn position(&self, e: EntityId) -> Pos {
        self.npc(e).map_or(Pos::default(), |n| n.pos)
    }
    fn zone(&self, e: EntityId) -> EntityId {
        self.npc(e).map_or(EntityId::NONE, |n| n.zone)
    }
    fn zone_owner(&self, zone: EntityId) -> EntityId {
        self.zones.get(&zone).map_or(EntityId::NONE, |z| z.0)
    }
    fn zone_security(&self, zone: EntityId) -> SecurityLevel {
        self.zones.get(&zone).map_or(SecurityLevel::None, |z| z.1)
    }
    fn is_in_fov(&self, _observer: EntityId, _target: EntityId) -> bool {
        true
    }
    fn is_sprinting(&self, _e: EntityId) -> bool {
        self.player_sprinting
    }
    fn cheat_god(&self) -> bool {
        self.god
    }
    fn roaming_points_by_name(&self, anchor_name: &str) -> Vec<EntityId> {
        self.roaming.get(anchor_name).cloned().unwrap_or_default()
    }
    fn next_patrol_point(&self, current: EntityId) -> EntityId {
        // Patrol points of the test world: the next point of the same anchor.
        let Some(p) = self.points.get(&current) else {
            return EntityId::NONE;
        };
        let Some(a) = self.anchors.get(&p.anchor) else {
            return EntityId::NONE;
        };
        let i = a.points.iter().position(|&x| x == current).unwrap_or(0);
        a.points
            .get((i + 1) % a.points.len())
            .copied()
            .unwrap_or(EntityId::NONE)
    }
    fn random_navigation_point(&mut self, center: Pos, radius: f32) -> Pos {
        Pos::new(center.x + radius, center.y, center.z)
    }
}

impl NpcApi for TestWorld {
    fn species(&self, npc: EntityId) -> Species {
        self.npc(npc).map_or(Species::Human, |n| n.species)
    }
    fn npc_class(&self, npc: EntityId) -> NpcClass {
        self.npc(npc).map_or(NpcClass::None, |n| n.class)
    }
    fn npc_type(&self, npc: EntityId) -> NpcType {
        self.npc(npc).map_or(NpcType::Personal, |n| n.npc_type)
    }
    fn alignment(&self, npc: EntityId) -> Alignment {
        self.npc(npc).map_or(Alignment::None, |n| n.alignment)
    }
    fn guard_status(&self, npc: EntityId) -> GuardStatus {
        self.npc(npc)
            .map_or(GuardStatus::Inactive, |n| n.guard_status)
    }
    fn guard_point(&self, npc: EntityId) -> EntityId {
        self.npc(npc).map_or(EntityId::NONE, |n| n.guard_point)
    }
    fn level_raw(&self, npc: EntityId) -> u32 {
        self.npc(npc).map_or(1, |n| n.level)
    }
    fn level_max_raw(&self, npc: EntityId) -> u32 {
        self.npc(npc).map_or(1, |n| n.level_max)
    }
    fn is_transformed(&self, npc: EntityId) -> bool {
        self.npc(npc).is_some_and(|n| n.transformed)
    }
    fn has_status(&self, npc: EntityId, mask: u32) -> bool {
        self.npc(npc).is_some_and(|n| n.status & mask != 0)
    }
    fn attack_reason(&self, npc: EntityId) -> AttackReason {
        self.npc(npc)
            .map_or(AttackReason::None, |n| n.attack_reason)
    }
    fn last_player_attack_reason(&self, npc: EntityId) -> AttackReason {
        self.npc(npc)
            .map_or(AttackReason::None, |n| n.last_player_ar)
    }
    fn last_fight_timestamp(&self, npc: EntityId) -> f32 {
        self.npc(npc).map_or(0.0, |n| n.last_fight_ts)
    }
    fn set_last_fight_timestamp(&mut self, npc: EntityId, t: f32) {
        if let Some(n) = self.npc_mut(npc) {
            n.last_fight_ts = t;
        }
    }
    fn last_fight_vs_player(&self, npc: EntityId) -> LastFight {
        self.npc(npc).map_or(LastFight::None, |n| n.last_fight)
    }
    fn ransacked(&self, npc: EntityId) -> bool {
        self.npc(npc).is_some_and(|n| n.ransacked)
    }
    fn set_ransacked(&mut self, npc: EntityId, v: bool) {
        if let Some(n) = self.npc_mut(npc) {
            n.ransacked = v;
        }
    }
    fn current_target(&self, npc: EntityId) -> EntityId {
        self.npc(npc).map_or(EntityId::NONE, |n| n.target)
    }
    fn set_current_target(&mut self, npc: EntityId, target: EntityId) {
        if let Some(n) = self.npc_mut(npc) {
            n.target = target;
        }
    }
    fn enclave(&self, npc: EntityId) -> EntityId {
        self.npc(npc).map_or(EntityId::NONE, |n| n.enclave)
    }
    fn set_time_stamp_transformed(&mut self, npc: EntityId, t: u32) {
        if let Some(n) = self.npc_mut(npc) {
            n.time_stamp_transformed = t;
        }
    }
    fn primary_pose(&self, _npc: EntityId) -> u32 {
        0
    }
    fn arena_active(&self, _npc: EntityId) -> bool {
        self.arena
    }
    fn has_svm_sample(&self, _npc: EntityId, _family: &str) -> bool {
        self.has_speech
    }
    fn in_dialog_with_player(&self, _npc: EntityId) -> bool {
        self.in_dialog
    }
    fn find_smalltalk_partner(&self, _npc: EntityId) -> EntityId {
        self.smalltalk_partner
    }
    fn revolution_attitude(&self, npc: EntityId) -> Attitude {
        self.npc(npc).map_or(Attitude::None, |n| n.revolution)
    }
    fn enclave_status(&self, enclave: EntityId) -> EnclaveStatus {
        self.enclaves
            .get(&enclave)
            .map_or(EnclaveStatus::Routine, |e| e.status)
    }
    fn enclave_alignment(&self, enclave: EntityId) -> Alignment {
        self.enclaves
            .get(&enclave)
            .map_or(Alignment::None, |e| e.alignment)
    }
    fn enclave_switched(&self, enclave: EntityId) -> bool {
        self.enclaves.get(&enclave).is_some_and(|e| e.switched)
    }
    fn ai_mode(&self, npc: EntityId) -> AiMode {
        self.npc(npc).map_or(AiMode::Routine, |n| n.ai_mode)
    }
    fn set_ai_mode(&mut self, npc: EntityId, mode: AiMode) {
        if let Some(n) = self.npc_mut(npc) {
            n.ai_mode = mode;
        }
    }
    fn ani_state(&self, npc: EntityId) -> AniState {
        self.npc(npc).map_or(AniState::Stand, |n| n.ani_state)
    }
    fn current_action(&self, _npc: EntityId) -> Action {
        Action::None
    }
    fn current_task(&self, npc: EntityId) -> Option<State> {
        self.npc(npc).and_then(|n| n.task)
    }
    fn set_task(&mut self, npc: EntityId, task: State) {
        if let Some(n) = self.npc_mut(npc) {
            n.task = Some(task);
            n.state = Some(task);
            n.progress = 0;
            n.task_position = 0;
            n.state_time = 0.0;
        }
    }
    fn current_state(&self, npc: EntityId) -> Option<State> {
        self.npc(npc).and_then(|n| n.state)
    }
    fn set_state(&mut self, npc: EntityId, state: State) {
        if let Some(n) = self.npc_mut(npc) {
            n.state = Some(state);
            n.progress = 0;
            n.state_time = 0.0;
        }
    }
    fn set_task_callback(&mut self, npc: EntityId, cb: Callback) {
        if let Some(n) = self.npc_mut(npc) {
            n.task_cb = cb;
        }
    }
    fn set_local_callback(&mut self, npc: EntityId, cb: Callback) {
        if let Some(n) = self.npc_mut(npc) {
            n.local_cb = cb;
        }
    }
    fn state_time(&self, npc: EntityId) -> f32 {
        self.npc(npc).map_or(0.0, |n| n.state_time)
    }
    fn set_state_time(&mut self, npc: EntityId, t: f32) {
        if let Some(n) = self.npc_mut(npc) {
            n.state_time = t;
        }
    }
    fn task_position(&self, npc: EntityId) -> u32 {
        self.npc(npc).map_or(0, |n| n.task_position)
    }
    fn set_task_position(&mut self, npc: EntityId, pos: u32) {
        if let Some(n) = self.npc_mut(npc) {
            n.task_position = pos;
        }
    }
    fn state_progress(&self, npc: EntityId) -> u32 {
        self.npc(npc).map_or(0, |n| n.progress)
    }
    fn set_state_progress(&mut self, npc: EntityId, progress: u32) {
        if let Some(n) = self.npc_mut(npc) {
            n.progress = progress;
        }
    }
    fn full_stop(&mut self, npc: EntityId) {
        self.full_stops += 1;
        if let Some(n) = self.npc_mut(npc) {
            n.pending.clear();
        }
    }
    fn destination_point(&self, npc: EntityId) -> EntityId {
        self.npc(npc).map_or(EntityId::NONE, |n| n.dest)
    }
    fn set_destination_point(&mut self, npc: EntityId, point: EntityId) {
        if let Some(n) = self.npc_mut(npc) {
            n.dest = point;
        }
    }
    fn is_on_destination(&self, npc: EntityId) -> bool {
        self.npc(npc).is_some_and(|n| n.on_dest)
    }
    fn set_on_destination(&mut self, npc: EntityId, v: bool) {
        if let Some(n) = self.npc_mut(npc) {
            n.on_dest = v;
        }
    }
    fn is_travelling(&self, _npc: EntityId) -> bool {
        false
    }
    fn in_processing_range(&self, _npc: EntityId) -> bool {
        self.in_range
    }
    fn start_position(&self, npc: EntityId) -> Option<Pos> {
        self.npc(npc).and_then(|n| n.start_pos)
    }
    fn set_start_position(&mut self, npc: EntityId, pos: Pos) {
        if let Some(n) = self.npc_mut(npc) {
            n.start_pos = Some(pos);
        }
    }
    fn routine_point(&self, npc: EntityId, time: RoutineTime) -> EntityId {
        self.npc(npc)
            .and_then(|n| n.routine_points.get(&time).copied())
            .unwrap_or(EntityId::NONE)
    }
    fn point_use_type(&self, point: EntityId) -> UseType {
        self.points
            .get(&point)
            .map_or(UseType::None, |p| p.use_type)
    }
    fn point_anchor(&self, point: EntityId) -> EntityId {
        self.points.get(&point).map_or(EntityId::NONE, |p| p.anchor)
    }
    fn point_user(&self, point: EntityId) -> EntityId {
        self.points.get(&point).map_or(EntityId::NONE, |p| p.user)
    }
    fn set_point_user(&mut self, point: EntityId, user: EntityId) {
        if let Some(p) = self.points.get_mut(&point) {
            p.user = user;
        }
    }
    fn point_owner(&self, point: EntityId) -> EntityId {
        self.points.get(&point).map_or(EntityId::NONE, |p| p.owner)
    }
    fn anchor_type(&self, anchor: EntityId) -> AnchorType {
        self.anchors
            .get(&anchor)
            .map_or(AnchorType::Local, |a| a.kind)
    }
    fn anchor_max_users(&self, anchor: EntityId) -> u32 {
        self.anchors.get(&anchor).map_or(0, |a| a.max_users)
    }
    fn set_anchor_max_users(&mut self, anchor: EntityId, v: u32) {
        if let Some(a) = self.anchors.get_mut(&anchor) {
            a.max_users = v;
        }
    }
    fn anchor_user_count(&self, anchor: EntityId) -> u32 {
        self.anchors.get(&anchor).map_or(0, |a| a.users)
    }
    fn set_anchor_user_count(&mut self, anchor: EntityId, v: u32) {
        if let Some(a) = self.anchors.get_mut(&anchor) {
            a.users = v;
        }
    }
    fn anchor_points(&self, anchor: EntityId) -> Vec<EntityId> {
        self.anchors
            .get(&anchor)
            .map(|a| a.points.clone())
            .unwrap_or_default()
    }
    fn find_use_object(&self, npc: EntityId, use_type: UseType) -> EntityId {
        self.npc(npc)
            .and_then(|n| n.use_objects.get(&use_type).copied())
            .unwrap_or(EntityId::NONE)
    }
    fn party_leader(&self, npc: EntityId) -> EntityId {
        self.npc(npc).map_or(EntityId::NONE, |n| n.leader)
    }
    fn set_party_leader(&mut self, npc: EntityId, leader: EntityId) {
        if let Some(n) = self.npc_mut(npc) {
            n.leader = leader;
        }
    }
    fn party_waiting(&self, npc: EntityId) -> bool {
        self.npc(npc).is_some_and(|n| n.waiting)
    }
    fn set_party_waiting(&mut self, npc: EntityId, v: bool) {
        if let Some(n) = self.npc_mut(npc) {
            n.waiting = v;
        }
    }
    fn party_member_type(&self, npc: EntityId) -> PartyMemberType {
        self.npc(npc)
            .map_or(PartyMemberType::None, |n| n.member_type)
    }
    fn set_party_member_type(&mut self, npc: EntityId, t: PartyMemberType) {
        if let Some(n) = self.npc_mut(npc) {
            n.member_type = t;
        }
    }
    fn party_members(&self, leader: EntityId) -> Vec<EntityId> {
        self.npcs
            .iter()
            .filter(|(id, n)| n.leader == leader && leader.is_some() && **id != leader)
            .map(|(id, _)| *id)
            .collect()
    }
    fn set_dialog_party_enabled(&mut self, npc: EntityId, v: bool) {
        if let Some(n) = self.npc_mut(npc) {
            n.dialog_party_enabled = v;
        }
    }
    fn standing_in_water(&self, _npc: EntityId) -> bool {
        self.in_water
    }
}

impl InventoryApi for TestWorld {
    fn has_items(&self, e: EntityId, template: &str, count: u32) -> bool {
        self.npc(e)
            .is_some_and(|n| n.items.iter().filter(|i| *i == template).count() as u32 >= count)
    }
    fn assure_items(&mut self, e: EntityId, template: &str, _flags: u32, count: u32) -> i32 {
        if let Some(n) = self.npc_mut(e) {
            if let Some(i) = n.items.iter().position(|i| i == template) {
                return i as i32;
            }
            for _ in 0..count {
                n.items.push(template.into());
            }
            return n.items.len() as i32 - count as i32;
        }
        -1
    }
    fn find_stack_index(&self, e: EntityId, slot: Slot) -> i32 {
        if self.npc(e).is_some_and(|n| n.slots.contains_key(&slot)) {
            slot as i32 + 1
        } else {
            -1
        }
    }
    fn unequip_stack(&mut self, _e: EntityId, stack: i32) {
        if stack >= 0 {
            self.unequipped.push(stack);
        }
    }
    fn slot_use_type(&self, e: EntityId, slot: Slot) -> UseType {
        self.npc(e)
            .and_then(|n| n.slots.get(&slot))
            .map_or(UseType::None, |s| s.use_type)
    }
    fn stack_use_type(&self, _e: EntityId, _stack: i32) -> UseType {
        UseType::None
    }
    fn is_skill_active(&self, e: EntityId, perk: &str) -> bool {
        self.npc(e).is_some_and(|n| n.skills.contains(perk))
    }
    fn slot_item_category(&self, e: EntityId, slot: Slot) -> u32 {
        self.npc(e)
            .and_then(|n| n.slots.get(&slot))
            .map_or(0, |s| s.category)
    }
    fn slot_spell_name(&self, e: EntityId, slot: Slot) -> String {
        self.npc(e)
            .and_then(|n| n.slots.get(&slot))
            .map(|s| s.spell.clone())
            .unwrap_or_default()
    }
    fn slot_spell_deity(&self, e: EntityId, slot: Slot) -> SpellDeity {
        self.npc(e)
            .and_then(|n| n.slots.get(&slot))
            .map_or(SpellDeity::None, |s| s.deity)
    }
}

impl StatsApi for TestWorld {
    fn pm_attribute(&self, attr: Attribute) -> i32 {
        self.pm.attributes.get(&attr).copied().unwrap_or(0)
    }
    fn pm_set_attribute(&mut self, attr: Attribute, v: i32) {
        self.pm.attributes.insert(attr, v);
    }
    fn pm_add_attribute(&mut self, attr: Attribute, delta: i32) {
        *self.pm.attributes.entry(attr).or_insert(0) += delta;
    }
    fn pm_protection(&self, p: Protection) -> i32 {
        self.pm.protection.get(&p).copied().unwrap_or(0)
    }
    fn pm_set_protection(&mut self, p: Protection, v: i32) {
        self.pm.protection.insert(p, v);
    }
    fn pm_add_protection(&mut self, p: Protection, delta: i32) {
        *self.pm.protection.entry(p).or_insert(0) += delta;
    }
    fn pm_pool(&self, pool: Pool) -> i32 {
        self.pm.pools.get(&pool).map_or(0, |p| p.0)
    }
    fn pm_pool_max(&self, pool: Pool) -> i32 {
        self.pm.pools.get(&pool).map_or(0, |p| p.1)
    }
    fn pm_set_pool(&mut self, pool: Pool, v: i32) {
        let e = self.pm.pools.entry(pool).or_insert((0, 0));
        e.0 = v;
    }
    fn pm_set_pool_max(&mut self, pool: Pool, v: i32) {
        let e = self.pm.pools.entry(pool).or_insert((0, 0));
        e.1 = v;
    }
    fn pm_add_pool(&mut self, pool: Pool, delta: i32) {
        let e = self.pm.pools.entry(pool).or_insert((0, 0));
        e.0 = (e.0 + delta).clamp(0, e.1.max(0));
    }
    fn pm_add_pool_max(&mut self, pool: Pool, delta: i32) {
        let e = self.pm.pools.entry(pool).or_insert((0, 0));
        e.1 += delta;
    }
    fn chapter(&self) -> u32 {
        self.chapter
    }
    fn political_fame(&self, alignment: Alignment) -> i32 {
        self.political_fame.get(&alignment).copied().unwrap_or(0)
    }
    fn enclave_player_fame(&self, enclave: EntityId) -> i32 {
        self.enclaves.get(&enclave).map_or(0, |e| e.player_fame)
    }
    fn player_level(&self) -> u32 {
        self.npc(self.player).map_or(1, |n| n.level)
    }
    fn has_damage_receiver(&self, e: EntityId) -> bool {
        self.npc(e).is_some_and(|n| !n.dr.is_empty())
    }
    fn dr_pool(&self, e: EntityId, pool: Pool) -> i32 {
        self.npc(e).and_then(|n| n.dr.get(&pool)).map_or(0, |p| p.0)
    }
    fn dr_pool_max(&self, e: EntityId, pool: Pool) -> i32 {
        self.npc(e).and_then(|n| n.dr.get(&pool)).map_or(0, |p| p.1)
    }
    fn dr_set_pool(&mut self, e: EntityId, pool: Pool, v: i32) {
        if let Some(n) = self.npc_mut(e) {
            n.dr.entry(pool).or_insert((0, 0)).0 = v;
        }
    }
    fn dr_set_pool_max(&mut self, e: EntityId, pool: Pool, v: i32) {
        if let Some(n) = self.npc_mut(e) {
            n.dr.entry(pool).or_insert((0, 0)).1 = v;
        }
    }
}
