# XCMSH validated indexed-mesh subset

2026-10-20: independently validated bytes from six unique installed GENOMFLE meshes and, in a second pass, five raw class82 visible meshes including bench, torch and houses. The Rust reader is `crates/formats/src/xcmsh.rs`. It preserves source XYZ, normals, UVs and index order. Engine.dll native analysis previously timed out; this schema is supported by local byte checks and independent tool-author documentation, not successful Engine.dll decompilation.

## Evidence and scope

Ignored local files: `inspect_xcmsh.py` (exploratory field walker), `mesh-samples.json` (archive/member/offset/hash provenance), `mesh-*.observations.json` (every field/range, decoded positions and indices), and `sample.xcmsh`, all beneath `.local/research/`.

The initial six unique accepted payloads ended exactly at their string-table boundary, with no unexplained bytes. Every decoded position was finite, every index in bounds, every triangle nondegenerate, and each computed vertex AABB exactly matched its serialized submesh AABB. That initial pass did not include multiple submeshes; the second raw pass validated a bench with three independent submeshes. Visual rendering was not part of these byte-level checks.

| Local sample | Member short name | Vertices | Indices / triangles | Stream tags |
| --- | --- | ---: | ---: | --- |
| mesh-0 / sample | G3_Myrtana_Ruins_Wall_07_COL | 56 | 138 / 46 | 5,3,1,0 |
| mesh-1 | G3_Myrtana_Ruins_Wall_06_COL | 70 | 192 / 64 | 5,3,1,0 |
| mesh-2 | G3_Myrtana_Ruins_Wall_03_COL | 110 | 312 / 104 | 5,3,1,0 |
| mesh-3 | G3_Animation_Head_Beard_Myrtana_03_03 | 90 | 288 / 96 | 12,5,4,3,1,0,64 |
| mesh-6 | G3_World_Rivermesh_01_Depth_117 | 3 | 3 / 1 | 5,3,1,0 |
| mesh-7 | G3_World_Rivermesh_01_Depth_210 | 3 | 3 / 1 | 5,3,1,0 |

First four came from `_compiledMesh.p00`; last two from `_compiledMesh.pak`. mesh-9/10 are duplicate sample copies and do not add independent evidence.

## Validated schema

Numbers are little endian. Envelope: magic GENOMFLE[8], u16 version1, u32 absolute string-table offset. Body begins byte14. Trailer: u32 0xDEADBEEF, u8 version1, u32 count, repeated u16 byte length + raw string bytes. Body strings are u16 indices into that table. Resolve every index with bounds checks. The shared `GenomeFile` parser validates complete table termination.

Body sequence for the strictly supported class83 / mesh35 / resource30 subset:

1. u16=1, u8=1, u16=1, u8=1.
2. u16 class-name index resolving to eCResourceMeshComplex_PS, u8=1, u16=0, u16 classversion83.
3. Opaque6 bytes; u16 property-header version30; u32 propertycount.
4. Each property: u16 name index, u16 type index, u16 propertyversion30, u32 payloadbytes, bounded opaque payload. Observed BoundingBox:bCBox has24 bytes and ResourcePriority:float has4.
5. u16 meshversion35, u16 resourcePSversion30, opaque u32, finite opaque f32, u32 submeshcount.
6. Each submesh follows the sequence below. The final cursor must equal the trailer offset.

Submesh version5 sequence:

1. u16 version5, opaque u32, six finite f32 AABB (minXYZ,maxXYZ), opaque u32, u16 material string index, u32 streamcount.
2. Each stream: u32 tag, u16 version1, u8 flag1, u32 elementcount, tightly packed array. Tags may occur in any order.
3. Auxiliary A: u8 marker1, u32 count, count*4 opaque bytes.
4. Auxiliary B: same.
5. Nested auxiliaries: u8 marker1, u32 count; each child has A-layout array, B-layout array and opaque84 bytes.
6. Auxiliary C: u32 count, count*24 opaque bytes, **no marker**.
7. Auxiliary D: u8 marker1, u32 count, count*4 opaque bytes.

| Tag | Stride | Interpretation |
| ---: | ---: | --- |
| 0 | 4 | u32 indices; count divisible by3 |
| 1 | 12 | three f32 positions |
| 3 | 12 | three f32 normals |
| 4,5 | 4 | opaque color-like packed bytes |
| 12 | 8 | two f32 UV coordinates |
| 64 | 12 | finite three-float tangent candidate, currently skipped |
| 73 | 8 | auxiliary two-float coordinate stream, validated and skipped; observed in raw houses |

Unknown tags/versions fail explicitly. Duplicate stream tags, non-finite decoded floats, missing positions/indices, count mismatches and out-of-range indices fail. Stream sizes are checked before allocation; a64MiB aggregate stream ceiling and per-count caps bound output. Auxiliary semantics remain unknown; their known lengths establish parsing alignment only. Missing normals/UVs produce empty vectors. Packed color mapping and tangent handedness are unresolved.

## Exact collision sample

Source `_compiledMesh.p00`, member `G3_Master_Collisions_01/G3_Myrtana_Ruins_Wall_07_COL.xcmsh`, payload offset1504643,2855 decoded bytes, SHA-256 ba1c9b07d723e5630b10049de629b51ce57d4b1d79aea0a47c898e4655c1b16c.

| Absolute offset / half-open range | Observation |
| --- | --- |
| 10 | trailer offset2731 |
| 25 | classversion83 |
| 35 | propertycount2 |
| [49,73) | BoundingBox six f32 |
| [83,87) | ResourcePriority f32 zero |
| 87,89 | mesh35,resource30 |
| 91,95 | opaque u32=2324,f32=0 |
| 99 | submeshcount1 |
| 103,105 | submesh5,opaque u32=146 |
| [109,133) | submesh bounds |
| 133,137,139 | opaque2324,materialindex5,streamcount4 |
| [143,154) / [154,378) | tag5 descriptor /56 color-like elements |
| [378,389) / [389,1061) | tag3 descriptor /56 normals |
| [1061,1072) / [1072,1744) | tag1 descriptor /56 positions |
| [1744,1755) / [1755,2307) | tag0 descriptor /138 indices |
| 2307,2312 | A/B marker1,count0 each |
| 2317 | nested marker1,count0 |
| 2322 / [2326,2542) | C count9 /216 bytes |
| 2542,2543 / [2547,2731) | D marker1,count46 /184 bytes |
| [2731,2855) | complete string table |

Material string5 is G3_Architecture_Bridge_Large_01_COL.xshmat. This differently named material is an actual stored reference. Bounds min=(-29.4072265625,-36.12689971923828,-319.676513671875), max=(30.8583984375,343.0047912597656,325.60546875).

First position=(30.8583984375,343.0047912597656,-254.850341796875). First twelve stored indices=0,1,2,2,3,0,4,0,3,3,5,4.

### Winding

For every triangle across the six unique samples, cross(v1-v0,v2-v0) dot sum(vertexnormals) is negative, with no positive/zero cases. Unchanged XYZ therefore has stored index order opposing stored normals. A renderer requiring matching geometric/stored normal direction can swap each triangle's second and third indices. Coordinate reflection can reverse orientation too; choose coordinate conversion and winding together. The parser preserves source order deliberately.

## Supported raw class82 variant: second pass

Raw class82 resources start with the body directly at byte0, with no GENOMFLE header or trailer. Every body string is u16 byte-length followed by exactly that many bytes, with no NUL. In this branch classversion82 replaces83; the mesh35/resource30/submesh5 sequence above remains unchanged. The same bounded field walker resolved these raw bytes to exact EOF and verified every submesh's index bounds, finite positions, nondegenerate triangles and exact computed-vs-stored AABB. The Rust reader now supports this branch with a16384-byte inline string ceiling. It validates/skips tag73 as two finite f32 per vertex without treating it as base UVs.

| Sample / SHA-256 | Source member | Size | Submesh vertices / indices |
| --- | --- | ---: | --- |
| raw-0 / 6f26fc8611a1cfb36c092ecd9fbc244388132b1e4607224568b130f7dc39a341 | G3_Objects_Myrtana_Interacts_01/G3_Object_Interact_Bench_03.xcmsh |16420|6/12,89/228,145/234|
| raw-4 / 8701805e1ca283d529b7c4add18b9be8b6a6b9d1220c2d8a8a9d0c7961e0ad14 | G3_Items_Misc_01/G3_Item_Misc_Torch_01.xcmsh |25816|381/828|
| raw-6 / 0dd9faf9fc6520138183aa660e355334b36fb93fd874cbdc7308aa2bf1ac386d | G3_World_Lowpoly_Static_Objects_Myrtana_01/G3_Myrtana_Faring_House_Hut_02_LOWPOLY.xcmsh |4549|64/96|
| raw-7 / 7e6a238e584755f36111ff532e6468b21150d3f0dde6643216fe7607bbcecbc6 | G3_World_Lowpoly_Static_Objects_Varant_01/G3_Varant_City_House_03_LOWPOLY.xcmsh |7902|104/282|
| raw-8 / 440c64aba4430965d4e0541bdeede551a17e8e8c1ea6d0ce5c90d1722d16231a | G3_World_Lowpoly_Static_Objects_Varant_01/G3_Varant_City_House_04_LOWPOLY.xcmsh |7902|104/282|

All five are zlib-compressed members of the base `_compiledMesh.pak`; decompressed hashes above identify the exact examined bytes. Full archive offsets, stored lengths and source names are in `.local/research/raw-mesh-samples.json`; each raw payload and complete field trace are in raw-N.xcmsh / raw-N.observations.json. The corrected sampling helper is `sample_raw_meshes.py`.

For these samples classversion82 is at byte35, meshversion35 at134 and submeshcount at146. Submesh strings/stream offsets vary after this point. Bench03 material references are G3_Architecture_Wood_08_A.xshmat, G3_Nature_Tree_Bark_01_A.xshmat and G3_Nature_Tree_Rings_01_B.xshmat. Its positions occupy[445,517),[3437,4505),[11334,13074), with local u32 index arrays[528,576),[4516,5428),[13085,14021). Keep each submesh's indices local; the public API returns separate submeshes, so it does not rebase them.

The torch's material is G3_Items_Misc_01_A.xshmat; positions[10938,15510), indices[15521,18833). Faring house positions[1180,1948), indices[785,1169), demonstrating that indices can precede positions. Its tag73 payload[3005,3517) has64 elements of8 bytes. Its material is G3_World_Lowpoly__Static_Objects_Myrtana_01_Diffuse_03.xshmat.

The earlier mesh-4/5/8 tent/water candidates also parse as raw82 with the exploratory walker; they are additional leads but the five explicitly tabulated visible resources are the second-pass validation set. Four other selected base resources (BrokenBench01/02, ObjectTorch01/02) instead have classversion1 with additional historical fields. They remain explicitly unsupported; no branch was guessed. Older/newer versions, the full stream set, auxiliary acceleration semantics, material graphs, animation, world axes/units and visual fidelity remain unresolved.

## Primary research sources

The independent tool author's [mesh reader](https://github.com/Baltram/rmtools/blob/5525421bf4b22636259bdc0d250ee96a5abcae66/mimicry/source/Mimicry/mi_xcmshreader.cpp) and [Genome wrapper](https://github.com/Baltram/rmtools/blob/5525421bf4b22636259bdc0d250ee96a5abcae66/mimicry/source/Mimicry/mi_genomefile.cpp) supplied schema leads, then local byte checks corroborated the accepted subset. Those GPLv3 source downloads remain ignored locally; no third-party implementation code was copied into Rust. Refer also to [community object-persistence documentation](https://auronen.cokoliv.eu/gmc/genome/general_info/object_persistence/).

Synthetic Rust fixtures cover source-value preservation, truncation, count limits, invalid indices, non-finite floats, unsupported tags and versions. The task owner runs consolidated Cargo validation; no Cargo command was launched by this research worker.
