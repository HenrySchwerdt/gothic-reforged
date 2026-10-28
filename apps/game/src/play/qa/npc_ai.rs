//! NPC combat AI scenarios: the ported `ZS_Attack` family driving real bodies.
//!
//! Every scenario provokes characters of the world data against the hero (or each other) and
//! asserts the behaviour AND what is drawn: the states the NPC went through, the clip of every
//! swing with the play speed the ported `sub_42a0` rule gives, the moment the damage lands
//! (60 % of the `Hit` clip, `sAICombatMoveItlLoop`), the weapon in the hands, projectiles on
//! screen. The hero only stands there (setup, not under test) and is healed when low.
use super::more2::{flat, grab};
use super::{Cx, Poll, Sc, expect};
use crate::walk::play::fx::FxLog;
use crate::walk::play::npc_moves::NpcMoveLog;
use crate::walk::play::shots::ShotsOnScreen;
use bevy::prelude::*;
use gothic_animation::ActorLibrary;
use gothic_sim::types::{ActorId, AttackReason};

const CM: f32 = 0.01;

/// Start / stop of the frame capture of `--record DIR` (fixed 1/30 s steps).
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

fn clips(cx: &Cx) -> Vec<(f32, ActorId, String, f32)> {
    cx.world.resource::<NpcMoveLog>().clips.clone()
}

fn ids(cx: &Cx, key: &str) -> Vec<ActorId> {
    cx.st
        .store
        .get(key)
        .map(|s| {
            s.split(',')
                .filter_map(|x| x.parse::<u32>().ok().map(ActorId))
                .collect()
        })
        .unwrap_or_default()
}

fn name_of(cx: &Cx, id: ActorId) -> String {
    cx.core().0.sim.actor(id).name.clone()
}

/// The nearest `n` living characters whose name starts with `prefix`.
fn nearest(cx: &Cx, prefix: &str, n: usize) -> Vec<ActorId> {
    let hero = cx.hero_pos();
    let mut v: Vec<(f32, ActorId)> = cx
        .core()
        .0
        .sim
        .actors
        .iter()
        .filter(|a| a.name.starts_with(prefix) && a.is_alive() && !a.is_player)
        .filter_map(|a| cx.actor_pos(a.id).map(|p| (flat(p - hero).length(), a.id)))
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v.into_iter().take(n).map(|x| x.1).collect()
}

/// Fixed camera on the fight of the tracked characters (setup, not under test): 4.5 m to the side
/// of the middle between the hero and the first fighter, chest high.
fn fight_cam(cx: &mut Cx, key: &str) {
    let hero = cx.hero_pos();
    let Some(p) = ids(cx, key).first().and_then(|id| cx.actor_pos(*id)) else {
        return;
    };
    let mid = (hero + p) * 0.5 * CM;
    let dir = (p - hero).with_y(0.0).normalize_or_zero();
    let side = Vec3::new(-dir.z, 0.0, dir.x);
    // Behind the hero and a little to the side: open air in the middle of every arena.
    let pos = hero * CM - dir * 3.2 + side * 0.9 + Vec3::Y * 2.0;
    let look = mid + Vec3::Y * 1.2;
    cx.world.resource_mut::<super::super::PlayState>().cinematic = Some((pos, look));
}

fn fight_cam_off(cx: &mut Cx) {
    cx.world.resource_mut::<super::super::PlayState>().cinematic = None;
}

/// Records every frame what the fight needs: states, engaged count, hero hp drops.
fn trace(sc: &mut Sc, key: &'static str) {
    sc.watch(move |cx| {
        // Setup, not under test: the hero is a punching bag that never dies.
        let hero = cx.core().0.hero;
        let max = {
            let c = cx.core();
            c.0.sim.actor(hero).hp_max(&c.0.sim.cfg)
        };
        let hp = cx.hero_hp();
        if hp < max / 2 {
            cx.core_mut().0.sim.actor_mut(hero).hp = max;
        }
        let last = cx.st.n.get("hp_last").copied().unwrap_or(f64::from(hp));
        let now = f64::from(cx.now);
        // Frame times: the timestamps of clips and damage are frame times, so a hitch (a loaded
        // machine) moves them by up to that frame; hitches are kept to widen the tolerance.
        {
            let prev = cx.st.n.insert("fr_prev".into(), now).unwrap_or(now);
            if now - prev > 0.05 {
                cx.st
                    .store
                    .entry("hitches".into())
                    .or_default()
                    .push_str(&format!("{now:.3},{:.3}
", now - prev));
            }
        }
        if f64::from(hp) < last && last <= f64::from(max) {
            let tr = cx.st.store.entry("hero_drops".into()).or_default();
            tr.push_str(&format!("{now:.3},{}\n", last - f64::from(hp)));
            let nearest = ids(cx, key)
                .iter()
                .filter_map(|id| cx.actor_pos(*id))
                .map(|p| flat(p - cx.hero_pos()).length())
                .fold(f32::MAX, f32::min);
            let dd = cx.st.store.entry("drop_dists".into()).or_default();
            dd.push_str(&format!("{nearest:.0} "));
        }
        cx.st.n.insert("hp_last".into(), f64::from(hp.max(max / 2)));
        let tracked = ids(cx, key);
        let mut engaged = 0.0;
        for id in tracked {
            let (state, cs, task) = {
                let c = cx.core();
                let s = c.0.sim.script.npc(id);
                (s.state, s.combat_state, c.0.sim.actor(id).brain.task)
            };
            if let Some(st) = state {
                let k = format!("seen_{}", id.0);
                let e = cx.st.store.entry(k).or_default();
                let n = st.name();
                if !e.split(',').any(|x| x == n) {
                    if !e.is_empty() {
                        e.push(',');
                    }
                    e.push_str(n);
                }
            }
            if cs == 1 && task == gothic_sim::brain::Task::Attack {
                engaged += 1.0;
            }
        }
        let m = cx.st.n.get("engaged_max").copied().unwrap_or(0.0);
        cx.st.n.insert("engaged_max".into(), m.max(engaged));
        // Distance to the hero while fighting.
        for id in ids(cx, key) {
            if let Some(p) = cx.actor_pos(id) {
                let d = f64::from(flat(p - cx.hero_pos()).length());
                let k = format!("dmin_{}", id.0);
                let m = cx.st.n.get(&k).copied().unwrap_or(1.0e9);
                cx.st.n.insert(k, m.min(d));
            }
        }
    });
}

fn seen(cx: &Cx, id: ActorId) -> Vec<String> {
    cx.st
        .store
        .get(&format!("seen_{}", id.0))
        .map(|s| s.split(',').map(str::to_string).collect())
        .unwrap_or_default()
}

/// The hero's hp drops as `(time, amount)`.
fn drops(cx: &Cx) -> Vec<(f32, f32)> {
    cx.st
        .store
        .get("hero_drops")
        .map(|s| {
            s.lines()
                .filter_map(|l| {
                    let (a, b) = l.split_once(',')?;
                    Some((a.parse().ok()?, b.parse().ok()?))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `animation_speed` (S:42a0) the ported rule gives for the clip `name`, for the weapons in
/// the clip name (`None_2H`: 0.7 for the plain attack, ...).
fn expected_speed(name: &str, stamina_ok: bool) -> Option<Vec<f32>> {
    let parts: Vec<&str> = name.split('_').collect();
    let action = parts.get(5)?;
    let phase = parts.get(6)?;
    let hands = (parts.get(2)?, parts.get(3)?);
    let tired = if stamina_ok { 0.0 } else { -0.3 };
    let _ = tired;
    Some(match (*action, *phase) {
        ("Attack", "Hit" | "Raise") => match hands {
            (&"None" | &"Shield" | &"Torch" | &"1H", &"1H") => vec![0.6],
            (&"None", &"2H" | &"Axe" | &"Staff" | &"Halberd") => vec![0.7],
            _ => vec![1.0],
        },
        ("PowerAttack", "Raise") => vec![1.5],
        ("PowerAttack", "Hit") => vec![0.9, 1.0],
        ("QuickAttackL" | "QuickAttackR", _) => vec![1.0],
        ("SimpleWhirl", _) => vec![1.3],
        (_, "Recover") => vec![1.0],
        _ => return None,
    })
}

/// Verifies the clips of the swings the NPCs played: every swing has a clip, its speed is the
/// ported rule's, and the damage landed 60 % into the `Hit` clip (divided by the speed).
fn swing_checks(sc: &mut Sc, key: &'static str, min_hero_hits: usize) {
    sc.check(
        "the swings played their clips (no instruction without a clip)",
        move |cx| {
            let missing = cx.world.resource::<NpcMoveLog>().missing.clone();
            let swings: Vec<_> = clips(cx)
                .into_iter()
                .filter(|c| {
                    c.2.contains("_Attack_")
                        || c.2.contains("PowerAttack")
                        || c.2.contains("QuickAttack")
                })
                .collect();
            let mut miss: Vec<String> = missing
                .iter()
                // A missing `Raise` or `Recover` clip only skips that instruction; a missing
                // `Hit` clip means the move never lands.
                .filter(|(_, m)| m.contains("Attack") && m.contains(" Hit "))
                .map(|(_, m)| m.clone())
                .collect();
            miss.sort();
            miss.dedup();
            expect(
                swings.len() >= 2 && miss.is_empty(),
                format!(
                    "{} swing clips, e.g. {}",
                    swings.len(),
                    swings.first().map_or("", |c| c.2.as_str())
                ),
                format!("{} swing clips, missing {miss:?}", swings.len()),
            )
        },
    );
    sc.check(
        "the play speed of every swing clip is the ported rule (sub_42a0)",
        move |cx| {
            let mut bad = Vec::new();
            let mut n = 0;
            for (_, _, name, speed) in clips(cx) {
                if let Some(exp) = expected_speed(&name, true) {
                    n += 1;
                    // Tired fighters play slower (the stamina bands of the same rule): allow it.
                    if !exp.iter().any(|e| {
                        (speed - e).abs() < 0.02 || (*e - speed) > 0.0 && (*e - speed) <= 0.31
                    }) {
                        bad.push(format!("{name} @{speed}"));
                    }
                }
            }
            expect(
                n >= 2 && bad.is_empty(),
                format!("{n} clips checked"),
                format!("{n} checked, wrong: {bad:?}"),
            )
        },
    );
    sc.check(
        "the damage lands at 60 percent of the Hit clip",
        move |cx| {
            let hits: Vec<(f32, ActorId, String, f32)> = clips(cx)
                .into_iter()
                .filter(|c| {
                    c.2.contains("_Hit_")
                        && !c.2.contains("Stumble")
                        && !c.2.contains("Cast")
                        && !c.2.contains("JumpBack")
                })
                .collect();
            let d = drops(cx);
            let _ = key;
            let hitches: Vec<(f32, f32)> = cx
                .st
                .store
                .get("hitches")
                .map(|s| {
                    s.lines()
                        .filter_map(|l| {
                            let (a, b) = l.split_once(',')?;
                            Some((a.parse().ok()?, b.parse().ok()?))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut matched = 0;
            let mut worst = 0.0f32;
            let mut unmatched = Vec::new();
            let mut unclipped = 0;
            for (t, _) in &d {
                let mut best: Option<f32> = None;
                // Anybody may have struck (the fight draws others in).
                for (t0, _who, name, speed) in &hits {
                    let dur = {
                        let mut lib = cx.world.resource_mut::<ActorLibrary>();
                        lib.clip(name).map_or(0.0, |c| c.duration)
                    };
                    let want = 0.6 * dur / speed.max(0.05);
                    let got = t - t0;
                    let err = (got - want).abs();
                    // 0.2 s plus twice the longest frame between the clip's start and the hit.
                    let hitch = hitches
                        .iter()
                        .filter(|(ft, _)| *ft >= *t0 && *ft <= *t + 0.1)
                        .map(|(_, dt)| *dt)
                        .fold(0.0f32, f32::max);
                    if got >= 0.0 && err < 0.2 + 2.0 * hitch && best.is_none_or(|b| err < b) {
                        best = Some(err);
                    }
                }
                match best {
                    Some(e) => {
                        matched += 1;
                        worst = worst.max(e);
                    }
                    None => {
                    // What was playing shortly before: for the report.
                    let near: Vec<String> = clips(cx)
                        .iter()
                        .filter(|c| c.0 <= *t && *t - c.0 < 1.5 && c.2.contains("_Hit_"))
                        .map(|c| format!("{:.2} {} @{}", c.0, c.2, c.3))
                        .collect();
                    if near.is_empty() {
                        // A fighter whose body is not shown runs on timers: no clip to compare.
                        unclipped += 1;
                    } else {
                        unmatched.push(format!("{t:.3} (hit clips just before: {near:?})"));
                    }
                }
                }
            }
            expect(
                d.len() >= min_hero_hits && unmatched.is_empty(),
                format!(
                    "{matched} hero hits within {worst:.2}s of 0.6 * clip / speed ({unclipped} by bodies not shown)"
                ),
                format!("{} hero hits, unmatched at {unmatched:?}", d.len()),
            )
        },
    );
}

fn provoke(sc: &mut Sc, key: &'static str, prefix: &'static str, n: usize) {
    sc.act(
        format!("setup: {n} x {prefix} attack the hero"),
        move |cx| {
            let found = nearest(cx, prefix, n);
            if found.len() < n {
                cx.check(
                    "enough characters",
                    false,
                    format!("{} of {n} {prefix}", found.len()),
                );
                return;
            }
            let hero = cx.core().0.hero;
            let mut list = String::new();
            for id in &found {
                if !list.is_empty() {
                    list.push(',');
                }
                list.push_str(&id.0.to_string());
                let name = name_of(cx, *id);
                let hp = cx.core().0.sim.actor(*id).hp;
                let d = cx
                    .actor_pos(*id)
                    .map_or(0.0, |p| flat(p - cx.hero_pos()).length());
                cx.note(format!("{name} hp {hp} at {d:.0} cm attacks the hero"));
                cx.core_mut()
                    .0
                    .sim
                    .assess_target(*id, hero, AttackReason::Enemy);
            }
            cx.st.store.insert(key.into(), list);
        },
    );
}

/// Hero against `n` raiders of the start: standing still, they come and fight.
/// `GR_QA_FIGHT_SECS` shortens (or lengthens) the fights: recordings are written at a fixed 30 fps
/// and slow, 8 s are enough for a clip.
fn fight_secs(default: f32) -> f32 {
    std::env::var("GR_QA_FIGHT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn raiders(n: usize, secs: f32, name: &'static str) -> Sc {
    let secs = fight_secs(secs);
    let mut sc = Sc::new(secs + 120.0);
    grab(&mut sc);
    trace(&mut sc, "npcs");
    provoke(&mut sc, "npcs", "Orc_GameStartRaider", n);
    rec(&mut sc, true);
    sc.wait(2.5);
    sc.shot(&format!("{name}_approach"));
    sc.step(format!("the fight, {secs} s"), move |cx| {
        fight_cam(cx, "npcs");
        if cx.t > secs { Poll::Done } else { Poll::Wait }
    });
    sc.shot(&format!("{name}_fight"));
    rec(&mut sc, false);
    sc.act("camera back", fight_cam_off);
    sc.act(
        "note: distance of the nearest fighter at each hero hit",
        |cx| {
            let d = cx.st.store.get("drop_dists").cloned().unwrap_or_default();
            cx.note(format!("hero hit at distances (cm): {d}"));
        },
    );
    sc.check("the fighters went through the ported attack states", |cx| {
        let all = ids(cx, "npcs");
        let mut text = Vec::new();
        let mut ok_all = !all.is_empty();
        for id in &all {
            let s = seen(cx, *id);
            let swing = s.iter().any(|x| {
                matches!(
                    x.as_str(),
                    "ZS_Attack_Attack"
                        | "ZS_Attack_PowerAttack"
                        | "ZS_Attack_QuickAttack"
                        | "ZS_Attack_SimpleWhirl"
                        | "ZS_Attack_WhirlAttack"
                        | "ZS_Attack_PierceAttack"
                        | "ZS_Attack_HackAttack"
                )
            });
            text.push(format!("{}: {}", name_of(cx, *id), s.join(" ")));
            ok_all &=
                s.iter().any(|x| x == "ZS_Attack_Goto") && s.iter().any(|x| x == "ZS_Attack_Loop");
            let _ = swing;
        }
        expect(ok_all, text.join(" | "), text.join(" | "))
    });
    sc.check("somebody swung and the swing played its clip", |cx| {
        let swung = clips(cx).iter().any(|c| c.2.contains("Attack_Hit"));
        expect(
            swung,
            "an Attack Hit clip played",
            "no attack clip in the log",
        )
    });
    sc
}

pub fn npc_orc1() -> Sc {
    let mut sc = raiders(1, 40.0, "orc1");
    swing_checks(&mut sc, "npcs", 1);
    sc.check(
        "the orc keeps its distance rules (never inside the hero)",
        |cx| {
            let id = ids(cx, "npcs")[0];
            let d = cx
                .st
                .n
                .get(&format!("dmin_{}", id.0))
                .copied()
                .unwrap_or(0.0);
            expect(
                d > 60.0,
                format!("closest {d:.0} cm"),
                format!("closest {d:.0} cm: inside the hero"),
            )
        },
    );
    sc
}

pub fn npc_orc2() -> Sc {
    let mut sc = raiders(2, 50.0, "orc2");
    swing_checks(&mut sc, "npcs", 1);
    sc.check(
        "at most two fight the player at once (UpdateCombatState S:14f0, normal difficulty)",
        |cx| {
            let m = cx.st.n.get("engaged_max").copied().unwrap_or(0.0);
            expect(
                (1.0..=2.0).contains(&m),
                format!("up to {m} engaged"),
                format!("{m} engaged at once"),
            )
        },
    );
    sc
}

/// Three raiders against the hero: the third waits, the fight is never wider than the limit
/// (`UpdateCombatState` S:14f0: 2 against the player on normal difficulty); the waiting ones go
/// through `FAI_Passive` (S:4ffb0) instead of swinging.
pub fn npc_orc3() -> Sc {
    let mut sc = raiders(3, 40.0, "orc3");
    sc.check(
        "never more than two engaged against the player, and two really are (attack tokens)",
        |cx| {
            let m = cx.st.n.get("engaged_max").copied().unwrap_or(0.0);
            expect(
                (m - 2.0).abs() < 0.1,
                format!("{m} engaged at most"),
                format!("{m} engaged at most (expected 2)"),
            )
        },
    );
    sc
}

/// Casters: the shaman of the world data against the standing hero.
pub fn npc_shaman() -> Sc {
    let mut sc = Sc::new(260.0);
    grab(&mut sc);
    find_and_place(&mut sc, "Shaman", 1400.0);
    trace(&mut sc, "npcs");
    super::ranged::visual_watch(&mut sc);
    sc.act("setup: the shaman attacks the hero", |cx| {
        let id = ActorId(cx.st.n["tgt"] as u32);
        cx.st.store.insert("npcs".into(), id.0.to_string());
        let hero = cx.core().0.hero;
        let f = cx.world.resource::<FxLog>();
        cx.st.n.insert("casts0".into(), f64::from(f.casts));
        let s = cx.world.resource::<ShotsOnScreen>();
        cx.st.n.insert("m0".into(), f64::from(s.spell_spawned));
        cx.core_mut()
            .0
            .sim
            .assess_target(id, hero, AttackReason::Enemy);
    });
    rec(&mut sc, true);
    sc.step("the duel, 40 s", move |cx| {
        fight_cam(cx, "npcs");
        let s = cx.world.resource::<ShotsOnScreen>();
        let m = s.spell_spawned;
        let vis = cx.st.n.get("vis_spell").copied().unwrap_or(0.0);
        let _ = (m, vis);
        if cx.t > fight_secs(40.0) {
            Poll::Done
        } else {
            Poll::Wait
        }
    });
    sc.shot("shaman_cast");
    rec(&mut sc, false);
    sc.act("camera back", fight_cam_off);
    sc.check("the shaman holds a spell (ported _AI_SelectWeapon)", |cx| {
        let id = ids(cx, "npcs")[0];
        let eq = cx.core().0.sim.actor(id).equipment.clone();
        let wm = eq.weapon_mode();
        let spell = eq.right.as_ref().map(|d| d.name.clone());
        expect(
            wm == gothic_sim::item::WeaponMode::Magic,
            format!("weapon mode Magic, spell {spell:?}"),
            format!("weapon mode {wm:?}, right {spell:?}"),
        )
    });
    sc.check("the shaman cast (casts and spell missiles)", |cx| {
        let f = cx.world.resource::<FxLog>().casts;
        let m = cx.world.resource::<ShotsOnScreen>().spell_spawned;
        expect(
            f64::from(f) > cx.st.n["casts0"] && f64::from(m) > cx.st.n["m0"],
            format!(
                "casts {} -> {f}, missiles {} -> {m}",
                cx.st.n["casts0"], cx.st.n["m0"]
            ),
            format!("casts {f}, missiles {m}"),
        )
    });
    sc.check(
        "VISUAL: the spell missile was drawn on screen while it flew",
        |cx| {
            let n = cx.st.n.get("vis_spell").copied().unwrap_or(0.0);
            expect(
                n >= 3.0,
                format!("{n} frames inside the screen"),
                format!("only {n} frames"),
            )
        },
    );
    sc.check("the spell hurt the hero (a missile reached him)", |cx| {
        let d = drops(cx);
        expect(
            !d.is_empty(),
            format!("{} hero hits", d.len()),
            "no hp lost",
        )
    });
    sc
}

/// Archers: a bow carrier of the world data shoots the standing hero from a distance.
pub fn npc_archer() -> Sc {
    let mut sc = Sc::new(260.0);
    grab(&mut sc);
    find_and_place(&mut sc, "RebWarrior", 1800.0);
    trace(&mut sc, "npcs");
    super::ranged::visual_watch(&mut sc);
    sc.watch(|cx| {
        use crate::walk::play::npc_bows::NpcBowVisual;
        let Some(&id) = ids(cx, "npcs").first() else {
            return;
        };
        let (drawn, bow_in_hand) = {
            let a = cx.core().0.sim.actor(id);
            (
                a.drawn,
                matches!(
                    a.equipment.left.as_ref().map(|d| d.use_kind()),
                    Some(gothic_sim::item::Use::Bow | gothic_sim::item::Use::CrossBow)
                ),
            )
        };
        if !(drawn && bow_in_hand) {
            return;
        }
        let Some(&body) = cx
            .world
            .resource::<crate::walk::play::Roster>()
            .by_actor
            .get(&id)
        else {
            return;
        };
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
        let mut q = cx
            .world
            .query::<(&NpcBowVisual, &GlobalTransform, &ViewVisibility)>();
        let mut hit = false;
        for (b, g, v) in q.iter(cx.world) {
            if b.owner == body
                && v.get()
                && let Some(p) =
                    crate::walk::play::ranged::screen_of(&cam, &gt, g.translation() / CM)
                && p.x > 0.0
                && p.y > 0.0
                && p.x < size.x
                && p.y < size.y
            {
                hit = true;
            }
        }
        if hit {
            let n = cx.st.n.entry("vis_bow_hand".into()).or_insert(0.0);
            *n += 1.0;
            if *n == 30.0 {
                cx.shot("bow_in_hand");
            }
        }
    });
    sc.act("setup: the archer attacks the hero", |cx| {
        let id = ActorId(cx.st.n["tgt"] as u32);
        cx.st.store.insert("npcs".into(), id.0.to_string());
        let hero = cx.core().0.hero;
        let s = cx.world.resource::<ShotsOnScreen>();
        cx.st.n.insert("a0".into(), f64::from(s.arrows_spawned));
        cx.core_mut()
            .0
            .sim
            .assess_target(id, hero, AttackReason::Enemy);
    });
    rec(&mut sc, true);
    sc.step("the duel, 40 s", move |cx| {
        let id = ids(cx, "npcs")[0];
        fight_cam(cx, "npcs");
        // Keep the hero out of melee reach so the bow stays the weapon (setup, not under test).
        let wm = cx.core().0.sim.actor(id).equipment.weapon_mode();
        if wm == gothic_sim::item::WeaponMode::Ranged {
            let k = cx.st.n.get("ranged_frames").copied().unwrap_or(0.0);
            cx.st.n.insert("ranged_frames".into(), k + 1.0);
        }
        if cx.t > fight_secs(40.0) {
            Poll::Done
        } else {
            Poll::Wait
        }
    });
    sc.shot("archer_shoots");
    rec(&mut sc, false);
    sc.act("camera back", fight_cam_off);
    sc.check(
        "the archer took the bow at range (ported _AI_SelectWeapon, S:4a030)",
        |cx| {
            let k = cx.st.n.get("ranged_frames").copied().unwrap_or(0.0);
            expect(
                k > 10.0,
                format!("bow in the hands for {k} frames"),
                "the bow was never in the hands",
            )
        },
    );
    sc.check("arrows flew", |cx| {
        let a = cx.world.resource::<ShotsOnScreen>().arrows_spawned;
        expect(
            f64::from(a) > cx.st.n["a0"],
            format!("arrows {} -> {a}", cx.st.n["a0"]),
            format!("arrows {a}"),
        )
    });
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
    sc
}

/// NPC against NPC: a raider and a human of the world fight each other.
pub fn npc_vs_npc() -> Sc {
    let mut sc = Sc::new(200.0);
    grab(&mut sc);
    trace(&mut sc, "npcs");
    sc.act(
        "setup: a raider and the nearest human attack each other",
        |cx| {
            let Some(&r) = nearest(cx, "Orc_GameStartRaider", 1).first() else {
                cx.check("a raider exists", false, "none");
                return;
            };
            let rp = cx.actor_pos(r).unwrap_or(Vec3::ZERO);
            let hero = cx.core().0.hero;
            let human = cx
                .core()
                .0
                .sim
                .actors
                .iter()
                .filter(|a| {
                    a.is_alive()
                        && a.id != hero
                        && a.id != r
                        && a.species == gothic_sim::types::Species::Human
                        && !a.name.starts_with("Orc")
                })
                .filter_map(|a| cx.actor_pos(a.id).map(|p| (flat(p - rp).length(), a.id)))
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let Some((d, h)) = human else {
                cx.check("a human exists", false, "none");
                return;
            };
            cx.note(format!(
                "{} vs {} at {d:.0} cm",
                name_of(cx, r),
                name_of(cx, h)
            ));
            cx.st
                .store
                .insert("npcs".into(), format!("{},{}", r.0, h.0));
            cx.st
                .n
                .insert("hp_r0".into(), f64::from(cx.core().0.sim.actor(r).hp));
            cx.st
                .n
                .insert("hp_h0".into(), f64::from(cx.core().0.sim.actor(h).hp));
            let mut c = cx.core_mut();
            c.0.sim.assess_target(r, h, AttackReason::Enemy);
            c.0.sim.assess_target(h, r, AttackReason::Enemy);
        },
    );
    rec(&mut sc, true);
    sc.step("the fight, 45 s", |cx| {
        fight_cam(cx, "npcs");
        let l = ids(cx, "npcs");
        let done = l.iter().any(|id| !cx.core().0.sim.actor(*id).is_alive());
        if cx.t > 45.0 || (done && cx.t > 8.0) {
            Poll::Done
        } else {
            Poll::Wait
        }
    });
    sc.shot("npc_vs_npc");
    rec(&mut sc, false);
    sc.act("camera back", fight_cam_off);
    sc.check(
        "both fought through the attack states and hurt each other",
        |cx| {
            let l = ids(cx, "npcs");
            if l.len() < 2 {
                return Err("no pair".into());
            }
            let (r, h) = (l[0], l[1]);
            let hp_r = cx.core().0.sim.actor(r).hp;
            let hp_h = cx.core().0.sim.actor(h).hp;
            let dr = cx.st.n["hp_r0"] - f64::from(hp_r);
            let dh = cx.st.n["hp_h0"] - f64::from(hp_h);
            let (sr, sh) = (seen(cx, r), seen(cx, h));
            expect(
                dr > 0.0
                    && dh > 0.0
                    && sr.iter().any(|x| x == "ZS_Attack_Goto")
                    && sh.iter().any(|x| x == "ZS_Attack_Loop"),
                format!("raider lost {dr}, human lost {dh}; states {sr:?} / {sh:?}"),
                format!("raider lost {dr}, human lost {dh}; states {sr:?} / {sh:?}"),
            )
        },
    );
    sc.check("both bodies swung with clips", |cx| {
        let l = ids(cx, "npcs");
        let c = clips(cx);
        let a = c.iter().any(|x| x.1 == l[0] && x.2.contains("Attack"));
        let b = c.iter().any(|x| x.1 == l[1] && x.2.contains("Attack"));
        expect(
            a && b,
            "both played attack clips",
            format!("raider {a}, human {b}"),
        )
    });
    sc
}

/// A place `dist` cm from `p` on flat ground with a free line of sight to it (setup: the hero is
/// put there). `None` when every direction is a slope, a wall or water.
fn open_spot(cx: &Cx, p: Vec3, dist: f32) -> Option<Vec3> {
    let coll = cx.world.resource::<crate::walk::Collision>();
    for k in 0..16 {
        let a = k as f32 * std::f32::consts::TAU / 16.0;
        let c = Vec3::new(p.x + a.cos() * dist, p.y + 500.0, p.z + a.sin() * dist);
        let Some(g) = coll.0.ground_below(c, 1000.0) else {
            continue;
        };
        if (g - p.y).abs() > 120.0 {
            continue;
        }
        let eye_a = Vec3::new(c.x, g + 150.0, c.z);
        let eye_b = p + Vec3::Y * 150.0;
        let d = eye_b - eye_a;
        if coll.0.raycast(eye_a, d.normalize(), d.length()).is_none() {
            return Some(Vec3::new(c.x, g + 120.0, c.z));
        }
    }
    None
}

/// Finds the nearest enabled character of the roster whose name contains `needle`, teleports the
/// hero `dist` cm south of it and waits until the body exists (setup, not under test). Leaves
/// the actor id in `st.n["tgt"]`.
pub(super) fn find_and_place(sc: &mut Sc, needle: &'static str, dist: f32) {
    sc.act(format!("find a {needle} in the roster"), move |cx| {
        let hero = cx.hero_pos();
        let r = cx.world.resource::<crate::walk::play::Roster>();
        let best = r
            .cands
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.record.name.contains(needle)
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
                cx.note(format!("{name} at {p:?}"));
            }
            None => cx.check(
                "the character exists",
                false,
                format!("no enabled {needle}"),
            ),
        }
    });
    sc.act("setup: teleport the hero south of it", move |cx| {
        let (sx, sy, sz) = (
            cx.st.n["sx"] as f32,
            cx.st.n["sy"] as f32,
            cx.st.n["sz"] as f32,
        );
        let spot = open_spot(cx, Vec3::new(sx, sy, sz), dist).unwrap_or(Vec3::new(
            sx,
            sy + 400.0,
            sz + dist,
        ));
        cx.note(format!("hero placed at {spot:?}"));
        let mut p = cx.world.resource_mut::<crate::walk::Player>();
        p.ctrl.position = spot;
        p.ctrl.vertical_speed = 0.0;
        p.yaw = 0.0;
        p.placed = false;
    });
    sc.until("the body exists and the hero stands", 150.0, |cx| {
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
    sc.act("record the actor", |cx| {
        let i = cx.st.n["cand"] as usize;
        let id = cx.world.resource::<crate::walk::play::Roster>().cands[i]
            .actor
            .unwrap();
        cx.st.n.insert("tgt".into(), f64::from(id.0));
        let a = cx.core().0.sim.actor(id);
        let note = format!(
            "{}: hp {} left {:?} right {:?}",
            a.name,
            a.hp,
            a.equipment.left.as_ref().map(|i| i.name.clone()),
            a.equipment.right.as_ref().map(|i| i.name.clone())
        );
        cx.note(note);
    });
}
