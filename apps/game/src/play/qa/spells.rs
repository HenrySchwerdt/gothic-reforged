//! `spell_zoo`: every spell of the item data cast by the hero through the ported cast pipeline
//! (`PS_Magic*` -> `MagicInstant/Projectile/Summon` -> `FuncOnTargetHit`) at a character of the
//! world data. Each spell reports the `MagicEvent`s it produced and the script states the victim
//! went through; a spell that produces no `Cast` event is a failure.
use super::more2::{grab, tgt_id};
use super::{Cx, Poll, Sc, expect};
use bevy::ecs::message::MessageCursor;
use bevy::prelude::*;
use gothic_runtime::{Command, CoreCommand, CoreEvent, Event};
use gothic_sim::types::ActorId;

/// Which spells a batch casts.
#[derive(Clone, Copy)]
enum Batch {
    /// Everything that works on a character (not animal-only, not teleports).
    Person,
    /// Spells that need an animal in focus (`CanTameAnimal`, `CanTransformInto`, call / banish).
    Animal,
    /// One teleport last (it moves the hero).
    Teleport,
}

fn is_animal_spell(spell: &str) -> bool {
    matches!(
        spell,
        "Spl_AnimalForm" | "Spl_TameAnimal" | "Spl_CallAnimals" | "Spl_BanishVermin"
    )
}

/// One item template per spell (preferring the rune `It_Spell_*`), sorted by the spell name.
fn spell_items(cx: &Cx, batch: Batch) -> Vec<String> {
    let mut best: std::collections::BTreeMap<String, String> = Default::default();
    for i in cx.core().0.sim.items.iter() {
        let Some(sp) = i.spell.clone() else { continue };
        if sp.contains("Template") {
            continue;
        }
        let keep = match batch {
            Batch::Person => !is_animal_spell(&sp) && !sp.starts_with("Spl_Teleport"),
            Batch::Animal => is_animal_spell(&sp),
            Batch::Teleport => sp == "Spl_Teleport_Ardea",
        };
        if !keep {
            continue;
        }
        let rune = i.name.to_ascii_lowercase().starts_with("it_spell_");
        let e = best.entry(sp).or_default();
        if e.is_empty() || (rune && !e.to_ascii_lowercase().starts_with("it_spell_")) {
            *e = i.name.clone();
        }
    }
    best.into_values().collect()
}

/// Setup, not under test: the hero keeps mana and life, the victim stays alive.
fn keep_alive(cx: &mut Cx) {
    let hero = cx.core().0.hero;
    let tgt = tgt_id(cx);
    let mut core = cx.core_mut();
    let a = core.0.sim.actor_mut(hero);
    a.base.mana_max = a.base.mana_max.max(1000);
    a.mana = a.base.mana_max;
    a.hp = a.hp.max(a.base.hp_max.max(200));
    if let Some(t) = tgt {
        let a = core.0.sim.actor_mut(t);
        if a.is_alive() {
            a.hp = a.hp.max(a.base.hp_max.max(200));
        }
    }
}

fn collect(sc: &mut Sc, key: &'static str) {
    let mut cur: MessageCursor<CoreEvent> = MessageCursor::default();
    sc.watch(move |cx| {
        let idx = cx.st.n.get(&format!("{key}i")).copied().unwrap_or(-1.0) as i64;
        let evs: Vec<String> = {
            let msgs = cx.world.resource::<Messages<CoreEvent>>();
            cur.read(msgs)
                .filter_map(|e| match &e.0 {
                    Event::Magic(m) => Some(format!("{m:?}")),
                    _ => None,
                })
                .collect()
        };
        for e in evs {
            let kind = e.split([' ', '{', '(']).next().unwrap_or("?").to_string();
            let s = cx.st.store.entry(format!("{key}ev{idx}")).or_default();
            if !s.contains(&format!("{kind},")) {
                s.push_str(&kind);
                s.push(',');
                if kind != "Phase" && kind != "Cast" {
                    let full = cx.st.store.entry(format!("{key}full{idx}")).or_default();
                    full.push_str(&format!(" | {}", &e[..e.len().min(150)]));
                }
            }
        }
        if idx >= 0
            && let Some(t) = tgt_id(cx)
        {
            let st = cx.core().0.sim.script.npc(t).state.map(|s| s.name().to_string());
            if let Some(n) = st {
                let s = cx.st.store.entry(format!("{key}st{idx}")).or_default();
                if !s.contains(&format!("{n},")) {
                    s.push_str(&n);
                    s.push(',');
                }
            }
        }
        keep_alive(cx);
    });
}

fn batch_steps(sc: &mut Sc, key: &'static str, batch: Batch, label: &'static str) {
    collect(sc, key);
    sc.act(format!("{label}: list the spells"), move |cx| {
        let v = spell_items(cx, batch);
        cx.note(format!("{label}: {} spells: {}", v.len(), v.join(" ")));
        cx.st.store.insert(format!("{key}list"), v.join(","));
        cx.st.n.insert(format!("{key}i"), -1.0);
    });
    sc.step(format!("{label}: cast each spell"), move |cx| {
        let list: Vec<String> = cx.st.store[&format!("{key}list")]
            .split(',')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        let i = cx.st.n[&format!("{key}i")] as i64;
        let phase_t = cx.now - cx.st.n.get(&format!("{key}t0")).copied().unwrap_or(0.0) as f32;
        let tgt = tgt_id(cx);
        if i < 0 || phase_t > 5.0 {
            if i >= 0 {
                let ev = cx.st.store.get(&format!("{key}ev{i}")).cloned().unwrap_or_default();
                let st = cx.st.store.get(&format!("{key}st{i}")).cloned().unwrap_or_default();
                let full = cx.st.store.get(&format!("{key}full{i}")).cloned().unwrap_or_default();
                cx.note(format!("spell {}: events {ev} victim states {st}{full}", list[i as usize]));
            }
            let n = (i + 1) as usize;
            if n >= list.len() {
                return Poll::Done;
            }
            cx.st.n.insert(format!("{key}i"), n as f64);
            cx.st.n.insert(format!("{key}t0"), f64::from(cx.now));
            cx.st.n.insert(format!("{key}cast"), 0.0);
            let name = list[n].clone();
            {
                // Setup: the hero is in his own shape again (a transformation lasts 30 to 60 s).
                let mut core = cx.core_mut();
                for t in core.0.sim.script.magic.transforms.iter_mut() {
                    t.until_ms = 0;
                }
            }
            if let Some(t) = tgt {
                // Setup: the victim goes back to an idle routine state (a sleeper or a stunned
                // character would hide the next spell's effect).
                let idle = gothic_sim::script::routine::state::State::from_name("ZS_HangAround").unwrap();
                cx.core_mut().0.sim.qa_enter_state(t, idle);
            }
            // (the item is handed over a second later: the end of a transformation empties the hands)
            cx.st.store.insert(format!("{key}pending"), name);
            return Poll::Wait;
        }
        if phase_t > 1.0
            && let Some(name) = cx.st.store.remove(&format!("{key}pending"))
        {
            cx.world.write_message(CoreCommand(Command::GiveHero {
                template: name.clone(),
                amount: 1,
            }));
            if name.to_ascii_lowercase().contains("potion") {
                // Potions are drunk (`UseItem`), they are no runes.
                cx.world
                    .write_message(CoreCommand(Command::UseItem { template: name }));
                cx.st.n.insert(format!("{key}cast"), 1.0);
            } else {
                cx.world
                    .write_message(CoreCommand(Command::SelectSpell { template: name }));
            }
            cx.st.n.insert(format!("{key}t0"), f64::from(cx.now));
            return Poll::Wait;
        }
        if cx.st.store.contains_key(&format!("{key}pending")) {
            return Poll::Wait;
        }
        // A cast that fizzled (the victim was out of sight for a moment) is tried once more.
        let fizzled = cx
            .st
            .store
            .get(&format!("{key}ev{i}"))
            .is_some_and(|e| !e.contains("Cast") && !e.contains("Weapon") && !e.contains("Transform"));
        if phase_t > 3.2 && cx.st.n[&format!("{key}cast")] == 1.0 && fizzled {
            cx.st.n.insert(format!("{key}cast"), 2.0);
            cx.world.write_message(CoreCommand(Command::CastSpell {
                target: tgt,
                power: false,
            }));
        }
        if phase_t > 1.5 && cx.st.n[&format!("{key}cast")] == 0.0 {
            cx.st.n.insert(format!("{key}cast"), 1.0);
            cx.world.write_message(CoreCommand(Command::CastSpell {
                target: tgt,
                power: false,
            }));
        }
        Poll::Wait
    });
    sc.check(format!("{label}: every spell produced a Cast event"), move |cx| {
        let list: Vec<String> = cx.st.store[&format!("{key}list")]
            .split(',')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        let bad: Vec<&String> = list
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                !cx.st
                    .store
                    .get(&format!("{key}ev{i}"))
                    .is_some_and(|e| e.contains("Cast") || e.contains("Weapon") || (list[*i].to_ascii_lowercase().contains("potion") && !e.is_empty()))
            })
            .map(|(_, n)| n)
            .collect();
        expect(
            bad.is_empty(),
            format!("{} spells cast", list.len()),
            format!("no Cast event for {bad:?}"),
        )
    });
}

pub fn spell_zoo() -> Sc {
    let mut sc = Sc::new(1800.0);
    grab(&mut sc);
    super::npc_ai::find_and_place(&mut sc, "Shaman", 900.0);
    sc.act("the victim is the shaman", |cx| {
        let _ = ActorId(0);
        let _ = cx;
    });
    batch_steps(&mut sc, "p", Batch::Person, "person spells");
    super::npc_ai::find_and_place(&mut sc, "Wolf", 700.0);
    batch_steps(&mut sc, "a", Batch::Animal, "animal spells");
    batch_steps(&mut sc, "t", Batch::Teleport, "teleport");
    sc
}
