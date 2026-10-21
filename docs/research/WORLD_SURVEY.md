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
| documents | 2 536 (2 408 node, 104 lrentdat parsed, 24 failed) |
| parsed | **2 512 (99.05 %)** |
| entities | 231 126 (133 419 with a mesh reference) |
| mesh/asset reference occurrences | 151 272 (xcmsh 133 594, xlmsh 1 074, fxa 16 604) |
| unique mesh references | 4 768 |
| resolved against `_compiledMesh.pak/.p00` | 4 757 (basename lookup) |
| unresolved | 11 |
| resolved meshes that parse | 4 757 / 4 757 |
| xlmsh LOD levels | 212, all resolve to an xcmsh |
| placements in `WorldIndex` | 134 668 (xcmsh/xlmsh, from 2 512 documents) |

Fixes made for this: SysDyn main `.lrentdat` (80 MB) failed the
text budget because property names were allocated per property; names/types are
now borrowed and only retained strings are charged. Entity cap raised to 4 M
(still bounded by remaining-bytes / 298). One CStat cell
(`x-125000y0z-145000`) repeats an entity source ID, so IDs are no longer
required to be unique (identity is document + index). The ten-byte
`GENOMEDL` + u16 placeholder (seven GUID `.lrentdat` stubs) is an empty
document.

### The 24 remaining failures (precise classes)

Open-world pass (2026-10-21): **5 of the 24 now parse** (the five 124-byte table-less layers
below are empty documents); 19 remain, with the structure findings added under each class.
`ow_scan` / `ow_extract` / `ow_find` / `ow_props` (`crates/importer/examples/`) reproduce the
numbers; extracted bytes stay under `.local/legacy/`.

1. **Eight GUID `.lrentdat`, GENOMFLE header with table offset 0, no
   `0xDEADBEEF` table anywhere** (Montera dynamic objects 03, Nordmar
   outdoor NPC 01, Varant outdoor dynamic objects 02, Varant ruins dynamic
   objects 01, Alshedim, Morasul outdoor NPC 01, two Morasul temple groups;
   Montera is 6.9 MB). Body is the normal v83 layout (`GENOMEDL`, u16 83, context
   class, 0x40/0x53 dynamic entities, `0xDEADC0DE` terminators, ~18 000
   property sets) but every string is a table index and the table is missing.
   Not in the shared `stringtable.bin` (dialogue text) nor
   `compiledinfos_G3_World_01.bin` (info/dialog classes). Context-class size is 0.
   Unresolved: where the engine takes the table from. Not guessed.
   Findings (2026-10-21): five of the eight are 124-byte layers that hold only the context
   (`GENOMEDL`, u16 83, context class with string index 0, size 0, then u16 30 and two indexed
   properties = the context box and `RootEntity`/path, no entity count): **parsed as empty
   documents** (`WorldDocument::parse_tableless`). The other three carry content (Montera
   dynamic objects 03 6.9 MB, Varant outdoor dynamic objects 02 3.1 MB, Morasul outdoor NPC 01
   407 KB) in the normal v83 entity layout (`0x40`/`0x53`, 20-byte ids, `0xDEADC0DE`) with every
   string a `u16` index; the indices are assigned in order of first use (context class 0, its
   property names and types 1..4, ...), so the table is the file's own string sequence and
   cannot be recovered without it (entity names, mesh and voice names, scripts are values).
   Their sectors (`Montera_Dynamic_Objects_03`, ...) also have a named `*_SHyb.node` (22 KB) and
   `.sec`/`.secdat` with normal tables that do not hold the strings; the Morasul NPC GUID layer
   has no `.sec` at all (an orphan, probably never loaded). No archive holds a companion table
   (searched every archive for the GUID). Unresolved, not guessed: the characters of that
   layer are missing from `npcs.json`.
2. **Eleven `*_SpG.node`** (Myrtana landscape 02/03/04, water depth/test,
   Faring dummy water, Vangard magic barrier, Nordmar and Varant location add-ons,
   Fireclan/Wolfclan city) **and two `*_Spat.node`** (Faring VFX, Morasul lights):
   *no GENOMFLE envelope*, u16 30, u32 entity count, then legacy spatial version
   0x22 with common-entity version 0x38 (56) and inline length-prefixed strings.
   Header fields differ from v83 (nine flags before alpha, two after, a 19-byte
   tail then `u32` values that are not the v83 scaling/flag pair, class records as
   `u16 0x32, u32 size, inline name`). These contain level-mesh/water entities
   (`eCVisualMeshStatic_PS` appears inline). Unparsed; landscape geometry for
   Ardea comes from the CStat cells (`terrain.rs`), not these.
   Findings (2026-10-21), `g3_test_faring_dummy_water_01_spg.node` (3 KB): `u16 30`, `u32 3`
   entities, each `u16 0`, `u16 0x22`, `u16 0x38`, `u16 1`, 20-byte id, flag bytes, inline
   `u16 len + name` (`Root`, ..., `G3_Dummy_Faring_Water_01`), two 4x4 matrices and boxes, then
   a count and the property sets `u16 0x32, u32 size, u16 len + name` (`eCVisualMeshStatic_PS`).
   The field-level layout between the matrices and the box tail is not decoded; the contents
   (dummy water, level mesh remnants, a magic barrier, add-on locations) are not part of the
   playable Ardea slice.
3. **Three raw `GENOMEDL` v56 `.lrentdat`** (`G3_StartUp` SysDyn, Faring VFX,
   Morasul temple lights): same legacy generation as 2, plus an inline string
   table directly after the first accessor. Unparsed.
   Findings: the raw `GENOMEDL` v56 layer is the same container with inline strings
   (`u16 len + text`): `eCEntityDynamicContext` with `ContextBox bCBox`, `ID bCGuid`,
   `RootEntity` ..., then `0x40` entities. Faring VFX (551 B) and the StartUp SysDyn (6 KB)
   are near empty; the Morasul temple lights (43 KB) are lights only.

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
