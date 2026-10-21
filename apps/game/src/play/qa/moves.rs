//! Movement-state scenarios (`p4_*`): jump, fall damage and landing clips, slide, swim and dive.
//!
//! Each scenario asserts the state that was entered (movement mode machine), the clip that plays
//! (`Animator::current_name`), the hit points against the original formula
//! `floor(160 * (t - 0.9)^2)` (`OnMovementModeChanged`, S:adbf0) and the positions.
use super::{Cx, Poll, Sc, expect};
use crate::walk::play::HeroActor;
use crate::walk::play::move_states::MoveStates;
use bevy::prelude::*;
use gothic_animation::locomotion::MoveKind;
use gothic_animation::{Animator, Locomotion};
use gothic_physics::MovementMode;

/// The hero's current clip name and locomotion kind.
pub(super) fn hero_clip(cx: &mut Cx) -> (String, Option<MoveKind>) {
    let mut q = cx
        .world
        .query_filtered::<(&Animator, &Locomotion), With<HeroActor>>();
    q.iter(cx.world)
        .next()
        .map(|(a, l)| (a.current_name().unwrap_or("").to_string(), Some(l.key.kind)))
        .unwrap_or_default()
}

pub(super) fn mode(cx: &Cx) -> MovementMode {
    cx.player().ctrl.machine.mode()
}

pub(super) fn ms<'a>(cx: &'a Cx<'_>) -> &'a MoveStates {
    cx.world.resource::<MoveStates>()
}

pub(super) fn grab(sc: &mut Sc) {
    sc.wait(1.0);
    sc.act("grab the mouse with a click", |cx| {
        cx.cursor_to(Vec2::new(640.0, 360.0));
        cx.mouse_click(MouseButton::Left);
    });
    sc.wait(0.4);
}

/// Wait until the hero stands on the ground at the start.
pub(super) fn settle(sc: &mut Sc) {
    sc.until("grounded at the start", 60.0, |cx| {
        cx.player().ctrl.grounded && cx.player().placed
    });
    sc.wait(1.0);
}

/// Jump height / air time / distance, standing and running; the `Jump_Stand` clip.
pub fn p4_jump() -> Sc {
    let mut sc = Sc::new(120.0);
    grab(&mut sc);
    settle(&mut sc);
    sc.act("note the start", |cx| {
        let p = cx.player().ctrl.position;
        cx.st.n.insert("y0".into(), f64::from(p.y));
        cx.note(format!("start {p:?}"));
    });
    sc.act("press the jump key (Alt)", |cx| {
        cx.st.n.insert("peak".into(), 0.0);
        cx.tap(KeyCode::AltLeft);
    });
    sc.step(
        "airborne: record the apex and the clip, until landed",
        |cx| {
            let y0 = cx.st.n["y0"] as f32;
            let y = cx.player().ctrl.position.y;
            let peak = cx.st.n["peak"].max(f64::from(y - y0));
            cx.st.n.insert("peak".into(), peak);
            let m = mode(cx);
            if m == MovementMode::Jumping {
                let (name, kind) = hero_clip(cx);
                if name.contains("Jump_Stand") && kind == Some(MoveKind::Jump) {
                    cx.st.n.insert("jump_clip".into(), 1.0);
                    cx.st.n.insert("jump_kind".into(), 1.0);
                }
                cx.st.n.entry("air0".into()).or_insert(cx.now as f64);
            }
            if cx.player().ctrl.grounded && cx.t > 0.3 {
                let air = cx.now as f64 - cx.st.n.get("air0").copied().unwrap_or(cx.now as f64);
                cx.st.n.insert("air".into(), air);
                return Poll::Done;
            }
            if cx.t > 5.0 {
                return Poll::Fail("never landed".into());
            }
            Poll::Wait
        },
    );
    sc.check("standing jump: apex ~183.5 cm (600 cm/s, g 981)", |cx| {
        let peak = cx.st.n["peak"];
        expect(
            (peak - 183.5).abs() < 14.0,
            format!("apex {peak:.1} cm"),
            format!("apex {peak:.1} cm, want 183.5"),
        )
    });
    sc.check("air time ~1.22 s", |cx| {
        let air = cx.st.n["air"];
        expect(
            (air - 1.22).abs() < 0.25,
            format!("{air:.2} s"),
            format!("{air:.2} s, want 1.22"),
        )
    });
    sc.check("the jump mode and the Jump_Stand clip played", |cx| {
        expect(
            cx.st.n.contains_key("jump_clip") && cx.st.n["jump_kind"] > 0.5,
            "Jump_Stand clip during mode 6",
            "no Jump_Stand clip while jumping",
        )
    });
    sc.check("a free fall (jump) costs no hit points", |cx| {
        let hp0 = cx
            .core()
            .0
            .sim
            .actor(cx.core().0.hero)
            .hp_max(&cx.core().0.sim.cfg);
        expect(
            cx.hero_hp() >= hp0 - 1,
            format!("hp {}", cx.hero_hp()),
            "lost hp",
        )
    });
    sc.wait(1.2);
    sc.act("running jump: W + Alt", |cx| {
        let p = cx.player().ctrl.position;
        cx.st.n.insert("rx".into(), f64::from(p.x));
        cx.st.n.insert("rz".into(), f64::from(p.z));
        cx.st.n.insert("peak".into(), 0.0);
        cx.key_down(KeyCode::KeyW);
    });
    sc.wait(1.0);
    sc.act("jump at full run", |cx| {
        let p = cx.player().ctrl.position;
        cx.st.n.insert("jx".into(), f64::from(p.x));
        cx.st.n.insert("jz".into(), f64::from(p.z));
        cx.st.n.insert("y0".into(), f64::from(p.y));
        cx.tap(KeyCode::AltLeft);
    });
    sc.step("record the running jump until it lands", |cx| {
        if cx.t > 0.1 && cx.player().ctrl.grounded {
            let p = cx.player().ctrl.position;
            let (dx, dz) = (p.x - cx.st.n["jx"] as f32, p.z - cx.st.n["jz"] as f32);
            cx.st.n.insert("jump_dist".into(), f64::from(dx.hypot(dz)));
            cx.key_up(KeyCode::KeyW);
            return Poll::Done;
        }
        let y0 = cx.st.n["y0"] as f32;
        let peak = cx.st.n["peak"].max(f64::from(cx.player().ctrl.position.y - y0));
        cx.st.n.insert("peak".into(), peak);
        if cx.t > 5.0 {
            cx.key_up(KeyCode::KeyW);
            return Poll::Fail("never landed".into());
        }
        Poll::Wait
    });
    sc.check(
        "running jump covers ~ run speed x air time (400 cm/s x 1.22 s)",
        |cx| {
            let d = cx.st.n["jump_dist"];
            expect(
                (330.0..=620.0).contains(&d),
                format!("{d:.0} cm"),
                format!("{d:.0} cm"),
            )
        },
    );
    sc.shot("after_jump");
    sc
}

/// Fall damage and landing clips at several heights against the original formula.
pub fn p4_fall() -> Sc {
    let mut sc = Sc::new(150.0);
    grab(&mut sc);
    settle(&mut sc);
    sc.act("note the ground", |cx| {
        let p = cx.player().ctrl.position;
        cx.st.n.insert("gx".into(), f64::from(p.x));
        cx.st.n.insert("gy".into(), f64::from(p.y));
        cx.st.n.insert("gz".into(), f64::from(p.z));
        cx.st.n.insert("hp0".into(), f64::from(cx.hero_hp()));
    });
    for (tag, h) in [("low", 250.0f32), ("mid", 1104.0), ("high", 1500.0)] {
        let tag_ = tag.to_string();
        sc.act(format!("drop from {h} cm ({tag})"), move |cx| {
            let (gx, gy, gz) = (
                cx.st.n["gx"] as f32,
                cx.st.n["gy"] as f32,
                cx.st.n["gz"] as f32,
            );
            // Full health, then fall.
            let hero = cx.core().0.hero;
            let max = {
                let c = cx.core();
                c.0.sim.actor(hero).hp_max(&c.0.sim.cfg)
            };
            cx.core_mut().0.sim.actors[hero.0 as usize].hp = max;
            cx.st.n.insert(format!("{tag}_hp0"), f64::from(max));
            cx.st.n.insert(format!("{tag}_clip"), 0.0);
            cx.st.n.insert(format!("{tag}_moved"), 0.0);
            cx.st.n.remove(&format!("{tag_}_landed_at"));
            cx.st.n.remove(&format!("{tag_}_hx"));
            let mut p = cx.world.resource_mut::<crate::walk::Player>();
            p.ctrl.last_landing = None;
            p.ctrl.teleport(Vec3::new(gx, gy + h, gz));
        });
        let tag2 = tag.to_string();
        sc.step(format!("falls and lands ({tag})"), move |cx| {
            let (name, kind) = hero_clip(cx);
            let landed = cx.player().ctrl.last_landing;
            if name.contains("Fall_Hit") && kind == Some(MoveKind::HeavyLand) {
                cx.st.n.insert(format!("{tag2}_clip"), 2.0);
            } else if (name.contains("Fall_End") || name.contains("Fall_Recover"))
                && cx.st.n[&format!("{tag2}_clip")] < 1.0
            {
                cx.st.n.insert(format!("{tag2}_clip"), 1.0);
            }
            if let Some(l) = landed {
                cx.st.n.insert(format!("{tag2}_t"), f64::from(l.fall_time));
                let at = *cx
                    .st
                    .n
                    .entry(format!("{tag2}_landed_at"))
                    .or_insert(cx.now as f64);
                let p = cx.player().ctrl.position;
                let x0 = *cx
                    .st
                    .n
                    .entry(format!("{tag2}_hx"))
                    .or_insert(f64::from(p.x));
                let z0 = *cx
                    .st
                    .n
                    .entry(format!("{tag2}_hz"))
                    .or_insert(f64::from(p.z));
                // The hero tries to walk on while the heavy landing clip plays.
                if cx.now as f64 - at < 1.2 {
                    cx.key_down(KeyCode::KeyW);
                    let d = (f64::from(p.x) - x0).hypot(f64::from(p.z) - z0);
                    let m = cx.st.n.entry(format!("{tag2}_moved")).or_insert(0.0);
                    *m = m.max(d);
                } else {
                    cx.key_up(KeyCode::KeyW);
                }
                if cx.now as f64 - at > 4.0 {
                    cx.key_up(KeyCode::KeyW);
                    return Poll::Done;
                }
            }
            if cx.t > 14.0 {
                cx.key_up(KeyCode::KeyW);
                return Poll::Fail("no landing".into());
            }
            Poll::Wait
        });
        sc.act(format!("clear the landing ({tag})"), |cx| {
            cx.key_up(KeyCode::KeyW);
        });
        let tag3 = tag.to_string();
        sc.check(
            format!("{tag}: hit points lost = floor(160 (t-0.9)^2)"),
            move |cx| {
                let t = cx.st.n[&format!("{tag3}_t")] as f32;
                let want = if t > 0.9 {
                    (160.0 * (t - 0.9) * (t - 0.9)).floor() as i32
                } else {
                    0
                };
                let lost = cx.st.n[&format!("{tag3}_hp0")] as i32 - cx.hero_hp();
                expect(
                    lost == want,
                    format!("fell {t:.3} s: lost {lost} hp (formula {want})"),
                    format!("fell {t:.3} s: lost {lost} hp, formula {want}"),
                )
            },
        );
        let tag4 = tag.to_string();
        sc.check(
            format!("{tag}: the landing clip matches the damage"),
            move |cx| {
                let clip = cx.st.n[&format!("{tag4}_clip")];
                let t = cx.st.n[&format!("{tag4}_t")] as f32;
                let heavy = t > 0.9;
                expect(
                    if heavy { clip == 2.0 } else { clip >= 1.0 },
                    format!("clip code {clip} (2 = Fall_Hit, 1 = Fall_End)"),
                    format!("clip code {clip}, heavy {heavy}"),
                )
            },
        );
        if h > 700.0 {
            let tag5 = tag.to_string();
            sc.check(
                format!("{tag}: no walking away during ZS_HeavyLand"),
                move |cx| {
                    let m = cx.st.n[&format!("{tag5}_moved")];
                    expect(
                        m < 30.0,
                        format!("moved {m:.0} cm"),
                        format!("moved {m:.0} cm"),
                    )
                },
            );
        }
        sc.wait(1.0);
    }
    sc.check("fall damage events reached the simulation", |cx| {
        let n = ms(cx).landings;
        expect(
            n >= 1,
            format!("{n} damaging landings"),
            "no damaging landing",
        )
    });
    sc.shot("after_falls");
    sc
}
