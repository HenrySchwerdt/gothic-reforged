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
