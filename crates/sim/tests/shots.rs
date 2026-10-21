//! Arrows and missiles fly, hit and stick by the rules of the projectile port; the old dice roll
//! for a ranged hit is gone.
mod common;
use common::*;
use glam::Vec3;
use gothic_projectile::launch::lob_direction;
use gothic_sim::actor::Actor;
use gothic_sim::config::SimConfig;
use gothic_sim::item::ItemDef;
use gothic_sim::shots::HeroAim;
use gothic_sim::sim::{EventKind, Sim};
use gothic_sim::types::*;
use gothic_sim::world::{Grid2dWorld, Pos};

const SPELLS: &str = r#"[
 {"name":"Spl_Fireball","spell_type":0,"deity":2,"min_mana":15,"max_mana":30,"target":1,"cast_type":1,
  "func_on_cast":"","func_on_hit":"","anim_cast":"","anim_power":"",
  "spawn":{"template":"Mis_Fireball","damage":{"DamageType":4,"DamageAmount":50,"DamageManaMultiplier":2.0,"DamageHitMultiplier":1.0},
           "projectile":{"ShootVelocity":100.0}}}
]"#;

fn world(orc_at: f32) -> (Sim<Grid2dWorld>, ActorId, ActorId) {
    let mut db = synthetic_items();
    db.insert(ItemDef {
        name: "It_Spell_Fireball".into(),
        category: "Spellbook".into(),
        use_type: Some("Cast".into()),
        spell: Some("Spl_Fireball".into()),
        permanent: true,
        ..ItemDef::default()
    });
    let mut sim = Sim::new(SimConfig::default(), open_world(), db);
    sim.load_spells(SPELLS).expect("spells");
    sim.player_alignment = Alignment::Reb;
    let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
    let mut orc = Actor::npc(ActorId(0), "Orc_Raider", Species::Orc, 10, 10);
    orc.alignment = Alignment::Orc;
    orc.base_attitude_to_player = Attitude::Hostile;
    orc.refresh(&SimConfig::default());
    orc.ai_controlled = false;
    orc.hp = 100_000;
    let orc = sim.spawn(orc, Pos::flat(orc_at, 0.0));
    (sim, hero, orc)
}

fn archer(orc_at: f32) -> (Sim<Grid2dWorld>, ActorId, ActorId) {
    let (mut sim, hero, orc) = world(orc_at);
    sim.equip(hero, "It_Bow_Test").expect("bow");
    sim.equip(hero, "It_Arrow").expect("arrows");
    sim.actor_mut(hero).inventory.insert("It_Arrow", 5);
    assert!(sim.player_ready_ranged());
    assert!(sim.run_until(600, |s| s.player_idle()));
    (sim, hero, orc)
}

/// The aim a player gives: the crosshair on the point the lob needs to land on `goal`.
fn aim_at(from: Vec3, goal: Vec3) -> HeroAim {
    let dir = lob_direction(from, goal, 200.0).expect("in range");
    HeroAim {
        origin: from,
        pick_point: from + dir * 5000.0,
    }
}

const HAND: Vec3 = Vec3::new(40.0, 144.0, 0.0);

#[test]
fn a_drawn_arrow_leaves_at_55_metres_per_second_and_the_hit_is_geometry() {
    let (mut sim, _hero, orc) = archer(3000.0);
    sim.set_hero_aim(Some(aim_at(HAND, Vec3::new(3000.0, 120.0, 0.0))));
    sim.player_aim(true);
    sim.run_ticks(90); // 4.5 s: full strength
    sim.drain_events();
    let hp0 = sim.actor(orc).hp;
    assert!(sim.player_release(Some(orc)));
    // The arrow appears once the release state has run; follow its first two ticks.
    let mut first: Option<Vec3> = None;
    let mut speeds: Vec<f32> = Vec::new();
    for _ in 0..200 {
        sim.run_ticks(1);
        let views = sim.shot_views();
        if let Some(v) = views.iter().find(|v| !v.spell && v.flying) {
            if let Some(prev) = first {
                speeds.push((v.position - prev).length() / 0.05);
            }
            first = Some(v.position);
        }
        if sim.actor(orc).hp < hp0 {
            break;
        }
    }
    assert!(sim.actor(orc).hp < hp0, "the lobbed arrow reached the orc");
    let mean = speeds.iter().sum::<f32>() / speeds.len() as f32;
    assert!(mean > 4800.0 && mean < 5700.0, "mean speed {mean} cm/s");
    let ev = sim.drain_events();
    assert!(ev.iter().any(|e| matches!(&e.kind, EventKind::ProjectileImpact { on_actor: Some(a), spell: false, .. } if *a == orc)));
}

#[test]
fn an_arrow_that_misses_sticks_in_the_ground_and_fades_away() {
    let (mut sim, _hero, orc) = archer(3000.0);
    // Aim 4 m to the side of the orc.
    let mut aim = aim_at(HAND, Vec3::new(3000.0, 120.0, 400.0));
    aim.origin = HAND;
    sim.set_hero_aim(Some(aim));
    sim.player_aim(true);
    sim.run_ticks(90);
    sim.drain_events();
    let hp0 = sim.actor(orc).hp;
    assert!(sim.player_release(Some(orc)));
    sim.run_ticks(100);
    assert_eq!(sim.actor(orc).hp, hp0, "a miss is a miss");
    let views = sim.shot_views();
    let stuck = views
        .iter()
        .find(|v| !v.spell && !v.flying)
        .expect("the arrow stands in the ground");
    assert!(stuck.position.y < 60.0, "{:?}", stuck.position);
    assert!(stuck.position.z > 300.0, "it landed beside the orc");
    // Ten seconds later it has decayed.
    sim.run_ticks(260);
    assert!(
        sim.shot_views().iter().all(|v| v.spell || v.flying),
        "the arrow is gone"
    );
    let ev = sim.drain_events();
    assert!(
        ev.iter()
            .any(|e| matches!(&e.kind, EventKind::ProjectileEnded { .. }))
    );
}

#[test]
fn a_head_shot_hits_harder_than_a_body_shot() {
    let loss = |goal_y: f32| {
        let (mut sim, _hero, orc) = archer(2500.0);
        // The orc is 200 cm tall; above 85 percent of that is the head.
        sim.set_hero_aim(Some(aim_at(HAND, Vec3::new(2500.0, goal_y, 0.0))));
        sim.player_aim(true);
        sim.run_ticks(90);
        let hp0 = sim.actor(orc).hp;
        sim.player_release(Some(orc));
        sim.run_ticks(120);
        hp0 - sim.actor(orc).hp
    };
    let body = loss(100.0);
    let head = loss(185.0);
    assert!(body > 0 && head > 0, "both shots hit: {body} / {head}");
    assert!(head > body, "head shot {head} against body shot {body}");
}

#[test]
fn a_short_draw_falls_short() {
    let (mut sim, _hero, orc) = archer(3500.0);
    // The crosshair is on the orc, but the string was only touched: the arrow drops before it.
    sim.set_hero_aim(Some(HeroAim {
        origin: HAND,
        pick_point: Vec3::new(3500.0, 120.0, 0.0),
    }));
    sim.player_aim(true);
    sim.run_ticks(3);
    let hp0 = sim.actor(orc).hp;
    sim.player_release(Some(orc));
    sim.run_ticks(200);
    assert_eq!(sim.actor(orc).hp, hp0);
}

#[test]
fn a_fireball_flies_100_kmh_and_burns_the_orc() {
    let (mut sim, hero, orc) = world(2778.0);
    sim.equip(hero, "It_Spell_Fireball").expect("rune");
    assert!(sim.player_select_spell("It_Spell_Fireball"));
    assert!(sim.run_until(600, |s| s.player_idle()));
    let hp0 = sim.actor(orc).hp;
    sim.player_cast(Some(orc), false).expect("cast");
    let mut seen = Vec::new();
    for _ in 0..400 {
        sim.run_ticks(1);
        if let Some(v) = sim.shot_views().iter().find(|v| v.spell && v.flying) {
            seen.push(v.position);
        }
        if sim.actor(orc).hp < hp0 {
            break;
        }
    }
    assert!(sim.actor(orc).hp < hp0);
    assert!(
        seen.len() > 8,
        "the missile was visible for {} ticks",
        seen.len()
    );
    let step = (seen[5] - seen[4]).length() / 0.05;
    assert!((step - 2777.8).abs() < 5.0, "speed {step} cm/s");
    let straight = (seen[seen.len() - 1] - seen[0]).normalize();
    assert!(
        straight.x > 0.99,
        "a straight line at the orc: {straight:?}"
    );
}
