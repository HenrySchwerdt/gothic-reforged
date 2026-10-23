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

All logs, environment maps and images are under `.local/qa/white-patches/`.
Additional foliage/static isolation is in progress, without builds/source edits.
If necessary the next source probe should retain entity transform/part kind,
geometry/resource provenance, material/layer identity and a0 absence reason.
Current `GR_DUMP` is restricted near `(40,40)` and is not general attribution.
