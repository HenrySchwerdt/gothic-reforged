//! Data-free fallback for `gothic-environment`: build a **synthetic** [`WeatherData`] (the decoded
//! shape of `WeatherStates.g3ws`) from the placeholder [`DayTable`] and [`WeatherTable`].
//!
//! `gothic-environment` is authoritative: its `WeatherAdmin` ports the original weather rules and
//! consumes the original keyframes. When the installation's `WeatherStates.g3ws` is missing (or a
//! developer forces it, see `GR_ENV_SYNTH` in `docs/ENVIRONMENT.md`) the game can feed this
//! synthetic record into the very same `WeatherAdmin` instead of switching to a second, divergent
//! environment pipeline. Nothing here is original data; every number is a placeholder derived
//! from `data/day.ron` / `data/weather.ron`.
//!
//! Mapping (all of it synthetic and **not verified against the original**):
//!
//! * one weather state per [`WeatherKind`], named like the states of the original file where a
//!   counterpart is known ([`WeatherKind::state_name`]);
//! * every state scale is sampled once per game hour (25 keys, `t = hour / 24`, the last key
//!   closes the day) from the day table with the kind's [`WeatherProfile`] applied the same way
//!   [`crate::Environment::frame`] applies it (overcast greying, sun dimming, fog density / tint);
//!   lightning, wetness and snow cover have no slot in the record and are dropped;
//! * climates become environments (`temperate` -> `Myrtana`, `cold` -> `Nordmar`, `desert` ->
//!   `Varant`, others keep their name). The original picks each state independently with a fixed
//!   probability, so the climate's transition weights are reduced to their long-run share
//!   ([`stationary`]); hold times are converted from real seconds to game days with the clock
//!   rate passed in;
//! * rain and snow get emitter parameters for `eCPrecipitation_PS`; sand has no counterpart in
//!   the original emitter and produces no particles.
use crate::curve::{Lerp, Rgb};
use crate::day::DayTable;
use crate::weather::{WeatherKind, WeatherProfile, WeatherTable};
use gothic_formats::weather::{
    EnvironmentEntry, FloatScale, StateScales, WeatherData, WeatherEnvironment, WeatherState,
};

/// Noon sun illuminance (lux) of the placeholder table that maps to a record intensity of 1.
pub const SUN_LUX_PER_UNIT: f32 = 32_000.0;
/// Moon illuminance (lux) that maps to a record intensity of 1 (night light stays readable).
pub const MOON_LUX_PER_UNIT: f32 = 400.0;
/// Ambient brightness (cd/m^2) that maps to an ambient intensity of 1.
pub const AMBIENT_PER_UNIT: f32 = 4_000.0;
/// Particles per second at full rain / snow strength.
pub const RAIN_RATE: f32 = 3_000.0;
pub const SNOW_RATE: f32 = 1_500.0;

impl WeatherKind {
    /// Name of the synthetic weather state of this kind (`--weather <name>`). `Sunny` and `Rainy`
    /// follow the state names of the original file; the others are placeholders.
    pub fn state_name(self) -> &'static str {
        match self {
            Self::Clear => "Sunny",
            Self::Rain => "Rainy",
            Self::Storm => "Storm",
            Self::Snow => "Snow",
            Self::Sandstorm => "Sandstorm",
        }
    }

    pub fn from_state_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|k| k.state_name().eq_ignore_ascii_case(name))
    }
}

/// Environment name the game uses for a climate of the weather table.
pub fn environment_name(climate: &str) -> &str {
    match climate {
        "temperate" => "Myrtana",
        "cold" => "Nordmar",
        "desert" => "Varant",
        other => other,
    }
}

/// Long-run share of each kind for a climate's transition table (power iteration of the Markov
/// chain; kinds without a row stay where they are). Shares sum to 1.
pub fn stationary(
    climate: &std::collections::BTreeMap<WeatherKind, Vec<(WeatherKind, f32)>>,
) -> [f32; 5] {
    let idx = |k: WeatherKind| WeatherKind::ALL.iter().position(|&x| x == k).unwrap_or(0);
    let mut p = [0.2_f32; 5];
    for _ in 0..256 {
        let mut next = [0.0_f32; 5];
        for (i, &from) in WeatherKind::ALL.iter().enumerate() {
            match climate.get(&from) {
                Some(row) if row.iter().map(|(_, w)| w.max(0.0)).sum::<f32>() > 0.0 => {
                    let total: f32 = row.iter().map(|(_, w)| w.max(0.0)).sum();
                    for &(to, w) in row {
                        next[idx(to)] += p[i] * w.max(0.0) / total;
                    }
                }
                _ => next[i] += p[i],
            }
        }
        p = next;
    }
    let sum: f32 = p.iter().sum();
    p.map(|v| v / sum.max(1e-9))
}

fn rgba(c: Rgb) -> [f32; 4] {
    [c.0, c.1, c.2, 1.0]
}

/// One weather state for `kind`, sampled hourly from the day table.
pub fn state(day: &DayTable, table: &WeatherTable, kind: WeatherKind) -> WeatherState {
    let w = table.profile(kind);
    let mut s = StateScales::default();
    for h in 0..=24u32 {
        let hour = f64::from(h);
        let t = h as f32 / 24.0;
        let d = day.sample(hour);
        let overcast = (w.cloud_cover * w.cloud_darkness).clamp(0.0, 1.0);
        let grey = |c: Rgb| c.desaturate(overcast).scale(1.0 - 0.55 * overcast);
        let sun = d.sun_illuminance / SUN_LUX_PER_UNIT;
        let moon = d.moon_illuminance / MOON_LUX_PER_UNIT * (1.0 - w.cloud_cover * 0.8);
        // The original has one directional light (sun by day, moon by night).
        let (light, intensity) = if sun >= moon {
            (d.sun_color.desaturate(overcast * 0.5), sun)
        } else {
            (d.moon_color, moon)
        };
        let fog_density = w.fog_density.max(0.05);
        let fog = Rgb::lerp(grey(d.fog_color), w.fog_tint, w.fog_tint_amount);
        let ambient = (d.ambient_brightness / AMBIENT_PER_UNIT).max(0.0).sqrt();
        s.sun_diffuse.keys.push((t, rgba(light)));
        s.sun_specular.keys.push((t, rgba(light)));
        s.sun_intensity
            .keys
            .push((t, intensity * (1.0 - w.sun_dim)));
        s.ambient_general
            .keys
            .push((t, rgba(grey(d.ambient_color))));
        s.ambient_back
            .keys
            .push((t, rgba(grey(d.ambient_color).scale(0.25))));
        s.ambient_intensity.keys.push((t, ambient));
        s.fog_color.keys.push((t, rgba(fog)));
        // Day table metres -> record cm.
        s.fog_start
            .keys
            .push((t, d.fog_start * 100.0 / fog_density));
        s.fog_end.keys.push((t, d.fog_end * 100.0 / fog_density));
        s.sky_color.keys.push((t, rgba(grey(d.sky_zenith))));
        s.haze_color.keys.push((t, rgba(grey(d.sky_horizon))));
        s.cloud_density.keys.push((t, w.cloud_cover));
        s.cloud_color
            .keys
            .push((t, rgba(d.cloud_color.scale(1.0 - 0.7 * w.cloud_darkness))));
        s.shadow_density.keys.push((t, 1.0 - 0.6 * overcast));
    }
    s.extra_a = FloatScale::default();
    s.extra_b = FloatScale::default();
    let mut st = WeatherState {
        name: kind.state_name().into(),
        scales: s,
        ..WeatherState::default()
    };
    precipitation(&mut st, &w);
    st
}

/// Emitter look for the dominant precipitation of a profile (rain wins over snow).
fn precipitation(st: &mut WeatherState, w: &WeatherProfile) {
    st.speed_min_max = [800.0, 1000.0];
    st.size = [1.0, 1.0];
    st.splash_size_scale = [1.0, 1.0];
    if w.rain > 0.0 && w.rain >= w.snow {
        st.particles_per_second = (w.rain * RAIN_RATE).round() as i32;
        st.direction_scale = w.wind;
        st.current_texture_tile = 2;
        st.splash_duration = 0.4;
        st.splash_texture_tile = 3;
        st.splash_size_scale = [25.0, 25.0];
    } else if w.snow > 0.0 {
        st.particles_per_second = (w.snow * SNOW_RATE).round() as i32;
        st.speed_min_max = [150.0, 250.0];
        st.size = [2.0, 2.0];
        st.direction_scale = w.wind;
        st.current_texture_tile = 0;
        st.jitter_power = 30.0;
        st.jitter_speed = 1.0;
    }
}

/// The whole synthetic record: one state per kind and one environment per climate.
/// `hours_per_second` is the game clock rate used to turn hold seconds into days.
pub fn synthetic_weather_data(
    day: &DayTable,
    table: &WeatherTable,
    hours_per_second: f64,
) -> WeatherData {
    let states = WeatherKind::ALL
        .into_iter()
        .map(|k| state(day, table, k))
        .collect();
    let to_days = |s: f32| (f64::from(s) * hours_per_second / 24.0) as f32;
    let (lo, hi) = (to_days(table.hold_seconds.0), to_days(table.hold_seconds.1));
    let environments = table
        .climates
        .iter()
        .enumerate()
        .map(|(i, (climate, rows))| {
            let share = stationary(rows);
            WeatherEnvironment {
                name: environment_name(climate).into(),
                entries: WeatherKind::ALL
                    .into_iter()
                    .zip(share)
                    .map(|(k, p)| EnvironmentEntry {
                        state: k.state_name().into(),
                        probability: p,
                        min_duration: lo,
                        max_duration: hi,
                    })
                    .collect(),
                climate: climate.clone(),
                climate_id: i as i32,
            }
        })
        .collect();
    WeatherData {
        states,
        environments,
    }
}

/// [`synthetic_weather_data`] from the built-in placeholder tables and clock rate.
pub fn default_weather_data() -> WeatherData {
    synthetic_weather_data(
        &DayTable::default(),
        &WeatherTable::default(),
        crate::Clock::new(0.0).hours_per_second,
    )
}
