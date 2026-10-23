//! Day/night cycle and weather for Gothic Reforged.
//!
//! The model is Bevy-free and deterministic: a [`DayTable`] of keyframe curves (sky gradient,
//! sun/moon light, ambient, fog, clouds, stars), a [`SkyPath`] for sun and moon directions, a
//! [`WeatherMachine`] blending between [`WeatherKind`]s per climate, and a [`Surface`] with
//! wetness and snow cover for materials. [`Environment::frame`] folds them into one
//! [`EnvFrame`] of render parameters. All colours and curves live in RON tables
//! (`data/day.ron`, `data/weather.ron`, synthetic placeholders) so the original values can be
//! plugged in later.
//!
//! With the `render` feature, [`plugin::EnvironmentPlugin`] drives Bevy lights, ambient light,
//! clear colour and distance fog, draws a sky dome and a particle-free full-screen
//! rain/snow/sand overlay.
//!
//! Relation to `gothic-environment`: that crate ports the original weather rules and consumes the
//! original `WeatherStates.g3ws` and is authoritative in the game. This crate is the data-free
//! reference; [`g3ws::synthetic_weather_data`] turns its placeholder tables into a synthetic
//! `WeatherData` so the game can run the authoritative pipeline without the original file
//! (see `docs/ENVIRONMENT.md`).
//!
//! ```
//! use gothic_env_synth::{Environment, WeatherKind};
//! let mut env = Environment::default();
//! env.clock.hours = 12.0;
//! env.force_weather(WeatherKind::Rain);
//! for _ in 0..60 * 30 {
//!     env.update(1.0 / 30.0);
//! }
//! let f = env.frame();
//! assert!(f.precipitation.rain > 0.9 && f.wetness > 0.0);
//! ```
pub mod curve;
pub mod day;
pub mod env;
pub mod g3ws;
#[cfg(feature = "render")]
pub mod plugin;
pub mod rng;
pub mod sun;
pub mod surface;
pub mod weather;

pub use curve::{Curve, Interp, Lerp, Rgb};
pub use day::{DaySample, DayTable};
pub use env::{Clock, Clouds, EnvFrame, Environment, Fog, Precipitation, exposure_for};
pub use sun::SkyPath;
pub use surface::Surface;
pub use weather::{SurfaceRates, WeatherKind, WeatherMachine, WeatherProfile, WeatherTable};
