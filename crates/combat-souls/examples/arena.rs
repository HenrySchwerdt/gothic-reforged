//! Souls-combat arena: the whole flow on primitive shapes, no game data needed.
//!
//! ```text
//! cargo run -p gothic-combat-souls --example arena --features demo -- [--combat souls]
//!     [--classic]   foes attack like the original (no telegraph)
//!     [--shots]     scripted run that writes .local/souls-*.png and exits
//!     [--actor [--stance 1h|2h] [--clip-pack FILE] [--data-dir DIR]]
//!                   draw the Gothic 3 hero (needs the game data) instead of the capsule; with a
//!                   local clip pack (default `.local/er/er_clips.grcp` if present) the pack's
//!                   clips and action windows drive it, exactly as in `--play --combat souls`
//! ```
//! Controls: WASD move, Shift walk, Space tap = roll / backstep, Space hold = sprint, LMB light,
//! RMB heavy (hold to charge), Q guard (press just before a hit to parry, then attack to
//! riposte), Tab / MMB lock-on, mouse wheel switches target, Esc quits. Gamepad: see
//! `plugin::gather_input`.
//!
//! The camera is the Gothic camera solver (`gothic-camera`), steered by `LockCam` while locked on.
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::time::TimeUpdateStrategy;
use bevy::window::{CursorGrabMode, CursorOptions, PresentMode, PrimaryWindow, WindowResolution};
use gothic_animation::{
    ActorLibrary, ActorSpawner, AnimationPlugin, Animator, LocomotionInput, NpcSpec, Stance,
    WeaponSpec, pack_overlay as ov,
};
use gothic_camera::{
    CameraConfig, CameraInput, CameraMode, CameraState, HeroView, NoWorld, PresetTable,
};
use gothic_combat_souls::actions::ActionId;
use gothic_combat_souls::animmap::{self, PackPose};
use gothic_combat_souls::events::Event as SEvent;
use gothic_combat_souls::foe::FoeState;
use gothic_combat_souls::hero::{Button, State};
use gothic_combat_souls::plugin::{
    InputLatch, SoulsBackend, SoulsEvent, SoulsPlugin, SoulsRes, SoulsView, from_bevy, to_bevy,
};
use gothic_combat_souls::sink::SimpleSink;
use gothic_combat_souls::world::OpenField;
use gothic_combat_souls::{FoeProfile, SoulsWorld, Tuning};
use std::f32::consts::{FRAC_PI_2, PI};
use std::time::Duration;

const CM: f32 = 0.01;
const ARENA: f32 = 1800.0;
const HERO: u32 = 0;

#[derive(Component)]
struct HeroVis {
    body: Entity,
    sword: Entity,
}

#[derive(Component)]
struct FoeVis {
    id: u32,
    body: Entity,
    arm: Entity,
    material: Handle<StandardMaterial>,
}

#[derive(Component)]
struct Hud;
#[derive(Component)]
struct LogText;
#[derive(Component, Clone, Copy)]
enum BarKind {
    Hp,
    Stamina,
    Poise,
    FoeHp,
}

#[derive(Resource)]
struct Cam {
    state: CameraState,
    placed: bool,
}

#[derive(Resource, Default)]
struct EventLog(Vec<String>);

#[derive(Resource)]
struct Opts {
    shots: bool,
    /// `--shot-prefix er-`: screenshots are `.local/souls-<prefix><name>.png`.
    prefix: String,
}

/// `--actor`: the Gothic 3 hero drawn instead of the capsule.
#[derive(Resource)]
struct ActorOpt {
    stance: Stance,
    /// Lock-on strafe clips exist in the pack.
    strafe: bool,
    pack: bool,
}

#[derive(Component)]
struct ActorVis;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let classic = args.iter().any(|a| a == "--classic");
    let shots = args.iter().any(|a| a == "--shots");
    let profile = if classic {
        FoeProfile::classic()
    } else {
        FoeProfile::telegraphed()
    };

    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .filter(|s| !s.starts_with("--"))
            .cloned()
    };
    let tuning = Tuning::default();
    let mut res = SoulsRes::new(SoulsWorld::new(from_bevy(Vec3::ZERO), 400.0, tuning));
    // `--actor`: open the game archives, load the clip pack if there is one.
    let actor = if args.iter().any(|a| a == "--actor") {
        let dir = flag("--data-dir")
            .unwrap_or_else(|| r"D:\SteamLibrary\steamapps\common\Gothic 3\Data".into());
        let mut lib =
            ActorLibrary::open(std::path::Path::new(&dir)).expect("opening Gothic 3 archives");
        let pack_path = flag("--clip-pack").unwrap_or_else(|| ".local/er/er_clips.grcp".into());
        let (mut pack, mut strafe) = (false, false);
        if std::path::Path::new(&pack_path).exists() {
            let p = lib
                .load_clip_pack(std::path::Path::new(&pack_path))
                .expect("clip pack");
            let inst = gothic_combat_souls::pack::install(&mut res.anim, &p);
            eprintln!("arena: clip pack {pack_path}: {inst:?}");
            pack = inst.remapped > 0;
            strafe = inst.strafe;
        } else {
            eprintln!("arena: no clip pack at {pack_path}, Gothic motions");
        }
        let stance = match flag("--stance").as_deref() {
            Some("2h") => Stance::TwoHand,
            _ => Stance::OneHand,
        };
        Some((
            lib,
            ActorOpt {
                stance,
                strafe,
                pack,
            },
        ))
    } else {
        None
    };
    let mut sink = SimpleSink::default();
    sink.add(HERO, 400.0, 40.0, 0.0);
    for (i, (x, z)) in [(0.0, -650.0), (-600.0, -1000.0), (650.0, -1050.0)]
        .into_iter()
        .enumerate()
    {
        let id = i as u32 + 1;
        sink.add(id, 500.0, 45.0, 5.0);
        let foe = res
            .world
            .add_foe(id, from_bevy(Vec3::new(x, 0.0, z)), 500.0, profile.clone());
        // In shots mode the script decides when they attack; otherwise they fight on their own.
        foe.autonomous = !shots;
    }

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Gothic Reforged / Souls combat arena".into(),
            resolution: WindowResolution::new(1280, 720),
            present_mode: PresentMode::AutoVsync,
            ..default()
        }),
        ..default()
    }))
    .add_plugins(SoulsPlugin::default())
    .insert_resource(res)
    .insert_resource(SoulsBackend {
        sink: Box::new(sink),
        env: Box::new(OpenField {
            arena_radius: Some(ARENA),
        }),
    })
    .insert_resource(ClearColor(Color::srgb(0.07, 0.07, 0.09)))
    .insert_resource(Opts {
        shots,
        prefix: flag("--shot-prefix").unwrap_or_default(),
    })
    .init_resource::<EventLog>()
    .insert_resource(GlobalAmbientLight {
        brightness: 350.0,
        ..default()
    })
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (
            drive_view,
            camera_system,
            pose_hero,
            pose_foes,
            hud_system,
            collect_log,
            lock_marker,
            quit_on_escape,
        )
            .chain()
            .after(gothic_combat_souls::plugin::gather_input),
    );
    if let Some((lib, opt)) = actor {
        app.add_plugins(AnimationPlugin)
            .insert_resource(lib)
            .insert_resource(opt)
            .add_systems(Startup, spawn_actor.after(setup))
            .add_systems(Update, pose_actor.after(pose_hero));
    }
    if shots {
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .insert_resource(Script::default())
        .add_systems(
            PreUpdate,
            script_input.after(gothic_combat_souls::plugin::gather_input),
        )
        .add_systems(Last, script_shots);
    }
    app.run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    res: Res<SoulsRes>,
    opts: Res<Opts>,
    mut windows: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    if !opts.shots
        && let Ok(mut c) = windows.single_mut()
    {
        c.grab_mode = CursorGrabMode::Locked;
        c.visible = false;
    }
    let mut state = CameraState::new(CameraConfig::default(), PresetTable::default());
    state.seed(7);
    state.set_mode(CameraMode::Melee, false);
    commands.insert_resource(Cam {
        state,
        placed: false,
    });
    commands.spawn((Camera3d::default(), Transform::from_xyz(0.0, 3.0, 8.0)));
    commands.spawn((
        DirectionalLight {
            illuminance: 14_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-6.0, 12.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Floor and a ring wall.
    let floor = materials.add(StandardMaterial {
        base_color: Color::srgb(0.22, 0.2, 0.18),
        perceptual_roughness: 0.95,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(ARENA * CM, 0.2))),
        MeshMaterial3d(floor),
        Transform::from_xyz(0.0, -0.1, 0.0),
    ));
    let stone = materials.add(StandardMaterial {
        base_color: Color::srgb(0.32, 0.3, 0.28),
        perceptual_roughness: 1.0,
        ..default()
    });
    let pillar = meshes.add(Cuboid::new(0.8, 3.5, 0.8));
    for i in 0..16 {
        let a = i as f32 / 16.0 * 2.0 * PI;
        commands.spawn((
            Mesh3d(pillar.clone()),
            MeshMaterial3d(stone.clone()),
            Transform::from_xyz(
                a.cos() * (ARENA * CM + 0.5),
                1.75,
                a.sin() * (ARENA * CM + 0.5),
            ),
        ));
    }

    // Hero.
    let steel = materials.add(StandardMaterial {
        base_color: Color::srgb(0.7, 0.72, 0.78),
        metallic: 0.8,
        perceptual_roughness: 0.35,
        ..default()
    });
    let cloth = materials.add(StandardMaterial {
        base_color: Color::srgb(0.25, 0.32, 0.55),
        ..default()
    });
    let skin = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.65, 0.5),
        ..default()
    });
    let blade_mesh = meshes.add(Cuboid::new(0.12, 0.035, 1.3));
    let sword = commands
        .spawn((Transform::from_xyz(0.42, 0.55, -0.1), Visibility::default()))
        .with_children(|p| {
            p.spawn((
                Mesh3d(blade_mesh),
                MeshMaterial3d(steel.clone()),
                Transform::from_xyz(0.0, 0.0, -0.7),
            ));
        })
        .id();
    let body = commands
        .spawn((Transform::from_xyz(0.0, 0.9, 0.0), Visibility::default()))
        .with_children(|p| {
            p.spawn((
                Mesh3d(meshes.add(Capsule3d::new(0.32, 0.75))),
                MeshMaterial3d(cloth),
                Transform::default(),
            ));
            p.spawn((
                Mesh3d(meshes.add(Sphere::new(0.2))),
                MeshMaterial3d(skin),
                Transform::from_xyz(0.0, 0.78, 0.0),
            ));
            p.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.12, 0.12, 0.2))),
                MeshMaterial3d(steel),
                Transform::from_xyz(0.0, 0.78, -0.2),
            ));
        })
        .id();
    commands.entity(body).add_child(sword);
    commands
        .spawn((
            HeroVis { body, sword },
            Transform::default(),
            Visibility::default(),
        ))
        .add_child(body);

    // Foes.
    for foe in &res.world.foes {
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.25, 0.42, 0.22),
            emissive: LinearRgba::BLACK,
            ..default()
        });
        let club_mat = materials.add(StandardMaterial {
            base_color: Color::srgb(0.25, 0.18, 0.1),
            ..default()
        });
        let arm = commands
            .spawn((Transform::from_xyz(0.7, 0.7, -0.1), Visibility::default()))
            .with_children(|p| {
                p.spawn((
                    Mesh3d(meshes.add(Cuboid::new(0.28, 0.28, 1.6))),
                    MeshMaterial3d(club_mat),
                    Transform::from_xyz(0.0, 0.0, -0.85),
                ));
            })
            .id();
        let fbody = commands
            .spawn((Transform::from_xyz(0.0, 1.15, 0.0), Visibility::default()))
            .with_children(|p| {
                p.spawn((
                    Mesh3d(meshes.add(Capsule3d::new(0.5, 1.0))),
                    MeshMaterial3d(material.clone()),
                    Transform::default(),
                ));
                p.spawn((
                    Mesh3d(meshes.add(Sphere::new(0.28))),
                    MeshMaterial3d(material.clone()),
                    Transform::from_xyz(0.0, 1.05, -0.1),
                ));
            })
            .id();
        commands.entity(fbody).add_child(arm);
        commands
            .spawn((
                FoeVis {
                    id: foe.id,
                    body: fbody,
                    arm,
                    material,
                },
                Transform::default(),
                Visibility::default(),
            ))
            .add_child(fbody);
    }

    // HUD.
    commands
        .spawn((
            Hud,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
        ))
        .with_children(|r| {
            let bar =
                |r: &mut ChildSpawnerCommands, top: f32, left: f32, w: f32, color: Color, kind| {
                    r.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(left),
                            top: px(top),
                            width: px(w + 4.0),
                            height: px(18),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                    ));
                    r.spawn((
                        kind,
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(left + 2.0),
                            top: px(top + 2.0),
                            width: px(w),
                            height: px(14),
                            ..default()
                        },
                        BackgroundColor(color),
                    ));
                };
            bar(
                r,
                16.0,
                16.0,
                320.0,
                Color::srgb(0.75, 0.15, 0.15),
                BarKind::Hp,
            );
            bar(
                r,
                40.0,
                16.0,
                260.0,
                Color::srgb(0.2, 0.7, 0.3),
                BarKind::Stamina,
            );
            bar(
                r,
                64.0,
                16.0,
                160.0,
                Color::srgb(0.85, 0.75, 0.3),
                BarKind::Poise,
            );
            bar(
                r,
                16.0,
                460.0,
                360.0,
                Color::srgb(0.65, 0.3, 0.1),
                BarKind::FoeHp,
            );
            r.spawn((
                LogText,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(18.0),
                    ..default()
                },
                TextColor(Color::WHITE),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(16),
                    bottom: px(16),
                    ..default()
                },
            ));
        });
}

fn drive_view(cam: Res<Cam>, mut view: ResMut<SoulsView>) {
    view.cam_yaw = cam.state.view_yaw();
}

fn quit_on_escape(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}

#[allow(clippy::too_many_arguments)]
fn camera_system(
    time: Res<Time>,
    mut cam: ResMut<Cam>,
    mut res: ResMut<SoulsRes>,
    motion: Res<AccumulatedMouseMotion>,
    opts: Res<Opts>,
    mut q: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    let Ok((mut tf, mut proj)) = q.single_mut() else {
        return;
    };
    let dt = time.delta_secs().clamp(0.001, 0.1);
    let hero = &res.world.hero;
    let view = HeroView {
        position: hero.pos,
        box_height: 175.0,
        moving: hero.speed > 20.0,
        speed: hero.speed,
        in_combat: true,
        ..HeroView::default()
    };
    if !cam.placed {
        cam.state
            .reset(&view, gothic_combat_souls::math::dir_of(hero.yaw));
        cam.placed = true;
    }
    let target = res.world.lock_pos();
    let (hero_pos, hero_yaw) = (hero.pos, hero.yaw);
    if res.world.recenter {
        res.lock_cam.recenter();
    }
    res.lock_cam
        .drive(&mut cam.state, hero_pos, target, hero_yaw, dt);
    let look = if target.is_some() || opts.shots {
        Vec2::ZERO
    } else {
        motion.delta
    };
    let out = cam.state.update(
        &CameraInput {
            dt,
            hero: view,
            look,
            ..CameraInput::default()
        },
        &NoWorld,
    );
    tf.translation = to_bevy(out.position) * CM;
    tf.rotation = out.rotation;
    if let Projection::Perspective(p) = &mut *proj {
        p.fov = out.fov_deg.to_radians();
        p.near = (out.z_near * CM).max(0.05);
    }
}

fn yaw_rot(yaw: f32) -> Quat {
    Quat::from_rotation_y(yaw)
}

/// Procedural sword pose for an action and frame: (rotation of the arm pivot).
fn sword_pose(id: Option<ActionId>, f: f32, guarding: bool, charge_hold: bool) -> Quat {
    let rx = Quat::from_rotation_x;
    let ry = Quat::from_rotation_y;
    let t = |from: f32, to: f32| ((f - from) / (to - from)).clamp(0.0, 1.0);
    match id {
        Some(ActionId::Light(1)) => ry(-1.1 + 2.2 * t(4.0, 13.0)) * rx(0.15),
        Some(ActionId::Light(2)) => ry(1.1 - 2.2 * t(4.0, 13.0)) * rx(0.15),
        Some(ActionId::Light(_)) | Some(ActionId::GuardCounter) => rx(2.5 - 3.1 * t(8.0, 17.0)),
        Some(ActionId::Heavy) => {
            if charge_hold {
                rx(2.7)
            } else {
                rx(2.7 - 3.4 * t(20.0, 27.0))
            }
        }
        Some(ActionId::RunLight) | Some(ActionId::RollLight) => ry(-0.9 + 1.8 * t(5.0, 13.0)),
        Some(ActionId::Riposte) => {
            rx(0.1) * ry(0.0) * Quat::from_rotation_z(0.0) * rx(-0.5 * t(20.0, 33.0))
        }
        Some(ActionId::GuardHit) | Some(ActionId::GuardBreak) => rx(0.9) * ry(0.5),
        Some(ActionId::Roll(_)) | Some(ActionId::Backstep) => rx(-0.2),
        _ if guarding => rx(1.0) * ry(0.45),
        _ => rx(-0.35) * ry(-0.15),
    }
}

fn pose_hero(
    res: Res<SoulsRes>,
    mut roots: Query<(&HeroVis, &mut Transform)>,
    mut others: Query<&mut Transform, Without<HeroVis>>,
) {
    let h = &res.world.hero;
    let Ok((vis, mut tf)) = roots.single_mut() else {
        return;
    };
    tf.translation = to_bevy(h.pos) * CM;
    tf.rotation = yaw_rot(h.yaw);
    let (id, f) = match h.state {
        State::Act(a) => (Some(a.id), a.f),
        _ => (None, 0.0),
    };
    let charge_hold = matches!(h.state, State::Act(a) if a.id == ActionId::Heavy && !a.charge_done && a.f >= 12.0);
    if let Ok(mut s) = others.get_mut(vis.sword) {
        s.rotation = sword_pose(id, f, h.guarding, charge_hold);
    }
    if let Ok(mut b) = others.get_mut(vis.body) {
        let mut rot = Quat::IDENTITY;
        let mut lift = 0.9;
        match id {
            Some(ActionId::Roll(_)) => {
                let p = (f / 24.0).clamp(0.0, 1.0);
                rot = Quat::from_rotation_x(-2.0 * PI * p);
                lift = 0.55;
            }
            Some(ActionId::Backstep) => {
                rot = Quat::from_rotation_x(0.35);
            }
            Some(ActionId::Hurt(_) | ActionId::Stagger | ActionId::GuardBreak) => {
                rot = Quat::from_rotation_x(0.45);
            }
            _ => {}
        }
        if h.sprinting {
            rot = Quat::from_rotation_x(-0.3);
        }
        if h.is_dead() {
            rot = Quat::from_rotation_x(FRAC_PI_2);
            lift = 0.35;
        }
        b.rotation = rot;
        b.translation.y = lift;
    }
}

fn spawn_actor(mut spawner: ActorSpawner, opt: Res<ActorOpt>) {
    let mut spec = NpcSpec::new("G3_Hero_Body_Player")
        .with_part("G3_Hero_Head_Player")
        .with_stance(opt.stance)
        .with_weapon(WeaponSpec::one_hand_sword(
            if opt.stance == Stance::TwoHand {
                "G3_Weapon_2H_Sword_01"
            } else {
                "G3_Weapon_1H_Sword_01"
            },
        ));
    spec.start_time = 0.0;
    match spawner.spawn(&spec, Transform::from_scale(Vec3::splat(CM))) {
        Ok(e) => {
            spawner.commands.entity(e).insert(ActorVis);
        }
        Err(e) => eprintln!("arena: hero actor failed: {e:#}"),
    }
}

/// Moves the hero actor with the flow's hero and picks its animation: the pack's clips (or the
/// stock locomotion) by the same rules as the game glue.
#[allow(clippy::too_many_arguments)]
fn pose_actor(
    time: Res<Time>,
    res: Res<SoulsRes>,
    opt: Res<ActorOpt>,
    mut lib: ResMut<ActorLibrary>,
    mut actor: Query<(&mut Transform, &mut LocomotionInput, &mut Animator), With<ActorVis>>,
    mut vis: Query<&mut Visibility, With<HeroVis>>,
    mut prev: Local<Option<Vec3>>,
) {
    let Ok((mut tf, mut input, mut animator)) = actor.single_mut() else {
        return;
    };
    for mut v in &mut vis {
        *v = Visibility::Hidden;
    }
    let dt = time.delta_secs().max(1e-4);
    let h = &res.world.hero;
    let pos = to_bevy(h.pos);
    let velocity = prev.map_or(Vec3::ZERO, |p| (pos - p) / dt);
    *prev = Some(pos);
    tf.translation = pos * CM;
    tf.rotation = yaw_rot(h.yaw);
    tf.scale = Vec3::splat(CM);
    input.velocity = velocity;
    input.stance = opt.stance;
    if !opt.pack {
        return;
    }
    use gothic_combat_souls::pack::{PREFIX, style};
    let Some(style) = style(opt.stance) else {
        return;
    };
    let pose = animmap::pack_pose(
        h,
        &res.world.tuning,
        opt.strafe && res.world.lock.target.is_some(),
        gothic_combat_souls::plugin::from_bevy(velocity),
    );
    match pose {
        PackPose::Action { id, frame } => {
            // Start the action's clip the first frame of the action, then pin it to the frame.
            let Some(pattern) = animmap::pack_clip(id) else {
                return;
            };
            let name = format!("{PREFIX}{}", pattern.replace("{S}", style));
            let fresh = !animator
                .overlay
                .as_ref()
                .is_some_and(|o| o.motion.clip.name.eq_ignore_ascii_case(&name));
            if fresh && !ov::start(&mut lib, &mut animator, &name, 0.0) {
                return;
            }
            ov::follow_time(
                &mut animator,
                PREFIX,
                frame / gothic_combat_souls::tuning::ANIM_FPS,
            );
        }
        PackPose::Guard => {
            let name = format!("{PREFIX}{style}_Guard");
            if !ov::hold_loop(&mut lib, &mut animator, PREFIX, &name, 1.0) {
                ov::fade_out(&mut animator, PREFIX, dt, 0.14);
            }
        }
        PackPose::Strafe { gait, dir, rate } => {
            let name = format!("{PREFIX}{gait}_{dir}");
            if !ov::hold_loop(&mut lib, &mut animator, PREFIX, &name, rate) {
                ov::fade_out(&mut animator, PREFIX, dt, 0.14);
            }
        }
        PackPose::None => ov::fade_out(&mut animator, PREFIX, dt, 0.14),
    }
}

fn pose_foes(
    res: Res<SoulsRes>,
    foes: Query<(&FoeVis, Entity)>,
    mut tfs: Query<&mut Transform>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (vis, e) in &foes {
        let Some(foe) = res.world.foe(vis.id) else {
            continue;
        };
        if let Ok(mut tf) = tfs.get_mut(e) {
            tf.translation = to_bevy(foe.pos) * CM;
            tf.rotation = yaw_rot(foe.yaw);
        }
        let mut arm = Quat::from_rotation_x(-0.4);
        let mut tilt = 0.0;
        let mut glow = 0.0;
        let mut tint = Color::srgb(0.25, 0.42, 0.22);
        match foe.state {
            FoeState::Windup(_) => {
                let p = foe.windup_progress().unwrap_or(0.0);
                arm = Quat::from_rotation_x(-0.4 + 3.0 * p);
                glow = p * p;
                tilt = -0.15 * p;
            }
            FoeState::Strike(_) => {
                let p = (foe.t / 0.2).clamp(0.0, 1.0);
                arm = Quat::from_rotation_x(2.6 - 3.4 * p);
                glow = 1.0;
                tilt = 0.25 * p;
            }
            FoeState::Recover => {
                arm = Quat::from_rotation_x(-0.8);
                tilt = 0.2;
            }
            FoeState::Flinch => tilt = -0.25,
            FoeState::Stagger { parried } => {
                tilt = -0.5;
                arm = Quat::from_rotation_x(0.6);
                tint = if parried {
                    Color::srgb(0.9, 0.9, 0.3)
                } else {
                    Color::srgb(0.9, 0.6, 0.2)
                };
            }
            FoeState::Dead => {
                tilt = -FRAC_PI_2;
                tint = Color::srgb(0.15, 0.2, 0.15);
            }
            _ => {}
        }
        if let Ok(mut a) = tfs.get_mut(vis.arm) {
            a.rotation = arm;
        }
        if let Ok(mut b) = tfs.get_mut(vis.body) {
            b.rotation = Quat::from_rotation_x(tilt);
            b.translation.y = if foe.state == FoeState::Dead {
                0.55
            } else {
                1.15
            };
        }
        if let Some(mut m) = materials.get_mut(&vis.material) {
            m.base_color = tint;
            m.emissive = LinearRgba::new(4.0 * glow, 0.5 * glow, 0.0, 1.0);
        }
    }
}

fn lock_marker(res: Res<SoulsRes>, mut gizmos: Gizmos) {
    // Slash trail while the swing is live.
    let h = &res.world.hero;
    if let State::Act(a) = h.state
        && let Some(hit) = a.id.def().hit
        && a.f >= hit.from
        && a.f < hit.to + 4.0
    {
        let origin = to_bevy(h.pos) * CM + Vec3::Y * 1.1;
        let half = hit.arc_deg.to_radians();
        let n = 12;
        let pts = (0..=n).map(|i| {
            let a = h.yaw - half + 2.0 * half * i as f32 / n as f32;
            origin + to_bevy(gothic_combat_souls::math::dir_of(a)) * 1.9
        });
        gizmos.linestrip(pts, Color::srgb(1.0, 0.55, 0.15));
    }
    let Some(p) = res.world.lock_pos() else {
        return;
    };
    let base = to_bevy(p) * CM;
    gizmos.circle(
        Isometry3d::new(base + Vec3::Y * 0.03, Quat::from_rotation_x(-FRAC_PI_2)),
        0.95,
        Color::srgb(1.0, 0.85, 0.2),
    );
    let top = base + Vec3::Y * 3.0;
    gizmos.sphere(
        Isometry3d::from_translation(top),
        0.09,
        Color::srgb(1.0, 0.85, 0.2),
    );
    gizmos.line(
        top - Vec3::Y * 0.12,
        top - Vec3::Y * 0.45,
        Color::srgb(1.0, 0.85, 0.2),
    );
}

fn collect_log(mut ev: MessageReader<SoulsEvent>, mut log: ResMut<EventLog>) {
    for SoulsEvent(e) in ev.read() {
        let line = match e {
            SEvent::HeroAction(a) => format!("{a:?}"),
            SEvent::HeroHit {
                damage,
                kind,
                poise_broken,
                ..
            } => {
                format!(
                    "hit {kind:?} {damage}{}",
                    if *poise_broken { " POISE BREAK" } else { "" }
                )
            }
            SEvent::HeroHurt { result, damage, .. } => format!("took {result:?} {damage}"),
            SEvent::FoeWindup { foe, .. } => format!("foe {foe} winds up"),
            SEvent::FoeStaggered { by_parry, .. } => {
                (if *by_parry { "PARRIED" } else { "staggered" }).to_string()
            }
            SEvent::FoeDied { foe } => format!("foe {foe} down"),
            SEvent::LockChanged { target } => format!("lock {target:?}"),
            SEvent::HeroDied => "YOU DIED".to_string(),
            _ => continue,
        };
        log.0.push(line);
    }
    let n = log.0.len();
    if n > 7 {
        log.0.drain(..n - 7);
    }
}

fn hud_system(
    res: Res<SoulsRes>,
    log: Res<EventLog>,
    mut bars: Query<(&BarKind, &mut Node)>,
    mut text: Query<&mut Text, With<LogText>>,
) {
    let h = &res.world.hero;
    let t = &res.world.tuning;
    let (foe_hp, foe_max) = res
        .world
        .lock
        .target
        .and_then(|id| res.world.foe(id))
        .map_or((0.0, 1.0), |f| (f.hp, f.max_hp));
    for (kind, mut node) in &mut bars {
        let (frac, full) = match kind {
            BarKind::Hp => (h.hp / h.max_hp.max(1.0), 320.0),
            BarKind::Stamina => (h.stamina / t.max_stamina, 260.0),
            BarKind::Poise => (h.poise / t.max_poise, 160.0),
            BarKind::FoeHp => (foe_hp / foe_max.max(1.0), 360.0),
        };
        node.width = px((frac.clamp(0.0, 1.0) * full).max(0.0));
    }
    if let Ok(mut tx) = text.single_mut() {
        tx.0 = format!(
            "{}  [{}]\n{}",
            res.world.hero_state_name(),
            if res.world.lock.target.is_some() {
                "locked"
            } else {
                "free"
            },
            log.0.join("\n")
        );
    }
}

// ---- scripted screenshot run ------------------------------------------------------------------

#[derive(Resource, Default)]
struct Script {
    shot_pending: Vec<(u64, &'static str)>,
    started: bool,
    exit_at: Option<u64>,
}

/// Plays a fixed input timeline against the world tick counter.
fn script_input(
    res: Res<SoulsRes>,
    mut latch: ResMut<InputLatch>,
    mut script: ResMut<Script>,
    view: Res<SoulsView>,
) {
    latch.scripted = true;
    if !script.started {
        script.started = true;
        script.shot_pending = vec![
            (70, "lockon"),
            (128, "telegraph"),
            (152, "roll"),
            (172, "roll2"),
            (306, "attack"),
            (392, "parry"),
            (470, "strafe"),
        ];
        script.exit_at = Some(520);
    }
    let tick = res.world.tick;
    let was = latch.current;
    let mut mv = Vec2::ZERO;
    let (mut dodge, mut light, mut guard) = (false, false, false);
    let mut lock = false;
    // Lock on at tick 40.
    if tick == 40 {
        lock = true;
    }
    // Foe 1 starts a telegraphed swing; roll sideways across it, then close in and strike.
    if (136..143).contains(&tick) {
        dodge = tick < 138;
        mv = Vec2::X;
    }
    if (200..282).contains(&tick) {
        mv = Vec2::Y;
    }
    if (284..286).contains(&tick) {
        light = tick == 284;
    }
    // Circle the locked-on foe to the right (the lock-on strafe).
    if (410..505).contains(&tick) {
        mv = Vec2::X;
    }
    // Guard just before the second swing for a parry.
    if (375..400).contains(&tick) {
        guard = true;
    }
    let c = &mut latch.current;
    c.cam_yaw = view.cam_yaw;
    c.mv = mv;
    c.lock |= lock;
    let m = |b: &mut Button, held: bool, was: bool| {
        b.pressed |= held && !was;
        b.released |= !held && was;
        b.held = held;
    };
    m(&mut c.dodge, dodge, was.dodge.held);
    m(&mut c.light, light, was.light.held);
    m(&mut c.guard, guard, was.guard.held);
}

fn script_shots(
    opts: Res<Opts>,
    mut commands: Commands,
    mut script: ResMut<Script>,
    mut res: ResMut<SoulsRes>,
    mut exit: MessageWriter<AppExit>,
    mut frames: Local<u32>,
) {
    *frames += 1;
    let tick = res.world.tick;
    // Scripted foe behaviour.
    if tick == 100 {
        res.world.foe_mut(1).map(|f| f.command_attack(0));
    }
    if tick == 330 {
        // Bring the foe next to the hero for a clean parry, then swing.
        let hero_pos = res.world.hero.pos;
        let facing = res.world.hero.facing();
        if let Some(f) = res.world.foe_mut(1) {
            f.pos = hero_pos + facing * 200.0;
            f.yaw = gothic_combat_souls::math::yaw_of(-facing);
            f.command_attack(1);
        }
    }
    let due: Vec<_> = script
        .shot_pending
        .iter()
        .filter(|(t, _)| *t <= tick)
        .copied()
        .collect();
    script.shot_pending.retain(|(t, _)| *t > tick);
    for (_, name) in due {
        let path = format!(".local/souls-{}{name}.png", opts.prefix);
        eprintln!("souls: shot {path} at tick {tick}");
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
    if let Some(at) = script.exit_at
        && tick >= at
    {
        exit.write(AppExit::Success);
    }
    let _ = *frames;
}
