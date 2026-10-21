# Modding

Gothic Reforged has three mod tiers (`docs/ARCHITECTURE.md`): **data overlays** (this page),
**script mods** and **mechanic plugins**. Mods never contain original game files; they patch
the data the importer reads from the player's installation.

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
