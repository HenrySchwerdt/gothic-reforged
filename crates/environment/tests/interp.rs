//! Deterministic tests of the keyframe interpolation, sky path, weather machine and the frame.
use gothic_environment::{
    Curve, DayTable, Environment, Interp, Rgb, WeatherKind, WeatherMachine, WeatherTable,
};

fn curve() -> Curve<f32> {
    Curve::new(vec![(6.0, 0.0), (12.0, 10.0), (18.0, 4.0)]).unwrap()
}

#[test]
fn keys_are_hit_exactly_and_midpoints_interpolate() {
    let c = curve();
    for (h, v) in [(6.0, 0.0), (12.0, 10.0), (18.0, 4.0)] {
        for i in [Interp::Linear, Interp::Smooth, Interp::Step] {
            assert_eq!(c.sample(h, i), v, "hour {h} {i:?}");
        }
    }
    assert_eq!(c.sample(9.0, Interp::Linear), 5.0);
    assert_eq!(c.sample(9.0, Interp::Smooth), 5.0); // smoothstep(0.5) = 0.5
    assert_eq!(c.sample(7.5, Interp::Linear), 2.5);
    assert_eq!(c.sample(7.5, Interp::Smooth), 10.0 * 0.15625);
    assert_eq!(c.sample(11.9, Interp::Step), 0.0);
    assert_eq!(c.sample(15.0, Interp::Linear), 7.0);
}

#[test]
fn wraps_across_midnight() {
    let c = curve();
    // Segment 18 -> 6 (+24) is 12 hours long: 4 at 18, 0 at 6.
    assert_eq!(c.sample(0.0, Interp::Linear), 2.0);
    assert_eq!(c.sample(24.0, Interp::Linear), 2.0);
    assert_eq!(c.sample(-24.0, Interp::Linear), 2.0);
    assert_eq!(c.sample(21.0, Interp::Linear), 3.0);
    assert_eq!(c.sample(3.0, Interp::Linear), 1.0);
    assert_eq!(c.sample(48.0 + 9.0, Interp::Linear), 5.0);
    assert_eq!(Curve::constant(7.0f32).sample(13.3, Interp::Smooth), 7.0);
}

#[test]
fn sampling_is_continuous_over_the_day() {
    let day = DayTable::default();
    let mut prev = day.sample(0.0);
    let steps = 24 * 600;
    for i in 1..=steps {
        let h = f64::from(i) * 24.0 / f64::from(steps);
        let s = day.sample(h);
        // No jumps: 1/600 h steps change nothing by more than a small amount.
        assert!(
            (s.sun_illuminance - prev.sun_illuminance).abs() < 100.0,
            "jump at {h}"
        );
        assert!((s.fog_end - prev.fog_end).abs() < 5.0, "jump at {h}");
        for (a, b) in [
            (s.sky_zenith, prev.sky_zenith),
            (s.sky_horizon, prev.sky_horizon),
        ] {
            assert!(
                (a.0 - b.0).abs() + (a.1 - b.1).abs() + (a.2 - b.2).abs() < 0.02,
                "jump at {h}"
            );
        }
        prev = s;
    }
}

#[test]
fn color_curves_interpolate_per_channel() {
    let c = Curve::new(vec![(0.0, Rgb(0.0, 1.0, 0.5)), (12.0, Rgb(1.0, 0.0, 0.5))]).unwrap();
    assert_eq!(c.sample(6.0, Interp::Linear), Rgb(0.5, 0.5, 0.5));
    assert_eq!(c.sample(18.0, Interp::Linear), Rgb(0.5, 0.5, 0.5));
}

#[test]
fn tables_round_trip_through_ron() {
    let day = DayTable::default();
    assert_eq!(DayTable::from_ron(&day.to_ron().unwrap()).unwrap(), day);
    let w = WeatherTable::default();
    assert_eq!(WeatherTable::from_ron(&w.to_ron().unwrap()).unwrap(), w);
    let mut broken = w.clone();
    broken.profiles.remove(&WeatherKind::Snow);
    assert!(broken.validate().is_err());
}

#[test]
fn weather_is_deterministic_per_seed() {
    let table = WeatherTable::default();
    let run = |seed| {
        let mut m = WeatherMachine::new(&table, "temperate", WeatherKind::Clear, seed);
        let mut log = Vec::new();
        for i in 0..(6 * 3600) {
            m.update(&table, 1.0);
            if i % 60 == 0 {
                log.push((m.target(), m.blend().to_bits(), m.lightning().to_bits()));
            }
        }
        log
    };
    assert_eq!(run(1), run(1));
    assert_ne!(run(1), run(2));
    let kinds: std::collections::BTreeSet<_> = run(3).into_iter().map(|(k, _, _)| k).collect();
    assert!(kinds.contains(&WeatherKind::Rain), "{kinds:?}");
    assert!(
        !kinds.contains(&WeatherKind::Sandstorm),
        "temperate climate has no sandstorms"
    );
}

#[test]
fn transitions_are_continuous_and_finish_in_time() {
    let table = WeatherTable::default();
    let mut m = WeatherMachine::new(&table, "temperate", WeatherKind::Clear, 0);
    m.automatic = false;
    m.force(&table, WeatherKind::Storm);
    let mut prev = m.current(&table);
    let dt = 0.1;
    let steps = (table.transition_seconds / dt).ceil() as usize + 1;
    for _ in 0..steps {
        m.update(&table, dt);
        let c = m.current(&table);
        assert!((c.rain - prev.rain).abs() < 0.01);
        assert!((c.cloud_cover - prev.cloud_cover).abs() < 0.01);
        prev = c;
    }
    assert_eq!(m.blend(), 1.0);
    assert_eq!(m.current(&table), table.profile(WeatherKind::Storm));
    // Forcing mid-transition starts from the current look (no jump).
    m.force(&table, WeatherKind::Snow);
    for _ in 0..50 {
        m.update(&table, dt);
    }
    let mid = m.current(&table);
    m.force(&table, WeatherKind::Sandstorm);
    assert_eq!(m.current(&table), mid);
}

#[test]
fn climate_changes_the_weather_mix() {
    let table = WeatherTable::default();
    let mut m = WeatherMachine::new(&table, "desert", WeatherKind::Clear, 9);
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..(24 * 3600) {
        m.update(&table, 1.0);
        seen.insert(m.target());
    }
    assert!(seen.contains(&WeatherKind::Sandstorm));
    assert!(!seen.contains(&WeatherKind::Rain) && !seen.contains(&WeatherKind::Snow));
}

#[test]
fn frame_combines_day_and_weather() {
    let mut env = Environment::default();
    env.weather.automatic = false;
    env.clock.hours = 12.0;
    env.clock.hours_per_second = 0.0;
    let clear = env.frame();
    assert!(clear.sun_illuminance > 20000.0);
    assert!(clear.sun_elevation_deg > 60.0);
    assert_eq!(clear.precipitation.rain, 0.0);

    env.force_weather(WeatherKind::Rain);
    for _ in 0..(10 * 60) {
        env.update(1.0);
    }
    let rain = env.frame();
    assert_eq!(rain.precipitation.rain, 1.0);
    assert!(rain.sun_illuminance < clear.sun_illuminance * 0.5);
    assert!(rain.fog.end < clear.fog.end);
    assert!(rain.wetness > 0.9, "{}", rain.wetness);

    env.force_weather(WeatherKind::Clear);
    for _ in 0..(10 * 60) {
        env.update(1.0);
    }
    assert!(env.frame().wetness < rain.wetness, "dries without rain");

    env.clock.hours = 0.0;
    let night = env.frame();
    assert_eq!(night.sun_illuminance, 0.0);
    assert!(night.exposure_ev100 < clear.exposure_ev100 - 4.0);
    assert!(
        (13.0..=14.0).contains(&clear.exposure_ev100),
        "{}",
        clear.exposure_ev100
    );
    assert!(night.moon_illuminance > 0.0 && night.stars > 0.5);
}

#[test]
fn identical_runs_give_identical_frames() {
    let run = || {
        let mut env =
            Environment::new(DayTable::default(), WeatherTable::default(), "temperate", 5);
        env.clock.hours_per_second = 0.01;
        let mut out = Vec::new();
        for i in 0..20_000 {
            env.update(0.5);
            if i % 500 == 0 {
                out.push(serde_json::to_string(&env.frame()).unwrap());
            }
        }
        out
    };
    assert_eq!(run(), run());
}

#[test]
fn shaders_parse_and_validate() {
    for (name, src) in [
        ("sky", include_str!("../src/shaders/sky.wgsl")),
        (
            "precipitation",
            include_str!("../src/shaders/precipitation.wgsl"),
        ),
    ] {
        let module = naga::front::wgsl::parse_str(src)
            .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(src)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert!(
            module.entry_points.iter().any(|e| e.name == "fragment"),
            "{name}"
        );
    }
}
