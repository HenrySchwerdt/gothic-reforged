//! The day table: every time-of-day colour and curve, data-driven (RON).
use crate::curve::{Curve, Interp, Rgb};
use crate::sun::SkyPath;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Time-of-day keyframes. `data/day.ron` holds synthetic placeholder values; the maintainer can
/// swap in the original tables later without code changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DayTable {
    #[serde(default)]
    pub interpolation: Interp,
    #[serde(default)]
    pub sky_path: SkyPath,
    /// Directional sun light colour and illuminance (lux).
    pub sun_color: Curve<Rgb>,
    pub sun_illuminance: Curve<f32>,
    pub moon_color: Curve<Rgb>,
    pub moon_illuminance: Curve<f32>,
    /// Ambient light colour and brightness (cd/m^2, Bevy's `GlobalAmbientLight`).
    pub ambient_color: Curve<Rgb>,
    pub ambient_brightness: Curve<f32>,
    /// Sky gradient.
    pub sky_zenith: Curve<Rgb>,
    pub sky_horizon: Curve<Rgb>,
    /// Glow around the sun on the sky.
    pub sun_glow: Curve<Rgb>,
    pub cloud_color: Curve<Rgb>,
    /// Fog colour and linear start/end distances (metres).
    pub fog_color: Curve<Rgb>,
    pub fog_start: Curve<f32>,
    pub fog_end: Curve<f32>,
    /// Star visibility 0..1.
    pub stars: Curve<f32>,
}

/// The built-in synthetic table.
pub const DEFAULT_DAY: &str = include_str!("../data/day.ron");

impl DayTable {
    pub fn from_ron(text: &str) -> Result<Self> {
        ron::from_str(text).context("day table")
    }

    pub fn to_ron(&self) -> Result<String> {
        Ok(ron::ser::to_string_pretty(
            self,
            ron::ser::PrettyConfig::default(),
        )?)
    }

    /// Every curve sampled at `hour`.
    pub fn sample(&self, hour: f64) -> DaySample {
        let i = self.interpolation;
        DaySample {
            sun_color: self.sun_color.sample(hour, i),
            sun_illuminance: self.sun_illuminance.sample(hour, i),
            moon_color: self.moon_color.sample(hour, i),
            moon_illuminance: self.moon_illuminance.sample(hour, i),
            ambient_color: self.ambient_color.sample(hour, i),
            ambient_brightness: self.ambient_brightness.sample(hour, i),
            sky_zenith: self.sky_zenith.sample(hour, i),
            sky_horizon: self.sky_horizon.sample(hour, i),
            sun_glow: self.sun_glow.sample(hour, i),
            cloud_color: self.cloud_color.sample(hour, i),
            fog_color: self.fog_color.sample(hour, i),
            fog_start: self.fog_start.sample(hour, i),
            fog_end: self.fog_end.sample(hour, i),
            stars: self.stars.sample(hour, i),
        }
    }
}

impl Default for DayTable {
    fn default() -> Self {
        Self::from_ron(DEFAULT_DAY).expect("built-in day table")
    }
}

/// [`DayTable`] at one hour.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DaySample {
    pub sun_color: Rgb,
    pub sun_illuminance: f32,
    pub moon_color: Rgb,
    pub moon_illuminance: f32,
    pub ambient_color: Rgb,
    pub ambient_brightness: f32,
    pub sky_zenith: Rgb,
    pub sky_horizon: Rgb,
    pub sun_glow: Rgb,
    pub cloud_color: Rgb,
    pub fog_color: Rgb,
    pub fog_start: f32,
    pub fog_end: f32,
    pub stars: f32,
}
