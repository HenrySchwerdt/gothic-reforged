//! The synthetic `WeatherData` runs through the authoritative `gothic-environment` pipeline.
//! Synthetic placeholder tables only; no game data.
use gothic_env_synth::g3ws::{
    default_weather_data, environment_name, stationary, synthetic_weather_data,
};
use gothic_env_synth::{Clock, DayTable, WeatherKind, WeatherTable};
use gothic_environment::{WeatherAdmin, packet, precip::PrecipParams};

#[test]
fn one_state_per_kind_and_hourly_keys() {
    let d = default_weather_data();
    assert_eq!(d.states.len(), WeatherKind::ALL.len());
    for (s, k) in d.states.iter().zip(WeatherKind::ALL) {
        assert_eq!(s.name, k.state_name());
        assert_eq!(WeatherKind::from_state_name(&s.name), Some(k));
        let keys = &s.scales.sun_intensity.keys;
        assert_eq!(keys.len(), 25);
        assert_eq!(keys[0].0, 0.0);
        assert_eq!(keys[24].0, 1.0);
        assert!(keys.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(s.scales.fog_start.at(0.5) < s.scales.fog_end.at(0.5));
    }
    assert!(d.state("Sunny").is_some() && d.state("Rainy").is_some());
}

#[test]
fn climates_become_named_environments_with_probabilities() {
    let d = default_weather_data();
    for name in ["Myrtana", "Nordmar", "Varant"] {
        let e = d.environment(name).unwrap_or_else(|| panic!("{name}"));
        let sum: f32 = e.entries.iter().map(|x| x.probability).sum();
        assert!((sum - 1.0).abs() < 1e-3, "{name}: {sum}");
        assert!(
            e.entries
                .iter()
                .all(|x| x.min_duration > 0.0 && x.min_duration <= x.max_duration)
        );
    }
    let p = |env: &str, st: &str| {
        d.environment(env)
            .unwrap()
            .entries
            .iter()
            .find(|e| e.state == st)
            .unwrap()
            .probability
    };
    assert!(p("Myrtana", "Sunny") > p("Myrtana", "Rainy"));
    assert_eq!(p("Myrtana", "Snow"), 0.0);
    assert!(p("Nordmar", "Snow") > 0.0);
    assert!(p("Varant", "Sandstorm") > 0.0 && p("Varant", "Rainy") == 0.0);
    assert_eq!(environment_name("elsewhere"), "elsewhere");
}

#[test]
fn hold_seconds_convert_to_days() {
    let t = WeatherTable::default();
    let d = synthetic_weather_data(&DayTable::default(), &t, 1.0 / 120.0);
    let e = &d.environments[0].entries[0];
    let days = |s: f32| s / 120.0 / 24.0;
    assert!((e.min_duration - days(t.hold_seconds.0)).abs() < 1e-6);
    assert!((e.max_duration - days(t.hold_seconds.1)).abs() < 1e-6);
}

#[test]
fn stationary_share_of_a_two_state_chain() {
    use WeatherKind::*;
    let mut rows = std::collections::BTreeMap::new();
    // Clear -> Rain with 1/4, Rain -> Clear with 1/2: long-run Clear 2/3, Rain 1/3.
    rows.insert(Clear, vec![(Clear, 3.0), (Rain, 1.0)]);
    rows.insert(Rain, vec![(Clear, 1.0), (Rain, 1.0)]);
    let p = stationary(&rows);
    // Kinds without a row keep their initial mass (absorbing), so look at the ratio.
    assert!((p[0] / p[1] - 2.0).abs() < 1e-3, "{p:?}");
}

#[test]
fn admin_runs_on_synthetic_data_day_night_and_rain() {
    let mut admin = WeatherAdmin::new(default_weather_data(), 1);
    assert_eq!(admin.current_state("Myrtana"), Some("Sunny"));
    let noon = packet(&admin.evaluate(0.5, |_, _| None), 0.5, 0.9, 100_000.0);
    let night = packet(&admin.evaluate(0.02, |_, _| None), 0.02, 0.9, 100_000.0);
    let lum = |c: [f32; 3]| c[0] + c[1] + c[2];
    assert!(lum(noon.sun_color) > 2.0 * lum(night.sun_color));
    assert!(lum(noon.sky_color) > lum(night.sky_color));
    assert!(night.star_visibility > noon.star_visibility);
    let base = PrecipParams::default();
    assert_eq!(
        admin
            .evaluate_precipitation(&base, |_, _| None)
            .particles_per_second,
        0.0
    );

    admin.force_state("Myrtana", "Rainy");
    let rain = admin.evaluate_precipitation(&base, |_, _| None);
    assert!(rain.particles_per_second > 1000.0);
    let rainy_noon = packet(&admin.evaluate(0.5, |_, _| None), 0.5, 0.9, 100_000.0);
    assert!(lum(rainy_noon.sun_color) < lum(noon.sun_color));
    assert!(rainy_noon.fog_end < noon.fog_end);

    // Weather keeps rolling when the admin is stepped (no panics, valid states).
    let mut admin = WeatherAdmin::new(default_weather_data(), 9);
    for _ in 0..2_000 {
        admin.step(0.01);
        assert!(admin.current_state("Myrtana").is_some());
    }
}

#[test]
fn deterministic() {
    let a = default_weather_data();
    let b = synthetic_weather_data(
        &DayTable::default(),
        &WeatherTable::default(),
        Clock::new(0.0).hours_per_second,
    );
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
}
