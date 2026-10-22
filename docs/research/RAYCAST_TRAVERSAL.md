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

## Evidence

- Baseline oracle commit `f65fccb`: 53 optimized physics unit tests pass, including
  six independently enumerated fixture cases. Log:
  `.local/physics-raycast-oracle-baseline.log`.
- Candidate implementation, runtime measurements and delivery status: pending.
