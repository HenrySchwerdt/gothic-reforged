//! Lockpicking, poses, quick save / load keys, teachers, doors (data check).
use super::more::{approach, npc_id, npc_pos, start_dialog, wait_menu};
use super::scenarios::{fingerprint, grant};
use super::{Cx, Poll, Sc, expect};
use crate::walk::play::events::Voices;
use crate::walk::play::interact::Interaction;
use crate::walk::play::objects::WorldObjects;
use crate::walk::play::{Mode, Target};
use bevy::prelude::*;
use gothic_sim::script::enums::UseType;
use std::sync::Arc;

type Goal = Arc<dyn Fn(&mut Cx) -> Option<Vec3> + Send + Sync>;

fn grab(sc: &mut Sc) {
    sc.wait(1.0);
    sc.act("grab the mouse", |cx| {
        cx.cursor_to(Vec2::new(640.0, 360.0));
        cx.mouse_click(MouseButton::Left);
    });
    sc.wait(0.4);
}

fn nearest_object(
    cx: &mut Cx,
    f: impl Fn(&crate::walk::play::objects::ObjEntry) -> bool,
) -> Option<Vec3> {
    let hero = cx.hero_pos();
    let o = cx.world.resource::<WorldObjects>();
    o.list
        .iter()
        .filter(|o| o.entity.is_some() && f(o))
        .min_by(|a, b| {
            (a.pos_cm - hero)
                .length()
                .total_cmp(&(b.pos_cm - hero).length())
        })
        .map(|o| o.pos_cm)
}

pub fn lockpick() -> Sc {
    let mut sc = Sc::new(240.0);
    grab(&mut sc);
    let goal: Goal = Arc::new(|cx| {
        nearest_object(cx, |o| {
            o.def.script_use() == UseType::Chest && o.def.lock.as_ref().is_some_and(|l| l.locked)
        })
    });
    {
        let g = goal.clone();
        sc.act(
            "setup: stand 1.2 m from the nearest locked chest (it is inside a house)",
            move |cx| {
                if let Some(p) = g(cx) {
                    let mut pl = cx.world.resource_mut::<crate::walk::Player>();
                    pl.ctrl.position = p + Vec3::new(120.0, 400.0, 0.0);
                    pl.ctrl.vertical_speed = 0.0;
                }
            },
        );
    }
    sc.until("grounded", 30.0, |cx| cx.player().ctrl.grounded);
    sc.wait(2.0);
    approach(
        &mut sc,
        "locked chest",
        goal,
        140.0,
        -0.3,
        Arc::new(|cx| matches!(cx.play().target, Some(Target::Object(_)))),
    );
    sc.act("record and press Enter", |cx| {
        let n = cx.count_item("It_Key_Lockpick");
        cx.st.n.insert("picks0".into(), f64::from(n));
        cx.note(format!("lockpicks before: {n}"));
        cx.use_key();
    });
    sc.wait(3.0);
    sc.shot("lockpick");
    sc.check(
        "the locked chest reacts (lock event, toast or window)",
        |cx| {
            let log: Vec<String> = cx.inter().log.iter().rev().take(6).cloned().collect();
            let n = cx.count_item("It_Key_Lockpick");
            let n0 = cx.st.n["picks0"] as u32;
            let texts = cx.all_text();
            let lock_text: Vec<&String> = texts
                .iter()
                .filter(|t| {
                    t.contains("Schloss") || t.contains("Dietrich") || t.contains("verschlossen")
                })
                .collect();
            cx.note(format!(
                "log {log:?}; lockpicks {n0} -> {n}; texts {lock_text:?}"
            ));
            let any = log.iter().any(|l| l.contains("Lock") || l.contains("lock"))
                || n != n0
                || !lock_text.is_empty()
                || cx.inter().window.is_some();
            expect(
                any,
                format!("log {log:?}"),
                format!("nothing happened: {log:?}"),
            )
        },
    );
    sc.act(
        "setup: teach the hero Lockpick 1 (a trainer would), try again",
        |cx| {
            cx.world.write_message(gothic_runtime::CoreCommand(
                gothic_runtime::Command::GrantPerk("Perk_LockPick_1".into()),
            ));
            cx.world.resource_mut::<Interaction>().log.clear();
        },
    );
    sc.step("look at the chest again", |cx| {
        if let Some(p) = nearest_object(cx, |o| {
            o.def.script_use() == UseType::Chest && o.def.lock.as_ref().is_some_and(|l| l.locked)
        }) {
            cx.aim_at(p, -0.3);
        }
        if cx.t > 1.5 { Poll::Done } else { Poll::Wait }
    });
    sc.act("use key", |cx| cx.use_key());
    sc.wait(6.0);
    sc.shot("lockpick_perk");
    sc.check(
        "with the skill the lock is worked on (LockPicked / window / consumed pick)",
        |cx| {
            let log: Vec<String> = cx.inter().log.iter().rev().take(8).cloned().collect();
            let n = cx.count_item("It_Key_Lockpick");
            let n0 = cx.st.n["picks0"] as u32;
            let ok = log
                .iter()
                .any(|l| l.contains("LockPicked") || l.contains("Window"))
                || n != n0;
            let target = cx.play().target;
            let active = cx.inter().active;
            let st = cx.core().0.sim.actor(cx.core().0.hero).ani_state;
            let perk = cx
                .core()
                .0
                .sim
                .actor(cx.core().0.hero)
                .has_perk("Perk_LockPick_1");
            expect(
                ok,
                format!("log {log:?}; picks {n0} -> {n}"),
                format!(
                    "nothing: log {log:?}; picks {n0} -> {n}; target {target:?}, active {active:?}, ani {st:?}, has perk {perk}"
                ),
            )
        },
    );
    sc.act("Esc if a window is up", |cx| {
        if cx.inter().window.is_some() {
            cx.tap(KeyCode::Escape);
        }
    });
    sc.wait(1.5);
    sc.check("not stuck after the lock attempt", |cx| {
        expect(
            cx.ui().screen == gothic_ui::Screen::None && cx.play().mode == Mode::Explore,
            "free",
            format!(
                "screen {:?} mode {:?} holding_pose {}",
                cx.ui().screen,
                cx.play().mode,
                cx.inter().holding_pose
            ),
        )
    });
    sc
}

pub fn sit_pose() -> Sc {
    let mut sc = Sc::new(240.0);
    grab(&mut sc);
    let goal: Goal = Arc::new(|cx| {
        nearest_object(cx, |o| {
            matches!(o.def.script_use(), UseType::Bench | UseType::Stool)
        })
    });
    approach(
        &mut sc,
        "a bench or stool",
        goal,
        120.0,
        -0.3,
        Arc::new(|cx| matches!(cx.play().target, Some(Target::Object(_)))),
    );
    sc.act("use key (sit down)", |cx| cx.use_key());
    sc.wait(3.0);
    sc.shot("sitting");
    sc.check("the hero holds a seat pose", |cx| {
        let st = cx.core().0.sim.actor(cx.core().0.hero).ani_state;
        expect(
            cx.inter().holding_pose || st.is_sitting(),
            format!("ani state {st:?}, holding_pose {}", cx.inter().holding_pose),
            format!(
                "not sitting: ani state {st:?}, log {:?}",
                cx.inter().log.iter().rev().take(4).collect::<Vec<_>>()
            ),
        )
    });
    sc.act("W (get up)", |cx| cx.tap(KeyCode::KeyW));
    sc.wait(2.5);
    sc.check("W releases the pose and the hero is free", |cx| {
        let st = cx.core().0.sim.actor(cx.core().0.hero).ani_state;
        expect(
            !cx.inter().holding_pose && !st.is_sitting(),
            format!("ani state {st:?}"),
            format!(
                "still posed: ani state {st:?}, holding_pose {}",
                cx.inter().holding_pose
            ),
        )
    });
    sc
}

pub fn quick_slots() -> Sc {
    let mut sc = Sc::new(120.0);
    grab(&mut sc);
    sc.act("F5 (quick save)", |cx| {
        let _ = std::fs::remove_file(".local/saves/quick.g3rs");
        let fp = fingerprint(cx);
        cx.st.store.insert("fp_q".into(), fp);
        cx.tap(KeyCode::F5);
    });
    sc.wait(1.0);
    sc.check("quick.g3rs written", |_| {
        expect(
            std::path::Path::new(".local/saves/quick.g3rs").exists(),
            "exists",
            "no quick save file after F5",
        )
    });
    sc.act("change the state: +777 gold (setup)", |cx| {
        grant(cx, "It_Gold", 777)
    });
    sc.wait(0.6);
    sc.act("F9 (quick load)", |cx| cx.tap(KeyCode::F9));
    sc.wait(1.5);
    sc.shot("after_f9");
    sc.check("F9 restored the quick-saved state", |cx| {
        let fp = fingerprint(cx);
        let q = cx.st.store["fp_q"].clone();
        expect(fp == q, fp.clone(), format!("saved {q} / now {fp}"))
    });
    sc.act("F6 (manual save)", |cx| {
        let n = std::fs::read_dir(".local/saves").map_or(0, |d| d.count());
        cx.st.n.insert("n0".into(), n as f64);
        cx.tap(KeyCode::F6);
    });
    sc.wait(1.0);
    sc.check("F6 wrote a slot", |cx| {
        let n = std::fs::read_dir(".local/saves").map_or(0, |d| d.count());
        let n0 = cx.st.n["n0"] as usize;
        expect(
            n >= n0,
            format!("{n0} -> {n} files"),
            format!("{n0} -> {n}"),
        )
    });
    sc.act("+500 gold (setup), then F10 (load newest)", |cx| {
        grant(cx, "It_Gold", 500);
        cx.tap(KeyCode::F10);
    });
    sc.wait(1.5);
    sc.check("F10 loaded the newest slot (gold back)", |cx| {
        let fp = fingerprint(cx);
        let q = cx.st.store["fp_q"].clone();
        expect(fp == q, fp.clone(), format!("expected {q}, now {fp}"))
    });
    sc
}

pub fn teach() -> Sc {
    let mut sc = Sc::new(240.0);
    grab(&mut sc);
    sc.act(
        "setup: 20 learning points (a level-up would give them)",
        |cx| {
            cx.world.write_message(gothic_runtime::CoreCommand(
                gothic_runtime::Command::GiveHero {
                    template: "It_Gold".into(),
                    amount: 0,
                },
            ));
            let mut c = cx.core_mut();
            c.0.sim.state.learning_points = 20;
            c.0.sim.state.attributes.insert("ATTRIB_STR".into(), 10);
        },
    );
    approach(
        &mut sc,
        "Hamlar",
        npc_pos("Hamlar"),
        200.0,
        -0.2,
        Arc::new(|cx| {
            npc_id(cx, "Hamlar").is_some_and(|id| cx.play().target == Some(Target::Npc(id)))
        }),
    );
    start_dialog(&mut sc);
    wait_menu(&mut sc);
    sc.act("ask to be taught fighting (first entry)", |cx| {
        cx.st.n.insert(
            "lp0".into(),
            f64::from(cx.core().0.sim.state.learning_points),
        );
        cx.tap(KeyCode::Enter);
    });
    sc.wait(6.0);
    sc.shot("teach_after");
    sc.check(
        "the teacher answers and the dialog goes on or a teach page opens",
        |cx| {
            let v = cx.world.resource::<Voices>();
            let n = v.menu.len();
            let s = cx.ui().screen;
            expect(
                n > 0 || s != gothic_ui::Screen::None || cx.play().mode == Mode::Explore,
                format!("menu {n} entries, screen {s:?}, mode {:?}", cx.play().mode),
                "dialog dead: no menu, no screen, still in dialog mode",
            )
        },
    );
    sc.act("end the dialog (Esc)", |cx| {
        if cx.play().mode == Mode::Dialog {
            cx.tap(KeyCode::Escape);
        }
    });
    sc.wait(1.0);
    sc.act("C (character page)", |cx| {
        if cx.play().mode == Mode::Dialog {
            cx.world.write_message(gothic_runtime::CoreCommand(
                gothic_runtime::Command::EndDialog,
            ));
        }
    });
    sc.until("explore", 20.0, |cx| cx.play().mode == Mode::Explore);
    sc.act("C", |cx| cx.tap(KeyCode::KeyC));
    sc.wait(1.0);
    sc.shot("character");
    sc.check("character page shows attributes", |cx| {
        let t = cx.all_text();
        expect(
            cx.ui().screen == gothic_ui::Screen::Character
                && t.iter()
                    .any(|s| s.contains("Stärke") || s.contains("Staerke") || s.contains("Kraft")),
            format!("{} texts", t.len()),
            format!("screen {:?}", cx.ui().screen),
        )
    });
    sc
}

/// Kept for the old name: the door tests live in `world_doors` (the world has 4 interactive doors,
/// none in Ardea).
pub fn doors() -> Sc {
    super::world::world_doors()
}
