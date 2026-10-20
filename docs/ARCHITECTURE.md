# Architecture — Rust/Bevy Implementation

## Principles

- Use Bevy ECS/plugins/resources/schedules for orchestration; prefer idiomatic Bevy over a parallel bespoke ECS.
- Stable domain IDs distinguish source game entities, runtime entities and savegame identity.
- Separate **canonical imported data**, **authoritative simulation**, **presentation**, and **platform/UI** concerns.
- Define replaceable mechanic boundaries at action/state/event and save-schema levels; don't force trait objects where plugins and systems suffice.
- Favor safe Rust, explicit errors, dependency direction, observability, and testable deterministic behavior where practical.
- Bevy, physics and animation crate versions must be validated for current compatibility before pinning.

## Proposed workspace (evolve as implementation dictates)

```text
apps/
  game/             Bevy executable
  launcher/         Local installation locator, first-run conversion
  inspector/        Format/asset/quest debug tooling
crates/
  formats/          Documented source file parsers
  importer/         Versioned, deterministic conversion
  asset-graph/      Dependencies, stable IDs, hashes, provenance
  game-data/        Canonical world/templates/quest/dialogue representation
  game-core/        Canonical runtime game state, simulation primitives
  gameplay-api/     Actions/commands/events/shared capability contracts
  world/            Streaming, cells/chunks, terrain and navigation
  npc-ai/           Routines, combat behavior, pathfinding
  quests/           Conditions, actions, dialogue interpreter
  inventory/        Equipment, trade, items, crafting
  progression/      Skills, faction reputation and character stats
  combat-classic/   Faithful combat implementation
  magic-classic/    Faithful spell implementation
  movement-classic/ Faithful movement implementation
  input/            Keyboard/gamepad mapping -> game actions
  presentation/     Visuals, animation, VFX, audio
  hud-classic/      Thin state-reading UI
  localization/     Localized text and speech resource resolution
  savegame/         Versioned persistence and migration
profiles/           Import/gameplay configuration (no original assets)
tests/              Licensed-free synthetic fixtures and regression tests
docs/
```

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
