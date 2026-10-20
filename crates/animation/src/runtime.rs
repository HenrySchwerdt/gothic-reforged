//! ECS integration: components, the spawner and the per-frame systems.
//!
//! Frame flow:
//! 1. `Update`: [`locomotion_system`] reads [`LocomotionInput`], steps the state machine and
//!    starts/cross-fades clips on the [`Animator`].
//! 2. `PostUpdate`: [`animate_system`] advances every animator in parallel and evaluates joint
//!    matrices; after transform propagation [`write_joints_system`] copies them into the joint
//!    entities' `GlobalTransform`s (the joints have no `Transform`: the CPU already holds the
//!    model-space matrices, so no hierarchy propagation is needed for the skeleton) and
//!    [`attachment_system`] places equipment.
use crate::{
    animator::{Animator, Overlay},
    library::{ActorLibrary, AssetStores, Composition},
    locomotion::{
        FORWARD, LocalMotion, LocoKey, Locomotion, LocomotionInput, MoveDir, MoveKind, RIGHT,
        Stance,
    },
};
use anyhow::{Result, anyhow};
use bevy::{
    camera::visibility::{NoFrustumCulling, Visibility},
    ecs::{
        component::Component,
        entity::Entity,
        hierarchy::ChildOf,
        resource::Resource,
        system::{Commands, Query, Res, ResMut, SystemParam},
    },
    math::{Affine3A, Vec3},
    mesh::{
        Mesh, Mesh3d,
        skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
    },
    prelude::{Assets, GlobalTransform, Image, MeshMaterial3d, StandardMaterial, Time, Transform},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// Marks a skeleton joint entity (carries only a `GlobalTransform`, written by the systems).
#[derive(Component)]
pub struct SkeletonJoint;

/// Joint entities of a character, in composition order.
#[derive(Component)]
pub struct CharacterJoints(pub Vec<Entity>);

/// Link back to the composition (part names, joint lookup by node name).
#[derive(Component)]
pub struct CharacterComposition(pub Arc<Composition>);

/// Equipment placed at a joint of its parent character each frame.
#[derive(Component)]
pub struct Attachment {
    /// Index into the character's joints.
    pub joint: u16,
    /// Offset from the joint (mesh space to slot space).
    pub offset: Affine3A,
}

/// Switches an [`Attachment`] between a drawn and a holstered joint depending on the parent's
/// [`Stance`].
#[derive(Component)]
pub struct WeaponMount {
    pub drawn_joint: u16,
    pub holster_joint: u16,
}

/// Wall-clock cost of the animation systems in the last frame.
#[derive(Resource, Default, Debug)]
pub struct AnimationStats {
    pub characters: usize,
    pub locomotion: Duration,
    pub animate: Duration,
    pub write: Duration,
    /// Exponential moving average of the three phases combined, in microseconds.
    pub average_micros: f32,
}

#[derive(Clone, Debug)]
pub struct WeaponSpec {
    /// Static mesh name (`G3_Weapon_1H_Sword_01`).
    pub mesh: String,
    pub drawn_slot: String,
    pub holster_slot: String,
    /// Mesh-space to slot-space offset.
    pub offset: Transform,
}

impl WeaponSpec {
    pub fn one_hand_sword(mesh: &str) -> Self {
        Self {
            mesh: mesh.into(),
            drawn_slot: "Slot_RightHand_Weapon".into(),
            holster_slot: "Slot_MeleeBackRight".into(),
            offset: Transform::IDENTITY,
        }
    }
}

/// Description of a human NPC: body skeleton first, then any number of parts bound to the body
/// skeleton by node name (head, hair, armor pieces).
#[derive(Clone, Debug)]
pub struct NpcSpec {
    pub parts: Vec<String>,
    pub stance: Stance,
    pub weapons: Vec<WeaponSpec>,
    /// Initial playback offset in seconds (desynchronises crowds).
    pub start_time: f32,
    /// Scripted motion (exact name) played in a loop instead of locomotion; the character
    /// then gets no [`Locomotion`]/[`LocomotionInput`] components.
    pub motion: Option<String>,
}

impl NpcSpec {
    pub fn new(body: impl Into<String>) -> Self {
        Self {
            parts: vec![body.into()],
            stance: Stance::Unarmed,
            weapons: Vec::new(),
            start_time: 0.0,
            motion: None,
        }
    }

    pub fn with_part(mut self, part: impl Into<String>) -> Self {
        self.parts.push(part.into());
        self
    }

    pub fn with_stance(mut self, stance: Stance) -> Self {
        self.stance = stance;
        self
    }

    pub fn with_weapon(mut self, weapon: WeaponSpec) -> Self {
        self.weapons.push(weapon);
        self
    }

    pub fn with_motion(mut self, name: impl Into<String>) -> Self {
        self.motion = Some(name.into());
        self
    }

    pub fn with_start_time(mut self, t: f32) -> Self {
        self.start_time = t;
        self
    }

    /// Composes body and head from NPC item-template names such as `Body_AssWarrior` and
    /// `Head_Hero_05_S24` (see [`crate::library::template_actor_candidates`]). Parts are
    /// resolved against the installed actors at spawn time.
    pub fn from_item_templates(
        library: &ActorLibrary,
        body: &str,
        head: Option<&str>,
    ) -> Result<Self> {
        let pick = |template: &str| -> Result<String> {
            crate::library::template_actor_candidates(template)
                .into_iter()
                .find(|c| library.has_actor(c))
                .ok_or_else(|| anyhow!("no installed actor matches template {template:?}"))
        };
        let mut spec = Self::new(pick(body)?);
        if let Some(head) = head {
            spec.parts.push(pick(head)?);
        }
        Ok(spec)
    }
}

/// System parameter that spawns fully animated characters.
#[derive(SystemParam)]
pub struct ActorSpawner<'w, 's> {
    pub commands: Commands<'w, 's>,
    pub library: ResMut<'w, ActorLibrary>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    images: ResMut<'w, Assets<Image>>,
    bindposes: ResMut<'w, Assets<SkinnedMeshInverseBindposes>>,
}

impl ActorSpawner<'_, '_> {
    /// Spawns a character at `transform`. Drive it through its [`LocomotionInput`]; or call
    /// [`Animator::play`] directly for scripted motions.
    pub fn spawn(&mut self, spec: &NpcSpec, transform: Transform) -> Result<Entity> {
        let Self {
            commands,
            library,
            meshes,
            materials,
            images,
            bindposes,
        } = self;
        let mut stores = AssetStores {
            meshes,
            materials,
            images,
            bindposes,
        };
        let names: Vec<&str> = spec.parts.iter().map(String::as_str).collect();
        let composition = library.composition(&names, &mut stores)?;
        let rig = composition.rig.clone();
        let mut animator = Animator::new(
            rig.clone(),
            composition.plan.clone(),
            composition.joint_nodes.clone(),
            composition.facing_fix,
        );
        let mut locomotion = Locomotion::default();
        locomotion.key.stance = spec.stance;
        let idle_key = LocoKey {
            kind: MoveKind::Idle,
            dir: MoveDir::Fwd,
            stance: spec.stance,
            sneak: false,
        };
        if let Some(name) = &spec.motion {
            let motion = library.motion(&rig, name)?;
            animator.play(motion, 0.0, 1.0, spec.start_time);
        } else if let Some(name) = library.locomotion_motion(&rig.family, idle_key) {
            let motion = library.motion(&rig, &name)?;
            animator.play(motion, 0.0, 1.0, spec.start_time);
            locomotion.playing = Some(idle_key);
        }
        animator.evaluate();

        let root = commands
            .spawn((
                transform,
                Visibility::default(),
                animator,
                CharacterComposition(composition.clone()),
            ))
            .id();
        if spec.motion.is_none() {
            commands.entity(root).insert((
                locomotion,
                LocomotionInput {
                    stance: spec.stance,
                    ..Default::default()
                },
            ));
        }
        let joints: Vec<Entity> = (0..composition.joint_nodes.len())
            .map(|_| {
                commands
                    .spawn((SkeletonJoint, GlobalTransform::default(), ChildOf(root)))
                    .id()
            })
            .collect();
        for (part, part_joints) in composition.parts.iter().zip(&composition.part_joints) {
            let skin_joints: Vec<Entity> =
                part_joints.iter().map(|&j| joints[j as usize]).collect();
            for m in &part.meshes {
                commands.spawn((
                    Mesh3d(m.mesh.clone()),
                    MeshMaterial3d(m.material.clone()),
                    SkinnedMesh {
                        inverse_bindposes: part.inverse_bindposes.clone(),
                        joints: skin_joints.clone(),
                    },
                    part.aabb,
                    Transform::default(),
                    Visibility::default(),
                    ChildOf(root),
                ));
            }
        }
        for weapon in &spec.weapons {
            let asset = library.static_mesh(&weapon.mesh, &mut stores)?;
            let joint_of = |slot: &str| -> Result<u16> {
                composition
                    .joint_by_name
                    .get(&slot.to_lowercase())
                    .copied()
                    .ok_or_else(|| anyhow!("rig has no slot {slot:?}"))
            };
            let drawn = joint_of(&weapon.drawn_slot)?;
            let holster = joint_of(&weapon.holster_slot).unwrap_or(drawn);
            let start = if spec.stance.weapon_drawn() {
                drawn
            } else {
                holster
            };
            for m in &asset.meshes {
                commands.spawn((
                    Mesh3d(m.mesh.clone()),
                    MeshMaterial3d(m.material.clone()),
                    Attachment {
                        joint: start,
                        offset: Affine3A::from_mat4(weapon.offset.to_matrix()),
                    },
                    WeaponMount {
                        drawn_joint: drawn,
                        holster_joint: holster,
                    },
                    Transform::default(),
                    Visibility::default(),
                    NoFrustumCulling,
                    ChildOf(root),
                ));
            }
        }
        commands.entity(root).insert(CharacterJoints(joints));
        Ok(root)
    }

    /// Starts an upper-body overlay motion (blended through the rig's upper-body mask).
    pub fn overlay(
        &mut self,
        animator: &mut Animator,
        motion_name: &str,
        weight: f32,
    ) -> Result<()> {
        let rig = animator.rig.clone();
        let motion = self.library.motion(&rig, motion_name)?;
        animator.set_overlay(Some(Overlay {
            motion,
            time: 0.0,
            rate: 1.0,
            weight,
            mask: rig.upper_body_mask().into(),
        }));
        Ok(())
    }
}

/// Reads [`LocomotionInput`], steps [`Locomotion`] and switches clips on the [`Animator`].
pub fn locomotion_system(
    mut library: ResMut<ActorLibrary>,
    mut stats: ResMut<AnimationStats>,
    mut query: Query<(
        &mut Locomotion,
        &LocomotionInput,
        &GlobalTransform,
        &mut Animator,
    )>,
) {
    let started = Instant::now();
    let mut count = 0;
    for (mut loco, input, global, mut animator) in &mut query {
        count += 1;
        let rig = animator.rig.clone();
        let inverse = global.rotation().inverse();
        let local: Vec3 = inverse * input.velocity;
        let v = LocalMotion {
            forward: local.dot(FORWARD),
            right: local.dot(RIGHT),
            up: local.y,
        };
        let finished = animator.finished();
        let key = loco.step(input, v, finished);
        if loco.playing != Some(key) {
            if let Some(name) = library.locomotion_motion(&rig.family, key) {
                loco.playing = Some(key);
                if animator.current_name() != Some(name.as_str()) {
                    match library.motion(&rig, &name) {
                        Ok(motion) => animator.play(motion, loco.config.fade, 1.0, 0.0),
                        Err(error) => eprintln!("locomotion clip {name}: {error:#}"),
                    }
                }
            } else {
                loco.playing = Some(key);
            }
        }
        if let Some(clip) = animator.current_clip() {
            let rate = loco.rate_for(key, clip.nominal_speed);
            animator.set_rate(rate);
        }
    }
    stats.characters = count;
    stats.locomotion = started.elapsed();
}

/// Advances every animator (parallel) and evaluates joint matrices.
pub fn animate_system(
    time: Res<Time>,
    mut stats: ResMut<AnimationStats>,
    mut query: Query<&mut Animator>,
) {
    let started = Instant::now();
    let dt = time.delta_secs().min(0.1);
    query
        .par_iter_mut()
        .for_each(|mut animator| animator.update(dt));
    stats.animate = started.elapsed();
}

/// Copies joint matrices into the joint entities (model space times the character's world).
pub fn write_joints_system(
    mut stats: ResMut<AnimationStats>,
    characters: Query<
        (&Animator, &CharacterJoints, &GlobalTransform),
        bevy::ecs::query::Without<SkeletonJoint>,
    >,
    mut joints: Query<&mut GlobalTransform, bevy::ecs::query::With<SkeletonJoint>>,
) {
    let started = Instant::now();
    for (animator, entities, global) in &characters {
        let world = global.affine();
        let mut it = joints.iter_many_mut(&entities.0);
        let mut i = 0;
        while let Some(joint) = it.fetch_next() {
            if let Ok(mut joint) = joint {
                *joint = GlobalTransform::from(world * animator.joints[i]);
            }
            i += 1;
        }
    }
    stats.write = started.elapsed();
    let total = (stats.locomotion + stats.animate + stats.write).as_secs_f32() * 1.0e6;
    stats.average_micros = if stats.average_micros == 0.0 {
        total
    } else {
        stats.average_micros * 0.95 + total * 0.05
    };
}

/// Chooses the drawn or holstered joint for weapons from the parent's stance.
pub fn weapon_mount_system(
    inputs: Query<&LocomotionInput>,
    mut mounts: Query<(&WeaponMount, &ChildOf, &mut Attachment)>,
) {
    for (mount, parent, mut attachment) in &mut mounts {
        let Ok(input) = inputs.get(parent.parent()) else {
            continue;
        };
        let joint = if input.stance.weapon_drawn() {
            mount.drawn_joint
        } else {
            mount.holster_joint
        };
        if attachment.joint != joint {
            attachment.joint = joint;
        }
    }
}

/// Places attachments at their joints.
pub fn attachment_system(
    characters: Query<(&Animator, &GlobalTransform), bevy::ecs::query::Without<Attachment>>,
    mut attachments: Query<(&Attachment, &ChildOf, &mut GlobalTransform)>,
) {
    for (attachment, parent, mut global) in &mut attachments {
        let Ok((animator, root)) = characters.get(parent.parent()) else {
            continue;
        };
        let Some(joint) = animator.joints.get(attachment.joint as usize) else {
            continue;
        };
        *global = GlobalTransform::from(root.affine() * *joint * attachment.offset);
    }
}
