# Walk-mode performance

Budget: <8 ms/frame (target 4) at 1280x720 on an RTX 3060 Ti; walk mode is CPU/render-queue bound, not fill bound.

## Measurement
`.local/prof.sh <label> [ENV=VAL ...]` runs `--walk --smoke` (no vsync) and prints median/p95 frame time,
mesh-entity and visible counts; `GR_PROF=1` adds per-pass GPU timestamps (RenderDiagnosticsPlugin).
Ablation switches: `GR_HIDE=terrain,static,tree,foliage,water`, `GR_NOSHADOW`, `GR_NOSKY`, `GR_NOVEG`,
`GR_RES=WxH`, `GR_CASC/GR_SHDIST/GR_SHFIRST/GR_SHMAP`, `GR_NOCULL`. Other agents' builds skew timings: compare
relative numbers from interleaved runs. Resolution 640x360 did not change the time: not fill bound.

`--bench-walk` waits until tile and vegetation queues are empty, the normal player has been placed, and
the existing smoke stability condition has held for 91 frames. Its warm-up then starts, followed by
the measured interval and a forward-5 s/back-5 s route clock. Each JSON sample records a per-run ID,
readiness wait, start/end player position and yaw, camera transform, entity/visible-mesh counts, and
one route snapshot per second. A 300 s readiness timeout exits with an error; configured warm-up is
counted only after readiness and is not included in that timeout. These fields expose mismatched
workloads; they do not guarantee equal movement distance when frame/input rates or collision response
differ, so compare route endpoints and counts before attributing A/B timing differences to code.

## Findings (loaded machine, ms median)
| Change | Before | After |
|---|---|---|
| Baseline | 16 | - |
| Hide statics (ablation) | 4.4 | static props dominate (main + 4 shadow cascades) |
| No shadows (ablation) | 6.0 | shadows cost ~10 ms with 4 cascades / 160 m |
| Distance-cull small statics (VisibilityRange, radius*60 m, 40..400 m) | 14.8 | ~12 |
| Drop distant-LOD "world_lowpoly" submeshes (also fixes smeared roofs) | ~12 | ~10.4 |
| 3 cascades, 90 m, first bound 12 m | 10.4 | ~9.5-10 |
GPU (RenderDiagnostics): opaque pass 1.9, shadows 0.65, GPU mesh preprocessing ~3 (bin_unpacking, uniform_allocation) -> ~6 ms GPU total; the rest is per-entity CPU work (extract/queue/specialise for ~9k mesh entities).

## Next
Merge static props per tile/material (fewer entities), cull shadows for small casters, impostors for distant statics.

## CPU profile and fixes (2026-10-20, performance agent)

Tools: `--bench <s> [--bench-warmup s] [--bench-name n] [--bench-walk] [--bench-gpu] [--trace]` (JSON to `.local/bench/`, no vsync), F3 overlay (fps, frame ms avg/p95/max, main-thread CPU, wait, GPU when `--bench-gpu`/`GR_PROF`, entities, visible meshes), `--features trace` (Bevy spans with system names, own aggregating layer; `--trace` writes `.local/bench/trace-*.txt`, F4 dumps it). `cargo build --profile profiling` = release without LTO plus line tables. Scenario: `--play`, 2560x1440, Ardea plaza, 66 NPCs, 13.9k entities. Machine shared with compiling agents (load 60-100 %), so compare only runs taken together.

### Finding: the frame is render-thread CPU bound, not main-thread or GPU bound
Main thread 5-9 ms, GPU 6-10 ms, yet 38 ms/frame. The render world spends its time in the **shadow pass bookkeeping**: with 4 cascades / 160 m every distinct mesh in each cascade is a bin with its own `Queue::write_buffer` (about 2000 per frame) and `create_bind_group` (about 1350 per frame).

Top costs, baseline trace (ms/frame summed over threads, inflated by span overhead):

| # | Span | ms | Note |
|---|------|----|------|
| 1 | `write_binned_instance_buffers<Shadow>` | 15.7 | ~2000 write_buffer calls |
| 2 | `prepare_preprocess_bind_groups` | 15.1 | ~1350 bind groups |
| 3 | `submit_pending_command_buffers` | 11.5 | follows from shadow views |
| 4 | `Queue::write_buffer` total | 17.8 | children of 1 |
| 5 | `Device::create_bind_group` total | 10.0 | children of 2 |
| 6 | `write_binned_instance_buffers<Opaque3d>` | 3.1 | |
| 7 | `camera_driver` (render passes) | 3.4 | |
| 8 | `per_view_shadow_pass` | 1.6 | |
| 9 | `prepare_clusters_for_gpu_clustering` | 1.2 | |
| 10 | `animate_system` | 0.9 (spikes 20) | all 67 animators every frame |
| 11 | `write_binned<AlphaMask3d>` | 0.85 | |
| 12 | `write_batched_instance_buffers` | 0.84 | |
| 13 | `check_dir_light_mesh_visibility` | 0.66 (spike 17) | |
| 14 | `sync_npcs` | 0.59 | rewrote transforms every frame |
| 15 | `transform::mark_dirty_trees` / propagate | 0.45 | spikes 8-20 ms on tile spawn |

Main-thread gameplay (core tick at 30 Hz, perception, UI, focus, camera) is below 0.3 ms each; there was no O(N^2) hot spot at 66 NPCs.

### Shadow cost by setting (render thread, same machine, avg ms)
4 cascades / 160 m: 37.5 | 3 / 100 m (new default): 17 | 2 / 60 m: 15 | none: 11.5 (p50 8.3).

### Changes
* `perf.rs`: shadow preset 3 cascades / 100 m / first bound 6 m, applied unless `GR_CASC`/`GR_SHDIST`/`GR_SHFIRST` is set. Distance-based `NotShadowCaster` LOD for small props was tried and gave no gain (reverted).
* Animation update-rate LOD (`crates/animation`): full rate under 12 m, 30 Hz to 30 m, 15 Hz to 60 m, 8 Hz beyond, 10 Hz behind the camera; first updates staggered. Joint and attachment writes are skipped for characters whose pose and root transform did not change (`Animator::joints_dirty`).
* `sync_npcs` and `mirror_state` write only on change (stops transform/UI change flags every frame).
* Profiles: `[profile.profiling]` added. `[profile.release]` unchanged (thin LTO); measured fat LTO + codegen-units 1 + panic=abort is a further ~35 % off, see below; recommended for shipping builds, not for iteration (30 min build).

### Before / after (avg frame ms, 2560x1440, load 80 %, two rounds)
| Scenario | Baseline | New, no LTO | New, fat LTO |
|----------|----------|-------------|--------------|
| `--play` standing, Ardea | 23-24 (38 on a quieter-GPU run) | 18 | 11.5-12.3 (p99 13.5) |
| `--play` walking | 19.2 | n/a | 12.4 |
| main-thread CPU | 5.4-6.1 | 4.3-5.1 | 3.1-3.3 |

### Still GPU/render-bound (look agent / tree agent)
Remaining ~9 ms is shadow bins: merge static props into fewer meshes or instance them (one bin per mesh per cascade is the unit cost), drop shadows for vegetation beyond ~40 m, consider 2 cascades. UI: `ui::hud::update_hud` assigns bar widths, visibilities and rose rotation every frame (flags layout); compare before writing. `walk.rs` calls `std::env::var_os("GR_NOSKY")` every frame in `sky_follow`.

## Update: merge experiment, shadows, sea
- Merging statics per tile+material (`GR_MERGE=1`, `merge_statics`) cut entities 9.4k -> 5.9k but was ~4 ms SLOWER
  (unculled tile-sized meshes in every shadow cascade); disabled by default, kept for later per-cluster merging.
- Small statics (<3 m radius, `GR_SHMIN`) no longer cast shadows; defaults: 3 cascades, 90 m. Median ~9.5-10.5 ms on a
  loaded machine (statics+shadows are still ~5 ms; without shadows ~6 ms).
- Sea: one camera-following 8 km water plane at sea level; sea-level tile water is hidden.
- Sky: clouds sampled from the original `sky/Wolkentest_01` density texture (loaded through the loader thread).

## Regression bisect (2026-10-21, performance agent)
Build: release, LTO off, `--play --bench`, 2560x1440 default window, AutoNoVsync, min of several runs (machine shared with compiling agents; load 100 % makes runs vary 8.7..19 ms).
The reported 21 ms was load noise plus a non-LTO build; bloom, SpeedTree, lighting and the camera are not the cause.

| Toggle (play, avg ms) | ms |
|---|---|
| HEAD, 3 cascades / 100 m | 10.0-10.1 (main CPU 3.0) |
| GR_NOBLOOM | 9.7 (bloom ~0.4) |
| GR_NOVEG | 9.7 |
| GR_NOCHAR | 7.3 (29 NPCs incl. their shadows ~2.7) |
| GR_NOSHADOW | 4.4 (shadows ~5.7) |
| 2 cascades / 60 m / 8 | 7.9 |
| 2 cascades / 100 m / 20 (new default) | 8.65-8.7 |

Change: `perf::shadow_preset` now 2 cascades, 100 m, first bound 20 m. Walk: 8.2 best (noisy). The remaining cost is per-mesh shadow bins; thin/fat LTO (shipping profile) is a further ~25-35 %, so expect play ~6-7 ms there (not rebuilt: 17-30 min under load).
Startup (warm cache, OS-cached): database from cooked cache 50 ms, actor/audio/NPC data 0.7 s, world loader thread (VFS + 134k placed-mesh index) 1.9 s in parallel; process start to bench end for a 1 s bench = 3.8 s, i.e. ~2.8 s to the first playable frame. Already < 3 s; a nav/terrain cook was not needed. Next: cache the placed-mesh index in the cooker.
Root `Cargo.toml`: `[profile.dev]`/`[profile.test]` debug = "line-tables-only" so test binaries link.
