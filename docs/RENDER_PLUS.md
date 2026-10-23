# Render-plus: optional "remaster" render profile (`crates/render-plus`)

Not to be confused with the *remaster asset profile* (`gothic_formats::remaster`,
`GR_REMASTER=<dir>`), which replaces meshes and textures; this crate only adds rendering
effects and works with either asset set.

The faithful renderer stays the default. `gothic-render-plus` adds a profile of modern effects
that a host switches on per camera. `RenderProfile::default()` (and
`Preset::Faithful`) switches nothing on, and a camera without `RenderPlusCamera` is never
touched.

| Effect | Bevy 0.20 feature used | Camera / light changes |
|---|---|---|
| `ssao` | `ScreenSpaceAmbientOcclusion` (GTAO-style, Low/Medium/High/Ultra) | depth + normal prepass, MSAA off |
| `taa` | `TemporalAntiAliasing` + `ContrastAdaptiveSharpening` | depth + motion-vector prepass, MSAA off |
| `volumetric` | `VolumetricFog` on the camera, `VolumetricLight` on the sun, a camera-following `FogVolume` | none |
| `soft_shadows` | PCSS `soft_shadow_size` on the sun (`pcss` feature, Bevy's experimental PCSS), `ContactShadows`, temporal shadow filtering when TAA is on | depth prepass |
| `ssr` | `ScreenSpaceReflections` | **deferred** prepass, MSAA off |

Notes:

* SSR in Bevy 0.20 only reflects on deferred surfaces. Turning it on adds a deferred prepass
  to the camera. Meshes marked `RenderPlusReflective` (water, polished floors) switch their
  `StandardMaterial` to `OpaqueRendererMethod::Deferred` while SSR is on and back to forward
  when it is off. Everything else stays forward, so the faithful path is untouched.
  A deferred material is invisible to a camera without the deferred prepass, so only marked
  surfaces switch. Custom (non-`StandardMaterial`) water shaders would need their own
  deferred variant.
* Volumetric fog only renders inside `FogVolume`s. The demo places one under its slatted
  roof, where the sun is cut into shafts. The plugin keeps one around the camera
  (`volumetric.density`, `volumetric.extent`); set the density to 0 and place your own
  volumes to control it per area.
* Without the `pcss` feature, soft shadows fall back to contact shadows plus temporal filtering.
  The feature is off by default because it is experimental in Bevy and changes the
  `DirectionalLight` struct for the whole build.
* What a camera had before (MSAA, prepasses, shadow filtering, contact shadows on the sun) is
  recorded on first sight and restored when the effects go off again.

## Budget

`profile.budget` (on in the Low/Medium/High presets): a governor watches the real frame time
and, while it stays more than 10 % over `target_ms` (EMA plus raw frame, so hitches from
shader compiles do not count), drops effects in `shed_order` (default SSR, volumetric, soft
shadows, SSAO, TAA). It measures what each drop saved and restores the newest dropped effect
when the frame would stay under 90 % of the target with that cost added back. Changes wait
90 frames to settle. `RenderPlusState` shows the current plan and the last change.

## In the game (`--render-plus`)

`gothic-reforged --walk|--play --render-plus [faithful|low|medium|high|profile.json]` (or
`GR_RENDER_PLUS=...`; no value means `medium`) adds `RenderPlusPlugin` with that profile
(`apps/game/src/walk/plus.rs`). The player camera gets `RenderPlusCamera`, the shadowed sun
`RenderPlusSun`; the budget runs on the real frame time. Without the flag nothing is added.
`--features render-plus-pcss` builds PCSS for soft sun shadows.

* SSR is forced off in the game: terrain, statics and water use the game's own forward
  materials (`GothicMaterial`, `WaterMaterial`), which Bevy 0.20's SSR cannot reflect.
* Not verified here (no game data in CI): how SSAO / TAA look with the game's materials
  (they rely on the prepasses the custom materials produce), interplay with the original
  bloom / HDR post (`GR_HDR`) and distance fog, and the budget on real scenes. Check with
  `--render-plus high` against real data and adjust `budget.shed_order`.

Still open:
* With `gothic-environment` (REN-02): its distance fog stays; volumetric density can follow
  the weather (fog and rain raise `volumetric.density`).
* Water reflections need a deferred-capable water material.

## Demo and measured cost

```
cargo run --release -p gothic-render-plus --example render_plus_demo --features demo -- \
    --bench out/                      # A/B runs, screenshots, out/frame_times.json
cargo run --release -p gothic-render-plus --example render_plus_demo --features demo,pcss
                                      # interactive: 1-5 toggle, 0 faithful, 9 all, B budget
```

The bench renders the greybox courtyard faithful, with each effect alone, and with all
effects together. Each run gets 120 frames of warm-up and 240 measured frames with vsync off,
then a screenshot.

### Measured here (software rasteriser)

The CI container has no GPU. These numbers come from Mesa llvmpipe (Vulkan, 256-bit, CPU) at
960×540, PCSS on, `--frames 120`. They show the **relative** cost on a CPU rasteriser, which favours
compute-light effects and penalises texture-heavy ones. They are **not** the per-effect cost on
a mid-range GPU that the issue asks for. That table still has to be filled in by running the
bench on real hardware (one command, see above; the JSON records the adapter name).

| Run | mean ms | p95 ms | delta vs faithful |
|---|---|---|---|
| faithful | 92.0 | 111.4 | — |
| ssao (High) | 170.5 | 200.1 | +78 |
| taa (+ CAS; MSAA 4× off) | 106.1 | 135.4 | +14 |
| volumetric (64 steps, arcade fog volume) | 141.3 | 170.5 | +49 |
| soft_shadows (PCSS + contact shadows) | 123.5 | 150.5 | +31 |
| ssr (24 steps, pool deferred) | 119.5 | 143.4 | +27 |
| all | 348.5 | 470.7 | +256 |

Screenshots of the same runs, each A/B against `0_faithful.png`. Mean absolute pixel change
in the region the effect targets (0–255):
- **SSAO:** contact darkening at the pool curb and sphere bases (Δ6.4 on the pool, Δ4.7 at the spheres).
- **TAA:** rail aliasing smoothed (Δ2.0).
- **Volumetric:** shafts under the slatted roof (Δ42.7 in the arcade).
- **Soft shadows:** softer shadow ends of the rails and spheres (Δ2.2).
- **SSR:** pool reflections (Δ4.2).

## Mid-GPU cost (to measure)

```
cargo run --release -p gothic-render-plus --example render_plus_demo --features demo,pcss -- \
    --bench out --size 1920x1080
```

The budget governor drops effects at runtime when the frame goes over `target_ms`. Shed order
by default: SSR, volumetric, soft shadows, SSAO, TAA. That roughly follows the expected GPU
cost, with SSR and volumetric the most expensive at 1080p. Adjust it once real numbers exist.
