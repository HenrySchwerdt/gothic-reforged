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
    // A step of a locally generated clip pack (see `er_pack`): `pack:ER_{S}_Light1`.
    if action.starts_with(gothic_combat_souls::animmap::PACK_PREFIX) {
        let style = super::er_pack::style(stance)?;
        let name = gothic_combat_souls::animmap::pack_name(action, style)?;
        return lib.has_motion(&name).then_some(name);
    }
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
            // A script clip plays on the whole body: the original has no body mask and no
            // additive layer (`SetBlendMode` mix is never set, see WIRING_ENGINE_STATUS.md); a
            // motion replaces the nodes it has tracks for, blended in by its intro fade.
            let mask: Vec<f32> = if std::env::var_os("GR_OVERLAY_UPPER").is_some() {
                // the old host rule, kept for before/after renders (`overlay_visual` QA)
                rig.upper_body_mask()
            } else {
                vec![1.0; rig.slot_node.len()]
            };
            animator.set_overlay(Some(Overlay {
                motion,
                time: 0.0,
                rate: 1.0,
                weight: 1.0,
                mask: mask.into(),
            }));
            true
        }
        Err(e) => {
            eprintln!("overlay {name}: {e:#}");
            false
        }
    }
}

/// Runs the overlay queue: clears finished overlays and starts the next step. The weight of a
/// running overlay follows the clip's fades of `G3_Ani_Blendings.txt`: it comes in over the
/// smaller of the extro of what played before and its own intro (`sAIPlayAniStart`,
/// Game:0x1016a360, `blend::script_fade`) and leaves over the standard fade at its end
/// (the locomotion mixer replacing a finished clip, `blend::mixer_fade`).
pub fn overlay_system(
    mut lib: ResMut<ActorLibrary>,
    blendings: Res<super::Blendings>,
    mut q: Query<(Entity, &mut Animator, &mut OverlayQueue, &CombatStance)>,
    mut fades: Local<std::collections::HashMap<Entity, (String, f32)>>,
) {
    for (entity, mut animator, mut queue, stance) in &mut q {
        let done = animator
            .overlay
            .as_ref()
            .is_none_or(|o| o.time >= o.motion.clip.duration);
        if !done {
            // intro fade of a fresh overlay (what played before is the primary clip)
            let name = animator
                .overlay
                .as_ref()
                .map(|o| o.motion.clip.name.clone())
                .unwrap_or_default();
            let intro = match fades.get(&entity) {
                Some((n, f)) if *n == name => *f,
                _ => {
                    let old = animator.current_name().unwrap_or("").to_owned();
                    let f = gothic_animation::blend::script_fade(&blendings.0, &old, &name);
                    fades.insert(entity, (name, f));
                    f
                }
            };
            let outro = gothic_animation::blend::STANDARD_FADE;
            if let Some(o) = animator.overlay.as_mut()
                && !super::er_pack::is_pack(o)
            {
                let t = o.time;
                let d = o.motion.clip.duration;
                let w_in = if intro > 1e-4 {
                    (t / intro).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let w_out = if outro > 1e-4 {
                    ((d - t) / outro).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                o.weight = w_in.min(w_out);
            }
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

/// Queues arbitrary overlay steps `(action, phase)`.
pub fn queue_steps(queue: &mut OverlayQueue, steps: &[(&'static str, &'static str)]) {
    queue.0.clear();
    queue.0.extend(steps.iter().copied());
}

pub fn queue_stumble(queue: &mut OverlayQueue) {
    queue.0.clear();
    queue.0.push_back(("QuickStumble", "Hit"));
}

/// The clips of the reaction an `AssessHit` result asks for (`_AI_QuickStumble`, `_AI_Stumble`,
/// `_AI_PierceStumble`, `_AI_ParadeStumble`, `_AI_HeavyParadeStumble`: the `gEAction` of the
/// result is the motion's action; the sided ones exist as `...L` / `...R`). The first clip the
/// character's set has plays; `None` for results that have no stumble (knock-downs and deaths
/// play the lying clips).
pub fn queue_hit_reaction(
    queue: &mut OverlayQueue,
    result: gothic_sim::combat::HitResult,
    from_left: bool,
) {
    use gothic_sim::combat::HitResult as R;
    let (a, b): (&'static str, &'static str) = match result {
        R::QuickStumble => ("QuickStumble", "QuickStumble"),
        R::Stumble => ("StumbleL", "StumbleR"),
        R::PierceStumble => ("PierceStumble", "PierceStumble"),
        R::ParadeStumble | R::QuickParadeStumble | R::MagicParade => {
            ("ParadeStumbleL", "ParadeStumbleR")
        }
        R::HeavyParadeStumble => ("HeavyParadeStumble", "ParadeStumbleL"),
        _ => return,
    };
    let (first, second) = if from_left { (a, b) } else { (b, a) };
    queue.0.clear();
    queue.0.push_back((first, "Hit"));
    queue.0.push_back((second, "Hit"));
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
