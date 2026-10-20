//! Crime scenarios: theft seen by a guard, murder, weapon reactions, excuses.
mod common;
use common::*;
use gothic_sim::actor::Actor;
use gothic_sim::brain::Task;
use gothic_sim::config::SimConfig;
use gothic_sim::crime::Memorized;
use gothic_sim::sim::{EventKind, Sim};
use gothic_sim::types::*;
use gothic_sim::world::{Grid2dWorld, Pos, Rect, WorldQuery};

struct Scene {
    sim: Sim<Grid2dWorld>,
    hero: ActorId,
    guard: ActorId,
    guard2: ActorId,
}

fn guard_actor(name: &str) -> Actor {
    let mut g = Actor::npc(ActorId(0), name, Species::Human, 20, 20);
    g.npc_type = NpcType::Enclave;
    g.alignment = Alignment::Mid;
    g.enclave = Some("trelis".into());
    g.equipment.equip(gothic_sim::item::ItemDef::weapon(
        "It_1H_Sword_01",
        "1H",
        "Blade",
        50,
    ));
    g
}

/// A guard 400 units from the hero, facing him; a second guard nearby; hero is a Mid member.
fn scene(seed: u64, outdoors: bool) -> Scene {
    let cfg = SimConfig {
        seed,
        ..SimConfig::default()
    };
    let mut world = open_world();
    world.outdoors = outdoors;
    let mut sim = Sim::new(cfg, world, synthetic_items());
    sim.player_alignment = Alignment::Mid;
    sim.state.level = 1;
    sim.state.skills.insert("perk_1h_1".into());
    let hero = sim.spawn_hero(Pos::flat(400.0, 0.0));
    let guard = sim.spawn(guard_actor("Guard_01"), Pos::flat(0.0, 0.0));
    sim.world.set_facing(guard, 0.0);
    let guard2 = sim.spawn(guard_actor("Guard_02"), Pos::flat(200.0, 600.0));
    sim.world.set_facing(guard2, -std::f32::consts::FRAC_PI_4);
    Scene {
        sim,
        hero,
        guard,
        guard2,
    }
}

#[test]
fn theft_seen_by_guard_triggers_attack_enclave_memory_and_help() {
    let Scene {
        mut sim,
        hero,
        guard,
        guard2,
    } = scene(1, true);
    // The second guard looks away: it does not witness the theft but answers the call.
    sim.world.set_facing(guard2, std::f32::consts::PI);
    // Before the crime the guard is neutral to the player [CA 10.4].
    assert_eq!(sim.attitude(guard, hero), Attitude::Neutral);

    let reacted = sim.witness_theft("It_Sword_01", false, false);
    assert!(reacted.contains(&guard), "the guard saw it: {reacted:?}");

    let g = sim.actor(guard);
    assert_eq!(g.brain.task, Task::Attack);
    assert_eq!(g.target, Some(hero));
    assert_eq!(g.attack_reason, AttackReason::Theft);
    assert_eq!(g.ai_mode, AiMode::Combat);
    // Guards report to the enclave; the enclave now knows the player as a thief.
    let e = &sim.enclaves["trelis"];
    assert_eq!(e.known_crime, Crime::Theft);
    assert_eq!(e.theft_count, 1);
    assert!(e.suspect_comment);
    assert_eq!(sim.attitude(guard, hero), Attitude::Angry);
    // CallHelp: the second guard joins with StopThief.
    assert_eq!(sim.actor(guard2).brain.task, Task::Attack);
    assert_eq!(sim.actor(guard2).attack_reason, AttackReason::StopThief);
    let ev = sim.drain_events();
    assert!(ev.iter().any(|e| matches!(
        e.kind,
        EventKind::CrimeMemorized {
            outcome: Memorized::Enclave,
            crime: Crime::Theft,
            ..
        }
    )));
    assert!(
        ev.iter()
            .any(|e| matches!(e.kind, EventKind::HelpCalled { helper, .. } if helper == guard2))
    );

    // The guard runs down the thief and hits him.
    let hp0 = sim.actor(hero).hp;
    let hit = sim.run_until(400, |s| s.actor(hero).hp < hp0);
    assert!(hit, "guard reaches and hits the hero");
}

#[test]
fn nobody_reacts_without_sight_or_with_trivial_items() {
    // Behind a wall (indoors: LOS applies).
    let Scene { mut sim, .. } = scene(2, false);
    sim.world.add_wall(Rect::new(300.0, -2000.0, 350.0, 2000.0));
    assert!(sim.witness_theft("It_Sword_01", false, false).is_empty());

    // Trivial items and items the player dropped never bother anybody [CA 10.2].
    let Scene { mut sim, .. } = scene(2, true);
    for item in [
        "It_Arrow",
        "It_Bolt",
        "It_Bacon",
        "It_Pl_Health_Herb_01",
        "It_Ru_Teleport_Ardea",
    ] {
        assert!(sim.witness_theft(item, false, false).is_empty(), "{item}");
    }
    assert!(sim.witness_theft("It_Sword_01", true, false).is_empty());
    assert!(sim.enclaves.is_empty());

    // Out of the 1000 unit sight range.
    let Scene { mut sim, hero, .. } = scene(2, true);
    sim.world.teleport(hero, Pos::flat(1500.0, 0.0));
    assert!(sim.witness_theft("It_Sword_01", false, false).is_empty());

    // Guard looking the other way.
    let Scene { mut sim, guard, .. } = scene(2, true);
    sim.world.set_facing(guard, std::f32::consts::PI);
    let r = sim.witness_theft("It_Sword_01", false, false);
    assert!(!r.contains(&guard));
}

#[test]
fn civilian_witness_remembers_privately() {
    let Scene { mut sim, hero, .. } = scene(3, true);
    let mut civ = Actor::npc(ActorId(0), "Civ", Species::Human, 5, 5);
    civ.alignment = Alignment::Mid;
    let civ = sim.spawn(civ, Pos::flat(300.0, 0.0));
    sim.world.set_facing(civ, 0.0);
    let reacted = sim.witness_theft("It_Sword_01", false, true);
    assert!(reacted.contains(&civ));
    assert_eq!(sim.actor(civ).last_player_crime, Crime::Theft);
    assert_eq!(sim.attitude(civ, hero), Attitude::Angry);
}

#[test]
fn excuse_calms_or_enrages_the_guard() {
    // Needs Perk_Excuse; succeeds with probability (Theft+1)/130 for Theft 100 [CA 10.5].
    let mut calm = 0;
    let n = 60;
    for seed in 0..n {
        let Scene {
            mut sim,
            hero,
            guard,
            ..
        } = scene(seed, true);
        sim.actor_mut(hero).perks.insert("perk_excuse".into());
        sim.actor_mut(hero).base.theft = 100;
        sim.witness_theft("It_Sword_01", false, false);
        let ok = sim.excuse(guard);
        if ok {
            calm += 1;
            assert_ne!(sim.actor(guard).brain.task, Task::Attack);
            assert_eq!(sim.actor(guard).last_player_crime, Crime::None);
        } else {
            assert_eq!(sim.actor(guard).brain.task, Task::Attack);
        }
    }
    println!(
        "excuse succeeded {calm}/{n} (expected about {})",
        n * 101 / 130
    );
    assert!((35..=58).contains(&calm), "{calm}");
    // Without the perk it never works.
    let Scene { mut sim, guard, .. } = scene(1, true);
    sim.witness_theft("It_Sword_01", false, false);
    assert!(!sim.excuse(guard));
}

#[test]
fn drawing_a_weapon_is_a_warning_not_a_crime() {
    let Scene {
        mut sim,
        hero,
        guard,
        ..
    } = scene(4, true);
    sim.equip(hero, "It_1H_Sword_01").unwrap();
    sim.actor_mut(hero).drawn = true;
    sim.world.teleport(hero, Pos::flat(500.0, 0.0));
    // Within 700, in view: the guard starts ZS_ReactToWeapon.
    sim.run_ticks(10);
    assert_eq!(sim.actor(guard).brain.task, Task::ReactToWeapon);
    // Second warning at 4 s.
    sim.run_until(100, |s| s.actor(guard).brain.warned == 1);
    let t = sim.now_ms();
    assert!((4000..4300).contains(&t), "warned at {t}");
    // Lowering the weapon ends it without a fight: "WiseMove", then the guard keeps an eye on the
    // suspect (`ZS_ObserveSuspect`) until it loses interest and resumes its routine.
    sim.actor_mut(hero).drawn = false;
    sim.run_ticks(10);
    assert_eq!(sim.actor(guard).brain.task, Task::Reaction);
    assert!(sim.run_until(400, |s| s.actor(guard).brain.task == Task::Routine));
    assert!(sim.now_ms() < 15_000, "back to work at {}", sim.now_ms());
    assert_eq!(sim.actor(guard).last_player_crime, Crime::None);
    assert!(
        sim.enclaves.is_empty(),
        "drawing a weapon is never a crime [CA 10.3]"
    );
}

#[test]
fn keeping_the_weapon_drawn_ends_in_a_fight_after_eight_seconds() {
    let Scene {
        mut sim,
        hero,
        guard,
        ..
    } = scene(4, true);
    sim.equip(hero, "It_1H_Sword_01").unwrap();
    sim.actor_mut(hero).drawn = true;
    sim.world.teleport(hero, Pos::flat(500.0, 0.0));
    sim.run_until(300, |s| s.actor(guard).brain.task == Task::Attack);
    let t = sim.now_ms();
    assert_eq!(sim.actor(guard).attack_reason, AttackReason::ReactToWeapon);
    assert!((8000..8400).contains(&t), "attack at {t}");
    // No crime was recorded for the weapon itself.
    assert!(sim.enclaves.is_empty());
}

#[test]
fn murder_before_a_guard_becomes_a_known_crime() {
    let Scene {
        mut sim,
        hero,
        guard,
        ..
    } = scene(5, true);
    sim.equip(hero, "It_1H_Sword_01").unwrap();
    sim.actor_mut(hero).action = Action::Attack;
    let mut v = Actor::npc(ActorId(0), "Victim", Species::Human, 3, 3);
    v.alignment = Alignment::Mid;
    v.base_attitude_to_player = Attitude::Hostile; // so that the defeat is lethal [CA 2.11]
    let vid = sim.spawn(v, Pos::flat(450.0, 0.0));
    sim.actor_mut(vid).hp = 1;
    // The guard sees the killing.
    let out = sim.melee_hit(hero, vid);
    assert!(out.killed(), "{out:?}");
    assert_eq!(sim.enclaves["trelis"].known_crime, Crime::Murder);
    assert_eq!(sim.attitude(guard, hero), Attitude::Hostile);
    assert_eq!(sim.actor(guard).attack_reason, AttackReason::Murder);
}

#[test]
fn knocking_out_a_non_guard_records_the_attack_crime_on_the_victim() {
    let Scene { mut sim, hero, .. } = scene(6, true);
    sim.equip(hero, "It_1H_Sword_01").unwrap();
    sim.actor_mut(hero).action = Action::Attack;
    let mut v = Actor::npc(ActorId(0), "Peasant", Species::Human, 3, 3);
    v.alignment = Alignment::Mid;
    v.base_attitude_to_player = Attitude::Neutral;
    let vid = sim.spawn(v, Pos::flat(450.0, 0.0));
    sim.actor_mut(vid).hp = 1;
    let out = sim.melee_hit(hero, vid);
    assert!(out.defeated(), "{out:?}");
    assert_eq!(sim.actor(vid).ai_mode, AiMode::Down);
    assert_eq!(sim.actor(vid).last_player_crime, Crime::Attack);
    assert!(sim.state.npc("Peasant").unwrap().defeated);
    // Defeat XP is paid once: 5 * round(max(10*3, 50)/5) = 50.
    assert_eq!(sim.state.xp, 50);
    // It wakes after 5 s (defeated by the player) and goes for its attacker [CA 9.4].
    sim.run_until(200, |s| s.actor(vid).ai_mode != AiMode::Down);
    assert!((5000..6200).contains(&sim.now_ms()), "{}", sim.now_ms());
    assert_eq!(
        sim.actor(vid).hp,
        1 + 5 * 10,
        "+10 HP per second while unconscious"
    );
}
