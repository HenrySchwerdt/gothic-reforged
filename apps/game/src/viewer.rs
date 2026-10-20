use anyhow::{Result, ensure};
use bevy::{
    asset::RenderAssetUsages,
    input::mouse::AccumulatedMouseMotion,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    window::WindowResolution,
};
use gothic_formats::{world_node::WorldDocument, xcmsh::Xcmsh};
use gothic_world::{Residency, StreamingObserver, StreamingSettings, WorldStreamingPlugin};
use std::{io::Read, path::Path};

#[derive(Resource)]
struct Preview {
    meshes: Vec<Xcmsh>,
    instances: Vec<(usize, Mat4)>,
    normalization: Mat4,
    label: String,
}
#[derive(Resource)]
struct SmokeCapture {
    enabled: bool,
    frame: u32,
}

pub fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let argument = |flag: &str| -> Result<Option<&str>> {
        if let Some(index) = args.iter().position(|a| a == flag) {
            Ok(Some(
                args.get(index + 1)
                    .filter(|s| !s.starts_with("--"))
                    .ok_or_else(|| anyhow::anyhow!("{flag} requires a file path"))?
                    .as_str(),
            ))
        } else {
            Ok(None)
        }
    };
    let mut preview = if let Some(world_path) = argument("--world")? {
        ensure!(argument("--mesh")?.is_none(), "choose --mesh or --world");
        let archive = argument("--mesh-archive")?
            .ok_or_else(|| anyhow::anyhow!("--world requires --mesh-archive"))?;
        let world = WorldDocument::parse(&read_preview(world_path)?)?;
        let scene = gothic_importer::preview_scene::import_preview(&world, Path::new(archive))?;
        eprintln!(
            "Preview: {} unique meshes, {} instances, {} issues. Single explicit archive; materials/LOD unresolved.",
            scene.meshes.len(),
            scene.instances.len(),
            scene.issues.len()
        );
        for issue in scene.issues.iter().take(20) {
            eprintln!("  {issue}");
        }
        ensure!(
            !scene.instances.is_empty(),
            "no supported original meshes in this scene; inspect scene-info diagnostics"
        );
        let label = format!(
            "Original placements / {} instances / {} issues / materials unresolved",
            scene.instances.len(),
            scene.issues.len()
        );
        Preview {
            meshes: scene.meshes.into_iter().map(|m| m.mesh).collect(),
            instances: scene
                .instances
                .into_iter()
                .map(|i| (i.mesh_index, Mat4::from_cols_array_2d(&i.world_matrix)))
                .collect(),
            normalization: Mat4::IDENTITY,
            label,
        }
    } else if let Some(path) = argument("--mesh")? {
        Preview {
            meshes: vec![Xcmsh::parse(&read_preview(path)?)?],
            instances: vec![(0, Mat4::IDENTITY)],
            normalization: Mat4::IDENTITY,
            label: "Original mesh / normalized debug preview / materials unresolved".into(),
        }
    } else {
        Preview {
            meshes: Vec::new(),
            instances: Vec::new(),
            normalization: Mat4::IDENTITY,
            label: "Original world import pending".into(),
        }
    };
    preview.normalization = normalization(&preview)?;
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Gothic Reforged / development viewer".into(),
                resolution: WindowResolution::new(1280, 720),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(WorldStreamingPlugin)
        .insert_resource(preview)
        .insert_resource(SmokeCapture {
            enabled: args.iter().any(|a| a == "--smoke"),
            frame: 0,
        })
        .insert_resource(ClearColor(Color::srgb(0.025, 0.035, 0.045)))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (fly_camera, draw_cells, update_title, smoke_capture),
        )
        .run();
    Ok(())
}

fn read_preview(path: &str) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    ensure!(
        file.metadata()?.len() <= 64 * 1024 * 1024,
        "preview file exceeds 64 MiB"
    );
    let mut bytes = Vec::new();
    file.take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 64 * 1024 * 1024,
        "preview file grew beyond ceiling"
    );
    Ok(bytes)
}

fn normalization(preview: &Preview) -> Result<Mat4> {
    if preview.instances.is_empty() {
        return Ok(Mat4::IDENTITY);
    }
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut vertices = 0usize;
    for (index, matrix) in &preview.instances {
        ensure!(
            matrix.is_finite() && matrix.determinant().abs() > 1e-12,
            "singular or non-finite instance transform"
        );
        for part in &preview.meshes[*index].submeshes {
            vertices = vertices
                .checked_add(part.positions.len())
                .ok_or_else(|| anyhow::anyhow!("vertex budget overflow"))?;
            ensure!(
                vertices <= 2_000_000,
                "preview exceeds two million instanced vertices"
            );
            for x in [part.bounds_min[0], part.bounds_max[0]] {
                for y in [part.bounds_min[1], part.bounds_max[1]] {
                    for z in [part.bounds_min[2], part.bounds_max[2]] {
                        let p = matrix.transform_point3(Vec3::new(x, y, z));
                        ensure!(p.is_finite(), "transformed bounds overflow");
                        min = min.min(p);
                        max = max.max(p);
                    }
                }
            }
        }
    }
    let span = (max - min).max_element();
    let center = min + (max - min) * 0.5;
    ensure!(
        span.is_finite() && center.is_finite(),
        "preview bounds overflow"
    );
    Ok(Mat4::from_scale(Vec3::splat(200.0 / span.max(0.001))) * Mat4::from_translation(-center))
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    preview: Res<Preview>,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(80.0, 120.0, 160.0).looking_at(Vec3::ZERO, Vec3::Y),
        StreamingObserver,
    ));
    let mut handles = Vec::new();
    for asset in &preview.meshes {
        let mut parts = Vec::new();
        for source in &asset.submeshes {
            let mut indices = source.indices.clone();
            // Verified source winding opposes stored normals with unchanged XYZ.
            for triangle in indices.chunks_exact_mut(3) {
                triangle.swap(1, 2);
            }
            let mut mesh = Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::RENDER_WORLD,
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, source.positions.clone())
            .with_inserted_indices(Indices::U32(indices));
            if source.normals.len() == source.positions.len() {
                mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, source.normals.clone());
            } else {
                mesh.compute_smooth_normals();
            }
            parts.push(meshes.add(mesh));
        }
        handles.push(parts);
    }
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.61, 0.48),
        perceptual_roughness: 0.9,
        cull_mode: None,
        ..default()
    });
    for (index, matrix) in &preview.instances {
        for handle in &handles[*index] {
            commands.spawn((
                Mesh3d(handle.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_matrix(preview.normalization * *matrix),
            ));
        }
    }
    if !preview.instances.is_empty() {
        commands.spawn((
            DirectionalLight {
                illuminance: 18000.0,
                ..default()
            },
            Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, -0.7, 0.0)),
        ));
    }
}

fn smoke_capture(mut commands: Commands, mut capture: ResMut<SmokeCapture>) {
    if !capture.enabled {
        return;
    }
    capture.frame += 1;
    if capture.frame == 90 {
        use bevy::render::view::screenshot::{Screenshot, save_to_disk};
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(".local/viewer-smoke.png"));
    }
    if capture.frame == 240 {
        commands.write_message(bevy::app::AppExit::Success);
    }
}

fn fly_camera(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    time: Res<Time>,
    mut query: Query<&mut Transform, With<StreamingObserver>>,
) {
    let Ok(mut t) = query.single_mut() else {
        return;
    };
    if buttons.pressed(MouseButton::Right) {
        let (yaw, pitch, _) = t.rotation.to_euler(EulerRot::YXZ);
        t.rotation = Quat::from_euler(
            EulerRot::YXZ,
            yaw - mouse.delta.x * 0.003,
            (pitch - mouse.delta.y * 0.003).clamp(-1.5, 1.5),
            0.0,
        );
    }
    let mut direction = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        direction += *t.forward();
    }
    if keys.pressed(KeyCode::KeyS) {
        direction -= *t.forward();
    }
    if keys.pressed(KeyCode::KeyA) {
        direction -= *t.right();
    }
    if keys.pressed(KeyCode::KeyD) {
        direction += *t.right();
    }
    if keys.pressed(KeyCode::Space) {
        direction.y += 1.0;
    }
    if keys.pressed(KeyCode::ControlLeft) {
        direction.y -= 1.0;
    }
    let speed = if keys.pressed(KeyCode::ShiftLeft) {
        240.0
    } else {
        80.0
    };
    t.translation += direction.normalize_or_zero() * speed * time.delta_secs().min(0.1);
}
fn draw_cells(mut gizmos: Gizmos, settings: Res<StreamingSettings>, resident: Res<Residency>) {
    let s = settings.cell_size;
    for cell in &resident.cells {
        let x = cell.x as f32 * s;
        let z = cell.z as f32 * s;
        gizmos.linestrip(
            [
                Vec3::new(x, 0.0, z),
                Vec3::new(x + s, 0.0, z),
                Vec3::new(x + s, 0.0, z + s),
                Vec3::new(x, 0.0, z + s),
                Vec3::new(x, 0.0, z),
            ],
            Color::srgb(0.3, 0.65, 0.5),
        );
    }
    gizmos.axes(Transform::default(), 20.0);
}
fn update_title(resident: Res<Residency>, preview: Res<Preview>, mut windows: Query<&mut Window>) {
    if !resident.is_changed() {
        return;
    }
    for mut window in &mut windows {
        window.title = format!(
            "Gothic Reforged | {} | {} synthetic cells | WASD / RMB / Space / Ctrl / Shift",
            preview.label,
            resident.cells.len()
        );
    }
}
