//! Weather: per-kind profiles, climate transition tables and a deterministic state machine
//! that blends from whatever the weather looks like now to the next kind.
use crate::curve::{Lerp, Rgb};
use crate::rng::Rng;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum WeatherKind {
    Clear,
    Rain,
    Storm,
    Snow,
    Sandstorm,
}

impl WeatherKind {
    pub const ALL: [WeatherKind; 5] = [
        Self::Clear,
        Self::Rain,
        Self::Storm,
        Self::Snow,
        Self::Sandstorm,
    ];
}

/// What a weather kind looks like at full strength. Blends linearly between kinds.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WeatherProfile {
    /// Sky covered by clouds, 0..1.
    pub cloud_cover: f32,
    /// How dark the clouds are, 0..1 (also greys the sky).
    pub cloud_darkness: f32,
    /// Cloud drift speed (m/s at cloud height).
    pub cloud_speed: f32,
    /// Precipitation strengths 0..1 (overlay density, wetting / snow accumulation).
    pub rain: f32,
    pub snow: f32,
    pub sand: f32,
    /// Wind speed (m/s): slants rain, drives sand.
    pub wind: f32,
    /// Fog distance divisor (1 = the day table's fog, 4 = four times closer).
    pub fog_density: f32,
    /// Fog colour tint and how much of it replaces the day's fog colour (0..1).
    pub fog_tint: Rgb,
    pub fog_tint_amount: f32,
    /// Sun light taken away, 0..1.
    pub sun_dim: f32,
    /// Lightning strikes per minute.
    pub lightning_per_minute: f32,
}

impl Lerp for WeatherProfile {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        let f = |x: f32, y: f32| f32::lerp(x, y, t);
        Self {
            cloud_cover: f(a.cloud_cover, b.cloud_cover),
            cloud_darkness: f(a.cloud_darkness, b.cloud_darkness),
            cloud_speed: f(a.cloud_speed, b.cloud_speed),
            rain: f(a.rain, b.rain),
            snow: f(a.snow, b.snow),
            sand: f(a.sand, b.sand),
            wind: f(a.wind, b.wind),
            fog_density: f(a.fog_density, b.fog_density),
            fog_tint: Rgb::lerp(a.fog_tint, b.fog_tint, t),
            fog_tint_amount: f(a.fog_tint_amount, b.fog_tint_amount),
            sun_dim: f(a.sun_dim, b.sun_dim),
            lightning_per_minute: f(a.lightning_per_minute, b.lightning_per_minute),
        }
    }
}

/// Profiles, climates (transition weights) and timing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeatherTable {
    /// Seconds a change of weather takes.
    pub transition_seconds: f32,
    /// A weather holds for a random time in this range (seconds) before the next pick.
    pub hold_seconds: (f32, f32),
    /// Wind / cloud drift direction in degrees (0 = towards -z, 90 = towards +x).
    pub wind_direction_deg: f32,
    pub profiles: BTreeMap<WeatherKind, WeatherProfile>,
    /// Climate (region) -> current kind -> weighted next kinds.
    pub climates: BTreeMap<String, BTreeMap<WeatherKind, Vec<(WeatherKind, f32)>>>,
    /// Surface response.
    pub surface: SurfaceRates,
}

/// How fast surfaces get wet / snowed in and dry / thaw (fractions per second).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceRates {
    /// At full rain.
    pub wetting: f32,
    /// Without rain, scaled by sun (half rate at night).
    pub drying: f32,
    /// At full snow.
    pub snowing: f32,
    /// Without snow.
    pub thawing: f32,
}

pub const DEFAULT_WEATHER: &str = include_str!("../data/weather.ron");

impl WeatherTable {
    pub fn from_ron(text: &str) -> Result<Self> {
        let t: Self = ron::from_str(text).context("weather table")?;
        t.validate()?;
        Ok(t)
    }

    pub fn to_ron(&self) -> Result<String> {
        Ok(ron::ser::to_string_pretty(
            self,
            ron::ser::PrettyConfig::default(),
        )?)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.transition_seconds > 0.0,
            "transition_seconds must be > 0"
        );
        ensure!(
            self.hold_seconds.0 > 0.0 && self.hold_seconds.0 <= self.hold_seconds.1,
            "hold_seconds must be (min, max) with 0 < min <= max"
        );
        for k in WeatherKind::ALL {
            ensure!(self.profiles.contains_key(&k), "no profile for {k:?}");
        }
        for (name, table) in &self.climates {
            for (from, next) in table {
                if next.iter().map(|(_, w)| *w).sum::<f32>() <= 0.0
                    || next.iter().any(|(_, w)| *w < 0.0)
                {
                    bail!(
                        "climate {name}: weights after {from:?} must be >= 0 with a positive sum"
                    );
                }
            }
        }
        Ok(())
    }

    pub fn profile(&self, k: WeatherKind) -> WeatherProfile {
        self.profiles.get(&k).copied().unwrap_or_default()
    }
}

impl Default for WeatherTable {
    fn default() -> Self {
        Self::from_ron(DEFAULT_WEATHER).expect("built-in weather table")
    }
}

/// Deterministic weather state: given the same seed, table and sequence of `update` steps it
/// produces the same weather everywhere.
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherMachine {
    climate: String,
    /// Weather as it looked when the current transition began.
    from: WeatherProfile,
    to: WeatherKind,
    /// Transition progress 0..1.
    blend: f32,
    /// Seconds until the next automatic pick (counts down once the transition is done).
    hold: f32,
    /// When false, the weather only changes through [`WeatherMachine::force`].
    pub automatic: bool,
    lightning: f32,
    rng: Rng,
}

impl WeatherMachine {
    pub fn new(table: &WeatherTable, climate: &str, start: WeatherKind, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let hold = Self::roll_hold(table, &mut rng);
        Self {
            climate: climate.into(),
            from: table.profile(start),
            to: start,
            blend: 1.0,
            hold,
            automatic: true,
            lightning: 0.0,
            rng,
        }
    }

    fn roll_hold(table: &WeatherTable, rng: &mut Rng) -> f32 {
        let (a, b) = table.hold_seconds;
        a + (b - a) * rng.next_f32()
    }

    /// The kind being blended to (or holding).
    pub fn target(&self) -> WeatherKind {
        self.to
    }

    /// Transition progress 0..1 (1 = settled).
    pub fn blend(&self) -> f32 {
        self.blend
    }

    pub fn climate(&self) -> &str {
        &self.climate
    }

    /// Switch climate (region); takes effect at the next pick.
    pub fn set_climate(&mut self, climate: &str) {
        self.climate = climate.into();
    }

    /// Lightning flash brightness 0..1 (decays quickly after a strike).
    pub fn lightning(&self) -> f32 {
        self.lightning
    }

    /// Start blending towards `kind` from the current look (continuous even mid-transition).
    pub fn force(&mut self, table: &WeatherTable, kind: WeatherKind) {
        self.from = self.current(table);
        self.to = kind;
        self.blend = 0.0;
        self.hold = Self::roll_hold(table, &mut self.rng);
    }

    /// The blended profile now.
    pub fn current(&self, table: &WeatherTable) -> WeatherProfile {
        let t = self.blend.clamp(0.0, 1.0);
        let t = t * t * (3.0 - 2.0 * t);
        WeatherProfile::lerp(self.from, table.profile(self.to), t)
    }

    /// Weighted pick of the next kind from the climate table (stays if the climate or kind
    /// has no entry).
    fn pick(&mut self, table: &WeatherTable) -> WeatherKind {
        let Some(next) = table
            .climates
            .get(&self.climate)
            .and_then(|c| c.get(&self.to))
        else {
            return self.to;
        };
        let total: f32 = next.iter().map(|(_, w)| *w).sum();
        let mut r = self.rng.next_f32() * total;
        for (k, w) in next {
            if r < *w {
                return *k;
            }
            r -= w;
        }
        next.last().map_or(self.to, |(k, _)| *k)
    }

    /// Advance by `dt` seconds of game time.
    pub fn update(&mut self, table: &WeatherTable, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        if self.blend < 1.0 {
            self.blend = (self.blend + dt / table.transition_seconds).min(1.0);
        } else if self.automatic {
            self.hold -= dt;
            if self.hold <= 0.0 {
                let next = self.pick(table);
                if next == self.to {
                    self.hold = Self::roll_hold(table, &mut self.rng);
                } else {
                    self.force(table, next);
                }
            }
        }
        self.lightning = (self.lightning - dt * 3.0).max(0.0);
        let rate = self.current(table).lightning_per_minute;
        if rate > 0.0 && self.rng.next_f32() < rate / 60.0 * dt {
            self.lightning = 1.0;
        }
    }
}
