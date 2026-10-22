//! `bow_faithful`: the hero's bow through the ported `PS_Ranged*` states with real input.
//!
//! Every step asserts a state name, a timing or a visual against the value the original gives:
//! `PS_Ranged` hands the body to the camera (movement constraint 0, `ProcessMovements` aligns it
//! every frame), the bow hangs in the left hand (`gCInventorySlot::GetBone`, `Slot_LeftHand_Weapon`)
//! with the arrow in the right, a press is `PS_Ranged_Aim` (`Aim_Raise`) then `PS_Ranged_PowerAim`
//! (`Aim_PowerRaise`, 2.0 s, the bow's `Cock` clip bends the limbs in step) and `PS_Ranged_Aim_Loop`
//! (the full draw held), the pitch of the camera blends `Aim_O_Fwd` with `Aim_O_Up` / `Aim_O_Down`,
//! the release is `PS_Ranged_Shoot` (the arrow leaves the right hand), `PS_Ranged_Aim_Reload`
//! nocks the next one (`Reload_Begin`, `Raise`, `End`: 0.28 + 0.4 + 0.28 s) and the right button
//! lowers the draw without a shot.
use super::more::approach;
use super::more2::{grab, pick_target, target_goal, tgt_id};
use super::{Cx, Poll, Sc, expect};
use crate::walk::play::HeroActor;
use crate::walk::play::ranged::{BowVisual, RangedView};
use crate::walk::play::shots::ShotsOnScreen;
use bevy::prelude::*;
use gothic_animation::Animator;
use gothic_animation::runtime::CharacterComposition;
use gothic_sim::script::routine::state::State;
use std::sync::Arc;

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

fn hero_state(cx: &Cx) -> (Option<State>, Option<State>, f32) {
    cx.core().0.sim.hero_script_state()
}

fn state_name(cx: &Cx) -> String {
    hero_state(cx)
        .1
        .map_or("-".into(), |s| s.name().to_string())
}

fn arrows(cx: &Cx) -> u32 {
    cx.world.resource::<ShotsOnScreen>().arrows_spawned
}

fn quiver(cx: &Cx) -> u32 {
    cx.count_item("Arrow")
}

/// World position (metres) of a joint of the hero.
fn hero_joint(cx: &mut Cx, name: &str) -> Option<Vec3> {
    let mut q = cx
        .world
        .query_filtered::<(&Animator, &CharacterComposition, &GlobalTransform), With<HeroActor>>();
    let (anim, comp, root) = q.single(cx.world).ok()?;
    let j = *comp.0.joint_by_name.get(name)?;
    Some(
        (root.affine() * *anim.joints.get(j as usize)?)
            .translation
            .into(),
    )
}

/// World position (metres) of a joint of the bow actor.
fn bow_joint(cx: &mut Cx, name: &str) -> Option<Vec3> {
    let mut q = cx.world.query::<(
        &BowVisual,
        &Animator,
        &CharacterComposition,
        &GlobalTransform,
    )>();
    let (_, anim, comp, root) = q.iter(cx.world).next()?;
    let j = *comp.0.joint_by_name.get(&name.to_ascii_lowercase())?;
    Some(
        (root.affine() * *anim.joints.get(j as usize)?)
            .translation
            .into(),
    )
}

/// Position of a joint of the bow actor in the bow's own frame (cm).
/// What the bow actor plays: clip name and time.
fn bow_clip(cx: &mut Cx) -> Option<(String, f32)> {
    let mut q = cx.world.query::<(&BowVisual, &Animator)>();
    let (_, anim) = q.iter(cx.world).next()?;
    Some((anim.current_name()?.to_string(), anim.current_time()))
}

fn bow_joint_local(cx: &mut Cx, name: &str) -> Option<Vec3> {
    let mut q = cx
        .world
        .query::<(&BowVisual, &Animator, &CharacterComposition)>();
    let (_, anim, comp) = q.iter(cx.world).next()?;
    let j = *comp.0.joint_by_name.get(&name.to_ascii_lowercase())?;
    Some(anim.joints.get(j as usize)?.translation.into())
}

/// World position (metres) of the bow actor's own origin.
fn bow_origin(cx: &mut Cx) -> Option<Vec3> {
    let mut q = cx.world.query::<(&BowVisual, &GlobalTransform)>();
    q.iter(cx.world).next().map(|(_, g)| g.translation())
}

/// Records every state change with the scenario time and keeps the others from attacking.
fn trace(sc: &mut Sc) {
    sc.watch(|cx| {
        // Setup, not under test: the hero must survive, and nobody else fights while the bow is
        // under test.
        {
            let hero = cx.core().0.hero;
            let max = {
                let c = cx.core();
                c.0.sim.actor(hero).hp_max(&c.0.sim.cfg)
            };
            if cx.hero_hp() < max / 2 {
                cx.core_mut().0.sim.actor_mut(hero).hp = max;
            }
            if cx.st.n.get("freeze_others").copied() == Some(1.0) {
                for a in cx.core_mut().0.sim.actors.iter_mut() {
                    if a.id != hero {
                        a.ai_controlled = false;
                    }
                }
            }
        }
        let name = state_name(cx);
        let last = cx.st.store.get("last_state").cloned().unwrap_or_default();
        if name != last {
            cx.st.store.insert("last_state".into(), name.clone());
            let now = cx.now;
            cx.st
                .store
                .entry("trace".into())
                .or_default()
                .push_str(&format!("{now:.2}:{name}\n"));
            cx.st.n.insert(format!("t_{name}"), f64::from(now));
            let prev = last;
            if !prev.is_empty() {
                cx.st.n.insert(format!("t_end_{prev}"), f64::from(now));
            }
        }
        // The overlay clips of the aim (the pitch blend) as they change.
        let clips = cx.world.resource::<RangedView>().clips.clone();
        let joined = format!("{:?}", clips);
        if cx.st.store.get("last_clips") != Some(&joined) {
            cx.st.store.insert("last_clips".into(), joined.clone());
            let now = cx.now;
            cx.st
                .store
                .entry("trace".into())
                .or_default()
                .push_str(&format!("{now:.2}:  overlay {joined}\n"));
            for c in clips.iter().flatten() {
                cx.st
                    .store
                    .entry("clips_seen".into())
                    .or_default()
                    .push_str(&format!("{c}\n"));
            }
        }
        // The body against the camera while the task keeps the movement constraint at 0.
        if matches!(hero_state(cx).0, Some(State::PsRanged)) && cx.player().placed {
            let d = crate::walk::play::angle_delta(cx.player().yaw, cx.play().hero_yaw).abs();
            let m = cx.st.n.entry("max_yaw_err".into()).or_insert(0.0);
            *m = m.max(f64::from(d));
            cx.st.n.insert("yaw_err".into(), f64::from(d));
        }
    });
}

/// Turn the camera toward the raider (plus `up` cm) until the error is small.
fn turn_to(sc: &mut Sc, label: &str, up: f32, pitch: f32) {
    sc.step(label.to_string(), move |cx| {
        if cx.t < 0.05 {
            cx.st.store.remove("clips_seen");
        }
        let Some(id) = tgt_id(cx) else {
            return Poll::Fail("no target".into());
        };
        let Some(p) = cx.actor_pos(id) else {
            return Poll::Fail("no target position".into());
        };
        let err = cx.aim_at(p + Vec3::new(0.0, up, 0.0), pitch);
        let dp = (pitch - cx.player().pitch).abs();
        if err < 0.02 && dp < 0.02 {
            Poll::Done
        } else if cx.t > 4.0 {
            Poll::Fail(format!(
                "camera did not settle: yaw error {err}, pitch error {dp}"
            ))
        } else {
            Poll::Wait
        }
    });
}

/// The line of the draw: from the right hand (the string) to the left hand (the grip), as a
/// horizontal direction (the bow is aimed along it), and its angle to where the camera looks.
fn aim_line_error(cx: &mut Cx) -> Option<f32> {
    // The arrow flies from the string (the right hand) to the bow: from the hand to the point of
    // the chord between the limb tips that is nearest to it.
    let (a, b) = (bow_joint(cx, "Bone03")?, bow_joint(cx, "Bone06")?);
    let r = hero_joint(cx, "slot_righthand_weapon")?;
    let ab = b - a;
    let t = ((r - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
    let l = a + ab * t;
    let line = Vec2::new(l.x - r.x, l.z - r.z).normalize_or_zero();
    let yaw = cx.player().yaw;
    let look = Vec2::new(-yaw.sin(), -yaw.cos());
    let right = Vec2::new(yaw.cos(), -yaw.sin());
    let hero = cx.hero_pos() * 0.01;
    let rel = |p: Vec3| {
        let d = Vec2::new(p.x - hero.x, p.z - hero.z);
        (
            d.dot(look) * 100.0,
            d.dot(right) * 100.0,
            p.y * 100.0 - hero.y * 100.0,
        )
    };
    cx.note(format!(
        "hands in the camera frame (forward, right, up cm): left {:?} right {:?}",
        rel(l),
        rel(r)
    ));
    Some(line.dot(look).clamp(-1.0, 1.0).acos())
}

fn trace_has(cx: &Cx, s: &str) -> bool {
    cx.st.store.get("trace").is_some_and(|t| t.contains(s))
}

pub fn bow_faithful() -> Sc {
    let mut sc = Sc::new(300.0);
    grab(&mut sc);
    pick_target(&mut sc);
    trace(&mut sc);
    super::ranged::visual_watch(&mut sc);
    rec(&mut sc, true);
    sc.act("setup: quiver and health", |cx| {
        let hero = cx.core().0.hero;
        cx.core_mut().0.sim.actor_mut(hero).stamina = 100;
        super::scenarios::grant(cx, "Arrow", 20);
    });
    // ---- stand 9 m from the raider, then freeze everybody else ------------------------------------
    approach(
        &mut sc,
        "the raider",
        target_goal(),
        900.0,
        -0.05,
        Arc::new(|_| true),
    );
    sc.act("setup: the others stand still", |cx| {
        cx.st.n.insert("freeze_others".into(), 1.0);
        if let Some(id) = tgt_id(cx)
            && let Some(p) = cx.actor_pos(id)
        {
            cx.aim_at(p, -0.05);
        }
    });
    sc.wait(0.6);
    // ---- the bow key: quick slot 3 --------------------------------------------------------------
    sc.act("quick slot 3: the bow", |cx| {
        let q = quiver(cx);
        cx.st.n.insert("quiver0".into(), f64::from(q));
        cx.tap(KeyCode::Digit3);
    });
    sc.until("the hero walks with the bow ready", 8.0, |cx| {
        hero_state(cx).1 == Some(State::PsRangedWalkLoop)
    });
    sc.wait(1.2);
    sc.check(
        "PS_Ranged: bow task, ranged configuration, camera-aligned body (constraint 0), combat mode",
        |cx| {
            let (task, _, _) = hero_state(cx);
            let (_, constraint) = cx.core().0.sim.hero_alignment();
            let cfg = cx.core().0.sim.hero_script_modes();
            let ai = cx.core().0.sim.actor(cx.core().0.hero).ai_mode;
            let err = cx.st.n.get("yaw_err").copied().unwrap_or(9.9);
            expect(
                task == Some(State::PsRanged)
                    && constraint == 0.0
                    && format!("{ai:?}") == "Combat"
                    && err.to_degrees() < 10.0,
                format!(
                    "task {task:?}, constraint {constraint}, ai {ai:?}, camera-body error {:.1} deg, modes {cfg:?}",
                    err.to_degrees()
                ),
                format!("task {task:?} constraint {constraint} ai {ai:?} err {err} modes {cfg:?}"),
            )
        },
    );
    sc.check(
        "the bow hangs in the LEFT hand (Slot_LeftHand_Weapon), nearer to it than to the right",
        |cx| {
            let left = hero_joint(cx, "slot_lefthand_weapon");
            let right = hero_joint(cx, "slot_righthand_weapon");
            let bow = bow_origin(cx);
            match (left, right, bow) {
                (Some(l), Some(r), Some(b)) => {
                    let (dl, dr) = (l.distance(b), r.distance(b));
                    expect(
                        dl < 0.05 && dl < dr,
                        format!(
                            "bow {:.1} cm from the left slot, {:.1} cm from the right",
                            dl * 100.0,
                            dr * 100.0
                        ),
                        format!("left {dl} right {dr}"),
                    )
                }
                other => Err(format!("missing joints or bow actor: {other:?}")),
            }
        },
    );
    sc.check(
        "the string is relaxed: the limb tips are as far apart as the Relax clip leaves them",
        |cx| {
            let (a, b) = (bow_joint(cx, "Bone03"), bow_joint(cx, "Bone06"));
            match (a, b) {
                (Some(a), Some(b)) => {
                    let tips = a.distance(b);
                    cx.st.n.insert("tips_relaxed".into(), f64::from(tips));
                    if let Some(l) = bow_joint_local(cx, "Bone03") {
                        cx.st.n.insert("tip_rx".into(), f64::from(l.x));
                        cx.st.n.insert("tip_ry".into(), f64::from(l.y));
                        cx.st.n.insert("tip_rz".into(), f64::from(l.z));
                    }
                    expect(
                        tips > 1.0,
                        format!("tips {:.0} cm apart", tips * 100.0),
                        format!("tips {tips}"),
                    )
                }
                other => Err(format!("missing bow joints: {other:?}")),
            }
        },
    );
    sc.shot("ready");
    // ---- press and hold: Aim, PowerAim, Aim_Loop --------------------------------------------------
    sc.act("hold the left button (draw)", |cx| {
        let a = arrows(cx);
        cx.st.n.insert("arrows0".into(), f64::from(a));
        cx.st.n.insert("t_press".into(), f64::from(cx.now));
        cx.mouse_down(MouseButton::Left);
    });
    sc.until("PS_Ranged_Aim", 3.0, |cx| state_name(cx) == "PS_Ranged_Aim");
    sc.until("PS_Ranged_PowerAim", 3.0, |cx| {
        state_name(cx) == "PS_Ranged_PowerAim"
    });
    sc.check(
        "Aim_Raise lasts 0.4 s (the clip), then the draw starts",
        |cx| {
            let a = cx.st.n.get("t_PS_Ranged_Aim").copied().unwrap_or(-1.0);
            let p = cx.st.n.get("t_PS_Ranged_PowerAim").copied().unwrap_or(-1.0);
            let d = p - a;
            expect(
                (0.38..0.6).contains(&d),
                format!("Aim lasted {d:.2} s"),
                format!("Aim {a:.2}, PowerAim {p:.2}"),
            )
        },
    );
    sc.wait(1.0);
    sc.check(
        "the body faces the camera while the bow is drawn; the aim overlay is Aim_PowerRaise Fwd",
        |cx| {
            let err = cx.st.n.get("yaw_err").copied().unwrap_or(9.9);
            let clips = cx.st.store.get("clips_seen").cloned().unwrap_or_default();
            expect(
                err.to_degrees() < 10.0 && clips.contains("Aim_PowerRaise_O_Fwd"),
                format!("body-camera error {:.1} deg", err.to_degrees()),
                format!("err {err}, clips {clips}"),
            )
        },
    );
    sc.until("PS_Ranged_Aim_Loop", 5.0, |cx| {
        state_name(cx) == "PS_Ranged_Aim_Loop"
    });
    sc.check(
        "PowerAim ends after the PowerRaise clip (2.0 s) in the Aim_Loop; full draw held",
        |cx| {
            let p = cx.st.n.get("t_PS_Ranged_PowerAim").copied().unwrap_or(-1.0);
            let l = cx.st.n.get("t_PS_Ranged_Aim_Loop").copied().unwrap_or(-1.0);
            let d = l - p;
            let mult = cx.core().0.sim.script.npc(cx.core().0.hero).state_time;
            let _ = mult;
            let draw = cx.world.resource::<RangedView>().draw;
            expect(
                (1.95..2.4).contains(&d),
                format!("PowerAim lasted {d:.2} s (the clip: 2.0 s), draw {draw:.2}"),
                format!("PowerAim {p:.2}, Aim_Loop {l:.2}"),
            )
        },
    );
    sc.wait(0.3);
    sc.check(
        "the string is drawn by the RIGHT hand: limbs bent in, the hand at the string's middle, pulled back from the limb tips",
        |cx| {
            let a = bow_joint(cx, "Bone03");
            let b = bow_joint(cx, "Bone06");
            let hand = hero_joint(cx, "slot_righthand_weapon");
            match (a, b, hand) {
                (Some(a), Some(b), Some(h)) => {
                    let ab = b - a;
                    let t = (h - a).dot(ab) / ab.length_squared().max(1e-9);
                    let depth = (a + ab * t.clamp(0.0, 1.0)).distance(h);
                    let tips = a.distance(b);
                    let get = |cx: &Cx, k: &str| cx.st.n.get(k).copied().unwrap_or(0.0) as f32;
                    let r = Vec3::new(
                        get(cx, "tip_rx"),
                        get(cx, "tip_ry"),
                        get(cx, "tip_rz"),
                    );
                    let moved = bow_joint_local(cx, "Bone03").map_or(0.0, |l| l.distance(r));
                    let clip = bow_clip(cx);
                    cx.note(format!("bow actor plays {clip:?}"));
                    expect(
                        (0.3..0.7).contains(&t) && (0.2..0.8).contains(&depth) && moved > 5.0,
                        format!(
                            "hand at {:.0} % of the string, {:.0} cm pulled back; the limb tip moved {:.0} cm (tips {:.0} cm apart)",
                            t * 100.0,
                            depth * 100.0,
                            moved,
                            tips * 100.0,
                        ),
                        format!("t {t} depth {depth} tips {tips} moved {moved}"),
                    )
                }
                other => Err(format!("missing joints: {other:?}")),
            }
        },
    );
    sc.check(
        "the draw points along the view: the arrow line (string hand to the bow) is within 20 degrees of the camera direction",
        |cx| match aim_line_error(cx) {
            Some(e) => expect(
                e.to_degrees() < 20.0,
                format!("{:.1} deg off", e.to_degrees()),
                format!("{:.1} deg off the camera direction", e.to_degrees()),
            ),
            None => Err("no hand joints".into()),
        },
    );
    sc.shot("drawn");
    // ---- the pitch blend ------------------------------------------------------------------------------
    turn_to(&mut sc, "look up", 3000.0, 0.45);
    sc.wait(0.8);
    sc.check("aiming up blends Aim_O_Up over Aim_O_Fwd", |cx| {
        let mut clips = cx.st.store.get("clips_seen").cloned().unwrap_or_default();
        for c in cx.world.resource::<RangedView>().clips.iter().flatten() {
            clips.push_str(c);
        }
        expect(
            clips.contains("_O_Up"),
            "an Up variant was played".to_string(),
            format!("clips {clips}"),
        )
    });
    sc.shot("aim_up");
    turn_to(&mut sc, "look down", -3000.0, -0.45);
    sc.wait(0.8);
    sc.check("aiming down blends Aim_O_Down over Aim_O_Fwd", |cx| {
        let mut clips = cx.st.store.get("clips_seen").cloned().unwrap_or_default();
        for c in cx.world.resource::<RangedView>().clips.iter().flatten() {
            clips.push_str(c);
        }
        expect(
            clips.contains("_O_Down"),
            "a Down variant was played".to_string(),
            format!("clips {clips}"),
        )
    });
    sc.shot("aim_down");
    // ---- the body follows a turn of the camera ----------------------------------------------------
    sc.act("note the camera yaw", |cx| {
        let yaw = cx.player().yaw;
        cx.st.n.insert("yaw_before".into(), f64::from(yaw));
    });
    sc.step("turn the camera by 100 degrees", |cx| {
        let yaw0 = cx.st.n["yaw_before"] as f32;
        let to = yaw0 + 100f32.to_radians();
        let hero = cx.hero_pos();
        let dir = Vec3::new(-to.sin(), 0.0, -to.cos());
        let err = cx.aim_at(hero + dir * 1000.0, -0.05);
        if err < 0.02 {
            Poll::Done
        } else if cx.t > 4.0 {
            Poll::Fail("camera did not settle".into())
        } else {
            Poll::Wait
        }
    });
    sc.wait(1.2);
    sc.check(
        "the body turned with the camera (aligned within 10 degrees after the turn model's 200 deg/s)",
        |cx| {
            let before = cx.st.n["yaw_before"] as f32;
            let now = cx.player().yaw;
            let turned = crate::walk::play::angle_delta(before, now).abs();
            let err = crate::walk::play::angle_delta(cx.player().yaw, cx.play().hero_yaw).abs();
            expect(
                turned.to_degrees() > 20.0 && err.to_degrees() < 10.0,
                format!(
                    "camera turned {:.0} deg, body-camera error {:.1} deg",
                    turned.to_degrees(),
                    err.to_degrees()
                ),
                format!("turned {turned} err {err}"),
            )
        },
    );
    turn_to(&mut sc, "face the raider again", 0.0, -0.05);
    sc.wait(0.8);
    // ---- the shot -----------------------------------------------------------------------------------------
    sc.act("release the left button (shoot)", |cx| {
        let n = arrows(cx);
        cx.st.n.insert("arrows_before".into(), f64::from(n));
        cx.st
            .n
            .insert("quiver_before".into(), f64::from(quiver(cx)));
        cx.mouse_up(MouseButton::Left);
    });
    sc.until("PS_Ranged_Shoot", 3.0, |cx| {
        state_name(cx) == "PS_Ranged_Shoot"
    });
    sc.wait(0.35);
    sc.check(
        "the arrow left the bow: a simulated shot, one arrow used up, visible in flight",
        |cx| {
            let n = arrows(cx);
            let before = cx.st.n["arrows_before"] as u32;
            let q = quiver(cx);
            let qb = cx.st.n["quiver_before"] as u32;
            let vis = cx.st.n.get("vis_arrow").copied().unwrap_or(0.0);
            expect(
                n > before && q + 1 == qb && vis > 0.0,
                format!("arrows spawned {before} -> {n}, quiver {qb} -> {q}, frames in view {vis}"),
                format!("arrows {before} -> {n}, quiver {qb} -> {q}"),
            )
        },
    );
    sc.shot("arrow_flight");
    sc.until("PS_Ranged_Aim_Reload", 3.0, |cx| {
        state_name(cx) == "PS_Ranged_Aim_Reload"
    });
    sc.until("the hero walks again", 5.0, |cx| {
        state_name(cx) == "PS_Ranged_Walk_Loop"
    });
    sc.check(
        "Aim_Reload nocks the next arrow (0.28 + 0.4 + 0.28 s) before walking again",
        |cx| {
            let s = cx
                .st
                .n
                .get("t_PS_Ranged_Aim_Reload")
                .copied()
                .unwrap_or(-1.0);
            let e = cx
                .st
                .n
                .get("t_end_PS_Ranged_Aim_Reload")
                .copied()
                .unwrap_or(-1.0);
            let d = e - s;
            let clips = cx.st.store.get("clips_seen").cloned().unwrap_or_default();
            let ok = (0.9..1.3).contains(&d)
                && clips.contains("Reload_Begin")
                && clips.contains("Reload_Raise")
                && clips.contains("Reload_End");
            expect(
                ok,
                format!("Aim_Reload lasted {d:.2} s with all three reload clips"),
                format!("lasted {d:.2}, clips {clips}"),
            )
        },
    );
    // ---- the right button gives up the draw ----------------------------------------------------------
    sc.act("click the left button, then the right button", |cx| {
        cx.st
            .n
            .insert("arrows_abort0".into(), f64::from(arrows(cx)));
        cx.st
            .n
            .insert("quiver_abort0".into(), f64::from(quiver(cx)));
        cx.mouse_down(MouseButton::Left);
    });
    sc.until("PS_Ranged_PowerAim again", 3.0, |cx| {
        state_name(cx) == "PS_Ranged_PowerAim"
    });
    sc.wait(0.5);
    sc.act("right button: give up the draw", |cx| {
        cx.mouse_click(MouseButton::Right);
    });
    sc.act("let go of the left button", |cx| {
        cx.mouse_up(MouseButton::Left);
    });
    sc.until("back to walking", 4.0, |cx| {
        state_name(cx) == "PS_Ranged_Walk_Loop"
    });
    sc.check(
        "the abort lowers the draw without a shot or a lost arrow",
        |cx| {
            let n = arrows(cx);
            let q = quiver(cx);
            let (nb, qb) = (
                cx.st.n["arrows_abort0"] as u32,
                cx.st.n["quiver_abort0"] as u32,
            );
            expect(
                n == nb && q == qb && trace_has(cx, "PS_Ranged_Reload"),
                format!("arrows {nb} -> {n}, quiver {qb} -> {q}, via PS_Ranged_Reload"),
                format!("arrows {nb} -> {n}, quiver {qb} -> {q}"),
            )
        },
    );
    // ---- the weapon key ---------------------------------------------------------------------------------------
    sc.act("Space: put the bow away", |cx| cx.tap(KeyCode::Space));
    sc.wait(1.5);
    sc.check(
        "the weapon key puts the bow on the back: PS_Normal, bow hung at Slot_Bow",
        |cx| {
            let (task, _, _) = hero_state(cx);
            let back = hero_joint(cx, "slot_bow");
            let bow = bow_origin(cx);
            let dist = match (back, bow) {
                (Some(a), Some(b)) => a.distance(b),
                _ => f32::MAX,
            };
            expect(
                task == Some(State::PsNormal) && dist < 0.05,
                format!("task {task:?}, bow {:.1} cm from Slot_Bow", dist * 100.0),
                format!("task {task:?}, dist {dist}"),
            )
        },
    );
    sc.shot("holstered");
    rec(&mut sc, false);
    sc
}
