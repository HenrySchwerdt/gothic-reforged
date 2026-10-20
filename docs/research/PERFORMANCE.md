# Walk-mode performance

Budget: <8 ms/frame (target 4) at 1280x720 on an RTX 3060 Ti; walk mode is CPU/render-queue bound, not fill bound.

## Measurement
`.local/prof.sh <label> [ENV=VAL ...]` runs `--walk --smoke` (no vsync) and prints median/p95 frame time,
mesh-entity and visible counts; `GR_PROF=1` adds per-pass GPU timestamps (RenderDiagnosticsPlugin).
Ablation switches: `GR_HIDE=terrain,static,tree,foliage,water`, `GR_NOSHADOW`, `GR_NOSKY`, `GR_NOVEG`,
`GR_RES=WxH`, `GR_CASC/GR_SHDIST/GR_SHFIRST/GR_SHMAP`, `GR_NOCULL`. Other agents' builds skew timings: compare
relative numbers from interleaved runs. Resolution 640x360 did not change the time: not fill bound.

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
