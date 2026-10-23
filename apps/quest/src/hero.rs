//! The nameless hero as the player's avatar: real `G3_Hero_Body_Player` + `G3_Head_Hero_Hero_01` actors from the
//! cooked cache (mirrored like the world), driven by the cloud full-body IK (`VrBody`) through the generic
//! retargeter. The head mesh lives on render layer 1 only, so the player's own eyes never see inside it.
use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use gothic_vr::body::Joint;
use gothic_vr::{VrBody, VrSet, XrHand, XrHead};

use crate::actor::{ActorRig, PartOptions, spawn_body, spawn_part};
use crate::ardea::Ardea;
use crate::qmat::QuestMaterial;
use crate::retarget::Retarget;

#[derive(Resource)]
pub struct Hero {
    pub rig: ActorRig,
    pub rt: Retarget,
}

/// Original locomotion clips of the hero (legs while walking; the upper body stays IK-driven).
#[derive(Resource, Default)]
pub struct HeroAnim {
    pub set: Option<crate::anim::ClipSet>,
    pub gait: crate::anim::Gait,
}

#[derive(Component)]
pub struct HeroRoot;

pub struct HeroPlugin;

impl Plugin for HeroPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                spawn_hero
                    .run_if(not(resource_exists::<Hero>))
                    .run_if(resource_exists::<Ardea>),
                drive_hero.after(VrSet::Body),
            )
                .chain(),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_hero(
    mut commands: Commands,
    mut a: ResMut<Ardea>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut qmats: ResMut<Assets<QuestMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut inv: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
) {
    let root = commands
        .spawn((
            HeroRoot,
            Transform::from_scale(Vec3::splat(0.01)),
            Visibility::default(),
        ))
        .id();
    let body = PartOptions {
        cutout: false,
        hide_from_eyes: false,
    };
    let Some(rig) = spawn_body(
        &mut commands,
        &mut a,
        "g3_hero_body_player.xact",
        root,
        body,
        &mut meshes,
        &mut qmats,
        &mut images,
        &mut inv,
    ) else {
        error!("hero body actor missing in the data pack");
        commands.entity(root).despawn();
        // Stop retrying every frame.
        commands.insert_resource(Hero {
            rig: ActorRig::empty(),
            rt: Retarget::empty(),
        });
        return;
    };
    let head = PartOptions {
        cutout: true,
        hide_from_eyes: true,
    };
    if spawn_part(
        &mut commands,
        &mut a,
        "g3_head_hero_hero_01.xact",
        &rig,
        head,
        &mut meshes,
        &mut qmats,
        &mut images,
        &mut inv,
    )
    .is_none()
    {
        warn!("hero head actor missing in the data pack");
    }
    let rt = Retarget::new(&rig, "Hero");
    info!("[hero] avatar spawned: {} nodes", rig.names.len());
    if gothic_xr_quest::knob("mirror").as_deref() == Some("1") {
        spawn_mirror(&mut commands, &mut images, &mut meshes, &mut std_mats);
    }
    let clips = ardea_clips_path("clips_hero.grcp");
    let set = clips.and_then(|p| crate::anim::ClipSet::load(&p, &rig, "Hero"));
    info!(
        "[hero] clip set: {}",
        if set.is_some() {
            "loaded"
        } else {
            "missing (IK legs only)"
        }
    );
    commands.insert_resource(HeroAnim {
        set,
        gait: Default::default(),
    });
    commands.insert_resource(Hero { rig, rt });
}

pub fn ardea_clips_path(name: &str) -> Option<std::path::PathBuf> {
    // Original clip playback (hero legs, orc body): `setprop debug.gr.clips 0` turns it off.
    if gothic_xr_quest::knob("clips").as_deref() == Some("0") {
        return None;
    }
    let p = crate::ardea::data_dir()?.join(name);
    p.exists().then_some(p)
}

/// A render-to-texture "mirror": a camera in front of the player looking back at the avatar, shown on a quad.
fn spawn_mirror(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
) {
    let size = Extent3d {
        width: 512,
        height: 512,
        depth_or_array_layers: 1,
    };
    let mut img = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    img.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let handle = images.add(img);
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: -1,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.3, 0.35, 0.45)),
            ..default()
        },
        RenderTarget::Image(handle.clone().into()),
        RenderLayers::layer(0).with(1),
        bevy::core_pipeline::tonemapping::Tonemapping::None,
        MirrorCam,
        Transform::IDENTITY,
    ));
    commands.spawn((
        Mesh3d(meshes.add(Rectangle::new(1.2, 1.2))),
        MeshMaterial3d(mats.add(StandardMaterial {
            base_color_texture: Some(handle),
            unlit: true,
            ..default()
        })),
        MirrorQuad,
        Transform::IDENTITY,
    ));
}

#[derive(Component)]
struct MirrorCam;
#[derive(Component)]
struct MirrorQuad;

#[allow(clippy::type_complexity)]
fn drive_hero(
    frame: Res<gothic_xr_quest::XrFrameData>,
    hero: Option<Res<Hero>>,
    mut anim: Option<ResMut<HeroAnim>>,
    time: Res<Time>,
    body: Res<VrBody>,
    head: Query<&Transform, (With<XrHead>, Without<XrHand>)>,
    hands: Query<(&Transform, &XrHand), Without<XrHead>>,
    phys_hands: Query<
        (&Transform, &gothic_vr_physics::PhysicsHand),
        (Without<XrHand>, Without<XrHead>),
    >,
    mut joints: Query<
        &mut Transform,
        (
            Without<XrHand>,
            Without<XrHead>,
            Without<MirrorCam>,
            Without<MirrorQuad>,
            Without<gothic_vr_physics::PhysicsHand>,
        ),
    >,
    mut mirror: Query<
        &mut Transform,
        (
            With<MirrorCam>,
            Without<MirrorQuad>,
            Without<XrHand>,
            Without<XrHead>,
            Without<gothic_vr_physics::PhysicsHand>,
        ),
    >,
    mut quad: Query<
        &mut Transform,
        (
            With<MirrorQuad>,
            Without<MirrorCam>,
            Without<XrHand>,
            Without<XrHead>,
            Without<gothic_vr_physics::PhysicsHand>,
        ),
    >,
) {
    let Some(hero) = hero else { return };
    let Some(pose) = &body.pose else { return };
    let mut pose = pose.clone();
    if hero.rig.joints.is_empty() {
        return;
    }
    let head_rot = head.iter().next().map(|t| t.rotation);
    // Hand targets: the controller's grip pose, or - while a weapon is held - the physics hand that carries it, so the
    // visible fist and the weapon (jointed to the physics hand) always coincide.
    let mut hand_rot = [None; 2];
    let mut hand_pos = [None; 2];
    for (t, h) in &hands {
        if h.tracked {
            hand_rot[h.hand.index()] = Some(t.rotation);
            hand_pos[h.hand.index()] = Some(t.translation);
        }
    }
    for (t, ph) in &phys_hands {
        if ph.held.is_some() && t.translation.is_finite() {
            hand_rot[ph.hand.index()] = Some(t.rotation);
            hand_pos[ph.hand.index()] = Some(t.translation);
        }
    }
    // Proportions: scale the avatar so its pelvis-to-head distance equals the IK's (the player's), then rigidly move it
    // so the head bone sits just below/behind the eye point (eye = tracked head pose; the bone is the skull base,
    // ~9 cm below and behind the eyes). The head mesh itself is on render layer 1 (hidden from the player's eyes).
    // Proportions come from the untouched IK pose (eye to pelvis).
    let head_p0 = pose.get(gothic_vr::body::Joint::Head);
    let anchor0 = head_rot.map_or(head_p0, |r| head_p0 + r * Vec3::new(0.0, -0.09, 0.09));
    let torso0 = (anchor0 - pose.get(gothic_vr::body::Joint::Pelvis)).length();
    if let Some(r) = head_rot {
        place_trunk(&mut pose, head_p0, r);
    }
    let pose = &pose;
    let scale = hero
        .rt
        .torso_bind_m(&hero.rig)
        .map_or(1.0, |b| (torso0 / b).clamp(0.6, 1.6));
    let curl = [
        [frame.hands[0].squeeze, frame.hands[0].trigger],
        [frame.hands[1].squeeze, frame.hands[1].trigger],
    ];
    // Legs: original walk/run clips by body-local velocity once the player moves (IK legs when standing).
    let mut ov = None;
    if let Some(a) = anim.as_deref_mut()
        && let Some(set) = &a.set
    {
        let pos = pose.get(gothic_vr::body::Joint::Pelvis);
        let (w, layers) = a.gait.update(set, pos, pose.body_yaw, time.delta_secs());
        if w > 0.0 && !layers.is_empty() {
            let refs: Vec<(&str, f32, f32)> = layers
                .iter()
                .map(|(k, t, wt)| (k.as_str(), *t, *wt))
                .collect();
            let rot = set.sample_world(&refs, Quat::from_rotation_y(pose.body_yaw));
            let mut weight = vec![0.0; rot.len()];
            for side in ["Left", "Right"] {
                for part in [
                    "Leg_Hip_1",
                    "Leg_Leg_1",
                    "Leg_Leg_2",
                    "Foot_Foot_1",
                    "Foot_Toe_1",
                ] {
                    if let Some(&i) = hero.rig.by_name.get(&format!("Hero_{side}_{part}")) {
                        weight[i] = w;
                    }
                }
            }
            ov = Some(crate::anim::PoseOverride { rot, weight });
        }
    }
    // The visible palm centre goes to the grip position: move the IK wrist target back by the palm offset.
    let mut pose2 = pose.clone();
    for (k, joint) in [(0usize, Joint::HandL), (1usize, Joint::HandR)] {
        if let (Some(g), Some(p)) = (hand_rot[k], hand_pos[k])
            && let Some(w) = hero.rt.wrist_for_grip(k, p, g, scale)
        {
            pose2.joints[joint.index()] = w;
        }
    }
    hero.rt.apply(
        &hero.rig,
        &mut joints,
        &pose2,
        head_rot,
        hand_rot,
        scale,
        None,
        curl,
        ov.as_ref(),
        None,
    );
    // Mirror: 2.2 m in front of the avatar (its facing), camera looks back at it.
    let fwd = Quat::from_rotation_y(pose.body_yaw) * Vec3::NEG_Z;
    let base = pose.get(gothic_vr::body::Joint::Pelvis);
    if let Ok(mut c) = mirror.single_mut() {
        let at = Vec3::new(base.x, base.y + 0.5, base.z) + fwd * 2.6;
        *c = Transform::from_translation(at)
            .looking_at(Vec3::new(base.x, base.y + 0.45, base.z), Vec3::Y);
    }
    if let Ok(mut q) = quad.single_mut() {
        let at = Vec3::new(base.x, 1.5, base.z)
            + fwd * 2.2
            + Quat::from_rotation_y(pose.body_yaw) * Vec3::new(1.6, 0.0, 0.0);
        *q = Transform::from_translation(at).looking_at(Vec3::new(base.x, 1.5, base.z), Vec3::Y);
        q.rotate_y(std::f32::consts::PI);
    }
}

/// Eye to neck pivot in the head frame (down, back), head-centre offset of ~8-10 cm like Blade & Sorcery style bodies.
pub const EYE_TO_NECK: Vec3 = Vec3::new(0.0, -0.12, 0.09);
/// How far the trunk is pushed behind the neck pivot (m) so the chest front stays clear of the eyes when looking down.
pub const TORSO_BACK: f32 = 0.12;

/// Hangs the trunk from the neck pivot: the pivot follows the full head pose, the trunk keeps its yaw-only orientation
/// (gravity), shifted back along the torso heading. Hands keep their IK targets (the arms stretch).
pub fn place_trunk(pose: &mut gothic_vr::body::BodyPose, eye: Vec3, head_rot: Quat) {
    let fwd = Quat::from_rotation_y(pose.body_yaw) * Vec3::NEG_Z;
    let pivot = eye + head_rot * EYE_TO_NECK - fwd * TORSO_BACK;
    let delta = pivot - pose.get(Joint::Neck);
    for j in [
        Joint::Pelvis,
        Joint::Spine,
        Joint::Chest,
        Joint::Neck,
        Joint::ShoulderL,
        Joint::ShoulderR,
    ] {
        pose.joints[j.index()] += delta;
    }
}
