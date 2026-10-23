//! Host-side screenshots of the Quest avatar code (needs the local Ardea pack and a GPU): renders the hero with the same
//! actor, retarget, holster and material code as the headset build to `.local/quest/host_*.png`.
//!   1. host_fpv.png          first person, looking down at chest, hands and the left-hip sword
//!   2. host_third_front.png  third person, front three-quarter
//!   3. host_third_back.png   third person from behind (sword scabbard, bow, quiver)
//!   4. host_orc.png          orc close-up with its weapon
use bevy::asset::AssetPlugin;
use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::RunSystemOnce;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use gothic_quest::actor::{ActorRig, PartOptions, spawn_body, spawn_part};
use gothic_quest::ardea::{self, Ardea};
use gothic_quest::qmat::{QuestMaterial, QuestMaterialPlugin};
use gothic_quest::retarget::Retarget;
use gothic_quest::weapons::holster_zone;
use gothic_vr::Pose;
use gothic_vr::body::{BodyIk, Proportions};
use gothic_vr_npc::animator::NpcAnimator;
use gothic_vr_npc::ragdoll::ActiveRagdoll;

#[derive(Component)]
struct ShotCam;
#[derive(Component)]
struct HeldSword;

struct Built {
    hero: ActorRig,
    hero_rt: Retarget,
    orc: ActorRig,
    orc_rt: Retarget,
}

fn main() {
    let dir =
        ardea::data_dir().expect("run from the repo root with .local/quest/ardea-pack present");
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                ..default()
            })
            .set(AssetPlugin::default())
            .disable::<bevy::log::LogPlugin>(),
    );
    app.add_plugins(QuestMaterialPlugin);
    app.insert_resource(gothic_xr_quest::XrFrameData::default());
    ardea::build(&mut app, dir).expect("ardea build");
    app.insert_resource(ClearColor(Color::srgb(0.55, 0.64, 0.76)));
    app.finish();
    app.cleanup();
    for _ in 0..3 {
        app.update();
    }
    let out = std::path::PathBuf::from(".local/quest");
    let size = Extent3d {
        width: 1000,
        height: 1000,
        depth_or_array_layers: 1,
    };
    let mut img = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::default(),
    );
    img.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
        | TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC
        | TextureUsages::RENDER_ATTACHMENT;
    let handle = app.world_mut().resource_mut::<Assets<Image>>().add(img);
    app.world_mut().spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgb(0.55, 0.64, 0.76)),
            ..default()
        },
        RenderTarget::Image(handle.clone().into()),
        Projection::Perspective(PerspectiveProjection {
            fov: 1.2,
            near: 0.03,
            ..default()
        }),
        RenderLayers::layer(0),
        ShotCam,
        Transform::IDENTITY,
    ));
    app.world_mut().spawn((
        DirectionalLight {
            illuminance: 9000.0,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.5, 0.0)),
    ));
    app.world_mut().insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.75, 0.8, 1.0),
        brightness: 500.0,
        ..default()
    });

    let built = app.world_mut().run_system_once(build_scene).unwrap();
    let Built {
        hero,
        hero_rt,
        orc,
        orc_rt,
    } = built;

    // Player pose: eye height 1.7 m, controllers held in front in the natural grip (thumb up, palm inward).
    let natural = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    let head = Pose::new(Vec3::new(0.0, 1.7, 0.0), Quat::IDENTITY);
    let lh = Pose::new(Vec3::new(-0.25, 1.15, -0.38), natural);
    let rh = Pose::new(Vec3::new(0.28, 1.1, -0.42), natural);
    let mut ik = BodyIk::new(Proportions::for_eye_height(1.7));
    let mut pose = ik.solve(&head, [Some(lh), Some(rh)], |_, _| 0.0, 0.016);
    for _ in 0..40 {
        pose = ik.solve(&head, [Some(lh), Some(rh)], |_, _| 0.0, 0.016);
    }
    let hero_rt = std::sync::Arc::new(hero_rt);
    let torso0 = (head.position + head.rotation * Vec3::new(0.0, -0.09, 0.09)
        - pose.get(gothic_vr::body::Joint::Pelvis))
    .length();
    let sc_hero = (torso0 / hero_rt.torso_bind_m(&hero).unwrap()).clamp(0.6, 1.6);
    let mut pose3 = pose.clone();
    gothic_quest::hero::place_trunk(&mut pose3, head.position, head.rotation);
    let apply_hero = |app: &mut App, curl: [[f32; 2]; 2]| {
        let rt = hero_rt.clone();
        let mut p3 = pose3.clone();
        // Like the game (hero.rs): the visible palm centre goes to the grip position.
        for (k, joint, h) in [
            (0usize, gothic_vr::body::Joint::HandL, lh),
            (1usize, gothic_vr::body::Joint::HandR, rh),
        ] {
            if let Some(w) = rt.wrist_for_grip(k, h.position, h.rotation, sc_hero) {
                p3.joints[joint.index()] = w;
            }
        }
        let rig_copy = ActorRig {
            root: hero.root,
            by_name: hero.by_name.clone(),
            names: hero.names.clone(),
            parent: hero.parent.clone(),
            bind_local: hero.bind_local.clone(),
            bind_global: hero.bind_global.clone(),
            joints: hero.joints.clone(),
        };
        app.world_mut()
            .run_system_once(
                move |mut q: Query<&mut Transform, (Without<ShotCam>, Without<HeldSword>)>| {
                    rt.apply(
                        &rig_copy,
                        &mut q,
                        &p3,
                        Some(head.rotation),
                        [Some(lh.rotation), Some(rh.rotation)],
                        sc_hero,
                        None,
                        curl,
                        None,
                        None,
                    );
                },
            )
            .unwrap();
    };
    apply_hero(&mut app, [[0.0; 2]; 2]);
    let _ = gothic_quest::hero::ardea_clips_path("clips_hero.grcp").and_then(|p| gothic_quest::anim::ClipSet::load(&p, &hero, "Hero"));
    // Orc pose.
    {
        let rag = ActiveRagdoll::new(&NpcAnimator::standing(
            Vec3::new(0.0, 0.0, -4.0),
            std::f32::consts::PI,
        ));
        let mut opose = rag.pose();
        // Same foot lift as the game: the orc's ankles are taller than the ragdoll's 6 cm.
        let ankle_m = orc.bind_global[orc.by_name["Orc_Left_Foot_Foot_1"]]
            .w_axis
            .y
            * 0.01
            * 1.12;
        let lift = (ankle_m - 0.06 + 0.02).max(0.0);
        println!("orc foot lift {lift:.3} m (bind ankle height {ankle_m:.3} m)");
        for j in opose.joints.iter_mut() {
            j.y += lift;
        }
        println!(
            "ORC pose: pelvis {:?} hipL {:?} kneeL {:?} ankleL {:?}",
            opose.get(gothic_vr::body::Joint::Pelvis),
            opose.get(gothic_vr::body::Joint::HipL),
            opose.get(gothic_vr::body::Joint::KneeL),
            opose.get(gothic_vr::body::Joint::AnkleL)
        );
        let rig_copy = ActorRig {
            root: orc.root,
            by_name: orc.by_name.clone(),
            names: orc.names.clone(),
            parent: orc.parent.clone(),
            bind_local: orc.bind_local.clone(),
            bind_global: orc.bind_global.clone(),
            joints: orc.joints.clone(),
        };
        // Like the game (orcs.rs): the original idle clip drives the whole body, `GR_CLIPS=0` turns it off.
        let ov = gothic_quest::hero::ardea_clips_path("clips_orc.grcp")
            .and_then(|p| gothic_quest::anim::ClipSet::load(&p, &orc, "Orc"))
            .and_then(|set| {
                let idle = set.find("ambient_loop")?;
                let rot = set.sample_world(
                    &[(idle.as_str(), 0.0, 1.0)],
                    Quat::from_rotation_y(opose.body_yaw),
                );
                let weight = rot.iter().map(|r| r.is_some() as u32 as f32).collect();
                Some(gothic_quest::anim::PoseOverride { rot, weight })
            });
        println!("orc clip override: {}", ov.is_some());
        app.world_mut()
            .run_system_once(
                move |mut q: Query<&mut Transform, (Without<ShotCam>, Without<HeldSword>)>| {
                    orc_rt.apply(
                        &rig_copy,
                        &mut q,
                        &opose,
                        None,
                        [None, Some(natural)],
                        1.12,
                        None,
                        [[0.0; 2], [1.0, 0.7]],
                        ov.as_ref(),
                        None,
                    );
                },
            )
            .unwrap();
    }

    // Debug: lowest posed orc vertex and the lowest ones influenced by the foot bones.
    {
        app.update();
        app.update();
        let data = app
            .world()
            .resource::<Ardea>()
            .cache()
            .load_actor(gothic_cook::AssetId::of("g3_orc_body_warrior.xact"))
            .unwrap();
        let verts = data.vertices().to_vec();
        let subs = data.submeshes().to_vec();
        let orc2 = ActorRig {
            root: orc.root,
            by_name: orc.by_name.clone(),
            names: orc.names.clone(),
            parent: orc.parent.clone(),
            bind_local: orc.bind_local.clone(),
            bind_global: orc.bind_global.clone(),
            joints: orc.joints.clone(),
        };
        app.world_mut()
            .run_system_once(move |g: Query<&GlobalTransform>| {
                let mats: Vec<Mat4> = (0..orc2.joints.len()).map(|k| g.get(orc2.joints[k]).unwrap().to_matrix() * orc2.bind_global[k].inverse()).collect();
                let foot: Vec<usize> = orc2.names.iter().enumerate().filter(|(_, n)| n.contains("Foot") || n.contains("Toe")).map(|(i, _)| i).collect();
                let (mut min_all, mut min_foot, mut n_foot) = (f32::MAX, f32::MAX, 0);
                for sub in &subs {
                    for v in &verts[sub.first_vertex as usize..(sub.first_vertex + sub.vertex_count) as usize] {
                        let local = Vec3::new(-v.pos[0], v.pos[1], v.pos[2]);
                        let mut acc = Vec3::ZERO;
                        let mut foot_w = 0.0;
                        for k in 0..4 {
                            let wt = v.weights[k] as f32 / 255.0;
                            if wt > 0.0 {
                                acc += mats[v.joints[k] as usize].transform_point3(local) * wt;
                                if foot.contains(&(v.joints[k] as usize)) {
                                    foot_w += wt;
                                }
                            }
                        }
                        min_all = min_all.min(acc.y);
                        if foot_w > 0.5 {
                            min_foot = min_foot.min(acc.y);
                            n_foot += 1;
                        }
                    }
                }
                println!("ORC posed mesh: lowest vertex y {min_all:.3} m; vertices mostly bound to foot/toe bones: {n_foot}, lowest {min_foot:.3} m");
                for n in ["Orc_Left_Foot_Foot_1", "Orc_Left_Leg_Leg_2", "Orc_Left_Leg_Leg_1", "Orc_Left_Leg_Hip_1", "Orc_Spine_Spine_1"] {
                    println!("ORC node {n}: {:?}", g.get(orc2.joints[orc2.by_name[n]]).unwrap().translation());
                }
            })
            .unwrap();
    }
    let shots: [(&str, Vec3, Vec3); 6] = [
        (
            "host_fpv.png",
            Vec3::new(0.0, 1.7, 0.0),
            Vec3::new(0.0, 0.7, -0.55),
        ),
        (
            "host_third_front.png",
            Vec3::new(1.6, 1.5, -2.3),
            Vec3::new(-0.05, 1.0, 0.0),
        ),
        (
            "host_third_back.png",
            Vec3::new(-1.3, 1.6, 2.1),
            Vec3::new(-0.05, 1.1, 0.1),
        ),
        (
            "host_orc.png",
            Vec3::new(-1.7, 1.2, -1.9),
            Vec3::new(0.0, 0.9, -4.0),
        ),
        (
            // Over the player's shoulder: the orc (z = -4, facing +Z) must look at the player.
            "host_orc_player.png",
            Vec3::new(1.1, 1.8, 1.6),
            Vec3::new(-0.2, 1.1, -2.6),
        ),
        (
            "host_fpv_grip.png",
            Vec3::new(0.0, 1.7, 0.0),
            Vec3::new(0.15, 1.15, -0.65),
        ),
    ];
    for (file, from, to) in shots {
        if file == "host_fpv_grip.png" {
            // Fingers curled (grip = trigger = 1) around the sword held in the right hand.
            apply_hero(&mut app, [[1.0, 1.0], [1.0, 1.0]]);
            let mut hq = app
                .world_mut()
                .query_filtered::<&mut Transform, With<HeldSword>>();
            for mut t in hq.iter_mut(app.world_mut()) {
                *t = Transform::from_translation(rh.position).with_rotation(rh.rotation);
            }
        }
        let mut tf = Transform::from_translation(from).looking_at(to, Vec3::Y);
        if file == "host_fpv.png" {
            tf = Transform::from_translation(from).looking_at(to, Vec3::Y);
        }
        {
            let mut q = app
                .world_mut()
                .query_filtered::<&mut Transform, With<ShotCam>>();
            for mut t in q.iter_mut(app.world_mut()) {
                *t = tf;
            }
        }
        for _ in 0..if file == "host_fpv.png" { 60 } else { 6 } {
            app.update();
        }
        let path = out.join(file);
        app.world_mut()
            .spawn(Screenshot::image(handle.clone()))
            .observe(save_to_disk(path.clone()));
        for _ in 0..8 {
            app.update();
        }
        println!("saved {}", path.display());
    }
    for _ in 0..10 {
        app.update();
    }
}

fn build_scene(
    mut commands: Commands,
    mut a: ResMut<Ardea>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut qmats: ResMut<Assets<QuestMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut inv: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
) -> Built {
    // Ground.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(40.0, 40.0))),
        MeshMaterial3d(std_mats.add(StandardMaterial {
            base_color: Color::srgb(0.32, 0.38, 0.22),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::IDENTITY,
    ));
    // Hero.
    let root = commands
        .spawn((
            Transform::from_scale(Vec3::splat(0.01)),
            Visibility::default(),
        ))
        .id();
    let hero = spawn_body(
        &mut commands,
        &mut a,
        "g3_hero_body_player.xact",
        root,
        PartOptions {
            cutout: false,
            hide_from_eyes: false,
        },
        &mut meshes,
        &mut qmats,
        &mut images,
        &mut inv,
    )
    .expect("hero body");
    spawn_part(
        &mut commands,
        &mut a,
        "g3_head_hero_hero_01.xact",
        &hero,
        PartOptions {
            cutout: true,
            hide_from_eyes: true,
        },
        &mut meshes,
        &mut qmats,
        &mut images,
        &mut inv,
    );
    let hero_rt = Retarget::new(&hero, "Hero");
    // Holstered weapons on the body-relative zones (same function as the headset build).
    let sword_vis = a
        .cache()
        .load_mesh(gothic_cook::AssetId::of(
            "g3_weapon_1h_sword_orcslayer_01.xcmsh",
        ))
        .ok()
        .and_then(|md| gothic_quest::grip::frame_for("g3_weapon_1h_sword_orcslayer_01.xcmsh", &md))
        .map(|g| g.visual(0.01))
        .expect("sword grip");
    let (sw, _) = holster_zone(Vec3::ZERO, 1.7, 0.0, "Slot_MeleeBackRight");
    let sword = commands.spawn((sw, Visibility::default())).id();
    a.spawn_static_mirrored(
        &mut commands,
        "g3_weapon_1h_sword_orcslayer_01.xcmsh",
        sword,
        sword_vis,
        &mut meshes,
        &mut qmats,
        &mut images,
    );
    // The same sword held in the right hand (moved there for the last shot).
    let held = commands
        .spawn((
            HeldSword,
            Transform::from_translation(Vec3::new(0.0, -100.0, 0.0)),
            Visibility::default(),
        ))
        .id();
    a.spawn_static_mirrored(
        &mut commands,
        "g3_weapon_1h_sword_orcslayer_01.xcmsh",
        held,
        sword_vis,
        &mut meshes,
        &mut qmats,
        &mut images,
    );
    let bow_rot = Quat::from_mat3(&Mat3::from_cols(Vec3::Z, Vec3::X, Vec3::Y));
    let (bt, _) = holster_zone(Vec3::ZERO, 1.7, 0.0, "Slot_Bow");
    let bow = commands.spawn((bt, Visibility::default())).id();
    let holder = commands
        .spawn((
            Transform::from_rotation(bow_rot).with_scale(Vec3::splat(0.01)),
            Visibility::default(),
            ChildOf(bow),
        ))
        .id();
    let bowroot = commands
        .spawn((Transform::IDENTITY, Visibility::default(), ChildOf(holder)))
        .id();
    let _ = spawn_body(
        &mut commands,
        &mut a,
        "it_weapon_ranged_bow_hunting_01.xact",
        bowroot,
        PartOptions {
            cutout: false,
            hide_from_eyes: false,
        },
        &mut meshes,
        &mut qmats,
        &mut images,
        &mut inv,
    );
    let quiver = gothic_quest::weapons::spawn_quiver(
        &mut commands,
        &mut a,
        &mut meshes,
        &mut qmats,
        &mut images,
        &mut std_mats,
    );
    let head_pose = gothic_vr::Pose::new(Vec3::new(0.0, 1.7, 0.0), Quat::IDENTITY);
    let qc = gothic_vr_archery::bow::Quiver {
        offset: Vec3::new(0.12, -0.12, 0.16),
        radius: 0.26,
    }
    .center(&head_pose, gothic_vr::Hand::Right);
    commands.entity(quiver).insert(
        Transform::from_translation(qc - Vec3::Y * 0.1).with_rotation(Quat::from_rotation_z(-0.2)),
    );
    // Orc.
    let oroot = commands
        .spawn((
            Transform::from_scale(Vec3::splat(0.01 * 1.12)),
            Visibility::default(),
        ))
        .id();
    let orc = spawn_body(
        &mut commands,
        &mut a,
        "g3_orc_body_warrior.xact",
        oroot,
        PartOptions {
            cutout: false,
            hide_from_eyes: false,
        },
        &mut meshes,
        &mut qmats,
        &mut images,
        &mut inv,
    )
    .expect("orc body");
    spawn_part(
        &mut commands,
        &mut a,
        "g3_orc_head_head01.xact",
        &orc,
        PartOptions {
            cutout: true,
            hide_from_eyes: false,
        },
        &mut meshes,
        &mut qmats,
        &mut images,
        &mut inv,
    );
    if let Some(&si) = orc.by_name.get("Slot_RightHand_Weapon") {
        a.spawn_static(
            &mut commands,
            "g3_weapon_1h_sword_orcslayer_01.xcmsh",
            orc.joints[si],
            Transform::IDENTITY,
            &mut meshes,
            &mut qmats,
            &mut images,
        );
    }
    let orc_rt = Retarget::new(&orc, "Orc");
    Built {
        hero,
        hero_rt,
        orc,
        orc_rt,
    }
}
