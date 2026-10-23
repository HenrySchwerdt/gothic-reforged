# Static props at scale (`crates/static-render`)

`gothic-static-render` renders large numbers of static props (houses, rocks, barrels, trees)
cheaply. It combines Bevy 0.20's GPU-driven path with coarse culling of its own:

| Layer | What it does | Where |
| --- | --- | --- |
| Batching | All instances of a registered mesh share one `Mesh` + one material handle per LOD, so Bevy's GPU preprocessing puts them in **one multi-draw-indirect batch**. | Bevy (`GpuPreprocessingMode::Culling`) |
| Per-instance culling | `NoCpuCulling` on the camera: instance frustum tests run in the GPU preprocessing compute pass instead of the per-entity CPU loop. | Bevy, enabled by this crate |
| Cluster culling | Instances are grouped into BVH clusters (64 by default). Each frame, clusters are tested against the camera frustum and draw distance. Clusters nobody sees are hidden as a whole, so per-entity CPU work only touches the visible clusters. | `ClusterBvh` |
| Shadow cascade culling | For each cascade of the `StaticShadowLight`, the cascade's view slice is boxed in light space and extended towards the light (`caster_reach`). Clusters inside stay alive as casters even when off screen; all others drop out of the cascade's caster lists too. | `cascade_caster_volume` |
| Hysteresis | A cluster is shown as soon as it is needed, but hidden only after `hide_after_frames` (30) frames out of view and out of every cascade. Every visibility flip makes Bevy re-bin the cluster's entities, so turning the camera back and forth would otherwise cost frame spikes. | `StaticRenderPlugin` |
| LOD | Each level is its own entity with Bevy's `VisibilityRange` (dithered cross-fade over `margin`). | `LodRanges` |

## API

```rust
app.add_plugins(StaticRenderPlugin::default());     // cluster_size, gpu_culling, caster_reach, hide_after_frames
commands.spawn((Camera3d::default(), StaticCullCamera));
commands.spawn((DirectionalLight { shadow_maps_enabled: true, ..default() },
                CascadeShadowConfigBuilder { num_cascades: 3, ..default() }.build(),
                StaticShadowLight));

let house = statics.register(StaticMeshDesc {
    lods: vec![lod0, lod1, lod2],                       // Handle<Mesh>, finest first
    ranges: LodRanges::new(vec![60.0, 160.0, 400.0], 6.0), // end distance per level, fade width
    material,                                           // Handle<StandardMaterial>
    bounds: Aabb::new(min, max),                        // local, covering all levels
    cast_shadows: true,
});
statics.add(house, Transform::from_xyz(x, 0.0, z));     // spawned + clustered next Update
```

`StaticRenderStats` (resource) reports instances, entities, clusters, visible and
shadow-only clusters, visible instances, BVH nodes visited and cull time per frame.

The core (`ClusterBvh`, `Frustum`, `cascade_caster_volume`, `LodRanges`) is Bevy-free and
tested against brute force: no visible instance is ever culled, and the result is
deterministic.

## Benchmark

```sh
cargo run --release -p gothic-static-render --example bench --features demo -- \
    --instances 10000 --frames 600 --out bench.json
# A/B: --no-cluster-culling, --no-gpu-culling, --no-lod
```

The scene is a procedural town: houses (3 LODs), trees (3 LODs), barrels (2 LODs) and crates.
It has a 4 km ground, one sun with 3 shadow cascades, and a deterministic camera flight.
After the warm-up, the bench writes JSON with:
- frame-time mean, p50, p95, p99 and max (ms);
- instances, entities and clusters;
- average visible instances, visible clusters and shadow-only clusters;
- average cluster-cull time;
- the GPU adapter.

The target from the issue is under 4 ms at 10k props with 3 cascades on a mid-range GPU.
Measure that on real hardware.

Reference run on a software rasteriser (lavapipe, 4 cores, 1280×720, 90 frames):

| | mean | p50 | p95 | p99 | instances drawn |
| --- | --- | --- | --- | --- | --- |
| cluster culling | 135.8 ms | 129.8 ms | 151.5 ms | 165.5 ms | ~2 060 of 10 000 |
| `--no-cluster-culling` | 135.8 ms | 123.0 ms | 151.0 ms | 157.7 ms | 10 000 |

On a CPU rasteriser the pixel work dominates, and Bevy's GPU culling already drops off-screen
instances, so both variants are equally fast. The cluster cull itself costs about 40 µs per
frame. What it saves is CPU work per visible entity (extraction, shadow caster lists for
3 cascades), and that only shows up on a real GPU, where the CPU side is the bottleneck.
Without hysteresis, the same run showed p99 spikes of more than 600 ms. A software rasteriser (lavapipe) only shows that the bench
runs and writes its report.

## In the game (`--static-render`)

Opt-in; without it the walk spawns and culls its props exactly as before. Glue:
`apps/game/src/walk/cloud/statics.rs`, plus a one-line hook in `walk::spawn_pending` that marks
the parts it routes.

```sh
cargo run --release -p gothic-reforged --features viewer -- --walk --static-render      # cluster + GPU culling
cargo run --release -p gothic-reforged --features viewer -- --walk --static-render cluster
GR_STATIC_RENDER=gpu cargo run --release -p gothic-reforged --features viewer -- --play
```

What is wired:

* The world loader's static parts (`Static` and the merged small / mid / big batches; not
  terrain, trees, ground cover or water, nor parts hidden by `GR_HIDE`) go into the crate's
  `ClusterBvh` (64 per cluster). Each frame the clusters are tested against the player camera's
  frustum (up to its far plane) and the caster volume of every cascade of the sun; clusters
  outside all of them are hidden after 30 frames (shown again at once).
* `gpu` (on by default): `NoCpuCulling` on the player camera, so Bevy's GPU preprocessing does
  the per-instance frustum culling. This applies to everything that camera draws.
* The props keep their `GothicMaterial`, their shared mesh / material handles (which is what
  Bevy batches into multi-draw-indirect calls), their per-instance `VisibilityRange` (the
  original small-object culling and LoD stages) and their shadow flags, so the culling rules
  are unchanged: cluster culling only removes what no view or cascade can see.
* While tiles stream in or out, the BVH is rebuilt at most every 30 frames; a prop that
  arrived since the last rebuild stays visible until the next one.

Not used: the crate's `StaticRenderPlugin` / `StaticMeshes` registry. It spawns its own
entities with a `StandardMaterial` and one LoD table per mesh and cannot remove instances,
whereas the walk streams tiles in and out, uses its own material and computes LoD ranges per
placed instance. Routing the props through it would need a material-generic registry with
per-instance ranges and removal.

Not verified: the effect on real data. The issue's bench result (3.79 ms mean / 4.9 ms p99 at
10k props on an RTX 3060 Ti) is the synthetic town, not the game. To re-measure, run the walk benchmark with and
without the flag on the same machine and compare the JSON in `.local/bench/`:

```sh
cargo run --release -p gothic-reforged --features viewer -- --walk --bench 60 --bench-walk --bench-gpu --bench-name statics-off
cargo run --release -p gothic-reforged --features viewer -- --walk --bench 60 --bench-walk --bench-gpu --bench-name statics-on --static-render
GR_PROF=1 cargo run --release -p gothic-reforged --features viewer -- --walk --static-render   # + cluster stats every 10 s
```

With `GR_PROF` the log prints props, clusters, visible and shadow-only clusters, rebuilds and
the cluster cull time per frame (`static-render: ...`).
