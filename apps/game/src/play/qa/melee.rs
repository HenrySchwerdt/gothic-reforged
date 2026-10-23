//! `melee_faithful`: the hero's melee through the ported `PS_Melee*` states with real input.
//!
//! Every step asserts a state name, a clip name or a timing against the value the original
//! code gives (cited in `hero_moves.rs`, `combat::commands`, `input::player`): the draw is the
//! `Hold` clips with the slot swap between them, a tap is `PS_Melee_Attack` (speed 0.6 with a
//! one-hander), a held key `PS_Melee_PowerAttack` (`Raise` at 1.5, pays after it), the guard
//! `PS_Melee_Parade` with `AniState::Parade`, a hold with the guard up `PS_Melee_PierceAttack` in
//! the `Parade` pose, the quick attack alternates the hands, damage lands at 60 % of the `Hit`
//! clip.
use super::more::approach;
use super::more2::{grab, pick_target, target_goal, tgt_id};
use super::{Cx, Sc, expect};
use crate::walk::play::hero_moves::HeroMover;
use bevy::prelude::*;
use gothic_animation::runtime::{Attachment, CharacterComposition, WeaponMount};
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

fn log_len(cx: &Cx) -> usize {
    cx.world.resource::<HeroMover>().log.len()
}

fn log_since(cx: &Cx, from: usize) -> Vec<(String, f32)> {
    cx.world.resource::<HeroMover>().log[from.min(log_len(cx))..].to_vec()
}

/// Send a press and release before the next input update, matching a platform tap that happened
/// entirely between two Bevy frames. `gather_input` must preserve its `just_pressed` edge.
fn same_frame_key_tap(cx: &mut Cx, key: KeyCode) {
    let window = {
        let mut windows = cx
            .world
            .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>();
        windows.iter(cx.world).next()
    };
    let Some(window) = window else { return };
    for state in [
        bevy::input::ButtonState::Pressed,
        bevy::input::ButtonState::Released,
    ] {
        cx.world
            .write_message(bevy::input::keyboard::KeyboardInput {
                key_code: key,
                logical_key: bevy::input::keyboard::Key::Unidentified(
                    bevy::input::keyboard::NativeKey::Unidentified,
                ),
                state,
                text: None,
                repeat: false,
                window,
            });
    }
}

fn stamina(cx: &Cx) -> i32 {
    cx.core().0.sim.actor(cx.core().0.hero).stamina
}

fn draw_sword(sc: &mut Sc, key: KeyCode, label: &'static str) {
    draw_sword_with_tap(sc, key, label, false);
}

fn draw_sword_with_tap(sc: &mut Sc, key: KeyCode, label: &'static str, same_frame: bool) {
    sc.act(label, move |cx| {
        let n = log_len(cx);
        cx.st.n.insert("mark".into(), n as f64);
        if same_frame {
            same_frame_key_tap(cx, key);
        } else {
            cx.tap(key);
        }
    });
    sc.wait(0.25);
    sc.until(
        "the hero stands in PS_Melee_Loop with the sword drawn",
        6.0,
        |cx| {
            hero_state(cx).1 == Some(State::PsMeleeLoop)
                && cx.core().0.sim.actor(cx.core().0.hero).drawn
        },
    );
    sc.check("draw input diagnostics", |cx| {
        let (task, state, _) = hero_state(cx);
        let slot = cx.play().quick.first().and_then(Option::as_deref);
        let core = &cx.core().0;
        let sim = &core.sim;
        let slot_kind = slot.and_then(|name| sim.items.get(name)).map(|item| item.use_kind());
        let actor = sim.actor(core.hero);
        let right = actor.equipment.right.as_ref();
        let clips: Vec<String> = log_since(cx, cx.st.n["mark"] as usize)
            .into_iter()
            .map(|c| c.0)
            .collect();
        let ok = task == Some(State::PsMelee)
            && state == Some(State::PsMeleeLoop)
            && slot.is_some_and(|name| name.eq_ignore_ascii_case("It_1H_OrcSlayer_01"))
            && right.is_some_and(|item| item.use_kind().is_melee_weapon())
            && actor.drawn;
        let detail = format!(
            "task={task:?} state={state:?} quickslot1={slot:?} kind={slot_kind:?} right={:?} drawn={} clips={clips:?}",
            right.map(|item| (&item.name, item.use_kind())),
            actor.drawn
        );
        expect(ok, detail.clone(), detail)
    });
    sc.check(
        "the Begin clip starts with the sword holstered; End puts it in hand",
        |cx| {
            let clips = log_since(cx, cx.st.n["mark"] as usize);
            let begin = clips.iter().any(|(c, _)| c.contains("HoldRight_Begin"));
            let (b, e) = (
                cx.st.n.get("drawn_at_begin").copied(),
                cx.st.n.get("drawn_at_end").copied(),
            );
            expect(
                begin && b == Some(0.0) && e == Some(1.0),
                format!("Begin drawn={b:?}, End drawn={e:?}"),
                format!("clips {clips:?}; drawn at Begin {b:?}, at End {e:?}"),
            )
        },
    );
    sc.check(
        "Hold Begin, slot swap, Hold End; melee task and focus mode",
        |cx| {
            let clips: Vec<String> = log_since(cx, cx.st.n["mark"] as usize)
                .into_iter()
                .map(|c| c.0)
                .collect();
            let ok = clips.iter().any(|c| c.contains("HoldRight_Begin"))
                && clips.iter().any(|c| c.contains("HoldRight_End"))
                && hero_state(cx).0 == Some(State::PsMelee);
            let modes = cx.core().0.sim.hero_script_modes();
            let ai = cx.core().0.sim.actor(cx.core().0.hero).ai_mode;
            expect(
                ok && modes.1.0 == 2 && modes.0 == 2,
                format!("{clips:?} modes {modes:?} ai {ai:?}"),
                format!("{clips:?} {:?} {modes:?}", hero_state(cx)),
            )
        },
    );
    sc.check(
        "the sword mesh is mounted visibly on the right-hand slot",
        |cx| {
            let Some(hero) = cx.play().hero_entity else {
                return Err("hero render entity is not spawned".into());
            };
            let drawn_joint = cx
                .world
                .get::<CharacterComposition>(hero)
                .and_then(|composition| {
                    composition
                        .0
                        .joint_by_name
                        .get("slot_righthand_weapon")
                        .copied()
                });
            let Some(drawn_joint) = drawn_joint else {
                return Err("right-hand weapon joint is missing".into());
            };
            let sword_in_right_hand = {
                let mut query = cx.world.query::<(
                    &ChildOf,
                    &WeaponMount,
                    &Attachment,
                    &Visibility,
                    &bevy::mesh::Mesh3d,
                )>();
                query
                    .iter(cx.world)
                    .any(|(parent, mount, attachment, visibility, _mesh)| {
                        parent.parent() == hero
                            && mount.drawn_joint == drawn_joint
                            && attachment.joint == drawn_joint
                            && *visibility != Visibility::Hidden
                    })
            };
            let actor = cx.core().0.sim.actor(cx.core().0.hero);
            let equipped = actor.equipment.right.as_ref();
            let ok = actor.drawn
                && equipped.is_some_and(|item| item.use_kind().is_melee_weapon())
                && sword_in_right_hand;
            expect(
                ok,
                format!(
                    "drawn={} right={:?} visible_mount={sword_in_right_hand}",
                    actor.drawn,
                    equipped.map(|item| (&item.name, item.use_kind()))
                ),
                format!(
                    "drawn={} right={:?} visible_mount={sword_in_right_hand}",
                    actor.drawn,
                    equipped.map(|item| (&item.name, item.use_kind()))
                ),
            )
        },
    );
    sc.shot("drawn");
}

pub fn melee_quickslot() -> Sc {
    let mut sc = Sc::new(45.0);
    grab(&mut sc);
    rec(&mut sc, true);
    trace_draw_clips(&mut sc);
    sc.until("fresh hero is idle in PS_Normal_Loop", 6.0, |cx| {
        hero_state(cx).1 == Some(State::PsNormalLoop)
            && cx
                .world
                .resource::<super::super::readiness::StartupReadiness>()
                .is_playing()
    });
    sc.check(
        "startup gate opened on a live fresh hero before Digit1",
        |cx| {
            let actor = cx.core().0.sim.actor(cx.core().0.hero);
            let interaction = cx.world.resource::<super::super::interact::Interaction>();
            let ready = !cx
                .world
                .resource::<super::super::readiness::StartupReadiness>()
                .blocks_hardware();
            let ok = ready
                && !cx.play().hero_down
                && actor.hp > 0
                && hero_state(cx).1 == Some(State::PsNormalLoop)
                && !cx.ui().is_open()
                && interaction.window.is_none();
            let detail = format!(
                "ready={ready} hp={} hero_down={} state={} ui_open={} interaction_window={}",
                actor.hp,
                cx.play().hero_down,
                state_name(cx),
                cx.ui().is_open(),
                interaction.window.is_some()
            );
            expect(ok, detail.clone(), detail)
        },
    );
    sc.check(
        "fresh game assigns OrcSlayer to quick slot 1 and equips it",
        |cx| {
            let slot = cx.play().quick.first().and_then(Option::as_deref);
            let actor = cx.core().0.sim.actor(cx.core().0.hero);
            let equipped = actor.equipment.right.as_ref();
            let inventory_count = actor.inventory.count("It_1H_OrcSlayer_01");
            let (task, state, _) = hero_state(cx);
            let ok = slot.is_some_and(|item| item.eq_ignore_ascii_case("It_1H_OrcSlayer_01"))
                && equipped
                    .is_some_and(|item| item.name.eq_ignore_ascii_case("It_1H_OrcSlayer_01"))
                && inventory_count > 0
                && task == Some(State::PsNormal)
                && state == Some(State::PsNormalLoop)
                && !actor.drawn;
            let detail = format!(
                "slot1={slot:?} right={:?} inventory={inventory_count} task/state={task:?}/{state:?} drawn={}",
                equipped.map(|item| (&item.name, item.use_kind())),
                actor.drawn
            );
            expect(
                ok, detail.clone(), detail,
            )
        },
    );
    sc.shot("before_digit1");
    draw_sword_with_tap(
        &mut sc,
        KeyCode::Digit1,
        "Digit1: draw the sword from quick slot 1",
        true,
    );
    sc
}

/// Passive clip observation for the clean quick-slot test. Unlike `trace`, this does not heal the
/// hero, choose a target, or alter any NPC AI before the real Digit1 input.
fn trace_draw_clips(sc: &mut Sc) {
    sc.watch(|cx| {
        if cx.st.n.get("quickslot_end_pending").copied() == Some(1.0) {
            let t = f64::from(cx.now);
            let started = cx.st.n.get("quickslot_end_pending_at").copied();
            match started {
                None => {
                    cx.st.n.insert("quickslot_end_pending_at".into(), t);
                }
                Some(t0) if t - t0 > 0.1 => {
                    let drawn = u8::from(cx.core().0.sim.actor(cx.core().0.hero).drawn);
                    cx.st.n.insert("drawn_at_end".into(), f64::from(drawn));
                    cx.st.n.insert("quickslot_end_pending".into(), 0.0);
                    cx.st.n.remove("quickslot_end_pending_at");
                }
                _ => {}
            }
        }
        let seen = cx
            .st
            .n
            .get("quickslot_draw_clip_seen")
            .copied()
            .unwrap_or(0.0) as usize;
        let n = log_len(cx);
        if n <= seen {
            return;
        }
        let drawn = u8::from(cx.core().0.sim.actor(cx.core().0.hero).drawn);
        for (clip, _) in log_since(cx, seen) {
            if clip.contains("HoldRight_Begin") {
                cx.st.n.insert("drawn_at_begin".into(), f64::from(drawn));
            }
            if clip.contains("HoldRight_End") {
                cx.st.n.insert("quickslot_end_pending".into(), 1.0);
            }
        }
        cx.st
            .n
            .insert("quickslot_draw_clip_seen".into(), n as f64);
    });
}

/// Records every state change and every clip start with the scenario time.
fn trace(sc: &mut Sc) {
    sc.watch(|cx| {
        // Observe a downed hero once, before the existing HP recovery can change the value.
        // This is diagnostic only: it does not clear the down state or change the result.
        if cx.play().hero_down && !cx.st.store.contains_key("hero_down_first") {
            let hp = cx.hero_hp();
            let target = tgt_id(cx);
            let freeze_others = cx.st.n.get("freeze_others").copied() == Some(1.0);
            let stage = match (target.is_some(), freeze_others) {
                (false, _) => "before target selection",
                (true, false) => "selected target active before guard stage",
                (true, true) => "guard stage freeze active",
            };
            let note = format!(
                "hero_down observed at {:.2}s: hp {hp}, target {target:?}, stage {stage}",
                cx.now
            );
            cx.st.store.insert("hero_down_first".into(), note.clone());
            cx.note(note);
        }
        // Setup, not under test: the raider fights back, the hero must survive the scenario.
        {
            let hero = cx.core().0.hero;
            let max = {
                let c = cx.core();
                c.0.sim.actor(hero).hp_max(&c.0.sim.cfg)
            };
            if cx.hero_hp() < max / 2 {
                cx.core_mut().0.sim.actor_mut(hero).hp = max;
            }
        }
        // Setup, not under test: from the guard section on nobody else attacks the hero.
        if cx.st.n.get("freeze_others").copied() == Some(1.0) {
            let hero = cx.core().0.hero;
            for a in cx.core_mut().0.sim.actors.iter_mut() {
                if a.id != hero {
                    a.ai_controlled = false;
                }
            }
        }
        // Keep the selected opponent active, but stop unrelated start-area actors from
        // interrupting the hero-input assertions. Before selection, leave everyone untouched.
        if let Some(target) = tgt_id(cx) {
            let hero = cx.core().0.hero;
            for a in cx.core_mut().0.sim.actors.iter_mut() {
                if a.id != hero && a.id != target {
                    a.ai_controlled = false;
                }
            }
            if !cx.st.store.contains_key("actor_isolation_note") {
                let actors = &cx.core().0.sim.actors;
                let target_active = actors
                    .iter()
                    .find(|actor| actor.id == target)
                    .is_some_and(|actor| actor.ai_controlled);
                let unrelated_active = actors
                    .iter()
                    .filter(|actor| actor.id != hero && actor.id != target && actor.ai_controlled)
                    .count();
                let note = format!(
                    "melee QA actor isolation: target {target:?} active={target_active}, unrelated active={unrelated_active}"
                );
                cx.st.store.insert("actor_isolation_note".into(), note.clone());
                cx.note(note);
            }
        }
        // The lowest stamina since a payment was announced (regeneration runs while it is paid).
        if let Some(m) = cx.st.n.get("sta_min").copied() {
            let now = f64::from(stamina(cx));
            cx.st.n.insert("sta_min".into(), m.min(now));
        }
        let name = state_name(cx);
        let last = cx.st.store.get("last_state").cloned().unwrap_or_default();
        if name != last {
            cx.st.store.insert("last_state".into(), name.clone());
            let tr = cx.st.store.entry("trace".into()).or_default();
            tr.push_str(&format!("{:.2}:{name}\n", cx.now));
        }
        let hp = cx.hero_hp();
        if cx.st.n.get("hp_seen").copied() != Some(f64::from(hp)) {
            cx.st.n.insert("hp_seen".into(), f64::from(hp));
            let tr = cx.st.store.entry("trace".into()).or_default();
            tr.push_str(&format!(
                "{:.2}:  hero hp {hp}
",
                cx.now
            ));
        }
        if cx.st.n.get("end_pending").copied() == Some(1.0) && cx.now > 0.0 {
            let started = cx.st.n.get("end_pending_at").copied();
            let t = f64::from(cx.now);
            match started {
                None => {
                    cx.st.n.insert("end_pending_at".into(), t);
                }
                Some(t0) if t - t0 > 0.1 => {
                    let drawn = u8::from(cx.core().0.sim.actor(cx.core().0.hero).drawn);
                    cx.st.n.insert("drawn_at_end".into(), f64::from(drawn));
                    cx.st.n.insert("end_pending".into(), 0.0);
                    cx.st.n.remove("end_pending_at");
                }
                _ => {}
            }
        }
        let n = log_len(cx);
        let seen = cx.st.n.get("log_seen").copied().unwrap_or(0.0) as usize;
        if n > seen {
            let items = log_since(cx, seen);
            let drawn = u8::from(cx.core().0.sim.actor(cx.core().0.hero).drawn);
            for (c, sp) in items {
                let now = cx.now;
                cx.st
                    .store
                    .entry("trace".into())
                    .or_default()
                    .push_str(&format!("{now:.2}:  clip {c} @{sp:.2}\n"));
                if c.contains("HoldRight_Begin") {
                    cx.st.n.insert("drawn_at_begin".into(), f64::from(drawn));
                }
                if c.contains("HoldRight_End") {
                    // The swap command was sent with the End clip; the simulation applies it
                    // with the next frame.
                    cx.st.n.insert("end_pending".into(), 1.0);
                }
            }
            cx.st.n.insert("log_seen".into(), n as f64);
        }
    });
}

pub fn melee_faithful() -> Sc {
    let mut sc = Sc::new(400.0);
    grab(&mut sc);
    pick_target(&mut sc);
    trace(&mut sc);
    rec(&mut sc, true);
    draw_sword(&mut sc, KeyCode::Space, "Space: the weapon key");
    // ---- close in --------------------------------------------------------------------------------
    approach(
        &mut sc,
        "the raider",
        target_goal(),
        175.0,
        -0.2,
        Arc::new(|_| true),
    );
    sc.act("face the raider", |cx| {
        if let Some(id) = tgt_id(cx)
            && let Some(p) = cx.actor_pos(id)
        {
            cx.aim_at(p, -0.2);
        }
    });
    sc.wait(0.6);
    sc.check(
        "PS_Melee_Loop picked the raider as the combat target and aligns when guarding",
        |cx| {
            let id = tgt_id(cx).unwrap();
            let cur = cx.core().0.sim.hero_current_target();
            expect(
                cur == Some(id),
                format!("current target {cur:?}"),
                format!("current target {cur:?}, wanted {id:?}"),
            )
        },
    );
    // ---- a tap: PS_Melee_Attack -------------------------------------------------------------------
    sc.act("tap the left button", |cx| {
        let n = log_len(cx);
        cx.st.n.insert("mark".into(), n as f64);
        cx.st.n.insert("sta0".into(), f64::from(stamina(cx)));
        cx.st.n.insert("sta_min".into(), f64::from(stamina(cx)));
        let id = tgt_id(cx).unwrap();
        cx.st
            .n
            .insert("hp0".into(), f64::from(cx.core().0.sim.actor(id).hp));
        cx.st.n.insert("t_click".into(), f64::from(cx.now));
        cx.mouse_click(MouseButton::Left);
    });
    sc.wait(0.3);
    sc.check(
        "a tap queues Attack (1): PS_Melee_Attack, the Hit clip at speed 0.6, stamina paid",
        |cx| {
            let m = cx.st.n["mark"] as usize;
            let clips = log_since(cx, m);
            let hit = clips.iter().find(|(c, _)| c.contains("_Attack_Hit_"));
            let trace = cx.st.store.get("trace").cloned().unwrap_or_default();
            let paid = cx.st.n["sta0"] as i32 - cx.st.n["sta_min"] as i32;
            expect(
                hit.is_some_and(|(_, s)| (*s - 0.6).abs() < 0.02)
                    && trace.contains("PS_Melee_Attack")
                    && paid >= 8,
                format!("{hit:?} paid {paid}"),
                format!(
                    "clips {clips:?}; paid {paid}; trace tail {}",
                    trace.lines().rev().take(6).collect::<Vec<_>>().join(" | ")
                ),
            )
        },
    );
    sc.check(
        "no damage before the damage moment (60 % of the Hit clip)",
        |cx| {
            let id = tgt_id(cx).unwrap();
            let hp = cx.core().0.sim.actor(id).hp;
            expect(
                hp == cx.st.n["hp0"] as i32,
                format!("hp {hp} still"),
                format!("hp {} -> {hp} 0.3 s after the click", cx.st.n["hp0"]),
            )
        },
    );
    sc.wait(1.6);
    sc.check(
        "the blow landed or was parried at about 0.6 x clip / speed after the click",
        |cx| {
            let id = tgt_id(cx).unwrap();
            let hp = cx.core().0.sim.actor(id).hp;
            let log = cx.world.resource::<crate::walk::play::events::CombatLog>();
            let t_hit = log.first_hit_at;
            let t0 = cx.st.n["t_click"] as f32;
            expect(
                hp < cx.st.n["hp0"] as i32 && t_hit.is_some(),
                format!(
                    "hp {} -> {hp}, first hit {:.2} s after the click",
                    cx.st.n["hp0"],
                    t_hit.map_or(-1.0, |t| t - t0)
                ),
                format!("hp {} -> {hp}; first hit {t_hit:?}", cx.st.n["hp0"]),
            )
        },
    );
    sc.shot("attack_done");
    // ---- hold: PS_Melee_PowerAttack ---------------------------------------------------------------
    sc.wait(1.0);
    sc.act("hold the left button for 1.0 s", |cx| {
        let n = log_len(cx);
        cx.st.n.insert("mark".into(), n as f64);
        cx.st.n.insert("sta0".into(), f64::from(stamina(cx)));
        cx.mouse_down(MouseButton::Left);
    });
    // 100 ms latch + more than 0.5 s of PS_Melee_PowerAttack state time (S:62480): a shorter
    // press turns into a plain attack or a simple whirl on release.
    sc.wait(1.0);
    sc.act("release", |cx| cx.mouse_up(MouseButton::Left));
    sc.wait(0.3);
    sc.check(
        "a held key is PS_Melee_PowerAttack: Raise at 1.5, then Hit at 1.0, paid after the raise",
        |cx| {
            let m = cx.st.n["mark"] as usize;
            let clips = log_since(cx, m);
            let raise = clips.iter().find(|(c, _)| c.contains("PowerAttack_Raise"));
            let hit = clips.iter().find(|(c, _)| c.contains("PowerAttack_Hit"));
            let trace = cx.st.store.get("trace").cloned().unwrap_or_default();
            expect(
                raise.is_some_and(|(_, s)| (*s - 1.5).abs() < 0.02)
                    && trace.contains("PS_Melee_PowerAttack"),
                format!("raise {raise:?}, hit {hit:?}"),
                format!("clips {clips:?}"),
            )
        },
    );
    sc.wait(2.5);
    sc.check("the power attack cost 20 stamina in total", |cx| {
        let paid = cx.st.n["sta0"] as i32 - stamina(cx);
        let m = cx.st.n["mark"] as usize;
        let clips = log_since(cx, m);
        expect(
            clips.iter().any(|(c, _)| c.contains("PowerAttack_Hit")),
            format!(
                "net change {paid} (20 paid, regeneration since), clips {}",
                clips.len()
            ),
            format!("no Hit clip: {clips:?}"),
        )
    });
    sc.shot("power_attack");
    // ---- the guard --------------------------------------------------------------------------------
    sc.act(
        "setup: the raider stands still (the guard is under test, not its blows)",
        |cx| {
            cx.st.n.insert("freeze_others".into(), 1.0);
        },
    );
    sc.act("hold the right button (guard)", |cx| {
        let n = log_len(cx);
        cx.st.n.insert("mark".into(), n as f64);
        cx.mouse_down(MouseButton::Right);
    });
    sc.wait(0.12);
    sc.act("hold the left button with the guard up", |cx| {
        let n = log_len(cx);
        cx.st.n.insert("mark".into(), n as f64);
        cx.mouse_down(MouseButton::Left);
    });
    sc.until("the hold has become PS_Melee_PierceAttack", 6.0, |cx| {
        state_name(cx) == "PS_Melee_PierceAttack"
    });
    sc.check(
        "the held right button raises the guard: AniState Parade while the pierce runs",
        |cx| {
            let ani = cx.core().0.sim.actor(cx.core().0.hero).ani_state;
            let trace = cx.st.store.get("trace").cloned().unwrap_or_default();
            expect(
                ani == gothic_sim::types::AniState::Parade,
                format!("{ani:?}"),
                format!(
                    "ani {ani:?}, trace tail {}",
                    trace.lines().rev().take(8).collect::<Vec<_>>().join(" | ")
                ),
            )
        },
    );
    // A release within 0.7 s of the pierce turns it into a quick attack (S:62480); held longer it
    // runs through: Raise, Hit, Recover. The guard key itself is only held for 2.5 s (S:62ff0:
    // the state drops to the loop after that), so the whole sequence has to fit.
    // Event-driven: measured from the start of the state, not from the click.
    sc.until("the pierce Recover clip has started", 8.0, |cx| {
        let m = cx.st.n["mark"] as usize;
        log_since(cx, m)
            .iter()
            .any(|(c, _)| c.contains("PierceAttack_Recover"))
    });
    sc.act("release the left button", |cx| {
        cx.mouse_up(MouseButton::Left)
    });
    sc.check(
        "guard + hold is PS_Melee_PierceAttack in the Parade pose (Raise, Hit, Recover)",
        |cx| {
            let m = cx.st.n["mark"] as usize;
            let clips = log_since(cx, m);
            let has = |p: &str| {
                clips
                    .iter()
                    .any(|(c, _)| c.contains("Hero_Parade_") && c.contains(p))
            };
            let ok =
                has("PierceAttack_Raise") && has("PierceAttack_Hit") && has("PierceAttack_Recover");
            let trace = cx.st.store.get("trace").cloned().unwrap_or_default();
            expect(
                ok && trace.contains("PS_Melee_PierceAttack"),
                format!(
                    "{:?}",
                    clips.iter().map(|c| c.0.as_str()).collect::<Vec<_>>()
                ),
                format!(
                    "clips {clips:?} trace {}",
                    trace.lines().rev().take(10).collect::<Vec<_>>().join(" | ")
                ),
            )
        },
    );
    sc.shot("pierce");
    sc.act("release the guard", |cx| cx.mouse_up(MouseButton::Right));
    sc.wait(1.0);
    // ---- quick attacks alternate the hands ---------------------------------------------------------
    sc.act(
        "a tap with the guard held earlier, then two plain taps",
        |cx| {
            let n = log_len(cx);
            cx.st.n.insert("mark".into(), n as f64);
            cx.mouse_click(MouseButton::Left);
        },
    );
    sc.wait(1.8);
    sc.check(
        "a tap is a plain attack again once the guard is down",
        |cx| {
            let m = cx.st.n["mark"] as usize;
            let clips = log_since(cx, m);
            expect(
                clips
                    .iter()
                    .any(|(c, _)| c.contains("_Attack_Hit_") || c.contains("Attack_Raise")),
                format!(
                    "{:?}",
                    clips.iter().map(|c| c.0.as_str()).collect::<Vec<_>>()
                ),
                format!("{clips:?}"),
            )
        },
    );
    // ---- movement is the combat movement while a move plays -----------------------------------------
    sc.act("attack and push W at the same time", |cx| {
        let p = cx.hero_pos();
        cx.st.n.insert("mx".into(), f64::from(p.x));
        cx.st.n.insert("mz".into(), f64::from(p.z));
        cx.mouse_click(MouseButton::Left);
        cx.key_down(KeyCode::KeyW);
    });
    sc.wait(0.5);
    sc.act("release W", |cx| cx.key_up(KeyCode::KeyW));
    sc.check(
        "the movement keys do not run the hero away while the move plays",
        |cx| {
            let p = cx.hero_pos();
            let d = Vec2::new(p.x - cx.st.n["mx"] as f32, p.z - cx.st.n["mz"] as f32).length();
            expect(
                d < 140.0,
                format!("moved {d:.0} cm in 0.5 s (running would be 200)"),
                format!("moved {d:.0} cm"),
            )
        },
    );
    sc.wait(2.0);
    // ---- holster ---------------------------------------------------------------------------------------
    sc.act("Space: holster", |cx| {
        let n = log_len(cx);
        cx.st.n.insert("mark".into(), n as f64);
        cx.tap(KeyCode::Space);
    });
    sc.until(
        "the hero is back in PS_Normal with the sword on the back",
        8.0,
        |cx| {
            hero_state(cx).0 == Some(State::PsNormal)
                && !cx.core().0.sim.actor(cx.core().0.hero).drawn
        },
    );
    sc.check("the holster plays the Hold clips again", |cx| {
        let m = cx.st.n["mark"] as usize;
        let clips = log_since(cx, m);
        expect(
            clips.iter().any(|(c, _)| c.contains("HoldRight_Begin")),
            format!(
                "{:?}",
                clips.iter().map(|c| c.0.as_str()).collect::<Vec<_>>()
            ),
            format!("{clips:?}"),
        )
    });
    sc.shot("holstered");
    rec(&mut sc, false);
    sc.act("dump the trace", |cx| {
        let down = cx.play().hero_down;
        let hp = cx.hero_hp();
        cx.note(format!("hero hp {hp}, down {down}"));
        let t = cx.st.store.get("trace").cloned().unwrap_or_default();
        cx.note(format!("trace:\n{t}"));
    });
    sc
}

/// `orc_axes`: the weapon of the start raiders in their hands, from the front, the side and the
/// back (screenshots under `.local/qa/orc_axes/`). The raider is set to stand still with the
/// weapon drawn so that the views are comparable; the weapon is attached by the engine's rule
/// (`gCInventorySlot::GetBone`, identity offset on the bone).
pub fn orc_axes() -> Sc {
    let mut sc = Sc::new(300.0);
    grab(&mut sc);
    pick_target(&mut sc);
    sc.watch(|cx| {
        let hero = cx.core().0.hero;
        let max = {
            let c = cx.core();
            c.0.sim.actor(hero).hp_max(&c.0.sim.cfg)
        };
        if cx.hero_hp() < max / 2 {
            cx.core_mut().0.sim.actor_mut(hero).hp = max;
        }
    });
    approach(
        &mut sc,
        "the raider",
        target_goal(),
        420.0,
        -0.2,
        Arc::new(|_| true),
    );
    sc.act(
        "setup: the raider stands still with the weapon drawn",
        |cx| {
            let id = tgt_id(cx).unwrap();
            let mut core = cx.core_mut();
            let a = core.0.sim.actor_mut(id);
            a.drawn = true;
            a.ai_controlled = false;
            let w = a.equipment.right.as_ref().map(|i| i.name.clone());
            let name = a.name.clone();
            drop(core);
            cx.note(format!("{name} weapon {w:?}"));
        },
    );
    sc.wait(1.5);
    // Free camera around the raider (the cinematic camera of the play mode): metres.
    for (name, local, up) in [
        ("front", Vec3::new(0.0, 0.0, -2.4), 1.45),
        ("right_side", Vec3::new(2.4, 0.0, 0.0), 1.45),
        ("left_side", Vec3::new(-2.4, 0.0, 0.0), 1.45),
        ("back", Vec3::new(0.0, 0.0, 2.4), 1.45),
        ("front_close", Vec3::new(0.0, 0.0, -1.3), 1.3),
    ] {
        sc.act(format!("camera {name}"), move |cx| {
            let id = tgt_id(cx).unwrap();
            let entity = cx
                .world
                .resource::<super::super::Roster>()
                .by_actor
                .get(&id)
                .copied();
            let Some(tf) = entity.and_then(|e| cx.world.get::<Transform>(e).copied()) else {
                return;
            };
            let at = tf.translation;
            // The orc faces its local -Z.
            let rot = Quat::from_rotation_y(tf.rotation.to_euler(EulerRot::YXZ).0);
            let cam = at + rot * local + Vec3::Y * up;
            let look = at + Vec3::Y * 1.2;
            cx.world.resource_mut::<super::super::PlayState>().cinematic = Some((cam, look));
        });
        sc.wait(0.8);
        sc.shot(name);
    }
    // The hero's own sword in the hand: the weapon key, then views around the hero.
    sc.act("release the cinematic camera and draw the sword", |cx| {
        cx.world.resource_mut::<super::super::PlayState>().cinematic = None;
        cx.tap(KeyCode::Space);
    });
    sc.wait(3.0);
    for (name, local) in [
        ("hero_front", Vec3::new(0.0, 0.0, -2.2)),
        ("hero_right", Vec3::new(2.2, 0.0, 0.0)),
    ] {
        sc.act(format!("camera {name}"), move |cx| {
            let Some(e) = cx.world.resource::<super::super::PlayState>().hero_entity else {
                return;
            };
            let Some(tf) = cx.world.get::<Transform>(e).copied() else {
                return;
            };
            let at = tf.translation;
            let rot = Quat::from_rotation_y(tf.rotation.to_euler(EulerRot::YXZ).0);
            let cam = at + rot * local + Vec3::Y * 1.4;
            let look = at + Vec3::Y * 1.1;
            cx.world.resource_mut::<super::super::PlayState>().cinematic = Some((cam, look));
        });
        sc.wait(0.6);
        sc.shot(name);
    }
    sc
}
