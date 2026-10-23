//! Opt-in data-free fallback for the day-time environment (`GR_ENV_SYNTH`).
//!
//! The original `WeatherStates.g3ws` stays authoritative: `env.rs` runs `gothic-environment`'s
//! `WeatherAdmin` on it. This module only decides which `WeatherData` that admin gets:
//!
//! * `GR_ENV_SYNTH` unset or `0`: the loaded file, unchanged behaviour (no file -> the built-in
//!   light set, as before);
//! * `GR_ENV_SYNTH=auto`: the loaded file, or the synthetic placeholder record of
//!   `gothic-env-synth` when the file is missing;
//! * `GR_ENV_SYNTH=1` (or `force`): always the synthetic record, even with the original data.
//!
//! The synthetic states are named `Sunny`, `Rainy`, `Storm`, `Snow`, `Sandstorm`, so
//! `--weather Rainy` works with both.
use gothic_formats::weather::WeatherData;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SynthMode {
    Off,
    Auto,
    Force,
}

impl SynthMode {
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            None | Some("" | "0" | "off" | "false" | "no") => Self::Off,
            Some("auto" | "fallback") => Self::Auto,
            Some(_) => Self::Force,
        }
    }

    pub fn from_env() -> Self {
        Self::parse(std::env::var("GR_ENV_SYNTH").ok().as_deref())
    }
}

/// Where the weather data came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Original,
    Synthetic,
    Missing,
}

/// Pick the weather data for the admin. `synth` builds the synthetic record (only called when it
/// is used).
pub fn select(
    real: Option<WeatherData>,
    mode: SynthMode,
    synth: impl FnOnce() -> WeatherData,
) -> (Option<WeatherData>, Source) {
    match (mode, real) {
        (SynthMode::Force, _) | (SynthMode::Auto, None) => (Some(synth()), Source::Synthetic),
        (_, Some(d)) => (Some(d), Source::Original),
        (SynthMode::Off, None) => (None, Source::Missing),
    }
}

/// [`select`] with `GR_ENV_SYNTH` and the built-in placeholder tables; logs a synthetic choice.
pub fn weather_data(real: Option<WeatherData>) -> Option<WeatherData> {
    let (data, source) = select(
        real,
        SynthMode::from_env(),
        gothic_env_synth::g3ws::default_weather_data,
    );
    if source == Source::Synthetic {
        eprintln!(
            "environment: GR_ENV_SYNTH: using the SYNTHETIC placeholder weather states of \
             gothic-env-synth (not original data)"
        );
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use gothic_formats::weather::WeatherState;

    fn real() -> WeatherData {
        WeatherData {
            states: vec![WeatherState {
                name: "Original".into(),
                ..WeatherState::default()
            }],
            environments: Vec::new(),
        }
    }

    fn synth() -> WeatherData {
        gothic_env_synth::g3ws::default_weather_data()
    }

    #[test]
    fn env_synth_mode_parsing() {
        assert_eq!(SynthMode::parse(None), SynthMode::Off);
        for v in ["", "0", "off", "False", "no"] {
            assert_eq!(SynthMode::parse(Some(v)), SynthMode::Off, "{v}");
        }
        for v in ["auto", "AUTO", "fallback"] {
            assert_eq!(SynthMode::parse(Some(v)), SynthMode::Auto, "{v}");
        }
        for v in ["1", "force", "yes"] {
            assert_eq!(SynthMode::parse(Some(v)), SynthMode::Force, "{v}");
        }
    }

    #[test]
    fn env_synth_off_keeps_original_behaviour() {
        let (d, s) = select(Some(real()), SynthMode::Off, || unreachable!());
        assert_eq!(
            (d.unwrap().states[0].name.as_str(), s),
            ("Original", Source::Original)
        );
        let (d, s) = select(None, SynthMode::Off, || unreachable!());
        assert!(d.is_none());
        assert_eq!(s, Source::Missing);
    }

    #[test]
    fn env_synth_auto_only_fills_missing_data() {
        let (d, s) = select(Some(real()), SynthMode::Auto, || unreachable!());
        assert_eq!(
            (d.unwrap().states[0].name.as_str(), s),
            ("Original", Source::Original)
        );
        let (d, s) = select(None, SynthMode::Auto, synth);
        assert_eq!(s, Source::Synthetic);
        assert!(d.unwrap().environment("Myrtana").is_some());
    }

    #[test]
    fn env_synth_force_replaces_original() {
        let (d, s) = select(Some(real()), SynthMode::Force, synth);
        assert_eq!(s, Source::Synthetic);
        let d = d.unwrap();
        assert!(d.state("Original").is_none() && d.state("Rainy").is_some());
    }

    #[test]
    fn env_synth_data_drives_the_admin() {
        let mut admin = gothic_environment::WeatherAdmin::new(synth(), 0x6a09_e667);
        admin.force_state("Myrtana", "Rainy");
        assert_eq!(admin.current_state("Myrtana"), Some("Rainy"));
        let p = admin.evaluate_precipitation(
            &gothic_environment::precip::PrecipParams::default(),
            |_, _| None,
        );
        assert!(p.particles_per_second > 0.0);
    }
}
