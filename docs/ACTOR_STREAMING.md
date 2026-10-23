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
* `request_on(commands, entity, request, position, stance)` does the same for an existing root
  (it keeps its own components); it is also how a failed look is re-requested.
  `BuildRequest::start_time` sets the playback offset and is not part of the look.
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

## Real source: `LibrarySource` (`gothic_animation::library_source`)

`ActorLibrary`'s loading is split into a CPU stage and an `Assets` stage. The CPU stage
(`build_part`, `skeleton`, `build_static`, `decode_texture`, `cpu_material`) reads and parses
`.xact` / `.xcmsh` / `.xshmat` / `.ximg`, applies the left-handed fix (x mirrored, winding
reversed by the reflection), builds the compact joint tables, inverse bind poses, the plan,
the facing fix and the materials. The blocking `part` / `composition` / `static_mesh_variant`
now call that stage and only add the result to `Assets` (with the same caches as before), so
the blocking path's behaviour is unchanged. `ActorLibrary::build_bundle(&BuildRequest)` runs
the same stage for a whole character and returns an `ActorBundle`.

`LibrarySource` implements `BuildSource` with a pool of `ActorLibrary` instances (archive
reads seek a file cursor, so every concurrent build needs its own handles). A library is
opened lazily on a worker when every pooled one is busy, so at most one per worker exists.
`LibrarySource::open(data_dir)` opens the four archives like `ActorLibrary::open`;
`LibrarySource::new(|| ...)` takes any opener (for example one that also loads a clip pack).

`BuildRequest::from_spec(&NpcSpec)` gives the request for what `ActorSpawner::spawn(spec)`
spawns: parts, weapons (drawn slot plus holster slot), slot attachments as `mesh@M<switch>`,
the scripted motion and `start_time` (not part of the look key). Weapon offsets other than
identity are refused (no caller uses them).

What a committed character shares with the blocking spawn: the skeleton, joint order,
part joint tables, inverse bind poses, mesh buffers, material parameters, textures,
`slot_mounts`, attachment joints and offsets, the weapon mount joints, and the starting
motion. For locomotion characters the bundle carries the idle of every stance
(`ActorBundle::idle`), and the commit plays the idle of the requested stance with
`Locomotion::playing` set, exactly as `ActorSpawner::spawn` does. Differences that remain:

* Assets are shared per look (one copy of a part's meshes per look that uses it) instead of
  per part; textures are still shared by name. More looks with the same body therefore cost
  more mesh memory than the blocking path.
* `Composition::key` is the request key (`Hero_Body+Hero_Head`), not the resolved archive
  paths.
* Clips registered on the game's `ActorLibrary` resource at run time (the `--combat souls`
  clip pack) are not in the source's libraries. Only an explicit `motion` naming such a clip
  would differ; the idle and facing come from the stock catalog in both paths.
* A broken idle clip for one stance is logged and skipped instead of failing the spawn.

## Game wiring (`apps/game/src/play/npc_stream.rs`), opt-in

`--stream-actors` (or `GR_STREAM_ACTORS=1`) with `--play` installs an `ActorStreamer` over
`LibrarySource::open(<data dir>)`, the `ActorStreamPlugin` and tags the walk camera as
`StreamFocus`. Without the flag nothing is installed and NPCs spawn exactly as before.

* `npcs::spawn_npcs` keeps choosing, placing and budgeting the queued characters. With the
  flag it calls `streamer.request` instead of `ActorSpawner::spawn`; the candidate's `visual`
  is the placeholder root.
* When the root is committed, `npcs::bind_spawned` (the code that used to follow the blocking
  spawn) adds the presentation components, puts the candidate in `live` and spawns or binds the
  core actor. Until then the character counts as pending (`pending_in_ring`, `Roster::total`),
  so loading screens and QA readiness checks wait for it.
* A look that fails with weapons is requested again without them on the same root
  (`ActorStreamer::request_on`), like the blocking retry; a failing bare look marks the
  candidate failed and removes the placeholder.
* The hero (and everything else using `ActorSpawner`) still spawns blocking.
* After each burst the log prints `play: actor stream idle: ...` with the build and commit
  times from `StreamStats`.

**GPU upload:** the commit only moves buffers into `Assets`; the render world uploads them in
the next extract. If uploads of many large textures show up as spikes, bound them with Bevy's
`RenderAssetBytesPerFrame`.

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

### Real NPC population (not measured yet)

Everything above uses synthetic characters. The comparison with the real Ardea population needs
the installed game and has not been run. On the machine with the installation:

```text
cargo run --release -p gothic-reforged --features viewer -- --play --data-dir "D:\SteamLibrary\steamapps\common\Gothic 3\Data" --bench 60 --bench-warmup 0 --bench-walk --bench-name npcs-blocking
cargo run --release -p gothic-reforged --features viewer -- --play --data-dir "D:\SteamLibrary\steamapps\common\Gothic 3\Data" --bench 60 --bench-warmup 0 --bench-walk --bench-name npcs-stream --stream-actors
```

`--bench-warmup 0` keeps the initial spawn burst in the numbers. Compare `p99_ms`, `max_ms`
and the frames over 16 ms in `.local/bench/npcs-*.json`, and note the `play: actor stream
idle:` log lines of the streamed run. Also check by eye that streamed NPCs look and animate
like blocking ones (textures, weapons in hand or holster, hair and beards).

## Tests

`cargo test -p gothic-animation --test library_source` (synthetic `.xact` / `.ximg` written
into temporary G3V0 archives, no game data):
- the bundle equals the blocking composition: skeleton, joint tables, plan length,
  `slot_mounts`, inverse bind poses, bounds, mesh buffers, materials and texture bytes, and
  the mirrored vertex positions;
- a streamed character has the same composition, parts, joints and mesh buffers as one from
  `ActorSpawner::spawn`, with locomotion in the requested stance;
- unknown actors, missing weapon meshes and unopenable archives fail the build with the
  blocking path's errors, and the streamer drops those requests;
- pooled libraries are reused; `BuildRequest::from_spec` encodes weapons, switches and the
  start time.

`cargo test -p gothic-animation --test stream`:
- everything requested is spawned, and each look is built once;
- the frame budget limits commits per frame;
- the nearest request spawns first;
- cancelled and failed requests are dropped without spawning;
- attachments and `spawn_blocking` work.
