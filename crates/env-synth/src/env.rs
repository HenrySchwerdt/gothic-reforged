//! Clock + day table + weather + surface -> one frame of render parameters.
use crate::curve::{Lerp, Rgb};
use crate::day::DayTable;
use crate::sun::elevation_deg;
use crate::surface::Surface;
use crate::weather::{WeatherKind, WeatherMachine, WeatherTable};
use serde::Serialize;

/// Game clock in hours.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Clock {
    /// Hours since day 0, 00:00.
    pub hours: f64,
    /// Game hours per real second (placeholder: 1 game hour = 2 real minutes).
    pub hours_per_second: f64,
}

impl Clock {
    pub fn new(hours: f64) -> Self {
        Self {
            hours,
            hours_per_second: 1.0 / 120.0,
        }
    }

    pub fn hour_of_day(&self) -> f64 {
        self.hours.rem_euclid(24.0)
    }

    pub fn day(&self) -> u64 {
        (self.hours / 24.0).floor().max(0.0) as u64
    }
}

/// Fog for a camera (linear falloff).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Fog {
    pub color: Rgb,
    pub start: f32,
    pub end: f32,
}

/// Cloud layer parameters for the sky shader.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Clouds {
    pub cover: f32,
    pub darkness: f32,
    pub color: Rgb,
    /// Drift (m/s) along x and z.
    pub velocity: [f32; 2],
}

/// Precipitation overlay strengths 0..1 and wind (m/s along x, z).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct Precipitation {
    pub rain: f32,
    pub snow: f32,
    pub sand: f32,
    pub wind: [f32; 2],
}

/// Everything the renderer needs for one frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct EnvFrame {
    pub hour: f32,
    pub day: u64,
    pub weather: WeatherKind,
    pub weather_blend: f32,
    /// Unit vectors towards the sun / moon.
    pub sun_dir: [f32; 3],
    pub sun_elevation_deg: f32,
    pub sun_color: Rgb,
    pub sun_illuminance: f32,
    pub moon_dir: [f32; 3],
    pub moon_color: Rgb,
    pub moon_illuminance: f32,
    pub ambient_color: Rgb,
    pub ambient_brightness: f32,
    pub sky_zenith: Rgb,
    pub sky_horizon: Rgb,
    pub sun_glow: Rgb,
    pub stars: f32,
    pub clouds: Clouds,
    pub fog: Fog,
    pub precipitation: Precipitation,
    pub lightning: f32,
    pub wetness: f32,
    pub snow_cover: f32,
    /// Suggested camera exposure (EV100) for the scene light, so noon and night both read.
    pub exposure_ev100: f32,
}

/// Scene illuminance (lux) to EV100 calibration (100 000 lux -> EV 15, like Bevy's
/// `Exposure::SUNLIGHT`) and the EV range used.
pub const EXPOSURE_CALIBRATION: f32 = 3.0;
pub const EXPOSURE_RANGE: (f32, f32) = (5.0, 15.5);

/// EV100 for a scene lit by `lux` of direct light plus `ambient` cd/m^2.
pub fn exposure_for(lux: f32, ambient: f32) -> f32 {
    let e = (lux + ambient * std::f32::consts::PI).max(1e-3);
    (e / EXPOSURE_CALIBRATION)
        .log2()
        .clamp(EXPOSURE_RANGE.0, EXPOSURE_RANGE.1)
}

/// The whole environment state. Deterministic: the same tables, seed and `update` steps give
/// the same frames.
#[derive(Debug, Clone)]
pub struct Environment {
    pub day: DayTable,
    pub weather_table: WeatherTable,
    pub clock: Clock,
    pub weather: WeatherMachine,
    pub surface: Surface,
}

/// Fade of a body's light around the horizon (0 below -2 deg, 1 above +4 deg).
fn horizon_fade(elevation_deg: f32) -> f32 {
    let t = ((elevation_deg + 2.0) / 6.0).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Environment {
    pub fn new(day: DayTable, weather_table: WeatherTable, climate: &str, seed: u64) -> Self {
        let weather = WeatherMachine::new(&weather_table, climate, WeatherKind::Clear, seed);
        Self {
            day,
            weather_table,
            clock: Clock::new(8.0),
            weather,
            surface: Surface::default(),
        }
    }

    /// Advance by `dt` real seconds.
    pub fn update(&mut self, dt: f32) {
        self.clock.hours += self.clock.hours_per_second * f64::from(dt);
        self.weather.update(&self.weather_table, dt);
        let w = self.weather.current(&self.weather_table);
        let sun = self.day.sky_path.sun(self.clock.hours);
        self.surface.update(
            &self.weather_table.surface,
            &w,
            horizon_fade(elevation_deg(sun)),
            dt,
        );
    }

    pub fn force_weather(&mut self, kind: WeatherKind) {
        self.weather.force(&self.weather_table, kind);
    }

    pub fn frame(&self) -> EnvFrame {
        let hour = self.clock.hour_of_day();
        let d = self.day.sample(hour);
        let w = self.weather.current(&self.weather_table);
        let path = self.day.sky_path;
        let sun_dir = path.sun(hour);
        let moon_dir = path.moon(hour);
        let sun_el = elevation_deg(sun_dir);
        let overcast = (w.cloud_cover * w.cloud_darkness).clamp(0.0, 1.0);
        let lightning = self.weather.lightning();
        let grey = |c: Rgb| c.desaturate(overcast).scale(1.0 - 0.55 * overcast);
        let flash = Rgb(0.8, 0.85, 1.0).scale(lightning);
        let fog_density = w.fog_density.max(0.05);
        let wind = w.wind;
        let (s, c) = self.weather_table.wind_direction_deg.to_radians().sin_cos();
        let sun_illuminance = d.sun_illuminance * horizon_fade(sun_el) * (1.0 - w.sun_dim);
        let moon_illuminance = d.moon_illuminance
            * horizon_fade(elevation_deg(moon_dir))
            * (1.0 - w.cloud_cover * 0.8);
        let ambient_brightness = d.ambient_brightness * (1.0 + 4.0 * lightning);
        EnvFrame {
            hour: hour as f32,
            day: self.clock.day(),
            weather: self.weather.target(),
            weather_blend: self.weather.blend(),
            sun_dir,
            sun_elevation_deg: sun_el,
            sun_color: d.sun_color.desaturate(overcast * 0.5),
            sun_illuminance,
            moon_dir,
            moon_color: d.moon_color,
            moon_illuminance,
            ambient_color: grey(d.ambient_color).plus(flash),
            ambient_brightness,
            sky_zenith: grey(d.sky_zenith).plus(flash.scale(0.3)),
            sky_horizon: grey(d.sky_horizon).plus(flash.scale(0.3)),
            sun_glow: d.sun_glow.scale(1.0 - w.cloud_cover),
            stars: d.stars * (1.0 - w.cloud_cover),
            clouds: Clouds {
                cover: w.cloud_cover,
                darkness: w.cloud_darkness,
                color: d.cloud_color.scale(1.0 - 0.7 * w.cloud_darkness),
                velocity: [s * w.cloud_speed, -c * w.cloud_speed],
            },
            fog: Fog {
                color: Rgb::lerp(grey(d.fog_color), w.fog_tint, w.fog_tint_amount),
                start: d.fog_start / fog_density,
                end: d.fog_end / fog_density,
            },
            precipitation: Precipitation {
                rain: w.rain,
                snow: w.snow,
                sand: w.sand,
                wind: [s * wind, -c * wind],
            },
            lightning,
            wetness: self.surface.wetness,
            snow_cover: self.surface.snow_cover,
            // Exposure follows the base light, not lightning flashes (they should flash).
            exposure_ev100: exposure_for(sun_illuminance + moon_illuminance, d.ambient_brightness),
        }
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::new(DayTable::default(), WeatherTable::default(), "temperate", 0)
    }
}
