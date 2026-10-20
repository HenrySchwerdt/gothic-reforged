# Architecture — Rust/Bevy Implementation

## Implemented foundation (2026-10-20)

The initial workspace deliberately instantiates only four boundaries: `gothic-formats` (bounded lazy archive reader), `gothic-importer` (installation inventory and provenance evidence), `gothic-world` (Bevy residency planner), and the inspector/game executables. The larger tree below remains a proposal; absent gameplay crates are not stubs or completed features.

`gothic-formats` uses buffered `Read + Seek`, indexes archive metadata, and fetches selected payloads on demand. It does not bulk-decompress the installation. `gothic-importer` depends on formats and contains no rendering. `gothic-world` contains no archive I/O or original game data. Its deterministic cell planner runs with a bounded number of residency changes and distinct load/retain radii. This is a synthetic residency laboratory; actual asynchronous loading, unloading, memory budgets, original world sectors, collision and navigation remain to be connected.

The game defaults to a finite headless smoke run. The `viewer` Cargo feature enables the Bevy 3D renderer and window backend. No physics dependency is selected until original collision shapes, units and behavior can be tested. Current version/toolchain and completed validation are tracked in `HANDOFF_CLAUDE.md`.

## Principles

- Use Bevy ECS/plugins/resources/schedules for orchestration; prefer idiomatic Bevy over a parallel bespoke ECS.
- Stable domain IDs distinguish source game entities, runtime entities and savegame identity.
- Separate **canonical imported data**, **authoritative simulation**, **presentation**, and **platform/UI** concerns.
- Define replaceable mechanic boundaries at action/state/event and save-schema levels; don't force trait objects where plugins and systems suffice.
- Favor safe Rust, explicit errors, dependency direction, observability, and testable deterministic behavior where practical.
- Bevy, physics and animation crate versions must be validated for current compatibility before pinning.

## Target architecture (decided 2026-10-20)

Goal: the shipped game needs only the original assets; all logic lives in our Rust code, runs faster than the original and is moddable.

### Layers (dependencies point downward only)

```text
L4 apps/game            Bevy host: input, camera, wiring of plugins, launcher/first-run cook
L3 presentation         gothic-render(world_loader/terrain/vegetation), gothic-animation,
                        gothic-audio, gothic-ui, gothic-physics  (Bevy plugins, no game rules)
L2 bridge               gothic-runtime: Bevy plugin that owns the core, ticks it at a fixed rate,
                        turns Bevy input into Commands and core Events into presentation messages
L1 core (no Bevy)       gothic-gameplay (quests, dialogs, state/save), gothic-sim (stats, combat,
                        AI, perception, routines, trade), ported Script_Game logic (gothic-script)
L0 data                 gothic-formats (bounded parsers), gothic-importer (VFS, world index,
                        cook), canonical GameDatabase with stable IDs
```

### Core rules

1. **The simulation is pure Rust and Bevy-free.** `Core::tick(dt, &[Command]) -> Vec<Event>` with a seeded RNG
   and a fixed 30 Hz tick. Presentation interpolates. Benefits: headless tests, replays, deterministic
   bug reports, and a save game that is simply the serialized core state (versioned, migratable).
2. **One direction of control.** Input -> `Command` (Talk, Attack, UseItem, Move...) -> core -> `Event`
   (LineSpoken, QuestChanged, Hit, Died, ItemMoved...) -> animation, audio, UI. The UI never mutates
   state; animation never decides hits.
3. **Engine interface as a trait.** Everything the core needs from the 3D world (positions, line of sight,
   pathing, routine points, time of day) goes through `WorldQuery`. The Bevy host implements it with
   physics and navmesh; tests use a 2D grid world. The ported Script_Game functions are written against
   exactly this interface (see `research/SCRIPT_ENGINE_INTERFACE.md`).
4. **Faithful by default, better by option.** Original behavior comes from the recovered rules
   (`GAME_RULES.md`, `COMBAT_AI_RULES.md`); improvements are named `Variants` / profiles, never silent
   changes. Original-data bugs found by questlint are fixed in a separate patch layer.
5. **Cooked asset cache.** First run converts the installation once into a versioned cache under the user
   data dir (GPU-ready meshes, BC textures, motions, packed world cells, the GameDatabase), memory-mapped
   at runtime. Original files stay the source of truth; the cache is rebuildable and never redistributed.
   Startup and streaming read only cooked data.
6. **Budgeted streaming.** World cells load on worker threads; main-thread spawning has a per-frame time
   budget; GPU instancing for repeated props and vegetation; CPU animation sampled in parallel.
   Performance budgets are tested in release smoke runs (target: <8 ms at 1440p on an RTX 3060 Ti).
7. **Mods.** Three tiers: (a) data overlays (TOML/RON patches over the GameDatabase: items, NPCs, quests,
   dialog lines), (b) a sandboxed script layer that subscribes to core Events and issues Commands
   (Rhai first, pure Rust, hot-reloadable), (c) Rust plugins replacing a mechanic behind its contract.
   Mods never touch original files.
8. **Proof of playability.** questlint (static) plus a headless bot that plays quests through the real
   core with the 2D or navmesh world; every quest gets a regression test once it passes.

### Current crate map

`formats`, `importer`, `world`, `physics`, `animation`, `audio`, `ui`, `gameplay`, `sim`, `runtime`
(L2 bridge: `Core` = sim + dialog + state at 30 Hz, `Command`/`Event` Bevy messages, JSON save/load);
apps `game` (`--play`: `apps/game/src/play/`), `inspector`, `questlint`, `dialog-cli`. To add: `script`
(ported Script_Game), cook step in `importer`, `mods`.

## Data flow

```text
User-provided installation
    -> version/locale/patch detection
    -> read-only extraction + format parsing
    -> validated canonical data + provenance manifest
    -> staged converted asset cache
    -> Bevy runtime/world simulation
    -> actions/events -> chosen mechanic plugin
    -> presentation + UI
```

Never silently insert guessed lore, texts, meshes or missing quest edges. Emit a blocking diagnostic or a visibly incomplete development result.

## Mechanic contracts

Input devices generate logical actions; gameplay receives validated commands and changes canonical state. UI reads projections/events; animation consumes outcomes rather than resolving hit/damage rules. Example: `Attack` action -> combat command -> hit evaluation -> damage/state event -> animation, sound, UI. Concrete systems may differ by profile, but shared save schema and quest hooks must remain compatible or have explicit migrations.

Classic variants are initial implementations; alternative plugins (e.g. `combat-modern`, `magic-modern`, `hud-modern`) are future implementations, not hardcoded flags spread throughout the codebase.

## Versioning and portability

Version canonical-import schemas, per-format decoders, manifests, game-state serialization, mechanic-API contracts and cache invalidation. Keep filesystem paths platform-neutral, support case differences and Unicode filenames, and avoid Windows-only assumptions. Test rendering, audio, input and shaders on each claimed platform.

## Coding standards

`cargo fmt --check`, strict Clippy configuration, automated tests, error-context propagation and focused logging. Security-conscious parsers treat all game files as untrusted; enforce bounds, length limits and resource ceilings. No unwarranted `unsafe`, hidden catch-all fallback, mock implementation presented as production, or huge panics on malformed inputs. Architectural changes require docs and affected tests.
