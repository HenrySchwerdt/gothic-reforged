# Gothic Reforged - Codex continuation, 2026-10-22

Quota reset VERIFIED2026-10-22 22:15UTC: 5h0% used, weekly16%; next reset03:15:16UTC.
Resumed bounded work: controlled-wind screenshot implementation is committed2a82eb6
(walk.rs/windsys.rs/new pure helper; no shader changes). Optimized GNU viewer build
FINISHED successfully8m15s; exec93271 closed; log `.local/pc-wind-shots-build.log`.
Immutable `.local/artifacts/gothic-reforged-wind-shots.exe`, SHA256
`9372eac6354ea91f7a412733ac3d953b7447dff31dfdba3729addffe757d5c9a`, source2a82eb6.
VALIDATION COMPLETE22:42UTC: NaN CLI exits1 beforeloader; controlled six-shot boundary
smoke exits0, grounded360/360, all6 requestedPNG files present, clean renderer/shader logs.
All6 capture CPU records exactly match selected current/previous records; all4 adjacent
pairs within hour/day groups preserve previous=current-from-prior. Intentional long gap
between groups is not a continuity test. Summary `.local/qa/wind-shots/validation-summary.json`.
Root inspected hour-before/hour-at images; no general jitter cure or GPU-readback claim.
Normal-mode magic9/bow15/melee13 each explicitPASS/exit0; saves remain empty. Logs:
`.local/qa/wind-shots-normal-combat/`. No new Desktop package; existing releases untouched.
No active compiler, game or coder remains. Current code differs from compiled2a82eb6 only
by one formatting-only startup-expression line; later other changes are documentation.
Helper/windsys targeted rustfmt PASS; walk retains3 pre-existing formatting hunks confirmed
against5856cbe baseline, no new formatting hunks. Do not report full-project fmt/Clippy green.
Helper tests5/5 pass (exact-source standalone harness); startup and per-shot deadlines240s
use Time<Real>, capture waits for ScreenshotCaptured, Bevy Time/simulation untouched.
See `docs/research/WIND_SHOT_CAPTURE.md`. Automation active; retain >=5% quota reserve.
Usage checked22:42UTC: 5h22% used, weekly19%, reset2026-10-23T03:15:16Z. Continue with
cheap bounded agents; NEXT practical performance task is review/promote/run the prepared
physics broadphase oracle tests, then implement/measure a safe DDA candidate only after
oracle passes. Do not benchmark under compilers, or infer speedup from synthetic tests.

Native UV evidence: original leaf/global HLSL extracted create-new under own ignored
`.local/qa/tree-diagnostic/`, no code committed. Fixed UV path passes through, but generated
XFX_PS_DIFFUSE expression remains unknown. Separate leaf textures absent in bounded
readable-archive inventory, not proof absent in unsupported archives/loose files.
Engine ABI candidates mapped in `docs/research/SPT_NATIVE_UV_EVIDENCE.md`; no fresh native
decompilation or original runtime. Existing compiled cache check completed:29ge_leaf blobs
(28VS/1PS) pass UVs through under reviewed existing decoder. Summary with originalcache/
decoderSHA and selectedranges `.local/qa/tree-diagnostic/leaf-cache-uv-summary.json`.
No unsafe unpickling/helper writes used. Selected runtime permutation and upstream SDK
mapping remain unknown. No active REA target/import; old Engine Ghidra timeout preserved.

Performance next: expensive in-aim ray query still gathers all XZ rectangle cells, sorts/
deduplicates candidates. Prior measured idle gate already shipped. SupercoverDDA proposal
requires oracle and boundary/tie tests before optimization. wind_stability prepared ONLY
ignored `.local/physics-raycast-oracle-proposal.rs` (not built/run or added to crate).
It enumerates fixture triangles independently, preserving sorted tie normals, inclusive
max, boundaries/corners, large triangles and remove/reinsert. Review/run before any DDA.

CURRENT22:56UTC: conservative traversal committed60d0812; optimized physics57/57PASS.
Final log `.local/physics-raycast-dda-tests.log`; two early failed attempts retained under
`.local/physics-raycast-dda-tests-{initial-compile-error,first-run-failure}.log`.
GNU viewer build running ONE exec53589, `.local/pc-raycast-dda-build.log`.
wind_clock owns polling that same job and ALL subsequent game runs, no duplicate builds:
create immutable raycast-dda EXE, sameEXE Camera A/B -CompareRaycastTraversal,
then unforced normal magic/bow/melee QA. Root must inspect results before packaging.
No Desktop delivery or measured performance gain yet. Source/model review finds no
concrete miss; pathological rays retain legacy expensive rectangular fallback.
## Active continuation 2026-10-22 22:50 UTC

Physics oracle promoted and committed `f65fccb`: GNU optimized physics unit suite53PASS,
including6 independent brute-force broadphase oracle cases. Log
`.local/physics-raycast-oracle-baseline.log`. Production behavior unchanged by that commit.
Active cheap agents: wind_stability implements conservative full-ray DDA candidate-cell
selection in physics (no early-out; preserve sorted tie/normal semantics and legacy path);
water_renderer reviews numeric edge cases read-only; wind_clock prepares same-EXE
`-CompareRaycastTraversal` benchmark control. No game build or benchmark started yet.
Next: review candidate+oracle tests, finish one optimized game build, then run same-quality
legacy/candidate measurement with forced diagnostic picking, and normal combat QA.
Do not infer a speedup from unit tests; Desktop packages remain unchanged.
## Completed bounded atlas diagnosis, 2026-10-22 19:28 UTC

Current source `cd4288a7b3a0a0ed9f174275e7ed3461ec00d4b7` adds an OFF-by-default
`GR_SPT_FLIP_ATLAS_V=1` experiment: remap leaf/frond/billboard V before silhouette
measurement and generation. Do not enable by default without multiple-species evidence.
Read `docs/research/SPT_ATLAS_DIAGNOSIS.md`. No released package changed.

GNU optimized game build FINISHED successfully in8m12s, session22722 closed; log
`.local/qa/tree-diagnostic/atlas-origin-game-build.log`. Immutable diagnostic EXE
`.local/artifacts/gothic-reforged-atlas-origin.exe`, SHA256
`f60cea222be15bd6a7bd7b4585be0ff23eb789a4a9a9804ba41c1458323f3d39`.
Compiled sourcecd4288a7b3a0a0ed9f174275e7ed3461ec00d4b7; later changes only docs.
TWO sequential flag-off/on NOWIND smoke comparisons finished successfully: each exit0,
grounded360/360, clean fatal renderer/shader checks, saves empty. Fixed apple/aspen/oak
viewpoints, evidence prefixes `.local/qa/tree-diagnostic/atlas-{off,on}`. Root inspected
on-apple, on-oak and off-oak; agent inspected matching off/on apple/oak. Apple changes
from sparse branches to dense green clusters; the oak-view scene becomes thinner when
flipped. Density is not a fidelity oracle and this multi-tree view does not isolate an
individual oak's material. Default stays OFF: no all-species UV rule established.
No active compiler, game or coding agent remains. This diagnostic was NOT packaged or
delivered as a production fix. Latest LeafPhase/GrassFix packages remain unchanged.

Evidence completed since last candidate:
- Existing `spt_dump` rebuilt; default variant0 apple generates400 independent quads,
  vertex layout checked, diagonals101.83..103.40cm. These are NOT runtime silhouette-
  calibrated meshes. Dump and numerical summary: `.local/qa/tree-diagnostic/`.
- `GR_SPT_DUMP_LEAVES=1` now prints exact leaf metadata on stderr without changing JSON.
  Apple leaf BulfordHollyLeaves_RT_1.tga points at upper-left cell. RedOak and PinOak/
  EuropeanAspen metadata also retained. No asset bytes committed.
- New read-only `vfs_match` compares selected overlay payload against saved reference
  (XIMG is rewrapped for DDS comparison). Both actual apple SPT and Composite01 match;
  intentional mismatching reference returns MATCH false and nonzero. Selected archives
  speedtrees.pak and _compiledimage.pak. Build/run logs retained.
- Exact texture DDS re-exported from base to own ignored evidence; SHA matches prior
  export006f39c4226a17c6564bd67af83f574cbe7cac45c48e3dc47912963515b382c5.
- Original DDS2048 vs old flattened PNG512 orientation checked numerically: same-
  orientation blue-composited meanRGBerror<.71/channel; flipped8.68/16.10/34.26.
  Alpha>=110 coverage4.89% upper-left (bare-tree motif) vs36.21% lower-left (leaf cluster).
  This is an atlas-origin candidate, not a confirmed all-species V-flip rule.
- Same apple camera with GR_NOWIND remains sparse: own `.local/qa/tree-diagnostic/nowind-*`,
  exit0, grounded360/360, clean fatal renderer scan. Thus wind is not required for sparse crown.

Latest delivered LeafPhase and prior GrassFix remain unchanged and verified below.
NEXT after quota permits: verify original intended per-record UV conventions and source
sprite identities using multiple species, including fronds and billboard transitions;
avoid simply choosing denser output or hardcoding an apple-only exception. The all-rect
flip also changes silhouette calibration, so isolate raw sprite mapping from generated
crown shape in further controlled evidence. Leaf temporal capture remains unimplemented;
existing clock buffer checks and qualitative images do not establish jitter is solved.
Quota checkpoint at19:23UTC: 5h window92% used, reset2026-10-22T22:08:55Z; weekly14%.
User explicitly requests conserving quota across the day. Finish the bounded active
capture, document it, then check current usage before new expensive work. If >=95%,
reserve remaining capacity for user messages/failure handling; defer new builds/agents/
RE until the actual reset, with silent unchanged heartbeats. Automation remains active.

## Latest continuation: leaf-phase candidate (after Cloud PR review)

Compiled source `543db4549efe7dd1bb7eaf56788a171ca95f3bda` ports only PR71's
spatial leaf phase coefficients `(0.37, 0.29) -> (0.031, 0.023)` into our shared
`gothic_wind.wesl`. Wind amplitude, temporal frequency, bounded clock, previous-frame
records, 80-byte buffer, normal handling and visibility/fade contracts are unchanged.
The coefficient norm is 12.2 times smaller; this bounds spatial phase spread for a
given vertex separation, not frame jitter or visual quality. No actual card extents
were measured. Details: `docs/research/LEAF_PHASE_COHERENCE.md`.

Optimized GNU viewer release built successfully in 8m17s. Immutable candidate:
`.local/artifacts/gothic-reforged-leaf-phase.exe`, SHA256
`0134ceb9c06d0f77bfca10fdf62726c56841500cdde0b9c179ffd6cd08381f52`.
Build log: `.local/pc-leaf-phase-build.log`. Existing release shader checker passed
11 variants plus its expected negative fixture: `.local/leaf-phase-shader-check.log`.
Actual candidate combat QA passed magic9/bow15/melee13, body trace PASS, exits0,
clean fatal renderer checks, saves empty: `.local/qa/leaf-phase-combat/`.

Actual baseline and candidate smoke runs both exited0, grounded360/360, clean
renderer logs. Four repeated fixed-camera screenshots each are retained under
`.local/qa/leaf-phase/{baseline,candidate}-leaf{0,1,2,3}.png`, plus walk/overview.
Camera metres `(38.83,55.5,28.5)`, yaw0, pitch0.12, targeting loaded apple tree
at centimetres `(3883,5156,2109)`. Root inspected baseline leaf0 and candidate
leaf0/leaf3: sparse foliage persists; no claim that jitter is cured. Wind advances
normally and capture timestamps/phases are not matched, so these are qualitative
render checks, not an exact temporal A/B or a performance benchmark.

Desktop test candidate: `C:/Users/PowerTower/Desktop/GothicReforged-Codex-20261022-LeafPhase/`.
Final package QA from C:/Windows/Temp PASSED magic9/bow15/melee13, each exit0 with
explicit QA RESULT PASS and clean renderer/shader/validation checks. EXE hash unchanged,
package saves empty before/after, no Temp save directory created. Evidence:
`.local/qa/leaf-phase-package/`. GrassFix below remains the prior verified release.
No active compiler, game or coder remains at this checkpoint, 2026-10-22 19:01 UTC.

Next: assess leaf motion with controlled time and actual card geometry before
porting PR71's inferred angle/amplitude/frequency changes. Sparse tree crowns and
remaining isolated white/checker spots still require causal investigation.
No other Cloud changes merged. Check live agents/processes before any new jobs.
Previous full handoff preserved `.local/HANDOFF-before-leaf-phase-candidate.md`.

The sections below retain the earlier 18:35 UTC checkpoint. The continuation above
takes precedence where its later candidate state differs.

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

## Prior delivered PC package - VERIFIED GrassFix

C:/Users/PowerTower/Desktop/GothicReforged-Codex-20261022-GrassFix/GothicReforged.exe
SHA256 95f1fafa733216c30d4e3038642bff697b76b32157b0ba8ffd07597b8f73162c
Compiled ec313601352feeb23d32b77e180a230837679257. Optimized GNUviewer thin-LTO,6m54s.
Immutable .local/artifacts/gothic-reforged-foliage-fade.exe; .local/pc-foliage-fade-build.log.
FinalpackagefromC:/Windows/Temp: magic9/bow15/melee13 explicitPASS, body0deg, cleanrenderer/
shaderchecks, independentpackagesavesempty. .local/qa/grassfix-package/.
PrepackagecombatPASS .local/qa/foliage-fade-combat/; normal/NOWINDsmokes grounded360/360 with
cleanrendererlogs .local/qa/foliage-fade/. Actualoverviewbefore/after confirms HUGEWHITEGRASS
PATCHESREMOVED inBOTHmodes. Closewalkgrassretained. Smallisolatedwhite/checkerspotsremain.
Rootinspectednormaloverview+NOWINDwalk, no unmeasuredspeedupclaim. PRUEFSTAND.txt+build-info
recordsource/hash/evidence/limits. OldPerfLeaf70318,WindFocusb115,etc preservedunchanged.
OnlyHANDOFF+packageREADMEtemplate were dirty atpackaging; explicitcompiledcommitrecorded.
ScriptREADME now recommends finite deterministic --bench-camera, not frame-dependentwalk.

## LIVE work / Cloud continuation

No activecompiler/game/coder atcheckpoint. Check collaboration.list_agents/processesbeforejobs.
CodeHEADec31360 plus rootdocs/script updates. DeliveredGrassFiximmutable/current.
UserjustsaidClaudeCloudcontinuedinPRs/branchesandaskedinspection. Read-onlyreviewdone,
no merge/cherrypick or remotemessages. FourrecentPRsattachedtothischat and fetched as
codex-cloud-audit/{foliage-sway,orc-shape-probe,vr-feel,input-unused-mut}; exactheadsrecordedin
NEW docs/research/CLOUD_PR_REVIEW_20261022.md and .local/research/cloud-pr-audit-20261022.json.

PR71 2a997662... cloud/foliage-sway: relevantremainingleafwobble. PortsemanticchangesintoOUR
sharedgothic_wind, notoldvertexshader. Narrowfirstcandidate .37/.29 ->.031/.023 spatialphase
forcardcoherence. Strongerangle-degree/crownamplitude/frequencychanges inferred, needcontrolled
realGPUcapture. Keepboundedf64clock,current/previousrecords,worldnormals,visibility+fade.
PR72 ba2d6213... orcshapeprobe: diagnosticONLY,noshapefix; notrunwithdata. UsesbasePAKlikecurrent
ActorLibrary. Canportasread-onlyexamplelaterifneeded; don'tclaimorcanatomyfixed.
PR73 557642f0... VRfeel: genericOpenXRhaptics/contacts/bow/twohandimprovements. SeparateQuest
runtimeNOTwired. Genericvibrate checksrunningbutnotFOCUSED; play_haptics Lastunconditional.
Dynamicsecondaryhandtrackingneeds test(dampingtargetleaderangularvelocity). Noactualheadset
proof. Metaofficialdocsconsultedbyreviewer; exactfindingsinreviewdoc. NoPRVRbuildperformedhere.
PR74 8eb11a74... inputunusedmut safeone-linecandidate. OurCargo.lockalreadyhasportcovrepair+
40additionallocaldependencylines, soNEVERreplaceentirelockwithcloudversion. NoCloudcodeported.
AllfourGitHubActions: releaseWindows/Linux,fmt,VRjobs,OpenXR PASS; ordinarycheckjobsFAILED
workspaceClippy-Dwarnings. Examined#71/#74Windowslogs show42apps/game errors. Combinedcommit
statusesemptyisNOTallgreen; actualActionsjobschecked. Olderopen#62-66inventoryrecorded,
includingactorstream#66andopt-inwiring; prioritize#71first. Branchfetchdidnotcheckoutfiles.

NEXT: safe semanticportofPR71coarseleafphase withcontrolledsame-sceneGPUvalidation, then
inferredamplitudechangesseparately. Keepcurrentreleasebaseline. Remainingisolatedwhitepatches
needmaterial/alpha/LODdiagnosisbeforeguesses. Futureperformancebottleneck measuredonly; native
VR/APK prerequisites+actualheadsetvalidationstillpending. No newrenderchangesauthorizedyet.
Heartbeatclaude-abwarten-dann-gothic-pc-build-fortsetzenACTIVEevery15min, silentunchanged;
disableonlyfullcompletion/cancel. Cheapcoders, isolatededits, noduplicatebuilds/measurements.

## Renderer follow-up evidence / constraints

8405cef LODvisibilitydither sourcefix mirrorsBevy calls inGothicforwardandbothprepassfragments.
Shaderchecker now ACTUALforwardfragmentwith/withoutdither (previousforward/coloronlyvertex),
NOWINDwith/withoutdither,wind/compressed/motion/shadow/normal/missingnormal:11variants+negative
PASS. .local/shader-check-dither-build2.log. Immutablecandidate0d02e13733c5d8c7c10ed55ba631e8c632480cf2add7e218e049a8926544729b
.local/artifacts/gothic-reforged-lod-dither.exe,6m55s,normal/NOWINDsmokesPASS .local/qa/lod-dither/.
WhitegrassfieldsSTILLPRESENTinthisbaseline; ditherwasNOTtheprimarywhitegrasscause.

Foliagefadeec31360 correctsactualcolor/depthdisagreement usingONEpuregothic_visibility helper
withEXACTexisting3Ddistance/smoothstep/IGNexpression inforward+bothprepassbranches.
PrepassreusesBevymesh_view_bindings::view, no duplicateUBO. Moduleembedded+retainedandchecker-
registered. F_FADEONLYsetforFoliage;materialcachekeysinclude kind; bothproducersgo through
spawn_part ALWAYSaddingNotShadowCaster. Bevyexcludesmarkerfordirectional/point/spot. Thus
F_FADEmaterialsNEVERenterlightviews; bothactiveviewuniformsareactualmaincamera. Commentswarn
futurefoliageshadowfeaturemustprovide maincamerapositionbeforeenabling.
NOnewcamera-global, NO96bytebuffer. Old80bytewind/probelayoutUNCHANGED. Initialview-role/global
proposalwasNOTimplemented. Bevyhasnogenericmainvsshadowkeydef; doNOTinventonefromDEPTH_PREPASS.
Releasechecker11variants+negativePASS .local/fade-shader-check-release2.log; GNUviewercheckPASS
.local/fade-viewer-check.log. Actual95f1...GPUcapture+combat+packagetestsPASSabove.

Readonlyalternativetextureaudit: priorlogsDOexist(earlyagentmisscorrected). Theyshownonfatal
TonemappingNone/DebandDitherwarningpluscache/navfallbacks,nopertexturewarnings. world_loader::
texture/walk::texture_imagecansilentlyreturnNone; optionaltexturefallbackwhiteisacandidatemechanism
forREMAININGartifacts, notproven. GR_DUMP islimitednear40mstaticregion,GR_TREE_LOGpositionsnot
materials; no generalscreenpixel->materialtool. Don'tclaimmissingtextureswithoutactualdata.
Charcolor/prepassalphacutoffmatches; nocharVisibilityRange found.
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
