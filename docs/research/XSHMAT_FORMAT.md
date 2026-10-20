# XSHMAT shader-material subset

2026-10-20. Reader: `crates/formats/src/xshmat.rs`. Independent implementation from local byte observations of the installed Gothic 3 1.75 `_compiledMaterial.pak` (1150 `.xshmat` members). Public schema leads (Baltram/rmtools mimicry reader, georgeto/g3dit) were used only to know what to look for; no code was copied (both GPL). Engine.dll was not decompiled for this; behavior claims below are inferred from data, not from the engine.

## Two storage variants

Both carry class `eCResourceShaderMaterial_PS`. In the installed archive 1150 of 1150 parse; roughly the old/raw variant is the majority (the sample set of 400 had most members raw).

1. **GENOMFLE-wrapped** (`GENOMFLE`, version 1, trailing string table, see `genome.rs`). Names, types and string values in the body are u16 indices into the table.
2. **Raw**: no envelope. The body starts directly with `01 00 01 01 00 01`, then `u16 length + "eCResourceShaderMaterial_PS"`. Every string is inline `u16 length + bytes` (no NUL).

## Property record (both variants)

`name, type, u16 version = 30, u32 payload_size, payload[payload_size]`, where `name`/`type` are string references (index or inline as above). Objects are nested (`eCColorSrcSampler`, `eCColorSrcConstant`, `eCColorSrcCombiner`, `eCShaderDefault`) with their own small headers, but the reader does not decode the tree: it scans the body for records with *known name and exact expected type string and version 30 and an in-bounds size*. That combination does not occur by accident in unrelated data.

| Property | Type string | Payload |
| --- | --- | --- |
| `ImageFilePath` | `bCImageResourceString` | one string reference (index, or inline u16 len + bytes); the authored texture file name, usually `*.tga`, sometimes `Data/Materials/Images/...` or `*.dds` |
| `BlendMode` | `bTPropertyContainer<enum eEShaderMaterialBlendMode>` | 6 bytes: u16 (always 1 observed) + u32 enum value |
| `MaskReference` | `char` | 1 byte |
| `EnableSpecular`, `DisableLighting` | `bool` | 1 byte, 0/1 |

Not decoded: constant `Color` (observed 16 bytes: a u32 followed by three f32), `Alpha`, `TexRepeatU/V` (enum), `AnimationSpeed`, `CombinerType`, `FallbackMaterial`, `TransformationType`, `MaxShaderVersion`, and which combiner input a sampler feeds.

## Texture roles

Samplers are returned in file order. Role is derived from the file name (`normal`, `specular`, `detail`, `cubemap` substrings); the first remaining sampler is Diffuse, later unclassified ones are Other. Evidence: `G3_Architecture_Wood_08_A` has Diffuse_01 then Normal_01; `G3_Architecture_Beliar_Statue_02_A` has Diffuse, Normal, Specular and `G3_Misc_Cubemap_01.dds`; `G3_Objects_Chain_01_A` references a Normal from another asset family (`G3_Architecture_Iron_01_Normal_01.tga`), so sharing across assets is normal. The true semantic slot (combiner wiring) is unverified; naming convention is used instead.

Survey over all 1150 materials: 0 parse failures; 99 have no Diffuse (water materials with only normals and a cubemap, textureless colour-only editor/legacy materials, one specular-only gold).

## BlendMode / MaskReference (observed, inferred meaning)

Counts: 0 -> 993, 1 -> 99, 2 -> 53, 7 -> 4, 8 -> 1. Value 1 always appears with a nonzero `MaskReference` (e.g. foliage `Plant_Herb_13_A` mask 176, hair, flags): alpha test with reference/255. Value 2 appears on beards, eyes, eyebrows, eyelashes: alpha blend. Value 7 on light streaks, waterfalls, crystals, magic barrier: additive-like. The symbolic enum names were not recovered; the viewer maps 0 Opaque, 1 Mask(ref/255), 2 and 8 Blend, 7 Add. Treat as a rendering hint.

## Texture name to image

Materials reference `.tga` sources; `_compiledImage.pak` stores `level/<stem>.ximg` (also other folders, plus `_Intern/`). The importer maps `stem(tga/dds name) + ".ximg"` to a unique case-insensitive basename in the image archive. Ambiguous or missing names are reported as issues, not guessed. Lightmaps (`Lightmaps.pak`), `Materials.pak` and the `.dds` sources were not used.

Ardea scene result: see HANDOFF notes in the final report; all submesh materials of the preview scene resolved.

## Rendering notes

- XIMG DXT1/3/5 blocks are uploaded untouched as BC1/BC2/BC3 sRGB with the authored mip chain (largest first), repeat addressing, anisotropic filtering. UV v is **not** flipped: the engine is a D3D title (top-left origin) which matches wgpu.
- Normal maps are not applied: the channel packing of the Gothic 3 normal textures (possible DXT5nm swizzle) is unverified. Specular maps are loaded but unused.
- Source winding opposes stored normals (see `XCMSH_LAYOUT.md`); the viewer swaps the second and third index per triangle and keeps double-sided rendering until handedness of the world transform is settled.

## Limits

- No XSHMAT writer, no tree decoder, no shader selection beyond the five properties above, no terrain/blended-layer materials, no animated textures (`AnimationSpeed`), no fallback-material chain.
- Only the single explicit `_compiledMaterial.pak` is searched; no patch/overlay precedence (`.p00`) was investigated for materials (none is shipped next to it in this installation).
- Parser bound: 4 MiB input, at most 64 samplers.
