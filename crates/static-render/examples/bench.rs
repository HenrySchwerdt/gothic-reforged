//! Benchmark: a procedural town of houses, barrels, crates and trees (10k props by default)
//! with 3 shadow cascades and a deterministic camera flight; writes frame times as JSON.
//!
//! `cargo run --release -p gothic-static-render --example bench --features demo -- \
//!     [--instances 10000] [--frames 600] [--warmup 120] [--out bench.json] \
//!     [--no-cluster-culling] [--no-gpu-culling] [--no-lod] [--hide-after FRAMES]`
use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;
use bevy::window::PresentMode;
use gothic_static_render::plugin::{
    StaticCullCamera, StaticMeshDesc, StaticMeshes, StaticRenderPlugin, StaticRenderStats,
    StaticShadowLight,
};
use gothic_static_render::{Aabb, LodRanges};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Resource, Clone)]
struct Args {
    instances: usize,
    frames: usize,
    warmup: usize,
    out: PathBuf,
    cluster_culling: bool,
    gpu_culling: bool,
    lod: bool,
    hide_after: u16,
}

fn parse_args() -> Args {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let val = |k: &str| {
        a.iter()
            .position(|x| x == k)
            .and_then(|i| a.get(i + 1))
            .cloned()
    };
    let flag = |k: &str| a.iter().any(|x| x == k);
    Args {
        instances: val("--instances")
            .and_then(|v| v.parse().ok())
            .unwrap_or(10_000),
        frames: val("--frames").and_then(|v| v.parse().ok()).unwrap_or(600),
        warmup: val("--warmup").and_then(|v| v.parse().ok()).unwrap_or(120),
        out: val("--out").map_or_else(|| PathBuf::from("bench.json"), PathBuf::from),
        cluster_culling: !flag("--no-cluster-culling"),
        gpu_culling: !flag("--no-gpu-culling"),
        lod: !flag("--no-lod"),
        hide_after: val("--hide-after")
            .and_then(|v| v.parse().ok())
            .unwrap_or(30),
    }
}

#[derive(Resource, Default)]
struct Samples {
    frame: usize,
    ms: Vec<f64>,
    visible_instances: Vec<usize>,
    visible_clusters: Vec<usize>,
    shadow_only_clusters: Vec<usize>,
    cull_us: Vec<u64>,
}

#[derive(Serialize)]
struct Report {
    scene: &'static str,
    instances: usize,
    entities: usize,
    clusters: usize,
    cascades: usize,
    cluster_culling: bool,
    gpu_culling: bool,
    lod: bool,
    frames: usize,
    mean_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
    avg_visible_instances: f64,
    avg_visible_clusters: f64,
    avg_shadow_only_clusters: f64,
    avg_cluster_cull_us: f64,
    adapter: String,
}

fn main() {
    let args = parse_args();
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "gothic-static-render bench".into(),
                resolution: (1280, 720).into(),
                present_mode: PresentMode::AutoNoVsync,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(StaticRenderPlugin {
            cluster_culling: args.cluster_culling,
            hide_after_frames: args.hide_after,
            gpu_culling: args.gpu_culling,
            ..default()
        })
        .insert_resource(args)
        .init_resource::<Samples>()
        .add_systems(Startup, setup)
        .add_systems(Update, (fly, sample))
        .run();
}

/// xorshift for a reproducible layout.
struct R(u64);
impl R {
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

fn merged(parts: Vec<(Mesh, Transform)>) -> Mesh {
    let mut it = parts.into_iter();
    let (m, t) = it.next().expect("parts");
    let mut out = m.transformed_by(t);
    for (m, t) in it {
        out.merge(&m.transformed_by(t)).expect("compatible meshes");
    }
    out
}

fn setup(
    mut commands: Commands,
    args: Res<Args>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut statics: ResMut<StaticMeshes>,
) {
    let mat = |m: &mut Assets<StandardMaterial>, r: f32, g: f32, b: f32| {
        m.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            perceptual_roughness: 0.85,
            ..default()
        })
    };
    let ranges = |ends: Vec<f32>, lod: bool| {
        if lod {
            LodRanges::new(ends, 6.0)
        } else {
            LodRanges::new(vec![*ends.last().unwrap()], 0.0)
        }
    };
    let pick = |lods: Vec<Handle<Mesh>>, lod: bool| if lod { lods } else { vec![lods[0].clone()] };

    // House: body + roof + chimney; LOD1 body + roof box; LOD2 one box.
    let house0 = merged(vec![
        (
            Cuboid::new(8.0, 5.0, 6.0).mesh().build(),
            Transform::from_xyz(0.0, 2.5, 0.0),
        ),
        (
            Cuboid::new(8.6, 0.4, 4.6).mesh().build(),
            Transform::from_xyz(0.0, 6.2, -1.4).with_rotation(Quat::from_rotation_x(0.6)),
        ),
        (
            Cuboid::new(8.6, 0.4, 4.6).mesh().build(),
            Transform::from_xyz(0.0, 6.2, 1.4).with_rotation(Quat::from_rotation_x(-0.6)),
        ),
        (
            Cylinder::new(0.4, 2.5).mesh().resolution(16).build(),
            Transform::from_xyz(2.5, 7.0, 0.0),
        ),
    ]);
    let house1 = merged(vec![
        (
            Cuboid::new(8.0, 5.0, 6.0).mesh().build(),
            Transform::from_xyz(0.0, 2.5, 0.0),
        ),
        (
            Cuboid::new(8.0, 2.4, 4.0).mesh().build(),
            Transform::from_xyz(0.0, 6.2, 0.0),
        ),
    ]);
    let house2 = Cuboid::new(8.0, 7.0, 6.0)
        .mesh()
        .build()
        .transformed_by(Transform::from_xyz(0.0, 3.5, 0.0));
    let house = statics.register(StaticMeshDesc {
        lods: pick(
            vec![meshes.add(house0), meshes.add(house1), meshes.add(house2)],
            args.lod,
        ),
        ranges: ranges(vec![60.0, 160.0, 400.0], args.lod),
        material: mat(&mut materials, 0.62, 0.55, 0.45),
        bounds: Aabb::new(Vec3::new(-4.5, 0.0, -3.5), Vec3::new(4.5, 8.5, 3.5)),
        cast_shadows: true,
    });
    // Tree: trunk + crown; LOD1 low-poly; LOD2 a box.
    let tree0 = merged(vec![
        (
            Cylinder::new(0.3, 3.0).mesh().resolution(12).build(),
            Transform::from_xyz(0.0, 1.5, 0.0),
        ),
        (
            Sphere::new(2.2).mesh().ico(3).unwrap(),
            Transform::from_xyz(0.0, 4.5, 0.0),
        ),
    ]);
    let tree1 = merged(vec![
        (
            Cylinder::new(0.3, 3.0).mesh().resolution(5).build(),
            Transform::from_xyz(0.0, 1.5, 0.0),
        ),
        (
            Sphere::new(2.2).mesh().ico(1).unwrap(),
            Transform::from_xyz(0.0, 4.5, 0.0),
        ),
    ]);
    let tree2 = Cuboid::new(3.5, 6.0, 3.5)
        .mesh()
        .build()
        .transformed_by(Transform::from_xyz(0.0, 3.5, 0.0));
    let tree = statics.register(StaticMeshDesc {
        lods: pick(
            vec![meshes.add(tree0), meshes.add(tree1), meshes.add(tree2)],
            args.lod,
        ),
        ranges: ranges(vec![40.0, 120.0, 300.0], args.lod),
        material: mat(&mut materials, 0.2, 0.42, 0.18),
        bounds: Aabb::new(Vec3::new(-2.3, 0.0, -2.3), Vec3::new(2.3, 6.8, 2.3)),
        cast_shadows: true,
    });
    let barrel0 = Cylinder::new(0.45, 1.1)
        .mesh()
        .resolution(24)
        .build()
        .transformed_by(Transform::from_xyz(0.0, 0.55, 0.0));
    let barrel1 = Cylinder::new(0.45, 1.1)
        .mesh()
        .resolution(6)
        .build()
        .transformed_by(Transform::from_xyz(0.0, 0.55, 0.0));
    let barrel = statics.register(StaticMeshDesc {
        lods: pick(vec![meshes.add(barrel0), meshes.add(barrel1)], args.lod),
        ranges: ranges(vec![25.0, 90.0], args.lod),
        material: mat(&mut materials, 0.45, 0.3, 0.18),
        bounds: Aabb::new(Vec3::new(-0.5, 0.0, -0.5), Vec3::new(0.5, 1.1, 0.5)),
        cast_shadows: true,
    });
    let crate_mesh = meshes.add(
        Cuboid::new(1.0, 1.0, 1.0)
            .mesh()
            .build()
            .transformed_by(Transform::from_xyz(0.0, 0.5, 0.0)),
    );
    let crate_id = statics.register(StaticMeshDesc {
        lods: vec![crate_mesh],
        ranges: LodRanges::new(vec![80.0], 4.0),
        material: mat(&mut materials, 0.55, 0.42, 0.25),
        bounds: Aabb::new(Vec3::new(-0.5, 0.0, -0.5), Vec3::new(0.5, 1.0, 0.5)),
        cast_shadows: false,
    });

    // Layout: a town grid of houses with props around them, trees on the outskirts.
    let mut r = R(0x5eed_1234_abcd_ef01);
    let n = args.instances;
    let houses = n / 10;
    let side = (houses as f32).sqrt().ceil() as i32;
    let spacing = 22.0;
    let half = side as f32 * spacing * 0.5;
    let mut count = 0;
    'outer: for gx in 0..side {
        for gz in 0..side {
            if count >= houses {
                break 'outer;
            }
            let p = Vec3::new(gx as f32 * spacing - half, 0.0, gz as f32 * spacing - half);
            let yaw = (r.f() * 4.0).floor() * std::f32::consts::FRAC_PI_2;
            statics.add(
                house,
                Transform::from_translation(p).with_rotation(Quat::from_rotation_y(yaw)),
            );
            count += 1;
        }
    }
    let rest = n - count;
    for i in 0..rest {
        let (id, around) = match i % 10 {
            0..=3 => (barrel, true),
            4..=6 => (crate_id, true),
            _ => (tree, false),
        };
        let p = if around {
            let gx = (r.f() * side as f32).floor();
            let gz = (r.f() * side as f32).floor();
            Vec3::new(
                gx * spacing - half + 5.0 + r.f() * 4.0,
                0.0,
                gz * spacing - half + 4.0 + r.f() * 6.0,
            )
        } else {
            let a = r.f() * std::f32::consts::TAU;
            let d = half + 10.0 + r.f() * 250.0;
            Vec3::new(a.cos() * d, 0.0, a.sin() * d)
        };
        statics.add(
            id,
            Transform::from_translation(p)
                .with_rotation(Quat::from_rotation_y(r.f() * std::f32::consts::TAU)),
        );
    }

    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(4000.0, 4000.0))),
        MeshMaterial3d(mat(&mut materials, 0.35, 0.4, 0.3)),
    ));
    commands.spawn((
        StaticShadowLight,
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.0).looking_to(Vec3::new(-0.6, -0.5, -0.4), Vec3::Y),
        CascadeShadowConfigBuilder {
            num_cascades: 3,
            minimum_distance: 0.1,
            first_cascade_far_bound: 25.0,
            maximum_distance: 200.0,
            overlap_proportion: 0.2,
        }
        .build(),
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.7, 0.75, 0.9),
        brightness: 400.0,
        ..default()
    });
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 12.0, 0.0),
        StaticCullCamera,
    ));
}

/// Deterministic flight: a slow circle through the town, looking around.
fn fly(samples: Res<Samples>, mut cams: Query<&mut Transform, With<StaticCullCamera>>) {
    let t = samples.frame as f32 / 60.0;
    for mut c in &mut cams {
        let p = Vec3::new(
            (t * 0.15).cos() * 120.0,
            9.0 + (t * 0.3).sin() * 4.0,
            (t * 0.15).sin() * 120.0,
        );
        let look = p + Vec3::new((t * 0.4).cos(), -0.15, (t * 0.4).sin());
        *c = Transform::from_translation(p).looking_at(look, Vec3::Y);
    }
}

fn sample(
    time: Res<Time<Real>>,
    args: Res<Args>,
    stats: Res<StaticRenderStats>,
    adapter: Option<Res<bevy::render::renderer::RenderAdapterInfo>>,
    mut samples: ResMut<Samples>,
    mut exit: MessageWriter<AppExit>,
) {
    samples.frame += 1;
    if samples.frame <= args.warmup {
        return;
    }
    samples.ms.push(f64::from(time.delta_secs()) * 1000.0);
    samples.visible_instances.push(stats.visible_instances);
    samples.visible_clusters.push(stats.visible_clusters);
    samples
        .shadow_only_clusters
        .push(stats.shadow_only_clusters);
    samples.cull_us.push(stats.cull_micros);
    if samples.ms.len() < args.frames {
        return;
    }
    let mut sorted = samples.ms.clone();
    sorted.sort_by(f64::total_cmp);
    let pct = |p: f64| sorted[((sorted.len() - 1) as f64 * p).round() as usize];
    let avg = |v: &[usize]| v.iter().sum::<usize>() as f64 / v.len().max(1) as f64;
    let report = Report {
        scene: "procedural town (houses, trees, barrels, crates)",
        instances: stats.instances,
        entities: stats.entities,
        clusters: stats.clusters,
        cascades: 3,
        cluster_culling: args.cluster_culling,
        gpu_culling: args.gpu_culling,
        lod: args.lod,
        frames: sorted.len(),
        mean_ms: sorted.iter().sum::<f64>() / sorted.len() as f64,
        p50_ms: pct(0.5),
        p95_ms: pct(0.95),
        p99_ms: pct(0.99),
        max_ms: *sorted.last().unwrap_or(&0.0),
        avg_visible_instances: avg(&samples.visible_instances),
        avg_visible_clusters: avg(&samples.visible_clusters),
        avg_shadow_only_clusters: avg(&samples.shadow_only_clusters),
        avg_cluster_cull_us: samples.cull_us.iter().sum::<u64>() as f64
            / samples.cull_us.len().max(1) as f64,
        adapter: adapter.map_or_else(
            || "unknown".into(),
            |a| format!("{} ({:?})", a.name, a.backend),
        ),
    };
    let json = serde_json::to_string_pretty(&report).expect("json");
    println!("{json}");
    if let Err(e) = std::fs::write(&args.out, &json) {
        eprintln!("writing {}: {e}", args.out.display());
    }
    exit.write(AppExit::Success);
}
