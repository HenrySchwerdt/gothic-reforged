# Codex continuation — 2026-10-22, Europe/Berlin

## Preserve these sources

User authorized starting now, parallel cheaper coding models, renderer performance, water/shore
bugs, leaf jitter, VR repair, a double-click PC release and actual screenshots. Continue autonomously
and conserve tokens. The separate Claude sessions hit their weekly limit; their last background
host build completed. Do not change the original installation or overwrite older releases/saves.

**Current working source: `D:/gr-wt/codex-renderer`, branch `codex/renderer-stability`.**
Based on Claude's integrated `36fef36` (not the stale primary checkout). Claude's two dirty QA
files were preserved/applied. The latest door QA says PASS but routes confirmation directly to
the door after a nearby book steals focus; that is not evidence that the focus bug is fixed.

Primary checkout `C:/Users/PowerTower/dev/gothic-reforged` and Claude's `D:/gr-wt/integrate`
were not edited. Generated evidence/checkpoints under the primary `.local/` are intentional.
Recovery files: primary `.local/checkpoints/codex-resume-20261022-0720/` contains
`root-source-at-takeover.zip`, `quest-vendor-at-takeover.zip`, `integration-at-start.patch`, and
`PENDING_CONTINUATION.md`. Keep all of them. Root Quest sources and vendored VR sources were
copied into this checkout; source assets stay local/ignored.

## Changes and limits

- Wind: fixed-rate branch and leaf phase on CPU/GPU; gust amplitude retained. Actual-offset
  finite-difference regression samples gust boundaries, including long CPU reference times.
  GPU Globals wrap after one hour; do not claim multi-hour GPU continuity. See WIND_STABILITY.md.
- Water: source identity retained for water parts only. Confirmed planar Oceanmesh resources
  establish sea height from transformed vertices. Other water surfaces remain independent.
  DepthPrepass/Reverse-Z scene-depth shoreline fade added. See WATER_RENDERER_AUDIT.md.
- **Open renderer limitation:** wind color vertices still disagree with static shadow/depth
  prepass geometry. Depth-only variants lack normals required by leaf flutter; a safe follow-up
  needs pass-aware vertex layout and a shared wind vertex path. Do not claim this is fixed.
- Quest: grip/aim action activation + complete pose validity, session-focus input gating and
  last-valid inactive pose cache. Pointer requires active aim. Generic gate requires
  `openxr::ActionInput`. See VR_RENDERER_AUDIT.md for verified Meta sources.
- Avatar diagnostic now exits nonzero for failed geometry checks. Finger-axis frame alignment
  is measured with open fingers; curl-angle change remains printed separately. Claude's actual
  palm offset, weapon stabilizer and orc implementation were retained. New host validation pending.
- PC game no-argument launch now selects play mode. Optional `runtime-root.txt` beside EXE
  resolves relative to EXE directory and establishes cwd before assets/saves are accessed.

## Validation so far

Environment/formats/render-rule release tests: **146 passed, 1 ignored**; XR tests: **3 passed**.
The first thin PC release built and rendered; its independent runtime package launched into
play with no arguments from Windows/Temp and closed gracefully with exit 0, with zero saves.
Preview only: `D:/gr-releases/GothicReforged-Validation-20261022`; do not mistake it for final.
Actual coast screenshots showed the depth-fade alone left the faceted defect unresolved.
The subsequent typed Water shader/depth-proxy fix is in the current rebuild. Textureless Sea
materials are a counterexample: manager fixed their classification to remain non-solid water.
See the water audit's manager section. Current central PC build session began 08:36; inspect
`.local/pc-release-build.log` for actual success before copying its output.

The two new Quest combat bridge tests now PASS: synthetic fixture registers `sim.hero`, and
NPC hits assign the hit-confirmed victim as the script target. They cover both hit directions
and repeated HP deltas. Host visual examples and Android compilation remain separate gates;
agent `vr_audit` owns the Android build. Do not duplicate its jobs.

At 08:45, the rebuilt PC candidate rendered the matched coast route with no shader errors.
Primary `.local/qa/codex-coast-fixed-{low,shore}.png` show the gray/cyan depth-hull triangles
and bright shoreline strip gone; compare earlier `codex-coast-after-{low,shore}.png`.
Walking regression: grounded 360/360 frames. Timing from these smoke runs overlaps compilers
and is not a performance comparison. The native notes explicitly leave original pass selection
unproven; this implementation uses scene depth instead of the observed problematic proxy family.
New binary `.local/artifacts/gothic-reforged-water2.exe` is the verified water candidate.

Baseline original Claude PC binary is preserved at `.local/artifacts/gothic-reforged-baseline.exe`.
At 1280x720, play, GPU diagnostics, 30 s warm-up + 20 s sample: avg 12.905 ms, p50 12.382,
p95 16.734, p99 18.124, GPU avg 3.442 ms, 12,765 entities / 1,554 visible meshes. Host VR
compilation overlapped part of baseline run, so rerun interleaved when all builds stop before
claiming a performance gain. Primary `.local/bench/codex-baseline-1792648427.json` holds raw data.

Primary `.local/qa/codex-coast-before-*.png` are actual baseline coast captures, not all useful
views. The `in`/`low` views expose faceted water/seabed artifacts; first failed camera choices
in `codex-baseline-coast/shore.png` show mostly cliffs. Do not present those as successful QA.
WorldIndex and raw Ocean068/depth118 mesh evidence: primary `.local/research/codex-*`.

## Builds and next actions

PC: Git Bash `source /c/Users/PowerTower/dev/gothic-reforged/.local/env.sh`, own cwd, set
`CARGO_TARGET_DIR=D:/gr-build/integrate RUST_MIN_STACK=67108864`; build
`cargo +stable-x86_64-pc-windows-gnu build --release -j4 -p gothic-reforged --features viewer --locked`.
Current release is thin LTO/codegen1. Preserve GNU dlltool adapter; default MSVC cannot link.
Logs in this checkout `.local/environment-tests.log`, `pc-release-build.log`, `xr-tests.log`,
`quest-host-build.log`, `vr-hero-baseline.log`. Never run simultaneous performance measurements
with cargo/rustc or other game instances. Keep screenshots and raw measurements.

Quest source is `apps/quest`, but its vendor dependencies duplicate root VR package names.
Do not blindly add it as a root workspace member. Build the isolated staging package
`.local/qmine-codex/Cargo.toml` (own `[workspace]`, explicit metadata/deps) from copied sources,
using `.local/vendor-codex`; never edit `.local/vendor` which links to the original checkout.
Refresh staged src/examples after editing canonical `apps/quest` source. Host uses original
`.local/quest-env.sh`, override target dir to primary `.local/target-hostq`, explicit
`--target x86_64-pc-windows-gnu`. Own `.local/quest/ardea-pack` links to original read-only
pack; own host screenshot outputs go beside it. Headset Quest3 serial 2G0YC1ZG1003WN was
connected; black screenshot/empty app log cannot establish an app crash. No deployment yet.

Next: finish builds/tests, shader runtime smoke and coastline comparison; fix any real
failures before packaging. Run new hero_check and host_shots. Benchmark warmed scenarios
interleaved against baseline without build load. Package under a **new** Desktop directory,
with independent runtime metadata and empty saves; do not overwrite Claude's Desktop package.
Update this document and audit test sections to reflect actual results. Commit isolated code
and keep original caches/backups. Continue larger renderer/VR issues through existing heartbeat
`claude-abwarten-dann-gothic-pc-build-fortsetzen` (now renamed for renderer/VR/release); no duplicate.
