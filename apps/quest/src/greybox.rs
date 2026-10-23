//! Greybox test scene: ground grid, pillars/cubes in rings, controller visualisation.
use bevy::prelude::*;
use gothic_xr_quest::{XrFrameData, XrRig};

#[derive(Component)]
struct HandViz(usize);
#[derive(Component)]
struct Marker;

fn unlit() -> bool {
    gothic_xr_quest::knob("unlit").as_deref() == Some("1")
}

/// Sun + controller visualisation only (used with the Ardea world).
pub fn build_lighting_only(app: &mut App) {
    app.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.75, 0.8, 1.0),
        brightness: gothic_xr_quest::knob_f32("ambient", 600.0),
        ..default()
    })
    .add_systems(Startup, setup_light_and_hands)
    .add_systems(Update, update_hands);
}

fn setup_light_and_hands(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    rig: Query<Entity, With<XrRig>>,
) {
    commands.spawn((
        DirectionalLight {
            illuminance: gothic_xr_quest::knob_f32("sun", 12000.0),
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.5, 0.0)),
    ));
    spawn_hands(&mut commands, &mut meshes, &mut mats, rig.single().ok());
}

pub fn build(app: &mut App) {
    crate::locomotion::init(app, Vec3::new(0.0, 100.0, 0.0));
    app.add_systems(Startup, ground_collision);
    build_inner(app);
}

fn ground_collision(
    mut phys: ResMut<crate::locomotion::Physics>,
    mut world: ResMut<gothic_vr::VrWorld>,
) {
    let s = 40_000.0;
    let q = [
        Vec3::new(-s, 0.0, -s),
        Vec3::new(s, 0.0, -s),
        Vec3::new(s, 0.0, s),
        Vec3::new(-s, 0.0, s),
    ];
    world
        .0
        .insert_chunk(1, vec![[q[0], q[2], q[1]], [q[0], q[3], q[2]]]);
    phys.ready = true;
}

fn build_inner(app: &mut App) {
    app.insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: gothic_xr_quest::knob_f32("ambient", 400.0),
        ..default()
    })
    .add_systems(Startup, setup)
    .add_systems(Update, update_hands);
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    rig: Query<Entity, With<XrRig>>,
) {
    commands.spawn((
        DirectionalLight {
            illuminance: gothic_xr_quest::knob_f32("sun", 9000.0),
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.5, 0.0)),
    ));
    let ground = meshes.add(Plane3d::default().mesh().size(400.0, 400.0));
    commands.spawn((
        Mesh3d(ground),
        MeshMaterial3d(mats.add(StandardMaterial {
            base_color: Color::srgb(0.25, 0.3, 0.25),
            perceptual_roughness: 1.0,
            unlit: unlit(),
            ..default()
        })),
    ));
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let n_rings = std::env::var("QUEST_GREYBOX_RINGS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8u32);
    for ring in 1..=n_rings {
        let r = ring as f32 * 4.0;
        let count = (ring * 10) as usize;
        let mat = mats.add(StandardMaterial {
            base_color: Color::hsl(ring as f32 * 40.0 % 360.0, 0.5, 0.55),
            perceptual_roughness: 0.8,
            unlit: unlit(),
            ..default()
        });
        for i in 0..count {
            let a = i as f32 / count as f32 * std::f32::consts::TAU;
            let h = 1.0 + ((i * 7 + ring as usize * 3) % 5) as f32;
            commands.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_xyz(a.cos() * r, h * 0.5, a.sin() * r)
                    .with_scale(Vec3::new(1.0, h, 1.0)),
                Marker,
            ));
        }
    }
    spawn_hands(&mut commands, &mut meshes, &mut mats, rig.single().ok());
}

fn spawn_hands(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    rig: Option<Entity>,
) {
    let hand_mesh = meshes.add(Cuboid::new(0.05, 0.04, 0.12));
    let ray_mesh = meshes.add(Cuboid::new(0.004, 0.004, 1.0));
    let hand_mat = [
        mats.add(StandardMaterial {
            base_color: Color::srgb(0.9, 0.2, 0.2),
            unlit: true,
            ..default()
        }),
        mats.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.4, 0.95),
            unlit: true,
            ..default()
        }),
    ];
    for i in 0..2 {
        let hand = commands
            .spawn((HandViz(i), Transform::IDENTITY, Visibility::Hidden))
            .with_children(|p| {
                p.spawn((
                    Mesh3d(hand_mesh.clone()),
                    MeshMaterial3d(hand_mat[i].clone()),
                ));
                p.spawn((
                    Mesh3d(ray_mesh.clone()),
                    MeshMaterial3d(hand_mat[i].clone()),
                    Transform::from_xyz(0.0, 0.0, -0.5),
                ));
            })
            .id();
        if let Some(rig) = rig {
            commands.entity(rig).add_child(hand);
        }
    }
}

fn update_hands(
    frame: Res<XrFrameData>,
    mut hands: Query<(&HandViz, &mut Transform, &mut Visibility)>,
) {
    for (h, mut t, mut v) in &mut hands {
        let s = &frame.hands[h.0];
        *v = if s.active {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if s.active {
            *t = s.aim;
            // Trigger squeezes the pointer scale a bit for visible feedback.
            t.scale = Vec3::splat(1.0 + 0.6 * s.trigger);
        }
    }
}
