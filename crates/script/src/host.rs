//! Engine-interface traits: everything the ported script functions need from the host.
//!
//! Mirrors the split in `SCRIPT_ENGINE_INTERFACE.md` section 3, trimmed to the calls the ported
//! batches (B00-B05) actually make. A host (the real simulation, or [`crate::testhost::TestWorld`])
//! implements the traits; script code takes `&mut dyn ScriptHost`.
//!
//! Conventions: reads take `&self`, writes `&mut self`. An absent entity is [`EntityId::NONE`]
//! (the engine's global `None` sentinel). Distances are centimetres, times seconds.

use crate::config::{Difficulty, EngineSetup, Variants};
use crate::enums::{
    Action, AiMode, Alignment, AnchorType, AniState, AttackReason, Attitude, EnclaveStatus,
    GuardStatus, LastFight, NpcClass, NpcType, PartyMemberType, SecurityLevel, Slot, Species,
    SpellDeity, UseType,
};
use crate::routine::state::{Callback, State};
use crate::routine::time::RoutineTime;

/// Copyable entity handle (`Entity` in the script ABI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct EntityId(pub u32);

impl EntityId {
    /// The `None` sentinel entity.
    pub const NONE: EntityId = EntityId(0);
    /// True unless this is [`EntityId::NONE`].
    pub const fn is_some(self) -> bool {
        self.0 != 0
    }
    /// True for [`EntityId::NONE`].
    pub const fn is_none(self) -> bool {
        self.0 == 0
    }
}

/// World position.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pos {
    /// X (cm).
    pub x: f32,
    /// Y (cm, up).
    pub y: f32,
    /// Z (cm).
    pub z: f32,
}

impl Pos {
    /// Construct.
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
}

/// The six base attributes (`PSPlayerMemory` base values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Attribute {
    /// STR.
    Strength,
    /// DEX.
    Dexterity,
    /// INT.
    Intelligence,
    /// Alchemy skill.
    Alchemy,
    /// Smithing skill.
    Smithing,
    /// Theft skill.
    Theft,
}

/// The six protection channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Protection {
    /// Blades.
    Blades,
    /// Impact.
    Impact,
    /// Missile.
    Missile,
    /// Fire.
    Fire,
    /// Ice.
    Ice,
    /// Lightning.
    Lightning,
}

/// A resource pool (current value and maximum).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Pool {
    /// Hit points.
    Hit,
    /// Mana.
    Mana,
    /// Stamina.
    Stamina,
}

/// One queued AI instruction (`_AI_*` script functions). The host executes them in order and only
/// resumes the state once the queue is empty ("polled until finished").
#[derive(Debug, Clone, PartialEq)]
pub enum AiCommand {
    /// `_AI_StandUp`: leave any sitting/lying pose.
    StandUp,
    /// `_AI_HoldItems`: hold template items in the right/left hand (empty template = free hand).
    HoldItems {
        /// Right-hand template.
        right: &'static str,
        /// Left-hand template.
        left: &'static str,
    },
    /// `_AI_HoldInventoryItems` with stack indices; `-1` means "holster / hold nothing".
    HoldInventoryStacks {
        /// Right-hand stack index.
        right: i32,
        /// Left-hand stack index.
        left: i32,
    },
    /// `_AI_Action`: play a use animation for `secs` seconds at the current point.
    Action {
        /// Animation action.
        action: Action,
        /// Duration.
        secs: i32,
    },
    /// `_AI_Action` with no explicit action: the engine derives the animation from the point's
    /// use type (Cook, Grind, Barbecue, Bard, Lumberjack and the Trader stand variants).
    ActionAtPoint {
        /// Duration.
        secs: i32,
    },
    /// `_AI_AniStateAction`: enter a persistent pose (sit, kneel, sleep).
    AniStateAction(Action),
    /// `_AI_ChangeAction`.
    ChangeAction,
    /// `_AI_Enjoy`: idle in place, optionally with two held items, for `secs`.
    Enjoy {
        /// First item template (may be empty).
        item_a: &'static str,
        /// Second item template (may be empty).
        item_b: &'static str,
        /// Duration.
        secs: i32,
    },
    /// `_AI_Goto_Ext`: walk to an entity.
    GotoExt {
        /// Destination entity (a point or an object).
        target: EntityId,
    },
    /// `_AI_Goto_Ext` to a world position (ambient wandering).
    GotoPos {
        /// Destination.
        pos: Pos,
    },
    /// `sAIWaitInstr`: idle for `secs` seconds.
    Wait {
        /// Seconds.
        secs: i32,
    },
    /// `_AI_SaySVM`: play a voice-sample line (`Speech_00`, `Smalltalk_03`...).
    SaySvm {
        /// Sample family prefix.
        family: &'static str,
        /// Sample index.
        index: i32,
    },
    /// `_AI_UseItem`.
    UseItem(&'static str),
    /// `_AI_Cast`: cast the spell on the hand slot.
    Cast {
        /// Use the power-cast animation.
        power: bool,
    },
    /// `_AI_SelectWeapon`.
    SelectWeapon,
}

/// `gCScriptProcessingUnit` + `gCScriptAdmin` + engine globals: the per-invocation context.
pub trait ScriptCtx {
    /// `GetSelfEntity`.
    fn self_entity(&self) -> EntityId;
    /// `GetOtherEntity`.
    fn other_entity(&self) -> EntityId;
    /// `eCApplication::GetEngineSetup` (shipped `ge3.INI` values by default).
    fn setup(&self) -> EngineSetup;
    /// Named behaviour variants (default: shipped game).
    fn variants(&self) -> Variants;
    /// Game difficulty.
    fn difficulty(&self) -> Difficulty;
    /// `Entity::GetRandomNumber(n)`: uniform in `0..n` (`0` when `n <= 0`).
    fn random(&mut self, n: i32) -> i32;
    /// `Entity::GetPlayingTime`: seconds of play time.
    fn playing_time(&self) -> f32;
    /// `Entity::GetWorldTime`: hour of the day in `0.0..24.0`.
    fn world_hour(&self) -> f32;
    /// Enqueue one AI instruction for `npc`.
    fn queue_ai(&mut self, npc: EntityId, cmd: AiCommand);
    /// `GUI2::PrintGameMessage` with a localised-string key; `$(name)` is replaced by `name`.
    fn print_game_message(&mut self, key: &str, name: &str, kind: u32);
}

/// `Entity`, `eCEntity` and scene lookups.
pub trait EntityApi {
    /// `Entity::GetPlayer`.
    fn player(&self) -> EntityId;
    /// True if the handle refers to a live entity.
    fn exists(&self, e: EntityId) -> bool;
    /// `Entity::GetName`.
    fn name(&self, e: EntityId) -> String;
    /// `Entity::GetPosition`.
    fn position(&self, e: EntityId) -> Pos;
    /// `Entity::GetZone`.
    fn zone(&self, e: EntityId) -> EntityId;
    /// `Entity::IsInFOV(observer, target)`.
    fn is_in_fov(&self, observer: EntityId, target: EntityId) -> bool;
    /// `Entity::IsSprinting`.
    fn is_sprinting(&self, e: EntityId) -> bool;
    /// `Entity::IsCheatGodEnabled`.
    fn cheat_god(&self) -> bool;
    /// `Entity::GetAIZoneOwner`: the enclave entity owning a zone (`NONE` if unowned).
    fn zone_owner(&self, zone: EntityId) -> EntityId;
    /// `Entity::GetAIZoneSecurityLevel`.
    fn zone_security(&self, zone: EntityId) -> SecurityLevel;
    /// `Entity::GetRoamingPointsByName`.
    fn roaming_points_by_name(&self, anchor_name: &str) -> Vec<EntityId>;
    /// `Entity::GetNextPatrolPoint`.
    fn next_patrol_point(&self, current: EntityId) -> EntityId;
    /// `Entity::GetRandomNavigationPoint` around `center` within `radius`.
    fn random_navigation_point(&mut self, center: Pos, radius: f32) -> Pos;
}

/// `PSNpc`, `PSRoutine`, `PSNavigation`, `PSParty`, `PSEnclave`, `PSInteraction`/`PSAnchor`
/// (interact points and anchors), `PSCharacterMovement`.
pub trait NpcApi {
    // ---- PSNpc ---------------------------------------------------------------------------
    /// `PSNpc.Species`.
    fn species(&self, npc: EntityId) -> Species;
    /// `PSNpc.Class`.
    fn npc_class(&self, npc: EntityId) -> NpcClass;
    /// `PSNpc.Type`.
    fn npc_type(&self, npc: EntityId) -> NpcType;
    /// `PSNpc.PoliticalAlignment`.
    fn alignment(&self, npc: EntityId) -> Alignment;
    /// `PSNpc.GuardStatus`.
    fn guard_status(&self, npc: EntityId) -> GuardStatus;
    /// `PSNpc::GetGuardPoint`.
    fn guard_point(&self, npc: EntityId) -> EntityId;
    /// `PSNpc.Level` (raw property).
    fn level_raw(&self, npc: EntityId) -> u32;
    /// `PSNpc.LevelMax` (raw property).
    fn level_max_raw(&self, npc: EntityId) -> u32;
    /// `PSNpc::IsTransformed`.
    fn is_transformed(&self, npc: EntityId) -> bool;
    /// `PSNpc::HasStatusEffects(mask)`.
    fn has_status(&self, npc: EntityId, mask: u32) -> bool;
    /// `PSNpc.AttackReason`.
    fn attack_reason(&self, npc: EntityId) -> AttackReason;
    /// `PSNpc.LastPlayerAR`.
    fn last_player_attack_reason(&self, npc: EntityId) -> AttackReason;
    /// `PSNpc.LastFightTimestamp` (play-time seconds, `0` = never).
    fn last_fight_timestamp(&self, npc: EntityId) -> f32;
    /// Write `PSNpc.LastFightTimestamp`.
    fn set_last_fight_timestamp(&mut self, npc: EntityId, t: f32);
    /// `PSNpc.LastFightAgainstPlayer`.
    fn last_fight_vs_player(&self, npc: EntityId) -> LastFight;
    /// `PSNpc.Ransacked`.
    fn ransacked(&self, npc: EntityId) -> bool;
    /// Write `PSNpc.Ransacked`.
    fn set_ransacked(&mut self, npc: EntityId, v: bool);
    /// `PSNpc::GetCurrentTarget`.
    fn current_target(&self, npc: EntityId) -> EntityId;
    /// `PSNpc::SetCurrentTarget`.
    fn set_current_target(&mut self, npc: EntityId, target: EntityId);
    /// `PSNpc::GetEnclave`.
    fn enclave(&self, npc: EntityId) -> EntityId;
    /// `PSNpc.TimeStampTransformed` write (used when leaving a party).
    fn set_time_stamp_transformed(&mut self, npc: EntityId, t: u32);
    /// `PSNpc::GetPrimaryPose`: 1 and 2 are the unarmed/relaxed poses.
    fn primary_pose(&self, npc: EntityId) -> u32;

    /// `PSArena.Status == Running` for the arena entity the NPC takes part in.
    fn arena_active(&self, npc: EntityId) -> bool;
    /// An SVM voice sample family exists for the NPC (`Speech_00` for `ZS_Preach_SVM`).
    fn has_svm_sample(&self, npc: EntityId, family: &str) -> bool;
    /// The dialog page is open and `npc` is the player's conversation partner.
    fn in_dialog_with_player(&self, npc: EntityId) -> bool;
    /// Partner search of `ZS_HangAround` / `ZS_Smalltalk`: a free NPC in range to chat with.
    fn find_smalltalk_partner(&self, npc: EntityId) -> EntityId;
    /// `GetRevolutionAttitude` (S:ea50, not ported yet): attitude of the NPC in an enclave
    /// uprising or flight.
    fn revolution_attitude(&self, npc: EntityId) -> Attitude;

    // ---- PSEnclave -----------------------------------------------------------------------
    /// `PSEnclave.Status`.
    fn enclave_status(&self, enclave: EntityId) -> EnclaveStatus;
    /// `PSEnclave.PoliticalAlignment`.
    fn enclave_alignment(&self, enclave: EntityId) -> Alignment;
    /// `PSEnclave.Switched`.
    fn enclave_switched(&self, enclave: EntityId) -> bool;

    // ---- PSRoutine -----------------------------------------------------------------------
    /// `PSRoutine.AIMode`.
    fn ai_mode(&self, npc: EntityId) -> AiMode;
    /// Write `PSRoutine.AIMode`.
    fn set_ai_mode(&mut self, npc: EntityId, mode: AiMode);
    /// `PSRoutine.AniState`.
    fn ani_state(&self, npc: EntityId) -> AniState;
    /// `PSRoutine.Action`.
    fn current_action(&self, npc: EntityId) -> Action;
    /// `PSRoutine::GetCurrentTask`.
    fn current_task(&self, npc: EntityId) -> Option<State>;
    /// `PSRoutine::SetTask`: replace the task (resets state, progress and task position).
    fn set_task(&mut self, npc: EntityId, task: State);
    /// `PSRoutine::GetCurrentState`.
    fn current_state(&self, npc: EntityId) -> Option<State>;
    /// `PSRoutine::SetState`: replace the state (resets its step progress).
    fn set_state(&mut self, npc: EntityId, state: State);
    /// `PSRoutine::SetTaskCallback`.
    fn set_task_callback(&mut self, npc: EntityId, cb: Callback);
    /// `PSRoutine::SetLocalCallback`.
    fn set_local_callback(&mut self, npc: EntityId, cb: Callback);
    /// `PSRoutine::GetStateTime` (seconds in the current state).
    fn state_time(&self, npc: EntityId) -> f32;
    /// `PSRoutine::SetStateTime`.
    fn set_state_time(&mut self, npc: EntityId, t: f32);
    /// `PSRoutine.TaskPosition` (cycled by multi-object tasks such as `ZS_Smith`).
    fn task_position(&self, npc: EntityId) -> u32;
    /// Write `PSRoutine.TaskPosition`.
    fn set_task_position(&mut self, npc: EntityId, pos: u32);
    /// Step counter of the top state frame (the compare-and-store slot in the DLL).
    fn state_progress(&self, npc: EntityId) -> u32;
    /// Write the step counter of the top state frame.
    fn set_state_progress(&mut self, npc: EntityId, progress: u32);
    /// `PSRoutine::FullStop`: abort movement and animation queues.
    fn full_stop(&mut self, npc: EntityId);

    // ---- PSNavigation --------------------------------------------------------------------
    /// `PSNavigation::GetCurrentDestinationPoint`.
    fn destination_point(&self, npc: EntityId) -> EntityId;
    /// `PSNavigation::SetCurrentDestinationPoint`.
    fn set_destination_point(&mut self, npc: EntityId, point: EntityId);
    /// `PSNavigation.IsOnDestination`.
    fn is_on_destination(&self, npc: EntityId) -> bool;
    /// Write `PSNavigation.IsOnDestination`.
    fn set_on_destination(&mut self, npc: EntityId, v: bool);
    /// `PSNavigation::IsTravelling`.
    fn is_travelling(&self, npc: EntityId) -> bool;
    /// `PSNavigation::IsInProcessingRange`.
    fn in_processing_range(&self, npc: EntityId) -> bool;
    /// `PSNavigation.StartPosition` (`None` while unset, i.e. the zero vector).
    fn start_position(&self, npc: EntityId) -> Option<Pos>;
    /// Write `PSNavigation.StartPosition`.
    fn set_start_position(&mut self, npc: EntityId, pos: Pos);
    /// `PSNavigation::Get{Working,Relaxing,Sleeping}Point`: the routine point of the given time
    /// slot. The routine-name to point resolution is not in the script DLL and is the host's data.
    fn routine_point(&self, npc: EntityId, time: RoutineTime) -> EntityId;

    // ---- PSInteraction / PSAnchor ---------------------------------------------------------
    /// `PSInteraction::GetUseType` of an interact point (`None` for `EntityId::NONE`).
    fn point_use_type(&self, point: EntityId) -> UseType;
    /// `PSInteraction::GetAnchorPoint`.
    fn point_anchor(&self, point: EntityId) -> EntityId;
    /// `PSInteraction::GetUser`.
    fn point_user(&self, point: EntityId) -> EntityId;
    /// `PSInteraction::SetUser`.
    fn set_point_user(&mut self, point: EntityId, user: EntityId);
    /// `PSInteraction::GetOwner`.
    fn point_owner(&self, point: EntityId) -> EntityId;
    /// `PSAnchor.AnchorType`.
    fn anchor_type(&self, anchor: EntityId) -> AnchorType;
    /// `PSAnchor.MaxUsers`.
    fn anchor_max_users(&self, anchor: EntityId) -> u32;
    /// Write `PSAnchor.MaxUsers`.
    fn set_anchor_max_users(&mut self, anchor: EntityId, v: u32);
    /// `PSAnchor.UserCount`.
    fn anchor_user_count(&self, anchor: EntityId) -> u32;
    /// Write `PSAnchor.UserCount`.
    fn set_anchor_user_count(&mut self, anchor: EntityId, v: u32);
    /// `PSAnchor::GetInteractPoints`.
    fn anchor_points(&self, anchor: EntityId) -> Vec<EntityId>;
    /// Scanner result (`S:7c90` read through `S:8190`): the nearest free interaction object of
    /// `use_type` available to `npc`, or `NONE`.
    fn find_use_object(&self, npc: EntityId, use_type: UseType) -> EntityId;

    // ---- PSParty -------------------------------------------------------------------------
    /// `PSParty::GetPartyLeader` of `npc` (`NONE` if it follows nobody).
    fn party_leader(&self, npc: EntityId) -> EntityId;
    /// `PSParty::SetPartyLeader`.
    fn set_party_leader(&mut self, npc: EntityId, leader: EntityId);
    /// `PSParty.Waiting`.
    fn party_waiting(&self, npc: EntityId) -> bool;
    /// Write `PSParty.Waiting`.
    fn set_party_waiting(&mut self, npc: EntityId, v: bool);
    /// `PSParty.PartyMemberType`.
    fn party_member_type(&self, npc: EntityId) -> PartyMemberType;
    /// Write `PSParty.PartyMemberType`.
    fn set_party_member_type(&mut self, npc: EntityId, t: PartyMemberType);
    /// `PSParty::GetMembers` of a leader (the player or an NPC).
    fn party_members(&self, leader: EntityId) -> Vec<EntityId>;
    /// `PSDialog.PartyEnabled` write (cleared by `StopBringNPC`).
    fn set_dialog_party_enabled(&mut self, npc: EntityId, v: bool);

    // ---- PSCharacterMovement -------------------------------------------------------------
    /// `PSCharacterMovement::GetFloorMaterial` is water (material 3).
    fn standing_in_water(&self, npc: EntityId) -> bool;
}

/// `PSInventory`, `PSItem`, `PSInteraction` as seen from item use types.
pub trait InventoryApi {
    /// `PSInventory::HasItems(template, count)`.
    fn has_items(&self, e: EntityId, template: &str, count: u32) -> bool;
    /// `PSInventory::AssureItems(template, flags, count)`: create the item if missing and return
    /// its stack index.
    fn assure_items(&mut self, e: EntityId, template: &str, flags: u32, count: u32) -> i32;
    /// `PSInventory::FindStackIndex(slot)`: stack index equipped in a slot, `-1` if none.
    fn find_stack_index(&self, e: EntityId, slot: Slot) -> i32;
    /// `PSInventory::UnEquipStack`.
    fn unequip_stack(&mut self, e: EntityId, stack: i32);
    /// Use type of the item in a slot (`UseType::None` if empty).
    fn slot_use_type(&self, e: EntityId, slot: Slot) -> UseType;
    /// `PSInventory::GetUseType(stack)`.
    fn stack_use_type(&self, e: EntityId, stack: i32) -> UseType;
    /// `PSInventory::IsSkillActive(perk)`.
    fn is_skill_active(&self, e: EntityId, perk: &str) -> bool;
    /// `PSItem::GetCategory` of the item in a slot (`0` = none).
    fn slot_item_category(&self, e: EntityId, slot: Slot) -> u32;
    /// Template name of the spell attached to the item in a slot.
    fn slot_spell_name(&self, e: EntityId, slot: Slot) -> String;
    /// `PSMagic.SpellDeity` of the spell in a slot.
    fn slot_spell_deity(&self, e: EntityId, slot: Slot) -> SpellDeity;
}

/// `PSPlayerMemory` and `PSDamageReceiver`: where stats are stored.
pub trait StatsApi {
    /// `PSPlayerMemory::Get<Attr>`.
    fn pm_attribute(&self, attr: Attribute) -> i32;
    /// `PSPlayerMemory::Set<Attr>`.
    fn pm_set_attribute(&mut self, attr: Attribute, v: i32);
    /// `PSPlayerMemory::AddBaseValue` + `PrintAddValue`.
    fn pm_add_attribute(&mut self, attr: Attribute, delta: i32);
    /// `PSPlayerMemory::GetProtection<T>`.
    fn pm_protection(&self, p: Protection) -> i32;
    /// `PSPlayerMemory::SetProtection<T>`.
    fn pm_set_protection(&mut self, p: Protection, v: i32);
    /// `PSPlayerMemory::AddProtection<T>`.
    fn pm_add_protection(&mut self, p: Protection, delta: i32);
    /// `PSPlayerMemory::Get<Pool>Points`.
    fn pm_pool(&self, pool: Pool) -> i32;
    /// `PSPlayerMemory::Get<Pool>PointsMax`.
    fn pm_pool_max(&self, pool: Pool) -> i32;
    /// `PSPlayerMemory::Set<Pool>Points`.
    fn pm_set_pool(&mut self, pool: Pool, v: i32);
    /// `PSPlayerMemory::Set<Pool>PointsMax`.
    fn pm_set_pool_max(&mut self, pool: Pool, v: i32);
    /// `PSPlayerMemory::Add<Pool>Points`.
    fn pm_add_pool(&mut self, pool: Pool, delta: i32);
    /// `PSPlayerMemory::Add<Pool>PointsMax` (print-only in the DLL; the host stores the value).
    fn pm_add_pool_max(&mut self, pool: Pool, delta: i32);
    /// `PSPlayerMemory.Chapter`.
    fn chapter(&self) -> u32;
    /// `PSPlayerMemory::GetPoliticalFame(alignment)`.
    fn political_fame(&self, alignment: Alignment) -> i32;
    /// `PSEnclave.PlayerFame`.
    fn enclave_player_fame(&self, enclave: EntityId) -> i32;
    /// Level of the player entity (`PSNpc.Level`).
    fn player_level(&self) -> u32;
    /// Does the entity have a `PSDamageReceiver`?
    fn has_damage_receiver(&self, e: EntityId) -> bool;
    /// `PSDamageReceiver.HitPoints` / `StaminaPoints`.
    fn dr_pool(&self, e: EntityId, pool: Pool) -> i32;
    /// `PSDamageReceiver.HitPointsMax` / `StaminaPointsMax`.
    fn dr_pool_max(&self, e: EntityId, pool: Pool) -> i32;
    /// Write `PSDamageReceiver.HitPoints` / `StaminaPoints`.
    fn dr_set_pool(&mut self, e: EntityId, pool: Pool, v: i32);
    /// Write `PSDamageReceiver.HitPointsMax` / `StaminaPointsMax`.
    fn dr_set_pool_max(&mut self, e: EntityId, pool: Pool, v: i32);
}

/// Everything a ported script function can call.
pub trait ScriptHost: ScriptCtx + EntityApi + NpcApi + InventoryApi + StatsApi {}
impl<T: ScriptCtx + EntityApi + NpcApi + InventoryApi + StatsApi> ScriptHost for T {}
