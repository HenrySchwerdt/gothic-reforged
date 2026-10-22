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
step), `--isolated` (every quest alone, with only its prerequisites; prints one line per quest and the total), `--max-rounds N` (default 10000), `--json FILE`. Exit code 0 = every target completed successfully, 2 = otherwise.

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

Progress is any change of the game state, except the clock and the quest log (permanent lines
append to the log every time they run). Idle rounds advance the clock three hours so
`QuestRunning` infos can trigger.

- An info chosen without effect is *futile* until the next progress and stops counting for
  planning. Real data has many permanent quest lines (refusals, "not yet" answers) that would
  otherwise outscore everything and be chosen again forever.
- Without progress, each NPC with something useful is visited once.
- When none is left, the clock jumps a day ahead and the sweep repeats once. Only then does
  the run end, and the unfinished targets are analysed.

For an unmet game event the report lists every info that sets it and why that setter did not
run (`already told`, `available, never chosen`, or its own unmet condition), so the remaining
blockers can be checked against the data.

## What the bot does about the real data (P5 findings)

Each rule below fixed a class of "stuck" quests on the real install; the original behaviour is
cited as `Class::method (Game.dll address)`.

* **Quests nothing can start** (no activator info, no `RunQuest`): the world objective alone
  finishes them. `gCQuest_PS::CheckDeliveryEntitiesStatus (0x10156bb0)` runs for Open, Running
  and Lost quests and the hooks `OnReceiveItem (0x10158c70)`, `OnNPCKilled (0x10159110)`,
  `OnEnclaveStateChanged (0x10159710)`, `OnSpellCast (0x1015acc0)`, `OnEnter (0x10158e20)` do not
  look at the status. The bot performs the player action (kill, pickup, free the enclave, cast,
  enter) through those hooks: trace `cheat` kinds `pickup`, `free_enclave`, `cast`.
* **World objectives run when the dialogs on the route are dry**, not up front: killing an NPC
  earlier removes lines that need it alive, finishing a quest earlier removes lines that need it
  Open or Running (`Falk_HuntSnapper`, `Gunock_ReportGrok`).
* **Route closure**: the planner follows `Parent` chains (children of a plain `Info` line count,
  `gCInfo_PS::OnInfoScriptFinished` shows children of any finished info), the quests of parent
  lines, the infos that set needed events, and infos that start/succeed route quests.
* **Priorities**: infos that set a needed event first, deliveries last (they finish the quest),
  lines that end the conversation last within a menu, lines that close/fail/cancel a route quest
  (and NPCs whose auto-run `Important`/`News` line does) only when nothing else is left.
* **Refusals**: `IS_Running_NotEnoughGold` / `NotEnoughLP` (`gCInfo_PS::Execute 0x101c0a30`) are
  answered by earning the missing gold / learning points (trace `earn`), then retrying.
* **Required items / NPC states**: route infos that need items in the player inventory get them
  (hunted / bought / found); infos that need another NPC defeated, dead or talked to get that.
* **Developer menu**: the info owner `sh` (365 infos: "Befreie Myrtana!", gold/item gifts) is a
  cheat menu, not an NPC; the bot never talks to it (it used to "complete" quests through it).
* **Loop detection**: progress is a *new* game state; returning to a seen state (two companion
  quests that make each other `Lost`) is not progress. Having talked to an NPC alone is not
  progress either. Scores are cached per state, so a sweep costs one NPC per round.
* `gothic_gameplay::Assumptions::friend_reputation_gate`: `EnclaveFriend` / `PoliticalFriend`
  lines need fame >= 75 (`gCInfo_PS::Execute`); the port ignored it, so `Ishtar_Test_Running`
  was cleared by a line a player cannot yet use.

Known data defects (not port bugs): `Ishtar_Test` lists the delivery entity `"Zuben "` (trailing
space) and the original compares delivery entities with exact `bCString::operator==` in
`gCInfo_PS::OnDelivery (0x101c2620)`, so the Report delivery never matches; `Ishtar_Revolution`
depends on it. `Innos_Contact` vs `Beliar_Contact` (and the Adanos artefact quests) exclude each
other by design (`CloseQuest` in the chosen side's delivery line).

Debugging: `questbot <Data> --dump NAME` (an info, a quest with its infos, `event:NAME` for every
info that sets / clears / needs an event, `route:QUEST` for the planner's route, `--ws` for names
with stray whitespace, `--owners`). `--isolated --quest A --quest B` runs only those quests.

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
