# Gothic Reforged - Codex continuation, 2026-10-22 17:45 UTC

## Read first / preservation

Only edit D:/gr-wt/codex-renderer, branch codex/renderer-stability.
Primary C:/Users/PowerTower/dev/gothic-reforged and Claude's dirty D:/gr-wt/integrate preserved.
Original game D:/SteamLibrary/steamapps/common/Gothic 3 is READ ONLY; original saves untouched.
User authorizes autonomous manager + cheap gpt-6-luna coders, local PC EXE/screenshots,
performance/render/VR improvements, careful quota use. Full Gothic3 parity is NOT achieved.
No remote publishing/messages, original content/decompiled bodies in Git, or unneeded installs.
Own generated evidence stays ignored .local. Existing takeover archives remain intact.

Full previous journal preserved verbatim .local/HANDOFF-before-perfleaf-release-compaction.md,
plus .local/HANDOFF-before-2bae7cf-compaction.md and earlier Git history. Some older14:44/14:48
headings were estimates; actual tool/Git/build UTC timestamps take precedence.

## LATEST delivered PC package - verified

C:/Users/PowerTower/Desktop/GothicReforged-Codex-20261022-PerfLeaf/GothicReforged.exe
SHA256 70318dac8d38ed791aa24eba58b38a7e7e1302d2da8c618d4f63dcb0ccd6b371
Compiled code43043b82229476bb829ff1e9b7157eecc80c0caa. Optimized GNU viewer-only thin-LTO,
build7m01s, log .local/pc-perf-leaf-parity-v2-build.log; immutable artifact
.local/artifacts/gothic-reforged-perf-leaf-parity-v2.exe. Only HANDOFF was dirty at packaging;
build-info explicitly records compiled commit, hash and validation. PRUEFSTAND.txt lists limits.

Actual final PACKAGE launched from C:/Windows/Temp, runtime-root.txt selects package runtime:
magic9 PASS (body0.0deg; actual camera99.811826deg; original turn profile200deg/s), bow15 PASS,
melee13 PASS. Renderer/shader scans clean; independent package saves empty before/after.
Evidence .local/qa/perf-leaf-package/. Prepackage same3 QA PASS plus normal/GR_NOWIND smokes
both grounded360/360, clean render logs: .local/qa/perf-leaf-parity-v2/.
Actual inspected images there: normal-walk.png, nowind-walk.png, magic_faithful/05_holstered.png.
Old WindFocus b11537..., Wind, water2, Claude packages retained. Earlier Desktop Perf c5c92...
remains labelled CANDIDATE, not latest release. Do not replace/overwrite old artifacts.

## LIVE work / next gate

No active compiler/game or coding jobs at package completion. Check collaboration.list_agents
and processes before new jobs; agents water_renderer, wind_clock, wind_stability are idle.
Source frozen43043b8 plus root docs update. No new renderer changes authorized yet.
Heartbeat claude-abwarten-dann-gothic-pc-build-fortsetzen remains ACTIVE every15min. Stay quiet
when unchanged; disable only full completion/user cancellation. User away; continue scoped
cheap coders, coordinated builds, checkpoints before quota. No duplicate builds/benchmarks.

NEXT renderer issue is actual WHITE/CHECKER distant LOD/foliage/terrain patches, visible in
BOTH normal-walk.png and nowind-walk.png. Do NOT claim world-normal fix removed these.
Read-only water_renderer audit found concrete source issues to verify/fix with matched images:
1. Gothic forward AND prepass carry visibility_range_dither but neither fragment calls Bevy's
   visibility_range_dither discard. Bevy standard forward+prepass both do. Near/far LODs can
   overlap fully: plausible checker/z-fighting; not a color/depth asymmetry by itself.
2. Gothic forward discards F_FADE foliage by distance; custom prepass ignores F_FADE. Thus
   invisible distant grass can still write depth. Mirror exact fade semantics/camera data in
   prepass. This affects depth consumers; alone not proven cause of white color pixels.
3. Character forward/prepass alpha cutoff matches; no VisibilityRange found for characters.
Audit is SOURCE EVIDENCE, not final root-cause proof. First review exact Bevy contracts and
proposal, implement bounded parity changes, checker + meaningful GPU comparisons, THEN new
release/checks. Keep43043b8 and current Desktop package immutable baseline. No shader edits yet.

Other follow-ups: actual measured performance bottleneck beyond idle ray; native VR/APK SDK
prerequisites and real headset validation; no unsupported VR-ready claims.

## Recent implemented fixes / evidence

Idle aim2bae7cf skips expensive unused pick_point rays only when !in_aim && !reloading &&
pre-update crosshairfade<=0. Reload and final fade frame still get fresh pick. Preserve hand
anchors, magic hero_hand/glow/draw/clips and heroAim clearing. Cached GR_AIM_ALWAYS_PICK=1
restores redundant queries for same-EXE A/B, no visible semantics change. Opt-in GR_AIM_PROFILE
summarizes actual measured/premeasure counts once at AppExit; normal runs have no timer/log.
Bench switch -CompareIdleAimPick toggles legacy/optimized and restores caller env.
Direct diagnostic legacy880/880 idle queries5858.023ms (~6.657ms/query); optimized0 queries,
1009 skippedframes. Source checks viewer and viewer,trace PASS.

Same c5c92...EXE camera A/B/A/B1280x720, warmup30/sample20,2 repetitions:
median averages11.266 ->9.956ms (-11.63%); p9512.523->10.530. Ordinary stationary play,
hero/physics visible:11.834->9.964ms (-15.80%), p9512.738->10.431, exact entities12753/visible1547.
.local/bench-idle-aim-{camera,play}/summary.json. Scene-specific small samples; no general
movement speedup guarantee and NO new final70318...package benchmark. Don’t repeat absent reason.

Wind prepass453eb55 now feeds mesh_normal_local_to_world into wind_offset like forward,
not local vertex.normal. Combined NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_NORMALS guard
is required by Bevy Vertex layout; gmat::specialize adds both + normalfetch for custom shadow/
depth. Temporary unconditional VERTEX_NORMALS guard failed GR_NOWIND and was restored.
Final .local/shader-check-wind-world-normal-final.log all8 variants+negativePASS.
Previous wind offset uses same worldnormal: static vegetation transforms supported; moving
instance transform previousnormal parity NOT validated. Static checker does not prove GPU parity.

Magic QA investigation: original aim_at repeated excess raw counts and stopped at first
position crossing, while smoothing coasted; nominal100deg actually~270deg. Body correctly
integrated200deg/s, no stale QA sample/animation overwrite. Original c5optimized and previous
WindFocus failed32-35deg body error; c5legacy oncePASS. Raw evidence retained .local/qa/idle-aim-
package/magic-failure-first, magic-compare-*.log and .local/qa/magic-body-trace-optimized/.
Trace23a48c4 is opt-in GR_MAGIC_BODY_TRACE=1 + exact magic_faithful; cap256 rows,<=60Hz,
single deferred dump at assertion or timeout, no default logs/timers. Correct body dt/no skips.

QA-only camera helper43043b8 (after incomplete a2f25b6): captures target once, uses active gain,
bounded time-scaled PD for yaw AND pitch-.05 through normal input, fractional X/Y count carry
because controls.rs rounds MouseMotion to i32. Joint <=.5deg/<=2deg/s for150ms, existing4stimeout,
1swait and10deg body assertion unchanged. Gameplay camera filter/options/physics UNCHANGED.
RealCameraState CPU24/24: actual quantization boundary,30/60/120+jitter,Normal/Magic,wrap/reverse,
yaw0 with initialpitch-.7rad+residualX/Ymotion, postwait drift/settings preservation. Final logs
.local/qa/magic-camera-joint-settle-test.log and magic-camera-joint-viewer-check.log (PASS).
Earlier float-only23tests missed quantization; first09eb...gamecandidate timed out. It and logs
.local/qa/perf-leaf-parity/ retained; NOT delivered. Pitch legacy feedback failed4.28s CPU case,
leading to QA-only joint PD. Final actual70318...QA+package PASS described above.

## Prior renderer / timing verification retained

Water: exactframed ShaderWater classification, planarOcean sea-level, ReverseZ depth shorefade;
suppress ONLY untextured G3_World_Rivermesh_01_Depth_<digits> proxy family. RegularSea stays
non-solid water. Actual corrected coast images retained; static exports don’t prove full parity.
Color/depth/shadow shared deformation edd3b55, compressed/missing normals guards; Globals
prepass binding1 vs forward11. Shaderchecker e816c67 checks8 WESL/Naga variants+negative only.

Clock13cead4: boundedf64 sharedwindclock, current+previous4vec4, encodedf32 hour/day tests;
environment28/28PASS. Grassfocus93655de adds fifthvec4=>80byte sharedbuffer instead of per-
material rewrites; new materials immediately see latestfocus. Queuewrite once/frame stableasset.

Finite deterministic camera bench3c89829 (-Camera/--bench-camera), analytical2mradius/10sloop,
settled+warmup, applied/expected pose metadata, fly hideshero/bypassesPLAYERphysics notNPCs;
fly_pos DOES updategrassfocus. Focus A/B medians11.6917->11.2088ms(-4.13%),2 reps, scene-specific.
.local/bench-grass-focus-camera/. Snapshot .local/camera-bench-snapshot intentionally OLDfocus;
source manifests prove only5focusfiles differ, perf/walk same. Old --bench-walk frame/physics-
dependent, old3.69% not causal. Exact camera math harness4PASS. ExitCode script fix35879f9.

GPU probe227d0a7 actual hour/day boundary3 distinct80byte readbacks each: bitwisePASS, stable
BufferId/previous continuity, pose<=3.85um. Synthetic offsets3599->3600 and86399->86400,
not hourlong session/vertex silhouette proof. .local/qa/wind-clock-probe/, .local/profile-wind-probe/.
Immutable traceprobe EXE93d3ebf... retained. Normalflagoff no copies/poll/log.
Diagnostic inclusive aim_frame6.4764ms, preprocessbindgroups3.3471, shadowbinwrites3.0687;
never sum nested/parallel spans or compare trace timings to shipping. Future ray-grid supercover
DDA candidate READONLY audit only, no implementation. Need brute-force oracle/tie/boundary tests.
Standalone GPU-stage unit harness neverlinked (ZEROtests); actual GPU runtime probes passed.

## VR status / tools

Earlier validity/focus gating, ported QuestSim bridge and grip checks PASS. Host thirdperson
head layer fixture dee70e2 fixed; actual host images verified. Native AndroidAArch64 lib built
104687840bytes SHA3b720252047aa76d1b319ce02b4fabb467caa5c96e60fd5204eedc0576a58968.
No new APK: bounded SDK search found NDK only, missingpackagingtools. No headsetdeployment or
runtimevalidation; old Quest package preserved. Before Quest work read mandatory Meta verify-
first skill/current official sources. Ownquest workspace uses own.local/qmine-codex/vendor-codex;
NEVER edit .local/vendor junction toprimary. No SDK/license/install/deployment assumptions.

Exact Windows build via Git Bash (default MSVC link.exe unavailable):
source /c/Users/PowerTower/dev/gothic-reforged/.local/env.sh
cd /d/gr-wt/codex-renderer
export CARGO_TARGET_DIR=D:/gr-build/integrate RUST_MIN_STACK=67108864
cargo +stable-x86_64-pc-windows-gnu build --release -j4 -p gothic-reforged --features viewer --locked
Do NOT add --target: then RUSTFLAGS fail to reach HOST buildscripts (missinggcc libs). Failed
MSVC/--target tracechecks retained; exact knownenv checkPASS. Don’t install toolchains unnecessarily.
Diagnostic tracefeature notshipping. Poll SAME execsession; never duplicate builds. No broad
cargo fmt (previous unrelated formatting restored). Runtime/QA FAIL may exit0: requireexplicit
PASS and inspect logs. Shaderchecker D:/gr-build/shader-check/release/gothic-shader-check.exe
--repo D:/gr-wt/codex-renderer. Package only NEWdir, preserve compiledcommit/hash, finalQA from
C:/Windows/Temp mandatory. PS script successfulcompletion != $LASTEXITCODE; don’t rerun valid
benchmarks because a wrapper tested null LASTEXITCODE. Preserve failures rather than hide them.