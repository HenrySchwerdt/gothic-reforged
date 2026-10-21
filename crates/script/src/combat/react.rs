//! B10: the reaction states that sit between the daily routine and the fight: reacting to
//! damage, drawn weapons and trespassing, threatening, fleeing, watching a fight, observing a
//! suspect, eating and ransacking bodies, fetching a lost weapon and talking.
//!
//! Every state is a step table over [`Out`]; timers are the state time (reset on `SetState`) and
//! the task time. Distances are centimetres. Where the decompilation hid a condition the body
//! says so with `[I]`.

use super::attack::cancel_attack;
use crate::assess::send_perception_warn;
use crate::crime::set_attack_reason;
use crate::down::reset_all;
use crate::enums::{
    Action, AiMode, AniState, AttackReason, Attitude, LastFight, NpcType, Slot, Species, UseType,
};
use crate::host::{AiCommand, EntityId, ScriptHost};
use crate::perception::assess_target;
use crate::predicates::{can_threaten, follow_time, has_dangerous_weapon, is_guard, is_humanoid};
use crate::routine::select::continue_routine;
use crate::routine::state::{Callback, State};
use crate::routine::work::{Def, Out, Step};
use Step::Custom;

fn queue(host: &mut dyn ScriptHost, npc: EntityId, cmd: AiCommand) -> Out {
    host.queue_ai(npc, cmd);
    Out::Yield
}

fn go(host: &mut dyn ScriptHost, npc: EntityId, state: State) -> Out {
    host.set_state(npc, state);
    Out::Leave
}

fn task(host: &mut dyn ScriptHost, npc: EntityId, state: State) -> Out {
    host.set_task(npc, state);
    Out::Leave
}

/// `FUN_1000baf0`: speak a voice line (`AI_Say` by SVM name). Returns whether the NPC has one.
fn say(host: &mut dyn ScriptHost, npc: EntityId, family: &'static str) -> bool {
    if !host.has_svm_sample(npc, family) {
        return false;
    }
    host.queue_ai(npc, AiCommand::SaySvm { family, index: 0 });
    true
}

/// Speak and, if the line exists, warn the bystanders (`SendPerceptionWarn`).
fn say_and_warn(host: &mut dyn ScriptHost, npc: EntityId, family: &'static str) {
    if say(host, npc, family) {
        send_perception_warn(host, npc);
    }
}

fn holster(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    queue(
        host,
        npc,
        AiCommand::HoldInventoryStacks {
            right: -1,
            left: -1,
        },
    )
}

fn face_target(host: &mut dyn ScriptHost, npc: EntityId) {
    let t = host.current_target(npc);
    host.set_on_destination(npc, false);
    host.set_look_at(npc, t);
}

// ---- ZS_ReactToDamage ------------------------------------------------------------------------------

fn damage_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    say(host, npc, "WhatAreYouDoing");
    host.set_ai_mode(npc, AiMode::Reaction);
    face_target(host, npc);
    host.set_ani_state(npc, AniState::Stand);
    queue(host, npc, AiCommand::SelectWeapon)
}

fn damage_start(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_task_callback(npc, Callback::OnReactToDamage);
    go(host, npc, State::ReactToDamageLoop)
}

/// Port of `OnReactToDamage` (S:20af0): poll the surroundings; give up when the target is
/// farther than 7 m (30 m for archers and mages) or after ten seconds.
pub fn on_react_to_damage(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("on_react_to_damage");
    crate::assess::assess_active_perception(host, npc);
    let target = host.current_target(npc);
    let mut range = 700.0;
    if matches!(
        super::select::npc_weapon_mode(host, npc),
        crate::helpers::WeaponMode::Ranged | crate::helpers::WeaponMode::Magic
    ) {
        range = 3000.0;
    }
    if host.distance(npc, target) > range || host.task_time(npc) > 10.0 {
        host.full_stop(npc);
        host.set_state(npc, State::ReactToDamageEnd);
        return true;
    }
    false
}

fn damage_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.state_time(npc) > 0.1 {
        let target = host.current_target(npc);
        if host.distance(npc, target) >= 550.0 {
            face_target(host, npc);
            host.set_state_time(npc, 0.0);
            return go(host, npc, State::ReactToDamageGoBack);
        }
    }
    Out::Abort
}

fn damage_go_back(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let target = host.current_target(npc);
    host.set_local_callback(npc, Callback::OnReactToDamageGoBack);
    queue(host, npc, AiCommand::GotoExt { target })
}

/// Port of `OnReactToDamage_GoBack` (S:20f90): stop walking once within 3 m.
pub fn on_react_to_damage_go_back(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("on_react_to_damage_go_back");
    if host.state_time(npc) > 0.1 {
        let target = host.current_target(npc);
        if host.distance(npc, target) < 300.0 {
            host.stop_ai_goto(npc);
            host.set_state(npc, State::ReactToDamageLoop);
            return true;
        }
        host.set_state_time(npc, 0.0);
    }
    false
}

fn damage_end(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_combat_state(npc, 0);
    let target = host.current_target(npc);
    let fighters = crate::combat::attack::fighters_on(host, target);
    if fighters > 0 {
        host.full_stop(npc);
        let opponent = host.current_target(target);
        host.set_current_target(npc, target);
        host.set_alternative_target(npc, opponent);
        return task(host, npc, State::WatchFight);
    }
    continue_routine(host, npc);
    Out::Leave
}

// ---- ZS_ReactToWeapon ------------------------------------------------------------------------------

fn weapon_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let player = host.player();
    if host.ai_mode(npc) == AiMode::Routine {
        if host.last_fight_vs_player(npc) == LastFight::Won {
            say_and_warn(host, npc, "LookingForTroubleAgain");
        } else if host.slot_use_type(player, Slot::RightHand) == UseType::Cast
            || host.slot_use_type(player, Slot::LeftHand) == UseType::Cast
        {
            say_and_warn(host, npc, "StopMagic");
        } else {
            say_and_warn(host, npc, "WeaponDown");
        }
    }
    host.set_ai_mode(npc, AiMode::Reaction);
    queue(host, npc, AiCommand::StandUp)
}

fn weapon_face(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    face_target(host, npc);
    queue(host, npc, AiCommand::SelectWeapon)
}

fn weapon_start(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_task_callback(npc, Callback::OnReactToWeapon);
    go(host, npc, State::ReactToWeaponWaitForSecondWarn)
}

/// Port of `OnReactToWeapon` (S:216e0): the player walked away (beyond 10 m) with a threat
/// still drawn: "YesGetLost" and end; one who lowered the weapon within 10 m earns "WiseMove".
pub fn on_react_to_weapon(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("on_react_to_weapon");
    let player = host.player();
    if host.distance(npc, player) <= 1000.0 {
        if !has_dangerous_weapon(host, player) {
            say(host, npc, "WiseMove");
            host.set_state(npc, State::ReactToWeaponEnd);
            return true;
        }
        return false;
    }
    say(host, npc, "YesGetLost");
    host.set_task(npc, State::ReactToWeaponEnd);
    true
}

fn second_warn(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.state_time(npc) > 4.0 {
        let player = host.player();
        if host.slot_use_type(player, Slot::RightHand) == UseType::Cast
            || host.slot_use_type(player, Slot::LeftHand) == UseType::Cast
        {
            say_and_warn(host, npc, "ISaidStopMagic");
        } else {
            say_and_warn(host, npc, "ISaidWeaponDown");
        }
        return go(host, npc, State::ReactToWeaponWaitForAttack);
    }
    Out::Abort
}

fn wait_for_attack(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.state_time(npc) > 4.0 {
        let player = host.player();
        if !has_dangerous_weapon(host, player) {
            say(host, npc, "WiseMove");
            return go(host, npc, State::ReactToWeaponEnd);
        }
        assess_target(host, npc, player, AttackReason::ReactToWeapon);
        return Out::Leave;
    }
    Out::Abort
}

fn weapon_end(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_task_callback(npc, Callback::None);
    holster(host, npc)
}

fn weapon_end_wait(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    queue(host, npc, AiCommand::WaitMs { ms: 200 })
}

fn to_observe(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    task(host, npc, State::ObserveSuspect)
}

// ---- ZS_ReactToZone --------------------------------------------------------------------------------

fn zone_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    say_and_warn(host, npc, "GetLost");
    host.set_ai_mode(npc, AiMode::Reaction);
    queue(host, npc, AiCommand::StandUp)
}

fn zone_face(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    face_target(host, npc);
    queue(host, npc, AiCommand::SelectWeapon)
}

fn zone_start(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_task_callback(npc, Callback::OnReactToZone);
    go(host, npc, State::ReactToZoneLoop)
}

/// Is the player standing in a zone somebody guards (an owner and security level 2 or more)?
fn player_in_guarded_zone(host: &dyn ScriptHost) -> bool {
    let zone = host.zone(host.player());
    zone.is_some() && host.zone_owner(zone).is_some() && host.zone_security(zone).raw() >= 2
}

/// Port of `OnReactToZone` (S:221c0): the trespasser within 5 m is told to leave after 4 s
/// ("ISaidGetOutOfHere", enclave zones) and attacked with reason Intruder after 8 s; leaving
/// the guarded zone calms everything down.
pub fn on_react_to_zone(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("on_react_to_zone");
    let player = host.player();
    if host.distance(npc, player) > 500.0 {
        return false;
    }
    host.stop_ai_goto(npc);
    if host.task_position(npc) == 0 && host.task_time(npc) > 4.0 {
        if player_in_guarded_zone(host) {
            say_and_warn(host, npc, "ISaidGetOutOfHere");
            host.set_task_position(npc, 1);
        } else {
            host.set_task_callback(npc, Callback::None);
            cancel_attack(host, npc);
            return true;
        }
    }
    if host.task_position(npc) == 1 && host.task_time(npc) > 8.0 {
        host.set_task_callback(npc, Callback::None);
        if player_in_guarded_zone(host) {
            host.full_stop(npc);
            assess_target(host, npc, player, AttackReason::Intruder);
        } else {
            cancel_attack(host, npc);
        }
        return true;
    }
    false
}

fn zone_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.state_time(npc) > 1.0 {
        let player = host.player();
        host.set_state_time(npc, 0.0);
        if host.distance(npc, player) > 500.0 {
            return queue(host, npc, AiCommand::GotoExt { target: player });
        }
    }
    Out::Abort
}

// ---- ZS_Threaten -----------------------------------------------------------------------------------

fn threaten_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_ai_mode(npc, AiMode::Reaction);
    holster(host, npc)
}

fn threaten_stand(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    queue(host, npc, AiCommand::StandUp)
}

fn threaten_face(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    face_target(host, npc);
    go(host, npc, State::ThreatenLoop)
}

/// Port of `ZS_Threaten_Loop` (S:26440): a creature that hisses at an intruder attacks below
/// 7 m (reason Angry), loses interest beyond 10 m, and attacks after 5 s anyway.
fn threaten_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    gothic_portcov::hit!("threaten_loop");
    let target = host.current_target(npc);
    let d = host.distance(npc, target);
    if d < 700.0 {
        assess_target(host, npc, target, AttackReason::Angry);
        return Out::Leave;
    }
    if d > 1000.0 {
        continue_routine(host, npc);
        return Out::Leave;
    }
    if host.task_time(npc) > 5.0 {
        assess_target(host, npc, target, AttackReason::Angry);
        return Out::Leave;
    }
    Out::Abort
}

// ---- ZS_Flee ---------------------------------------------------------------------------------------

fn flee_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.ai_mode(npc) != AiMode::Flee {
        let enclave = host.enclave(npc);
        let revolt = enclave.is_some()
            && host.enclave_status(enclave) == crate::enums::EnclaveStatus::Attack;
        say(host, npc, if revolt { "RunRevolution" } else { "RunAway" });
        host.set_ai_mode(npc, AiMode::Flee);
        reset_all(host, npc);
        return queue(host, npc, AiCommand::StandUp);
    }
    Out::Next
}

fn flee_pick_point(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let point = host.find_use_object(npc, UseType::Flee);
    if point.is_some() {
        host.set_alternative_target(npc, point);
        return go(host, npc, State::FleeGotoFleePoint);
    }
    go(host, npc, State::FleeGoBack)
}

fn flee_goto_point(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let point = host.alternative_target(npc);
    host.set_look_at(npc, EntityId::NONE);
    host.set_local_callback(npc, Callback::OnFleeGotoFleePoint);
    queue(host, npc, AiCommand::GotoExt { target: point })
}

fn flee_goto_done(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    continue_routine(host, npc);
    Out::Leave
}

/// Port of `OnFlee_GotoFleePoint` (S:1dff0): every half second check whether the flee point is
/// reached (5 m); then flee again from the pursuer, or watch for a follower.
pub fn on_flee_goto_fleepoint(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("on_flee_goto_fleepoint");
    if host.state_time(npc) <= 0.5 {
        return false;
    }
    let point = host.alternative_target(npc);
    if host.distance(npc, point) > 500.0 {
        host.set_state_time(npc, 0.0);
        return false;
    }
    host.stop_ai_goto(npc);
    let target = host.current_target(npc);
    if target.is_some() {
        host.set_state(npc, State::Flee);
    } else {
        host.set_look_at(npc, target);
        host.set_state(npc, State::FleeObserveFollower);
    }
    true
}

fn flee_observe(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.state_time(npc) > 0.5 {
        let target = host.current_target(npc);
        if host.distance(npc, target) > 2000.0 {
            host.set_state_time(npc, 0.0);
        } else {
            return go(host, npc, State::Flee);
        }
    }
    Out::Abort
}

fn flee_go_back(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let target = host.current_target(npc);
    host.set_look_at(npc, target);
    host.set_local_callback(npc, Callback::OnFleeGoBack);
    let home = host
        .start_position(npc)
        .map(|p| AiCommand::GotoPos { pos: p })
        .unwrap_or(AiCommand::GotoExt { target });
    queue(host, npc, home)
}

fn flee_go_back_done(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    continue_routine(host, npc);
    Out::Leave
}

/// Port of `OnFlee_GoBack` (S:1e470): once the pursuer is within 20 m watch it.
pub fn on_flee_go_back(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("on_flee_go_back");
    if host.state_time(npc) > 0.5 {
        let target = host.current_target(npc);
        if host.distance(npc, target) < 2000.0 {
            host.set_state_time(npc, 0.0);
            host.stop_ai_goto(npc);
            host.set_look_at(npc, target);
            host.set_state(npc, State::FleeObserveFollower);
            return true;
        }
        host.set_state_time(npc, 0.0);
    }
    false
}

// ---- ZS_WatchFight ---------------------------------------------------------------------------------

fn watch_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let a = host.current_target(npc);
    let b = host.alternative_target(npc);
    if crate::predicates::is_boss(host, npc) {
        continue_routine(host, npc);
        return Out::Leave;
    }
    if host.ai_mode(npc) == AiMode::Routine
        && is_humanoid(host, npc)
        && is_humanoid(host, a)
        && is_humanoid(host, b)
        && host.ai_mode(a) != AiMode::Flee
        && host.ai_mode(b) != AiMode::Flee
    {
        say(host, npc, "ThereIsAFight");
    }
    host.set_ai_mode(npc, AiMode::Reaction);
    queue(host, npc, AiCommand::StandUp)
}

fn watch_holster(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    holster(host, npc)
}

fn watch_start(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_task_callback(npc, Callback::OnWatchFight);
    let a = host.current_target(npc);
    let b = host.alternative_target(npc);
    let arena = host.ai_mode(a) == AiMode::Arena
        || host.ai_mode(b) == AiMode::Arena
        || host.attack_reason(a) == AttackReason::Arena
        || host.attack_reason(b) == AttackReason::Arena;
    go(
        host,
        npc,
        if arena {
            State::WatchFightSpectator
        } else {
            State::WatchFightLoop
        },
    )
}

/// Port of `OnWatchFight` (S:27e40): a fight where somebody is dead or down ends the show
/// (applause for the player's victory); a fight that left the arena zone is ignored.
pub fn on_watch_fight(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("on_watch_fight");
    let player = host.player();
    let a = host.current_target(npc);
    let b = host.alternative_target(npc);
    let over = |host: &dyn ScriptHost, e: EntityId| {
        e.is_some() && matches!(host.ai_mode(e), AiMode::Dead | AiMode::Down)
    };
    let finished = over(host, a) || over(host, b);
    if finished {
        let now = host.playing_time();
        for e in [a, b] {
            if e == player && over(host, e) {
                host.set_last_fight_vs_player(npc, LastFight::Won);
                host.set_last_fight_timestamp(npc, now);
            }
        }
        host.stop_ai_goto(npc);
        host.set_task(npc, State::Applaud);
        return true;
    }
    if a.is_none() && b.is_none() {
        host.stop_ai_goto(npc);
        continue_routine(host, npc);
        return true;
    }
    false
}

fn nearest_fighter(host: &dyn ScriptHost, npc: EntityId) -> (EntityId, f32) {
    let a = host.current_target(npc);
    let b = host.alternative_target(npc);
    let da = if a.is_some() {
        host.distance(npc, a)
    } else {
        f32::MAX
    };
    let db = if b.is_some() {
        host.distance(npc, b)
    } else {
        f32::MAX
    };
    if db < da { (b, db) } else { (a, da) }
}

fn watch_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.state_time(npc) > 0.1 {
        let (near, d) = nearest_fighter(host, npc);
        if near.is_some() {
            host.set_on_destination(npc, false);
            host.set_look_at(npc, near);
            if d >= 550.0 {
                if d > 600.0 {
                    host.set_state_time(npc, 0.0);
                    return go(host, npc, State::WatchFightGoto);
                }
                return go(host, npc, State::WatchFightGoBack);
            }
        }
    }
    Out::Abort
}

fn watch_go_back(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let target = host.current_target(npc);
    if host.can_evade(npc, target, crate::host::dir::BACK) {
        host.set_local_callback(npc, Callback::OnWatchFightGoBack);
        return queue(host, npc, AiCommand::GotoExt { target });
    }
    go(host, npc, State::WatchFightLoop)
}

fn watch_back_done(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    go(host, npc, State::WatchFightLoop)
}

/// Port of `OnWatchFight_GoBack` (S:286b0): stop once the fight is 6 m away again.
pub fn on_watch_fight_go_back(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("on_watch_fight_go_back");
    if host.state_time(npc) > 0.1 {
        let target = host.current_target(npc);
        if host.distance(npc, target) <= 600.0 {
            host.set_state_time(npc, 0.0);
            host.stop_ai_goto(npc);
            host.set_state(npc, State::WatchFightLoop);
            return true;
        }
    }
    false
}

fn watch_goto(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let target = host.current_target(npc);
    let b = host.alternative_target(npc);
    if host.species(npc) == Species::Human
        && target.is_some()
        && b.is_some()
        && host.attack_reason(target) != AttackReason::Arena
    {
        host.set_local_callback(npc, Callback::OnWatchFightGoto);
        return queue(host, npc, AiCommand::GotoExt { target });
    }
    go(host, npc, State::WatchFightLoop)
}

fn watch_goto_done(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    go(host, npc, State::WatchFightLoop)
}

/// Port of `OnWatchFight_Goto` (S:28a60): walk toward the fight until 6 m away; chasing longer
/// than `GetFollowTime` makes the spectator wait for the fighters to flee.
pub fn on_watch_fight_goto(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("on_watch_fight_goto");
    let target = host.current_target(npc);
    if host.state_time(npc) <= 0.5 || host.distance(npc, target) >= 600.0 {
        if host.state_time(npc) > 0.5
            && host.distance(npc, target) >= 600.0
            && host.task_time(npc) > follow_time(host, npc) as f32
        {
            host.set_state_time(npc, 0.0);
            host.full_stop(npc);
            host.set_state(npc, State::WatchFightWaitForFlee);
            return true;
        }
        return false;
    }
    host.set_state_time(npc, 0.0);
    host.stop_ai_goto(npc);
    host.set_state(npc, State::WatchFightLoop);
    true
}

fn spectator_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    // Walk to a spectator seat when the arena has one (`UseType::Spectator`), else watch from here.
    let seat = host.find_use_object(npc, UseType::Spectator);
    if seat.is_none() {
        return go(host, npc, State::WatchFightSpectatorLoop);
    }
    queue(host, npc, AiCommand::GotoExt { target: seat })
}

fn spectator_seated(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    go(host, npc, State::WatchFightSpectatorLoop)
}

fn spectator_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.state_time(npc) > 0.1 {
        let (near, _) = nearest_fighter(host, npc);
        if near.is_some() {
            host.set_on_destination(npc, false);
            host.set_look_at(npc, near);
        }
        host.set_state_time(npc, 0.0);
    }
    Out::Abort
}

fn watch_wait_for_flee(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_last_dist_to_target(npc, 0.0);
    go(host, npc, State::WatchFightWaitForFleeLoop)
}

fn watch_wait_for_flee_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.state_time(npc) > 1.0 {
        let target = host.current_target(npc);
        let d = host.distance(npc, target);
        if d <= 2000.0 && host.last_dist_to_target(npc) <= d {
            host.set_last_dist_to_target(npc, d);
            host.set_state_time(npc, 0.0);
            return go(host, npc, State::WatchFightLoop);
        }
        continue_routine(host, npc);
        return Out::Leave;
    }
    Out::Abort
}

/// Port of `CheerFight` (S:28bb0): a spectator's voice reaction to a seen hit. Friends of the
/// hit fighter groan ("Ooh"), the others cheer; heavy reactions get "HeavyHit". Returns the
/// line spoken.
pub fn cheer_fight(
    host: &mut dyn ScriptHost,
    spectator: EntityId,
    victim: EntityId,
    heavy: bool,
) -> Option<&'static str> {
    gothic_portcov::hit!("cheer_fight");
    if !is_humanoid(host, spectator) || !host.has_svm_sample(spectator, "Watchfight") {
        return None;
    }
    let a = host.current_target(spectator);
    let b = host.alternative_target(spectator);
    if victim != a && victim != b {
        return None;
    }
    let pick = host.random(3);
    let line: &'static str = if heavy {
        "HeavyHit"
    } else if crate::attitude::get_attitude(host, spectator, victim) == Attitude::Friendly {
        ["Ooh_00", "Ooh_01", "Ooh_02"][pick as usize]
    } else {
        ["Cheer_00", "Cheer_01", "Cheer_02"][pick as usize]
    };
    say(host, spectator, line);
    Some(line)
}

// ---- ZS_ObserveSuspect and friends ----------------------------------------------------------------

fn observe_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let target = host.current_target(npc);
    if target.is_none() && host.last_task(npc) != Some(State::Talk) {
        continue_routine(host, npc);
        return Out::Leave;
    }
    host.set_ai_mode(npc, AiMode::Reaction);
    if crate::predicates::is_gate_guard(host, npc) {
        return task(host, npc, State::Guard);
    }
    host.set_task_callback(npc, Callback::OnGoto);
    let sitting = matches!(
        host.ani_state(npc),
        AniState::SitGround | AniState::SitStool | AniState::SitBench | AniState::SitThrone
    );
    if !sitting || !host.is_in_fov(npc, target) {
        return queue(host, npc, AiCommand::StandUp);
    }
    Out::Next
}

fn observe_face(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let target = host.current_target(npc);
    if matches!(
        host.ani_state(npc),
        AniState::SitGround | AniState::SitStool | AniState::SitBench | AniState::SitThrone
    ) {
        host.set_look_at(npc, target);
    }
    face_target(host, npc);
    go(host, npc, State::ObserveSuspectLoop)
}

/// Port of `ZS_ObserveSuspect_Loop` (S:1e8b0): once a second look again. A sitting observer
/// who lost sight of the suspect starts over; a harmless suspect far away (7 m) or watched for
/// more than 4 s ends the observation [I on the zone/guard clauses].
fn observe_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    gothic_portcov::hit!("observe_loop");
    if host.state_time(npc) <= 1.0 {
        return Out::Abort;
    }
    let target = host.current_target(npc);
    let sitting = matches!(
        host.ani_state(npc),
        AniState::SitGround | AniState::SitStool | AniState::SitBench | AniState::SitThrone
    );
    if sitting && !host.is_in_fov(npc, target) {
        return task(host, npc, State::ObserveSuspect);
    }
    let hostile = crate::attitude::get_attitude(host, npc, target).raw() >= Attitude::Hostile.raw();
    if !hostile && host.distance(npc, target) > 700.0 {
        continue_routine(host, npc);
        return Out::Leave;
    }
    if host.task_time(npc) > 4.0 && !hostile {
        continue_routine(host, npc);
        host.set_task_time(npc, 0.0);
        return Out::Leave;
    }
    host.set_state_time(npc, 0.0);
    Out::Abort
}

fn sneak_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_ai_mode(npc, AiMode::Reaction);
    say_and_warn(host, npc, "WhatsThisSupposedToBe");
    queue(host, npc, AiCommand::StandUp)
}

fn sneak_face(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    face_target(host, npc);
    queue(host, npc, AiCommand::WaitMs { ms: 200 })
}

fn observe_weapon_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_ai_mode(npc, AiMode::Reaction);
    queue(host, npc, AiCommand::StandUp)
}

fn observe_weapon_select(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    queue(host, npc, AiCommand::SelectWeapon)
}

fn applaud(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_ai_mode(npc, AiMode::Reaction);
    if host.find_stack_index(npc, Slot::LeftHand) == -1
        && host.find_stack_index(npc, Slot::RightHand) == -1
    {
        let secs = 2 + host.random(5);
        return queue(
            host,
            npc,
            AiCommand::Action {
                action: Action::Hold,
                secs,
            },
        );
    }
    Out::Next
}

fn zone_player_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_ai_mode(npc, AiMode::Reaction);
    say(host, npc, "WhyAreYouInHere");
    task(host, npc, State::ObserveSuspect)
}

fn footsteps(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_ai_mode(npc, AiMode::Reaction);
    say(host, npc, "YouDisturbedMySlumber");
    queue(host, npc, AiCommand::StandUp)
}

// ---- ZS_EatBody ------------------------------------------------------------------------------------

fn eat_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_ai_mode(npc, AiMode::Reaction);
    host.set_look_at(npc, EntityId::NONE);
    holster(host, npc)
}

fn eat_walk(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_task_callback(npc, Callback::OnEatBodyPerception);
    let body = host.current_target(npc);
    queue(host, npc, AiCommand::GotoExt { target: body })
}

fn eat_arrive(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let body = host.current_target(npc);
    if host.distance(npc, body) <= 260.0 {
        return go(host, npc, State::EatBodyLoop);
    }
    continue_routine(host, npc);
    Out::Leave
}

fn eat_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let secs = (host.random(3) + 2) * 60;
    queue(
        host,
        npc,
        AiCommand::Action {
            action: Action::EatGround,
            secs,
        },
    )
}

fn eat_done(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    continue_routine(host, npc);
    Out::Leave
}

/// Port of `OnEatBody_Perception` (S:1d5a0): a meal that is disturbed by a hostile creature
/// within 5 m is defended (`AssessTarget` with reason DriveAway) against up to five candidates.
pub fn on_eat_body_perception(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("on_eat_body_perception");
    let mut candidates = vec![host.alternative_target(npc)];
    for n in host.all_npcs() {
        if n != npc && host.distance(npc, n) < 500.0 {
            candidates.push(n);
        }
    }
    for c in candidates {
        if c.is_none() || c == npc {
            continue;
        }
        if host.distance(npc, c) < 500.0
            && matches!(
                host.ai_mode(c),
                AiMode::Routine | AiMode::Combat | AiMode::Reaction
            )
            && crate::attitude::get_attitude(host, npc, c).raw() >= Attitude::Hostile.raw()
        {
            assess_target(host, npc, c, AttackReason::DriveAway);
            return true;
        }
    }
    false
}

// ---- ZS_RansackBody and ZS_FetchWeapon -------------------------------------------------------------

fn ransack_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_ai_mode(npc, AiMode::Reaction);
    host.set_task_callback(npc, Callback::OnRansackBody);
    let body = host.current_target(npc);
    queue(host, npc, AiCommand::GotoExt { target: body })
}

/// The loot: the body's gold is split in half (a body without gold earns a curse), then the
/// ransacker looks for a weapon to pick up.
fn ransack_loot(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let body = host.current_target(npc);
    if host.distance(npc, body) > 400.0 || host.ai_mode(body) != AiMode::Down {
        continue_routine(host, npc);
        return Out::Leave;
    }
    host.set_task_time(npc, 0.0);
    let gold = host.item_amount(body, "It_Gold");
    if gold < 1 {
        say(host, npc, "ShitNoGold");
    } else {
        let line = ["RansackGold_00", "RansackGold_01", "RansackGold_02"][host.random(3) as usize];
        say(host, npc, line);
        let half = (gold / 2).max(1).min(gold);
        host.transfer_items(body, npc, "It_Gold", half);
    }
    queue(host, npc, AiCommand::WaitMs { ms: 2000 })
}

fn ransack_done(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let body = host.current_target(npc);
    host.set_current_target(npc, EntityId::NONE);
    host.set_alternative_target(npc, body);
    task(host, npc, State::FetchWeapon)
}

/// Port of `OnRansackBody` (S:1fd70): a body that stopped being down ends the ransacking.
pub fn on_ransack_body(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("on_ransack_body");
    crate::assess::assess_active_perception(host, npc);
    let body = host.current_target(npc);
    if body.is_some() && host.ai_mode(body) != AiMode::Down {
        host.full_stop(npc);
        host.set_current_target(npc, EntityId::NONE);
        host.set_task(npc, State::ObserveSuspect);
        return true;
    }
    false
}

fn fetch_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_ai_mode(npc, AiMode::Reaction);
    queue(host, npc, AiCommand::StandUp)
}

/// Walk to the weapon the NPC dropped (the alternative target) and take it; nothing within 20 m
/// means back to the routine.
fn fetch_goto(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let item = host.alternative_target(npc);
    if item.is_none() || host.distance(npc, item) > 2000.0 {
        continue_routine(host, npc);
        return Out::Leave;
    }
    host.set_current_target(npc, item);
    host.set_task_callback(npc, Callback::OnGoto);
    queue(host, npc, AiCommand::GotoExt { target: item })
}

fn fetch_take(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let item = host.current_target(npc);
    queue(host, npc, AiCommand::TakeItem { item })
}

fn fetch_done(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_task_callback(npc, Callback::None);
    host.set_current_target(npc, EntityId::NONE);
    continue_routine(host, npc);
    Out::Leave
}

// ---- ZS_Talk -----------------------------------------------------------------------------------------

fn talk_enter(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_current_target(npc, EntityId::NONE);
    host.set_ai_mode(npc, AiMode::Talk);
    host.start_dialog(npc);
    queue(host, npc, AiCommand::StandUp)
}

fn talk_face(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let player = host.player();
    host.set_look_at(npc, player);
    if !matches!(host.species(npc), Species::Bison) {
        host.set_on_destination(npc, false);
    }
    go(host, npc, State::TalkLoop)
}

fn talk_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if !host.in_dialog_with_player(npc) {
        host.set_ai_mode(npc, AiMode::Routine);
        continue_routine(host, npc);
        return Out::Leave;
    }
    Out::Abort
}

/// Port of `ZS_Lee_Teleport` (S:25c00): Lee stands up, takes the Vengard temple teleport rune
/// and casts it, then goes back to his routine.
fn lee_stand(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    gothic_portcov::hit!("lee_stand");
    queue(host, npc, AiCommand::StandUp)
}

fn lee_rune(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    queue(
        host,
        npc,
        AiCommand::HoldItems {
            right: "It_Teleport_VangardTempel",
            left: "",
        },
    )
}

fn lee_cast(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    queue(host, npc, AiCommand::Cast { power: false })
}

fn lee_done(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    continue_routine(host, npc);
    Out::Leave
}

// ---- tables ----------------------------------------------------------------------------------------

const REACT_TO_DAMAGE: &[Step] = &[Custom(damage_enter), Custom(damage_start)];
const REACT_TO_DAMAGE_LOOP: &[Step] = &[Custom(damage_loop)];
const REACT_TO_DAMAGE_GO_BACK: &[Step] = &[Custom(damage_go_back), Custom(damage_loop_back)];
const REACT_TO_DAMAGE_END: &[Step] = &[Custom(damage_end)];
const REACT_TO_WEAPON: &[Step] = &[
    Custom(weapon_enter),
    Custom(weapon_face),
    Custom(weapon_start),
];
const REACT_TO_WEAPON_SECOND: &[Step] = &[Custom(second_warn)];
const REACT_TO_WEAPON_ATTACK: &[Step] = &[Custom(wait_for_attack)];
const REACT_TO_WEAPON_END: &[Step] = &[
    Custom(weapon_end),
    Custom(weapon_end_wait),
    Custom(to_observe),
];
const REACT_TO_ZONE: &[Step] = &[Custom(zone_enter), Custom(zone_face), Custom(zone_start)];
const REACT_TO_ZONE_LOOP: &[Step] = &[Custom(zone_loop)];
const THREATEN: &[Step] = &[
    Custom(threaten_enter),
    Custom(threaten_stand),
    Custom(threaten_face),
];
const THREATEN_LOOP: &[Step] = &[Custom(threaten_loop)];
const FLEE: &[Step] = &[Custom(flee_enter), Custom(flee_pick_point)];
const FLEE_GOTO: &[Step] = &[Custom(flee_goto_point), Custom(flee_goto_done)];
const FLEE_OBSERVE: &[Step] = &[Custom(flee_observe)];
const FLEE_GO_BACK: &[Step] = &[Custom(flee_go_back), Custom(flee_go_back_done)];
const WATCH_FIGHT: &[Step] = &[
    Custom(watch_enter),
    Custom(watch_holster),
    Custom(watch_start),
];
const WATCH_FIGHT_LOOP: &[Step] = &[Custom(watch_loop)];
const WATCH_FIGHT_GO_BACK: &[Step] = &[Custom(watch_go_back), Custom(watch_back_done)];
const WATCH_FIGHT_GOTO: &[Step] = &[Custom(watch_goto), Custom(watch_goto_done)];
const WATCH_FIGHT_SPECTATOR: &[Step] = &[Custom(spectator_enter), Custom(spectator_seated)];
const WATCH_FIGHT_SPECTATOR_LOOP: &[Step] = &[Custom(spectator_loop)];
const WATCH_FIGHT_WAIT_FOR_FLEE: &[Step] = &[Custom(watch_wait_for_flee)];
const WATCH_FIGHT_WAIT_FOR_FLEE_LOOP: &[Step] = &[Custom(watch_wait_for_flee_loop)];
const OBSERVE_SUSPECT: &[Step] = &[Custom(observe_enter), Custom(observe_face)];
const OBSERVE_SUSPECT_LOOP: &[Step] = &[Custom(observe_loop)];
const REACT_TO_SNEAK: &[Step] = &[Custom(sneak_enter), Custom(sneak_face), Custom(to_observe)];
const OBSERVE_WEAPON: &[Step] = &[
    Custom(observe_weapon_enter),
    Custom(observe_weapon_select),
    Custom(to_observe),
];
const APPLAUD: &[Step] = &[Custom(applaud), Custom(to_observe)];
const OBSERVE_ZONE_PLAYER: &[Step] = &[Custom(zone_player_enter)];
const REACT_TO_FOOTSTEPS: &[Step] = &[Custom(footsteps), Custom(to_observe)];
const EAT_BODY: &[Step] = &[Custom(eat_enter), Custom(eat_walk), Custom(eat_arrive)];
const EAT_BODY_LOOP: &[Step] = &[Custom(eat_loop), Custom(eat_done)];
const RANSACK_BODY: &[Step] = &[
    Custom(ransack_enter),
    Custom(ransack_loot),
    Custom(ransack_done),
];
const FETCH_WEAPON: &[Step] = &[
    Custom(fetch_enter),
    Custom(fetch_goto),
    Custom(fetch_take),
    Custom(fetch_done),
];
const TALK: &[Step] = &[Custom(talk_enter), Custom(talk_face)];
const TALK_LOOP: &[Step] = &[Custom(talk_loop)];
const LEE_TELEPORT: &[Step] = &[
    Custom(lee_stand),
    Custom(lee_rune),
    Custom(lee_cast),
    Custom(lee_done),
];

fn damage_loop_back(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    go(host, npc, State::ReactToDamageLoop)
}

/// The definition of a reaction state, `None` for states outside B10.
pub fn definition(state: State) -> Option<Def> {
    use State as S;
    let steps = |s: &'static [Step]| Some(Def::Steps(s));
    match state {
        S::ReactToDamage => steps(REACT_TO_DAMAGE),
        S::ReactToDamageLoop => steps(REACT_TO_DAMAGE_LOOP),
        S::ReactToDamageGoBack => steps(REACT_TO_DAMAGE_GO_BACK),
        S::ReactToDamageEnd => steps(REACT_TO_DAMAGE_END),
        S::ReactToWeapon => steps(REACT_TO_WEAPON),
        S::ReactToWeaponWaitForSecondWarn => steps(REACT_TO_WEAPON_SECOND),
        S::ReactToWeaponWaitForAttack => steps(REACT_TO_WEAPON_ATTACK),
        S::ReactToWeaponEnd => steps(REACT_TO_WEAPON_END),
        S::ReactToZone => steps(REACT_TO_ZONE),
        S::ReactToZoneLoop => steps(REACT_TO_ZONE_LOOP),
        S::Threaten => steps(THREATEN),
        S::ThreatenLoop => steps(THREATEN_LOOP),
        S::Flee => steps(FLEE),
        S::FleeGotoFleePoint => steps(FLEE_GOTO),
        S::FleeObserveFollower => steps(FLEE_OBSERVE),
        S::FleeGoBack => steps(FLEE_GO_BACK),
        S::WatchFight => steps(WATCH_FIGHT),
        S::WatchFightLoop => steps(WATCH_FIGHT_LOOP),
        S::WatchFightGoBack => steps(WATCH_FIGHT_GO_BACK),
        S::WatchFightGoto => steps(WATCH_FIGHT_GOTO),
        S::WatchFightSpectator => steps(WATCH_FIGHT_SPECTATOR),
        S::WatchFightSpectatorLoop => steps(WATCH_FIGHT_SPECTATOR_LOOP),
        S::WatchFightWaitForFlee => steps(WATCH_FIGHT_WAIT_FOR_FLEE),
        S::WatchFightWaitForFleeLoop => steps(WATCH_FIGHT_WAIT_FOR_FLEE_LOOP),
        S::ObserveSuspect => steps(OBSERVE_SUSPECT),
        S::ObserveSuspectLoop => steps(OBSERVE_SUSPECT_LOOP),
        S::ReactToSneak => steps(REACT_TO_SNEAK),
        S::ObserveWeapon => steps(OBSERVE_WEAPON),
        S::Applaud => steps(APPLAUD),
        S::ObserveZonePlayer => steps(OBSERVE_ZONE_PLAYER),
        S::ReactToFootsteps => steps(REACT_TO_FOOTSTEPS),
        S::EatBody => steps(EAT_BODY),
        S::EatBodyLoop => steps(EAT_BODY_LOOP),
        S::RansackBody => steps(RANSACK_BODY),
        S::FetchWeapon => steps(FETCH_WEAPON),
        S::Talk => steps(TALK),
        S::TalkLoop => steps(TALK_LOOP),
        S::LeeTeleport => steps(LEE_TELEPORT),
        _ => None,
    }
}

/// Run the local or task callback `cb` of a reaction state; `true` means it changed the state.
pub fn run_callback(host: &mut dyn ScriptHost, npc: EntityId, cb: Callback) -> bool {
    match cb {
        Callback::OnReactToDamage => on_react_to_damage(host, npc),
        Callback::OnReactToDamageGoBack => on_react_to_damage_go_back(host, npc),
        Callback::OnReactToWeapon => on_react_to_weapon(host, npc),
        Callback::OnReactToZone => on_react_to_zone(host, npc),
        Callback::OnWatchFight => on_watch_fight(host, npc),
        Callback::OnWatchFightGoBack => on_watch_fight_go_back(host, npc),
        Callback::OnWatchFightGoto => on_watch_fight_goto(host, npc),
        Callback::OnFleeGotoFleePoint => on_flee_goto_fleepoint(host, npc),
        Callback::OnFleeGoBack => on_flee_go_back(host, npc),
        Callback::OnEatBodyPerception => on_eat_body_perception(host, npc),
        Callback::OnRansackBody => on_ransack_body(host, npc),
        _ => false,
    }
}

/// Is `npc` a guard or friend-type NPC that never lets a trespasser be (used by the zone
/// reactions to decide who may speak the warnings).
pub fn may_warn(host: &dyn ScriptHost, npc: EntityId) -> bool {
    is_guard(host, npc) || host.npc_type(npc) != NpcType::Friend
}

/// Can this species play the threaten reaction at all (`CanThreaten`)?
pub fn threatens(host: &dyn ScriptHost, npc: EntityId) -> bool {
    can_threaten(host, npc)
}

/// Record a drive-away attack reason from a perception.
pub fn drive_away(host: &mut dyn ScriptHost, npc: EntityId, target: EntityId) {
    set_attack_reason(host, npc, target, AttackReason::DriveAway);
}
