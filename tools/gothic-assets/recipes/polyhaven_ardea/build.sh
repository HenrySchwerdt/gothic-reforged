#!/bin/bash
# Builds the local Poly Haven (CC0) ground + foliage remaster profile for Ardea.
T=.local/target-meshy/tool/release/gothic-assets.exe
P=.local/remaster/polyhaven-ardea
X=.local/polyhaven/tex
rm -rf $P
tex() { # target normal-target asset real-size [extra args]
  local t=$1 n=$2 a=$3 r=$4; shift 4
  local ao=(); [ -s $X/$a/ao.jpg ] && ao=(--ao $X/$a/ao.jpg)
  $T texture --profile $P --target $t --normal-target $n --image $X/$a/diff.jpg --normal $X/$a/nor.jpg "${ao[@]}" --real-size $r --tile 7 "$@" | cut -c1-200
}
tex G3_Nature_Ground_Grass_01_Diffuse_01 G3_Nature_Ground_Grass_01_Normal_01 sparse_grass 3.2 --flatten 0.8 --match-histogram 0.85
tex G3_Nature_Earthground_01_Diffuse_02 G3_Architecture_Ground_City_04_Normal_01 brown_mud_02 1.8 --flatten 0.8 --match-histogram 0.85
tex G3_Architecture_Ground_City_03_Diffuse_01 G3_Architecture_Ground_City_03_Normal_01 cobblestone_floor_13 2.2 --flatten 0.8 --match-histogram 0.85
tex G3_Nature_Ground_Path_02_Diffuse_01 G3_Nature_Ground_Path_02_Normal_01 rocky_trail_02 2.8 --flatten 0.8 --match-histogram 0.85
tex G3_Nature_Wall_Stone_02_Diffuse_01 G3_Nature_Wall_Stone_12_Normal_01 mossy_rock 3.6 --match-histogram 0.85
tex G3_Nature_Dunes_PlainSand_02_Diffuse_03 G3_Nature_Dunes_PlainSand_02_Normal_03 coast_sand_04 4.0 --match-histogram 0.85
tex G3_Nature_Ground_Forest_01_Diffuse_01 G3_Nature_Ground_Forest_01_Normal_01 leaves_forest_ground 1.8 --flatten 0.8 --match-histogram 0.85
$T texture --profile $P --target G3_Nature_Plant_Herbs_Composit_01_Diffuse_S1 --image .local/polyhaven/atlas/s1_new.png --cutout --cutoff 0.39 | cut -c1-120
