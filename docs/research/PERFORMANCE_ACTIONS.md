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
