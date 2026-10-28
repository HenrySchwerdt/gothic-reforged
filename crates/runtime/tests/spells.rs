//! The hero's spells reach their effects through the core: items granted at run time
//! (`GiveHero`), selected, cast at a character, one spell after the other.
use gothic_gameplay::{GameDatabase, GameState};
use gothic_runtime::{Command, Core, Event};
use gothic_sim::data::NpcRecord;
use gothic_sim::item::{ItemDb, ItemDef};
use gothic_sim::script::magic::api::MagicEvent;
use gothic_sim::world::Pos;
use std::sync::Arc;

const SPELLS: &str = r#"[
 {"name":"Spl_Disease","spell_type":1,"deity":1,"min_mana":10,"max_mana":10,"target":1,"cast_type":2,
  "func_on_cast":"MagicInstant","func_on_hit":"MagicDisease","anim_cast":"","anim_power":"","spawn":null},
 {"name":"Spl_Poison","spell_type":1,"deity":1,"min_mana":10,"max_mana":10,"target":1,"cast_type":2,
  "func_on_cast":"MagicInstant","func_on_hit":"MagicPoison","anim_cast":"","anim_power":"","spawn":null}
]"#;

fn core() -> Core {
    let text = "[LocAdmin_Languages]\r\nLanguages=English;German\r\nCurrentLanguage=English\r\nCurrentRevision=0\r\n[LocAdmin_Strings]\r\nQUEST_X=Quest;;Quest;\r\n[LocAdmin_Revisions]\r\n";
    let mut b = vec![0xFF, 0xFE];
    for u in text.encode_utf16() {
        b.extend(u.to_le_bytes());
    }
    let strings = gothic_formats::stringtable::StringTable::parse(&b).unwrap();
    let db = Arc::new(GameDatabase::from_parts(vec![], vec![], strings, "English").unwrap());
    let mut items = ItemDb::default();
    for (n, sp) in [("It_Spell_Disease", "Spl_Disease"), ("It_Spell_Poison", "Spl_Poison")] {
        items.insert(ItemDef {
            name: n.into(),
            category: "Spellbook".into(),
            use_type: Some("Cast".into()),
            spell: Some(sp.into()),
            permanent: true,
            ..ItemDef::default()
        });
    }
    let mut c = Core::new(db, items, GameState::new(), 7);
    c.guess_weapons = false;
    c
}

#[test]
fn the_hero_casts_one_spell_after_the_other_at_a_character() {
    let mut c = core();
    c.sim.load_spells(SPELLS).unwrap();
    let evs = c.tick(
        0.0,
        &[Command::SpawnNpc {
            record: Box::new(NpcRecord {
                name: "Victim".into(),
                species: "Human".into(),
                ..Default::default()
            }),
            at: Pos::flat(300.0, 0.0),
            yaw: 0.0,
        }],
    );
    let victim = evs
        .iter()
        .find_map(|e| match e {
            Event::ActorSpawned { actor, .. } => Some(*actor),
            _ => None,
        })
        .expect("spawned");
    let mut seen: Vec<String> = Vec::new();
    c.tick(
        0.05,
        &[
            Command::GiveHero {
                template: "It_Spell_Disease".into(),
                amount: 1,
            },
            Command::GiveHero {
                template: "It_Spell_Poison".into(),
                amount: 1,
            },
        ],
    );
    for name in ["It_Spell_Disease", "It_Spell_Poison"] {
        c.tick(
            0.05,
            &[
                Command::SelectSpell {
                    template: name.into(),
                },
            ],
        );
        let hero = c.sim.hero.unwrap();
        seen.push(format!(
            "{name}: right hand right after the select {:?}",
            c.sim.actor(hero).equipment.right.as_ref().map(|d| d.name.clone()),
        ));
        for _ in 0..40 {
            c.tick(0.05, &[]);
        }
        seen.push(format!(
            "{name}: right hand {:?}, bag {:?}",
            c.sim.actor(hero).equipment.right.as_ref().map(|d| d.name.clone()),
            c.sim.actor(hero).inventory.count(name)
        ));
        let mut evs = c.tick(
            0.05,
            &[Command::CastSpell {
                target: Some(victim),
                power: false,
            }],
        );
        for _ in 0..120 {
            evs.extend(c.tick(0.05, &[]));
        }
        for e in &evs {
            match e {
                Event::Magic(MagicEvent::Status { mask, on, .. }) => {
                    seen.push(format!("{name}: status {mask} {on}"))
                }
                Event::CastRefused(r) => seen.push(format!("{name}: refused {r:?}")),
                Event::Notice(t) => seen.push(format!("{name}: {t}")),
                _ => {}
            }
        }
    }
    assert!(
        seen.iter().any(|s| s.contains("Disease") && s.contains("status 8 true")),
        "{seen:?}"
    );
    assert!(
        seen.iter().any(|s| s.contains("Poison") && s.contains("status 4 true")),
        "{seen:?}"
    );
}
