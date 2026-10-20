//! Tests of `SelectCombatMove` and the `FAI_*` handlers against the in-memory host.

use super::select::*;
use crate::enums::{Action, AiMode, AniState, AttackReason, Slot, Species, UseType};
use crate::host::{EntityApi, EntityId, NpcApi};
use crate::testhost::TestWorld;

fn fighters(species: Species, dist: f32) -> (TestWorld, EntityId, EntityId) {
    let mut w = TestWorld::new();
    let a = w.spawn_npc("A", species, 10, 30);
    let b = w.spawn_npc("B", Species::Human, 10, 30);
    for e in [a, b] {
        w.set_slot(e, Slot::RightHand, UseType::OneH, 1);
        w.set_ai_mode(e, AiMode::Combat);
    }
    w.set_combat_state(a, 1);
    w.set_distance(a, b, dist);
    w.set_target(a, b);
    (w, a, b)
}

#[test]
fn dead_or_down_fighters_make_no_move() {
    let (mut w, a, b) = fighters(Species::Orc, 300.0);
    w.set_ai_mode(a, AiMode::Down);
    assert_eq!(select_combat_move(&mut w, a, b), Action::None);
    w.set_ai_mode(a, AiMode::Dead);
    assert_eq!(select_combat_move(&mut w, a, b), Action::None);
}

#[test]
fn the_combat_state_picks_the_handler() {
    let (mut w, a, b) = fighters(Species::Orc, 800.0);
    // Active: close in.
    assert_eq!(select_combat_move(&mut w, a, b), Action::Fwd);
    // Passive at 8 m: approach to 4.5 m; at 3 m: back off; in between: circle.
    w.set_combat_state(a, 0);
    assert_eq!(select_combat_move(&mut w, a, b), Action::Fwd);
    w.set_distance(a, b, 300.0);
    assert_eq!(select_combat_move(&mut w, a, b), Action::Back);
    w.set_distance(a, b, 400.0);
    assert_eq!(select_combat_move(&mut w, a, b), Action::TurnLeft);
    // Anything else is not a state of the fight.
    w.set_combat_state(a, 5);
    assert_eq!(select_combat_move(&mut w, a, b), Action::None);
}

#[test]
fn a_knocked_down_enemy_is_given_room() {
    let (mut w, a, b) = fighters(Species::Orc, 700.0);
    w.set_ani_state(b, AniState::SitKnockDown);
    assert_eq!(fai_active(&mut w, a, b), Action::Fwd, "beyond 6 m: go on");
    w.set_distance(a, b, 550.0);
    assert_eq!(fai_active(&mut w, a, b), Action::Wait);
    w.set_distance(a, b, 300.0);
    assert_eq!(
        fai_active(&mut w, a, b),
        Action::Back,
        "orcs cannot jump back, they step back"
    );
}

#[test]
fn archers_run_in_when_far_and_shoot_otherwise() {
    let (mut w, a, b) = fighters(Species::Human, 2500.0);
    w.set_slot(a, Slot::LeftHand, UseType::Bow, 1);
    w.set_slot(a, Slot::RightHand, UseType::Arrow, 1);
    assert_eq!(select_combat_move(&mut w, a, b), Action::Fwd);
    w.set_distance(a, b, 1200.0);
    assert_eq!(select_combat_move(&mut w, a, b), Action::Shoot);
    // Without a line of sight the archer looks for another target first (none here).
    w.los = false;
    assert_eq!(fai_ranged(&mut w, a, b), Action::Shoot);
}

#[test]
fn mages_cast_in_the_open_and_misted_ones_cast_or_shoot_close_up() {
    let (mut w, a, b) = fighters(Species::Human, 900.0);
    w.set_slot(a, Slot::RightHand, UseType::Cast, 1);
    w.set_slot(a, Slot::LeftHand, UseType::None, 0);
    // 80 percent cast, otherwise wait.
    w.rolls = vec![10];
    assert_eq!(fai_magic(&mut w, a, b), Action::Cast);
    w.rolls = vec![95];
    assert_eq!(fai_magic(&mut w, a, b), Action::Wait);
    w.set_status(a, STATUS_MISTED);
    w.set_distance(a, b, 500.0);
    assert_eq!(select_combat_move(&mut w, a, b), Action::Cast);
}

#[test]
fn animals_sprint_at_the_player_and_otherwise_bite_in_reach() {
    let mut w = TestWorld::new();
    let wolf = w.spawn_npc("Wolf", Species::Wolf, 5, 5);
    let player = w.player();
    w.set_ai_mode(wolf, AiMode::Combat);
    w.set_combat_state(wolf, 1);
    w.set_target(wolf, player);
    w.set_distance(wolf, player, 150.0);
    w.set_current_action_direct(wolf, Action::SprintAttack);
    assert_eq!(fai_active(&mut w, wolf, player), Action::SprintAttack);
    w.set_current_action_direct(wolf, Action::None);
    w.last_hit_ts.insert(wolf, 10);
    let m = fai_active(&mut w, wolf, player);
    assert!(
        matches!(m, Action::Attack | Action::PowerAttack),
        "in reach a wolf bites: {m:?}"
    );
    w.set_distance(wolf, player, 1200.0);
    w.set_attack_reason(wolf, AttackReason::Prey);
    assert_eq!(fai_active(&mut w, wolf, player), Action::Fwd);
}
