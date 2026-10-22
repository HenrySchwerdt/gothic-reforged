//! `magic_faithful`: the hero's spells through the ported `PS_Magic*` states with real input.
//!
//! Every step asserts a state name, a clip name, a timing or a visual against the original: the
//! spell rune is held in the RIGHT hand (`gCInventorySlot::GetBone`, `Slot_RightHand_Weapon`),
//! `PS_Magic_Loop` gives the body to the camera, the cast key queues the spell's cast action
//! (`GetCastAction`), `PS_Magic_PowerCast` pays the mana when `_AI_Cast` begins, plays
//! `Cast_Raise` (the length of its clip), executes the cast (the missile leaves the right hand),
//! then `Hit` and `Recover`; `PS_Magic_Reload` brings the hero back to the loop; a key let go
//! before 0.7 s aborts and refunds; the weapon key puts the spell away.
use super::more::approach;
use super::more2::{grab, pick_target, target_goal};
use super::{Cx, Poll, Sc, expect};
use crate::walk::play::HeroActor;
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

fn mana(cx: &Cx) -> i32 {
    cx.core().0.sim.actor(cx.core().0.hero).mana
}

fn spells(cx: &Cx) -> u32 {
    cx.world.resource::<ShotsOnScreen>().spell_spawned
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

/// The overlay clip the hero plays now.
fn overlay_name(cx: &mut Cx) -> Option<String> {
    let mut q = cx.world.query_filtered::<&Animator, With<HeroActor>>();
    let a = q.single(cx.world).ok()?;
    a.overlay.as_ref().map(|o| o.motion.clip.name.clone())
}

/// Records state changes, the overlay clips and the first position of a new missile against the
/// right hand; keeps the hero alive and the others quiet.
fn trace(sc: &mut Sc) {
    sc.watch(|cx| {
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
            if !last.is_empty() {
                cx.st.n.insert(format!("t_end_{last}"), f64::from(now));
            }
            cx.st
                .n
                .insert(format!("mana_at_{name}"), f64::from(mana(cx)));
        }
        if let Some(o) = overlay_name(cx)
            && cx.st.store.get("last_overlay") != Some(&o)
        {
            cx.st.store.insert("last_overlay".into(), o.clone());
            let now = cx.now;
            cx.st
                .store
                .entry("overlays".into())
                .or_default()
                .push_str(&format!("{now:.2}:{o}\n"));
        }
        // A new spell missile: where is it against the right-hand slot?
        let mut fresh: Vec<(u64, Vec3)> = cx
            .core()
            .0
            .sim
            .script
            .magic
            .flights
            .iter()
            .filter(|f| !f.shot.spec.spell.is_empty())
            .map(|f| (f.shot.id, f.shot.projectile.previous))
            .collect();
        fresh.retain(|(id, _)| cx.st.n.get(&format!("seen_{id}")).is_none());
        for (id, pos) in fresh {
            cx.st.n.insert(format!("seen_{id}"), 1.0);
            if let Some(hand) = hero_joint(cx, "slot_righthand_weapon") {
                let d = (pos * 0.01).distance(hand);
                cx.st.n.insert("missile_hand_dist".into(), f64::from(d));
                cx.st.n.insert("t_missile".into(), f64::from(cx.now));
                cx.st
                    .n
                    .insert("mana_at_missile".into(), f64::from(mana(cx)));
            }
        }
        if matches!(hero_state(cx).0, Some(State::PsMagic)) && cx.player().placed {
            let d = crate::walk::play::angle_delta(cx.player().yaw, cx.play().hero_yaw).abs();
            cx.st.n.insert("yaw_err".into(), f64::from(d));
        }
    });
}

/// Turn the camera toward the raider until the error is small.
fn turn_to(sc: &mut Sc, label: &str, yaw_add: f32) {
    let trace_body = label == "turn the camera by 100 degrees";
    let mut turn = None;
    let mut last_t = None;
    sc.step(label.to_string(), move |cx| {
        if turn.is_none() {
            if trace_body && !cx.st.n.contains_key("magic_body_trace_started") {
                cx.st.n.insert("magic_body_trace_started".into(), 1.0);
                if let Some(trace) = cx
                    .world
                    .resource_mut::<crate::walk::Player>()
                    .magic_body_trace
                    .as_mut()
                {
                    trace.begin();
                }
            }
            let yaw = cx.player().yaw;
            turn = Some(gothic_camera::aim::ScriptedYawTurn::new(yaw, yaw_add));
            last_t = Some(cx.now);
        }

        let yaw = cx.player().yaw;
        let dt = last_t.map_or(0.0, |t| (cx.now - t).clamp(0.0, 0.1));
        last_t = Some(cx.now);
        let gain = cx
            .world
            .resource::<crate::walk::play::cam::PlayCam>()
            .state
            .yaw_input_gain();
        let (counts, settled) = turn.as_mut().unwrap().update(yaw, dt, gain);
        if counts != 0.0 {
            cx.mouse_motion(Vec2::new(counts, 0.0));
        }
        if settled {
            Poll::Done
        } else if cx.t > 4.0 {
            Poll::Fail("camera did not settle".into())
        } else {
            Poll::Wait
        }
    });
}

pub fn magic_faithful() -> Sc {
    let mut sc = Sc::new(300.0);
    grab(&mut sc);
    pick_target(&mut sc);
    trace(&mut sc);
    super::ranged::visual_watch(&mut sc);
    rec(&mut sc, true);
    sc.act("setup: fireball rune in quick slot 6, mana", |cx| {
        cx.world
            .resource_mut::<crate::walk::play::PlayState>()
            .quick[5] = Some("It_Spell_Fireball".into());
        super::scenarios::grant(cx, "It_Spell_Fireball", 1);
        let hero = cx.core().0.hero;
        let mut core = cx.core_mut();
        let a = core.0.sim.actor_mut(hero);
        a.mana = a.mana.max(100);
        a.stamina = a.stamina.max(100);
    });
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
    });
    turn_to(&mut sc, "face the raider", 0.0);
    sc.wait(0.3);
    // ---- the spell key --------------------------------------------------------------------------------
    sc.act("quick slot 6: the fireball rune", |cx| {
        cx.tap(KeyCode::Digit6)
    });
    sc.until("the hero stands in PS_Magic_Loop", 8.0, |cx| {
        hero_state(cx).1 == Some(State::PsMagicLoop)
    });
    sc.wait(1.0);
    sc.check(
        "PS_Magic: the magic task, the rune in the RIGHT hand, left hand empty, body on the camera",
        |cx| {
            let (task, _, _) = hero_state(cx);
            let (_, constraint) = cx.core().0.sim.hero_alignment();
            let modes = cx.core().0.sim.hero_script_modes();
            let (right, left) = {
                let c = cx.core();
                let eq = &c.0.sim.actor(c.0.hero).equipment;
                (
                    eq.right.as_ref().map(|i| i.name.clone()),
                    eq.left.as_ref().map(|i| i.name.clone()),
                )
            };
            let err = cx.st.n.get("yaw_err").copied().unwrap_or(9.9);
            expect(
                task == Some(State::PsMagic)
                    && right.as_deref() == Some("It_Spell_Fireball")
                    && left.is_none()
                    && constraint == 0.0
                    && modes.1.0 == 4
                    && err.to_degrees() < 10.0,
                format!(
                    "task {task:?}, right {right:?}, left {left:?}, constraint {constraint}, focus mode {}, body-camera error {:.1} deg",
                    modes.1.0,
                    err.to_degrees()
                ),
                format!("task {task:?} right {right:?} left {left:?} constraint {constraint} modes {modes:?} err {err}"),
            )
        },
    );
    sc.shot("ready");
    turn_to(
        &mut sc,
        "turn the camera by 100 degrees",
        100f32.to_radians(),
    );
    sc.wait(1.0);
    sc.check("the body follows the camera in PS_Magic_Loop", |cx| {
        let err = cx.st.n.get("yaw_err").copied().unwrap_or(9.9);
        let passed = err.to_degrees() < 10.0;
        if cx.player().magic_body_trace.is_some() {
            let (
                heading,
                player_yaw,
                command,
                fly,
                placed,
                face,
                velocity,
                mode,
                turn_speed,
                turn_accel,
                turn_decel,
            ) = {
                let player = cx.player();
                (
                    player.ctrl.heading,
                    player.yaw,
                    player.command,
                    player.fly,
                    player.placed,
                    player.face.map(|face| (-face.x).atan2(-face.z)),
                    player.ctrl.turn.velocity_deg,
                    player.ctrl.machine.mode(),
                    player.ctrl.config.turn_speed_max_deg,
                    player.ctrl.config.turn_accel_deg,
                    player.ctrl.config.turn_decel_deg,
                )
            };
            let hero_yaw_post_sync = cx.play().hero_yaw;
            let (alignment, constraint) = cx.core().0.sim.hero_alignment();
            let script_modes = cx.core().0.sim.hero_script_modes();
            let camera_yaw_post_camera = {
                let mut camera = cx
                    .world
                    .query_filtered::<&Transform, With<crate::walk::PlayerCamera>>();
                camera.single(cx.world).ok().map(|transform| {
                    transform.rotation.to_euler(EulerRot::YXZ).0
                })
            };
            let snapshot = format!(
                "phase=post_wait_assert qa_time={:.3}s result={} yaw_error_deg={:.3} controller_heading_post_sync={heading:.6} hero_yaw_post_sync={hero_yaw_post_sync:.6} player_yaw={player_yaw:.6} player_command={command} fly={fly} placed={placed} camera_yaw_post_camera={camera_yaw_post_camera:?} face_target_yaw={face:?} turn_velocity_deg_s={velocity:.3} mode={mode:?} loaded_turn_profile_deg_s={turn_speed:.3} accel_deg_s2={turn_accel:.3} decel_deg_s2={turn_decel:.3} alignment={alignment:?} constraint_deg={constraint:.3} script_modes={script_modes:?}",
                cx.now,
                if passed { "PASS" } else { "FAIL" },
                err.to_degrees(),
            );
            if let Some(trace) = cx
                .world
                .resource_mut::<crate::walk::Player>()
                .magic_body_trace
                .as_mut()
            {
                eprint!("{}", trace.finish(snapshot));
            }
        }
        expect(
            passed,
            format!("body-camera error {:.1} deg", err.to_degrees()),
            format!("error {err}"),
        )
    });
    turn_to(&mut sc, "face the raider again", 0.0);
    sc.wait(1.8);
    // ---- the cast ------------------------------------------------------------------------------------------
    sc.act("hold the left button (cast key)", |cx| {
        let n = spells(cx);
        cx.st.n.insert("spells0".into(), f64::from(n));
        cx.st.n.insert("mana0".into(), f64::from(mana(cx)));
        cx.mouse_down(MouseButton::Left);
    });
    sc.until("the cast state", 3.0, |cx| {
        matches!(
            state_name(cx).as_str(),
            "PS_Magic_Cast" | "PS_Magic_PowerCast" | "PS_Magic_QuickCast"
        )
    });
    sc.wait(0.3);
    sc.check(
        "the mana is paid when the cast begins, before the missile leaves; the body is blocked (constraint 360)",
        |cx| {
            let m0 = cx.st.n["mana0"] as i32;
            let now = mana(cx);
            let (_, constraint) = cx.core().0.sim.hero_alignment();
            let launched = spells(cx) > cx.st.n["spells0"] as u32;
            expect(
                now < m0 && !launched && constraint == 360.0,
                format!("mana {m0} -> {now} with the missile still in the hand, constraint {constraint}"),
                format!("mana {m0} -> {now}, launched {launched}, constraint {constraint}"),
            )
        },
    );
    sc.shot("raise");
    sc.until("the missile leaves", 4.0, |cx| {
        spells(cx) > cx.st.n["spells0"] as u32
    });
    sc.check(
        "the cast executes when its Raise clip ends: PowerCast 0.92 s after the state began",
        |cx| {
            let state = ["PS_Magic_PowerCast", "PS_Magic_Cast", "PS_Magic_QuickCast"]
                .into_iter()
                .find(|s| cx.st.n.contains_key(&format!("t_{s}")))
                .unwrap_or("-");
            let t0 = cx.st.n.get(&format!("t_{state}")).copied().unwrap_or(-9.0);
            let t1 = cx.st.n.get("t_missile").copied().unwrap_or(-9.0);
            let d = t1 - t0;
            let want = match state {
                "PS_Magic_PowerCast" => 0.92,
                "PS_Magic_QuickCast" => 0.16,
                _ => 0.8,
            };
            expect(
                (d - want).abs() < 0.2,
                format!(
                    "{state}: the missile left {d:.2} s after the state began (clip {want:.2} s)"
                ),
                format!("{state}: {d:.2} s, want {want:.2}"),
            )
        },
    );
    sc.check(
        "the fireball leaves the RIGHT hand (Slot_RightHand_Weapon), visible in flight",
        |cx| {
            let d = cx.st.n.get("missile_hand_dist").copied().unwrap_or(9.0);
            let vis = cx.st.n.get("vis_spell").copied().unwrap_or(0.0);
            expect(
                d < 0.35,
                format!(
                    "spawned {:.0} cm from the right-hand slot, {vis} frames on screen",
                    d * 100.0
                ),
                format!("distance {d} m"),
            )
        },
    );
    sc.wait(0.6);
    sc.shot("fireball");
    sc.check(
        "Cast_Raise, Cast_Hit, Cast_Recover play in that order",
        |cx| {
            let o = cx.st.store.get("overlays").cloned().unwrap_or_default();
            let at = |p: &str| o.find(p);
            let ok = match (at("_Raise_"), at("_Hit_"), at("_Recover_")) {
                (Some(a), Some(b), Some(c)) => a < b && b < c,
                _ => false,
            };
            expect(
                ok && o.contains("Cast_P0_"),
                format!("overlays: {}", o.replace('\n', " | ")),
                format!("overlays: {o}"),
            )
        },
    );
    sc.act("let go of the left button", |cx| {
        cx.mouse_up(MouseButton::Left)
    });
    sc.until("PS_Magic_Reload then the loop", 8.0, |cx| {
        state_name(cx) == "PS_Magic_Loop" && cx.st.n.contains_key("t_PS_Magic_Reload")
    });
    sc.check(
        "after the cast the hero reloads (the rune is held again) and is back in the loop",
        |cx| {
            let right = {
                let c = cx.core();
                c.0.sim
                    .actor(c.0.hero)
                    .equipment
                    .right
                    .as_ref()
                    .map(|i| i.name.clone())
            };
            expect(
                right.as_deref() == Some("It_Spell_Fireball"),
                format!("right hand {right:?}"),
                format!("right hand {right:?}"),
            )
        },
    );
    // ---- an early release aborts ---------------------------------------------------------------------------
    sc.act(
        "setup: mana for a plain cast only (20 of the 30 a power cast needs)",
        |cx| {
            let hero = cx.core().0.hero;
            cx.core_mut().0.sim.actor_mut(hero).mana = 20;
        },
    );
    sc.wait(0.3);
    sc.act("press the cast key and let go at once", |cx| {
        cx.st.n.insert("spells1".into(), f64::from(spells(cx)));
        cx.st.n.insert("mana1".into(), f64::from(mana(cx)));
        cx.mouse_down(MouseButton::Left);
    });
    sc.until("the cast state again", 3.0, |cx| {
        matches!(
            state_name(cx).as_str(),
            "PS_Magic_Cast" | "PS_Magic_PowerCast" | "PS_Magic_QuickCast"
        )
    });
    sc.act("let go before 0.7 s", |cx| cx.mouse_up(MouseButton::Left));
    sc.wait(1.5);
    sc.check(
        "a key let go early aborts the cast: no missile, the mana comes back, the loop again",
        |cx| {
            let n = spells(cx);
            let n0 = cx.st.n["spells1"] as u32;
            let (m, m0) = (mana(cx), cx.st.n["mana1"] as i32);
            let state = state_name(cx);
            expect(
                n == n0 && m >= m0 && state == "PS_Magic_Loop",
                format!("missiles {n0} -> {n}, mana {m0} -> {m}, state {state}"),
                format!("missiles {n0} -> {n}, mana {m0} -> {m}, state {state}"),
            )
        },
    );
    // ---- the weapon key --------------------------------------------------------------------------------------
    sc.act("Space: put the spell away", |cx| cx.tap(KeyCode::Space));
    sc.wait(1.5);
    sc.check("the weapon key ends the magic task", |cx| {
        let (task, _, _) = hero_state(cx);
        let right = {
            let c = cx.core();
            c.0.sim
                .actor(c.0.hero)
                .equipment
                .right
                .as_ref()
                .map(|i| i.name.clone())
        };
        expect(
            task == Some(State::PsNormal) && !cx.core().0.sim.actor(cx.core().0.hero).drawn,
            format!("task {task:?}, right hand {right:?}"),
            format!("task {task:?}, right hand {right:?}"),
        )
    });
    sc.shot("holstered");
    rec(&mut sc, false);
    sc
}
