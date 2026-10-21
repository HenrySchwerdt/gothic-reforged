# Environment: day/night and weather (`crates/environment`)

`gothic-environment` computes everything about the time of day and the weather: sun and
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

`crates/environment/data/day.ron` and `data/weather.ron` are **synthetic placeholders**, not
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

* `cargo run -p gothic-environment --example greybox --features demo`: a grey test scene
  cycling a day in two minutes, with the weather changing every 15 s. Keys: Space (pause),
  W (next weather), A (automatic weather), Up and Down (time speed).
  `-- --screenshots DIR` captures clear noon, dusk, rain, storm, snow and sandstorm, then exits.
  This also works headless with `xvfb-run` and a software Vulkan driver.
* `cargo run -p gothic-environment --example timeline`: one day as JSON lines, no window.

## Integration hooks (game)

To wire the environment into the game:
- In `apps/game`, add `EnvironmentPlugin` and tag the play camera `EnvironmentCamera` and the
  existing sun light `Sun`. That replaces the fixed `GlobalAmbientLight` / `DistanceFog`
  setup in `walk.rs`.
- Drive the clock from the core's game time: set `state.env.clock.hours` from the script
  time, and use `hours_per_second = 0` with `paused` so the core stays authoritative.
- Set the climate from the region (Myrtana, Nordmar, Varant).
- Scripted weather uses `force_weather`.
