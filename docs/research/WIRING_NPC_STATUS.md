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
