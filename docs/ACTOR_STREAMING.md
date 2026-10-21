# Asynchronous character building (`gothic_animation::stream`)

Spawning a character has two parts:
- **Building** (main cost): read and parse the part files, build skinned mesh buffers, decode
  textures, joint tables, the evaluation plan.
- **Committing** (cheap): move the buffers into `Assets` and spawn the entities.

`ActorStreamer` runs the build on worker threads and commits on the main thread. The commit
runs nearest-first and stays within a per-frame time budget. A crowd walking into view
therefore costs a few small commits per frame instead of one long stall.

```text
request()  --> queue (nearest to StreamFocus first) --> worker: BuildSource::build
  |                                                            |
  +-> placeholder root entity (PendingActor)     ActorBundle --+
                                                               v
stream_commit_system (Update, budget): assets once per look, entities per request
```

## API

```rust
app.add_plugins(ActorStreamPlugin)
   .insert_resource(ActorStreamer::new(Arc::new(my_source), StreamConfig::default()));
commands.spawn((Camera3d::default(), StreamFocus));            // distances measured from here

// in a system:
let root = streamer.request(&mut commands, BuildRequest::new(["Hero_Body", "Hero_Head"]),
                            transform, Stance::default());
```

* `request` returns the character's root entity at once: a placeholder with `Transform`,
  `Visibility` and `PendingActor`. When the character is committed, the root gets exactly the
  components of `ActorSpawner::spawn`: `Animator`, `CharacterComposition`, joints,
  `PartMeshes`, skinned meshes, attachments and weapon mounts. Systems that query those need
  no change. Despawning the root before then cancels the spawn.
* Looks are cached by `BuildRequest::key()` (parts plus attachments). Equal requests share one
  build and one set of assets. Textures are deduplicated by name across looks.
* `StreamConfig { workers (default: cores - 1, max 4), frame_budget (1.5 ms), max_per_frame (8) }`.
  While any character is ready, at least one is committed per frame.
* `spawn_blocking` builds and spawns on the calling thread, outside the budget. Use it for
  loading screens and tests; it is also the pre-streaming behaviour the benchmark compares
  against.
* `failure(key)` reports a failed build (pending requests for that look are dropped), and
  `is_idle()` tells whether nothing is queued or in flight. `StreamStats` has per-frame and
  cumulative numbers.
* A worker that panics in `build` is caught; the look is reported as failed.

`BuildSource` is the only thing a host implements: `fn build(&self, &BuildRequest) ->
Result<ActorBundle>`. It runs on worker threads and must not touch the ECS. An `ActorBundle`
holds only CPU data: `Mesh`, `Image` and `StandardMaterial` values, the rig, the joint tables
and the evaluation plan.

## Integration hooks (not done in this PR)

* **Real source:** split `ActorLibrary` into a CPU stage that returns an `ActorBundle`
  (reading `.xact` / `._xmac`, building meshes and the plan, decoding textures) and the
  existing `Assets` stage. `ActorLibrary` today interleaves file reads with `Assets::add`;
  the CPU part moves into a `BuildSource` that owns a `Vfs` handle, and `commit_assets`
  replaces the `Assets` part. `stream_synthetic::SyntheticSource` shows the shape.
* **Callers:** NPC spawning in `apps/game` (`play/**`, sectors, template spawning) calls
  `streamer.request` instead of `ActorSpawner::spawn`, with the camera as `StreamFocus`.
  Code that needs the character immediately (the hero at load, cutscenes) keeps using
  `spawn_blocking`.
* **GPU upload:** the commit only moves buffers into `Assets`; the render world uploads them
  in the next extract. If uploads of many large textures show up as spikes, bound them with
  Bevy's `RenderAssetBytesPerFrame`.

## Benchmark

`cargo run --release -p gothic-animation --example actor_stream_bench -- --mode stream|blocking`
spawns 200 synthetic characters in bursts while the app runs: 64 joints, 3 parts with
about 6 000 vertices and a 512² texture with mips each, and about 120 distinct looks. It
writes frame times as JSON. The frame loop is paced like a 60 Hz game (4 ms sleep per frame).

Headless release build in the CI container (shared CPU, other builds running in parallel):

| | blocking (old) | stream |
|---|---|---|
| frames over 16 ms | 6 | 0 |
| frame p99 | 185 ms | 2.2 ms |
| frame max | 334 ms | 15 ms |
| frames until all 200 spawned | 90 | 106 |

Streaming takes about 16 frames longer to show everyone. In exchange, the burst frames that
froze the game for up to a third of a second are gone. The few remaining 8–15 ms frames are
single commits that took that long. They appear at random spawns, which points to the
container being preempted by the parallel builds rather than to the commit itself; this
still needs measuring on an idle desktop.

## Tests

`cargo test -p gothic-animation --test stream`:
- everything requested is spawned, and each look is built once;
- the frame budget limits commits per frame;
- the nearest request spawns first;
- cancelled and failed requests are dropped without spawning;
- attachments and `spawn_blocking` work.
