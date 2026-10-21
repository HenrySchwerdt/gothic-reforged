//! Desktop presentation of the rig (needs `DefaultPlugins`): eye cameras (mono or side by
//! side), greybox meshes, a debug avatar drawn with gizmos, the teleport arc and the comfort
//! vignette.
use crate::body::BONES;
use crate::greybox::GreyboxBox;
use crate::input::Hand;
use crate::plugin::{VrBody, VrDisplay, VrPlayer, VrSet, XrEyeCamera, XrHand};
use crate::rig::{Eye, StereoMode, eye_viewport};
use bevy::camera::Viewport;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// Cameras, gizmo avatar, teleport arc and vignette.
pub struct VrRenderPlugin;

impl Plugin for VrRenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_rig_visuals).add_systems(
            Update,
            (viewports, draw_avatar, vignette, capture_and_exit).after(VrSet::Sync),
        );
    }
}

/// Vignette ring in front of an eye camera.
#[derive(Component)]
struct Vignette;

fn spawn_rig_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let ring = meshes.add(Annulus::new(0.06, 0.6));
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgba(0.0, 0.0, 0.0, 0.0),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    for (order, eye) in Eye::BOTH.into_iter().enumerate() {
        commands
            .spawn((
                Camera3d::default(),
                Camera {
                    order: order as isize,
                    is_active: eye == Eye::Left,
                    ..default()
                },
                Projection::Perspective(PerspectiveProjection {
                    near: 0.03,
                    fov: 90f32.to_radians(),
                    ..default()
                }),
                XrEyeCamera(eye),
                Transform::from_xyz(0.0, 1.7, 0.0),
            ))
            .with_child((
                Vignette,
                Mesh3d(ring.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_xyz(0.0, 0.0, -0.05),
            ));
    }
}

fn viewports(
    display: Res<VrDisplay>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cams: Query<(&mut Camera, &XrEyeCamera)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let size = window.physical_size();
    for (mut cam, eye) in &mut cams {
        let active = display.stereo == StereoMode::SideBySide || eye.0 == Eye::Left;
        if cam.is_active != active {
            cam.is_active = active;
        }
        let (pos, sz) = eye_viewport(display.stereo, eye.0, size);
        let want = Some(Viewport {
            physical_position: pos,
            physical_size: sz,
            ..default()
        });
        if cam
            .viewport
            .as_ref()
            .map(|v| (v.physical_position, v.physical_size))
            != want
                .as_ref()
                .map(|v| (v.physical_position, v.physical_size))
        {
            cam.viewport = want;
        }
    }
}

fn draw_avatar(
    mut gizmos: Gizmos,
    body: Res<VrBody>,
    player: Option<Res<VrPlayer>>,
    hands: Query<(&Transform, &XrHand)>,
) {
    if let Some(pose) = &body.pose {
        for (a, b) in BONES {
            gizmos.line(pose.get(a), pose.get(b), Color::srgb(0.9, 0.8, 0.3));
        }
    }
    for (t, h) in &hands {
        let color = match (h.hand, h.tracked) {
            (_, false) => Color::srgb(0.4, 0.4, 0.4),
            (Hand::Left, true) => Color::srgb(0.3, 0.6, 1.0),
            (Hand::Right, true) => Color::srgb(1.0, 0.4, 0.3),
        };
        gizmos.sphere(Isometry3d::from_translation(t.translation), 0.04, color);
        gizmos.ray(t.translation, t.forward() * 0.12, color);
    }
    if let Some(aim) = player.as_ref().and_then(|p| p.last.teleport_aim.as_ref()) {
        let color = if aim.target.is_some() {
            Color::srgb(0.2, 1.0, 0.4)
        } else {
            Color::srgb(1.0, 0.2, 0.2)
        };
        gizmos.linestrip(aim.arc.iter().copied(), color);
        if let Some(t) = aim.target {
            gizmos.circle(
                Isometry3d::new(
                    t + Vec3::Y * 0.02,
                    Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
                ),
                0.3,
                color,
            );
        }
    }
}

fn vignette(
    player: Option<Res<VrPlayer>>,
    rings: Query<&MeshMaterial3d<StandardMaterial>, With<Vignette>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let v = player.map_or(0.0, |p| p.last.vignette);
    if let Some(handle) = rings.iter().next()
        && let Some(mut m) = materials.get_mut(&handle.0)
    {
        m.base_color = Color::srgba(0.0, 0.0, 0.0, v);
    }
}

/// Spawn render meshes for greybox boxes.
pub fn spawn_greybox(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    boxes: &[GreyboxBox],
) {
    for b in boxes {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::from_size(b.half_extents * 2.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(b.color[0], b.color[1], b.color[2]),
                perceptual_roughness: 0.9,
                ..default()
            })),
            Transform::from_translation(b.center).with_rotation(b.rotation),
            Name::new(b.name),
        ));
    }
}

/// A sun and ambient light for greybox scenes.
pub fn spawn_greybox_lighting(commands: &mut Commands) {
    commands.spawn((
        DirectionalLight {
            illuminance: 9000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 10.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// Take a screenshot of the primary window after `at_frame` frames, then exit (smoke tests).
#[derive(Resource, Debug, Clone)]
pub struct CaptureAndExit {
    pub path: String,
    pub at_frame: u32,
    frame: u32,
}

impl CaptureAndExit {
    pub fn new(path: impl Into<String>, at_frame: u32) -> Self {
        Self {
            path: path.into(),
            at_frame,
            frame: 0,
        }
    }
}

/// Runs [`CaptureAndExit`] when the resource is present.
pub fn capture_and_exit(
    mut commands: Commands,
    capture: Option<ResMut<CaptureAndExit>>,
    mut exit: MessageWriter<AppExit>,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    let Some(mut c) = capture else {
        return;
    };
    c.frame += 1;
    if c.frame == c.at_frame {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(c.path.clone()));
    }
    if c.frame == c.at_frame + 20 {
        exit.write(AppExit::Success);
    }
}
