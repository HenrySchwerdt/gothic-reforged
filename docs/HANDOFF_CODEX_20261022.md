# Gothic Reforged — current Codex handoff

## USER PRIORITY CORRECTION — gameplay and PR integration

On2026-10-23 the user explicitly rejected further leaf/cosmetic work and reported:
Digit1 does not draw the sword, gray model rims, overly broad NPC/quest dialog and
teaching offers, missing quests and original-style game startup/loading. Stop leaf
work. Previous smoke results do not establish playability. The user expressly
authorized merging the existing PRs; remote integration is now in scope.

GitHub merged: PR74=30b0cd3e59a2ef8d55c5e159e5c145033c55e27c,
PR72=5ad6505f8d6e46f4a87bdc2239ec93e5d400e72e,
PR73=3746f941c32a33bc9f2d559a65c24c5ac6e4f3cd,
PR71=5110fefbbea7682cb9b5c15b570c9fea45868642,
PR64=68e500948213ef7564d217a32c4f9cbe9bccf9bb.
Remaining PR62/63/65/66 have conflicts; integrate deliberately while preserving
local fixes and Claude work. These remote merges are not in the delivered EXE yet.

Local in-progress changes: water_renderer disabled unconditional actor rim light
in gmat.rs (source evidence original rim was permutation-gated). wind_clock owns
six gameplay/sim/runtime files for canonical Teach subject membership, stale-choice
revalidation, and save-load canonical-subject rehydration. Do NOT seed TeachEnabled
from legacy npcs.json's lossy false values. wind_stability owns a new default-combat
Digit1 QA path; existing melee QA uses Space, so the reported key was not covered.
The default kit does assign sword to slot1; no guessed input fix has been made.
Package README wrongly says Space jumps (actual default Space weapon, Alt jump).
No new build/test/runtime job yet; coordinate ONE validation/build after source freeze.
Keep root, foreign worktrees, saves and original installation intact. Heartbeat prompt
was updated to gameplay-first; no further leaf captures/builds are authorized.

## Completed flutter-only probe

Root reviewed the new default-1 `GR_LEAF_FLUTTER` diagnostic: finite0..4, cached
on first tree-leaf material creation; invalid/unset=>1. It scales only material
wind_mat.z for TreeLeaves; no shader, geometry, branch-sway or clock changes.
Private source5f69e5a built6m55s, root session35688 CLOSED actualexit0; log
`.local/pc-leaf-flutter-probe-build.log`. Immutable EXE SHA
`65fb8bb5fb05f4d2413d381abc3132d6e4788ce6f3af9beda9c7e099355644cd` under
`.local/leaf-flutter-probe/artifacts/`. ONE same-EXE pair completed at the aspen
camera, synthetic3600/3600.125/3600.25; default GR_RES1280x720 only, candidate
adds GR_LEAF_FLUTTER0. No UV flip/NOWIND, benchmark, combat QA or packaging.
Both exit0/grounded360/360; selected/captured CPU wind records match within/between
variants, no fatal renderer markers, own runtime saves0. Root reviewed all six
images; subtle visual differences do not establish a jitter fix. Default remains1.
Full evidence/limits: `docs/research/LEAF_PHASE_COHERENCE.md` and ignored
`.local/leaf-flutter-probe/comparison/summary.json`. All own jobs finished.
Do not repeat build/captures. Next jitter work needs a temporal measurement that
separates wind displacement, alpha sampling and unrelated scene animation.

## Latest APK packaging audit

Read-only follow-up found the old APK script also selects the wrong workspace and
requires a missing local env file. Sourcing the primary env would redirect root and
outputs into primary. Exact SDK inputs remain absent. See
`docs/research/QUEST_APK_PREREQUISITES.md` for evidence and isolated future inputs.
No install/configuration, native rebuild, APK/signing, or device operation occurred.
Do not repeat this audit/build while inputs remain unchanged. No own jobs pending.
Added `scripts/quest/check-apk-inputs.ps1`: read-only file/type checks plus staged
manifest SHA equality; JSON and exit0/2, no tools executed. Synthetic matching,
missing-file, mismatched-manifest and drive-relative-path cases passed. Root real
explicit-input check returned child exit2, manifest match, missing SDK + four SDK
files only. Logs under `.local/quest-apk-preflight/`; see prerequisites note for
limits. File-input completeness is not build/tool/ABI/APK/headset validation.

## Latest reporting correction

Source-only smoke log correction: `WALK TEST` now reports the actual sampled-frame
count instead of the false literal "in 6 s". Stage2 still collects360 post-motion
samples, spanning359 endpoint intervals; movement, timestep and stage transitions
are unchanged. Root reviewed the format arguments and diff; `git diff --check`
passed. No build/test/game run was needed for this reporting-only edit. Delivered
EXEs retain their old wording. See `docs/research/PERFORMANCE_ACTIONS.md`.
No own agent/compiler/game job is pending; do not rebuild just for this log text.

## Active leaf diagnostic

The delivered RenderPerf package below remains unchanged. Source `53f16f5` adds
default-OFF `GR_SPT_FLIP_LEAF_RENDER_V=1`: only generated leaf UVs flip after
calibration; fronds, billboard, geometry and other shading inputs stay unchanged. Public
mixed frond/leaf generation invariants passed with the formats suite (101 passed,
1 ignored), `.local/spt-leaf-render-v-tests.log`. This is not a native convention fix.
Root GNU viewer release build2300 exited0,9m33s; log
`.local/pc-leaf-uv-only-probe-build.log`. New private immutable artifact SHA
`d4b0c1d417690a759c5cd672d984fd8ca0ff51275c935d2d6457f8e672f757a1` is under
`.local/leaf-uv-only-probe/`. First startup rejected GR_NOWIND combined with
--wind-shot-times before images; failed logs preserved under comparison/default.
The valid pair completed under `.local/leaf-uv-only-probe/comparison-fixed-wind/`:
matching synthetic wind times, GR_RES1280x720 only for baseline, candidate adds
GR_SPT_FLIP_LEAF_RENDER_V1; no NOWIND or old atlas-wide flag. Both exited0,
grounded360/360, with matching CPU selected/captured wind records. Root reviewed all
six requested images: apple and aspen views look denser, while the oak-labeled view
changes ambiguously and some crowns look thinner. Nearby historical loader records
are not rendered-object identity proof. Keep BOTH V-flip switches default-OFF.
See `docs/research/SPT_ATLAS_DIAGNOSIS.md` for protocol and evidence limits.
No benchmark, desktop package change, jitter claim or native-fidelity conclusion.
REA objdump was unavailable on Windows; bounded local LLVM disassembly and one
successful Ghidra analysis of a NEW 4608-byte synthetic carrier are retained. The
unchanged full Engine.dll was not reimported. Output-field semantics remain unproven;
one final 256-byte literal read identified a diagnostic string, not a vertex table.
Native provider closed; own agents finished, no compiler/game/native job pending.
Do not repeat these captures/build/native reads. A future fidelity change needs
independent intended-sprite/corner-association evidence, not density alone.

Updated 2026-10-23 04:51 UTC. The complete previous handoff was preserved byte-for-byte at
[`docs/research/HANDOFF_CODEX_HISTORY_PRE_RENDERPERF.md`](research/HANDOFF_CODEX_HISTORY_PRE_RENDERPERF.md)
(SHA-256 `e7d85d3c96a505ffb3ec0d76b08127078aa7a3a18aaa0c7249339b9be12b088f`). It contains stale
“RUNNING” checkpoints and abandoned proposals; use it only as historical evidence, never resume
those jobs or follow its old pending-build instructions.

## Validated Windows package

The validated package is
`C:\Users\PowerTower\Desktop\GothicReforged-Codex-20261023-RenderPerf\`.
Its `GothicReforged.exe` SHA-256 is
`8af2257ce156e49a4ac6486a5b32a6d22753830e6c71e8c2382bc5c8d14c6823`.
It was compiled from source commit `cb909ff3494e29f41533930190dd7fd9cbc46739` by the GNU
release build in `.local/pc-direct-render-release-build.log` (8m17s), then packaged from clean
HEAD `3716a5552c80928bd89ef633d8ff9809f9b71804`. `build-info.json` records the exe hash, clean
source state, provenance and validation status. The package runtime has its own empty `saves`
directory; no old saves were copied.

The packaged executable passed melee 13/13, magic 9/9 and bow 15/15 assertions. Melee selected
its intended target, unrelated active actors were zero, and the hero was not down. The fixed smoke
finished with 360/360 grounded frames. Root reviewed both final smoke images and approved them.
Evidence: `.local/no-indirect-release/qa-package/`,
`.local/no-indirect-release/qa-package-smoke/`, and
`.local/no-indirect-release/bench/summary.json`. The two reviewed fixed smoke shots used engine
times 3600.000000000 s and 3600.016666667 s. Package instructions and limits are in
`PRUEFSTAND.txt` and `LIESMICH.txt`. These are synthetic wind times, not Bevy simulation-time
jumps or GPU readbacks. This is a validated technical foundation, not a complete or
playable Gothic 3 reimplementation.

## Rendering result and limits

Windows now defaults to `GR_NO_INDIRECT_DRAWING` absent, which selects the no-indirect-drawing
path. Set `GR_NO_INDIRECT_DRAWING=0` for the reference path or `=1` for the candidate path.
Other platforms’ defaults were not changed. The release A/B used the same EXE hash above, an
interleaved A/B/A/B order, 1280x720, 30 s settled-world warmup and 20 s per run, twice per path.
It used the analytical 10 s-period, 2 m-radius camera-only route, with no hero or player physics;
both variants recorded 12,752 entities and 1,541–1,546 visible meshes at route boundaries, with
about 35 μm of anchor variation.

The two-run median average frame time was 10.0434 ms reference versus 5.4898 ms candidate
(-45.34%); p95 was 10.7174 versus 5.9794 ms, and whole-frame GPU average 2.9299 versus
1.1016 ms. These results describe this fixed scene and small sample only, not general FPS,
walking performance, other hardware, or VR. QA and image review passed for this package.
Detailed methodology and diagnostic trace caveats are in
[`docs/research/PERFORMANCE_ACTIONS.md`](research/PERFORMANCE_ACTIONS.md).

Trace-only numbers are attribution evidence, not release timings. The earlier trace showed
`Queue::write_buffer` 1125.8 to 18 calls/frame; default shadow-bin and preprocess-bind-group
spans were 3.259 and 3.095 ms/frame, absent from candidate top 40. That absence does not prove
zero cost. The trace build measured 15.672/6.535 ms average across variants and is not a valid
shipping comparison. Fixed-view images were reviewed with no obvious geometry or shadow loss;
small pixel differences remained. A separate smoke-walk covered 1642 cm versus 890 cm and is
not parity evidence because its elapsed simulation time differed.

## Working rules and reproducible commands

Continue only in `D:\gr-wt\codex-renderer`, branch `codex/renderer-stability`. This handoff
supersedes older primary-checkout and ignored-file “pending” pointers; treat them as read-only.
Do not edit the primary checkout `C:\Users\PowerTower\dev\gothic-reforged`, other agents’
worktrees, delivered Desktop packages, or Cloud/Claude branches. Treat
`D:\SteamLibrary\steamapps\common\Gothic 3` as read-only. Keep generated evidence under ignored
`.local/`; never commit original assets or decompiled bodies. Check `git status`, active processes
and agents before starting work. The active-work section supersedes older delivery job states.

For an intentional release rebuild, use the reviewed environment and exact command; do not add
`--target`, which suppresses the required host `RUSTFLAGS`:

```sh
source /c/Users/PowerTower/dev/gothic-reforged/.local/env.sh
cd /d/gr-wt/codex-renderer
export CARGO_TARGET_DIR=D:/gr-build/integrate RUST_MIN_STACK=67108864
cargo +stable-x86_64-pc-windows-gnu build --release -j4 -p gothic-reforged --features viewer --locked
```

The validated comparison can be reproduced only after creating a fresh isolated runtime and a
new output directory, with no compiler or game active:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/bench-renderer.ps1 `
  -BaselineExe <same-exe> -CandidateExe <same-exe> -RuntimeRoot <isolated-runtime> `
  -OutputDir <new-output-dir> -Camera -CompareIndirectDrawing `
  -Resolution 1280x720 -WarmupSeconds 30 -Seconds 20 -Repetitions 2
```

The harness records and restores environment overrides. Keep unrelated `GR_*` variables unset,
use the same EXE hash for both paths, and preserve failed logs. Do not repeat this already-passed
build, QA or benchmark unless code changes or a concrete failure requires it.

## Remaining work and evidence boundaries

- Leaf appearance and SDK fidelity remain unresolved. The measured VFS atlas and SPT records make
  an atlas-coordinate mismatch plausible, but the sparse crown persisted in prior captures. Keep
  `GR_SPT_FLIP_ATLAS_V` off: the opt-in changes silhouette-derived geometry and improved one view
  while thinning another; it is not a fidelity fix. PR71’s narrow spatial phase port is present,
  but captures did not establish reduced jitter. No hour-long temporal or vertex-silhouette proof
  exists. Shared-clock hour/day GPU readbacks passed earlier; they do not establish jitter fidelity.
  An unchanged Engine Ghidra import already timed out; use retained evidence before more native work.
  See `docs/research/SPT_ATLAS_DIAGNOSIS.md`, `SPT_NATIVE_UV_EVIDENCE.md`, and
  `LEAF_PHASE_COHERENCE.md`.
- Do not revive the global DDA/NPC-LOS raycast prototype. It caused two default melee QA failures
  while a legacy run passed, with no proven narrowphase bug. Current acceleration is limited to
  `CameraWorld::ray_nearest` via its explicit accelerated API; ordinary physics and NPC LOS retain legacy behavior. Preserve the
  failed artifacts under `.local/` and require real loaded-world LOS equivalence before any scope
  expansion.
- Quest work is not validated: no new APK, headset run, install, or deployment was performed.
  The host third-person layer fixture was fixed and an earlier Android native library built;
  packaging SDK tools remain missing. Do not repeat completed host fixes or claim headset validation.
  Review `docs/research/VR_RENDERER_AUDIT.md`, `VR_COMBAT_BNS.md`, and
  `CLOUD_PR_REVIEW_20261022.md`; consult the applicable Meta VR skill before Quest-specific work.
- Cloud PRs 71–74 were reviewed, not merged. Preserve their branches and foreign work. Exact heads,
  CI failures, and review limits are in `docs/research/CLOUD_PR_REVIEW_20261022.md`.
- Do not claim full Gothic 3 parity, cross-platform performance, general FPS gains, or a VR result.

Earlier global-DDA failures, water and renderer audits, failed QA, screenshots, retained trace
history and detailed checkpoints remain in the archived handoff above. Use those logs as evidence,
not as instructions to repeat stale jobs.

At 2026-10-23 05:46 UTC, account quota was last reported at 61% used (weekly40%); the next reset was expected
around 08:18 UTC. Recheck the actual limit before expensive work; do not spend the remaining reserve
on broad analysis. The continuation heartbeat remains active for leaf fidelity, renderer and VR
work; stay quiet while status is unchanged and notify only on a meaningful result, failure or
required action.
