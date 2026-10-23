# Claude Cloud review — 2026-10-22

Repository: HenrySchwerdt/gothic-reforged. Read-only review; no merge or cherry-pick.
The four recent heads were fetched into codex-cloud-audit/* refs without checking out files.
All four use dev/engine-continuation base 0d8ab9e9bf842f8af0ce9e3f3c4e2310b9605d18,
which is also the merge base of our renderer branch. They are open drafts.
Raw metadata/CI summary: ignored .local/research/cloud-pr-audit-20261022.json.

| PR | Exact reviewed head | Assessment / next step |
| --- | --- | --- |
| [71: foliage sway](https://github.com/HenrySchwerdt/gothic-reforged/pull/71) | 2a997662243ed56f0fd427b9890434602cef7484 | Relevant to remaining leaf wobble. Port semantics into current shared wind shader, preserving bounded clock and prepass parity; do not copy the old vertex shader wholesale. |
| [72: orc shape probe](https://github.com/HenrySchwerdt/gothic-reforged/pull/72) | ba2d62139d073104e34d8923236b3c2e26b1ae40 | Diagnostic example, not a shape fix. Prints bind/idle joint heights, changed bone offsets and scales. Cloud had no original data, so actual orc diagnosis remains pending. |
| [73: VR feel](https://github.com/HenrySchwerdt/gothic-reforged/pull/73) | 557642f03645edda33b1a1f6e6bc93fb3ee44232 | Generic VR haptics, physical blade contacts, two-hand constraints and bow changes. Separate Quest runtime is not wired by this PR; focus handling and dynamic two-hand tracking need review before local integration. |
| [74: input CI](https://github.com/HenrySchwerdt/gothic-reforged/pull/74) | 8eb11a7463464f1827d50af599c051c34bda2fbb | The one unused-mut removal is applicable. Our lock already contains the portcov repair plus additional local dependencies; replacing it with the PR lock would remove 40 lines of local dependency data. |

## PR71 integration evidence

Spatial leaf phase coefficients change .37/.29 to .031/.023 so vertices across a card
move more coherently. Current equivalent is in shared gothic_wind.wesl, rather than the
PR's older gothic_vertex.wesl implementation. This is the narrowest candidate to try first.
Angle conversion to radians and crown vibration amplitude/frequency changes are stronger
visual changes based on an explicitly inferred SpeedTree mapping. The synthetic calm-bound
test does not establish original SDK parity or an actual in-game improvement.

The PR's GR_LEAF_FLUTTER uses wind_mat.w; our current path initializes it to zero and
does not consume it. Port both producer and consumer consistently if introducing that control.
Preserve f64 bounded phases, exact previous-frame records, shared focus, world-normal
deformation parity, visibility dithering, and the newly fixed foliage depth fade.
A controlled capture is still required before claiming reduced visible jitter.

## PR73 review evidence and limits

The new generic OpenXrInput::vibrate checks running, not SessionState::FOCUSED.
play_haptics runs in Last without a focus guard. READY starts running; STOPPING/end clears it.
The existing separate crates/xr-quest path has its own focused haptic gate. No Quest haptic
mixer wiring is added here, so a generic backend merge is not a delivered Quest feature.

Two-hand orientation follows the secondary hand, but the angular damping target uses the
leader's angular velocity. Static settling tests do not prove dynamic secondary-hand tracking;
add a dynamic test before treating that risk as resolved. No demonstrated failure was asserted.
Reviewed stab/clash/bow changes had no obvious blocker in the inspected diff.

PR-reported Monado/headless testing does not establish physical controller sensation or
headset tuning. No local VR build, headset run, SDK installation or deployment was done for
this review. Official Meta haptics reference consulted by the VR reviewer:
https://developers.meta.com/horizon/documentation/native/android/mobile-openxr-haptic

## Actual GitHub Actions status checked

For all four exact heads: Windows/Linux release jobs, fmt, Windows/Linux VR jobs and
OpenXR jobs succeeded. Both ordinary check jobs failed at workspace Clippy with -D warnings.
Examined Windows logs for #71 and #74 both end with 42 gothic-reforged errors in apps/game.
Thus these PRs are not all-green CI, despite successful release builds and VR jobs.
No claim that every Clippy issue was independently fixed or that headset behavior was tested.

Actions runs: #71 39727632539; #72 39784521460; #73 39789457037; #74 39819250302.
Other older open drafts #62–66 still exist; this pass prioritized today's #71–74.
No remote comments, PR metadata changes or merges were sent.
