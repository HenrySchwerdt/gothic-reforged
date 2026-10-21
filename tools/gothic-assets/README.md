# gothic-assets — remaster asset pipeline

Turns generated models (binary glTF, e.g. from Meshy) into replacements for individual original
Gothic 3 assets. The output is a **local remaster profile** that the game reads with
`GR_REMASTER=<dir>`; any asset without an override keeps the original (no profile = faithful).

Own Cargo workspace (glTF, image and BC-encoder crates stay out of the game's lock file):

```bash
source .local/env.sh
cd tools/gothic-assets
CARGO_TARGET_DIR=../../.local/target-meshy/tool cargo +stable-x86_64-pc-windows-gnu build --release
```

## Commands

| Command | Does |
|---|---|
| `info NAME` | original mesh / actor: bounds, submeshes, materials, slots, collision |
| `views NAME --out DIR [--az 45 --elev 25] [--glb F]` | CPU-rendered reference views (input for multi-view generation); with `--glb` also the fitted model |
| `obj NAME OUT.obj`, `glb NAME OUT.glb` | original geometry for inspection |
| `static --glb F --target NAME --profile DIR` | static prop / building replacement |
| `actor --glb F --target ACTOR --profile DIR [--bake 1+2] [--own-uv]` | skinned character part replacement |
| `texture --image F --target STEM --profile DIR [--normal F --normal-target STEM] [--ao F] [--real-size M --tile 7] [--match-histogram 0.85] [--flatten 0.8] [--cutout]` | replaces one texture (ground, foliage atlas) |
| `sprite --gltf F --nodes N --albedo PNG --w --h [--az --elev] --out PNG` | alpha cut-out sprite of a plant model |
| `terrain-survey [--x --z --r]`, `mat NAME`, `png STEM OUT`, `list SUBSTR` | inspection helpers |

Recipes: `recipes/polyhaven_ardea/` (CC0 ground textures and grass for Ardea).

## What `static` does

1. glTF space (right-handed, Y up, metres, CCW) -> Gothic space (`x -> -x`, centimetres after the fit).
2. Fit onto the original: yaw in 90-degree steps, uniform scale (`--scale volume|height|footprint`),
   floor and centre alignment, translation-only ICP; chamfer distance and per-axis extent ratios are
   reported, deviations > 5 % / 25 % are warnings (wrong proportions or axes).
3. Winding check against the original (auto flip when most triangles disagree).
4. meshopt LODs (`--lods 0.5,0.2`), collision (`--collision auto|generated|original|none`; `auto`
   keeps walk-through props walk-through), `--lowpoly NAME` replaces a separately placed distance
   mesh with the coarsest LOD.
5. Textures for the Gothic material shader: sRGB DXT1/DXT5 diffuse, DXT5nm normal map (x in alpha,
   y in green, OpenGL +Y), full mip chains; `--gain auto` matches the original's mean albedo.
6. Writes `<stem>.grmo`, DDS files, `manifest.txt` entries and `reports/<stem>.json`.

## What `actor` does

The generated mesh is fitted onto the original **bind-pose** geometry. Every new vertex takes its
skin weights from the closest point of the original surface (blended, top 4, normalised), so the
original skeleton, joint positions, slots (`Slot_Hair`, `Slot_Beard`, `Slot_Helmet`, weapon slots)
and every original animation keep working. Every new triangle takes material and UV chart of the
closest facing original triangle, so material-switch variants (`_S<n>` textures) keep working.
`--bake 1+2[=STEM]` bakes the generated colour into the original UV layout of those materials and
overrides that one texture stem (other variants stay original).

## Runtime

`gothic_formats::remaster` holds the format (`.grmo`, DDS, manifest) and `Profile::global()`;
hooks: `apps/game/src/world_loader.rs` (static meshes, textures, collision) and
`crates/animation/src/library.rs` (actor geometry, textures).

## Legal

Everything derived from original assets or renders of them (reference views, generated models,
the profile) stays under the gitignored `.local/`. Only this code is in the repository.
