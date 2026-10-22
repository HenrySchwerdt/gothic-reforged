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

VFS match logs, leaf records, the fresh base DDS and numerical summaries live under
ignored `.local/qa/tree-diagnostic/`. The previously exported atlas thumbnail remains
read-only at `C:/Users/PowerTower/dev/gothic-reforged/.local/scratch/`.
No original game assets or generated images are committed.
