//! Tests of the B10 reaction states against the in-memory host.

use super::react::*;
use crate::enums::{
    Action, AiMode, Alignment, AttackReason, SecurityLevel, Slot, Species, UseType,
};
use crate::host::{AiCommand, EntityApi, EntityId, NpcApi};
use crate::routine::state::{Callback, State};
use crate::routine::work::{Flow, run_state};
use crate::testhost::TestWorld;

fn pair(dist: f32) -> (TestWorld, EntityId, EntityId) {
    let mut w = TestWorld::new();
    w.has_speech = true;
    let npc = w.spawn_npc("Guard", Species::Human, 5, 5);
    let other = w.spawn_npc("Thug", Species::Human, 5, 5);
    w.set_alignment(npc, Alignment::Reb);
    w.set_alignment(other, Alignment::Orc);
    w.set_distance(npc, other, dist);
    w.set_target(npc, other);
    (w, npc, other)
}

/// Run until the state machine settles (every queued command counts as finished at once).
fn settle(w: &mut TestWorld, a: EntityId) {
    for _ in 0..24 {
        let before = (w.current_task(a), w.current_state(a));
        if run_state(w, a) == Flow::Yield {
            continue;
        }
        if before == (w.current_task(a), w.current_state(a)) {
            break;
        }
    }
}

/// Run until the state machine queues its next command (and leave that command pending).
fn until_command(w: &mut TestWorld, a: EntityId) {
    for _ in 0..24 {
        if run_state(w, a) == Flow::Yield {
            break;
        }
    }
}

fn said(w: &TestWorld, family: &str) -> bool {
    w.log
        .iter()
        .any(|c| matches!(c, AiCommand::SaySvm { family: f, .. } if *f == family))
}

#[test]
fn the_weapon_warning_runs_on_a_timer_and_ends_in_an_attack() {
    let (mut w, npc, _) = pair(500.0);
    let player = w.player();
    w.set_distance(npc, player, 500.0);
    w.set_target(npc, player);
    w.set_slot(player, Slot::RightHand, UseType::OneH, 1);
    w.set_task(npc, State::ReactToWeapon);
    settle(&mut w, npc);
    assert_eq!(w.ai_mode(npc), AiMode::Reaction);
    assert_eq!(
        w.current_state(npc),
        Some(State::ReactToWeaponWaitForSecondWarn)
    );
    assert!(said(&w, "WeaponDown"));
    w.set_state_time(npc, 3.0);
    settle(&mut w, npc);
    assert_eq!(
        w.current_state(npc),
        Some(State::ReactToWeaponWaitForSecondWarn),
        "nothing happens before four seconds"
    );
    w.set_state_time(npc, 4.5);
    settle(&mut w, npc);
    assert_eq!(
        w.current_state(npc),
        Some(State::ReactToWeaponWaitForAttack)
    );
    assert!(said(&w, "ISaidWeaponDown"));
    w.set_state_time(npc, 4.5);
    settle(&mut w, npc);
    assert_eq!(w.current_task(npc), Some(State::Attack));
    assert_eq!(w.attack_reason(npc), AttackReason::ReactToWeapon);
}

#[test]
fn a_lowered_weapon_ends_the_warning_in_observation() {
    let (mut w, npc, _) = pair(500.0);
    let player = w.player();
    w.set_distance(npc, player, 500.0);
    w.set_target(npc, player);
    w.set_task(npc, State::ReactToWeapon);
    settle(&mut w, npc);
    assert!(
        on_react_to_weapon(&mut w, npc),
        "nothing in the player's hands"
    );
    settle(&mut w, npc);
    assert_eq!(w.current_task(npc), Some(State::ObserveSuspect));
    assert!(said(&w, "WiseMove"));
    // Walking away with the weapon out earns a last remark.
    let (mut w, npc, _) = pair(500.0);
    let player = w.player();
    w.set_slot(player, Slot::RightHand, UseType::OneH, 1);
    w.set_distance(npc, player, 1500.0);
    assert!(on_react_to_weapon(&mut w, npc));
    assert!(said(&w, "YesGetLost"));
}

#[test]
fn the_zone_guard_warns_then_attacks_the_intruder() {
    let (mut w, npc, _) = pair(300.0);
    let player = w.player();
    w.set_distance(npc, player, 300.0);
    let zone = w.add_named_zone("Camp");
    let owner = w.add_enclave(Alignment::Reb);
    w.set_zone(zone, owner, SecurityLevel::Enclave);
    w.set_zone_of(player, zone);
    w.set_task(npc, State::ReactToZone);
    settle(&mut w, npc);
    w.set_task_time_value(npc, 4.5);
    assert!(!on_react_to_zone(&mut w, npc));
    assert!(said(&w, "ISaidGetOutOfHere"));
    w.set_task_time_value(npc, 8.5);
    assert!(on_react_to_zone(&mut w, npc));
    assert_eq!(w.current_task(npc), Some(State::Attack));
    assert_eq!(w.attack_reason(npc), AttackReason::Intruder);
}

#[test]
fn a_threatening_creature_attacks_when_the_intruder_is_close() {
    let mut w = TestWorld::new();
    let wolf = w.spawn_npc("Wolf", Species::Wolf, 5, 5);
    let player = w.player();
    w.set_target(wolf, player);
    w.set_distance(wolf, player, 900.0);
    w.set_task(wolf, State::Threaten);
    settle(&mut w, wolf);
    assert_eq!(w.current_state(wolf), Some(State::ThreatenLoop));
    assert_eq!(w.ai_mode(wolf), AiMode::Reaction);
    w.set_distance(wolf, player, 600.0);
    settle(&mut w, wolf);
    assert_eq!(w.current_task(wolf), Some(State::Attack));
}

#[test]
fn a_fleeing_npc_runs_to_the_flee_point_and_runs_again_while_chased() {
    let (mut w, npc, _) = pair(400.0);
    let point = w.add_item("FleePoint");
    w.set_use_object(npc, UseType::Flee, point);
    w.set_task(npc, State::Flee);
    until_command(&mut w, npc); // StandUp
    until_command(&mut w, npc); // the walk
    assert_eq!(w.ai_mode(npc), AiMode::Flee);
    assert_eq!(w.current_state(npc), Some(State::FleeGotoFleePoint));
    assert_eq!(w.alternative_target(npc), point);
    assert_eq!(w.local_callback(npc), Callback::OnFleeGotoFleePoint);
    w.set_distance(npc, point, 900.0);
    w.set_state_time(npc, 0.6);
    assert!(!on_flee_goto_fleepoint(&mut w, npc), "still running");
    w.set_distance(npc, point, 300.0);
    w.set_state_time(npc, 0.6);
    assert!(on_flee_goto_fleepoint(&mut w, npc));
    assert_eq!(
        w.current_state(npc),
        Some(State::Flee),
        "pursuer still there: flee again"
    );
    w.set_target(npc, EntityId::NONE);
    w.set_state(npc, State::FleeGotoFleePoint);
    w.set_state_time(npc, 0.6);
    assert!(on_flee_goto_fleepoint(&mut w, npc));
    assert_eq!(w.current_state(npc), Some(State::FleeObserveFollower));
}

#[test]
fn spectators_watch_until_somebody_goes_down_and_then_applaud() {
    let (mut w, npc, foe) = pair(900.0);
    let b = w.spawn_npc("Other", Species::Human, 5, 5);
    w.set_target(npc, foe);
    w.set_alternative_target(npc, b);
    w.set_distance(npc, b, 900.0);
    w.set_task(npc, State::WatchFight);
    settle(&mut w, npc);
    assert_eq!(w.current_state(npc), Some(State::WatchFightLoop));
    assert!(!on_watch_fight(&mut w, npc));
    w.set_ai_mode(b, AiMode::Down);
    assert!(on_watch_fight(&mut w, npc));
    assert_eq!(w.current_task(npc), Some(State::Applaud));
}

#[test]
fn a_bystander_who_was_hit_looks_around_and_then_joins_the_audience() {
    let (mut w, npc, foe) = pair(400.0);
    let fighter = w.spawn_npc("Fighter", Species::Human, 5, 5);
    w.set_target(foe, fighter);
    w.set_target(fighter, foe);
    w.set_ai_mode(fighter, AiMode::Combat);
    w.set_combat_state(fighter, 1);
    w.set_task(npc, State::ReactToDamage);
    settle(&mut w, npc);
    assert_eq!(w.current_state(npc), Some(State::ReactToDamageLoop));
    w.set_task_time_value(npc, 10.5);
    assert!(on_react_to_damage(&mut w, npc));
    assert_eq!(w.current_state(npc), Some(State::ReactToDamageEnd));
    settle(&mut w, npc);
    assert_eq!(w.current_task(npc), Some(State::WatchFight));
    assert_eq!(w.alternative_target(npc), fighter, "the foe's opponent");
}

#[test]
fn cheering_depends_on_whom_the_spectator_likes() {
    let (mut w, npc, foe) = pair(400.0);
    w.set_alignment(foe, Alignment::Reb);
    w.has_speech = true;
    let rival = w.spawn_npc("Rival", Species::Human, 5, 5);
    w.set_target(npc, foe);
    w.set_alternative_target(npc, rival);
    let line = cheer_fight(&mut w, npc, foe, false).expect("a reaction");
    assert!(line.starts_with("Ooh"), "a friend was hit: {line}");
    assert_eq!(cheer_fight(&mut w, npc, foe, true), Some("HeavyHit"));
    let player = w.player();
    assert_eq!(cheer_fight(&mut w, npc, player, false), None);
}

#[test]
fn animals_eat_bodies_and_thieves_ransack_them() {
    let mut w = TestWorld::new();
    w.has_speech = true;
    let wolf = w.spawn_npc("Wolf", Species::Wolf, 5, 5);
    let body = w.spawn_npc("Corpse", Species::Human, 5, 5);
    w.set_ai_mode(body, AiMode::Dead);
    w.set_target(wolf, body);
    w.set_distance(wolf, body, 150.0);
    w.set_task(wolf, State::EatBody);
    settle(&mut w, wolf);
    assert!(w.log.iter().any(|c| matches!(
        c,
        AiCommand::Action {
            action: Action::EatGround,
            ..
        }
    )));
    let mut w = TestWorld::new();
    w.has_speech = true;
    let thief = w.spawn_npc("Thief", Species::Human, 5, 5);
    let victim = w.spawn_npc("Victim", Species::Human, 5, 5);
    w.set_ai_mode(victim, AiMode::Down);
    w.set_target(thief, victim);
    w.set_distance(thief, victim, 100.0);
    w.set_task(thief, State::RansackBody);
    settle(&mut w, thief);
    assert!(said(&w, "ShitNoGold"));
    assert_ne!(
        w.current_task(thief),
        Some(State::RansackBody),
        "fetched the weapon, back to work"
    );
}
