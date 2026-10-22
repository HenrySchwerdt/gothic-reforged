# Performance follow-up candidates

Read-only review against `docs/research/PERFORMANCE.md` and the manager-provided current PC
baseline: `--play`, 1280x720, average 12.905 ms, GPU 3.442 ms, 12,765 entities / 1,554 visible.
The referenced JSON (`.local/bench/codex-baseline-1792648427.json`) was not present in this
worktree, so this note uses those supplied summary values. They do not isolate any candidate
below. No code was changed and no new profile or build was run; these are ranked hypotheses, not
newly proven bottlenecks.

## 1. Shadow pass mesh-bin work

`PERFORMANCE.md` already has direct trace evidence from an earlier run: shadow bin instance-buffer
writes and preprocess bind-group preparation dominated render-thread time, with thousands of
per-frame writes/bind groups across cascades. The current baseline remains CPU/render-thread heavy
relative to the reported GPU time, so this is still the strongest measured lead. The old
merge-per-tile experiment reduced entity count but ran about 4 ms slower because larger meshes
lost useful culling; do not repeat that strategy.

Candidate to investigate: instance repeated identical mesh/material pairs in small spatial clusters,
preserving per-cluster bounds, culling and the current shadow settings. First confirm the current
release trace still attributes time to `write_binned_instance_buffers<Shadow>` and
`prepare_preprocess_bind_groups`; count shadow bins and buffer writes. Compare interleaved baseline
and prototype runs with compiler load gone, same scene/camera/cascades/resolution, using avg/p95
frame time and render-thread spans. Also verify the cluster bounds retain the same visible and
shadowed geometry. No merge prototype is proposed without that culling check.

## 2. Moving-player grass-focus material fan-out

`walk/windsys.rs::grass_focus` runs in `Update`. After each 2 cm player movement it clones the full
wind-material registry and mutates the `player` uniform on each vegetation material. That is
source-level repeated work that can invalidate many material preparations per moving frame; the
number of affected assets and its cost in this baseline are unknown. This is not the `Assets::get_mut`
false-positive: the uniform assignment dereferences the guard mutably and changes the asset.

Instrument updates per second, registered wind material count, and render-side material/bind-group
preparation while standing still and while walking at a fixed speed. If this is material, evaluate
moving the shared player focus into one per-view uniform so grass keeps identical behavior without
rewriting each material. Preserve the same wind response and compare with vegetation visible; do not
use `GR_NOVEG` as the optimization result.

## 3. Environment light pushes to every material

In play mode, `walk/env.rs::env_update` pushes changed sun/SH values at most every 0.5 s. Each push
calls `gmat::push_light_env`, which iterates all Gothic and character materials and rewrites their
lighting fields. This path is conditional on the environment actually changing beyond its 0.02
threshold; default walk mode has a stopped day clock, so the cost may be limited to initialization
there. No current trace establishes that these periodic pushes matter.

Capture `env_update` and material asset events/preparation through a representative play-time day
cycle. Correlate push frames with p95/max frame time and the number of live materials. Only if the
push causes measurable spikes, prototype shared view/environment lighting data and compare images
and timings at identical sun/weather states; this would avoid asset fan-out without reducing
lighting quality.

`stream_tiles` / `stream_vegetation` were inspected but not promoted: the PVS recomputation exits
early when the observed cells are unchanged, and the vegetation search is a bounded 7x7
neighborhood. The per-frame resident-set difference checks still run, but no supplied profile
indicates they are material to the steady-state baseline.

## Grass-focus camera A/B evidence and trace follow-up (2026-10-22)

The committed analytical camera workload was used to compare the shared-focus buffer against the old per-material focus path. Immutable artifacts and provenance are in `D:/gr-wt/codex-renderer/.local/artifacts/gothic-reforged-camera-{oldfocus,focus}.exe`; source snapshots/manifests and the reverse-patch proof are summarized in `docs/HANDOFF_CODEX_20261022.md`. The 1280x720 A/B/A/B run used 30s settled-world warmup, 20s per sample, two repetitions, output `.local/bench-grass-focus-camera/summary.json`. Per pair, average/p95/GPU ms old→shared: 11.6661/12.8463/3.3069→11.2035/12.2293/3.2022, then 11.7172/12.9147/3.3681→11.2142/12.2673/3.1892. Run medians changed −4.13% average (11.6917→11.2088ms), p95 12.8805→12.2483ms, GPU average 3.3375→3.1957ms. The route followed the same 10s-period, 2m-radius loop; 21 expected/actual samples per run stayed within 3.85µm position residual and zero quaternion-component residual. Entity count matched at 12,752; start visible meshes differed by at most five (0.32%), final counts by two or fewer. `observer_cm` returns `Player.fly_pos` in fly mode, which `grass_focus` consumes, so this route does exercise the moving 2cm-gated grass-focus updates. It bypasses hero/controller physics and hides the hero; it is not whole-game movement parity. Two repetitions give only a small descriptive estimate, not high-confidence causal proof. The earlier generic walking comparison’s 3.69% average reduction is not causal evidence: its route endpoints and visible-mesh counts differed materially.

### Next bottleneck trace

Once the wind probe source is frozen, combine it and trace support into one optimized build rather than linking a trace-only binary and then rebuilding. The existing app feature is `trace = ["viewer", "bevy/trace", "bevy/debug"]`; `--trace` is installed before the Bevy app, and the benchmark resets span counters when measurement begins. Bevy 0.20 function systems emit INFO `system` spans with a `name` field, which the app table keys as `system:<name>`; check for the wind-probe/system spans in the output rather than assuming they are present. Use the reviewed GNU environment and target:

```sh
source /c/Users/PowerTower/dev/gothic-reforged/.local/env.sh
cd /d/gr-wt/codex-renderer
export CARGO_TARGET_DIR=D:/gr-build/integrate RUST_MIN_STACK=67108864
cargo +stable-x86_64-pc-windows-gnu build --release -j4 -p gothic-reforged --features viewer,trace --locked
```

Run that frozen artifact from the existing Wind runtime at 1280x720 with the wind-probe option chosen by its owner plus `--play --bench 20 --bench-warmup 30 --bench-camera --bench-gpu --bench-name windprobe-trace --trace`; keep unrelated `GR_*` flags unset. The bench JSON is written under runtime `.local/bench/` and, with trace enabled, contains `top_spans_per_frame` entries (`span`, `ms_per_frame`, `calls_per_frame`, `max_ms`); a companion `.local/bench/trace-<unix>.txt` contains the top spans and a systems-only section. These spans are inclusive and summed across threads, so nested totals must not be added as if they were disjoint frame costs. The trace/debug build is diagnostic only; its timings are not a release-performance result. Rebuild without `trace` for final performance comparison.