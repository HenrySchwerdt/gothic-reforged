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
so neither varies by frame. Both the color and Gothic prepass vertex stages now import the same
wind function. The prepass shader uses Bevy 0.20's prepass view `Globals` binding (group 0,
binding 1), and `GothicMaterial::specialize` adds the existing normal attribute at location 3 only
to depth-only and shadow prepass layouts when a mesh has normals. Bevy's normal/deferred pass keeps
its default location 3 input and normal decompression. For a mesh without normals, both paths use
the up-vector fallback. `GR_NOWIND=1` leaves Bevy's default vertex stages selected.

The prepass layout change reads the normal attribute already present in the mesh buffer; it does
not allocate another mesh buffer or duplicate vertex data. The extra fetch applies only to custom
depth/shadow pipelines. Bevy's compressed normal format is retained and decoded through its normal
prepass IO helper. The current game mesh path supplies normals, but pipeline support for absent and
compressed normals is kept explicit.

This is source-level parity work and has not yet passed pipeline validation or an in-game silhouette
check. The coordinated runtime check should exercise opaque and alpha-cutout trees and grass with
color, depth prepass, and shadow views enabled, then inspect all generated pipelines for shader or
vertex-layout errors. Also check a normal prepass, a mesh with no normal attribute, Bevy-compressed
normals, and `GR_NOWIND=1`. Confirm moving silhouettes and shadow edges agree; this shader copies
Bevy's static mesh path and does not implement morphing or skinning, which are not used by the Gothic
world meshes targeted here.

The first runtime attempt reached pipeline creation but failed WGPU validation: the custom prepass
vertex shader imported forward `mesh_view_bindings::globals` at group 0, binding 11, which the
prepass layout does not provide. Bevy 0.20's prepass layout binds `Globals` at binding 1; the shader
now declares that prepass binding directly. The failed attempt is not parity evidence, and the
source correction still needs a full pipeline/runtime rerun (including `GR_NOWIND=1`).

The next `GR_NOWIND=1` smoke exposed another shader-variant issue despite a successful process exit:
Bevy still type-checks the custom prepass module's vertex entry. In Bevy's prepass IO, `Vertex.normal`
exists only when both `NORMAL_PREPASS_OR_DEFERRED_PREPASS` and `VERTEX_NORMALS` are defined, while
`VertexOutput.world_normal` exists whenever `NORMAL_PREPASS_OR_DEFERRED_PREPASS` is defined. The
shader uses braced WESL branches so the normal read and output assignment share the combined guard;
the fallback writes only the normal-prepass output when the mesh has no normal attribute.
`world_position` and builtin `position` are unconditional, while previous position, unclipped depth,
UVs, color, instance index, and visibility dither remain guarded by their matching Bevy defines.
The corrected variant still needs runtime validation.

Wrapped GPU time remains a separate issue. Bevy 0.20's `GlobalsUniform.time` wraps to zero after one
hour, so a fixed-frequency phase still jumps at that boundary. Avoiding it needs a bounded phase
accumulator or a separate unwrapped wind-time uniform, with a shared render-view data path; it is
not addressed by this focused pass-parity change. The 10/50-hour cases above remain CPU stress
references, not GPU runtime samples.

## Shared bounded clock implementation

The hourly phase reset is addressed in source with one shared Bevy 0.20 `ShaderBuffer` asset,
without a Bevy engine fork or per-material updates. A fixed 64-byte record contains bounded
vegetation radians, tree/leaf cycle fractions and gust strength for both the current and previous
frame. `crates/environment/src/wind_clock.rs` integrates shared rates with f64 delta time, wraps
phases, and computes gust strength from f64 elapsed time. It retains the existing `hash11` sequence;
integer slot selection avoids large-time division/cancellation and slot IDs remain exactly
representable through the day-long test window. The same handle is bound to every `GothicMaterial`;
a `PrepareResources` render system writes it once per frame. Per-material assets and bind groups are
not mutated for the clock. Color/depth/shadow paths consume the current record, while motion-vector
prepass consumes the saved previous record. Frequencies were verified shared: `WindEnv::new`
derives vegetation rates from `VegetationWind::original()` and SpeedTree values from the single
world wind packet; material-specific values are mode, height and leaf size, with grass focus shared
through the app clock buffer. This
supersedes the earlier proposal to extend Bevy `GlobalsUniform`; no engine patch or extra view bind
group is needed.

The environment release suite passes 28 tests, including encoded f32 shader-phase sin/cos checks for
all five oscillators across 3600 seconds and 24 hours, a rate/gust configuration change with exact
previous-record preservation, legacy gust comparisons at early and day-adjacent slots with nonzero
envelopes, and smooth gust samples at hour/day boundaries. The shader skips storage-record reads for
zero-weight or non-wind vertices. The standalone checker passes eight WESL/Naga variants, including
motion vectors, but does not validate GPU binding/layout. Viewer `cargo check` passes. Actual GPU
binding, fixed prepared-buffer identity, and visual continuity through color, depth, shadow and
motion-vector passes still require a runtime smoke before delivery. This storage binding requires
at least one vertex-stage storage-buffer slot; if a WebGL2 target is added, provide a fallback or
explicitly reject unsupported devices.

Before this change, the environment release suite passed 24 tests including the actual-offset gust-boundary regression. Current clock validation is CPU/source evidence; GPU hour-wrap and prepass silhouette parity remain open.

## Manager runtime validation — build5

The thin release build completed in6m52s. Binary SHA256:
`e638e428f27585fa7c75dd0228c99bd853dc2a86ee975a3884bb64aa05ad41ad`.
Both actual `--walk --smoke` modes (normal and `GR_NOWIND=1`) now pass with no shader,
import or renderer validation errors and grounded360/360 in each walk test. Logs and real
screenshots are in own `.local/qa/wind-prepass5/`; manager inspected normal-walk.png.
This validates the current world-mesh/depth/shadow runtime path; absent/compressed normals,
normal/deferred/motion passes have compiler coverage, not equivalent live GPU coverage.
Hourly clock continuity and complete silhouette/long-session visual proof remain open.

Normal-path A/B/A/B (previous candidate3 vs delivered water2, before the final conditional-guard
fix): median run average11.9551/12.0011ms, p95medians12.9127/12.9762ms, GPU3.4068/3.4089ms.
Same1280x720 scene, no compilers,2runs each,30s warmup/20s sample. Essentially equal, no
claimed speedup. Final guard fix leaves the exercised normal-wind condition unchanged.
Standalone checker passes8representative variants and rejects the historical unbraced normal
assignment. GPU pipeline-layout proof remains supplied by these actual runtime smokes.

## Clock manager validation — 14:05

Thin release candidate built8m18s, SHA25658c7554a9b6f3b4f425371ad3706853f450bf89daafdd6ac19391cf3b950fde0.
Actual normal and GR_NOWIND --walk --smoke runs PASS with no shader/renderer/import errors,
and grounded360/360 in both tests. Manager inspected actual normal-walk.png. Evidence in
own .local/qa/wind-clock/{normal,nowind}.log and images. This validates current GPU bindings
and rendering path; no hour-long GPU session or direct buffer-identity instrumentation run.

Controlled idle A/B/A/B,1280x720,30s warmup+20s sample,2runs per variant, compilers quiet:
previous Wind release12.0353ms vs shared-clock candidate12.0263ms (-0.075%). p95medians
13.3983/13.0837ms;GPU3.4099/3.4224ms. Essentially equal; no speedup claim. Raw JSONs and
summary own .local/bench-wind-clock, manual-renderer-log-review=false.
Final package/play QA pending. Earlier Desktop releases remain unchanged.

## Shared grass-focus prototype — source only

The app-side `WindClockFrame` now appends one focus `vec4` to the 16 phase/gust floats returned
by `WindClock::advance`; the shared storage buffer grows from 64 to 80 bytes. `grass_focus` keeps
the existing `observer_cm(&player) * CM` conversion, active `w=1`, `GR_NOGRASSFOCUS` gate, and
2 cm movement threshold, but updates the shared frame instead of mutating each vegetation
material asset. The shader reads this shared focus only for vegetation mode. Current and previous
prepass displacement use the same current focus, as before. `GothicParams.player` remains in the
uniform layout for this prototype but is no longer read by the wind shader.

Newly streamed vegetation materials now use the latest shared focus immediately. Previously, a
material created between focus updates retained the default inactive focus until the next movement
crossed the 2 cm threshold; this small initialization difference is intentional. The live count
of registered wind-material assets is not recorded in the saved clock smoke logs. `GR_WINDLOG`
prints that registry count when focus updates, and it counts unique cached material assets rather
than visible grass instances. This prototype has no matched moving-walk performance result yet;
the source-level reduction in asset mutations is a hypothesis, not a speedup claim.
