//! Greybox demo: a grey test scene cycling a whole day in two minutes and every weather kind
//! (clear, rain, storm, snow, sandstorm) every 15 seconds. The ground shows the wetness and
//! snow-cover parameters.
//!
//! `cargo run -p gothic-env-synth --example greybox --features demo`
//! Keys: Space pause, W next weather, Up/Down time speed, A automatic weather.
//! `--screenshots DIR` captures a few frames into DIR and exits (works under `xvfb-run`).
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use gothic_env_synth::WeatherKind;
use gothic_env_synth::plugin::{
    CurrentEnv, EnvironmentCamera, EnvironmentPlugin, EnvironmentState, Moon, Sun, next_weather,
};
use std::path::PathBuf;

const DAY_SECONDS: f64 = 120.0;
const WEATHER_SECONDS: f32 = 15.0;

#[derive(Resource)]
struct Demo {
    weather_timer: f32,
    screenshots: Option<PathBuf>,
    frame: u32,
}

#[derive(Component)]
struct Ground;

#[derive(Component)]
struct Hud;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let screenshots = args
        .iter()
        .position(|a| a == "--screenshots")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "gothic-env-synth greybox".into(),
                resolution: (1280, 720).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EnvironmentPlugin {
            start_hour: 5.0,
            hours_per_second: 24.0 / DAY_SECONDS,
            ..default()
        })
        .insert_resource(Demo {
            weather_timer: 0.0,
            screenshots,
            frame: 0,
        })
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (cycle_weather, keys, ground_surface, hud, orbit, capture),
        )
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut state: ResMut<EnvironmentState>,
) {
    state.env.weather.automatic = false;
    commands.spawn((
        Camera3d::default(),
        Tonemapping::AcesFitted,
        Transform::from_xyz(30.0, 8.0, 30.0).looking_at(Vec3::new(0.0, 3.0, 0.0), Vec3::Y),
        EnvironmentCamera,
    ));
    commands.spawn((
        Sun,
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::default(),
    ));
    commands.spawn((Moon, DirectionalLight::default(), Transform::default()));
    commands.spawn((
        Ground,
        Mesh3d(meshes.add(Plane3d::default().mesh().size(400.0, 400.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.45, 0.45, 0.42),
            perceptual_roughness: 0.9,
            ..default()
        })),
    ));
    let grey = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 0.6, 0.6),
        perceptual_roughness: 0.7,
        ..default()
    });
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    for i in 0..24 {
        let a = i as f32 / 24.0 * std::f32::consts::TAU;
        let r = 12.0 + (i % 3) as f32 * 9.0;
        let h = 2.0 + (i * 7 % 5) as f32 * 2.0;
        commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(grey.clone()),
            Transform::from_xyz(a.cos() * r, h / 2.0, a.sin() * r)
                .with_scale(Vec3::new(2.0, h, 2.0)),
        ));
    }
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(3.0).mesh().uv(32, 16))),
        MeshMaterial3d(grey),
        Transform::from_xyz(0.0, 3.0, 0.0),
    ));
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(18.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            top: px(10),
            ..default()
        },
    ));
}

fn cycle_weather(time: Res<Time>, mut demo: ResMut<Demo>, mut state: ResMut<EnvironmentState>) {
    if state.env.weather.automatic || state.paused {
        return;
    }
    demo.weather_timer += time.delta_secs();
    if demo.weather_timer >= WEATHER_SECONDS {
        demo.weather_timer = 0.0;
        let next = next_weather(state.env.weather.target());
        state.env.force_weather(next);
    }
}

fn keys(input: Res<ButtonInput<KeyCode>>, mut state: ResMut<EnvironmentState>) {
    if input.just_pressed(KeyCode::Space) {
        state.paused = !state.paused;
    }
    if input.just_pressed(KeyCode::KeyW) {
        let next = next_weather(state.env.weather.target());
        state.env.force_weather(next);
    }
    if input.just_pressed(KeyCode::KeyA) {
        state.env.weather.automatic = !state.env.weather.automatic;
    }
    if input.just_pressed(KeyCode::ArrowUp) {
        state.time_scale *= 2.0;
    }
    if input.just_pressed(KeyCode::ArrowDown) {
        state.time_scale /= 2.0;
    }
}

/// Wet ground: darker and glossier; snow: towards white and rough.
fn ground_surface(
    current: Res<CurrentEnv>,
    ground: Query<&MeshMaterial3d<StandardMaterial>, With<Ground>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let f = &current.0;
    for h in &ground {
        if let Some(mut m) = materials.get_mut(&h.0) {
            let dry = Vec3::new(0.45, 0.45, 0.42) * (1.0 - 0.45 * f.wetness);
            let c = dry.lerp(Vec3::splat(0.92), f.snow_cover);
            m.base_color = Color::srgb(c.x, c.y, c.z);
            m.perceptual_roughness =
                (0.9 - 0.75 * f.wetness).max(0.1) * (1.0 - f.snow_cover) + 0.8 * f.snow_cover;
        }
    }
}

fn hud(
    current: Res<CurrentEnv>,
    state: Res<EnvironmentState>,
    mut text: Query<&mut Text, With<Hud>>,
) {
    let f = &current.0;
    for mut t in &mut text {
        let h = f.hour;
        t.0 = format!(
            "{:02}:{:02}  {:?} ({:.0}%)  sun {:.0} deg {:.0} lx  EV {:.1}  wet {:.2}  snow {:.2}{}\n[Space] pause  [W] next weather  [A] auto weather ({})  [Up/Down] speed x{}",
            h as u32,
            (h.fract() * 60.0) as u32,
            f.weather,
            f.weather_blend * 100.0,
            f.sun_elevation_deg,
            f.sun_illuminance,
            f.exposure_ev100,
            f.wetness,
            f.snow_cover,
            if state.paused { "  PAUSED" } else { "" },
            if state.env.weather.automatic {
                "on"
            } else {
                "off"
            },
            state.time_scale,
        );
    }
}

fn orbit(time: Res<Time>, mut cams: Query<&mut Transform, With<EnvironmentCamera>>) {
    let a = time.elapsed_secs() * 0.05;
    for mut t in &mut cams {
        *t = Transform::from_xyz(a.cos() * 34.0, 7.0, a.sin() * 34.0)
            .looking_at(Vec3::new(0.0, 4.0, 0.0), Vec3::Y);
    }
}

/// With `--screenshots DIR`: one capture per weather kind (fast-forwarded), then exit.
fn capture(
    mut commands: Commands,
    mut demo: ResMut<Demo>,
    mut state: ResMut<EnvironmentState>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(dir) = demo.screenshots.clone() else {
        return;
    };
    demo.frame += 1;
    // (frame, hour, weather): settle for a while, then capture.
    const SHOTS: [(u32, f64, WeatherKind); 6] = [
        (60, 12.0, WeatherKind::Clear),
        (120, 19.2, WeatherKind::Clear),
        (180, 11.0, WeatherKind::Rain),
        (240, 15.0, WeatherKind::Storm),
        (300, 10.0, WeatherKind::Snow),
        (360, 14.0, WeatherKind::Sandstorm),
    ];
    for (i, (frame, hour, kind)) in SHOTS.iter().enumerate() {
        if demo.frame == frame - 50 {
            state.paused = true;
            state.env.clock.hours = *hour;
            state.env.force_weather(*kind);
            // Jump through the transition.
            let env = &mut state.env;
            for _ in 0..200 {
                env.weather.update(&env.weather_table, 1.0);
            }
            state.env.surface.wetness = if matches!(kind, WeatherKind::Rain | WeatherKind::Storm) {
                0.9
            } else {
                0.0
            };
            state.env.surface.snow_cover = if *kind == WeatherKind::Snow { 0.8 } else { 0.0 };
        }
        if demo.frame == *frame {
            let _ = std::fs::create_dir_all(&dir);
            let path = dir.join(format!("{i}_{kind:?}_{hour}.png"));
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path));
        }
    }
    if demo.frame == SHOTS[SHOTS.len() - 1].0 + 30 {
        exit.write(AppExit::Success);
    }
}
