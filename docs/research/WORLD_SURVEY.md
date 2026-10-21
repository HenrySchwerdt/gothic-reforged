# Full-installation world and mesh survey

2026-10-20, Gothic 3 1.75 (`Data/*.pak` + `*.p00`, read-only). Reproduce:

```
gothic-inspector world-survey  "<G3>/Data" .local/world-survey.json
gothic-inspector mesh-survey   "<G3>/Data" .local/mesh-survey.json
gothic-inspector world-query   "<G3>/Data" X Y Z RADIUS      # spatial index smoke test
cargo run -p gothic-importer --release --example survey -- world|mesh "<G3>/Data" OUT.json
```

Each run takes about 6 s (release). JSON reports (counts, names, error classes,
never payload bytes) stay under the gitignored `.local/`. Code:
`gothic_formats::vfs`, `gothic_importer::{survey, world_index}`.

## Virtual file system

25 archives mount in this order: all `.pak` sorted by name, then `.p00..p99`
by patch number and name (`_compiledmesh.p00`, `music.p00`,
`projects_compiled.p00`, `speech_english.p00`, `speech_german.p00`). Later
layers replace earlier ones by case-insensitive path: 153 517 visible members,
55 overridden. The three CPT archives are not G3V0 and are skipped by the
`.pak`/`.pNN` filter (not mounted, not an error).

`_compiledMesh.p00` contains eight payloadless records (offset, stored and
decoded size all zero), all `G3_Master_Collisions_01/G3_Object_Strawbale*_COL.xcmsh`.
**None of them has a base entry** in any lower layer, so they cannot be
deletions of existing content. The VFS therefore ignores them (never shadows a
lower layer) and lists them in `ignored_payloadless` with `base_exists`. If a
future archive has a payloadless record whose `base_exists` is true, that is the
signal to revisit deletion semantics. `Projects_compiled.p00` holds only
`stringtable.bin` and `compiledinfos_G3_World_01.bin` (no world documents).

## World documents (`*.node`, `*.lrentdat` in Projects_compiled)

| Result | Count |
| --- | ---: |
| documents | 2 536 (2 410 node + 11 unlisted level meshes, 112 lrentdat, 3 unfinalized and unused) |
| parsed | **2 533 (99.9 %; the other 3 are never loaded by the engine)** |
| entities | 231 223 (133 419 with a mesh reference) |
| mesh/asset reference occurrences | 151 272 (xcmsh 133 594, xlmsh 1 074, fxa 16 604) |
| unique mesh references | 4 768 |
| resolved against `_compiledMesh.pak/.p00` | 4 757 (basename lookup) |
| unresolved | 11 |
| resolved meshes that parse | 4 757 / 4 757 |
| xlmsh LOD levels | 212, all resolve to an xcmsh |
| placements in `WorldIndex` | 134 668 (xcmsh/xlmsh, from 2 522 documents) |

Fixes made for this: SysDyn main `.lrentdat` (80 MB) failed the
text budget because property names were allocated per property; names/types are
now borrowed and only retained strings are charged. Entity cap raised to 4 M
(still bounded by remaining-bytes / 298). One CStat cell
(`x-125000y0z-145000`) repeats an entity source ID, so IDs are no longer
required to be unique (identity is document + index). The ten-byte
`GENOMEDL` + u16 placeholder (seven GUID `.lrentdat` stubs) is an empty
document.

### The 24 former failures (resolved 2026-10-21, verified against the decompiled loaders)

All 24 are now accounted for: **21 parse** and **3 are proven unused** (not failures).
Reproduce: `ow_try FILE...` (parse any extracted document), `ow_refs DATA_DIR` (which layers any
sector names), `ow_region DATA_DIR` (entities / `gCNPC_PS` sets per region), all in
`crates/importer/examples/`. Function addresses are from the decompiled `Engine.dll`
(`.local/research/engine/`).

**How the engine reads a layer.** A sector (`.secdat`: `u16` version, `u32` dynamic count,
`u32` geometry count, then that many names) lists its layers *by name*
(`gCSector::DoLoadData` 0x10193c40 -> `ImportDynamicLayer`/`ImportGeometryLayer`); a name `X`
resolves to the header `X.lrent`/`X.lrgeo` and the data file `X.lrentdat`/`X.lrgeodat`/`X.node`
(`eCProcessibleElement::GetDataFilePath`, `LoadData` 0x1005d2f0). Nothing enumerates a directory
for layers. `LoadData` and `eCGeometrySpatialContext::ReadNodes` 0x1021c080 handle an envelope:
if the first eight bytes are `GENOMFLE`, read `u16` version and `u32` table offset, **seek to the
offset and require `0xDEADBEEF` there (`ge_archivefile.cpp` line 0x1f2, `CallFatalError`
otherwise)**, load the string table (`FUN_1029f220`), seek back. `eCArchiveFile::Read(bCString)`
0x102a7600 returns `table[u16 index]` when a table is loaded and a plain `u16 length + bytes`
string when not. The writer (`eCArchiveFile::Open` 0x102a7280 for writing) emits
`GENOMFLE`, version 1 and a **placeholder offset 0**; only `Close` 0x102a7500 appends the table
and patches the offset.

1. **Three never-finalized `{GUID}.lrentdat` layers: unused (proven).** Montera dynamic objects 03
   (6.9 MB), Varant outdoor dynamic objects 02 (3.1 MB), Morasul outdoor NPC 01 (0.4 MB) still
   have the placeholder offset 0: the editor wrote the body but never ran `Close`, so the string
   table was never produced anywhere. That is why no companion table exists in any archive, and
   why `stringtable.bin` / `compiledinfos_*.bin` do not hold the strings. The shipped engine
   could not load them: offset 0 makes `LoadData` read `GENO` where it requires `0xDEADBEEF`
   (fatal error), so they must never be reached, and they are not: (a) the three directories have
   **no `.lrent` header** (`ow_refs`), (b) the three sectors' `.secdat` list **zero dynamic
   layers** (Montera 03 and Varant 02: geometry layer `*_SHyb` only; the Morasul one has no `.sec`
   at all), (c) no `.secdat` anywhere names them. Likewise every other header-less GUID
   `.lrentdat`/`.lrgeodat` leftover (the five 124-byte ones parse as empty documents).
   `WorldDocument::parse` keeps reporting them as an error with the explanation, and
   `world_node::is_unfinalized_layer` lets tools classify them; the survey counts them as
   `documents_unfinalized_unused`. Not recoverable, and not needed.
   *Where the Mora Sul / Montera / Varant NPCs really come from:* the named, finalized layers
   (`ow_region`): Mora Sul city 218 entities / 69 `gCNPC_PS` (`G3_Varant_01_MoraSul_NPC_01/02`),
   Mora Sul temple 299 / 101, Montera city 637 / 195, Varant `_outdoor` 26 / 4, Faring 466 / 144;
   the outdoor population is in the huge `SysDyn_{9A103CC2}` system layer (26 976
   entities, 5 888 NPC property sets) and the Myrtana `_Outdoor` layers (2 631 entities, 2 629 NPC sets). The per-region counts
   are in line with the other cities (Ardea 211 / 61, Geldern 440 / 137, Silden 432 / 129), so
   nothing is missing from the shipped game: the orphan layers are development leftovers (the
   Morasul one sits in the only `Outdoor_MoraSul_NPC` directory that has no sector at all).

2. **Eleven `*_SpG.node` + two `*_Spat.node`: parse; the eleven SpG are unused.** They are
   *raw* documents with no envelope. `ReadNodes` reads a `u16` version from byte 0 (the shipped
   files carry 30, 56 or 72) and dispatches: **< 56 -> `ReadNodesOld` 0x1021b9c0** (inline
   `u16 length + bytes` strings), **56..82 -> `ReadNodesV56` 0x1021bc30**, which reads a
   `bCPOStream` (`ReadFile` 0x10057b50): `u16` stream version 1, one flag byte, `u32` count and
   that many inline strings, `u32` payload size, then a payload in which every string is a `u16`
   index into that list (`bCPOStream::Read(bCString)` 0x10057da0). Payload: `u32` entity count,
   per entity two bools (has template, patching disabled), `eCSpatialEntity::Read` 0x10228d50
   (`u16` 0x1e..0x22: template flag + 20-byte id, 0x23 adds 84 bytes of boxes),
   `eCEntity::Read` 0x102257f0 -> **`ReadV63` 0x10225840** for entity versions below 83,
   trailing 20-byte template id if has-template and not patching-disabled, then the parent/child
   pairs ended by `(-1, -1)` (`ReadDependency` 0x1021a150). `ReadV63` differs from v83 by:
   per-version extra bools, `bStack_71` flag before the alpha, an inline name, two spheres and
   floats, the property sets in `u16 version, u32 size, name, accessor object, 0xDEADC0DE`
   records, and **objects whose `bCPropertyObjectBase::ReadData` 0x10056ca0 header depends on the
   object version**: 1 = class name string + 20-byte id, 2..81 = 20-byte id, 82+ = nothing.
   All 13 parse (`WorldDocument::parse`, bounded, synthetic tests in `world_node.rs`).
   Only the two `_Spat` documents are listed by a sector (`Faring_VFX`, `MoraSul_Temple_Lights_01`:
   a lone root entity each). The eleven `_SpG` documents sit in `*_Levelmesh_*` directories that
   have **no `.sec`/`.secdat`** and no sector names them (`ow_refs`): old landscape/water level
   meshes that were replaced by the CStat cells; their mesh names are dangling (485 of them do
   not exist in any archive). `survey::is_unlisted_levelmesh` therefore excludes them from the
   entity/mesh totals and from `WorldIndex` (`documents_unlisted_levelmesh`: 11).

3. **Three raw `GENOMEDL` layers: parse and are used.** `gCDynamicLayer::DoLoadData` 0x100c5a50
   branches on the first eight bytes: `GENOMEDL` + version <= 56 goes through the same
   `bCPOStream` path as item 2 (`gCWorld::OnLoadDynamicContext` 0x1019c420), anything else
   without a `GENOMFLE` header (the `G3_StartUp` `SysDyn_{D9B95D54}` layer starts directly with the
   accessor bytes `01 00 01 01 00 01`) reads the accessor with inline strings. Entities are
   `eCDynamicEntity` below version 83 (`u16` derived-class version, `u16` 1, extra bool, creator
   flag) followed by `ReadV63`. Faring VFX (551 B, 1 entity), Morasul temple lights (43 KB,
   92 lights) and the StartUp SysDyn (2 entities) all decode; their sectors list them
   (`G3_Myrtana_01_Faring_VFX`, `G3_Varant_01_MoraSul_Temple_Lights_01`).
   Typed property decoding (`parse_with_properties`) is not implemented for the pre-83
   layouts: it returns one empty set list per entity.

Totals now: 2 536 documents, 2 533 parse, 3 unfinalized-unused, 0 failures; 231 223 entities
counted (11 unlisted level meshes with 785 further entities excluded), mesh numbers unchanged
(4 768 unique, 4 757 resolved, 11 dangling); `WorldIndex` 134 668 placements, 3 documents
reported as failed.

The unresolved 11 mesh names (`G3_Armor_Shield_Tower_03..07`,
`G3_Weapons_Melee_1H_Sword_Normal_01`, `..._2H_Axe_Double_02`,
`..._2H_Halberd_01`, `G3_Object_Interact_Pick_Ore_01`,
`G3_Object_DemoSign_E3_01`, `G3_World_Border_Demo_001`) exist in no archive
(not just not in `_compiledMesh`); they are dangling references in shipped data.

## Meshes (`.xcmsh` / `.xlmsh` in `_compiledMesh.pak/.p00`)

**5 510 / 5 510 parse (100 %)**: 5 402 xcmsh, 108 xlmsh. What was added (checked
against 5 510 real resources, tail must end exactly on the table boundary):

- Class version is any of 1..=83 for both formats (rule from the community
  reader: version < 81 has an extra string, < 82 has 20 extra bytes). 1 052 meshes
  and 83 LOD files were classversion 1 or other historical versions.
- Resource version 22..30 variants (optional u32, optional u16/bool).
- Submesh versions 1..=5: nested arrays only above v1, A/B arrays above v2, C/D
  arrays above v3.
- Stream tags 2 (16-byte transformed positions), 6, 15, 18, 21, 64, 72, 73 are
  length-checked and skipped; 4/5 stay opaque. 758 meshes used these.
- XLM SH may be GENOMFLE-wrapped (table indices) as well as raw.
- NaN/inf UV values (one river mesh, `G3_Myrtana_Rivermesh_24_Part_02`) are
  sanitised to 0.0 rather than rejecting the mesh; positions stay strict.

## Spatial index

`WorldIndex::build(&mut vfs)` parses every world document and stores one
`WorldEntityRecord { entity_id (hex), name, document, resource, world_matrix }`
per mesh reference (strings interned). `query(center, radius)` is 3D,
`query_xz` ignores height; grid cell 2 500 units in X/Z; results are returned in
insertion order and checked against brute force in tests. Positions are source
translation vectors; matrices are untouched. A full build takes about 1.8 s.
Radius uses entity origins, not mesh bounds; viewers should pad by mesh extent.
Verified: query at the Ardea torch `(92424.8, 5220.3, -10935.7)` r=600 returns
the torch from the `Ardea_NPC_01` document and CStat barrels from the
`x95000y0z-15000` cell.
