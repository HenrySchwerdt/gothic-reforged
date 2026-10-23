# Physics ray traversal experiment

The previous ray broadphase enumerates every XZ cell in the segment's bounding
rectangle. Long diagonal aim rays can visit many cells far from the segment.
The camera opt-in candidate visits segment cells with a conservative one-cell halo, then keeps
the existing triangle AABB filter, sorted triangle identities and narrow phase.
It does not stop at the first hit. Vertical, thin and small queries keep the
rectangle path, as do unsupported numeric ranges. Ordinary `raycast`, `raycast_hit`
and capsule queries retain the reference behavior. Only the camera trait calls
the explicit `raycast_accelerated` API; NPC line-of-sight uses ordinary `raycast`.

`GR_RAYCAST_LEGACY=1` selects the rectangle implementation for a process. This
allows comparisons using one executable, with identical renderer settings.
The candidate requires independent ray-distance and normal/tie oracle tests;
passing these tests alone does not establish a performance improvement.

## Measurement protocol

Finish all compiler jobs before measuring. Use a new output directory outside
the runtime root and the same immutable executable for both arguments:

```powershell
$env:GR_AIM_PROFILE = '1'
./scripts/bench-renderer.ps1 -BaselineExe $exe -CandidateExe $exe `
  -RuntimeRoot $runtime -OutputDir $newOutput -Camera `
  -WarmupSeconds 30 -Seconds 20 -Repetitions 2 -CompareRaycastTraversal
```

The comparison switch forces `GR_AIM_ALWAYS_PICK=1` for both variants so the
otherwise skipped idle aiming query is actually measured. It sets the legacy
traversal only in the baseline and restores the caller's environment afterwards.
Restore `GR_AIM_PROFILE` in the caller as well. Retain raw frame measurements,
aim-query timing logs, hashes, renderer diagnostics and scene/pose metadata.

This forced workload measures traversal cost in a reproducible camera scene.
It is not a general FPS claim or an in-combat benchmark. Validate normal magic,
bow and melee scenarios separately, plus unforced gameplay frame times before
shipping. Check actual screenshots where visual behavior could differ. Existing
Desktop packages remain unchanged until a replacement is explicitly packaged
and validated from its final location.

For the unforced control, use a second new output directory and add
`-UnforcedRaycastComparison` to the command above. This removes the forced-pick
flag in both variants, while still changing only ray traversal. The switch is
accepted only together with `-CompareRaycastTraversal`.

## Evidence

Current source **`3e5cbdb`** scopes the optimization to camera queries after the
global prototype's melee failures. Physics59 + camera25 + camera integration13 +
movement integration14 tests pass (111 total), final log
`.local/physics-camera-raycast-scoped-tests-camera-regression.log`.
The optimized game build completed in 8m50s; immutable artifact
`.local/artifacts/gothic-reforged-camera-raycast.exe`, SHA256
`445c3773b4de48cabad3a94c07a5d17ea799a149834d2da5377605047e99b47f`.
QA-first validation passes melee13/magic9/bow15 (37 checks, explicit PASS/exit0),
with target active, unrelated actors inactive and final hero-down false. The trace
still records incoming target damage (HP200→149→200); no invulnerability was added.
Evidence `.local/qa/camera-raycast-combat/`.

Fresh same-binary comparison for this scoped build, 1280x720, 30s warmup and 20s
samples, two runs per variant:

| Workload | Legacy median run-average | Candidate | Change |
| --- | ---: | ---: | ---: |
| Forced idle target picking | 11.3107 ms | 10.3308 ms | -8.66% |
| Normal unforced camera | 10.0506 ms | 10.0418 ms | -0.087% |

Forced query CPU per call: legacy6.7275/6.6663 ms, candidate4.0053/3.9923 ms
(about40% less CPU time). Forced p95 medians12.655 versus11.052 ms; normal
p9510.899 versus10.6356 ms. These are descriptive small-sample results for this
camera route. Normal throughput is essentially unchanged; no general FPS claim.
Root independently checked the forced raw profiles: all12,752 entities, visible
meshes1540–1543, expected camera-only route; tiny simulation/pose variation remains.
Child environment maps and both benchmark summaries are clean. Evidence:
`.local/bench-camera-raycast-{forced,normal}/`.

Delivered separately on the Desktop:
`C:/Users/PowerTower/Desktop/GothicReforged-Codex-20261023-AimPerf/GothicReforged.exe`.
Root independently verified the hash above against the final EXE and zero files
in the new runtime saves folder. Compiled source is recorded separately from the
packaging/documents commit in `build-info.json`. Existing packages are preserved.
Final EXE from `C:/Windows/Temp`: melee13/magic9/bow15, explicit PASS/exit0, correct
child environments and melee fixture diagnostics. Smoke exit0, grounded360/360;
actual `smoke-walk.png` and `smoke-overview.png` visually inspected by root.
All final evidence `.local/qa/camera-raycast-package/`. Remaining small white/checker
patches are visible in the overview; leaf motion/atlas fidelity and VR are not
certified by this release. No APK/headset validation, no remote publication.

Earlier prototype measurements below do not certify the current scoped executable.

Global prototype outcomes: two default melee runs failed with downed hero, while
one startup-legacy run passed. The difference is unresolved, so global NPC LOS
optimization is withdrawn. Do not re-enable it on synthetic tests alone: require
comparison of real-world LOS rays within one loaded world and inspect any mismatch.
All original logs/screenshots are retained; no claim of a proven DDA numeric bug.

Read-only scheduling audit found another possible confound, not a proven cause:
`npcs::los_service` drains pending LOS in Update without a time budget; runtime
`PlayWorld::line_of_sight` returns cached answers or optimistic true and caps the
pending vector at 64 without deduplication. AI runs at fixed 30 Hz, while an
Update may follow multiple fixed ticks. Changed frame cadence can therefore
change answer age/queue pressure even if ray answers match. Trace fixed ticks per
Update, queued/answered/dropped requests and answer age before altering that
service. `spawn_npcs` has a separate presentation budget, not a Sim AI budget.

The QA fixture independently isolates the selected opponent before the attack
checks, pauses unrelated NPC AI, preserves the target's existing AI until the
original guard-stage freeze, and logs target/unrelated activity plus the first
hero-down event. It does not revive the hero, alter health recovery, change game
mechanics or weaken existing assertions. Runtime validation is still required.

- Baseline oracle commit `f65fccb`: 53 optimized physics unit tests pass, including
  six independently enumerated fixture cases. Log:
  `.local/physics-raycast-oracle-baseline.log`.
- Candidate source commit `60d0812`: 57 optimized physics tests pass, including
  seeded long rays and a long-distance normal tie that explicitly select DDA.
  Log `.local/physics-raycast-dda-tests.log`; early failed attempts retained
  separately. Independent source review found no concrete miss.
- The step limit bounds the new traversal only. A rejected extreme ray keeps
  the existing rectangle fallback, which can itself be expensive.
- First optimized viewer build completed in 9m06s, source `60d0812`, immutable
  `.local/artifacts/gothic-reforged-raycast-dda.exe`, SHA256
  `58889185fa3c2d477ac6ff3e61ef6ee4a1ee84a16abea56afb1dfbbee6cc0d2c`.
- First forced camera comparison: `.local/bench-raycast-dda-camera/summary.json`.
  **INVALID as normal-quality performance evidence:** wrapper cleanup left
  `GR_NOWIND` and `GR_WIND_CLOCK_PROBE_START` present with empty values. All four
  stderr logs contain `wind-clock probe FAIL`. Keep these raw files as a failed
  experiment; the following numbers are descriptive only.
  Two runs per variant, same executable, 1280x720, 30s warmup and 20s samples.
  Median run-average frame time 11.2279 ms legacy versus 11.2983 ms candidate
  (+0.63%); query timing stays approximately 6.6 ms per call. **No gain shown.**
  Renderer-only checks passed but missed the probe failure; both variants force
  one idle pick per frame. Do not promote this result into a speedup claim.
- The initial selection heuristic estimates nine independent halo cells for
  every path step, then demands another factor of two. Neighboring halos overlap
  heavily. It therefore rejects the actual `PICK_RANGE = 10_000 cm` camera ray.
  The first implementation's 50,000–90,000 cm tests did not catch this workload
  mismatch. A revised heuristic and explicit real-range camera test are pending.
- The unfinished unforced first-build baseline has the same probe failure and is
  invalid. No candidate was launched; no completed normal A/B or combat QA yet.
- Environment cleanup must use `Remove-Item Env:\NAME` for absent variables.
  On this host, .NET `SetEnvironmentVariable(name, $null, 'Process')` leaves an
  empty variable visible in a real child process. Repro/proof:
  `.local/bench-raycast-dda-env-child-check.log`. Snapshot and restore existing
  values, including deliberately empty ones; remove variables originally absent.
  Benchmark preflight now rejects any probe variable, failure logs reject probe
  FAIL, and the script restores absence using `Remove-Item`.
- Revised source `e5a93e4`: actual camera-range selection, 59/59 optimized physics
  tests including explicit nonempty hits, log
  `.local/physics-raycast-dda-range-hit-tests.log`. Viewer build 8m39s, immutable
  `.local/artifacts/gothic-reforged-raycast-dda-range.exe`, SHA256
  `426a2da54b6f7af640f4ae640351823ac1f99b5302f363250fec5307d1098c6d`.
- Clean forced comparison `.local/bench-raycast-dda-range-camera/summary.json`:
  same executable, 2 runs/variant, 1280x720, 30s warmup + 20s samples.
  Median run-average frame time 11.2491 ms legacy versus 10.3318 ms candidate
  (-8.15%); median run p95 12.6293 versus 11.1218 ms. Query CPU time per call
  6.5865/6.6908 ms legacy versus 3.9767/3.9718 ms candidate (about 40% less).
  All measured frames make forced idle picks; this is a diagnostic workload.
  Only `GR_AIM_PROFILE=1` is inherited after child-process cleanup verification;
  all four logs are free of probe failures and renderer review flags.
- Root independently checked raw profiles and camera metadata: 12,752 entities
  in every run, visible mesh count 1541–1543, same analytical route and renderer
  settings. Slight pose/visible-count variation remains; this is a small sample,
  not an assertion of perfectly identical simulation states or general FPS gain.
- First clean normal control: `.local/bench-raycast-dda-range-camera-normal/`.
  Mean-frame medians 10.1253 versus 10.1212 ms (-0.041%), essentially unchanged;
  zero idle picks in all four profiles. P95 rows legacy 11.1849/10.9687 ms versus
  candidate 12.0334/10.9468 ms. An additional identical unforced A/B is underway
  to check the single candidate tail outlier; do not claim improved normal FPS.
- Magic and bow QA pass with explicit markers and exit0. **Melee QA FAILS**:
  power clips empty, then PierceAttack timeout. Evidence
  `.local/qa/raycast-dda-range-combat/melee_faithful/03_power_attack.png` shows
  "Du wurdest niedergeschlagen" with surrounding orcs. The watcher heals HP but
  does not revive a downed hero, and freezes other actors only after the power
  checks. Earlier successful traces also contain incoming damage. This suggests
  a fixture exposure, but DDA also serves NPC LOS, so causality remains unproven.
- A same-EXE startup-legacy melee run is queued after the repeat measurement.
  QA-only isolation proposal `.local/qa/melee-ai-isolation-proposal.md` is unmerged:
  preserve selected target AI, pause unrelated actors early; never revive/reset
  lifecycle or weaken assertions. No new Desktop release until failures are
  understood and final-package validation completes.
