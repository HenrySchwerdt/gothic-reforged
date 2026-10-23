# Remaining white/checker patches

The delivered AimPerf overview still contains isolated pale checker-like patches.
This is separate from the previously fixed broad grass fade/prepass problem.
No material has yet been attributed to an individual patch.

## Evidence and candidate mechanism

Accepted package, fade and leaf-phase logs contain no missing-texture/material
errors. That does not exclude silent fallback. In `apps/game/src/walk.rs`,
`texture_image` accepts only DXT1/3/5 with dimensions divisible by four;
`image_handle` propagates rejection as `None`, while `surface_material` still
creates the material. `apps/game/src/walk/shaders/gothic.wesl` samples `tex_a0`.
Pinned Bevy0.20 `AsBindGroup` substitutes a fallback for an absent optional image;
the texture fallback is opaque white. This is a concrete possible white-surface
path, **not proof that these patches use it**. Do not change formats or suppress
meshes without identifying the affected resource.

Source references and limits: `.local/qa/white-patches/code-audit.md`; log audit:
`.local/qa/white-patches/log-audit.md`. Existing SPT atlas/cache evidence does not
identify a screenshot pixel or its live material.

## Controlled category isolation

Use the unchanged delivered EXE, SHA256
`445c3773b4de48cabad3a94c07a5d17ea799a149834d2da5377605047e99b47f`, cwd
`C:/Windows/Temp`, clean child environment and `GR_RES=1280x720`. Exact equivalent
overview pose is metres `(15,140,90)`, yaw0, pitch-0.55; same70° perspective.

```text
--walk --smoke --screenshot ABS_PREFIX --wind-shot-times 3600
--shots fixed-overview:15,140,90,0,-0.55
```

Normal versus `GR_HIDE=tree`: both exit0, grounded360/360; selected/captured CPU
wind current/previous records and poses match. No GPU readback claim. Analyze
only `*-fixed-overview.png`; the earlier default overview is uncontrolled.
Root inspected both actual PNGs. Trees disappear and visible meshes2295→2074,
but the small checker patches remain. Thus hiding trees does not eliminate them;
this does not identify another category or prove a texture fallback.

Further category captures completed with the same controlled pose/time: hiding
foliage leaves the patches (2,275 visible meshes); hiding static geometry removes
them along with the village and many unrelated objects (1,331 visible meshes).
All four captures exit0 and report grounded360/360. This narrows the investigation
to content affected by static hiding, without identifying an individual object.

## Cull/LOD diagnostic and material sample

The same EXE with `GR_DUMP=1` produces the baseline image
`dump-static-fixed-overview.png`. Adding only `GR_NOCULL=1` produces
`nocull-static-fixed-overview.png`: most checker patches disappear, while some
white geometry remains. Root inspected both PNGs and the approximate close view
`dump-static-central-close.png`. The close pose is an exploratory guess, not a
mapping to a known mesh. Visible meshes increase from2,295 to5,261 with NOCULL;
all LOD stages may overlap. This is diagnostic evidence of cull/range/LOD-sensitive
content, **not a production fix or a same-quality performance comparison**.

Both overview captures match selected/captured CPU wind records at3600s and the
same pose; both smoke runs exit0 with grounded360/360. Normal dump child env is
exactly `GR_DUMP=1, GR_RES=1280x720`; NOCULL adds only `GR_NOCULL=1`.
The close shot uses3600.016666667s. These are not GPU buffer readbacks.

The baseline dump samples29 large nearby static parts; all have a material and
first-layer texture dimensions, with zero missing tex0 entries. None is linked
to an individual checker patch. Zero DUMP lines with NOCULL is expected:
`walk.rs` places the dump inside `Static && GR_NOCULL absent`, so the diagnostic
itself is bypassed. Do not interpret this as a load failure or compare dump
counts as scene completeness. See `.local/qa/white-patches/dump-summary.md`.

Static small-object ranges and mesh LOD stages use Bevy VisibilityRange;
forward and prepass shaders both contain its dither call. The previous F_FADE
foliage fix addresses a different path. A pinned-Bevy specialization audit is
complete: both custom vertex shaders compute the level from the same instance
index and world translation; `GothicMaterial::specialize` retains inherited
definitions. Root verified pinned Bevy0.20 `prepass/mod.rs:581-582,1189` and
`render/mesh.rs:3716-3717` propagate VISIBILITY_RANGE_DITHER. No discrepancy was
found in this narrow path. This does not establish runtime pipeline parity.

A separate alpha discrepancy is confirmed in source: `GothicParams::default`
sets `misc.z=0.5` (`gmat.rs:419-427`), and both custom prepass fragment variants
unconditionally discard `a0.a < misc.z` (`gothic_prepass.wesl:138-140,159-161`).
Forward applies its cutoff only under F_MASK (`gothic.wesl:119-120`).
`surface_material` marks mask blend mode1 with F_MASK; ordinary opaque materials
retain no mask flag (`walk.rs:936-962`, with a separate foliage exception).
Thus opaque low-alpha texels can disagree between depth and color. Blend/Add
also lack F_MASK, but their actual participation in depth/shadow passes must be
checked before making any claim about those modes. No affected static texture
has yet been shown to have such alpha, and this discrepancy is **not attributed
to the checker patches**. Do not ship an assumed visual fix without reproduction.

Next discriminating work: a single same-pose GR_LODG factor change can distinguish
static LOD-stage dependence from the broader NOCULL result. Then identify one
actual patch's entity/material, blend mode and alpha coverage, before modifying
alpha discard semantics. A depth/object-ID or targeted pipeline probe may be
needed. Keep all production visibility and quality defaults unchanged meanwhile.

All logs, environment maps and images are under `.local/qa/white-patches/`.
No source behavior, delivered executable, original data or foreign checkout was
changed for these captures. If necessary the next source probe should retain
entity transform/part kind, geometry/resource provenance, material/layer identity
and a0 absence reason, together with the actual depth/pipeline state.
Current `GR_DUMP` is restricted near `(40,40)` and is not general attribution.

## Resolution in the static depth-dither build — 2026-10-23

The follow-up static dither fix is compiled in source `09332fc` and packaged in
`GothicReforged-Codex-20261023-DitherFix` (EXE SHA-256
`ec1fcc0688e64ddd09091702c84a6e11dafcba5d337e8f88eb627e0b52e9bf94`). The
controlled overview at the same pose and synthetic wind time no longer shows
the previously visible checker artifacts; root inspected the actual image.
The mechanism is the missing colorless fragment stage for opaque visibility
range dither in the depth prepass, while preserving inherited shader definitions
and matching alpha-test behavior to F_MASK. Both changes shipped together; the
comparison does not separately quantify each change's contribution. This is
evidence for the fix in the checked scene, not
a claim about every material or scene.

The packaged EXE passed melee/magic/bow QA (13/9/15) and a 360/360 grounded
walk smoke. Logs/screenshots are under `.local/qa/static-dither-package/` and
`.local/qa/static-dither/`. An initial run of the unbundled EXE failed to find
runtime `.local/items.json` from Temp; packaging supplied the runtime data and
the final packaged QA then passed. No performance or VR validation was done.
