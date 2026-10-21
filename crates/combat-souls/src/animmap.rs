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
    fn rate_fits_clip_to_action() {
        let r = AnimMap::rate(ActionId::Light(1), 0.8);
        assert!((r - 1.0).abs() < 1e-3);
        assert!(AnimMap::rate(ActionId::Light(1), 10.0) <= 2.5);
    }
}
