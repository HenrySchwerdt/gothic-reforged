//! Greybox A/B scene for the render-plus profile: a courtyard with pillars, a slatted roof
//! (light shafts), props on the ground (AO, contact shadows), thin rails (aliasing) and a
//! pool (reflections).
//!
//! Interactive: `1`–`5` toggle SSAO / TAA / volumetric / soft shadows / SSR, `0` faithful,
//! `9` everything, `B` the frame budget. The window title shows what is on and the frame time.
//!
//! `--bench DIR`: runs faithful, each effect alone and everything, measures the frame time of
//! each (after a warm-up for pipeline compilation), saves a screenshot per run and writes
//! `DIR/frame_times.json`, then exits.
//!
//! `cargo run --release -p gothic-render-plus --example render_plus_demo --features demo[,pcss] --
//!     [--bench DIR] [--frames 240] [--size 1280x720]`
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::CascadeShadowConfigBuilder;
use bevy::light::FogVolume;
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::PresentMode;
use gothic_render_plus::plugin::{
    RenderPlusCamera, RenderPlusPlugin, RenderPlusReflective, RenderPlusState, RenderPlusSun,
};
use gothic_render_plus::{Effect, Preset, RenderProfile};
use std::path::PathBuf;

const WARMUP: u32 = 120;

#[derive(Resource)]
struct Bench {
    dir: Option<PathBuf>,
    frames: u32,
    run: usize,
    frame: u32,
    samples: Vec<f32>,
    results: Vec<String>,
    high: RenderProfile,
}

fn runs(high: &RenderProfile) -> Vec<(&'static str, RenderProfile)> {
    let mut v = vec![("faithful", RenderProfile::default())];
    for e in Effect::ALL {
        v.push((e.name(), high.only(e)));
    }
    v.push(("all", high.clone()));
    v
}

fn arg(name: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter()
        .position(|x| x == name)
        .and_then(|i| a.get(i + 1))
        .cloned()
}

fn main() {
    let (w, h) = arg("--size")
        .and_then(|s| {
            let (a, b) = s.split_once('x')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        })
        .unwrap_or((1280u32, 720u32));
    let mut high = RenderProfile::preset(Preset::High);
    high.budget.enabled = false;
    // No camera-wide fog: the scene's own fog volume fills the arcade, where the slatted roof
    // cuts the sun into shafts.
    high.volumetric.density = 0.0;
    let bench = Bench {
        dir: arg("--bench").map(PathBuf::from),
        frames: arg("--frames").and_then(|s| s.parse().ok()).unwrap_or(240),
        run: 0,
        frame: 0,
        samples: Vec::new(),
        results: Vec::new(),
        high,
    };
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "render-plus demo".into(),
                resolution: (w, h).into(),
                present_mode: PresentMode::AutoNoVsync,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(RenderPlusPlugin)
        .insert_resource(bench)
        .add_systems(Startup, setup)
        .add_systems(Update, (keys, title, run_bench))
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Tonemapping::AcesFitted,
        Msaa::Sample4,
        Transform::from_xyz(-14.0, 4.5, 16.0).looking_at(Vec3::new(2.0, 2.0, -2.0), Vec3::Y),
        RenderPlusCamera::default(),
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.75, 0.82, 1.0),
        brightness: 350.0,
        ..default()
    });
    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            color: Color::srgb(1.0, 0.93, 0.82),
            ..default()
        },
        CascadeShadowConfigBuilder {
            maximum_distance: 60.0,
            first_cascade_far_bound: 12.0,
            ..default()
        }
        .build(),
        Transform::from_xyz(10.0, 14.0, -6.0).looking_at(Vec3::ZERO, Vec3::Y),
        RenderPlusSun::default(),
    ));

    let mat = |m: &mut Assets<StandardMaterial>, c: Color, rough: f32| {
        m.add(StandardMaterial {
            base_color: c,
            perceptual_roughness: rough,
            ..default()
        })
    };
    let stone = mat(&mut materials, Color::srgb(0.55, 0.53, 0.5), 0.85);
    let dark = mat(&mut materials, Color::srgb(0.32, 0.3, 0.28), 0.9);
    let wood = mat(&mut materials, Color::srgb(0.45, 0.3, 0.18), 0.8);
    let metal = mat(&mut materials, Color::srgb(0.7, 0.7, 0.72), 0.35);
    let water = materials.add(StandardMaterial {
        base_color: Color::srgb(0.04, 0.07, 0.09),
        perceptual_roughness: 0.04,
        reflectance: 0.6,
        ..default()
    });

    // Ground and the pool.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(80.0, 80.0))),
        MeshMaterial3d(stone.clone()),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(10.0, 7.0))),
        MeshMaterial3d(water),
        Transform::from_xyz(-3.0, 0.02, 4.0),
        RenderPlusReflective,
    ));
    // Haze under the arcade (only drawn when volumetric fog is on).
    commands.spawn((
        FogVolume {
            density_factor: 0.25,
            absorption: 0.1,
            scattering: 0.6,
            ..default()
        },
        Transform::from_xyz(4.0, 3.0, -5.0).with_scale(Vec3::new(14.0, 6.0, 10.0)),
    ));
    let curb = meshes.add(Cuboid::new(10.6, 0.3, 0.3));
    for z in [0.35, 7.65] {
        commands.spawn((
            Mesh3d(curb.clone()),
            MeshMaterial3d(dark.clone()),
            Transform::from_xyz(-3.0, 0.15, z),
        ));
    }

    // Pillars and a slatted roof between them.
    let pillar = meshes.add(Cuboid::new(0.8, 6.0, 0.8));
    for x in [-2.0, 4.0, 10.0] {
        for z in [-8.0, -2.0] {
            commands.spawn((
                Mesh3d(pillar.clone()),
                MeshMaterial3d(stone.clone()),
                Transform::from_xyz(x, 3.0, z),
            ));
        }
    }
    let slat = meshes.add(Cuboid::new(13.0, 0.25, 0.6));
    for i in 0..8 {
        commands.spawn((
            Mesh3d(slat.clone()),
            MeshMaterial3d(wood.clone()),
            Transform::from_xyz(4.0, 6.1, -8.4 + i as f32 * 1.0),
        ));
    }
    // A closed room behind the arcade (dark interior, shafts through the slats).
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(14.0, 7.0, 0.5))),
        MeshMaterial3d(dark.clone()),
        Transform::from_xyz(4.0, 3.5, -10.0),
    ));

    // Props touching the ground: crates, a barrel ring, spheres.
    let crate_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    for (i, (x, z, r)) in [
        (0.5, -4.5, 0.2),
        (1.6, -4.3, -0.4),
        (1.0, -5.4, 0.9),
        (7.0, -5.0, 0.1),
    ]
    .into_iter()
    .enumerate()
    {
        commands.spawn((
            Mesh3d(crate_mesh.clone()),
            MeshMaterial3d(wood.clone()),
            Transform::from_xyz(x, 0.5 + if i == 2 { 1.0 } else { 0.0 }, z)
                .with_rotation(Quat::from_rotation_y(r)),
        ));
    }
    let sphere = meshes.add(Sphere::new(0.5).mesh().uv(32, 16));
    for i in 0..5 {
        commands.spawn((
            Mesh3d(sphere.clone()),
            MeshMaterial3d(metal.clone()),
            Transform::from_xyz(-8.0 + i as f32 * 1.2, 0.5, 1.5),
        ));
    }
    // Lanterns behind the pool: bright, on screen, so their reflections show SSR.
    let lantern = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.6, 0.25),
        emissive: LinearRgba::rgb(6.0, 2.8, 0.8),
        ..default()
    });
    for x in [-6.5, -0.5] {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.5, 1.6, 0.5))),
            MeshMaterial3d(lantern.clone()),
            Transform::from_xyz(x, 0.8, -0.3),
        ));
    }
    // Thin rails: sub-pixel geometry that shimmers without TAA.
    let rail = meshes.add(Cuboid::new(0.03, 1.2, 0.03));
    for i in 0..40 {
        commands.spawn((
            Mesh3d(rail.clone()),
            MeshMaterial3d(metal.clone()),
            Transform::from_xyz(-10.0 + i as f32 * 0.25, 0.6, 9.0),
        ));
    }
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(10.0, 0.05, 0.05))),
        MeshMaterial3d(metal),
        Transform::from_xyz(-5.1, 1.2, 9.0),
    ));
}

fn keys(input: Res<ButtonInput<KeyCode>>, mut profile: ResMut<RenderProfile>, bench: Res<Bench>) {
    if bench.dir.is_some() {
        return;
    }
    let toggles = [
        (KeyCode::Digit1, Effect::Ssao),
        (KeyCode::Digit2, Effect::Taa),
        (KeyCode::Digit3, Effect::Volumetric),
        (KeyCode::Digit4, Effect::SoftShadows),
        (KeyCode::Digit5, Effect::Ssr),
    ];
    for (key, e) in toggles {
        if input.just_pressed(key) {
            let on = !profile.is_on(e);
            profile.set(e, on);
        }
    }
    if input.just_pressed(KeyCode::Digit0) {
        *profile = RenderProfile::default();
    }
    if input.just_pressed(KeyCode::Digit9) {
        *profile = bench.high.clone();
    }
    if input.just_pressed(KeyCode::KeyB) {
        profile.budget.enabled = !profile.budget.enabled;
    }
}

fn title(state: Res<RenderPlusState>, time: Res<Time<Real>>, mut windows: Query<&mut Window>) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let active: Vec<&str> = state.plan.active().iter().map(|e| e.name()).collect();
    let text = format!(
        "render-plus demo | {} | {:.1} ms",
        if active.is_empty() {
            "faithful".to_string()
        } else {
            active.join(" + ")
        },
        time.delta_secs() * 1000.0
    );
    if window.title != text && time.elapsed_secs() % 0.25 < time.delta_secs() {
        window.title = text;
    }
}

fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() - 1) as f32 * p).round() as usize]
}

fn run_bench(
    mut commands: Commands,
    mut bench: ResMut<Bench>,
    mut profile: ResMut<RenderProfile>,
    time: Res<Time<Real>>,
    adapter: Option<Res<RenderAdapterInfo>>,
    windows: Query<&Window>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(dir) = bench.dir.clone() else {
        return;
    };
    let all = runs(&bench.high);
    let b = &mut *bench;
    if b.run >= all.len() {
        // Let the last screenshot reach the disk.
        b.frame += 1;
        if b.frame == 30 {
            let window = windows
                .single()
                .map(|w| format!("{}x{}", w.physical_width(), w.physical_height()))
                .unwrap_or_default();
            let (gpu, backend) = adapter
                .map(|a| (a.name.clone(), format!("{:?}", a.backend)))
                .unwrap_or_default();
            let json = format!(
                "{{\n  \"gpu\": {:?},\n  \"backend\": {:?},\n  \"resolution\": {:?},\n  \"pcss\": {},\n  \"runs\": [\n{}\n  ]\n}}\n",
                gpu,
                backend,
                window,
                cfg!(feature = "pcss"),
                b.results.join(",\n")
            );
            let _ = std::fs::write(dir.join("frame_times.json"), json);
            info!("wrote {}", dir.join("frame_times.json").display());
            exit.write(AppExit::Success);
        }
        return;
    }
    let (name, run_profile) = &all[b.run];
    if b.frame == 0 {
        *profile = run_profile.clone();
        b.samples.clear();
    }
    b.frame += 1;
    if b.frame > WARMUP {
        b.samples.push(time.delta_secs() * 1000.0);
    }
    if b.frame == WARMUP + b.frames {
        let mut s = b.samples.clone();
        s.sort_by(f32::total_cmp);
        let mean = s.iter().sum::<f32>() / s.len().max(1) as f32;
        let (p50, p95) = (percentile(&s, 0.5), percentile(&s, 0.95));
        info!("{name}: mean {mean:.2} ms, p50 {p50:.2}, p95 {p95:.2}");
        b.results.push(format!(
            "    {{ \"name\": {name:?}, \"mean_ms\": {mean:.3}, \"p50_ms\": {p50:.3}, \"p95_ms\": {p95:.3}, \"frames\": {} }}",
            s.len()
        ));
        let _ = std::fs::create_dir_all(&dir);
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(dir.join(format!("{}_{name}.png", b.run))));
        b.run += 1;
        b.frame = 0;
    }
}
