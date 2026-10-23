# Performance follow-up candidates

Historical read-only review against `docs/research/PERFORMANCE.md` and the then-current PC
baseline: `--play`, 1280x720, average 12.905 ms, GPU 3.442 ms, 12,765 entities / 1,554 visible.
The referenced JSON (`.local/bench/codex-baseline-1792648427.json`) was not present in this
worktree, so this note uses those supplied summary values. They do not isolate any candidate
below. No code was changed and no new profile or build was run; these are ranked hypotheses, not
newly proven bottlenecks.

## Corrected renderer fixed-camera baseline (2026-10-23)

`.local/bench-dither-fixed-baseline/summary.json` records four interleaved runs (A/B/A/B),
30 s settled-world warmup and 20 s measurement at 1280x720, using source commit
`09332fc78a01005db5c2c42f6e57ee0dbae76abe` and executable SHA-256
`ec1fcc0688e64ddd09091702c84a6e11dafcba5d337e8f88eb627e0b52e9bf94`. Both nominal variants
used that same executable, so the measured `-0.35%` average difference (10.2595 to 10.2236 ms)
is run-to-run variation, not a speedup comparison. Per-run averages were 10.1674 / 10.3516 ms
for baseline and 10.1596 / 10.2877 ms for candidate. Baseline/candidate median p50 / p95 was
10.0176 / 11.6265 and 9.9814 / 11.7087 ms; whole-frame GPU average was 2.8630 / 2.9236 ms.
Runs recorded 1,933–1,969 frames and 12,752 entities; visible meshes were 1,545–1,546 at route
start and 1,541 at end. The analytical camera-only route used a 10 s period and 2 m radius (4 m
maximum displacement), with no hero or physics. The runtime was
`C:\Users\PowerTower\Desktop\GothicReforged-Codex-20261023-DitherFix\runtime`; the child
environment recorded only `GR_RES=1280x720`, with no aim-pick or raycast comparison switch
enabled. GPU timing is whole-frame aggregate, not per-pass attribution.

The earlier retained trace below remains a historical lead only. The corrected-source indirect-
drawing trace and release validation are complete; see the next section. Instrumented timings are
for CPU-span attribution, not shipping performance.

## Fresh fixed-camera indirect-drawing trace (2026-10-23)

The completed diagnostic pair is in `.local/dither-fixed-trace/trace/{default,no-indirect}/bench.json`.
Both runs used the same trace/debug executable built from source
`558b9de1a4bc039b6e02987dc3943c8639db2b89` (SHA-256
`75c76b2301e9442dce27fd6b50dbf572bf44f906b896464ef60ebe96720a570f`). At 1280x720, each used a
30 s settled-world warmup followed by a 20 s analytical camera-only route; the only child environment
change was `GR_NO_INDIRECT_DRAWING=1` for `no-indirect`. `Queue::write_buffer` fell from 1125.8
calls/frame (4.713 ms/frame) to 18.0 (0.426 ms/frame). In the default trace,
`write_binned_instance_buffers<Shadow>` was 3.259 ms/frame and
`prepare_preprocess_bind_groups` 3.095 ms/frame; neither appears in the candidate top 40. Their
absence there does not establish zero cost. Root reviewed the fixed-view images: no obvious geometry,
shadow or checker loss, though small pixel differences remain.

These are instrumented diagnostic spans and frame timings, not shipping-performance results; spans
are inclusive and must not be summed as disjoint costs. The trace run averages were 15.672 ms / GPU
4.222 ms default and 6.535 ms / GPU 1.304 ms with the flag, so the large timing shift is not a valid
release gain claim. The separate smoke-walk traveled 1642 cm versus 890 cm and is not workload parity.
Source audit explains this limitation: smoke uses360 uncapped Update steps with each
`time.delta_secs().min(0.1)`, but prints a literal "in6s" without accumulating stage-2
elapsed time (`walk.rs` smoke stage2). Faster rendering can therefore integrate less
simulated time in those360steps. Grounded360/360 is a step-count result; do not infer
movement-speed differences from distance/6. The analytical camera benchmark avoids this driver.
Release A/B and QA are complete on build source `cb909ff3494e29f41533930190dd7fd9cbc46739`
(log `.local/pc-direct-render-release-build.log`), packaged from clean HEAD
`3716a5552c80928bd89ef633d8ff9809f9b71804`. On one EXE hash
`8af2257ce156e49a4ac6486a5b32a6d22753830e6c71e8c2382bc5c8d14c6823`, the release A/B/A/B
camera test (1280x720, 30 s warmup, 20 s per run, two runs per variant) measured median run
average 10.0434 ms / p95 10.7174 ms / GPU 2.9299 ms for `GR_NO_INDIRECT_DRAWING=0`, versus
5.4898 / 5.9794 / 1.1016 ms for `=1` (-45.34% average in this fixed scene). Both variants had
12,752 entities and 1,541–1,546 visible meshes at route boundaries, with about 35 μm anchor
variation. This is not a generalized FPS, movement, hardware or VR claim; only two samples per
variant were collected. Package QA passed melee13, magic9 and bow15 assertions; fixed smoke passed
360/360 grounded frames, and root approved the final images. Evidence is in
`.local/no-indirect-release/bench/summary.json`, `qa-package/` and `qa-package-smoke/`.
The harness switch is `scripts/bench-renderer.ps1 -CompareIndirectDrawing` (commit `8677336`).
Windows defaults to the candidate path; other platforms’ defaults are unchanged.

## 1. Historical shadow-pass mesh-bin lead

`PERFORMANCE.md` has trace evidence from an earlier run: shadow bin instance-buffer writes and
preprocess bind-group preparation dominated render-thread time. The fresh indirect-drawing trace
shows the default shadow-bin span at 3.259 ms/frame, but it is absent from the candidate top 40;
that does not establish zero candidate cost. The validated release A/B improved this fixed scene,
but it does not isolate shadow work from other preprocessing. The old merge-per-tile experiment
reduced entity count but ran about 4 ms slower because larger meshes lost useful culling; do not
repeat that strategy.

Do not start a merge prototype from this historical lead alone. If a future trace on the validated
release identifies material residual shadow work, measure it at the same camera/cascades and verify
that any experiment retains visible and shadowed geometry before timing claims.

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

### Completed diagnostic trace procedure

The `viewer,trace` diagnostic build and fixed-camera pair described above are complete. Bevy INFO
system spans were captured with `--trace`; span counters reset at measurement start and are reported
as `top_spans_per_frame` in the bench JSON. The chosen source/runtime and exact evidence paths are
recorded above. These spans are inclusive and summed across threads, so do not add parent and child
times as if they were disjoint. The diagnostic build timings are not release timings; the same-EXE
release comparison and package QA are now recorded above. Do not repeat the trace or release runs
on unchanged code.
## Ranged aim picking

The retained camera-only probe (`.local/profile-wind-probe/trace-1792684619.txt`) measured
`walk::play::ranged::aim_frame` at 6.476 ms/frame inclusive (max 9.138 ms), versus 8.735 ms for
the enclosing `Update` schedule. Do not add those inclusive parent/child spans. The trace has no
subspans inside `aim_frame`, so it does not prove which call dominates.

`aim_frame` computes a 10,000 cm collision pick every frame. That value is only consumed while aiming,
while a bow reload phase is active, or while the crosshair is fading. The idle path still updates the
casting-hand anchor and `hero_hand` for spells, clears a stale hero aim, advances draw/fade state, and
stops or updates clips exactly as before; the optimization skips only the unused pick. The guard uses
the **pre-update** fade amount so the last visible fade frame still gets a fresh pick. Reload is
included because `hero_aim_sine` uses the pick during reload even when `view.aiming` is false.

For profiling, `GR_AIM_PROFILE=1` plus finite `--bench <seconds>` emits one bounded summary at
benchmark exit. It reports aim/reload/fade frames, pick calls, skipped/forced idle picks and
cumulative query time separately for `pre_measure` and `measurement`. The pre-measure bucket covers
startup, loading/readiness and warmup; both buckets count only frames where camera and hero are
present. `Instant` timing is used only with this opt-in profile. Timing A/B runs should leave this
variable unset. With a `trace` build, the actual query also has a nested `ranged_pick_point` span.

`GR_AIM_ALWAYS_PICK` restores the redundant idle pick without changing `RangedView`, simulation
aim/hand, crosshair, or animation outputs. `scripts/bench-renderer.ps1 -CompareIdleAimPick` sets it
for baseline runs and removes it for candidate runs, records the per-run value, and restores the
caller's process environment in `finally`. The same executable can be supplied for both variants;
use this to isolate the query cost before comparing code revisions.

Existing gameplay coverage to rerun after any related change:

- `bow_faithful` (`apps/game/src/play/qa/bow.rs`) exercises Aim and PowerAim, the actual shot,
  Aim_Reload with all three reload clips, and bow pitch blends.
- `magic_faithful` (`apps/game/src/play/qa/magic.rs`) and `magic_bow`
  (`apps/game/src/play/qa/ranged.rs`) exercise cast/launch/impact and the magic hand/missile path.

These scenarios cover active aim, reload and spell-hand behavior. They do not verify that idle
collision picks are skipped or establish a performance gain; use the opt-in profile and same-binary
A/B harness for that.
