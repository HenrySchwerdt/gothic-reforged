#!/bin/bash
# Downloads the CC0 Poly Haven inputs of the Ardea ground/foliage recipe into .local/polyhaven.
# Poly Haven assets are CC0 (https://polyhaven.com/license); they are still not committed here.
set -e
D=.local/polyhaven
UA="gothic-reforged-asset-pipeline"
mkdir -p $D/tex $D/models
for id in sparse_grass brown_mud_02 cobblestone_floor_13 rocky_trail_02 mossy_rock coast_sand_04 leaves_forest_ground; do
  curl -sf -A "$UA" "https://api.polyhaven.com/files/$id" -o $D/files_$id.json
  mkdir -p $D/tex/$id
  for pair in Diffuse:diff nor_gl:nor AO:ao; do
    key=${pair%%:*}; out=${pair##*:}
    url=$(PYTHONUTF8=1 python -c "import json,sys;f=json.load(open('$D/files_$id.json',encoding='utf-8'));print(f.get('$key',{}).get('2k',{}).get('jpg',{}).get('url',''))" | tr -d '\r')
    [ -n "$url" ] && [ ! -s $D/tex/$id/$out.jpg ] && curl -sf -A "$UA" "$url" -o $D/tex/$id/$out.jpg
  done
done
for id in grass_medium_02 grass_bermuda_01 weed_plant_02; do
  curl -sf -A "$UA" "https://api.polyhaven.com/files/$id" -o $D/mfiles_$id.json
  PYTHONUTF8=1 python -c "
import json
f=json.load(open('$D/mfiles_$id.json',encoding='utf-8'))
g=f['gltf']['1k']['gltf']; print('$id.gltf', g['url'])
for rel,v in g.get('include',{}).items(): print(rel, v['url'])
print('textures/${id}_diff_2k.png', f['Diffuse']['2k']['png']['url'])
" | tr -d '\r' | while read rel url; do
    mkdir -p "$D/models/$id/$(dirname $rel)"; [ -s "$D/models/$id/$rel" ] || curl -sf -A "$UA" "$url" -o "$D/models/$id/$rel"
  done
done
echo "downloaded into $D"
