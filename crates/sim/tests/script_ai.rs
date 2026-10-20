//! The simulation runs the ported combat and reaction states of `gothic-script`.
mod common;
use common::*;
use gothic_script::enums as se;
use gothic_script::routine::state::State;
use gothic_sim::actor::Actor;
use gothic_sim::brain::Task;
use gothic_sim::config::SimConfig;
use gothic_sim::sim::Sim;
use gothic_sim::types::*;
use gothic_sim::world::Pos;

fn orc() -> Actor {
    let mut o = Actor::npc(ActorId(0), "Orc_Raider", Species::Orc, 10, 30);
    o.alignment = Alignment::Orc;
    o.refresh(&SimConfig::default());
    o
}

#[test]
fn a_duel_walks_through_the_attack_states_and_the_victor_goes_back_to_its_routine() {
    let items = synthetic_items();
    let mut sim = Sim::new(SimConfig::default(), open_world(), items.clone());
    sim.player_alignment = Alignment::Reb;
    sim.state.level = 1;
    sim.state.xp = 500;
    sim.state.skills.insert("perk_1h_1".into());
    let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
    sim.equip(hero, "It_1H_Sword_01").unwrap();
    {
        let h = sim.actor_mut(hero);
        h.ai_controlled = true;
        h.drawn = true;
    }
    let mut o = orc();
    if let Some(w) = gothic_sim::data::default_weapon(&items, o.species) {
        o.equipment.equip(w);
    }
    o.base_attitude_to_player = Attitude::None;
    let orc_id = sim.spawn(o, Pos::flat(1000.0, 0.0));
    sim.world.set_facing(orc_id, std::f32::consts::PI);
    sim.start_attack(hero, orc_id, AttackReason::Enemy);

    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..4000 {
        sim.step();
        if let Some(s) = sim.script.npc(orc_id).state {
            seen.insert(s);
        }
        if sim.actor(hero).is_down_or_dead() || sim.actor(orc_id).is_down_or_dead() {
            break;
        }
    }
    for s in [State::Attack, State::AttackLoop, State::AttackGoto] {
        assert!(seen.contains(&s), "{s:?} was never entered: {seen:?}");
    }
    assert!(
        seen.iter().any(|s| matches!(
            s,
            State::AttackAttack
                | State::AttackPowerAttack
                | State::AttackQuickAttack
                | State::AttackWhirlAttack
                | State::AttackHackAttack
                | State::AttackPierceAttack
        )),
        "no swing state: {seen:?}"
    );
    // The hero loses against the level-30 raider; the raider ends the fight and resumes its
    // routine ("ZS_Attack_End" -> ContinueRoutine).
    assert!(sim.actor(hero).is_down_or_dead());
    sim.run_ticks(200);
    assert_eq!(sim.actor(orc_id).brain.task, Task::Routine);
    assert_eq!(sim.actor(orc_id).ai_mode, AiMode::Routine);
    assert!(!matches!(
        sim.script.npc(orc_id).task,
        Some(State::Attack) | Some(State::AttackEnd)
    ));
}

#[test]
fn a_hunted_npc_runs_for_the_flee_point() {
    let items = synthetic_items();
    let mut sim = Sim::new(SimConfig::default(), open_world(), items);
    sim.player_alignment = Alignment::Reb;
    sim.spawn_hero(Pos::flat(5000.0, 5000.0));
    let mut runner = Actor::npc(ActorId(0), "Peasant", Species::Human, 3, 3);
    runner.alignment = Alignment::Reb;
    let runner_id = sim.spawn(runner, Pos::flat(0.0, 0.0));
    let wolf = sim.spawn(
        Actor::npc(ActorId(0), "Wolf", Species::Wolf, 5, 5),
        Pos::flat(-300.0, 0.0),
    );
    let point = sim.script.add_point(
        se::UseType::Flee,
        gothic_script::host::EntityId::NONE,
        Pos::flat(2500.0, 0.0),
    );
    sim.script
        .set_use_object(runner_id, se::UseType::Flee, point);
    sim.start_flee(runner_id, wolf);
    sim.run_ticks(20);
    assert_eq!(sim.actor(runner_id).ai_mode, AiMode::Flee);
    assert_eq!(sim.actor(runner_id).brain.task, Task::Flee);
    assert_eq!(
        sim.script.npc(runner_id).state,
        Some(State::FleeGotoFleePoint)
    );
    sim.run_ticks(200);
    let p = sim.world_pos(runner_id);
    assert!(p.x > 1500.0, "the runner moved toward the point: {p:?}");
}

#[test]
fn a_creature_threatens_before_it_attacks() {
    let items = synthetic_items();
    let mut sim = Sim::new(SimConfig::default(), open_world(), items);
    sim.player_alignment = Alignment::Reb;
    let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
    let wolf = sim.spawn(
        Actor::npc(ActorId(0), "Wolf", Species::Wolf, 5, 5),
        Pos::flat(900.0, 0.0),
    );
    sim.actor_mut(wolf).base_attitude_to_player = Attitude::Angry;
    sim.world.set_facing(wolf, std::f32::consts::PI);
    let mut tasks = std::collections::BTreeSet::new();
    for _ in 0..400 {
        sim.step();
        tasks.insert(sim.script.npc(wolf).task);
    }
    // A wolf angry at a human first hisses (ZS_Threaten), then attacks within seven meters.
    assert!(tasks.contains(&Some(State::Threaten)), "{tasks:?}");
    assert!(tasks.contains(&Some(State::Attack)), "{tasks:?}");
    let _ = hero;
}
