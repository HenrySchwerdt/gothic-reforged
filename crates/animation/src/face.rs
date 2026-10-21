//! Facial animation and lip-sync (ENGINE_MAP B14, batch E38).
//!
//! The original attaches a second actor to a speaking character: the *animated head*
//! (`G3_Head_..._Animated_NN`), the same head mesh with extra face bones (lips, jaw, brows). A
//! `.xlip` file lists `{time, viseme}` entries; `eCWrapper_emfx2Motion::CreateLipSyncMotion`
//! (Engine:0x10052cb0) turns them into an ordinary motion from the head's `_LipSync` source
//! motion: every node track with at least 14 keys is a *viseme track* (one key per viseme), and
//! the new motion gets one key per lip-sync entry, taken from the source track at the entry's
//! viseme index. The motion plays once in the facial slot (7); the engine interpolates between
//! keys. Tracks with fewer than 14 keys are not viseme tracks and stay at their static pose.
//!
//! In Bevy the static head of a character is a skinned mesh bound to the body skeleton, which has
//! no face bones. [`FaceRig`] therefore gives the speaking character a child skeleton of its
//! own: the animated head part with its own rig, whose body-shared bones copy the body's current
//! pose (the head keeps following the body animation) and whose face bones are driven by the
//! lip-sync motion. The static head is hidden while it exists.
use crate::{
    animator::Animator,
    clip::{Clip, ClipBinding, ClipNode, Track},
    library::Composition,
    pose::Trs,
    rig::{EvalStep, NONE, Rig},
    runtime::{ActorSpawner, PartMeshes, SkeletonJoint},
};
use bevy::{
    camera::visibility::Visibility,
    ecs::{
        component::Component,
        entity::Entity,
        message::{Message, MessageReader},
        query::{With, Without},
        system::{Commands, Query, Res},
    },
    math::{Affine3A, Quat, Vec3},
    prelude::{GlobalTransform, Time},
};
use std::{collections::HashMap, sync::Arc};

/// Key count that marks a node track as a viseme track.
pub const VISEME_KEYS: usize = 14;
/// The rest viseme every lip-sync starts from and returns to.
pub const REST_VISEME: u32 = 1;
/// Seconds the mouth takes to close after the last entry.
pub const CLOSE_SECS: f32 = 0.2;

/// Start lip-sync on a character: `frames` are the `{seconds, viseme}` entries of a `.xlip`.
#[derive(Message, Debug, Clone)]
pub struct LipSyncStart {
    pub entity: Entity,
    pub frames: Vec<(f32, u32)>,
}

/// Candidate animated-head actors for a static head part name (`G3_Head_Hero_Myrtana_01@M1`):
/// the `_Animated` variant is named by inserting `_Animated` before the trailing number, with
/// or without the race token (`G3_Head_Fat_Myrtana_01` -> `G3_Head_Fat_Animated_01`). The
/// material switch of the part is tried first, then none.
pub fn facial_actor_candidates(head_part: &str) -> Vec<String> {
    let (stem, switch) = match head_part.rsplit_once("@M") {
        Some((s, n)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => (s, Some(n)),
        _ => (head_part, None),
    };
    let stem = stem.strip_suffix("_LOD1").unwrap_or(stem);
    let stem = stem.strip_suffix("_LOD2").unwrap_or(stem);
    let mut names = Vec::new();
    if let Some((base, number)) = stem.rsplit_once('_')
        && number.chars().all(|c| c.is_ascii_digit())
        && !base.ends_with("_Animated")
    {
        names.push(format!("{base}_Animated_{number}"));
        // Drop the middle token of `G3_Head_<Race>_<Variant>`: `G3_Head_Fat_Myrtana`.
        let parts: Vec<&str> = base.split('_').collect();
        if parts.len() >= 4 {
            let shorter = parts[..parts.len() - 1].join("_");
            names.push(format!("{shorter}_Animated_{number}"));
        }
    }
    let mut out = Vec::new();
    for n in names {
        if let Some(sw) = switch {
            out.push(format!("{n}@M{sw}"));
        }
        out.push(n);
    }
    out
}

/// Name of the source motion of an animated head actor.
pub fn lipsync_source_name(actor: &str) -> String {
    format!("{}_LipSync", actor.split('@').next().unwrap_or(actor))
}

/// Port of eCWrapper_emfx2Motion::CreateLipSyncMotion (Engine:0x10052cb0). Returns `None` when
/// there are no entries or the source has no viseme track.
pub fn build_lipsync_clip(name: &str, source: &Clip, frames: &[(f32, u32)]) -> Option<Clip> {
    let mut timeline: Vec<(f32, u32)> = frames.to_vec();
    let end = timeline.last()?.0;
    timeline.push((end + CLOSE_SECS, REST_VISEME));
    let pick = |len: usize, v: u32| (v as usize).min(len - 1);
    let mut nodes = Vec::new();
    for n in &source.nodes {
        let pos = (n.pos.len() >= VISEME_KEYS).then(|| {
            Track::new(
                timeline
                    .iter()
                    .map(|&(t, v)| (t, n.pos.values()[pick(n.pos.len(), v)]))
                    .collect::<Vec<(f32, Vec3)>>(),
            )
        });
        let rot = (n.rot.len() >= VISEME_KEYS).then(|| {
            let mut keys: Vec<(f32, Quat)> = timeline
                .iter()
                .map(|&(t, v)| (t, n.rot.values()[pick(n.rot.len(), v)]))
                .collect();
            // Neighbouring keys may come from different visemes: keep them in one hemisphere.
            for i in 1..keys.len() {
                if keys[i - 1].1.dot(keys[i].1) < 0.0 {
                    keys[i].1 = -keys[i].1;
                }
            }
            Track::new(keys)
        });
        let scale = (n.scale.len() >= VISEME_KEYS).then(|| {
            Track::new(
                timeline
                    .iter()
                    .map(|&(t, v)| (t, n.scale.values()[pick(n.scale.len(), v)]))
                    .collect::<Vec<(f32, Vec3)>>(),
            )
        });
        if pos.is_none() && rot.is_none() && scale.is_none() {
            continue;
        }
        nodes.push(ClipNode {
            name: n.name.clone(),
            pos: pos.unwrap_or_else(|| Track::new(Vec::new())),
            rot: rot.unwrap_or_else(|| Track::new(Vec::new())),
            scale: scale.unwrap_or_else(|| Track::new(Vec::new())),
            rest: n.rest,
        });
    }
    if nodes.is_empty() {
        return None;
    }
    let by_name: HashMap<String, u16> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.name.clone(), i as u16))
        .collect();
    Some(Clip {
        name: name.to_owned(),
        duration: end + CLOSE_SECS,
        nodes,
        looping: false,
        nominal_speed: 0.0,
        implied_dir: Vec3::ZERO,
        implied_turn: 0.0,
        by_name,
    })
}

/// The animated head actor and its lip-sync source motion.
pub struct FaceAssets {
    pub part: Arc<crate::library::PartAsset>,
    pub source: Arc<Clip>,
}

/// Child skeleton of a speaking character (see the module note).
#[derive(Component)]
pub struct FaceRig {
    rig: Arc<Rig>,
    plan: Arc<[EvalStep]>,
    joint_nodes: Vec<u16>,
    /// Body animator slot per face-rig slot (`NONE` for face-only bones).
    body_slot: Box<[u16]>,
    pose: Vec<Trs>,
    model: Vec<Affine3A>,
    facing: Affine3A,
    clip: Arc<Clip>,
    binding: ClipBinding,
    time: f32,
    joints: Vec<Entity>,
    meshes: Vec<Entity>,
    hidden: Vec<Entity>,
}

impl FaceRig {
    pub fn time(&self) -> f32 {
        self.time
    }

    pub fn duration(&self) -> f32 {
        self.clip.duration
    }

    /// Sets the playback time (tools that render a fixed frame).
    pub fn seek(&mut self, t: f32) {
        self.time = t;
    }

    /// Port of the facial layer pass: body bones copy the body pose, the lip-sync motion
    /// overrides the face bones it animates, everything else rests. Returns the model-space
    /// matrix (with the facing fix) of every compact joint of the head part.
    fn evaluate(&mut self, body_pose: &[Trs]) -> Vec<Affine3A> {
        let t = self.time.min(self.clip.duration);
        for (slot, p) in self.pose.iter_mut().enumerate() {
            let b = self.body_slot[slot];
            *p = if b != NONE && usize::from(b) < body_pose.len() {
                body_pose[usize::from(b)]
            } else {
                self.rig.rest[slot]
            };
            let n = self.binding.node_of_slot[slot];
            if n != NONE {
                *p = self.clip.nodes[usize::from(n)].sample(t);
            }
        }
        crate::rig::evaluate(&self.plan, &self.pose, &mut self.model);
        self.joint_nodes
            .iter()
            .map(|&n| self.facing * self.model[usize::from(n)])
            .collect()
    }
}

/// Index of the head part inside a composition (never the body).
pub fn head_part_index(composition: &Composition) -> Option<usize> {
    composition
        .parts
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, p)| p.name.to_ascii_lowercase().contains("head"))
        .map(|(i, _)| i)
}

/// Reads [`LipSyncStart`] messages and gives the character its [`FaceRig`].
pub fn face_start_system(
    mut starts: MessageReader<LipSyncStart>,
    mut spawner: ActorSpawner,
    characters: Query<(
        &crate::runtime::CharacterComposition,
        &Animator,
        &PartMeshes,
        Option<&FaceRig>,
    )>,
) {
    for start in starts.read() {
        let Ok((composition, animator, part_meshes, existing)) = characters.get(start.entity)
        else {
            continue;
        };
        if let Some(old) = existing {
            for e in old.joints.iter().chain(&old.meshes) {
                spawner.commands.entity(*e).despawn();
            }
            for e in &old.hidden {
                spawner.commands.entity(*e).insert(Visibility::Inherited);
            }
        }
        let comp = &composition.0;
        let Some(head) = head_part_index(comp) else {
            continue;
        };
        let candidates = facial_actor_candidates(&comp.parts[head].name);
        let Some(assets) = spawner.load_face(&candidates) else {
            continue;
        };
        let Some(clip) = build_lipsync_clip(
            &format!("{}_speech", assets.part.name),
            &assets.source,
            &start.frames,
        ) else {
            continue;
        };
        let clip = Arc::new(clip);
        let rig = assets.part.rig.clone();
        let binding = ClipBinding::new(&rig, &clip);
        let body_rig = &animator.rig;
        let body_slot: Box<[u16]> = rig
            .slot_node
            .iter()
            .map(|&node| {
                body_rig
                    .index
                    .get(&rig.names[usize::from(node)].to_lowercase())
                    .map_or(NONE, |&b| body_rig.slot_of[usize::from(b)])
            })
            .collect();
        let joint_nodes = assets.part.joint_nodes.clone();
        let plan: Arc<[EvalStep]> = rig.plan(&joint_nodes).into();
        let (joints, meshes) = spawner.spawn_face_parts(start.entity, &assets.part);
        let hidden = part_meshes
            .0
            .get(head)
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        for e in &hidden {
            spawner.commands.entity(*e).insert(Visibility::Hidden);
        }
        let pose = rig.rest.clone();
        let model = vec![Affine3A::IDENTITY; rig.names.len()];
        spawner.commands.entity(start.entity).insert(FaceRig {
            rig,
            plan,
            joint_nodes,
            body_slot,
            pose,
            model,
            facing: Affine3A::from_quat(comp.facing_fix),
            clip,
            binding,
            time: 0.0,
            joints,
            meshes,
            hidden,
        });
    }
}

/// Advances every [`FaceRig`], writes its joints and removes it when the speech ended.
pub fn face_update_system(
    time: Res<Time>,
    mut commands: Commands,
    mut characters: Query<
        (Entity, &mut FaceRig, &Animator, &GlobalTransform),
        Without<SkeletonJoint>,
    >,
    mut joints: Query<&mut GlobalTransform, With<SkeletonJoint>>,
) {
    let dt = time.delta_secs().min(0.1);
    for (entity, mut face, animator, global) in &mut characters {
        face.time += dt;
        let world = global.affine();
        let matrices = face.evaluate(animator.local_pose());
        let mut it = joints.iter_many_mut(&face.joints);
        let mut i = 0;
        while let Some(joint) = it.fetch_next() {
            if let Ok(mut joint) = joint {
                *joint = GlobalTransform::from(world * matrices[i]);
            }
            i += 1;
        }
        if face.time > face.clip.duration {
            for e in face.joints.iter().chain(&face.meshes) {
                commands.entity(*e).despawn();
            }
            for e in &face.hidden {
                commands.entity(*e).insert(Visibility::Inherited);
            }
            commands.entity(entity).remove::<FaceRig>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY_TRS: Trs = Trs {
        t: Vec3::ZERO,
        r: Quat::IDENTITY,
        s: Vec3::ONE,
    };

    fn rot(i: usize) -> Quat {
        Quat::from_rotation_x(i as f32 * 0.1)
    }

    fn source() -> Clip {
        let viseme_node = ClipNode {
            name: "Jaw".into(),
            pos: Track::new(Vec::new()),
            rot: Track::new((0..15).map(|i| (i as f32 * 0.04, rot(i))).collect()),
            scale: Track::new(Vec::new()),
            rest: IDENTITY_TRS,
        };
        let pose_only = ClipNode {
            name: "Brow".into(),
            // 10 keys: not a viseme track
            pos: Track::new(
                (0..10)
                    .map(|i| (i as f32 * 0.04, Vec3::splat(i as f32)))
                    .collect(),
            ),
            rot: Track::new(Vec::new()),
            scale: Track::new(Vec::new()),
            rest: IDENTITY_TRS,
        };
        let mut by_name = HashMap::new();
        by_name.insert("Jaw".to_owned(), 0);
        by_name.insert("Brow".to_owned(), 1);
        Clip {
            name: "x_LipSync".into(),
            duration: 0.56,
            nodes: vec![viseme_node, pose_only],
            looping: false,
            nominal_speed: 0.0,
            implied_dir: Vec3::ZERO,
            implied_turn: 0.0,
            by_name,
        }
    }

    #[test]
    fn lipsync_motion_takes_the_source_key_of_each_viseme() {
        let clip = build_lipsync_clip("t", &source(), &[(0.0, 1), (0.5, 10), (1.0, 3)]).unwrap();
        // only the viseme track survives
        assert_eq!(clip.nodes.len(), 1);
        assert_eq!(clip.nodes[0].name, "Jaw");
        assert!((clip.duration - (1.0 + CLOSE_SECS)).abs() < 1e-6);
        let at = |t: f32| clip.nodes[0].sample(t).r;
        assert!(at(0.0).angle_between(rot(1)) < 1e-4);
        assert!(at(0.5).angle_between(rot(10)) < 1e-4);
        assert!(at(1.0).angle_between(rot(3)) < 1e-4);
        // halfway between two entries the keys are interpolated
        let mid = at(0.25);
        assert!(mid.angle_between(rot(1)) > 0.05 && mid.angle_between(rot(10)) > 0.05);
        // the mouth closes after the last entry
        assert!(at(clip.duration).angle_between(rot(REST_VISEME as usize)) < 1e-4);
    }

    #[test]
    fn viseme_beyond_the_key_count_is_clamped_and_empty_input_gives_nothing() {
        let clip = build_lipsync_clip("t", &source(), &[(0.0, 99)]).unwrap();
        assert!(clip.nodes[0].sample(0.0).r.angle_between(rot(14)) < 1e-4);
        assert!(build_lipsync_clip("t", &source(), &[]).is_none());
    }

    #[test]
    fn opposite_hemisphere_keys_do_not_swing_the_long_way() {
        let mut src = source();
        let mut keys: Vec<(f32, Quat)> = (0..15).map(|i| (i as f32 * 0.04, rot(i))).collect();
        keys[10].1 = -keys[10].1; // same rotation, flipped sign
        src.nodes[0].rot = Track::new(keys);
        let clip = build_lipsync_clip("t", &src, &[(0.0, 1), (1.0, 10)]).unwrap();
        let mid = clip.nodes[0].sample(0.5).r;
        assert!(
            mid.angle_between(rot(5)) < 0.3,
            "{}",
            mid.angle_between(rot(5))
        );
    }

    #[test]
    fn animated_head_names() {
        assert_eq!(
            facial_actor_candidates("G3_Head_Hero_Myrtana_01"),
            [
                "G3_Head_Hero_Myrtana_Animated_01",
                "G3_Head_Hero_Animated_01"
            ]
        );
        assert_eq!(
            facial_actor_candidates("G3_Head_Fat_Myrtana_02@M3"),
            [
                "G3_Head_Fat_Myrtana_Animated_02@M3",
                "G3_Head_Fat_Myrtana_Animated_02",
                "G3_Head_Fat_Animated_02@M3",
                "G3_Head_Fat_Animated_02"
            ]
        );
        assert!(facial_actor_candidates("G3_Fat_Head_Head01").is_empty());
        assert_eq!(
            lipsync_source_name("G3_Head_Fat_Animated_01@M2"),
            "G3_Head_Fat_Animated_01_LipSync"
        );
    }
}
