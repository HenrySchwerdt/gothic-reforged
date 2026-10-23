//! Orc raiders: real `G3_Orc_Body_Warrior` + `G3_Orc_Head_Head01` actors (the data-driven appearance of
//! `Orc_GameStartRaider_Warrior_01`, mirrored like all actors) driven by the cloud `vr-npc` active ragdoll and combat
//! AI through the same retargeter as the hero; damage goes through `combat.rs` (the Gothic pipeline).
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use gothic_vr::VrPlayer;
use gothic_vr_npc::{VrNpc, spawn_npc};
use gothic_vr_physics::props::{self};
use gothic_vr_physics::{Prop, hands::HAND_RADIUS};

use crate::actor::{ActorRig, PartOptions, spawn_body, spawn_part};
use crate::ardea::Ardea;
use crate::combat::GothicCombat;
use crate::locomotion::Physics;
use crate::qmat::QuestMaterial;
use crate::retarget::Retarget;

#[derive(Component)]
pub struct OrcVisual {
    pub npc: Entity,
    pub rig: ActorRig,
    pub rt: Retarget,
    pub gait: crate::anim::Gait,
    pub hit: crate::anim::OneShot,
    pub idle_t: f32,
}

/// Original orc clips (walk/run by velocity, stumble reactions on hits), shared by all orcs of the same actor.
#[derive(Resource, Default)]
pub struct OrcClips(pub Option<crate::anim::ClipSet>);

/// The orc rig is posed at its own size (bind height 196 cm): no extra scale (was 1.12, compounding the squash).
const ORC_SCALE: f32 = 1.0;

pub struct OrcsPlugin;

impl Plugin for OrcsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OrcClips>().add_systems(
            Update,
            (
                spawn_orcs.run_if(resource_exists::<Ardea>),
                drive_orcs,
                npc_prof,
            ),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_orcs(
    mut commands: Commands,
    mut done: Local<bool>,
    mut clips_loaded: Local<bool>,
    mut a: ResMut<Ardea>,
    phys: Res<Physics>,
    player: Res<VrPlayer>,
    world: Res<gothic_vr::VrWorld>,
    mut gc: Option<ResMut<GothicCombat>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut qmats: ResMut<Assets<QuestMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut inv: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    if *done || !phys.ready {
        return;
    }
    *done = true;
    let Some(gc) = gc.as_mut() else {
        warn!("[orcs] no gamedata (items.json/orcs.json) in the pack: no orcs");
        return;
    };
    let n: usize = gothic_xr_quest::knob("orcs")
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    let feet = player.ctrl.position / 100.0;
    // Orcs wait on the far side of the plaza, facing the player's start.
    for i in 0..n {
        let ang = (i as f32 - (n as f32 - 1.0) * 0.5) * 0.5;
        let p = feet + Quat::from_rotation_y(ang) * Vec3::new(0.0, 0.0, -14.0);
        let g = world
            .0
            .ground_below(Vec3::new(p.x, p.y + 3.0, p.z) * 100.0, 1500.0)
            .map_or(feet.y, |y| y / 100.0);
        let pos = Vec3::new(p.x, g, p.z);
        let yaw = (p.x - feet.x).atan2(p.z - feet.z); // face the player (-Z forward convention of vr-npc)
        let npc = spawn_npc(
            &mut commands,
            pos,
            yaw,
            Some(&props::two_handed_sword()),
            1000 + i as u64,
        );
        gc.new_orc(npc);
        let root = commands
            .spawn((
                Transform::from_scale(Vec3::splat(0.01 * ORC_SCALE)),
                Visibility::default(),
            ))
            .id();
        let opts = PartOptions {
            cutout: false,
            hide_from_eyes: false,
        };
        let Some(rig) = spawn_body(
            &mut commands,
            &mut a,
            "g3_orc_body_warrior.xact",
            root,
            opts,
            &mut meshes,
            &mut qmats,
            &mut images,
            &mut inv,
        ) else {
            warn!("[orcs] orc body actor missing in the pack");
            commands.entity(root).despawn();
            return;
        };
        let _ = spawn_part(
            &mut commands,
            &mut a,
            "g3_orc_head_head01.xact",
            &rig,
            PartOptions {
                cutout: true,
                hide_from_eyes: false,
            },
            &mut meshes,
            &mut qmats,
            &mut images,
            &mut inv,
        );
        // The weapon is attached the way the original does it (gCInventorySlot bone `Slot_RightHand_Weapon` of the
        // skeleton, identity offset, mesh mirrored like the actors): no extra rotation.
        match rig.by_name.get("Slot_RightHand_Weapon") {
            Some(&si) => {
                let ok = a.spawn_static_mirrored(
                    &mut commands,
                    "g3_weapon_orc_halberd_01.xcmsh",
                    rig.joints[si],
                    Transform::IDENTITY,
                    &mut meshes,
                    &mut qmats,
                    &mut images,
                );
                if !ok {
                    warn!("[orcs] weapon mesh missing");
                }
            }
            None => warn!("[orcs] orc skeleton has no Slot_RightHand_Weapon"),
        }
        // NOTE: the vendored vr-npc had `foot_lift` (raise the ragdoll ground for the orc's taller boots); the workspace
        // crate does not, so orcs stand slightly sunk until vr-npc grows an equivalent.
        let mut rt = Retarget::new(&rig, "Orc");
        rt.keep_bind_lengths = true;
        if !*clips_loaded {
            *clips_loaded = true;
            let set = crate::hero::ardea_clips_path("clips_orc.grcp")
                .and_then(|p| crate::anim::ClipSet::load(&p, &rig, "Orc"));
            info!(
                "[orcs] clip set: {}",
                if set.is_some() {
                    "loaded"
                } else {
                    "missing (ragdoll only)"
                }
            );
            commands.insert_resource(OrcClips(set));
        }
        commands.spawn((
            OrcVisual {
                npc,
                rig,
                rt,
                gait: Default::default(),
                hit: Default::default(),
                idle_t: 0.0,
            },
            Transform::IDENTITY,
        ));
    }
    info!("[orcs] spawned {n} orc raiders");
    let _ = HAND_RADIUS;
}

fn drive_orcs(
    mut orcs: Query<&mut OrcVisual>,
    npcs: Query<&VrNpc>,
    weapons: Query<&Transform, With<Prop>>,
    clips: Res<OrcClips>,
    time: Res<Time>,
    mut hits: MessageReader<gothic_vr_melee::HitMessage>,
    mut joints: Query<&mut Transform, (Without<OrcVisual>, Without<Prop>, Without<VrNpc>)>,
) {
    let dt = time.delta_secs();
    let hits: Vec<_> = hits.read().cloned().collect();
    for mut o in &mut orcs {
        let Ok(n) = npcs.get(o.npc) else { continue };
        let pose = n.ragdoll.pose();
        // The physical weapon prop's frame is the OpenXR grip frame (blade -Z = little finger -> thumb), so its
        // rotation drives the right hand through the same analytic hand mapping as the player's controllers: the
        // visible weapon (attached to Slot_RightHand_Weapon, identity) then points where the collision prop does.
        let grip = n
            .weapon
            .and_then(|w| weapons.get(w).ok())
            .map(|t| t.rotation);
        let mut ov = None;
        if let Some(set) = &clips.0 {
            let yaw = Quat::from_rotation_y(pose.body_yaw);
            // Hit reaction: original stumble clip over the whole body (the ragdoll still gets the physical impulse).
            let me = o.npc;
            for h in hits.iter().filter(|h| h.victim == me) {
                let right = yaw * Vec3::X;
                let needle = if matches!(h.bone, gothic_vr_melee::Bone::Head) {
                    "quickstumble_hit"
                } else if h.direction.dot(right) > 0.0 {
                    "stumblel_hit"
                } else {
                    "stumbler_hit"
                };
                o.hit.start(set, needle);
            }
            let standing = n.ragdoll.stance() == gothic_vr_npc::ragdoll::Stance::Standing;
            let (gw, layers) = if standing {
                o.gait.update(set, n.ai.root, pose.body_yaw, dt)
            } else {
                (0.0, vec![])
            };
            let shot = o.hit.update(dt);
            let n_nodes = o.rig.names.len();
            let mut rot = vec![None; n_nodes];
            let mut weight = vec![0.0f32; n_nodes];
            let mut any = false;
            if standing {
                // Base: the original idle clip over the whole body (the orc holds its halberd with both hands, like on
                // the desktop); the ragdoll only supplies the root position and the physical reactions.
                o.idle_t += dt;
                if let Some(idle) = set.find("ambient_loop") {
                    rot = set.sample_world(&[(idle.as_str(), o.idle_t, 1.0)], yaw);
                    for (i, r) in rot.iter().enumerate() {
                        if r.is_some() {
                            weight[i] = 1.0;
                        }
                    }
                    any = true;
                }
                if gw > 0.0 && !layers.is_empty() {
                    let refs: Vec<(&str, f32, f32)> = layers
                        .iter()
                        .map(|(k, t, w)| (k.as_str(), *t, *w))
                        .collect();
                    let walk = set.sample_world(&refs, yaw);
                    for side in ["Left", "Right"] {
                        for part in [
                            "Leg_Hip_1",
                            "Leg_Leg_1",
                            "Leg_Leg_2",
                            "Foot_Foot_1",
                            "Foot_Toe_1",
                        ] {
                            if let Some(&i) = o.rig.by_name.get(&format!("Orc_{side}_{part}")) {
                                if let (Some(w), Some(base)) = (walk[i], rot[i]) {
                                    rot[i] = Some(base.slerp(w, gw));
                                }
                            }
                        }
                    }
                }
            }
            if let Some((k, t, w)) = shot {
                rot = set.sample_world(&[(k.as_str(), t, 1.0)], yaw);
                for (i, r) in rot.iter().enumerate() {
                    if r.is_some() {
                        weight[i] = w;
                    }
                }
                any = true;
            }
            if any {
                ov = Some(crate::anim::PoseOverride { rot, weight });
            }
        }
        // Always stand the orc's own (taller) legs on the ground: the human-sized ragdoll pelvis is lower.
        let ground = Some(n.ai.root.y);
        o.rt.apply(
            &o.rig,
            &mut joints,
            &pose,
            None,
            [None, if ov.is_some() { None } else { grip }],
            ORC_SCALE,
            None,
            [[0.0; 2], [1.0, 0.7]],
            ov.as_ref(),
            ground,
        );
    }
}

/// Once per second: where the NPC fixed step spends its time (ms per second of wall time).
fn npc_prof(time: Res<Time<Real>>, mut last: Local<f32>) {
    if time.elapsed_secs() - *last < 1.0 {
        return;
    }
    *last = time.elapsed_secs();
    info!("[npc-prof] unavailable in workspace vr-npc");
}
