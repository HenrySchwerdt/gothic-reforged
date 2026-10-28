//! Scenarios of the original controls, the bow, spells and magic-using NPCs.
use super::more::approach;
use super::more2::{grab, pick_target, target_goal, tgt_id};
use super::{Cx, Poll, Sc, expect};
use crate::walk::play::fx::FxLog;
use crate::walk::play::shots::ShotsOnScreen;
use bevy::prelude::*;
use std::sync::Arc;

fn shots(cx: &Cx) -> (u32, u32, u32, u32) {
    let s = cx.world.resource::<ShotsOnScreen>();
    (
        s.spell_spawned,
        s.arrows_spawned,
        s.impacts,
        s.effects_started,
    )
}

/// Starts / stops the frame capture of `--record DIR` (fixed 1/30 s steps).
fn rec(sc: &mut Sc, on: bool) {
    sc.act(if on { "record on" } else { "record off" }, move |cx| {
        if let Some(mut r) = cx
            .world
            .get_resource_mut::<crate::walk::play::record::Recorder>()
        {
            r.active = on;
        }
    });
}

/// Visual watcher: every frame the shots drawn are projected through the game camera; frames in
/// which a spell missile / an arrow is inside the screen are counted, and so are the frames in
/// which the bow actor exists and is inside the screen.
pub(super) fn visual_watch(sc: &mut Sc) {
    sc.watch(|cx| {
        use crate::walk::play::ranged::BowVisual;
        let (cam, gt) = {
            let mut q = cx
                .world
                .query_filtered::<(&Camera, &GlobalTransform), With<crate::walk::PlayerCamera>>();
            let Ok((c, g)) = q.single(cx.world) else {
                return;
            };
            (c.clone(), *g)
        };
        let size = cam
            .logical_viewport_size()
            .unwrap_or(Vec2::new(1280.0, 720.0));
        let inside = |p: Vec2| p.x > 0.0 && p.y > 0.0 && p.x < size.x && p.y < size.y;
        let live = cx.world.resource::<ShotsOnScreen>().live();
        let flights: Vec<(u64, bool)> = cx
            .core()
            .0
            .sim
            .script
            .magic
            .flights
            .iter()
            .map(|f| (f.shot.id, !f.shot.spec.spell.is_empty()))
            .collect();
        for (id, e, pos) in live {
            let spell = flights.iter().find(|f| f.0 == id).is_some_and(|f| f.1);
            let drawn = cx.world.get::<Transform>(e).is_some();
            if drawn
                && let Some(p) = crate::walk::play::ranged::screen_of(&cam, &gt, pos)
                && inside(p)
            {
                let k = if spell { "vis_spell" } else { "vis_arrow" };
                let n = cx.st.n.entry(k.into()).or_insert(0.0);
                *n += 1.0;
                // A frame from the middle of the flight, with the missile's screen position.
                if (*n == 6.0 && spell) || (*n == 4.0 && !spell) {
                    let tag = if spell {
                        "fireball_mid_flight"
                    } else {
                        "arrow_mid_flight"
                    };
                    cx.note(format!("{tag}: shot at screen {p:?}"));
                    cx.shot(tag);
                }
            }
        }
        // The hero's body parts must be visible while he aims or casts.
        {
            use gothic_sim::script::routine::state::State;
            let st = cx.core().0.sim.script.npc(cx.core().0.hero).state;
            let busy = matches!(
                st,
                Some(
                    State::PsRangedAim
                        | State::PsRangedPowerAim
                        | State::PsRangedAimLoop
                        | State::PsMagicCast
                        | State::PsMagicPowerCast
                        | State::PsMagicQuickCast
                )
            );
            if busy {
                let mut q = cx.world.query_filtered::<
                    &gothic_animation::runtime::PartMeshes,
                    With<crate::walk::play::HeroActor>,
                >();
                let ents: Vec<Entity> = q
                    .iter(cx.world)
                    .flat_map(|p| p.0.iter().flat_map(|(_, es)| es.iter().copied()))
                    .collect();
                let visible = ents.iter().any(|e| {
                    cx.world
                        .get::<InheritedVisibility>(*e)
                        .is_some_and(|v| v.get())
                });
                *cx.st.n.entry("aim_frames".into()).or_insert(0.0) += 1.0;
                if visible {
                    *cx.st.n.entry("aim_hero_visible".into()).or_insert(0.0) += 1.0;
                }
            }
        }
        let mut q = cx
            .world
            .query::<(&BowVisual, &GlobalTransform, &InheritedVisibility)>();
        let bows: Vec<(Vec3, bool)> = q
            .iter(cx.world)
            .map(|(_, g, v)| (g.translation() / CM, v.get()))
            .collect();
        for (p, vis) in bows {
            if vis
                && let Some(sp) = crate::walk::play::ranged::screen_of(&cam, &gt, p)
                && inside(sp)
            {
                *cx.st.n.entry("vis_bow".into()).or_insert(0.0) += 1.0;
            }
        }
    });
}

const CM: f32 = 0.01;

fn set_hero_full(cx: &mut Cx) {
    let hero = cx.core().0.hero;
    let mut core = cx.core_mut();
    let a = core.0.sim.actor_mut(hero);
    a.hp = a.hp.max(150);
    a.mana = a.mana.max(100);
    a.stamina = a.stamina.max(100);
}

/// The Controls=Standard keys: Q / E turn the camera and not the body, Shift is walk, there is
/// no dodge, the left button is no attack while the sword is away.
pub fn controls_basic() -> Sc {
    let mut sc = Sc::new(200.0);
    grab(&mut sc);
    sc.wait(2.0);
    sc.act("record yaw", |cx| {
        let p = cx.player();
        let (y, h) = (p.yaw, cx.play().hero_yaw);
        cx.st.n.insert("cam0".into(), f64::from(y));
        cx.st.n.insert("body0".into(), f64::from(h));
    });
    sc.act("hold E", |cx| cx.key_down(KeyCode::KeyE));
    sc.wait(0.6);
    sc.act("release E", |cx| cx.key_up(KeyCode::KeyE));
    sc.wait(0.4);
    sc.check("E rotates the camera, not the body", |cx| {
        let dy = crate::walk::play::angle_delta(cx.st.n["cam0"] as f32, cx.player().yaw).abs();
        let db = crate::walk::play::angle_delta(cx.st.n["body0"] as f32, cx.play().hero_yaw).abs();
        expect(
            dy > 0.15 && db < 0.1,
            format!(
                "camera turned {:.0} deg, body {:.0} deg",
                dy.to_degrees(),
                db.to_degrees()
            ),
            format!(
                "camera {:.0} deg, body {:.0} deg",
                dy.to_degrees(),
                db.to_degrees()
            ),
        )
    });
    sc.shot("after_E");
    // Run, then walk with Shift held.
    sc.act("hold W", |cx| {
        cx.key_down(KeyCode::KeyW);
    });
    sc.wait(1.6);
    sc.act("sample run speed", |cx| {
        let v = cx.player().ctrl.velocity;
        cx.st
            .n
            .insert("run".into(), f64::from(Vec2::new(v.x, v.z).length()));
        cx.key_down(KeyCode::ShiftLeft);
    });
    sc.wait(1.6);
    sc.act("sample walk speed", |cx| {
        let v = cx.player().ctrl.velocity;
        cx.st
            .n
            .insert("walk".into(), f64::from(Vec2::new(v.x, v.z).length()));
        cx.key_up(KeyCode::ShiftLeft);
        cx.key_up(KeyCode::KeyW);
    });
    sc.check("Shift walks (slower than the run)", |cx| {
        let (r, w) = (cx.st.n["run"], cx.st.n["walk"]);
        expect(
            r > 150.0 && w > 20.0 && w < r * 0.8,
            format!("run {r:.0} cm/s, walk {w:.0} cm/s"),
            format!("run {r:.0} cm/s, walk {w:.0} cm/s"),
        )
    });
    sc.wait(1.0);
    sc.act("double tap A", |cx| {
        let p = cx.hero_pos();
        cx.st.n.insert("tx".into(), f64::from(p.x));
        cx.st.n.insert("tz".into(), f64::from(p.z));
        cx.tap(KeyCode::KeyA);
    });
    sc.wait(0.12);
    sc.act("second tap", |cx| cx.tap(KeyCode::KeyA));
    sc.wait(1.0);
    sc.check("a double tap is no dodge", |cx| {
        let p = cx.hero_pos();
        let d = Vec2::new(p.x - cx.st.n["tx"] as f32, p.z - cx.st.n["tz"] as f32).length();
        expect(
            d < 200.0,
            format!("moved {d:.0} cm"),
            format!("slid {d:.0} cm"),
        )
    });
    sc.act("left click with the sword away", |cx| {
        let log = cx.world.resource::<crate::walk::play::events::CombatLog>();
        let n = log.hits_on_npcs;
        cx.st.n.insert("hits0".into(), f64::from(n));
        let st = cx.core().0.sim.actor(cx.core().0.hero).stamina;
        cx.st.n.insert("sta0".into(), f64::from(st));
        cx.mouse_click(MouseButton::Left);
    });
    sc.wait(1.0);
    sc.check("the left button is the use key, no attack", |cx| {
        let log = cx.world.resource::<crate::walk::play::events::CombatLog>();
        let st = cx.core().0.sim.actor(cx.core().0.hero).stamina;
        expect(
            log.hits_on_npcs == cx.st.n["hits0"] as u32 && st >= cx.st.n["sta0"] as i32 - 1,
            "no swing",
            "a click without a weapon attacked",
        )
    });
    sc.act("Space: draw the sword", |cx| cx.tap(KeyCode::Space));
    sc.wait(1.0);
    sc.act("click: the swing starts on the release", |cx| {
        let st = cx.core().0.sim.actor(cx.core().0.hero).stamina;
        cx.st.n.insert("sta1".into(), f64::from(st));
        cx.mouse_down(MouseButton::Left);
    });
    sc.wait(0.08);
    sc.act("release", |cx| cx.mouse_up(MouseButton::Left));
    sc.wait(0.3);
    sc.shot("swing");
    sc.act("holster", |cx| cx.tap(KeyCode::Space));
    sc.wait(0.5);
    sc
}

/// Fireball and bow against the nearest raider: cast phases with their effects, the real
/// missile and arrow, the impact, the hand glow ending, the aim camera and crosshair.
pub fn magic_bow() -> Sc {
    let mut sc = Sc::new(300.0);
    visual_watch(&mut sc);
    grab(&mut sc);
    pick_target(&mut sc);
    sc.act(
        "setup: fireball rune in quick slot 6 (the spell book's drag does this), mana",
        |cx| {
            cx.world
                .resource_mut::<crate::walk::play::PlayState>()
                .quick[5] = Some("It_Spell_Fireball".into());
            super::scenarios::grant(cx, "It_Spell_Fireball", 1);
            set_hero_full(cx);
        },
    );
    approach(
        &mut sc,
        "the raider",
        target_goal(),
        900.0,
        -0.2,
        Arc::new(|_| true),
    );
    sc.step(
        "step back to 18 m so the flight is long enough to see",
        |cx| {
            if let Some(p) = tgt_id(cx).and_then(|i| cx.actor_pos(i)) {
                cx.aim_at(p, -0.15);
            }
            cx.key_down(KeyCode::KeyS);
            let d = tgt_id(cx)
                .and_then(|i| cx.actor_pos(i))
                .map_or(0.0, |p| (p - cx.hero_pos()).length());
            if d > 1800.0 || cx.t > 12.0 {
                cx.key_up(KeyCode::KeyS);
                Poll::Done
            } else {
                Poll::Wait
            }
        },
    );
    sc.act("record and select the fireball", |cx| {
        let (m, a, i, e) = shots(cx);
        cx.st.n.insert("m0".into(), f64::from(m));
        cx.st.n.insert("a0".into(), f64::from(a));
        cx.st.n.insert("i0".into(), f64::from(i));
        cx.st.n.insert("e0".into(), f64::from(e));
        let mana = cx.core().0.sim.actor(cx.core().0.hero).mana;
        cx.st.n.insert("mana0".into(), f64::from(mana));
        let f = cx.world.resource::<FxLog>();
        cx.st.n.insert("casts0".into(), f64::from(f.casts));
        cx.tap(KeyCode::Digit6);
    });
    sc.step("aim 1.5 s", |cx| {
        if let Some(p) = tgt_id(cx).and_then(|i| cx.actor_pos(i)) {
            cx.aim_at(p, -0.15);
        }
        if cx.t > 1.5 { Poll::Done } else { Poll::Wait }
    });
    sc.shot("spell_ready");
    rec(&mut sc, true);
    sc.act("hold left (cast)", |cx| cx.mouse_down(MouseButton::Left));
    sc.step("hold 0.4 s: the load glow", |cx| {
        if let Some(p) = tgt_id(cx).and_then(|i| cx.actor_pos(i)) {
            cx.aim_at(p, -0.15);
        }
        if cx.t > 0.4 { Poll::Done } else { Poll::Wait }
    });
    sc.shot("spell_load");
    sc.step("hold to 1.0 s", |cx| {
        if let Some(p) = tgt_id(cx).and_then(|i| cx.actor_pos(i)) {
            cx.aim_at(p, -0.15);
        }
        // The missile leaves when the raise clip ends (0.8 s).
        let (m, ..) = shots(cx);
        if cx.t > 1.0 || m as f64 > cx.st.n["m0"] {
            Poll::Done
        } else {
            Poll::Wait
        }
    });
    sc.act("release left", |cx| cx.mouse_up(MouseButton::Left));
    sc.wait(0.35);
    sc.shot("spell_flight");
    sc.wait(2.5);
    sc.shot("spell_after");
    rec(&mut sc, false);
    sc.check(
        "the missile left at the end of the raise and was a simulated shot",
        |cx| {
            let (m, _, i, e) = shots(cx);
            let mana = cx.core().0.sim.actor(cx.core().0.hero).mana;
            let casts = cx.world.resource::<FxLog>().casts;
            expect(
                m as f64 > cx.st.n["m0"] && casts as f64 > cx.st.n["casts0"],
                format!(
                    "shots {} -> {m}, impacts {} -> {i}, effects {} -> {e}, mana {} -> {mana}",
                    cx.st.n["m0"], cx.st.n["i0"], cx.st.n["e0"], cx.st.n["mana0"]
                ),
                format!("no missile (casts {casts}, mana {mana})"),
            )
        },
    );
    sc.check("the fireball hit something (impact effect)", |cx| {
        let (_, _, i, _) = shots(cx);
        expect(
            i as f64 > cx.st.n["i0"],
            format!("impacts {} -> {i}", cx.st.n["i0"]),
            "no impact event",
        )
    });
    // ---- the bow ----------------------------------------------------------------------------
    sc.act("setup: heal", |cx| set_hero_full(cx));
    sc.act("bow: quick slot 3", |cx| cx.tap(KeyCode::Digit3));
    sc.step("aim 2 s", |cx| {
        if let Some(p) = tgt_id(cx).and_then(|i| cx.actor_pos(i)) {
            cx.aim_at(p, -0.15);
        }
        if cx.t > 2.0 { Poll::Done } else { Poll::Wait }
    });
    sc.shot("bow_ready");
    sc.act("E: look at the hero from the side", |cx| {
        cx.key_down(KeyCode::KeyE)
    });
    sc.wait(1.5);
    sc.act("release E", |cx| cx.key_up(KeyCode::KeyE));
    sc.wait(1.0);
    sc.shot("bow_side");
    sc.act("Q: back behind", |cx| cx.key_down(KeyCode::KeyQ));
    sc.wait(1.5);
    sc.act("release Q", |cx| cx.key_up(KeyCode::KeyQ));
    sc.step("aim again 1.5 s", |cx| {
        if let Some(p) = tgt_id(cx).and_then(|i| cx.actor_pos(i)) {
            cx.aim_at(p, -0.15);
        }
        if cx.t > 1.5 { Poll::Done } else { Poll::Wait }
    });
    rec(&mut sc, true);
    sc.act("hold left (draw the string)", |cx| {
        let (_, a, ..) = shots(cx);
        cx.st.n.insert("a1".into(), f64::from(a));
        let q = cx.count_item("Arrow");
        cx.st.n.insert("quiver0".into(), f64::from(q));
        cx.mouse_down(MouseButton::Left)
    });
    sc.step("draw 0.7 s", |cx| {
        if let Some(p) = tgt_id(cx).and_then(|i| cx.actor_pos(i)) {
            cx.aim_at(p, -0.15);
        }
        if cx.t > 0.7 { Poll::Done } else { Poll::Wait }
    });
    sc.shot("bow_draw");
    sc.step("draw to 2.2 s", |cx| {
        if let Some(p) = tgt_id(cx).and_then(|i| cx.actor_pos(i)) {
            cx.aim_at(p, -0.15);
        }
        if cx.t > 1.6 { Poll::Done } else { Poll::Wait }
    });
    sc.shot("bow_full");
    sc.act("release (shoot)", |cx| cx.mouse_up(MouseButton::Left));
    sc.wait(0.3);
    sc.shot("arrow_flight");
    sc.wait(2.0);
    sc.shot("arrow_after");
    rec(&mut sc, false);
    sc.check("an arrow flew as a simulated shot", |cx| {
        let (_, a, i, _) = shots(cx);
        let q = cx.count_item("Arrow");
        expect(
            a as f64 > cx.st.n["a1"] && q < cx.st.n["quiver0"] as u32,
            format!(
                "arrows {} -> {a}, quiver {} -> {q}, impacts total {i}",
                cx.st.n["a1"], cx.st.n["quiver0"]
            ),
            format!("no arrow (arrows {a}, quiver {q})"),
        )
    });
    sc.check(
        "VISUAL: the fireball was drawn on screen while it flew",
        |cx| {
            let n = cx.st.n.get("vis_spell").copied().unwrap_or(0.0);
            expect(
                n >= 3.0,
                format!("{n} frames inside the screen"),
                format!("only {n} frames"),
            )
        },
    );
    sc.check(
        "VISUAL: the arrow was drawn on screen while it flew",
        |cx| {
            let n = cx.st.n.get("vis_arrow").copied().unwrap_or(0.0);
            expect(
                n >= 2.0,
                format!("{n} frames inside the screen"),
                format!("only {n} frames"),
            )
        },
    );
    sc.check(
        "VISUAL: the hero stays visible while he aims and casts",
        |cx| {
            let n = cx.st.n.get("aim_frames").copied().unwrap_or(0.0);
            let v = cx.st.n.get("aim_hero_visible").copied().unwrap_or(0.0);
            expect(
                n > 10.0 && v >= n * 0.98,
                format!("{v} of {n} aim / cast frames"),
                format!("hero visible in {v} of {n} aim / cast frames"),
            )
        },
    );
    sc.check(
        "VISUAL: the bow actor was visible inside the screen",
        |cx| {
            let n = cx.st.n.get("vis_bow").copied().unwrap_or(0.0);
            expect(n >= 20.0, format!("{n} frames"), format!("only {n} frames"))
        },
    );
    sc.act("Space: put the bow away", |cx| cx.tap(KeyCode::Space));
    sc.wait(1.5);
    sc.shot("holstered");
    sc
}

/// Magic-using NPC: an orc shaman from the world data against the hero.
pub fn shaman_duel() -> Sc {
    let mut sc = Sc::new(400.0);
    grab(&mut sc);
    sc.act("find a shaman in the roster", |cx| {
        let hero = cx.hero_pos();
        let r = cx.world.resource::<crate::walk::play::Roster>();
        let best = r
            .cands
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.record.name.contains("Shaman")
                    && r.docs.get(c.doc as usize).is_some_and(|d| d.enabled)
            })
            .min_by(|a, b| {
                (a.1.pos_cm - hero)
                    .length()
                    .total_cmp(&(b.1.pos_cm - hero).length())
            })
            .map(|(i, c)| (i, c.pos_cm, c.record.name.clone()));
        match best {
            Some((i, p, name)) => {
                cx.st.n.insert("cand".into(), i as f64);
                cx.st.n.insert("sx".into(), f64::from(p.x));
                cx.st.n.insert("sy".into(), f64::from(p.y));
                cx.st.n.insert("sz".into(), f64::from(p.z));
                cx.note(format!("shaman {name} at {p:?}"));
            }
            None => cx.check("a shaman exists", false, "no enabled shaman in the roster"),
        }
    });
    sc.act("setup: teleport the hero 18 m south of the shaman", |cx| {
        let (sx, sy, sz) = (
            cx.st.n["sx"] as f32,
            cx.st.n["sy"] as f32,
            cx.st.n["sz"] as f32,
        );
        let mut p = cx.world.resource_mut::<crate::walk::Player>();
        p.ctrl.position = Vec3::new(sx, sy + 400.0, sz + 1800.0);
        p.ctrl.vertical_speed = 0.0;
        p.yaw = 0.0;
        p.placed = false;
    });
    sc.until("the shaman exists and the hero stands", 120.0, |cx| {
        let i = cx.st.n["cand"] as usize;
        let spawned = cx
            .world
            .resource::<crate::walk::play::Roster>()
            .cands
            .get(i)
            .is_some_and(|c| c.actor.is_some());
        spawned && cx.player().ctrl.grounded
    });
    sc.wait(2.0);
    sc.act("record the shaman", |cx| {
        let i = cx.st.n["cand"] as usize;
        let id = cx.world.resource::<crate::walk::play::Roster>().cands[i]
            .actor
            .unwrap();
        cx.st.n.insert("tgt".into(), f64::from(id.0));
        let a = cx.core().0.sim.actor(id);
        let (hp, mana, name) = (a.hp, a.mana, a.name.clone());
        let weapon = a.equipment.right.as_ref().map(|w| w.name.clone());
        cx.note(format!(
            "shaman {name}: hp {hp} mana {mana}, right hand {weapon:?}"
        ));
        cx.st.n.insert("mana0".into(), f64::from(mana));
        let (m, a, i2, e) = shots(cx);
        cx.st.n.insert("m0".into(), f64::from(m));
        cx.st.n.insert("a0".into(), f64::from(a));
        cx.st.n.insert("i0".into(), f64::from(i2));
        cx.st.n.insert("e0".into(), f64::from(e));
        let f = cx.world.resource::<FxLog>();
        cx.st.n.insert("casts0".into(), f64::from(f.casts));
    });
    sc.shot("shaman_idle");
    // Provoke him: a fireball-free way is an arrow or a sword; the hero simply stays in sight
    // and hits the shaman with the bow.
    sc.act("bow: quick slot 3", |cx| cx.tap(KeyCode::Digit3));
    sc.step("aim at the shaman and shoot every 3 s, watch 60 s", {
        let mut last = -10.0f32;
        let mut down = false;
        move |cx| {
            super::more2::stay_alive(cx);
            let Some(id) = tgt_id(cx) else {
                return Poll::Fail("no shaman".into());
            };
            if let Some(p) = cx.actor_pos(id) {
                cx.aim_at(p + Vec3::Y * 110.0, 0.0);
            }
            if !cx.core().0.sim.actor(id).is_alive() || cx.t > 60.0 {
                cx.release_all();
                return Poll::Done;
            }
            if down && cx.now - last > 1.4 {
                cx.mouse_up(MouseButton::Left);
                down = false;
            } else if !down && cx.now - last > 3.0 {
                cx.mouse_down(MouseButton::Left);
                down = true;
                last = cx.now;
            }
            let mut mana_trace = cx.st.n.get("mana_min").copied().unwrap_or(1.0e9);
            mana_trace = mana_trace.min(f64::from(cx.core().0.sim.actor(id).mana));
            cx.st.n.insert("mana_min".into(), mana_trace);
            if (cx.t as u32).is_multiple_of(10)
                && cx.st.n.insert("trace".into(), f64::from(cx.t as u32))
                    != Some(f64::from(cx.t as u32))
            {
                let a = cx.core().0.sim.actor(id);
                let (mana, hp, state) = (a.mana, a.hp, cx.core().0.sim.script.npc(id).state);
                let (m, ..) = shots(cx);
                cx.note(format!(
                    "t={:.0}s shaman hp {hp} mana {mana} state {state:?} shots {m} hero hp {}",
                    cx.t,
                    cx.hero_hp()
                ));
            }
            Poll::Wait
        }
    });
    sc.shot("shaman_fight");
    sc.check(
        "the shaman cast spells (simulated missiles with effects)",
        |cx| {
            let (m, _, i, e) = shots(cx);
            let casts = cx.world.resource::<FxLog>().casts;
            expect(
                m as f64 > cx.st.n["m0"],
                format!(
                    "missiles {} -> {m}, impacts {} -> {i}, effects {} -> {e}, casts {} -> {casts}",
                    cx.st.n["m0"], cx.st.n["i0"], cx.st.n["e0"], cx.st.n["casts0"]
                ),
                format!("no missile from the shaman (casts {casts})"),
            )
        },
    );
    sc.check(
        "he holds a spell in the hands (NPC mana is fixed, S:35d40)",
        |cx| {
            let Some(id) = tgt_id(cx) else {
                return Err("no shaman".into());
            };
            let eq = &cx.core().0.sim.actor(id).equipment;
            let wm = eq.weapon_mode();
            let right = eq.right.as_ref().map(|d| d.name.clone());
            expect(
                wm == gothic_sim::item::WeaponMode::Magic,
                format!("weapon mode {wm:?}, right hand {right:?}"),
                format!("weapon mode {wm:?}, right hand {right:?}"),
            )
        },
    );
    sc
}

/// The focus name label next to the original start view (`.local/ref/orig-ardea-start.png`).
pub fn hud_focus() -> Sc {
    let mut sc = Sc::new(120.0);
    grab(&mut sc);
    sc.wait(3.0);
    sc.act("setup: look at Hamlar's neighbourhood", |_| {});
    sc.shot("start_view");
    let goal: super::more::Goal = super::more::npc_pos("Hamlar");
    approach(
        &mut sc,
        "Hamlar",
        goal,
        330.0,
        -0.15,
        Arc::new(|cx| cx.play().target.is_some()),
    );
    sc.wait(1.0);
    sc.shot("focus_name");
    sc.check("a focus label is shown for the focused character", |cx| {
        let l = cx.world.resource::<gothic_ui::FocusLabel>();
        expect(
            !l.text.is_empty() && l.alpha() > 0.3,
            format!("label {:?} alpha {:.2}", l.text, l.alpha()),
            format!("label {:?} alpha {:.2}", l.text, l.alpha()),
        )
    });
    // gCFocus_PS::OnAction (Game:0x1006e8e0) / FindFocusEntity (Game:0x1006da20): the lock key
    // keeps the focus while the camera looks away; a second press releases it.
    sc.act("R: lock the focus", |cx| cx.tap(KeyCode::KeyR));
    sc.wait(0.3);
    sc.act("turn the camera away", |cx| cx.mouse_motion(Vec2::new(900.0, 0.0)));
    sc.wait(0.6);
    sc.shot("focus_locked_looking_away");
    let locked_id = Arc::new(std::sync::Mutex::new(None));
    let rec = locked_id.clone();
    sc.check("the locked character stays focused with the camera turned away", move |cx| {
        let p = cx.play();
        *rec.lock().unwrap() = p.focus_lock;
        let ok = p.focus_lock.is_some() && p.focus == p.focus_lock;
        expect(
            ok,
            format!("lock {:?} focus {:?}", p.focus_lock, p.focus),
            format!("lock {:?} focus {:?}", p.focus_lock, p.focus),
        )
    });
    sc.act("R: unlock", |cx| cx.tap(KeyCode::KeyR));
    sc.wait(0.4);
    sc.check("the second press releases the lock and the focus is searched again", move |cx| {
        let p = cx.play();
        let was = *locked_id.lock().unwrap();
        expect(
            p.focus_lock.is_none() && p.focus != was,
            format!("lock {:?} focus {:?}", p.focus_lock, p.focus),
            format!("lock {:?} focus {:?}", p.focus_lock, p.focus),
        )
    });
    sc
}

/// gCFocus_PS::OnAction (Game:0x1006e8e0) / GetAngles (Game:0x1006c530): with the sword drawn
/// (`PS_Melee_Loop` sets `FocusLookAtMode` 2, "keys") every movement-key message moves the focus
/// direction vector by the camera's flattened forward / right axis; the focus angles are measured
/// against it, so strafing right makes the target to the right the one in view.
pub fn focus_steer() -> Sc {
    let mut sc = Sc::new(90.0);
    grab(&mut sc);
    sc.wait(2.0);
    sc.act("Space: draw the sword", |cx| cx.tap(KeyCode::Space));
    sc.wait(2.0);
    sc.check("the melee loop selected the keys look-at mode", |cx| {
        let m = cx.core().0.sim.script.player.focus_modes.1;
        expect(m == 2, format!("look-at mode {m}"), format!("look-at mode {m}, want 2"))
    });
    sc.act("hold D (strafe right)", |cx| cx.key_down(KeyCode::KeyD));
    sc.wait(0.4);
    sc.act("release D", |cx| cx.key_up(KeyCode::KeyD));
    sc.wait(0.2);
    sc.check("the focus direction points to the camera's right", |cx| {
        let right = {
            let mut q = cx
                .world
                .query_filtered::<&GlobalTransform, With<crate::walk::PlayerCamera>>();
            q.single(cx.world)
                .map(|t| {
                    let r = t.rotation() * Vec3::X;
                    Vec2::new(r.x, r.z).normalize_or_zero()
                })
                .unwrap_or(Vec2::ZERO)
        };
        let d = Vec2::from_array(cx.play().focus_dir);
        let dot = d.dot(right);
        expect(
            dot > 0.9,
            format!("focus_dir {d:?}, camera right {right:?}, dot {dot:.2}"),
            format!("focus_dir {d:?}, camera right {right:?}, dot {dot:.2}"),
        )
    });
    sc.act("hold A (strafe left)", |cx| cx.key_down(KeyCode::KeyA));
    sc.wait(1.0);
    sc.act("release A", |cx| cx.key_up(KeyCode::KeyA));
    sc.check("strafing left turns the direction to the camera's left", |cx| {
        let right = {
            let mut q = cx
                .world
                .query_filtered::<&GlobalTransform, With<crate::walk::PlayerCamera>>();
            q.single(cx.world)
                .map(|t| {
                    let r = t.rotation() * Vec3::X;
                    Vec2::new(r.x, r.z).normalize_or_zero()
                })
                .unwrap_or(Vec2::ZERO)
        };
        let d = Vec2::from_array(cx.play().focus_dir);
        let dot = d.dot(right);
        expect(
            dot < -0.5,
            format!("dot {dot:.2}"),
            format!("focus_dir {d:?}, camera right {right:?}, dot {dot:.2}"),
        )
    });
    sc
}

/// A mage's staff parries with the right button (`OnMagicHandleInput` S:7d360: use key 2 while
/// `GetUseType(FindStackIndex(LeftHand))` is the staff, 0xc, queues action 0x10;
/// `PS_Magic_Loop` S:7a590 turns it into `PS_Magic_StaffParade`). `PS_Magic` holds the staff in
/// the left hand next to the spell in the right.
pub fn staff_parade() -> Sc {
    use gothic_sim::item::Use;
    let mut sc = Sc::new(120.0);
    grab(&mut sc);
    sc.act("setup: fireball rune in quick slot 6, a staff worn, mana", |cx| {
        cx.world
            .resource_mut::<crate::walk::play::PlayState>()
            .quick[5] = Some("It_Spell_Fireball".into());
        super::scenarios::grant(cx, "It_Spell_Fireball", 1);
        super::scenarios::grant(cx, "It_Staff", 1);
        set_hero_full(cx);
    });
    sc.wait(1.0);
    sc.act("wear the staff", |cx| {
        cx.world
            .write_message(gothic_runtime::CoreCommand(gothic_runtime::Command::UseItem {
                template: "It_Staff".into(),
            }));
    });
    sc.wait(1.0);
    sc.act("what the staff became", |cx| {
        let c = cx.core();
        let hero = c.0.hero;
        let stacks: Vec<String> = c
            .0
            .sim
            .actor(hero)
            .inventory
            .stacks()
            .iter()
            .filter(|s| s.template.contains("Staff") || s.template.contains("Spell_Fire"))
            .map(|s| format!("{} slot {:?} x{}", s.template, s.slot, s.amount))
            .collect();
        let eq = &c.0.sim.actor(hero).equipment;
        let (l, r) = (eq.left.as_ref().map(|i| i.name.clone()), eq.right.as_ref().map(|i| i.name.clone()));
        drop(c);
        cx.note(format!("stacks {stacks:?}, left {l:?}, right {r:?}"));
    });
    sc.act("quick slot 6: the spell", |cx| cx.tap(KeyCode::Digit6));
    sc.wait(3.0);
    sc.check("in magic mode the spell is in the right hand and the staff in the left", |cx| {
        let eq = &cx.core().0.sim.actor(cx.core().0.hero).equipment;
        let left = eq.left.as_ref().map(|i| (i.name.clone(), i.use_kind()));
        let right = eq.right.as_ref().map(|i| (i.name.clone(), i.use_kind()));
        let (_, st, _) = cx.core().0.sim.hero_script_state();
        expect(
            left.as_ref().is_some_and(|l| l.1 == Use::Staff)
                && right.as_ref().is_some_and(|r| r.1 == Use::Cast),
            format!("left {left:?}, right {right:?}, state {st:?}"),
            format!("left {left:?}, right {right:?}, state {st:?}"),
        )
    });
    sc.act("right button: the parade", |cx| cx.mouse_click(MouseButton::Right));
    sc.wait(1.0);
    sc.check("the staff parade state is entered", |cx| {
        let trace = cx.world.resource::<crate::walk::play::PlayState>().sneaking;
        let _ = trace;
        let (_, st, _) = cx.core().0.sim.hero_script_state();
        let name = st.map(|s| s.name()).unwrap_or("-");
        expect(
            name.contains("StaffParade"),
            format!("state {name}"),
            format!("state {name}"),
        )
    });
    sc
}

/// Close-up poses: sword in hand, bow in Aim_Aim from behind and from the side, arrow on the
/// string, fireball at the cast and at the release (frames are cropped around the hero by the
/// caller of the scenario).
pub fn bow_pose() -> Sc {
    let mut sc = Sc::new(180.0);
    grab(&mut sc);
    sc.act("setup: fireball rune in quick slot 6, mana", |cx| {
        cx.world
            .resource_mut::<crate::walk::play::PlayState>()
            .quick[5] = Some("It_Spell_Fireball".into());
        super::scenarios::grant(cx, "It_Spell_Fireball", 1);
        set_hero_full(cx);
    });
    sc.wait(1.0);
    sc.act("Space: sword", |cx| cx.tap(KeyCode::Space));
    sc.wait(1.5);
    sc.shot("sword_back");
    sc.act("E", |cx| cx.key_down(KeyCode::KeyE));
    sc.wait(0.7);
    sc.act("release E", |cx| cx.key_up(KeyCode::KeyE));
    sc.wait(0.6);
    sc.shot("sword_side");
    sc.act("Q", |cx| cx.key_down(KeyCode::KeyQ));
    sc.wait(0.7);
    sc.act("release Q", |cx| cx.key_up(KeyCode::KeyQ));
    sc.act("holster", |cx| cx.tap(KeyCode::Space));
    sc.wait(1.0);
    sc.act("bow: quick slot 3", |cx| cx.tap(KeyCode::Digit3));
    sc.wait(2.0);
    sc.shot("bow_ready_back");
    sc.act("hold left (draw)", |cx| cx.mouse_down(MouseButton::Left));
    sc.wait(2.6);
    sc.shot("bow_aim_back");
    sc.act("E", |cx| cx.key_down(KeyCode::KeyE));
    sc.wait(0.7);
    sc.act("release E", |cx| cx.key_up(KeyCode::KeyE));
    sc.wait(0.6);
    sc.shot("bow_aim_side");
    sc.act("Q", |cx| cx.key_down(KeyCode::KeyQ));
    sc.wait(0.7);
    sc.act("release Q", |cx| cx.key_up(KeyCode::KeyQ));
    sc.act("release", |cx| cx.mouse_up(MouseButton::Left));
    sc.wait(1.0);
    sc.act("spell: quick slot 6", |cx| cx.tap(KeyCode::Digit6));
    sc.wait(1.5);
    sc.act("hold left (cast)", |cx| cx.mouse_down(MouseButton::Left));
    sc.wait(0.35);
    sc.shot("cast_raise");
    sc.step("hold until the missile leaves", |cx| {
        let (m, ..) = shots(cx);
        if m > 0 || cx.t > 3.0 {
            Poll::Done
        } else {
            Poll::Wait
        }
    });
    sc.shot("cast_release");
    sc.act("release", |cx| cx.mouse_up(MouseButton::Left));
    sc.wait(1.0);
    sc
}
