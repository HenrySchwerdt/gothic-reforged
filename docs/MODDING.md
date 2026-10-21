# Modding

Gothic Reforged has three mod tiers (`docs/ARCHITECTURE.md`): **data overlays** (this page),
**script mods** and **mechanic plugins**. Mods never contain original game files; they patch
the data the importer reads from the player's installation.

## Data overlay mods (`crates/mods`)

A mod is a folder with a manifest and patch files:

```text
mods/
  forest_hermit/
    mod.toml
    patches/
      10_items.toml        # applied in file-name order
      20_hermit.toml
      30_alice.toml
      40_strings.ron       # TOML and RON are both accepted
```

The complete example is `mods/examples/forest_hermit` (a new NPC, item, quest with two
dialog infos, a new dialog line for an existing NPC and its strings).

### `mod.toml`

```toml
[mod]
id = "forest_hermit"        # [a-z0-9_-], unique
name = "The Forest Hermit"
version = "1.0.0"           # semver
description = "..."
authors = ["..."]

[dependencies]              # mod id = semver requirement; must be present and match
base_fixes = ">=1.0"

[load]                      # ordering hints without a hard dependency
after = ["other_mod"]
before = ["late_mod"]
```

**Load order** is a topological order of dependencies and `after`/`before` hints, ties broken
by mod id (deterministic). A mod whose dependency is missing or has a non-matching version is
not loaded (nor are mods depending on it); mods in a dependency cycle are not loaded. These
are reported as `LOAD` problems.

### Patch operations

| Section | Records | Identified by |
| --- | --- | --- |
| `quests` | quest definitions (`gothic_formats::quest::Quest`) | `name` |
| `infos` | dialog infos incl. their script lines (`gothic_formats::info::Info`) | `name` |
| `items` | item templates (`gothic_sim::item::ItemDef`) | `name` |
| `npcs` | NPC templates/entities (`npcs.json` form, `gothic_sim::data::NpcRecord`) | `name` |
| `strings` | localized strings | `key` |

```toml
[[items.add]]                       # new record; fields not given take their defaults
name = "It_HermitTea"
category = "Potion"
value = 25

[[quests.patch]]                    # change fields of an existing record
name = "Q_Letter"
set = { experience_points = 250 }

[[infos.delete]]
name = "B_Smalltalk"

[[strings.set]]                     # add or replace; languages not listed keep their text
key = "MOD_HERMIT_NAME"
texts = { English = "Hermit", German = "Einsiedler" }

[[strings.delete]]
key = "OLD_KEY"
```

Field names are the record types' Rust field names (`quest_type`, `delivery_entities`,
`condition_type`, `cond_player_knows`, `script = [{ command, entity1, entity2, id1, id2, text }]`,
...). `set` replaces whole top-level fields (a list is replaced, not appended). Every added or
patched record is converted back to its typed form, so unknown fields and wrong types are
caught. Records match case-insensitively; a patch cannot rename a record (delete + add).

### Conflicts and errors

Every write is attributed to (kind, record, field) and the mod that made it. If a later mod
writes what an earlier mod wrote (the same field, or the whole record by add/delete), that is
a **conflict**: the later mod in load order wins and the report lists it:

```text
mods: 2 applied (balance@1.0.0, economy@1.0.0), 4 operations, 1 conflicts, 0 errors, 0 load-order problems
CONFLICT item It_Herb field value: economy overrides balance
```

Operations that cannot apply are **errors** and are skipped, everything else still applies:
patching or deleting a missing record, adding an existing one, an unknown field, a value of the
wrong type, renaming, or a string in a language the table does not have.

### Using it

```rust
use gothic_mods::{ModData, apply, discover, resolve};
let base = ModData::from_database(&db);            // + items / NPC records if loaded
let (data, report) = apply(base, &resolve(discover("mods".as_ref())?));
eprint!("{}", report.to_text());                   // or serde_json::to_string(&report)
let db = data.to_database("German")?;
let items = data.items_typed()?;
let npcs = data.npcs_typed()?;
```

* **Hot reload** (development): `HotReload::new(base, "mods")`, then `poll()` once in a while;
  it returns the re-applied data whenever a file under `mods/` was added, removed or changed.
* **Export**: `gothic_mods::diff(&base, &edited)` produces the patch that turns one data set
  into the other (`apply(base, diff(base, x)) == x`), e.g. to turn edits made in a tool into a
  mod. Unsetting an optional field can only be expressed in RON (TOML has no null).

## Script mods (`crates/mods-script`)

Script mods are [Rhai](https://rhai.rs) scripts. They react to core **events** and act through
a small, capability-limited **command** API. They cannot touch files, the network, the clock,
other modules or `eval`, and every call runs under an operation budget. A script mod is a mod
folder whose `mod.toml` has a `[script]` section. The same folder can also carry data overlay
patches.

```text
mods/examples/chicken_rain/          # quest success -> chickens fall from the sky
  mod.toml
  scripts/main.rhai
mods/examples/orc_rhymes/            # all orcs speak in rhymes (subtitle hook)
  mod.toml
  scripts/main.rhai
```

```toml
[mod]
id = "chicken_rain"
version = "1.0.0"

[script]
entry = "scripts/main.rhai"              # default: main.rhai (must stay inside the folder)
capabilities = ["world", "spawn", "ui"]  # everything else is denied
```

### Events

A mod handles an event by defining a function with the event's handler name. The argument is
a map. Mods run in mod-id order; within a mod, events run in the order the core emitted them.

| Handler | Fields of `e` |
| --- | --- |
| `on_tick(e)` | `dt` (seconds, once per core tick) |
| `on_quest_changed(e)` | `quest`, `status` (`Running`, `Success`, `Failed`, ...) |
| `on_actor_spawned(e)` | `actor` (id), `name` |
| `on_actor_killed(e)` | `victim`, `killer` (id or `()`) |
| `on_line_spoken(e)` | `actor`, `speaker`, `key`, `text`, `player`, `species`, `guild` |
| `on_dialog_started(e)` / `on_dialog_ended(e)` | `npc` |
| `on_item_moved(e)` | `item`, `amount`, `gained` |
| `on_xp_gained(e)` | `amount`, `level_after` |
| `on_load()` / `on_reload()` | called after loading / after a hot reload |
| `rewrite_line(line)` | subtitle hook (needs `subtitles`): return a string to replace the text, nothing to keep it |

**State.** Rhai functions cannot see globals. Each mod has one persistent map, `this`, which
every handler can read and write. It survives hot reloads. Set it up in `on_load`:

```rust
fn on_load() { this.rains = 0; }
fn on_quest_changed(e) {
    if e.status == "Success" { this.rains += 1; }
}
```

### API

| Function | Capability |
| --- | --- |
| `log(text)`, `print(text)` | always (collected by the host, `take_log()`) |
| `rand(lo, hi)` (inclusive int), `rand_float()` (`[0, 1)`), `chance(p)` | always, seeded |
| `hero_pos()` -> `#{x, y, z}`, `actor(id)` -> `#{id, name, species, guild, pos, alive}` or `()`, `actors()`, `quest_status(name)`, `game_hours()` | `world` |
| `spawn_npc(template, #{x, y, z})`, `spawn_npc(template, pos, yaw)` | `spawn` |
| `give_hero(template, n)`, `give_npc(actor, template, n)` | `items` |
| `notice(text)` | `ui` |
| `fn rewrite_line(line)` is called | `subtitles` |

A call without its capability is a runtime error ("mod x has no capability 'spawn'").

### Determinism and limits

* **RNG:** each mod has its own SplitMix64 stream, seeded from the world seed and its mod id.
  The same seed and events give the same commands on every platform. There is no clock.
* **Budgets** (`ScriptConfig`) apply per mod:
  * 100 000 Rhai operations per call and 250 000 per tick;
  * 64 commands per tick;
  * limits on string, array and map size and on call depth.
  A mod that hits 3 errors, budget overruns included, is disabled until its next reload. Other
  mods keep running.
* **Hot reload:** `poll_reload()` re-reads the scripts. A changed script is recompiled and
  keeps `this`. A script that no longer compiles is reported and keeps running its previous
  version.

### Hosting

```rust
use gothic_mods_script::{ScriptConfig, ScriptHost};
let mut host = ScriptHost::new(ScriptConfig { seed: world_seed, ..Default::default() });
host.discover("mods".as_ref())?;                       // every mod.toml with [script]
// each core tick:
host.set_world(world_view);                            // hero, actors, quests, time
let commands = host.handle(&script_events);            // -> Vec<Issued { by, command }>
// for each subtitle:
let text = host.rewrite_line(&line).unwrap_or(line.text);
for e in host.take_errors() { warn!("{e:?}") }
```

With the `runtime` feature, `gothic_mods_script::runtime` converts between the game core and
the script vocabulary: `event_from_runtime(&Event) -> Option<ScriptEvent>` and
`command_to_runtime(&ScriptCommand, |template| npc_record) -> Option<Command>`. `Notice`
commands are shown by the presentation layer. Tests and tools drive mods with
`gothic_mods_script::StubCore`, a headless stand-in for the core.

