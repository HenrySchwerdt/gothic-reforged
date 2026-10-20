//! Development viewer for animated `.xact` actors (`gothic-animation`).
//!
//! ```text
//! gothic-reforged --actor Hero [--motion walk|run|idle|<name>] [--stance 1h] [--sword]
//!                 [--yaw DEG] [--smoke] [--screenshot PATH]
//! gothic-reforged --actor Hero --crowd 100 [--bench] [--smoke] [--screenshot PATH]
//! ```
//!
//! Single mode shows the hero; without `--motion` it is driven through the locomotion state
//! machine (keys 1..9 pick idle, walk, run, sprint, sneak, jump, fall, strafe, turn; F cycles
//! the stance). `--crowd N` spawns N different human NPCs (body, head, stance, weapons) that
//! follow scripted `LocomotionInput` sequences; `--bench` prints frame and animation timings.
//! Explicit archives only (`--data-dir`), no patch overlay.
use anyhow::{Result, anyhow};
use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    prelude::*,
    window::{PresentMode, WindowResolution},
};
use gothic_animation::{
    ActorLibrary, ActorSpawner, AnimationPlugin, AnimationStats, Animator, Locomotion,
    LocomotionInput, NpcSpec, Stance, WeaponSpec,
    locomotion::{FORWARD, RIGHT},
};
use std::path::PathBuf;

const DEFAULT_DATA_DIR: &str = r"D:\SteamLibrary\steamapps\common\Gothic 3\Data";

#[derive(Resource, Clone)]
struct Config {
    crowd: usize,
    smoke: bool,
    bench: bool,
    screenshot: String,
    second_screenshot: String,
    motion: Option<String>,
    stance: Stance,
    sword: bool,
    yaw_degrees: f32,
    times: [f32; 2],
    /// Upper-body overlay action (`ArmsCrossed`, ...) and initial ground speed (cm/s).
    overlay: Option<String>,
    speed: f32,
}

#[derive(Component)]
struct Orbit {
    target: Vec3,
    distance: f32,
    yaw: f32,
    pitch: f32,
}

/// Scripted input source of a crowd member.
#[derive(Component)]
struct CrowdMember {
    index: usize,
    stance: Stance,
}

#[derive(Component)]
struct Hero;

#[derive(Resource, Default)]
struct Frames {
    count: u32,
    deltas: Vec<f32>,
    animation: Vec<[f32; 3]>,
}

fn parse_stance(value: &str) -> Result<Stance> {
    Ok(match value.to_lowercase().as_str() {
        "unarmed" | "none" => Stance::Unarmed,
        "fist" => Stance::Fist,
        "1h" => Stance::OneHand,
        "1hshield" | "shield" => Stance::OneHandShield,
        "2h" => Stance::TwoHand,
        "bow" => Stance::Bow,
        "crossbow" => Stance::CrossBow,
        "staff" => Stance::Staff,
        "magic" | "cast" => Stance::Magic,
        "torch" => Stance::Torch,
        other => return Err(anyhow!("unknown stance {other:?}")),
    })
}

pub fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| -> Option<&str> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .filter(|s| !s.starts_with("--"))
            .map(String::as_str)
    };
    let has = |name: &str| args.iter().any(|a| a == name);
    let data_dir = PathBuf::from(flag("--data-dir").unwrap_or(DEFAULT_DATA_DIR));
    let crowd: usize = flag("--crowd").map(str::parse).transpose()?.unwrap_or(0);
    let sword = has("--sword");
    let stance = match flag("--stance") {
        Some(s) => parse_stance(s)?,
        None if sword => Stance::OneHand,
        None => Stance::Unarmed,
    };
    let screenshot = flag("--screenshot")
        .unwrap_or(if crowd > 0 {
            ".local/crowd.png"
        } else {
            ".local/actor-smoke.png"
        })
        .to_string();
    let second_screenshot = match screenshot.strip_suffix(".png") {
        Some(stem) => format!("{stem}-2.png"),
        None => format!("{screenshot}-2"),
    };
    let config = Config {
        crowd,
        smoke: has("--smoke"),
        bench: has("--bench"),
        screenshot,
        second_screenshot,
        motion: flag("--motion").map(str::to_string),
        stance,
        sword,
        yaw_degrees: flag("--yaw").map(str::parse).transpose()?.unwrap_or(0.0),
        times: [
            flag("--time-a").map(str::parse).transpose()?.unwrap_or(0.0),
            flag("--time-b").map(str::parse).transpose()?.unwrap_or(0.5),
        ],
        overlay: flag("--overlay").map(str::to_string),
        speed: flag("--speed").map(str::parse).transpose()?.unwrap_or(0.0),
    };
    let library = ActorLibrary::open(&data_dir)
        .map_err(|e| e.context(format!("opening archives in {}", data_dir.display())))?;
    eprintln!(
        "motion catalog: {} named motions; actors: {}",
        library.catalog.len(),
        library.actor_names(|_| true).len()
    );
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Gothic Reforged / actor viewer".into(),
                resolution: WindowResolution::new(1280, 720),
                present_mode: if config.bench {
                    PresentMode::AutoNoVsync
                } else {
                    PresentMode::AutoVsync
                },
                ..default()
            }),
            ..default()
        }))
        .add_plugins(AnimationPlugin)
        .insert_resource(library)
        .insert_resource(config)
        .init_resource::<Frames>()
        .insert_resource(ClearColor(Color::srgb(0.07, 0.08, 0.1)))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                crowd_script,
                hero_keys,
                hero_overlay,
                smoke_drive,
                orbit,
                capture,
                update_title,
            )
                .chain(),
        )
        .run();
    Ok(())
}

fn alias(name: &str) -> String {
    match name.to_lowercase().as_str() {
        "idle" => "Hero_Stand_None_None_P0_Ambient_Loop_N_Fwd_01_%_00_P0_0".into(),
        "walk" => "Hero_Stand_None_None_P0_Move_Walk_N_Fwd_00_%_00_P0_160".into(),
        "run" => "Hero_Stand_None_None_P0_Move_Run_N_Fwd_00_%_00_P0_400".into(),
        _ => name.to_string(),
    }
}

/// Deterministic pseudo-random value in 0..1 from an index and a salt.
fn hash01(index: usize, salt: u64) -> f32 {
    let mut x = (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ salt.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 30;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 27;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

fn setup(mut spawner: ActorSpawner, config: Res<Config>) {
    let mut center = Vec3::new(0.0, 95.0, 0.0);
    let mut distance = 300.0;
    let mut pitch = 0.08;
    // The authored facing is rotated to -Z (Bevy forward), so the front is seen from -Z.
    let mut yaw = config.yaw_degrees.to_radians() + std::f32::consts::PI;
    if config.crowd == 0 {
        let mut spec = NpcSpec::new("G3_Hero_Body_Player")
            .with_part("G3_Hero_Head_Player")
            .with_stance(config.stance);
        if config.sword {
            spec = spec.with_weapon(WeaponSpec::one_hand_sword("G3_Weapon_1H_Sword_01"));
        }
        if let Some(motion) = &config.motion {
            spec = spec.with_motion(alias(motion));
        }
        match spawner.spawn(&spec, Transform::default()) {
            Ok(entity) => {
                spawner.commands.entity(entity).insert(Hero);
            }
            Err(error) => eprintln!("hero spawn failed: {error:#}"),
        }
    } else {
        let (bodies, heads) = crowd_parts(&mut spawner);
        eprintln!(
            "crowd parts: {} bodies, {} heads",
            bodies.len(),
            heads.len()
        );
        let columns = (config.crowd as f32).sqrt().ceil() as usize;
        let spacing = 130.0;
        let swords = [
            "G3_Weapon_1H_Sword_01",
            "G3_Weapon_1H_Sword_Broad_01",
            "G3_Weapon_1H_Sword_Long_01",
            "G3_Weapon_1H_Sword_Rusty_01",
            "G3_Weapon_1H_Sword_Bastard_01",
        ];
        let stances = [
            Stance::Unarmed,
            Stance::OneHand,
            Stance::OneHandShield,
            Stance::TwoHand,
            Stance::Fist,
            Stance::Staff,
            Stance::Unarmed,
            Stance::OneHand,
        ];
        let mut spawned = 0;
        for i in 0..config.crowd {
            let body = &bodies[(hash01(i, 1) * bodies.len() as f32) as usize % bodies.len()];
            let head = &format!(
                "{}@M{}",
                heads[(hash01(i, 2) * heads.len() as f32) as usize % heads.len()],
                (hash01(i, 5) * 30.0) as u32
            );
            let stance = stances[(hash01(i, 3) * stances.len() as f32) as usize % stances.len()];
            let mut spec = NpcSpec::new(body.clone())
                .with_part(head.clone())
                .with_stance(stance)
                .with_start_time(hash01(i, 4) * 2.0);
            let sword = swords[i % swords.len()];
            match stance {
                Stance::OneHand => spec = spec.with_weapon(WeaponSpec::one_hand_sword(sword)),
                Stance::OneHandShield => {
                    spec = spec
                        .with_weapon(WeaponSpec::one_hand_sword(sword))
                        .with_weapon(WeaponSpec {
                            mesh: "G3_Armor_Shield_Round_Big_01".into(),
                            drawn_slot: "Slot_LeftHand_Shield".into(),
                            holster_slot: "Slot_ShieldBack".into(),
                            offset: Transform::IDENTITY,
                        });
                }
                Stance::TwoHand => {
                    spec = spec.with_weapon(WeaponSpec::one_hand_sword("G3_Weapon_2H_Sword_01"))
                }
                Stance::Staff => {
                    spec = spec.with_weapon(WeaponSpec::one_hand_sword(
                        "G3_Weapon_Staff_QuarterStaff_01",
                    ))
                }
                _ => {}
            }
            let (col, row) = (i % columns, i / columns);
            let position = Vec3::new(
                (col as f32 - (columns as f32 - 1.0) * 0.5) * spacing,
                0.0,
                (row as f32) * -spacing,
            );
            match spawner.spawn(&spec, Transform::from_translation(position)) {
                Ok(entity) => {
                    spawner
                        .commands
                        .entity(entity)
                        .insert(CrowdMember { index: i, stance });
                    spawned += 1;
                }
                Err(error) => eprintln!("crowd member {i} ({body} + {head}): {error:#}"),
            }
        }
        eprintln!("spawned {spawned}/{} crowd members", config.crowd);
        let rows = config.crowd.div_ceil(columns);
        let width = columns as f32 * spacing;
        let depth = rows as f32 * spacing;
        center = Vec3::new(0.0, 90.0, -depth * 0.5 + spacing * 0.5);
        distance = (width.max(depth) * 1.15).max(400.0);
        pitch = 0.55;
        yaw = std::f32::consts::PI;
    }
    spawner.commands.spawn((
        Camera3d::default(),
        Orbit {
            target: center,
            distance,
            yaw,
            pitch,
        },
        Transform::default(),
    ));
    spawner.commands.spawn((
        DirectionalLight {
            illuminance: 20000.0,
            ..default()
        },
        Transform::from_xyz(-300.0, 500.0, -500.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    spawner.commands.insert_resource(GlobalAmbientLight {
        brightness: 900.0,
        ..default()
    });
}

/// Human bodies sharing the hero skeleton and plain (non-LOD, non-animated) heads.
fn crowd_parts(spawner: &mut ActorSpawner) -> (Vec<String>, Vec<String>) {
    let library = &spawner.library;
    let plain = |n: &str| {
        !["LOD", "Animated", "Ghost", "Ancient", "Mummy", "Skeleton"]
            .iter()
            .any(|bad| n.contains(bad))
    };
    let bodies: Vec<String> = library
        .actor_names(|n| n.starts_with("G3_Hero_Body_") && plain(n))
        .into_iter()
        .filter(|n| {
            !["Skeleton", "Mummy", "Zombie", "Ghost", "Fat"]
                .iter()
                .any(|bad| n.contains(bad))
        })
        .collect();
    let heads: Vec<String> = library
        .actor_names(|n| {
            (n.starts_with("G3_Hero_Head_") || n.starts_with("G3_Head_Hero_")) && plain(n)
        })
        .into_iter()
        .filter(|n| !n.contains("Slave_Head"))
        .collect();
    (bodies, heads)
}

/// Scripted locomotion input per crowd member: a repeating sequence of states.
fn crowd_script(time: Res<Time>, mut query: Query<(&CrowdMember, &mut LocomotionInput)>) {
    // (duration, speed cm/s, strafe cm/s, sneak, airborne, turn error)
    const SCRIPT: [(f32, f32, f32, bool, bool, f32); 9] = [
        (2.5, 0.0, 0.0, false, false, 0.0),
        (3.0, 150.0, 0.0, false, false, 0.0),
        (3.0, 380.0, 0.0, false, false, 0.0),
        (2.5, 140.0, 0.0, true, false, 0.0),
        (2.0, 0.0, 130.0, false, false, 0.0),
        (2.0, 480.0, 0.0, false, false, 0.0),
        (1.6, 0.0, 0.0, false, true, 0.0),
        (2.0, 0.0, 0.0, false, false, 1.5),
        (2.5, 90.0, 0.0, false, false, 0.0),
    ];
    let total: f32 = SCRIPT.iter().map(|s| s.0).sum();
    for (member, mut input) in &mut query {
        let mut t = (time.elapsed_secs() + hash01(member.index, 9) * total) % total;
        let mut step = SCRIPT[0];
        for s in SCRIPT {
            if t < s.0 {
                step = s;
                break;
            }
            t -= s.0;
        }
        let (_, speed, strafe, sneak, air, turn) = step;
        // Character frame: the controller would use the world velocity; here the entity keeps
        // its rotation, so rig-forward is the entity's local direction.
        let forward = gothic_animation::locomotion::FORWARD;
        let right = gothic_animation::locomotion::RIGHT;
        let mut velocity = forward * speed + right * strafe;
        if air {
            velocity.y = if t < 0.9 { 260.0 } else { -200.0 };
        }
        input.velocity = velocity;
        input.grounded = !air || t > 1.2;
        input.stance = member.stance;
        input.sneak = sneak;
        input.turn_error = turn;
    }
}

/// Keyboard control of the single hero through the locomotion input.
fn hero_keys(keys: Res<ButtonInput<KeyCode>>, mut query: Query<&mut LocomotionInput, With<Hero>>) {
    for mut input in &mut query {
        let preset = |input: &mut LocomotionInput,
                      speed: f32,
                      strafe: f32,
                      sneak: bool,
                      grounded: bool,
                      up: f32,
                      turn: f32| {
            input.velocity = FORWARD * speed + RIGHT * strafe + Vec3::Y * up;
            input.sneak = sneak;
            input.grounded = grounded;
            input.turn_error = turn;
        };
        let table = [
            (KeyCode::Digit1, [0.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
            (KeyCode::Digit2, [150.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
            (KeyCode::Digit3, [380.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
            (KeyCode::Digit4, [490.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
            (KeyCode::Digit5, [140.0, 0.0, 1.0, 1.0, 0.0, 0.0]),
            (KeyCode::Digit6, [0.0, 0.0, 0.0, 0.0, 300.0, 0.0]),
            (KeyCode::Digit7, [0.0, 0.0, 0.0, 0.0, -300.0, 0.0]),
            (KeyCode::Digit8, [0.0, 130.0, 0.0, 1.0, 0.0, 0.0]),
            (KeyCode::Digit9, [0.0, 0.0, 0.0, 1.0, 0.0, 1.5]),
        ];
        for (key, v) in table {
            if keys.just_pressed(key) {
                preset(&mut input, v[0], v[1], v[2] > 0.5, v[3] > 0.5, v[4], v[5]);
            }
        }
        if keys.just_pressed(KeyCode::KeyF) {
            const CYCLE: [Stance; 6] = [
                Stance::Unarmed,
                Stance::OneHand,
                Stance::OneHandShield,
                Stance::TwoHand,
                Stance::Fist,
                Stance::Staff,
            ];
            let at = CYCLE.iter().position(|s| *s == input.stance).unwrap_or(0);
            input.stance = CYCLE[(at + 1) % CYCLE.len()];
        }
    }
}

/// Applies `--overlay <action>` (upper-body layer) and `--speed` to the hero once.
fn hero_overlay(
    config: Res<Config>,
    mut library: ResMut<ActorLibrary>,
    mut done: Local<bool>,
    mut query: Query<(&mut Animator, &mut LocomotionInput), With<Hero>>,
) {
    if *done {
        return;
    }
    for (mut animator, mut input) in &mut query {
        *done = true;
        input.velocity = gothic_animation::locomotion::FORWARD * config.speed;
        let Some(action) = &config.overlay else {
            continue;
        };
        let query = gothic_animation::names::MotionQuery {
            pose: Some("Stand"),
            left: Some("None"),
            right: Some("None"),
            pn: Some("P0"),
            action: Some(action),
            ..Default::default()
        };
        let Some(name) = library
            .catalog
            .find(&animator.rig.family, &query)
            .map(str::to_string)
        else {
            eprintln!("no motion for overlay action {action:?}");
            continue;
        };
        let rig = animator.rig.clone();
        match library.motion(&rig, &name) {
            Ok(motion) => {
                eprintln!("overlay {name}");
                animator.set_overlay(Some(gothic_animation::Overlay {
                    motion,
                    time: 0.0,
                    rate: 1.0,
                    weight: 1.0,
                    mask: rig.upper_body_mask().into(),
                }));
            }
            Err(error) => eprintln!("overlay {name}: {error:#}"),
        }
    }
}

/// Deterministic animation times for single-actor screenshots.
fn smoke_drive(
    config: Res<Config>,
    frames: Res<Frames>,
    mut query: Query<&mut Animator, With<Hero>>,
) {
    if !config.smoke || config.crowd > 0 || config.motion.is_none() {
        return;
    }
    for mut animator in &mut query {
        animator.paused = true;
        let duration = animator.current_clip().map_or(0.0, |c| c.duration);
        let t = if frames.count < 130 {
            config.times[0]
        } else {
            config.times[1] * duration * 2.0
        };
        animator.seek(t);
    }
}

fn orbit(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    mut camera: Query<(&mut Orbit, &mut Transform)>,
) {
    let Ok((mut orbit, mut transform)) = camera.single_mut() else {
        return;
    };
    if buttons.pressed(MouseButton::Left) || buttons.pressed(MouseButton::Right) {
        orbit.yaw -= mouse.delta.x * 0.005;
        orbit.pitch = (orbit.pitch + mouse.delta.y * 0.005).clamp(-1.4, 1.4);
    }
    let dt = time.delta_secs().min(0.1);
    if keys.pressed(KeyCode::KeyA) {
        orbit.yaw += dt;
    }
    if keys.pressed(KeyCode::KeyD) {
        orbit.yaw -= dt;
    }
    if keys.pressed(KeyCode::KeyW) {
        orbit.distance *= 1.0 - dt;
    }
    if keys.pressed(KeyCode::KeyS) {
        orbit.distance *= 1.0 + dt;
    }
    orbit.distance = (orbit.distance * (1.0 - scroll.delta.y * 0.1)).max(5.0);
    let direction = Quat::from_euler(EulerRot::YXZ, orbit.yaw, -orbit.pitch, 0.0) * Vec3::Z;
    transform.translation = orbit.target + direction * orbit.distance;
    transform.look_at(orbit.target, Vec3::Y);
}

/// Screenshots, benchmark sampling and exit handling.
fn capture(
    mut commands: Commands,
    config: Res<Config>,
    time: Res<Time>,
    stats: Res<AnimationStats>,
    mut frames: ResMut<Frames>,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    frames.count += 1;
    let frame = frames.count;
    if config.bench && (150..450).contains(&frame) {
        frames.deltas.push(time.delta_secs());
        frames.animation.push([
            stats.locomotion.as_secs_f32() * 1.0e6,
            stats.animate.as_secs_f32() * 1.0e6,
            stats.write.as_secs_f32() * 1.0e6,
        ]);
    }
    if !config.smoke && !config.bench {
        return;
    }
    if config.crowd > 0 {
        if frame == 120 {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(config.screenshot.clone()));
        }
        if config.bench && frame == 450 {
            report(&frames, &stats, config.crowd);
            commands.write_message(AppExit::Success);
        } else if !config.bench && frame == 200 {
            commands.write_message(AppExit::Success);
        }
    } else {
        if frame == 80 {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(config.screenshot.clone()));
        }
        if frame == 180 {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(config.second_screenshot.clone()));
        }
        if frame == 260 {
            commands.write_message(AppExit::Success);
        }
    }
}

fn report(frames: &Frames, stats: &AnimationStats, crowd: usize) {
    let mut sorted = frames.deltas.clone();
    sorted.sort_by(f32::total_cmp);
    if sorted.is_empty() {
        return;
    }
    let mean = sorted.iter().sum::<f32>() / sorted.len() as f32;
    let p95 = sorted[(sorted.len() as f32 * 0.95) as usize % sorted.len()];
    let n = frames.animation.len().max(1) as f32;
    let avg = |i: usize| frames.animation.iter().map(|a| a[i]).sum::<f32>() / n;
    println!(
        "BENCH crowd={crowd} characters={} frames={} frame_ms mean={:.2} p95={:.2} max={:.2} | animation_us locomotion={:.0} animate={:.0} write={:.0} total={:.0}",
        stats.characters,
        sorted.len(),
        mean * 1000.0,
        p95 * 1000.0,
        sorted[sorted.len() - 1] * 1000.0,
        avg(0),
        avg(1),
        avg(2),
        avg(0) + avg(1) + avg(2)
    );
}

fn update_title(
    config: Res<Config>,
    stats: Res<AnimationStats>,
    query: Query<(&Animator, Option<&Locomotion>), With<Hero>>,
    mut windows: Query<&mut Window>,
) {
    for mut window in &mut windows {
        window.title = if config.crowd > 0 {
            format!(
                "Gothic Reforged | crowd {} | animation {:.0} us/frame",
                stats.characters, stats.average_micros
            )
        } else if let Ok((animator, loco)) = query.single() {
            format!(
                "Gothic Reforged | {} | {:?} | keys 1-9 states, F stance",
                animator.current_name().unwrap_or("-"),
                loco.map(|l| l.key.kind)
            )
        } else {
            "Gothic Reforged / actor viewer".into()
        };
    }
}
