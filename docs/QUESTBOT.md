# Quest bot (`crates/questbot`)

Headless harness that plays the Bevy-free quest/dialog core (`gothic-gameplay`) through
quests like a player and reports the ones it cannot finish. Issue: TOOL-01.

```sh
cargo run -p gothic-questbot -- --fixture --trace                   # synthetic 10-quest database
cargo run -p gothic-questbot -- --fixture --broken                  # + 2 unfinishable quests
cargo run -p gothic-questbot --release -- "D:\...\Gothic 3\Data" --json .local/questbot.json
cargo run -p gothic-questbot --release -- <Data> --quest Q_Name --quest Q_Other --trace
```

Options: `--language German|English|...` (string table language, default German),
`--quest NAME` (repeatable; default: every quest), `--strict` (no world assumptions: owner
proximity, unknown NPC inventories and BringNpc deliveries fail), `--trace` (print every
step), `--json FILE`. Exit code 0 = every target completed successfully, 2 = otherwise.

## How it plays

Each round:

1. **World cheats** for running quests whose objective is not a dialog: item quests get their
   delivery items, kill / drive-away targets are killed, defeat / arena / sparring targets
   are defeated, enter-area quests enter their destination (once per quest, through the
   quest manager's own world hooks `npc_killed`, `npc_defeated`, `area_entered`).
2. **Planning** (`plan::Planner`): relevant quests are the targets plus their
   `FinishedQuests` prerequisites; useful events are the `CondPlayerKnows` events of their
   infos, closed over the infos that set them. Every available info is scored: info of an
   unfinished relevant quest (delivery > success > activator) > sets a useful event >
   starts/finishes a relevant quest by script > unexplored talk.
3. **Talking**: the NPC with the best offer is addressed; the bot keeps choosing the best
   unchosen menu entry (descending into dialog trees) and leaves when nothing useful remains.

Two rounds without any change of the game state (the clock excluded; idle rounds advance it
three hours so `QuestRunning` infos can trigger) end the run; unfinished targets are then
analysed.

## Report format

`--json` writes (schema `gothic-questbot/1`):

```json
{
  "schema": "gothic-questbot/1",
  "targets": 12,
  "completed": ["Q_Scout", "..."],
  "unsuccessful": [{ "quest": "Q_X", "status": "Failed" }],
  "stuck": [
    {
      "quest": "Q_Lost",
      "status": "Open",
      "reason": "not_activated",
      "prerequisites": [],
      "blockers": [
        {
          "info": "H_Lost",
          "owner": "Hermit",
          "condition": "Activator",
          "unmet": "Event(\"Never_Set\")",
          "hint": "no info sets game event Never_Set"
        }
      ]
    }
  ],
  "rounds": 17,
  "trace": [
    {
      "step": 1,
      "hours": 0,
      "action": { "kind": "talk", "npc": "captain", "info": null, "label": "(dialog start)", "ran": ["C_Scout"] },
      "quest_changes": [{ "quest": "Q_Scout", "from": "Open", "to": "Running" }]
    },
    {
      "step": 2,
      "hours": 1,
      "action": { "kind": "cheat", "cheat": "enter", "who": "PC_Hero", "area": "Area_Pass", "quest": "Q_Scout" },
      "quest_changes": [{ "quest": "Q_Scout", "from": "Running", "to": "Success" }]
    }
  ]
}
```

| `reason` | Meaning |
| --- | --- |
| `prerequisite_unfinished` | a `FinishedQuests` entry is not finished (`prerequisites` lists them) |
| `not_activated` | still `Open`; the activating infos are blocked (see `blockers`) |
| `objective_unmet` | `Running`; no delivery/running info is available and no cheat applies |
| `no_activator` | nothing in the data starts the quest (no activator info, no `RunQuest`) |

Each blocker names an info that would advance the quest, its condition type, the unmet rule
from `gothic_gameplay::conditions::Unmet` and, where the data allows, a hint: which infos set
a missing game event (or that none does), the status of a prerequisite quest, a missing
engine trigger (`Fight`, `PickPocket`, ...) or an NPC flag no script has set.

The text output prints the same: a summary line, the trace with `--trace`, then
`UNSUCCESSFUL` and `STUCK` lines with indented blockers.

## Hooks for the real game

* The fixture (`questbot::fixture`) uses the game's INI record format; the real run loads
  `Quests.pak`, `Infos.pak`, `Strings.pak` (and `Templates.pak` item names) through
  `GameDatabase::load_install`.
* Combat and movement are cheated through the quest manager hooks; driving `crates/sim`
  instead (real fights for `Kill`/`Defeat`, walking for `EnterArea`/`BringNpc`) only needs
  `QuestBot::world_cheats` replaced.
* `Assumptions::PERMISSIVE` (default) treats NPC proximity and unknown NPC inventories as
  satisfied; `--strict` shows which quests depend on that.
