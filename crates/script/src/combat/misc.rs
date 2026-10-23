//! B06/B12 leftovers that belong with the fight states: the arena tasks (`ZS_Arena`), the
//! travel and wait-for-destination tasks, the revolution attack and success tasks, the walk to
//! the player's zone and the goto completion handler (`EndGoto`).

use crate::assess::assess_active_perception;
use crate::enums::{AiMode, AttackReason, Species};
use crate::host::{AiCommand, EntityId, Pool, ScriptHost};
use crate::perception::assess_target;
use crate::routine::select::continue_routine;
use crate::routine::state::{Callback, State};
use crate::routine::work::{Def, Out, Step};
use crate::stats::{pool_max, set_pool};
use Step::Custom;

fn queue(host: &mut dyn ScriptHost, npc: EntityId, cmd: AiCommand) -> Out {
    host.queue_ai(npc, cmd);
    Out::Yield
}

fn go(host: &mut dyn ScriptHost, npc: EntityId, state: State) -> Out {
    host.set_state(npc, state);
    Out::Leave
}

// ---- ZS_Arena ---------------------------------------------------------------------------------------

/// Port of `ZS_Arena` (S:119a0), first half: the fighter walks to its arena start point in AI
/// mode Arena.
fn arena_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    gothic_portcov::hit!("arena_enter");
    host.set_ai_mode(npc, AiMode::Arena);
    let dest = host.destination_point(npc);
    queue(host, npc, AiCommand::GotoExt { target: dest })
}

/// Second half: hit points and stamina are filled up (`SetHitPointsMax(Max)`, `SetHitPoints`,
/// the same for stamina), the weapons are refreshed, the NPC turns to the player and waits in
/// `ZS_Arena_Loop` with the `OnArena` callback.
fn arena_ready(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    for pool in [Pool::Stamina, Pool::Hit] {
        let max = pool_max(host, npc, pool);
        set_pool(host, npc, pool, max);
    }
    let player = host.player();
    host.set_look_at(npc, player);
    queue(host, npc, AiCommand::SelectWeapon)
}

fn arena_start(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_task_callback(npc, Callback::OnArena);
    go(host, npc, State::ArenaLoop)
}

/// Port of `OnArena` (S:11e60): once the player stands in the arena zone the fighter attacks
/// him with reason Arena.
pub fn on_arena(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("on_arena");
    let player = host.player();
    let dest = host.destination_point(npc);
    let arena_zone = host.zone(dest);
    if arena_zone.is_some() && host.zone(player) == arena_zone {
        assess_target(host, npc, player, AttackReason::Arena);
        return true;
    }
    false
}

/// Port of `ZS_Arena_Loop` (S:11f40): an arena that is no longer running ends the bout: the
/// fighter says "YouLeftTheArena", the player gets the warning message and the NPC leaves via
/// `ZS_Attack_Arena_End`.
fn arena_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    gothic_portcov::hit!("arena_loop");
    let arena = host.arena_of(npc);
    if arena.is_some() && !host.arena_running(arena) {
        host.queue_ai(
            npc,
            AiCommand::SaySvm {
                family: "YouLeftTheArena",
                index: 0,
            },
        );
        host.print_game_message("GO_ArenaWarning", "", 13);
        host.full_stop(npc);
        host.set_attack_reason(npc, AttackReason::Arena);
        host.set_current_attacker(npc, EntityId::NONE);
        let player = host.player();
        host.set_current_target(npc, player);
        return go(host, npc, State::AttackArenaEnd);
    }
    Out::Abort
}

// ---- ZS_TravelToDestination --------------------------------------------------------------------------

fn travel_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_ai_mode(npc, AiMode::Routine);
    let dest = host.destination_point(npc);
    queue(host, npc, AiCommand::GotoExt { target: dest })
}

fn travel_start(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    go(host, npc, State::TravelToDestinationLoop)
}

fn travel_wait(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    queue(host, npc, AiCommand::WaitMs { ms: 5000 })
}

/// The traveller has arrived (the walk is over): the party link is cut and the NPC rests.
fn travel_done(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_party_leader(npc, EntityId::NONE);
    Out::Next
}

fn travel_again(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    go(host, npc, State::TravelToDestination)
}

// ---- ZS_WaitForFreeDestination -----------------------------------------------------------------------

fn wait_free_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_task_callback(npc, Callback::OnWork);
    go(host, npc, State::WaitForFreeDestinationLoop)
}

/// Port of `ZS_WaitForFreeDestination_Loop` (S:42db0): every second look whether the player has
/// moved more than 1.5 m away from the destination point; then the routine continues.
fn wait_free_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    gothic_portcov::hit!("wait_free_loop");
    if host.state_time(npc) > 1.0 {
        host.set_state_time(npc, 0.0);
        let dest = host.destination_point(npc);
        let player = host.player();
        if host.distance(player, dest) > 150.0 {
            continue_routine(host, npc);
            return Out::Leave;
        }
    }
    Out::Abort
}

// ---- revolution ------------------------------------------------------------------------------------

fn revolution_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_ai_mode(npc, AiMode::Reaction);
    queue(host, npc, AiCommand::StandUp)
}

fn revolution_weapon(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    queue(host, npc, AiCommand::SelectWeapon)
}

/// The fighters of a revolution walk to the rally point (an `AttackPoint` destination) and
/// then wait in `ZS_RevolutionAttack_Loop`.
fn revolution_walk(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_task_callback(npc, Callback::OnRevolutionAttack);
    let dest = host.destination_point(npc);
    if dest.is_some() && host.point_use_type(dest) == crate::enums::UseType::AttackPoint {
        return queue(host, npc, AiCommand::GotoExt { target: dest });
    }
    Out::Next
}

fn revolution_loop_start(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    go(host, npc, State::RevolutionAttackLoop)
}

/// Port of `OnRevolutionAttack` (S:61090): the first enemy that shows up is attacked with
/// reason Revolution.
pub fn on_revolution_attack(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("on_revolution_attack");
    let other = crate::combat::select::find_other_target(host, npc);
    if other.is_some() {
        assess_target(host, npc, other, AttackReason::Revolution);
        return true;
    }
    false
}

fn revolution_loop(_host: &mut dyn ScriptHost, _npc: EntityId) -> Out {
    Out::Abort
}

/// Port of `ZS_RevolutionSuccess` (S:611e0): the winners put everything away, face the player
/// and applaud.
fn success_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    gothic_portcov::hit!("success_enter");
    host.set_ai_mode(npc, AiMode::Reaction);
    crate::down::reset_all(host, npc);
    let player = host.player();
    host.set_look_at(npc, player);
    queue(host, npc, AiCommand::StandUp)
}

fn success_holster(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    queue(
        host,
        npc,
        AiCommand::HoldInventoryStacks {
            right: -1,
            left: -1,
        },
    )
}

fn success_applaud(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_task(npc, State::Applaud);
    Out::Leave
}

// ---- ZS_GotoPlayerZone -------------------------------------------------------------------------------

fn goto_zone_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_ai_mode(npc, AiMode::Reaction);
    queue(host, npc, AiCommand::StandUp)
}

fn goto_zone_walk(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let dest = host.destination_point(npc);
    queue(host, npc, AiCommand::GotoExt { target: dest })
}

/// Arrived: if the zone check of the witness finds nothing to do the NPC just observes the
/// player there.
fn goto_zone_arrived(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if !crate::assess::assess_player_zone(host, npc) {
        host.set_task(npc, State::ObserveZonePlayer);
    }
    Out::Leave
}

// ---- goto status ---------------------------------------------------------------------------------------

/// The statuses of a finished goto instruction (`PSNavigation::GetGotoStatus`) the script reacts
/// to; the others are only logged by the DLL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GotoStatus {
    /// 0: arrived.
    Arrived,
    /// 6: the destination is blocked by somebody standing on it.
    Blocked,
    /// 15: deadlock (the NPC cannot reach any point).
    Deadlock,
    /// Everything else (path not found, aborted, ...).
    Other,
}

/// Port of `EndGoto` (S:42880): act on how a goto instruction ended.
///
/// Arrival clears "can be pushed while idle". A blocked destination (the player is in the way)
/// makes the NPC wait (`ZS_WaitForFreeDestination`), and a deadlock pins all three routine
/// points to the NPC's current destination and starts `ZS_Deadlock`. Returns whether the NPC
/// is on its destination afterwards.
pub fn end_goto(
    host: &mut dyn ScriptHost,
    npc: EntityId,
    status: GotoStatus,
    blocker: EntityId,
) -> bool {
    gothic_portcov::hit!("end_goto");
    match status {
        GotoStatus::Arrived => host.set_can_be_pushed(npc, false),
        GotoStatus::Blocked => {
            if blocker == host.player() {
                continue_routine(host, npc);
                host.full_stop(npc);
                host.set_task(npc, State::WaitForFreeDestination);
            }
        }
        GotoStatus::Deadlock => {
            host.full_stop(npc);
            host.set_task(npc, State::Deadlock);
        }
        GotoStatus::Other => {}
    }
    host.is_on_destination(npc)
}

/// Port of `HandleControlledReturnHome` (S:42bc0): a controlled (mind-controlled) creature whose
/// master is not the player any more and that stands on its destination becomes a plain
/// creature again (`PartyMemberType` cleared). Returns whether it did.
pub fn handle_controlled_return_home(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("handle_controlled_return_home");
    if host.party_member_type(npc) != crate::enums::PartyMemberType::Controlled {
        return false;
    }
    if host.party_leader(npc) == host.player() || !host.is_on_destination(npc) {
        return false;
    }
    host.set_party_member_type(npc, crate::enums::PartyMemberType::None);
    true
}

/// Port of `OnWaitForPlayer` (S:86d50): a plain perception poll.
pub fn on_wait_for_player(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("on_wait_for_player");
    assess_active_perception(host, npc)
}

/// Does the species ever stand in an arena (humans and orcs only)?
pub fn can_enter_arena(host: &dyn ScriptHost, npc: EntityId) -> bool {
    matches!(host.species(npc), Species::Human | Species::Orc)
}

const ARENA: &[Step] = &[
    Custom(arena_enter),
    Custom(arena_ready),
    Custom(arena_start),
];
const ARENA_LOOP: &[Step] = &[Custom(arena_loop)];
const TRAVEL: &[Step] = &[Custom(travel_enter), Custom(travel_start)];
const TRAVEL_LOOP: &[Step] = &[
    Custom(travel_wait),
    Custom(travel_done),
    Custom(travel_again),
];
const WAIT_FREE: &[Step] = &[Custom(wait_free_enter)];
const WAIT_FREE_LOOP: &[Step] = &[Custom(wait_free_loop)];
const REVOLUTION_ATTACK: &[Step] = &[
    Custom(revolution_enter),
    Custom(revolution_weapon),
    Custom(revolution_walk),
    Custom(revolution_loop_start),
];
const REVOLUTION_LOOP: &[Step] = &[Custom(revolution_loop)];
const REVOLUTION_SUCCESS: &[Step] = &[
    Custom(success_enter),
    Custom(success_holster),
    Custom(success_applaud),
];
const GOTO_PLAYER_ZONE: &[Step] = &[
    Custom(goto_zone_enter),
    Custom(goto_zone_walk),
    Custom(goto_zone_arrived),
];

/// The definition of one of the leftover states, `None` for other states.
pub fn definition(state: State) -> Option<Def> {
    use State as S;
    let steps = |s: &'static [Step]| Some(Def::Steps(s));
    match state {
        S::Arena => steps(ARENA),
        S::ArenaLoop => steps(ARENA_LOOP),
        S::TravelToDestination => steps(TRAVEL),
        S::TravelToDestinationLoop => steps(TRAVEL_LOOP),
        S::WaitForFreeDestination => steps(WAIT_FREE),
        S::WaitForFreeDestinationLoop => steps(WAIT_FREE_LOOP),
        S::RevolutionAttack => steps(REVOLUTION_ATTACK),
        S::RevolutionAttackLoop => steps(REVOLUTION_LOOP),
        S::RevolutionSuccess => steps(REVOLUTION_SUCCESS),
        S::GotoPlayerZone => steps(GOTO_PLAYER_ZONE),
        _ => None,
    }
}

/// Run the callbacks of these states.
pub fn run_callback(host: &mut dyn ScriptHost, npc: EntityId, cb: Callback) -> bool {
    match cb {
        Callback::OnArena => on_arena(host, npc),
        Callback::OnRevolutionAttack => on_revolution_attack(host, npc),
        Callback::OnWaitForPlayer => on_wait_for_player(host, npc),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::Species;
    use crate::host::{EntityApi, NpcApi, StatsApi};
    use crate::routine::work::run_state;
    use crate::testhost::TestWorld;

    #[test]
    fn the_arena_fighter_is_refilled_and_waits_for_the_player() {
        let mut w = TestWorld::new();
        let a = w.spawn_npc("Fighter", Species::Human, 10, 10);
        w.dr_set_pool(a, Pool::Hit, 10);
        w.dr_set_pool(a, Pool::Stamina, 5);
        w.set_task(a, State::Arena);
        while run_state(&mut w, a) != crate::routine::work::Flow::Yield
            || w.current_state(a) != Some(State::ArenaLoop)
        {
            if w.current_state(a) == Some(State::ArenaLoop) {
                break;
            }
            if w.log.len() > 8 {
                break;
            }
        }
        assert_eq!(w.ai_mode(a), AiMode::Arena);
        assert_eq!(w.dr_pool(a, Pool::Hit), w.dr_pool_max(a, Pool::Hit));
        assert_eq!(w.dr_pool(a, Pool::Stamina), w.dr_pool_max(a, Pool::Stamina));
    }

    #[test]
    fn a_blocked_destination_waits_and_a_deadlock_stops_the_routine() {
        let mut w = TestWorld::new();
        let a = w.spawn_npc("Walker", Species::Human, 5, 5);
        let player = w.player();
        end_goto(&mut w, a, GotoStatus::Blocked, player);
        assert_eq!(w.current_task(a), Some(State::WaitForFreeDestination));
        end_goto(&mut w, a, GotoStatus::Deadlock, EntityId::NONE);
        assert_eq!(w.current_task(a), Some(State::Deadlock));
    }

    #[test]
    fn the_wait_for_a_free_destination_ends_when_the_player_steps_aside() {
        let mut w = TestWorld::new();
        let a = w.spawn_npc("Walker", Species::Human, 5, 5);
        let point = w.add_item("Dummy");
        w.set_destination(a, point);
        let player = w.player();
        w.set_distance(player, point, 50.0);
        w.set_task(a, State::WaitForFreeDestination);
        run_state(&mut w, a);
        w.set_state_time(a, 1.5);
        run_state(&mut w, a);
        assert_eq!(w.current_state(a), Some(State::WaitForFreeDestinationLoop));
        w.set_distance(player, point, 400.0);
        w.set_state_time(a, 1.5);
        run_state(&mut w, a);
        assert_ne!(w.current_state(a), Some(State::WaitForFreeDestinationLoop));
    }
}
