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

## Follow-up: shared wind clock without engine changes

After the current color/depth/shadow shader path passes runtime pipeline and silhouette checks, the
preferred clock implementation is one shared Bevy 0.20 `ShaderBuffer` asset, not a Bevy engine
fork or per-material updates. `bevy_render::storage::ShaderBuffer` is the asset-side type and
`GpuShaderBuffer` is its prepared render-world counterpart. Add one `#[storage(8, read_only)]`
handle to `GothicMaterial`, create the fixed-size asset once, and give the same handle to every
Gothic material. In a render-world system in `RenderSystems::PrepareResources`, resolve its
`GpuShaderBuffer` from `RenderAssets` and call `RenderQueue::write_buffer` once per frame. Bevy
prepares assets before `PrepareResources` and bind groups after it. Keep the asset and GPU buffer
size/identity fixed and do not mutate `Assets<ShaderBuffer>` after initial preparation; direct
content writes retain the buffer identity and do not invalidate cached material bind groups.

Local Bevy 0.20 authority is the Cargo source at
`%USERPROFILE%\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\bevy_render-0.20.0\src\`:
`storage.rs` defines `ShaderBuffer`, `GpuShaderBuffer`, and `RenderChangedShaderBuffers` (including
the content-only/no-bind-group-invalidation contract); `render_asset.rs` registers and prepares
`RenderAssets`; `lib.rs` defines and orders `RenderSystems::PrepareAssets`, `PrepareResources`, and
`PrepareBindGroups`; `renderer\mod.rs` exposes `RenderQueue` (whose wrapped `wgpu::Queue` provides
`write_buffer`). This repository's `crates\environment\src\wind.rs` `WindEnv` is the source of shared wind rates and
gust parameters; app-side phase accumulation belongs with its update path, while the GPU upload
and shared material handle belong in `apps\game\src\walk\gmat.rs` (or a narrowly
scoped adjacent render module if that file becomes unwieldy).

The shared record should contain bounded cycle fractions for the two vegetation phases, tree sway,
leaf rocking and leaf rustling, plus the current gust strength. Maintain phases in an app-side f64
clock by integrating the current global frequencies with `delta_secs_f64()` and wrapping each phase
to `[0, 1)`. Compute gust strength on the CPU from f64 elapsed time and integer-hashed gust slots;
the shader then reads the shared record once per vertex and never multiplies an unbounded elapsed
time by a frequency. The current `WindEnv` frequencies and gust parameters are shared across Gothic
materials, so this record is view-global in practice. This design supersedes the earlier proposal to
extend Bevy `GlobalsUniform`; no Bevy engine patch or extra view bind group is needed.

Validate phase encoding with synthetic times immediately before and after 3600 seconds and 24
hours, plus long-running phase samples and gust-slot transitions. Then inject those records through
the render upload path and verify adjacent GPU frames across the hour boundary for branch, leaf,
depth-prepass and shadow continuity. Confirm the prepared GPU buffer identity stays fixed and no
material assets or bind groups are rebuilt per frame. This storage binding requires at least one
vertex-stage storage-buffer slot; provide a fallback or reject unsupported devices explicitly if a
WebGL2 target is added.

Manager validation: the environment release test suite passed all 24 tests, including the actual-offset gust-boundary regression. This is CPU evidence; GPU hour-wrap and prepass silhouette parity remain open.

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