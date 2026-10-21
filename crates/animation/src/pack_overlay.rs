//! Full-body overlays from a clip pack (see [`crate::clip_pack`]): helpers that start, time and
//! fade the overlay layer of an [`Animator`] with a pack clip. A pack clip is recognised by the
//! name prefix its clips were registered with (`ER_`, ...).
use crate::{
    animator::{Animator, Overlay},
    library::ActorLibrary,
};
use std::sync::Arc;

/// Whether the animator's overlay is a pack clip with this name prefix.
pub fn is_pack(o: &Overlay, prefix: &str) -> bool {
    o.motion.clip.name.starts_with(prefix)
}

/// Full-body weights for the rig of `animator`.
pub fn full_mask(animator: &Animator) -> Arc<[f32]> {
    Arc::from(vec![1.0f32; animator.rig.slot_node.len()])
}

/// Starts overlay `name` (full body, weight 1) unless the same clip already plays.
pub fn start(lib: &mut ActorLibrary, animator: &mut Animator, name: &str, rate: f32) -> bool {
    if let Some(o) = animator.overlay.as_mut()
        && o.motion.clip.name.eq_ignore_ascii_case(name)
    {
        o.rate = rate;
        o.weight = 1.0;
        return true;
    }
    let rig = animator.rig.clone();
    match lib.motion(&rig, name) {
        Ok(motion) => {
            let mask = full_mask(animator);
            animator.set_overlay(Some(Overlay {
                motion,
                time: 0.0,
                rate,
                weight: 1.0,
                mask,
            }));
            true
        }
        Err(_) => false,
    }
}

/// Like [`start`], but a running one-shot overlay that is not a pack clip is left alone.
pub fn hold_loop(
    lib: &mut ActorLibrary,
    animator: &mut Animator,
    prefix: &str,
    name: &str,
    rate: f32,
) -> bool {
    if animator
        .overlay
        .as_ref()
        .is_some_and(|o| !is_pack(o, prefix) && o.time < o.motion.clip.duration)
    {
        return false;
    }
    start(lib, animator, name, rate)
}

/// Pins a running pack overlay to `seconds` into its clip (rate 0), e.g. to an action frame.
pub fn follow_time(animator: &mut Animator, prefix: &str, seconds: f32) {
    if let Some(o) = animator.overlay.as_mut()
        && is_pack(o, prefix)
    {
        o.rate = 0.0;
        o.weight = 1.0;
        o.time = seconds.clamp(0.0, o.motion.clip.duration);
    }
}

/// Fades a running pack overlay out over `seconds` and clears it when gone.
pub fn fade_out(animator: &mut Animator, prefix: &str, dt: f32, seconds: f32) {
    let done = match animator.overlay.as_mut() {
        Some(o) if is_pack(o, prefix) => {
            o.rate = 0.0;
            o.weight -= dt / seconds.max(1e-3);
            o.weight <= 0.0
        }
        _ => false,
    };
    if done {
        animator.set_overlay(None);
    }
}
