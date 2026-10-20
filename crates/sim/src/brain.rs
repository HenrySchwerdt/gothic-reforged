//! Per-actor AI state: the task (`ZS_*` state) and its timers [CA 9, CA 10.3, CA 11].
use crate::routine::{RoutinePoint, RoutineTask, RoutineTime};
use crate::types::{Action, ActorId};

/// AI tasks (the `ZS_*` states the simulation models).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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

#[derive(Debug, Clone, Default)]
pub struct Brain {
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
