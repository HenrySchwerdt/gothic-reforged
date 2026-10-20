# Terrain / landscape format

Research date: 2026-10-20, Gothic 3 1.75 install. Evidence: local byte inspection plus the
Rust parsers `gothic-formats::{terrain, world_node, xcmsh}`. Reproduce with
`gothic-inspector terrain-survey` (below). Nothing from the original install is committed.

## Result: there is no heightfield

Gothic 3 stores its landscape as **ordinary static triangle meshes ("landscape cells")**,
not as a heightmap or a dedicated terrain file. All candidate terrain extensions in
`Projects_compiled.pak` (`.lrgeo`, `.lrgeodat`, `.sec`, `.secdat`, `.lrent`, `.lrentdat`, `.node`, `.wrl*`)
are world/sector/entity containers; none holds height samples. Full per-archive extension counts
and sizes are in `.local/extension-inventory.json` (generated; not tracked).

| Piece | Location | Role |
| --- | --- | --- |
| Render mesh | `_compiledMesh.pak`: `G3_<Region>_Landscape_01/LOD/G3_<Region>_Landscape_Cell_<N>.xcmsh` | Terrain triangles, several submeshes (materials) per cell |
| Collision / nav mesh | same folder, `.xnvmsh` (`_compiledPhysic.pak` holds other physics meshes) | Not parsed yet |
| Placement | `Projects_compiled.pak`: `G3_World_01/SysDyn_{9A103CC2-...}/G3_World_01_x<X>y0z<Z>_CStat.node` | One `eCVisualMeshStatic_PS` entity per cell, named `G3_<Region>_Landscape_01_<N>`, referencing the `.xcmsh` **and** the `.xnvmsh` |
| Low-poly distant world | `World/_Level/G3_World_Lowpoly_01_Levelmesh_01/*` (`.node`, 6.6 MB `_Spat.node`, `G3_World_Lowpoly_Landscape_01_SpG.node`) | Far-LOD world; not decoded |
| Material | `_compiledMaterial.pak` `G3_*_Landscape*.xshmat` | Terrain shader/material; splat/blend semantics not decoded |

The `.node` file *name* coordinates (`x85000y0z-15000`) are the 10 000-unit spatial-grid sector the
entity is **assigned to**; they are not the cell's geometric extent. Cells are ~17 000-25 000 units wide
and are listed in whichever sector the writer chose.

### Coverage (survey on the installed data)

* 2177 `*_CStat.node` documents scanned; 0 parse failures (after `world_node.rs` accepted duplicate source IDs;
  before that fix one Varant document, containing cell 235, was rejected).
* 782 landscape-cell placements: Myrtana 303, Varant 360, Nordmar 119. Every placement's `.xcmsh` and `.xnvmsh` exists
  in the archives and every archive cell is placed exactly once (no duplicates, none orphaned).
* 782/782 render meshes parse with `xcmsh.rs` (2 082 155 triangles total). No `.xnvmsh` parser exists yet.

## Coordinates and units (measured)

* **World matrix of every cell entity is identity** (translation 0): cell vertices are stored directly in
  world coordinates. Baking a cell therefore means "copy positions as-is"; `terrain::TerrainPatch` still applies the matrix
  (row-vector convention: X/Y/Z basis rows then translation) so a future non-identity placement works.
* **Y is up.** Averaged vertex normal Y over the Ardea export is +0.73 (82 % of vertices have normal.y > 0.5).
* Extents of the landscape: X -299 982 .. 193 491, Y -24 182 .. 50 554, Z -270 018 .. 228 612.
* **Unit: 1 unit = 1 cm (inferred).** Median terrain edge is ~201 units (about 2 m, plausible vertex spacing),
  sector grid is 10 000 units (100 m), vertical relief in the Ardea box is ~78 m. This is an inference from
  scale plausibility, not from a documented engine constant; verify against the hero collision capsule size
  once NPC/hero data is imported.
* **Winding:** stored triangle order is opposite to stored normals (cross(v1-v0,v2-v0) . normal < 0 for 47 444 of 47 449
  Ardea triangles), consistent with `XCMSH_LAYOUT.md`. Swap two indices per triangle (or flip the X axis together with
  the winding) when converting to a right-handed Y-up renderer such as Bevy. The parser preserves source order.
* **UVs tile**: Ardea UV values span roughly -1008..1014 (U) and -29..305 (V); textures repeat across cells.
  Material/texture blending for terrain is not decoded.

## Ardea export

Ardea (box X 80 000..100 000, Z -30 000..0) is covered by Myrtana cells 131, 132, 133, 154, 155, 156, 177, 178, 179.

```
gothic-inspector terrain-survey \
  "D:/.../Gothic 3/Data/Projects_compiled.pak" \
  "D:/.../Data/_compiledMesh.pak,D:/.../Data/_compiledMesh.p00" \
  --box 80000,100000,-30000,0 \
  --obj .local/ardea-terrain.obj --json .local/ardea-terrain.json \
  --out .local/terrain-ardea-summary.json --quiet
```

Result (local, ignored): 36 539 vertices, 47 449 triangles; world bounds X 79 486..101 389, Y -2 386..5 455,
Z -30 585..907. Triangles whose XZ AABB overlaps the box are kept (so bounds slightly exceed the box). The JSON has
`positions`, `normals`, `uvs`, `indices` (flat triangle list) and `cells`; OBJ has the same with `vt`/`vn`.
`--bounds yes` instead parses all cells and writes per-cell bounds to the summary. Pass the mesh archives
comma-joined; later entries override earlier ones (`.p00` patches over `.pak`). The command accepts native Windows-style paths.

## Not decoded / next steps

* `.xnvmsh` (collision + navigation) cell meshes: needed for walking. Same Genome envelope; class/stream layout unknown.
* LOD semantics: cells sit under a `LOD` directory and have multiple submeshes; whether submeshes are LODs or material
  groups is not established (all submeshes were merged in the export).
* Terrain materials (`G3_*_Landscape*.xshmat`) and texture blending, water meshes (`G3_*_Watermesh_*`), vegetation
  (`.spt` SpeedTrees; `Speedtrees.pak`), lightmaps (`Lightmaps.pak` `.xlmp`) are separate formats.
* `NavigationMap.xnav` (18 MB), `G3_World_01_NavZoneClusters.txt`, `..._CriticalNavObjects.txt`, `..._CriticalCollisionCircles.txt`
  in `Projects_compiled.pak` are the global nav data; untouched.
* Sector activation: `G3_World_01.wrldatasc` carries `[Sector.List]` `Name=true|false` start states which info scripts
  later change with `SetSectorStatus` (see `GAMEDATA_FORMATS.md`).

## Collision mesh `.xnvmsh` (decoded, 2026-10-20)

`.xnvmsh` is a GENOMFLE wrapper around several **PhysX 2.x cooked triangle meshes** (`NXS\x01MESH`).
Per stream: `u32 0, u32 cook_version (9|11|19), f32 0.001, u32 0xff, u32 0, u32 vertex_count, u32 triangle_count`,
then `vertex_count * 3 * f32` positions in **metres** (x100 = render-mesh cm space), then `triangle_count * 3`
indices (u8 when `vertex_count <= 256`, else u16); the rest (material ids, adjacency, `OPC` tree, `HBM` hull)
is ignored. Reader: `crates/formats/src/xnvmsh.rs` (signature scan, every stream validated, bad streams skipped).
Verified on Myrtana cells 131/155/178: 6-7 streams each, bounds x100 equal the render cell bounds, 6 516 / 18 933 / 28 971
triangles (render meshes hold more). Submeshes of the render mesh are **material groups, not LODs** (disjoint triangles).

## Walkable region (`gothic-reforged --features viewer -- --walk`)

* World is mirrored on X (`F = diag(-1,1,1)`) for Bevy: this flips handedness and winding together, so source index order
  is kept and **single-sided back-face culling renders correctly** (verified visually on terrain and houses). Instance
  matrices become `F*M*F`.
* Terrain cells are re-binned into 25 m tiles by triangle centroid; collision per tile is the xnvmsh triangles (render
  triangles if a tile has no collision cell), plus the render triangles of opaque static props.
* Terrain material blending (splat/detail) is undecoded: each submesh uses its first diffuse texture, which shows as flat
  pale patches where the original blends layers.
