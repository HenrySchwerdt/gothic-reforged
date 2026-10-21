//! Greybox VR demo (VR-01/VR-02): desktop fallback or scripted headless run.
//!
//! ```text
//! cargo run -p gothic-vr --example greybox                  # window, keyboard/mouse emulation
//! cargo run -p gothic-vr --example greybox -- --stereo      # side-by-side eye views
//! cargo run -p gothic-vr --example greybox -- --headless    # scripted CI run, exits 0 on success
//! cargo run -p gothic-vr --example greybox -- --screenshot shot.png   # capture after 90 frames, exit
//! GOTHIC_VR_BACKEND=openxr cargo run -p gothic-vr --example greybox   # see docs/vr/OPENXR.md
//! ```
use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use gothic_vr::desktop::{DesktopInput, InputScript};
use gothic_vr::greybox::{self, STAIR_RISE, STAIR_STEPS};
use gothic_vr::locomotion::ComfortSettings;
use gothic_vr::rig::StereoMode;
use gothic_vr::{
    DesktopXr, Pose, VrBody, VrDisplay, VrInputRes, VrPlayer, VrPlugin, VrRenderPlugin, VrWorld,
    XrBackendKind, XrFrame,
};
use std::time::Duration;

const START: Vec3 = Vec3::new(4.0, 0.0, -1.0);

fn main() -> AppExit {
    let args: Vec<String> = std::env::args().collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    let mut app = App::new();
    let boxes = greybox::default_layout();
    if has("--headless") {
        app.add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::ZERO)))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 72.0,
            )))
            .add_plugins(VrPlugin {
                backend: XrBackendKind::Scripted,
            })
            .insert_resource(DesktopXr {
                script: Some(ci_script()),
                ..default()
            })
            .add_systems(Update, finish_headless.after(gothic_vr::VrSet::Sync));
    } else {
        app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Gothic Reforged VR greybox (desktop fallback)".into(),
                ..default()
            }),
            ..default()
        }))
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
        .insert_resource(boxes_resource(boxes.clone()))
        .add_systems(Startup, spawn_scene)
        .add_systems(Update, toggle_stereo);
    }
    if let Some(i) = args.iter().position(|a| a == "--screenshot")
        && let Some(path) = args.get(i + 1)
    {
        app.insert_resource(gothic_vr::render::CaptureAndExit::new(path.clone(), 90));
    }
    app.insert_resource(VrWorld(greybox::collision_world(&boxes)))
        .insert_resource(VrPlayer::new(
            START,
            ComfortSettings::default(),
            &XrFrame::default().head,
        ));
    app.run()
}

#[derive(Resource)]
struct Boxes(Vec<greybox::GreyboxBox>);

fn boxes_resource(b: Vec<greybox::GreyboxBox>) -> Boxes {
    Boxes(b)
}

fn spawn_scene(
    mut commands: Commands,
    boxes: Res<Boxes>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    gothic_vr::render::spawn_greybox(&mut commands, &mut meshes, &mut materials, &boxes.0);
    gothic_vr::render::spawn_greybox_lighting(&mut commands);
    info!(
        "Desktop VR fallback: WASD move, Q/E snap turn, T teleport aim, RMB+mouse look, mouse \
         moves the active hand, Tab switches hand, G/F grip, LMB trigger, C crouch, F1 stereo"
    );
}

fn toggle_stereo(keys: Res<ButtonInput<KeyCode>>, mut display: ResMut<VrDisplay>) {
    if keys.just_pressed(KeyCode::F1) {
        display.stereo = match display.stereo {
            StereoMode::Mono => StereoMode::SideBySide,
            StereoMode::SideBySide => StereoMode::Mono,
        };
    }
}

/// Walk up the stairs, look around, snap-turn, crouch, then teleport back down.
fn ci_script() -> InputScript {
    let i = DesktopInput::default;
    InputScript::new(vec![
        (10, i()),
        (
            150,
            DesktopInput {
                move_axes: Vec2::Y,
                ..i()
            },
        ),
        (
            30,
            DesktopInput {
                hand_delta: Vec2::new(8.0, -4.0),
                trigger: true,
                ..i()
            },
        ),
        (
            1,
            DesktopInput {
                turn_axis: 1.0,
                ..i()
            },
        ),
        (5, i()),
        (
            1,
            DesktopInput {
                turn_axis: 1.0,
                ..i()
            },
        ),
        (
            72,
            DesktopInput {
                crouch: true,
                ..i()
            },
        ),
        (72, i()),
    ])
}

fn finish_headless(
    desktop: Res<DesktopXr>,
    player: Res<VrPlayer>,
    body: Res<VrBody>,
    input: Res<VrInputRes>,
    mut exit: MessageWriter<AppExit>,
) {
    if !desktop.script_done {
        return;
    }
    let feet = player.ctrl.position / gothic_vr::CM_PER_M;
    let platform = STAIR_RISE * STAIR_STEPS as f32;
    let body_ok = body.pose.as_ref().is_some_and(|p| p.is_finite());
    let turned = player.loco.yaw();
    let head: Pose = player.world(&input.0.head());
    println!(
        "greybox headless: frames={} feet={feet:.2} head={:.2} yaw={:.1}deg body_ok={body_ok}",
        input.0.frames,
        head.position,
        turned.to_degrees()
    );
    let ok = (feet.y - platform).abs() < 0.05
        && body_ok
        && (turned.to_degrees() + 60.0).abs() < 0.5
        && feet.z < -8.0;
    if ok {
        println!("greybox headless: OK (climbed stairs onto the platform, turned, body solved)");
        exit.write(AppExit::Success);
    } else {
        eprintln!("greybox headless: FAILED");
        exit.write(AppExit::from_code(1));
    }
}
