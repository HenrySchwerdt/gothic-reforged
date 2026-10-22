//! Original day-time environment: weather states, zones, sun path and the packets the renderer
//! consumes (directional light, hemisphere SH ambient, sky, fog).
//!
//! Bevy-free. The numbers come from `WeatherStates.g3ws` (see `gothic_formats::weather`); the
//! rules are ports of `eCWeatherAdmin::Process` (Engine.dll:0x1028d070),
//! `eCWeatherEnvironment::Process` (0x1028f780), `eCWeatherZone_PS::GetBoxBlendFactor`
//! (0x10296b60) / `DoOverwrites` (0x10296ed0) and `eCHemisphere_PS::UpdateSHHemisphere`
//! (0x100ecf50). Everything is plain data so a generic, data-driven weather system can be fed
//! from [`EnvPacket`].
pub mod precip;
pub mod sky;
pub mod wind;
use gothic_formats::weather::{WeatherData, WeatherState};

/// L0 / L1 real spherical-harmonic constants (floats at Engine.dll:0x10426c94 / 0x10426c98).
pub const SH_L0: f32 = 0.282_095;
pub const SH_L1: f32 = 0.488_602_5;
/// Sun rises at this day fraction and sets at [`SUNSET`] (constants of `Process`).
pub const SUNRISE: f32 = 0.25;
pub const SUNSET: f32 = 0.916_666_7;
/// Day fraction used for the "fixed" probe mode and as default start (11:00).
pub const DEFAULT_DAY_TIME: f32 = 0.46;

/// The blended environment record (`eCWeatherAdmin` +0x34, 0xA0 bytes): the same 16 fields as one
/// state at one point of the day. Colours are `[r, g, b, a]`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EnvRecord {
    pub sun_diffuse: [f32; 4],
    pub sun_specular: [f32; 4],
    pub sun_intensity: f32,
    pub shadow_density: f32,
    pub ambient_back: [f32; 4],
    pub ambient_general: [f32; 4],
    pub ambient_intensity: f32,
    pub fog_color: [f32; 4],
    pub fog_start: f32,
    pub fog_end: f32,
    pub sky_color: [f32; 4],
    pub haze_color: [f32; 4],
    pub cloud_density: f32,
    pub cloud_color: [f32; 4],
    pub extra_a: f32,
    pub extra_b: f32,
}

fn lerp4(a: [f32; 4], b: [f32; 4], f: f32) -> [f32; 4] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * f)
}

impl EnvRecord {
    /// Port of `eCWeatherStates::SetCurrentDayTime` (0x102915c0): evaluate every scale at `t`.
    pub fn from_state(s: &WeatherState, t: f32) -> Self {
        gothic_portcov::hit!("from_state");
        let k = &s.scales;
        Self {
            sun_diffuse: k.sun_diffuse.at(t),
            sun_specular: k.sun_specular.at(t),
            sun_intensity: k.sun_intensity.at(t),
            shadow_density: k.shadow_density.at(t),
            ambient_back: k.ambient_back.at(t),
            ambient_general: k.ambient_general.at(t),
            ambient_intensity: k.ambient_intensity.at(t),
            fog_color: k.fog_color.at(t),
            fog_start: k.fog_start.at(t),
            fog_end: k.fog_end.at(t),
            sky_color: k.sky_color.at(t),
            haze_color: k.haze_color.at(t),
            cloud_density: k.cloud_density.at(t),
            cloud_color: k.cloud_color.at(t),
            extra_a: k.extra_a.at(t),
            extra_b: k.extra_b.at(t),
        }
    }

    /// Field-wise linear mix (`self * (1 - f) + other * f`).
    pub fn mix(&self, other: &Self, f: f32) -> Self {
        let m = |a: f32, b: f32| a + (b - a) * f;
        Self {
            sun_diffuse: lerp4(self.sun_diffuse, other.sun_diffuse, f),
            sun_specular: lerp4(self.sun_specular, other.sun_specular, f),
            sun_intensity: m(self.sun_intensity, other.sun_intensity),
            shadow_density: m(self.shadow_density, other.shadow_density),
            ambient_back: lerp4(self.ambient_back, other.ambient_back, f),
            ambient_general: lerp4(self.ambient_general, other.ambient_general, f),
            ambient_intensity: m(self.ambient_intensity, other.ambient_intensity),
            fog_color: lerp4(self.fog_color, other.fog_color, f),
            fog_start: m(self.fog_start, other.fog_start),
            fog_end: m(self.fog_end, other.fog_end),
            sky_color: lerp4(self.sky_color, other.sky_color, f),
            haze_color: lerp4(self.haze_color, other.haze_color, f),
            cloud_density: m(self.cloud_density, other.cloud_density),
            cloud_color: lerp4(self.cloud_color, other.cloud_color, f),
            extra_a: m(self.extra_a, other.extra_a),
            extra_b: m(self.extra_b, other.extra_b),
        }
    }

    /// Weighted accumulation (`FUN_102905e0` scale + `FUN_10290510` add of the original).
    pub fn accumulate(&mut self, other: &Self, w: f32) {
        let a4 = |d: &mut [f32; 4], s: [f32; 4]| d.iter_mut().zip(s).for_each(|(d, s)| *d += s * w);
        a4(&mut self.sun_diffuse, other.sun_diffuse);
        a4(&mut self.sun_specular, other.sun_specular);
        a4(&mut self.ambient_back, other.ambient_back);
        a4(&mut self.ambient_general, other.ambient_general);
        a4(&mut self.fog_color, other.fog_color);
        a4(&mut self.sky_color, other.sky_color);
        a4(&mut self.haze_color, other.haze_color);
        a4(&mut self.cloud_color, other.cloud_color);
        self.sun_intensity += other.sun_intensity * w;
        self.shadow_density += other.shadow_density * w;
        self.ambient_intensity += other.ambient_intensity * w;
        self.fog_start += other.fog_start * w;
        self.fog_end += other.fog_end * w;
        self.cloud_density += other.cloud_density * w;
        self.extra_a += other.extra_a * w;
        self.extra_b += other.extra_b * w;
    }
}

/// Overwrite mode of a zone field (`eEWeatherZoneOverwrite`): 0 none, 1 replace, 2 multiply, 3 add.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Overwrite {
    #[default]
    None,
    Replace,
    Multiply,
    Add,
}

impl Overwrite {
    pub fn from_raw(v: i32) -> Self {
        match v {
            1 => Self::Replace,
            2 => Self::Multiply,
            3 => Self::Add,
            _ => Self::None,
        }
    }
    pub fn apply(self, current: f32, value: f32) -> f32 {
        match self {
            Self::None => current,
            Self::Replace => value,
            Self::Multiply => current * value,
            Self::Add => current + value,
        }
    }
}

/// A zone field: overwrite mode plus operand (colours use the rgb operand, alpha is kept).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Field<T> {
    pub mode: Overwrite,
    pub value: T,
}

/// The overwrite set of an `eCWeatherZone_PS`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ZoneOverwrites {
    pub light_diffuse: Field<[f32; 3]>,
    pub light_specular: Field<[f32; 3]>,
    pub light_intensity: Field<f32>,
    pub ambient_back: Field<[f32; 3]>,
    pub ambient_general: Field<[f32; 3]>,
    pub ambient_intensity: Field<f32>,
    pub fog_color: Field<[f32; 3]>,
    pub fog_start: Field<f32>,
    pub fog_end: Field<f32>,
    pub sky_color: Field<[f32; 3]>,
    pub haze_color: Field<[f32; 3]>,
    pub cloud_thickness: Field<f32>,
    pub cloud_color: Field<[f32; 3]>,
}

impl ZoneOverwrites {
    /// Port of `eCWeatherZone_PS::DoOverwrites` (Engine.dll:0x10296ed0).
    pub fn apply(&self, r: &mut EnvRecord) {
        gothic_portcov::hit!("apply");
        let col = |c: &mut [f32; 4], f: &Field<[f32; 3]>| {
            for (v, o) in c.iter_mut().zip(f.value) {
                *v = f.mode.apply(*v, o);
            }
        };
        col(&mut r.sun_diffuse, &self.light_diffuse);
        col(&mut r.sun_specular, &self.light_specular);
        r.sun_intensity = self
            .light_intensity
            .mode
            .apply(r.sun_intensity, self.light_intensity.value);
        col(&mut r.ambient_back, &self.ambient_back);
        col(&mut r.ambient_general, &self.ambient_general);
        r.ambient_intensity = self
            .ambient_intensity
            .mode
            .apply(r.ambient_intensity, self.ambient_intensity.value);
        col(&mut r.fog_color, &self.fog_color);
        r.fog_start = self.fog_start.mode.apply(r.fog_start, self.fog_start.value);
        r.fog_end = self.fog_end.mode.apply(r.fog_end, self.fog_end.value);
        col(&mut r.sky_color, &self.sky_color);
        col(&mut r.haze_color, &self.haze_color);
        r.cloud_density = self
            .cloud_thickness
            .mode
            .apply(r.cloud_density, self.cloud_thickness.value);
        col(&mut r.cloud_color, &self.cloud_color);
    }
}

/// `eEWeatherZoneShape`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneShape {
    Circle2d,
    Rect2d,
    Sphere3d,
    Box3d,
}

impl ZoneShape {
    pub fn from_raw(v: i32) -> Self {
        match v {
            0 => Self::Circle2d,
            1 => Self::Rect2d,
            2 => Self::Sphere3d,
            _ => Self::Box3d,
        }
    }
}

/// One weather zone in its local frame.
#[derive(Debug, Clone)]
pub struct Zone {
    pub shape: ZoneShape,
    pub inner_extends: [f32; 3],
    pub outer_extends: [f32; 3],
    pub inner_radius: f32,
    pub outer_radius: f32,
    pub environment: String,
    pub overwrites: ZoneOverwrites,
}

impl Zone {
    /// Port of `eCWeatherZone_PS::GetBoxBlendFactor` (0x10296b60) and, by analogy [I], the
    /// circle / rect / sphere variants. `local` is the probe in the zone's local frame (cm).
    pub fn blend_factor(&self, local: [f32; 3]) -> f32 {
        gothic_portcov::hit!("blend_factor");
        match self.shape {
            ZoneShape::Box3d => ramp(
                &[local[0], local[1], local[2]],
                &self.inner_extends,
                &self.outer_extends,
            ),
            ZoneShape::Rect2d => ramp(
                &[local[0], local[2]],
                &[self.inner_extends[0], self.inner_extends[2]],
                &[self.outer_extends[0], self.outer_extends[2]],
            ),
            ZoneShape::Circle2d => {
                let d = (local[0] * local[0] + local[2] * local[2]).sqrt();
                ramp(&[d], &[self.inner_radius], &[self.outer_radius])
            }
            ZoneShape::Sphere3d => {
                let d = (local[0].powi(2) + local[1].powi(2) + local[2].powi(2)).sqrt();
                ramp(&[d], &[self.inner_radius], &[self.outer_radius])
            }
        }
    }

    /// Size used to rank zones (smaller, more specific zones first).
    pub fn volume(&self) -> f32 {
        match self.shape {
            ZoneShape::Box3d => self.outer_extends.iter().product(),
            ZoneShape::Rect2d => self.outer_extends[0] * self.outer_extends[2],
            ZoneShape::Circle2d | ZoneShape::Sphere3d => self.outer_radius.powi(3),
        }
    }
}

/// Distance falloff shared by all shapes: 0 outside any outer extent, 1 inside all inner extents,
/// otherwise `1 - sqrt(sum(((|p| - inner) / (outer - inner))^2))` clamped at 0.
fn ramp(p: &[f32], inner: &[f32], outer: &[f32]) -> f32 {
    let mut inside = true;
    let mut sum = 0.0;
    for i in 0..p.len() {
        let a = p[i].abs();
        if a > outer[i] {
            return 0.0;
        }
        if a >= inner[i] {
            inside = false;
            let span = outer[i] - inner[i];
            if span > 0.0 {
                let f = (a - inner[i]) / span;
                sum += f * f;
            }
        }
    }
    if inside {
        1.0
    } else {
        (1.0 - f32::sqrt(sum)).max(0.0)
    }
}

/// Sun and moon placement for a day fraction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SunState {
    /// Between sunrise and sunset.
    pub day: bool,
    /// Progress along the current arc (sun by day, moon by night), 0..1.
    pub progress: f32,
    /// Unit vector towards the light source (sun by day, moon by night), y up.
    pub to_light: [f32; 3],
    /// Unit vector towards the sun (below the horizon at night).
    pub to_sun: [f32; 3],
    /// `clamp((90 deg - |a|) / (90 deg - 60 deg), 0, 1)`: fades the sky sun colour near the horizon.
    pub height_factor: f32,
}

/// Highest elevation of the arc in degrees and its azimuth (radians) [I]: the original builds the
/// light from euler angles (arc angle swept by day time, fixed 60 degree term); the azimuth is the
/// level's authoring choice and is a parameter here.
pub const ARC_MAX_ELEVATION_DEG: f32 = 60.0;

/// Port of the sun/moon part of `eCWeatherAdmin::Process`: day progress
/// `(t - 0.25) / (0.9167 - 0.25)`, night progress over the remaining third, angle
/// `progress * pi - pi / 2`.
pub fn sun_state(t: f32, azimuth: f32) -> SunState {
    gothic_portcov::hit!("sun_state");
    let t = t.rem_euclid(1.0);
    let day = t > SUNRISE && t < SUNSET;
    let progress = if day {
        (t - SUNRISE) / (SUNSET - SUNRISE)
    } else {
        ((t + 1.0 - SUNSET).rem_euclid(1.0)) / (1.0 - (SUNSET - SUNRISE))
    };
    let angle = progress * std::f32::consts::PI - std::f32::consts::FRAC_PI_2;
    let height_factor = ((std::f32::consts::FRAC_PI_2 - angle.abs())
        / (std::f32::consts::FRAC_PI_2 - 60f32.to_radians()))
    .clamp(0.0, 1.0);
    // Arc from the horizon (east) over the highest point (towards `azimuth`) to the west.
    let e = ARC_MAX_ELEVATION_DEG.to_radians();
    let (sa, ca) = azimuth.sin_cos();
    let east = [ca, 0.0, -sa];
    let up_dir = [sa * e.cos(), e.sin(), ca * e.cos()];
    let (s, c) = (progress * std::f32::consts::PI).sin_cos();
    // c: -1..1 along the east-west axis (sunrise side first), s: height.
    let v: [f32; 3] = std::array::from_fn(|i| -c * east[i] + s * up_dir[i]);
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    let to_light = [v[0] / n, v[1] / n, v[2] / n];
    let to_sun = if day {
        to_light
    } else {
        [-to_light[0], -to_light[1].abs(), -to_light[2]]
    };
    SunState {
        day,
        progress,
        to_light,
        to_sun,
        height_factor,
    }
}

/// What the renderer needs per frame (all colours linear-as-authored, i.e. gamma-space values of
/// the original shaders, intensities already multiplied in).
#[derive(Debug, Clone, Copy)]
pub struct EnvPacket {
    pub day_time: f32,
    pub sun: SunState,
    /// Directional light diffuse x intensity.
    pub sun_color: [f32; 3],
    pub sun_specular: [f32; 3],
    /// Shader `vecShadowDensity` source.
    pub shadow_density: f32,
    /// `arrSHCofficients` (4 x rgb).
    pub sh: [[f32; 3]; 4],
    pub sky_color: [f32; 3],
    pub haze_color: [f32; 3],
    /// Sky sun colour (sun diffuse faded by the height factor).
    pub sky_sun_color: [f32; 3],
    pub cloud_color: [f32; 3],
    pub cloud_density: f32,
    pub fog_color: [f32; 3],
    /// cm, already clipped to the far plane.
    pub fog_start: f32,
    pub fog_end: f32,
    /// Unit vector towards the moon (see [`sky::moon_direction`]).
    pub moon_dir: [f32; 3],
    /// Visibility of the star field 0..1 (see [`sky::star_visibility`]).
    pub star_visibility: f32,
}

/// Port of the fog clip at the end of `eCWeatherAdmin::Process`: when the fog end exceeds the far
/// plane the start is scaled by the same ratio and the end becomes the far plane.
pub fn clip_fog(start: f32, end: f32, far: f32) -> (f32, f32) {
    gothic_portcov::hit!("clip_fog");
    if end <= far {
        (start, end)
    } else {
        (start / end * far, far)
    }
}

/// Port of `eCHemisphere_PS::UpdateSHHemisphere` (0x100ecf50) for the weather-driven hemisphere:
/// the general ambient is the constant term, the back light adds `colour * 0.282` to L0 and
/// `colour * 0.4886 * dir` to L1. The back light direction is the side opposite the light [I].
pub fn hemisphere_sh(
    general: [f32; 3],
    back: [f32; 3],
    back_dir: [f32; 3],
    k: f32,
) -> [[f32; 3]; 4] {
    gothic_portcov::hit!("hemisphere_sh");
    let mut sh = [[0.0; 3]; 4];
    for c in 0..3 {
        sh[0][c] = (general[c] + back[c] * SH_L0) * k;
        for a in 0..3 {
            sh[1 + a][c] = back[c] * SH_L1 * back_dir[a] * k;
        }
    }
    sh
}

/// Port of the light block of `eCWeatherAdmin::Process` (Engine.dll:0x1028d070, x87 arithmetic
/// read from the machine code at 0x1028d919..0x1028dd3b; the decompiler loses it). With the sun
/// height factor `H` (`sun_state.height_factor`) the directional light's `Intensity` is the
/// record intensity times `H` (0x1028da86 `fmul`), and the hemisphere gets
/// `BackLight = ambient_back * (0.5 + 0.5 H) + sun * I * (0.5 - 0.5 H)^2` and
/// `SunLight = sun * I * (0.5 + 0.5 H) * (0.5 - 0.5 H) + ambient_back * (0.5 - 0.5 H)` (the
/// colour products are `bCFloatColor::GetProduct` calls 0x10396c20, the sums `GetSum` 0x10397070).
/// Returns `(sun diffuse * I * H, back light colour, sun light colour)`.
pub fn weather_light_terms(
    sun_diffuse: [f32; 3],
    intensity: f32,
    ambient_back: [f32; 3],
    h: f32,
) -> ([f32; 3], [f32; 3], [f32; 3]) {
    gothic_portcov::hit!("weather_light_terms");
    let a = 0.5 + 0.5 * h;
    let b = 1.0 - a;
    let mut dir = [0.0; 3];
    let mut back = [0.0; 3];
    let mut sun = [0.0; 3];
    for c in 0..3 {
        let si = sun_diffuse[c] * intensity;
        dir[c] = si * h;
        back[c] = ambient_back[c] * a + si * b * b;
        sun[c] = si * a * b + ambient_back[c] * b;
    }
    (dir, back, sun)
}

/// Turn a blended record into the renderer packet.
pub fn packet(rec: &EnvRecord, day_time: f32, azimuth: f32, far_cm: f32) -> EnvPacket {
    let sun = sun_state(day_time, azimuth);
    let rgb = |c: [f32; 4]| [c[0], c[1], c[2]];
    let scale = |c: [f32; 3], k: f32| [c[0] * k, c[1] * k, c[2] * k];
    let back_dir = [-sun.to_light[0], -sun.to_light[1], -sun.to_light[2]];
    let (dir_diffuse, back, sun_light) = weather_light_terms(
        rgb(rec.sun_diffuse),
        rec.sun_intensity,
        rgb(rec.ambient_back),
        sun.height_factor,
    );
    let mut sh = hemisphere_sh(
        rgb(rec.ambient_general),
        back,
        back_dir,
        rec.ambient_intensity,
    );
    // `UpdateSHHemisphere` (0x100ecf50) adds the SunLight colour along the sun axis as well
    // (helper 0x100edbf0: colour * 0.282095 to L0, colour * 0.4886025 * dir to L1).
    for c in 0..3 {
        let col = sun_light[c] * rec.ambient_intensity;
        sh[0][c] += col * SH_L0;
        for a in 0..3 {
            sh[1 + a][c] += col * SH_L1 * sun.to_light[a];
        }
    }
    let (fog_start, fog_end) = clip_fog(rec.fog_start, rec.fog_end, far_cm);
    EnvPacket {
        day_time,
        sun,
        sun_color: dir_diffuse,
        sun_specular: scale(rgb(rec.sun_specular), rec.sun_intensity * sun.height_factor),
        shadow_density: rec.shadow_density,
        sh,
        sky_color: rgb(rec.sky_color),
        haze_color: rgb(rec.haze_color),
        sky_sun_color: scale(rgb(rec.sun_diffuse), sun.height_factor),
        cloud_color: rgb(rec.cloud_color),
        cloud_density: rec.cloud_density,
        fog_color: rgb(rec.fog_color),
        fog_start,
        fog_end,
        moon_dir: sky::moon_direction(&sun),
        star_visibility: sky::star_visibility(rgb(rec.sky_color)),
    }
}

/// Small deterministic generator (xorshift64*) for the weather rolls.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    pub fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let v = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (v >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Runtime of one weather environment: which state is current, which comes next and how far the
/// cross-fade has progressed (`eCWeatherEnvironment::Process`, 0x1028f780).
#[derive(Debug, Clone)]
struct EnvRun {
    name: String,
    /// Indices into `WeatherData::states`.
    current: usize,
    next: usize,
    /// Remaining days of the current state.
    remaining: f32,
    /// Cross-fade 0..1 between `current` and `next`.
    blend: f32,
}

/// Length of the cross-fade between two states, in days (one hour).
const FADE_DAYS: f32 = 1.0 / 24.0;

/// The weather admin: owns the states, runs the environments and blends zones.
#[derive(Debug, Clone)]
pub struct WeatherAdmin {
    pub data: WeatherData,
    pub default_env: String,
    runs: Vec<EnvRun>,
    zones: Vec<Zone>,
    order: Vec<usize>,
    rng: Rng,
}

impl WeatherAdmin {
    pub fn new(data: WeatherData, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let runs = data
            .environments
            .iter()
            .map(|e| {
                // A new game starts in the most probable state (Sunny for Myrtana).
                let cur = most_probable(&data, e);
                let next = pick_state(&data, e, &mut rng);
                let dur = pick_duration(&data, e, cur, &mut rng);
                EnvRun {
                    name: e.name.clone(),
                    current: cur,
                    next,
                    remaining: dur,
                    blend: 0.0,
                }
            })
            .collect();
        Self {
            data,
            default_env: "Myrtana".to_string(),
            runs,
            zones: Vec::new(),
            order: Vec::new(),
            rng,
        }
    }

    pub fn set_zones(&mut self, zones: Vec<Zone>) {
        let mut order: Vec<usize> = (0..zones.len()).collect();
        order.sort_by(|&a, &b| zones[a].volume().total_cmp(&zones[b].volume()));
        self.zones = zones;
        self.order = order;
    }

    pub fn zones(&self) -> &[Zone] {
        &self.zones
    }

    /// Force one environment into a named state (for tests, scripts and the `--weather` flag).
    pub fn force_state(&mut self, env: &str, state: &str) {
        let Some(si) = self.data.states.iter().position(|s| s.name == state) else {
            return;
        };
        for r in self.runs.iter_mut().filter(|r| r.name == env) {
            r.current = si;
            r.next = si;
            r.remaining = f32::MAX;
            r.blend = 0.0;
        }
    }

    /// Advance all environments by `dt_days` (fractions of a day).
    pub fn step(&mut self, dt_days: f32) {
        for (run, env) in self.runs.iter_mut().zip(&self.data.environments) {
            run.remaining -= dt_days;
            if run.remaining < FADE_DAYS && run.current != run.next {
                run.blend = (1.0 - run.remaining / FADE_DAYS).clamp(0.0, 1.0);
            }
            if run.remaining <= 0.0 {
                run.current = run.next;
                run.blend = 0.0;
                run.next = pick_state(&self.data, env, &mut self.rng);
                run.remaining = pick_duration(&self.data, env, run.current, &mut self.rng);
            }
        }
    }

    fn env_record(&self, name: &str, t: f32) -> Option<EnvRecord> {
        let run = self.runs.iter().find(|r| r.name == name)?;
        let a = EnvRecord::from_state(&self.data.states[run.current], t);
        if run.blend <= 0.0 || run.current == run.next {
            return Some(a);
        }
        let b = EnvRecord::from_state(&self.data.states[run.next], t);
        Some(a.mix(&b, run.blend))
    }

    /// Precipitation emitter at a probe position: the states of the zones' environments (with
    /// their cross-fades) blended with the same weights as [`WeatherAdmin::evaluate`], applied to
    /// the world's `base` emitter (`eCWeatherAdmin::Process` pushes the state values into the
    /// precipitation entity).
    pub fn evaluate_precipitation(
        &self,
        base: &precip::PrecipParams,
        mut local_of: impl FnMut(usize, &Zone) -> Option<[f32; 3]>,
    ) -> precip::PrecipParams {
        let mut acc: Option<precip::PrecipParams> = None;
        let mut total = 0.0;
        let mut add = |p: precip::PrecipParams, w: f32, acc: &mut Option<precip::PrecipParams>| {
            total += w;
            *acc = Some(match acc.take() {
                None => p,
                Some(a) => a.mix(&p, w / total),
            });
        };
        let mut used = 0.0;
        for &zi in &self.order {
            let zone = &self.zones[zi];
            let Some(local) = local_of(zi, zone) else {
                continue;
            };
            let f = zone.blend_factor(local);
            if f <= 0.0 {
                continue;
            }
            let w = f.min(1.0 - used);
            let Some(p) = self.env_precipitation(&zone.environment, base) else {
                continue;
            };
            add(p, w, &mut acc);
            used += w;
            if used >= 1.0 {
                break;
            }
        }
        if used < 1.0
            && let Some(p) = self.env_precipitation(&self.default_env, base)
        {
            add(p, 1.0 - used, &mut acc);
        }
        acc.unwrap_or_else(|| base.clone())
    }

    fn env_precipitation(
        &self,
        name: &str,
        base: &precip::PrecipParams,
    ) -> Option<precip::PrecipParams> {
        let run = self.runs.iter().find(|r| r.name == name)?;
        let a = base.with_state(&self.data.states[run.current]);
        if run.blend <= 0.0 || run.current == run.next {
            return Some(a);
        }
        let b = base.with_state(&self.data.states[run.next]);
        Some(a.mix(&b, run.blend))
    }

    /// Name of the state an environment currently shows.
    pub fn current_state(&self, env: &str) -> Option<&str> {
        let run = self.runs.iter().find(|r| r.name == env)?;
        Some(&self.data.states[run.current].name)
    }

    /// Blend at a probe position. `local_of(zone_index)` maps the probe into the zone's frame.
    /// Zones are visited most specific first; accumulated weight is capped at 1 and the remainder
    /// goes to the default environment (Process step 3).
    pub fn evaluate(
        &self,
        t: f32,
        mut local_of: impl FnMut(usize, &Zone) -> Option<[f32; 3]>,
    ) -> EnvRecord {
        let t = t.rem_euclid(1.0);
        let mut acc = EnvRecord::default();
        let mut total = 0.0;
        for &zi in &self.order {
            let zone = &self.zones[zi];
            let Some(local) = local_of(zi, zone) else {
                continue;
            };
            let f = zone.blend_factor(local);
            if f <= 0.0 {
                continue;
            }
            let w = f.min(1.0 - total);
            let Some(mut rec) = self.env_record(&zone.environment, t) else {
                continue;
            };
            zone.overwrites.apply(&mut rec);
            acc.accumulate(&rec, w);
            total += w;
            if total >= 1.0 {
                break;
            }
        }
        if total < 1.0 {
            let rec = self
                .env_record(&self.default_env, t)
                .unwrap_or_else(|| EnvRecord::from_state(&self.data.states[0], t));
            acc.accumulate(&rec, 1.0 - total);
        }
        acc
    }
}

fn most_probable(data: &WeatherData, env: &gothic_formats::weather::WeatherEnvironment) -> usize {
    env.entries
        .iter()
        .max_by(|a, b| a.probability.total_cmp(&b.probability))
        .and_then(|e| data.states.iter().position(|s| s.name == e.state))
        .unwrap_or(0)
}

fn pick_state(
    data: &WeatherData,
    env: &gothic_formats::weather::WeatherEnvironment,
    rng: &mut Rng,
) -> usize {
    let total: f32 = env.entries.iter().map(|e| e.probability.max(0.0)).sum();
    let mut roll = rng.next_f32() * total.max(1e-6);
    for e in &env.entries {
        roll -= e.probability.max(0.0);
        if roll <= 0.0
            && e.probability > 0.0
            && let Some(i) = data.states.iter().position(|s| s.name == e.state)
        {
            return i;
        }
    }
    env.entries
        .iter()
        .filter(|e| e.probability > 0.0)
        .find_map(|e| data.states.iter().position(|s| s.name == e.state))
        .unwrap_or(0)
}

fn pick_duration(
    data: &WeatherData,
    env: &gothic_formats::weather::WeatherEnvironment,
    state: usize,
    rng: &mut Rng,
) -> f32 {
    let name = &data.states[state].name;
    env.entries
        .iter()
        .find(|e| &e.state == name)
        .map_or(0.1, |e| {
            let (lo, hi) = (
                e.min_duration.min(e.max_duration),
                e.max_duration.max(e.min_duration),
            );
            (lo + (hi - lo) * rng.next_f32()).max(FADE_DAYS * 1.5)
        })
}

impl Zone {
    /// Build a zone from the decoded `eCWeatherZone_PS` property set of a world entity.
    pub fn from_properties(set: &gothic_formats::propstream::PropertySet) -> Option<Self> {
        use gothic_formats::propstream::PropValue;
        let f = |n: &str| set.float(n).unwrap_or(0.0);
        let v3 = |n: &str| match set.get(n) {
            Some(PropValue::Vec3(v)) | Some(PropValue::ColorRgb(v)) | Some(PropValue::Euler(v)) => {
                *v
            }
            _ => [0.0; 3],
        };
        let mode = |n: &str| match set.get(n) {
            Some(PropValue::Enum { value, .. }) => Overwrite::from_raw(*value),
            _ => Overwrite::None,
        };
        let shape = match set.get("Shape") {
            Some(PropValue::Enum { value, .. }) => ZoneShape::from_raw(*value),
            _ => return None,
        };
        let env = set.string("WeatherEnvironment")?.to_string();
        let fl = |m: &str, v: &str| Field {
            mode: mode(m),
            value: f(v),
        };
        let co = |m: &str, v: &str| Field {
            mode: mode(m),
            value: v3(v),
        };
        Some(Self {
            shape,
            inner_extends: v3("InnerExtends"),
            outer_extends: v3("OuterExtends"),
            inner_radius: f("InnerRadius"),
            outer_radius: f("OuterRadius"),
            environment: env,
            overwrites: ZoneOverwrites {
                light_diffuse: co("LightDiffuseOverwrite", "LightDiffuseColor"),
                light_specular: co("LightSpecularOverwrite", "LightSpecularColor"),
                light_intensity: fl("LightIntensityOverwrite", "LightIntensity"),
                ambient_back: co("AmbientBackLightOverwrite", "AmbientBackLightColor"),
                ambient_general: co("AmbientGeneralOverwrite", "AmbientGeneralColor"),
                ambient_intensity: fl("AmbientIntensityOverwrite", "AmbientIntensity"),
                fog_color: co("FogColorOverwrite", "FogColor"),
                fog_start: fl("FogStartrOverwrite", "FogStart"),
                fog_end: fl("FogEndrOverwrite", "FogEnd"),
                sky_color: co("SkyColorOverwrite", "SkyColor"),
                haze_color: co("HazeColorOverwrite", "HazeColor"),
                cloud_thickness: fl("CloudThicknessrOverwrite", "CloudThickness"),
                cloud_color: co("CloudColorOverwrite", "CloudColor"),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gothic_formats::weather::{ColorScale, EnvironmentEntry, FloatScale, WeatherEnvironment};

    fn state(name: &str, sun: f32) -> WeatherState {
        let mut s = WeatherState {
            name: name.into(),
            ..Default::default()
        };
        s.scales.sun_intensity = FloatScale {
            keys: vec![(0.0, sun), (1.0, sun)],
        };
        s.scales.sun_diffuse = ColorScale {
            keys: vec![(0.0, [1.0, 0.5, 0.25, 1.0]), (1.0, [1.0, 0.5, 0.25, 1.0])],
        };
        s
    }

    fn admin() -> WeatherAdmin {
        let data = WeatherData {
            states: vec![state("Sunny", 3.0), state("Rainy", 1.0)],
            environments: ["Myrtana", "Cave"]
                .iter()
                .map(|n| WeatherEnvironment {
                    name: (*n).into(),
                    entries: vec![
                        EnvironmentEntry {
                            state: "Sunny".into(),
                            probability: 1.0,
                            min_duration: 0.1,
                            max_duration: 0.2,
                        },
                        EnvironmentEntry {
                            state: "Rainy".into(),
                            probability: 0.0,
                            min_duration: 0.1,
                            max_duration: 0.2,
                        },
                    ],
                    climate: String::new(),
                    climate_id: 0,
                })
                .collect(),
        };
        WeatherAdmin::new(data, 7)
    }

    #[test]
    fn box_factor_inside_edge_outside() {
        let z = Zone {
            shape: ZoneShape::Box3d,
            inner_extends: [100.0, 100.0, 100.0],
            outer_extends: [200.0, 200.0, 200.0],
            inner_radius: 0.0,
            outer_radius: 0.0,
            environment: "Cave".into(),
            overwrites: ZoneOverwrites::default(),
        };
        assert_eq!(z.blend_factor([50.0, -50.0, 99.0]), 1.0);
        assert_eq!(z.blend_factor([201.0, 0.0, 0.0]), 0.0);
        assert!((z.blend_factor([150.0, 0.0, 0.0]) - 0.5).abs() < 1e-6);
        // Two axes in the ramp: 1 - sqrt(0.25 + 0.25).
        assert!((z.blend_factor([150.0, 150.0, 0.0]) - (1.0 - 0.5f32.sqrt())).abs() < 1e-6);
    }

    #[test]
    fn overwrite_modes() {
        assert_eq!(Overwrite::Replace.apply(3.0, 0.0), 0.0);
        assert_eq!(Overwrite::Multiply.apply(3.0, 0.5), 1.5);
        assert_eq!(Overwrite::Add.apply(3.0, 0.5), 3.5);
        assert_eq!(Overwrite::None.apply(3.0, 9.0), 3.0);
    }

    #[test]
    fn dungeon_zone_replaces_light() {
        let mut a = admin();
        let ov = ZoneOverwrites {
            light_intensity: Field {
                mode: Overwrite::Replace,
                value: 0.0,
            },
            ..ZoneOverwrites::default()
        };
        a.set_zones(vec![Zone {
            shape: ZoneShape::Box3d,
            inner_extends: [10.0; 3],
            outer_extends: [20.0; 3],
            inner_radius: 0.0,
            outer_radius: 0.0,
            environment: "Cave".into(),
            overwrites: ov,
        }]);
        let inside = a.evaluate(0.5, |_, _| Some([0.0; 3]));
        assert_eq!(inside.sun_intensity, 0.0);
        let outside = a.evaluate(0.5, |_, _| Some([100.0; 3]));
        assert_eq!(outside.sun_intensity, 3.0);
        let edge = a.evaluate(0.5, |_, _| Some([15.0, 0.0, 0.0]));
        assert!((edge.sun_intensity - 1.5).abs() < 1e-5);
    }

    #[test]
    fn sun_path_phases() {
        let sunrise = sun_state(0.2501, 0.9);
        let noon = sun_state(0.5833, 0.9);
        let sunset = sun_state(0.9, 0.9);
        let night = sun_state(0.05, 0.9);
        assert!(sunrise.day && noon.day && sunset.day && !night.day);
        assert!(noon.to_light[1] > 0.8);
        assert!(sunrise.to_light[1].abs() < 0.05);
        assert!((noon.height_factor - 1.0).abs() < 1e-5);
        assert!(sunrise.height_factor < 0.05);
        assert!(
            night.to_light[1] > 0.0,
            "the moon is above the horizon at night"
        );
        assert!(night.to_sun[1] < 0.0);
    }

    #[test]
    fn fog_clipped_to_far_plane() {
        assert_eq!(clip_fog(1000.0, 4000.0, 8000.0), (1000.0, 4000.0));
        assert_eq!(clip_fog(2000.0, 20000.0, 10000.0), (1000.0, 10000.0));
    }

    #[test]
    fn sh_constants_and_back_light() {
        let sh = hemisphere_sh([1.0; 3], [2.0; 3], [0.0, 0.0, 1.0], 0.5);
        assert!((sh[0][0] - (1.0 + 2.0 * SH_L0) * 0.5).abs() < 1e-6);
        assert!((sh[3][1] - 2.0 * SH_L1 * 0.5).abs() < 1e-6);
        assert_eq!(sh[1][0], 0.0);
    }

    #[test]
    fn weather_rolls_follow_probabilities_and_fade() {
        let mut a = admin();
        // Rainy has probability 0: the roll never picks it.
        for _ in 0..200 {
            a.step(0.05);
            assert_eq!(a.current_state("Myrtana"), Some("Sunny"));
        }
        a.force_state("Myrtana", "Rainy");
        let r = a.evaluate(0.5, |_, _| None);
        assert_eq!(r.sun_intensity, 1.0);
    }

    #[test]
    fn precipitation_follows_the_forced_state() {
        let mut a = admin();
        a.data.states[1].particles_per_second = 3000;
        a.data.states[1].current_texture_tile = 2;
        let base = precip::PrecipParams::default();
        let dry = a.evaluate_precipitation(&base, |_, _| None);
        assert_eq!(dry.particles_per_second, 0.0);
        a.force_state("Myrtana", "Rainy");
        let wet = a.evaluate_precipitation(&base, |_, _| None);
        assert_eq!(wet.particles_per_second, 3000.0);
        assert_eq!(wet.current_tile, 2);
    }

    #[test]
    fn moon_and_stars_in_the_packet() {
        let mut rec = EnvRecord {
            sky_color: [0.02, 0.08, 0.15, 1.0],
            ..Default::default()
        };
        let night = packet(&rec, 0.0, 0.9, 100_000.0);
        assert_eq!(night.star_visibility, 1.0);
        assert!(night.moon_dir[1] >= 0.0);
        rec.sky_color = [0.3, 0.5, 0.75, 1.0];
        assert_eq!(packet(&rec, 0.5, 0.9, 100_000.0).star_visibility, 0.0);
    }

    #[test]
    fn scale_record_matches_state() {
        let s = state("X", 2.0);
        let r = EnvRecord::from_state(&s, 0.3);
        assert_eq!(r.sun_intensity, 2.0);
        let m = r.mix(&EnvRecord::from_state(&state("Y", 4.0), 0.3), 0.5);
        assert_eq!(m.sun_intensity, 3.0);
    }
}
