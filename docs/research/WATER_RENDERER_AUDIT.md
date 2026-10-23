# Water renderer audit

Date: 2026-10-22

## Findings and source evidence

- Previously, `apps/game/src/walk.rs::spawn_part` assigned `Caches::sea_y` from the first
  streamed water part, then hid every part within 0.3 m of that level. `spawn_pending` also
  removed all water triangles in that height band from `WaterSurfaces`. An inland lake/river
  loaded first could therefore suppress unrelated water and drive the camera-following ocean
  plane and hero water-level query.
- The original-world placement query supplied by the manager is retained at
  `C:\Users\PowerTower\dev\gothic-reforged\.local\research\codex-water-placements.json`.
  Near Ardea it identifies `G3_World_Oceanmesh_068.xcmsh` (entity
  `a4e8defc993f384198abcab49747e27300000000`) and sibling Oceanmesh 069/051/062 instances at
  matrix Y approximately -453.168 cm. The same query includes `G3_World_Rivermesh_01_Depth_118...`
  at other heights. The committed world survey contains only the generic resource pattern
  `G3_*_Watermesh_*`; concrete per-instance names come from the ignored local query above.
- The water shader's deep/shallow colour mix remains view-angle-driven (`ndv`); it has no
  refraction input. The shader now adds a screen-space shoreline alpha fade from scene depth.
  Native-engine notes in `docs/research/ENGINE_MAP.md` describe shore-fade/depth-colour inputs,
  but their values are not decoded by this renderer.

## Changes

- The walk camera now requests Bevy's `DepthPrepass`. The water shader samples it through
  `bevy_pbr::prepass::utils::prepass_depth` and linearizes scene/water NDC depth through
  `bevy_pbr::render::view_transformations::depth_ndc_to_view_z`. Bevy 0.20 uses reverse-Z
  (clear depth 0; visible geometry positive); the shader fades alpha by positive view-space
  separation. `WaterMaterial::enable_prepass() == false` stays in effect, so water cannot
  occlude itself in the prepass. The 3 m transition distance is an explicit renderer tuning
  value, not recovered Gothic material data.
- `is_confirmed_ocean_resource` recognizes only the evidence-backed basename pattern
  `G3_World_Oceanmesh_<digits>.xcmsh`, case-insensitively. River, landscape-water and other
  water-material meshes remain tile surfaces regardless of height. `Part::ocean_plane_y_m`
  uses every transformed vertex of a confirmed ocean part; it returns a sea height only when
  all vertices are within 1 cm in Y. Non-planar ocean parts remain visible as source meshes.
  Only confirmed planar Ocean parts at the selected Ocean vertex height are hidden/replaced;
  the 0.3 m water-level removal heuristic is gone.
- Water `Part`s retain their source resource name and world-index entity id through a shared
  `Arc`; non-water parts do not allocate or retain this metadata. `GR_WATER_AUDIT=1` logs a
  `WATER_SOURCE` row for each water part: resource, entity id, material, ocean classification,
  computed plane Y, placed Y, projected XZ area and triangle count.
- Unit cases cover the confirmed Ocean pattern and reject the Ardea river-depth and Myrtana
  landscape-water naming patterns. Tests were added but not run per task constraints.

## Runtime checks needed

1. Run with `GR_WATER_AUDIT=1` at startup and after streaming coast/inland tiles. Retain the
   `WATER_SOURCE` rows under `.local/` and verify the logged Ocean planes against the placement
   evidence above. Check any Ocean row with `ocean_plane_y_m=None` and keep it visible.
2. Capture Ardea coast, inland lake/river, tile seams, camera high above land, and underwater.
   Confirm lakes/rivers stay visible regardless of sea height and the ocean plane uses the
   transformed mesh-vertex height.
3. Confirm the fade from multiple camera angles. Its depth gap is measured along the view ray,
   so its apparent horizontal width changes with viewing angle.
4. Compare vegetation silhouettes at the water edge. The vegetation material's prepass vertex
   path must match its wind-deformed colour path to avoid stale depth silhouettes.
5. Measure the frame-time cost of enabling the depth prepass in walk mode.

## Manager validation and depth-hull correction

At 08:33 Europe/Berlin, coordinated release tests passed: 24 environment, 100 format unit,
8 format integration and 14 render-rule tests (146 total, one additional test ignored).
The pure source/planarity rules now live in `gothic-render-rules::water`.

The first built candidate rendered without shader errors, but actual coast screenshots still
showed the large cyan/gray triangles. Scene-depth alpha alone did not resolve that defect.
Further original-asset inspection found `G3_Myrtana_Rivermesh_Depth_01_A.xshmat` declares a
framed `eCShaderWater` object with no texture samplers. Its filename lacks "water", so the
old loader rejected it and rendered the depth hull as a fallback surface. The sampled hull
has a 23 m local Y span. The new bounded object-class query identifies that shader family;
only the confirmed `G3_World_Rivermesh_01_Depth_<digits>.xcmsh` family with that untextured
water material is excluded from visible geometry and its inappropriate collision/water-level
surface paths. Replacing its proxy role with scene depth is an implementation choice; native
pass equivalence is not established by this observation alone.

Important counterexample: `G3_Myrtana_Water_Sea_01_A.xshmat` also has no samplers. It must
remain water, not become a solid fallback surface or be deleted solely for lacking textures.
`MatKind::WaterUntextured` records the ambiguity, and `is_water()` includes it. Only the
additional confirmed depth-mesh identity permits suppression. Parsed original material reports
are retained in this checkout's `.local/{depth,ocean,sea}-material-parsed.json`; the inspector
now supports `material-info ARCHIVE MEMBER` to reproduce this check.

`GR_HIDE=water` was previously bypassed by the early water return and the ocean-follow system.
Both visual paths now honor it without changing water physics. Source Seamesh and low-poly
water overlaps remain candidates for further inspection; their roles are not inferred merely
from their elevations. The corrected candidate still needs its new coast screenshots and
controlled performance comparison; the first candidate is not the final water result.


Final candidate validation: thin release built; actual matched codex-coast-fixed-low.png and codex-coast-fixed-shore.png in primary .local/qa show the triangles/bright strip removed. No shader errors; walking grounded360/360. Final Desktop package melee_faithful QA PASS. Controlled A/B performance is recorded separately in the handoff when complete.
