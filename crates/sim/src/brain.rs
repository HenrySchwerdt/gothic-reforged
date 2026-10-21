//! Per-actor AI state: the task (`ZS_*` state) and its timers [CA 9, CA 10.3, CA 11].
use crate::routine::{RoutinePoint, RoutineTask, RoutineTime};
use crate::types::{Action, ActorId};

/// AI tasks (the `ZS_*` states the simulation models).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Task {
    /// Daily routine (`ContinueRoutine`): walk to the routine point and perform its task.
    #[default]
    Routine,
    /// `ZS_ReactToWeapon` [CA 10.3].
    ReactToWeapon,
    /// `ZS_Threaten`: attack below 700 or after 5 s, give up beyond 1000 [CA 10.3].
    Threaten,
    /// `ZS_Attack` loop [CA 9.1].
    Attack,
    /// `ZS_Attack_WaitForFlee` [CA 9.1].
    AttackWaitForFlee,
    /// Any other state of the ported reaction machinery (watching a fight, observing a
    /// suspect, eating, ransacking, talking, ...).
    Reaction,
    /// `ZS_Flee`.
    Flee,
    FollowPlayer,
    WaitForPlayer,
    /// `ZS_Unconscious` (defeated, wakes up later) [CA 9.4].
    Unconscious,
    /// `ZS_Dead`.
    Dead,
}

/// A melee strike whose hit window opens at `at_ms`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Strike {
    pub at_ms: u64,
    pub action: Action,
    pub target: ActorId,
}

/// A running combat movement instruction (`sAIGotoInstr` of the attack states): the host moves
/// the actor every tick and runs the state's local callback until the callback ends it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GotoState {
    /// The actor to walk to or circle around.
    pub target: Option<ActorId>,
    /// A fixed destination (points, objects, home positions).
    pub pos: Option<crate::world::Pos>,
    pub kind: gothic_script::host::GotoKind,
    pub walk_mode: u8,
    /// Stop (arrived) within this distance.
    pub stop: f32,
    pub since_ms: u64,
}

/// One step of the `_AI_*` command an NPC executes (the plan of
/// `gothic_script::combat::commands::begin_melee_command` and its siblings).
#[derive(Debug, Clone, PartialEq)]
pub enum NpcStep {
    /// `AddStaminaPoints(delta)` between two instructions.
    Stamina(i32),
    /// `sAICombatMoveInstr`: a clip of `action` in `phase` at `speed` against `target`.
    Move {
        action: gothic_script::enums::Action,
        phase: gothic_script::host::MovePhase,
        speed: f32,
        pose: gothic_script::enums::AniState,
        target: Option<ActorId>,
    },
    /// `sAIWaitInstr(ms)`.
    Wait(u32),
    /// The command is over: the pose it leaves (`Parade` keeps the guard up).
    End { guard: bool },
}

/// The instruction in flight of an NPC's plan.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum NpcExec {
    #[default]
    Idle,
    /// A timed instruction (`sAIWaitInstr`, headless clips).
    Wait { until_ms: u64 },
    /// A combat-move clip: the host reports the damage moment and the end (`Sim::npc_move_hit`,
    /// `Sim::npc_exec_done`); headless runs use the timers.
    Move {
        seq: u32,
        action: gothic_script::enums::Action,
        hit_phase: bool,
        target: Option<ActorId>,
        /// Headless: when the damage lands (`None` once it did or for clips without a blow).
        hit_at_ms: Option<u64>,
        /// Headless: when the instruction ends; the host path uses the deadline only.
        end_ms: u64,
        hit_sent: bool,
    },
}

/// The `_AI_*` instructions an NPC works through, one after the other.
#[derive(Debug, Clone, Default)]
pub struct NpcPlan {
    pub steps: std::collections::VecDeque<NpcStep>,
    pub exec: NpcExec,
    /// Sequence number of the last instruction handed to the host.
    pub seq: u32,
    /// Deadline of the instruction in flight (a host that never answers must not freeze it).
    pub deadline_ms: u64,
    /// The host plays this actor's clips and reports back.
    pub host_drives: bool,
}

impl NpcPlan {
    /// Nothing queued and nothing running.
    pub fn is_idle(&self) -> bool {
        self.steps.is_empty() && self.exec == NpcExec::Idle
    }
}

#[derive(Debug, Clone, Default)]
pub struct Brain {
    /// The `_AI_*` instruction plan in flight.
    pub plan: NpcPlan,
    /// Headless timers: when the `Raise` clip of the move in flight started.
    pub last_raise_ms: Option<u64>,
    /// The melee weapons stowed while the actor holds a bow or a spell (`left`, `right`).
    pub melee_kit: Option<(Option<crate::item::ItemDef>, Option<crate::item::ItemDef>)>,
    pub task: Task,
    pub since_ms: u64,
    /// Task to return to after a stumble.
    pub resume: Task,
    /// No new decision before this time (move / stumble animation).
    pub busy_until_ms: u64,
    pub strike: Option<Strike>,
    pub parade_until_ms: u64,
    pub last_move: Action,
    /// When the actor last took a hit (the `CanHit` / `MinHitDuration` gate) [CA 9.3].
    pub last_hit_taken_ms: Option<u64>,
    pub next_perception_ms: u64,
    pub next_second_ms: u64,
    /// Start of the current chase (target out of attack range) [CA 9.1].
    pub chase_since_ms: Option<u64>,
    /// ReactToWeapon stage: 0 line, 1 second warning given [CA 10.3].
    pub warned: u8,
    pub unconscious_secs: u32,
    pub unconscious_arena: bool,
    pub routine_time: Option<RoutineTime>,
    pub routine_point: Option<RoutinePoint>,
    pub routine_task: Option<RoutineTask>,
    pub last_destination_ms: u64,
    pub potions_used: u32,
    /// The combat movement in progress, if any.
    pub goto: Option<GotoState>,
    /// When the actor last started a strike (`PSNpc::SetLastHit`, read by `CanHit`).
    pub last_strike_ms: Option<u64>,
}
