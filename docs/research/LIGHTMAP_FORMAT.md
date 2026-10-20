# Lightmaps (`Lightmaps.pak`, `.xlmp`)

Reader: `crates/formats/src/xlmp.rs` (bounded, synthetic tests). 11232 of 11233 installed files parse (the exception, a Varant skeleton, has a different head and is rejected with "implausible texel scale"). Probe tools: `crates/formats/examples/{xlmp_dump,pak_dump,lm_probe}.rs`.

## Identity
Member name `<mesh stem>_{GUID}.xlmp`. The GUID is the first 16 bytes of the 20-byte world entity source ID in Microsoft mixed-endian order (verified: 10802 world records match). One file per placed instance; `_LOD1` variants exist per mesh LOD. The string table names `eCResourceLightmap_PS` and the render mesh (`.xcmsh`).

## Layout (GENOMFLE v1 body)
`... f32 texel_scale (0.04 = 25 cm/texel; 0.02 on some) , u32 block_count`, then per submesh in submesh order:
`01 03 00`, `01 u32 N` + N x 4 bytes `[b,g,r,a]`, `01 u32 N` + N x vec3 f32, `u16 tile_count`, tiles.
Tile: `44 33 22 11`, u16 1, u32 index, 8 bytes `ff`, u32 w, u32 h, u32 size (0 or w*h*4); if size > 0: BGRA texels, then `u32 4*h` and h x (u16 start, u16 end) valid span per row.

* Vertex `[b,g,r,a]`: `a` equals the mesh's own tag-5 vertex AO (no new information); `rgb` is baked colour of static lights (mostly 0; 3353 files have any).
* vec3: unit vector, dominant incident light direction ((1,0,0) when unlit).
* Tiles: dark colour images (RGB mostly 0, max about 165, alpha = coverage). They hold only local static lights (torches, fires): large faces are black. The sun and sky are NOT baked, so the lightmap cannot replace dynamic sun shadows. 2277 files carry tile data.
* mesh tag 73 (`uv2`) is chart-local position in centimetres (1/texel_scale units), -1,-1 = not in a chart; tile size is about ceil(extent*scale)+2 per axis. Several charts can be packed into one tile (offsets inside uv2).

## Unresolved
The vertex/triangle -> tile index is not stored; ordering heuristics (connected components in index order) match tile sizes for simple meshes (carpets) but fail for buildings. Tiles are therefore decoded but not applied; only per-vertex light is applied (`world_loader::instance_light`, shader `LIGHTMAP_GAIN`, `GR_NOLM=1` disables). Next step: Engine.dll serializer/consumer of `eCResourceLightmap_PS`.
