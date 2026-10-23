# Environment: day/night and weather (`crates/env-synth`)

> Naming: this is the game-data-free reference environment (`gothic-env-synth`, cloud REN-02).
> The Gothic 3 parity work on the real weather data (`WeatherStates.g3ws`, weather zones, the
> world's precipitation / skydome / wind entities) lives in `crates/environment`
> (`gothic-environment`) and is **authoritative** in the game. See
> [Relation to `gothic-environment`](#relation-to-gothic-environment-issue-38) for the decision.

`gothic-env-synth` computes everything about the time of day and the weather: sun and
moon, sky gradient, clouds, fog, rain, snow, sand, lightning, and surface wetness and snow
cover. The model is Bevy-free and deterministic. The `render` feature adds a Bevy plugin that
applies it to the scene.

## Model

| Piece | What it does |
| --- | --- |
| `Curve<T>` | Cyclic keyframes over 0..24 h, `(hour, value)`; `Linear`, `Smooth` (smoothstep) or `Step`. Wraps from the last key to the first key of the next day. |
| `DayTable` | Curves for the sun colour and illuminance (lux), the moon, ambient colour and brightness, sky zenith and horizon, sun glow, cloud colour, fog colour, fog start and end (m), and stars. Also holds the `SkyPath`. |
| `SkyPath` | Sun and moon directions from the hour, latitude and declinations (Y up, -Z north, `north_yaw_deg` rotates the sky). |
| `WeatherTable` | One `WeatherProfile` per kind (`Clear`, `Rain`, `Storm`, `Snow`, `Sandstorm`), climates (region -> weighted next kinds), transition and hold times, and wetting and drying rates. |
| `WeatherMachine` | Seeded state machine. It holds a kind for a random time, picks the next kind from the climate's weights, and blends from the current look (no jumps, even mid-transition). It also produces lightning flashes. `force(kind)` and `automatic = false` give scripted control. |
| `Surface` | `wetness` and `snow_cover` (0..1). Rain wets, sun and time dry; snow covers, no snow thaws. |
| `Environment` | Clock (game hours, `hours_per_second`) + all of the above. `update(dt)`, then `frame()` gives an `EnvFrame`. |

`EnvFrame` contains:
- the hour and the weather, with its blend;
- sun and moon direction, colour and illuminance (faded at the horizon and dimmed by weather);
- ambient colour and brightness (with lightning);
- the sky gradient, sun glow and stars;
- clouds (cover, darkness, colour, drift);
- fog (colour, linear start and end);
- precipitation (rain, snow and sand strength, wind);
- lightning, wetness and snow cover.

The same tables, seed and `update` steps always give the same frames.

## Data

`crates/env-synth/data/day.ron` and `data/weather.ron` are **synthetic placeholders**, not
the original values. To plug in the original tables, write them in the same RON shape and
load them with `DayTable::from_ron` / `WeatherTable::from_ron`, or pass them to the plugin.
Colours are linear RGB tuples. Curves are lists of `(hour, value)` with increasing hours in
`[0, 24)`.

```ron
sun_illuminance: [(5.0, 0.0), (6.5, 4000.0), (12.0, 32000.0), (18.5, 4000.0), (20.0, 0.0)],
sky_horizon: [(0.0, (0.01, 0.012, 0.03)), (7.0, (0.75, 0.6, 0.5)), /* ... */],
```

Climates map a region to its weather mix. The defaults are `temperate` (rain and storms),
`cold` (snow) and `desert` (sandstorms). Switch region with `weather.set_climate("desert")`.

## Bevy plugin (`--features render`)

```rust
app.add_plugins(EnvironmentPlugin { climate: "temperate".into(), seed, ..default() });
commands.spawn((Camera3d::default(), EnvironmentCamera));       // gets DistanceFog + sky dome
commands.spawn((Sun, DirectionalLight { shadow_maps_enabled: true, ..default() }));
commands.spawn((Moon, DirectionalLight::default()));
```

The plugin:
- ticks the environment;
- writes `CurrentEnv(EnvFrame)`;
- turns the `Sun` and `Moon` lights and sets their colour and illuminance;
- sets `GlobalAmbientLight`, `ClearColor`, and `DistanceFog` on every `EnvironmentCamera`;
- draws two shaders, both validated with naga in the tests:
  - a **sky dome**: an unlit sphere following the camera, with gradient, sun disc and glow, moon, stars, and fbm clouds drifting with the wind (`shaders/sky.wgsl`);
  - a **precipitation overlay**: a full-screen UI material with procedural rain streaks, snow flakes and blowing sand in three parallax layers, slanted by wind, plus lightning flashes. It uses no particles and costs one full-screen quad (`shaders/precipitation.wgsl`).

`EnvironmentState` exposes the `Environment`, `paused` and `time_scale`, so you can set the
clock, force weather or switch climate.

**Wetness and snow for materials:** read `CurrentEnv.0.wetness` / `.snow_cover`. The greybox
demo darkens and smooths the ground when wet and whitens it with snow. Real materials can
take the same parameters into a shader.

## Demos

* `cargo run -p gothic-env-synth --example greybox --features demo`: a grey test scene
  cycling a day in two minutes, with the weather changing every 15 s. Keys: Space (pause),
  W (next weather), A (automatic weather), Up and Down (time speed).
  `-- --screenshots DIR` captures clear noon, dusk, rain, storm, snow and sandstorm, then exits.
  This also works headless with `xvfb-run` and a software Vulkan driver.
* `cargo run -p gothic-env-synth --example timeline`: one day as JSON lines, no window.

## Relation to `gothic-environment` (issue #38)

### What each crate models

| | `gothic-environment` (authoritative) | `gothic-env-synth` (reference / fallback) |
| --- | --- | --- |
| Source of numbers | Original `WeatherStates.g3ws` (16 keyframed scales per state, environments with state probabilities and durations), `eCWeatherZone_PS`, `eCPrecipitation_PS`, `eCSkydome_PS`, `eCSpeedTreeWind_PS` entities | Synthetic RON tables (`data/day.ron`, `data/weather.ron`), explicitly not original values |
| Rules | Ports of `eCWeatherAdmin::Process`, `eCWeatherEnvironment::Process`, zone blend / overwrites, SH hemisphere, fog clip (addresses in the crate docs) | Own design: Markov weather machine, smoothstep blends, lightning, wetness / snow cover, exposure |
| Time | Day fraction 0..1, sun arc 0.25..0.9167 | Game hours 0..24, latitude / declination sky path |
| Weather | Independent weighted state picks per environment, 1 h cross-fade, durations in days | Climate transition table, hold seconds, blend from the current look |
| Output | `EnvRecord` / `EnvPacket` (sun colour x intensity, SH, sky / haze, clouds, fog cm), `PrecipParams`, wind, skydome | `EnvFrame` (lux, cd/m^2, fog m, precipitation strengths, wetness) |
| Bevy | None (the game's `walk/env.rs`, `walk/precip.rs`, `walk/windsys.rs`, `gmat.rs` and `gothic-fx` consume it) | Optional `render` plugin with its own sky dome and full-screen precipitation overlay |

Overlap: time of day, sun / moon placement, sky / fog / cloud colours, a seeded weather picker
and precipitation. Every overlapping piece exists in `gothic-environment` as a port of the
original, so nothing of the overlap was folded into it. The env-synth extras (lightning,
surface wetness / snow, exposure, sand) have no counterpart in the original data or rules;
adding them to `gothic-environment` would mix invented behaviour into the faithful crate, so
they stay in env-synth.

### Decision

* `gothic-environment` stays authoritative and is **unchanged**. With the original data present
  and `GR_ENV_SYNTH` unset, the game behaves exactly as before.
* `gothic-env-synth` is kept as the data-free reference (its demos, tests, shaders and plugin are
  untouched) and becomes an **explicit fallback data source**: `g3ws::synthetic_weather_data`
  turns its placeholder tables into a synthetic `WeatherData` (the decoded `.g3ws` shape) that
  runs through the same `WeatherAdmin` / `packet` / `evaluate_precipitation` pipeline. One
  render path, no second environment plugin in the game; the env-synth Bevy plugin is not wired
  into `apps/game`.
* Mapping (synthetic, in `crates/env-synth/src/g3ws.rs`): one state per `WeatherKind`
  (`Sunny`, `Rainy`, `Storm`, `Snow`, `Sandstorm`), every scale sampled hourly (25 keys) with the
  weather profile applied as in `Environment::frame`; lux / cd/m^2 / metres are scaled to record
  intensities and cm; climates become environments (`temperate` -> `Myrtana`, `cold` ->
  `Nordmar`, `desert` -> `Varant`) whose state probabilities are the long-run shares of the
  transition table; hold seconds become days with the clock rate (1 game hour = 2 real minutes);
  rain and snow get `eCPrecipitation_PS` parameters, sand produces no particles. Lightning,
  wetness and snow cover are dropped (no slot in the record).

### Game wiring (`apps/game/src/walk/env_synth.rs`)

`OrigEnv::install` passes the loaded data through `env_synth::weather_data`:

| `GR_ENV_SYNTH` | Original `.g3ws` present | Original missing |
| --- | --- | --- |
| unset / `0` / `off` (default) | original (unchanged) | built-in light set (unchanged) |
| `auto` / `fallback` | original | synthetic placeholder states |
| `1` / `force` / anything else | synthetic placeholder states | synthetic placeholder states |

A synthetic choice is logged (`environment: GR_ENV_SYNTH: using the SYNTHETIC ...`). Weather
zones, static lights, wind and the world's precipitation / skydome entities still come from the
world; only the weather states are replaced. `--weather Rainy`, `--hour`, `--day-speed` work with
both.

### Verification status

* Tested (synthetic data only, no game assets): `cargo +1.99 test -p gothic-env-synth`
  (`tests/g3ws_fallback.rs`: state / key layout, environments and probabilities, duration
  conversion, stationary shares, `WeatherAdmin` day / night / rain on the synthetic record) and
  `cargo +1.99 test -p gothic-reforged --features viewer -- env_synth` (mode parsing and
  selection; `Off` never touches the synthetic builder).
* **Not verified in game**: how the synthetic states look in the walk viewer (brightness of the
  scaled lux / ambient mapping, fog distances, rain density), and that the default path with
  real data is visually identical (the code path is unchanged; only `install` now calls the
  selector, which returns the original data untouched when `GR_ENV_SYNTH` is unset).
* Check with real data (Windows; `--walk` defaults to the installation's `Data` directory on D:,
  PowerShell syntax):

  ```
  cargo run --release -p gothic-reforged --features viewer -- --walk --hour 12
  $env:GR_ENV_SYNTH = "1"; cargo run --release -p gothic-reforged --features viewer -- --walk --hour 12 --weather Rainy
  Remove-Item Env:GR_ENV_SYNTH
  ```

  The first run must look as before (log: `environment: N weather states, ...`, no
  `GR_ENV_SYNTH` line); the second logs the synthetic line and shows placeholder colours and
  rain. `GR_ENV_SHOT=<file.png>:<seconds>` captures both for comparison.

### Not wired / remaining

* The env-synth Bevy plugin, sky / precipitation WGSL shaders, lightning, wetness and snow cover
  are not used by the game; issue #13 (game integration) stays open for any of them.
* Region -> climate switching and script-driven time are not connected for either crate.
