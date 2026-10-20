//! B08: `ZS_Attack` and its sub-states, `OnAttack` and the local callbacks of the combat
//! movements.
//!
//! `ZS_Attack` sets the fighting mode, `ZS_Attack_Loop` asks [`select_combat_move`] what to do
//! and switches to the state that performs it (swing, parade, jump, approach, strafe, ...); every
//! such state ends by returning to the loop. Movement states install a local callback
//! (`OnAttack_Goto`, `OnAttack_Strafe`, ...) that the host calls every tick while the movement
//! instruction runs; it can end the movement by changing the state.

use super::range::{attack_range, combat_walk_mode};
use super::select::{find_other_target, npc_weapon_mode, reason_keeps_target, select_combat_move};
use crate::damage::can_be_killed;
use crate::down::kill;
use crate::enums::{
    Action, AiMode, AttackReason, Attitude, LastFight, NpcClass, PartyMemberType, Species, UseType,
};
use crate::helpers::WeaponMode;
use crate::host::{AiCommand, EntityId, GotoKind, ScriptHost, dir};
use crate::perception::assess_target;
use crate::predicates::{
    follow_time, has_attack_reason_to_kill, has_dangerous_weapon, want_to_ransack,
    zone_attitude_to_player,
};
use crate::routine::select::continue_routine;
use crate::routine::state::{Callback, State};
use crate::routine::work::{Def, Out, Step};
use crate::stats;

use Step::Custom;

/// Seconds between the checks of the movement callbacks (`OnAttack` re-run interval).
const RECHECK_SECS: f32 = 1.0;

fn queue(host: &mut dyn ScriptHost, npc: EntityId, cmd: AiCommand) -> Out {
    host.queue_ai(npc, cmd);
    Out::Yield
}

fn leave(host: &mut dyn ScriptHost, npc: EntityId, state: State) -> Out {
    host.set_state(npc, state);
    Out::Leave
}

/// Port of `OnAttack` (S:147c0): the per-attack-pass sanity check. Returns `true` when it
/// changed the state (the caller must stop).
///
/// A target that is gone, dead or down ends the fight; a talking one pauses it. Intruder
/// reasons against the player stop unless the zone is angry at the player, an arena fighter
/// who left the arena is called back, a drive-away fighter that strayed 8 m from its alternative
/// target eats it, and a swimming target is waited out on the shore. Otherwise a more pressing
/// opponent can take over (`AssessTarget` with reason ReactToDamage/Enemy/Prey).
pub fn on_attack(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    let target = host.current_target(npc);
    let tmode = if target.is_some() {
        host.ai_mode(target)
    } else {
        AiMode::Dead
    };
    if target.is_none() || matches!(tmode, AiMode::Dead | AiMode::Down) {
        host.full_stop(npc);
        host.set_state(npc, State::AttackEnd);
        return true;
    }
    if tmode == AiMode::Talk {
        host.full_stop(npc);
        host.set_state(npc, State::AttackWaitForDialog);
        return true;
    }
    let reason = host.attack_reason(npc);
    if reason == AttackReason::Intruder && target == host.player() {
        let zone = host.zone(npc);
        if zone_attitude_to_player(host, zone) != Attitude::Angry {
            host.full_stop(npc);
            host.set_state(npc, State::AttackWaitForFlee);
            return true;
        }
    }
    if reason == AttackReason::Arena {
        let arena = host.arena_of(npc);
        if arena.is_some() && !host.arena_running(arena) {
            host.full_stop(npc);
            host.queue_ai(
                npc,
                AiCommand::SaySvm {
                    family: "YouLeftTheArena",
                    index: 0,
                },
            );
            host.set_current_attacker(npc, EntityId::NONE);
            host.set_current_target(npc, EntityId::NONE);
            host.print_game_message("GO_ArenaWarning", "", 13);
            host.set_attack_reason(npc, AttackReason::Arena);
            host.set_state(npc, State::AttackArenaEnd);
            return true;
        }
    }
    if reason == AttackReason::DriveAway {
        let alt = host.alternative_target(npc);
        if alt.is_some() && host.distance(npc, alt) > 800.0 {
            host.full_stop(npc);
            host.set_current_target(npc, alt);
            host.set_task(npc, State::EatBody);
            return true;
        }
    }
    if host.is_swimming(target) {
        host.full_stop(npc);
        host.set_state(npc, State::AttackWaitOnShore);
        return true;
    }
    if !reason_keeps_target(reason) {
        let other = find_other_target(host, npc);
        if other.is_some() && other != target {
            let mut why = AttackReason::ReactToDamage;
            match crate::attitude::get_attitude(host, npc, other) {
                Attitude::Hostile => why = AttackReason::Enemy,
                Attitude::Predator => why = AttackReason::Prey,
                _ => {}
            }
            if reason == AttackReason::Revolution {
                why = AttackReason::Revolution;
            }
            host.stop_ai_goto(npc);
            host.set_look_at(npc, other);
            assess_target(host, npc, other, why);
            return true;
        }
    }
    if host.combat_state(npc) == 0 {
        update_combat_state(host, npc);
    }
    false
}

/// Port of S:1880: how many NPCs (not the player) are engaged in combat against `target`.
pub fn fighters_on(host: &dyn ScriptHost, target: EntityId) -> usize {
    let player = host.player();
    host.all_npcs()
        .into_iter()
        .filter(|&n| {
            n != player
                && host.combat_state(n) == 1
                && host.ai_mode(n) == AiMode::Combat
                && host.current_target(n) == target
        })
        .count()
}

/// Port of S:14f0: may the NPC step up to the front line? A fighter already engaged with a
/// target stays engaged; otherwise it joins while fewer than the allowed number of NPCs fight
/// the same target (1; 2 or 3 against the player on normal/hard difficulty).
pub fn update_combat_state(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    let target = host.current_target(npc);
    if host.combat_state(npc) == 1 && target.is_some() {
        return true;
    }
    let player = host.player();
    let engaged = fighters_on(host, target);
    let mut limit = 1;
    if target == player && (host.setup().extended_content || host.setup().ai_mode) {
        limit = match host.difficulty() {
            crate::config::Difficulty::Normal => 2,
            crate::config::Difficulty::Hard => 3,
            crate::config::Difficulty::Easy => 1,
        };
    }
    if (engaged as i32) < limit {
        host.set_combat_state(npc, 1);
        return true;
    }
    host.set_combat_state(npc, 0);
    false
}

/// Port of `CancelAttack` (S:1a4d0): give the fight up. A fight against the player is recorded
/// as cancelled; the NPC returns to its routine.
pub fn cancel_attack(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    let target = host.current_target(npc);
    if target == host.player() && !host.is_transformed(npc) {
        host.set_last_fight_vs_player(npc, LastFight::Cancel);
    }
    host.set_ai_mode(npc, AiMode::Routine);
    crate::predicates::set_last_fight_timestamp(host, npc, target);
    continue_routine(host, npc);
    true
}

// ---- ZS_Attack and the loop ---------------------------------------------------------------------

fn attack_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.ai_mode(npc) == AiMode::Dead {
        host.full_stop(npc);
        host.set_task(npc, State::Dead);
        return Out::Leave;
    }
    host.set_look_at(npc, EntityId::NONE);
    host.set_ai_mode(npc, AiMode::Combat);
    let target = host.current_target(npc);
    if target == host.player() && !host.is_transformed(npc) {
        host.set_last_fight_vs_player(npc, LastFight::Running);
    }
    queue(host, npc, AiCommand::StandUp)
}

fn attack_enter_done(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_on_destination(npc, false);
    let target = host.current_target(npc);
    host.set_look_at(npc, target);
    leave(host, npc, State::AttackLoop)
}

/// First step of `ZS_Attack_Loop` (S:14d70): `OnAttack` roughly once per second of loop
/// time, then re-select the weapon.
fn loop_prepare(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_look_at(npc, EntityId::NONE);
    if host.state_position(npc) != 0 {
        if host.state_time(npc) > RECHECK_SECS {
            host.set_state_time(npc, 0.0);
            host.set_state_position(npc, 0);
            if host.current_target(npc).is_none() {
                host.full_stop(npc);
                return leave(host, npc, State::AttackEnd);
            }
        }
    } else {
        host.set_state_position(npc, 1);
        if on_attack(host, npc) {
            host.set_state_position(npc, 0);
            return Out::Leave;
        }
    }
    queue(host, npc, AiCommand::SelectWeapon)
}

/// The state that carries out `mv` (the dispatch of `ZS_Attack_Loop` and `OnAttack_Goto`).
fn state_for_move(
    host: &mut dyn ScriptHost,
    npc: EntityId,
    target: EntityId,
    mv: Action,
) -> Option<State> {
    use Action as A;
    Some(match mv {
        A::Attack => State::AttackAttack,
        A::PowerAttack => State::AttackPowerAttack,
        A::QuickAttack => State::AttackQuickAttack,
        A::SimpleWhirl => State::AttackSimpleWhirl,
        A::TurnLeft => State::AttackTurnLeft,
        A::TurnRight => State::AttackTurnRight,
        A::SprintAttack => {
            host.set_current_action(npc, A::SprintAttack);
            let range = attack_range(host, npc, target, A::PowerAttack) as f32;
            if range < host.distance(npc, target) {
                State::AttackSprint
            } else {
                State::AttackPowerAttack
            }
        }
        A::WhirlAttack => State::AttackWhirlAttack,
        A::PierceAttack => State::AttackPierceAttack,
        A::HackAttack => State::AttackHackAttack,
        A::Parade => State::AttackParade,
        A::Shoot => State::AttackShoot,
        A::Cast => State::AttackCast,
        A::Heal => State::AttackDrinkPotion,
        A::Wait => State::AttackWait,
        A::JumpBack => State::AttackJumpBack,
        A::Fwd => State::AttackGoto,
        A::Back => State::AttackGoBack,
        A::Left => State::AttackStrafeLeft,
        A::Right => State::AttackStrafeRight,
        A::Move => State::AttackSprint,
        _ => return None,
    })
}

/// The potion rule of the loop: a fighter below 40 percent hit points with a health potion,
/// facing an opponent that is no ranged/magic threat and more than 7 m away, drinks instead.
fn wants_potion(host: &dyn ScriptHost, npc: EntityId, target: EntityId) -> bool {
    if matches!(host.npc_class(npc), NpcClass::Mage | NpcClass::Paladin) {
        return false;
    }
    if stats::pool_percent(host, npc, crate::host::Pool::Hit) >= 40 {
        return false;
    }
    if !host.has_items(npc, "It_Potion_Health", 1) {
        return false;
    }
    if npc_weapon_mode(host, target) != WeaponMode::Melee && has_dangerous_weapon(host, target) {
        return false;
    }
    host.distance(npc, target) > 700.0
}

/// Second step of `ZS_Attack_Loop`: decide and switch.
fn loop_decide(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let target = host.current_target(npc);
    let mut mv = select_combat_move(host, npc, target);
    if wants_potion(host, npc, target) {
        mv = Action::Heal;
    }
    match state_for_move(host, npc, target, mv) {
        Some(state) => {
            host.stop_ai_goto(npc);
            leave(host, npc, state)
        }
        None => {
            host.set_current_action(npc, Action::None);
            Out::Abort
        }
    }
}

// ---- swings --------------------------------------------------------------------------------------

fn swing(host: &mut dyn ScriptHost, npc: EntityId, action: Action) -> Out {
    let target = host.current_target(npc);
    host.set_look_at(npc, EntityId::NONE);
    queue(host, npc, AiCommand::Attack { action, target })
}

fn do_attack(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    swing(host, npc, Action::Attack)
}
fn do_power(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    swing(host, npc, Action::PowerAttack)
}
fn do_quick(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    swing(host, npc, Action::QuickAttack)
}
fn do_simple_whirl(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    swing(host, npc, Action::SimpleWhirl)
}
fn do_whirl(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    swing(host, npc, Action::WhirlAttack)
}
fn do_pierce(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    swing(host, npc, Action::PierceAttack)
}
fn do_hack(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    swing(host, npc, Action::HackAttack)
}
fn do_finish(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    swing(host, npc, Action::FinishingAttack)
}

/// The wind-up of the heavy moves: raise the guard first.
fn wind_up(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let target = host.current_target(npc);
    queue(host, npc, AiCommand::Parade { target })
}

fn stamp_hit(host: &mut dyn ScriptHost, _npc: EntityId) -> Out {
    let npc = _npc;
    host.set_last_hit(npc);
    Out::Next
}

fn back_to_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    leave(host, npc, State::AttackLoop)
}

fn back_to_loop_stamped(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_last_hit(npc);
    leave(host, npc, State::AttackLoop)
}

fn say_finishing(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let target = host.current_target(npc);
    if crate::predicates::is_humanoid(host, target) {
        host.queue_ai(
            npc,
            AiCommand::SaySvm {
                family: "FinishingMove",
                index: 0,
            },
        );
    }
    Out::Next
}

// ---- other actions -------------------------------------------------------------------------------

fn do_parade(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let target = host.current_target(npc);
    queue(host, npc, AiCommand::Parade { target })
}

/// After the guard is up: while the opponent keeps swinging hold it for two seconds.
fn parade_hold(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let target = host.current_target(npc);
    if super::moves::is_attack_action(host.current_action(target)) {
        return queue(host, npc, AiCommand::WaitMs { ms: 2000 });
    }
    Out::Next
}

fn do_jump_back(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let target = host.current_target(npc);
    queue(host, npc, AiCommand::JumpBack { target })
}

fn do_drink(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    queue(host, npc, AiCommand::UseItem("It_Potion_Health"))
}

fn do_wait(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_current_action(npc, Action::Wait);
    queue(host, npc, AiCommand::WaitMs { ms: 100 })
}

fn do_cast(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let power = host.random(100) < 20;
    queue(host, npc, AiCommand::Cast { power })
}

/// `ZS_Attack_Shoot` (S:18ed0): re-supply the arrows (and bolts for a crossbow), take aim and
/// loose.
fn shoot_prepare(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let target = host.current_target(npc);
    host.set_current_action(npc, Action::Shoot);
    host.set_look_at(npc, target);
    let count = (host.random(10) + 10) as u32;
    host.assure_items(npc, "It_Arrow", 0, count);
    if host.slot_use_type(npc, crate::enums::Slot::LeftHand) == UseType::CrossBow {
        host.assure_items(npc, "It_Bolt", 0, count);
    }
    queue(host, npc, AiCommand::WaitMs { ms: 1000 })
}

fn shoot_fire(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let target = host.current_target(npc);
    queue(host, npc, AiCommand::Shoot { target })
}

// ---- movement states -----------------------------------------------------------------------------

fn goto_instr(
    host: &mut dyn ScriptHost,
    npc: EntityId,
    cb: Callback,
    kind: GotoKind,
    walk_mode: u8,
) -> Out {
    let target = host.current_target(npc);
    host.set_local_callback(npc, cb);
    queue(
        host,
        npc,
        AiCommand::CombatGoto {
            target,
            walk_mode,
            kind,
        },
    )
}

fn do_goto(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_current_action(npc, Action::Fwd);
    host.set_task_time(npc, 0.0);
    let walk = combat_walk_mode(host, npc);
    goto_instr(host, npc, Callback::OnAttackGoto, GotoKind::Approach, walk)
}

fn do_sprint(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.current_action(npc) != Action::SprintAttack {
        host.set_current_action(npc, Action::Fwd);
        host.set_task_time(npc, 0.0);
        return goto_instr(host, npc, Callback::OnAttackGoto, GotoKind::Approach, 3);
    }
    Out::Next
}

fn do_change_position(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_state_time(npc, 0.0);
    host.set_last_hit(npc);
    goto_instr(
        host,
        npc,
        Callback::OnAttackPositioning,
        GotoKind::Approach,
        2,
    )
}

fn do_go_back(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let target = host.current_target(npc);
    if host.can_evade(npc, target, dir::BACK) {
        return goto_instr(host, npc, Callback::OnAttackGoBack, GotoKind::Back, 2);
    }
    Out::Next
}

fn do_strafe_left(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    goto_instr(host, npc, Callback::OnAttackStrafe, GotoKind::StrafeLeft, 2)
}
fn do_strafe_right(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    goto_instr(
        host,
        npc,
        Callback::OnAttackStrafe,
        GotoKind::StrafeRight,
        2,
    )
}
fn do_turn_left(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_on_destination(npc, true);
    goto_instr(host, npc, Callback::OnAttackTurn, GotoKind::TurnLeft, 2)
}
fn do_turn_right(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_on_destination(npc, true);
    goto_instr(host, npc, Callback::OnAttackTurn, GotoKind::TurnRight, 2)
}

/// End of a movement state: the instruction is over, back to the loop.
fn movement_done(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_on_destination(npc, false);
    leave(host, npc, State::AttackLoop)
}

/// Port of `OnAttack_Goto` (S:158c0): the approach callback. Re-checks the fight, gives up a
/// chase that outlasts `GetFollowTime`, and switches to the swing the selected move calls for
/// (`Fwd`, `Back`, `Left` and `Right` keep the movement running).
pub fn on_attack_goto(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    if host.state_position(npc) == 0 {
        host.set_state_position(npc, 1);
        if on_attack(host, npc) {
            return true;
        }
    } else if host.state_time(npc) > RECHECK_SECS {
        host.set_state_time(npc, 0.0);
        host.set_state_position(npc, 0);
        if host.current_target(npc).is_none() {
            host.full_stop(npc);
            host.set_state(npc, State::AttackEnd);
            return true;
        }
    }
    if host.attack_reason(npc) != AttackReason::Intruder
        && host.task_time(npc) > follow_time(host, npc) as f32
    {
        host.full_stop(npc);
        host.set_state(npc, State::AttackWaitForFlee);
        return true;
    }
    let target = host.current_target(npc);
    let mv = select_combat_move(host, npc, target);
    match mv {
        Action::Fwd | Action::Back | Action::Left | Action::Right => false,
        Action::Move => {
            if host.current_state(npc) == Some(State::AttackSprint) {
                return false;
            }
            host.stop_ai_goto(npc);
            host.set_state(npc, State::AttackSprint);
            true
        }
        Action::Wait => {
            host.stop_ai_goto(npc);
            host.set_state(npc, State::AttackLoop);
            true
        }
        other => match state_for_move(host, npc, target, other) {
            Some(state) => {
                host.stop_ai_goto(npc);
                host.set_state(npc, state);
                true
            }
            None => false,
        },
    }
}

/// Port of `OnAttack_Positioning` (S:16190): walk in until the distance drops under 5 m (or a
/// second passes); closer than that, circle toward the free side first.
pub fn on_attack_positioning(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    if on_attack(host, npc) {
        return true;
    }
    let target = host.current_target(npc);
    let d = host.last_dist_to_target(npc);
    if d < 500.0 {
        let right_free = host.can_evade(npc, target, dir::RIGHT_SIDE);
        let left_free = host.can_evade(npc, target, dir::LEFT_SIDE);
        let turn = if !right_free
            && left_free
            && super::moves::can_do(host, npc, target, Action::TurnRight)
        {
            Some(State::AttackTurnRight)
        } else if super::moves::can_do(host, npc, target, Action::TurnLeft) {
            Some(State::AttackTurnLeft)
        } else if super::moves::can_do(host, npc, target, Action::TurnRight) {
            Some(State::AttackTurnRight)
        } else {
            None
        };
        if let Some(state) = turn {
            host.stop_ai_goto(npc);
            host.set_state(npc, state);
            return true;
        }
    }
    if host.state_time(npc) > RECHECK_SECS || d < 500.0 {
        host.stop_ai_goto(npc);
        host.set_state(npc, State::AttackLoop);
        return true;
    }
    false
}

/// Port of `OnAttack_GoBack` (S:165b0): keep backing off while the selection still says
/// `Back`, re-evaluated every 0.4 s.
pub fn on_attack_go_back(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    if on_attack(host, npc) {
        return true;
    }
    if host.state_time(npc) > 0.4 {
        let target = host.current_target(npc);
        if select_combat_move(host, npc, target) == Action::Back {
            host.set_state_time(npc, 0.0);
            return false;
        }
        host.stop_ai_goto(npc);
        host.set_state(npc, State::AttackLoop);
        return true;
    }
    false
}

/// Port of `OnAttack_Strafe` (S:16a10): strafe for 0.6 s.
pub fn on_attack_strafe(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    if host.state_time(npc) > 0.6 {
        host.stop_ai_goto(npc);
        host.set_state(npc, State::AttackLoop);
        return true;
    }
    false
}

/// Port of `OnAttack_Turn` (S:16e50): turn for 0.6 s.
pub fn on_attack_turn(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    if host.state_time(npc) > 0.6 {
        host.stop_ai_goto(npc);
        host.set_on_destination(npc, false);
        host.full_stop(npc);
        host.set_state(npc, State::AttackLoop);
        return true;
    }
    false
}

// ---- waiting states ------------------------------------------------------------------------------

fn wait_for_flee_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_last_dist_to_target(npc, 0.0);
    if crate::predicates::is_humanoid(host, npc) {
        let family = if host.attack_reason(npc) == AttackReason::Intruder {
            "YesGetLost"
        } else {
            "StandStill"
        };
        host.queue_ai(npc, AiCommand::SaySvm { family, index: 0 });
    }
    leave(host, npc, State::AttackWaitForFleeLoop)
}

/// Port of `ZS_Attack_WaitForFlee_Loop` (S:1a380): after half a second look again: beyond 20 m
/// the attack is cancelled, a target that came closer is fought again, otherwise wait on.
fn wait_for_flee_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.state_time(npc) > 0.5 {
        let target = host.current_target(npc);
        let d = host.distance(npc, target);
        if d > 2000.0 {
            cancel_attack(host, npc);
            return Out::Leave;
        }
        if d < host.last_dist_to_target(npc) {
            return leave(host, npc, State::AttackLoop);
        }
        host.set_last_dist_to_target(npc, d);
        host.set_state_time(npc, 0.0);
    }
    Out::Abort
}

fn wait_on_shore(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.state_time(npc) > 1.0 {
        let target = host.current_target(npc);
        if !host.is_swimming(target) {
            return leave(host, npc, State::AttackLoop);
        }
        if host.distance(npc, target) > 1000.0 {
            cancel_attack(host, npc);
            return Out::Leave;
        }
        host.set_state_time(npc, 0.0);
    }
    Out::Abort
}

fn wait_for_dialog(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let target = host.current_target(npc);
    if target.is_none() || host.ai_mode(target) != AiMode::Talk {
        return leave(host, npc, State::AttackLoop);
    }
    Out::Abort
}

fn arena_end(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_attack_reason(npc, AttackReason::None);
    host.set_current_attacker(npc, EntityId::NONE);
    host.full_stop(npc);
    leave(host, npc, State::AttackEnd)
}

// ---- ZS_Attack_End -------------------------------------------------------------------------------

/// Step 1 of `ZS_Attack_End` (S:1a610): finish a downed enemy that deserves it, take a new
/// opponent if one is waiting, record how the fight against the player went, stand up.
fn end_wrap_up(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let target = host.current_target(npc);
    if host.combat_state(npc) == 1
        && target.is_some()
        && host.ai_mode(target) == AiMode::Down
        && has_attack_reason_to_kill(host, npc, target)
        && super::moves::can_do(host, npc, target, Action::FinishingAttack)
        && can_be_killed(host, target)
    {
        return leave(host, npc, State::AttackFinishingAttack);
    }
    let other = find_other_target(host, npc);
    if other.is_some() {
        let mut why = AttackReason::ReactToDamage;
        if host.attack_reason(npc) == AttackReason::Revolution {
            why = AttackReason::Revolution;
        } else {
            match crate::attitude::get_attitude(host, npc, other) {
                Attitude::Hostile => why = AttackReason::Enemy,
                Attitude::Predator => why = AttackReason::Prey,
                _ => {}
            }
        }
        host.stop_ai_goto(npc);
        host.set_look_at(npc, other);
        assess_target(host, npc, other, why);
        return Out::Leave;
    }
    let player = host.player();
    let now = host.playing_time();
    if target.is_some() && host.current_attacker(target) == player {
        if !host.is_transformed(player) {
            host.set_last_fight_vs_player(npc, LastFight::Friendly);
        }
        host.set_last_fight_timestamp(npc, now);
    } else if target == player && matches!(host.ai_mode(target), AiMode::Dead | AiMode::Down) {
        if !host.is_transformed(npc) {
            host.set_last_fight_vs_player(npc, LastFight::Won);
        }
        host.set_last_fight_timestamp(npc, now);
    }
    queue(host, npc, AiCommand::StandUp)
}

fn end_holster(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_on_destination(npc, false);
    if host.species(npc) != Species::Bloodfly {
        return queue(
            host,
            npc,
            AiCommand::HoldInventoryStacks {
                right: -1,
                left: -1,
            },
        );
    }
    Out::Next
}

fn end_finish(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.party_member_type(npc) == PartyMemberType::Summoned {
        kill(host, npc, EntityId::NONE);
    }
    let target = host.current_target(npc);
    if host.combat_state(npc) == 1 && target.is_some() && want_to_ransack(host, npc, target) {
        host.set_combat_state(npc, 0);
        host.set_current_target(npc, target);
        host.set_task(npc, State::RansackBody);
        return Out::Leave;
    }
    host.set_combat_state(npc, 0);
    queue(host, npc, AiCommand::WaitMs { ms: 2000 })
}

fn end_resume(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    continue_routine(host, npc);
    Out::Leave
}

// ---- the step tables -----------------------------------------------------------------------------

/// Port of `ZS_Attack` (S:14480): enter combat mode.
const ATTACK: &[Step] = &[Custom(attack_enter), Custom(attack_enter_done)];
/// Port of `ZS_Attack_Loop` (S:14d70): check, choose, switch.
const ATTACK_LOOP: &[Step] = &[Custom(loop_prepare), Custom(loop_decide)];
/// Port of `ZS_Attack_Attack` (S:16f00).
const ATTACK_ATTACK: &[Step] = &[Custom(do_attack), Custom(back_to_loop_stamped)];
/// Port of `ZS_Attack_PowerAttack` (S:173b0).
const ATTACK_POWER: &[Step] = &[Custom(do_power), Custom(back_to_loop_stamped)];
/// Port of `ZS_Attack_QuickAttack` (S:179b0).
const ATTACK_QUICK: &[Step] = &[Custom(do_quick), Custom(back_to_loop_stamped)];
/// Port of `ZS_Attack_SimpleWhirl` (S:17160).
const ATTACK_SIMPLE_WHIRL: &[Step] = &[Custom(do_simple_whirl), Custom(back_to_loop_stamped)];
/// Port of `ZS_Attack_WhirlAttack` (S:17600): guard, then the whirl.
const ATTACK_WHIRL: &[Step] = &[
    Custom(wind_up),
    Custom(stamp_hit),
    Custom(do_whirl),
    Custom(back_to_loop),
];
/// Port of `ZS_Attack_PierceAttack` (S:18100): guard, then the thrust.
const ATTACK_PIERCE: &[Step] = &[
    Custom(wind_up),
    Custom(stamp_hit),
    Custom(do_pierce),
    Custom(back_to_loop),
];
/// Port of `ZS_Attack_HackAttack` (S:184b0): guard, then the overhead hack.
const ATTACK_HACK: &[Step] = &[
    Custom(wind_up),
    Custom(stamp_hit),
    Custom(do_hack),
    Custom(back_to_loop),
];
/// Port of `ZS_Attack_FinishingAttack` (S:18860): a word, the guard, the killing blow.
const ATTACK_FINISHING: &[Step] = &[
    Custom(say_finishing),
    Custom(wind_up),
    Custom(do_finish),
    Custom(back_to_loop),
];
/// Port of `ZS_Attack_Parade` (S:17c00).
const ATTACK_PARADE: &[Step] = &[Custom(do_parade), Custom(parade_hold), Custom(back_to_loop)];
/// Port of `ZS_Attack_JumpBack` (S:17eb0).
const ATTACK_JUMP_BACK: &[Step] = &[Custom(do_jump_back), Custom(back_to_loop)];
/// Port of `ZS_Attack_DrinkPotion` (S:18c50).
const ATTACK_DRINK: &[Step] = &[Custom(do_drink), Custom(back_to_loop)];
/// Port of `ZS_Attack_Wait` (S:154e0).
const ATTACK_WAIT: &[Step] = &[Custom(do_wait), Custom(back_to_loop)];
/// Port of `ZS_Attack_Cast` (S:19990).
const ATTACK_CAST: &[Step] = &[Custom(do_cast), Custom(back_to_loop)];
/// Port of `ZS_Attack_Shoot` (S:18ed0).
const ATTACK_SHOOT: &[Step] = &[
    Custom(shoot_prepare),
    Custom(shoot_fire),
    Custom(back_to_loop),
];
/// Port of `ZS_Attack_Goto` (S:15640).
const ATTACK_GOTO: &[Step] = &[Custom(do_goto), Custom(movement_done)];
/// Port of `ZS_Attack_Sprint` (S:15d90).
const ATTACK_SPRINT: &[Step] = &[Custom(do_sprint), Custom(movement_done)];
/// Port of `ZS_Attack_ChangePosition` (S:15fa0).
const ATTACK_CHANGE_POSITION: &[Step] = &[Custom(do_change_position), Custom(movement_done)];
/// Port of `ZS_Attack_GoBack` (S:163d0).
const ATTACK_GO_BACK: &[Step] = &[Custom(do_go_back), Custom(movement_done)];
/// Port of `ZS_Attack_StrafeLeft` (S:16690).
const ATTACK_STRAFE_LEFT: &[Step] = &[Custom(do_strafe_left), Custom(movement_done)];
/// Port of `ZS_Attack_StrafeRight` (S:16850).
const ATTACK_STRAFE_RIGHT: &[Step] = &[Custom(do_strafe_right), Custom(movement_done)];
/// Port of `ZS_Attack_TurnLeft` (S:16ab0).
const ATTACK_TURN_LEFT: &[Step] = &[Custom(do_turn_left), Custom(movement_done)];
/// Port of `ZS_Attack_TurnRight` (S:16c80).
const ATTACK_TURN_RIGHT: &[Step] = &[Custom(do_turn_right), Custom(movement_done)];
/// Port of `ZS_Attack_WaitForFlee` (S:1a230).
const ATTACK_WAIT_FOR_FLEE: &[Step] = &[Custom(wait_for_flee_enter)];
/// Port of `ZS_Attack_WaitForFlee_Loop` (S:1a380).
const ATTACK_WAIT_FOR_FLEE_LOOP: &[Step] = &[Custom(wait_for_flee_loop)];
/// Port of `ZS_Attack_WaitOnShore` (S:1a0f0).
const ATTACK_WAIT_ON_SHORE: &[Step] = &[Custom(wait_on_shore)];
/// Port of `ZS_Attack_WaitForDialog` (S:1a000).
const ATTACK_WAIT_FOR_DIALOG: &[Step] = &[Custom(wait_for_dialog)];
/// Port of `ZS_Attack_Arena_End` (S:1ae70).
const ATTACK_ARENA_END: &[Step] = &[Custom(arena_end)];
/// Port of `ZS_Attack_End` (S:1a610).
const ATTACK_END: &[Step] = &[
    Custom(end_wrap_up),
    Custom(end_holster),
    Custom(end_finish),
    Custom(end_resume),
];

/// The definition of a combat state, `None` for states outside B08.
pub fn definition(state: State) -> Option<Def> {
    use State as S;
    let steps = |s: &'static [Step]| Some(Def::Steps(s));
    match state {
        S::Attack => steps(ATTACK),
        S::AttackLoop => steps(ATTACK_LOOP),
        S::AttackAttack => steps(ATTACK_ATTACK),
        S::AttackPowerAttack => steps(ATTACK_POWER),
        S::AttackQuickAttack => steps(ATTACK_QUICK),
        S::AttackSimpleWhirl => steps(ATTACK_SIMPLE_WHIRL),
        S::AttackWhirlAttack => steps(ATTACK_WHIRL),
        S::AttackPierceAttack => steps(ATTACK_PIERCE),
        S::AttackHackAttack => steps(ATTACK_HACK),
        S::AttackFinishingAttack => steps(ATTACK_FINISHING),
        S::AttackParade => steps(ATTACK_PARADE),
        S::AttackJumpBack => steps(ATTACK_JUMP_BACK),
        S::AttackDrinkPotion => steps(ATTACK_DRINK),
        S::AttackWait => steps(ATTACK_WAIT),
        S::AttackCast => steps(ATTACK_CAST),
        S::AttackShoot => steps(ATTACK_SHOOT),
        S::AttackGoto => steps(ATTACK_GOTO),
        S::AttackSprint => steps(ATTACK_SPRINT),
        S::AttackChangePosition => steps(ATTACK_CHANGE_POSITION),
        S::AttackGoBack => steps(ATTACK_GO_BACK),
        S::AttackStrafeLeft => steps(ATTACK_STRAFE_LEFT),
        S::AttackStrafeRight => steps(ATTACK_STRAFE_RIGHT),
        S::AttackTurnLeft => steps(ATTACK_TURN_LEFT),
        S::AttackTurnRight => steps(ATTACK_TURN_RIGHT),
        S::AttackWaitForFlee => steps(ATTACK_WAIT_FOR_FLEE),
        S::AttackWaitForFleeLoop => steps(ATTACK_WAIT_FOR_FLEE_LOOP),
        S::AttackWaitOnShore => steps(ATTACK_WAIT_ON_SHORE),
        S::AttackWaitForDialog => steps(ATTACK_WAIT_FOR_DIALOG),
        S::AttackArenaEnd => steps(ATTACK_ARENA_END),
        S::AttackEnd => steps(ATTACK_END),
        _ => None,
    }
}

/// Run the local callback of the NPC's movement instruction (called by the host every tick
/// while a [`AiCommand::CombatGoto`] runs). `true` means the movement is over because the
/// callback changed the state.
pub fn run_local_callback(host: &mut dyn ScriptHost, npc: EntityId, cb: Callback) -> bool {
    match cb {
        Callback::OnAttackGoto => on_attack_goto(host, npc),
        Callback::OnAttackPositioning => on_attack_positioning(host, npc),
        Callback::OnAttackGoBack => on_attack_go_back(host, npc),
        Callback::OnAttackStrafe => on_attack_strafe(host, npc),
        Callback::OnAttackTurn => on_attack_turn(host, npc),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{Slot, UseType};
    use crate::host::{CombatApi, NpcApi, StatsApi};
    use crate::routine::work::{Flow, run_state};
    use crate::testhost::TestWorld;

    fn duel(dist: f32) -> (TestWorld, EntityId, EntityId) {
        let mut w = TestWorld::new();
        let a = w.spawn_npc("Orc", Species::Orc, 10, 30);
        let b = w.spawn_npc("Hero", Species::Human, 10, 30);
        for e in [a, b] {
            w.set_slot(e, Slot::RightHand, UseType::OneH, 1);
        }
        w.set_distance(a, b, dist);
        w.set_target(a, b);
        w.set_combat_state(a, 1);
        w.set_task(a, State::Attack);
        (w, a, b)
    }

    /// Run the NPC's state machine until it queues something; returns that command.
    fn step(w: &mut TestWorld, a: EntityId) -> Option<AiCommand> {
        let before = w.log.len();
        for _ in 0..16 {
            if run_state(w, a) == Flow::Yield {
                break;
            }
        }
        w.log.get(before).cloned()
    }

    #[test]
    fn the_fight_starts_by_standing_up_and_enters_the_loop() {
        let (mut w, a, _) = duel(250.0);
        assert_eq!(step(&mut w, a), Some(AiCommand::StandUp));
        assert_eq!(w.ai_mode(a), AiMode::Combat);
        assert_eq!(step(&mut w, a), Some(AiCommand::SelectWeapon));
        assert_eq!(w.current_state(a), Some(State::AttackLoop));
    }

    #[test]
    fn an_orc_in_reach_swings_and_returns_to_the_loop() {
        let (mut w, a, b) = duel(250.0);
        step(&mut w, a);
        step(&mut w, a); // SelectWeapon
        let mut cmd = step(&mut w, a).expect("a swing");
        if matches!(cmd, AiCommand::Parade { .. }) {
            // The heavy moves raise the guard first.
            cmd = step(&mut w, a).expect("the thrust");
        }
        match cmd {
            AiCommand::Attack { action, target } => {
                assert!(crate::combat::moves::is_attack_action(action), "{action:?}");
                assert_eq!(target, b);
            }
            other => panic!("expected a swing, got {other:?}"),
        }
        // After the swing: the hit is stamped and the loop resumes.
        run_state(&mut w, a);
        assert_eq!(w.current_state(a), Some(State::AttackLoop));
        assert_eq!(w.hit_stamped, vec![a]);
    }

    #[test]
    fn out_of_reach_the_orc_walks_in_and_the_callback_hands_over_to_a_swing() {
        let (mut w, a, b) = duel(800.0);
        step(&mut w, a);
        step(&mut w, a);
        let cmd = step(&mut w, a).expect("the approach");
        assert_eq!(
            cmd,
            AiCommand::CombatGoto {
                target: b,
                walk_mode: 2,
                kind: GotoKind::Approach
            }
        );
        assert_eq!(w.local_callback(a), Callback::OnAttackGoto);
        // Still far: the movement goes on.
        assert!(!run_local_callback(&mut w, a, Callback::OnAttackGoto));
        // Close enough: the callback switches to a swing and stops the movement.
        w.set_distance(a, b, 250.0);
        w.set_state_position(a, 0);
        assert!(run_local_callback(&mut w, a, Callback::OnAttackGoto));
        assert!(w.stopped_goto > 0);
        assert!(matches!(
            w.current_state(a),
            Some(
                State::AttackAttack
                    | State::AttackPowerAttack
                    | State::AttackQuickAttack
                    | State::AttackPierceAttack
            )
        ));
    }

    #[test]
    fn a_chase_that_lasts_too_long_ends_in_wait_for_flee() {
        let (mut w, a, b) = duel(800.0);
        w.set_state(a, State::AttackGoto);
        w.set_state_position(a, 1);
        w.set_task_time_value(a, 6.0);
        assert!(on_attack_goto(&mut w, a));
        assert_eq!(w.current_state(a), Some(State::AttackWaitForFlee));
        // Beyond 20 m the wait ends in a cancelled attack.
        w.set_distance(a, b, 2500.0);
        w.set_state(a, State::AttackWaitForFleeLoop);
        w.set_state_time(a, 0.6);
        run_state(&mut w, a);
        assert_eq!(w.ai_mode(a), AiMode::Routine);
        assert_eq!(w.last_fight_vs_player(a), LastFight::None);
    }

    #[test]
    fn a_dead_target_ends_the_fight_and_the_orc_goes_back_to_work() {
        let (mut w, a, b) = duel(250.0);
        w.set_ransacked(a, true);
        w.set_ai_mode(b, AiMode::Dead);
        assert!(on_attack(&mut w, a));
        assert_eq!(w.current_state(a), Some(State::AttackEnd));
        // The end state: stand up, holster, wait two seconds, resume the routine.
        assert_eq!(step(&mut w, a), Some(AiCommand::StandUp));
        assert!(matches!(
            step(&mut w, a),
            Some(AiCommand::HoldInventoryStacks {
                right: -1,
                left: -1
            })
        ));
        assert_eq!(step(&mut w, a), Some(AiCommand::WaitMs { ms: 2000 }));
        run_state(&mut w, a);
        assert_eq!(w.combat_state(a), 0);
        assert_ne!(w.ai_mode(a), AiMode::Combat);
    }

    #[test]
    fn a_victor_who_wants_loot_goes_ransacking() {
        let (mut w, a, b) = duel(250.0);
        w.set_ai_mode(b, AiMode::Dead);
        w.set_state(a, State::AttackEnd);
        step(&mut w, a);
        step(&mut w, a);
        run_state(&mut w, a);
        assert_eq!(w.current_task(a), Some(State::RansackBody));
        assert_eq!(w.combat_state(a), 0);
    }

    #[test]
    fn a_downed_enemy_is_finished_when_the_reason_allows() {
        let (mut w, a, b) = duel(250.0);
        w.set_attack_reason(a, AttackReason::Enemy);
        w.set_ai_mode(b, AiMode::Down);
        w.set_state(a, State::AttackEnd);
        run_state(&mut w, a);
        assert_eq!(w.current_state(a), Some(State::AttackFinishingAttack));
        assert!(matches!(step(&mut w, a), Some(AiCommand::SaySvm { .. })));
        assert_eq!(w.log.last(), Some(&AiCommand::Parade { target: b }));
        assert!(matches!(
            step(&mut w, a),
            Some(AiCommand::Attack {
                action: Action::FinishingAttack,
                ..
            })
        ));
    }

    #[test]
    fn a_wounded_fighter_drinks_when_the_enemy_is_far() {
        let (mut w, a, _) = duel(900.0);
        w.give_item(a, "It_Potion_Health");
        w.dr_set_pool(a, crate::host::Pool::Hit, 100);
        w.set_state(a, State::AttackLoop);
        step(&mut w, a);
        run_state(&mut w, a);
        assert_eq!(w.current_state(a), Some(State::AttackDrinkPotion));
        assert_eq!(
            step(&mut w, a),
            Some(AiCommand::UseItem("It_Potion_Health"))
        );
    }

    #[test]
    fn a_swimming_target_is_waited_out_on_the_shore() {
        let (mut w, a, _) = duel(250.0);
        w.swimming = true;
        assert!(on_attack(&mut w, a));
        assert_eq!(w.current_state(a), Some(State::AttackWaitOnShore));
    }
}
