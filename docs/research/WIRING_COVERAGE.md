# Wiring coverage: ported original functions vs. the running game

Why this document exists: the user keeps finding behaviours the original clearly has (example: the body
turning toward the aim direction while the bow is drawn) that exist as *ported functions* but that play mode
(`apps/game/src/play/**`, `crates/runtime`) never calls, because ad-hoc glue does the job. This file is the
hard, automated gap list, plus the two static audits that explain *why* each gap exists, plus a prioritized
work list for whoever wires things up. Companion documents:

* `WIRING_GLUE_SCAN.md`: every place in play/runtime that re-implements a rule, mapped to the ported function that should be used (103 rows).
* `WIRING_STATE_CHECKLIST.md`: per original player state (PS_Normal, Sneak, Swim, Evade, Interact*, HUD, Melee*, Ranged*, Magic*, focus/lock-on, hits), every behaviour the original performs and whether our game does it (yes / partial / no, with file:line evidence; 215 rows).

## How the numbers were produced (reproducible)

1. `crates/portcov` (`gothic-portcov`): `gothic_portcov::hit!("<fn>")` registers a static counter on its first call; a
   background thread (and `gothic_portcov::dump()`) writes `{"<file>#<fn>": count}` to `$GOTHIC_PORTCOV_OUT`. The macro
   expands to nothing unless the game is built with the cargo feature `port-coverage`
   (`cargo build --release -p gothic-reforged --features "viewer port-coverage"`), so default builds pay nothing.
   `State`s (the `ZS_*` / `PS_*` / `BS_*` data tables, which cannot carry a macro) are counted by name in
   `routine::work::run_state` (`state:<Registry name>`).
2. `scripts/portcov/instrument.py` inserted the macro mechanically as the first statement of every function whose
   doc comment contains `Port of <Name> (<Dll>:<addr>)` (782 functions; 142 further doc blocks sit on tables, consts and
   structs and are covered by the state counters or are not countable) and writes the static manifest. It is idempotent;
   `--skip-dirty` leaves files with uncommitted work alone (files that were dirty when this was run are listed at the end
   if any were skipped).
3. `scripts/portcov/run_all.sh` ran all 29 QA scenarios (`--play --qa <scenario>`, `docs/research/QA_REPORT.md`) with the
   feature on; `scripts/portcov/report.py` merges the 29 dumps with the manifest and appends the generated tables below.
   Scenario outcome in this run: 27 PASS, `shaman_duel` FAIL (the shaman never casts: "casts 0", a real bug, see P1-8),
   `souls_basic` panics (needs `--combat souls`, which is an opt-in alternative combat mode, not the classic game).

**How to read "never called".** It means no QA scenario executed the function. Three kinds, shown per row:
`DEAD` (no production reference anywhere: nobody can ever call it), `INTERNAL-ONLY` (only referenced inside its own crate;
the entry point that would reach it is never reached from play code), `WIRED-NOT-EXERCISED` (a host crate references it, but
no scenario reaches that call: long-running routines, rare events, or an input the harness never sends). `?` marks names shared
by several functions (the reference count is a plain name match, so treat that status as a hint). The QA scenarios are short
(10 to 100 s of play), so `ZS_*` routine tasks are under-exercised by construction; the *hero* states and the engine-side
calls are the reliable part of the signal, and the static checklist covers the rest.

## Headline

* **Refresh 2026-10-28 (branch day/desktop2 at 491e6bb plus the session cherry-pick, 40 scenarios incl. spell_zoo / crime_theft / focus_steer / sleep_bed; `routine_zoo` timed out, `walk_to_reddock` misses the frame budget under the instrumented build): 798 instrumented ported functions, 461 called, 337 never called** (42% uncalled; 114 DEAD, 159 INTERNAL-ONLY, 64 WIRED-NOT-EXERCISED). Earlier runs: 2026-10-21 262 of 782; 2026-10-27 314 of 797. States entered: **268 of 375** (65 on 2026-10-21, 68 on 2026-10-27). The prose below is the 2026-10-21 analysis and is largely outdated (fixed since: bow Aim_Loop, falls/landings, NPC combat animation and routines, lock-on, focus steering, sleep); the generated tables are current.
* Script states entered at least once (2026-10-21 figure; current: 268 of 375): **65 of 375** (PS_: 24 of 101, ZS_: 41 of 266, BS_: 0 of 8). The hero's `PS_*` set that play really reaches:
  `PS_Normal(+_Loop, _Jump, _Sneak)`, `PS_Interact(+_Loop, _End, _Chest, _Chest_Loop, _Anvil, _SitBench)`, `PS_Melee(+_Loop, _QuickAttack)`,
  `PS_Ranged(+_Walk, _Aim, _PowerAim, _Shoot)`, `PS_Magic(+_Loop, _PowerCast, _Reload, _Summon)`.
  Never entered: `PS_Melee_{Attack,PowerAttack,HackAttack,PierceAttack,SimpleWhirl,WhirlAttack,FinishingAttack,Parade,Jump,JumpBack,Sneak}`,
  `PS_Ranged_{Aim_Loop,Aim_Reload,Reload,Jump,Sneak,CrossBowSneak_Loop,Walk_Loop}`, `PS_Magic_{Cast,QuickCast,FlameSword,StaffParade*,Jump,Sneak,Summon_Abort}`, `PS_MagicPotion`,
  `PS_Swim(+_Loop)`, `PS_Slide_Jump`, `PS_Normal_Take`, `PS_GonnaLoot`, `PS_HUD*`, `PS_QuickUse`, `PS_StopForMenu`, `PS_Ghost*`, `PS_DuskToDawn*`,
  all other `PS_Interact_*` (Door, boulders, bed, tools, ...), `PS_Evade` (`ZS_Evade*`).
* **Runtime evidence for the bow bug**: `PS_Ranged_PowerAim` ran 928 ticks and `PS_Ranged_Aim_Loop` never: the ported bow never leaves PowerAim, because `ps_ranged_power_aim` never hands over to the aim loop.
* By crate the ports that are *fully used* are camera, physics controller, projectile flight, fx, environment, audio sequencers; the ports that are mostly bypassed are:
  `gameplay/inventory/list.rs` (57 of 65 never called), `script/magic/effects.rs` (47 of 48: only the fireball path runs), `script/damage.rs` (23 of 24; the sim has its own
  `combat::assess_hit`/`resolve_hit`, parity-tested in `sim/script_host.rs` tests but not the code that runs), `gameplay/stats.rs` (14 of 14), `script/stats.rs` (14 of 19), `script/assess.rs` (15 of 19),
  `script/speech.rs` (16 of 18), `script/magic/area.rs` (14 of 14), `script/routine/party.rs` (10 of 10), `world/zones.rs` (9 of 9), `gameplay/mover.rs` (6 of 6: doors never move),
  `animation/{attach,visual,body_parts,lookat}.rs` (24 of 24: the whole EMotionFX-wrapper layer; the game plays clips through its own `anim.rs` overlay glue), `gameplay/inventory/{stack,order}.rs`.
* Two **dual implementations** (ported function exists *and* an independent host/sim copy runs): damage (`script::damage::assess_hit` vs `sim::combat::assess_hit`), hero facing (`physics` turn model is used, the script's facing decisions are not).

## Prioritized fix list (what a player notices first)

Evidence ids refer to rows in the two companion documents (A1 = glue scan row A1; "SC" = state checklist). "Port" = the ported function to use; "no port" = a decompile pass is needed first (do not guess).

| # | What the player sees | Cause | Fix |
| --- | --- | --- | --- |
| P0-1 | **Hero does not turn to the aim direction while drawing the bow / casting** (turns toward a focused NPC instead, +90 degree model hack, legs stand still) | In the original no `PS_Ranged*` state turns the body: `PS_Ranged`/`PS_Magic_Loop` set the movement constraint to 0 (`sub_20b0`), then `gCCharacterControl_PS::ProcessMovements` (Game:0xddd40) aligns the body to the camera forward every frame via `SetAlignmentLookAt`, and `ProcessControledRotationLookAt` (Game:0xe4060) rotates it at TurnSpeedMax 200 deg/s; melee/cast states set the blocked constraint instead. Nothing of this is ported (`ScriptHost::set_look_at` is an empty stub); `hero.rs:150-182 face_target` + `hero.rs:1026 aim_blend` are glue (A1, A17, SC combat section 0) | Port the look-at rule as a pure fn next to `physics::step_turn` (inputs: constraint, camera forward, entity azimuth, free-fly flag); add `set_movement_constraints`/`set_alignment_target` to `ScriptHost` and call them from `ps_ranged`, `ps_magic_loop`, `ps_magic_cast`, the melee states; feed `MoveCommand.face` and `LocomotionInput.turn_error` (turn-in-place clips) from it; delete the 3-branch `face_target` for ranged/magic and the `aim_blend` quarter turn. Needs a short REA pass on the in-between constraint values (marked [I] in SC) |
| P0-2 | **Melee feels late and shallow**: hit lands at click time (no 0.1 to 0.8 s hit window, no reach check, no whiff), swing queued on button release, no combos (Whirl, Hack, Finishing, SimpleWhirl, jump/sprint attack), no guard pose, JumpBack plays the evade slide, animation speed always 1.0 | `script_player.rs:545-552` `melee_hit` immediately; `hero.rs:567-600` only Quick/Power/Pierce; `PS_Melee_{Parade,JumpBack}` never entered; S:42a0 speed multipliers not ported (C1-C5, SC melee) | Route the hero through the NPC strike path (`combat::commands::hit_window_secs` + `sim::ai_exec::begin_swing`/`resolve_strike`), start the swing on press, port the chain logic of `OnPlayerAction` (`player_interface/OnPlayerAction.c`) into `input::player`, call `ps_melee_parade` / `ps_melee_jump_back`, apply the S:42a0 anim rate table in `anim.rs` |
| P0-3 | **No target lock-on**; focus is a text label only, nothing is highlighted in a fight | `SessionKey::Lock` unread; `gameplay::focus::lock_holds` has no caller (A5, SC section 4) | Wire Lock key to the focus state; use `lock_holds` in `focus()`; steer focus by movement keys in combat (`gCFocus_PS::OnAction` cases 1-4: no port yet) |
| P0-4 | **No fall damage, no sprint, no swimming states, no dive, sliding has no animation** | `walk.rs:1363` discards `StepReport` (landed / took_off / started_sliding); `drain_mode_changes()` has no non-test caller, so `OnMovementModeChanged` (S:adbf0: `land_from_fall` DEAD, `Sim::land`, `ZS_Fall/Land/HeavyLand/Slide/DiveIn`, `PS_Swim`) never runs; `second_tick` uses a default `TickEnv` so the sprint/interaction stamina variants never apply (A12, SC section 1, 3) | Feed `StepReport` + mode changes into the core (new `Command::HeroMovement`), call `Sim::land` / `land_from_fall`, start `PS_Swim`/`ZS_DiveIn`, add Sprint to `hero.rs:363` (needs REA: what selects mode 5), pass the real `TickEnv` |
| P0-5 | **Bow behaves wrongly after the first shot / jump** | `ps_ranged_power_aim` never hands over to `PS_Ranged_Aim_Loop` (runtime evidence above); `normal.rs:218 back_to_loop` sends a ranged hero to `PsRangedAimLoop` after any jump/sneak (crosshair + aim clips appear); `msg::RELOAD` has no arm in `hero.rs:489-545`; `PS_Ranged_Aim_Reload` (re-nock) conflated with `PS_Ranged_Reload`; abort does not un-hold the ammunition; `ps_ranged_crossbow_sneak` unreachable (SC combat section 2) | Fix the three state hand-overs in `script/player/ranged.rs`/`normal.rs`, add the RELOAD arm, make `input::player::{ranged_walk_loop,ranged_aim_loop,power_aim_loop}` (all DEAD) the router for those states |
| P0-6 | **Sky and weather run on a different clock from the NPCs**: sleeping or waiting never moves the sun; routines and music follow the sim clock | `OrigEnv.day_time` constant (speed 0 unless `--day-speed`) vs `core.sim.clock` (J1, E9); `PlayerEvent::TimeScale` ignored | One world clock feeding `environment::WeatherAdmin`; implement `PS_DuskToDawn` presentation (fade + time scale 0.3) |
| P1-1 | **Doors, gates and movers never move** (only a toast) | `gameplay::mover::*` and `set_door_status` have no caller (F3, `PS_Interact_Door` never entered) | Register `gCDoor_PS`/`gCMover_PS` entities as `Mover`s, drive `on_process`, collision follows |
| P1-2 | **Hero never walks to / aligns with the use point; no tools in hands; no take/drink/hold animations** (hero `Event::Anim` and the `AiCommand` stream of the ported states are dropped) | `script_player.rs:564` drops hero `Goto*`; `HoldInventoryStacks` is 400 ms of busy time only; `PS_Normal_Take`, `PS_GonnaLoot`, `PS_HUD*`, `PS_QuickUse` never entered (F1, F2, F6, E4, SC section 5/6) | Play the hero's `AiCommand` stream like `routines.rs` does for NPCs, implement `_AI_Goto_Ext` for the hero (no port: use-point alignment needs REA), attach tool props via `animation::attach` (DEAD) |
| P1-3 | **NPC combat animation is late and uniform**: swing clip starts at the damage moment, every hit plays QuickStumble regardless of `HitResult`, no parry pose, no wind-up for NPC archers/casters | `events.rs:344-363` (C6, C7, D5, G4): `routines.rs` shows only 4 of ~20 `AiCommand`s | Emit a "swing started" event from `begin_swing`; choose stumble/knockdown clips from `Hit.result` via `combat::commands::start_stumble/start_lie_knock_down/start_get_up`; play `AiCommand::{Attack,Parade,Shoot,Cast,Stumble,Turn*}` |
| P1-4 | **NPCs run too fast** (flat 550 cm/s, play sprint clips) and see through walls | `sim.rs:40-41 RUN_SPEED/WALK_SPEED`, `MovementProfile::human_npc()` unused (G1); `PlayWorld::line_of_sight` always true (G6) | Use `physics::profile` (run 400, walk 150, sneak 280) and species/template values; hand the collision world to `PlayWorld` for LOS and `is_outdoors` |
| P1-5 | **Spells other than the fireball do nothing visible**: summons show a toast, status effects (freeze, burn, sleep), animal forms, area spells, magic parade (staff), flame sword | `script/magic/effects.rs` 47 of 48 and `magic/area.rs` 14 of 14 never called; `MagicEvent::{Summoned,Status,Transform,Area}` fall through `_ => {}` in `fx.rs:294`; `PS_Magic_StaffParade`, `PS_MagicPotion`, `PS_Magic_FlameSword` unreachable (E5, SC magic) | Present `MagicEvent`s (spawn summon bodies, status visuals); map `msg::PARADE` with a staff to `PS_Magic_StaffParade` |
| P1-6 | **Shaman never casts** (QA `shaman_duel` FAIL: casts 0, mana unspent) | not investigated here; NPC cast path (`ZS_Attack_Cast`, `AiCommand::Cast`) | Debug the QA failure first, it is an existing regression |
| P2-1 | No head look-at (NPCs and hero never turn their heads), no idle variation table, whole-body reactions (stumble, knockdown, evade) play as upper-body overlays | `animation::{lookat,visual,process,animator::play_slot}` DEAD; `anim.rs:73-78` (H1, H5, H6) | Replace `anim.rs` overlay glue by `VisualAnimation::set_motion` + `process_plan`; drive `LookAtSet` from focus / dialog partner |
| P2-2 | Camera: no tremor (quakes, heavy hits, falls), `ge3.INI` camera config not read, first-person toggle unconditional, no inventory camera mode | `CameraState::add_tremor` unused, `CameraConfig::from_ini` unused, `first_person_camera_allowed` DEAD (B4, B6, B7) | wire |
| P2-3 | Boulder carrying, `CarryStone`, grindstone/fry/bookstand pages, pickpocketing, sleeping-NPC robbery, quick-loot key | `Sim::set_boulder` never called; `BS_*` 0 of 8 entered; `Confirm::Rob` toast (F4, F5, SC section 5) | register interact classes, implement pages |
| P2-4 | Footsteps always earth, no land/jump sounds, ambient always forest, music location fixed | `audio.rs:61,97-182` (J2, J3, J5) | feed floor material / zone entities into the ported sequencers |
| P2-5 | Hero stamina/regen, perks and consumables use host re-implementations (`quick_use`, `potion_of` name heuristics) | no port of `OnPlayerUseQuickSlot`, `OnPlayerSelectWeapon`, `OnPlayerPotionQuickUse` (E2, E3) | REA pass, then port (listed under "no port") |

Rules with **no ported function yet** (need a decompile pass before any "fix" can be called a port): hero facing / `ProcessMovements` look-at and movement constraints, `OnPlayerSelectWeapon`, `OnPlayerUseQuickSlot`,
`OnPlayerPotionQuickUse`, use-point alignment of `PS_Interact`, hit-type to animation selection, lake/river water volumes, music trigger rule for fights, `gCFocus_PS::OnAction` target steering, what starts the sprint gait.

Process recommendation: add a `required_hits` list per QA scenario (e.g. `combat_basic` must hit `ps_melee_parade` once a parry exists, `magic_bow` must enter `PS_Ranged_Aim_Loop`) and make `scripts/portcov/run_all.sh` + `report.py` part of the QA run, so a ported function that stays uncalled is a visible regression instead of something the user finds by playing.

---

# Generated coverage tables

Generated by `scripts/portcov/` (crate `gothic-portcov`, cargo feature `port-coverage` of `gothic-reforged`). `scripts/portcov/instrument.py` inserts `gothic_portcov::hit!("<fn>")` at the top of every function whose doc comment says `Port of <Name> (<Dll>:<addr>)`; the macro expands to nothing without the feature. Each QA scenario was run as `gothic-reforged --play --qa <scenario>` with the feature on, and the call counts of all runs were merged (`scripts/portcov/run_all.sh`, `scripts/portcov/report.py`). **Called** means executed at least once by the running game in any scenario (unit tests do not count; a function reached only from another never-used function counts as never called only if it is itself never executed). Scenarios run: `bow_faithful`, `bow_pose`, `combat_basic`, `controls_basic`, `craft_anvil`, `crime_theft`, `dialog_quest`, `doors`, `focus_steer`, `hud_focus`, `lockpick`, `loot_chest`, `magic_faithful`, `melee_faithful`, `menu_save_load`, `npc_archer`, `npc_orc1`, `npc_vs_npc`, `perf_idle`, `quick_slots`, `routine_reentry`, `routine_zoo`, `sector_switch`, `shaman_duel`, `sit_pose`, `sleep_bed`, `spell_zoo`, `startup_sanity`, `swim`, `teach`, `teleport`, `trade_click`, `trade_esc`, `triggers`, `ui_screens`, `walk_to_reddock`, `world_chest`, `world_doors`, `world_routines`, `world_walk`.

## Totals

* Doc blocks with `Port of`: 940; instrumentable functions: 798 (+0 not instrumented, see the end); other items (tables, consts, structs, impls; not countable): 142.
* **Called in play: 461**, **never called: 337** (42% of instrumented functions).
* Of the never-called: **114 DEAD** (no production reference anywhere: nobody could call them), **159 internal-only** (referenced only inside their own crate, entry point never reached), **64 wired-not-exercised** (referenced from another crate, but no QA scenario reaches the call; long-running routines, rare events). A trailing "?" marks names shared by several functions (reference count is a name match, so the status is a heuristic).
* Script states (ZS_/PS_/BS_, data tables interpreted by `run_state`): 268 of 375 entered at least once.

## By subsystem

| Subsystem / original state family | called | never called |
| --- | ---: | ---: |
| gameplay (inventory, trade, mover, treasure, focus, stats) | 62 | 101 |
| world (entity admin, PVS, sectors, triggers, zones) | 20 | 44 |
| animation (EMotionFX wrapper, blending, attachments) | 13 | 35 |
| script helpers / predicates / stats | 45 | 34 |
| ZS_* routine tasks | 18 | 24 |
| spell effects / casting | 51 | 19 |
| ZS_Attack / combat AI | 61 | 14 |
| ui | 14 | 11 |
| PS_Magic* | 9 | 8 |
| damage pipeline | 36 | 7 |
| audio sequencers | 20 | 6 |
| render rules | 1 | 6 |
| perception / crime | 16 | 6 |
| PS_Interact boulder | 0 | 4 |
| PS_Interact* | 11 | 3 |
| PS_Normal / Sneak / Swim | 4 | 3 |
| PS_Ranged* (bow, crossbow) | 11 | 3 |
| fx / particles | 17 | 2 |
| down / knock-out | 7 | 2 |
| PS_Interact craft | 1 | 2 |
| nav | 0 | 1 |
| PS_Melee* | 3 | 1 |
| sim (stats, combat, AI, host) | 5 | 1 |
| camera (PSCamera) | 1 | 0 |
| environment / weather | 7 | 0 |
| formats | 3 | 0 |
| input / player control | 6 | 0 |
| physics / movement | 1 | 0 |
| projectiles | 9 | 0 |
| runtime | 3 | 0 |
| PS_Evade | 2 | 0 |
| PS_* other (player api / commands) | 3 | 0 |
| PS_HUD use item | 1 | 0 |

## Script states never entered in play

**PS_** (62 of 101): `PS_DuskToDawn_Loop`, `PS_GhostSummoner`, `PS_GhostSummoner_Loop`, `PS_Ghost_Jump`, `PS_GonnaLoot`, `PS_HUD`, `PS_HUD_Equip`, `PS_HUD_Loop`, `PS_HUD_UnEquip`, `PS_HUD_Use`, `PS_Interact_Alchemy`, `PS_Interact_Barbecue`, `PS_Interact_Bookstand`, `PS_Interact_Campfire`, `PS_Interact_CarryStone`, `PS_Interact_CarryStone_End`, `PS_Interact_CarryStone_Loop`, `PS_Interact_Cauldron`, `PS_Interact_DrinkWaterBarrel`, `PS_Interact_Forge`, `PS_Interact_GrindStone`, `PS_Interact_HoldOrcBoulder`, `PS_Interact_HoldOrcBoulder_Loop`, `PS_Interact_LiftOrcBoulder`, `PS_Interact_LiftOrcBoulder_Loop`, `PS_Interact_OrcDrum`, `PS_Interact_PickOre`, `PS_Interact_PirateTreasure`, `PS_Interact_SawLog`, `PS_Interact_Shrine`, `PS_Interact_SitStool`, `PS_Interact_SitThrone`, `PS_Interact_SleepGround`, `PS_Interact_Stoneplate`, `PS_Interact_Talk`, `PS_Interact_TiltOrcBoulder`, `PS_Interact_TiltOrcBoulder_Loop`, `PS_Interact_UseWaterpipe`, `PS_Interact_WaitForExit`, `PS_Magic_Jump`, `PS_Magic_QuickCast`, `PS_Magic_Sneak`, `PS_Magic_StaffParade`, `PS_Magic_StaffParade_Loop`, `PS_Melee_FinishingAttack`, `PS_Melee_HackAttack`, `PS_Melee_Jump`, `PS_Melee_JumpBack`, `PS_Melee_QuickAttack`, `PS_Melee_SimpleWhirl`, `PS_Melee_Sneak`, `PS_Melee_WhirlAttack`, `PS_Normal_Jump`, `PS_Normal_Sneak`, `PS_QuickUse`, `PS_Ranged_CrossBowSneak_Loop`, `PS_Ranged_Jump`, `PS_Ranged_Sneak`, `PS_Slide_Jump`, `PS_StopForMenu`, `PS_Swim`, `PS_Swim_Loop`

**ZS_** (37 of 266): `ZS_Dead`, `ZS_Dead_Loop`, `ZS_PiercedDead`, `ZS_RagDollDead`, `ZS_RagDollDead_Loop`, `ZS_Study_Bookshelf`, `ZS_Study_Bookstand`, `ZS_Stumble`, `ZS_Stunned`, `ZS_Stunned_End`, `ZS_Stunned_Loop`, `ZS_Talk`, `ZS_Talk_Loop`, `ZS_Timebubble`, `ZS_Timebubble_Loop`, `ZS_Trader`, `ZS_Trader_Bookshelf`, `ZS_Trader_Bookstand`, `ZS_Trader_Chest`, `ZS_Trader_Stand`, `ZS_Trader_SweepFloor`, `ZS_TravelToDestination`, `ZS_TravelToDestination_Loop`, `ZS_Unconscious`, `ZS_Unconscious_End`, `ZS_Unconscious_Loop`, `ZS_WaitForDawn`, `ZS_WaitForFreeDestination`, `ZS_WaitForFreeDestination_Loop`, `ZS_WaitForMeteor`, `ZS_WaitForPlayer`, `ZS_WaitForPlayer_Loop`, `ZS_WatchFight_Spectator`, `ZS_WatchFight_Spectator_Loop`, `ZS_WatchFight_WaitForFlee`, `ZS_WatchFight_WaitForFlee_Loop`, `ZS_Woodwork_Repair`

**BS_** (8 of 8): `BS_Alchemy`, `BS_AncientWisdom`, `BS_BookOfFlood`, `BS_BookOfRhobar`, `BS_BookOfZuben`, `BS_Chronicles`, `BS_NothingNew`, `BS_Smith`

Entered: `PS_DuskToDawn` (3), `PS_Ghost` (4), `PS_Ghost_Loop` (6), `PS_Interact` (14), `PS_Interact_Anvil` (3), `PS_Interact_Chest` (8), `PS_Interact_Chest_Loop` (49), `PS_Interact_Door` (9), `PS_Interact_End` (20), `PS_Interact_Loop` (37), `PS_Interact_SitBench` (3), `PS_Interact_SleepBed` (2), `PS_Magic` (156), `PS_MagicPotion` (33), `PS_Magic_Cast` (8), `PS_Magic_FlameSword` (3), `PS_Magic_Loop` (1571), `PS_Magic_PowerCast` (4), `PS_Magic_Reload` (94), `PS_Magic_Summon` (89), `PS_Magic_Summon_Abort` (1), `PS_Melee` (8), `PS_Melee_Attack` (34), `PS_Melee_Loop` (566), `PS_Melee_Parade` (4), `PS_Melee_PierceAttack` (1), `PS_Melee_PowerAttack` (2), `PS_Normal` (54), `PS_Normal_Loop` (11581), `PS_Normal_Take` (2), `PS_Ranged` (6), `PS_Ranged_Aim` (10), `PS_Ranged_Aim_Loop` (218), `PS_Ranged_Aim_Reload` (20), `PS_Ranged_PowerAim` (164), `PS_Ranged_Reload` (3), `PS_Ranged_Shoot` (30), `PS_Ranged_Walk` (8), `PS_Ranged_Walk_Loop` (113), `ZS_Acrobat` (60), `ZS_Alchemy` (1), `ZS_Alchemy_Alchemy` (3), `ZS_Alchemy_Bookstand` (3), `ZS_Alchemy_Cauldron` (3), `ZS_Ambient` (14), `ZS_Ambient_Loop` (1), `ZS_Ambient_Sleep` (2), `ZS_Ambient_Sleep_Loop` (2), `ZS_AnimalSleep` (3), `ZS_AnimalSleep_SleepGround` (61), `ZS_Applaud` (3), `ZS_Arena` (1), `ZS_Arena_Loop` (60), `ZS_Attack` (8168), `ZS_Attack_Arena_End` (1), `ZS_Attack_Attack` (166), `ZS_Attack_Cast` (183), `ZS_Attack_ChangePosition` (1), `ZS_Attack_DrinkPotion` (2), `ZS_Attack_End` (169), `ZS_Attack_FinishingAttack` (2), `ZS_Attack_GoBack` (41), `ZS_Attack_Goto` (312), `ZS_Attack_HackAttack` (111), `ZS_Attack_JumpBack` (2), `ZS_Attack_Loop` (9644), `ZS_Attack_Parade` (26), `ZS_Attack_PierceAttack` (30), `ZS_Attack_PowerAttack` (129), `ZS_Attack_QuickAttack` (103), `ZS_Attack_Shoot` (636), `ZS_Attack_SimpleWhirl` (2), `ZS_Attack_Sprint` (522), `ZS_Attack_StrafeLeft` (1), `ZS_Attack_StrafeRight` (1), `ZS_Attack_TurnLeft` (766), `ZS_Attack_TurnRight` (118), `ZS_Attack_Wait` (1320), `ZS_Attack_WaitForDialog` (83), `ZS_Attack_WaitForFlee` (36), `ZS_Attack_WaitForFlee_Loop` (4277), `ZS_Attack_WaitOnShore` (31), `ZS_Attack_WhirlAttack` (100), `ZS_Barbecue` (1), `ZS_Bard` (5), `ZS_Boss` (25), `ZS_Boss_Pace` (45), `ZS_Boss_Stand` (9), `ZS_Boss_Throne` (4), `ZS_Break` (2), `ZS_Break_Bench` (2), `ZS_Break_Chest` (3), `ZS_Break_Pee` (3), `ZS_Break_SitGround` (2), `ZS_Break_Stand` (3), `ZS_Break_Stool` (2), `ZS_Break_WaterBarrel` (3), `ZS_CarryStones` (2), `ZS_Cook` (1), `ZS_Deadlock` (3), `ZS_Decay` (1), `ZS_Digger` (1), `ZS_Digger_DigGround` (3), `ZS_Digger_PickGround` (3), `ZS_Digger_PickOre` (3), `ZS_Digger_WaterBarrel` (3), `ZS_DiveIn` (60), `ZS_EatBody` (3), `ZS_EatBody_Loop` (1), `ZS_EvadeBackward` (2), `ZS_EvadeForward` (2), `ZS_EvadeLeft` (2), `ZS_EvadeLoop` (6), `ZS_EvadeRight` (2), `ZS_Fall` (60), `ZS_Farmer` (1), `ZS_Farmer_FieldWork` (3), `ZS_Farmer_RakeField` (3), `ZS_FetchWeapon` (2), `ZS_Firerain` (60), `ZS_Firerain_Loop` (60), `ZS_Flee` (5), `ZS_Flee_GoBack` (6), `ZS_Flee_GotoFleePoint` (2), `ZS_Flee_ObserveFollower` (77), `ZS_FollowNPC` (3), `ZS_FollowNPC_Loop` (62), `ZS_FollowPlayer` (3), `ZS_FollowPlayer_Loop` (62), `ZS_Freeze` (1), `ZS_Freeze_Loop` (2), `ZS_FrostWave_Loop` (61), `ZS_Frostwave` (60), `ZS_Goblin` (1), `ZS_Goblin_Banana` (61), `ZS_Goblin_FryMeat` (3), `ZS_Goblin_SitGround` (3), `ZS_Goblin_Smalltalk` (61), `ZS_GotoPlayerZone` (2), `ZS_Grind` (1), `ZS_Guard` (11), `ZS_Guard_Loop` (12), `ZS_GuidePlayer` (2), `ZS_GuidePlayer_Loop` (60), `ZS_GuidePlayer_Wait` (1), `ZS_Hailstorm` (61), `ZS_Hailstorm_FadeIn` (60), `ZS_Hailstorm_FadeIn_Loop` (61), `ZS_Hailstorm_FadeOut_Loop` (60), `ZS_Hailstorm_Loop` (60), `ZS_HangAround` (2301), `ZS_HangAround_Bench` (295), `ZS_HangAround_DoNothing` (210), `ZS_HangAround_FryMeat` (18), `ZS_HangAround_Pee` (3), `ZS_HangAround_SitGround` (3355), `ZS_HangAround_Stand` (115), `ZS_HangAround_Stool` (221), `ZS_HangAround_Waterpipe` (3), `ZS_HealSelf` (3), `ZS_HealSelf_Cast` (2), `ZS_HeatWave_Loop` (61), `ZS_Heatwave` (60), `ZS_HeavyLand` (60), `ZS_HeavyParadeStumble` (60), `ZS_Herd` (1), `ZS_Herd_DrinkWater` (1), `ZS_Herd_EatGround` (2), `ZS_IceBomb` (60), `ZS_IceBomb_Loop` (60), `ZS_Idiot` (17), `ZS_Land` (60), `ZS_Lee_Teleport` (4), `ZS_LieKnockDown` (61), `ZS_Light` (60), `ZS_Light_Loop` (60), `ZS_Lightning` (61), `ZS_Lightning_Loop` (60), `ZS_Listen` (1), `ZS_Listen_Loop` (2), `ZS_Lumberjack` (1), `ZS_MagicHypnosis` (3), `ZS_MagicHypnosis_Loop` (18), `ZS_MagicSleep` (6), `ZS_MagicSleep_Loop` (27), `ZS_Meteor` (61), `ZS_Meteor_Loop` (60), `ZS_Mist` (61), `ZS_Mist_End` (60), `ZS_Mist_Loop` (60), `ZS_Mist_NoAggro` (61), `ZS_Mist_NoAggro_Loop` (60), `ZS_Misted` (2), `ZS_Misted_Loop` (2), `ZS_Monster` (15), `ZS_Monster_Roam` (11), `ZS_NavEval` (60), `ZS_NavEval_Loop` (61), `ZS_ObserveSuspect` (23), `ZS_ObserveSuspect_Loop` (791), `ZS_ObserveWeapon` (3), `ZS_ObserveZonePlayer` (1), `ZS_OrcParty` (1), `ZS_OrcParty_OrcBoulder` (5), `ZS_OrcParty_OrcDance` (3), `ZS_OrcParty_OrcDrum` (3), `ZS_OrcParty_Spectator` (3), `ZS_ParadeStumble` (60), `ZS_PierceStumble` (61), `ZS_PiercedKO` (60), `ZS_Pray` (1), `ZS_Pray_Shrine` (3), `ZS_Preach` (1), `ZS_Preach_AutoPreach` (3), `ZS_Preach_SVM` (13), `ZS_QuickStumble` (60), `ZS_RansackBody` (4), `ZS_ReactToDamage` (2), `ZS_ReactToDamage_End` (2), `ZS_ReactToDamage_GoBack` (48), `ZS_ReactToDamage_Loop` (120), `ZS_ReactToFootsteps` (2), `ZS_ReactToSneak` (3), `ZS_ReactToWeapon` (27), `ZS_ReactToWeapon_End` (24), `ZS_ReactToWeapon_WaitForAttack` (307), `ZS_ReactToWeapon_WaitForSecondWarn` (358), `ZS_ReactToZone` (3), `ZS_ReactToZone_Loop` (97), `ZS_RevolutionAttack` (3), `ZS_RevolutionAttack_Loop` (119), `ZS_RevolutionSuccess` (3), `ZS_RunToAttackPoint` (2), `ZS_SitKnockDown` (60), `ZS_SitKnockDown_GetUpAttack` (61), `ZS_SitKnockDown_GetUpParade` (60), `ZS_SitKnockDown_Loop` (60), `ZS_SlaveBabe` (1), `ZS_SlaveBabe_CleanFloor` (3), `ZS_SlaveBabe_Dance` (3), `ZS_SlaveBabe_FanBoss` (3), `ZS_SlaveBabe_SweepFloor` (3), `ZS_Sleep` (2), `ZS_Sleep_Bed` (3), `ZS_Sleep_Ground` (3), `ZS_Sleep_Loop` (147), `ZS_Slide` (60), `ZS_Slide_Loop` (60), `ZS_Smalltalk` (8), `ZS_Smalltalk_Leader` (15), `ZS_Smalltalk_Leader_Loop` (20), `ZS_Smalltalk_Partner` (12), `ZS_Smalltalk_Partner_Loop` (1411), `ZS_Smith` (2), `ZS_Smith_Anvil` (3), `ZS_Smith_Cool` (3), `ZS_Smith_Forge` (3), `ZS_Smith_GrindStone` (3), `ZS_StandUp` (2), `ZS_Study` (3), `ZS_Threaten` (42), `ZS_Threaten_Loop` (14), `ZS_WatchFight` (15), `ZS_WatchFight_GoBack` (10), `ZS_WatchFight_Goto` (5), `ZS_WatchFight_Loop` (3593), `ZS_Woodwork` (5), `ZS_Woodwork_SawLog` (6)

## Ported functions never called in play (by subsystem)

### gameplay (inventory, trade, mover, treasure, focus, stats) (101)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `mod_value` (gCItem_PS) | Game:0x100b9d20 | value of the first modifier whose tag equals tag, 0 when there is none. | INTERNAL-ONLY | crates/gameplay/src/inventory/item.rs:187 |
| `pick_up` (the pickup path) | - | a pickup entity's amount and quality are added to the inventory. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/inventory/list.rs:492 |
| `find_stack_index_by_use_type` (the stack list's FindStackIndex by use type) | Game:0x100c39b0 | the stack list's FindStackIndex by use type (Game:0x100c39b0). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:559 |
| `find_stack_index_by_slot` (gCInventory_PS) | - | index of the stack linked in slot. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:614 |
| `find_stack` (gCInventory_PS) | - | gCInventory_PS::FindStack(gESlot). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:623 |
| `find_stack_index_by_hot_key` (gCInventory_PS) | Game:0x100b5900 | quick slot lookup. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:629 |
| `stack_use_type` (gCInventory_PS) | Game:0x100b5a90 | 0 for an invalid index. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/inventory/list.rs:638 |
| `stack_category` (gCInventory_PS) | Game:0x100b5a70 | gCInventory_PS::GetStackCategory (Game:0x100b5a70). | WIRED-NOT-EXERCISED ? | crates/gameplay/src/inventory/list.rs:644 |
| `set_stack_amount` (gCInventory_PS) | Game:0x100b5b40 | Amount 0 disposes the stack (only if it is unlinked). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:652 |
| `set_stack_quality` (gCInventory_PS) | Game:0x100b5ab0 | gCInventory_PS::SetStackQuality (Game:0x100b5ab0). | DEAD | crates/gameplay/src/inventory/list.rs:667 |
| `dispose_stack` (the stack list's DisposeStack) | Game:0x100c3c20 | delete only unlinked stacks. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:688 |
| `remove_items` (the stack list's DisposeItems) | Game:0x100c38e0 | remove n items. Permanent items (skills, spells) are never reduced but the call succeeds; removing the whole stack needs it to be unlinked. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:714 |
| `merge_stacks` (gCInventory_PS) | - | gCInventory_PS::MergeStacks: merge stack from into into. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:736 |
| `split_stack` (gCInventory_PS) | - | gCInventory_PS::SplitStack: cut n items into a new stack. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:751 |
| `set_default_item` (gCInventory_PS) | Game:0x100b5480 | declare the default item of a slot (template inventories only). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:772 |
| `default_item` (gCInventory_PS) | Game:0x100b5420 | template name or empty. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:790 |
| `link_default_item_to_slot` (gCInventory_PS) | Game:0x100b5620 | spawn the slot's default item when no stack occupies the slot. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:816 |
| `unlink_default_item_from_slot` (gCInventory_PS) | Game:0x100b5660 | remove the slot's default item unless a stack occupies the slot. True when there was nothing to do. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:826 |
| `link_default_items` (gCInventory_PS) | Game:0x100b5530 | for slots 1..=18 without a stack, spawn the default item. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:845 |
| `unlink_default_items` (gCInventory_PS) | Game:0x100b55e0 | gCInventory_PS::UnlinkDefaultItems (Game:0x100b55e0). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:857 |
| `patch_from_template` (gCInventory_PS) | Game:0x100b56d0 | as run by OnChildrenAvailable for an entity instance: take the default items and treasure sets of the creating template, then link the defaults. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:873 |
| `unlink_stack_at` (gCInventorySlot) | Game:0x100bd570 | on stack idx. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:899 |
| `unlink_stack_from_slot` (gCInventory_PS) | Game:0x100b51f0 | detach whatever stack sits in slot, then give the slot its default item back. Slot None does nothing. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:914 |
| `link_stack_to_slot` (gCInventory_PS) | Game:0x100b5180, Game:0x100bd410 | together with the stack list's LinkStack (Game:0x100c3f80) and gCInventorySlot::LinkToSlot (Game:0x100bd410). A stack that already sits in another slo... | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:929 |
| `equip_stack_to_slot` (gCInventory_PS) | Game:0x100b80e0 | put the stack into a specific slot, freeing the slot and the stack's old slot first. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:975 |
| `equip_stack` (gCInventory_PS) | Game:0x100b8160 | equip an item into the carry slot its use type implies. A second one-hander (or one when the right hand holds a one-hander) takes the alternative slot... | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:994 |
| `unequip_stack` (gCInventory_PS) | Game:0x100b8310 | only stacks in their carry slot can be unequipped. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/inventory/list.rs:1038 |
| `right_equip_slot` (gCInventory_PS) | Game:0x100b85d0 | where the right-hand weapon returns to. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1053 |
| `left_equip_slot` (gCInventory_PS) | Game:0x100b84c0 | where the left-hand item returns to. A one-hander returns to BackLeft only if the right hand or the back right slot holds a one-hander too. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1068 |
| `hold_stack_right_hand` (gCInventory_PS) | Game:0x100b8430 | return the current right-hand item to its carry slot, then draw stack idx (-1: draw nothing, the slot falls back to its default item). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1089 |
| `hold_stack_left_hand` (gCInventory_PS) | Game:0x100b8390 | gCInventory_PS::HoldStackLeftHand (Game:0x100b8390). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1107 |
| `hold_stacks` (gCInventory_PS) | Game:0x100b8370 | hold right and left, the right hand first. The original function declares its parameters as (left, right); the script layer calls this with the order ... | WIRED-NOT-EXERCISED ? | crates/gameplay/src/inventory/list.rs:1126 |
| `drop_stack` (gCInventoryStack) | Game:0x100b4460, Game:0x100c06a0 | plus gCInventory_PS::DropStack (Game:0x100b4460): unlink the stack and delete it, reporting what to spawn in the world. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1143 |
| `drop_slot` (gCInventory_PS) | Game:0x100b4530 | gCInventory_PS::DropSlot (Game:0x100b4530). | WIRED-NOT-EXERCISED ? | crates/gameplay/src/inventory/list.rs:1169 |
| `drop_items_from_stack` (gCInventory_PS) | Game:0x100b4580 | drop n items of a stack (the stack is unlinked from its slot). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1176 |
| `drop_items_from_slot` (gCInventory_PS) | Game:0x100b4630 | gCInventory_PS::DropItemsFromSlot (Game:0x100b4630). | DEAD | crates/gameplay/src/inventory/list.rs:1202 |
| `transfer_stack_to` (gCInventory_PS) | Game:0x100b6de0 | move the whole stack. | WIRED-NOT-EXERCISED | crates/gameplay/src/inventory/list.rs:1267 |
| `transfer_all_to` (gCInventory_PS) | Game:0x100b6d70 | move every stack; true when all moved. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1280 |
| `transaction_amount` (gCInventory_PS) | Game:0x100b70a0 | gCInventory_PS::GetTransactionAmount (Game:0x100b70a0). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1313 |
| `perform_transaction` (gCInventory_PS) | Game:0x100b7130 | both sides hand over their marked items. self_gets is the stack type of the items this side receives and other_gets of the items the other side receiv... | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1350 |
| `is_armor_set_equipped` (gCInventory_PS) | Game:0x100b7ca0 | (Game:0x100b7ca0): all six pieces of the set sit in slots. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1395 |
| `is_armor_piece_equipped` (gCInventory_PS) | Game:0x100b7d40 | gCInventory_PS::IsArmorPieceEquipped (Game:0x100b7d40). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1404 |
| `apply_stats` (gCInventorySlot) | Game:0x100be5f0 | A complete armor set grants its bonus and skills; the item's own modifiers are halved while the stack is damaged; an undamaged item also activates its... | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1412 |
| `unapply_stats` (gCInventorySlot) | Game:0x100be920 | gCInventorySlot::OnUnApplyStats (Game:0x100be920). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1439 |
| `find_skill_stack_index` (gCInventory_PS) | Game:0x100b6870 | the permanent stack whose item teaches skill. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1464 |
| `find_spell_stack_index` (gCInventory_PS) | Game:0x100b6940 | gCInventory_PS::FindSpellStackIndex (Game:0x100b6940). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1476 |
| `activate_skill` (gCInventory_PS) | Game:0x100b67b0 | gCInventory_PS::ActivateSkill (Game:0x100b67b0). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1487 |
| `deactivate_skill` (gCInventory_PS) | Game:0x100b6810 | gCInventory_PS::DeactivateSkill (Game:0x100b6810). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1500 |
| `is_skill_active` (gCInventory_PS) | Game:0x100b5f80 | learned or kept active by equipment. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/inventory/list.rs:1513 |
| `skill_activation_count` (gCInventory_PS) | Game:0x100b6760 | gCInventory_PS::GetSkillActivationCount (Game:0x100b6760). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1522 |
| `has_learned_skill` (gCInventory_PS) | Game:0x100b5fc0 | gCInventory_PS::HasLearnedSkill (Game:0x100b5fc0). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1529 |
| `has_learned_spell` (gCInventory_PS) | Game:0x100b68f0 | gCInventory_PS::HasLearnedSpell (Game:0x100b68f0). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1536 |
| `learn_stack` (gCInventory_PS) | Game:0x100b5ef0 | mark a skill/spell stack learned. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1543 |
| `learn_skill` (gCInventory_PS) | Game:0x100b6790 | gCInventory_PS::LearnSkill (Game:0x100b6790). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1556 |
| `learn_spell` (gCInventory_PS) | Game:0x100b6920 | gCInventory_PS::LearnSpell (Game:0x100b6920). | DEAD | crates/gameplay/src/inventory/list.rs:1563 |
| `can_learn_skill` (gCInventory_PS) | Game:0x100b5ff0 | skill is the requirement block of the skill template (gCSkill_PS), spell the one of the magic property set (gCMagic_PS) when the template has one; att... | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1570 |
| `can_equip` (gCInventory_PS) | Game:0x100b7dc0 | equip_bonus is the result of the script hook OnPlayerGetEquipBonus(0, 0, quality) (added to every attribute requirement, floored at 0); missing_value(... | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1624 |
| `consume_items` (gCInventory_PS) | Game:0x100b7970 | apply the item's modifiers as one-off consumption (Consume) and activate its skills permanently. The caller removes the item afterwards with [Inventor... | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1653 |
| `compare_protection` (gCInventoryStack) | Game:0x100c2f10 | higher PROT_BLADE first. | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:95 |
| `compare_value` (gCInventoryStack) | Game:0x100c2b00 | more valuable first. | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:115 |
| `compare_slot` (gCInventoryStack) | Game:0x100c29d0 | linked slot order, unlinked stacks after slot 18. | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:149 |
| `compare_armor` (gCInventoryStack) | Game:0x100c2610 | gCInventoryStack::CompareArmor (Game:0x100c2610). | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:192 |
| `compare_default` (gCInventoryStack) | Game:0x100c2880 | gCInventoryStack::CompareDefault (Game:0x100c2880). | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:204 |
| `execute_script` (gCItem_PS) | Game:0x100b9df0 | scripts guarded by a SetGameEvent line run only once per event; otherwise every line runs in order. (The original returns false in both cases and sets... | DEAD | crates/gameplay/src/inventory/script.rs:214 |
| `is_display_slot` (gCInventory_PS) | Game:0x100b34e0 | slots whose item is shown on the character (carry slots, armor, jewellery and the body parts below Helmet). | DEAD | crates/gameplay/src/inventory/slot.rs:97 |
| `equip_slots` (gCInventorySlot) | Game:0x100bd2a0 | Returns the primary and the alternative carry slot of an item with this use type; Slot::None when it has none. | INTERNAL-ONLY ? | crates/gameplay/src/inventory/slot.rs:140 |
| `is_skinned_slot` (gCInventory_PS) | Game:0x100b50b0 | the item in this slot replaces a skinned body part. | DEAD | crates/gameplay/src/inventory/slot.rs:216 |
| `equip_slots` (gCInventorySlot) | - | primary and alternative slot. | INTERNAL-ONLY ? | crates/gameplay/src/inventory/stack.rs:122 |
| `is_permanent` (gCInventorySlot) | Game:0x100bcfd0 | spells and skills, or items flagged Permanent. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/inventory/stack.rs:128 |
| `is_equipment` (gCInventorySlot) | Game:0x100bd040 | weapons, armor and artefacts except letters. | DEAD | crates/gameplay/src/inventory/stack.rs:135 |
| `is_consumable` (gCInventorySlot) | Game:0x100bd0b0 | the alchemy category. | DEAD | crates/gameplay/src/inventory/stack.rs:146 |
| `is_equipped_in_primary` (gCInventoryStack) | Game:0x100c0440 | gCInventoryStack::IsEquippedInPrimary (Game:0x100c0440). | INTERNAL-ONLY | crates/gameplay/src/inventory/stack.rs:158 |
| `is_equipped_in_alternative` (gCInventoryStack) | Game:0x100c0490 | gCInventoryStack::IsEquippedInAlternative (Game:0x100c0490). | INTERNAL-ONLY | crates/gameplay/src/inventory/stack.rs:164 |
| `is_equipped` (gCInventoryStack) | Game:0x100c03e0 | linked to its primary or alternative carry slot. A weapon that is currently drawn sits in a hand slot and is not "equipped" by this test; see [Stack::... | WIRED-NOT-EXERCISED ? | crates/gameplay/src/inventory/stack.rs:170 |
| `is_equipped_to_slot` (gCInventoryStack) | Game:0x100c03c0 | gCInventoryStack::IsEquippedToSlot (Game:0x100c03c0). | INTERNAL-ONLY | crates/gameplay/src/inventory/stack.rs:183 |
| `must_split_before_linking` (gCInventoryStack) | Game:0x100c0550 | a stack of an equippable item has to be split before one piece is linked; ammunition only when the target is the right hand. | INTERNAL-ONLY | crates/gameplay/src/inventory/stack.rs:196 |
| `is_mergeable` (gCInventoryStack) | Game:0x100c0530 | unassigned quick slot and not linked. | DEAD | crates/gameplay/src/inventory/stack.rs:209 |
| `can_merge` (gCInventoryStack) | Game:0x100c0590 | same template, quality and stack type, both without quick slot and both unlinked. | INTERNAL-ONLY | crates/gameplay/src/inventory/stack.rs:216 |
| `merge` (gCInventoryStack) | Game:0x100c0950 | move all of other into self. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/inventory/stack.rs:229 |
| `split` (gCInventoryStack) | Game:0x100c09b0 | cut n items off into a new unlinked stack. None unless 0 < n < amount. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/inventory/stack.rs:240 |
| `stack_gold_value` (gCInventoryStack) | Game:0x100c0660 | gCInventoryStack::GetStackGoldValue (Game:0x100c0660). | DEAD | crates/gameplay/src/inventory/stack.rs:266 |
| `is_empty` (gCInventoryStack) | Game:0x100c03b0 | gCInventoryStack::IsEmpty (Game:0x100c03b0). | WIRED-NOT-EXERCISED ? | crates/gameplay/src/inventory/stack.rs:272 |
| `transfer_amount` (gCInventoryStack) | Game:0x100c0ce0 | the marked amount, everything when the mark is negative, never more than the stack holds. | DEAD | crates/gameplay/src/inventory/stack.rs:284 |
| `transfer_remainder` (gCInventoryStack) | Game:0x100c0d00 | what stays behind after the transfer. | DEAD | crates/gameplay/src/inventory/stack.rs:295 |
| `shows_normal_icon` (gCInventoryStack) | Game:0x100c0a40 | the normal icon once the stack is learned or activated, the grey one before. | INTERNAL-ONLY | crates/gameplay/src/inventory/stack.rs:306 |
| `icon_file_names` (gCInventoryStack) | Game:0x100c0ae0 | (normal, grey) icon file names. An empty template icon is replaced by category_default(category) for both. A grey variant (_Grey before the extension)... | DEAD | crates/gameplay/src/inventory/stack.rs:313 |
| `toggle` (gCMover_PS) | Game:0x1a7130 | open what is closed or closing, close what is open or opening. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/mover.rs:113 |
| `attribute` (gCAttribute) | Game:0x183850 | base 100, no modifier. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/stats.rs:37 |
| `pool` (gCStat) | Game:0x1845f0 | value and maximum 100. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/stats.rs:46 |
| `value` (gCAttribute) | Game:0x183880 | base plus modifier. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/stats.rs:57 |
| `maximum` (gCAttribute) | Game:0x183890 | gCAttribute::GetMaximum / gCStat::GetMaximum (Game:0x183890 / 0x184630). | INTERNAL-ONLY | crates/gameplay/src/stats.rs:63 |
| `set_value` (gCAttribute) | Game:0x1838a0 | the base follows so that the value is as asked, then the stat is capped. | INTERNAL-ONLY | crates/gameplay/src/stats.rs:73 |
| `set_maximum` (gCStat) | Game:0x184640 | gCStat::SetMaximum (Game:0x184640). | INTERNAL-ONLY | crates/gameplay/src/stats.rs:81 |
| `cap` (gCAttribute) | Game:0x183a10 | a pool's maximum is at least 0; the value stays within 0..=maximum. | WIRED-NOT-EXERCISED | crates/gameplay/src/stats.rs:90 |
| `apply_perm` (gCAttribute) | Game:0x183980 | the permanent change of a consumed item. Only the add (0) and set (4) operations report success, as in the original. | INTERNAL-ONLY | crates/gameplay/src/stats.rs:105 |
| `apply_temp` (gCAttribute) | Game:0x1839d0 | what worn equipment does: operation 0 raises the value modifier, operation 1 (pools only) the maximum modifier. | INTERNAL-ONLY | crates/gameplay/src/stats.rs:177 |
| `unapply_temp` (gCAttribute) | Game:0x1839f0 | gCAttribute::UnapplyTemp / gCStat::UnapplyTemp (Game:0x1839f0 / 0x1848d0). | INTERNAL-ONLY | crates/gameplay/src/stats.rs:197 |
| `can_consume` (gCAttrib) | Game:0x186ea0 | would the permanent change leave the value non-negative (checked before the cap)? | DEAD | crates/gameplay/src/stats.rs:215 |
| `apply_mod` (gCPlayerMemory_PS) | Game:0x14efa0 | equipment raises a temporary modifier. A raised maximum of HP / MP takes the current value along in proportion (value + value * p / max, rounded). | WIRED-NOT-EXERCISED | crates/gameplay/src/stats.rs:227 |
| `unapply_mod` (gCPlayerMemory_PS) | Game:0x14f0a0 | the opposite; the value shrinks with the maximum (value * (max - p) / max, rounded). | WIRED-NOT-EXERCISED | crates/gameplay/src/stats.rs:243 |
| `consume` (gCPlayerMemory_PS) | Game:0x14ed60 | the permanent change. | WIRED-NOT-EXERCISED | crates/gameplay/src/stats.rs:256 |

### world (entity admin, PVS, sectors, triggers, zones) (44)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `decay` (eCDynamicEntity) | Engine:0x1022a1b0 | states are the decay states of the entity's property sets after asking each to decay. An entity without property sets is done at once. | WIRED-NOT-EXERCISED | crates/world/src/dynamic.rs:27 |
| `process_decay` (the end of eCDynamicEntity) | Engine:0x1022a980 | when every property set reports a finished decay state the entity kills itself. Entities without sets never do. | DEAD | crates/world/src/dynamic.rs:46 |
| `is_auto_kill_enabled` (eCDynamicEntity) | Engine:0x1022a140 | any set asks for it. | DEAD | crates/world/src/dynamic.rs:58 |
| `update_parent_dependencies` (eCEntity) | Engine:0x10227860 | the tree box of an entity is its node box merged with the tree boxes of its children; the change bubbles up. | INTERNAL-ONLY | crates/world/src/entity.rs:436 |
| `cache_out` (eCEntity) | Engine:0x10224150 | leave the processing range, then drop residency. mark_stale sets the update-pending bit. recursive also caches out the children. | INTERNAL-ONLY | crates/world/src/entity.rs:535 |
| `contains_parent` (eCNode) | Engine:0x1020da60 | is ancestor on the parent chain of id? | INTERNAL-ONLY | crates/world/src/entity.rs:599 |
| `move_to_node` (eCNode) | Engine:0x1020dd00 | leave the current parent and attach to parent, keeping the local matrix like AttachChild. A missing target is a no-op, as in the original. | DEAD | crates/world/src/entity.rs:613 |
| `clone_entity` (eCNode) | Engine:0x1020d960 | a copy of the entity (flags, matrices, boxes, template) without children, attached next to the original. Residency bits are not copied: the clone is n... | INTERNAL-ONLY | crates/world/src/entity.rs:628 |
| `clone_subgraph` (eCNode) | Engine:0x1020d9c0 | clone an entity and, recursively, its descendants; the descendants hang under the clones. Returns the clone of id. | INTERNAL-ONLY | crates/world/src/entity.rs:652 |
| `set_temporary` (eCEntity) | - | eCEntity::SetTemporary: the entity is not saved with the world. | INTERNAL-ONLY | crates/world/src/entity.rs:669 |
| `resolve` (eCEntityProxy) | Engine:0x1022e470 | and the cached read of operator eCEntity*: the cached entity while it is registered, else the id lookup. | WIRED-NOT-EXERCISED ? | crates/world/src/proxy.rs:138 |
| `parse` (eCPropertyProxy) | Engine:0x1022ea80 | and SplitAddress (0x1022eb70). The expression is address op "value"; the address splits at its last two dots into [entity.]set.property. | WIRED-NOT-EXERCISED ? | crates/world/src/proxy.rs:188 |
| `execute` (eCPropertyProxy) | Engine:0x1022e800 | with SetValue: parse, find the entity (by name, or the owner), find the set and the property and apply the operator. An empty expression, or one witho... | WIRED-NOT-EXERCISED ? | crates/world/src/proxy.rs:240 |
| `entity_by_part_name` (eCSceneAdmin) | Engine:0x10232c20 | prefix search, case insensitive; index picks from the name-ordered matches. None index (the original's -1) returns the first match found in registry o... | DEAD | crates/world/src/scene.rs:201 |
| `count_by_part_name` (eCSceneAdmin) | Engine:0x10232f90 | eCSceneAdmin::GetEntityCountByPartName (Engine:0x10232f90). | DEAD | crates/world/src/scene.rs:229 |
| `trigger_targets` (eCSceneAdmin) | Engine:0x10233ba0 | the entities that receive the trigger from source. Delivery (the entity's trigger virtual) is the caller's job. | WIRED-NOT-EXERCISED | crates/world/src/scene.rs:248 |
| `enable_layer_activation` (gCSector) | Game:0x10192ca0 | with the switch off, OnEnable only stores the flag and leaves the layers alone. | DEAD | crates/world/src/sector.rs:89 |
| `add_layer` (gCSector) | - | gCSector::InsertGeometryLayer / InsertDynamicLayer: attach a layer. Returns false for an unknown sector. A layer added to a disabled sector starts dis... | DEAD | crates/world/src/sector.rs:108 |
| `contains_entity` (gCSector) | Game:0x10192b60 | is entity below a layer root of the sector? | DEAD | crates/world/src/sector.rs:221 |
| `collect` (gCSectorPersistence_PS) | Game:0x101a18f0 | the save image. | WIRED-NOT-EXERCISED ? | crates/world/src/sector.rs:257 |
| `update_all` (gCSectorPersistence_PS) | Game:0x101a1800 | re-apply a save image. Returns the names that are not part of the world (the original logs a warning). | DEAD | crates/world/src/sector.rs:269 |
| `enable_context` (eCContextBase) | Engine:0x1021a060 | for a context root: a context that is switched off is cached out first (stale), then the root and its subtree are enabled or disabled. | INTERNAL-ONLY | crates/world/src/sector.rs:290 |
| `split_template_path` (gCDatabase) | Game:0x1018e1d0 | Layer:Name, slashes trimmed. The layer is None when the path names only the template. | INTERNAL-ONLY | crates/world/src/template.rs:88 |
| `ref_template` (eCTemplateEntity) | Engine:0x1022c7a0 | eCTemplateEntity::GetRefTemplate (Engine:0x1022c7a0). | WIRED-NOT-EXERCISED | crates/world/src/template.rs:141 |
| `patch_entity` (eCTemplateEntity) | Engine:0x1022bf30 | Applies the template chain (base first) to the property sets of entity:  * a template set that is flagged replace, or any set when replace_all is give... | INTERNAL-ONLY | crates/world/src/template.rs:171 |
| `spawn` (gCEntity) | Game:0x100664e0 | and eCEntity::CreateFromTemplate: spawn an entity from the template named by path (Layer:Name or Name), place it, cache it in and patch its property s... | WIRED-NOT-EXERCISED ? | crates/world/src/template.rs:227 |
| `patch_instances` (eCTemplateEntity) | Engine:0x1022c620 | re-patch every live instance after the template changed. Returns how many were patched. | DEAD | crates/world/src/template.rs:265 |
| `delete_template` (eCTemplateEntity) | Engine:0x1022bea0 | kill all instances of the template, newest first, and drop the template. Returns the killed entities. | DEAD | crates/world/src/template.rs:283 |
| `on_intersect` (eCTrigger_PS) | Engine:0x1007da70 | eCTrigger_PS::OnIntersect (Engine:0x1007da70). | DEAD | crates/world/src/trigger.rs:358 |
| `on_trigger` (eCTrigger_PS) | Engine:0x1007da00 | another trigger fired at this entity. | DEAD | crates/world/src/trigger.rs:385 |
| `on_untrigger` (eCTrigger_PS) | Engine:0x1007da50 | eCTrigger_PS::OnUntrigger (Engine:0x1007da50). | DEAD | crates/world/src/trigger.rs:398 |
| `on_damage` (eCTrigger_PS) | Engine:0x1007dad0 | damage above the threshold fires. | INTERNAL-ONLY | crates/world/src/trigger.rs:410 |
| `trigger_by_name` (eCTrigger_PS) | Engine:0x1007da20 |  (Engine:0x1007da20): fire the triggers whose trigger_name equals name. | DEAD | crates/world/src/trigger.rs:436 |
| `filter` (eCEventFilter_PS) | Engine:0x10077740 | and its siblings: the named entities get the configured event instead. | WIRED-NOT-EXERCISED | crates/world/src/trigger.rs:521 |
| `list_activate` (eCTriggerList_PS) | Engine:0x10084220 | eCTriggerList_PS::TriggerTarget / DoTriggering (Engine:0x10084220 / 0x10084040). | INTERNAL-ONLY | crates/world/src/trigger.rs:830 |
| `contains` (gCCollisionCircle_PSObj) | Game:0x1013b190 | is the point inside the volume? inclusive includes the border (bCInfCylinder::Cull's flag). | WIRED-NOT-EXERCISED ? | crates/world/src/zones.rs:24 |
| `first_hit` (gCCollisionCircle_PSObj) | Game:0x1013ad30 | does the segment from -> to touch the volume? Returns the entry parameter (0..1) of the first touch. | INTERNAL-ONLY | crates/world/src/zones.rs:38 |
| `contains@2` (gCNegZone_PS) | Game:0x10119710 | the sum of the signed angles under which the stakes' edges are seen from the point; a full turn means inside. A point on a stake or on an edge is unde... | WIRED-NOT-EXERCISED ? | crates/world/src/zones.rs:119 |
| `intersects_point` (gCNegZone_PSObj) | Game:0x1013c780 | the bounding circle culls first, then the polygon decides. | INTERNAL-ONLY | crates/world/src/zones.rs:141 |
| `intersects_stretch` (gCNegZone_PSObj) | - | gCNegZone_PSObj::IntersectsStretch: does the segment enter the zone (an end inside, or a crossing of an edge)? | INTERNAL-ONLY | crates/world/src/zones.rs:148 |
| `is_illegal` (gCNegZone_PS) | Game:0x10119470 | an authored polygon whose non-adjacent edges cross cannot be walked around. | INTERNAL-ONLY | crates/world/src/zones.rs:159 |
| `add_circle` (gCDynamicCollisionCircle_PS) | Game:0x10106a30 | a circle exists while its radius is positive. Returns whether one was created. | DEAD | crates/world/src/zones.rs:209 |
| `remove_circle` (DestroyCollisionCirclePSObject) | Game:0x10106c90 | DestroyCollisionCirclePSObject (Game:0x10106c90). | DEAD | crates/world/src/zones.rs:227 |
| `follow` (gDynamicCollisionCircle_PS) | Game:0x10108c20 | the circle follows its entity; it is recreated at the entity's new place. | WIRED-NOT-EXERCISED | crates/world/src/zones.rs:233 |

### animation (EMotionFX wrapper, blending, attachments) (35)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `stop_slot` (eCWrapper_emfx2Actor) | Engine:0x10047910 | Zero fade removes the instance, otherwise it fades out like SwitchFadeOut. | DEAD | crates/animation/src/animator.rs:174 |
| `stop_at_loop_end` (eCWrapper_emfx2Actor) | Engine:0x100479c0 | eCWrapper_emfx2Actor::StopAtLoopEnd (Engine:0x100479c0). | INTERNAL-ONLY ? | crates/animation/src/animator.rs:185 |
| `set_target_weight` (eCWrapper_emfx2Actor) | Engine:0x10049ca0 | eCWrapper_emfx2Actor::SetTargetWeight (Engine:0x10049ca0). | INTERNAL-ONLY ? | crates/animation/src/animator.rs:193 |
| `set_blend_mode` (eCWrapper_emfx2Actor) | Engine:0x10049d10 | eCWrapper_emfx2Actor::SetBlendMode (Engine:0x10049d10). | INTERNAL-ONLY ? | crates/animation/src/animator.rs:201 |
| `is_slot_playing` (eCWrapper_emfx2Actor) | Engine:0x1004a040 | eCWrapper_emfx2Actor::IsActorPlaying (Engine:0x1004a040). | DEAD | crates/animation/src/animator.rs:225 |
| `add_once` (eCVisualAnimation_PS) | Engine:0x10030f50 | like [add] but the attachment is placed a single time. The entity's own rendering and processing are switched off by the caller (the item is drawn thr... | DEAD | crates/animation/src/attach.rs:59 |
| `remove_once` (eCVisualAnimation_PS) | Engine:0x10030fe0 | eCVisualAnimation_PS::RemoveOnceAttachment (Engine:0x10030fe0). | DEAD | crates/animation/src/attach.rs:89 |
| `clear` (ClearAttachments) | Engine:0x10030bc0 | ClearAttachments (Engine:0x10030bc0). | WIRED-NOT-EXERCISED ? | crates/animation/src/attach.rs:101 |
| `clear_once` (ClearOnceAttachments) | Engine:0x10030cb0 | ClearOnceAttachments (Engine:0x10030cb0). | DEAD | crates/animation/src/attach.rs:107 |
| `relink` (RelinkAttachments) | Engine:0x10031110 | forgets the cached bone nodes (the skeleton changed, for instance after a head was attached). | DEAD | crates/animation/src/attach.rs:113 |
| `parse` (eCAnimationAdmin) | Engine:0x10020a20 | the G3_Ani_Blendings.txt part. The text must contain *Version 1; every *Entry-> is followed by a clip key and two integers (intro, extro) stored as by... | WIRED-NOT-EXERCISED ? | crates/animation/src/blend.rs:79 |
| `register_variation` (eCAnimationAdmin) | Engine:0x100211c0 | on first sight of a clip family, word 11 of the underscore-separated file name is its variation maximum. | DEAD | crates/animation/src/blend.rs:133 |
| `with_variation` (eCAnimationAdmin) | Engine:0x10021290 | with a known maximum the two characters before the trailing underscore of the key become a random number in 0..=max (rand is the caller's random value... | DEAD | crates/animation/src/blend.rs:153 |
| `set_target_weight` (eCWrapper_emfx2Actor) | Engine:0x10049ca0 | an instant change of current and target weight. | INTERNAL-ONLY ? | crates/animation/src/blend.rs:326 |
| `set_blend_mode` (eCWrapper_emfx2Actor) | Engine:0x10049d10 | only mode 2 sets the mix flag. | INTERNAL-ONLY ? | crates/animation/src/blend.rs:337 |
| `stop_at_loop_end` (eCWrapper_emfx2Actor) | Engine:0x100479c0 | a running instance gets its maximum loops set to the current loop plus one. | INTERNAL-ONLY ? | crates/animation/src/blend.rs:344 |
| `idle_mixer_decision` (the owner switch of the idle mixer. running is ) | - |  with a non-zero play speed; remaining the time to the end of the clip and duration its length; items_changed says the hand-item combination changed s... | DEAD | crates/animation/src/blend.rs:632 |
| `add` (eCVisualAnimationFactory) | Engine:0x1003a270 | an existing part of the same name is removed first; the new part is visible. An empty first-level mesh name is refused (the original needs the owner's... | WIRED-NOT-EXERCISED ? | crates/animation/src/body_parts.rs:65 |
| `remove` (eCVisualAnimationFactory) | Engine:0x1003a3b0 | drop the part and let the caller strip bones no remaining mesh uses. | WIRED-NOT-EXERCISED ? | crates/animation/src/body_parts.rs:90 |
| `set_visible` (eCVisualAnimationFactory) | Engine:0x1003a430 | eCVisualAnimationFactory::SetBodyPartVisible (Engine:0x1003a430). | DEAD | crates/animation/src/body_parts.rs:103 |
| `is_visible` (eCVisualAnimationFactory) | Engine:0x1003a480 | false for unknown parts. | WIRED-NOT-EXERCISED ? | crates/animation/src/body_parts.rs:120 |
| `set_constraint` (eCVisualAnimation_PS) | Engine:0x10032750 | one constraint per node name; setting an existing one replaces limits and speed and resets its state. | DEAD | crates/animation/src/lookat.rs:85 |
| `remove_constraint` (eCVisualAnimation_PS) | Engine:0x10032890 | eCVisualAnimation_PS::RemoveLookAtConstraint (Engine:0x10032890). | DEAD | crates/animation/src/lookat.rs:104 |
| `clear` (ClearLookAtConstraints) | Engine:0x10032180 | ClearLookAtConstraints (Engine:0x10032180). | WIRED-NOT-EXERCISED ? | crates/animation/src/lookat.rs:116 |
| `enable_all` (eCLookAtConstraint) | Engine:0x1002afa0 | for every constraint. | DEAD | crates/animation/src/lookat.rs:122 |
| `process_plan` (the gating in eCVisualAnimation_PS) | Engine:0x10032d40 | an actor is advanced when it was seen this frame, when a live ragdoll needs its physics stepped, when once-only attachments are waiting, or when it is... | DEAD | crates/animation/src/process.rs:43 |
| `step` (the chain step in eCVisualAnimation_PS) | - | 0 when nothing plays. | WIRED-NOT-EXERCISED ? | crates/animation/src/process.rs:115 |
| `set_motion` (eCVisualAnimation_PS) | Engine:0x1002bee0 | eCVisualAnimation_PS::SetMotion (Engine:0x1002bee0). | DEAD | crates/animation/src/visual.rs:35 |
| `has_motion` (eCVisualAnimation_PS) | - | eCVisualAnimation_PS::HasMotion: a motion was set for the slot. | WIRED-NOT-EXERCISED ? | crates/animation/src/visual.rs:47 |
| `owner` (eCVisualAnimation_PS) | Engine:0x1002bfc0 | eCVisualAnimation_PS::GetMotionOwner (Engine:0x1002bfc0). | WIRED-NOT-EXERCISED | crates/animation/src/visual.rs:53 |
| `set_owner` (eCVisualAnimation_PS) | Engine:0x1002bfa0 | eCVisualAnimation_PS::SetMotionOwner (Engine:0x1002bfa0). | DEAD | crates/animation/src/visual.rs:61 |
| `reset` (GameReset) | Engine:0x1002b700 | for the descriptors: names cleared, owners free. | WIRED-NOT-EXERCISED ? | crates/animation/src/visual.rs:75 |
| `animation_delta` (eCVisualAnimation_PS) | Engine:0x100301b0 | / the frame delta of eCWrapper_emfx2Actor::Process: animation time is the application's scaled frame time times the entity's local scale. | DEAD | crates/animation/src/visual.rs:82 |
| `aim_pose` (the weight computation of UpdateAim) | - | gives f = /pitch/ / (pi/2); the centre pose gets 1 - f and the tilt pose full weight once f > 0. [I] for the exact weights. | WIRED-NOT-EXERCISED | crates/animation/src/visual.rs:109 |
| `physics_lod` (the distance test of ProcessPhysicsLOD. The decomp) | - | the distance test of ProcessPhysicsLOD. The decompiled band expression is garbled, so the linear ramp over the last 250 units before the limit is [I]. | DEAD | crates/animation/src/visual.rs:156 |

### script helpers / predicates / stats (34)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `focus_name_attitude` (GetFocusNameAttitude) | - | the attitude shown on the focus name of npc, i.e. how the NPC regards the player [I: the argument order of the inner call is not recoverable]. | DEAD | crates/script/src/attitude.rs:224 |
| `ani_state_by_action` (GetAniStateByAction) | S:aa030 | the pose an action leaves the character in. current is the pose before; a death keeps the lying family of a knocked-down character (sitting and lying ... | DEAD | crates/script/src/callbacks.rs:21 |
| `dcc_radius` (GetDCCRadius) | S:aa3a0 | the radius of the character-controller capsule in cm: trolls 120, ambient creatures 20, everybody else 60. | DEAD | crates/script/src/callbacks.rs:55 |
| `first_person_camera_allowed` (IsFirstPersonCameraAllowed) | S:aaef0 | not while down or dead, and not in the knock-down tasks. | DEAD | crates/script/src/callbacks.rs:68 |
| `refresh_hit_points` (RefreshHitPoints) | S:ab840 | stamina and hit points back to their maxima (the maxima themselves are re-clamped by the setters). | DEAD | crates/script/src/callbacks.rs:81 |
| `on_enter_magic_barrier` (OnEnterMagicBarrier) | S:5ebb0 | a living character that enters the barrier gets its effect. | DEAD | crates/script/src/callbacks.rs:93 |
| `on_leave_magic_barrier` (OnLeaveMagicBarrier) | S:5ec90 | the barrier effect ends (reported as the effect name with a leading minus). | DEAD | crates/script/src/callbacks.rs:105 |
| `on_touch_barrier` (OnToucheBarrier) | S:5ed60 | a living character that runs into the barrier stops, forgets who is hitting it (the current attacker becomes the last one) and stumbles back. | DEAD | crates/script/src/callbacks.rs:117 |
| `routine_point` (the routine-point helper) | - | the NPC's working, relaxing or sleeping point depending on GetRoutineTime (0 sleeping, 2 relaxing, otherwise working). | WIRED-NOT-EXERCISED ? | crates/script/src/helpers.rs:148 |
| `use_object` (the use-object accessor) | - | the interaction object currently found for npc for a use type (the scanner S:7c90 fills the table), or NONE. | WIRED-NOT-EXERCISED ? | crates/script/src/helpers.rs:156 |
| `is_undead` (IsUndead) | S:112f0 | Skeleton or Zombie. | WIRED-NOT-EXERCISED ? | crates/script/src/predicates.rs:27 |
| `can_sleep` (CanSleep) | - | species that use sleeping points. | DEAD | crates/script/src/predicates.rs:86 |
| `can_threaten` (CanThreaten) | - | species that can play the threaten reaction (Goblin through Bloodfly, SwampLurker, Ogre, Stalker, Dragon). | INTERNAL-ONLY | crates/script/src/predicates.rs:116 |
| `can_eat_ground` (CanEatGround) | - | grazing and scavenging species. | DEAD | crates/script/src/predicates.rs:148 |
| `can_swim` (CanSwim) | S:bd420 | a stub that always returns false in the shipped script. | DEAD | crates/script/src/predicates.rs:176 |
| `has_attack_reason_to_watch` (HasAttackReasonToWatch) | S:10550 | reasons 17..=18 and 20 (Duel, Arena, Kill). | DEAD | crates/script/src/predicates.rs:203 |
| `zone_attitude_to_player` (GetZoneAttitudeToPlayer) | - | how the owner of zone regards the player.  Public zones owned by an enclave: security level Political compares the player's political fame of the owne... | INTERNAL-ONLY | crates/script/src/predicates.rs:212 |
| `start_say_confirm` (StartSayConfirm) | S:51680 | party members have three variants, everybody else "Sure". | DEAD | crates/script/src/speech.rs:173 |
| `start_say_follow` (StartSayFollow) | S:518f0 | the companion's reaction to "follow me" (only when the player gives the order). | DEAD | crates/script/src/speech.rs:184 |
| `start_say_holding_position` (StartSayHoldingPosition) | S:517c0 | "Sure" for NPCs without party voices and for party members of type 3 (a hired fighter), "PartyWait" for the other companions. | DEAD | crates/script/src/speech.rs:195 |
| `start_say_refuse_talk` (StartSayRefuseTalk) | S:52280 | a sleeping NPC says "SleepRefuse"; everybody else rotates through three refusals (what they are busy with, a polite excuse, a brush-off). The second a... | INTERNAL-ONLY | crates/script/src/speech.rs:262 |
| `start_say_crime_refuse` (StartSayCrimeRefuse) | S:524c0 | an NPC that knows about a crime of the player refuses to talk; after a theft the three steps are "DontStealAnything", "TheftRefuse", "GetLost", otherw... | INTERNAL-ONLY | crates/script/src/speech.rs:294 |
| `start_say_zone_refuse` (StartSayZoneRefuse) | S:52640 | an NPC in a zone hostile to the player refuses to talk. In a locked-down (Enclave-level) zone the first step names the owning enclave ("NoFriendOfORC"... | INTERNAL-ONLY | crates/script/src/speech.rs:310 |
| `start_say_nothing_to_get` (StartSayNothingToGet) | S:6f560 | the player's comment on an emptied container. | INTERNAL-ONLY | crates/script/src/speech.rs:346 |
| `boss_news_plan` (_AI_SayBossNews) | S:3c490 | the line plan of a boss telling the player about the crimes against his faction. After the opener "BadNews" the lines depend on the level; at level 3 ... | INTERNAL-ONLY | crates/script/src/speech.rs:486 |
| `crime_investigation_plan` (_AI_SayCrimeInvestigation) | S:3be50 | the question an investigator asks and the answer. crime 1 is a murder, 2 a theft; step 1 asks, 2 accuses. The follow-up (reached when the player had s... | DEAD | crates/script/src/speech.rs:530 |
| `guard_news_plan` (_AI_SayGuardNews) | S:3a3c0 | as a line plan [I]. The guard stops the player ("Halt"), names the crime he knows (murders/thefts counts of the enclave: "KnownMurder" or "KnownTheft"... | DEAD | crates/script/src/speech.rs:553 |
| `diff_to_player_level` (GetDiffToPlayerLevel) | S:36eb0 | CurrentLevel(npc) - player level, 0 for the player. | DEAD | crates/script/src/stats.rs:68 |
| `set_attribute` (SetStrength) | S:33ae0, S:33ba0 | SetDexterity (S:33ba0), SetIntelligence (S:33c60), SetAlchemy (S:33d20), SetSmithing (S:33de0), SetTheft (S:33ea0): player only. | DEAD | crates/script/src/stats.rs:100 |
| `add_attribute` (AddStrength) | S:33f60, S:34060 | AddDexterity (S:34060), AddIntelligence (S:34160), AddAlchemy (S:34260), AddSmithing (S:34360), AddTheft (S:34460): adds to the player's base value an... | INTERNAL-ONLY | crates/script/src/stats.rs:110 |
| `set_protection` (SetProtectionBlades) | S:34c60, S:34d20 | SetProtectionImpact (S:34d20), SetProtectionMissile (S:34de0), SetProtectionFire (S:34ea0), SetProtectionIce (S:34f60), SetProtectionLightning (S:3502... | DEAD | crates/script/src/stats.rs:150 |
| `add_protection` (AddProtectionBlades) | S:350e0, S:351a0 | ...Impact (S:351a0), ...Missile (S:35260), ...Fire (S:35320), ...Ice (S:353e0), ...Lightning (S:354a0): player only. | DEAD | crates/script/src/stats.rs:161 |
| `set_pool_max` (SetHitPointsMax) | S:359f0, S:36080 | SetManaPointsMax (S:36080), SetStaminaPointsMax (S:367a0): writes the maximum and re-clamps the current value (HitPointsMax also floors the current va... | INTERNAL-ONLY | crates/script/src/stats.rs:271 |
| `add_pool_max` (AddHitPointsMax) | S:35c20, S:36260 | AddManaPointsMax (S:36260), AddStaminaPointsMax (S:36a20): the player's maximum grows (and the gain is printed as HP/MP/SP); no-op for NPCs. | DEAD | crates/script/src/stats.rs:341 |

### ZS_* routine tasks (24)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `guard_attack_reason` (GetGuardAttackReason) | S:3d050 | why a guard attacks somebody whose crime it knows: a murderer is fought to the death (reason Revolution, 19), a thief with reason Theft, anybody else ... | INTERNAL-ONLY | crates/script/src/routine/dialog.rs:14 |
| `excuse` (_AI_Excuse) | S:3d130 | the player excuses a crime to a guard.  Needs Perk_Excuse. Crimes up to Attack always work; for theft and murder the guard rolls Random(130): above 11... | WIRED-NOT-EXERCISED ? | crates/script/src/routine/dialog.rs:38 |
| `bribe` (_AI_Bribe) | S:3d7d0 | the player offers a guard money to forget a crime.  Accepted (the dialog took the gold): the guard says LetsForgetThis, ThisTimeYoureLucky and NoMoreT... | WIRED-NOT-EXERCISED ? | crates/script/src/routine/dialog.rs:102 |
| `pick_pocket` (PickPocket) | S:3dd70 | one attempt. A caught thief is attacked (reason Theft); a successful one counts toward the victim enclave's thefts unless ExtendedContent is on and th... | DEAD | crates/script/src/routine/dialog.rs:183 |
| `has_guard_news` (HasGuardNews) | S:bcf00 | a guard with the GuardAndBoss voice block whose enclave knows of a murder or a theft by the player has something to say. | DEAD | crates/script/src/routine/info.rs:27 |
| `has_boss_news` (HasBossNews) | S:bd050 | a boss with the GuardAndBoss voice block whose political camp has more crimes on record than comments made, as long as the player is not already marke... | DEAD | crates/script/src/routine/info.rs:37 |
| `has_heal_info` (HasHealInfo) | S:bd2c0 | a humanoid member of the player's party who carries a health potion can offer to heal him. | DEAD | crates/script/src/routine/info.rs:49 |
| `join_player_party` (JoinPlayerParty) | S:a8480 | message GO_PartyJoin, member type Party. | DEAD | crates/script/src/routine/party.rs:49 |
| `join_player_mob` (JoinPlayerMob) | S:a8900 | message GO_MobJoin, member type Mob. | DEAD | crates/script/src/routine/party.rs:55 |
| `join_player_slavery` (JoinPlayerSlavery) | S:a8d30 | message GO_SlaveJoin, member type Slave. | DEAD | crates/script/src/routine/party.rs:61 |
| `dismiss_player_party` (DismissPlayerParty) | S:a8690 | message GO_PartyDismiss; a Controlled member also restarts its routine. A summoned creature is killed instead. | INTERNAL-ONLY | crates/script/src/routine/party.rs:92 |
| `dismiss_player_mob` (DismissPlayerMob) | S:a8b10 | message GO_MobDismiss. | DEAD | crates/script/src/routine/party.rs:99 |
| `wait_player_party` (WaitPlayerParty) | S:a8f40 | the member waits where it stands (GO_SlaveDismiss message, Waiting set, ZS_WaitForPlayer). Summoned creatures ignore the call. | DEAD | crates/script/src/routine/party.rs:105 |
| `can_join_player_party` (CanJoinPlayerParty) | S:a80a0 | summoned creatures always may; otherwise a waiting or out-of-range companion is dismissed to make room and an active, in-range one blocks, so at most ... | DEAD | crates/script/src/routine/party.rs:120 |
| `start_guide_player` (StartGuidePlayer) | S:a9110 | the NPC becomes the player's guide (the player's PSParty leader is set to the NPC). | DEAD | crates/script/src/routine/party.rs:141 |
| `stop_guide_player` (StopGuidePlayer) | S:a9200 | only the current guide can stop guiding; the guide link is cleared and the stop time recorded. Returns false when npc is not the guide. | DEAD | crates/script/src/routine/party.rs:149 |
| `stop_bring_npc` (StopBringNPC) | S:a9370 | clears the dialog's PartyEnabled flag so the NPC can no longer be asked to join. | DEAD | crates/script/src/routine/party.rs:163 |
| `is_free_destination` (IsFreeDestination) | S:a9d00 | can npc use interact point point?  A point without an interaction property is never free; an owned point is free only for its owner; an anchor-backed ... | INTERNAL-ONLY | crates/script/src/routine/select.rs:136 |
| `select_routine_destination` (SelectRoutineDestination) | S:a7050 | pick the point the NPC should walk to for the current routine time.  Skips NPCs in a running arena fight, members of the player's party and NPCs that ... | DEAD | crates/script/src/routine/select.rs:245 |
| `send_to_arena` (SendToArena) | S:a7f80 | stop what the NPC does and, if it is within processing range, restart its routine (the arena task takes over through SelectRoutineTask).  The DLL refu... | DEAD | crates/script/src/routine/select.rs:450 |
| `rtn_monster` (Rtn_Monster) | S:86a10 | entry for monsters: just resume the routine. | DEAD | crates/script/src/routine/select.rs:467 |
| `rtn_dead` (Rtn_Dead) | S:86a80 | entry for dead NPCs: AI mode Dead and ZS_Dead. | DEAD | crates/script/src/routine/select.rs:473 |
| `rtn_player` (Rtn_Player) | S:86ba0 | entry for the player entity. Resets AI mode and Ransacked, then picks PS_Ghost (status bit 0x80), else by weapon mode PS_Melee, PS_Ranged, PS_Magic or... | DEAD | crates/script/src/routine/select.rs:480 |
| `travelling_walk_mode` (GetTravellingWalkMode) | S:a9c00 | 2 (fast) for Minecrawler, Scavenger, Wolf, Bison, Rhino and Snapper, 1 otherwise. | DEAD | crates/script/src/routine/time.rs:82 |

### spell effects / casting (19)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `zs_stunned` (ZS_Stunned) | S:5dd70 | the victim stumbles and stands dazed; ZS_Stunned_Loop waits out [STUN_SECS] and ZS_Stunned_End stands up and picks the routine (or the fight) up again... | INTERNAL-ONLY | crates/script/src/magic/area.rs:399 |
| `zs_stunned_loop` (ZS_Stunned_Loop) | S:5e300 | wait until the stun is over, then ZS_Stunned_End. | INTERNAL-ONLY | crates/script/src/magic/area.rs:423 |
| `zs_stunned_end` (ZS_Stunned_End) | S:5e4d0 | stand up; a victim who knows its attacker assesses him (AssessAgressor), otherwise the routine continues. | INTERNAL-ONLY | crates/script/src/magic/area.rs:434 |
| `zs_nothing` (ZS_WaitForMeteor) | S:5a000 | and ZS_WaitForDawn: the DLL's bodies are empty. | INTERNAL-ONLY | crates/script/src/magic/area.rs:599 |
| `is_game` (IsGame) | S:57fc0 | huntable animals (Boar, Bison, Rhino, Deer). | DEAD | crates/script/src/magic/effects.rs:62 |
| `is_large` (IsLarge) | S:57ec0 | the large animals (Minecrawler, Sabertooth, Shadowbeast, Bison, Rhino, Ripper, Alligator); transformations into them last half as long. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:68 |
| `can_transform_into` (CanTransformInto) | S:5ea30 | any animal but a pig; otherwise the hero is told Impossible. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:104 |
| `transform_duration` (GetTransformDuration) | S:582a0 | max(INT - 60, 0) + 60 seconds, halved for a large form; the hero gets x1 (x2 with Perk_Druid, plus 0.5 with Perk_Watermage). | INTERNAL-ONLY | crates/script/src/magic/effects.rs:174 |
| `magic_burn` (MagicBurn) | S:56e60 | set the target on fire. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:203 |
| `magic_freeze` (MagicFreeze) | S:56f30 | freeze the target (ZS_Freeze handles the rest). | INTERNAL-ONLY | crates/script/src/magic/effects.rs:209 |
| `magic_refresh` (MagicRefresh) | S:572d0 | mana to the maximum. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:236 |
| `magic_keep_stamina` (MagicKeepStamina) | S:58b70 | only the hero; the engine parks a "stamina held at maximum" flag on him (it reuses GuardStatus and LastDistToGuardPoint). | INTERNAL-ONLY | crates/script/src/magic/effects.rs:285 |
| `magic_speed_up` (MagicSpeedUp) | S:58a30 | only the hero; he runs fast for (StaminaMax - 200) / 2 + 60 seconds (never less than 60). | INTERNAL-ONLY | crates/script/src/magic/effects.rs:295 |
| `magic_stun` (MagicStun) | S:58cb0 | every character within 12 m, nearest first, that is neither dead nor down, not already stunned, not on guard duty and not in the caster's party gets t... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:306 |
| `magic_dispel` (MagicDispel) | S:53460 | for a damage element (ice 5, fire 4, lightning 6 — the DLL's parameter 4, 5 or 6 picks it) remove the area-spell entities of that element within 25 m ... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:336 |
| `magic_summon_companion` (MagicSummonCompanion) | S:55b90 | the creature named by the spell's Spawn. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:642 |
| `send_perception_transform` (SendPerceptionTransform) | S:53f40 | everybody who can notice the change and had the old body as its current or alternative target now has the new one (the DLL also moves the alignment ta... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:687 |
| `transform_back` (TransformBack) | S:58610 | the transformation ends — the hero (or the animal-formed character) takes his own shape back at the animal's place, the alien body is removed. | WIRED-NOT-EXERCISED | crates/script/src/magic/effects.rs:763 |
| `return_to_body` (ReturnToBody) | S:58470 | the soul returns, the ghost marks are cleared. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:813 |

### ZS_Attack / combat AI (14)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `on_ai_move` (the OnAI_* animation-event callbacks) | S:38580, S:41e30 | the first call after the hit window opened moves the stage counter to 1 (damage strip on). Returns true exactly then. A hack counts a second stage (St... | DEAD | crates/script/src/combat/commands.rs:114 |
| `start_parade` (_AI_Parade) | S:46810, S:46d80 | and OnAI_Parade (S:46d80): the guard goes up (Action = Parade, pose Parade) and holds. Returns the seconds the pose lasts before the callback lowers i... | INTERNAL-ONLY | crates/script/src/combat/commands.rs:138 |
| `start_jump` (_AI_Jump / _AI_JumpBack) | S:46220, S:46560 | charge the stamina; returns the cost. | INTERNAL-ONLY | crates/script/src/combat/commands.rs:158 |
| `start_die` (_AI_Die) | S:3e510 | the dying NPC plays Die and is out of the fight. | WIRED-NOT-EXERCISED | crates/script/src/combat/commands.rs:183 |
| `start_get_up` (_AI_GetUpAttack / _AI_GetUpParade) | S:41bd0, S:41f20 | a knocked-down fighter attacks or parries on its way up; the pose becomes standing and the action the get-up move. | DEAD | crates/script/src/combat/commands.rs:207 |
| `start_reload` (_AI_Reload) | S:4a4f0 | reloading a crossbow restores the action to Reload. | DEAD | crates/script/src/combat/commands.rs:222 |
| `on_arena` (OnArena) | S:11e60 | once the player stands in the arena zone the fighter attacks him with reason Arena. | INTERNAL-ONLY | crates/script/src/combat/misc.rs:54 |
| `wait_free_loop` (ZS_WaitForFreeDestination_Loop) | S:42db0 | every second look whether the player has moved more than 1.5 m away from the destination point; then the routine continues. | INTERNAL-ONLY | crates/script/src/combat/misc.rs:126 |
| `end_goto` (EndGoto) | S:42880 | act on how a goto instruction ended.  Arrival clears "can be pushed while idle". A blocked destination (the player is in the way) makes the NPC wait (... | DEAD | crates/script/src/combat/misc.rs:248 |
| `handle_controlled_return_home` (HandleControlledReturnHome) | S:42bc0 | a controlled (mind-controlled) creature whose master is not the player any more and that stands on its destination becomes a plain creature again (Par... | DEAD | crates/script/src/combat/misc.rs:279 |
| `on_wait_for_player` (OnWaitForPlayer) | S:86d50 | a plain perception poll. | INTERNAL-ONLY | crates/script/src/combat/misc.rs:294 |
| `choose_defensive` (S) | - | S:7030: the defensive-only picker (Parade, JumpBack, Left, Right, Back), skill-gated. | INTERNAL-ONLY | crates/script/src/combat/moves.rs:827 |
| `close_range` (GetCloseRange) | S:4eb70 | stop distance of the approach (600 for the big monsters, else 500). | DEAD | crates/script/src/combat/range.rs:90 |
| `cheer_fight` (CheerFight) | S:28bb0 | a spectator's voice reaction to a seen hit. Friends of the hit fighter groan ("Ooh"), the others cheer; heavy reactions get "HeavyHit". Returns the li... | INTERNAL-ONLY | crates/script/src/combat/react.rs:677 |

### ui (11)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `stop` (gCGUIManager) | Game:0x1007c520 | gCGUIManager::StopSay (Game:0x1007c520): fade out. | WIRED-NOT-EXERCISED ? | crates/ui/src/dialogmenu.rs:147 |
| `calc_uv` (gCMap_PS) | Game:0x101a42e0 | normalised picture coordinates of a world position, None for a degenerate sheet. | WIRED-NOT-EXERCISED | crates/ui/src/map.rs:25 |
| `contains` (gCMap_PS) | Game:0x101a4240 | gCMap_PS::Contains (Game:0x101a4240). | WIRED-NOT-EXERCISED ? | crates/ui/src/map.rs:39 |
| `around` (the rectangle step of gCMap_PS) | Game:0x101a43d0 | the square around the bounding box of points (x, z), side max(dx, dz) * (scale + margin), centred on the box. scale is the gSMapGenerate scale; the en... | WIRED-NOT-EXERCISED ? | crates/ui/src/map.rs:48 |
| `view_window` (the window computation of gCHUDStaticMap's paint) | Game:0x100a87d0 | the map control shows: view_px is the control's client size, tex_px the picture's size, zoom 1 = pixel for pixel when the picture is larger than the c... | INTERNAL-ONLY | crates/ui/src/map.rs:110 |
| `item_rows` (gCHUDTableItemStats) | Game:0x100a3bb0 | row building: the table of info. With against (the worn item) each damage / bonus / value row carries the difference. in_inventory adds the "in invent... | INTERNAL-ONLY | crates/ui/src/stats.rs:104 |
| `print` (gCScreenOutputCtrl) | Game:0x10077630 | replace the text, show the window and start the timer. | WIRED-NOT-EXERCISED ? | crates/ui/src/texteffect.rs:193 |
| `print_end` (gCScreenOutputCtrl) | Game:0x10077610 | gCScreenOutputCtrl::PrintScreenEnd (Game:0x10077610). | INTERNAL-ONLY | crates/ui/src/texteffect.rs:208 |
| `quest_success` (gCScreenOutputCtrl) | Game:0x10077c20 | the quest title in white. | DEAD | crates/ui/src/texteffect.rs:214 |
| `quest_failed` (gCScreenOutputCtrl) | Game:0x10077cc0 | gCScreenOutputCtrl::PrintScreen_Quest_Failed (Game:0x10077cc0). | DEAD | crates/ui/src/texteffect.rs:221 |
| `info_say` (gCScreenOutputCtrl) | Game:0x10077880 | the spoken line, white for the player and yellow for everybody else. | DEAD | crates/ui/src/texteffect.rs:227 |

### PS_Magic* (8)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `ps_magic_staff_parade` (PS_Magic_StaffParade) | S:7c630, S:7c8c0 | and PS_Magic_StaffParade_Loop (S:7c8c0): with the staff the mage parries (_AI_Parade); the loop leaves it when a cast key comes. | INTERNAL-ONLY | crates/script/src/player/magic.rs:349 |
| `bs_book` (BS_BookOfFlood) | S:65180, S:656f0 | BS_BookOfRhobar (S:656f0) and BS_BookOfZuben: after half a second the hero reads two pages aloud (BookOf<Name>_<page>, the page counter advances by tw... | INTERNAL-ONLY | crates/script/src/player/magic.rs:480 |
| `bs_ancient_wisdom` (BS_AncientWisdom) | S:65080 | the bookstand of the ancient wisdom picks the book the hero has not finished: Flood, then Rhobar, then Zuben, and when all are read BS_NothingNew. | INTERNAL-ONLY | crates/script/src/player/magic.rs:526 |
| `bs_nothing_new` (BS_NothingNew) | S:64da0 | half a second, then one of three remarks (NothingNew_00..02), and on with the routine. | INTERNAL-ONLY | crates/script/src/player/magic.rs:538 |
| `bs_skill_book` (BS_Alchemy) | S:66a00 | and BS_Smith: after half a second a remark (AlchemyBook, SmithBook) and +3 Alchemy or Smithing (+5 without ExtendedContent); the stand is used up. | INTERNAL-ONLY | crates/script/src/player/magic.rs:560 |
| `bs_chronicles` (BS_Chronicles) | S:66190 | the Dargoth chronicles are read once; the game event Dargoth_Chronicles marks it. The five remarks and the quest hooks of the DLL are story data [U]; ... | INTERNAL-ONLY | crates/script/src/player/magic.rs:589 |
| `ps_ghost_jump` (PS_Ghost_Jump) | S:67a70 | one jump, then back to the loop. | INTERNAL-ONLY | crates/script/src/player/magic.rs:653 |
| `ps_ghost_summoner` (PS_GhostSummoner) | S:67c80 | and PS_GhostSummoner_Loop: the body the soul left behind is abandoned: hands down, standing, idle. | INTERNAL-ONLY | crates/script/src/player/magic.rs:675 |

### damage pipeline (7)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `assess_cmd_attack` (AssessCmdAttack) | S:29e70 | a companion attacks what the player focuses. | INTERNAL-ONLY | crates/script/src/assess.rs:85 |
| `send_perception_cmd_attack` (SendPerceptionCmdAttack) | S:31ff0 | every companion of the player hears the order. | DEAD | crates/script/src/assess.rs:96 |
| `assess_cmd_follow` (AssessCmdFollow) | S:56a80 | a companion that waits stops waiting and follows again. | INTERNAL-ONLY | crates/script/src/assess.rs:107 |
| `send_perception_cmd_follow` (SendPerceptionCmdFollow) | S:32170 | SendPerceptionCmdFollow (S:32170). | DEAD | crates/script/src/assess.rs:118 |
| `assess_player_enter_zone` (AssessPlayerEnterZone) | S:30000 | the player stepped into a zone with an owner within 7 m of the witness: a calm witness goes and observes (ZS_ObserveZonePlayer). | INTERNAL-ONLY | crates/script/src/assess.rs:370 |
| `send_perception_enter_zone` (SendPerceptionEnterZone) | S:33170 | SendPerceptionEnterZone (S:33170). | DEAD | crates/script/src/assess.rs:391 |
| `send_perception_preach` (SendPerceptionPreach) | S:32cf0 | calm humanoids within 10 m of a preaching NPC stop and listen (ZS_Listen). | DEAD | crates/script/src/assess.rs:429 |

### audio sequencers (6)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `step` (eCAudioEmitter_PS) | Engine:0x10057ef0 | decision part. playing tells whether the emitter's channel still sounds; nothing new starts before it stopped. | WIRED-NOT-EXERCISED ? | crates/audio/src/emitter.rs:119 |
| `select_sample` (eCResourceSoundList) | Engine:0x101bbb20, Game:0x10057f40 | and gCEffectCommandPlaySound::SelectSample (Game:0x10057f40): entry i is drawn with probability w_i / (N * max_w), so the remainder of the unit interv... | INTERNAL-ONLY | crates/audio/src/emitter.rs:190 |
| `voice_path` (gCEffectCommandPlayVoice) | Game:0x10059030 | the literal token (voice) is replaced by the speaker's Voice string and the whole path is lower-cased. | DEAD | crates/audio/src/emitter.rs:211 |
| `viseme_for_label` (eCVisualAnimationLipSync) | Engine:0x10027650 | case-insensitive lookup; unknown labels give the rest viseme. | DEAD | crates/audio/src/lipsync.rs:166 |
| `set_accept_triggers` (gCMusicSequencer) | - | gCMusicSequencer::SetAcceptTriggers: while off every trigger is ignored. | DEAD | crates/audio/src/sequencer.rs:327 |
| `execute_stop` (Execute) | - | all channels stop, events are cleared. | INTERNAL-ONLY | crates/audio/src/sequencer.rs:750 |

### render rules (6)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `area_factor` (eCFrustumFilter) | - | the projected area of the bounding sphere relative to the screen, pi * (r / z)^2 clamped to [0, 1], with z the depth along the camera forward axis (no... | DEAD | crates/render-rules/src/lib.rs:14 |
| `lod_input` (eCFrustumDatabase) | - | LoD input is the database's global LoD factor x area factor x entity LoD scale x entity-type LoD scale. | DEAD | crates/render-rules/src/lib.rs:23 |
| `lod_stage` (eCRenderContextMesh) | - | stages is the number of LoD meshes the visual has (0 or 1 means always stage 0). | DEAD | crates/render-rules/src/lib.rs:36 |
| `small_object_fade` (the non-ranged branch of eCFrustumFilter) | - | threshold = entity scale x database screen-cull factor x entity-type scale. Returns the fade-out value in [0, 1]; anything below 1 means the object is... | DEAD | crates/render-rules/src/lib.rs:86 |
| `intensity` (eCStaticPointLight_PS) | - | with t = d / range the smooth fall-off 2 (1 - 3 t^2 + 2 t^3) times the cosine of the incidence angle (param_2 = d cos). 0 beyond the range. | WIRED-NOT-EXERCISED | crates/render-rules/src/lib.rs:188 |
| `packet` (eCIlluminationAdmin) | - | collect lights whose range box touches the object's box (at most 10 candidates), rank by weight (descending), keep three, write position, colour x 2 a... | WIRED-NOT-EXERCISED ? | crates/render-rules/src/lib.rs:200 |

### perception / crime (6)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `get_player_crime` (GetPlayerCrime) | S:36fd0 | the crime the NPC remembers. | DEAD | crates/script/src/crime.rs:33 |
| `is_audible` (FUN_10009100) | - | "is audible": the entity is neither sneaking (Sneak or Parade pose) nor standing still. | INTERNAL-ONLY | crates/script/src/perception.rs:43 |
| `call_help` (CallHelp) | S:332e0 | caller was wronged by culprit; guards that are friendly toward the caller, not friendly toward the culprit and within 10 m of the culprit attack it wi... | WIRED-NOT-EXERCISED ? | crates/script/src/perception.rs:263 |
| `on_sleep` (OnSleep) | S:86dc0 | the callback of a sleeping NPC: wake into ZS_ReactToFootsteps when the player, whom the sleeper does not like, is audible, in clear view, within 10 m ... | INTERNAL-ONLY | crates/script/src/perception.rs:357 |
| `on_animal_sleep` (OnAnimalSleep) | S:89f20 | the callback of a sleeping animal: the player wakes it by coming close unless hidden. With Perk_Sneak_2 a sneaking player (or one in PS_Ranged) is nev... | INTERNAL-ONLY | crates/script/src/perception.rs:384 |
| `on_gate_guard` (OnGateGuard) | S:86c80 | the local callback of a gate guard: returns true once per [GATE_GUARD_POLL_SECS] of state time, when the caller should run the active perception scan. | WIRED-NOT-EXERCISED | crates/script/src/perception.rs:415 |

### PS_Interact boulder (4)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `can_handle` (OB_Light) | S:777b0, S:778e0 | OB_Medium (S:778e0), OB_Heavy (S:77a10) and OB_Faring_Boulder (S:77b40): the strength check of the stage the hero asks for. A weaker hero says TooHeav... | INTERNAL-ONLY | crates/script/src/player/boulder.rs:75 |
| `ps_boulder` (PS_Interact_TiltOrcBoulder) | S:765b0, S:76c70 | PS_Interact_HoldOrcBoulder (S:76c70) and PS_Interact_LiftOrcBoulder (S:77210): hands empty (tilting), the strength check of the stage (failure: back t... | INTERNAL-ONLY | crates/script/src/player/boulder.rs:110 |
| `ps_boulder_loop` (the three boulder loops) | S:76950 | hold the pose; the use key asks for the next stage, the exit key ends the interaction. A lift pays the first-time Strength bonus. | INTERNAL-ONLY | crates/script/src/player/boulder.rs:145 |
| `ps_carry_stone` (PS_Interact_CarryStone) | S:77cc0 | take the stone in both hands and walk (movement mode 3); PS_Interact_CarryStone_Loop waits for the exit key and PS_Interact_CarryStone_End puts it dow... | INTERNAL-ONLY | crates/script/src/player/boulder.rs:178 |

### PS_Interact* (3)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `is_basket` (IsBasket) | S:782e0 | the hero just opened a chest (action OpenChest) and the object is one of the harvestable "ChestPod" baskets, not a real chest. | INTERNAL-ONLY | crates/script/src/player/interact.rs:562 |
| `ps_interact_wait_for_exit` (PS_Interact_WaitForExit) | - | PS_Interact_WaitForExit: wait for the exit key, then stop and leave the interaction. | INTERNAL-ONLY | crates/script/src/player/interact.rs:571 |
| `ps_interact_talk` (PS_Interact_Shrine) | S:72600, S:78210 | and PS_Interact_Talk (S:78210): kneel (shrine) and start the conversation with the object. When nobody answers the interaction ends. | INTERNAL-ONLY | crates/script/src/player/interact.rs:799 |

### PS_Normal / Sneak / Swim (3)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `ps_gonna_loot` (PS_GonnaLoot) | S:83120 | stop and open the loot window of the focused body. | INTERNAL-ONLY | crates/script/src/player/normal.rs:144 |
| `ps_jump` (PS_*_Jump) | S:825f0 | pay the stamina, jump, return to the loop of the weapon mode. | INTERNAL-ONLY | crates/script/src/player/normal.rs:153 |
| `ps_sneak` (PS_*_Sneak) | - | PS_*_Sneak: switch between the sneaking and the standing pose. | INTERNAL-ONLY | crates/script/src/player/normal.rs:176 |

### PS_Ranged* (bow, crossbow) (3)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `ps_ranged_jump` (PS_Ranged_Jump) | - | PS_Ranged_Jump: _AI_Jump, then the walk loop again (the task default re-readies the weapon; nothing of it is lost). | INTERNAL-ONLY | crates/script/src/player/ranged.rs:477 |
| `ps_ranged_sneak` (PS_Ranged_Sneak) | S:860c0 | the sneak toggle. A crossbow hides the crosshair and sneaks in PS_Ranged_CrossBowSneak_Loop, a bow goes back to PS_Ranged_Walk_Loop. | INTERNAL-ONLY | crates/script/src/player/ranged.rs:500 |
| `ps_ranged_crossbow_sneak` (PS_Ranged_CrossBowSneak_Loop) | S:86320 | sneaking with a crossbow reads the same queue as the walk loop. | INTERNAL-ONLY | crates/script/src/player/ranged.rs:522 |

### fx / particles (2)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `scroller` (eCTexCoordSrcScroller) | Engine:0x102628f0 | the offset is the fractional part of direction * rate * time (wrapping keeps float precision over long sessions), added to the authored ScrollOffset. | INTERNAL-ONLY | crates/fx/src/matadmin.rs:47 |
| `trigger` (eCParticle_PS) | Engine:0x1014ef20 | queue count extra particles. | WIRED-NOT-EXERCISED ? | crates/fx/src/sim.rs:187 |

### down / knock-out (2)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `fall_damage` (the fall-damage rule of OnMovementModeChanged) | S:adbf0 | floor(160 * (t - 0.9)^2) for a fall of t seconds, 0 up to 0.9 s. | WIRED-NOT-EXERCISED ? | crates/script/src/down.rs:318 |
| `land_from_fall` (the landing rule of OnMovementModeChanged) | S:adbf0 | apply fall damage after t seconds in the ZS_Fall state.  Nothing happens for ghosts (status 0x80), the god-mode player and Bloodflies. Perk_Acrobatic ... | DEAD | crates/script/src/down.rs:329 |

### PS_Interact craft (2)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `fry_stack` (OnPlayerFryStack) | S:6a740 | fry amount pieces of raw meat. Items that cannot be fried are left alone; returns the number fried. | INTERNAL-ONLY | crates/script/src/player/craft.rs:84 |
| `transform_stack` (OnPlayerTransformStack) | S:6a8d0 | a transform request on the barbecue is frying (the only transformation the hero's HUD offers); the stacks are rearranged afterwards by the host. | DEAD | crates/script/src/player/craft.rs:104 |

### nav (1)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `choose` (gCNavigationMap) | - | gCNavigationMap::DetectFinalTargetAsOffset: among the poses that lie inside the zone the character is in, take the one nearest to the character. | WIRED-NOT-EXERCISED ? | crates/nav/src/agent.rs:198 |

### PS_Melee* (1)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `ps_melee_jump_back` (PS_Melee_JumpBack) | S:7fc20 | _AI_JumpBack against the opponent, no alignment. | INTERNAL-ONLY | crates/script/src/player/melee.rs:312 |

### sim (stats, combat, AI, host) (1)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `apply_stat` (the player memory entry points) | Game:0x14efa0 | apply an item modifier event to the hero. Characters that use the NPC formulas have no attribute store and ignore it. | INTERNAL-ONLY | crates/sim/src/actor.rs:417 |

## Ported functions called in play (calls over all scenarios)

| function | calls | scenarios | file |
| --- | ---: | --- | --- |
| `color_factor` | 118285094 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/sim.rs:54 |
| `calc_color` | 118285094 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/sim.rs:489 |
| `tick_particle` | 118131039 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/sim.rs:308 |
| `color_scale` | 113927236 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/scale.rs:80 |
| `blend_factor` | 72041884 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/environment/src/lib.rs:241 |
| `vector_scale` | 69510720 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/scale.rs:62 |
| `calc_frame` | 35424799 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/sim.rs:96 |
| `is_linked` | 21990897 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/stack.rs:152 |
| `can_do` | 7210604 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/combat/moves.rs:232 |
| `update` | 4922659 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/sim.rs:202 |
| `float_scale` | 4412535 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/scale.rs:52 |
| `process` | 4112623 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/effect.rs:33 |
| `has_finished` | 4112623 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/effect.rs:49 |
| `spawn` | 3433260 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/sim.rs:378 |
| `process` | 2975779 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/sim.rs:254 |
| `is_enabled` | 1834882 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/sector.rs:145 |
| `routine_time` | 1674686 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/routine/time.rs:44 |
| `add_items` | 1596158 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/list.rs:436 |
| `is_playing` | 1583796 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/blend.rs:310 |
| `run_stop_prevention` | 1568291 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/blend.rs:393 |
| `new` | 1479948 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/stack.rs:82 |
| `process` | 1447327 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/attach.rs:138 |
| `class_priority` | 1427715 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/focus.rs:161 |
| `is_valid` | 1427715 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/focus.rs:173 |
| `create_items` | 1389555 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/list.rs:474 |
| `boost` | 1050732 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/physics/src/relax.rs:8 |
| `in_range2` | 995048 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/focus.rs:180 |
| `focus_check` | 995048 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/focus.rs:354 |
| `angles` | 976195 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/focus.rs:246 |
| `in_angles` | 968965 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/focus.rs:188 |
| `at` | 902656 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/formats/src/weather.rs:35 |
| `at@2` | 902656 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/formats/src/weather.rs:48 |
| `reset` | 578210 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/sim.rs:167 |
| `species_can_sidestep` | 479234 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/combat/moves.rs:190 |
| `max_range` | 349175 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/focus.rs:132 |
| `political_attitude` | 339483 | bow_faithful, combat_basic, controls_basic, melee_faithful, npc_archer | crates/script/src/predicates.rs:489 |
| `priority` | 306412 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/focus.rs:196 |
| `page_flags` | 250695 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/ui/src/manager.rs:89 |
| `find_focus` | 232750 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/focus.rs:383 |
| `clip_fog` | 225542 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/environment/src/lib.rs:392 |
| `get_attitude` | 216710 | bow_faithful, bow_pose, combat_basic, controls_basic, dialog_quest, ma | crates/script/src/attitude.rs:171 |
| `outlaw_attitude` | 215875 | bow_faithful, bow_pose, combat_basic, controls_basic, dialog_quest, ma | crates/script/src/attitude.rs:17 |
| `item_gold_value` | 210000 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/stack.rs:260 |
| `combined_quality` | 201384 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/stack.rs:254 |
| `calculate_rect_indices` | 199697 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/pvs.rs:96 |
| `process` | 199697 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/pvs.rs:257 |
| `start_due_events` | 122728 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/sequencer.rs:642 |
| `environment` | 122634 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/ambient.rs:134 |
| `dominant_environment` | 122634 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/ambient.rs:168 |
| `process` | 122634 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/ambient.rs:221 |
| `update_envelope` | 122634 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/sequencer.rs:818 |
| `update_channels` | 122634 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/sequencer.rs:862 |
| `pick_point` | 122634 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/camera/src/aim.rs:32 |
| `tick` | 122634 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/ui/src/texteffect.rs:239 |
| `water_beds` | 122633 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/ambient_play.rs:168 |
| `bone` | 122633 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/slot.rs:172 |
| `draw_multiplier` | 122633 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/projectile/src/aim.rs:31 |
| `from_state` | 112832 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/environment/src/lib.rs:51 |
| `apply` | 112832 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/environment/src/lib.rs:175 |
| `look_counts` | 112810 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/input/src/control.rs:113 |
| `sun_state` | 112771 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/environment/src/lib.rs:323 |
| `hemisphere_sh` | 112771 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/environment/src/lib.rs:403 |
| `weather_light_terms` | 112771 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/environment/src/lib.rs:423 |
| `score_move` | 102627 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, sector_switch, sha | crates/script/src/combat/moves.rs:436 |
| `offer` | 102627 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, sector_switch, sha | crates/script/src/combat/moves.rs:653 |
| `has_lower_priority` | 99311 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/focus.rs:331 |
| `cull` | 96977 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/admin.rs:183 |
| `update_child_dependencies` | 78704 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/entity.rs:389 |
| `update_processing_range` | 77063 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/runtime/src/core.rs:752 |
| `process` | 77063 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/admin.rs:300 |
| `flush_kills` | 77063 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/admin.rs:455 |
| `species_attitude` | 75213 | bow_faithful, combat_basic, controls_basic, melee_faithful, npc_orc1,  | crates/script/src/attitude.rs:33 |
| `weight_sphere` | 74100 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/render-rules/src/lib.rs:165 |
| `recalibrate` | 59811 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/pvs.rs:379 |
| `category` | 52660 | lockpick, loot_chest, trade_click, trade_esc, world_chest | crates/gameplay/src/inventory/stack.rs:110 |
| `is_humanoid` | 51805 | combat_basic, melee_faithful, npc_archer, npc_orc1, npc_vs_npc, routin | crates/script/src/predicates.rs:21 |
| `generate_plunder` | 48000 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/treasure.rs:138 |
| `generate_plunder_inventory` | 42640 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/treasure.rs:332 |
| `process` | 38992 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/trigger.rs:932 |
| `hand_class` | 33561 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/helpers.rs:44 |
| `weapon_mode` | 33545 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/helpers.rs:103 |
| `compare` | 26330 | lockpick, loot_chest, trade_click, trade_esc, world_chest | crates/gameplay/src/inventory/order.rs:242 |
| `is_in_fist_mode` | 25969 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, routine_zoo, secto | crates/script/src/predicates.rs:282 |
| `compare_focus_name` | 25533 | lockpick, loot_chest, trade_click, trade_esc, world_chest | crates/gameplay/src/inventory/order.rs:163 |
| `can_receive_perception_from` | 24184 | combat_basic, crime_theft, doors, lockpick, loot_chest, melee_faithful | crates/script/src/perception.rs:50 |
| `dst_volume` | 19136 | combat_basic, dialog_quest, melee_faithful, npc_archer, npc_orc1, npc_ | crates/audio/src/music.rs:73 |
| `src_volume` | 19081 | combat_basic, dialog_quest, melee_faithful, npc_archer, npc_orc1, npc_ | crates/audio/src/music.rs:62 |
| `route` | 18601 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, dia | crates/input/src/player.rs:246 |
| `magic_input` | 18601 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, dia | crates/input/src/player.rs:507 |
| `is_boss` | 18512 | bow_faithful, combat_basic, controls_basic, melee_faithful, npc_archer | crates/script/src/predicates.rs:328 |
| `play_slot` | 18297 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/animator.rs:134 |
| `new` | 18297 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/blend.rs:272 |
| `assess_fight_sound` | 16849 | combat_basic, melee_faithful, npc_archer, npc_orc1, npc_vs_npc, sector | crates/script/src/assess.rs:165 |
| `on_attack_turn` | 16378 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, routine_zoo, secto | crates/script/src/combat/attack.rs:653 |
| `get_cast_action` | 16041 | bow_pose, magic_faithful, spell_zoo, teleport | crates/script/src/magic/cast.rs:64 |
| `switch_fade_out` | 15505 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/animator.rs:161 |
| `switch_fade_out` | 15505 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/blend.rs:353 |
| `combat_move_length` | 13781 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, routine_zoo, secto | crates/script/src/combat/range.rs:7 |
| `attack_range` | 13781 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, routine_zoo, secto | crates/script/src/combat/range.rs:76 |
| `select_combat_move` | 12545 | combat_basic, melee_faithful, npc_archer, npc_orc1, npc_vs_npc, sector | crates/script/src/combat/select.rs:27 |
| `ps_normal_loop` | 11581 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/player/normal.rs:101 |
| `evaluate` | 10507 | combat_basic, controls_basic, dialog_quest, lockpick, melee_faithful,  | crates/animation/src/face.rs:201 |
| `on_attack_goto` | 9745 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, routine_zoo, secto | crates/script/src/combat/attack.rs:530 |
| `follow_time` | 9710 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, sector_switch, sha | crates/script/src/predicates.rs:182 |
| `fai_active` | 7606 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, sector_switch, sha | crates/script/src/combat/select.rs:67 |
| `find_other_target` | 7543 | combat_basic, melee_faithful, npc_archer, npc_orc1, npc_vs_npc, routin | crates/script/src/combat/select.rs:277 |
| `build_situation` | 7512 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, sector_switch, sha | crates/script/src/combat/moves.rs:86 |
| `crowd_on_target` | 7512 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, sector_switch, sha | crates/script/src/combat/moves.rs:130 |
| `aggression_weight` | 7512 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, sector_switch, sha | crates/script/src/combat/moves.rs:147 |
| `on_attack` | 7145 | combat_basic, melee_faithful, npc_archer, npc_orc1, npc_vs_npc, routin | crates/script/src/combat/attack.rs:44 |
| `can_parade_move_of` | 6849 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, sector_switch, sha | crates/script/src/damage.rs:587 |
| `pick` | 6845 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, sector_switch, sha | crates/script/src/combat/moves.rs:697 |
| `choose_full` | 6839 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, sector_switch, sha | crates/script/src/combat/moves.rs:773 |
| `is_ambient_creature` | 5328 | combat_basic, melee_faithful, routine_zoo, sector_switch, shaman_duel, | crates/script/src/predicates.rs:55 |
| `memorize_player_crime` | 5310 | combat_basic, melee_faithful, sector_switch, shaman_duel, spell_zoo | crates/script/src/crime.rs:63 |
| `can_attack` | 5310 | combat_basic, melee_faithful, sector_switch, shaman_duel, spell_zoo | crates/script/src/perception.rs:83 |
| `set_attack_reason` | 5309 | combat_basic, melee_faithful, sector_switch, shaman_duel, spell_zoo | crates/script/src/crime.rs:39 |
| `assess_target` | 5309 | combat_basic, melee_faithful, sector_switch, shaman_duel, spell_zoo | crates/script/src/perception.rs:95 |
| `assess_attack` | 5309 | combat_basic, melee_faithful, sector_switch, shaman_duel, spell_zoo | crates/script/src/perception.rs:151 |
| `attitude_to_player` | 4974 | bow_faithful, bow_pose, combat_basic, controls_basic, dialog_quest, ma | crates/script/src/attitude.rs:108 |
| `list_count` | 4910 | trade_click, trade_esc | crates/gameplay/src/trade.rs:23 |
| `set_transaction_amount` | 4508 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/list.rs:1302 |
| `set_transaction_amount` | 4508 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/stack.rs:278 |
| `is_guard` | 4431 | bow_faithful, combat_basic, controls_basic, melee_faithful, npc_archer | crates/script/src/predicates.rs:334 |
| `start_stand_up` | 4367 | combat_basic, melee_faithful, menu_save_load, npc_archer, npc_orc1, np | crates/script/src/combat/commands.rs:149 |
| `wait_for_flee_loop` | 4277 | routine_zoo, sector_switch, spell_zoo | crates/script/src/combat/attack.rs:681 |
| `notify_enclave` | 3990 | routine_reentry, routine_zoo, sector_switch, shaman_duel, spell_zoo | crates/script/src/crime.rs:214 |
| `on_watch_fight` | 3766 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, sector_switch | crates/script/src/combat/react.rs:498 |
| `fai_passive` | 3697 | sector_switch, spell_zoo | crates/script/src/combat/select.rs:132 |
| `pool` | 3652 | bow_pose, combat_basic, magic_faithful, melee_faithful, npc_archer, np | crates/script/src/stats.rs:171 |
| `lookup` | 3646 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/efm.rs:97 |
| `max_angles` | 3615 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, doo | crates/gameplay/src/focus.rs:145 |
| `pool_percent` | 3594 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/stats.rs:218 |
| `read_grid` | 3200 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/formats/src/terrain/vegetation.rs:247 |
| `start` | 3080 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/effect.rs:17 |
| `choose_weapon_mode` | 2903 | combat_basic, melee_faithful, npc_archer, npc_orc1, npc_vs_npc, sector | crates/script/src/combat/weapon.rs:52 |
| `select_weapon_mode` | 2903 | combat_basic, melee_faithful, npc_archer, npc_orc1, npc_vs_npc, sector | crates/script/src/combat/weapon.rs:163 |
| `cache_in` | 2742 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/entity.rs:517 |
| `trade_unit_price` | 2728 | trade_click, trade_esc | crates/sim/src/trade.rs:46 |
| `hang_around` | 2301 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/routine/work.rs:1827 |
| `change_enclave_status` | 2182 | sector_switch, spell_zoo | crates/script/src/crime.rs:158 |
| `add` | 2178 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/attach.rs:45 |
| `remove` | 2178 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/attach.rs:76 |
| `update` | 1894 | bow_faithful, bow_pose, shaman_duel | crates/projectile/src/aim.rs:154 |
| `accumulate_key_direction` | 1835 | bow_pose, combat_basic, focus_steer, melee_faithful | crates/gameplay/src/focus.rs:281 |
| `primary` | 1774 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, dia | crates/input/src/player.rs:324 |
| `ps_magic_loop` | 1571 | bow_pose, magic_faithful, spell_zoo, teleport | crates/script/src/player/magic.rs:104 |
| `find_sounds` | 1512 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/ambient.rs:144 |
| `select_sound` | 1512 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/ambient.rs:157 |
| `crosshair_size` | 1508 | bow_faithful, bow_pose, shaman_duel | crates/projectile/src/aim.rs:37 |
| `task_for_use_type` | 1187 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/routine/select.rs:68 |
| `select_routine_task` | 1187 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/routine/select.rs:99 |
| `speed_cm_s` | 1185 | bow_faithful, bow_pose, magic_faithful, npc_archer, sector_switch, sha | crates/projectile/src/params.rs:339 |
| `fighters_on` | 1113 | combat_basic, melee_faithful, npc_archer, npc_orc1, npc_vs_npc, routin | crates/script/src/combat/attack.rs:136 |
| `update_combat_state` | 1111 | combat_basic, melee_faithful, npc_archer, npc_orc1, npc_vs_npc, sector | crates/script/src/combat/attack.rs:151 |
| `can_move` | 1091 | trade_click, trade_esc | crates/gameplay/src/trade.rs:57 |
| `is_disposable` | 1091 | trade_click, trade_esc | crates/gameplay/src/inventory/stack.rs:189 |
| `continue_routine` | 1078 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/routine/select.rs:366 |
| `weakness` | 1040 | bow_faithful, bow_pose, combat_basic, magic_faithful, melee_faithful,  | crates/script/src/damage.rs:122 |
| `start_cast_phase` | 894 | bow_pose, magic_faithful, shaman_duel, spell_zoo, teleport | crates/script/src/magic/cast.rs:147 |
| `intro` | 875 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/blend.rs:116 |
| `extro` | 875 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/blend.rs:125 |
| `steer` | 853 | bow_faithful, bow_pose, magic_faithful, npc_archer, sector_switch, sha | crates/projectile/src/flight.rs:240 |
| `observe_loop` | 791 | combat_basic, routine_zoo, teleport | crates/script/src/combat/react.rs:742 |
| `is_in_parade_mode` | 693 | bow_pose, combat_basic, focus_steer, melee_faithful, npc_orc1, npc_vs_ | crates/script/src/damage.rs:631 |
| `level` | 610 | combat_basic, melee_faithful, npc_archer, npc_orc1, npc_vs_npc, sector | crates/script/src/stats.rs:24 |
| `on_attack_go_back` | 595 | combat_basic, melee_faithful, npc_vs_npc, sector_switch, spell_zoo | crates/script/src/combat/attack.rs:622 |
| `assess_murder` | 577 | shaman_duel, spell_zoo | crates/script/src/assess.rs:245 |
| `on_enter_processing_range` | 576 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/trigger.rs:451 |
| `on_cache_in` | 576 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/trigger.rs:472 |
| `ps_melee_loop` | 566 | bow_pose, combat_basic, focus_steer, melee_faithful | crates/script/src/player/melee.rs:160 |
| `animation_speed` | 553 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, routine_zoo, secto | crates/script/src/combat/commands.rs:263 |
| `has_dangerous_weapon` | 542 | combat_basic, melee_faithful, routine_zoo, teleport | crates/script/src/predicates.rs:304 |
| `on_react_to_weapon` | 540 | combat_basic, melee_faithful, routine_zoo, teleport | crates/script/src/combat/react.rs:185 |
| `end_time` | 516 | bow_faithful, bow_pose, combat_basic, magic_faithful, melee_faithful,  | crates/fx/src/efm.rs:75 |
| `assess_use_interact_object` | 508 | doors, lockpick, loot_chest, world_chest, world_doors | crates/script/src/assess.rs:336 |
| `fai_ranged` | 494 | npc_archer, sector_switch, spell_zoo | crates/script/src/combat/select.rs:185 |
| `level_max` | 460 | combat_basic, melee_faithful, npc_archer, npc_orc1, npc_vs_npc, sector | crates/script/src/stats.rs:31 |
| `power_rank` | 442 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, sector_switch, spe | crates/script/src/damage.rs:225 |
| `fai_misted` | 430 | spell_zoo | crates/script/src/combat/select.rs:253 |
| `do_logical_damage` | 426 | bow_faithful, bow_pose, combat_basic, magic_faithful, melee_faithful,  | crates/script/src/damage.rs:1026 |
| `can_see` | 421 | combat_basic, crime_theft, doors, lockpick, routine_zoo, world_chest,  | crates/script/src/perception.rs:29 |
| `assure_items` | 389 | craft_anvil, npc_archer, quick_slots, routine_zoo, sector_switch, slee | crates/gameplay/src/inventory/list.rs:511 |
| `start_stumble` | 368 | combat_basic, melee_faithful, npc_archer, npc_orc1, npc_vs_npc, sector | crates/script/src/combat/commands.rs:189 |
| `use_type` | 354 | trade_click, trade_esc | crates/gameplay/src/inventory/stack.rs:116 |
| `attribute` | 330 | combat_basic, melee_faithful, npc_archer, npc_orc1, npc_vs_npc, sector | crates/script/src/stats.rs:87 |
| `armor_protection` | 327 | combat_basic, melee_faithful, npc_archer, npc_orc1, npc_vs_npc, sector | crates/script/src/damage.rs:162 |
| `assess_hit` | 327 | combat_basic, melee_faithful, npc_archer, npc_orc1, npc_vs_npc, sector | crates/script/src/damage.rs:712 |
| `protection` | 327 | combat_basic, melee_faithful, npc_archer, npc_orc1, npc_vs_npc, sector | crates/script/src/stats.rs:121 |
| `shoot` | 322 | bow_faithful, bow_pose, magic_faithful, npc_archer, sector_switch, sha | crates/projectile/src/flight.rs:256 |
| `fai_magic` | 318 | shaman_duel, spell_zoo | crates/script/src/combat/select.rs:204 |
| `send_perception_fight_sound` | 313 | combat_basic, melee_faithful, npc_archer, npc_orc1, npc_vs_npc, sector | crates/script/src/assess.rs:233 |
| `combat_walk_mode` | 312 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, routine_zoo, secto | crates/script/src/combat/range.rs:104 |
| `set_pool` | 311 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, routine_zoo, secto | crates/script/src/stats.rs:237 |
| `begin_melee_command` | 310 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, routine_zoo, secto | crates/script/src/combat/commands.rs:423 |
| `add_stamina_points` | 307 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, routine_zoo, secto | crates/script/src/stats.rs:327 |
| `start_say_aargh` | 282 | combat_basic, melee_faithful, npc_archer, npc_orc1, npc_vs_npc, sector | crates/script/src/speech.rs:208 |
| `current_level` | 274 | combat_basic, melee_faithful, npc_archer, npc_orc1, npc_vs_npc, sector | crates/script/src/stats.rs:45 |
| `movement_mode_changed` | 230 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/sim/src/moves.rs:67 |
| `move_strength` | 221 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, sector_switch, spe | crates/script/src/damage.rs:201 |
| `defence_value` | 221 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, sector_switch, spe | crates/script/src/damage.rs:260 |
| `can_parade` | 221 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, sector_switch, spe | crates/script/src/damage.rs:637 |
| `shoot` | 221 | bow_faithful, bow_pose, npc_archer, sector_switch, shaman_duel, spell_ | crates/script/src/player/ranged.rs:35 |
| `ps_ranged_aim_loop` | 218 | bow_faithful, bow_pose | crates/script/src/player/ranged.rs:283 |
| `start_shoot` | 212 | npc_archer, routine_zoo, sector_switch, spell_zoo | crates/script/src/combat/commands.rs:171 |
| `npc_arrow` | 211 | npc_archer, sector_switch, spell_zoo | crates/projectile/src/launch.rs:52 |
| `find_fragment` | 189 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/sequencer.rs:440 |
| `select` | 189 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/sequencer.rs:484 |
| `assess_warn` | 189 | combat_basic, melee_faithful, routine_zoo | crates/script/src/assess.rs:37 |
| `can_be_killed` | 186 | combat_basic, melee_faithful, npc_vs_npc, sector_switch, shaman_duel,  | crates/script/src/damage.rs:376 |
| `add_mana_points` | 180 | bow_pose, magic_faithful, shaman_duel, spell_zoo, teleport | crates/script/src/stats.rs:312 |
| `compare_type` | 176 | trade_click, trade_esc | crates/gameplay/src/inventory/order.rs:77 |
| `build_lipsync_clip` | 166 | combat_basic, controls_basic, dialog_quest, lockpick, melee_faithful,  | crates/animation/src/face.rs:92 |
| `draw_multiplier` | 164 | bow_faithful, bow_pose, shaman_duel | crates/script/src/player/ranged.rs:24 |
| `ps_ranged_power_aim` | 164 | bow_faithful, bow_pose, shaman_duel | crates/script/src/player/ranged.rs:311 |
| `cast_begin` | 158 | bow_pose, magic_faithful, shaman_duel, spell_zoo, teleport | crates/script/src/magic/cast.rs:245 |
| `cast_execute` | 157 | bow_pose, magic_faithful, shaman_duel, spell_zoo, teleport | crates/script/src/magic/cast.rs:269 |
| `ps_magic` | 156 | bow_pose, magic_faithful, spell_zoo, teleport | crates/script/src/player/magic.rs:66 |
| `exit_time` | 149 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/sequencer.rs:563 |
| `find_transition` | 149 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/sequencer.rs:598 |
| `sleep_loop` | 147 | routine_zoo | crates/script/src/routine/work.rs:1549 |
| `compare_name` | 144 | trade_click, trade_esc | crates/gameplay/src/inventory/order.rs:174 |
| `compare_spell` | 144 | trade_click, trade_esc | crates/gameplay/src/inventory/order.rs:214 |
| `is_story_caster` | 144 | shaman_duel, spell_zoo | crates/script/src/combat/weapon.rs:34 |
| `on_watch_fight_goto` | 138 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, sector_switch | crates/script/src/combat/react.rs:609 |
| `choose_spell` | 135 | shaman_duel, spell_zoo | crates/script/src/combat/weapon.rs:115 |
| `is_upset` | 133 | combat_basic, melee_faithful, routine_zoo, spell_zoo | crates/script/src/predicates.rs:393 |
| `is_in_parade_pose` | 126 | combat_basic, melee_faithful, npc_vs_npc, sector_switch, spell_zoo | crates/script/src/damage.rs:611 |
| `assess_player_zone` | 123 | combat_basic, routine_zoo | crates/script/src/assess.rs:399 |
| `assess_active_perception` | 123 | combat_basic, routine_zoo | crates/script/src/assess.rs:463 |
| `assess_player_sneaking` | 123 | combat_basic, routine_zoo | crates/script/src/perception.rs:290 |
| `assess_player_weapon` | 123 | combat_basic, routine_zoo | crates/script/src/perception.rs:314 |
| `assess_defeat` | 118 | combat_basic | crates/script/src/assess.rs:303 |
| `ps_ranged_walk_loop` | 113 | bow_faithful, bow_pose, shaman_duel | crates/script/src/player/ranged.rs:217 |
| `secondary` | 111 | bow_faithful, combat_basic, melee_faithful | crates/input/src/player.rs:447 |
| `follow_distance` | 103 | routine_zoo | crates/script/src/routine/follow.rs:28 |
| `melee_primary` | 102 | combat_basic, melee_faithful | crates/input/src/player.rs:361 |
| `spell_missile` | 101 | bow_pose, magic_faithful, shaman_duel, spell_zoo | crates/projectile/src/launch.rs:71 |
| `magic_projectile` | 101 | bow_pose, magic_faithful, shaman_duel, spell_zoo | crates/script/src/magic/cast.rs:308 |
| `ps_magic_cast` | 101 | bow_pose, magic_faithful, spell_zoo, teleport | crates/script/src/player/magic.rs:169 |
| `begin_parade_command` | 97 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, routine_zoo, secto | crates/script/src/combat/commands.rs:559 |
| `caption` | 96 | menu_save_load | crates/ui/src/files.rs:94 |
| `execute_fragment` | 95 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/sequencer.rs:673 |
| `trigger_follow` | 95 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/sequencer.rs:771 |
| `trigger` | 94 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/sequencer.rs:349 |
| `ps_magic_reload` | 94 | bow_pose, magic_faithful, spell_zoo, teleport | crates/script/src/player/magic.rs:309 |
| `set_current_destination_point` | 70 | combat_basic, menu_save_load, quick_slots, routine_reentry, routine_zo | crates/script/src/routine/select.rs:170 |
| `on_ransack_body` | 65 | combat_basic, routine_zoo | crates/script/src/combat/react.rs:927 |
| `area_damage_tick` | 61 | spell_zoo | crates/script/src/magic/area.rs:324 |
| `on_entity_processing_changed` | 61 | sector_switch | crates/world/src/admin.rs:249 |
| `arena_loop` | 60 | routine_zoo | crates/script/src/combat/misc.rs:68 |
| `has_staff_magic_parade` | 59 | shaman_duel, spell_zoo | crates/script/src/damage.rs:664 |
| `can_parade_magic` | 59 | shaman_duel, spell_zoo | crates/script/src/damage.rs:678 |
| `on_revolution_attack` | 58 | routine_zoo | crates/script/src/combat/misc.rs:168 |
| `on_react_to_damage` | 58 | routine_zoo | crates/script/src/combat/react.rs:87 |
| `on_eat_body_perception` | 58 | routine_zoo | crates/script/src/combat/react.rs:862 |
| `on_guide_player` | 58 | routine_zoo | crates/script/src/routine/follow.rs:238 |
| `on_react_to_zone` | 57 | routine_zoo | crates/script/src/combat/react.rs:268 |
| `execute_transition` | 55 | combat_basic, dialog_quest, melee_faithful, npc_archer, npc_orc1, npc_ | crates/audio/src/sequencer.rs:732 |
| `in_state` | 54 | combat_basic, melee_faithful, npc_archer, npc_orc1, shaman_duel, sleep | crates/audio/src/sequencer.rs:333 |
| `on_watch_fight_go_back` | 54 | combat_basic, npc_orc1, npc_vs_npc | crates/script/src/combat/react.rs:576 |
| `ps_normal` | 54 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/player/normal.rs:72 |
| `quality_bonus` | 53 | combat_basic, melee_faithful, shaman_duel, spell_zoo | crates/script/src/damage.rs:289 |
| `skill_bonus` | 53 | combat_basic, melee_faithful, shaman_duel, spell_zoo | crates/script/src/damage.rs:305 |
| `can_cast` | 53 | bow_pose, magic_faithful, spell_zoo, teleport | crates/script/src/magic/cast.rs:99 |
| `lock_holds` | 50 | hud_focus | crates/gameplay/src/focus.rs:437 |
| `ps_interact_chest_loop` | 49 | lockpick, loot_chest, world_chest | crates/script/src/player/interact.rs:653 |
| `set_page` | 49 | controls_basic, craft_anvil, dialog_quest, lockpick, loot_chest, menu_ | crates/ui/src/manager.rs:199 |
| `can_parade_missile` | 47 | npc_archer, sector_switch, shaman_duel | crates/script/src/damage.rs:686 |
| `print` | 47 | combat_basic, craft_anvil, crime_theft, dialog_quest, doors, lockpick, | crates/ui/src/dialogmenu.rs:209 |
| `assess_take_item` | 45 | crime_theft | crates/script/src/perception.rs:222 |
| `zs_magic_sleep_loop` | 45 | routine_zoo, spell_zoo | crates/script/src/magic/area.rs:538 |
| `can_burn` | 44 | shaman_duel, spell_zoo | crates/script/src/damage.rs:414 |
| `on_follow_player` | 42 | routine_zoo | crates/script/src/routine/follow.rs:185 |
| `on_follow_npc` | 42 | routine_zoo | crates/script/src/routine/follow.rs:228 |
| `magic_instant` | 41 | spell_zoo, teleport | crates/script/src/magic/cast.rs:296 |
| `parse` | 40 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/efm.rs:107 |
| `from_ini` | 40 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/ui/src/log.rs:90 |
| `is_evil` | 37 | melee_faithful, npc_orc1, npc_vs_npc, sector_switch, spell_zoo | crates/script/src/predicates.rs:38 |
| `ps_interact_loop` | 37 | craft_anvil, sit_pose | crates/script/src/player/interact.rs:540 |
| `can_be_poisoned` | 36 | melee_faithful, npc_orc1, npc_vs_npc, sector_switch, spell_zoo | crates/script/src/damage.rs:443 |
| `on_attack_strafe` | 36 | routine_zoo | crates/script/src/combat/attack.rs:642 |
| `los_for_attack` | 35 | sector_switch, spell_zoo | crates/script/src/combat/select.rs:174 |
| `long_work_secs` | 33 | quick_slots, routine_zoo, sleep_bed, world_routines | crates/script/src/helpers.rs:114 |
| `ps_magic_potion` | 33 | spell_zoo | crates/script/src/player/magic.rs:372 |
| `is_droppable_weapon` | 32 | combat_basic, routine_reentry, routine_zoo, shaman_duel, spell_zoo | crates/script/src/predicates.rs:263 |
| `ps_ranged_shoot` | 30 | bow_faithful, bow_pose, shaman_duel | crates/script/src/player/ranged.rs:340 |
| `can_heal_self` | 29 | routine_zoo, shaman_duel | crates/script/src/routine/select.rs:353 |
| `basket_total` | 24 | trade_click, trade_esc | crates/gameplay/src/trade.rs:45 |
| `on_react_to_damage_go_back` | 24 | routine_zoo | crates/script/src/combat/react.rs:126 |
| `find_stack_index` | 23 | trade_click, trade_esc | crates/gameplay/src/inventory/list.rs:546 |
| `capture` | 23 | craft_anvil, doors, lockpick, loot_chest, menu_save_load, npc_archer,  | crates/runtime/src/save.rs:279 |
| `snapshot` | 23 | craft_anvil, doors, lockpick, loot_chest, menu_save_load, npc_archer,  | crates/sim/src/persist.rs:178 |
| `teach_offers` | 23 | controls_basic, dialog_quest, teach, trade_click, trade_esc | crates/ui/src/journal.rs:129 |
| `assess_mist` | 22 | spell_zoo | crates/script/src/magic/area.rs:271 |
| `is_gate_guard` | 20 | combat_basic, routine_zoo, spell_zoo, teleport, world_routines | crates/script/src/predicates.rs:346 |
| `want_to_ransack` | 20 | combat_basic, shaman_duel, spell_zoo | crates/script/src/predicates.rs:423 |
| `start_say_attack_end` | 20 | combat_basic, shaman_duel, spell_zoo | crates/script/src/speech.rs:51 |
| `ps_interact_end` | 20 | craft_anvil, doors, lockpick, loot_chest, sit_pose, sleep_bed, world_c | crates/script/src/player/interact.rs:583 |
| `ps_ranged_aim_reload` | 20 | bow_faithful, bow_pose, shaman_duel | crates/script/src/player/ranged.rs:434 |
| `reset_all` | 19 | combat_basic, routine_reentry, routine_zoo, shaman_duel, spell_zoo | crates/script/src/down.rs:50 |
| `magic_mist_tick` | 19 | spell_zoo | crates/script/src/magic/area.rs:308 |
| `is_big` | 18 | spell_zoo | crates/script/src/speech.rs:42 |
| `on_flee_go_back` | 18 | routine_zoo, spell_zoo | crates/script/src/combat/react.rs:436 |
| `use_stack` | 18 | bow_faithful, bow_pose, shaman_duel, spell_zoo | crates/script/src/player/use_item.rs:56 |
| `summon_party_monster` | 17 | spell_zoo | crates/script/src/magic/effects.rs:539 |
| `compare_damage` | 16 | trade_click, trade_esc | crates/gameplay/src/inventory/order.rs:89 |
| `compare_template` | 16 | trade_click, trade_esc | crates/gameplay/src/inventory/order.rs:105 |
| `compare_quality` | 16 | trade_click, trade_esc | crates/gameplay/src/inventory/order.rs:121 |
| `compare_quick_slot` | 16 | trade_click, trade_esc | crates/gameplay/src/inventory/order.rs:137 |
| `compare_weapon` | 16 | trade_click, trade_esc | crates/gameplay/src/inventory/order.rs:180 |
| `compare_recipe` | 16 | trade_click, trade_esc | crates/gameplay/src/inventory/order.rs:229 |
| `drop_hand_items` | 16 | combat_basic, routine_reentry, routine_zoo, shaman_duel, spell_zoo | crates/script/src/down.rs:59 |
| `remove_non_combat_items` | 16 | combat_basic, routine_reentry, routine_zoo, shaman_duel, spell_zoo | crates/script/src/down.rs:71 |
| `clean_up_plunder` | 16 | combat_basic, routine_reentry, routine_zoo, shaman_duel, spell_zoo | crates/script/src/down.rs:164 |
| `confirm` | 16 | controls_basic, craft_anvil, dialog_quest, doors, lockpick, loot_chest | crates/script/src/player/normal.rs:220 |
| `is_bothered_by_player_theft_of` | 15 | crime_theft, doors, lockpick, world_chest, world_doors | crates/script/src/perception.rs:166 |
| `pool_max` | 15 | spell_zoo | crates/script/src/stats.rs:187 |
| `magic_summon` | 15 | spell_zoo | crates/script/src/magic/cast.rs:353 |
| `spell_duration_bonus` | 15 | spell_zoo | crates/script/src/magic/effects.rs:150 |
| `is_deadly_damage` | 14 | combat_basic, shaman_duel, spell_zoo | crates/script/src/damage.rs:382 |
| `threaten_loop` | 14 | spell_zoo, walk_to_reddock | crates/script/src/combat/react.rs:328 |
| `ps_interact` | 14 | craft_anvil, doors, lockpick, loot_chest, sit_pose, sleep_bed, world_c | crates/script/src/player/interact.rs:433 |
| `kill` | 13 | routine_reentry, routine_zoo, shaman_duel, spell_zoo | crates/script/src/down.rs:120 |
| `magic_transform` | 13 | spell_zoo | crates/script/src/magic/effects.rs:705 |
| `send_perception_murder` | 11 | shaman_duel, spell_zoo | crates/script/src/assess.rs:295 |
| `can_open` | 11 | doors, lockpick, loot_chest, world_chest, world_doors | crates/script/src/player/interact.rs:31 |
| `deal_enabled` | 10 | trade_click, trade_esc | crates/gameplay/src/trade.rs:198 |
| `generate_trade` | 10 | trade_click, trade_esc | crates/gameplay/src/treasure.rs:253 |
| `refresh_trade` | 10 | trade_click, trade_esc | crates/gameplay/src/treasure.rs:300 |
| `hero_arrow` | 10 | bow_faithful, bow_pose, shaman_duel | crates/projectile/src/launch.rs:30 |
| `start_say_attack_reason` | 10 | combat_basic, melee_faithful, routine_zoo, spell_zoo | crates/script/src/speech.rs:91 |
| `ps_ranged_aim` | 10 | bow_faithful, bow_pose, shaman_duel | crates/script/src/player/ranged.rs:246 |
| `escape` | 10 | craft_anvil, lockpick, loot_chest, menu_save_load, trade_click, trade_ | crates/ui/src/manager.rs:246 |
| `zs_magic_sleep` | 9 | routine_zoo, spell_zoo | crates/script/src/magic/area.rs:507 |
| `ps_interact_door` | 9 | doors, world_doors | crates/script/src/player/interact.rs:672 |
| `ambient_action` | 9 | routine_zoo, world_routines | crates/script/src/routine/select.rs:432 |
| `reset` | 9 | controls_basic, dialog_quest, teach, trade_click, trade_esc | crates/ui/src/dialogmenu.rs:42 |
| `clear_baskets` | 8 | trade_click, trade_esc | crates/gameplay/src/trade.rs:164 |
| `evade_entry` | 8 | routine_zoo | crates/script/src/player/evade.rs:92 |
| `ps_interact_chest` | 8 | lockpick, loot_chest, world_chest | crates/script/src/player/interact.rs:613 |
| `ps_melee` | 8 | bow_pose, combat_basic, focus_steer, melee_faithful | crates/script/src/player/melee.rs:81 |
| `ps_ranged_walk` | 8 | bow_faithful, bow_pose, shaman_duel | crates/script/src/player/ranged.rs:185 |
| `smalltalk_dispatch` | 8 | quick_slots, routine_zoo, sleep_bed, world_routines | crates/script/src/routine/work.rs:1575 |
| `subtitle_align` | 8 | controls_basic, dialog_quest, teach, trade_click, trade_esc | crates/ui/src/dialogmenu.rs:103 |
| `start` | 8 | controls_basic, dialog_quest, teach, trade_click, trade_esc | crates/ui/src/dialogmenu.rs:133 |
| `send_perception_use_interact_object` | 7 | doors, lockpick, loot_chest, world_chest, world_doors | crates/script/src/assess.rs:358 |
| `unlock` | 7 | doors, lockpick, loot_chest, world_chest, world_doors | crates/script/src/player/interact.rs:79 |
| `door_status` | 6 | doors, world_doors | crates/gameplay/src/mover.rs:158 |
| `can_freeze` | 6 | shaman_duel, spell_zoo | crates/script/src/damage.rs:429 |
| `is_slave` | 6 | combat_basic | crates/script/src/predicates.rs:352 |
| `is_temporary_friendly_to_player` | 6 | combat_basic | crates/script/src/predicates.rs:384 |
| `choose_offensive` | 6 | sector_switch, spell_zoo | crates/script/src/combat/moves.rs:807 |
| `magic_time_bubble` | 6 | spell_zoo | crates/script/src/magic/effects.rs:833 |
| `evade_loop` | 6 | routine_zoo | crates/script/src/player/evade.rs:127 |
| `ps_ghost_loop` | 6 | spell_zoo | crates/script/src/player/magic.rs:643 |
| `ps_ranged` | 6 | bow_faithful, bow_pose, shaman_duel | crates/script/src/player/ranged.rs:142 |
| `send_perception_warn` | 5 | combat_basic, melee_faithful, routine_zoo | crates/script/src/assess.rs:77 |
| `start_say_defeat_comment` | 5 | combat_basic | crates/script/src/speech.rs:143 |
| `has_crime_refuse` | 5 | controls_basic, dialog_quest, teach, trade_click, trade_esc | crates/script/src/speech.rs:358 |
| `assess_talk` | 5 | controls_basic, dialog_quest, teach, trade_click, trade_esc | crates/script/src/speech.rs:378 |
| `motion_stopped` | 4 | doors, world_doors | crates/gameplay/src/mover.rs:123 |
| `ps_ghost` | 4 | spell_zoo | crates/script/src/player/magic.rs:611 |
| `ps_melee_parade` | 4 | combat_basic, melee_faithful | crates/script/src/player/melee.rs:291 |
| `holster_from_melee` | 4 | bow_faithful, bow_pose, magic_faithful, melee_faithful | crates/script/src/player/select_weapon.rs:20 |
| `draw_from_normal` | 4 | bow_pose, combat_basic, focus_steer, melee_faithful | crates/script/src/player/select_weapon.rs:39 |
| `select_weapon` | 4 | bow_pose, combat_basic, focus_steer, melee_faithful | crates/script/src/player/select_weapon.rs:52 |
| `enable` | 4 | sector_switch | crates/world/src/sector.rs:180 |
| `on_enable` | 4 | sector_switch | crates/world/src/sector.rs:189 |
| `delete_stack` | 3 | craft_anvil, spell_zoo | crates/gameplay/src/inventory/list.rs:700 |
| `restore` | 3 | menu_save_load, quick_slots | crates/runtime/src/save.rs:304 |
| `send_perception_defeat` | 3 | combat_basic | crates/script/src/assess.rs:328 |
| `memorize_enclave_crime` | 3 | combat_basic, spell_zoo | crates/script/src/crime.rs:89 |
| `unconscious_secs` | 3 | combat_basic | crates/script/src/down.rs:35 |
| `defeat` | 3 | combat_basic | crates/script/src/down.rs:176 |
| `set_last_fight_timestamp` | 3 | routine_zoo, sector_switch | crates/script/src/predicates.rs:366 |
| `cancel_attack` | 3 | routine_zoo, sector_switch | crates/script/src/combat/attack.rs:178 |
| `ps_dusk_to_dawn` | 3 | sleep_bed | crates/script/src/player/interact.rs:773 |
| `ps_magic_flame_sword` | 3 | spell_zoo | crates/script/src/player/magic.rs:249 |
| `ps_ranged_reload` | 3 | bow_faithful | crates/script/src/player/ranged.rs:389 |
| `apply_snapshot` | 3 | menu_save_load, quick_slots | crates/sim/src/persist.rs:224 |
| `open_menu` | 3 | menu_save_load | crates/ui/src/manager.rs:222 |
| `open` | 2 | doors, world_doors | crates/gameplay/src/mover.rs:58 |
| `close` | 2 | doors, world_doors | crates/gameplay/src/mover.rs:86 |
| `set_door_status` | 2 | doors, world_doors | crates/gameplay/src/mover.rs:142 |
| `step_basket` | 2 | trade_click, trade_esc | crates/gameplay/src/trade.rs:87 |
| `cancel` | 2 | trade_click, trade_esc | crates/gameplay/src/trade.rs:273 |
| `loot_key_index` | 2 | craft_anvil, loot_chest | crates/gameplay/src/trade.rs:280 |
| `generate_trade_inventory` | 2 | trade_click, trade_esc | crates/gameplay/src/treasure.rs:355 |
| `refresh_trade_inventory` | 2 | trade_click, trade_esc | crates/gameplay/src/treasure.rs:376 |
| `transfer_items_to` | 2 | trade_click | crates/gameplay/src/inventory/list.rs:1209 |
| `transfer_transaction_items_to` | 2 | trade_click | crates/gameplay/src/inventory/list.rs:1319 |
| `add_enclave_theft_count` | 2 | crime_theft, loot_chest | crates/script/src/crime.rs:126 |
| `give_xp` | 2 | combat_basic, spell_zoo | crates/script/src/damage.rs:1141 |
| `start_say_doesnt_work` | 2 | spell_zoo | crates/script/src/speech.rs:339 |
| `add_hit_points` | 2 | spell_zoo | crates/script/src/stats.rs:304 |
| `start_lie_knock_down` | 2 | combat_basic | crates/script/src/combat/commands.rs:201 |
| `zs_freeze_loop` | 2 | routine_zoo | crates/script/src/magic/area.rs:477 |
| `zs_misted` | 2 | routine_zoo | crates/script/src/magic/area.rs:562 |
| `zs_misted_loop` | 2 | routine_zoo | crates/script/src/magic/area.rs:580 |
| `is_animal` | 2 | spell_zoo | crates/script/src/magic/effects.rs:56 |
| `magic_cure` | 2 | spell_zoo | crates/script/src/magic/effects.rs:243 |
| `magic_teleport` | 2 | spell_zoo, teleport | crates/script/src/magic/effects.rs:653 |
| `player_sleep` | 2 | sleep_bed | crates/script/src/player/interact.rs:714 |
| `ps_interact_sleep_bed` | 2 | sleep_bed | crates/script/src/player/interact.rs:743 |
| `ps_normal_take` | 2 | crime_theft | crates/script/src/player/normal.rs:113 |
| `carry_stones` | 2 | routine_zoo | crates/script/src/routine/work.rs:1441 |
| `listen_loop` | 2 | routine_zoo | crates/script/src/routine/work.rs:1488 |
| `run_to_attack_point` | 2 | routine_zoo | crates/script/src/routine/work.rs:1627 |
| `trade_open` | 2 | trade_click, trade_esc | crates/sim/src/trade.rs:77 |
| `time_of_day` | 2 | dialog_quest, ui_screens | crates/ui/src/journal.rs:69 |
| `on_touch` | 2 | triggers | crates/world/src/trigger.rs:335 |
| `can_be_activated` | 2 | triggers | crates/world/src/trigger.rs:570 |
| `activate` | 2 | triggers | crates/world/src/trigger.rs:643 |
| `balance_gold` | 1 | trade_click | crates/gameplay/src/trade.rs:224 |
| `commit` | 1 | trade_click | crates/gameplay/src/trade.rs:258 |
| `can_be_diseased` | 1 | spell_zoo | crates/script/src/damage.rs:449 |
| `pipi_stumble` | 1 | shaman_duel | crates/script/src/damage.rs:1016 |
| `ambient_action_secs` | 1 | routine_zoo | crates/script/src/helpers.rs:125 |
| `send_perception_take_item` | 1 | crime_theft | crates/script/src/perception.rs:252 |
| `has_attack_reason_to_kill` | 1 | combat_basic | crates/script/src/predicates.rs:242 |
| `on_attack_positioning` | 1 | routine_zoo | crates/script/src/combat/attack.rs:584 |
| `begin_jump_back_command` | 1 | routine_zoo | crates/script/src/combat/commands.rs:594 |
| `arena_enter` | 1 | routine_zoo | crates/script/src/combat/misc.rs:27 |
| `success_enter` | 1 | routine_zoo | crates/script/src/combat/misc.rs:184 |
| `on_flee_goto_fleepoint` | 1 | routine_zoo | crates/script/src/combat/react.rs:385 |
| `lee_stand` | 1 | routine_zoo | crates/script/src/combat/react.rs:998 |
| `zs_freeze` | 1 | routine_zoo | crates/script/src/magic/area.rs:460 |
| `zs_decay` | 1 | routine_zoo | crates/script/src/magic/area.rs:592 |
| `can_tame_animal` | 1 | spell_zoo | crates/script/src/magic/effects.rs:86 |
| `can_control` | 1 | spell_zoo | crates/script/src/magic/effects.rs:115 |
| `magic_poison` | 1 | spell_zoo | crates/script/src/magic/effects.rs:215 |
| `magic_disease` | 1 | spell_zoo | crates/script/src/magic/effects.rs:225 |
| `magic_cure_poison` | 1 | spell_zoo | crates/script/src/magic/effects.rs:250 |
| `magic_cure_disease` | 1 | spell_zoo | crates/script/src/magic/effects.rs:258 |
| `magic_heal_other` | 1 | spell_zoo | crates/script/src/magic/effects.rs:266 |
| `magic_tame_animal` | 1 | spell_zoo | crates/script/src/magic/effects.rs:371 |
| `magic_word_of_control` | 1 | spell_zoo | crates/script/src/magic/effects.rs:387 |
| `magic_charm` | 1 | spell_zoo | crates/script/src/magic/effects.rs:397 |
| `magic_bloodlust` | 1 | spell_zoo | crates/script/src/magic/effects.rs:415 |
| `magic_fear` | 1 | spell_zoo | crates/script/src/magic/effects.rs:443 |
| `magic_sleep` | 1 | spell_zoo | crates/script/src/magic/effects.rs:456 |
| `magic_hypnosis` | 1 | spell_zoo | crates/script/src/magic/effects.rs:465 |
| `magic_call_animals` | 1 | spell_zoo | crates/script/src/magic/effects.rs:505 |
| `magic_summon_demon` | 1 | spell_zoo | crates/script/src/magic/effects.rs:570 |
| `magic_summon_skeleton` | 1 | spell_zoo | crates/script/src/magic/effects.rs:576 |
| `magic_summon_golem` | 1 | spell_zoo | crates/script/src/magic/effects.rs:582 |
| `magic_summon_goblin` | 1 | spell_zoo | crates/script/src/magic/effects.rs:588 |
| `magic_summon_army_of_darkness` | 1 | spell_zoo | crates/script/src/magic/effects.rs:605 |
| `magic_summon_army_of_cuteness` | 1 | spell_zoo | crates/script/src/magic/effects.rs:619 |
| `magic_summon_monster` | 1 | spell_zoo | crates/script/src/magic/effects.rs:629 |
| `magic_animal_form` | 1 | spell_zoo | crates/script/src/magic/effects.rs:733 |
| `magic_open_locks` | 1 | spell_zoo | crates/script/src/magic/effects.rs:782 |
| `magic_soul_travel` | 1 | spell_zoo | crates/script/src/magic/effects.rs:797 |
| `magic_telekinesis` | 1 | spell_zoo | crates/script/src/magic/effects.rs:826 |
| `synthesize` | 1 | craft_anvil | crates/script/src/player/craft.rs:32 |
| `ps_magic_summon_abort` | 1 | magic_faithful | crates/script/src/player/magic.rs:238 |
| `have_a_break` | 1 | routine_zoo | crates/script/src/routine/select.rs:418 |
| `close_menu` | 1 | menu_save_load | crates/ui/src/manager.rs:228 |
| `on_untouch` | 1 | triggers | crates/world/src/trigger.rs:350 |
| `deactivate` | 1 | triggers | crates/world/src/trigger.rs:724 |
