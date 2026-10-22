# Codex renderer and VR handoff — 2026-10-22

## Preserve this state

Worktree D:/gr-wt/codex-renderer, branch codex/renderer-stability. Implementation commits
434ee3a and 96fd98d preserve Claude integration 36fef36, its dirty QA and previously untracked
Quest sources. Do not reset primary C:/Users/PowerTower/dev/gothic-reforged or other D:/gr-wt
checkouts. Original D:/SteamLibrary/steamapps/common/Gothic 3 remains read-only. Primary
.local/checkpoints/codex-resume-20261022-0720 contains source/vendor ZIP backups and patches.
User authorizes autonomous manager work with cheaper gpt-6-luna coders. Existing heartbeat
claude-abwarten-dann-gothic-pc-build-fortsetzen continues this task; do not create a duplicate.

## Delivered PC milestone

Desktop/GothicReforged-Codex-20261022/GothicReforged.exe under C:/Users/PowerTower is an
optimized thin-LTO release (codegen1), SHA256
99bc33d3e2a32573711ee919b51d05fa35502f99706bf8677647019738bd05d5.
No arguments selects play. Adjacent runtime-root.txt resolves runtime relative to the EXE,
before asset/save access. Package runtime contains required JSON and independent empty saves;
original installed archives/global cooked cache are still required. Claude's older Desktop
package remains untouched. scripts/package-pc.ps1 creates only new output directories and
records source revision/hash; sourceCodeRoot must point here, not the primary runtime root.

Actual final package launched from Windows/Temp; --play --qa melee_faithful exited 0 with
RESULT PASS. Log .local/pc-package-melee.log; screenshots in final package
runtime/.local/qa/melee_faithful. Earlier no-argument preview launch also loaded play and
closed gracefully with exit0, creating no saves. D:/gr-releases/GothicReforged-Validation-20261022
is an EARLIER PREVIEW, not the final corrected release.

## Fixes and evidence

Water: bounded framed eCShaderWater recognition; source metadata only on water parts;
confirmed planar Oceanmesh vertices establish sea height. DepthPrepass Reverse-Z shoreline
fade excludes water itself. Exact untextured G3_World_Rivermesh_01_Depth_<digits> proxy family
no longer renders opaque fallback triangles. Regular textureless Sea is still water/non-solid;
do not broadly suppress textureless materials. Original native pass selection is not proven.
See WATER_RENDERER_AUDIT.md and WATER_NATIVE_NOTES.md. GR_HIDE=water supports visual ablation.

Actual matched coast screenshots in primary .local/qa/codex-coast-fixed-{low,shore}.png show
the faceted triangles and bright strip gone. Compare codex-coast-before-low.png (Claude) or
codex-coast-after-shore.png (failed depthfade-only candidate). Walk grounded360/360, no shader
errors. Smoke timing overlapped compilers and is NOT a performance comparison.

Wind: fixed oscillator frequency from base strength; gust changes amplitude. CPU actual-offset
regression covers gust boundaries including stress times. GPU Globals wraps hourly; this is
not multi-hour GPU continuity evidence. Color wind and static prepass/shadow silhouettes still
disagree; needs shared wind vertex path plus normal layout in depth-only variants. Hour-wrap
phase continuity is also open. See WIND_STABILITY.md.

XR: focus/action activation and complete pose validity gate input; preserve inactive last-valid
pose, neutralize scalars/buttons/sticks, pointer requires active aim. Quest combat uses actual
Sim::resolve_ported, syncs actor HP, registers hero and assigns NPC target to confirmed victim.
Claude palm offset, orcs and weapon stabilizer preserved. Geometry diagnostic exits nonzero
on failures and checks finger frame with open fingers, printing curl angle separately.

Tests: environment24 + formats100 unit/8integration + render-rules14 =146passed/1ignored;
XR3passed; Quest combat bridge2passed. These are targeted suites, not full workspace tests.
Fresh host hero_check exited0: grip/palm/blade/rotation sweep checks PASS. Detailed log
.local/vr-hero-final.log. Host screenshots build/run still pending at this checkpoint.

Android native release succeeded: primary .local/target-quest/aarch64-linux-android/release/
libgothic_quest.so, ELF64/AArch64,104687840bytes,SHA256
3b720252047aa76d1b319ce02b4fabb467caa5c96e60fd5204eedc0576a58968.
7warnings/0errors. No new APK: bounded local SDK searches found no aapt2/zipalign/apksigner/
android.jar. No device install or launch; old Quest3 package remains. Actual worn-headset
validation is pending. See VR_RENDERER_AUDIT.md for current Meta sources and build details.

## Builds and next steps

PC Git Bash source primary .local/env.sh; own cwd; CARGO_TARGET_DIR=D:/gr-build/integrate,
RUST_MIN_STACK=67108864; stable-x86_64-pc-windows-gnu build --release -j4 -p gothic-reforged
--features viewer --locked. Preserve GNU dlltool adapter. Current final .local/artifacts/
gothic-reforged-water2.exe matches Desktop release. Original Claude binary is baseline.exe
(with full prefix gothic-reforged-). Logs .local/pc-release-build.log and *tests.log.

Quest canonical apps/quest uses isolated .local/vendor-codex. .local/vendor links to original;
never edit it. apps/quest is deliberately not root workspace member (duplicate VR packages).
Build standalone .local/qmine-codex/Cargo.toml; keep staged src/examples synchronized with
canonical source. Root Quest vendor backup is in checkpoint ZIP. Own .local/quest/ardea-pack
links read-only original pack. Host source primary .local/quest-env.sh; CARGO_TARGET_DIR primary
.local/target-hostq; explicit --target x86_64-pc-windows-gnu. Android target dir .local/target-quest.

Current host build session22985 builds hero_check/host_shots; hero_check finished/tested,
host_shots linker still active at08:56. Once done run host_shots from own cwd, inspect PNGs.
Only then run scripts/bench-renderer.ps1 with baseline and water2 artifacts, final Desktop
package/runtime, new own .local/bench-final output, warmup30/sample20/repetitions2/1280x720.
No rustc/cargo/game may overlap benchmarks. Script is parser checked, actual run pending.
Update this handoff with real results. PERFORMANCE_ACTIONS.md ranks measured shadow-bin cost
and material-update fanout hypotheses; no unmeasured speedup claims. Old per-tile merging
was slower due to culling. Preserve quality in comparisons.
## Final validation checkpoint 09:02

Host build and host_shots completed with exit0. Five real PNGs under own .local/quest.
FPV grip screenshot was inspected. Third-person fixture hides the head because its camera
uses only layer0; head is layer1 intentionally for eye hiding. Runtime mirror includes0+1.
This fixture issue is documented, not evidence of missing assets; no headset validation yet.

Controlled idle A/B/A/B completed with no compilers:1280x720,30s warmup/20s measurement,
2runs per variant, same final-package runtime, no inherited GR_* variables. Run-average
median baseline12.0194ms versus candidate12.0713ms (+0.43%); p95 medians13.4306 versus
13.1976ms; GPU3.386 versus3.413ms. This is essentially comparable in this small sample,
not a speedup claim. Raw files/summary own .local/bench-final. Moving-player A/B/A/B is
completed in session56825 (see below and .local/bench-final-walk);
never overlap compilers. Candidate hash matches delivered Desktop EXE.
## Moving benchmark follow-up 09:07

A/B/A/B walking completed: own .local/bench-final-walk/summary.json. Run-average medians
12.7658ms baseline /13.9277ms candidate (+9.10%), p95medians17.4631/16.8876ms. Do NOT
interpret this as a measured steady-route regression or speedup: baseline individual runs
14.0745/11.4571ms, candidate13.6486/14.2069ms, with different end visible-mesh counts
(baseline1694/1883; candidate1851/...). bench_walk keys use process wallclock halfway through
warmup with no world-ready/start-position synchronization. Loading/frame/collision differences
may change route; metadata lacks position traces. Idle matched scene comparison above is the
stronger evidence. Need deterministic-ready route instrumentation before movement perf claims.

water_renderer (gpt-6-luna) now owns ONLY perf.rs benchmark readiness/route metadata improvement;
no shader/gameplay changes, no compiler launches yet. wind_stability next-design request failed
because selected model at capacity; no code changes from that request. Other agents completed.
The Desktop release remains previously validated commit96fd98d binary; future benchmark-only
source changes require a later build and must not be claimed included in that EXE.