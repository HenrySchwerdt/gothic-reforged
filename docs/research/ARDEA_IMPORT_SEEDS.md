# Ardea world import seeds

Read-only observations from the installed Gothic 3 1.75 on 2026-10-20. These
are source-data relationships and placement evidence, not verified gameplay.
No original files were changed; bounded inspection scripts and derived local
data are ignored under `.local/research/`.

## Reproduce and inspect

- `.local/research/ardea_probe.py`: bounded G3V0 inventory and payload reader.
- `.local/research/ardea-inventory.json`: all Ardea-named archive members.
- `.local/research/ardea_entity_probe.py`: exploratory structural parser.
- `.local/research/ardea-entities.json`: nine Ardea documents, entity IDs,
  property names/types/offsets, full world/local matrices, and graph edges.
- `.local/research/ardea-grid-entities.json`: six nearby static spatial cells.

All listed world members are in
`D:\SteamLibrary\steamapps\common\Gothic 3\Data\Projects_compiled.pak`.
The bounded search of `Projects_compiled.p00` found no Ardea-named members;
this does not establish global patch precedence or absence of indirect changes.

## Authored sectors and document counts

Prefix `G3_World_01/Myrtana/Ardea_City/`; each named group has a same-named
directory. Append the document name below within that directory.

| Group | Document suffix | Entities including root |
| --- | --- | ---: |
| `G3_Myrtana_01_Ardea_Dynamic_Objects_01` | `_SHyb.node` | 188 |
| `G3_Myrtana_01_Ardea_NavZones_01` | `_SHyb.node` | 36 |
| `G3_Myrtana_01_Ardea_NPC_01` | `.lrentdat` | 78 |
| `G3_Myrtana_01_Ardea_NPC_01` | `_SHyb.node` | 2 |
| `G3_Myrtana_01_Ardea_NPC_02` | `.lrentdat` | 133 |

Prefix `G3_World_01/Myrtana/Ardea_Outdoor/`, same directory convention:

| Group | Document suffix | Entities including root |
| --- | --- | ---: |
| `G3_Myrtana_01_Outdoor_Ardea_Dynamic_Objects_01` | `_SHyb.node` | 92 |
| `G3_Myrtana_01_Outdoor_Ardea_Fishercamp_02` | `_SHyb.node` | 10 |
| `G3_Myrtana_01_Outdoor_Ardea_NavZones_01` | `_SHyb.node` | 31 |
| `G3_Myrtana_01_Outdoor_Ardea_Vegetation_01` | `_SHyb.node` | 1 |

Total: 571 entities and 562 graph links, all graph indices in range. Each
document ends its graph with the two little-endian `-1` indices. A ten-byte
GUID-named `.lrentdat` beside NPC01 is a separate stub, not a GENOMFLE document.
Do not interpret its name or suffix as proof of a valid entity container.

The related `.sec`, `.secdat`, `.lrgeo`, and `.lrgeodat` files exist too. Sample
`.secdat` string tables name the corresponding group and/or `_SHyb` document.
Their full loading/activation semantics are still unimplemented. The one-root
vegetation document does not prove Ardea has no vegetation: other storage
paths and vegetation class payloads need separate decoding.

## Static scenery is in spatial cells

Searching only for `Ardea` misses most static scenery. Six sampled neighboring
documents live under:

`G3_World_01/SysDyn_{9A103CC2-4190-4DB3-9618-0419E5445AAD}/`

| Member basename | Entities | ResourceFileName properties |
| --- | ---: | ---: |
| `G3_World_01_x85000y0z-15000_CStat.node` | 263 | 241 |
| `G3_World_01_x85000y0z-25000_CStat.node` | 112 | 98 |
| `G3_World_01_x85000y0z-5000_CStat.node` | 168 | 160 |
| `G3_World_01_x95000y0z-15000_CStat.node` | 731 | 698 |
| `G3_World_01_x95000y0z-25000_CStat.node` | 138 | 127 |
| `G3_World_01_x95000y0z-5000_CStat.node` | 312 | 293 |

These contain 1,724 entities and 1,617 mesh references, including barrels,
shelves, shipwrecks and houses. Many static entities have empty names; use
their serialized 20-byte property IDs as source identities. The coordinate
values in filenames suggest a 10,000-unit grid, but the exact inclusion rule
and world-streaming semantics need verification against bounds and runtime.
Some scenery references `.xlmsh` LoD resources rather than `.xcmsh` geometry;
importing only XCMSH will leave those relationships unresolved.

## Minimal reproducible placement

Smallest useful document:

`G3_World_01/Myrtana/Ardea_City/G3_Myrtana_01_Ardea_NPC_01/G3_Myrtana_01_Ardea_NPC_01_SHyb.node`

Its decoded payload SHA-256 is
`08297d24b447934ea9122a622317bf09846dad382b0360462b6d0c04d5669303`.
Entity index 0 is an identity root; index 1 is `G3_Object_Torch_01`.

- Entity starts at payload byte 407; common entity header at 496.
- World matrix starts at 539, local matrix at 603; both match here.
- Serialized translation vector: `(92424.828125, 5220.29443359375, -10935.70703125)`.
- Raw 20-byte ID: `6c9b21bf0443c24894ea462b5ebcf89700000000`.
- `eCVisualMeshStatic_PS` has `ResourceFileName` and `ResourceFilePath`, both
  resolving string-table index 5 to `G3_Object_Torch_01.xcmsh`.
- The name property value is at byte 843, length 2; material switch at 917,
  four bytes, value 0.
- Mesh resolves by basename to `_compiledMesh.pak` member
  `G3_Objects_Myrtana_Misc_01/G3_Object_Torch_01.xcmsh`; archive offset
  261469822, 27734 stored bytes, 59685 decoded bytes, compression 2.

Another clean seed is outdoor Fishercamp entity 1:
`G3_Object_Interact_Bench_04`, translation
`(83958.75, 2149.091552734375, -19863.75390625)`.
Its resource is `_compiledMesh.pak` member
`G3_Objects_Myrtana_Interacts_01/G3_Object_Interact_Bench_04.xcmsh`, offset
238263853, 6704 stored bytes, 18875 decoded bytes. Nine bench/prop entities
share the ten-entity Fishercamp document; preserve their separate transforms.

Keep source matrices intact until the coordinate basis and units are proven.
A viewer can subtract a chosen source origin for numerical stability, provided
the original matrix remains available. Metres, axis flips, and quaternion
conventions have not been established by these static observations.

## Observed serialization subset

The GENOMFLE envelope is already implemented. Its string table contains raw
length-prefixed bytes; property-name and resource references are `u16` table
indices. The sampled textual names are ASCII-compatible; arbitrary source
string encoding remains unspecified.

- Node body: `u16(83)`, `u32(entity_count)`, entity records, then index pairs.
  Each record begins with two booleans, spatial version `u16(0x23)`, optional
  20-byte creator ID, 24-byte AABB and 60-byte oriented box.
- Lrentdat body: `GENOMEDL`, `u16(83)`, dynamic context property object, entity
  count, dynamic records, then graph pairs. Dynamic records begin with
  `u16(0x40)`, `u16(0x53)`, boolean and optional 20-byte creator ID.
- Common archive entity: `u16(0x53)`, `u16(1)`, 20-byte ID, seven boolean
  flags, alpha float, insertion u16, priority byte, three flags, name index,
  world matrix and local matrix, three AABBs, two spheres, culling/scaling
  metadata, and length-delimited property sets.
- The common header through property-set count is 298 bytes. World matrix
  starts at common-header offset 43, local matrix at 107. Each matrix stores
  four vectors of four little-endian f32 values: X basis, Y basis, Z basis,
  translation. Translation occupies float indices 12..14, with homogeneous 1
  in slot 15. This layout is supported by source and sampled actual bytes.
- Property sets have an accessor prefix, class-name table index, version/size,
  properties and opaque class-specific tail. Each property contains name/type
  table indices, magic u16, size u32 and payload. Mesh-string payloads sampled
  here are a u16 string-table index. Unsupported payloads can be skipped by
  their checked lengths without inventing gameplay behavior.
- **Trap:** the outer Lrentdat `eCEntityDynamicContext` size includes later
  entity data. Generic skip-to-object-end loses all entities. Its observed
  context properties are followed by class version plus 33 known metadata
  bytes before entity count. Treat this container explicitly.

## Primary implementation references

The community editor [g3dit](https://github.com/georgeto/g3dit) was inspected at
commit `7bf3ab9d6cb5fd546591201ff5546990c1b4b627`. It is GPL-3.0; downloaded
reference files remain ignored locally. No Java implementation was copied into
the Rust source. Relevant format evidence:

- [ArchiveEntity](https://github.com/georgeto/g3dit/blob/7bf3ab9d6cb5fd546591201ff5546990c1b4b627/LrentNode/src/main/java/de/george/lrentnode/archive/ArchiveEntity.java): common fields and matrix order.
- [NodeFile](https://github.com/georgeto/g3dit/blob/7bf3ab9d6cb5fd546591201ff5546990c1b4b627/LrentNode/src/main/java/de/george/lrentnode/archive/node/NodeFile.java) and [NodeEntity](https://github.com/georgeto/g3dit/blob/7bf3ab9d6cb5fd546591201ff5546990c1b4b627/LrentNode/src/main/java/de/george/lrentnode/archive/node/NodeEntity.java): spatial record framing.
- [LrentdatFile](https://github.com/georgeto/g3dit/blob/7bf3ab9d6cb5fd546591201ff5546990c1b4b627/LrentNode/src/main/java/de/george/lrentnode/archive/lrentdat/LrentdatFile.java) and [eCEntityDynamicContext](https://github.com/georgeto/g3dit/blob/7bf3ab9d6cb5fd546591201ff5546990c1b4b627/LrentNode/src/main/java/de/george/lrentnode/classes/eCEntityDynamicContext.java): context container exception.
- [bCMatrix](https://github.com/georgeto/g3dit/blob/7bf3ab9d6cb5fd546591201ff5546990c1b4b627/G3Utils/src/main/java/de/george/g3utils/structure/bCMatrix.java): serialized basis vectors and translation.
- [ClassUtil](https://github.com/georgeto/g3dit/blob/7bf3ab9d6cb5fd546591201ff5546990c1b4b627/LrentNode/src/main/java/de/george/lrentnode/util/ClassUtil.java) and [ClassProperty](https://github.com/georgeto/g3dit/blob/7bf3ab9d6cb5fd546591201ff5546990c1b4b627/LrentNode/src/main/java/de/george/lrentnode/properties/ClassProperty.java): property framing.

## Next validation

Implement a checked Rust subset with synthetic fixtures, compare all nine
authored documents and six static cells, then load a mesh at an authored matrix.
Retain provenance, support parent graph relationships, and resolve resources
through the verified archive layer. Rendering a torch or bench establishes a
placement smoke test, not scene completeness, navigation, AI, animation,
collision, template patching, or quest behavior.
