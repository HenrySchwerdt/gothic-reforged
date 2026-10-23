# SPT composite atlas V-origin diagnostic

The apple tree's sparse near crown persisted with `GR_NOWIND`, so leaf wind is not
required to reproduce it. The resolved VFS payloads were byte-matched to the retained
`.local` references: the renderer selects `speedtrees.pak`'s
`Myrtana/G3_Tree_S_GSApple_01.spt`, and the composite DDS rewrapped from
`_compiledimage.pak`'s `speedtree/G3_Speedtree_Misc_Composite_01_Diffuse_01.ximg`.

The parsed apple leaf is `BulfordHollyLeaves_RT_1.tga`, with raw rectangle
`[0.25,0.25, 0,0.25, 0,0, 0.25,0]`; the billboard rectangle is
`[0.25,1, 0,1, 0,0.75, 0.25,0.75]`. The retained 512 px PNG thumbnail shows a sparse,
bare-tree sprite in the upper-left cell and a leaf-cluster sprite in the lower-left cell.
The original DDS is 2048 px; measured fraction of decoded pixels with alpha >= 110 was 4.89% in
the upper-left cell and 36.21% in the lower-left. The thumbnail was checked against the
DDS orientation; it is not vertically inverted. These observations make a V-origin or
per-record atlas-coordinate mismatch plausible, but do not establish the original
SpeedTree SDK convention or which sprite the game intended for every species.
Coverage measures decoded mip0 quarters without GPU filtering, not screen coverage.
The PNG has flattened alpha over blue; comparison therefore composites the DDS thumbnail
over `(80,120,200)`. Same-orientation mean RGB error is under0.71/channel versus
8.68/16.10/34.26 after vertical inversion. Raw RGBA equality is inapplicable here.

`GR_SPT_FLIP_ATLAS_V=1` is an opt-in causal diagnostic. It remaps all V coordinates in
leaf, frond, and billboard rectangles immediately after SPT parsing and before silhouette
measurement or mesh generation. The default remains off. Comparing flag-off and flag-on
captures can test whether the atlas cell selection explains the sparse appearance; even a
visual improvement would not prove original-game parity. Do not make this the default
without corroborating multiple species and their resolved source records.
The experiment also changes the silhouette-derived geometry calibration. A different
canopy in the capture therefore tests the combined atlas interpretation, rather than
isolating texture sampling alone. Wind should be disabled for this comparison.

## Reproduction tools

The existing `spt_dump` example accepts an extracted SPT path and optional variant.
Set `GR_SPT_DUMP_LEAVES=1` to also print leaf names, density, size and raw rectangles
on stderr; stdout remains the existing generated mesh JSON. Default generation is
uncalibrated, so its400 apple quads and101.83–103.40cm diagonals are not measurements
of the actual runtime variant. Leaf density semantics and exact SDK parity remain inferred.

`vfs_match DATA_DIR MEMBER_BASENAME REFERENCE_FILE` resolves through the same VFS
overlay rules as the renderer and returns an error on mismatch. A `.dds` reference
selects XIMG-to-DDS rewrapping before byte comparison. Actual apple and Composite01
references matched; an intentional apple-vs-aspen SPT comparison failed as expected.
Current VFS archives selected `speedtrees.pak` and `_compiledimage.pak` respectively.
These examples use the existing parsers and add no dependencies or new asset formats.
Numerical atlas reproduction: `C:/Python312/python.exe .local/qa/tree-diagnostic/atlas_metrics.py`.
It reads the fresh, VFS-matched DDS and the retained PNG, writing only JSON metrics.

## Actual flag-off/on renderer comparison

Optimized diagnostic source `cd4288a7b3a0a0ed9f174275e7ed3461ec00d4b7` built in8m12s.
Immutable `.local/artifacts/gothic-reforged-atlas-origin.exe` SHA256
`f60cea222be15bd6a7bd7b4585be0ff23eb789a4a9a9804ba41c1458323f3d39`.
Both finite smoke runs used GR_NOWIND=1, GR_RES=1280x720, and the same GrassFix runtime.
The V flag was unset for off and exactly1 for on, restored afterward. No concurrent
compiler or game ran. Each exited0, grounded360/360, clean fatal renderer/shader scan;
runtime saves stayed empty. No performance or combat-parity measurement was attempted.

```text
EXE --walk --smoke --screenshot PREFIX --shots "apple:38.83,55.5,28.5,0,0.12;aspen:46.31,54.5,12.89,0,0.05;oak:250,54,22.33,0,0.12"
```

Evidence `.local/qa/tree-diagnostic/atlas-{off,on}-{apple,aspen,oak,walk,overview}.png`
and matching smoke logs. At the apple viewpoint the central sparse branch-like crown
becomes dense green clusters with the flag. At the oak viewpoint the scene becomes
visibly thinner with more branches showing. The view includes multiple trees, so it
does not isolate each species or establish an individual oak's mapping. Root inspected
on-apple/on-oak/off-oak, and the QA agent inspected both apple/oak pairs.

This confirms the atlas interpretation strongly affects rendered foliage; it does not
establish that globally flipping V reproduces the original game. Denser foliage alone
is not a fidelity criterion. The default remains off and no diagnostic package replaces
the delivered LeafPhase or GrassFix releases. Further work needs original intended sprite
identities, multiple species including fronds, and separate calibration/mapping evidence.

VFS match logs, leaf records, the fresh base DDS and numerical summaries live under
ignored `.local/qa/tree-diagnostic/`. The previously exported atlas thumbnail remains
read-only at `C:/Users/PowerTower/dev/gothic-reforged/.local/scratch/`.
No original game assets or generated images are committed.

## Controlled leaf-only UV diagnostic, 2026-10-23

Source `53f16f5fcece7ddf9a2c9323e5068ddb0f0c2bfd` adds default-OFF
`GR_SPT_FLIP_LEAF_RENDER_V=1`. Unlike the older atlas-wide switch above, it flips
only generated leaf-card V after the unflipped calibration probe. Frond UVs, geometry,
normals, indices, bark and crown metrics remain unchanged in the synthetic mixed
frond/leaf test; the formats suite passed 101 tests with 1 ignored. Runtime-derived
colors and billboard output were not directly test-asserted; their code paths are unchanged.

One GNU viewer release built in 9m33s, log `.local/pc-leaf-uv-only-probe-build.log`.
The immutable private EXE SHA-256 is
`d4b0c1d417690a759c5cd672d984fd8ca0ff51275c935d2d6457f8e672f757a1`.
It does not replace the validated Desktop RenderPerf package.
The initial GR_NOWIND plus --wind-shot-times startup was rejected before images;
failed logs remain under `.local/leaf-uv-only-probe/comparison/default/`.

The successful pair is under `.local/leaf-uv-only-probe/comparison-fixed-wind/`.
Both runs used the three poses above, 1280x720, and active wind held at synthetic
times 3600.000000000, 3600.016666667 and 3600.033333333 seconds. The baseline child
had only `GR_RES=1280x720`; the candidate added only `GR_SPT_FLIP_LEAF_RENDER_V=1`.
All other GR overrides, including NOWIND and the older atlas-wide switch, were absent.
Both exited0, grounded360/360 and had no fatal renderer markers. CPU selected and
captured current/previous wind records matched within and between variants; these
are not GPU readbacks, a jitter test or a performance measurement.

Root visually reviewed all six `default/shot-{apple,aspen,oak}.png` and
`leaf-v-only/shot-{apple,aspen,oak}.png` images. The apple view becomes much denser
and greener; the aspen view looks brighter/denser. The oak-labeled view changes
texture appearance with some thinner-looking crowns. This isolates an effect of
leaf UV sampling without the older calibration confound, but density is not proof
of original-game fidelity. Both switches remain default-OFF.

Historical loader records near the first two poses name `S_GSApple_01` and
`S_EuropeanAspen_01`, respectively. Apple/Composite01 payloads were previously
VFS-matched; aspen's parsed record names Composite02 without an equivalent retained
VFS-match log. The nearest oak-named record for the third pose is `L_PinOak_01`,
30 m away in X/Z, with nearer non-oak records and no exact retained atlas mapping.
These are nearby candidates, not identification of the mesh dominating each view.
Exact metadata, environments and records are in `summary.json` and `observations.md`.

The bounded native investigation in `SPT_NATIVE_UV_EVIDENCE.md` found indexed
per-corner dataflow but did not establish TEXCOORD semantics or the SDK's V convention.
One final literal pointer proved to be an ASCII diagnostic string. No additional
native scan, unchanged full-binary import, build, or capture is pending. Further
work needs independent intended-sprite and corner-association evidence.
