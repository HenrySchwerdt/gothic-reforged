//! Rain and snow (`eCPrecipitation_PS`): feeds the weather state of the camera position into the
//! precipitation emitter of `gothic_fx::precip` and draws its camera facing quads.
//!
//! The emitter parameters come from `OrigEnv::precip` (world entity + the current weather states,
//! blended by weather zone); the ground below the spawn points is found with the collision
//! world. Disable with `GR_NOPRECIP=1`.
use super::{CM, Collision, PlayerCamera, env::OrigEnv};
use bevy::{camera::visibility::NoFrustumCulling, light::NotShadowCaster, prelude::*};
use gothic_fx::{
    draw::View,
    plugin::{FxMaterial, FxParams, empty_mesh, fill_mesh},
    precip::{Ground, Precipitation},
};
use gothic_physics::CollisionWorld;

/// `RainAndSnow` (2 x 2 tiles), delivered by the loader.
#[derive(Resource, Default)]
pub struct PrecipTexture(pub Option<Handle<Image>>);

pub struct PrecipPlugin;

impl Plugin for PrecipPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PrecipTexture>()
            .add_systems(Update, precip_update);
    }
}

/// Ground below the spawn points from the collision triangles (terrain, buildings, trunks).
struct CollisionGround<'a>(&'a CollisionWorld);

impl Ground for CollisionGround<'_> {
    fn ground_y(&mut self, x: f32, z: f32, from_y: f32, depth: f32) -> Option<f32> {
        self.0.ground_below(Vec3::new(x, from_y, z), depth)
    }
}

#[derive(Default)]
struct State {
    sim: Option<Precipitation>,
    mesh: Option<Handle<Mesh>>,
    drawn: usize,
}

fn precip_update(
    time: Res<Time>,
    env: Res<OrigEnv>,
    tex: Res<PrecipTexture>,
    collision: Res<Collision>,
    camera: Query<(&Transform, &Projection), With<PlayerCamera>>,
    mut state: Local<State>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FxMaterial>>,
    mut commands: Commands,
) {
    if std::env::var_os("GR_NOPRECIP").is_some() {
        return;
    }
    let (Ok((cam, projection)), Some(texture)) = (camera.single(), tex.0.clone()) else {
        return;
    };
    let params = env.precip.clone();
    if state.sim.is_none() {
        if params.particles_per_second <= 0.0 {
            return;
        }
        let mesh = meshes.add(empty_mesh());
        let material = materials.add(FxMaterial {
            params: FxParams {
                misc: Vec4::new(0.0, 1.0, 0.0, 0.0),
            },
            texture: Some(texture),
            alpha_mode: AlphaMode::Blend,
            depth_write: false,
        });
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            NoFrustumCulling,
            NotShadowCaster,
        ));
        state.mesh = Some(mesh);
        state.sim = Some(Precipitation::new(params.clone(), 0x5eed));
    }
    let State { sim, mesh, drawn } = &mut *state;
    let (Some(sim), Some(mesh)) = (sim.as_mut(), mesh.as_ref()) else {
        return;
    };
    sim.params = params;
    let dt = time.delta_secs().min(0.1);
    let cam_cm = cam.translation / CM;
    sim.step(dt, cam_cm, &mut CollisionGround(&collision.0));
    if sim.alive() == 0 && *drawn == 0 {
        return;
    }
    let mut view = View::new(cam_cm, cam.rotation);
    if let Projection::Perspective(p) = projection {
        let ty = (p.fov * 0.5).tan();
        view.tan_half = Some((ty * p.aspect_ratio, ty));
    }
    let quads = sim.quads(&view);
    *drawn = quads.len();
    if let Some(mut m) = meshes.get_mut(mesh) {
        fill_mesh(&mut m, &quads, Vec3::ZERO);
    }
}
