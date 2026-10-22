# Physics ray traversal experiment

The previous ray broadphase enumerates every XZ cell in the segment's bounding
rectangle. Long diagonal aim rays can visit many cells far from the segment.
The candidate visits segment cells with a conservative one-cell halo, then keeps
the existing triangle AABB filter, sorted triangle identities and narrow phase.
It does not stop at the first hit. Vertical, thin and small queries keep the
rectangle path, as do unsupported numeric ranges. Capsule queries are unchanged.

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
- Normal unforced comparison and combat validation are in progress. No new
  Desktop release until those checks and final-package validation complete.
