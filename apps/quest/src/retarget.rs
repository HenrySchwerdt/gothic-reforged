//! Retargets the generic humanoid `BodyPose` (joint positions from the VR body IK or the NPC ragdoll) onto a Gothic
//! actor skeleton (`Hero_*` / `Orc_*` naming): every deform bone's local +X axis is aimed at its child joint (swing
//! from the bind pose, roll kept), head and hands can follow tracked orientations.
use std::f32::consts::PI;

use bevy::prelude::*;
use gothic_vr::body::{BodyPose, Joint};

use crate::actor::ActorRig;

#[derive(Clone, Copy)]
enum Drive {
    Aim(Joint, Joint),
    Head,
    Hand(usize),
}

pub struct Retarget {
    drive: Vec<Option<Drive>>,
    /// Rest direction (model space, unit) of every aimed bone: from the bone to its child bone in the bind pose.
    rest_dir: Vec<Vec3>,
    order: Vec<usize>,
    rest_rot: Vec<Quat>,
    /// Node whose bind position sits at the IK pelvis.
    pelvis_bind_cm: Vec3,
    pub slot: std::collections::HashMap<&'static str, usize>,
    /// Per hand: the bind-pose anatomy (fingers F, thumb side T, palm normal N; see `hand_anatomy`) expressed in the
    /// hand bone's own rest frame, as matrix columns. `None` when the skeleton has no finger nodes.
    hand_local: [Option<Mat3>; 2],
    /// Yaw that turns the bind-pose facing (derived from the data, like `Rig::facing_in_pose`) to world -Z.
    fix: Quat,
    /// Limb bones whose child bone translation is rescaled to the IK segment length.
    limb_child: Vec<Option<usize>>,
    /// Keep the rig's own bind bone lengths (orcs): the IK/ragdoll skeleton has hero proportions, stretching the orc
    /// limbs to it squashed them ~10 % (orc bind height 196 cm vs hero 178 cm). Bones only aim, never stretch.
    pub keep_bind_lengths: bool,
    /// Bind-pose distance (cm) from this bone to its limb child.
    limb_len_cm: Vec<f32>,
    /// Nodes between a limb bone and its child (child first, the bone itself excluded): their translations carry the
    /// bone-to-child offset (helper nodes like `*_ROOT` sit in between) and are stretched together.
    limb_path: Vec<Vec<usize>>,
    head_node: Option<usize>,
    /// Palm centre (55 % from the wrist to the knuckle centroid) in the hand bone's own rest frame, cm.
    palm_local: [Option<Vec3>; 2],
    /// `Foot_Foot_1` nodes and their bind height (cm), for standing the avatar on the ground.
    foot_nodes: Vec<usize>,
    ankle_bind_cm: f32,
    /// Finger bones: (hand, flexion axis in the bone's own rest frame, max flexion in radians, 0 = thumb/1 = index/2 = other).
    fingers: Vec<Option<(usize, Vec3, f32, u8)>>,
}

/// Bind-pose facing of a skeleton in mirrored model space, same rule as `crates/animation` `Rig::facing_in_pose`:
/// pelvis-side spine to head for creatures (mostly horizontal), otherwise left foot to left toe.
fn bind_facing(rig: &ActorRig) -> Vec3 {
    let find = |suffix: &str| {
        rig.names
            .iter()
            .position(|n| n.to_lowercase().ends_with(suffix))
    };
    let at = |i: usize| rig.bind_global[i].w_axis.truncate();
    let horizontal = |d: Vec3| {
        let h = Vec3::new(d.x, 0.0, d.z);
        (h.length() > 1e-3).then(|| h.normalize())
    };
    if let (Some(base), Some(head)) = (find("spine_spine_1"), find("head_head_1")) {
        let d = at(head) - at(base);
        let h = Vec3::new(d.x, 0.0, d.z);
        if h.length() > 0.35 * d.length() {
            if let Some(f) = horizontal(d) {
                return f;
            }
        }
    }
    if let (Some(foot), Some(toe)) = (find("left_foot_foot_1"), find("left_foot_toe_1")) {
        if let Some(f) = horizontal(at(toe) - at(foot)) {
            return f;
        }
    }
    Vec3::Z
}

/// Hand anatomy basis `[F, T, N]` (unit columns): F along the fingers (wrist to middle finger), T from the little
/// finger to the thumb, N the outward palm normal. `right` selects the handedness of the cross product: for a left
/// hand N = F x T, for a right hand N = T x F (checked on a flat right hand, palm down, fingers -Z, thumb -X: N = -Y).
fn hand_anatomy(wrist: Vec3, middle: Vec3, thumb: Vec3, pinky: Vec3, right: bool) -> Mat3 {
    let f = (middle - wrist).normalize();
    let t = (thumb - pinky).normalize();
    let t = (t - f * f.dot(t)).normalize();
    let n = if right { t.cross(f) } else { f.cross(t) }.normalize();
    Mat3::from_cols(f, t, n)
}

/// World hand anatomy from an OpenXR grip pose rotation. OpenXR 1.1 spec, "grip" standard pose: +X is the ray normal
/// to the open palm (away from the palm for the left hand, into the palm for the right hand), -Z runs through the
/// tube formed by the non-thumb fingers from little finger to thumb, +Y completes the right-handed frame. Hence
/// F = -Y (Y points to the wrist), T = -Z, N = +X (left) / -X (right).
fn grip_anatomy(grip: Quat, right: bool) -> Mat3 {
    let f = grip * Vec3::NEG_Y;
    let t = grip * Vec3::NEG_Z;
    let x = grip * Vec3::X;
    Mat3::from_cols(f, t, if right { -x } else { x })
}

impl Retarget {
    pub fn empty() -> Self {
        Self {
            drive: vec![],
            rest_dir: vec![],
            order: vec![],
            rest_rot: vec![],
            pelvis_bind_cm: Vec3::ZERO,
            slot: Default::default(),
            hand_local: [None; 2],
            fix: Quat::from_rotation_y(PI),
            limb_child: vec![],
            keep_bind_lengths: false,
            limb_len_cm: vec![],
            limb_path: vec![],
            head_node: None,
            palm_local: [None; 2],
            foot_nodes: vec![],
            ankle_bind_cm: 0.0,
            fingers: vec![],
        }
    }

    /// `prefix` is `Hero` or `Orc`; `toe` the toe suffix (`Toe_1` / `Toe1_1`).
    pub fn new(rig: &ActorRig, prefix: &str) -> Self {
        let n = rig.names.len();
        let mut drive: Vec<Option<Drive>> = vec![None; n];
        let mut set = |name: String, d: Drive| {
            if let Some(&i) = rig.by_name.get(&name) {
                drive[i] = Some(d);
            }
        };
        use Joint::*;
        set(format!("{prefix}_Spine_Spine_1"), Drive::Aim(Pelvis, Spine));
        set(format!("{prefix}_Spine_Spine_2"), Drive::Aim(Spine, Chest));
        for k in 3..=5 {
            set(format!("{prefix}_Spine_Spine_{k}"), Drive::Aim(Chest, Neck));
        }
        for k in 1..=3 {
            set(format!("{prefix}_Head_Neck_{k}"), Drive::Aim(Neck, Head));
        }
        set(format!("{prefix}_Head_Head_1"), Drive::Head);
        for (side, sh, el, ha, hip, kn, an, hidx) in [
            (
                "Left", ShoulderL, ElbowL, HandL, HipL, KneeL, AnkleL, 0usize,
            ),
            (
                "Right", ShoulderR, ElbowR, HandR, HipR, KneeR, AnkleR, 1usize,
            ),
        ] {
            set(
                format!("{prefix}_{side}_Arm_Shoulder_1"),
                Drive::Aim(Chest, sh),
            );
            set(format!("{prefix}_{side}_Arm_Arm_1"), Drive::Aim(sh, el));
            set(format!("{prefix}_{side}_Arm_Arm_2"), Drive::Aim(el, ha));
            set(format!("{prefix}_{side}_Hand_Hand_1"), Drive::Hand(hidx));
            set(
                format!("{prefix}_{side}_Leg_Hip_1"),
                Drive::Aim(Pelvis, hip),
            );
            set(format!("{prefix}_{side}_Leg_Leg_1"), Drive::Aim(hip, kn));
            set(format!("{prefix}_{side}_Leg_Leg_2"), Drive::Aim(kn, an));
        }
        // Bone -> child bone (end of the segment), then the bind-pose direction between them. The mirrored skeleton's
        // local +X is not the bone axis (translations are mirrored), so the direction comes from the world positions.
        let mut tail: Vec<Option<usize>> = vec![None; n];
        let mut link = |from: String, to: String| {
            if let (Some(&f), Some(&t)) = (rig.by_name.get(&from), rig.by_name.get(&to)) {
                tail[f] = Some(t);
            }
        };
        for k in 1..=4 {
            link(
                format!("{prefix}_Spine_Spine_{k}"),
                format!("{prefix}_Spine_Spine_{}", k + 1),
            );
        }
        link(
            format!("{prefix}_Spine_Spine_5"),
            format!("{prefix}_Head_Neck_1"),
        );
        link(
            format!("{prefix}_Head_Neck_1"),
            format!("{prefix}_Head_Neck_2"),
        );
        link(
            format!("{prefix}_Head_Neck_2"),
            format!("{prefix}_Head_Neck_3"),
        );
        link(
            format!("{prefix}_Head_Neck_3"),
            format!("{prefix}_Head_Head_1"),
        );
        for side in ["Left", "Right"] {
            link(
                format!("{prefix}_{side}_Arm_Shoulder_1"),
                format!("{prefix}_{side}_Arm_Arm_1"),
            );
            link(
                format!("{prefix}_{side}_Arm_Arm_1"),
                format!("{prefix}_{side}_Arm_Arm_2"),
            );
            link(
                format!("{prefix}_{side}_Arm_Arm_2"),
                format!("{prefix}_{side}_Hand_Hand_1"),
            );
            link(
                format!("{prefix}_{side}_Leg_Hip_1"),
                format!("{prefix}_{side}_Leg_Leg_1"),
            );
            link(
                format!("{prefix}_{side}_Leg_Leg_1"),
                format!("{prefix}_{side}_Leg_Leg_2"),
            );
            link(
                format!("{prefix}_{side}_Leg_Leg_2"),
                format!("{prefix}_{side}_Foot_Foot_1"),
            );
        }
        let mut rest_dir = vec![Vec3::X; n];
        for i in 0..n {
            if let Some(Drive::Aim(..)) = drive[i] {
                let d = tail[i].map(|t| {
                    (rig.bind_global[t].w_axis.truncate() - rig.bind_global[i].w_axis.truncate())
                });
                match d {
                    Some(d) if d.length() > 0.5 => rest_dir[i] = d.normalize(),
                    _ => drive[i] = None,
                }
            }
        }
        // Parent-first order.
        let depth = |mut i: usize| {
            let mut d = 0;
            while rig.parent[i] >= 0 && d < 256 {
                i = rig.parent[i] as usize;
                d += 1;
            }
            d
        };
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by_key(|&i| depth(i));
        let rest_rot: Vec<Quat> = rig
            .bind_global
            .iter()
            .map(|m| Quat::from_mat4(m).normalize())
            .collect();
        let pelvis_node = rig
            .by_name
            .get(&format!("{prefix}_Spine_Spine_1"))
            .copied()
            .or_else(|| {
                rig.by_name
                    .get(&format!("{prefix}_Spine_Spine_ROOT"))
                    .copied()
            })
            .unwrap_or(0);
        let pelvis_bind_cm = rig.bind_global[pelvis_node].w_axis.truncate();
        let slot_names: [&'static str; 6] = [
            "Slot_MeleeBackLeft",
            "Slot_MeleeBackRight",
            "Slot_Bow",
            "Slot_RightHand_Weapon",
            "Slot_LeftHand_Weapon",
            "Slot_AxeBack",
        ];
        let slot = slot_names
            .iter()
            .filter_map(|s| rig.by_name.get(*s).map(|&i| (*s, i)))
            .collect();
        let pos = |name: &str| {
            rig.by_name
                .get(name)
                .map(|&i| rig.bind_global[i].w_axis.truncate())
        };
        let mut hand_local = [None; 2];
        for (k, side) in ["Left", "Right"].into_iter().enumerate() {
            let hand = format!("{prefix}_{side}_Hand_Hand_1");
            if let (Some(w), Some(m), Some(t), Some(p), Some(&hi)) = (
                pos(&hand),
                pos(&format!("{prefix}_{side}_Hand_Finger3_2")),
                pos(&format!("{prefix}_{side}_Hand_Finger1_1")),
                pos(&format!("{prefix}_{side}_Hand_Finger5_1")),
                rig.by_name.get(&hand),
            ) {
                let a = hand_anatomy(w, m, t, p, k == 1);
                hand_local[k] = Some(Mat3::from_quat(rest_rot[hi].inverse()) * a);
            }
        }
        let fb = bind_facing(rig);
        let fix = Quat::from_rotation_arc(fb, Vec3::NEG_Z);
        let mut limb_child = vec![None; n];
        let mut limb_len_cm = vec![0.0; n];
        let mut limb_path: Vec<Vec<usize>> = vec![Vec::new(); n];
        for side in ["Left", "Right"] {
            for part in [
                "Arm_Shoulder_1",
                "Arm_Arm_1",
                "Arm_Arm_2",
                "Leg_Hip_1",
                "Leg_Leg_1",
                "Leg_Leg_2",
            ] {
                if let Some(&i) = rig.by_name.get(&format!("{prefix}_{side}_{part}")) {
                    if let (Some(Drive::Aim(..)), Some(c)) = (drive[i], tail[i]) {
                        limb_child[i] = Some(c);
                        let mut path = vec![c];
                        let mut q = rig.parent[c];
                        while q >= 0 && q as usize != i && path.len() < 16 {
                            path.push(q as usize);
                            q = rig.parent[q as usize];
                        }
                        limb_path[i] = path;
                        limb_len_cm[i] = (rig.bind_global[c].w_axis.truncate()
                            - rig.bind_global[i].w_axis.truncate())
                        .length();
                    }
                }
            }
        }
        // Finger flexion: every finger joint rotates about the hand's T axis (little finger -> thumb), the sense that
        // curls the fingers towards the palm: +T for a left hand, -T for a right hand (see `hand_anatomy`). The axis is
        // expressed in each bone's own rest frame. Max flexion per joint (degrees): fingers 70/80/50, thumb 25/30/20.
        let mut fingers: Vec<Option<(usize, Vec3, f32, u8)>> = vec![None; n];
        for (k, side) in ["Left", "Right"].into_iter().enumerate() {
            let hand = format!("{prefix}_{side}_Hand_Hand_1");
            let (Some(&hi), Some(al)) = (rig.by_name.get(&hand), hand_local[k]) else {
                continue;
            };
            let t_model = rest_rot[hi] * al.col(1);
            let axis_model = if k == 0 { t_model } else { -t_model };
            for fnum in 1..=5 {
                for (j, deg) in [(1, 70.0f32), (2, 80.0), (3, 50.0)] {
                    if let Some(&fi) = rig
                        .by_name
                        .get(&format!("{prefix}_{side}_Hand_Finger{fnum}_{j}"))
                    {
                        let (deg, kind) = if fnum == 1 {
                            (deg * 0.4, 0u8)
                        } else if fnum == 2 {
                            (deg, 1u8)
                        } else {
                            (deg, 2u8)
                        };
                        fingers[fi] = Some((
                            k,
                            (rest_rot[fi].inverse() * axis_model).normalize(),
                            deg.to_radians(),
                            kind,
                        ));
                    }
                }
            }
        }
        let head_node = rig.by_name.get(&format!("{prefix}_Head_Head_1")).copied();
        let mut palm_local = [None; 2];
        for (k, side) in ["Left", "Right"].into_iter().enumerate() {
            let hand = format!("{prefix}_{side}_Hand_Hand_1");
            let roots: Vec<Vec3> = (2..=5)
                .filter_map(|f| pos(&format!("{prefix}_{side}_Hand_Finger{f}_1")))
                .collect();
            if let (Some(w), Some(&hi), false) =
                (pos(&hand), rig.by_name.get(&hand), roots.is_empty())
            {
                let kc = roots.iter().copied().sum::<Vec3>() / roots.len() as f32;
                palm_local[k] = Some(rest_rot[hi].inverse() * ((kc - w) * 0.55));
            }
        }
        let foot_nodes: Vec<usize> = ["Left", "Right"]
            .iter()
            .filter_map(|s| {
                rig.by_name
                    .get(&format!("{prefix}_{s}_Foot_Foot_1"))
                    .copied()
            })
            .collect();
        let ankle_bind_cm = foot_nodes
            .first()
            .map_or(0.0, |&f| rig.bind_global[f].w_axis.y);
        Self {
            drive,
            rest_dir,
            order,
            rest_rot,
            pelvis_bind_cm,
            slot,
            hand_local,
            fix,
            limb_child,
            keep_bind_lengths: false,
            limb_len_cm,
            limb_path,
            head_node,
            palm_local,
            foot_nodes,
            ankle_bind_cm,
            fingers,
        }
    }

    /// Where the hand bone (wrist) must be so that the visible palm centre sits at the controller's grip position
    /// (`grip_pos`, OpenXR grip = palm centroid): the hand bone's world rotation follows from the grip frame, the palm
    /// offset in the bone frame is rotated with it.
    pub fn wrist_for_grip(&self, k: usize, grip_pos: Vec3, grip: Quat, scale: f32) -> Option<Vec3> {
        let (al, pl) = (self.hand_local[k]?, self.palm_local[k]?);
        let rb = Quat::from_mat3(&(grip_anatomy(grip, k == 1) * al.inverse())).normalize();
        Some(grip_pos - rb * (pl * 0.01 * scale))
    }

    /// Bind-pose distance from the pelvis node to the head bone, metres (None without a head bone).
    pub fn torso_bind_m(&self, rig: &ActorRig) -> Option<f32> {
        let h = self.head_node?;
        Some((rig.bind_global[h].w_axis.truncate() - self.pelvis_bind_cm).length() * 0.01)
    }

    /// Debug dump: for every aimed bone the world rest axis (+X) and the IK direction it is aimed at.
    pub fn debug(&self, rig: &ActorRig, pose: &BodyPose) -> Vec<String> {
        let r_root = Quat::from_rotation_y(pose.body_yaw) * self.fix;
        let mut out = Vec::new();
        for &i in &self.order {
            if let Some(Drive::Aim(a, b)) = self.drive[i] {
                let axis = r_root * self.rest_dir[i];
                let dir = (pose.get(b) - pose.get(a)).normalize_or_zero();
                out.push(format!(
                    "{:28} rest_axis {:+.2} {:+.2} {:+.2}  ik_dir {:+.2} {:+.2} {:+.2}  bind_pos_cm {:?}",
                    rig.names[i], axis.x, axis.y, axis.z, dir.x, dir.y, dir.z, rig.bind_global[i].w_axis.truncate().round()
                ));
            }
        }
        out
    }

    /// Poses the skeleton. `scale` multiplies the avatar (1 = hero proportions). `head_anchor`: world position the
    /// head bone is moved to (rigid shift of the whole avatar) so the player's eyes sit inside the head. Limb bones
    /// are stretched to the IK segment lengths so hands and feet land on their targets.
    #[allow(clippy::too_many_arguments)]
    pub fn apply<F: bevy::ecs::query::QueryFilter>(
        &self,
        rig: &ActorRig,
        q: &mut Query<&mut Transform, F>,
        pose: &BodyPose,
        head_rot: Option<Quat>,
        hand_rot: [Option<Quat>; 2],
        scale: f32,
        head_anchor: Option<Vec3>,
        curl: [[f32; 2]; 2],
        ov: Option<&crate::anim::PoseOverride>,
        ground: Option<f32>,
    ) {
        let n = rig.names.len();
        let r_root = Quat::from_rotation_y(pose.body_yaw) * self.fix;
        let mut rw: Vec<Quat> = vec![Quat::IDENTITY; n];
        let mut wp: Vec<Vec3> = vec![Vec3::ZERO; n];
        let pelvis = pose.get(Joint::Pelvis);
        let wscale = 0.01 * scale;
        let mut root_t = pelvis - r_root * (self.pelvis_bind_cm * wscale);
        // Limb stretching: the child bone's translation is scaled so the chain reaches the IK joint exactly.
        let mut tscale = vec![1.0f32; n];
        for &i in &self.order {
            let p = rig.parent[i];
            let parent_rw = if p >= 0 { rw[p as usize] } else { r_root };
            let base_rest = r_root * self.rest_rot[i];
            let local_t = rig.bind_local[i].translation * tscale[i];
            let wp_i = if p >= 0 {
                wp[p as usize] + parent_rw * (local_t * wscale)
            } else {
                root_t + r_root * (local_t * wscale)
            };
            wp[i] = wp_i;
            let aimed = match self.drive[i] {
                Some(Drive::Aim(a, b)) => {
                    // Limb bones aim from their actual (FK) position at the IK joint; spine bones use the IK segment.
                    let (dir, len) = if self.limb_child[i].is_some() {
                        let d = pose.get(b) - wp_i;
                        (d.normalize_or_zero(), d.length())
                    } else {
                        ((pose.get(b) - pose.get(a)).normalize_or_zero(), 0.0)
                    };
                    if dir == Vec3::ZERO {
                        None
                    } else {
                        if let Some(c) = self.limb_child[i] {
                            let bind = self.limb_len_cm[i] * wscale;
                            if bind > 1e-4 && !self.keep_bind_lengths {
                                let r = (len / bind).clamp(0.4, 2.5);
                                tscale[c] = r;
                                for &q in &self.limb_path[i] {
                                    tscale[q] = r;
                                }
                            }
                        }
                        let axis = r_root * self.rest_dir[i];
                        Some(Quat::from_rotation_arc(axis, dir) * base_rest)
                    }
                }
                Some(Drive::Head) => head_rot.map(|h| h * (self.fix * self.rest_rot[i])),
                Some(Drive::Hand(k)) => match (hand_rot[k], self.hand_local[k]) {
                    // Rb * A_local = A_world  =>  Rb = A_world * A_local^-1 (both bases share their handedness).
                    (Some(g), Some(al)) => {
                        Some(Quat::from_mat3(&(grip_anatomy(g, k == 1) * al.inverse())).normalize())
                    }
                    _ => None,
                },
                None => None,
            };
            // Original-clip override (leg gait, hit reactions): blended over whatever the IK / ragdoll produced.
            let aimed = match ov.and_then(|o| o.rot[i].map(|r| (r, o.weight[i]))) {
                Some((r, wt)) if wt > 0.0 => {
                    let base = aimed.unwrap_or(parent_rw * rig.bind_local[i].rotation);
                    Some(base.slerp(r, wt.min(1.0)))
                }
                _ => aimed,
            };
            match aimed {
                Some(w) => {
                    rw[i] = w;
                    if let Ok(mut t) = q.get_mut(rig.joints[i]) {
                        t.rotation = (parent_rw.inverse() * w).normalize();
                        t.translation = local_t;
                    }
                }
                None => {
                    // Finger joints bend with the controller's analog grip (thumb/middle/ring/little) and trigger (index).
                    let mut local = rig.bind_local[i].rotation;
                    if let Some(Some((hand, axis, max, kind))) = self.fingers.get(i) {
                        let c = match kind {
                            1 => curl[*hand][1],
                            0 => curl[*hand][0] * 0.7,
                            _ => curl[*hand][0],
                        };
                        local *= Quat::from_axis_angle(*axis, max * c.clamp(0.0, 1.0));
                    }
                    rw[i] = parent_rw * local;
                    if let Ok(mut t) = q.get_mut(rig.joints[i]) {
                        t.translation = local_t;
                        t.rotation = local;
                    }
                }
            }
        }
        // Stand on the ground: put the lowest ankle at ground height + the bind ankle height (clip-driven legs are as long
        // as the original skeleton, the IK pose is not).
        if let (Some(g), false) = (ground, self.foot_nodes.is_empty()) {
            let low = self
                .foot_nodes
                .iter()
                .map(|&f| wp[f].y)
                .fold(f32::MAX, f32::min);
            root_t.y += g + self.ankle_bind_cm * wscale - low;
        }
        if let (Some(target), Some(h)) = (head_anchor, self.head_node) {
            root_t += target - wp[h];
        }
        if let Ok(mut root) = q.get_mut(rig.root) {
            root.rotation = r_root;
            root.scale = Vec3::splat(wscale);
            root.translation = root_t;
        }
    }
}
