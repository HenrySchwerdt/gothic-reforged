# Wind phase stability

The SpeedTree-style branch and leaf oscillators previously evaluated their phase as
`frequency(current_gust_strength) * globals.time`. Because gust strength changes with time, the
phase derivative also depended on total runtime (`f + t * f'`). A gust could therefore change
oscillator speed abruptly, with the phase error growing as the session ran longer. The CPU
reference in `crates/environment/src/wind.rs` used the same formula.

Both CPU and GPU now sample oscillator frequencies from the clamped base `WindStrange` strength.
Gusts continue to change bend, vibration, rocking, and rustling amplitudes. This keeps each phase
rate constant and the two implementations aligned; it is a stability choice in the inferred
SpeedTree reimplementation, not a claim about SpeedTree SDK parity. A CPU regression samples
actual branch and leaf offsets on multiple gust onsets and endings near 0, 1, 10, and 50 hours,
using the representable `f32` frame interval. The 10/50-hour cases are CPU stress references:
Bevy 0.20's `GlobalsUniform.time` wraps to zero after one hour, so they do not represent GPU
runtime samples.

The tree seed is derived from the instance's world translation, and leaf phase also uses world XZ,
so neither varies by frame. The color vertex stage applies wind. `GothicMaterial`'s custom
shadow/depth prepass still uses a static vertex path. Bevy 0.20 does provide time in that view
layout at group 0, binding 1, but depth-only and shadow variants omit the normal input required
for leaf flutter. Matching the animated silhouette needs a pass-aware normal vertex input/layout
and a wind-enabled prepass vertex entry point. It remains unresolved here; do not claim wind/shadow
parity. Wrapped GPU time can also cause a phase discontinuity at each hour boundary even with a
fixed frequency; avoiding that needs a bounded phase accumulator or a separate unwrapped wind-time
uniform and is outside this focused fix.

Manager validation: the environment release test suite passed all 24 tests, including the actual-offset gust-boundary regression. This is CPU evidence; GPU hour-wrap and prepass silhouette parity remain open.
