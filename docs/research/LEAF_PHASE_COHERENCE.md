# Leaf wind phase coherence

The shared wind shader now derives the per-leaf flutter phase from world XZ with coefficients
`(0.031, 0.023)` instead of `(0.37, 0.29)`. Since `wp` is in world metres and the dot product is
fed to a phase in cycles, the coefficients are cycles per metre. This change affects phase
coherence only: wind amplitudes, oscillator rates, the bounded f64 clock and its previous-frame
records are unchanged. The same shader module is used by forward and depth/shadow prepass vertex
stages.

The geometric bound is `|Δphase| ≤ ||k|| · ||Δwp_xz||`. The old coefficient norm is about
`0.4701 cycles/m`; the new norm is about `0.0386 cycles/m`, a 12.2× reduction. For an illustrative
0.45 m XZ-projected separation across a card, this bounds the phase spread from about 0.212 cycles
to 0.0174 cycles. The 0.45 m extent is an example from the cloud PR's synthetic case, not a measured
maximum for installed Gothic 3 foliage; actual card extents and orientations were not sampled here.

This is a bounded analytic reduction in phase variation across a given span. It does not establish
that leaf jitter is fixed or visually improved in the game. The standalone release shader checker
only validates shader compilation variants; a matched GPU capture is still needed to assess the
rendered foliage. No angle-unit, flutter-amplitude, trunk-vibration or frequency changes from the
cloud PR were included because those remain inferred SpeedTree behavior.

## Actual renderer evidence

Release source `543db4549efe7dd1bb7eaf56788a171ca95f3bda` built in 8m17s; immutable
`.local/artifacts/gothic-reforged-leaf-phase.exe` has SHA256
`0134ceb9c06d0f77bfca10fdf62726c56841500cdde0b9c179ffd6cd08381f52`.
The existing shader checker passed all 11 variants and its negative fixture.
Runtime combat checks passed magic9/bow15/melee13; see `.local/qa/leaf-phase-combat/`.

Baseline and candidate used the same GrassFix runtime directory and the command below,
with a fresh absolute PREFIX per executable. GR_NOWIND was unset and GR_RES was 1280x720.
Both finite runs exited0 and the walking controller remained grounded360/360 frames.
No compiler or other game ran during either capture.

```text
EXE --walk --smoke --screenshot PREFIX --shots "leaf0:38.83,55.5,28.5,0,0.12;leaf1:38.83,55.5,28.5,0,0.12;leaf2:38.83,55.5,28.5,0,0.12;leaf3:38.83,55.5,28.5,0,0.12"
```

The camera targets a loaded `G3_Tree_S_GSApple_01.spt` at world centimetres
`(3883,5156,2109)`, recorded by GR_TREE_LOG in the magic QA log. Capture files live at
`.local/qa/leaf-phase/{baseline,candidate}-leaf{0,1,2,3}.png`; smoke logs and overview/walk
captures are adjacent. Root inspected baseline leaf0 and candidate leaf0/leaf3. Sparse
foliage remains and no temporal improvement is established. These repeated poses wait
for stable streaming, but the advancing wind phases and capture intervals are not matched.
The smoke frame-time summaries are diagnostic only and are not a controlled speedup test.

## Leaf-flutter amplitude diagnostic

`GR_LEAF_FLUTTER` is an opt-in, startup-cached multiplier for the leaf flutter amplitude input.
Unset or invalid values use `1`; accepted values are finite `0..=4`. `0` disables flutter,
`0.5` halves it, and `1` preserves the existing material value. It applies only to `TreeLeaves`;
branch sway, geometry, wind phases, and the shared clock are unchanged. Forward and depth/shadow
prepass use the same wind helper and material parameter. This is a diagnostic control, not a claim
about SpeedTree amplitude semantics or evidence that stronger flutter caused the observed leaf
appearance. No runtime comparison has been performed for this switch.
