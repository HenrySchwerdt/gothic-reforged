//! Movement-state scenarios, part 2: slide and swim / dive (see `moves.rs`).
use super::moves::{grab, hero_clip, mode, ms, settle};
use super::{Cx, Poll, Sc, expect};
use bevy::prelude::*;
use gothic_animation::locomotion::MoveKind;
use gothic_physics::MovementMode;

/// Slide on ground steeper than `SteepGroundAngleMax`: the mode, `Slide_Begin` / `Slide_End`,
/// the slide speed and the jump out of the slide (`OnSlide`, `PS_Slide_Jump`).
pub fn p4_slide() -> Sc {
    let mut sc = Sc::new(150.0);
    grab(&mut sc);
    settle(&mut sc);
    sc.act(
        "setup: a 55 degree test ramp (synthetic collision chunk) high above the start",
        |cx| {
            let o = cx.player().ctrl.position;
            let (ytop, l, t) = (o.y + 6000.0, 2000.0f32, 55.0f32.to_radians().tan());
            let p = |x: f32, z: f32| {
                let y = if x < l { ytop - x * t } else { ytop - l * t };
                Vec3::new(o.x + x, y, o.z + z)
            };
            let tris = vec![
                [p(0.0, -800.0), p(l, -800.0), p(0.0, 800.0)],
                [p(0.0, 800.0), p(l, -800.0), p(l, 800.0)],
                [p(l, -800.0), p(l + 3000.0, -800.0), p(l, 800.0)],
                [p(l, 800.0), p(l + 3000.0, -800.0), p(l + 3000.0, 800.0)],
            ];
            cx.world
                .resource_mut::<crate::walk::Collision>()
                .0
                .insert_chunk(0xFFFF_0000_0001, tris);
            cx.st.n.insert("sx".into(), f64::from(o.x + 150.0));
            cx.st.n.insert("sy".into(), f64::from(ytop - 150.0 * t));
            cx.st.n.insert("sz".into(), f64::from(o.z));
            cx.st.n.insert("sdeg".into(), 55.0);
            cx.note("synthetic 55 degree ramp, 2000 cm long, at the top of the world");
        },
    );
    sc.act("stand at the top of the ramp", |cx| {
        let (x, y, z) = (
            cx.st.n["sx"] as f32,
            cx.st.n["sy"] as f32,
            cx.st.n["sz"] as f32,
        );
        let mut p = cx.world.resource_mut::<crate::walk::Player>();
        p.ctrl.teleport(Vec3::new(x, y + 20.0, z));
    });
    sc.step("wait for the slide mode", |cx| {
        let m = mode(cx);
        if m == MovementMode::Sliding {
            return Poll::Done;
        }
        if cx.t > 6.0 {
            return Poll::Fail(format!("never slid, mode {m:?}"));
        }
        Poll::Wait
    });
    sc.act("note the slide start", |cx| {
        cx.st.n.insert("slide_t0".into(), cx.now as f64);
        cx.st.n.insert("vmax".into(), 0.0);
    });
    sc.step("sliding: the clip and the speed, the jump key", |cx| {
        let (name, kind) = hero_clip(cx);
        if name.contains("Slide_Begin") && kind == Some(MoveKind::Slide) {
            cx.st.n.insert("slide_clip".into(), 1.0);
        }
        let v = f64::from(cx.player().ctrl.velocity.length());
        let vm = cx.st.n["vmax"].max(v);
        cx.st.n.insert("vmax".into(), vm);
        let el = cx.now as f64 - cx.st.n["slide_t0"];
        if (0.3..0.4).contains(&el) && !cx.st.n.contains_key("early") {
            // too early: the jump key does nothing yet
            cx.tap(KeyCode::AltLeft);
            cx.st.n.insert("early".into(), 1.0);
        }
        if el > 0.45 && !cx.st.n.contains_key("early_mode") {
            cx.st
                .n
                .insert("early_mode".into(), f64::from(mode(cx).raw()));
        }
        if el > 0.8 && !cx.st.n.contains_key("jumped") {
            cx.tap(KeyCode::AltLeft);
            cx.st.n.insert("jumped".into(), 1.0);
        }
        if cx.st.n.contains_key("jumped") && (mode(cx) == MovementMode::Jumping || el > 2.5) {
            cx.st
                .n
                .insert("jump_mode".into(), f64::from(mode(cx).raw()));
            return Poll::Done;
        }
        if mode(cx) != MovementMode::Sliding && !cx.st.n.contains_key("jumped") {
            return Poll::Fail(format!("slide ended early, mode {:?}", mode(cx)));
        }
        if el > 8.0 {
            return Poll::Fail("slide never ended".into());
        }
        Poll::Wait
    });
    sc.shot("slide_jump");
    sc.step("ride the ramp down to the flat floor (Slide_End)", |cx| {
        let (name, kind) = hero_clip(cx);
        if name.contains("Slide_End") && kind == Some(MoveKind::SlideEnd) {
            cx.st.n.insert("slide_end_clip".into(), 1.0);
        }
        let tag = format!("{kind:?}/{}", name.rsplit("P0_").nth(1).unwrap_or(""));
        if cx.st.store.get("seq").is_none_or(|l| !l.ends_with(&tag)) {
            let l = cx.st.store.entry("seq".into()).or_default();
            l.push_str(" > ");
            l.push_str(&tag);
        }
        let m = mode(cx);
        if m != MovementMode::Sliding
            && m != MovementMode::Jumping
            && m != MovementMode::Falling
            && cx.player().ctrl.grounded
            && cx.t > 1.0
        {
            // watch the clips for another second after the slide ended
            let t0 = *cx.st.n.entry("ride_end_t".into()).or_insert(cx.t as f64);
            cx.st.n.insert("end_mode".into(), f64::from(m.raw()));
            if cx.t as f64 - t0 > 1.2 {
                return Poll::Done;
            }
        }
        if cx.t > 25.0 {
            return Poll::Fail(format!("still {m:?} after 25 s"));
        }
        Poll::Wait
    });
    sc.wait(1.0);
    sc.check(
        "the slide mode was entered on ground steeper than 49 degrees",
        |cx| {
            let deg = cx.st.n["sdeg"];
            expect(
                ms(cx).slides >= 1 && deg > 49.0,
                format!("{} slide(s) on {deg:.1} deg", ms(cx).slides),
                format!("slides {}", ms(cx).slides),
            )
        },
    );
    sc.check("Slide_Begin clip plays while sliding", |cx| {
        expect(
            cx.st.n.contains_key("slide_clip"),
            "Slide_Begin during mode 0xc",
            "no Slide_Begin clip",
        )
    });
    sc.check("slide speed grows toward SlideSpeedMax 800 cm/s along the slope (horizontal part = x cos 55)", |cx| {
        let v = cx.st.n["vmax"];
        expect(
            v > 300.0 && v <= 820.0,
            format!("max {v:.0} cm/s"),
            format!("max {v:.0}"),
        )
    });
    sc.check("the jump key is ignored before 0.6 s of sliding", |cx| {
        let m = cx.st.n.get("early_mode").copied().unwrap_or(12.0);
        expect(
            m as u8 == 0xc,
            format!("mode {m} at 0.45 s"),
            format!("mode {m} at 0.45 s"),
        )
    });
    sc.check(
        "the slide ends on flat ground with the Slide_End clip",
        |cx| {
            let m = cx.st.n.get("end_mode").copied().unwrap_or(-1.0);
            expect(
                cx.st.n.contains_key("slide_end_clip") && m >= 1.0,
                format!("mode {m} after the slide, Slide_End played"),
                format!(
                    "mode {m}, Slide_End seen: {}; sequence {}",
                    cx.st.n.contains_key("slide_end_clip"),
                    cx.st.store.get("seq").cloned().unwrap_or_default()
                ),
            )
        },
    );
    sc.check(
        "jump out of the slide after 0.6 s (OnSlide -> PS_Slide_Jump)",
        |cx| {
            let m = cx.st.n["jump_mode"] as u8;
            expect(
                m == 6 || ms(cx).slide_jumps >= 1,
                format!("mode {m}, {} slide jump(s)", ms(cx).slide_jumps),
                format!("mode {m}"),
            )
        },
    );
    sc
}

/// Swimming and diving: modes, the swim clips, holstered items, up / down keys.
pub fn p4_swim() -> Sc {
    let mut sc = Sc::new(260.0);
    grab(&mut sc);
    sc.act(
        "setup: teleport to the slope above the south coast of Ardea",
        |cx| {
            let mut p = cx.world.resource_mut::<crate::walk::Player>();
            p.ctrl.position = Vec3::new(6400.0, 190.0, -12_500.0);
            p.ctrl.vertical_speed = 0.0;
            p.yaw = 0.0;
        },
    );
    sc.until("grounded at the beach", 60.0, |cx| {
        cx.player().ctrl.grounded
    });
    sc.act("draw the weapon (Space)", |cx| cx.tap(KeyCode::Space));
    sc.wait(2.5);
    sc.act("note the drawn state", |cx| {
        let d = cx.play().drawn;
        cx.st.n.insert("drawn0".into(), f64::from(u8::from(d)));
    });
    sc.step(
        "walk south into the sea until swimming (W), up to 120 s",
        |cx| {
            let goal = Vec3::new(6400.0, 0.0, -21_000.0);
            cx.aim_at(goal, -0.2);
            cx.key_down(KeyCode::KeyW);
            let m = mode(cx);
            if m == MovementMode::Wading {
                cx.st.n.insert("waded".into(), 1.0);
                let (name, _) = hero_clip(cx);
                if name.contains("Hero_Wade") {
                    cx.st.n.insert("wade_clip".into(), 1.0);
                }
            }
            if m == MovementMode::Swimming {
                let p = cx.player().ctrl.position;
                cx.st.n.insert("swim_x".into(), f64::from(p.x));
                cx.st.n.insert("swim_y".into(), f64::from(p.y));
                cx.st.n.insert("swim_z".into(), f64::from(p.z));
                return Poll::Done;
            }
            if (cx.t as u32).is_multiple_of(10) && cx.t.fract() < 0.03 {
                let p = cx.player().ctrl.position;
                cx.note(format!("t={:.0} pos {p:?} mode {m:?}", cx.t));
            }
            if cx.t > 120.0 {
                cx.release_all();
                let p = cx.player().ctrl.position;
                return Poll::Fail(format!("never swam: {p:?} mode {m:?}"));
            }
            Poll::Wait
        },
    );
    sc.wait(1.5);
    sc.shot("swimming");
    sc.check("swim mode entered, PS_Swim made the swim ani state", |cx| {
        let hero = cx.core().0.hero;
        let ani = cx.core().0.sim.actor(hero).ani_state;
        expect(
            mode(cx) == MovementMode::Swimming && ani == gothic_sim::types::AniState::Swim,
            format!("mode {:?}, ani {ani:?}", mode(cx)),
            format!("mode {:?}, ani {ani:?}", mode(cx)),
        )
    });
    sc.check("PS_Swim holsters the weapon (HoldStacks -1,-1)", |cx| {
        let drawn0 = cx.st.n["drawn0"];
        expect(
            !cx.play().drawn,
            format!("drawn before {drawn0}, now {}", cx.play().drawn),
            "weapon still drawn while swimming",
        )
    });
    sc.check("the swim pose clips play", |cx| {
        let (name, _) = hero_clip(cx);
        expect(
            name.contains("Hero_Swim"),
            name.clone(),
            format!("clip {name}"),
        )
    });
    sc.act("dive: hold Ctrl (Down)", |cx| {
        cx.st
            .n
            .insert("surface_y".into(), f64::from(cx.player().ctrl.position.y));
        cx.key_down(KeyCode::ControlLeft);
    });
    sc.step("until diving below the surface", |cx| {
        if mode(cx) == MovementMode::Diving && cx.player().ctrl.dive_depth() > 120.0 {
            return Poll::Done;
        }
        if cx.t > 8.0 {
            return Poll::Fail(format!(
                "not diving: mode {:?}, depth {}",
                mode(cx),
                cx.player().ctrl.dive_depth()
            ));
        }
        Poll::Wait
    });
    sc.wait(0.8);
    sc.shot("diving");
    sc.check("diving mode, lower than the surface swim line", |cx| {
        let dy = cx.st.n["surface_y"] as f32 - cx.player().ctrl.position.y;
        expect(
            dy > 80.0,
            format!(
                "{dy:.0} cm below the swim line, depth {:.0}",
                cx.player().ctrl.dive_depth()
            ),
            format!("{dy:.0} cm"),
        )
    });
    sc.act("surface: release Ctrl, hold Alt (Up)", |cx| {
        cx.key_up(KeyCode::ControlLeft);
        cx.key_down(KeyCode::AltLeft);
    });
    sc.step("until swimming at the surface", |cx| {
        if mode(cx) == MovementMode::Swimming && cx.player().ctrl.dive_depth() < 5.0 {
            cx.key_up(KeyCode::AltLeft);
            return Poll::Done;
        }
        if cx.t > 10.0 {
            cx.key_up(KeyCode::AltLeft);
            return Poll::Fail(format!("mode {:?}", mode(cx)));
        }
        Poll::Wait
    });
    sc.check("resurfaced", |cx| {
        expect(
            mode(cx) == MovementMode::Swimming,
            format!("mode {:?}", mode(cx)),
            format!("mode {:?}", mode(cx)),
        )
    });
    sc.act("back to the beach (S)", |cx| {
        cx.aim_at(Vec3::new(6400.0, 0.0, -12_500.0), -0.2);
        cx.key_down(KeyCode::KeyW);
    });
    sc.step("until the hero stands again", |cx| {
        cx.aim_at(Vec3::new(6400.0, 0.0, -12_500.0), -0.2);
        if cx.player().ctrl.grounded
            && !matches!(mode(cx), MovementMode::Swimming | MovementMode::Diving)
        {
            cx.release_all();
            return Poll::Done;
        }
        if cx.t > 60.0 {
            cx.release_all();
            return Poll::Fail(format!("mode {:?}", mode(cx)));
        }
        Poll::Wait
    });
    sc.wait(0.5);
    sc.check("wading clips (Hero_Wade) played in shallow water", |cx| {
        expect(
            cx.st.n.contains_key("wade_clip"),
            "Hero_Wade clip while wading",
            "never saw a Hero_Wade clip",
        )
    });
    sc.check(
        "left the water: PS_Swim ended, the ani state is back to stand",
        |cx| {
            let hero = cx.core().0.hero;
            let ani = cx.core().0.sim.actor(hero).ani_state;
            expect(
                ani == gothic_sim::types::AniState::Stand,
                format!("ani {ani:?}"),
                format!("ani {ani:?}"),
            )
        },
    );
    // Falling into deep water: no damage, `ZS_DiveIn`, swim pose; a very long fall teaches
    // Perk_Acrobatic (OnMovementModeChanged: S:adbf0, 7.45 s at S:0x10107658).
    for (tag, h, secs_min) in [("dive", 1500.0f32, 0.0f32), ("long", 11500.0, 7.45)] {
        let t1 = tag.to_string();
        sc.act(
            format!("drop into the sea from {h} cm ({tag})"),
            move |cx| {
                let (x, y, z) = (
                    cx.st.n["swim_x"] as f32,
                    cx.st.n["swim_y"] as f32,
                    cx.st.n["swim_z"] as f32,
                );
                let hero = cx.core().0.hero;
                let max = {
                    let c = cx.core();
                    c.0.sim.actor(hero).hp_max(&c.0.sim.cfg)
                };
                cx.core_mut().0.sim.actors[hero.0 as usize].hp = max;
                cx.st.n.insert(format!("{t1}_dive_clip"), 0.0);
                let mut p = cx.world.resource_mut::<crate::walk::Player>();
                p.ctrl.last_landing = None;
                p.ctrl.teleport(Vec3::new(x, y + h, z));
            },
        );
        let t2 = tag.to_string();
        sc.step(format!("until swimming again ({tag})"), move |cx| {
            let (name, kind) = hero_clip(cx);
            if name.contains("Hero_Swim")
                && name.contains("Fall_Hit")
                && kind == Some(MoveKind::DiveIn)
            {
                cx.st.n.insert(format!("{t2}_dive_clip"), 1.0);
            }
            if mode(cx) == MovementMode::Swimming && cx.t > 1.0 {
                if cx.st.n[&format!("{t2}_dive_clip")] < 1.0 && cx.t < 20.0 {
                    return Poll::Wait;
                }
                return Poll::Done;
            }
            if cx.t > 40.0 {
                return Poll::Fail(format!(
                    "never swam: {:?} {:?}",
                    mode(cx),
                    cx.player().ctrl.position
                ));
            }
            Poll::Wait
        });
        sc.wait(0.5);
        let t3 = tag.to_string();
        sc.check(
            format!("{tag}: falling into deep water costs no hit points"),
            move |cx| {
                let hero = cx.core().0.hero;
                let max = {
                    let c = cx.core();
                    c.0.sim.actor(hero).hp_max(&c.0.sim.cfg)
                };
                let _ = &t3;
                expect(
                    cx.hero_hp() == max,
                    format!("hp {}/{max}", cx.hero_hp()),
                    format!("hp {}/{max}", cx.hero_hp()),
                )
            },
        );
        let t4 = tag.to_string();
        sc.check(
            format!("{tag}: the dive-in clip (Swim pose Fall_Hit) played"),
            move |cx| {
                expect(
                    cx.st.n[&format!("{t4}_dive_clip")] > 0.5,
                    "ZS_DiveIn clip",
                    "no Hero_Swim Fall_Hit clip",
                )
            },
        );
        if secs_min > 0.0 {
            sc.check(
                "a fall of more than 7.45 s into water teaches Perk_Acrobatic",
                |cx| {
                    let hero = cx.core().0.hero;
                    let has = cx.core().0.sim.actor(hero).has_perk("perk_acrobatic");
                    expect(has, "Perk_Acrobatic learned", "perk not learned")
                },
            );
        }
        sc.shot(&format!("{tag}_in_water"));
    }
    sc
}
