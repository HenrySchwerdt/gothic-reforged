//! The hero casts, shoots and summons through the ported magic and ranged states.
mod common;
use common::*;
use gothic_script::enums as se;
use gothic_script::magic::api::MagicEvent;
use gothic_script::routine::state::State;
use gothic_sim::actor::Actor;
use gothic_sim::config::SimConfig;
use gothic_sim::item::{DamageDef, ItemDef};
use gothic_sim::sim::{Event, EventKind, Sim};
use gothic_sim::types::*;
use gothic_sim::world::{Grid2dWorld, Pos};

/// The spell templates in the export format (`Spl_Fireball`: 15 to 30 mana, a 50 point fire
/// missile; `Spl_SummonSkeleton`: 60 mana).
const SPELLS: &str = r#"[
 {"name":"Spl_Fireball","spell_type":0,"deity":2,"min_mana":15,"max_mana":30,"target":1,"cast_type":1,
  "func_on_cast":"","func_on_hit":"","anim_cast":"","anim_power":"",
  "spawn":{"template":"Mis_Fireball","damage":{"DamageType":4,"DamageAmount":50,"DamageManaMultiplier":2.0,"DamageHitMultiplier":1.0},
           "projectile":{"ShootVelocity":100.0}}},
 {"name":"Spl_SummonSkeleton","spell_type":2,"deity":1,"min_mana":60,"max_mana":60,"target":0,"cast_type":2,
  "func_on_cast":"MagicInstant","func_on_hit":"MagicSummonSkeleton","anim_cast":"Summon","anim_power":"","spawn":null}
]"#;

fn spell_items(mut db: gothic_sim::item::ItemDb) -> gothic_sim::item::ItemDb {
    let cast = |name: &str, spell: &str, category: &str, permanent: bool| ItemDef {
        name: name.into(),
        category: category.into(),
        use_type: Some("Cast".into()),
        spell: Some(spell.into()),
        permanent,
        ..ItemDef::default()
    };
    db.insert(cast("It_Spell_Fireball", "Spl_Fireball", "Spellbook", true));
    db.insert(cast(
        "It_Scroll_Fireball",
        "Spl_Fireball",
        "Artefact",
        false,
    ));
    db.insert(cast(
        "It_Spell_SummonSkeleton",
        "Spl_SummonSkeleton",
        "Spellbook",
        true,
    ));
    db
}

fn world(seed: u64) -> (Sim<Grid2dWorld>, ActorId, ActorId) {
    let cfg = SimConfig {
        seed,
        ..SimConfig::default()
    };
    let mut sim = Sim::new(cfg, open_world(), spell_items(synthetic_items()));
    sim.load_spells(SPELLS).expect("spells");
    sim.player_alignment = Alignment::Reb;
    let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
    let mut orc = Actor::npc(ActorId(0), "Orc_Raider", Species::Orc, 10, 10);
    orc.alignment = Alignment::Orc;
    orc.base_attitude_to_player = Attitude::Hostile;
    orc.refresh(&SimConfig::default());
    orc.ai_controlled = false;
    let orc = sim.spawn(orc, Pos::flat(800.0, 0.0));
    (sim, hero, orc)
}

fn settle(sim: &mut Sim<Grid2dWorld>, max: u64) {
    assert!(
        sim.run_until(max, |s| s.player_idle()),
        "the hero never settled: {:?}",
        sim.script.npc(sim.hero.expect("hero")).state
    );
}

fn events_of(sim: &mut Sim<Grid2dWorld>) -> Vec<Event> {
    sim.drain_events()
}

#[test]
fn the_hero_casts_a_fireball_at_an_orc() {
    let (mut sim, hero, orc) = world(1);
    sim.equip(hero, "It_Spell_Fireball").expect("rune");
    assert!(sim.player_select_spell("It_Spell_Fireball"));
    settle(&mut sim, 600);
    sim.drain_events();
    let hp0 = sim.actor(orc).hp;
    let mana0 = sim.actor(hero).mana;
    assert_eq!(sim.player_cast(Some(orc), false), Ok(State::PsMagicCast));
    assert!(sim.run_until(900, |s| s.actor(orc).hp < hp0));
    let ev = events_of(&mut sim);
    assert!(ev.iter().any(|e| matches!(&e.kind, EventKind::Projectile { spell: true, template, .. } if template == "Mis_Fireball")));
    assert!(ev.iter().any(|e| matches!(
        &e.kind,
        EventKind::Hit { attacker, victim, ignored: None, .. } if *attacker == hero && *victim == orc
    )));
    assert_eq!(
        sim.actor(hero).mana,
        mana0 - 15,
        "the load phase pays the minimum"
    );
    assert!(sim.actor(orc).hp < hp0);
    assert!(ev.iter().any(|e| matches!(&e.kind, EventKind::Magic(MagicEvent::Cast { spell, .. }) if spell == "Spl_Fireball")));
    // The rune stays: a second cast works and costs mana again.
    sim.run_until(600, |s| s.player_idle());
    assert!(sim.player_cast(Some(orc), false).is_ok());
}

#[test]
fn a_power_cast_costs_the_maximum_mana() {
    let (mut sim, hero, orc) = world(1);
    sim.equip(hero, "It_Spell_Fireball").expect("rune");
    sim.player_select_spell("It_Spell_Fireball");
    settle(&mut sim, 600);
    let hp0 = sim.actor(orc).hp;
    let mana0 = sim.actor(hero).mana;
    assert_eq!(
        sim.player_cast(Some(orc), true),
        Ok(State::PsMagicPowerCast)
    );
    assert!(sim.run_until(900, |s| s.actor(orc).hp < hp0));
    assert_eq!(
        sim.actor(hero).mana,
        mana0 - 30,
        "min 15 at the load, 15 more for the charge"
    );
    // The power cast flies as a path-style Missile projectile, and AssessHit (Script_Game.dll
    // S:2b580, the check at S:2c0f5, COMBAT_AI_RULES 2.5 [V]) doubles a spell projectile whose
    // PathStyle == Missile (3); the mana paid only matters when it is 0 (S:2c096 attribute bonus).
    let power_loss = hp0 - sim.actor(orc).hp;
    let (mut plain, hero, orc) = world(1);
    plain.equip(hero, "It_Spell_Fireball").expect("rune");
    plain.player_select_spell("It_Spell_Fireball");
    settle(&mut plain, 600);
    plain.player_cast(Some(orc), false).expect("cast");
    plain.run_until(900, |s| s.actor(orc).hp < hp0);
    assert_eq!(hp0 - plain.actor(orc).hp, power_loss / 2);
}

#[test]
fn a_scroll_is_used_up_by_the_cast() {
    let (mut sim, hero, orc) = world(2);
    sim.actor_mut(hero)
        .inventory
        .insert("It_Scroll_Fireball", 1);
    sim.equip(hero, "It_Scroll_Fireball").expect("scroll");
    assert!(sim.player_select_spell("It_Scroll_Fireball"));
    settle(&mut sim, 600);
    sim.player_cast(Some(orc), false).expect("cast");
    sim.run_until(900, |s| s.actor(orc).hp < 1_000_000 && s.player_idle());
    sim.run_ticks(200);
    assert!(
        sim.actor(hero).equipment.right.is_none(),
        "the scroll is gone"
    );
    assert_eq!(sim.actor(hero).inventory.count("It_Scroll_Fireball"), 0);
}

#[test]
fn the_hero_does_not_cast_without_mana() {
    let (mut sim, hero, orc) = world(3);
    sim.equip(hero, "It_Spell_Fireball").expect("rune");
    sim.player_select_spell("It_Spell_Fireball");
    settle(&mut sim, 600);
    sim.actor_mut(hero).mana = 5;
    sim.drain_events();
    sim.player_cast(Some(orc), false).expect("state");
    sim.run_ticks(300);
    let ev = events_of(&mut sim);
    assert!(
        !ev.iter()
            .any(|e| matches!(&e.kind, EventKind::Projectile { .. }))
    );
    assert_eq!(sim.actor(hero).mana, 5);
    assert!(ev.iter().any(|e| matches!(&e.kind, EventKind::Magic(MagicEvent::Effect { name, .. }) if name == "eff_magic_fizzle_01")));
}

#[test]
fn a_summon_spell_adds_a_skeleton_to_the_hero_party() {
    let (mut sim, hero, _orc) = world(4);
    sim.equip(hero, "It_Spell_SummonSkeleton").expect("rune");
    assert!(sim.player_select_spell("It_Spell_SummonSkeleton"));
    settle(&mut sim, 600);
    let n = sim.actors.len();
    assert_eq!(sim.player_cast(None, false), Ok(State::PsMagicSummon));
    assert!(sim.run_until(900, |s| s.actors.len() > n));
    let ev = events_of(&mut sim);
    let skeleton = sim.actors.last().expect("summon").id;
    assert!(ev.iter().any(|e| matches!(&e.kind, EventKind::Magic(MagicEvent::Summoned { template, .. }) if template == "Skeleton_Sword")));
    assert_eq!(sim.actor(skeleton).party_leader, Some(hero));
    assert!(sim.actor(skeleton).summoned);
    assert_eq!(sim.actor(hero).mana, 100 - 60);
}

fn archer_world(seed: u64) -> (Sim<Grid2dWorld>, ActorId, ActorId) {
    let (mut sim, hero, orc) = world(seed);
    sim.equip(hero, "It_Bow_Test").expect("bow");
    sim.equip(hero, "It_Arrow").expect("arrows");
    sim.actor_mut(hero).inventory.insert("It_Arrow", 5);
    assert!(sim.player_ready_ranged());
    settle(&mut sim, 600);
    (sim, hero, orc)
}

/// Draw for `ticks` steps, shoot, and return how much damage the orc took.
fn shoot_after(ticks: u64, seed: u64) -> (i32, Vec<Event>) {
    let (mut sim, _hero, orc) = archer_world(seed);
    sim.player_aim(true);
    sim.run_ticks(ticks);
    let hp0 = sim.actor(orc).hp;
    assert!(sim.player_release(Some(orc)));
    sim.run_ticks(200);
    let ev = sim.drain_events();
    (hp0 - sim.actor(orc).hp, ev)
}

#[test]
fn the_hero_shoots_arrows_and_the_draw_sets_the_damage() {
    let mut full = 0;
    let mut quick = 0;
    let mut shots = 0;
    for seed in 0..20 {
        let (a, ev_a) = shoot_after(90, seed);
        let (b, _) = shoot_after(3, seed);
        assert!(ev_a.iter().any(|e| matches!(&e.kind, EventKind::Projectile { spell: false, template, .. } if template == "It_Arrow")));
        full += a;
        quick += b;
        shots += 1;
    }
    assert!(full > 0, "arrows hit");
    assert!(
        full * 2 > quick * 3,
        "a drawn bow hits much harder: {full} against {quick} over {shots} shots"
    );
}

#[test]
fn shooting_uses_ammunition_and_the_bow_is_ready_again() {
    let (mut sim, hero, orc) = archer_world(7);
    sim.player_aim(true);
    sim.run_ticks(80);
    assert!(sim.player_release(Some(orc)));
    sim.run_ticks(200);
    assert_eq!(sim.actor(hero).inventory.get("It_Arrow").copied(), Some(4));
    assert!(sim.player_idle(), "back to walking with the bow");
    let ev = events_of(&mut sim);
    assert_eq!(
        ev.iter()
            .filter(|e| matches!(&e.kind, EventKind::Projectile { .. }))
            .count(),
        1
    );
}

#[test]
fn an_npc_mage_casts_fireballs_at_the_hero() {
    let (mut sim, hero, orc) = world(5);
    let mut cfg_actor = sim.actor(orc).clone();
    cfg_actor.equipment.equip(
        spell_items(synthetic_items())
            .get("It_Spell_Fireball")
            .cloned()
            .expect("rune"),
    );
    cfg_actor.drawn = true;
    cfg_actor.ai_controlled = true;
    cfg_actor.name = "Orc_Shaman".into();
    *sim.actor_mut(orc) = cfg_actor;
    let hp0 = sim.actor(hero).hp;
    sim.start_attack(orc, hero, AttackReason::Enemy);
    let hit = sim.run_until(3000, |s| s.actor(hero).hp < hp0);
    let ev = events_of(&mut sim);
    assert!(
        ev.iter()
            .any(|e| matches!(&e.kind, EventKind::Projectile { spell: true, .. })),
        "the shaman launched a spell missile"
    );
    assert!(hit, "the fireball reached the hero");
}

#[test]
fn the_real_spell_export_matches_the_templates() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/spells.json");
    let Ok(json) = std::fs::read_to_string(path) else {
        eprintln!("no .local/spells.json: skipped");
        return;
    };
    let spells = gothic_sim::script_magic::parse_spells(&json).expect("parse");
    assert!(spells.len() > 90);
    let fireball = spells
        .iter()
        .find(|s| s.name == "Spl_Fireball")
        .expect("fireball");
    assert_eq!((fireball.min_mana, fireball.max_mana), (15, 30));
    assert_eq!(fireball.deity, se::SpellDeity::Innos);
    let d = fireball.damage.expect("damage");
    assert_eq!((d.damage_type, d.amount), (se::DamageType::Fire, 50));
    let _ = DamageDef::new("Fire", 1);
}

#[test]
fn the_bow_hangs_on_the_back_when_the_hands_are_emptied() {
    // The real item export (skipped where it is not available).
    let Some(text) = [
        ".local/items.json",
        "../../.local/items.json",
        "C:/Users/PowerTower/dev/gothic-reforged/.local/items.json",
    ]
    .iter()
    .find_map(|p| std::fs::read_to_string(p).ok()) else {
        return;
    };
    let items = gothic_sim::item::ItemDb::from_json(&text).expect("items");
    let mut a =
        gothic_sim::actor::Actor::npc(ActorId(0), "Hero", gothic_sim::types::Species::Human, 1, 1);
    let bow = items.get("It_Bow_Hunting").cloned().expect("bow");
    let arrow = items.get("Arrow").cloned().expect("arrow");
    a.inventory.insert("Arrow", 20);
    a.wear(&bow, &items);
    a.wear(&arrow, &items);
    a.hold(None, Some(&bow), &items);
    a.drawn = true;
    a.wear(&bow, &items);
    a.hold(None, Some(&bow), &items);
    assert_eq!(
        a.equipment.left.as_ref().map(|d| d.name.as_str()),
        Some("It_Bow_Hunting")
    );
    a.hold(None, None, &items);
    let stacks: Vec<_> = a
        .inventory
        .stacks()
        .iter()
        .map(|s| (s.template.clone(), s.slot, s.use_type(), s.category()))
        .collect();
    assert_eq!(
        a.equipment.left.as_ref().map(|d| d.name.as_str()),
        Some("It_Bow_Hunting"),
        "{stacks:?}"
    );
}
/// A transformation potion: drunk through `PS_MagicPotion`, the potion's spell turns the hero into
/// the animal of its `Spawn` template (`MagicTransform`, S:57880) and `TransformBack` ends it.
#[test]
fn a_transformation_potion_turns_the_hero_into_an_animal() {
    let spells = r#"[
 {"name":"Spl_Transform_Wolf","spell_type":1,"deity":3,"min_mana":30,"max_mana":0,"target":0,"cast_type":2,
  "func_on_cast":"MagicInstant","func_on_hit":"MagicTransform","anim_cast":"","anim_power":"",
  "spawn":{"template":"Wolf","guid":"53a0542c0862174eab1c370f107d80ee00000000"}}
]"#;
    let mut db = synthetic_items();
    db.insert(ItemDef {
        name: "It_Potion_Wolf".into(),
        category: "Alchemy".into(),
        use_type: Some("Potion".into()),
        spell: Some("Spl_Transform_Wolf".into()),
        ..ItemDef::default()
    });
    let mut sim = Sim::new(SimConfig::default(), open_world(), db);
    sim.load_spells(spells).expect("spells");
    let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
    sim.state.add_item("it_potion_wolf", 1);
    let info = sim.items.info("It_Potion_Wolf");
    sim.actor_mut(hero).inventory.add_items(
        "It_Potion_Wolf",
        info,
        1,
        0,
        gothic_gameplay::inventory::StackType::Normal,
    );
    sim.drain_events();
    let out = sim.player_use_item("It_Potion_Wolf");
    assert!(matches!(
        out,
        Some(gothic_script::player::use_item::UseOutcome::Magic(State::PsMagicPotion))
    ), "{out:?}");
    sim.run_ticks(400);
    let ev = events_of(&mut sim);
    let tf: Vec<_> = ev
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Magic(MagicEvent::Transform { template, secs, .. }) => {
                Some((template.clone(), *secs))
            }
            _ => None,
        })
        .collect();
    assert!(
        tf.iter().any(|(t, s)| t == "Wolf" && *s > 30.0),
        "{tf:?} state {:?} task {:?} / {:?}",
        sim.script.npc(hero).state,
        sim.script.npc(hero).task,
        ev.iter().map(|e| format!("{:?}", e.kind)).filter(|k| k.contains("Magic")).collect::<Vec<_>>()
    );
}

#[test]
fn the_bow_hangs_on_the_back_when_the_hands_are_emptied() {
    // The real item export (skipped where it is not available).
    let Some(text) = [
        ".local/items.json",
        "../../.local/items.json",
        "C:/Users/PowerTower/dev/gothic-reforged/.local/items.json",
    ]
    .iter()
    .find_map(|p| std::fs::read_to_string(p).ok()) else {
        return;
    };
    let items = gothic_sim::item::ItemDb::from_json(&text).expect("items");
    let mut a =
        gothic_sim::actor::Actor::npc(ActorId(0), "Hero", gothic_sim::types::Species::Human, 1, 1);
    let bow = items.get("It_Bow_Hunting").cloned().expect("bow");
    let arrow = items.get("Arrow").cloned().expect("arrow");
    a.inventory.insert("Arrow", 20);
    a.wear(&bow, &items);
    a.wear(&arrow, &items);
    a.hold(None, Some(&bow), &items);
    a.drawn = true;
    a.wear(&bow, &items);
    a.hold(None, Some(&bow), &items);
    assert_eq!(
        a.equipment.left.as_ref().map(|d| d.name.as_str()),
        Some("It_Bow_Hunting")
    );
    a.hold(None, None, &items);
    let stacks: Vec<_> = a
        .inventory
        .stacks()
        .iter()
        .map(|s| (s.template.clone(), s.slot, s.use_type(), s.category()))
        .collect();
    assert_eq!(
        a.equipment.left.as_ref().map(|d| d.name.as_str()),
        Some("It_Bow_Hunting"),
        "{stacks:?}"
    );
}

/// Selecting another rune puts it into the hand in place of the one held.
#[test]
fn selecting_another_rune_replaces_the_one_in_the_hand() {
    let (mut sim, hero, _) = world(1);
    let hand = |sim: &Sim<Grid2dWorld>| {
        sim.actor(hero)
            .equipment
            .right
            .as_ref()
            .map(|d| d.name.clone())
    };
    for name in ["It_Spell_Fireball", "It_Spell_SummonSkeleton"] {
        let info = sim.items.info(name);
        sim.actor_mut(hero).inventory.add_items(
            name,
            info,
            1,
            0,
            gothic_gameplay::inventory::StackType::Normal,
        );
    }
    assert!(sim.player_select_spell("It_Spell_Fireball"));
    assert_eq!(hand(&sim).as_deref(), Some("It_Spell_Fireball"));
    assert!(sim.player_select_spell("It_Spell_SummonSkeleton"));
    assert_eq!(hand(&sim).as_deref(), Some("It_Spell_SummonSkeleton"));
}
