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
    sc.check("the taking was offered to the witnesses (ported SendPerceptionTakeItem)", |cx| {
        if !gothic_portcov::ENABLED {
            return super::ok("coverage build off: not counted");
        }
        let n = gothic_portcov::count("send_perception_take_item") as f64;
        expect(n > cx.st.n["take0"], format!("{n} offers"), "no witness was offered the theft")
    });
    sc
}
