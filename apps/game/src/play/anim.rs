//! One-shot character reactions: attack swings, hit stumbles and death, on top of locomotion.
use bevy::prelude::*;
use gothic_animation::names::MotionQuery;
use gothic_animation::{ActorLibrary, Animator, Overlay, Stance};
use std::collections::VecDeque;

/// Queued upper-body overlay steps `(action, phase)`.
#[derive(Component, Default)]
pub struct OverlayQueue(pub VecDeque<(&'static str, &'static str)>);

/// The character's weapon stance as the animation set sees it.
#[derive(Component, Clone, Copy)]
pub struct CombatStance(pub Stance);

/// Dead or knocked out: the lying pose is held.
#[derive(Component)]
pub struct Fallen;

pub fn find_motion(
    lib: &ActorLibrary,
    family: &str,
    stance: Stance,
    pose: &str,
    action: &str,
    phase: &str,
) -> Option<String> {
    let (left, right) = stance.hands();
    let tries = [
        (Some(left), Some(right)),
        (Some("None"), Some(right)),
        (None, Some(right)),
        (None, None),
    ];
    for (l, r) in tries {
        let q = MotionQuery {
            pose: Some(pose),
            left: l,
            right: r,
            pn: Some("P0"),
            action: Some(action),
            phase: Some(phase),
            ..Default::default()
        };
        if let Some(n) = lib.catalog.find(family, &q) {
            return Some(n.to_string());
        }
    }
    None
}

/// Plays one overlay step on the upper body.
pub fn start_overlay(
    lib: &mut ActorLibrary,
    animator: &mut Animator,
    stance: Stance,
    action: &str,
    phase: &str,
) -> bool {
    let family = animator.rig.family.clone();
    let Some(name) = find_motion(lib, &family, stance, "Stand", action, phase) else {
        return false;
    };
    let rig = animator.rig.clone();
    match lib.motion(&rig, &name) {
        Ok(motion) => {
            animator.set_overlay(Some(Overlay {
                motion,
                time: 0.0,
                rate: 1.0,
                weight: 1.0,
                mask: rig.upper_body_mask().into(),
            }));
            true
        }
        Err(e) => {
            eprintln!("overlay {name}: {e:#}");
            false
        }
    }
}

/// Runs the overlay queue: clears finished overlays and starts the next step.
pub fn overlay_system(
    mut lib: ResMut<ActorLibrary>,
    mut q: Query<(&mut Animator, &mut OverlayQueue, &CombatStance)>,
) {
    for (mut animator, mut queue, stance) in &mut q {
        let done = animator
            .overlay
            .as_ref()
            .is_none_or(|o| o.time >= o.motion.clip.duration);
        if !done {
            continue;
        }
        animator.overlay = None;
        while let Some((action, phase)) = queue.0.pop_front() {
            if start_overlay(&mut lib, &mut animator, stance.0, action, phase) {
                break;
            }
        }
    }
}

/// Queues a swing (raise, strike, recover).
pub fn queue_swing(queue: &mut OverlayQueue) {
    queue.0.clear();
    queue.0.extend([("Attack", "Raise"), ("Attack", "Hit")]);
}

pub fn queue_stumble(queue: &mut OverlayQueue) {
    queue.0.clear();
    queue.0.push_back(("QuickStumble", "Hit"));
}

/// Base-layer death / knock-out motion; returns whether one was found.
pub fn play_fall(lib: &mut ActorLibrary, animator: &mut Animator, stance: Stance) -> bool {
    let family = animator.rig.family.clone();
    animator.overlay = None;
    let name = find_motion(lib, &family, stance, "LieKnockDown", "Die", "Begin")
        .or_else(|| find_motion(lib, &family, stance, "LieDead", "Ambient", "Loop"));
    let Some(name) = name else { return false };
    let rig = animator.rig.clone();
    match lib.motion(&rig, &name) {
        Ok(m) => {
            animator.play(m, 0.1, 1.0, 0.0);
            true
        }
        Err(e) => {
            eprintln!("fall {name}: {e:#}");
            false
        }
    }
}
