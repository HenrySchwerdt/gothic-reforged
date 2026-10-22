# Gothic Reforged - current Codex handoff, 2026-10-22

Read this current file first. Historical journal preserved verbatim in
`.local/HANDOFF-before-2bae7cf-compaction.md` and prior Git commits. Previous approximate
14:44/14:48 time headings were wrong; actual tool/build/Git timestamps take precedence.

## Scope and preservation

- Edit source ONLY `D:/gr-wt/codex-renderer`, branch `codex/renderer-stability`.
- Original game read-only: `D:/SteamLibrary/steamapps/common/Gothic 3`.
- Preserve primary `C:/Users/PowerTower/dev/gothic-reforged`, Claude's dirty integration
  `D:/gr-wt/integrate`, other worktrees, original saves and older Desktop packages.
- User authorized autonomous manager + cheaper gpt-6-luna coders, performance/render/VR fixes,
  runnable optimized PC EXE and real screenshots. Full Gothic 3 parity is NOT achieved.
- No remote publishing/messages, original content/decompiled bodies in Git, or unneeded installs.
- Backups in primary `.local/checkpoints/codex-resume-20261022-0720/`; PENDING_CONTINUATION.md
  there is the short resume pointer. Original takeover archives/patches remain intact.

## Latest VERIFIED user-facing release (unchanged)

`C:/Users/PowerTower/Desktop/GothicReforged-Codex-20261022-WindFocus/GothicReforged.exe`

SHA256 `b11537c354934be159de02849122dab43ac8fd57ad2466df518fb5a8f341f972`.
Compiled game source `93655de06d2ad1e22499d721ad7649e3a73de7a3`; packaging HEAD d208696 differs
only in script/docs. build-info.json explicitly records executable source. Optimized thin-LTO.
Normal + GR_NOWIND GPU smokes: clean, grounded360/360. Final packaged melee_faithful PASS
from C:/Windows/Temp; independent saves empty. Actual inspected screenshot:
`runtime/.local/qa/melee_faithful/05_holstered.png`. Requires local original PAKs/cook cache.
PRUEFSTAND.txt records limitations. Earlier Wind/water2/Claude packages retained unchanged.
WindClock folder is a previous candidate, not latest verified release.

## ACTIVE work - check jobs/agents before starting anything

Current source HEAD `2bae7cfc6d636ef5e851e55a8a348701069ba40d`: idle aim-ray optimization.
Checks passed with viewer and viewer,trace; runtime/release validation PENDING.

Agent `/root/wind_stability` owns ONE optimized viewer-only (NO trace) release build and
sequential validation pipeline. Log `.local/pc-idle-aim-build.log`; immutable output planned
`.local/artifacts/gothic-reforged-idle-aim.exe`. Inspect agent/session/log; do NOT duplicate.

Authorized after build, sequential, no compiler or other game during runs:
1. Normal + GR_NOWIND --walk --smoke, renderer logs clean, grounded360/360.
2. QA bow_faithful, magic_faithful, melee_faithful from existing Wind/runtime,1280x720;
   GR_INPUT_DEBUG only QA. Require explicit QA PASS even when exit0; stop on failure.
3. Two separate diagnostic camera benches10s/warmup5s with GR_AIM_PROFILE=1, legacy flag
   GR_AIM_ALWAYS_PICK=1 then unset. Need measured state/query counts and cumulative query ms.
4. Clear profile/debug flags. Same-EXE A/B/A/B via bench-renderer.ps1 -Camera
   -CompareIdleAimPick -WarmupSeconds30 -Seconds20 -Repetitions2 -Resolution1280x720;
   fresh `.local/bench-idle-aim-camera`. Actual per-run legacy value and hashes recorded.
5. Report to root; NO packaging or extra measurements before review. Root may next measure
   ordinary idle play (hero/physics visible) with the same legacy toggle, then package if all pass.

No other source writer. water_renderer completed aim source. wind_clock completed read-only
physics candidate audit. Previous pipeline artifacts are immutable; don't use the mutable
D:/gr-build/integrate/release EXE as a baseline without verifying its hash.

## Aim-ray change - preserve these semantics

Measured diagnostic system aim_frame was6.4764ms/frame inclusive; underlying query attribution
is still pending new direct counters. Old code always raycasted even when result unused.
New pick required for `in_aim || reloading || pre-update fade>0`; reload needs hero_aim_sine,
and final fade frame still needs a fresh distance. Hands/hero_hand for spells, glow, draw,
heroAim clearing, animation/crosshair semantics retained. No fabricated hit.
Cached GR_AIM_ALWAYS_PICK restores redundant idle work ONLY for same-binary comparison.
GR_AIM_PROFILE emits one AppExit summary split into pre-measure and actual measurement counts.
Nested ranged_pick_point span exists only in trace feature. Script restores caller env in finally.
Source checks: `.local/aim-pick-check-viewer{,-trace}-final.log`; no unit-test mirror added.

## Verified renderer work and measured evidence

Water: exact framed ShaderWater detection; planar Ocean mesh sea-level; Reverse-Z scene-depth
shore fade; suppress only known untextured G3_World_Rivermesh_01_Depth_<digits> depth proxies.
Regular Sea remains non-solid water. Actual corrected coast images in primary ignored QA.
Static/native findings do not establish full original behavior. Parser/render-rules tests passed.

Wind: shared color/depth/shadow deformation (edd3b55), compressed/missing-normal guards,
correct prepass Globals binding1 vs forward11. Fast checker tools/shader-check (e816c67)
checks8 WESL/Naga variants plus deliberate invalid fixture, not GPU bindings/layout.
Bounded f64 shared clock13cead4: environment28/28 tests inspect encoded f32 hour/day phases,
previous sample across config changes, legacy gust slots. Shared grass focus93655de removes
per-material asset mutation, latest focus initializes newly streamed materials immediately.

Matched camera workload3c89829: finite --bench-camera/-Camera, settled + warmup, analytical
2m-radius/10s loop,4m max displacement. Fly route hides hero/bypasses PLAYER physics, not all
NPC simulation. Moving fly_pos DOES feed grass_focus via observer_cm. Applied timestamp,
expected/actual pose metadata account for sampling jitter. Existing --bench-walk remains
frame/physics dependent; old3.69% walk reduction is NOT causal evidence.

Matched grass camera A/B: `.local/bench-grass-focus-camera/summary.json`,1280x720,30s warmup,
20s samples,A/B/A/B,2 repetitions, clean renderer logs/no inherited GR flags. Average medians
11.6917 ->11.2088ms (-4.13%),p95 12.8805 ->12.2483,GPU3.3375 ->3.1957ms. Anchors within33um,
expected/actual pose residual<=3.85um; entities12752 equal; visible counts differ<=5 atstart.
Scene-specific small estimate, not general speedup guarantee. Full source/patch manifests prove
only5 focus files differ; perf.rs/walk.rs identical. `.local/camera-source-manifest-comparison.txt`.
Disposable `.local/camera-bench-snapshot` is intentionally left in OLD-focus state; never treat
it as active source. Both immutable camera artifacts and provenance retained.

GPU boundary diagnostic227d0a7: immutable wind-probe-trace EXE SHA256
93d3ebf238a2efc048d3cfaea6094152934e61742824a6e7187ed17a4c9f352f. Exact opt-in
GR_WIND_CLOCK_PROBE_START=3599.0/86399.0, only finite warmed --bench-camera. Actual GPU tests
PASS3 distinct80-byte readbacks each, bitwise equality, stable BufferId, continuous previous
records. Hour3599.783492 ->3600.000997; day86399.759410 ->86400.003279. Synthetic offset,
NOT hours-long GPU or temporal silhouette proof. `.local/qa/wind-clock-probe/` + JSON copies
in `.local/profile-wind-probe/`. Normal flag-off path performs no GPU copies/poll/logging.

Probe-unset diagnostic trace:1263frames,15.836ms avg,16.596p95,4.472GPU,11.283mainCPU.
Inclusive aim_frame6.4764, preprocess bindgroups3.3471, shadow binwrites3.0687ms. Do not sum
nested/parallel spans or compare trace/debug timings to shipping release. Trace/table/JSON in
`.local/profile-wind-probe/`. Source-backed next hypothesis after idle ray gate: conservative
ray-grid supercover DDA instead of full long-ray AABB rectangle; preserve nearest hit/normal
ordering, boundary/vertical/corner cases, finite cap/fallback and existing oversized-triangle
index policy. Not implemented. Needs independent brute-force synthetic oracle and measurement.

## Failures/limits retained (never hide them)

- First packaged WindClock melee test lost held-input Power/Pierce. Two controlled old/new
  comparisons and final WindFocus packaged run PASS; cause not proven. Logs retained.
- Shader wind candidates1-4 had genuine failures despite exit0; only verified Wind5 delivered.
- Benchmark null ExitCode fixed35879f9 with Start-Process -Wait -PassThru; null still rejected.
  Camera benchmark outer wrapper later misused LASTEXITCODE after a PS script; actual4runs and
  summary succeeded. No rerun. Preserve partial failed harness output.
- New GPU probe stage-unit harness could not link standalone real Bevy (thin-LTO/import-library
  search paths). ZERO unit tests executed; logs/provenance own.local. GPU runtime probes passed.
  Do not waste more retries. Camera exact-source math/flag harness DID pass4 tests.

## VR status - preserved, not headset-validated

Input validity/focus gates, Quest Sim::resolve_ported bridge, grip geometry checks passed earlier.
Third-person host head layer fixture dee70e2 corrected; actual host images inspected, FPV head
hidden. Android native AArch64 lib built (104687840bytes), SHA256
3b720252047aa76d1b319ce02b4fabb467caa5c96e60fd5204eedc0576a58968.
No new APK: Android SDK packaging tools absent in bounded search, NDK only. No headset install/
runtime UX validation. Original Quest3 package retained. Before new Quest work use mandatory
Meta verify-first skill/current sources. Canonical own apps/quest excluded rootworkspace; use
own.local/qmine-codex and vendor-codex, never edit .local/vendor (junction to primary).

## Exact build tools and continuation

Git Bash: source `/c/Users/PowerTower/dev/gothic-reforged/.local/env.sh`; cd own checkout;
export CARGO_TARGET_DIR=D:/gr-build/integrate RUST_MIN_STACK=67108864;
`cargo +stable-x86_64-pc-windows-gnu build --release -j4 -p gothic-reforged --features viewer --locked`.
Diagnostic adds trace feature, not shipping. Default MSVC link.exe unavailable. Keep GNU dlltool
adapter/LLVM-MinGW env. Do not use broad cargo fmt (previous side effects restored bytewise).
For exec session jobs poll SAME session; don't lose session IDs or duplicate builds.
Actual fatal render/QA failures may exit0; inspect logs and explicit markers.
Fast shader checker: D:/gr-build/shader-check/release/gothic-shader-check.exe --repo ownpath.
Package script requires NEW output dir, correct SourceCodeRoot and exact artifact hash; preserve
previous packages/saves and record executable compiled commit when packaging HEAD differs.

Heartbeat `claude-abwarten-dann-gothic-pc-build-fortsetzen` remains ACTIVE every15min. Silent when
unchanged. Do not duplicate or disable until full completion/user cancellation. User away,
continue scoped cheap coders with coordinated builds; preserve checkpoints before quota limits.
Quota reset happened17:22 Berlin after prior98% window; queued heartbeats were not executed work.
