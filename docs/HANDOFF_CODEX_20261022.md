# Gothic Reforged — Codex handoff, 2026-10-22

## Current action

Work only in `D:/gr-wt/codex-renderer`, branch `codex/renderer-stability`.
At 13:33, build5 passed both actual normal/GR_NOWIND smokes without shader or renderer errors,
with grounded360/360 each. Packaged gameplay melee_faithful QA passed from Windows/Temp.
The functional moving benchmark recorded readiness, poses and13one-second samples correctly.
The prior milestone source edd3b55 is verified for these paths; a subsequent clock build is active (see latest checkpoint below).
Failed wind builds1–4 are retained as diagnostic artifacts and must never be delivered.
Latest package: `C:/Users/PowerTower/Desktop/GothicReforged-Codex-20261022-Wind/GothicReforged.exe`.
SHA256 `e638e428f27585fa7c75dd0228c99bd853dc2a86ee975a3884bb64aa05ad41ad`.
Old water2 Desktop package remains available; the detailed earlier-release section below is historical.
User authorizes autonomous manager work with cheaper gpt-6-luna coders, renderer/performance,
leaf and VR fixes, PC EXE and real screenshots. Existing heartbeat
`claude-abwarten-dann-gothic-pc-build-fortsetzen` remains active, now every15minutes to reduce
queued prompt/token waste. Do not duplicate it. A usage block interrupted work09:11–12:23;
heartbeats during that gap were not work. At13:13 the5h window was33%used, weekly37%.

## Preserve

Primary `C:/Users/PowerTower/dev/gothic-reforged`, Claude `D:/gr-wt/integrate`, original
`D:/SteamLibrary/steamapps/common/Gothic 3`, existing Desktop packages and saves stay intact.
Claude integrated base36fef36 and dirty QA plus previously untracked Quest source were preserved
in own commit434ee3a. Primary ignored checkpoint `.local/checkpoints/codex-resume-20261022-0720/`
contains source/vendor ZIP backups, integration patch and PENDING_CONTINUATION.md.
Generated original-content evidence stays ignored; never commit original assets/decompiled bodies.
This is a development integration slice, not a complete Gothic3 port or gameplay-parity proof.
Historical verbose investigation log was copied to own `.local/HANDOFF-before-cleanup-1325.md`.

## Verified Desktop release

`C:/Users/PowerTower/Desktop/GothicReforged-Codex-20261022/GothicReforged.exe`
SHA256 `99bc33d3e2a32573711ee919b51d05fa35502f99706bf8677647019738bd05d5`, source96fd98d.
Optimized release: thinLTO, codegen1. No arguments selects play. Adjacent runtime-root.txt
resolves runtime relative to EXE before asset/save access. Runtime metadata copied into package;
separate empty saves. Original installed PAKs and existing cooked cache are still needed.
Launched from Windows/Temp; final package melee_faithful QA PASS. Earlier noargs startup
also loaded play and closed gracefully. Old Desktop GothicReforged-Test remains untouched.
`PRUEFSTAND.txt` and LIESMICH describe controls, validation and limits.
`D:/gr-releases/GothicReforged-Validation-20261022` is an earlier preview, not final.

## Implemented and verified

Water: bounded framed eCShaderWater detection; water source identity retained; planar Oceanmesh
vertices establish sea height. Reverse-Z scene-depth shoreline fade excludes water itself.
Only confirmed untextured G3_World_Rivermesh_01_Depth_<digits> proxies are suppressed from
visible/collision/water-level surface paths. Regular untextured Sea remains non-solid water.
Original native pass selection is unproven; see WATER_NATIVE_NOTES.md and WATER_RENDERER_AUDIT.md.
Primary `.local/qa/codex-coast-fixed-{low,shore}.png` show faceted triangles/bright strip removed.
Walk smoke grounded360/360; no shader errors. Original first depthfade-only candidate did not fix it.

Wind phase: fixed oscillator frequency from base strength; gust changes amplitude. Environment
actual-offset tests cover gust boundaries. GPU Globals wraps hourly; CPU10/50h cases do not prove
GPU continuity. New color/shadow/depth parity source shares gothic_wind.wesl, loads helper explicitly,
uses prepass Globals at group0/binding1 and adds existing normals only to custom depth/shadow input.
NORMAL_PREPASS_OR_DEFERRED_PREPASS and VERTEX_NORMALS guards use explicit braced blocks.
Build3 normal smoke passed, but GR_NOWIND failed; build4 exposed the previously unbraced guard.
Source correction passes8 WESL/Naga variants and rejects the historical unbraced assignment.
Build5 passed both normal and GR_NOWIND runtime smokes without shader/renderer errors. WIND_STABILITY.md documents scope and current evidence.
Shared bounded wind clock source is implemented in `crates/environment/src/wind_clock.rs`, bound through one stable 64-byte ShaderBuffer record with current and previous frame samples, and uploaded once in PrepareResources. Environment release tests pass28/28; viewer cargo check passes; standalone WESL/Naga checker passes its eight supported variants. These checks do not prove GPU buffer layout/binding or long-run visual continuity. Actual GPU smoke remains required before delivery.

XR/Quest: focus/action activation and complete pose validity gate input; preserve inactive last-valid
poses and neutralize inactive scalar/button/stick input. Aim pointer requires active aim. Quest combat
uses Sim::resolve_ported, synchronizes HP, registers hero and assigns confirmed NPC-hit target.
Claude palm offset, orcs and weapon stabilizer preserved. Host hero_check passed grip/palm/blade/
rotation sweeps. Headless third-person fixture was a camera mask issue; fixed camera layer0+1,
FPV still layer0. Rebuilt host_shots exits0; manager inspected new head/face image.
Own `.local/quest/host_*.png`; logs quest-host-shots-layer-{build,run}.log. No headset-parity claim.

Tests: environment24 + formats100unit/8integration + render-rules14 =146passed,1ignored;
XR3passed; Quest combat bridge2passed. These are targeted suites, not full workspace coverage.

Android native release built: primary `.local/target-quest/aarch64-linux-android/release/libgothic_quest.so`,
ELF64/AArch64,104687840bytes, SHA2563b720252047aa76d1b319ce02b4fabb467caa5c96e60fd5204eedc0576a58968.
No new APK: bounded SDK search found no aapt2/zipalign/apksigner/android.jar. No headset install/launch;
previous installed Quest package retained. Actual worn-headset validation remains open. See VR audit.

## Measurements and benchmark tooling

Idle A/B/A/B,1280x720,30s warmup+20s sample,2runs each, same package runtime, no compilers:
Claude baseline12.0194ms vs water2 release12.0713ms (+0.43%), p95medians13.4306/13.1976ms.
Own `.local/bench-final/summary.json`. Essentially equal; no speedup claim.
Normal wind3 vs water2:11.9551/12.0011ms (-0.38%),p95medians12.9127/12.9762ms,
GPU3.4068/3.4089ms. Own `.local/bench-wind-prepass3/summary.json`; no renderer review findings.
These normal-path timings do not make build3 deliverable because GR_NOWIND failed.
Old moving runs differed in actual workload and varied heavily; aggregate+9.1% is not a reliable
regression claim. See own `.local/bench-final-walk` and PERFORMANCE_ACTIONS.md.

Commit4b2357e instruments --bench-walk readiness: stream/vegetation empty, placed player,
91stableframes, then warmup. Records start/end poses, view transforms and1Hz route samples.
300s timeout bounds continuous lack of readiness. Idle timing unchanged; route is time-driven,
not deterministic physics. Build5 metadata smoke passed (13route snapshots, valid readiness/start/end data). Benchmark script ebe9738
rejects renderer failure logs even if exit0; import warnings mark manual review. Never time builds.

## Build/reproduce

PC Git Bash: source `/c/Users/PowerTower/dev/gothic-reforged/.local/env.sh`; own cwd; export
CARGO_TARGET_DIR=D:/gr-build/integrate RUST_MIN_STACK=67108864; run
`cargo +stable-x86_64-pc-windows-gnu build --release -j4 -p gothic-reforged --features viewer --locked`.
Keep GNU dlltool adapter and explicit GNU toolchain. Default MSVC linker is unavailable.
Artifacts in own `.local/artifacts/`; final water2 and preserved original baseline are separately named.
`scripts/package-pc.ps1` requires new OutputDir and correct SourceCodeRoot (own checkout).
Never overwrite a tested package with an unverified candidate. Verify hash/buildinfo and arbitrary-cwd play.

Fast shader checker commit e816c67: `tools/shader-check` is standalone with own lockfile;
use README GNU command, target D:/gr-build/shader-check. Built executable reads current WESL at
runtime: `D:/gr-build/shader-check/release/gothic-shader-check.exe --repo D:/gr-wt/codex-renderer`.
8positive variants and exact field-access negative passed. Logs own `.local/shader-check-{build,run}.log`.
It checks WESL/Naga, not GPU bindings, vertex fetch or silhouettes; real smokes are still required.

Quest canonical apps/quest is deliberately excluded from root workspace (duplicate vendor packages).
Build standalone own `.local/qmine-codex/Cargo.toml`; keep staged src/examples hash-synchronized.
Use own `.local/vendor-codex`; `.local/vendor` links original and must never be edited.
Primary `.local/quest-env.sh`; host target primary `.local/target-hostq`, explicit GNU target;
Android target primary `.local/target-quest`. Own ardea-pack junction reads original pack.

## Next work

Latest package and all current gates passed. Logs: own .local/qa/wind-prepass5/{normal,nowind}.log,
bench-ready.json and .local/pc-package-wind-melee.log; final package runtime/.local/qa/melee_faithful
contains actual gameplay images. Independent package saves remain empty. Build-info records dirty
handoff documentation only; executable source matches edd3b55. Previous packages untouched.

Next validate the shared wind clock on the GPU in an actual runtime smoke: hour boundary, branch/leaf motion, current-frame depth/shadow, and previous-frame motion vectors. Confirm the prepared GPU buffer identity remains stable and shader bindings are accepted. The shared frequency audit is complete: `WindEnv` supplies the same vegetation and SpeedTree rates to all wind materials; per-material values are mode, height, card size, and grass focus. Do not claim visual or gameplay parity from CPU/build checks alone. Do not mutate every material per frame or fork Bevy. Measured CPU bottleneck/performance hypotheses are
in PERFORMANCE_ACTIONS.md. Preserve quality and compare matched routes, not aggregate old walk data.
No new APK until local SDK prerequisites are provided/prepared; no headless result proves on-headset UX.
Continue through existing heartbeat with scoped cheap coders and coordinated builds. Current release
is a tested milestone, not completion of the full port or all renderer/VR goals.
Active next-task agent: `/root/wind_clock` (gpt-6-luna), started13:34. It confirmed vegetation,
tree and leaf rates come from the shared WindEnv packet and is implementing the bounded shared
clock. No full game release build is authorized until manager coordination; targeted tests/checker
are allowed. Do not duplicate its source changes/builds. User-facing Desktop release above remains
frozen at edd3b55. The next clock work is not included in that EXE.
## Latest clock checkpoint — 13:49

Clock source is frozen pending actual GPU validation. Pure environment clock module now stores
bounded f64 phases and f64 elapsed gust timing, retains legacy per-slot hash values, and copies
the exact last emitted sample into previous-frame data even across configuration changes.
Shared64byte ShaderBuffer uploads once per frame; static/zero-weight vertices return before
storage reads. Environment release28/28, viewer cargo-check and shader checker8variants PASS.
CPU tests inspect encoded f32 sin/cos for all5oscillators at hour/day boundaries, rate changes,
previous-record preservation and nonzero legacy gust samples near actual day-adjacent slots.
Logs own .local/wind-clock-{environment-tests,viewer-check,shader-check}.log.

water_renderer now owns full .local/pc-wind-clock-build.log. No concurrent performance tests.
After build, manager must run both normal+GR_NOWIND GPU smokes, check stable buffer behavior
and measure unchanged scene before any new Desktop package. No visual hour/day GPU proof yet.
Do not claim numerical tests are a long-running GPU session. Current Desktop Wind release stays
unchanged. Initial broad cargo-fmt side effects were byte-restored for11unrelated files and
verified zero staged/content diff; only intentional clock source/manifests/docs remain dirty.
## Clock manager validation — 14:05

Thin release candidate built8m18s, SHA25658c7554a9b6f3b4f425371ad3706853f450bf89daafdd6ac19391cf3b950fde0.
Actual normal and GR_NOWIND --walk --smoke runs PASS with no shader/renderer/import errors,
and grounded360/360 in both tests. Manager inspected actual normal-walk.png. Evidence in
own .local/qa/wind-clock/{normal,nowind}.log and images. This validates current GPU bindings
and rendering path; no hour-long GPU session or direct buffer-identity instrumentation run.

Controlled idle A/B/A/B,1280x720,30s warmup+20s sample,2runs per variant, compilers quiet:
previous Wind release12.0353ms vs shared-clock candidate12.0263ms (-0.075%). p95medians
13.3983/13.0837ms;GPU3.4099/3.4224ms. Essentially equal; no speedup claim. Raw JSONs and
summary own .local/bench-wind-clock, manual-renderer-log-review=false.
Final package/play QA pending. Earlier Desktop releases remain unchanged.
## Clock/combat checkpoint — 2026-10-22 14:15 Berlin

Clock source13cead4 packaged separately at Desktop/GothicReforged-Codex-20261022-WindClock.
Initial arbitrary-cwd melee_faithful FAILED (own .local/pc-package-wind-clock-melee.log):
normal attack succeeded, held-left PowerAttack and later Pierce failed. Exit0 did not imply PASS.
Preserve that failure. Controlled replay with immutable wind-prepass5 and wind-clock artifacts,
same WindClock/runtime cwd and GR_RES1280x720, then passed BOTH variants. Logs own
.local/qa/clock-combat-compare-1/{baseline,clock}.log. No cause proven and no combat changes
made; input/focus/timing remains an investigation, not an established clock regression.
GR_INPUT_DEBUG emits mapped edges plus grabbed/blocked, but cannot expose every filtered raw
mouse event. Next debug replay must wait for ongoing targeted compiler work to finish.
Current verified user-facing package remains Desktop/GothicReforged-Codex-20261022-Wind.
Clock folder is a candidate; do not claim robust packaged combat validation from a retry alone.

Uncommitted grass-focus prototype owned by wind_stability: 80-byte shared frame buffer stores
focus in fifth vec4, no per-material mutation/iteration; pure environment clock API stays16 floats.
No release artifact contains it yet. Agent now runs fast shader checker and viewer cargo-check;
no full release build or runtime authorized concurrently. Verify live material count in a separate
functional GR_WINDLOG run and matched moving workload A/B before any performance claim.

## Active focus build — 2026-10-22 14:20 Berlin

Source frozen/committed93655de. Shader checker8variants+negative and viewer cargo-check PASS,
logs .local/grass-focus-{shader-check,viewer-check}.log. Separate baseline functional moving run
with GR_WINDLOG observes72 registered wind materials (not vegetation-only mutation count), log
.local/grass-focus-count-baseline.log; logging timings explicitly excluded from benchmarks.
Debug melee comparison with packaged EXEs from C:/Windows/Temp passed baseline AND clock;
logs .local/qa/clock-combat-debug-1. Initial clock hold failure remains unresolved/nonreproduced.

Agent /root/wind_stability owns full release build session79843, .local/pc-grass-focus-build.log.
Authorized sequential pipeline: immutable .local/artifacts/gothic-reforged-grass-focus.exe;
normal+GR_NOWIND actual smokes in .local/qa/grass-focus (no fatal logs, grounded360/360);
then scripts/bench-renderer.ps1 against immutable wind-clock artifact, same WindClock/runtime,
-Walk -WarmupSeconds30 -Seconds20 -Repetitions2 -Resolution1280x720, .local/bench-grass-focus-walk.
Clear diagnostic/quality GR_* flags. No duplicate compiler/runtime jobs. Stop on smoke failure.
Root must inspect screenshots and route/count metadata before conclusions or packaging. No
new focus package delivered. Current verified release still Desktop/...-Wind, e638e428...
Quota78%5h used at14:17; reset17:22 Berlin. Conserve manager analysis and redundant polling.

## Grass GPU checkpoint — 2026-10-22 14:31 Berlin

Optimized source93655de built in6m57s. Immutable artifact own
.local/artifacts/gothic-reforged-grass-focus.exe SHA256
b11537c354934be159de02849122dab43ac8fd57ad2466df518fb5a8f341f972.
Normal+GR_NOWIND actual smokes PASS, grounded360/360, no shader/import/renderer errors.
Own .local/qa/grass-focus/{normal,nowind}.log and {normal,nowind}-walk.png;
manager inspected actual normal screenshot. Static image cannot prove temporal jitter absence.
Agent wind_stability now owns four-run moving A/B .local/bench-grass-focus-walk. No concurrent
build or runtime; inspect its session/result before new jobs. No focus Desktop package yet.

## Benchmark harness correction — 2026-10-22

First .local/bench-grass-focus-walk run aborted after a completed baseline because Start-Process
ExitCode was null. Preserve partial output; no comparison inferred. Commit35879f9 switches to
Start-Process -Wait -PassThru and explicitly rejects unconfirmed/null status and nonzero codes.
Agent verified synthetic child exit0 and exit7 plus PowerShell parser; renderer log guards retained.
wind_stability now owns a fresh COMPLETE A/B/A/B .local/bench-grass-focus-walk-2 with its own
-run.log, same clock/focus artifacts and WindClock/runtime, 1280x720, warmup30/sample20/-Walk.
No compilers or other game jobs. After result inspect actual route geometry/counts before claims.

## Grass comparison result — 2026-10-22 14:44 Berlin

Fresh A/B/A/B completed cleanly, .local/bench-grass-focus-walk-2/summary.json.
Actual runtimeRoot in summary is Desktop/GothicReforged-Codex-20261022-Wind/runtime (both variants),
not WindClock/runtime as prescribed in earlier planning notes. Same1280x720, warmup30/sample20,
empty inherited GR_*; no compiler load; hashes match clock58c755... and focusb11537... .
Baseline/focus medians avg13.503/13.005ms (-3.69%), p9515.959/15.469ms, GPU3.984/3.805ms.
Identical route starts but endpoint horizontal difference~0.66m; final visible meshes1612 versus
1620-1623, entity endpoints12627. Descriptive only, NO isolated speedup proof. A more precisely
matched route or same-feature tracing is needed to attribute gains. All renderer log guards clean.
Next create distinct Desktop WindFocus package and run packaged melee_faithful before delivery;
retain initial clock QA failure and prior good releases. No focus package delivered at this note.

## Latest VERIFIED Desktop milestone — 2026-10-22 14:48 Berlin

NEW separate package Desktop/GothicReforged-Codex-20261022-WindFocus/GothicReforged.exe,
SHA256b11537c354934be159de02849122dab43ac8fd57ad2466df518fb5a8f341f972.
Game source93655de06d2ad1e22499d721ad7649e3a73de7a3; packaging metadata additionally records
d208696 HEAD, clean source; later commits are script/docs only. build-info has explicit executable
source commit and explanation. Normal+GR_NOWIND smokes PASS; final packaged melee_faithful PASS
from C:/Windows/Temp with GR_INPUT_DEBUG and1280x720. Manager inspected package's actual
runtime/.local/qa/melee_faithful/05_holstered.png. Independent saves empty. PRUEFSTAND.txt
records tested scope, unresolved first clock hold failure and descriptive-only timing caveat.
Old packages preserved. This package supersedes Wind as latest tested milestone, not full port.

No build/runtime currently running after session38381 completed. wind_stability completed;
water_renderer has read-only precise route/trace proposal pending, no source edit permission.
Next: matched benchmark trajectory or same-feature trace to establish causal CPU/material benefit;
current route differs0.66m, so3.69% lower descriptive timings are NOT proven optimization.
Hour/day encoded CPU tests passed but no long-duration GPU session. VR native/head fixture previous
work preserved; no new APK/headset validation. Preserve first clock failure logs and initial
benchmark-null-exit partial output. Existing heartbeat remains active; do not duplicate automation.

## Next performance experiment proposal (read-only review; not implemented)

Existing --bench-walk injects W/S every5s but integrates frame-dependent collision physics;
streaming observes previous pose, then motion, camera, capture. Do not force-match these timings.
Proposed separate benchmark-only camera route: after readiness and warmup, capture anchor/yaw;
compute positions analytically from elapsed time on a small closed path within a resident tile.
Preserve ordinary play and existing walk route. A fly/kinematic camera hides hero/avoids physics,
so label it renderer/focus isolation, not whole-game movement parity. Direct ctrl.position still
incurs physics and needs careful system order with focus/camera capture. Verify trajectory AND
streaming order, not just equal endpoints. Suggested comparability tolerances (provisional, not
validated acceptance rules): start1cm/0.001rad, 1Hz poses5cm/0.005rad, same entities, visible count
within1%. Resolve sampling timestamp differences against analytical expected position.
Alternative: same-feature trace builds both variants with --trace; existing spans are inclusive
CPU. Current executables lack trace feature and bench script does not pass --trace. No trace
profiles yet. Never equate fewer source writes or descriptive3.69% with measured causal speedup.

## Continuation checkpoint — actual clock 2026-10-22 14:38 Berlin

Previous headings14:44/14:48 were estimated labels and are incorrect wall-clock times; sequence
and actual tool/git/build-info timestamps take precedence. Verified WindFocus package remains
latest and unchanged. No compiler/runtime active at entry, branch clean.

Agent water_renderer now owns benchmark-only deterministic camera route implementation in
perf.rs/walk.rs/bench-renderer.ps1 plus narrow tests/docs, no full build/runtime authorized yet.
Flag --bench-camera (finite --bench required; reject --bench-walk combination), label explicitly
renderer/focus isolation, do not change normal gameplay/quality. Expected vsactual timestamped
pose metadata must account for sample jitter. Existing immutable old clock executable does NOT
support this new flag, so cannot be used as route baseline. Manager must review matched-baseline
strategy before any new A/B. No code overlays/checkout/restore authorized to agents.

## Camera workload source checkpoint — 2026-10-22 (see git/tool UTC for exact time)

Committed3c8982997ad435252e19a0d10bf791e5826721b6: explicit finite --bench-camera route,
-Camera script switch mutually exclusive with-Walk, 2m radius/4m max displacement/10s loop.
Camera-only fly route hides hero, skips player physics. Ready+warmup gate, anchored actualcamera,
fixed rotation, requested-vs-applied trajectory timestamps and expected/actual sample metadata.
Normal focus schedule retained via mutually exclusive conditions; camera focus follows pose chain.
Review caught/fixed YXZ tuple extraction and new-time/old-pose snapshot mismatch before commit.
GNU release check passed (.local/bench-camera-check-final.log). Exact contiguous source test
extraction linked matching cached GNU glam,4/4 math/flag tests passed; provenance/hash in
.local/camera-bench-extraction-provenance-final.txt, output camera-bench-exact-tests-run-p2.log.
No actual camera GPU run yet. Existing old binaries do NOT implement --bench-camera.

Agent wind_stability now owns sequential EXPERIMENT under new own.local source snapshot from
3c89829 (no active checkout overlays). Audit dependency paths first; no unreviewed vendor links.
Build immutable camera-focus, then short finite actual camera functional run in existing Wind/runtime
and check metadata expected/actual residuals/render logs. Only after functional pass, reverse apply
five-codefile focus delta13cead4..93655de inside snapshot with check+hash manifests; preserve camera
instrumentation/scheduling. On conflicts STOP, no force. Build camera-oldfocus same sourcepath/target.
No full A/B authorized until manager reviews. No packages or original/foreign source mutation.
Inspect agent/session before any new jobs. Current delivered WindFocus b11537... remains verified.

## Camera functional GPU proof — 2026-10-22 (tool clock exact)

Snapshot3c89829 extracted874 trackedfiles, manifest .local/camera-bench-snapshot-manifest.sha256;
all internal path dependencies resolved, no vendor links needed. Focus snapshot build session10346
completed exit0, immutable camera-focus.exe SHA256
3dc6270f00a03e2e0e16746aa975abb52f57be426045bb2894bfc1de80b793b1.
Log .local/camera-focus-snapshot-build.log. Actual --play --bench-camera --bench4 --bench-warmup2
from Wind/runtime completed, no render/shader/import errors. Camera flag true;4.0054s measured.
Route end actual/expected position+rotation equal;1Hz max position residual3.82e-6m, rotation0.
Log .local/qa/camera-focus-functional.log; runtime .local/bench/camera-focus-functional-1792674166.json.
Mode label no_physics means PLAYER/controller physics bypass only; NPC/simulation not proven off.
Agent wind_stability next performs checked reversefocus patch inside snapshot only and oldfocus
build, retaining source manifests and immutable artifacts. No full A/B yet, no package updates.
Current delivered WindFocus remains b11537...; active checkout sources never overlaid.

## Quota-safe continuation — 2026-10-22, actual entry after15:08 Berlin

Root5h quota98% used; reset17:22 Berlin. NO loss of source or package: delivered WindFocus
unchanged b11537..., own committed camera instrumentation3c89829. No active checkout overlays.
Agent wind_stability owns oldfocus snapshot build session93896, log
.local/camera-oldfocus-snapshot-build.log. Reverse patch inside disposable snapshot was verified
exactly five focus files, retaining new camera scheduling; old manifest hash8c9bcf894ae335316f0d01e8a6c7dbc99d625b412d49727c8f91e792bee99cd3.
Agent must persist exact snapshot path and perfile manifest comparison; perf.rs/walk.rs identical.

Conditional authorization given: once oldfocus build+manifest pass, short actual oldfocus camera
functional, same Wind/runtime/1280x720/no other GRflags, confirm renderer clean and pose residuals.
Then complete A/B/A/B -Camera, baselinecamera-oldfocus vs camera-focus, warmup30/sample20/reps2,
fresh .local/bench-grass-focus-camera. No concurrentcompiler/runtime. Agent appends exact
sessions/results/paths/limitations here AND PENDING. Stop/document failures, no blind retries.
No new packaging/source changes authorized. Compare anchor/rotation, analytic residuals and
entity/visible counts before conclusions; no unproven speedup. Manager reviews afterquotareset.
Existing heartbeat active; do not duplicate. Check agent/jobs first before anything new.
