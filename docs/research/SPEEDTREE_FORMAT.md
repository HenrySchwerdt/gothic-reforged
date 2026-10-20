# SpeedTree `.spt` (Gothic 3) - format and generator

Source: `Data\Speedtrees.pak`, 98 files (`Myrtana/`, `Nordmar/*_winter`, `Varant/`), 5-9 kB each,
signature `__IdvSpt_02_` (SpeedTree RT 4.x, same container as Oblivion/FO3/FNV). Textures are NOT in
that pak: bark/leaf/frond images are `_compiledImage.pak` `speedtree/*.ximg`
(`G3_SpT_Bark_*_Diffuse_01` + `_Normal_01`, and six composite maps `G3_Speedtree_Misc_Composite_01..06`,
2048x2048 DXT5 except 03 (512x1024) and 05 (1024x1024)).

## Verdict: parameters only, no meshes

The files are procedural definitions. Branch tubes, fronds and leaf cards are generated at load time
by the engine (SpeedTreeRT). Only the parameters are recoverable; the generator here
(`crates/formats/src/terrain/spt_gen.rs`) is a re-implementation of the same kind of algorithm, **not** a
bit-exact clone. Legend: **V** = verified (byte/number layout, matches data), **I** = inferred by
correlating all 98 files (plausible, not confirmed against the engine).

## Container (V)

```
u32 1000, u32 12, "__IdvSpt_02_"            20-byte header
then records:  u32 tag, payload (shape fixed per tag)
```
Public tag dictionary: <https://github.com/matiaszanolli/ByroRedux> `crates/spt` (Oblivion corpus).
Shapes: bare marker (0 bytes), `u8`, 4 bytes (`u32`/`f32`), 12 bytes (`vec3`), fixed blobs
(8003/8005/8009 = 52, 13008 = 11, 13013 = 4), `u32 len + ascii` strings, `u32 n + n*32` arrays
(10002/10003). `13005` is bare or followed by a length-prefixed curve (disambiguate: next u32 is a
length when 8..=512, tags are >= 1000). Gothic 3 uses the same walk; the stream continues past
the Oblivion "geometry tail" (that region is just more tags: 14000-20005).

## Records that matter

| tag | payload | meaning |
|---|---|---|
| 2000 | string | bark texture (`.tga`; the cooked file is `<stem>.ximg`) (V) |
| 2001 | f32 | tree height in cm (V: 75 .. 1100) |
| 2003 | f32 | size variance in cm (I) |
| 2005 | u32 | generator seed (I) |
| 1014 | u32 | number of branch levels (V: 3..5) |
| 1016 ... 1017 | markers | one **branch level** block, repeated `1014` times (V) |
| 6000-6007, 6017 | string | `BezierSpline min max var { n  (x y tx ty w) * n }` per level (V) |
| 6008 | u32 | length segments (I) |
| 6009 | u32 | cross-section sides, `1` = flat ribbon (I) |
| 6010, 6011 | f32 | child start / end fraction along the branch (I) |
| 6012 | f32 | number of child branches (total over the level) (I) |
| 6013, 6014 | f32 | children per node, min/max (whorls) (I) |
| 6016 | u8 | level is a frond (flat textured ribbons) (I) |
| 4000.. | block | one per **leaf type**: 4002 density (I), 4003 texture name (V), 4005 card size as fraction of tree height (I), 4004/4006/4007 colour/size misc (unknown) |
| 5000-5006 | vec3.. | wind: 5002 = (-a, +a, freq) in degrees (I), 5004 (I) |
| 8003/8005/8009 | 13 f32 | material colours (branch / leaf / frond?) (I) |
| 9000-9014 | | LOD parameters (unknown) |
| 10002 | n x 8 f32 | leaf-card rectangles in the composite, 4 corners `(u,v)` = TR, TL, BL, BR; `v` counted from the top (V: rectangles land on leaf clusters) |
| 10003 | n x 8 f32 | frond rectangle(s) in the composite (V) |
| 10004 | u32 n + n x 8 f32 | **whole-tree billboard rectangle** in the composite (V: contains the species silhouette) |
| 12000..12001 | | collision objects: 12002 sphere (x,y,z,r), 12003 cylinder (base xyz, r, h), 12004 box (centre xyz, extents xyz); units are **feet** (V: Douglas fir cylinder r = 0.524 ft = 16 cm = 0.015 x 1100 cm) |
| 16000-16014 | | per-level wind/LOD triples (unknown) |
| 20002 | string | composite map (`G3_Speedtree_Misc_Composite_NN_Diffuse_01.dds`) (V) |
| 20003-20005, 20001, 21xxx | | billboard/shadow extras (unknown) |

Level 0 is the trunk. Typical shape: trunk, branches, twig/frond level, optional leaf-carrier level.
`cross-section 1` levels with `6016 = 0` are zero-thickness twig carriers that only hold leaf cards.
`4000` (u8) is NOT "leaf enabled" (0 for the oak with leaves); leaves exist when a 10002 rectangle exists
(palms have none: frond cards only).

Spline value semantics (I): `min..max` range scaled by a profile curve along the parent (x = position on
the parent 0..1), plus random `var`. Hermite with tangent slope `ty/tx` matches the stored linear
`(0,1)->(1,0)` taper defaults and the hump-shaped length curve of the firs. 6004 = length (fraction
of tree height), 6005 = radius (fraction of tree height; Douglas fir 0.015 x 1100 = 16.5 cm
matches the collision cylinder), 6006 = radius profile, 6007 = angle to the parent axis (degrees,
90 = perpendicular; trunk -90), 6001/6002 = gravity / disturbance, 6000 = distribution.

## Generator (`spt_gen.rs`)

1. Trunk: `segments` Hermite-less polyline from `size`, upright with disturbance, radius from the
   profile spline with a base flare.
2. Each deeper level: `child_count` children spread evenly over the previous level's branches,
   in whorls (`per_node`), between `child_start` and `child_end`; direction = parent axis rotated by
   the level angle spline around a golden-angle azimuth; length = size x length spline x curve; gravity
   droop per segment.
3. Frond levels become flat ribbons textured with the frond rectangle; tube levels (<= level 2) get
   3-8 sided bark tubes; leaf cards (two crossed quads per cluster, the leaf rectangle, normals pointing
   outwards from the crown centre) sit on the end/middle of the deepest non-frond level.
4. **Calibration** (game side, `apps/game/src/world_loader/trees.rs`): the crown radius and crown
   base of the generated skeleton are matched to the silhouette of the shipped whole-tree billboard
   (`10004` rectangle, alpha bounding box), because the exact length semantics are not recovered.
5. LOD: near = tubes + cards (3 random variants per species), far = two crossed quads cut from
   the billboard rectangle; Bevy `VisibilityRange` dither cross-fade 70-90 m.

## Not implemented / open

* Exact SpeedTree growth equations (tags 6000-6017 semantics beyond the table above), 4004/4006/4007,
  8xxx colours, 9xxx LOD, 16xxx wind per level, the 20003-20005 rectangles, self-shadow composite map.
* Wind sway shader (needs a custom vertex stage; the shared `GothicMaterial` has none).
* Camera-facing leaf cards (cards are baked crossed quads instead).
* Trunk collision is exported as `Spt::trunk_capsule_cm()` (cylinder radius/height in cm); the loader
  currently feeds it as an 8-sided prism into the tile collision soup.
