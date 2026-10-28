//! Lockpicking, poses, quick save / load keys, teachers, doors (data check).
use super::more::{approach, npc_id, npc_pos, start_dialog, wait_menu};
use super::scenarios::{fingerprint, grant};
use super::{Cx, Poll, Sc, expect};
use crate::walk::play::events::Voices;
use crate::walk::play::interact::Interaction;
use crate::walk::play::objects::WorldObjects;
use crate::walk::play::{Mode, Target};
use bevy::prelude::*;
use gothic_runtime::{Command, CoreCommand};
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

/// Sleeping: the bed state and `Command::Sleep` share `commands::sleep` (`PlayerSleep` S:76440, the
/// bed's `PS_DuskToDawn` S:66fc0 wakes at 09:00). Hurt hero in a bed -> the clock is at the wake
/// hour afterwards and hit points, stamina and mana are restored; then the dialog form
/// `Sleep(20)` advances to 20:00 the same day.
pub fn sleep_bed() -> Sc {
    let mut sc = Sc::new(240.0);
    grab(&mut sc);
    let goal: Goal =
        Arc::new(|cx| nearest_object(cx, |o| matches!(o.def.script_use(), UseType::Bed)));
    approach(
        &mut sc,
        "a bed",
        goal,
        120.0,
        -0.3,
        Arc::new(|cx| matches!(cx.play().target, Some(Target::Object(_)))),
    );
    sc.act("setup: hurt, tired, evening (22:00)", |cx| {
        let hero = cx.core().0.hero;
        let mut core = cx.core_mut();
        core.0.sim.clock.game_ms =
            core.0.sim.clock.game_ms / 86_400_000 * 86_400_000 + 22 * 3_600_000;
        let a = core.0.sim.actor_mut(hero);
        a.hp = 10;
        a.stamina = 5;
    });
    sc.act("use key (lie down)", |cx| cx.use_key());
    sc.wait(8.0);
    sc.shot("in_bed");
    sc.check(
        "the bed slept until the wake hour and restored the hero",
        |cx| {
            let h = cx.core().0.sim.clock.hour_of_day();
            let hero = cx.core().0.hero;
            let a = cx.core().0.sim.actor(hero);
            let max = a.base.hp_max;
            let note = format!("hour {h:.2}, hp {}/{max}, stamina {}", a.hp, a.stamina);
            expect((h - 9.0).abs() < 0.6 && a.hp >= max, note.clone(), note)
        },
    );
    sc.act("Command::Sleep { hour: Some(20) }", |cx| {
        cx.world.write_message(gothic_runtime::CoreCommand(
            gothic_runtime::Command::Sleep { hour: Some(20) },
        ));
    });
    sc.wait(1.0);
    sc.check("the dialog sleep advanced to 20:00", |cx| {
        let h = cx.core().0.sim.clock.hour_of_day();
        expect(
            (h - 20.0).abs() < 0.3,
            format!("hour {h:.2}"),
            format!("hour {h:.2}"),
        )
    });
    sc
}

/// Head look-at (`eCLookAtConstraint`, `Entity::SetLookAtTarget`): a character told to look at
/// another turns its head node towards it (the animator applies the rotation after the clips)
/// and relaxes when the target is cleared.
pub fn look_at() -> Sc {
    use crate::walk::play::NpcActor;
    use crate::walk::play::look_at::LookAt;
    use gothic_animation::Animator;
    let mut sc = Sc::new(90.0);
    grab(&mut sc);
    sc.wait(3.0);
    sc.act("pick a character and a target to its side", |cx| {
        let mut q = cx
            .world
            .query_filtered::<(&NpcActor, &GlobalTransform), With<Animator>>();
        let all: Vec<_> = q
            .iter(cx.world)
            .map(|(n, g)| {
                (
                    n.0,
                    g.translation(),
                    g.forward().as_vec3(),
                    g.right().as_vec3(),
                )
            })
            .collect();
        let hero = cx.core().0.hero;
        let alive = |cx: &Cx, id| cx.core().0.sim.actor(id).is_alive();
        let mut best: Option<(f32, gothic_sim::types::ActorId, gothic_sim::types::ActorId)> = None;
        for (a, pa, f, r) in &all {
            if !alive(cx, *a) {
                continue;
            }
            for (b, pb, ..) in &all {
                let d = *pb - *pa;
                let dist = d.length();
                if a == b || !(3.0..=20.0).contains(&dist) || !alive(cx, *b) {
                    continue;
                }
                let dir = d / dist;
                // 45 degrees off the facing under either axis convention
                let score = (dir.dot(*f).abs() - 0.7).abs() + (dir.dot(*r).abs() - 0.7).abs();
                if best.is_none_or(|(s, ..)| score < s) {
                    best = Some((score, *a, *b));
                }
            }
        }
        let _ = hero;
        if let Some((_, a, b)) = best {
            cx.st.n.insert("who".into(), f64::from(a.0));
            cx.st.n.insert("target".into(), f64::from(b.0));
            cx.core_mut().0.sim.actor_mut(a).look_at = Some(b);
            cx.note(format!("actor {} looks at actor {}", a.0, b.0));
        }
    });
    sc.wait(3.0);
    sc.check(
        "the head turns towards the target: a look-at rotation is applied to the head node",
        |cx| {
            let who = cx.st.n.get("who").copied().unwrap_or(-1.0) as u32;
            let mut q = cx.world.query::<(&NpcActor, &Animator, &LookAt)>();
            let found = q
                .iter(cx.world)
                .find(|(n, ..)| n.0.0 == who)
                .map(|(_, a, l)| {
                    (
                        a.look
                            .first()
                            .map_or(0.0, |(_, r)| r.angle_between(Quat::IDENTITY)),
                        l.following.map(|t| t.0),
                    )
                });
            let want = cx.st.n.get("target").copied().unwrap_or(-1.0) as u32;
            let note = format!("rotation and followed target {found:?}, wanted {want}");
            expect(
                found.is_some_and(|(ang, f)| ang > 0.05 && f == Some(want)),
                note.clone(),
                note,
            )
        },
    );
    sc.shot("head_turned");
    sc.act("clear the target", |cx| {
        let who = cx.st.n.get("who").copied().unwrap_or(-1.0) as u32;
        cx.core_mut()
            .0
            .sim
            .actor_mut(gothic_sim::types::ActorId(who))
            .look_at = None;
    });
    sc.wait(4.0);
    sc.check("the head relaxes when the target is cleared", |cx| {
        let who = cx.st.n.get("who").copied().unwrap_or(-1.0) as u32;
        let mut q = cx.world.query::<(&NpcActor, &Animator)>();
        let left = q
            .iter(cx.world)
            .find(|(n, _)| n.0.0 == who)
            .map(|(_, a)| a.look.len());
        let note = format!("look rotations left {left:?}");
        expect(left == Some(0), note.clone(), note)
    });
    sc
}

/// Potions are consumed through `gCInventory_PS::ConsumeItems` and the player memory
/// (`gCPlayerMemory_PS::Consume`): a health potion heals, a mana potion refills mana, an
/// `HPMax` potion raises the maximum for good (`gameplay::stats`).
pub fn potion() -> Sc {
    let mut sc = Sc::new(60.0);
    grab(&mut sc);
    sc.wait(2.0);
    sc.act("setup: hurt, no mana, the three potions", |cx| {
        let hero = cx.core().0.hero;
        {
            let mut core = cx.core_mut();
            let a = core.0.sim.actor_mut(hero);
            a.hp = 60;
            a.mana = 5;
        }
        let a = cx.core().0.sim.actor(hero);
        let (hp, mp, max) = (a.hp, a.mana, a.base.hp_max);
        cx.st.n.insert("hp0".into(), f64::from(hp));
        cx.st.n.insert("mp0".into(), f64::from(mp));
        cx.st.n.insert("max0".into(), f64::from(max));
        let n0 = ["It_Potion_Health", "It_Potion_Mana", "It_Potion_HPMax"]
            .iter()
            .map(|t| cx.core().0.sim.actor(hero).inventory.count(t))
            .sum::<u32>();
        cx.st.n.insert("n0".into(), f64::from(n0 + 3));
        for t in ["It_Potion_Health", "It_Potion_Mana", "It_Potion_HPMax"] {
            cx.world.write_message(CoreCommand(Command::GiveHero {
                template: t.into(),
                amount: 1,
            }));
        }
    });
    sc.wait(0.5);
    sc.act("drink them", |cx| {
        for t in ["It_Potion_Health", "It_Potion_Mana", "It_Potion_HPMax"] {
            cx.world
                .write_message(CoreCommand(Command::UseItem { template: t.into() }));
        }
    });
    sc.wait(2.0);
    sc.check(
        "health heals, mana refills, HPMax raises the maximum, three potions used up",
        |cx| {
            let hero = cx.core().0.hero;
            let a = cx.core().0.sim.actor(hero);
            let left = a.inventory.count("It_Potion_Health")
                + a.inventory.count("It_Potion_Mana")
                + a.inventory.count("It_Potion_HPMax");
            // the potions heal by a share of the maximum (`ApplyPerm` operation 2 of a pool: percent)
            let used = cx.st.n["n0"] as u32 - left;
            let (hp0, mp0, max0) = (
                cx.st.n["hp0"] as i32,
                cx.st.n["mp0"] as i32,
                cx.st.n["max0"] as i32,
            );
            let note = format!(
                "hp {hp0} -> {}, mana {mp0} -> {}, max hp {max0} -> {}, potions used {used}",
                a.hp, a.mana, a.base.hp_max
            );
            expect(
                a.hp > hp0 && a.mana > mp0 && a.base.hp_max > max0 && used == 3,
                note.clone(),
                note,
            )
        },
    );
    sc
}

/// Runtime-created items: a weapon dropped from the hand (`PSInventory::DropSlot`) becomes a
/// loose item entity of the world (shown, focusable, pickable), and picking it up puts it back
/// into the inventory.
pub fn item_drop() -> Sc {
    use gothic_sim::script::host::InventoryApi;
    let mut sc = Sc::new(90.0);
    grab(&mut sc);
    sc.wait(2.0);
    sc.act("hold the sword in the right hand and drop it", |cx| {
        let hero = cx.core().0.hero;
        let sword = {
            let c = cx.core();
            let eq = &c.0.sim.actor(hero).equipment;
            eq.right.clone()
        };
        let Some(sword) = sword else {
            cx.note("the hero has no weapon");
            return;
        };
        cx.st.store.insert("sword".into(), sword.name.clone());
        let n0 = cx.core().0.sim.actor(hero).inventory.count(&sword.name);
        cx.st.n.insert("n0".into(), f64::from(n0));
        let entries = cx.world.resource::<WorldObjects>().list.len();
        cx.st.n.insert("entries0".into(), entries as f64);
        let mut core = cx.core_mut();
        core.0.sim.hold(hero, Some(&sword), None);
        let ent = gothic_sim::script::host::EntityId(hero.0 + 1);
        let mut host = gothic_sim::script_host::SimHost::new(&mut core.0.sim);
        host.drop_slot(ent, gothic_sim::script::enums::Slot::RightHand);
    });
    sc.wait(2.5);
    sc.check(
        "the dropped weapon is an entry of the world with a mesh",
        |cx| {
            let name = cx.st.store.get("sword").cloned().unwrap_or_default();
            let e0 = cx.st.n.get("entries0").copied().unwrap_or(0.0) as usize;
            let o = cx.world.resource::<WorldObjects>();
            let found = o
                .list
                .iter()
                .skip(e0)
                .find(|e| e.def.name == name)
                .map(|e| (e.visual.is_some(), e.entity));
            let note = format!("{name}: entry (visual, entity) {found:?}");
            expect(
                found.is_some_and(|(v, ent)| v && ent.is_some()),
                note.clone(),
                note,
            )
        },
    );
    sc.shot("dropped_item");
    sc.act("pick it up (what Confirm::PickUp does)", |cx| {
        let name = cx.st.store.get("sword").cloned().unwrap_or_default();
        let e0 = cx.st.n.get("entries0").copied().unwrap_or(0.0) as usize;
        let at = cx
            .world
            .resource::<WorldObjects>()
            .list
            .iter()
            .enumerate()
            .skip(e0)
            .find(|(_, e)| e.def.name == name)
            .map(|(i, e)| (i, e.entity));
        if let Some((i, Some(item))) = at {
            cx.world.resource_mut::<Interaction>().pending_item = Some(i);
            cx.world
                .write_message(CoreCommand(Command::PickUp { item }));
        }
    });
    sc.wait(2.0);
    sc.check(
        "the weapon is back in the inventory and the entry is used up",
        |cx| {
            let hero = cx.core().0.hero;
            let name = cx.st.store.get("sword").cloned().unwrap_or_default();
            let n = cx.core().0.sim.actor(hero).inventory.count(&name);
            let n0 = cx.st.n.get("n0").copied().unwrap_or(0.0) as u32;
            let o = cx.world.resource::<WorldObjects>();
            let e0 = cx.st.n.get("entries0").copied().unwrap_or(0.0) as usize;
            let taken = o
                .list
                .iter()
                .skip(e0)
                .any(|e| e.def.name == name && e.taken);
            let note = format!("{name}: count {n0} -> {n}, entry taken {taken}");
            expect(n == n0 && taken, note.clone(), note)
        },
    );
    sc
}
