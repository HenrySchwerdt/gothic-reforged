//! Action -> animation mapping. Timing lives in [`crate::actions`]; this table only says which
//! motion plays for an action, so animations can be swapped (for example for locally extracted
//! ones) without touching any rule.
//!
//! Entries use the Gothic 3 motion grammar of `gothic-animation`: `(action, phase)` pairs such as
//! `("QuickAttackR", "Hit")` that `find_motion` resolves for the character's family and stance.
//! Each action has an ordered list of candidate sequences; the host plays the first sequence
//! whose every step exists in the animation set (humans lack some clips, monsters have others).
use crate::actions::{ActionId, Dir, Hurt as H};
use std::collections::HashMap;

/// One motion step: `(action, phase)`.
pub type Step = (String, String);
/// Steps played back to back.
pub type Seq = Vec<Step>;

fn seq(steps: &[(&str, &str)]) -> Seq {
    steps
        .iter()
        .map(|&(a, p)| (a.to_string(), p.to_string()))
        .collect()
}

#[derive(Clone, Debug)]
pub struct AnimMap {
    table: HashMap<ActionId, Vec<Seq>>,
    /// Looping pose while the guard is held.
    pub guard_hold: Vec<Seq>,
    pub death: Vec<Seq>,
}

impl Default for AnimMap {
    fn default() -> Self {
        Self::gothic()
    }
}

impl AnimMap {
    /// The built-in mapping onto existing Gothic 3 motions.
    pub fn gothic() -> Self {
        use ActionId::*;
        let mut t: HashMap<ActionId, Vec<Seq>> = HashMap::new();
        let jump_fallback = seq(&[("Jump", "Stand")]);
        let back = vec![
            seq(&[("EvadeBackward", "Hit")]),
            seq(&[("JumpBack", "Hit"), ("JumpBack", "Recover")]),
            seq(&[("JumpBack", "Hit")]),
            jump_fallback.clone(),
        ];
        t.insert(Backstep, back.clone());
        t.insert(Roll(Dir::Back), back);
        t.insert(
            Roll(Dir::Front),
            vec![seq(&[("EvadeForward", "Hit")]), jump_fallback.clone()],
        );
        t.insert(
            Roll(Dir::Left),
            vec![seq(&[("EvadeLeft", "Hit")]), jump_fallback.clone()],
        );
        t.insert(
            Roll(Dir::Right),
            vec![seq(&[("EvadeRight", "Hit")]), jump_fallback],
        );
        t.insert(
            Light(1),
            vec![
                seq(&[("QuickAttackR", "Hit"), ("QuickAttackR", "Recover")]),
                seq(&[("QuickAttackR", "Hit")]),
                seq(&[("Attack", "Hit")]),
            ],
        );
        t.insert(
            Light(2),
            vec![
                seq(&[("QuickAttackL", "Hit"), ("QuickAttackL", "Recover")]),
                seq(&[("QuickAttackL", "Hit")]),
                seq(&[("Attack", "Hit")]),
            ],
        );
        t.insert(
            Light(3),
            vec![
                seq(&[("Attack", "Raise"), ("Attack", "Hit")]),
                seq(&[("Attack", "Hit")]),
            ],
        );
        t.insert(
            Heavy,
            vec![
                seq(&[("PowerAttack", "Raise"), ("PowerAttack", "Hit")]),
                seq(&[("Attack", "Raise"), ("Attack", "Hit")]),
            ],
        );
        t.insert(
            RunLight,
            vec![seq(&[("Attack", "Hit")]), seq(&[("QuickAttackR", "Hit")])],
        );
        t.insert(
            RollLight,
            vec![seq(&[("QuickAttackL", "Hit")]), seq(&[("Attack", "Hit")])],
        );
        t.insert(
            GuardCounter,
            vec![
                seq(&[("PierceAttack", "Raise"), ("PierceAttack", "Hit")]),
                seq(&[("PowerAttack", "Hit")]),
                seq(&[("Attack", "Hit")]),
            ],
        );
        t.insert(
            Riposte,
            vec![
                seq(&[("FinishingAttack", "Hit")]),
                seq(&[("PierceAttack", "Hit")]),
                seq(&[("PowerAttack", "Hit")]),
            ],
        );
        t.insert(
            GuardHit,
            vec![
                seq(&[("ParadeStumbleR", "Hit")]),
                seq(&[("ParadeStumbleL", "Hit")]),
                seq(&[("QuickStumble", "Hit")]),
            ],
        );
        t.insert(
            GuardBreak,
            vec![
                seq(&[("StumbleL", "Hit"), ("StumbleL", "Recover")]),
                seq(&[("StumbleL", "Hit")]),
                seq(&[("QuickStumble", "Hit")]),
            ],
        );
        t.insert(
            Stagger,
            vec![
                seq(&[("StumbleR", "Hit"), ("StumbleR", "Recover")]),
                seq(&[("StumbleR", "Hit")]),
                seq(&[("QuickStumble", "Hit")]),
            ],
        );
        t.insert(
            ActionId::Hurt(H::Small),
            vec![
                seq(&[("LightStumble", "Hit")]),
                seq(&[("QuickStumble", "Hit")]),
            ],
        );
        t.insert(
            ActionId::Hurt(H::Middle),
            vec![seq(&[("QuickStumble", "Hit")])],
        );
        t.insert(
            ActionId::Hurt(H::Large),
            vec![seq(&[("StumbleL", "Hit")]), seq(&[("QuickStumble", "Hit")])],
        );
        t.insert(
            ActionId::Hurt(H::Knockdown),
            vec![
                seq(&[("LieKnockDown", "Begin")]),
                seq(&[("StumbleL", "Hit")]),
            ],
        );
        Self {
            table: t,
            guard_hold: vec![seq(&[("Parade", "Begin")])],
            death: vec![seq(&[("Die", "Begin")])],
        }
    }

    /// Candidate sequences for an action, best first.
    pub fn get(&self, id: ActionId) -> &[Seq] {
        self.table.get(&id).map_or(&[], Vec::as_slice)
    }

    /// Puts clips of a local clip pack first for every action [`pack_clip`] covers. `prefix` is
    /// the pack's clip-name prefix (`ER_`); `has(name)` says whether a clip exists for the
    /// weapon style `{S}` left as is (it is probed with both styles, and an action is taken
    /// over when at least one exists). Returns the actions that were taken over.
    pub fn use_pack(&mut self, prefix: &str, has: &dyn Fn(&str) -> bool) -> Vec<ActionId> {
        let mut taken = Vec::new();
        for id in pack_actions() {
            let Some(pattern) = pack_clip(id) else {
                continue;
            };
            let ok = ["Sword1H", "Great2H"]
                .iter()
                .any(|s| has(&format!("{prefix}{}", pattern.replace("{S}", s))));
            if !ok {
                continue;
            }
            let step = (format!("{PACK_PREFIX}{prefix}{pattern}"), String::new());
            let mut cands = vec![vec![step]];
            cands.extend(self.table.get(&id).cloned().unwrap_or_default());
            self.table.insert(id, cands);
            taken.push(id);
        }
        // Held guard: the pack's guard pose first.
        let step = (format!("{PACK_PREFIX}{prefix}{{S}}_Guard"), String::new());
        if ["Sword1H", "Great2H"]
            .iter()
            .any(|s| has(&format!("{prefix}{s}_Guard")))
        {
            self.guard_hold.insert(0, vec![step]);
        }
        taken
    }

    /// Replaces the candidates of one action (for example with extracted animations).
    pub fn set(&mut self, id: ActionId, seqs: Vec<Seq>) {
        self.table.insert(id, seqs);
    }

    /// Playback rate that makes a clip of `clip_secs` fill the action's active span.
    pub fn rate(id: ActionId, clip_secs: f32) -> f32 {
        let span = id.def().total / crate::tuning::ANIM_FPS;
        if clip_secs <= 0.0 {
            1.0
        } else {
            (clip_secs / span).clamp(0.6, 2.5)
        }
    }

    /// Sequences of a foe attack: raise (stretched over the wind-up) then the strike.
    pub fn foe_attack() -> Vec<Seq> {
        vec![
            seq(&[("Attack", "Raise"), ("Attack", "Hit")]),
            seq(&[("Attack", "Hit")]),
        ]
    }
}

/// Marks a step whose action names a clip of a locally generated clip pack instead of a Gothic
/// motion grammar action: `pack:ER_{S}_Light1` (the phase is unused). `{S}` stands for the style
/// of the drawn weapon (see [`pack_name`]).
pub const PACK_PREFIX: &str = "pack:";

/// Pack clip name of a pack step for the given weapon style (`Sword1H`, `Great2H`, ...), or
/// `None` for an ordinary Gothic step.
pub fn pack_name(action: &str, style: &str) -> Option<String> {
    action
        .strip_prefix(PACK_PREFIX)
        .map(|n| n.replace("{S}", style))
}

/// Actions that can be played from a clip pack, with the clip name pattern (without the pack's
/// own prefix) they use; `{S}` is the weapon style.
pub fn pack_clip(id: ActionId) -> Option<String> {
    let dir = |d: Dir| match d {
        Dir::Front => "Front",
        Dir::Back => "Back",
        Dir::Left => "Left",
        Dir::Right => "Right",
    };
    Some(match id {
        ActionId::Roll(d) => format!("Roll_{}", dir(d)),
        ActionId::Backstep => "Backstep".into(),
        ActionId::Light(1) => "{S}_Light1".into(),
        ActionId::Light(2) => "{S}_Light2".into(),
        ActionId::Light(_) => "{S}_Light3".into(),
        ActionId::Heavy => "{S}_Heavy1".into(),
        ActionId::RunLight => "{S}_RunLight".into(),
        ActionId::RollLight => "{S}_RollAttack".into(),
        ActionId::GuardCounter => "{S}_GuardCounter".into(),
        ActionId::Riposte => return None,
        ActionId::GuardHit => "GuardHit".into(),
        ActionId::GuardBreak => "GuardBreak".into(),
        ActionId::Stagger => "HurtLarge_Front".into(),
        ActionId::Hurt(H::Small) => "HurtSmall_Front".into(),
        ActionId::Hurt(H::Middle) => "HurtMiddle_Front".into(),
        ActionId::Hurt(H::Large) => "HurtLarge_Front".into(),
        ActionId::Hurt(H::Knockdown) => "HurtKnock_Front".into(),
    })
}

/// Every action [`pack_clip`] covers.
pub fn pack_actions() -> Vec<ActionId> {
    let mut v = vec![
        ActionId::Backstep,
        ActionId::Light(1),
        ActionId::Light(2),
        ActionId::Light(3),
        ActionId::Heavy,
        ActionId::RunLight,
        ActionId::RollLight,
        ActionId::GuardCounter,
        ActionId::GuardHit,
        ActionId::GuardBreak,
        ActionId::Stagger,
        ActionId::Hurt(H::Small),
        ActionId::Hurt(H::Middle),
        ActionId::Hurt(H::Large),
        ActionId::Hurt(H::Knockdown),
    ];
    v.extend([Dir::Front, Dir::Back, Dir::Left, Dir::Right].map(ActionId::Roll));
    v
}

/// What the hero's body should be doing when a clip pack drives the animation.
#[derive(Clone, Debug, PartialEq)]
pub enum PackPose {
    /// The running action's clip, at its action frame (30 fps).
    Action { id: ActionId, frame: f32 },
    /// Guard raised: the looping guard pose.
    Guard,
    /// Lock-on strafing: looping `Walk` / `Run` clip of a direction, played at `rate`.
    Strafe {
        gait: &'static str,
        dir: &'static str,
        rate: f32,
    },
    /// Nothing from the pack; the regular locomotion shows.
    None,
}

/// Ground speed above which a strafe clip plays (cm/s), and the speed the walk / run clips
/// represent at rate 1 (the pack's retargeted ER speeds, scaled to Gothic units).
pub const STRAFE_MIN: f32 = 25.0;
pub const RUN_ABOVE: f32 = 260.0;
pub const WALK_CLIP_SPEED: f32 = 160.0;
pub const RUN_CLIP_SPEED: f32 = 420.0;

/// Chooses the pack pose for the hero: the action in progress wins over a raised guard, which
/// wins over lock-on strafing. `velocity` is the hero's world velocity in cm/s.
pub fn pack_pose(
    hero: &crate::hero::Hero,
    tuning: &crate::tuning::Tuning,
    locked_on: bool,
    velocity: glam::Vec3,
) -> PackPose {
    if let Some(id) = hero.action() {
        return PackPose::Action {
            id,
            frame: hero.action_frame(),
        };
    }
    if hero.guard_up(tuning) {
        return PackPose::Guard;
    }
    if locked_on {
        let flat = glam::Vec3::new(velocity.x, 0.0, velocity.z);
        let speed = flat.length();
        if speed > STRAFE_MIN {
            let fwd = flat.dot(crate::math::dir_of(hero.yaw));
            let side = flat.dot(crate::math::right_of(hero.yaw));
            let dir = if fwd.abs() >= side.abs() {
                if fwd >= 0.0 { "Front" } else { "Back" }
            } else if side >= 0.0 {
                "Right"
            } else {
                "Left"
            };
            let (gait, nominal) = if speed > RUN_ABOVE {
                ("Run", RUN_CLIP_SPEED)
            } else {
                ("Walk", WALK_CLIP_SPEED)
            };
            return PackPose::Strafe {
                gait,
                dir,
                rate: (speed / nominal).clamp(0.5, 1.8),
            };
        }
    }
    PackPose::None
}

/// First candidate sequence all of whose steps exist, per the host's `exists(action, phase)`.
pub fn first_playable<'a>(
    cands: &'a [Seq],
    exists: &dyn Fn(&str, &str) -> bool,
) -> Option<&'a Seq> {
    cands
        .iter()
        .find(|s| !s.is_empty() && s.iter().all(|(a, p)| exists(a, p)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_has_a_candidate_and_swapping_works() {
        let mut m = AnimMap::gothic();
        for id in [
            ActionId::Roll(Dir::Front),
            ActionId::Roll(Dir::Left),
            ActionId::Backstep,
            ActionId::Light(1),
            ActionId::Light(3),
            ActionId::Heavy,
            ActionId::Riposte,
            ActionId::GuardHit,
            ActionId::Stagger,
            ActionId::Hurt(H::Large),
        ] {
            assert!(!m.get(id).is_empty(), "{id:?}");
        }
        m.set(ActionId::Heavy, vec![seq(&[("Custom", "Hit")])]);
        assert_eq!(m.get(ActionId::Heavy)[0][0].0, "Custom");
    }

    #[test]
    fn pack_steps_resolve_per_style_and_only_when_present() {
        let mut m = AnimMap::gothic();
        let none = m.clone().use_pack("ER_", &|_| false);
        assert!(none.is_empty(), "no pack, nothing changes");
        let taken = m.use_pack("ER_", &|n| n == "ER_Roll_Front" || n == "ER_Sword1H_Light1");
        assert!(taken.contains(&ActionId::Roll(Dir::Front)));
        assert!(taken.contains(&ActionId::Light(1)));
        assert!(!taken.contains(&ActionId::Roll(Dir::Left)));
        let first = &m.get(ActionId::Light(1))[0][0].0;
        assert_eq!(
            pack_name(first, "Sword1H").as_deref(),
            Some("ER_Sword1H_Light1")
        );
        assert_eq!(pack_name("Attack", "Sword1H"), None);
        // The Gothic candidates stay behind it.
        assert!(m.get(ActionId::Light(1)).len() > 1);
    }

    #[test]
    fn pack_pose_prefers_action_then_guard_then_strafe() {
        use crate::{hero::Hero, tuning::Tuning};
        use glam::Vec3;
        let t = Tuning::default();
        let h = Hero::new(0, Vec3::ZERO, 100.0, &t);
        // Facing -Z (yaw 0): moving along -Z is forward, along +X is right.
        assert_eq!(
            pack_pose(&h, &t, true, Vec3::new(0.0, 0.0, -150.0)),
            PackPose::Strafe {
                gait: "Walk",
                dir: "Front",
                rate: 150.0 / WALK_CLIP_SPEED
            }
        );
        match pack_pose(&h, &t, true, Vec3::new(300.0, 0.0, 10.0)) {
            PackPose::Strafe { gait, dir, .. } => assert_eq!((gait, dir), ("Run", "Right")),
            p => panic!("{p:?}"),
        }
        assert_eq!(
            pack_pose(&h, &t, false, Vec3::new(0.0, 0.0, -150.0)),
            PackPose::None
        );
        assert_eq!(pack_pose(&h, &t, true, Vec3::ZERO), PackPose::None);
    }

    #[test]
    fn rate_fits_clip_to_action() {
        let r = AnimMap::rate(ActionId::Light(1), 0.8);
        assert!((r - 1.0).abs() < 1e-3);
        assert!(AnimMap::rate(ActionId::Light(1), 10.0) <= 2.5);
    }
}
