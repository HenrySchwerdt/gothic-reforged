# Gothic Reforged — Codex handoff, 2026-10-22

## Current action

Work only in `D:/gr-wt/codex-renderer`, branch `codex/renderer-stability`.
At 13:33, build5 passed both actual normal/GR_NOWIND smokes without shader or renderer errors,
with grounded360/360 each. Packaged gameplay melee_faithful QA passed from Windows/Temp.
The functional moving benchmark recorded readiness, poses and13one-second samples correctly.
No active build/game job remains. Wind source commitedd3b55 is now verified for these paths.
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
Final GPU validation of build5 is pending. WIND_STABILITY.md documents exact scope/limitations.
Hourly continuity is still open: proposed shared ShaderBuffer with bounded CPU-f64 phases/gust,
once-per-frame RenderQueue write, no per-material asset mutation or engine fork. Not implemented.

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
not deterministic physics. Runtime metadata smoke pending after build5. Benchmark script ebe9738
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

Next implement shared bounded-phase WindClock via one stable ShaderBuffer, using documented design.
Confirm vegetation frequencies are world-global or encode appropriate groups; do not assume all
per-material values match. Preserve source-derived wind behavior, test hour/day transitions and
same-frame shadow/depth consumption; validate with checker then actual GPU. Do not mutate every
material per frame or fork Bevy unnecessarily. Measured CPU bottleneck/performance hypotheses are
in PERFORMANCE_ACTIONS.md. Preserve quality and compare matched routes, not aggregate old walk data.
No new APK until local SDK prerequisites are provided/prepared; no headless result proves on-headset UX.
Continue through existing heartbeat with scoped cheap coders and coordinated builds. Current release
is a tested milestone, not completion of the full port or all renderer/VR goals.