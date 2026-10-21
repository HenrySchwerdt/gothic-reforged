# Recipe: Ardea ground textures and grass from Poly Haven (CC0)

Run from the repository root (Git Bash), after building the tool (see `../../README.md`):

```bash
bash tools/gothic-assets/recipes/polyhaven_ardea/download.sh      # CC0 inputs -> .local/polyhaven
.local/target-meshy/tool/release/gothic-assets.exe png g3_nature_plant_herbs_composit_01_diffuse_s1 .local/polyhaven/atlas/s1.png
bash tools/gothic-assets/recipes/polyhaven_ardea/sprites.sh       # foliage sprite library
python tools/gothic-assets/recipes/polyhaven_ardea/atlas.py       # new grass tiles in the foliage atlas
bash tools/gothic-assets/recipes/polyhaven_ardea/build.sh         # profile .local/remaster/polyhaven-ardea
GR_REMASTER=.local/remaster/polyhaven-ardea .local/target-meshy/release/gothic-reforged.exe --walk
```

| Original texture | Poly Haven | Notes |
|---|---|---|
| G3_Nature_Ground_Grass_01 | sparse_grass | flatten 0.8, histogram 0.85, 3.2 m |
| G3_Nature_Earthground_01 | brown_mud_02 | 1.8 m |
| G3_Architecture_Ground_City_03 | cobblestone_floor_13 | 2.2 m |
| G3_Nature_Ground_Path_02 | rocky_trail_02 | 2.8 m |
| G3_Nature_Wall_Stone_02 | mossy_rock | 3.6 m |
| G3_Nature_Dunes_PlainSand_02 | coast_sand_04 | 4 m |
| G3_Nature_Ground_Forest_01 | leaves_forest_ground | 1.8 m |
| foliage atlas tiles of Grass_05/06/07, Forest_Underbrush_02 | grass_medium_02, grass_bermuda_01, weed_plant_02 sprites | herb, flower and fern tiles stay original |

Gothic terrain UVs are about 1 unit per metre and the layers sample them at x0.1428 (a 7 m
tile); `scale=` in the manifest corrects the real-world size of each replacement. Histogram
matching maps each replacement onto the original's palette and contrast so it sits in the
Gothic look; `--flatten` removes large brightness clouds that shimmer in the distance.

The atlas output is derived from the original atlas (untouched tiles are copied), so the
profile stays local. A shippable open-source pack would need its own atlas layout made only of
CC0 sprites (see the remaster notes).
