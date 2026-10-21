//! Day-time environment driven by the original weather data (`WeatherStates.g3ws` keyframes,
//! weather zones, sun path): directional light, hemisphere SH ambient, fog, sky colours.
//!
//! The numbers and rules live in `gothic-environment`; this module only moves the resulting
//! packet into Bevy resources and the material uniforms. Disable with `GR_ORIG_ENV=0`.
//! `GR_ENV_SYNTH=auto|1` feeds synthetic placeholder states instead (see `env_synth.rs`).
//!
//! Options: `--hour H` (0..24) or `GR_TIME=<day fraction>`, `--day-speed N` (game days per real
//! second), `--weather STATE` (force the Myrtana environment into a named state, e.g. `Rainy`).
use super::{
    FogEnd, PlayerCamera, SkyHandle, SunDir,
    gmat::{
        CHAR_LIGHTS, CharMaterial, GothicMaterial, SkyMaterial, WindEnv, push_char_lights,
        push_light_env, set_light_env, set_wind_env,
    },
};
use crate::world_loader::{Request, WeatherPayload, ZoneInstance};
use bevy::{
    pbr::{DistanceFog, FogFalloff},
    prelude::*,
};
use gothic_environment::{
    DEFAULT_DAY_TIME, EnvPacket, WeatherAdmin, packet, precip::PrecipParams, sky::SkydomeParams,
    wind::SpeedTreeWind,
};
use gothic_render_rules::lights::{self, StaticLight};
use std::collections::HashMap;

/// Edge of a static light grid cell (metres).
const LIGHT_CELL: f32 = 40.0;
/// Seconds between re-selections of the static lights around the camera.
const LIGHT_INTERVAL: f32 = 0.2;

/// Azimuth (radians) of the sun's highest point; the level's authoring choice, calibrated against
/// the reference screenshot (the previous fixed sun stood at 0.9).
const SUN_AZIMUTH: f32 = 0.9;
/// Far plane used to clip the original fog: the low-poly world pass (`fFarClippingPlaneLowPolyMesh_High`).
const FAR_CM: f32 = 100_000.0;
/// Minimum seconds between pushes of the light set into the material assets.
const PUSH_INTERVAL: f32 = 0.5;

#[derive(Resource)]
pub struct OrigEnv {
    enabled: bool,
    requested: bool,
    admin: Option<WeatherAdmin>,
    zones: Vec<ZoneInstance>,
    pub day_time: f32,
    /// Game days per real second.
    speed: f32,
    pub packet: Option<EnvPacket>,
    origin: Vec3,
    force: Option<String>,
    pushed: Option<(Vec4, [Vec4; 4])>,
    since_push: f32,
    /// Static point lights (Bevy space, metres), bucketed by [`LIGHT_CELL`].
    light_grid: HashMap<(i32, i32), Vec<StaticLight>>,
    since_lights: f32,
    char_lights: Vec<StaticLight>,
    /// The world's wind entity, waiting to be written into the materials.
    pending_wind: Option<SpeedTreeWind>,
    /// The world's precipitation entity and the emitter the weather currently asks for at the
    /// camera (`eCWeatherAdmin::Process` -> `eCPrecipitation_PS`).
    precip_base: PrecipParams,
    pub precip: PrecipParams,
    /// The world's skydome entity (sun / moon / star sizes).
    pub skydome: SkydomeParams,
}

impl OrigEnv {
    pub fn new(origin: Vec3, args: &[String]) -> Self {
        let arg = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        let day_time = arg("--hour")
            .and_then(|s| s.parse::<f32>().ok())
            .map(|h| (h / 24.0).rem_euclid(1.0))
            .or_else(|| std::env::var("GR_TIME").ok().and_then(|s| s.parse().ok()))
            .unwrap_or(DEFAULT_DAY_TIME);
        Self {
            enabled: std::env::var("GR_ORIG_ENV").map_or(true, |v| v != "0"),
            requested: false,
            admin: None,
            zones: Vec::new(),
            day_time,
            speed: arg("--day-speed")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0),
            packet: None,
            origin,
            force: arg("--weather"),
            pushed: None,
            since_push: PUSH_INTERVAL,
            light_grid: HashMap::new(),
            since_lights: LIGHT_INTERVAL,
            char_lights: Vec::new(),
            pending_wind: None,
            precip_base: PrecipParams::default(),
            precip: PrecipParams::default(),
            skydome: SkydomeParams::default(),
        }
    }

    /// Take the weather data and zones delivered by the loader.
    pub fn install(&mut self, payload: WeatherPayload) {
        let Some(data) = super::env_synth::weather_data(payload.data) else {
            eprintln!("environment: WeatherStates.g3ws missing, keeping the built-in light set");
            self.enabled = false;
            return;
        };
        let mut admin = WeatherAdmin::new(data, 0x6a09_e667);
        if let Some(state) = &self.force {
            admin.force_state("Myrtana", state);
        }
        admin.set_zones(payload.zones.iter().map(|z| z.zone.clone()).collect());
        eprintln!(
            "environment: {} weather states, {} zones, day time {:.3}",
            admin.data.states.len(),
            payload.zones.len(),
            self.day_time
        );
        for l in &payload.lights {
            // Gothic cm -> origin-relative mirrored metres (same mapping as the world tiles).
            let p = Vec3::new(
                self.origin.x - l.position.x,
                l.position.y - self.origin.y,
                l.position.z - self.origin.z,
            ) * 0.01;
            self.light_grid
                .entry((
                    (p.x / LIGHT_CELL).floor() as i32,
                    (p.z / LIGHT_CELL).floor() as i32,
                ))
                .or_default()
                .push(StaticLight {
                    position: p.to_array(),
                    color: l.color,
                    range: l.range * 0.01,
                });
        }
        eprintln!("environment: {} static point lights", payload.lights.len());
        self.pending_wind = payload.wind;
        if let Some(p) = payload.precipitation {
            self.precip_base = p;
        }
        if let Some(s) = payload.skydome {
            self.skydome = s;
        }
        self.zones = payload.zones;
        self.admin = Some(admin);
    }

    /// Gothic world position (cm) of a Bevy position (metres).
    fn gothic(&self, p: Vec3) -> Vec3 {
        Vec3::new(
            self.origin.x - p.x * 100.0,
            p.y * 100.0 + self.origin.y,
            p.z * 100.0 + self.origin.z,
        )
    }
}

pub struct EnvPlugin;

impl Plugin for EnvPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (env_request, env_update, env_shot).chain());
    }
}

/// Debug aid: `GR_ENV_SHOT=<file.png>:<seconds>` saves a screenshot of the window that many
/// seconds after the environment arrived and quits two seconds later.
fn env_shot(
    mut commands: Commands,
    time: Res<Time>,
    env: Res<OrigEnv>,
    mut exit: MessageWriter<AppExit>,
    mut state: Local<(f32, bool)>,
) {
    let Ok(spec) = std::env::var("GR_ENV_SHOT") else {
        return;
    };
    if env.packet.is_none() {
        return;
    }
    let (path, secs) = spec.rsplit_once(':').unwrap_or((spec.as_str(), "20"));
    let secs: f32 = secs.parse().unwrap_or(20.0);
    state.0 += time.delta_secs();
    if !state.1 && state.0 >= secs {
        use bevy::render::view::screenshot::{Screenshot, save_to_disk};
        state.1 = true;
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path.to_string()));
    }
    if state.1 && state.0 >= secs + 2.0 {
        exit.write(AppExit::Success);
    }
}

fn env_request(
    mut env: ResMut<OrigEnv>,
    stream: Res<super::Stream>,
    loader: Res<super::LoaderRes>,
) {
    if env.enabled && !env.requested && stream.ready {
        env.requested = true;
        let _ = loader.requests.send(Request::Weather);
    }
}

#[allow(clippy::too_many_arguments)]
fn env_update(
    time: Res<Time>,
    mut env: ResMut<OrigEnv>,
    mut sun_dir: ResMut<SunDir>,
    mut fog_end: ResMut<FogEnd>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<GlobalAmbientLight>,
    sky: Option<Res<SkyHandle>>,
    mut skies: ResMut<Assets<SkyMaterial>>,
    mut gothic: ResMut<Assets<GothicMaterial>>,
    mut chars: ResMut<Assets<CharMaterial>>,
    camera: Query<&Transform, With<PlayerCamera>>,
    mut lights: Query<&mut Transform, (With<DirectionalLight>, Without<PlayerCamera>)>,
    mut fog: Query<&mut DistanceFog, With<PlayerCamera>>,
) {
    if !env.enabled || env.admin.is_none() {
        return;
    }
    let dt = time.delta_secs();
    let advance = env.speed * dt;
    env.day_time = (env.day_time + advance).rem_euclid(1.0);
    env.since_push += dt;
    let Ok(cam) = camera.single() else {
        return;
    };
    let probe = env.gothic(cam.translation);
    let t = env.day_time;
    let (rec, precip) = {
        let env = &mut *env;
        let admin = env.admin.as_mut().expect("checked above");
        if advance > 0.0 {
            admin.step(advance);
        }
        let zones = &env.zones;
        let mut local = |i: usize, _: &gothic_environment::Zone| {
            Some(zones[i].world_to_local.transform_point3(probe).to_array())
        };
        (
            admin.evaluate(t, &mut local),
            admin.evaluate_precipitation(&env.precip_base, &mut local),
        )
    };
    env.precip = precip;
    if let Some(w) = env.pending_wind.take() {
        eprintln!(
            "environment: SpeedTree wind strength {:.2}, gusts {:?}",
            w.strength, w.gust_strength
        );
        set_wind_env(WindEnv::new(&w), &mut gothic);
    }
    let p = packet(&rec, t, SUN_AZIMUTH, FAR_CM);
    if std::env::var_os("GR_ENV_LOG").is_some()
        && (time.elapsed_secs() as u32) % 5 == 0
        && time.delta_secs() > 0.0
        && (time.elapsed_secs().fract() < time.delta_secs())
    {
        eprintln!(
            "ENV t={t:.3} probe={probe:?} sun={:?} sh0={:?} fog={:?} {:?}..{:?} sky={:?} haze={:?} dens={}",
            p.sun_color,
            p.sh[0],
            p.fog_color,
            p.fog_start,
            p.fog_end,
            p.sky_color,
            p.haze_color,
            p.shadow_density
        );
    }
    env.packet = Some(p);
    let to_light = Vec3::from(p.sun.to_light);
    sun_dir.0 = Vec3::from(p.sun.to_sun);
    if let Ok(mut l) = lights.single_mut() {
        *l = Transform::default().looking_to(-to_light, Vec3::Y);
    }
    let srgb = |c: [f32; 3]| {
        Color::srgb(
            c[0].clamp(0.0, 1.0),
            c[1].clamp(0.0, 1.0),
            c[2].clamp(0.0, 1.0),
        )
    };
    clear.0 = srgb(p.fog_color);
    ambient.color = srgb(p.sh[0]);
    // Fog: the original clips the end to the far plane; ours is the streamed world radius.
    let world_far_cm = fog_end.0 / super::CM;
    let (start_cm, end_cm) = gothic_environment::clip_fog(p.fog_start, p.fog_end, world_far_cm);
    if let Ok(mut f) = fog.single_mut() {
        f.color = srgb(p.fog_color);
        f.directional_light_color = srgb(p.sun_color.map(|c| c * 0.4));
        // Fog must not swallow the world when the camera is high above it.
        let lift = 3.0 * (cam.translation.y - 15.0).max(0.0);
        f.falloff = FogFalloff::Linear {
            start: start_cm * super::CM + lift * 0.35,
            end: end_cm * super::CM + lift,
        };
    }
    if let Some(handle) = sky
        && let Some(mut m) = skies.get_mut(&handle.0)
    {
        let v = |c: [f32; 3], w: f32| Vec4::new(c[0], c[1], c[2], w);
        m.params.sun_dir = p.sun.to_sun.into_vec4();
        m.params.zenith = v(p.sky_color, p.cloud_density);
        m.params.horizon = v(p.haze_color, 0.0);
        m.params.ground = v(p.fog_color, m.params.ground.w);
        m.params.sun_color = v(p.sky_sun_color, 0.0);
        m.params.cloud = v(p.cloud_color, 0.0);
        m.params.moon = Vec4::new(
            p.moon_dir[0],
            p.moon_dir[1],
            p.moon_dir[2],
            env.skydome.moon_size,
        );
        m.params.moon_color = v(env.skydome.moon_color, p.star_visibility);
        m.params.sun_misc.x = env.skydome.sun_size;
    }
    // Light set for the materials (sun colour x intensity, SH, shadow density).
    let sun = Vec4::new(p.sun_color[0], p.sun_color[1], p.sun_color[2], 4.0);
    let sh = [
        Vec4::new(p.sh[0][0], p.sh[0][1], p.sh[0][2], p.shadow_density),
        Vec4::new(p.sh[1][0], p.sh[1][1], p.sh[1][2], 0.0),
        Vec4::new(p.sh[2][0], p.sh[2][1], p.sh[2][2], 0.0),
        Vec4::new(p.sh[3][0], p.sh[3][1], p.sh[3][2], 0.0),
    ];
    set_light_env(sun, sh);
    let changed = env.pushed.is_none_or(|(ps, psh)| {
        let d = |a: Vec4, b: Vec4| (a - b).abs().max_element();
        d(ps, sun)
            .max(d(psh[0], sh[0]))
            .max(d(psh[1], sh[1]))
            .max(d(psh[2], sh[2]))
            .max(d(psh[3], sh[3]))
            > 0.02
    });
    // Static point lights around the camera for characters (`GetDynamicStaticLightData`, shared
    // set of the strongest lights instead of one packet per object).
    env.since_lights += dt;
    if env.since_lights >= LIGHT_INTERVAL && !env.light_grid.is_empty() {
        env.since_lights = 0.0;
        let c = cam.translation;
        let (cx, cz) = (
            (c.x / LIGHT_CELL).floor() as i32,
            (c.z / LIGHT_CELL).floor() as i32,
        );
        let near = (-1..=1).flat_map(|dx| (-1..=1).map(move |dz| (cx + dx, cz + dz)));
        let cand: Vec<StaticLight> = near
            .filter_map(|k| env.light_grid.get(&k))
            .flatten()
            .copied()
            .collect();
        let sel = lights::select(c.to_array(), 12.0, cand, CHAR_LIGHTS);
        if sel != env.char_lights {
            let mut pos = [Vec4::ZERO; CHAR_LIGHTS];
            let mut col = [Vec4::ZERO; CHAR_LIGHTS];
            for (i, l) in sel.iter().enumerate() {
                pos[i] = Vec4::new(
                    l.position[0],
                    l.position[1],
                    l.position[2],
                    1.0 / (l.range * l.range),
                );
                let k = lights::COLOR_BOOST;
                col[i] = Vec4::new(l.color[0] * k, l.color[1] * k, l.color[2] * k, 0.0);
            }
            push_char_lights(&mut chars, pos, col);
            env.char_lights = sel;
        }
    }
    if changed && env.since_push >= PUSH_INTERVAL {
        env.since_push = 0.0;
        env.pushed = Some((sun, sh));
        push_light_env(&mut gothic, &mut chars);
    }
}

trait IntoVec4 {
    fn into_vec4(self) -> Vec4;
}
impl IntoVec4 for [f32; 3] {
    fn into_vec4(self) -> Vec4 {
        Vec4::new(self[0], self[1], self[2], 0.0)
    }
}
