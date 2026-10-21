//! The hero's melee through the ported `PS_Melee*` states: the weapon key, the queued attack
//! actions, the instruction stream and the hit at the damage moment.
mod common;
use common::*;
use gothic_script::enums::{Action as SAction, AniState};
use gothic_script::host::MovePhase;
use gothic_script::routine::state::State;
use gothic_sim::actor::Actor;
use gothic_sim::config::SimConfig;
use gothic_sim::sim::{EventKind, Sim};
use gothic_sim::types::*;
use gothic_sim::world::{Grid2dWorld, Pos};

fn arena(host_drives: bool) -> (Sim<Grid2dWorld>, ActorId, ActorId) {
    let mut sim = Sim::new(SimConfig::default(), open_world(), synthetic_items());
    sim.player_alignment = Alignment::Reb;
    sim.state.level = 1;
    sim.state.xp = 500;
    sim.state.skills.insert("perk_1h_1".into());
    let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
    sim.equip(hero, "It_1H_Sword_01").expect("sword");
    let mut orc = Actor::npc(ActorId(0), "Orc_Raider", Species::Orc, 10, 30);
    orc.alignment = Alignment::Orc;
    orc.base_attitude_to_player = Attitude::None;
    let orc = sim.spawn(orc, Pos::flat(150.0, 0.0));
    sim.script.player.host_drives = host_drives;
    (sim, hero, orc)
}

/// The weapon key and the host's acks of the draw: the hero stands in `PS_Melee_Loop`.
fn draw(sim: &mut Sim<Grid2dWorld>) {
    run(sim, 2);
    sim.player_select_weapon();
    let ev = run(sim, 5);
    if let Some(seq) = ev.iter().find_map(|e| match e.kind {
        EventKind::HeroHold { seq, .. } => Some(seq),
        _ => None,
    }) {
        sim.hero_hold_swap(seq);
        sim.hero_exec_done(seq);
    }
    run(sim, 10);
}

fn run(sim: &mut Sim<Grid2dWorld>, ticks: usize) -> Vec<gothic_sim::sim::Event> {
    for _ in 0..ticks {
        sim.step();
    }
    sim.drain_events()
}

#[test]
fn the_weapon_key_draws_through_ps_melee_and_the_hold_swaps_the_slot() {
    let (mut sim, hero, _) = arena(true);
    run(&mut sim, 5);
    assert!(!sim.actor(hero).drawn);
    assert!(sim.player_select_weapon());
    let ev = run(&mut sim, 5);
    let hold = ev
        .iter()
        .find_map(|e| match e.kind {
            EventKind::HeroHold { seq, draw } => Some((seq, draw)),
            _ => None,
        })
        .expect("the draw is an instruction for the host");
    assert!(hold.1);
    assert!(
        !sim.actor(hero).drawn,
        "the item is still on the back during the Begin clip"
    );
    sim.hero_hold_swap(hold.0);
    assert!(sim.actor(hero).drawn, "the swap moves it into the hand");
    sim.hero_exec_done(hold.0);
    run(&mut sim, 10);
    assert_eq!(sim.script.npc(hero).state, Some(State::PsMeleeLoop));
    assert_eq!(sim.script.npc(hero).task, Some(State::PsMelee));
}

#[test]
fn a_quick_attack_hits_only_when_the_hit_clip_reaches_its_damage_moment() {
    let (mut sim, hero, orc) = arena(true);
    run(&mut sim, 2);
    sim.player_select_weapon();
    let ev = run(&mut sim, 5);
    let seq = ev
        .iter()
        .find_map(|e| match e.kind {
            EventKind::HeroHold { seq, .. } => Some(seq),
            _ => None,
        })
        .unwrap();
    sim.hero_hold_swap(seq);
    sim.hero_exec_done(seq);
    run(&mut sim, 10);
    sim.set_hero_control(Some(orc), Some(orc), false, false);
    sim.player_push_action(3, Some(orc));
    let stamina = sim.actor(hero).stamina;
    let ev = run(&mut sim, 6);
    let mv = ev
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::HeroMove {
                seq,
                action,
                phase,
                speed,
                pose,
                target,
            } => Some((*seq, *action, *phase, *speed, *pose, *target)),
            _ => None,
        })
        .expect("a combat-move instruction");
    assert_eq!(mv.1, SAction::QuickAttackR);
    assert_eq!(mv.2, MovePhase::Hit);
    assert_eq!(mv.4, AniState::Stand);
    assert_eq!(mv.5, Some(orc));
    assert!(
        (mv.3 - 1.0).abs() < 1e-4,
        "a quick attack plays at 1.0 with full stamina: {}",
        mv.3
    );
    assert!(
        sim.actor(hero).stamina < stamina,
        "the stamina was paid before the clip"
    );
    let hp = sim.actor(orc).hp;
    assert!(
        !ev.iter().any(|e| matches!(e.kind, EventKind::Hit { .. })),
        "no damage at the click"
    );
    sim.hero_move_hit(mv.0);
    let ev = sim.drain_events();
    assert!(
        ev.iter()
            .any(|e| matches!(e.kind, EventKind::Hit { attacker, victim, .. } if attacker == hero && victim == orc)),
        "the blow lands at the damage moment"
    );
    assert!(sim.actor(orc).hp < hp);
    // The hand alternates; the hit-less second ack does nothing.
    sim.hero_exec_done(mv.0);
    run(&mut sim, 8);
    sim.player_push_action(3, Some(orc));
    let ev = run(&mut sim, 8);
    assert!(ev.iter().any(|e| matches!(
        e.kind,
        EventKind::HeroMove {
            action: SAction::QuickAttackL,
            ..
        }
    )));
}

#[test]
fn the_power_attack_raises_first_and_pays_after_the_raise() {
    let (mut sim, hero, orc) = arena(true);
    draw(&mut sim);
    sim.set_hero_control(Some(orc), Some(orc), false, false);
    sim.player_push_action(2, Some(orc));
    let before = sim.actor(hero).stamina;
    let ev = run(&mut sim, 6);
    let raise = ev
        .iter()
        .find_map(|e| match e.kind {
            EventKind::HeroMove {
                seq,
                phase: MovePhase::Raise,
                speed,
                ..
            } => Some((seq, speed)),
            _ => None,
        })
        .expect("raise clip first");
    assert!((raise.1 - 1.5).abs() < 1e-4);
    assert!(
        sim.actor(hero).stamina >= before,
        "nothing paid during the wind-up"
    );
    let at_raise = sim.actor(hero).stamina;
    sim.hero_exec_done(raise.0);
    let ev = run(&mut sim, 3);
    assert!(
        sim.actor(hero).stamina <= at_raise - 15,
        "20 paid when the Hit clip starts: {at_raise} -> {}",
        sim.actor(hero).stamina
    );
    assert!(ev.iter().any(|e| matches!(
        e.kind,
        EventKind::HeroMove {
            phase: MovePhase::Hit,
            ..
        }
    )));
}

#[test]
fn headless_moves_have_no_clip_and_land_at_once() {
    let (mut sim, hero, orc) = arena(false);
    draw(&mut sim);
    sim.set_hero_control(Some(orc), Some(orc), false, false);
    let hp = sim.actor(orc).hp;
    sim.player_push_action(1, Some(orc));
    let ev = run(&mut sim, 10);
    assert!(
        ev.iter()
            .any(|e| matches!(e.kind, EventKind::Hit { .. } | EventKind::Whiff { .. }))
    );
    assert!(sim.actor(orc).hp <= hp);
}
