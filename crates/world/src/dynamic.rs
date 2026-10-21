//! Dynamic entities (`eCDynamicEntity`) and the dummy visual (`gCEntityDummyVisual_PS`).
//!
//! A dynamic entity is the only kind that is processed each frame. What is specific to it:
//! it *decays* (its property sets wind down, and the entity is killed once all of them are
//! done), it can opt into auto-kill, and it forwards the trigger events to its property sets.
//! The dummy visual keeps a stand-in visual at the entity's world matrix without scale.
use crate::entity::EntityTree;
use crate::pvs::EntityId;
use bevy::math::{Affine3A, Mat3A, Vec3A};

/// `eCEntityPropertySet::GetDecayState`: 0 and 1 are still running, 2 and up are finished.
pub type DecayState = u32;

/// First decay state that counts as finished.
pub const DECAY_DONE: DecayState = 2;

/// What [`decay`] / [`process_decay`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecayOutcome {
    /// Every property set finished: the entity was killed.
    Killed,
    /// Some set is still running; the children were marked temporary so a save does not keep
    /// a half-decayed subtree.
    Pending,
}

/// Port of `eCDynamicEntity::Decay` (Engine:0x1022a1b0): `states` are the decay states of the
/// entity's property sets after asking each to decay. An entity without property sets is done
/// at once.
pub fn decay(tree: &mut EntityTree, id: EntityId, states: &[DecayState]) -> DecayOutcome {
    if states.iter().all(|s| *s >= DECAY_DONE) {
        tree.kill(id);
        return DecayOutcome::Killed;
    }
    let kids: Vec<EntityId> = tree
        .get(id)
        .map(|e| e.children().to_vec())
        .unwrap_or_default();
    for k in kids {
        tree.set_temporary(k);
    }
    DecayOutcome::Pending
}

/// Port of the end of `eCDynamicEntity::OnProcess` (Engine:0x1022a980): when every property set
/// reports a finished decay state the entity kills itself. Entities without sets never do.
pub fn process_decay(tree: &mut EntityTree, id: EntityId, states: &[DecayState]) -> bool {
    if !states.is_empty() && states.iter().all(|s| *s >= DECAY_DONE) {
        tree.kill(id);
        true
    } else {
        false
    }
}

/// Port of `eCDynamicEntity::IsAutoKillEnabled` (Engine:0x1022a140): any set asks for it.
pub fn is_auto_kill_enabled(set_requests: &[bool]) -> bool {
    set_requests.iter().any(|r| *r)
}

/// Port of `gCEntityDummyVisual_PS::OnUpdatedWorldMatrix` (Game:0x100425b0).
#[derive(Debug, Default)]
pub struct DummyVisual {
    /// Re-entrancy guard (`+0x38`): moving the visual must not feed back.
    updating: bool,
}

impl DummyVisual {
    /// Forward the entity's world matrix, scale removed, to `apply` (the visual's
    /// `SetWorldMatrix`). Skipped in editor mode (`editor`) and while an update is running.
    pub fn on_updated_world_matrix(
        &mut self,
        world: Affine3A,
        editor: bool,
        mut apply: impl FnMut(Affine3A),
    ) {
        if self.updating {
            return;
        }
        self.updating = true;
        if !editor {
            apply(without_scale(world));
        }
        self.updating = false;
    }
}

/// The matrix with unit-length basis vectors (`bCMatrix::SetToScaling(1)`).
fn without_scale(m: Affine3A) -> Affine3A {
    let n = |v: Vec3A| v.try_normalize().unwrap_or(v);
    Affine3A {
        matrix3: Mat3A::from_cols(
            n(m.matrix3.x_axis),
            n(m.matrix3.y_axis),
            n(m.matrix3.z_axis),
        ),
        translation: m.translation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec3;

    #[test]
    fn decay_kills_when_all_sets_are_done_and_marks_children_otherwise() {
        let mut tree = EntityTree::new();
        let e = tree.create("Corpse", true);
        let c = tree.create("Item", false);
        tree.link(c, Some(e)).unwrap();
        assert_eq!(decay(&mut tree, e, &[2, 1]), DecayOutcome::Pending);
        assert!(tree.get(c).unwrap().has(crate::entity::flags::TEMPORARY));
        assert!(!tree.get(e).unwrap().is_killed());
        assert_eq!(decay(&mut tree, e, &[2, 3]), DecayOutcome::Killed);
        assert!(tree.get(e).unwrap().is_killed());
        let f = tree.create("Empty", true);
        assert!(!process_decay(&mut tree, f, &[]));
        assert!(process_decay(&mut tree, f, &[5]));
        assert!(is_auto_kill_enabled(&[false, true]));
    }

    #[test]
    fn dummy_visual_gets_an_unscaled_matrix_once_per_update() {
        let mut v = DummyVisual::default();
        let m = Affine3A::from_scale_rotation_translation(
            Vec3::splat(3.0),
            bevy::math::Quat::from_rotation_y(0.5),
            Vec3::new(1.0, 2.0, 3.0),
        );
        let mut got = None;
        v.on_updated_world_matrix(m, false, |x| got = Some(x));
        let g = got.unwrap();
        assert!((g.matrix3.x_axis.length() - 1.0).abs() < 1e-5);
        assert_eq!(g.translation, Vec3A::new(1.0, 2.0, 3.0));
        let mut called = false;
        v.on_updated_world_matrix(m, true, |_| called = true);
        assert!(!called);
        // Re-entrant update from inside the callback is ignored.
        let mut inner = 0;
        let v2 = std::cell::RefCell::new(DummyVisual::default());
        v2.borrow_mut().updating = true;
        v2.borrow_mut()
            .on_updated_world_matrix(m, false, |_| inner += 1);
        assert_eq!(inner, 0);
    }
}
