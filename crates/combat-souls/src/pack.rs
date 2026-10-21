//! Glue between the Souls flow and a local clip pack (`gothic_animation::clip_pack`): which pack
//! clip plays for which action, and the action windows stored with the clips. Everything here
//! is a no-op for actions the pack has no clip for, so a missing or partial pack leaves the
//! stock Gothic motions and the built-in timing in place.
use crate::{
    actions::{self, ActionId, Timing},
    animmap::{self, AnimMap},
};
use gothic_animation::{
    Stance,
    clip_pack::{ClipPack, ClipTiming},
};

/// Name prefix of the pack's clips in the animation library.
pub const PREFIX: &str = "ER_";

/// Weapon style used in the pack's clip names for a stance (`None`: no pack clips for it).
pub fn style(stance: Stance) -> Option<&'static str> {
    match stance {
        Stance::OneHand | Stance::OneHandShield => Some("Sword1H"),
        Stance::TwoHand => Some("Great2H"),
        _ => None,
    }
}

/// Pack timing as flow timing. Reactions keep the flow's own i-frames and travel.
pub fn to_timing(t: &ClipTiming, id: ActionId) -> Timing {
    let reaction = id.is_reaction();
    Timing {
        total: t.total,
        input_from: Some(t.input_from),
        cancel_light: Some(t.cancel_light),
        cancel_heavy: Some(t.cancel_heavy),
        cancel_dodge: Some(t.cancel_dodge),
        cancel_guard: Some(t.cancel_guard),
        cancel_move: Some(t.cancel_move),
        iframes: (!reaction).then_some(t.iframes),
        hit: t.hits.first().map(|h| (h.0, h.1)),
        travel: if reaction { None } else { t.travel },
        // A held heavy attack waits shortly before its first hit.
        hold: (id == ActionId::Heavy)
            .then(|| t.hits.first().map(|h| (h.0 - 3.0).max(2.0)))
            .flatten(),
    }
}

#[derive(Debug, Default)]
pub struct Installed {
    pub remapped: usize,
    pub timed: usize,
    pub strafe: bool,
}

/// Points the animation map at the pack's clips and installs the pack's action timing (taken
/// from the one-handed clips, falling back to the two-handed ones).
pub fn install(anim: &mut AnimMap, pack: &ClipPack) -> Installed {
    let has = |n: &str| pack.get(n).is_some();
    let taken = anim.use_pack(PREFIX, &has);
    let mut out = Installed {
        remapped: taken.len(),
        strafe: has("ER_Walk_Front") || has("ER_Run_Front"),
        ..Default::default()
    };
    for id in &taken {
        let Some(pattern) = animmap::pack_clip(*id) else {
            continue;
        };
        let timing = ["Sword1H", "Great2H"].iter().find_map(|s| {
            pack.get(&format!("{PREFIX}{}", pattern.replace("{S}", s)))
                .and_then(|c| c.timing.as_ref())
        });
        if let Some(t) = timing {
            actions::install_timing(*id, &to_timing(t, *id));
            out.timed += 1;
        }
    }
    out
}
