#!/bin/bash
# Renders the foliage sprite library (alpha cut-outs) from the CC0 plant models.
set -e
T=.local/target-meshy/tool/release/gothic-assets.exe
M=.local/polyhaven/models; O=.local/polyhaven/sprites/lib
mkdir -p $O
for v in a b c d e; do for az in 0 70 140; do
  $T sprite --gltf $M/grass_medium_02/grass_medium_02.gltf --nodes grass_medium_02_$v --albedo $M/grass_medium_02/textures/grass_medium_02_diff_2k.png --w 384 --h 384 --az $az --out $O/gm_${v}_$az.png
done; done
for v in seedling_a seedling_b seedling_c seedling_d medium_a medium_c medium_e medium_f; do for az in 0 90; do
  $T sprite --gltf $M/grass_bermuda_01/grass_bermuda_01.gltf --nodes grass_bermuda_01_$v --albedo $M/grass_bermuda_01/textures/grass_bermuda_01_diff_2k.png --w 256 --h 256 --az $az --out $O/bm_${v}_$az.png
done; done
for v in a b c d e; do
  $T sprite --gltf $M/weed_plant_02/weed_plant_02.gltf --nodes weed_plant_02_${v}_LOD0 --albedo $M/weed_plant_02/textures/weed_plant_02_diff_2k.png --w 384 --h 384 --elev 89 --out $O/weed_$v.png
done
