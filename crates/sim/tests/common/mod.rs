//! Shared fixtures for the scenario tests. Everything here is synthetic (public numbers from
//! the docs) except [`real_data`], which reads the gitignored `.local/` exports.
#![allow(dead_code)]
use gothic_sim::actor::Actor;
use gothic_sim::config::SimConfig;
use gothic_sim::item::{DamageDef, ItemDb, ItemDef, Modifier, Requirement};
use gothic_sim::sim::{Event, EventKind, Sim};
use gothic_sim::types::*;
use gothic_sim::world::{Grid2dWorld, Pos};
use std::path::PathBuf;

pub fn synthetic_items() -> ItemDb {
    let mut db = ItemDb::default();
    let mut sword = ItemDef::weapon("It_1H_Sword_01", "1H", "Blade", 50);
    sword.requirements.push(Requirement {
        tag: "STR".into(),
        op: 0,
        value: 100,
    });
    sword.required_skills.push("Perk_1H_1".into());
    sword.value = 50;
    db.insert(sword);
    db.insert(ItemDef::weapon(
        "It_Halberd_Orc_01",
        "Halberd",
        "Blade",
        200,
    ));
    db.insert(ItemDef::weapon("It_Arrow", "Arrow", "Missile", 40));
    db.insert(ItemDef::weapon("Arrow", "Arrow", "Missile", 40));
    db.insert(ItemDef {
        name: "It_Potion_Health".into(),
        use_type: Some("Potion".into()),
        modifiers: vec![Modifier {
            tag: "HP".into(),
            op: 2,
            value: 50,
        }],
        ..ItemDef::default()
    });
    db.insert(ItemDef {
        name: "It_Bow_Test".into(),
        use_type: Some("Bow".into()),
        damage: Some(DamageDef::new("Missile", 10)),
        ..ItemDef::default()
    });
    db
}

pub fn open_world() -> Grid2dWorld {
    let mut w = Grid2dWorld::new();
    w.outdoors = true;
    w
}

/// Result of one duel.
#[derive(Debug, Clone)]
pub struct Report {
    pub seed: u64,
    pub winner: Option<String>,
    pub seconds: f32,
    pub hero_hp: i32,
    pub orc_hp: i32,
    pub orc_dead: bool,
    pub hits_on_orc: u32,
    pub hits_on_hero: u32,
    pub parries_by_hero: u32,
    pub parries_by_orc: u32,
    pub whiffs: u32,
    pub xp: u64,
    pub level: u32,
    pub events: Vec<Event>,
}

/// Hero (level 1, 1H sword) vs `orc`; returns when one of them is down or after 180 s.
pub fn duel(seed: u64, items: &ItemDb, orc: Actor, hero_skill: i32) -> Report {
    let cfg = SimConfig {
        seed,
        ..SimConfig::default()
    };
    duel_cfg(cfg, items, orc, hero_skill)
}

/// [`duel`] with an explicit configuration (variants).
pub fn duel_cfg(cfg: SimConfig, items: &ItemDb, mut orc: Actor, hero_skill: i32) -> Report {
    let seed = cfg.seed;
    let mut sim = Sim::new(cfg, open_world(), items.clone());
    sim.player_alignment = Alignment::Reb; // Orcs are hostile to rebels [CA 10.4]
    sim.state.level = 1;
    sim.state.xp = 500; // cumulative XP of level 1
    sim.state.skills.insert("perk_1h_1".into());
    let hero = sim.spawn_hero(Pos::flat(0.0, 0.0));
    sim.equip(hero, "It_1H_Sword_01")
        .expect("hero can wield the sword");
    {
        let h = sim.actor_mut(hero);
        h.ai_controlled = true; // drive the hero with the same combat AI the player states use [CA 9.5]
        h.combat_skill_pct = Some(hero_skill);
        h.drawn = true;
    }
    if let Some(w) = gothic_sim::data::default_weapon(items, orc.species) {
        orc.equipment.equip(w);
    }
    orc.base_attitude_to_player = Attitude::None;
    let orc_id = sim.spawn(orc, Pos::flat(1000.0, 0.0));
    sim.world.set_facing(orc_id, std::f32::consts::PI);
    sim.start_attack(hero, orc_id, AttackReason::Enemy);

    let max_ticks = 180_000 / u64::from(sim.cfg.tick_ms);
    sim.run_until(max_ticks, |s| {
        s.actor(hero).is_down_or_dead() || s.actor(orc_id).is_down_or_dead()
    });
    let events = sim.drain_events();
    let mut r = Report {
        seed,
        winner: None,
        seconds: sim.now_ms() as f32 / 1000.0,
        hero_hp: sim.actor(hero).hp,
        orc_hp: sim.actor(orc_id).hp,
        orc_dead: sim.actor(orc_id).ai_mode == AiMode::Dead,
        hits_on_orc: 0,
        hits_on_hero: 0,
        parries_by_hero: 0,
        parries_by_orc: 0,
        whiffs: 0,
        xp: sim.state.xp,
        level: sim.state.level,
        events: vec![],
    };
    for e in &events {
        match &e.kind {
            EventKind::Hit {
                attacker,
                victim,
                damage,
                parried,
                ignored: None,
                ..
            } => {
                if *victim == orc_id && *attacker == hero {
                    if *parried {
                        r.parries_by_orc += 1;
                    } else if *damage > 0 {
                        r.hits_on_orc += 1;
                    }
                } else if *victim == hero {
                    if *parried {
                        r.parries_by_hero += 1;
                    } else if *damage > 0 {
                        r.hits_on_hero += 1;
                    }
                }
            }
            EventKind::Whiff { .. } => r.whiffs += 1,
            _ => {}
        }
    }
    r.winner = if sim.actor(hero).is_down_or_dead() && sim.actor(orc_id).is_down_or_dead() {
        Some("both".into())
    } else if sim.actor(orc_id).is_down_or_dead() {
        Some("hero".into())
    } else if sim.actor(hero).is_down_or_dead() {
        Some("orc".into())
    } else {
        None
    };
    r.events = events;
    r
}

pub fn local_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local")
}

/// Real item and NPC exports; `None` when the install-derived files are missing.
pub fn real_data() -> Option<(ItemDb, gothic_sim::data::NpcFile)> {
    let items = ItemDb::load(&local_dir().join("items.json")).ok()?;
    let npcs = gothic_sim::data::NpcFile::load(&local_dir().join("npcs.json")).ok()?;
    Some((items, npcs))
}
