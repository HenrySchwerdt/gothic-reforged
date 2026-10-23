//! Grip lab (host, needs the local Ardea pack and a GPU): the hero holds every weapon of the Quest app in the right
//! hand with the closed-grip pose and the authored data of `grips.json`, rendered from four views into one contact
//! sheet per weapon: `.local/quest/griplab/<weapon>.png` (python with Pillow tiles the views).
//! Usage: `grip_lab [mesh-name-substring]`.
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
use gothic_vr::Pose;
use gothic_vr::body::{BodyIk, Proportions};

#[derive(Component)]
struct ShotCam;
#[derive(Component)]
struct Weapon(&'static str);

struct Built {
    hero: ActorRig,
    hero_rt: Retarget,
}

const WEAPONS: [&str; 2] = [
    "g3_weapon_1h_sword_orcslayer_01.xcmsh",
    "g3_weapon_orc_halberd_01.xcmsh",
];

fn main() {
    let filter = std::env::args().nth(1).unwrap_or_default();
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
    let parts = out.join("_parts");
    std::fs::create_dir_all(&parts).unwrap();
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
    // Pipelines and textures stream in over the first frames.
    for _ in 0..120 {
        app.update();
    }
    let hero_rt = std::sync::Arc::new(hero_rt);
    let head = Pose::new(Vec3::new(0.0, 1.7, 0.0), Quat::IDENTITY);
    // The weapon prop frame is the OpenXR grip frame: blade along -Z. Tilt the blade up and a little forward.
    let tilt = Quat::from_rotation_x(1.2);
    let lh = Pose::new(
        Vec3::new(-0.25, 1.15, -0.38),
        Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
    );
    let rh = Pose::new(Vec3::new(0.28, 1.15, -0.45), tilt);
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
    // Like the game (hero.rs): the visible palm centre goes to the grip position.
    for (k, joint, h) in [
        (0usize, gothic_vr::body::Joint::HandL, lh),
        (1usize, gothic_vr::body::Joint::HandR, rh),
    ] {
        if let Some(w) = hero_rt.wrist_for_grip(k, h.position, h.rotation, sc) {
            pose3.joints[joint.index()] = w;
        }
    }
    let hp = rh.position;
    // GRIPLAB_CLOSE=1: close-ups of the fist only, cameras placed in the grip frame (OpenXR: -Z thumb end of the
    // fist, +Z little-finger end, -Y along the fingers, +-X palm normal) so every weapon gets the same anatomy views.
    let close = std::env::var("GRIPLAB_CLOSE").is_ok_and(|v| v == "1");
    let g = |v: Vec3| rh.rotation * v;
    // Views around the right hand: (name, camera offset from the hand, look-at offset).
    let views: [(&str, Vec3, Vec3); 4] = [
        ("palm side (from the body)", Vec3::new(-0.5, 0.1, -0.5), Vec3::ZERO),
        ("back of hand", Vec3::new(0.6, 0.0, 0.0), Vec3::ZERO),
        ("top", Vec3::new(0.0, 0.6, 0.0), Vec3::ZERO),
        (
            "wide side",
            Vec3::new(-1.6, 0.35, 0.25),
            Vec3::new(0.0, 0.3, 0.25),
        ),
    ];
    let close_views: [(&str, Vec3, Vec3); 4] = [
        ("close +X", g(Vec3::new(0.2, 0.0, 0.0)), Vec3::ZERO),
        ("close -X", g(Vec3::new(-0.2, 0.0, 0.0)), Vec3::ZERO),
        ("close thumb end (-Z)", g(Vec3::new(0.05, 0.05, -0.2)), Vec3::ZERO),
        ("close finger side (-Y)", g(Vec3::new(0.05, -0.2, 0.02)), Vec3::ZERO),
    ];
    let views = if close { close_views } else { views };
    for mesh in WEAPONS.iter().filter(|m| m.contains(filter.as_str())) {
        let curl_pair = gothic_quest::grip::curl(mesh);
        let curl = [[1.0, 1.0], curl_pair];
        let mesh: &'static str = mesh;
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
        let p3 = pose3.clone();
        app.world_mut()
            .run_system_once(
                move |mut q: Query<&mut Transform, (Without<ShotCam>, Without<Weapon>)>,
                      mut weapons: Query<
                    (&Weapon, &mut Transform, &mut Visibility),
                    Without<ShotCam>,
                >| {
                    rt.apply(
                        &rig_copy,
                        &mut q,
                        &p3,
                        Some(head.rotation),
                        [Some(lh.rotation), Some(rh.rotation)],
                        sc,
                        None,
                        curl,
                        None,
                        None,
                    );
                    for (w, mut t, mut v) in &mut weapons {
                        if w.0 == mesh {
                            *t = Transform::from_translation(rh.position)
                                .with_rotation(rh.rotation);
                            *v = Visibility::Visible;
                        } else {
                            *v = Visibility::Hidden;
                        }
                    }
                },
            )
            .unwrap();
        // Pipelines and textures stream in over the first frames.
                let stem = mesh.trim_end_matches(".xcmsh");
        let mut files = Vec::new();
        for (i, (name, off, look)) in views.iter().enumerate() {
            let up = if off.x == 0.0 && off.z == 0.0 { Vec3::NEG_Z } else { Vec3::Y };
            let tf = Transform::from_translation(hp + *off).looking_at(hp + *look, up);
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
            if i == 0 {
                // Grip metrics (right hand): handle line = grip position along grip -Z.
                let names = ["Hand_Hand_1", "Hand_Finger2_1", "Hand_Finger5_1", "Hand_Finger3_1", "Hand_Finger3_3", "Hand_Finger1_3"];
                let mut w = std::collections::HashMap::new();
                for n in names {
                    let full = hero.by_name.keys().find(|k| k.contains("Right") && k.ends_with(n)).cloned();
                    if let Some(full) = full {
                        let e = hero.joints[hero.by_name[&full]];
                        if let Some(gt) = app.world().get::<GlobalTransform>(e) {
                            w.insert(n, gt.translation());
                        }
                    }
                }
                let hdir = rh.rotation * Vec3::NEG_Z;
                let dist = |p: Vec3| { let d = p - rh.position; (d - hdir * d.dot(hdir)).length() * 100.0 };
                if let (Some(&wr), Some(&i1), Some(&l1), Some(&m1), Some(&m3)) = (w.get("Hand_Hand_1"), w.get("Hand_Finger2_1"), w.get("Hand_Finger5_1"), w.get("Hand_Finger3_1"), w.get("Hand_Finger3_3")) {
                    let kn = (i1 - l1).normalize();
                    println!("[metrics {mesh}] knuckle-line vs handle angle {:.1} deg | handle-to-line dist (cm): wrist {:.1}, middle knuckle {:.1}, middle tip-joint {:.1}, thumb tip {:.1}", kn.angle_between(hdir).to_degrees(), dist(wr), dist(m1), dist(m3), w.get("Hand_Finger1_3").map_or(-1.0, |&p| dist(p)));
                    let c = (i1 + l1) * 0.5; let along = (c - rh.position).dot(hdir) * 100.0;
                    println!("[metrics {mesh}] knuckle centre along handle from grip point: {along:.1} cm");
                    let f = (m1 - wr).normalize();
                    let palm = { let c = m3 - m1; (c - f * c.dot(f)).normalize() };
                    let near = rh.position + hdir * (c - rh.position).dot(hdir);
                    let d = near - c;
                    println!("[metrics {mesh}] handle relative to knuckle centre: along fingers {:.1} cm, toward palm side {:.1} cm, sideways {:.1} cm", d.dot(f) * 100.0, d.dot(palm) * 100.0, d.dot(f.cross(palm)) * 100.0);
                    let jw = |suffix: &str| hero.by_name.keys().find(|k| k.contains("Right") && k.ends_with(suffix)).and_then(|k| app.world().get::<GlobalTransform>(hero.joints[hero.by_name[k]])).map(|g| g.translation());
                    if let (Some(t2), Some(t3), Some(i2)) = (jw("Hand_Finger1_2"), jw("Hand_Finger1_3"), jw("Hand_Finger2_2")) {
                        let tip = t3 + (t3 - t2);
                        println!("[thumb] tip-to-index2 {:.1} cm, tip-to-handle-axis {:.1} cm", (tip - i2).length() * 100.0, dist(tip));
                    }
                    if std::env::var("GRIPLAB_METRICS_ONLY").is_ok() { std::process::exit(0); }
                }
            }
            let path = parts.join(format!("{stem}_{i}.png"));
            app.world_mut()
                .spawn(Screenshot::image(handle.clone()))
                .observe(save_to_disk(path.clone()));
            for _ in 0..8 {
                app.update();
            }
            files.push((path, *name));
        }
        // Tile the four views into one sheet.
        let list = files
            .iter()
            .map(|(p, n)| format!("(r'{}', '{}')", p.display(), n))
            .collect::<Vec<_>>()
            .join(",");
        let sheet = out.join(if close { format!("{stem}_close.png") } else { format!("{stem}.png") });
        let script = format!(
            "from PIL import Image, ImageDraw\n\
             v=[{list}]\n\
             s=Image.new('RGB',(2000,2000))\n\
             d=ImageDraw.Draw(s)\n\
             for i,(p,n) in enumerate(v):\n\
             \x20   s.paste(Image.open(p).convert('RGB'),((i%2)*1000,(i//2)*1000))\n\
             \x20   d.text(((i%2)*1000+8,(i//2)*1000+8),n,fill=(255,255,255))\n\
             s.save(r'{}')\n",
            sheet.display()
        );
        let ok = std::process::Command::new("python")
            .args(["-c", &script])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        println!(
            "{} {}",
            if ok { "sheet" } else { "FAILED sheet" },
            sheet.display()
        );
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
    for mesh in WEAPONS {
        let vis = a
            .cache()
            .load_mesh(gothic_cook::AssetId::of(mesh))
            .ok()
            .and_then(|md| gothic_quest::grip::frame_for(mesh, &md))
            .map(|g| {
                println!(
                    "{mesh}: blade {:?} handle_axis {:?} handle-vs-blade {:.1} deg edge {:?} edge.blade {:.3} centre {:?} guard {:.1} pommel {:.1} tip {:.1}",
                    g.blade, g.handle_axis, g.handle_axis.angle_between(g.blade).to_degrees(), g.edge, g.edge.dot(g.blade), g.center, g.guard, g.pommel, g.tip
                );
                g.visual(0.01)
            })
            .unwrap_or_else(|| Transform::from_scale(Vec3::splat(0.01)));
        let e = commands
            .spawn((
                Weapon(mesh),
                Transform::from_translation(Vec3::new(0.0, -100.0, 0.0)),
                Visibility::Hidden,
            ))
            .id();
        a.spawn_static_mirrored(
            &mut commands,
            mesh,
            e,
            vis,
            &mut meshes,
            &mut qmats,
            &mut images,
        );
    }
    Built { hero, hero_rt }
}
