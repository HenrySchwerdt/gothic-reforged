//! Greybox VR sandbox (synthetic content only): everything from VR-01..VR-06 in one level.
//!
//! * start area: comfort locomotion over stairs, ramps and a platform (VR-02);
//! * weapon table: sword, axe, two-handed sword, mace, bow and crates (VR-03);
//! * training dummies (VR-04);
//! * a ragdoll swordsman who duels you (VR-05);
//! * an archery range with targets at 5, 10 and 20 m behind the start (VR-06).
//!
//! ```text
//! cargo run -p gothic-vr-sandbox                     # desktop debug mode (mouse-driven hand)
//! cargo run -p gothic-vr-sandbox -- --stereo         # side-by-side eyes
//! cargo run -p gothic-vr-sandbox -- --peaceful       # the NPC does not attack
//! cargo run -p gothic-vr-sandbox -- --strength 120   # player strength (Gothic STR)
//! cargo run -p gothic-vr-sandbox -- --headless       # scripted CI duel, exits 0 on success
//! cargo run -p gothic-vr-sandbox -- --screenshot shot.png [--screenshot-frame 240]
//! ```
//!
//! Desktop controls (see `docs/vr/README.md`): WASD move, Q/E snap turn, T teleport, right
//! mouse + move look, mouse moves the active hand (swing!), wheel = hand depth, Tab switch
//! hand, G/F grip active/other hand, LMB trigger, C crouch, 1 hand to quiver, 2 right hand to
//! the bow string, H toggle hitzones, F1 stereo.
use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use gothic_vr::desktop::{HAND_MAX, HAND_MIN};
use gothic_vr::greybox::{self, GreyboxBox};
use gothic_vr::locomotion::ComfortSettings;
use gothic_vr::rig::StereoMode;
use gothic_vr::{
    DesktopXr, Hand, Pose, VrDisplay, VrPlayer, VrPlugin, VrRenderPlugin, VrSet, VrWorld,
    XrBackendKind, XrFrame,
};
use gothic_vr_archery::{ArrowHitMessage, FlyingArrow, HeldArrow, VrArcheryPlugin};
use gothic_vr_melee::{
    ClashMessage, HitMessage, HitzoneSet, PlayerAvatar, PlayerHitzonesPlugin, StabMessage,
    VrMeleePlugin, spawn_dummy,
};
use gothic_vr_npc::{NpcBone, VrNpc, VrNpcPlugin, spawn_npc};
use gothic_vr_physics::props::{self, PartShape};
use gothic_vr_physics::{
    PhysicsHand, Prop, VrPhysicsPlugin, VrStrength, spawn_prop, spawn_static_greybox,
};
use std::time::Duration;

/// Player start (feet), looking down -Z.
const START: Vec3 = Vec3::new(0.0, 0.0, 4.0);
const NPC_AT: Vec3 = Vec3::new(0.0, 0.0, 0.5);

#[derive(Resource, Default)]
struct Score {
    player_hits: u32,
    hits_taken: u32,
    clashes: u32,
    arrow_hits: u32,
}

#[derive(Resource)]
struct ShowHitzones(bool);

#[derive(Resource)]
struct Peaceful(bool);

fn arg<T: std::str::FromStr>(args: &[String], flag: &str) -> Option<T> {
    let i = args.iter().position(|a| a == flag)?;
    args.get(i + 1)?.parse().ok()
}

fn main() -> AppExit {
    let args: Vec<String> = std::env::args().collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    let headless = has("--headless");
    let mut app = App::new();
    if headless {
        app.add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::ZERO)))
            .add_plugins(TransformPlugin)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 90.0,
            )))
            .insert_resource(Time::<Fixed>::from_hz(90.0))
            .add_plugins(VrPlugin {
                backend: XrBackendKind::Scripted,
            })
            .insert_resource(DesktopXr {
                script: Some(gothic_vr::desktop::InputScript::new(vec![(
                    90 * 10,
                    Default::default(),
                )])),
                ..default()
            })
            .add_systems(Update, finish_headless.after(VrSet::Sync));
    } else {
        app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Gothic Reforged VR sandbox (desktop debug mode)".into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(Time::<Fixed>::from_hz(90.0))
        .add_plugins((
            VrPlugin {
                backend: XrBackendKind::from_env(),
            },
            VrRenderPlugin,
        ))
        .insert_resource(VrDisplay {
            stereo: if has("--stereo") {
                StereoMode::SideBySide
            } else {
                StereoMode::Mono
            },
            ..default()
        })
        .insert_resource(ShowHitzones(false))
        .add_systems(Startup, spawn_visual_level)
        .add_systems(
            Update,
            (
                attach_prop_meshes,
                attach_bone_meshes,
                attach_hand_meshes,
                attach_arrow_meshes,
                desktop_keys,
                draw_hitzones,
            ),
        );
        if let Some(path) = arg::<String>(&args, "--screenshot") {
            let at = arg(&args, "--screenshot-frame").unwrap_or(240);
            app.insert_resource(gothic_vr::render::CaptureAndExit::new(path, at));
        }
    }
    app.add_plugins((
        VrPhysicsPlugin::default(),
        VrMeleePlugin,
        PlayerHitzonesPlugin,
        VrNpcPlugin,
        VrArcheryPlugin,
    ))
    .insert_resource(VrStrength(arg(&args, "--strength").unwrap_or(60.0)))
    .insert_resource(Peaceful(has("--peaceful")))
    .init_resource::<Score>()
    .insert_resource(VrWorld(greybox::collision_world(&layout())))
    .insert_resource(VrPlayer::new(
        START,
        ComfortSettings::default(),
        &XrFrame::default().head,
    ))
    .add_systems(Startup, spawn_level)
    .add_systems(Update, score);
    app.run()
}

/// The default greybox plus the sandbox's furniture.
fn layout() -> Vec<GreyboxBox> {
    let mut boxes = greybox::default_layout();
    boxes.push(
        GreyboxBox::new(
            "weapon table",
            Vec3::new(-1.6, 0.425, 2.9),
            Vec3::new(0.9, 0.425, 0.35),
        )
        .colored([0.45, 0.32, 0.2]),
    );
    boxes
}

fn spawn_level(mut commands: Commands, peaceful: Res<Peaceful>) {
    spawn_static_greybox(&mut commands, &layout());
    // Weapon table (top at y = 0.85), weapons lying flat with the grip toward the player.
    let flat = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2) * Quat::from_rotation_y(0.0);
    let on_table = |x: f32| Pose::new(Vec3::new(x, 0.96, 3.1), flat);
    for (spec, x) in [
        (props::sword(), -2.3),
        (props::axe(), -1.95),
        (props::two_handed_sword(), -1.6),
        (props::mace(), -1.25),
    ] {
        spawn_prop(&mut commands, &spec, on_table(x));
    }
    spawn_prop(
        &mut commands,
        &props::bow(),
        Pose::new(
            Vec3::new(-0.9, 0.9, 2.9),
            Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
        ),
    );
    for (i, z) in [2.0f32, 1.6].into_iter().enumerate() {
        spawn_prop(
            &mut commands,
            &props::crate_box(Vec3::splat(0.15 + 0.05 * i as f32), 3.0 + 3.0 * i as f32),
            Pose::new(Vec3::new(-2.8, 0.3, z), Quat::IDENTITY),
        );
    }
    // Training dummies to the right.
    spawn_dummy(&mut commands, Vec3::new(2.0, 0.0, 2.6));
    spawn_dummy(&mut commands, Vec3::new(2.4, 0.0, 1.2));
    // Archery range behind the start (+Z): targets at 5, 10, 20 m.
    for d in [5.0, 10.0, 20.0] {
        spawn_dummy(&mut commands, Vec3::new(0.0, 0.0, START.z + d));
    }
    // The duellist.
    let npc = spawn_npc(
        &mut commands,
        NPC_AT,
        std::f32::consts::PI,
        Some(&props::sword()),
        7,
    );
    if peaceful.0 {
        commands.queue(move |world: &mut World| {
            if let Some(mut n) = world.get_mut::<VrNpc>(npc) {
                n.ai.cooldown = f32::INFINITY;
            }
        });
    }
}

fn spawn_visual_level(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    gothic_vr::render::spawn_greybox(&mut commands, &mut meshes, &mut materials, &layout());
    gothic_vr::render::spawn_greybox_lighting(&mut commands);
    info!(
        "VR sandbox: WASD move, Q/E turn, T teleport, RMB look, mouse = active hand, wheel = \
         depth, Tab switch hand, G/F grip, LMB trigger, C crouch, 1 quiver, 2 string, H hitzones"
    );
}

fn part_mesh(shape: &PartShape) -> Mesh {
    match *shape {
        PartShape::Box(h) => Cuboid::from_size(h * 2.0).into(),
        PartShape::Capsule {
            radius,
            half_length,
        } => Capsule3d::new(radius, half_length * 2.0).into(),
    }
}

fn attach_prop_meshes(
    mut commands: Commands,
    props: Query<(Entity, &Prop), Added<Prop>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (e, p) in &props {
        for part in &p.0.parts {
            let child = commands
                .spawn((
                    Mesh3d(meshes.add(part_mesh(&part.shape))),
                    MeshMaterial3d(materials.add(Color::srgb(
                        part.color[0],
                        part.color[1],
                        part.color[2],
                    ))),
                    Transform::from_translation(part.offset).with_rotation(part.rotation),
                ))
                .id();
            commands.entity(e).add_child(child);
        }
    }
}

fn attach_bone_meshes(
    mut commands: Commands,
    bones: Query<(Entity, &NpcBone), Added<NpcBone>>,
    npcs: Query<&VrNpc>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let skin = materials.add(Color::srgb(0.7, 0.55, 0.45));
    for (e, b) in &bones {
        let Ok(npc) = npcs.get(b.npc) else {
            continue;
        };
        let len = npc
            .ragdoll
            .position(b.bone.0)
            .distance(npc.ragdoll.position(b.bone.1));
        let r = if len > 0.3 { 0.07 } else { 0.05 };
        let child = commands
            .spawn((
                Mesh3d(meshes.add(Capsule3d::new(r, (len - r).max(0.01)))),
                MeshMaterial3d(skin.clone()),
                Transform::default(),
            ))
            .id();
        commands
            .entity(e)
            .insert(Visibility::default())
            .add_child(child);
    }
}

fn attach_hand_meshes(
    mut commands: Commands,
    hands: Query<Entity, Added<PhysicsHand>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for e in &hands {
        commands.entity(e).insert((
            Mesh3d(meshes.add(Sphere::new(gothic_vr_physics::hands::HAND_RADIUS))),
            MeshMaterial3d(materials.add(Color::srgb(0.85, 0.7, 0.6))),
        ));
    }
}

#[allow(clippy::type_complexity)]
fn attach_arrow_meshes(
    mut commands: Commands,
    arrows: Query<Entity, Or<(Added<FlyingArrow>, Added<HeldArrow>)>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for e in &arrows {
        let child = commands
            .spawn((
                Mesh3d(meshes.add(Capsule3d::new(0.006, 0.74))),
                MeshMaterial3d(materials.add(Color::srgb(0.5, 0.35, 0.2))),
                Transform::from_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            ))
            .id();
        commands
            .entity(e)
            .insert(Visibility::default())
            .add_child(child);
    }
}

/// Sandbox shortcuts on top of the desktop emulator.
fn desktop_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut desktop: ResMut<DesktopXr>,
    mut show: ResMut<ShowHitzones>,
    mut display: ResMut<VrDisplay>,
) {
    let quiver = gothic_vr_archery::bow::Quiver::default();
    if keys.just_pressed(KeyCode::Digit1) {
        let hand = desktop.emulator.active;
        desktop.emulator.hand_offsets[hand.index()] = Vec3::new(
            quiver.offset.x * hand.sign(),
            quiver.offset.y,
            quiver.offset.z,
        )
        .clamp(HAND_MIN, HAND_MAX);
    }
    if keys.just_pressed(KeyCode::Digit2) {
        // Right hand onto the string of a bow held in the left hand.
        let left = desktop.emulator.hand_offsets[Hand::Left.index()];
        desktop.emulator.hand_offsets[Hand::Right.index()] =
            (left + props::bow_string_rest()).clamp(HAND_MIN, HAND_MAX);
        desktop.emulator.active = Hand::Right;
    }
    if keys.just_pressed(KeyCode::KeyH) {
        show.0 = !show.0;
    }
    if keys.just_pressed(KeyCode::F1) {
        display.stereo = match display.stereo {
            StereoMode::Mono => StereoMode::SideBySide,
            StereoMode::SideBySide => StereoMode::Mono,
        };
    }
}

fn draw_hitzones(
    mut gizmos: Gizmos,
    show: Res<ShowHitzones>,
    sets: Query<(&HitzoneSet, Has<VrNpc>, Has<PlayerAvatar>)>,
) {
    for (s, npc, me) in &sets {
        // Dummies always show their zones; the NPC on request; never the player's own.
        if me || (!show.0 && npc) {
            continue;
        }
        let color = if npc {
            Color::srgb(1.0, 0.3, 0.3)
        } else {
            Color::srgb(0.9, 0.8, 0.2)
        };
        for z in &s.zones {
            gizmos.primitive_3d(
                &Capsule3d::new(z.radius, z.half_length * 2.0),
                Isometry3d::new(z.pose.position, z.pose.rotation),
                color,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn score(
    mut score: ResMut<Score>,
    mut hits: MessageReader<HitMessage>,
    mut clashes: MessageReader<ClashMessage>,
    mut stabs: MessageReader<StabMessage>,
    mut arrows: MessageReader<ArrowHitMessage>,
    avatar: Query<Entity, With<PlayerAvatar>>,
    names: Query<&Name>,
) {
    let me = avatar.iter().next();
    let name = |e: Entity| names.get(e).map_or("?".to_string(), |n| n.to_string());
    for h in hits.read() {
        if Some(h.victim) == me {
            score.hits_taken += 1;
        } else if h.attacker == me {
            score.player_hits += 1;
        }
        info!(
            "HIT {} -> {} {:?} {:?} {:.1} m/s quality {:.2}",
            h.attacker.map_or("-".into(), name),
            name(h.victim),
            h.bone,
            h.damage_type,
            h.impact_speed,
            h.quality
        );
    }
    for c in clashes.read() {
        score.clashes += 1;
        info!("{:?}! recoil {:.2}s", c.kind, c.recoil);
    }
    for s in stabs.read() {
        info!("{s:?}");
    }
    for a in arrows.read() {
        score.arrow_hits += 1;
        info!(
            "ARROW {:?} draw {:.2} damage {:.1} at {:.1} m/s",
            a.bone, a.draw_factor, a.damage, a.impact_speed
        );
    }
}

/// CI: the player stands idle in the start area while the duellist comes over and attacks.
fn finish_headless(
    desktop: Res<DesktopXr>,
    score: Res<Score>,
    npcs: Query<&VrNpc>,
    player: Res<VrPlayer>,
    mut exit: MessageWriter<AppExit>,
) {
    if !desktop.script_done {
        return;
    }
    let attacks: u32 = npcs.iter().map(|n| n.ai.attacks).sum();
    let finite = npcs
        .iter()
        .all(|n| n.ragdoll.pose().is_finite() && n.ai.root.is_finite())
        && player.ctrl.position.is_finite();
    println!(
        "vr-sandbox headless: npc attacks {attacks}, hits on player {}, clashes {}, finite {finite}",
        score.hits_taken, score.clashes
    );
    if attacks > 0 && score.hits_taken > 0 && finite {
        println!("vr-sandbox headless: OK");
        exit.write(AppExit::Success);
    } else {
        eprintln!("vr-sandbox headless: FAILED");
        exit.write(AppExit::from_code(1));
    }
}
