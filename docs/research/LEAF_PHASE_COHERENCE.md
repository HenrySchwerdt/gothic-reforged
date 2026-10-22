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
