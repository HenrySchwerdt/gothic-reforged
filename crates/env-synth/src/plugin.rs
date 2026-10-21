//! Bevy plugin: ticks the [`Environment`] and drives the scene from its [`EnvFrame`]:
//! - [`Sun`] / [`Moon`] directional lights (direction, colour, illuminance),
//! - `GlobalAmbientLight` and `ClearColor`,
//! - `DistanceFog` on every [`EnvironmentCamera`],
//! - a sky dome (gradient, sun, moon, stars, clouds) following the first [`EnvironmentCamera`],
//! - a full-screen rain / snow / sand overlay without particles.
//!
//! Space: Bevy metres, Y up, -Z north (sky directions from [`crate::SkyPath`]).
use crate::env::{EnvFrame, Environment};
use crate::weather::WeatherKind;
use crate::{DayTable, Rgb, WeatherTable};
use bevy::{
    asset::{embedded_asset, load_embedded_asset},
    camera::Exposure,
    light::{NotShadowCaster, NotShadowReceiver},
    mesh::MeshVertexBufferLayoutRef,
    pbr::{DistanceFog, FogFalloff, MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    },
    shader::{Shader, ShaderRef},
};
use std::sync::OnceLock;

static SKY_FS: OnceLock<Handle<Shader>> = OnceLock::new();
static PRECIP_FS: OnceLock<Handle<Shader>> = OnceLock::new();

/// Radius of the sky dome (keep it inside the camera's far plane).
pub const SKY_RADIUS: f32 = 900.0;

pub struct EnvironmentPlugin {
    pub day: DayTable,
    pub weather: WeatherTable,
    pub climate: String,
    pub seed: u64,
    pub start_hour: f64,
    /// Game hours per real second.
    pub hours_per_second: f64,
    pub sky_dome: bool,
    pub overlay: bool,
    /// Set `Exposure` on every [`EnvironmentCamera`] from [`EnvFrame::exposure_ev100`].
    pub auto_exposure: bool,
}

impl Default for EnvironmentPlugin {
    fn default() -> Self {
        Self {
            day: DayTable::default(),
            weather: WeatherTable::default(),
            climate: "temperate".into(),
            seed: 0,
            start_hour: 8.0,
            hours_per_second: 1.0 / 120.0,
            sky_dome: true,
            overlay: true,
            auto_exposure: true,
        }
    }
}

/// The environment state (mutate it to set the time, force weather, pause, ...).
#[derive(Resource)]
pub struct EnvironmentState {
    pub env: Environment,
    pub paused: bool,
    /// Multiplier on real time (debug fast-forward).
    pub time_scale: f32,
}

/// The parameters of the current frame (read it for materials: `wetness`, `snow_cover`, ...).
#[derive(Resource, Clone, Copy, Debug)]
pub struct CurrentEnv(pub EnvFrame);

/// Directional light that follows the sun.
#[derive(Component)]
pub struct Sun;

/// Directional light that follows the moon.
#[derive(Component)]
pub struct Moon;

/// A camera that gets distance fog; the first one also carries the sky dome.
#[derive(Component)]
pub struct EnvironmentCamera;

#[derive(Component)]
struct SkyDome;

#[derive(Component)]
struct PrecipOverlay;

#[derive(Resource)]
struct Handles {
    sky: Option<Handle<SkyMaterial>>,
    precip: Option<Handle<PrecipMaterial>>,
}

impl Plugin for EnvironmentPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/sky.wgsl");
        embedded_asset!(app, "shaders/precipitation.wgsl");
        let _ = SKY_FS.set(load_embedded_asset!(app, "shaders/sky.wgsl"));
        let _ = PRECIP_FS.set(load_embedded_asset!(app, "shaders/precipitation.wgsl"));
        let mut env = Environment::new(
            self.day.clone(),
            self.weather.clone(),
            &self.climate,
            self.seed,
        );
        env.clock.hours = self.start_hour;
        env.clock.hours_per_second = self.hours_per_second;
        let frame = env.frame();
        app.add_plugins((
            MaterialPlugin::<SkyMaterial>::default(),
            UiMaterialPlugin::<PrecipMaterial>::default(),
        ))
        .insert_resource(EnvironmentState {
            env,
            paused: false,
            time_scale: 1.0,
        })
        .insert_resource(CurrentEnv(frame))
        .insert_resource(SpawnFlags {
            sky: self.sky_dome,
            overlay: self.overlay,
            exposure: self.auto_exposure,
        })
        .add_systems(Startup, spawn_visuals)
        .add_systems(
            Update,
            (
                tick,
                (
                    apply_lights,
                    apply_ambient_and_fog,
                    apply_sky,
                    apply_overlay,
                ),
            )
                .chain(),
        );
    }
}

#[derive(Resource)]
struct SpawnFlags {
    sky: bool,
    overlay: bool,
    exposure: bool,
}

fn spawn_visuals(
    mut commands: Commands,
    flags: Res<SpawnFlags>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut sky_materials: ResMut<Assets<SkyMaterial>>,
    mut precip_materials: ResMut<Assets<PrecipMaterial>>,
) {
    let mut handles = Handles {
        sky: None,
        precip: None,
    };
    if flags.sky {
        let h = sky_materials.add(SkyMaterial::default());
        commands.spawn((
            Name::new("Sky dome"),
            SkyDome,
            Mesh3d(meshes.add(Sphere::new(SKY_RADIUS).mesh().uv(48, 24))),
            MeshMaterial3d(h.clone()),
            Transform::default(),
            NotShadowCaster,
            NotShadowReceiver,
        ));
        handles.sky = Some(h);
    }
    if flags.overlay {
        let h = precip_materials.add(PrecipMaterial::default());
        commands.spawn((
            Name::new("Precipitation overlay"),
            PrecipOverlay,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            MaterialNode(h.clone()),
            Visibility::Hidden,
            GlobalZIndex(-1),
        ));
        handles.precip = Some(h);
    }
    commands.insert_resource(handles);
}

fn tick(time: Res<Time>, mut state: ResMut<EnvironmentState>, mut current: ResMut<CurrentEnv>) {
    if !state.paused {
        let dt = time.delta_secs() * state.time_scale;
        state.env.update(dt);
    }
    current.0 = state.env.frame();
}

fn color(c: Rgb) -> Color {
    Color::linear_rgb(c.0, c.1, c.2)
}

fn v4(c: Rgb, w: f32) -> Vec4 {
    Vec4::new(c.0, c.1, c.2, w)
}

type LightQuery<'w, 's, A, B> =
    Query<'w, 's, (&'static mut Transform, &'static mut DirectionalLight), (With<A>, Without<B>)>;

fn apply_lights(
    current: Res<CurrentEnv>,
    mut suns: LightQuery<Sun, Moon>,
    mut moons: LightQuery<Moon, Sun>,
) {
    let f = &current.0;
    for (mut t, mut l) in &mut suns {
        let d = Vec3::from_array(f.sun_dir);
        *t = Transform::default().looking_to(-d, Vec3::Y);
        l.color = color(f.sun_color);
        l.illuminance = f.sun_illuminance;
    }
    for (mut t, mut l) in &mut moons {
        let d = Vec3::from_array(f.moon_dir);
        *t = Transform::default().looking_to(-d, Vec3::Y);
        l.color = color(f.moon_color);
        l.illuminance = f.moon_illuminance;
    }
}

fn apply_ambient_and_fog(
    current: Res<CurrentEnv>,
    flags: Res<SpawnFlags>,
    mut commands: Commands,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut cameras: Query<(Entity, Option<&mut DistanceFog>), With<EnvironmentCamera>>,
) {
    let f = &current.0;
    ambient.color = color(f.ambient_color);
    ambient.brightness = f.ambient_brightness;
    commands.insert_resource(ClearColor(color(f.sky_horizon)));
    let fog = DistanceFog {
        color: color(f.fog.color),
        directional_light_color: color(f.sun_glow.scale(0.3)),
        directional_light_exponent: 20.0,
        falloff: FogFalloff::Linear {
            start: f.fog.start,
            end: f.fog.end,
        },
    };
    for (e, existing) in &mut cameras {
        match existing {
            Some(mut d) => *d = fog.clone(),
            None => {
                commands.entity(e).insert(fog.clone());
            }
        }
        if flags.exposure {
            commands.entity(e).insert(Exposure {
                ev100: f.exposure_ev100,
            });
        }
    }
}

fn apply_sky(
    time: Res<Time>,
    current: Res<CurrentEnv>,
    handles: Option<Res<Handles>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    cameras: Query<&GlobalTransform, With<EnvironmentCamera>>,
    mut domes: Query<&mut Transform, With<SkyDome>>,
    mut drift: Local<Vec2>,
) {
    let Some(h) = handles.as_ref().and_then(|h| h.sky.clone()) else {
        return;
    };
    let f = &current.0;
    let center = cameras
        .iter()
        .next()
        .map_or(Vec3::ZERO, GlobalTransform::translation);
    for mut t in &mut domes {
        t.translation = center;
    }
    *drift += Vec2::from_array(f.clouds.velocity) * time.delta_secs();
    if let Some(mut m) = materials.get_mut(&h) {
        m.params = SkyParams {
            zenith: v4(f.sky_zenith, f.stars),
            horizon: v4(f.sky_horizon, f.clouds.cover),
            sun_dir: Vec3::from_array(f.sun_dir).extend(1.0 - f.clouds.cover * 0.9),
            sun_glow: v4(f.sun_glow, f.lightning),
            moon_dir: Vec3::from_array(f.moon_dir)
                .extend((1.0 - f.clouds.cover) * (f.moon_illuminance / 120.0).min(1.0)),
            cloud_color: v4(f.clouds.color, f.clouds.darkness),
            center: center.extend(time.elapsed_secs_wrapped()),
            cloud_offset: drift.extend(0.0).extend(0.0),
        };
    }
}

fn apply_overlay(
    time: Res<Time>,
    current: Res<CurrentEnv>,
    handles: Option<Res<Handles>>,
    windows: Query<&Window>,
    mut materials: ResMut<Assets<PrecipMaterial>>,
    mut overlays: Query<&mut Visibility, With<PrecipOverlay>>,
) {
    let Some(h) = handles.as_ref().and_then(|h| h.precip.clone()) else {
        return;
    };
    let f = &current.0;
    let p = f.precipitation;
    let visible = p.rain + p.snow + p.sand + f.lightning > 0.005;
    for mut v in &mut overlays {
        *v = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if !visible {
        return;
    }
    let aspect = windows
        .iter()
        .next()
        .map_or(16.0 / 9.0, |w| w.width() / w.height().max(1.0));
    if let Some(mut m) = materials.get_mut(&h) {
        m.params = PrecipParams {
            amount: Vec4::new(p.rain, p.snow, p.sand, time.elapsed_secs_wrapped()),
            wind: Vec4::new(
                (p.wind[0] / 20.0).clamp(-1.0, 1.0),
                f.lightning,
                aspect,
                0.0,
            ),
            sand_color: v4(Rgb(0.78, 0.6, 0.38), 1.0),
        };
    }
}

#[derive(ShaderType, Clone, Copy, Debug, Default)]
pub struct SkyParams {
    pub zenith: Vec4,
    pub horizon: Vec4,
    pub sun_dir: Vec4,
    pub sun_glow: Vec4,
    pub moon_dir: Vec4,
    pub cloud_color: Vec4,
    pub center: Vec4,
    pub cloud_offset: Vec4,
}

/// Unlit sky dome material (rendered from inside, no culling, no prepass, no shadows).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug, Default)]
pub struct SkyMaterial {
    #[uniform(0)]
    pub params: SkyParams,
}

impl Material for SkyMaterial {
    fn fragment_shader() -> ShaderRef {
        SKY_FS
            .get()
            .map_or(ShaderRef::Default, |h| ShaderRef::Handle(h.clone()))
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

#[derive(ShaderType, Clone, Copy, Debug, Default)]
pub struct PrecipParams {
    pub amount: Vec4,
    pub wind: Vec4,
    pub sand_color: Vec4,
}

/// Full-screen precipitation overlay (UI material).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug, Default)]
pub struct PrecipMaterial {
    #[uniform(0)]
    pub params: PrecipParams,
}

impl UiMaterial for PrecipMaterial {
    fn fragment_shader() -> ShaderRef {
        PRECIP_FS
            .get()
            .map_or(ShaderRef::Default, |h| ShaderRef::Handle(h.clone()))
    }
}

/// Convenience for debug keys: cycle to the next weather kind.
pub fn next_weather(k: WeatherKind) -> WeatherKind {
    let all = WeatherKind::ALL;
    let i = all.iter().position(|x| *x == k).unwrap_or(0);
    all[(i + 1) % all.len()]
}
