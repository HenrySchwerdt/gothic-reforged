//! Headless duel scenarios: level-1 hero with a one-handed sword against an Ardea orc.
mod common;
use common::*;
use gothic_sim::actor::Actor;
use gothic_sim::config::SimConfig;
use gothic_sim::types::*;

/// A typical Ardea orc raider: level 10 / LevelMax 30 (`Orc_GameStartRaider_Warrior_01`).
fn synthetic_orc() -> Actor {
    let mut o = Actor::npc(ActorId(0), "Orc_Raider", Species::Orc, 10, 30);
    o.alignment = Alignment::Orc;
    o.refresh(&SimConfig::default());
    o
}

#[test]
fn duel_is_deterministic_and_terminates() {
    let items = synthetic_items();
    let a = duel(42, &items, synthetic_orc(), 70);
    let b = duel(42, &items, synthetic_orc(), 70);
    assert_eq!(a.winner, b.winner);
    assert_eq!(a.seconds, b.seconds);
    assert_eq!((a.hero_hp, a.orc_hp), (b.hero_hp, b.orc_hp));
    assert_eq!(a.events, b.events, "same seed, same event log");
    assert!(a.winner.is_some(), "the fight ends within 180 s: {a:?}");
    assert!(a.seconds > 3.0, "not an instant kill: {}", a.seconds);
}

#[test]
fn damage_numbers_match_the_formulas() {
    // [CA 2.5, 2.7] hero: 50 + STR/2 = 100 minus the orc's 45 percent = 55 per plain hit;
    // orc: trunc(0.75*25 + 200/7) = 47 flat against an unarmored hero.
    let items = synthetic_items();
    let r = duel(7, &items, synthetic_orc(), 70);
    for e in &r.events {
        if let gothic_sim::sim::EventKind::Hit {
            damage,
            action,
            ignored: None,
            parried: false,
            victim,
            ..
        } = &e.kind
        {
            if victim.0 == 1 {
                // orc is actor 1: plain attack 55, quick 27, power 55, pierce 55 / hack 110
                let ok = matches!(
                    (*action, *damage),
                    (
                        Action::Attack
                            | Action::PowerAttack
                            | Action::PierceAttack
                            | Action::SimpleWhirl,
                        55
                    ) | (Action::QuickAttack, 27)
                        | (Action::HackAttack, 110)
                );
                assert!(ok, "unexpected hero damage {action:?} {damage}");
            } else {
                // hero is actor 0
                let ok = matches!(
                    (*action, *damage),
                    (
                        Action::Attack | Action::PierceAttack | Action::PowerAttack,
                        47
                    ) | (Action::HackAttack, 94)
                        | (Action::QuickAttack, 23)
                        | (Action::WhirlAttack | Action::SimpleWhirl, 47)
                );
                assert!(ok, "unexpected orc damage {action:?} {damage}");
            }
        }
    }
}

#[test]
fn win_rate_over_many_seeds_is_reported() {
    let items = synthetic_items();
    let mut hero = 0;
    let mut orc = 0;
    let mut other = 0;
    let mut secs = 0.0;
    for seed in 0..40 {
        let r = duel(seed, &items, synthetic_orc(), 70);
        match r.winner.as_deref() {
            Some("hero") => {
                hero += 1;
                assert!(r.orc_dead, "a hostile humanoid defeat is lethal [CA 2.11]");
                assert_eq!(r.xp, 600, "kill XP max(10*L, 50) = 100 added to level-1 XP");
            }
            Some("orc") => orc += 1,
            _ => other += 1,
        }
        secs += r.seconds;
    }
    println!(
        "synthetic duel over 40 seeds: hero {hero}, orc {orc}, other {other}, mean {:.1}s",
        secs / 40.0
    );
    assert_eq!(other, 0);
    // A level-1 hero is badly outclassed by a LevelMax-30 raider (600 HP, 45 percent
    // protection, 47 damage per hit): the formulas of the docs make this the expected balance.
    assert!(orc > hero, "orc {orc} hero {hero}");
}

#[test]
fn stronger_bot_skill_helps_the_hero() {
    let items = synthetic_items();
    // A weak opponent (level 3 / 5: 100 HP, 7 percent protection) so that skill matters.
    let weak_orc = || {
        let mut o = Actor::npc(ActorId(0), "Orc_Runt", Species::Orc, 3, 5);
        o.alignment = Alignment::Orc;
        o
    };
    let wins = |skill: i32| {
        (0..40)
            .filter(|s| duel(*s, &items, weak_orc(), skill).winner.as_deref() == Some("hero"))
            .count()
    };
    let weak = wins(5);
    let strong = wins(100);
    println!("hero wins out of 40: skill 5 -> {weak}, skill 100 -> {strong}");
    assert!(strong >= weak);
}

/// Uses the real extracted NPC / item data. Ignored by default: needs `.local/*.json`.
#[test]
#[ignore = "needs .local/npcs.json and .local/items.json"]
fn real_ardea_orc_duel_report() {
    let (items, npcs) = real_data().expect("run the inspector exports first");
    let cfg = SimConfig::default();
    let rec = npcs
        .entity("Orc_GameStartRaider_Warrior_01")
        .expect("typical Ardea raider");
    let mk = || rec.to_actor(ActorId(0), &cfg, &items);
    let o = mk();
    println!(
        "orc {}: level {}/{} -> HP {}, STR {}, stamina {}, protection {}",
        rec.name,
        o.level,
        o.level_max,
        o.hp_max(&cfg),
        o.strength(&cfg),
        o.stamina_max(&cfg),
        o.protection(&cfg, DamageType::Blade)
    );
    let (mut hero, mut orc, mut secs, mut hits_o, mut hits_h) = (0, 0, 0.0, 0, 0);
    let n = 100;
    for seed in 0..n {
        let r = duel(seed, &items, mk(), 70);
        match r.winner.as_deref() {
            Some("hero") => hero += 1,
            Some("orc") => orc += 1,
            _ => {}
        }
        secs += r.seconds;
        hits_o += r.hits_on_orc;
        hits_h += r.hits_on_hero;
        if seed == 0 {
            println!(
                "seed 0: winner {:?} after {:.1}s, hero HP {}, orc HP {}, hits on orc {}, on hero {}, hero parries {}, whiffs {}",
                r.winner,
                r.seconds,
                r.hero_hp,
                r.orc_hp,
                r.hits_on_orc,
                r.hits_on_hero,
                r.parries_by_hero,
                r.whiffs
            );
        }
    }
    println!(
        "{n} seeds: hero wins {hero}, orc wins {orc}, mean duration {:.1}s, mean hits on orc {:.1}, on hero {:.1}",
        secs / n as f32,
        hits_o as f32 / n as f32,
        hits_h as f32 / n as f32
    );
    assert!(hero + orc > 0);
}
