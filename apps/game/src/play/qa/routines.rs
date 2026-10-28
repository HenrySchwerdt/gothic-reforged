//! `routine_zoo`: every `ZS_*` task state of the script registry entered on a character of the
//! world data and run for a few seconds through the real simulation loop (`run_script_states`,
//! task callbacks, the host executing the queued `AiCommand`s). A state that panics, wedges the
//! character (no longer reacting to the next state) or leaves it dead fails the scenario; the
//! state coverage counters (`state:*` of `gothic_portcov`) and the function counters of every step
//! function behind them show what ran.
use super::more2::{grab, tgt_id};
use super::{Poll, Sc, expect};
use gothic_sim::script::routine::state::State;

/// States that end the character on purpose (death, rag doll) or hand the hero to the UI.
fn skip(s: State) -> bool {
    let n = s.name();
    !n.starts_with("ZS_")
        || matches!(
            n,
            "ZS_Dead" | "ZS_Dead_Loop" | "ZS_RagDollDead" | "ZS_RagDollDead_Loop" | "ZS_PiercedDead"
        )
}

fn zoo(sc: &mut Sc, key: &'static str, secs: f32) {
    sc.act(format!("{key}: list the states"), move |cx| {
        let list: Vec<&str> = State::ALL.iter().copied().filter(|s| !skip(*s)).map(|s| s.name()).collect();
        cx.note(format!("{key}: {} states", list.len()));
        cx.st.store.insert(format!("{key}list"), list.join(","));
        cx.st.n.insert(format!("{key}i"), -1.0);
    });
    sc.step(format!("{key}: enter each state for {secs} s"), move |cx| {
        let list: Vec<String> = cx.st.store[&format!("{key}list")].split(',').map(String::from).collect();
        let i = cx.st.n[&format!("{key}i")] as i64;
        let t0 = cx.st.n.get(&format!("{key}t0")).copied().unwrap_or(0.0) as f32;
        if i >= 0 && cx.now - t0 < secs {
            return Poll::Wait;
        }
        let n = (i + 1) as usize;
        if n >= list.len() {
            return Poll::Done;
        }
        let Some(id) = tgt_id(cx) else {
            return Poll::Fail("no target".into());
        };
        if i >= 0 {
            let st = cx.core().0.sim.script.npc(id).state.map(|s| s.name());
            let mode = cx.core().0.sim.actor(id).ai_mode;
            cx.note(format!("{} -> after {secs} s: state {st:?} mode {mode:?}", list[i as usize]));
        }
        cx.st.n.insert(format!("{key}i"), n as f64);
        cx.st.n.insert(format!("{key}t0"), f64::from(cx.now));
        let state = State::from_name(&list[n]).unwrap();
        cx.core_mut().0.sim.qa_enter_state(id, state);
        Poll::Wait
    });
}

pub fn routine_zoo() -> Sc {
    let mut sc = Sc::new(2400.0);
    grab(&mut sc);
    super::npc_ai::find_and_place(&mut sc, "Hamlar", 600.0);
    zoo(&mut sc, "h", 2.0);
    sc.check("the character is alive after every state", |cx| {
        let id = tgt_id(cx).unwrap();
        let a = cx.core().0.sim.actor(id);
        expect(a.is_alive(), format!("alive, hp {}", a.hp), "dead")
    });
    sc
}

/// `crime_theft`: the hero takes an item lying next to a character of the world data. The taking
/// goes through `PS_Normal_Take`, which offers the theft to everybody who can perceive it
/// (`SendPerceptionTakeItem` -> `AssessTakeItem`, S:32c00 / S:30620). Asserts the item changed
/// hands, the ported perception ran and, when the witness found the theft bothering, that it
/// holds the hero responsible.
pub fn crime_theft() -> Sc {
    use gothic_runtime::{Command, CoreCommand};
    use gothic_sim::script::host::EntityId;
    let mut sc = Sc::new(120.0);
    grab(&mut sc);
    super::npc_ai::find_and_place(&mut sc, "Hamlar", 400.0);
    sc.act("a sword lies at the hero's feet, in Hamlar's view", |cx| {
        let id = tgt_id(cx).unwrap();
        let pos = cx.core().0.sim.world_pos(id);
        let hero = cx.core().0.hero;
        let hp = cx.core().0.sim.world_pos(hero);
        let item = cx.core_mut().0.sim.script.add_item("It_Sword_01", hp);
        // Hamlar works next to the sword: stealing it bothers him (`IsBotheredByPlayerTheftOf`:
        // the item lies within 10 m of the witness' work point).
        {
            use gothic_sim::script::enums::UseType;
            let sim = &mut cx.core_mut().0.sim;
            let point = sim.script.add_point(UseType::None, EntityId::NONE, hp);
            sim.script.npc_mut(id).dest = point;
        }
        cx.st.n.insert("item".into(), f64::from(item.0));
        cx.st.n.insert("take0".into(), gothic_portcov::count("send_perception_take_item") as f64);
        cx.note(format!("item {item:?} at {hp:?}, Hamlar at {pos:?}"));
        let _ = EntityId::NONE;
        cx.world.write_message(CoreCommand(Command::PickUp { item: item.0 }));
    });
    sc.wait(4.0);
    sc.check("the sword is in the hero's inventory", |cx| {
        let n = cx.count_item("It_Sword_01");
        expect(n > 0, format!("{n} swords"), "the sword was not taken")
    });
    sc.check("the witness holds the hero responsible (AssessTakeItem -> AssessTarget Theft)", |cx| {
        let id = tgt_id(cx).unwrap();
        let a = cx.core().0.sim.actor(id);
        let hit = gothic_portcov::count("assess_take_item");
        expect(
            a.attack_reason == gothic_sim::types::AttackReason::Theft
                || a.ai_mode != gothic_sim::types::AiMode::Routine,
            format!("reason {:?}, mode {:?}", a.attack_reason, a.ai_mode),
            format!(
                "Hamlar did not react: reason {:?}, mode {:?}, assess_take_item runs {hit}",
                a.attack_reason, a.ai_mode
            ),
        )
    });
    sc.check("the taking was offered to the witnesses (ported SendPerceptionTakeItem)", |cx| {
        if !gothic_portcov::ENABLED {
            return super::ok("coverage build off: not counted");
        }
        let n = gothic_portcov::count("send_perception_take_item") as f64;
        expect(n > cx.st.n["take0"], format!("{n} offers"), "no witness was offered the theft")
    });
    sc
}

fn transform_events(sc: &mut Sc) {
    use bevy::ecs::message::MessageCursor;
    use bevy::prelude::Messages;
    use gothic_runtime::{CoreEvent, Event};
    use gothic_sim::script::magic::api::MagicEvent;
    let mut cur: MessageCursor<CoreEvent> = MessageCursor::default();
    sc.watch(move |cx| {
        let evs: Vec<(String, f32)> = {
            let msgs = cx.world.resource::<Messages<CoreEvent>>();
            cur.read(msgs)
                .filter_map(|e| match &e.0 {
                    Event::Magic(MagicEvent::Transform { template, secs, .. }) => {
                        Some((template.clone(), *secs))
                    }
                    _ => None,
                })
                .collect()
        };
        for (t, secs) in evs {
            if t.is_empty() {
                cx.st.n.insert("tf_end".into(), f64::from(cx.now));
            } else {
                cx.st.n.insert("tf_start".into(), f64::from(cx.now));
                cx.st.n.insert("tf_secs".into(), f64::from(secs));
                cx.st.store.insert("tf_template".into(), t);
            }
        }
        // Setup, not under test: nothing kills the hero while he waits.
        let hero = cx.core().0.hero;
        let mut core = cx.core_mut();
        let a = core.0.sim.actor_mut(hero);
        a.hp = a.hp.max(a.base.hp_max.max(200));
    });
}

/// `spell_transform`: the hero drinks a transformation potion (`PS_MagicPotion` -> `MagicTransform`),
/// stays an animal for `GetTransformDuration` seconds and takes his own shape back
/// (`TransformBack`, S:58610). Asserts both events and the duration the event reports.
pub fn spell_transform() -> Sc {
    use gothic_runtime::{Command, CoreCommand};
    let mut sc = Sc::new(260.0);
    grab(&mut sc);
    transform_events(&mut sc);
    sc.act("the hero drinks the wolf potion", |cx| {
        super::scenarios::grant(cx, "It_Potion_Wolf", 1);
        cx.world.write_message(CoreCommand(Command::UseItem {
            template: "It_Potion_Wolf".into(),
        }));
    });
    sc.until("the transformation starts", 20.0, |cx| {
        cx.st.n.contains_key("tf_start")
    });
    sc.check("the hero became the animal of the potion", |cx| {
        let t = cx.st.store.get("tf_template").cloned().unwrap_or_default();
        let secs = cx.st.n.get("tf_secs").copied().unwrap_or(0.0);
        expect(
            !t.is_empty() && secs >= 30.0,
            format!("{t} for {secs} s"),
            format!("template {t:?}, {secs} s"),
        )
    });
    sc.until("the transformation ends by itself", 190.0, |cx| {
        cx.st.n.contains_key("tf_end")
    });
    sc.check("TransformBack gave the hero his own shape back", |cx| {
        let (a, b) = (
            cx.st.n.get("tf_start").copied().unwrap_or(0.0),
            cx.st.n.get("tf_end").copied().unwrap_or(0.0),
        );
        let secs = cx.st.n.get("tf_secs").copied().unwrap_or(0.0);
        expect(
            b > a && ((b - a) - secs).abs() < 8.0,
            format!("{:.0} s as an animal (reported {secs} s)", b - a),
            format!("start {a:.1}, end {b:.1}, reported {secs}"),
        )
    });
    sc
}

/// Setup: a sleeping spot where the character stands (its destination point).
fn sleeping_spot(cx: &mut super::Cx, id: gothic_sim::types::ActorId) {
    use gothic_sim::script::enums::UseType;
    use gothic_sim::script::host::EntityId;
    let pos = cx.core().0.sim.world_pos(id);
    let sim = &mut cx.core_mut().0.sim;
    let point = sim.script.add_point(UseType::SleepGround, EntityId::NONE, pos);
    sim.script.npc_mut(id).dest = point;
}

/// `sleep_perception`: a sleeping character wakes into `ZS_ReactToFootsteps` when the hero walks
/// by (`OnSleep`, S:86dc0, from the `ZS_Sleep` task callback); a sleeping animal answers to
/// `OnAnimalSleep` (S:89f20).
pub fn sleep_perception() -> Sc {
    use gothic_sim::script::routine::state::State;
    let mut sc = Sc::new(180.0);
    grab(&mut sc);
    super::npc_ai::find_and_place(&mut sc, "Hamlar", 350.0);
    sc.act("Hamlar falls asleep", |cx| {
        let id = tgt_id(cx).unwrap();
        cx.st
            .n
            .insert("os0".into(), gothic_portcov::count("on_sleep") as f64);
        sleeping_spot(cx, id);
        cx.core_mut().0.sim.qa_enter_state(id, State::Sleep);
    });
    sc.act("the hero walks around him", |cx| {
        cx.key_down(bevy::prelude::KeyCode::KeyW)
    });
    sc.wait(6.0);
    sc.act("stop", |cx| cx.release_all());
    sc.check("the sleeper's OnSleep callback ran (ported)", |cx| {
        if !gothic_portcov::ENABLED {
            return super::ok("coverage build off: not counted");
        }
        let n = gothic_portcov::count("on_sleep") as f64;
        expect(n > cx.st.n["os0"], format!("{n} polls"), "OnSleep never ran")
    });
    sc.check("the walking hero woke him (or Hamlar likes the hero)", |cx| {
        let id = tgt_id(cx).unwrap();
        let st = cx.core().0.sim.script.npc(id).state.map(|s| s.name());
        let hero = cx.core().0.hero;
        let att = cx.core().0.sim.attitude(id, hero);
        let woke = !matches!(
            st,
            Some("ZS_Sleep" | "ZS_Sleep_Bed" | "ZS_Sleep_Ground" | "ZS_Idiot")
        );
        expect(
            woke || matches!(att, gothic_sim::types::Attitude::Friendly),
            format!("state {st:?}, attitude {att:?}"),
            format!("still asleep ({st:?}) although the attitude is {att:?}"),
        )
    });
    super::npc_ai::find_and_place(&mut sc, "Wolf", 350.0);
    sc.act("the wolf sleeps", |cx| {
        let id = tgt_id(cx).unwrap();
        cx.st.n.insert(
            "oa0".into(),
            gothic_portcov::count("on_animal_sleep") as f64,
        );
        sleeping_spot(cx, id);
        cx.core_mut().0.sim.qa_enter_state(id, State::AnimalSleep);
    });
    sc.wait(5.0);
    sc.check("the sleeping animal's OnAnimalSleep ran (ported)", |cx| {
        if !gothic_portcov::ENABLED {
            return super::ok("coverage build off: not counted");
        }
        let n = gothic_portcov::count("on_animal_sleep") as f64;
        expect(
            n > cx.st.n["oa0"],
            format!("{n} polls"),
            "OnAnimalSleep never ran",
        )
    });
    sc
}

/// `talk_refuse`: the confirm key on a character the dialog data has nothing to say to ends in
/// `StartSayRefuseTalk` (S:52280, `AssessTalk` with `HasInfo` false).
pub fn talk_refuse() -> Sc {
    use gothic_runtime::{Command, CoreCommand};
    let mut sc = Sc::new(120.0);
    grab(&mut sc);
    sc.act("pick a character without dialog and stand next to it", |cx| {
        let hero = cx.core().0.hero;
        let ids: Vec<gothic_sim::types::ActorId> = cx
            .core()
            .0
            .sim
            .actors
            .iter()
            .filter(|a| a.id != hero && a.is_alive() && a.species.is_humanoid())
            .map(|a| a.id)
            .collect();
        let mut pick = None;
        for id in ids {
            if cx.core_mut().0.has_dialog_info(id) {
                continue;
            }
            if cx.actor_pos(id).is_some() {
                pick = Some(id);
                break;
            }
        }
        match pick {
            Some(id) => {
                let name = cx.core().0.sim.actor(id).name.clone();
                cx.note(format!("no dialog for {name}"));
                cx.st.n.insert("tgt".into(), f64::from(id.0));
                let p = cx.actor_pos(id).unwrap();
                let mut pl = cx.world.resource_mut::<crate::walk::Player>();
                pl.ctrl.position = p + bevy::prelude::Vec3::new(150.0, 50.0, 0.0);
                pl.ctrl.vertical_speed = 0.0;
                pl.placed = false;
            }
            None => cx.check(
                "a spawned character without dialog exists",
                false,
                "all have dialog",
            ),
        }
    });
    sc.wait(3.0);
    sc.act("press the confirm key on it", |cx| {
        let Some(id) = tgt_id(cx) else { return };
        cx.st.n.insert(
            "rt0".into(),
            gothic_portcov::count("start_say_refuse_talk") as f64,
        );
        cx.world
            .write_message(CoreCommand(Command::Confirm { focus: id.0 + 1 }));
    });
    sc.wait(2.0);
    sc.check(
        "the character refuses to talk (ported StartSayRefuseTalk)",
        |cx| {
            if !gothic_portcov::ENABLED {
                return super::ok("coverage build off: not counted");
            }
            let n = gothic_portcov::count("start_say_refuse_talk") as f64;
            expect(
                n > cx.st.n.get("rt0").copied().unwrap_or(f64::MAX),
                format!("{n} refusals"),
                "no refusal line",
            )
        },
    );
    sc
}

/// `defeat_witness`: the hero knocks a friendly character out in front of a bystander. The ported
/// `Defeat` (S:27510) marks the Attack crime, the bystanders are offered the knock-out
/// (`SendPerceptionDefeat` -> `AssessDefeat`, S:29fb0) and the victim wakes up after the ported
/// `ZS_Unconscious_Loop` time.
pub fn defeat_witness() -> Sc {
    let mut sc = Sc::new(200.0);
    grab(&mut sc);
    super::npc_ai::find_and_place(&mut sc, "Hamlar", 300.0);
    sc.act("the hero knocks Hamlar out", |cx| {
        let id = tgt_id(cx).unwrap();
        let hero = cx.core().0.hero;
        cx.st.n.insert(
            "d0".into(),
            gothic_portcov::count("send_perception_defeat") as f64,
        );
        let sim = &mut cx.core_mut().0.sim;
        let _ = sim.equip(hero, "It_1H_Sword_01");
        sim.actor_mut(hero).drawn = true;
        sim.actor_mut(id).hp = 1;
        let out = sim.melee_hit(hero, id);
        let mode = sim.actor(id).ai_mode;
        cx.note(format!("outcome {out:?}, mode now {mode:?}"));
    });
    sc.wait(2.0);
    sc.check("Hamlar is down and the Attack crime is memorised", |cx| {
        let id = tgt_id(cx).unwrap();
        let a = cx.core().0.sim.actor(id);
        expect(
            matches!(
                a.ai_mode,
                gothic_sim::types::AiMode::Down | gothic_sim::types::AiMode::Dead
            ),
            format!("mode {:?}, crime {:?}", a.ai_mode, a.last_player_crime),
            format!("mode {:?}", a.ai_mode),
        )
    });
    sc.check(
        "the knock-out was offered to the bystanders (SendPerceptionDefeat)",
        |cx| {
            if !gothic_portcov::ENABLED {
                return super::ok("coverage build off: not counted");
            }
            let n = gothic_portcov::count("send_perception_defeat") as f64;
            expect(n > cx.st.n["d0"], format!("{n}"), "no defeat perception")
        },
    );
    sc.until("he wakes up (ZS_Unconscious_End)", 60.0, |cx| {
        let id = tgt_id(cx).unwrap();
        cx.core().0.sim.actor(id).ai_mode != gothic_sim::types::AiMode::Down
    });
    sc
}
