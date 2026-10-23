//! Bow lab (host, needs the local Ardea pack and a GPU): the hero holds the original bow in the left hand and draws
//! it with the right, rendered from several views at rest and at full draw into `.local/quest/griplab/bow_*.png`.
//! Uses the same `BowDraw` limb/string animation, grip frame (`props::bow()` grip rotation) and nock geometry
//! (`Archer::aim`) as the headset build.
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
use gothic_quest::weapons::BowDraw;
use gothic_vr::Pose;
use gothic_vr::body::{BodyIk, Proportions};

#[derive(Component)]
struct ShotCam;
#[derive(Component)]
struct BowProp;
#[derive(Component)]
struct ArrowVis;

struct Built {
    hero: ActorRig,
    hero_rt: Retarget,
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
    let out = std::path::PathBuf::from(".local/quest/griplab");
    std::fs::create_dir_all(&out).unwrap();
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

    let Built { hero, hero_rt } = app.world_mut().run_system_once(build_scene).unwrap();
    let archer = gothic_vr_archery::bow::Archer::default();
    let spec = gothic_vr_physics::props::bow();
    let grip_rot = spec.grips[0].rotation;
    // Natural grip: fingers forward, thumb up (see `retarget::grip_anatomy`).
    let natural = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    let head = Pose::new(Vec3::new(0.0, 1.7, 0.0), Quat::IDENTITY);
    // Full draw anchors at the right cheek; the bow points a little to the left of straight ahead.
    let anchor = Vec3::new(0.10, 1.57, 0.14);
    let aim = Quat::from_rotation_y(0.26) * Quat::from_rotation_x(-0.03);
    let reach = archer.bow.string_rest.z + archer.bow.max_draw;
    let bow_pose = Pose::new(anchor + aim * Vec3::NEG_Z * reach, aim);
    let lh = Pose::new(bow_pose.position, bow_pose.rotation * grip_rot);
    let _ = natural;
    let hero_rt = std::sync::Arc::new(hero_rt);
    let views: [(&str, Vec3, Vec3); 5] = [
        ("fpv", Vec3::new(0.0, 1.7, 0.0), Vec3::new(-0.15, 1.5, -0.7)),
        ("left", Vec3::new(-1.6, 1.5, -0.2), Vec3::new(0.0, 1.45, -0.2)),
        ("right", Vec3::new(1.6, 1.5, -0.2), Vec3::new(0.0, 1.45, -0.2)),
        ("front", Vec3::new(-0.3, 1.5, -2.2), Vec3::new(0.0, 1.45, 0.0)),
        ("top", Vec3::new(-0.1, 3.0, -0.1), Vec3::new(0.0, 1.4, -0.1)),
    ];
    for (state, draw) in [("rest", 0.0f32), ("draw", archer.bow.max_draw)] {
        // Drawing hand on the string at this draw (straight back from the nock).
        let nock =
            bow_pose.transform_point(archer.bow.string_rest) + bow_pose.rotation * Vec3::Z * draw;
        let rh = Pose::new(
            if draw > 0.0 {
                nock
            } else {
                Vec3::new(0.2, 1.2, -0.25)
            },
            bow_pose.rotation * Quat::from_rotation_y(-1.2) * grip_rot,
        );
        let mut ik = BodyIk::new(Proportions::for_eye_height(1.7));
        let mut pose = ik.solve(&head, [Some(lh), Some(rh)], |_, _| 0.0, 0.016);
        for _ in 0..40 {
            pose = ik.solve(&head, [Some(lh), Some(rh)], |_, _| 0.0, 0.016);
        }
        let torso0 = (head.position + head.rotation * Vec3::new(0.0, -0.09, 0.09)
            - pose.get(gothic_vr::body::Joint::Pelvis))
        .length();
        let sc = (torso0 / hero_rt.torso_bind_m(&hero).unwrap()).clamp(0.6, 1.6);
        let mut pose3 = pose.clone();
        gothic_quest::hero::place_trunk(&mut pose3, head.position, head.rotation);
        {
            let rt = hero_rt.clone();
            let rig_copy = ActorRig {
                root: hero.root,
                by_name: hero.by_name.clone(),
                names: hero.names.clone(),
                parent: hero.parent.clone(),
                bind_local: hero.bind_local.clone(),
                bind_global: hero.bind_global.clone(),
                joints: hero.joints.clone(),
            };
            let curl = [[1.0, 1.0], if draw > 0.0 { [0.9, 0.8] } else { [0.2, 0.0] }];
            app.world_mut()
                .run_system_once(
                    move |mut q: Query<
                        &mut Transform,
                        (Without<ShotCam>, Without<BowProp>, Without<ArrowVis>),
                    >| {
                        rt.apply(
                            &rig_copy,
                            &mut q,
                            &pose3,
                            Some(head.rotation),
                            [Some(lh.rotation), Some(rh.rotation)],
                            sc,
                            None,
                            curl,
                            None,
                            None,
                        );
                    },
                )
                .unwrap();
        }
        // Bow + arrow.
        let (d, apose) = archer.aim(&bow_pose, rh.position);
        let draw_now = if draw > 0.0 { d } else { 0.0 };
        println!("{state}: draw {draw_now:.3} m");
        app.world_mut()
            .run_system_once(
                move |mut bows: Query<(&mut Transform, &BowDraw), With<BowProp>>,
                      mut arrows: Query<
                    (&mut Transform, &mut Visibility),
                    (With<ArrowVis>, Without<BowProp>),
                >,
                      mut joints: Query<
                    &mut Transform,
                    (Without<BowProp>, Without<ArrowVis>, Without<ShotCam>),
                >| {
                    let mut set = Vec::new();
                    for (mut t, b) in &mut bows {
                        *t = Transform::from_translation(bow_pose.position)
                            .with_rotation(bow_pose.rotation);
                        let poses = b.pose(draw_now * 100.0);
                        set.extend(b.joint_entities().into_iter().zip(poses));
                    }
                    for (e, p) in set {
                        if let Ok(mut jt) = joints.get_mut(e) {
                            *jt = p;
                        }
                    }
                    for (mut t, mut v) in &mut arrows {
                        *t = Transform::from_translation(apose.position)
                            .with_rotation(apose.rotation);
                        *v = if draw > 0.0 {
                            Visibility::Visible
                        } else {
                            Visibility::Hidden
                        };
                    }
                },
            )
            .unwrap();
        for (name, from, to) in views {
            let tf = Transform::from_translation(from).looking_at(to, Vec3::Y);
            {
                let mut q = app
                    .world_mut()
                    .query_filtered::<&mut Transform, With<ShotCam>>();
                for mut t in q.iter_mut(app.world_mut()) {
                    *t = tf;
                }
            }
            for _ in 0..8 {
                app.update();
            }
            let path = out.join(format!("bow_{state}_{name}.png"));
            app.world_mut()
                .spawn(Screenshot::image(handle.clone()))
                .observe(save_to_disk(path.clone()));
            for _ in 0..8 {
                app.update();
            }
            println!("saved {}", path.display());
        }
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
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(40.0, 40.0))),
        MeshMaterial3d(std_mats.add(StandardMaterial {
            base_color: Color::srgb(0.32, 0.38, 0.22),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::IDENTITY,
    ));
    let root = commands
        .spawn((
            Transform::from_scale(Vec3::splat(0.01)),
            Visibility::default(),
        ))
        .id();
    let opts = |cutout, hide| PartOptions {
        cutout,
        hide_from_eyes: hide,
    };
    let hero = spawn_body(
        &mut commands,
        &mut a,
        "g3_hero_body_player.xact",
        root,
        opts(false, false),
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
        opts(true, true),
        &mut meshes,
        &mut qmats,
        &mut images,
        &mut inv,
    );
    let hero_rt = Retarget::new(&hero, "Hero");
    let bow = commands
        .spawn((BowProp, Transform::IDENTITY, Visibility::default()))
        .id();
    let holder = commands
        .spawn((
            gothic_quest::weapons::bow_visual(),
            Visibility::default(),
            ChildOf(bow),
        ))
        .id();
    let bowroot = commands
        .spawn((Transform::IDENTITY, Visibility::default(), ChildOf(holder)))
        .id();
    let rig = spawn_body(
        &mut commands,
        &mut a,
        "it_weapon_ranged_bow_hunting_01.xact",
        bowroot,
        opts(false, false),
        &mut meshes,
        &mut qmats,
        &mut images,
        &mut inv,
    )
    .expect("bow actor");
    commands
        .entity(bow)
        .insert(BowDraw::new(&rig).expect("bow skeleton"));
    let arrow = commands
        .spawn((ArrowVis, Transform::IDENTITY, Visibility::Hidden))
        .id();
    a.spawn_static(
        &mut commands,
        "g3_weapons_ranged_bow_arrow_01.xcmsh",
        arrow,
        Transform::from_translation(Vec3::new(0.0, 0.0, -0.46875))
            .with_rotation(Quat::from_rotation_y(std::f32::consts::PI))
            .with_scale(Vec3::splat(0.01)),
        &mut meshes,
        &mut qmats,
        &mut images,
    );
    Built { hero, hero_rt }
}
