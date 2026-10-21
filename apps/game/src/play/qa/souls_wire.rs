//! `--play --combat souls --qa souls_feedback`: the SOULS-03 wiring in the real game. Locks onto
//! the start raider and checks the HUD lock line, then holds the guard until the raider swings
//! and checks the telegraph / commit events, the impact feedback and the camera shake (or its
//! absence with `--souls-shake 0`). Screenshots: `NN_hud_lock.png`, `NN_hud_telegraph.png`,
//! `NN_hud_feedback.png`.
use super::more::approach;
use super::more2::{flat, grab, pick_target, target_goal, tgt_id};
use super::{Cx, Poll, Sc, expect};
use crate::walk::play::souls_hud::SoulsHudModel;
use crate::walk::play::souls_wire::LockView;
use bevy::prelude::*;
use gothic_combat_souls::plugin::{CameraShake, SoulsRes};
use std::sync::Arc;

fn hud<'a>(cx: &'a Cx<'_>) -> &'a SoulsHudModel {
    cx.world.resource::<SoulsHudModel>()
}

fn face_target(cx: &mut Cx) {
    if let Some(p) = tgt_id(cx).and_then(|i| cx.actor_pos(i)) {
        cx.aim_at(p, -0.2);
    }
}

pub fn souls_feedback() -> Sc {
    let mut sc = Sc::new(300.0);
    sc.step("souls mode is on (--combat souls)", |cx| {
        if cx.world.contains_resource::<SoulsRes>() {
            Poll::Done
        } else {
            Poll::Fail("run with --play --combat souls".into())
        }
    });
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
        face_target(cx);
        if cx.t > 1.0 { Poll::Done } else { Poll::Wait }
    });
    sc.act("Tab: lock on", |cx| cx.tap(KeyCode::Tab));
    sc.wait(0.8);
    sc.check("the HUD shows the lock on the raider", |cx| {
        let res = cx.world.resource::<SoulsRes>();
        let target = res.world.lock.target;
        let want = tgt_id(cx).map(|id| id.0 + 1);
        let line = hud(cx).shown_lock.clone();
        expect(
            target.is_some() && target == want && line.starts_with("LOCK"),
            format!(
                "{line:?}, locked {target:?}, lock changes {}",
                hud(cx).counts.lock_changes
            ),
            format!("lock target {target:?}, wanted {want:?}, HUD {line:?}"),
        )
    });
    sc.shot("hud_lock");
    sc.act("hold Q: guard", |cx| cx.key_down(KeyCode::KeyQ));
    sc.step("wait up to 25 s for a telegraphed swing", |cx| {
        face_target(cx);
        if !hud(cx).shown_threat.is_empty() || hud(cx).counts.telegraphs > 0 {
            return Poll::Done;
        }
        if cx.t > 25.0 || cx.play().hero_down {
            return Poll::Done;
        }
        Poll::Wait
    });
    sc.shot("hud_telegraph");
    sc.step("keep guarding until an impact (up to 20 s)", |cx| {
        face_target(cx);
        let close = tgt_id(cx)
            .and_then(|i| cx.actor_pos(i))
            .map(|p| flat(p - cx.hero_pos()).length());
        if close.is_some_and(|d| d > 300.0) {
            cx.key_down(KeyCode::KeyW);
        } else {
            cx.key_up(KeyCode::KeyW);
        }
        if hud(cx).counts.feedbacks > 0 || cx.t > 20.0 || cx.play().hero_down {
            cx.key_up(KeyCode::KeyW);
            return Poll::Done;
        }
        Poll::Wait
    });
    sc.shot("hud_feedback");
    sc.act("release Q", |cx| cx.key_up(KeyCode::KeyQ));
    sc.act("log the counters", |cx| {
        let c = hud(cx).counts;
        let v = cx.world.resource::<LockView>();
        let line = format!(
            "telegraphs {} commits {} feedbacks {} lock changes {} | occluded frames {} | \
             shaken frames {} max shake {:.2} cm",
            c.telegraphs,
            c.commits,
            c.feedbacks,
            c.lock_changes,
            v.occluded_frames,
            v.shaken_frames,
            v.max_shake_cm
        );
        cx.note(line);
    });
    sc.check("an NPC swing sent FoeTelegraph", |cx| {
        let n = hud(cx).counts.telegraphs;
        expect(n > 0, format!("{n} telegraph(s)"), "no telegraph in 25 s")
    });
    sc.check("a telegraph reached its commit cue", |cx| {
        let n = hud(cx).counts.commits;
        expect(n > 0, format!("{n} commit(s)"), "no FoeCommit")
    });
    sc.check("a guarded swing sent impact feedback", |cx| {
        let n = hud(cx).counts.feedbacks;
        expect(
            n > 0,
            format!("{n} feedback event(s)"),
            "no Event::Feedback",
        )
    });
    sc.check(
        "the feedback shook the camera (scaled by --souls-shake)",
        |cx| {
            let strength = cx.world.resource::<CameraShake>().strength.unwrap_or(1.0);
            let fb = hud(cx).counts.feedbacks;
            let max = cx.world.resource::<LockView>().max_shake_cm;
            if strength <= 0.0 {
                expect(
                    max == 0.0,
                    "shake off: no offset",
                    format!("shake off but moved {max:.2} cm"),
                )
            } else {
                expect(
                    fb > 0 && max > 0.0,
                    format!("peak {max:.2} cm at strength {strength}"),
                    format!("no shake (feedbacks {fb}, strength {strength})"),
                )
            }
        },
    );
    sc
}
