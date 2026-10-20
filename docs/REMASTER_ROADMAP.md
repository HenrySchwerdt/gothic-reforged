# Future Remaster & Modern Gameplay Roadmap

**Deferred until faithful game completion**. Gothic Reforged should remain able to render and play the original-style game for comparisons and regression.

## Asset presentation layers

Keep original canonical asset identities while resolving visual representations by configurable profiles:
- `faithful`: original-compatible converted visuals.
- `enhanced`: improved materials, lighting, environment props and animation polish.
- `reforged`: optional high-quality visual and gameplay modernization.

Build a dependency-aware local conversion/asset processing pipeline: material PBR reconstruction, LOD creation, optional texture restoration, mesh validation, rig improvements, animation retargeting and baking. Use separate canonical source skeletons and enhanced destination rigs with documented retarget mappings and quality assurance. No assumption that AI generation or upscale produces professional quality without art direction and manual polish.

## World and character art direction

Preserve Gothic 3 geographic identity, landmarks, dialogue, factions and atmosphere. Seek modern dark-fantasy art quality: natural vegetation, convincing architecture, physically coherent materials, atmospheric lighting, readable silhouettes and performant distance rendering. Measure frame-time and memory costs continuously. Do not copy Elden Ring or another game's distinctive designs/assets.

## Referencing Gothic 1 Remake and Gothic 2

May use the legally owned games for permitted technical study and visual inspiration. **Do not assume ownership grants rights to extract, adapt, package or ship other games' assets**. Import of Gothic 1 Remake protected assets is out of scope pending explicit legal clearance. Any own remaster assets require documented licenses and provenance.

## Replaceable player experience

Introduce optional systems without touching the canonical quest/inventory core:
- Controller-first remapping, accessible keyboard/mouse controls.
- Modern third-person camera and camera collision.
- Modern combat profile with lock-on, hit reactions, dodges and readable timing.
- Refined spell selection/casting and localized modern HUD.
- Modern inventory, quest journal and accessible menu navigation.

Each profile must declare compatibility with saves, quest scripting and other gameplay implementations; regression tests protect original progression. No firm promise of AAA visual quality or performance prior to validated prototypes.
