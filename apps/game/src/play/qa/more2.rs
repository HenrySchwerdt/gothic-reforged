//! Combat, magic, bow, swimming and routine re-entry scenarios.
use super::more::{approach, npc_pos};
use super::{Cx, Poll, Sc, expect};
use crate::walk::play::fx::FxLog;
use bevy::prelude::*;
use gothic_sim::types::ActorId;
use std::sync::Arc;

type Goal = Arc<dyn Fn(&mut Cx) -> Option<Vec3> + Send + Sync>;

pub(super) fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// Nearest living start raider.
pub(super) fn raider(cx: &Cx) -> Option<ActorId> {
    let hero = cx.hero_pos();
    cx.core()
        .0
        .sim
        .actors
        .iter()
        .filter(|a| a.name.starts_with("Orc_GameStartRaider") && a.is_alive())
        .filter_map(|a| cx.actor_pos(a.id).map(|p| (flat(p - hero).length(), a.id)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|a| a.1)
}

pub(super) fn target_goal() -> Goal {
    Arc::new(|cx| {
        let id = ActorId(cx.st.n.get("tgt").copied()? as u32);
        cx.actor_pos(id)
    })
}

pub(super) fn pick_target(sc: &mut Sc) {
    sc.act("choose the nearest raider", |cx| {
        let Some(id) = raider(cx) else {
            cx.check("a raider exists", false, "none alive");
            return;
        };
        let hp = cx.core().0.sim.actor(id).hp;
        cx.st.n.insert("tgt".into(), f64::from(id.0));
        cx.st.n.insert("tgt_hp0".into(), f64::from(hp));
        cx.st.n.insert("hero_hp0".into(), f64::from(cx.hero_hp()));
        let name = cx.core().0.sim.actor(id).name.clone();
        cx.note(format!("target {name} hp {hp}"));
    });
}

pub(super) fn grab(sc: &mut Sc) {
    sc.wait(1.0);
    sc.act("grab the mouse with a click", |cx| {
        cx.cursor_to(Vec2::new(640.0, 360.0));
        cx.mouse_click(MouseButton::Left);
    });
    sc.wait(0.4);
}

pub(super) fn tgt_id(cx: &Cx) -> Option<ActorId> {
    cx.st.n.get("tgt").map(|v| ActorId(*v as u32))
}

pub fn combat_basic() -> Sc {
    let mut sc = Sc::new(300.0);
    grab(&mut sc);
    pick_target(&mut sc);
    sc.act("Space: draw the sword", |cx| cx.tap(KeyCode::Space));
    sc.wait(1.5);
    sc.check("the sword is drawn", |cx| {
        expect(cx.play().drawn, "drawn", "state.drawn is false after Space")
    });
    // Parry: hold the right button; double tap A is no dodge in the original.
    sc.act("hold right mouse (parry)", |cx| {
        if cx.play().hero_down {
            return;
        }
        cx.mouse_down(MouseButton::Right);
    });
    sc.wait(1.0);
    sc.act("release parry", |cx| cx.mouse_up(MouseButton::Right));
    sc.wait(0.6);
    sc.act("dodge: double tap A", |cx| {
        let p = cx.hero_pos();
        cx.st.n.insert("dodge_x".into(), f64::from(p.x));
        cx.st.n.insert("dodge_z".into(), f64::from(p.z));
        cx.tap(KeyCode::KeyA);
    });
    sc.wait(0.12);
    sc.act("second tap", |cx| cx.tap(KeyCode::KeyA));
    sc.wait(1.2);
    sc.check(
        "no double-tap dodge in the player layer (the hero strafes two short steps)",
        |cx| {
            let p = cx.hero_pos();
            let d = Vec2::new(
                p.x - cx.st.n["dodge_x"] as f32,
                p.z - cx.st.n["dodge_z"] as f32,
            )
            .length();
            expect(
                d < 260.0,
                format!("moved {d:.0} cm"),
                format!("{d:.0} cm after two taps: a dodge slide ran"),
            )
        },
    );
    approach(
        &mut sc,
        "the raider",
        target_goal(),
        230.0,
        -0.2,
        Arc::new(|_| true),
    );
    // Fight: face the target, close in, left click every second.
    sc.step("attack for up to 25 s", {
        let mut last = -10.0f32;
        move |cx| {
            let Some(id) = tgt_id(cx) else {
                return Poll::Fail("no target".into());
            };
            if cx.play().hero_down {
                cx.release_all();
                return Poll::Done;
            }
            let alive = cx.core().0.sim.actor(id).is_alive();
            let Some(p) = cx.actor_pos(id) else {
                return Poll::Done;
            };
            if !alive || cx.t > 25.0 {
                cx.release_all();
                return Poll::Done;
            }
            cx.aim_at(p, -0.2);
            let d = flat(p - cx.hero_pos()).length();
            if d > 165.0 {
                cx.key_down(KeyCode::KeyW);
            } else {
                cx.key_up(KeyCode::KeyW);
                if cx.now - last > 1.0 {
                    cx.mouse_click(MouseButton::Left);
                    last = cx.now;
                    let n = cx.st.n.entry("clicks".into()).or_insert(0.0);
                    *n += 1.0;
                }
            }
            Poll::Wait
        }
    });
    sc.shot("fight");
    sc.check(
        "the fight exchanged blows (a hp changed or the orc died)",
        |cx| {
            let id = tgt_id(cx).unwrap();
            let hp = cx.core().0.sim.actor(id).hp;
            let hp0 = cx.st.n["tgt_hp0"] as i32;
            let h0 = cx.st.n["hero_hp0"] as i32;
            let h = cx.hero_hp();
            let clicks = cx.st.n.get("clicks").copied().unwrap_or(0.0);
            let log = cx.world.resource::<crate::walk::play::events::CombatLog>();
            let (hn, hh) = (log.hits_on_npcs, log.hits_on_hero);
            cx.note(format!("combat log: {hn} hits on npcs, {hh} on the hero"));
            expect(
                hp < hp0 || h < h0,
                format!("orc hp {hp0} -> {hp}, hero hp {h0} -> {h}, {clicks} clicks"),
                format!(
                    "no hp change at all: orc {hp0} -> {hp}, hero {h0} -> {h}, {clicks} clicks"
                ),
            )
        },
    );
    sc.check("the hero's own clicks hurt the orc", |cx| {
        let id = tgt_id(cx).unwrap();
        let hp = cx.core().0.sim.actor(id).hp;
        let hp0 = cx.st.n["tgt_hp0"] as i32;
        expect(
            hp < hp0,
            format!("orc hp {hp0} -> {hp}"),
            format!("orc untouched ({hp0})"),
        )
    });
    sc.act("holster", |cx| {
        if cx.play().drawn {
            cx.tap(KeyCode::Space);
        }
    });
    sc.wait(1.5);
    sc.check("not stuck: explore mode, no window or screen", |cx| {
        expect(
            cx.play().mode == crate::walk::play::Mode::Explore
                && cx.ui().screen == gothic_ui::Screen::None,
            "free",
            "stuck in a mode or screen",
        )
    });
    sc
}

pub fn magic_bow() -> Sc {
    super::ranged::magic_bow()
}

pub fn swim() -> Sc {
    let mut sc = Sc::new(180.0);
    grab(&mut sc);
    sc.act(
        "setup: teleport to the slope above the south coast of Ardea (play space)",
        |cx| {
            let mut p = cx.world.resource_mut::<crate::walk::Player>();
            p.ctrl.position = Vec3::new(6400.0, 15_000.0, -12_500.0);
            p.ctrl.vertical_speed = 0.0;
            p.yaw = 0.0;
        },
    );
    sc.until("grounded at the beach", 40.0, |cx| {
        cx.player().ctrl.grounded
    });
    sc.act("note position", |cx| {
        let p = cx.player().ctrl.position;
        cx.note(format!("beach start {p:?}"));
    });
    sc.shot("beach");
    sc.step("walk south into the sea (W), up to 90 s", |cx| {
        let goal = Vec3::new(6400.0, 0.0, -21_000.0);
        cx.aim_at(goal, -0.2);
        cx.key_down(KeyCode::KeyW);
        let mode = cx.player().ctrl.machine.mode();
        if mode == gothic_physics::MovementMode::Swimming {
            cx.st.n.insert("swam".into(), 1.0);
            return Poll::Done;
        }
        if cx.t > 90.0 {
            cx.release_all();
            return Poll::Done;
        }
        Poll::Wait
    });
    sc.wait(2.0);
    sc.shot("swimming");
    sc.check("the hero swims in deep water", |cx| {
        let p = cx.player().ctrl.position;
        let m = cx.player().ctrl.machine.mode();
        expect(
            cx.st.n.contains_key("swam"),
            format!("swimming at {p:?}, mode now {m:?}"),
            format!("never swam; ended at {p:?} mode {m:?}"),
        )
    });
    sc.act("keep swimming 4 s", |cx| cx.key_down(KeyCode::KeyW));
    sc.wait(4.0);
    sc.check("hero alive and not stuck while swimming", |cx| {
        expect(
            !cx.play().hero_down && cx.hero_hp() > 0,
            format!("hp {}", cx.hero_hp()),
            "hero died or is down",
        )
    });
    sc.act("turn back (S) and leave the water", |cx| {
        cx.key_up(KeyCode::KeyW);
        cx.key_down(KeyCode::KeyS);
    });
    sc.step("until walking again, 30 s", |cx| {
        let m = cx.player().ctrl.machine.mode();
        if m != gothic_physics::MovementMode::Swimming && m != gothic_physics::MovementMode::Diving
        {
            cx.release_all();
            return Poll::Done;
        }
        if cx.t > 30.0 {
            cx.release_all();
            return Poll::Done;
        }
        Poll::Wait
    });
    sc.check("the hero can leave the water", |cx| {
        let m = cx.player().ctrl.machine.mode();
        expect(
            m != gothic_physics::MovementMode::Swimming
                && m != gothic_physics::MovementMode::Diving,
            format!("mode {m:?}"),
            format!("still {m:?}"),
        )
    });
    sc
}

pub fn routine_reentry() -> Sc {
    let mut sc = Sc::new(240.0);
    sc.wait(1.0);
    sc.act("record 8 routine characters near the start", |cx| {
        let hero = cx.hero_pos();
        let me = cx.core().0.hero;
        let mut v: Vec<(f32, ActorId, String, Vec3)> = cx
            .core()
            .0
            .sim
            .actors
            .iter()
            .filter(|a| {
                a.id != me
                    && a.is_alive()
                    && a.species == gothic_sim::types::Species::Human
                    && !a.routine_name.is_empty()
            })
            .filter_map(|a| {
                let p = cx.actor_pos(a.id)?;
                Some((flat(p - hero).length(), a.id, a.name.clone(), p))
            })
            .collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v.truncate(8);
        let mut s = String::new();
        for (_, id, name, p) in &v {
            s.push_str(&format!("{}:{}:{}:{}:{};", id.0, name, p.x, p.y, p.z));
        }
        cx.note(format!("tracking {} characters: {s}", v.len()));
        cx.st.store.insert("track".into(), s);
    });
    sc.act("record the game clock", |cx| {
        let h = cx.core().0.state().hours;
        cx.st.n.insert("hours0".into(), h as f64);
        cx.st.n.insert(
            "min0".into(),
            (cx.core().0.sim.clock.game_ms / 60_000) as f64,
        );
    });
    sc.act(
        "setup: teleport the hero far away (outside the processing range and the tile ring)",
        |cx| {
            let mut p = cx.world.resource_mut::<crate::walk::Player>();
            p.ctrl.position = Vec3::new(11_900.0, 20_000.0, -14_000.0);
            p.ctrl.vertical_speed = 0.0;
        },
    );
    sc.wait(25.0);
    sc.act("setup: teleport back to the start", |cx| {
        let mut p = cx.world.resource_mut::<crate::walk::Player>();
        p.ctrl.position = crate::walk::play::HERO_START + Vec3::Y * 300.0;
        p.ctrl.vertical_speed = 0.0;
    });
    sc.until("hero grounded again", 40.0, |cx| cx.player().ctrl.grounded);
    sc.wait(8.0);
    sc.shot("back");
    sc.check(
        "tracked characters are alive, in the world and on their feet",
        |cx| {
            let track = cx.st.store["track"].clone();
            let mut bad: Vec<String> = Vec::new();
            let mut moved = 0;
            let mut n = 0;
            for item in track.split(';').filter(|s| !s.is_empty()) {
                let f: Vec<&str> = item.split(':').collect();
                let id = ActorId(f[0].parse().unwrap_or(0));
                let p0 = Vec3::new(
                    f[2].parse().unwrap_or(0.0),
                    f[3].parse().unwrap_or(0.0),
                    f[4].parse().unwrap_or(0.0),
                );
                n += 1;
                let alive = cx.core().0.sim.actor(id).is_alive();
                let Some(p) = cx.actor_pos(id) else {
                    bad.push(format!("{} missing", f[1]));
                    continue;
                };
                if !alive {
                    bad.push(format!("{} died", f[1]));
                }
                if !p.is_finite() || (p.y - p0.y).abs() > 600.0 {
                    bad.push(format!("{} at {p:?} (was {p0:?})", f[1]));
                }
                if (p - p0).length() > 30.0 {
                    moved += 1;
                }
            }
            expect(
                bad.is_empty(),
                format!("{n} tracked, {moved} moved"),
                format!("{}", bad.join("; ")),
            )
        },
    );
    sc.act("note the game clock", |cx| {
        let h = cx.core().0.state().hours;
        let m = (cx.core().0.sim.clock.game_ms / 60_000) as f64;
        let (h0, m0) = (cx.st.n["hours0"], cx.st.n["min0"]);
        cx.note(format!(
            "game clock: hours {h0} -> {h}, sim minutes {m0} -> {m}"
        ));
    });
    sc.check("the entities match the simulation positions", |cx| {
        let mut q = cx
            .world
            .query::<(&crate::walk::play::NpcActor, &Transform)>();
        let rows: Vec<(ActorId, Vec3)> = q
            .iter(cx.world)
            .map(|(a, t)| (a.0, t.translation))
            .collect();
        let mut off = 0;
        let mut worst = 0.0f32;
        for (id, t) in &rows {
            if let Some(p) = cx.actor_pos(*id) {
                let d = (p * 0.01 - *t).length();
                worst = worst.max(d);
                if d > 5.0 {
                    off += 1;
                }
            }
        }
        expect(
            off == 0,
            format!("{} entities, worst offset {worst:.2} m", rows.len()),
            format!(
                "{off} entities more than 5 m from their simulation position (worst {worst:.1} m)"
            ),
        )
    });
    let _ = npc_pos;
    sc
}

/// `--play --combat souls --qa souls_basic`: draw, lock on (Tab), roll (Space), light attacks.
pub fn souls_basic() -> Sc {
    let mut sc = Sc::new(300.0);
    grab(&mut sc);
    pick_target(&mut sc);
    sc.act("Digit1: wear and draw the sword", |cx| {
        cx.tap(KeyCode::Digit1)
    });
    sc.wait(1.5);
    approach(
        &mut sc,
        "the raider",
        target_goal(),
        450.0,
        -0.2,
        Arc::new(|_| true),
    );
    sc.step("face the raider 1 s", |cx| {
        if let Some(p) = tgt_id(cx).and_then(|i| cx.actor_pos(i)) {
            cx.aim_at(p, -0.2);
        }
        if cx.t > 1.0 { Poll::Done } else { Poll::Wait }
    });
    sc.act("Tab: lock on", |cx| cx.tap(KeyCode::Tab));
    sc.wait(0.8);
    sc.shot("locked");
    sc.check("Tab locks on a target (the player faces it)", |cx| {
        let f = cx.player().face.is_some();
        let gate = cx
            .world
            .resource::<crate::walk::play::souls::SoulsGate>()
            .active;
        let drawn = cx.play().drawn;
        expect(
            f,
            "face target set",
            format!("no lock-on target after Tab (souls gate active {gate}, drawn {drawn})"),
        )
    });
    sc.act("Space: roll", |cx| {
        let p = cx.hero_pos();
        cx.st.n.insert("roll_x".into(), f64::from(p.x));
        cx.st.n.insert("roll_z".into(), f64::from(p.z));
        cx.key_down(KeyCode::KeyA);
        cx.tap(KeyCode::Space);
    });
    sc.wait(0.7);
    sc.act("release A", |cx| cx.key_up(KeyCode::KeyA));
    sc.wait(0.6);
    sc.check("the roll moved the hero", |cx| {
        let p = cx.hero_pos();
        let d = Vec2::new(
            p.x - cx.st.n["roll_x"] as f32,
            p.z - cx.st.n["roll_z"] as f32,
        )
        .length();
        expect(
            d > 80.0 || cx.play().hero_down,
            format!("moved {d:.0} cm"),
            format!("only {d:.0} cm"),
        )
    });
    sc.step("light attacks for 20 s (walk in, click every 1.2 s)", {
        let mut last = -10.0f32;
        move |cx| {
            let Some(id) = tgt_id(cx) else {
                return Poll::Done;
            };
            if cx.play().hero_down || !cx.core().0.sim.actor(id).is_alive() || cx.t > 20.0 {
                cx.release_all();
                return Poll::Done;
            }
            if let Some(p) = cx.actor_pos(id) {
                cx.aim_at(p, -0.2);
                let d = flat(p - cx.hero_pos()).length();
                if d > 190.0 {
                    cx.key_down(KeyCode::KeyW);
                } else {
                    cx.key_up(KeyCode::KeyW);
                    if cx.now - last > 1.2 {
                        cx.mouse_down(MouseButton::Left);
                        last = cx.now;
                    } else if cx.now - last > 0.2 {
                        cx.mouse_up(MouseButton::Left);
                    }
                }
            }
            Poll::Wait
        }
    });
    sc.shot("souls_fight");
    sc.check("a light attack hurt the orc", |cx| {
        let id = tgt_id(cx).unwrap();
        let hp = cx.core().0.sim.actor(id).hp;
        let hp0 = cx.st.n["tgt_hp0"] as i32;
        expect(
            hp < hp0,
            format!("orc hp {hp0} -> {hp}"),
            format!("orc untouched ({hp0}), hero hp {}", cx.hero_hp()),
        )
    });
    sc
}

/// Name and weight of the hero's overlay clip, if any.
fn hero_overlay(cx: &mut Cx) -> Option<(String, f32)> {
    let e = cx.play().hero_entity?;
    let a = cx.world.get::<gothic_animation::Animator>(e)?;
    a.overlay
        .as_ref()
        .map(|o| (o.motion.clip.name.clone(), o.weight))
}

/// `--play --combat souls --qa souls_er` (with a local clip pack): lock-on strafe, roll, light
/// attack and guard each play their pack clip on the hero (screenshots `NN_er_*.png`). Without
/// the pack the checks that need it report and pass, so the scenario also runs on a plain
/// checkout.
pub fn souls_er() -> Sc {
    let mut sc = Sc::new(300.0);
    grab(&mut sc);
    pick_target(&mut sc);
    sc.act("Digit1: wear and draw the sword", |cx| {
        cx.tap(KeyCode::Digit1)
    });
    sc.wait(1.5);
    approach(
        &mut sc,
        "the raider",
        target_goal(),
        450.0,
        -0.2,
        Arc::new(|_| true),
    );
    sc.step("face the raider 1 s", |cx| {
        if let Some(p) = tgt_id(cx).and_then(|i| cx.actor_pos(i)) {
            cx.aim_at(p, -0.2);
        }
        if cx.t > 1.0 { Poll::Done } else { Poll::Wait }
    });
    sc.act("Tab: lock on", |cx| {
        cx.tap(KeyCode::Tab);
        let pack = cx
            .world
            .resource::<gothic_animation::ActorLibrary>()
            .has_motion("ER_Roll_Front");
        cx.st.n.insert("pack".into(), f64::from(u8::from(pack)));
        cx.note(format!("clip pack loaded: {pack}"));
    });
    sc.wait(0.8);
    sc.act("hold D: strafe right around the target", |cx| {
        cx.key_down(KeyCode::KeyD)
    });
    sc.wait(0.9);
    sc.act("sample the hero overlay (ov_strafe)", |cx| {
        let o = hero_overlay(cx);
        let name = o.map(|(n, _)| n).unwrap_or_default();
        cx.note(format!("overlay at ov_strafe: {name:?}"));
        cx.st.n.insert(
            "ov_strafe".into(),
            f64::from(u8::from(
                name.starts_with("ER_Walk") || name.starts_with("ER_Run"),
            )),
        );
    });
    sc.shot("er_strafe");
    sc.check("lock-on strafing plays a pack walk / run clip", |cx| {
        let pack = cx.st.n["pack"] > 0.5;
        let ok = !pack
            || !cx
                .world
                .resource::<gothic_combat_souls::plugin::SoulsRes>()
                .world
                .lock
                .target
                .is_some()
            || cx.st.n["ov_strafe"] > 0.5;
        expect(ok, "ok", "pack clip not playing")
    });
    sc.act("release D", |cx| cx.key_up(KeyCode::KeyD));
    sc.wait(0.5);
    sc.act("Space + A: roll left", |cx| {
        cx.key_down(KeyCode::KeyA);
        cx.tap(KeyCode::Space);
    });
    sc.wait(0.3);
    sc.act("sample the hero overlay (ov_roll)", |cx| {
        let o = hero_overlay(cx);
        let name = o.map(|(n, _)| n).unwrap_or_default();
        cx.note(format!("overlay at ov_roll: {name:?}"));
        cx.st.n.insert(
            "ov_roll".into(),
            f64::from(u8::from(name.starts_with("ER_Roll"))),
        );
    });
    sc.shot("er_roll_a");
    sc.check("the roll plays the pack roll clip", |cx| {
        let pack = cx.st.n["pack"] > 0.5;
        let ok = !pack || cx.st.n["ov_roll"] > 0.5;
        expect(ok, "ok", "pack clip not playing")
    });
    sc.wait(0.25);
    sc.shot("er_roll_b");
    sc.act("release A", |cx| cx.key_up(KeyCode::KeyA));
    sc.wait(1.2);
    sc.act("left click: light attack", |cx| {
        cx.mouse_click(MouseButton::Left)
    });
    sc.wait(0.32);
    sc.act("sample the hero overlay (ov_attack)", |cx| {
        let o = hero_overlay(cx);
        let name = o.map(|(n, _)| n).unwrap_or_default();
        cx.note(format!("overlay at ov_attack: {name:?}"));
        cx.st.n.insert(
            "ov_attack".into(),
            f64::from(u8::from(name.contains("_Light"))),
        );
    });
    sc.shot("er_attack");
    sc.check("the light attack plays a pack attack clip", |cx| {
        let pack = cx.st.n["pack"] > 0.5;
        let ok = !pack || cx.st.n["ov_attack"] > 0.5;
        expect(ok, "ok", "pack clip not playing")
    });
    sc.wait(1.6);
    sc.act("hold right: guard", |cx| cx.mouse_down(MouseButton::Right));
    sc.wait(0.7);
    sc.act("sample the hero overlay (ov_guard)", |cx| {
        let o = hero_overlay(cx);
        let name = o.map(|(n, _)| n).unwrap_or_default();
        cx.note(format!("overlay at ov_guard: {name:?}"));
        cx.st.n.insert(
            "ov_guard".into(),
            f64::from(u8::from(
                name.contains("_Guard") || name.starts_with("ER_Hurt"),
            )),
        );
    });
    sc.shot("er_guard");
    sc.check("guarding plays the pack guard pose", |cx| {
        let pack = cx.st.n["pack"] > 0.5;
        let ok = !pack || cx.st.n["ov_guard"] > 0.5;
        expect(ok, "ok", "pack clip not playing")
    });
    sc.act("release right", |cx| cx.mouse_up(MouseButton::Right));
    sc.wait(0.6);
    sc
}

/// Keeps the hero up in long observation scenarios (setup, not a rule).
pub(super) fn stay_alive(cx: &mut Cx) {
    let hero = cx.core().0.hero;
    let mut core = cx.core_mut();
    let a = core.0.sim.actor_mut(hero);
    a.hp = a.hp.max(120);
}
