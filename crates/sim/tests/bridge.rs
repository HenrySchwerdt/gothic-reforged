//! Integration with `gothic-gameplay` (quest hooks, forwarded effects) and the remaining
//! simulation systems (trade, falling, party, routines, ranged, magic, variants).
mod common;
use common::*;
use gothic_formats::{quest::Quest, stringtable::StringTable};
use gothic_gameplay::GameDatabase;
use gothic_gameplay::model::QuestStatus;
use gothic_gameplay::script::Output;
use gothic_sim::actor::Actor;
use gothic_sim::brain::Task;
use gothic_sim::config::SimConfig;
use gothic_sim::data::NpcRecord;
use gothic_sim::economy::TradeSide;
use gothic_sim::item::ItemDef;
use gothic_sim::routine::{AnchorKind, GameClock, RoutineTask, StaticRoutines};
use gothic_sim::sim::{EventKind, Sim, SpellSpec};
use gothic_sim::types::*;
use gothic_sim::world::{Grid2dWorld, Pos, WorldQuery};
use std::sync::Arc;

fn quest(name: &str, ty: u32, xp: u32, delivery: &str, amounts: &str) -> Quest {
    let text = format!(
        "[Quest]\r\nName={name}\r\nType={ty}\r\nFinishedQuests=\r\nLogTopic=QUEST_{name}\r\nExperiencePoints={xp}\r\nDeliveryEntities={delivery}\r\nDeliveryAmounts={amounts}\r\nDeliveryCounter\r\nDestinationEntity=\r\n"
    );
    Quest::parse(text.as_bytes()).unwrap()
}

fn strings() -> StringTable {
    let text = "[LocAdmin_Languages]\r\nLanguages=English;German\r\nCurrentLanguage=English\r\nCurrentRevision=0\r\n[LocAdmin_Strings]\r\nQUEST_X=Quest;;Quest;\r\n[LocAdmin_Revisions]\r\n";
    let mut b = vec![0xFF, 0xFE];
    for u in text.encode_utf16() {
        b.extend(u.to_le_bytes());
    }
    StringTable::parse(&b).unwrap()
}

fn database() -> Arc<GameDatabase> {
    let quests = vec![
        quest("KillOrc", 2, 20, "Orc_Target", "1"),
        quest("BeatGuard", 3, 0, "Guard_A", "1"),
    ];
    Arc::new(GameDatabase::from_parts(quests, vec![], strings(), "English").unwrap())
}

fn sim_with(cfg: SimConfig) -> Sim<Grid2dWorld> {
    let mut sim = Sim::new(cfg, open_world(), synthetic_items());
    sim.state.skills.insert("perk_1h_1".into());
    sim
}

fn armed_hero(sim: &mut Sim<Grid2dWorld>, at: Pos) -> ActorId {
    let hero = sim.spawn_hero(at);
    sim.equip(hero, "It_1H_Sword_01").unwrap();
    sim.actor_mut(hero).action = Action::Attack;
    hero
}

#[test]
fn killing_the_quest_target_completes_the_kill_quest_and_pays_xp() {
    let mut sim = sim_with(SimConfig::default()).with_database(database());
    sim.player_alignment = Alignment::Reb;
    sim.state.set_quest_status("KillOrc", QuestStatus::Running);
    let hero = armed_hero(&mut sim, Pos::flat(0.0, 0.0));
    let mut orc = Actor::npc(ActorId(0), "Orc_Target", Species::Orc, 10, 10);
    orc.alignment = Alignment::Orc;
    let oid = sim.spawn(orc, Pos::flat(100.0, 0.0));
    sim.world.set_facing(oid, std::f32::consts::PI);
    sim.actor_mut(oid).hp = 10;
    let out = sim.melee_hit(hero, oid);
    assert!(
        out.killed(),
        "{out:?} att {:?} / {:?}",
        sim.attitude(oid, hero),
        sim.attitude(hero, oid)
    );
    assert_eq!(sim.state.quest_status("KillOrc"), QuestStatus::Success);
    assert!(sim.state.npc("orc_target").unwrap().dead);
    // Kill XP: max(10*10, 50) = 100; quest reward 20 * 5 = 100.
    assert_eq!(sim.state.xp, 200);
    let ev = sim.drain_events();
    assert!(ev.iter().any(|e| matches!(&e.kind, EventKind::Quest { quest, to: QuestStatus::Success, .. } if quest == "KillOrc")));
    assert!(
        ev.iter()
            .any(|e| matches!(e.kind, EventKind::Xp { amount: 100, .. }))
    );
    // A second hit on the corpse does nothing and pays nothing again.
    let again = sim.melee_hit(hero, oid);
    assert!(again.ignored.is_some());
    assert_eq!(sim.state.xp, 200);
}

#[test]
fn level_up_from_kill_xp_raises_hero_stats() {
    let mut sim = sim_with(SimConfig::default());
    sim.player_alignment = Alignment::Reb;
    sim.state.level = 0;
    sim.state.xp = 450;
    let hero = armed_hero(&mut sim, Pos::flat(0.0, 0.0));
    let mut orc = Actor::npc(ActorId(0), "Orc", Species::Orc, 10, 10);
    orc.alignment = Alignment::Orc;
    let oid = sim.spawn(orc, Pos::flat(100.0, 0.0));
    sim.actor_mut(oid).hp = 1;
    sim.melee_hit(hero, oid);
    // 450 + 100 >= XPNext(0) = 500 -> level 1, +10 learning points [GAME_RULES 4.1].
    assert_eq!(sim.state.level, 1);
    assert_eq!(sim.state.learning_points, 10);
    assert_eq!(sim.actor(hero).level, 1);
}

#[test]
fn defeating_a_guard_counts_for_a_defeat_quest() {
    let mut sim = sim_with(SimConfig::default()).with_database(database());
    sim.player_alignment = Alignment::Mid;
    sim.state
        .set_quest_status("BeatGuard", QuestStatus::Running);
    let hero = armed_hero(&mut sim, Pos::flat(0.0, 0.0));
    let mut g = Actor::npc(ActorId(0), "Guard_A", Species::Human, 3, 3);
    g.npc_type = NpcType::Enclave;
    g.alignment = Alignment::Mid;
    g.enclave = Some("trelis".into());
    let gid = sim.spawn(g, Pos::flat(100.0, 0.0));
    sim.actor_mut(gid).hp = 1;
    let out = sim.melee_hit(hero, gid);
    assert!(
        out.defeated(),
        "friendly guard is knocked out, not killed: {out:?}"
    );
    assert_eq!(sim.state.quest_status("BeatGuard"), QuestStatus::Success);
    assert!(sim.state.npc("guard_a").unwrap().defeated);
    // Guards are not marked with the Attack crime [CA 10.5].
    assert_eq!(sim.actor(gid).last_player_crime, Crime::None);
}

#[test]
fn forwarded_effects_drive_the_world() {
    let mut sim = sim_with(SimConfig::default());
    sim.player_alignment = Alignment::Reb;
    let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
    let mut a = Actor::npc(ActorId(0), "Wulfgar", Species::Human, 10, 10);
    a.alignment = Alignment::Mid;
    let wulf = sim.spawn(a, Pos::flat(3000.0, 0.0));
    let tmpl = NpcRecord {
        name: "Wolf_Template".into(),
        species: "Wolf".into(),
        level: 5,
        level_max: 5,
        ..NpcRecord::default()
    };
    sim.npc_templates.insert("wolf_template".into(), tmpl);
    sim.dialog_partner = Some(wulf);
    let fx = |command: &str, e1: &str, e2: &str, a1: &str, a2: &str| Output::Effect {
        command: command.into(),
        entity1: e1.into(),
        entity2: e2.into(),
        arg1: a1.into(),
        arg2: a2.into(),
    };
    sim.apply_outputs(&[
        fx("MoveTo", "npc", "player", "", ""),
        fx("Spawn", "", "", "Wolf_Template", "wpos 500 0 500"),
        fx("DoTrade", "player", "npc", "", ""),
        fx("JoinPlayer", "npc", "", "true", ""),
        fx("Attack", "npc", "player", "Duel", ""),
        fx("Bogus", "", "", "", ""),
    ]);
    // MoveTo: the dialog partner teleported next to the hero.
    assert_eq!(sim.world_pos(wulf), Pos::flat(0.0, 0.0));
    let wolf = sim.find("Wolf_Template").expect("spawned");
    assert_eq!(sim.world_pos(wolf), Pos::new(500.0, 0.0, 500.0));
    assert_eq!(
        sim.actor(wolf).hp,
        100,
        "spawned NPCs start at full HP from their level"
    );
    assert!(sim.state.party.contains("wulfgar"));
    assert_eq!(sim.actor(wulf).party_leader, Some(hero));
    // Attack with reason Duel overrides the follow task.
    assert_eq!(sim.actor(wulf).brain.task, Task::Attack);
    let ev = sim.drain_events();
    assert!(
        ev.iter()
            .any(|e| matches!(e.kind, EventKind::TradeOpened { npc } if npc == wulf))
    );
    assert!(
        ev.iter()
            .any(|e| matches!(&e.kind, EventKind::Unhandled { command } if command == "bogus"))
    );
}

#[test]
fn trade_prices_and_transactions() {
    let mut sim = sim_with(SimConfig::default());
    let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
    let mut smith = Actor::npc(ActorId(0), "Smith", Species::Human, 10, 10);
    smith.inventory.insert("it_1h_sword_01", 2);
    smith.base_attitude_to_player = Attitude::Neutral; // no discount for friends
    let smith = sim.spawn(smith, Pos::flat(100.0, 0.0));
    sim.state.add_item("it_gold", 500);
    // Item value 50: buying costs 100 percent, selling pays 25 percent (12 after truncation).
    assert_eq!(
        sim.trade_price(smith, "It_1H_Sword_01", TradeSide::PlayerBuys),
        Some(50)
    );
    assert_eq!(
        sim.trade_price(smith, "It_1H_Sword_01", TradeSide::PlayerSells),
        Some(12)
    );
    assert_eq!(
        sim.trade(smith, "It_1H_Sword_01", TradeSide::PlayerBuys),
        Ok(50)
    );
    assert_eq!(sim.state.gold(), 450);
    assert_eq!(sim.state.item_count("it_1h_sword_01"), 1);
    assert_eq!(sim.actor(smith).inventory["it_1h_sword_01"], 1);
    assert_eq!(
        sim.trade(smith, "It_1H_Sword_01", TradeSide::PlayerSells),
        Ok(12)
    );
    assert_eq!(sim.state.gold(), 462);
    assert!(
        sim.trade(smith, "It_1H_Sword_01", TradeSide::PlayerSells)
            .is_err()
    );
    // A friendly NPC gives the same 10 percent as the Barter perk [CA 8.1].
    sim.actor_mut(smith).base_attitude_to_player = Attitude::Friendly;
    assert_eq!(
        sim.trade_price(smith, "It_1H_Sword_01", TradeSide::PlayerBuys),
        Some(45)
    );
    sim.actor_mut(smith).base_attitude_to_player = Attitude::Neutral;
    // Barter perk: buying -10 percent.
    sim.actor_mut(hero).perks.insert("perk_barter".into());
    assert_eq!(
        sim.trade_price(smith, "It_1H_Sword_01", TradeSide::PlayerBuys),
        Some(45)
    );
    // Not enough gold.
    sim.state.take_item("it_gold", 462);
    assert!(
        sim.trade(smith, "It_1H_Sword_01", TradeSide::PlayerBuys)
            .is_err()
    );
}

#[test]
fn falling_hurts_and_can_kill() {
    let mut sim = sim_with(SimConfig::default());
    let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
    assert_eq!(sim.land(hero, 0.5), 0);
    assert_eq!(sim.land(hero, 1.5), 57);
    assert_eq!(sim.actor(hero).hp, 200 - 57);
    sim.actor_mut(hero).perks.insert("perk_acrobatic".into());
    assert_eq!(sim.land(hero, 2.0), 96, "Acrobatic halves 193");
    let before = sim.actor(hero).hp;
    assert_eq!(before, 200 - 57 - 96);
    sim.actor_mut(hero).perks.clear();
    sim.land(hero, 2.0);
    assert_eq!(sim.actor(hero).ai_mode, AiMode::Dead);
    assert!(sim.state.game_over);
    // Immortal NPCs survive any fall.
    let mut n = Actor::npc(ActorId(0), "Immortal", Species::Human, 5, 5);
    n.immortal = true;
    let n = sim.spawn(n, Pos::flat(0.0, 0.0));
    sim.land(n, 5.0);
    assert!(sim.actor(n).is_alive() && sim.actor(n).hp >= 1);
}

#[test]
fn regeneration_ticks_once_per_second() {
    let mut sim = sim_with(SimConfig::default());
    let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
    sim.actor_mut(hero).hp = 100;
    sim.actor_mut(hero).stamina = 0;
    sim.run_ticks(60); // 3 s
    assert_eq!(
        sim.actor(hero).hp,
        100,
        "no passive HP regeneration without the perk [CA 5.2]"
    );
    assert!(
        sim.actor(hero).stamina >= 20 && sim.actor(hero).stamina <= 40,
        "{}",
        sim.actor(hero).stamina
    );
    // Variant: passive regeneration on.
    let mut cfg = SimConfig::default();
    cfg.variants.passive_hp_regen = Some(2);
    let mut sim = sim_with(cfg);
    let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
    sim.actor_mut(hero).hp = 100;
    sim.run_ticks(60);
    assert!(sim.actor(hero).hp >= 104);
}

#[test]
fn party_allows_one_active_companion_and_follows() {
    let mut sim = sim_with(SimConfig::default());
    let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
    let mk = |name: &str| {
        let mut a = Actor::npc(ActorId(0), name, Species::Human, 10, 10);
        a.alignment = Alignment::Mid;
        a
    };
    let a = sim.spawn(mk("Diego"), Pos::flat(100.0, 0.0));
    let b = sim.spawn(mk("Gorn"), Pos::flat(-100.0, 0.0));
    assert!(sim.join_party(a));
    assert!(!sim.join_party(b), "an active companion blocks [CA 11.4]");
    // A waiting companion is dismissed instead.
    sim.wait_party(a);
    assert!(sim.join_party(b));
    assert_eq!(sim.actor(a).party_leader, None);
    // The follower keeps (rank + 3) * 150 = 450 from the hero.
    sim.world.teleport(hero, Pos::flat(4000.0, 0.0));
    sim.run_ticks(300);
    let d = sim.dist(b, hero);
    assert!((400.0..=460.0).contains(&d), "{d}");
    // Swimming makes it wait.
    sim.actor_mut(hero).ani_state = AniState::Swim;
    sim.run_ticks(2);
    assert_eq!(sim.actor(b).brain.task, Task::WaitForPlayer);
}

#[test]
fn routine_follows_the_time_of_day() {
    let mut sim = sim_with(SimConfig::default());
    let mut r = StaticRoutines::default();
    r.add(
        "Smith",
        AnchorKind::Working,
        Pos::flat(1000.0, 0.0),
        "Anvil",
    );
    r.add("Smith", AnchorKind::Sleeping, Pos::flat(0.0, 500.0), "Bed");
    r.add(
        "Smith",
        AnchorKind::Relaxing,
        Pos::flat(0.0, 900.0),
        "Stool",
    );
    sim.resolver = Some(Box::new(r));
    let smith = sim.spawn(
        Actor::npc(ActorId(0), "Smith", Species::Human, 5, 5),
        Pos::flat(0.0, 0.0),
    );
    sim.run_ticks(150); // 7.5 s at 08:00 -> work
    assert_eq!(
        sim.actor(smith).brain.routine_task,
        Some(RoutineTask::Smith)
    );
    assert!(sim.world_pos(smith).dist_xz(Pos::flat(1000.0, 0.0)) < 60.0);
    sim.clock = GameClock::at(1, 3); // 03:00 -> sleep
    sim.run_ticks(150);
    assert_eq!(
        sim.actor(smith).brain.routine_task,
        Some(RoutineTask::Sleep)
    );
    assert!(sim.world_pos(smith).dist_xz(Pos::flat(0.0, 500.0)) < 60.0);
    sim.clock = GameClock::at(1, 21); // 21:00 -> relax
    sim.run_ticks(150);
    assert_eq!(
        sim.actor(smith).brain.routine_task,
        Some(RoutineTask::HangAround)
    );
    assert_eq!(sim.state.hours, sim.clock.total_hours());
}

#[test]
fn actors_outside_the_processing_range_do_not_think() {
    let mut sim = sim_with(SimConfig::default());
    let mut r = StaticRoutines::default();
    r.add(
        "Smith",
        AnchorKind::Working,
        Pos::flat(1000.0, 0.0),
        "Anvil",
    );
    sim.resolver = Some(Box::new(r));
    let smith = sim.spawn(
        Actor::npc(ActorId(0), "Smith", Species::Human, 5, 5),
        Pos::flat(0.0, 0.0),
    );
    // An empty range: nobody is processed, the smith stays where he is.
    sim.processing = Some(Default::default());
    sim.run_ticks(150);
    assert!(sim.world_pos(smith).dist_xz(Pos::flat(0.0, 0.0)) < 1.0);
    assert!(!sim.is_processed(smith));
    // Once inside, the routine runs.
    sim.processing = Some([smith].into_iter().collect());
    sim.run_ticks(150);
    assert!(sim.world_pos(smith).dist_xz(Pos::flat(1000.0, 0.0)) < 60.0);
    // Without a range everyone runs again.
    sim.processing = None;
    assert!(sim.is_processed(smith));
}

#[test]
fn bow_shots_use_draw_time_dex_and_protection() {
    let cfg = SimConfig {
        ranged_hit_pct: 100,
        ..SimConfig::default()
    };
    let mut sim = sim_with(cfg);
    let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
    sim.equip(hero, "It_Bow_Test").unwrap();
    let mut wolf = Actor::npc(ActorId(0), "Wolf", Species::Wolf, 10, 10);
    wolf.refresh(&sim.cfg);
    let wolf = sim.spawn(wolf, Pos::flat(1500.0, 0.0));
    // Full draw: b = 40, DEX/2 = 50 -> 90; wolf protection 15 percent: 90 - 13 = 77.
    let out = sim.shoot(hero, wolf, 2.0, "It_Arrow").expect("hit");
    assert_eq!(out.damage, 77);
    assert_eq!(sim.actor(wolf).hp, 200 - 77);
    // A tapped shot (0.2 multiplier): b = 8 -> 58 - 8 = 50.
    let out = sim.shoot(hero, wolf, 0.0, "It_Arrow").expect("hit");
    assert_eq!(out.damage, 58 - 8);
    assert_eq!(out.strength_class, 0, "v = 0 below 0.6 multiplier");
    // Missed shots are reported as whiffs.
    sim.cfg.ranged_hit_pct = 0;
    assert!(sim.shoot(hero, wolf, 2.0, "It_Arrow").is_none());
}

#[test]
fn spells_cost_mana_and_scale_with_intelligence() {
    let mut sim = sim_with(SimConfig::default());
    let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
    sim.actor_mut(hero).base.int = 200;
    let mut orc = Actor::npc(ActorId(0), "Orc", Species::Orc, 10, 30);
    orc.alignment = Alignment::Orc;
    let orc = sim.spawn(orc, Pos::flat(800.0, 0.0));
    let fireball = SpellSpec {
        name: "Spl_Fireball".into(),
        damage_type: DamageType::Fire,
        damage: 60,
        min_mana: 10,
        max_mana: 30,
        deity: gothic_sim::combat::Deity::Innos,
    };
    // [CA 2.5] b = 2*min(60,100) = 120, + b*INT/100 = 240 -> 360; fire protection 45 -> 198.
    let out = sim.cast(hero, orc, &fireball, false).unwrap();
    assert_eq!(out.damage, 198);
    assert_eq!(sim.actor(hero).mana, 90);
    assert_ne!(sim.actor(orc).status & status::BURNING, 0);
    // Not enough mana: nothing happens.
    sim.actor_mut(hero).mana = 5;
    assert!(sim.cast(hero, orc, &fireball, true).is_none());
}

#[test]
fn flee_variant_is_off_by_default() {
    let run = |variant: bool| {
        let mut cfg = SimConfig::default();
        if variant {
            cfg.variants.flee_below_hp_pct = Some(20);
        }
        let mut sim = sim_with(cfg);
        sim.player_alignment = Alignment::Reb;
        let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
        let mut orc = Actor::npc(ActorId(0), "Orc", Species::Orc, 10, 30);
        orc.alignment = Alignment::Orc;
        let o = sim.spawn(orc, Pos::flat(150.0, 0.0));
        sim.actor_mut(o).hp = 50; // below 20 percent of 600
        sim.start_attack(o, hero, AttackReason::Enemy);
        sim.run_ticks(5);
        sim.actor(o).brain.task
    };
    assert_eq!(
        run(false),
        Task::Attack,
        "WantToFlee is a stub in the original [CA 9.1]"
    );
    assert_eq!(run(true), Task::Flee);
}

#[test]
fn drinking_the_potion_rule_triggers_below_forty_percent() {
    let mut sim = sim_with(SimConfig::default());
    sim.player_alignment = Alignment::Reb;
    let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
    let mut orc = Actor::npc(ActorId(0), "Orc", Species::Orc, 10, 30);
    orc.alignment = Alignment::Orc;
    orc.inventory.insert("it_potion_health", 1);
    let o = sim.spawn(orc, Pos::flat(1000.0, 0.0));
    sim.actor_mut(o).hp = 200; // 33 percent of 600
    sim.start_attack(o, hero, AttackReason::Enemy);
    sim.run_ticks(3);
    assert_eq!(sim.actor(o).hp, 250, "It_Potion_Health heals 50");
    assert!(
        sim.drain_events()
            .iter()
            .any(|e| matches!(e.kind, EventKind::Potion { healed: 50, .. }))
    );
}

#[test]
fn equipment_requirements_are_enforced_for_the_player() {
    let mut sim = Sim::new(SimConfig::default(), open_world(), synthetic_items());
    let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
    // Perk_1H_1 is missing.
    assert!(sim.equip(hero, "It_1H_Sword_01").is_err());
    sim.state.skills.insert("perk_1h_1".into());
    // Re-sync the skills into the actor (as after learning).
    sim.refresh_hero();
    assert!(sim.equip(hero, "It_1H_Sword_01").is_ok());
    // Too weak.
    sim.actor_mut(hero).base.str = 50;
    let mut other = ItemDef::weapon("It_Heavy", "2H", "Blade", 100);
    other.requirements.push(gothic_sim::item::Requirement {
        tag: "STR".into(),
        op: 0,
        value: 120,
    });
    sim.items.insert(other);
    assert!(sim.equip(hero, "It_Heavy").is_err());
}
