//! Generic skeleton-to-skeleton animation retargeting.
//!
//! Converts clips authored for an arbitrary humanoid *source* skeleton (given as plain local
//! transforms, see [`SrcSkeleton`] / [`SrcClip`]) into [`Clip`]s for a Gothic 3 [`Rig`]. The module
//! knows nothing about any particular source game; it only needs a bone mapping and a basis that
//! relates the two coordinate frames. Offline tools (kept out of the repository together with
//! their inputs) fill the source structures; the output is written with [`crate::clip_pack`].
//!
//! Method, per frame:
//!
//! 1. Forward kinematics of the source gives a world rotation for every mapped bone. The source
//!    frame is carried into the target frame by `basis` (a rotation, or a reflection when the two
//!    games disagree on handedness; rotations are conjugated, so a reflection does not mirror the
//!    character).
//! 2. The rotation *delta* of a bone against its source rest pose is applied on top of the target
//!    rest pose after a per-bone *rest alignment*: the shortest-arc rotation that makes the
//!    target bone point where the source bone points at rest. This absorbs the A-pose versus
//!    T-pose difference, so an arm hanging at the side in the source also hangs in the target.
//! 3. World rotations are turned into the motion convention of the data set (relative to the
//!    nearest *animated* ancestor, see `rig.rs`). Animated bones without a mapping follow their
//!    parent in their rest pose.
//! 4. The root bone gets the source pelvis offset scaled by `scale`; an optional ground fix
//!    shifts the whole body vertically so the lower foot keeps its source height above the floor.
use crate::{
    clip::{Clip, ClipNode, Track},
    pose::Trs,
    rig::{NONE, Rig},
};
use bevy::math::{Mat3, Quat, Vec3};
use std::collections::HashMap;

/// Local-transform skeleton of the source animation set (parents before children).
#[derive(Clone, Debug, Default)]
pub struct SrcSkeleton {
    pub names: Vec<String>,
    /// Parent index or -1.
    pub parent: Vec<i32>,
    pub rest_t: Vec<Vec3>,
    pub rest_q: Vec<Quat>,
}

/// A sampled source clip: uniform frame rate, local translation + rotation for the animated
/// tracks only (every other bone keeps its rest transform).
#[derive(Clone, Debug, Default)]
pub struct SrcClip {
    pub name: String,
    pub fps: f32,
    pub frames: usize,
    /// Source bone index of each track.
    pub tracks: Vec<u16>,
    /// `frames * tracks.len()` entries, frame-major.
    pub t: Vec<Vec3>,
    pub q: Vec<Quat>,
}

impl SrcSkeleton {
    pub fn index(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == name)
    }

    /// World (position, rotation) of every bone for the given local pose.
    pub fn world(&self, t: &[Vec3], q: &[Quat]) -> (Vec<Vec3>, Vec<Quat>) {
        let n = self.names.len();
        let mut wp = vec![Vec3::ZERO; n];
        let mut wq = vec![Quat::IDENTITY; n];
        for i in 0..n {
            match self.parent[i] {
                p if p < 0 => {
                    wp[i] = t[i];
                    wq[i] = q[i];
                }
                p => {
                    let p = p as usize;
                    wp[i] = wp[p] + wq[p] * t[i];
                    wq[i] = (wq[p] * q[i]).normalize();
                }
            }
        }
        (wp, wq)
    }
}

/// The interchange file the offline extraction tools write (`SANM`, little endian):
/// `"SANM" u32 version=1 u32 boneCount`, per bone `u8 nameLen name i32 parent f32 t[3] f32 q[4]`,
/// then `u32 clipCount` and per clip `u8 nameLen name f32 fps u32 frames u32 tracks u16 bone[tracks]`
/// followed by `frames * tracks` transforms `f32 t[3] f32 q[4]`.
pub struct SrcAnims {
    pub skeleton: SrcSkeleton,
    pub clips: Vec<SrcClip>,
}

impl SrcAnims {
    #[allow(clippy::chunks_exact_to_as_chunks)]
    pub fn from_bytes(d: &[u8]) -> Result<Self, String> {
        let mut at = 0usize;
        let mut take = |n: usize| -> Result<&[u8], String> {
            let end = at
                .checked_add(n)
                .filter(|&e| e <= d.len())
                .ok_or("SANM truncated")?;
            let s = &d[at..end];
            at = end;
            Ok(s)
        };
        fn f(b: &[u8]) -> f32 {
            f32::from_le_bytes(b.try_into().unwrap())
        }
        fn u(b: &[u8]) -> u32 {
            u32::from_le_bytes(b.try_into().unwrap())
        }
        if take(4)? != b"SANM" || u(take(4)?) != 1 {
            return Err("not a SANM v1 file".into());
        }
        let nb = u(take(4)?) as usize;
        let mut sk = SrcSkeleton::default();
        for _ in 0..nb {
            let n = take(1)?[0] as usize;
            sk.names
                .push(String::from_utf8_lossy(take(n)?).into_owned());
            sk.parent.push(u(take(4)?) as i32);
            let v = take(28)?;
            sk.rest_t
                .push(Vec3::new(f(&v[0..4]), f(&v[4..8]), f(&v[8..12])));
            sk.rest_q.push(
                Quat::from_xyzw(f(&v[12..16]), f(&v[16..20]), f(&v[20..24]), f(&v[24..28]))
                    .normalize(),
            );
        }
        let nc = u(take(4)?) as usize;
        let mut clips = Vec::new();
        for _ in 0..nc {
            let n = take(1)?[0] as usize;
            let name = String::from_utf8_lossy(take(n)?).into_owned();
            let fps = f(take(4)?);
            let frames = u(take(4)?) as usize;
            let nt = u(take(4)?) as usize;
            let tr = take(nt * 2)?;
            let tracks: Vec<u16> = tr
                .chunks(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            let mut c = SrcClip {
                name,
                fps,
                frames,
                tracks,
                ..Default::default()
            };
            let raw = take(frames * nt * 28)?;
            for e in raw.chunks_exact(28) {
                c.t.push(Vec3::new(f(&e[0..4]), f(&e[4..8]), f(&e[8..12])));
                let q = Quat::from_xyzw(f(&e[12..16]), f(&e[16..20]), f(&e[20..24]), f(&e[24..28]));
                c.q.push(if q.length_squared() > 1e-12 {
                    q.normalize()
                } else {
                    Quat::IDENTITY
                });
            }
            clips.push(c);
        }
        Ok(Self {
            skeleton: sk,
            clips,
        })
    }
}

/// One mapped bone pair, by node name.
#[derive(Clone, Debug)]
pub struct Pair {
    pub target: String,
    pub source: String,
}

impl Pair {
    pub fn new(target: &str, source: &str) -> Self {
        Self {
            target: target.into(),
            source: source.into(),
        }
    }
}

/// Weapon grip alignment: after retargeting, the weapon carried in `slot` (a helper node under
/// the target hand) points where the source weapon bone's `src_axis` points at rest, so a
/// sword held in the source animation is held the same way by the target hand.
#[derive(Clone, Debug)]
pub struct Grip {
    pub hand: String,
    pub slot: String,
    /// Blade axis of the mounted weapon mesh in the slot's frame.
    pub slot_axis: Vec3,
    pub src_bone: String,
    /// Blade axis in the source weapon bone's frame.
    pub src_axis: Vec3,
}

#[derive(Clone, Debug)]
pub struct Options {
    /// Maps source-frame vectors into the target frame (orthonormal, determinant +-1).
    pub basis: Mat3,
    /// Rotation taking the rig's bind-pose world frame into the frame its motions are authored
    /// in (for the Gothic 3 hero: half a turn about Y, the rotation of the non-animated
    /// `Hero_ROOT` node).
    pub frame: Quat,
    /// Source length unit to target length unit, including any body-height ratio.
    pub scale: f32,
    pub pairs: Vec<Pair>,
    /// Target root (the animated bone without animated ancestor) and its source counterpart.
    pub root: Pair,
    /// Left and right foot bones (target, source) for the ground fix; `None` disables it.
    pub feet: Option<[Pair; 2]>,
    /// `(bone, aim)`: the rest alignment of `bone` points it at the mapped target bone `aim` the
    /// way the source bone points at its counterpart. Bones without an entry inherit the
    /// alignment of their closest mapped ancestor.
    pub aim: Vec<(String, String)>,
    /// `(bone, offset)`: extra rotation in the bone's own rest frame applied after retargeting,
    /// to correct systematic twist differences (hands, feet) found by looking at the result.
    pub offsets: Vec<(String, Quat)>,
    pub grips: Vec<Grip>,
}

#[derive(Debug)]
pub struct Report {
    pub mapped: usize,
    pub unmapped_target: Vec<String>,
    pub missing_source: Vec<String>,
}

fn conj(basis: Mat3, q: Quat) -> Quat {
    // B R B^-1 for orthonormal B (a reflection included); the result is a proper rotation.
    let r = Mat3::from_quat(q);
    Quat::from_mat3(&(basis * r * basis.transpose())).normalize()
}

/// Shortest-arc rotation taking `a` onto `b` (both non-zero).
fn arc(a: Vec3, b: Vec3) -> Quat {
    if a.length_squared() < 1e-12 || b.length_squared() < 1e-12 {
        return Quat::IDENTITY;
    }
    Quat::from_rotation_arc(a.normalize(), b.normalize())
}

struct Bone {
    /// Rig node index of the target.
    node: u16,
    src: usize,
    /// Rotation in the bone's own rest frame applied last.
    offset: Quat,
    /// Rest alignment applied in world space.
    align: Quat,
    /// World rotation of the target bone at bind.
    bind_rot: Quat,
}

/// Prepared mapping of one source skeleton onto one rig.
pub struct Retargeter<'a> {
    rig: &'a Rig,
    src: &'a SrcSkeleton,
    opt: &'a Options,
    bones: Vec<Bone>,
    /// Index into `bones` per slot, or `usize::MAX`.
    bone_of_slot: Vec<usize>,
    src_rest_world_pos: Vec<Vec3>,
    src_rest_world_rot: Vec<Quat>,
    root_bone: usize,
    feet: Option<[(usize, usize); 2]>,
    /// Bind world position of the root bone and of the feet.
    pub report: Report,
}

impl<'a> Retargeter<'a> {
    pub fn new(rig: &'a Rig, src: &'a SrcSkeleton, opt: &'a Options) -> Result<Self, String> {
        let (swp, swq) = src.world(&src.rest_t, &src.rest_q);
        // Source rest in target axes.
        let swp_t: Vec<Vec3> = swp.iter().map(|&p| opt.basis * p * opt.scale).collect();
        let swq_t: Vec<Quat> = swq.iter().map(|&q| conj(opt.basis, q)).collect();

        let mut bones: Vec<Bone> = Vec::new();
        let mut bone_of_slot = vec![usize::MAX; rig.slot_node.len()];
        let mut missing_source = Vec::new();
        let mut pairs = opt.pairs.clone();
        if !pairs.iter().any(|p| p.target == opt.root.target) {
            pairs.push(opt.root.clone());
        }
        for p in &pairs {
            let Some(node) = rig.node(&p.target) else {
                return Err(format!("target bone {} not in rig {}", p.target, rig.name));
            };
            let slot = rig.slot_of[node as usize];
            if slot == NONE {
                return Err(format!("target bone {} is a helper node", p.target));
            }
            let Some(s) = src.index(&p.source) else {
                missing_source.push(p.source.clone());
                continue;
            };
            let w = rig.bind_world[node as usize].to_scale_rotation_translation();
            bone_of_slot[slot as usize] = bones.len();
            let offset = opt
                .offsets
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(&p.target))
                .map_or(Quat::IDENTITY, |(_, q)| *q);
            bones.push(Bone {
                node,
                src: s,
                offset,
                align: Quat::IDENTITY,
                bind_rot: (opt.frame * w.1).normalize(),
            });
        }
        // Rest alignment: point each bone at its aim bone the way the source bone points.
        let mut aligns = vec![Quat::IDENTITY; bones.len()];
        let mut order: Vec<usize> = (0..bones.len()).collect();
        let depth = |node: u16| {
            let mut d = 0;
            let mut p = rig.eff_parent[node as usize];
            while p != NONE {
                d += 1;
                p = rig.eff_parent[p as usize];
            }
            d
        };
        order.sort_by_key(|&i| depth(bones[i].node));
        for &i in &order {
            let b = &bones[i];
            let name = &rig.names[b.node as usize];
            let aim = opt
                .aim
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .and_then(|(_, a)| rig.node(a))
                .and_then(|n| {
                    let slot = rig.slot_of[n as usize];
                    (slot != NONE && bone_of_slot[slot as usize] != usize::MAX)
                        .then(|| bone_of_slot[slot as usize])
                });
            aligns[i] = if let Some(ci) = aim {
                let c = &bones[ci];
                let g_dir = opt.frame
                    * (rig.bind_world[c.node as usize].w_axis.truncate()
                        - rig.bind_world[b.node as usize].w_axis.truncate());
                let s_dir = swp_t[c.src] - swp_t[b.src];
                arc(g_dir, s_dir)
            } else {
                // Inherit the closest mapped ancestor's alignment.
                let mut p = rig.eff_parent[b.node as usize];
                let mut found = Quat::IDENTITY;
                while p != NONE {
                    let slot = rig.slot_of[p as usize];
                    if slot != NONE && bone_of_slot[slot as usize] != usize::MAX {
                        found = aligns[bone_of_slot[slot as usize]];
                        break;
                    }
                    p = rig.eff_parent[p as usize];
                }
                found
            };
        }
        for (b, a) in bones.iter_mut().zip(&aligns) {
            b.align = *a;
        }
        // Grip alignment from the weapon blade axes.
        for g in &opt.grips {
            let (Some(hand), Some(slot), Some(src_bone)) =
                (rig.node(&g.hand), rig.node(&g.slot), src.index(&g.src_bone))
            else {
                continue;
            };
            let hs = rig.slot_of[hand as usize];
            if hs == NONE || bone_of_slot[hs as usize] == usize::MAX {
                continue;
            }
            let bone = &mut bones[bone_of_slot[hs as usize]];
            let hand_rot = rig.bind_world[hand as usize]
                .to_scale_rotation_translation()
                .1;
            let slot_rot = rig.bind_world[slot as usize]
                .to_scale_rotation_translation()
                .1;
            let axis_in_hand = hand_rot.inverse() * (slot_rot * g.slot_axis);
            let want = swq_t[src_bone] * g.src_axis;
            let v = (bone.align * bone.bind_rot).inverse() * want;
            bone.offset = arc(axis_in_hand, v) * bone.offset;
        }
        let root_slot = rig
            .slot_of
            .get(rig.node(&opt.root.target).ok_or("root")? as usize)
            .copied()
            .unwrap_or(NONE);
        let root_bone = bone_of_slot[root_slot as usize];
        if root_bone == usize::MAX {
            return Err("root bone has no source".into());
        }
        let feet = opt.feet.as_ref().and_then(|f| {
            let mut out = [(0usize, 0usize); 2];
            for (k, p) in f.iter().enumerate() {
                let node = rig.node(&p.target)?;
                let slot = rig.slot_of[node as usize];
                if slot == NONE || bone_of_slot[slot as usize] == usize::MAX {
                    return None;
                }
                out[k] = (slot as usize, src.index(&p.source)?);
            }
            Some(out)
        });
        let unmapped_target = rig
            .slot_node
            .iter()
            .enumerate()
            .filter(|(s, _)| bone_of_slot[*s] == usize::MAX)
            .map(|(_, &n)| rig.names[n as usize].clone())
            .collect();
        let mapped = bones.len();
        Ok(Self {
            rig,
            src,
            opt,
            bones,
            bone_of_slot,
            src_rest_world_pos: swp_t,
            src_rest_world_rot: swq_t,
            root_bone,
            feet,
            report: Report {
                mapped,
                unmapped_target,
                missing_source,
            },
        })
    }

    /// Evaluates the target pose for one source frame: motion-convention local transform per
    /// slot, plus the world position of every slot (for the ground fix).
    fn pose(&self, wq_src: &[Quat], wp_src: &[Vec3], dy: f32, out: &mut [Trs], wpos: &mut [Vec3]) {
        let rig = self.rig;
        let mut wrot = vec![Quat::IDENTITY; rig.slot_node.len()];
        for &node in &rig.topo {
            let node = node as usize;
            let slot = rig.slot_of[node];
            if slot == NONE {
                continue;
            }
            let slot = slot as usize;
            let ep = rig.eff_parent[node];
            let rest = rig.rest[slot];
            let bi = self.bone_of_slot[slot];
            if bi != usize::MAX {
                let b = &self.bones[bi];
                let delta =
                    conj(self.opt.basis, wq_src[b.src]) * self.src_rest_world_rot[b.src].inverse();
                let w = (delta * b.align * b.bind_rot * b.offset).normalize();
                wrot[slot] = w;
                if ep == NONE || bi == self.root_bone {
                    let p = self.opt.basis * (wp_src[b.src]) * self.opt.scale;
                    let bind = self.opt.frame * rig.bind_world[node].w_axis.truncate();
                    let pos = bind + (p - self.src_rest_world_pos[b.src]) + Vec3::Y * dy;
                    out[slot] = Trs {
                        t: pos,
                        r: w,
                        s: rest.s,
                    };
                    wpos[slot] = pos;
                } else {
                    let ps = rig.slot_of[ep as usize] as usize;
                    out[slot] = Trs {
                        t: rest.t,
                        r: (wrot[ps].inverse() * w).normalize(),
                        s: rest.s,
                    };
                    wpos[slot] = wpos[ps] + wrot[ps] * rest.t;
                }
            } else if ep == NONE {
                wrot[slot] = (self.opt.frame * rest.r).normalize();
                wpos[slot] = self.opt.frame * rest.t;
                out[slot] = rest;
            } else {
                let ps = rig.slot_of[ep as usize] as usize;
                wrot[slot] = (wrot[ps] * rest.r).normalize();
                wpos[slot] = wpos[ps] + wrot[ps] * rest.t;
                out[slot] = rest;
            }
        }
    }

    /// Fidelity of a retargeted clip: for every mapped bone, the mean and maximum distance (in
    /// target units) between how far the bone moved from its rest position relative to the root
    /// in the target and in the source (scaled), over all frames. The constant offsets that come
    /// from different body proportions cancel; what remains is motion that did not carry over.
    pub fn error_report(&self, clip: &SrcClip) -> Vec<(String, f32, f32)> {
        let slots = self.rig.slot_node.len();
        let mut local_t = self.src.rest_t.clone();
        let mut local_q = self.src.rest_q.clone();
        let mut tmp = vec![Trs::IDENTITY; slots];
        let mut wpos = vec![Vec3::ZERO; slots];
        let mut acc = vec![(0.0f32, 0.0f32); self.bones.len()];
        let ntr = clip.tracks.len();
        let root = &self.bones[self.root_bone];
        let root_slot = self.rig.slot_of[root.node as usize] as usize;
        // Rest displacement baseline.
        let (rp, rq) = self.src.world(&local_t, &local_q);
        self.pose(&rq, &rp, 0.0, &mut tmp, &mut wpos);
        let base_got: Vec<Vec3> = self
            .bones
            .iter()
            .map(|b| wpos[self.rig.slot_of[b.node as usize] as usize] - wpos[root_slot])
            .collect();
        let base_want: Vec<Vec3> = self
            .bones
            .iter()
            .map(|b| self.opt.basis * (rp[b.src] - rp[root.src]) * self.opt.scale)
            .collect();
        for f in 0..clip.frames.max(1) {
            for (k, &b) in clip.tracks.iter().enumerate() {
                local_t[b as usize] = clip.t[f * ntr + k];
                local_q[b as usize] = clip.q[f * ntr + k];
            }
            let (wp, wq) = self.src.world(&local_t, &local_q);
            self.pose(&wq, &wp, 0.0, &mut tmp, &mut wpos);
            for (i, b) in self.bones.iter().enumerate() {
                let slot = self.rig.slot_of[b.node as usize] as usize;
                let got = wpos[slot] - wpos[root_slot] - base_got[i];
                let want =
                    self.opt.basis * (wp[b.src] - wp[root.src]) * self.opt.scale - base_want[i];
                let e = (got - want).length();
                acc[i].0 += e;
                acc[i].1 = acc[i].1.max(e);
            }
        }
        let n = clip.frames.max(1) as f32;
        self.bones
            .iter()
            .zip(acc)
            .map(|(b, (sum, max))| (self.rig.names[b.node as usize].clone(), sum / n, max))
            .collect()
    }

    /// Retargets one clip. `anim_slots` are the slots written to the output (mapped slots plus
    /// anything else the caller wants); the others keep no tracks.
    pub fn clip(&self, clip: &SrcClip) -> Clip {
        self.clip_with_stats(clip).0
    }

    /// [`Self::clip`] plus, per frame, `[source lower toe height above its rest height, target
    /// lower toe height above the target's rest height after the ground fix, vertical shift]`.
    pub fn clip_with_stats(&self, clip: &SrcClip) -> (Clip, Vec<[f32; 3]>) {
        let n_src = self.src.names.len();
        let slots = self.rig.slot_node.len();
        let frames = clip.frames.max(1);
        let mut local_t = self.src.rest_t.clone();
        let mut local_q = self.src.rest_q.clone();
        let mut poses: Vec<Vec<Trs>> = Vec::with_capacity(frames);
        let mut src_foot_y: Vec<[f32; 2]> = Vec::with_capacity(frames);
        let mut wpos = vec![Vec3::ZERO; slots];
        let mut tmp = vec![Trs::IDENTITY; slots];
        let mut foot_actual: Vec<[f32; 2]> = Vec::with_capacity(frames);
        let ntr = clip.tracks.len();
        for f in 0..frames {
            for (k, &b) in clip.tracks.iter().enumerate() {
                local_t[b as usize] = clip.t[f * ntr + k];
                local_q[b as usize] = clip.q[f * ntr + k];
            }
            let (wp, wq) = self.src.world(&local_t, &local_q);
            debug_assert_eq!(wp.len(), n_src);
            self.pose(&wq, &wp, 0.0, &mut tmp, &mut wpos);
            if let Some(feet) = self.feet {
                foot_actual.push([wpos[feet[0].0].y, wpos[feet[1].0].y]);
                src_foot_y.push([
                    (self.opt.basis * wp[feet[0].1]).y * self.opt.scale,
                    (self.opt.basis * wp[feet[1].1]).y * self.opt.scale,
                ]);
            }
            poses.push(tmp.clone());
        }
        // Ground fix: keep the supporting (lower) foot where the source has it.
        let mut dys = vec![0.0f32; frames];
        if let Some(feet) = self.feet {
            let rest_foot: Vec<f32> = feet
                .iter()
                .map(|f| self.src_rest_world_pos[f.1].y)
                .collect();
            let bind_foot: Vec<f32> = feet
                .iter()
                .map(|f| {
                    self.rig.bind_world[self.rig.slot_node[f.0] as usize]
                        .w_axis
                        .y
                })
                .collect();
            for f in 0..frames {
                let low = if src_foot_y[f][0] <= src_foot_y[f][1] {
                    0
                } else {
                    1
                };
                let want = bind_foot[low] + (src_foot_y[f][low] - rest_foot[low]);
                dys[f] = want - foot_actual[f][low];
            }
            // Light smoothing against support-foot switches.
            let raw = dys.clone();
            for f in 0..frames {
                let a = raw[f.saturating_sub(1)];
                let c = raw[(f + 1).min(frames - 1)];
                dys[f] = 0.25 * a + 0.5 * raw[f] + 0.25 * c;
            }
        }
        // Build tracks.
        let dt = 1.0 / clip.fps.max(1.0);
        let mut nodes: Vec<ClipNode> = Vec::new();
        let mut by_name = HashMap::new();
        for (slot, &node) in self.rig.slot_node.iter().enumerate() {
            let bi = self.bone_of_slot[slot];
            if bi == usize::MAX {
                continue;
            }
            let is_root = bi == self.root_bone;
            let mut rot: Vec<(f32, Quat)> = Vec::with_capacity(frames);
            let mut pos: Vec<(f32, Vec3)> = Vec::new();
            for f in 0..frames {
                let mut p = poses[f][slot];
                if is_root {
                    p.t.y += dys[f];
                    pos.push((f as f32 * dt, p.t));
                }
                let mut q = p.r;
                if let Some(&(_, prev)) = rot.last()
                    && prev.dot(q) < 0.0
                {
                    q = -q;
                }
                rot.push((f as f32 * dt, q));
            }
            let rest = self.rig.rest[slot];
            let name = self.rig.names[node as usize].clone();
            by_name.insert(name.clone(), nodes.len() as u16);
            nodes.push(ClipNode {
                name,
                pos: Track::new(pos),
                rot: Track::new(rot),
                scale: Track::new(vec![]),
                rest,
            });
        }
        let mut stats = Vec::new();
        if let Some(feet) = self.feet {
            let rest_src = feet.map(|f| self.src_rest_world_pos[f.1].y);
            let rest_tgt = feet.map(|f| {
                self.rig.bind_world[self.rig.slot_node[f.0] as usize]
                    .w_axis
                    .y
            });
            for f in 0..frames {
                let low = if src_foot_y[f][0] <= src_foot_y[f][1] {
                    0
                } else {
                    1
                };
                stats.push([
                    src_foot_y[f][low] - rest_src[low],
                    foot_actual[f][low] + dys[f] - rest_tgt[low],
                    dys[f],
                ]);
            }
        }
        let out = Clip {
            name: clip.name.clone(),
            duration: (frames - 1) as f32 * dt,
            nodes,
            looping: false,
            nominal_speed: 0.0,
            implied_dir: Vec3::ZERO,
            implied_turn: 0.0,
            by_name,
        };
        (out, stats)
    }
}
