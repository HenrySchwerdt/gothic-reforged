//! Skeleton data derived from an `.xact` actor, plus the folded animation hierarchy.
//!
//! Motions are expressed relative to the nearest *animated* ancestor, not the actual parent
//! (see `docs/research/ACTOR_ANIMATION_FORMAT.md`). The rig therefore classifies nodes into
//!
//! * animated nodes (the deforming bones: everything that is not a helper), driven by motions
//!   and evaluated relative to their nearest animated ancestor (`eff_parent`), and
//! * helper nodes (`*_ROOT`, `*_END`, `Slot_*`, `Player`), which keep their bind-pose local
//!   transform relative to their real parent.
//!
//! Model matrix of an animated node: `model(eff_parent) * pose`; of a helper:
//! `model(parent) * bind_local`. A single pass over a parents-first order evaluates both.
use crate::pose::{Trs, quat_from_file};
use bevy::math::{Affine3A, Mat4, Vec3};
use gothic_formats::xact::Actor;
use std::collections::HashMap;

pub const NONE: u16 = u16::MAX;

pub struct Rig {
    pub name: String,
    pub names: Vec<String>,
    /// Actual parent of every node.
    pub parent: Vec<u16>,
    pub bind_local: Vec<Trs>,
    pub bind_world: Vec<Mat4>,
    /// Lower-cased node name to index.
    pub index: HashMap<String, u16>,
    /// Pose slot of each node (`NONE` for helpers).
    pub slot_of: Vec<u16>,
    /// Node index of each pose slot.
    pub slot_node: Vec<u16>,
    /// Parent used for evaluation (nearest animated ancestor for animated nodes, the actual
    /// parent for helpers).
    pub eff_parent: Vec<u16>,
    /// Rest pose per slot, relative to the animated ancestor (equals the bind pose).
    pub rest: Vec<Trs>,
    /// Parents-before-children order over all nodes.
    pub topo: Vec<u16>,
    /// Skeleton family prefix of the motion names (`Hero`, `Orc`, ...).
    pub family: String,
}

pub fn is_helper(name: &str) -> bool {
    name.ends_with("_ROOT")
        || name.ends_with("_END")
        || name.starts_with("Slot_")
        || name == "Player"
        || name.starts_with('!')
}

impl Rig {
    pub fn from_actor(name: &str, actor: &Actor) -> Self {
        let names: Vec<String> = actor.nodes.iter().map(|x| x.name.clone()).collect();
        let parent: Vec<u16> = actor
            .nodes
            .iter()
            .map(|x| x.parent.map_or(NONE, |p| p as u16))
            .collect();
        let bind_local: Vec<Trs> = actor
            .nodes
            .iter()
            .map(|x| Trs {
                t: Vec3::from(x.position),
                r: quat_from_file(x.rotation),
                s: Vec3::from(x.scale),
            })
            .collect();
        Self::from_parts(name, names, parent, bind_local)
    }

    pub fn from_parts(
        name: &str,
        names: Vec<String>,
        parent: Vec<u16>,
        bind_local: Vec<Trs>,
    ) -> Self {
        let n = names.len();
        // Parents-first order by actual depth (file order does not guarantee it).
        let mut depth = vec![0u32; n];
        for (i, d) in depth.iter_mut().enumerate() {
            let mut count = 0;
            let mut p = parent[i];
            while p != NONE && count <= n as u32 {
                count += 1;
                p = parent[p as usize];
            }
            *d = count;
        }
        let mut topo: Vec<u16> = (0..n as u16).collect();
        topo.sort_by_key(|&i| (depth[i as usize], i));
        let mut bind_world = vec![Mat4::IDENTITY; n];
        for &i in &topo {
            let local = Mat4::from(bind_local[i as usize].affine());
            bind_world[i as usize] = if parent[i as usize] == NONE {
                local
            } else {
                bind_world[parent[i as usize] as usize] * local
            };
        }
        let mut slot_of = vec![NONE; n];
        let mut slot_node = Vec::new();
        for (i, node_name) in names.iter().enumerate() {
            if !is_helper(node_name) {
                slot_of[i] = slot_node.len() as u16;
                slot_node.push(i as u16);
            }
        }
        let mut eff_parent = vec![NONE; n];
        for i in 0..n {
            if slot_of[i] == NONE {
                eff_parent[i] = parent[i];
            } else {
                let mut p = parent[i];
                while p != NONE && slot_of[p as usize] == NONE {
                    p = parent[p as usize];
                }
                eff_parent[i] = p;
            }
        }
        let rest: Vec<Trs> = slot_node
            .iter()
            .map(|&node| {
                let node = node as usize;
                let world = bind_world[node];
                let rel = match eff_parent[node] {
                    NONE => world,
                    p => bind_world[p as usize].inverse() * world,
                };
                Trs::from_affine(Affine3A::from_mat4(rel))
            })
            .collect();
        let index = names
            .iter()
            .enumerate()
            .rev()
            .map(|(i, n)| (n.to_lowercase(), i as u16))
            .collect();
        let family = names
            .iter()
            .zip(&slot_of)
            .find(|(_, s)| **s != NONE)
            .and_then(|(n, _)| n.split('_').next())
            .unwrap_or("")
            .to_string();
        Rig {
            name: name.to_string(),
            names,
            parent,
            bind_local,
            bind_world,
            index,
            slot_of,
            slot_node,
            eff_parent,
            rest,
            topo,
            family,
        }
    }

    pub fn node(&self, name: &str) -> Option<u16> {
        self.index.get(&name.to_lowercase()).copied()
    }

    /// Model-space horizontal direction the *animated* skeleton faces, measured from the left
    /// foot joint to its toe joint in the pose held in `model` (planned nodes only). `None`
    /// when the rig has no such joints. Motions are authored facing -X in this data set, so
    /// the animator rotates the result to the entity's forward axis.
    pub fn facing_in_pose(&self, model: &[Affine3A]) -> Option<Vec3> {
        let find = |suffix: &str| {
            self.names
                .iter()
                .position(|n| n.to_lowercase().ends_with(suffix))
        };
        let (foot, toe) = (find("left_foot_foot_1")?, find("left_foot_toe_1")?);
        let d = Vec3::from(model[toe].translation) - Vec3::from(model[foot].translation);
        let h = Vec3::new(d.x, 0.0, d.z);
        (h.length() > 1e-3).then(|| h.normalize())
    }

    /// Per-slot weights for an upper-body layer: 1 for everything below `Spine_2` (including
    /// arms, head and fingers), 0 for pelvis, legs and the lowest spine joint.
    pub fn upper_body_mask(&self) -> Vec<f32> {
        let Some(root) = self
            .names
            .iter()
            .position(|n| n.to_lowercase().ends_with("spine_spine_2"))
        else {
            return vec![0.0; self.slot_node.len()];
        };
        self.slot_node
            .iter()
            .map(|&node| {
                let mut p = node;
                while p != NONE {
                    if p as usize == root {
                        return 1.0;
                    }
                    p = self.parent[p as usize];
                }
                0.0
            })
            .collect()
    }

    /// Evaluation plan for the given nodes: those nodes plus all dependencies, parents first.
    pub fn plan(&self, wanted: &[u16]) -> Vec<EvalStep> {
        let mut needed = vec![false; self.names.len()];
        for &w in wanted {
            let mut n = w;
            while n != NONE && !needed[n as usize] {
                needed[n as usize] = true;
                n = self.eff_parent[n as usize];
            }
        }
        self.topo
            .iter()
            .filter(|&&n| needed[n as usize])
            .map(|&n| EvalStep {
                node: n,
                parent: self.eff_parent[n as usize],
                slot: self.slot_of[n as usize],
                fixed: self.bind_local[n as usize].affine(),
            })
            .collect()
    }
}

/// One step of the model-space evaluation.
#[derive(Clone, Copy, Debug)]
pub struct EvalStep {
    pub node: u16,
    pub parent: u16,
    /// Pose slot, or `NONE` when `fixed` (bind local) applies.
    pub slot: u16,
    pub fixed: Affine3A,
}

/// Computes model-space matrices for the plan from per-slot local poses.
#[inline]
pub fn evaluate(plan: &[EvalStep], pose: &[Trs], model: &mut [Affine3A]) {
    for step in plan {
        let local = if step.slot == NONE {
            step.fixed
        } else {
            pose[step.slot as usize].affine()
        };
        model[step.node as usize] = if step.parent == NONE {
            local
        } else {
            model[step.parent as usize] * local
        };
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use bevy::math::{Quat, Vec3A};

    /// Synthetic rig: Test_ROOT(helper) -> Hip_1 -> Hip_END(helper) -> Arm_1 -> Slot_Hand.
    pub fn synthetic() -> Rig {
        let names = [
            "Test_ROOT",
            "Test_Hip_1",
            "Test_Hip_END",
            "Test_Arm_1",
            "Slot_Hand",
        ];
        let parent = vec![NONE, 0, 1, 2, 3];
        let local = |t: Vec3| Trs {
            t,
            r: Quat::IDENTITY,
            s: Vec3::ONE,
        };
        let bind_local = vec![
            local(Vec3::new(0.0, 100.0, 0.0)),
            Trs::IDENTITY,
            local(Vec3::new(10.0, 0.0, 0.0)),
            Trs::IDENTITY,
            local(Vec3::new(0.0, 5.0, 0.0)),
        ];
        Rig::from_parts(
            "synthetic",
            names.iter().map(|s| s.to_string()).collect(),
            parent,
            bind_local,
        )
    }

    #[test]
    fn rest_pose_reproduces_bind_world() {
        let rig = synthetic();
        let plan = rig.plan(&[4]);
        let mut model = vec![Affine3A::IDENTITY; rig.names.len()];
        evaluate(&plan, &rig.rest, &mut model);
        for n in [1usize, 3, 4] {
            let want = rig.bind_world[n];
            let got = Mat4::from(model[n]);
            assert!(
                (want.w_axis - got.w_axis).length() < 1e-4,
                "node {n}: {want:?} vs {got:?}"
            );
        }
        // Arm_1 hangs off Hip_1 through the helper Hip_END.
        assert_eq!(rig.eff_parent[3], 1);
        assert_eq!(rig.slot_of.iter().filter(|&&s| s != NONE).count(), 2);
    }

    #[test]
    fn plan_contains_dependencies_in_order() {
        let rig = synthetic();
        let order: Vec<u16> = rig.plan(&[4]).iter().map(|s| s.node).collect();
        assert_eq!(order, vec![1, 3, 4]);
    }

    #[test]
    fn animated_child_follows_animated_parent_rotation() {
        let rig = synthetic();
        let plan = rig.plan(&[3]);
        let mut pose = rig.rest.clone();
        // Hip_1 (slot 0) rotated 90 degrees about Z: the arm's rest offset (10,0,0) in the
        // Hip frame (absorbing the helper) now points up.
        pose[0].r = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let mut model = vec![Affine3A::IDENTITY; rig.names.len()];
        evaluate(&plan, &pose, &mut model);
        let hip = model[1].translation;
        let arm = model[3].translation;
        assert!((hip - Vec3A::new(0.0, 100.0, 0.0)).length() < 1e-3);
        assert!(
            (arm - Vec3A::new(0.0, 110.0, 0.0)).length() < 1e-3,
            "{arm:?}"
        );
    }
}
