# NPC / combat AI / perception / spells wiring: status (day/cov-npc)

Measured with the `port-coverage` build (`WIRING_COVERAGE.md` method) over the baseline of 43 scenarios plus the
new `spell_zoo`, `routine_zoo`, `crime_theft`. Scope: `crates/script/src/**` except `player/` (335 ported functions).

* Never called: **146 at the baseline of this work -> 105 now.** Script `ZS_*` states entered: **41 of 267 -> 261 of 267**
  (not entered: `ZS_Dead`, `ZS_Dead_Loop`, `ZS_PiercedDead`, `ZS_RagDollDead*` are skipped by `routine_zoo` on purpose, `ZS_Nope` is a dummy).

## What changed

| Area | Before | Now |
| --- | --- | --- |
| Death / knock-out | `Sim::kill` / `Sim::defeat` re-implemented `Kill` / `Defeat` | they run `gothic_script::down::kill/defeat` (XP through the ported `GiveXP`, Attack crime through `memorize_player_crime`, party, enclave notice, plunder); unconscious time is `FUN_10007790` (30 s for NPCs knocked out by the player, the old 5 s misread the argument) |
| Perception broadcasts | `Sim::broadcast` copy of FightSound / Murder | `send_perception_fight_sound/murder/defeat` of `script::assess` |
| Theft | `witness_theft` (tests only), nothing in play | `PS_Normal_Take` calls `send_perception_take_item` |
| Task callbacks | `OnSleep`, `OnAnimalSleep`, `OnFollow*`, `OnGuidePlayer` registered by the states but dispatched by nobody | `routine::run_callback` behind `combat::run_callback`; gate guards scan only when `OnGateGuard` fires |
| Speech | `StartSayAttackReason`, `StartSayAttackEnd`, `StartSayAargh` never called | called by `ZS_Attack` (S:14480), `ZS_Attack_End` (S:1a610) and the stumble primitives, order checked in the decompile |
| `_AI_*` primitives | `start_stand_up/shoot/stumble/lie_knock_down/die` dead | called from the NPC executor (`StandUp`, `Shoot`) and from the hit path (result of `AssessHit`) |
| Spells | only the fireball path ran | `spell_zoo` casts every spell template of the item data (rune, scroll, potion, druid stone) at a character of the world; spell potions now drink through `PS_MagicPotion` (`use_stack`), `MagicHeal` is registered as the alias of `MagicCure` (same address 0x10053870 in the script registry) |
| Routine tasks | ~40 `ZS_*` states reached | `routine_zoo` puts every `ZS_*` state on a character and drives it through the real loop (`Sim::qa_enter_state`) |

## Still never called, with the reason

| Functions | Reason |
| --- | --- |
| `magic_burn/freeze/refresh/stun/dispel/keep_stamina/speed_up/summon_companion`, `is_game`, `is_large` | no spell of `spells.json` and no item names them (`Spl_Refresh`, `Spl_ShockWave`, `Spl_SpiritLink`, `Spl_Resurrect`, `Spl_BanishVermin`, `Spl_SummonImp` have no item); they are only referenced by entity templates the port does not spawn (registry wrappers `sub_d8690..d8750`). Candidates for DEAD; verify against the entity templates before deleting |
| `can_transform_into`, `transform_duration`, `transform_back`, `return_to_body`, `send_perception_transform` | transformation lasts 60 s+ (`GetTransformDuration`), `spell_zoo` leaves after 5 s per spell; next step: a dedicated scenario that waits it out |
| `join_player_party/mob/slavery`, `dismiss_*`, `wait_player_party`, `can_join_player_party`, `start/stop_guide_player`, `stop_bring_npc`, `excuse`, `bribe`, `pick_pocket`, `guard_attack_reason`, `has_*_news`, `start_say_confirm/follow/holding_position/refuse_talk/crime_refuse/zone_refuse`, `boss_news_plan`, `crime_investigation_plan`, `guard_news_plan` | called by the dialog layer (Info actions, `_AI_Say*News`), which the sim drives from its own dialog database; needs the dialog bridge (owner of the dialog/UI work) |
| `select_routine_destination`, `rtn_monster/dead/player`, `send_to_arena`, `is_free_destination`, `travelling_walk_mode`, `routine_point`, `use_object` | the host registers routine points from the world data (`ScriptTables::set_routine_point`) instead of `SelectRoutineDestination` scanning the interact points; replacing that is a data-seam change (world agent) |
| `on_sleep`, `on_animal_sleep`, `on_gate_guard`, `is_audible`, `call_help`, `get_player_crime`, `memorize_enclave_crime` | now dispatched but no scenario puts the hero near a sleeping NPC / a guard sees a theft (`crime_theft` has no secured zone); next: `crime_theft` in a secured zone, `sleep_perception` |
| `assess_cmd_attack/follow`, `send_perception_cmd_*`, `assess_warn`, `assess_player_enter_zone`, `send_perception_enter_zone`, `send_perception_preach`, `assess_defeat`, `send_perception_defeat` | triggered by hero input (companion commands, warning), zone entry, preaching NPCs and a knocked-out fighter seen by a bystander; need hero-side hooks (player/world agents) or a defeat QA |
| `start_parade`, `start_reload`, `start_get_up` | superseded: the NPC parade is `begin_parade_command` (S:46810 plan), crossbow reload and get-up attacks run on the hero-side plans; no caller can reach them. Mark DEAD |
| `pipi_stumble` | only on a zero-damage hit |
| `ani_state_by_action`, `dcc_radius`, `first_person_camera_allowed`, `refresh_hit_points`, barrier callbacks, `focus_name_attitude` | camera / physics / HUD / world owners |
| stats setters (`set_attribute`, `add_attribute`, `set_protection`, `add_protection`, `set_pool_max`, `add_pool_max`, `diff_to_player_level`) | the sim keeps its own stat model (`economy::sync_hero`); item stat application is in `gameplay::inventory` (not this area) |

## Known flake

`npc_orc1` ("the damage lands at 60 percent of the Hit clip", tolerance 0.2 s) failed on 2 of 5 runs while the machine was
shared by five agents (frame hitches), passed on the others; not changed by this work.


## Routine points: how the original resolves them, and what is really missing (day/cov-npc)

**Resolution rule (decompile).** A character refers to its routine points with `bCPropertyID` values (20 bytes:
16-byte GUID plus a validity word, `PROPERTY_STREAMS.md`). `eCEntityProxy::ResolveEntity` (Engine 0x1022e470) looks the
entity up with the proxy's `bCPropertyID` (proxy + 8) as the key; `eCEntityProxy::Compare` (0x1022e250) is
`bCPropertyID::operator==` (SharedBase 0x1000ea00), which compares exactly four dwords (offsets 0, 4, 8, 0xc = 16 bytes);
`bCGuid::operator==` (0x100025b0 -> 0x10002120) also compares 16 bytes. The last 4 bytes are not part of the identity.
Implemented once in `gothic_session::guid` (`guid_key`, `GuidIndex`); `apps/game` (`WorldObjects::by_guid`) uses it, the Quest
host can replace its own `take(32)` code by the same type.

**Exporter.** `gothic-inspector object-dump` now runs a first pass over every world document that collects the identities
used in `gCNavigation_PS.WorkingPoints / RelaxingPoints / SleepingPoints` (3 943 distinct identities for the 10 279 characters of the
world) and then exports every entity with such an identity whatever its classes (fields `routine_point`, `classes`), and lists the
references no world document answers under `unresolved_routine_points`. Result for the whole world: 28 786 entities (was 28 776; 10 more,
e.g. `gCAnchor_PS` entities such as `Geldern_Hang_Elite`, `Faring_Patrol_Arena`), 165 identities that no document answers.

**The "missing" points are no points.** Of the 165 unresolved identities, three account for 17 000 of the 17 342 references:
`6d62c61f...` (working slot), `aef515f1...` (sleeping slot) and `ef0ebc55...` (relaxing slot) are the slot defaults every character
without a point of its own carries (per slot exactly one id; 136 / 170 / 171 of the 315 characters within 150 m of Ardea). They are in
no `.node` / `.lrentdat` entity and in no template (`TemplateCatalog`, 1 192 NPC templates and all other `.tple`), so the original's
`ResolveEntity` returns null for them as well; a character with such a slot has no point there. One more id is unresolved,
the null id counts 65. For the 315 characters near Ardea: 648 references, 170 resolve (exact match on the 40 digits: 137), 477 are
slot defaults, 1 is unknown; characters with at least one point 63 (exact) -> 72 (16-byte match); the exporter change adds nothing
inside Ardea (its 10 new entities lie elsewhere). Across the world: 23 404 references, exact 5 358 -> 6 127 by 16-byte match, characters
with a point 2 248 -> 2 524.

## Open issue found by `spell_zoo` (equipment refactor, not changed here)

After the merge of `day/desktop2` (equipment derived from the stack list, commit 53f1f4c) the hero's runes granted at run time
behave differently in the running game: (1) `Command::GiveHero` put the item into the game state only, the stack had no rule data and
could not be held, so a second rune never replaced the first (fixed in `Core`: the item goes into the hero's bag with its
`ItemInfo`; `crates/runtime/tests/spells.rs` covers consecutive casts at core level and passes). (2) In the QA run the casts still
act "one spell behind" for part of the list (the effect of the previous rune appears: `Poison` -> the ice bomb area, `Sleep` ->
ice bomb, `SummonSkeleton` -> `Shadowbeast`), while the same sequence at core level is correct. The difference between the two is
the presentation: the hero's `HoldInventoryStacks` instruction is executed by the host (`hero_hold_swap`) with the stack indices
of the time the state queued it. Owner: equipment / hero presentation (desktop2).

## Companion commands, news speech, QA timing (day/cov-npc)

* **`SendPerceptionCmdAttack` / `SendPerceptionCmdFollow` (S:31ff0 / S:32170).** No script function of the decompiled library
  calls them (the only references to FUN_10031ff0 / FUN_10032170 are their registration wrappers `sub_d2e60` / `sub_d2ec0`), the
  info command table of the data has no such command (`info-survey` `script_command_histogram`: `JoinPlayer` 2, `FullStop` 1, `SayConfirm` 3,
  `SayHoldingPosition` 1, ... no attack/follow order), and the Game.dll decompile does not name them. `StartSayFollow` only answers
  `if (player == other)`: the order is given by the player's input, so the trigger is a key of the hero, not a dialog. The port now offers
  `Sim::player_companion_command(attack)` / `Command::CompanionCommand { attack }` (test `crates/sim/tests/companion.rs`); a host binds a key
  to it. This also fixed a real gap: `SimHost` never implemented `focus_entity`, so `AssessCmdAttack` could never see the player's focus.
* **Info voice commands.** `SayConfirm`, `SayHoldingPosition`, `SayGuardNews` and `SayBossNews` of the info scripts now emit
  `Output::Voice` next to their text line; the core runs `speech::start_say_confirm`, `start_say_holding_position`, `guard_news_plan`
  (counts of the enclave's murders and thefts) and `boss_news_finish` (which also starts the revolution at level 3) for the speaker.
  `crime_investigation_plan` (`_AI_SayCrimeInvestigation`, S:3be50) has no caller in the library and no info command: unreached in the
  shipped game.
* **QA timing.** `npc_orc1`'s "damage lands at 60 percent of the Hit clip" compares frame timestamps; it now widens its 0.2 s tolerance by twice the
  longest frame between the clip's start and the hit (frames over 50 ms are recorded), so a loaded machine no longer fails it. `spell_zoo`
  isolates its victim (`ai_controlled` off), so the shaman no longer casts at the hero.
