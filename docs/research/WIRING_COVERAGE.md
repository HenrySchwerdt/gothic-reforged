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

* **Refresh 2026-10-28 (evening; branch day/desktop2 at the merge of cov-npc 8d9d7d1 and cov-engine 8fd8e08, 44 scenarios incl. world_boulder, world_fry, staff_parade, spell_zoo, crime_theft; `routine_zoo` times out at 480 s, `doors` / `world_doors` / `spell_zoo` fail under the instrumented build only for `doors`, `world_doors` (timing, they pass on the normal build)): 798 instrumented ported functions, **509 called, 289 never called** (36% uncalled; 92 DEAD, 116 INTERNAL-ONLY, 81 WIRED-NOT-EXERCISED). States entered: **283 of 375**. History of called functions: 262 (2026-10-21), 314 (10-27), 461 (10-28 morning), 509 (now); states 65, 68, 268, 283. The prose below is the 2026-10-21 analysis and is mostly outdated; the generated tables are current.
* Script states entered at least once: **65 of 375** (PS_: 24 of 101, ZS_: 41 of 266, BS_: 0 of 8). The hero's `PS_*` set that play really reaches:
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

Generated by `scripts/portcov/` (crate `gothic-portcov`, cargo feature `port-coverage` of `gothic-reforged`). `scripts/portcov/instrument.py` inserts `gothic_portcov::hit!("<fn>")` at the top of every function whose doc comment says `Port of <Name> (<Dll>:<addr>)`; the macro expands to nothing without the feature. Each QA scenario was run as `gothic-reforged --play --qa <scenario>` with the feature on, and the call counts of all runs were merged (`scripts/portcov/run_all.sh`, `scripts/portcov/report.py`). **Called** means executed at least once by the running game in any scenario (unit tests do not count; a function reached only from another never-used function counts as never called only if it is itself never executed). Scenarios run: `bow_faithful`, `bow_pose`, `combat_basic`, `controls_basic`, `craft_anvil`, `crime_theft`, `dialog_quest`, `doors`, `focus_steer`, `hud_focus`, `lockpick`, `loot_chest`, `magic_faithful`, `melee_faithful`, `menu_save_load`, `npc_archer`, `npc_orc1`, `npc_vs_npc`, `perf_idle`, `quick_slots`, `routine_reentry`, `routine_zoo`, `sector_switch`, `shaman_duel`, `sit_pose`, `sleep_bed`, `spell_zoo`, `staff_parade`, `startup_sanity`, `swim`, `teach`, `teleport`, `trade_click`, `trade_esc`, `triggers`, `ui_screens`, `walk_to_reddock`, `world_boulder`, `world_chest`, `world_doors`, `world_fry`, `world_routines`, `world_walk`.

## Totals

* Doc blocks with `Port of`: 940; instrumentable functions: 798 (+0 not instrumented, see the end); other items (tables, consts, structs, impls; not countable): 142.
* **Called in play: 509**, **never called: 289** (36% of instrumented functions).
* Of the never-called: **92 DEAD** (no production reference anywhere: nobody could call them), **116 internal-only** (referenced only inside their own crate, entry point never reached), **81 wired-not-exercised** (referenced from another crate, but no QA scenario reaches the call; long-running routines, rare events). A trailing "?" marks names shared by several functions (reference count is a name match, so the status is a heuristic).
* Script states (ZS_/PS_/BS_, data tables interpreted by `run_state`): 283 of 375 entered at least once.

## By subsystem

| Subsystem / original state family | called | never called |
| --- | ---: | ---: |
| gameplay (inventory, trade, mover, treasure, focus, stats) | 90 | 73 |
| world (entity admin, PVS, sectors, triggers, zones) | 21 | 43 |
| script helpers / predicates / stats | 43 | 36 |
| animation (EMotionFX wrapper, blending, attachments) | 14 | 34 |
| ZS_* routine tasks | 22 | 20 |
| spell effects / casting | 58 | 12 |
| ZS_Attack / combat AI | 64 | 11 |
| ui | 14 | 11 |
| damage pipeline | 34 | 9 |
| PS_Magic* | 10 | 7 |
| audio sequencers | 21 | 5 |
| render rules | 2 | 5 |
| perception / crime | 17 | 5 |
| PS_Interact* | 10 | 4 |
| PS_Normal / Sneak / Swim | 4 | 3 |
| PS_Ranged* (bow, crossbow) | 11 | 3 |
| fx / particles | 17 | 2 |
| down / knock-out | 7 | 2 |
| nav | 0 | 1 |
| PS_Interact boulder | 3 | 1 |
| PS_Melee* | 3 | 1 |
| sim (stats, combat, AI, host) | 5 | 1 |
| camera (PSCamera) | 1 | 0 |
| environment / weather | 7 | 0 |
| formats | 3 | 0 |
| input / player control | 6 | 0 |
| physics / movement | 1 | 0 |
| projectiles | 9 | 0 |
| runtime | 3 | 0 |
| PS_Interact craft | 3 | 0 |
| PS_Evade | 2 | 0 |
| PS_* other (player api / commands) | 3 | 0 |
| PS_HUD use item | 1 | 0 |

## Script states never entered in play

**PS_** (57 of 101): `PS_DuskToDawn_Loop`, `PS_GhostSummoner`, `PS_GhostSummoner_Loop`, `PS_Ghost_Jump`, `PS_GonnaLoot`, `PS_HUD`, `PS_HUD_Equip`, `PS_HUD_Loop`, `PS_HUD_UnEquip`, `PS_HUD_Use`, `PS_Interact_Alchemy`, `PS_Interact_Bookstand`, `PS_Interact_Campfire`, `PS_Interact_CarryStone`, `PS_Interact_CarryStone_End`, `PS_Interact_CarryStone_Loop`, `PS_Interact_Cauldron`, `PS_Interact_Door`, `PS_Interact_DrinkWaterBarrel`, `PS_Interact_Forge`, `PS_Interact_GrindStone`, `PS_Interact_HoldOrcBoulder_Loop`, `PS_Interact_LiftOrcBoulder`, `PS_Interact_LiftOrcBoulder_Loop`, `PS_Interact_OrcDrum`, `PS_Interact_PickOre`, `PS_Interact_PirateTreasure`, `PS_Interact_SawLog`, `PS_Interact_Shrine`, `PS_Interact_SitStool`, `PS_Interact_SitThrone`, `PS_Interact_SleepGround`, `PS_Interact_Stoneplate`, `PS_Interact_Talk`, `PS_Interact_UseWaterpipe`, `PS_Interact_WaitForExit`, `PS_Magic_Jump`, `PS_Magic_QuickCast`, `PS_Magic_Sneak`, `PS_Melee_FinishingAttack`, `PS_Melee_HackAttack`, `PS_Melee_Jump`, `PS_Melee_JumpBack`, `PS_Melee_QuickAttack`, `PS_Melee_SimpleWhirl`, `PS_Melee_Sneak`, `PS_Melee_WhirlAttack`, `PS_Normal_Jump`, `PS_Normal_Sneak`, `PS_QuickUse`, `PS_Ranged_CrossBowSneak_Loop`, `PS_Ranged_Jump`, `PS_Ranged_Sneak`, `PS_Slide_Jump`, `PS_StopForMenu`, `PS_Swim`, `PS_Swim_Loop`

**ZS_** (27 of 266): `ZS_Dead`, `ZS_Dead_Loop`, `ZS_PiercedDead`, `ZS_RagDollDead`, `ZS_RagDollDead_Loop`, `ZS_Timebubble`, `ZS_Timebubble_Loop`, `ZS_Trader`, `ZS_Trader_Bookshelf`, `ZS_Trader_Bookstand`, `ZS_Trader_Chest`, `ZS_Trader_Stand`, `ZS_Trader_SweepFloor`, `ZS_TravelToDestination`, `ZS_TravelToDestination_Loop`, `ZS_Unconscious`, `ZS_Unconscious_End`, `ZS_Unconscious_Loop`, `ZS_WaitForDawn`, `ZS_WaitForFreeDestination`, `ZS_WaitForFreeDestination_Loop`, `ZS_WaitForMeteor`, `ZS_WaitForPlayer`, `ZS_WaitForPlayer_Loop`, `ZS_WatchFight_Spectator`, `ZS_WatchFight_Spectator_Loop`, `ZS_Woodwork_Repair`

**BS_** (8 of 8): `BS_Alchemy`, `BS_AncientWisdom`, `BS_BookOfFlood`, `BS_BookOfRhobar`, `BS_BookOfZuben`, `BS_Chronicles`, `BS_NothingNew`, `BS_Smith`

Entered: `PS_DuskToDawn` (6), `PS_Ghost` (4), `PS_Ghost_Loop` (5), `PS_Interact` (12), `PS_Interact_Anvil` (3), `PS_Interact_Barbecue` (2), `PS_Interact_Chest` (7), `PS_Interact_Chest_Loop` (48), `PS_Interact_End` (19), `PS_Interact_HoldOrcBoulder` (1), `PS_Interact_Loop` (79), `PS_Interact_SitBench` (3), `PS_Interact_SleepBed` (2), `PS_Interact_TiltOrcBoulder` (4), `PS_Interact_TiltOrcBoulder_Loop` (5), `PS_Magic` (159), `PS_MagicPotion` (22), `PS_Magic_Cast` (9), `PS_Magic_FlameSword` (3), `PS_Magic_Loop` (1438), `PS_Magic_PowerCast` (4), `PS_Magic_Reload` (94), `PS_Magic_StaffParade` (2), `PS_Magic_StaffParade_Loop` (14), `PS_Magic_Summon` (88), `PS_Magic_Summon_Abort` (1), `PS_Melee` (10), `PS_Melee_Attack` (36), `PS_Melee_Loop` (577), `PS_Melee_Parade` (4), `PS_Melee_PierceAttack` (1), `PS_Melee_PowerAttack` (3), `PS_Normal` (62), `PS_Normal_Loop` (12438), `PS_Normal_Take` (4), `PS_Ranged` (6), `PS_Ranged_Aim` (10), `PS_Ranged_Aim_Loop` (213), `PS_Ranged_Aim_Reload` (16), `PS_Ranged_PowerAim` (149), `PS_Ranged_Reload` (3), `PS_Ranged_Shoot` (24), `PS_Ranged_Walk` (7), `PS_Ranged_Walk_Loop` (73), `ZS_Acrobat` (60), `ZS_Alchemy` (1), `ZS_Alchemy_Alchemy` (3), `ZS_Alchemy_Bookstand` (3), `ZS_Alchemy_Cauldron` (3), `ZS_Ambient` (85), `ZS_Ambient_Loop` (45), `ZS_Ambient_Sleep` (2), `ZS_Ambient_Sleep_Loop` (2), `ZS_AnimalSleep` (3), `ZS_AnimalSleep_SleepGround` (60), `ZS_Applaud` (11), `ZS_Arena` (3), `ZS_Arena_Loop` (118), `ZS_Attack` (3866), `ZS_Attack_Arena_End` (1), `ZS_Attack_Attack` (423), `ZS_Attack_Cast` (28), `ZS_Attack_ChangePosition` (1), `ZS_Attack_DrinkPotion` (2), `ZS_Attack_End` (369), `ZS_Attack_FinishingAttack` (2), `ZS_Attack_GoBack` (132), `ZS_Attack_Goto` (417), `ZS_Attack_HackAttack` (260), `ZS_Attack_JumpBack` (2), `ZS_Attack_Loop` (9726), `ZS_Attack_Parade` (39), `ZS_Attack_PierceAttack` (28), `ZS_Attack_PowerAttack` (268), `ZS_Attack_QuickAttack` (286), `ZS_Attack_Shoot` (1699), `ZS_Attack_SimpleWhirl` (2), `ZS_Attack_Sprint` (529), `ZS_Attack_StrafeLeft` (1), `ZS_Attack_StrafeRight` (1), `ZS_Attack_TurnLeft` (820), `ZS_Attack_TurnRight` (418), `ZS_Attack_Wait` (1274), `ZS_Attack_WaitForDialog` (222), `ZS_Attack_WaitForFlee` (54), `ZS_Attack_WaitForFlee_Loop` (35183), `ZS_Attack_WaitOnShore` (31), `ZS_Attack_WhirlAttack` (240), `ZS_Barbecue` (4), `ZS_Bard` (8), `ZS_Boss` (27), `ZS_Boss_Pace` (50), `ZS_Boss_Stand` (9), `ZS_Boss_Throne` (4), `ZS_Break` (3), `ZS_Break_Bench` (2), `ZS_Break_Chest` (3), `ZS_Break_Pee` (3), `ZS_Break_SitGround` (2), `ZS_Break_Stand` (9), `ZS_Break_Stool` (3), `ZS_Break_WaterBarrel` (3), `ZS_CarryStones` (3), `ZS_Cook` (1), `ZS_Deadlock` (3), `ZS_Decay` (1), `ZS_Digger` (1), `ZS_Digger_DigGround` (3), `ZS_Digger_PickGround` (3), `ZS_Digger_PickOre` (3), `ZS_Digger_WaterBarrel` (3), `ZS_DiveIn` (60), `ZS_EatBody` (3), `ZS_EatBody_Loop` (1), `ZS_EvadeBackward` (2), `ZS_EvadeForward` (2), `ZS_EvadeLeft` (2), `ZS_EvadeLoop` (6), `ZS_EvadeRight` (2), `ZS_Fall` (60), `ZS_Farmer` (1), `ZS_Farmer_FieldWork` (3), `ZS_Farmer_RakeField` (3), `ZS_FetchWeapon` (10), `ZS_Firerain` (60), `ZS_Firerain_Loop` (61), `ZS_Flee` (2), `ZS_Flee_GoBack` (4), `ZS_Flee_GotoFleePoint` (2), `ZS_Flee_ObserveFollower` (60), `ZS_FollowNPC` (3), `ZS_FollowNPC_Loop` (104), `ZS_FollowPlayer` (3), `ZS_FollowPlayer_Loop` (86), `ZS_Freeze` (1), `ZS_Freeze_Loop` (2), `ZS_FrostWave_Loop` (61), `ZS_Frostwave` (60), `ZS_Goblin` (2), `ZS_Goblin_Banana` (60), `ZS_Goblin_FryMeat` (3), `ZS_Goblin_SitGround` (3), `ZS_Goblin_Smalltalk` (76), `ZS_GotoPlayerZone` (3), `ZS_Grind` (4), `ZS_Guard` (18), `ZS_Guard_Loop` (20), `ZS_GuidePlayer` (2), `ZS_GuidePlayer_Loop` (355), `ZS_GuidePlayer_Wait` (177), `ZS_Hailstorm` (60), `ZS_Hailstorm_FadeIn` (60), `ZS_Hailstorm_FadeIn_Loop` (60), `ZS_Hailstorm_FadeOut_Loop` (60), `ZS_Hailstorm_Loop` (61), `ZS_HangAround` (3051), `ZS_HangAround_Bench` (567), `ZS_HangAround_DoNothing` (520), `ZS_HangAround_FryMeat` (38), `ZS_HangAround_Pee` (3), `ZS_HangAround_SitGround` (3935), `ZS_HangAround_Stand` (141), `ZS_HangAround_Stool` (214), `ZS_HangAround_Waterpipe` (3), `ZS_HealSelf` (2), `ZS_HealSelf_Cast` (2), `ZS_HeatWave_Loop` (60), `ZS_Heatwave` (60), `ZS_HeavyLand` (60), `ZS_HeavyParadeStumble` (60), `ZS_Herd` (1), `ZS_Herd_DrinkWater` (1), `ZS_Herd_EatGround` (2), `ZS_IceBomb` (60), `ZS_IceBomb_Loop` (60), `ZS_Idiot` (21), `ZS_Land` (61), `ZS_Lee_Teleport` (4), `ZS_LieKnockDown` (60), `ZS_Light` (60), `ZS_Light_Loop` (61), `ZS_Lightning` (60), `ZS_Lightning_Loop` (60), `ZS_Listen` (4), `ZS_Listen_Loop` (7), `ZS_Lumberjack` (4), `ZS_MagicHypnosis` (3), `ZS_MagicHypnosis_Loop` (18), `ZS_MagicSleep` (3), `ZS_MagicSleep_Loop` (18), `ZS_Meteor` (60), `ZS_Meteor_Loop` (60), `ZS_Mist` (60), `ZS_Mist_End` (60), `ZS_Mist_Loop` (60), `ZS_Mist_NoAggro` (61), `ZS_Mist_NoAggro_Loop` (60), `ZS_Misted` (2), `ZS_Misted_Loop` (2), `ZS_Monster` (14), `ZS_Monster_Roam` (10), `ZS_NavEval` (60), `ZS_NavEval_Loop` (60), `ZS_ObserveSuspect` (46), `ZS_ObserveSuspect_Loop` (1269), `ZS_ObserveWeapon` (3), `ZS_ObserveZonePlayer` (2), `ZS_OrcParty` (1), `ZS_OrcParty_OrcBoulder` (5), `ZS_OrcParty_OrcDance` (3), `ZS_OrcParty_OrcDrum` (3), `ZS_OrcParty_Spectator` (3), `ZS_ParadeStumble` (60), `ZS_PierceStumble` (60), `ZS_PiercedKO` (61), `ZS_Pray` (60), `ZS_Pray_Shrine` (3), `ZS_Preach` (2), `ZS_Preach_AutoPreach` (3), `ZS_Preach_SVM` (26), `ZS_QuickStumble` (61), `ZS_RansackBody` (13), `ZS_ReactToDamage` (2), `ZS_ReactToDamage_End` (59), `ZS_ReactToDamage_GoBack` (48), `ZS_ReactToDamage_Loop` (121), `ZS_ReactToFootsteps` (2), `ZS_ReactToSneak` (3), `ZS_ReactToWeapon` (42), `ZS_ReactToWeapon_End` (30), `ZS_ReactToWeapon_WaitForAttack` (448), `ZS_ReactToWeapon_WaitForSecondWarn` (666), `ZS_ReactToZone` (3), `ZS_ReactToZone_Loop` (93), `ZS_RevolutionAttack` (3), `ZS_RevolutionAttack_Loop` (119), `ZS_RevolutionSuccess` (3), `ZS_RunToAttackPoint` (3), `ZS_SitKnockDown` (60), `ZS_SitKnockDown_GetUpAttack` (61), `ZS_SitKnockDown_GetUpParade` (60), `ZS_SitKnockDown_Loop` (60), `ZS_SlaveBabe` (1), `ZS_SlaveBabe_CleanFloor` (3), `ZS_SlaveBabe_Dance` (3), `ZS_SlaveBabe_FanBoss` (3), `ZS_SlaveBabe_SweepFloor` (3), `ZS_Sleep` (3), `ZS_Sleep_Bed` (3), `ZS_Sleep_Ground` (3), `ZS_Sleep_Loop` (147), `ZS_Slide` (60), `ZS_Slide_Loop` (60), `ZS_Smalltalk` (8), `ZS_Smalltalk_Leader` (14), `ZS_Smalltalk_Leader_Loop` (19), `ZS_Smalltalk_Partner` (12), `ZS_Smalltalk_Partner_Loop` (1414), `ZS_Smith` (2), `ZS_Smith_Anvil` (3), `ZS_Smith_Cool` (3), `ZS_Smith_Forge` (3), `ZS_Smith_GrindStone` (3), `ZS_StandUp` (2), `ZS_Study` (16), `ZS_Study_Bookshelf` (3), `ZS_Study_Bookstand` (23), `ZS_Stumble` (60), `ZS_Stunned` (2), `ZS_Stunned_End` (2), `ZS_Stunned_Loop` (30), `ZS_Talk` (10), `ZS_Talk_Loop` (2275), `ZS_Threaten` (156), `ZS_Threaten_Loop` (53), `ZS_WatchFight` (231), `ZS_WatchFight_GoBack` (73), `ZS_WatchFight_Goto` (2390), `ZS_WatchFight_Loop` (44890), `ZS_WatchFight_WaitForFlee` (10), `ZS_WatchFight_WaitForFlee_Loop` (294), `ZS_Woodwork` (4), `ZS_Woodwork_SawLog` (3)

## Ported functions never called in play (by subsystem)

### gameplay (inventory, trade, mover, treasure, focus, stats) (73)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `mod_value` (gCItem_PS) | Game:0x100b9d20 | value of the first modifier whose tag equals tag, 0 when there is none. | INTERNAL-ONLY | crates/gameplay/src/inventory/item.rs:187 |
| `pick_up` (the pickup path) | - | a pickup entity's amount and quality are added to the inventory. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/inventory/list.rs:507 |
| `find_stack_index_by_use_type` (the stack list's FindStackIndex by use type) | Game:0x100c39b0 | the stack list's FindStackIndex by use type (Game:0x100c39b0). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:574 |
| `find_stack_index_by_hot_key` (gCInventory_PS) | Game:0x100b5900 | quick slot lookup. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:644 |
| `stack_category` (gCInventory_PS) | Game:0x100b5a70 | gCInventory_PS::GetStackCategory (Game:0x100b5a70). | WIRED-NOT-EXERCISED ? | crates/gameplay/src/inventory/list.rs:659 |
| `set_stack_amount` (gCInventory_PS) | Game:0x100b5b40 | Amount 0 disposes the stack (only if it is unlinked). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:667 |
| `set_stack_quality` (gCInventory_PS) | Game:0x100b5ab0 | gCInventory_PS::SetStackQuality (Game:0x100b5ab0). | DEAD | crates/gameplay/src/inventory/list.rs:682 |
| `dispose_stack` (the stack list's DisposeStack) | Game:0x100c3c20 | delete only unlinked stacks. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:703 |
| `remove_items` (the stack list's DisposeItems) | Game:0x100c38e0 | remove n items. Permanent items (skills, spells) are never reduced but the call succeeds; removing the whole stack needs it to be unlinked. | WIRED-NOT-EXERCISED | crates/gameplay/src/inventory/list.rs:729 |
| `merge_stacks` (gCInventory_PS) | - | gCInventory_PS::MergeStacks: merge stack from into into. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:751 |
| `split_stack` (gCInventory_PS) | - | gCInventory_PS::SplitStack: cut n items into a new stack. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:766 |
| `set_default_item` (gCInventory_PS) | Game:0x100b5480 | declare the default item of a slot (template inventories only). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:787 |
| `default_item` (gCInventory_PS) | Game:0x100b5420 | template name or empty. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:805 |
| `link_default_items` (gCInventory_PS) | Game:0x100b5530 | for slots 1..=18 without a stack, spawn the default item. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:860 |
| `unlink_default_items` (gCInventory_PS) | Game:0x100b55e0 | gCInventory_PS::UnlinkDefaultItems (Game:0x100b55e0). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:872 |
| `patch_from_template` (gCInventory_PS) | Game:0x100b56d0 | as run by OnChildrenAvailable for an entity instance: take the default items and treasure sets of the creating template, then link the defaults. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:888 |
| `unequip_stack` (gCInventory_PS) | Game:0x100b8310 | only stacks in their carry slot can be unequipped. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/inventory/list.rs:1053 |
| `drop_items_from_stack` (gCInventory_PS) | Game:0x100b4580 | drop n items of a stack (the stack is unlinked from its slot). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1191 |
| `drop_items_from_slot` (gCInventory_PS) | Game:0x100b4630 | gCInventory_PS::DropItemsFromSlot (Game:0x100b4630). | DEAD | crates/gameplay/src/inventory/list.rs:1217 |
| `transfer_stack_to` (gCInventory_PS) | Game:0x100b6de0 | move the whole stack. | WIRED-NOT-EXERCISED | crates/gameplay/src/inventory/list.rs:1282 |
| `transfer_all_to` (gCInventory_PS) | Game:0x100b6d70 | move every stack; true when all moved. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1295 |
| `transaction_amount` (gCInventory_PS) | Game:0x100b70a0 | gCInventory_PS::GetTransactionAmount (Game:0x100b70a0). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1328 |
| `perform_transaction` (gCInventory_PS) | Game:0x100b7130 | both sides hand over their marked items. self_gets is the stack type of the items this side receives and other_gets of the items the other side receiv... | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1365 |
| `is_armor_set_equipped` (gCInventory_PS) | Game:0x100b7ca0 | (Game:0x100b7ca0): all six pieces of the set sit in slots. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1410 |
| `is_armor_piece_equipped` (gCInventory_PS) | Game:0x100b7d40 | gCInventory_PS::IsArmorPieceEquipped (Game:0x100b7d40). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1419 |
| `find_spell_stack_index` (gCInventory_PS) | Game:0x100b6940 | gCInventory_PS::FindSpellStackIndex (Game:0x100b6940). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1491 |
| `is_skill_active` (gCInventory_PS) | Game:0x100b5f80 | learned or kept active by equipment. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/inventory/list.rs:1528 |
| `skill_activation_count` (gCInventory_PS) | Game:0x100b6760 | gCInventory_PS::GetSkillActivationCount (Game:0x100b6760). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1537 |
| `has_learned_skill` (gCInventory_PS) | Game:0x100b5fc0 | gCInventory_PS::HasLearnedSkill (Game:0x100b5fc0). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1544 |
| `has_learned_spell` (gCInventory_PS) | Game:0x100b68f0 | gCInventory_PS::HasLearnedSpell (Game:0x100b68f0). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1551 |
| `learn_stack` (gCInventory_PS) | Game:0x100b5ef0 | mark a skill/spell stack learned. | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1558 |
| `learn_skill` (gCInventory_PS) | Game:0x100b6790 | gCInventory_PS::LearnSkill (Game:0x100b6790). | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1571 |
| `learn_spell` (gCInventory_PS) | Game:0x100b6920 | gCInventory_PS::LearnSpell (Game:0x100b6920). | DEAD | crates/gameplay/src/inventory/list.rs:1578 |
| `can_learn_skill` (gCInventory_PS) | Game:0x100b5ff0 | skill is the requirement block of the skill template (gCSkill_PS), spell the one of the magic property set (gCMagic_PS) when the template has one; att... | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1585 |
| `can_equip` (gCInventory_PS) | Game:0x100b7dc0 | equip_bonus is the result of the script hook OnPlayerGetEquipBonus(0, 0, quality) (added to every attribute requirement, floored at 0); missing_value(... | INTERNAL-ONLY | crates/gameplay/src/inventory/list.rs:1639 |
| `consume_items` (gCInventory_PS) | Game:0x100b7970 | apply the item's modifiers as one-off consumption (Consume) and activate its skills permanently. The caller removes the item afterwards with [Inventor... | WIRED-NOT-EXERCISED | crates/gameplay/src/inventory/list.rs:1668 |
| `compare_protection` (gCInventoryStack) | Game:0x100c2f10 | higher PROT_BLADE first. | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:95 |
| `compare_armor` (gCInventoryStack) | Game:0x100c2610 | gCInventoryStack::CompareArmor (Game:0x100c2610). | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:192 |
| `compare_recipe` (gCInventoryStack) | Game:0x100c26f0 | recipe order, then name. | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:229 |
| `execute_script` (gCItem_PS) | Game:0x100b9df0 | scripts guarded by a SetGameEvent line run only once per event; otherwise every line runs in order. (The original returns false in both cases and sets... | DEAD | crates/gameplay/src/inventory/script.rs:214 |
| `is_display_slot` (gCInventory_PS) | Game:0x100b34e0 | slots whose item is shown on the character (carry slots, armor, jewellery and the body parts below Helmet). | DEAD | crates/gameplay/src/inventory/slot.rs:97 |
| `is_skinned_slot` (gCInventory_PS) | Game:0x100b50b0 | the item in this slot replaces a skinned body part. | DEAD | crates/gameplay/src/inventory/slot.rs:216 |
| `is_permanent` (gCInventorySlot) | Game:0x100bcfd0 | spells and skills, or items flagged Permanent. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/inventory/stack.rs:128 |
| `is_equipment` (gCInventorySlot) | Game:0x100bd040 | weapons, armor and artefacts except letters. | DEAD | crates/gameplay/src/inventory/stack.rs:135 |
| `is_consumable` (gCInventorySlot) | Game:0x100bd0b0 | the alchemy category. | DEAD | crates/gameplay/src/inventory/stack.rs:146 |
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
| `open` (gCMover_PS) | Game:0x1a7780 | ignored while already opening. A closing mover reverses from where it is; instant jumps to the open pose. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/mover.rs:58 |
| `close` (gCMover_PS) | Game:0x1a7a70 | gCMover_PS::Close (Game:0x1a7a70). | WIRED-NOT-EXERCISED ? | crates/gameplay/src/mover.rs:86 |
| `toggle` (gCMover_PS) | Game:0x1a7130 | open what is closed or closing, close what is open or opening. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/mover.rs:113 |
| `motion_stopped` (gCMover_PS) | Game:0x1a7240 | the host calls it when the motion's play speed has dropped to zero (it ran to its end). With rest == 0 the finished motions end in the opposite state ... | WIRED-NOT-EXERCISED | crates/gameplay/src/mover.rs:123 |
| `attribute` (gCAttribute) | Game:0x183850 | base 100, no modifier. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/stats.rs:37 |
| `pool` (gCStat) | Game:0x1845f0 | value and maximum 100. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/stats.rs:46 |
| `value` (gCAttribute) | Game:0x183880 | base plus modifier. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/stats.rs:57 |
| `maximum` (gCAttribute) | Game:0x183890 | gCAttribute::GetMaximum / gCStat::GetMaximum (Game:0x183890 / 0x184630). | WIRED-NOT-EXERCISED | crates/gameplay/src/stats.rs:63 |
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

### world (entity admin, PVS, sectors, triggers, zones) (43)

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
| `enable` (gCSector) | Game:0x10192cd0 | The original asks the World_MCP entity's gCSectorPersistence_PS to switch (UpdateSector, which calls OnEnable and logs) and falls back to OnEnable its... | WIRED-NOT-EXERCISED ? | crates/world/src/sector.rs:180 |
| `contains_entity` (gCSector) | Game:0x10192b60 | is entity below a layer root of the sector? | DEAD | crates/world/src/sector.rs:221 |
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

### script helpers / predicates / stats (36)

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
| `start_say_defeat_comment` (StartSayDefeatComment) | S:51040 | a bystander comments on a knocked-out fighter. Only fights the bystander is not part of get a remark, and then only 30% of the time. | INTERNAL-ONLY | crates/script/src/speech.rs:143 |
| `start_say_confirm` (StartSayConfirm) | S:51680 | party members have three variants, everybody else "Sure". | WIRED-NOT-EXERCISED | crates/script/src/speech.rs:173 |
| `start_say_follow` (StartSayFollow) | S:518f0 | the companion's reaction to "follow me" (only when the player gives the order). | DEAD | crates/script/src/speech.rs:184 |
| `start_say_holding_position` (StartSayHoldingPosition) | S:517c0 | "Sure" for NPCs without party voices and for party members of type 3 (a hired fighter), "PartyWait" for the other companions. | WIRED-NOT-EXERCISED | crates/script/src/speech.rs:195 |
| `start_say_refuse_talk` (StartSayRefuseTalk) | S:52280 | a sleeping NPC says "SleepRefuse"; everybody else rotates through three refusals (what they are busy with, a polite excuse, a brush-off). The second a... | WIRED-NOT-EXERCISED | crates/script/src/speech.rs:262 |
| `start_say_crime_refuse` (StartSayCrimeRefuse) | S:524c0 | an NPC that knows about a crime of the player refuses to talk; after a theft the three steps are "DontStealAnything", "TheftRefuse", "GetLost", otherw... | INTERNAL-ONLY | crates/script/src/speech.rs:294 |
| `start_say_zone_refuse` (StartSayZoneRefuse) | S:52640 | an NPC in a zone hostile to the player refuses to talk. In a locked-down (Enclave-level) zone the first step names the owning enclave ("NoFriendOfORC"... | INTERNAL-ONLY | crates/script/src/speech.rs:310 |
| `start_say_doesnt_work` (StartSayDoesntWork) | S:6f660 | the player's grunt when an object cannot be used. | INTERNAL-ONLY | crates/script/src/speech.rs:339 |
| `start_say_nothing_to_get` (StartSayNothingToGet) | S:6f560 | the player's comment on an emptied container. | INTERNAL-ONLY | crates/script/src/speech.rs:346 |
| `boss_news_plan` (_AI_SayBossNews) | S:3c490 | the line plan of a boss telling the player about the crimes against his faction. After the opener "BadNews" the lines depend on the level; at level 3 ... | INTERNAL-ONLY | crates/script/src/speech.rs:486 |
| `crime_investigation_plan` (_AI_SayCrimeInvestigation) | S:3be50 | and no info command of the data names it (the info command table has no such entry: script_command_histogram of info-survey). Kept as a faithful port.... | DEAD | crates/script/src/speech.rs:530 |
| `guard_news_plan` (_AI_SayGuardNews) | S:3a3c0 | as a line plan [I]. The guard stops the player ("Halt"), names the crime he knows (murders/thefts counts of the enclave: "KnownMurder" or "KnownTheft"... | WIRED-NOT-EXERCISED | crates/script/src/speech.rs:556 |
| `diff_to_player_level` (GetDiffToPlayerLevel) | S:36eb0 | CurrentLevel(npc) - player level, 0 for the player. | DEAD | crates/script/src/stats.rs:68 |
| `set_attribute` (SetStrength) | S:33ae0, S:33ba0 | SetDexterity (S:33ba0), SetIntelligence (S:33c60), SetAlchemy (S:33d20), SetSmithing (S:33de0), SetTheft (S:33ea0): player only. | DEAD | crates/script/src/stats.rs:100 |
| `add_attribute` (AddStrength) | S:33f60, S:34060 | AddDexterity (S:34060), AddIntelligence (S:34160), AddAlchemy (S:34260), AddSmithing (S:34360), AddTheft (S:34460): adds to the player's base value an... | INTERNAL-ONLY | crates/script/src/stats.rs:110 |
| `set_protection` (SetProtectionBlades) | S:34c60, S:34d20 | SetProtectionImpact (S:34d20), SetProtectionMissile (S:34de0), SetProtectionFire (S:34ea0), SetProtectionIce (S:34f60), SetProtectionLightning (S:3502... | DEAD | crates/script/src/stats.rs:150 |
| `add_protection` (AddProtectionBlades) | S:350e0, S:351a0 | ...Impact (S:351a0), ...Missile (S:35260), ...Fire (S:35320), ...Ice (S:353e0), ...Lightning (S:354a0): player only. | DEAD | crates/script/src/stats.rs:161 |
| `set_pool_max` (SetHitPointsMax) | S:359f0, S:36080 | SetManaPointsMax (S:36080), SetStaminaPointsMax (S:367a0): writes the maximum and re-clamps the current value (HitPointsMax also floors the current va... | INTERNAL-ONLY | crates/script/src/stats.rs:271 |
| `add_pool_max` (AddHitPointsMax) | S:35c20, S:36260 | AddManaPointsMax (S:36260), AddStaminaPointsMax (S:36a20): the player's maximum grows (and the gain is printed as HP/MP/SP); no-op for NPCs. | DEAD | crates/script/src/stats.rs:341 |

### animation (EMotionFX wrapper, blending, attachments) (34)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `stop_slot` (eCWrapper_emfx2Actor) | Engine:0x10047910 | Zero fade removes the instance, otherwise it fades out like SwitchFadeOut. | DEAD | crates/animation/src/animator.rs:188 |
| `stop_at_loop_end` (eCWrapper_emfx2Actor) | Engine:0x100479c0 | eCWrapper_emfx2Actor::StopAtLoopEnd (Engine:0x100479c0). | INTERNAL-ONLY ? | crates/animation/src/animator.rs:199 |
| `set_target_weight` (eCWrapper_emfx2Actor) | Engine:0x10049ca0 | eCWrapper_emfx2Actor::SetTargetWeight (Engine:0x10049ca0). | INTERNAL-ONLY ? | crates/animation/src/animator.rs:207 |
| `set_blend_mode` (eCWrapper_emfx2Actor) | Engine:0x10049d10 | eCWrapper_emfx2Actor::SetBlendMode (Engine:0x10049d10). | INTERNAL-ONLY ? | crates/animation/src/animator.rs:215 |
| `is_slot_playing` (eCWrapper_emfx2Actor) | Engine:0x1004a040 | eCWrapper_emfx2Actor::IsActorPlaying (Engine:0x1004a040). | DEAD | crates/animation/src/animator.rs:239 |
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
| `set_visible` (eCVisualAnimationFactory) | Engine:0x1003a430 | eCVisualAnimationFactory::SetBodyPartVisible (Engine:0x1003a430). | WIRED-NOT-EXERCISED | crates/animation/src/body_parts.rs:103 |
| `is_visible` (eCVisualAnimationFactory) | Engine:0x1003a480 | false for unknown parts. | WIRED-NOT-EXERCISED ? | crates/animation/src/body_parts.rs:120 |
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

### ZS_* routine tasks (20)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `guard_attack_reason` (GetGuardAttackReason) | S:3d050 | why a guard attacks somebody whose crime it knows: a murderer is fought to the death (reason Revolution, 19), a thief with reason Theft, anybody else ... | INTERNAL-ONLY | crates/script/src/routine/dialog.rs:14 |
| `excuse` (_AI_Excuse) | S:3d130 | the player excuses a crime to a guard.  Needs Perk_Excuse. Crimes up to Attack always work; for theft and murder the guard rolls Random(130): above 11... | WIRED-NOT-EXERCISED ? | crates/script/src/routine/dialog.rs:38 |
| `bribe` (_AI_Bribe) | S:3d7d0 | the player offers a guard money to forget a crime.  Accepted (the dialog took the gold): the guard says LetsForgetThis, ThisTimeYoureLucky and NoMoreT... | WIRED-NOT-EXERCISED ? | crates/script/src/routine/dialog.rs:102 |
| `pick_pocket` (PickPocket) | S:3dd70 | one attempt. A caught thief is attacked (reason Theft); a successful one counts toward the victim enclave's thefts unless ExtendedContent is on and th... | WIRED-NOT-EXERCISED | crates/script/src/routine/dialog.rs:183 |
| `join_player_party` (JoinPlayerParty) | S:a8480 | message GO_PartyJoin, member type Party. | WIRED-NOT-EXERCISED | crates/script/src/routine/party.rs:49 |
| `join_player_mob` (JoinPlayerMob) | S:a8900 | message GO_MobJoin, member type Mob. | WIRED-NOT-EXERCISED | crates/script/src/routine/party.rs:55 |
| `join_player_slavery` (JoinPlayerSlavery) | S:a8d30 | message GO_SlaveJoin, member type Slave. | WIRED-NOT-EXERCISED | crates/script/src/routine/party.rs:61 |
| `dismiss_player_party` (DismissPlayerParty) | S:a8690 | message GO_PartyDismiss; a Controlled member also restarts its routine. A summoned creature is killed instead. | WIRED-NOT-EXERCISED | crates/script/src/routine/party.rs:92 |
| `dismiss_player_mob` (DismissPlayerMob) | S:a8b10 | message GO_MobDismiss. | WIRED-NOT-EXERCISED | crates/script/src/routine/party.rs:99 |
| `wait_player_party` (WaitPlayerParty) | S:a8f40 | the member waits where it stands (GO_SlaveDismiss message, Waiting set, ZS_WaitForPlayer). Summoned creatures ignore the call. | WIRED-NOT-EXERCISED | crates/script/src/routine/party.rs:105 |
| `can_join_player_party` (CanJoinPlayerParty) | S:a80a0 | summoned creatures always may; otherwise a waiting or out-of-range companion is dismissed to make room and an active, in-range one blocks, so at most ... | WIRED-NOT-EXERCISED | crates/script/src/routine/party.rs:120 |
| `start_guide_player` (StartGuidePlayer) | S:a9110 | the NPC becomes the player's guide (the player's PSParty leader is set to the NPC). | DEAD | crates/script/src/routine/party.rs:141 |
| `stop_guide_player` (StopGuidePlayer) | S:a9200 | only the current guide can stop guiding; the guide link is cleared and the stop time recorded. Returns false when npc is not the guide. | DEAD | crates/script/src/routine/party.rs:149 |
| `stop_bring_npc` (StopBringNPC) | S:a9370 | clears the dialog's PartyEnabled flag so the NPC can no longer be asked to join. | DEAD | crates/script/src/routine/party.rs:163 |
| `is_free_destination` (IsFreeDestination) | S:a9d00 | can npc use interact point point?  A point without an interaction property is never free; an owned point is free only for its owner; an anchor-backed ... | INTERNAL-ONLY | crates/script/src/routine/select.rs:136 |
| `select_routine_destination` (SelectRoutineDestination) | S:a7050 | pick the point the NPC should walk to for the current routine time.  Skips NPCs in a running arena fight, members of the player's party and NPCs that ... | DEAD | crates/script/src/routine/select.rs:245 |
| `send_to_arena` (SendToArena) | S:a7f80 | stop what the NPC does and, if it is within processing range, restart its routine (the arena task takes over through SelectRoutineTask).  The DLL refu... | DEAD | crates/script/src/routine/select.rs:450 |
| `rtn_monster` (Rtn_Monster) | S:86a10 | entry for monsters: just resume the routine. | DEAD | crates/script/src/routine/select.rs:467 |
| `rtn_dead` (Rtn_Dead) | S:86a80 | entry for dead NPCs: AI mode Dead and ZS_Dead. | WIRED-NOT-EXERCISED | crates/script/src/routine/select.rs:473 |
| `travelling_walk_mode` (GetTravellingWalkMode) | S:a9c00 | 2 (fast) for Minecrawler, Scavenger, Wolf, Bison, Rhino and Snapper, 1 otherwise. | DEAD | crates/script/src/routine/time.rs:82 |

### spell effects / casting (12)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `zs_nothing` (ZS_WaitForMeteor) | S:5a000 | and ZS_WaitForDawn: the DLL's bodies are empty. | INTERNAL-ONLY | crates/script/src/magic/area.rs:599 |
| `can_transform_into` (CanTransformInto) | S:5ea30 | any animal but a pig; otherwise the hero is told Impossible. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:104 |
| `magic_burn` (MagicBurn) | S:56e60 | names MagicBurn as a function; the only references are the script registry wrappers. Kept as a faithful port; see docs/research/WIRING_NPC_STATUS.md. ... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:203 |
| `magic_freeze` (MagicFreeze) | S:56f30 | names MagicFreeze as a function; the only references are the script registry wrappers. Kept as a faithful port; see docs/research/WIRING_NPC_STATUS.md... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:213 |
| `magic_disease` (MagicDisease) | S:57160 | a diseasable target loses all stamina and falls ill; either way it holds the caster responsible. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:233 |
| `magic_refresh` (MagicRefresh) | S:572d0 | Kept as a faithful port; see docs/research/WIRING_NPC_STATUS.md. Port of MagicRefresh (S:572d0): mana to the maximum. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:244 |
| `magic_keep_stamina` (MagicKeepStamina) | S:58b70 | names MagicKeepStamina as a function; the only references are the script registry wrappers. Kept as a faithful port; see docs/research/WIRING_NPC_STAT... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:296 |
| `magic_speed_up` (MagicSpeedUp) | S:58a30 | names MagicSpeedUp as a function; the only references are the script registry wrappers. Kept as a faithful port; see docs/research/WIRING_NPC_STATUS.m... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:310 |
| `magic_stun` (MagicStun) | S:58cb0 | names MagicStun as a function; the only references are the script registry wrappers. Kept as a faithful port; see docs/research/WIRING_NPC_STATUS.md. ... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:325 |
| `magic_dispel` (MagicDispel) | S:53460 | names MagicDispel as a function; the only references are the script registry wrappers. Kept as a faithful port; see docs/research/WIRING_NPC_STATUS.md... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:359 |
| `magic_summon_companion` (MagicSummonCompanion) | S:55b90 | names MagicSummonCompanion as a function; the only references are the script registry wrappers. Kept as a faithful port; see docs/research/WIRING_NPC_... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:669 |
| `return_to_body` (ReturnToBody) | S:58470 | the soul returns, the ghost marks are cleared. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:844 |

### ZS_Attack / combat AI (11)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `on_ai_move` (the OnAI_* animation-event callbacks) | S:38580, S:41e30 | the first call after the hit window opened moves the stage counter to 1 (damage strip on). Returns true exactly then. A hack counts a second stage (St... | DEAD | crates/script/src/combat/commands.rs:114 |
| `start_jump` (_AI_Jump / _AI_JumpBack) | S:46220, S:46560 | charge the stamina; returns the cost. | INTERNAL-ONLY | crates/script/src/combat/commands.rs:158 |
| `start_die` (_AI_Die) | S:3e510 | the dying NPC plays Die and is out of the fight. | WIRED-NOT-EXERCISED | crates/script/src/combat/commands.rs:183 |
| `start_get_up` (_AI_GetUpAttack / _AI_GetUpParade) | S:41bd0, S:41f20 | a knocked-down fighter attacks or parries on its way up; the pose becomes standing and the action the get-up move. | DEAD | crates/script/src/combat/commands.rs:207 |
| `start_reload` (_AI_Reload) | S:4a4f0 | reloading a crossbow restores the action to Reload. | DEAD | crates/script/src/combat/commands.rs:222 |
| `wait_free_loop` (ZS_WaitForFreeDestination_Loop) | S:42db0 | every second look whether the player has moved more than 1.5 m away from the destination point; then the routine continues. | INTERNAL-ONLY | crates/script/src/combat/misc.rs:126 |
| `end_goto` (EndGoto) | S:42880 | act on how a goto instruction ended.  Arrival clears "can be pushed while idle". A blocked destination (the player is in the way) makes the NPC wait (... | DEAD | crates/script/src/combat/misc.rs:248 |
| `handle_controlled_return_home` (HandleControlledReturnHome) | S:42bc0 | a controlled (mind-controlled) creature whose master is not the player any more and that stands on its destination becomes a plain creature again (Par... | DEAD | crates/script/src/combat/misc.rs:279 |
| `on_wait_for_player` (OnWaitForPlayer) | S:86d50 | a plain perception poll. | INTERNAL-ONLY | crates/script/src/combat/misc.rs:294 |
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

### damage pipeline (9)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `assess_cmd_attack` (AssessCmdAttack) | S:29e70 | a companion attacks what the player focuses. | INTERNAL-ONLY | crates/script/src/assess.rs:85 |
| `send_perception_cmd_attack` (SendPerceptionCmdAttack) | S:31ff0 | every companion of the player hears the order. | WIRED-NOT-EXERCISED | crates/script/src/assess.rs:96 |
| `assess_cmd_follow` (AssessCmdFollow) | S:56a80 | a companion that waits stops waiting and follows again. | INTERNAL-ONLY | crates/script/src/assess.rs:107 |
| `send_perception_cmd_follow` (SendPerceptionCmdFollow) | S:32170 | SendPerceptionCmdFollow (S:32170). | WIRED-NOT-EXERCISED | crates/script/src/assess.rs:118 |
| `assess_player_enter_zone` (AssessPlayerEnterZone) | S:30000 | the player stepped into a zone with an owner within 7 m of the witness: a calm witness goes and observes (ZS_ObserveZonePlayer). | INTERNAL-ONLY | crates/script/src/assess.rs:370 |
| `send_perception_enter_zone` (SendPerceptionEnterZone) | S:33170 | SendPerceptionEnterZone (S:33170). | DEAD | crates/script/src/assess.rs:391 |
| `send_perception_preach` (SendPerceptionPreach) | S:32cf0 | calm humanoids within 10 m of a preaching NPC stop and listen (ZS_Listen). | DEAD | crates/script/src/assess.rs:429 |
| `can_be_diseased` (CanBeDiseased) | - | CanBeDiseased: not with Perk_ImmuneToDisease. | INTERNAL-ONLY | crates/script/src/damage.rs:460 |
| `give_xp` (GiveXP) | S:4e160 | experience for receiver.  amount == -1 means "a kill or defeat of source": ambient creatures give 10, Pig and Cow 20, everything else max(10 * level, ... | WIRED-NOT-EXERCISED ? | crates/script/src/damage.rs:1152 |

### PS_Magic* (7)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `bs_book` (BS_BookOfFlood) | S:65180, S:656f0 | BS_BookOfRhobar (S:656f0) and BS_BookOfZuben: after half a second the hero reads two pages aloud (BookOf<Name>_<page>, the page counter advances by tw... | INTERNAL-ONLY | crates/script/src/player/magic.rs:494 |
| `bs_ancient_wisdom` (BS_AncientWisdom) | S:65080 | the bookstand of the ancient wisdom picks the book the hero has not finished: Flood, then Rhobar, then Zuben, and when all are read BS_NothingNew. | INTERNAL-ONLY | crates/script/src/player/magic.rs:540 |
| `bs_nothing_new` (BS_NothingNew) | S:64da0 | half a second, then one of three remarks (NothingNew_00..02), and on with the routine. | INTERNAL-ONLY | crates/script/src/player/magic.rs:552 |
| `bs_skill_book` (BS_Alchemy) | S:66a00 | and BS_Smith: after half a second a remark (AlchemyBook, SmithBook) and +3 Alchemy or Smithing (+5 without ExtendedContent); the stand is used up. | INTERNAL-ONLY | crates/script/src/player/magic.rs:574 |
| `bs_chronicles` (BS_Chronicles) | S:66190 | the Dargoth chronicles are read once; the game event Dargoth_Chronicles marks it. The five remarks and the quest hooks of the DLL are story data [U]; ... | INTERNAL-ONLY | crates/script/src/player/magic.rs:603 |
| `ps_ghost_jump` (PS_Ghost_Jump) | S:67a70 | one jump, then back to the loop. | INTERNAL-ONLY | crates/script/src/player/magic.rs:667 |
| `ps_ghost_summoner` (PS_GhostSummoner) | S:67c80 | and PS_GhostSummoner_Loop: the body the soul left behind is abandoned: hands down, standing, idle. | INTERNAL-ONLY | crates/script/src/player/magic.rs:689 |

### audio sequencers (5)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `select_sample` (eCResourceSoundList) | Engine:0x101bbb20, Game:0x10057f40 | and gCEffectCommandPlaySound::SelectSample (Game:0x10057f40): entry i is drawn with probability w_i / (N * max_w), so the remainder of the unit interv... | INTERNAL-ONLY | crates/audio/src/emitter.rs:190 |
| `voice_path` (gCEffectCommandPlayVoice) | Game:0x10059030 | the literal token (voice) is replaced by the speaker's Voice string and the whole path is lower-cased. | DEAD | crates/audio/src/emitter.rs:211 |
| `viseme_for_label` (eCVisualAnimationLipSync) | Engine:0x10027650 | case-insensitive lookup; unknown labels give the rest viseme. | DEAD | crates/audio/src/lipsync.rs:166 |
| `set_accept_triggers` (gCMusicSequencer) | - | gCMusicSequencer::SetAcceptTriggers: while off every trigger is ignored. | DEAD | crates/audio/src/sequencer.rs:327 |
| `execute_stop` (Execute) | - | all channels stop, events are cleared. | INTERNAL-ONLY | crates/audio/src/sequencer.rs:750 |

### render rules (5)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `area_factor` (eCFrustumFilter) | - | the projected area of the bounding sphere relative to the screen, pi * (r / z)^2 clamped to [0, 1], with z the depth along the camera forward axis (no... | DEAD | crates/render-rules/src/lib.rs:14 |
| `lod_input` (eCFrustumDatabase) | - | LoD input is the database's global LoD factor x area factor x entity LoD scale x entity-type LoD scale. | DEAD | crates/render-rules/src/lib.rs:23 |
| `lod_stage` (eCRenderContextMesh) | - | stages is the number of LoD meshes the visual has (0 or 1 means always stage 0). | DEAD | crates/render-rules/src/lib.rs:36 |
| `small_object_fade` (the non-ranged branch of eCFrustumFilter) | - | threshold = entity scale x database screen-cull factor x entity-type scale. Returns the fade-out value in [0, 1]; anything below 1 means the object is... | DEAD | crates/render-rules/src/lib.rs:86 |
| `intensity` (eCStaticPointLight_PS) | - | with t = d / range the smooth fall-off 2 (1 - 3 t^2 + 2 t^3) times the cosine of the incidence angle (param_2 = d cos). 0 beyond the range. | WIRED-NOT-EXERCISED | crates/render-rules/src/lib.rs:188 |

### perception / crime (5)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `get_player_crime` (GetPlayerCrime) | S:36fd0 | the crime the NPC remembers. | DEAD | crates/script/src/crime.rs:33 |
| `is_audible` (FUN_10009100) | - | "is audible": the entity is neither sneaking (Sneak or Parade pose) nor standing still. | INTERNAL-ONLY | crates/script/src/perception.rs:43 |
| `call_help` (CallHelp) | S:332e0 | caller was wronged by culprit; guards that are friendly toward the caller, not friendly toward the culprit and within 10 m of the culprit attack it wi... | WIRED-NOT-EXERCISED ? | crates/script/src/perception.rs:263 |
| `on_animal_sleep` (OnAnimalSleep) | S:89f20 | the callback of a sleeping animal: the player wakes it by coming close unless hidden. With Perk_Sneak_2 a sneaking player (or one in PS_Ranged) is nev... | WIRED-NOT-EXERCISED | crates/script/src/perception.rs:384 |
| `on_gate_guard` (OnGateGuard) | S:86c80 | the local callback of a gate guard: returns true once per [GATE_GUARD_POLL_SECS] of state time, when the caller should run the active perception scan. | WIRED-NOT-EXERCISED | crates/script/src/perception.rs:415 |

### PS_Interact* (4)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `is_basket` (IsBasket) | S:782e0 | the hero just opened a chest (action OpenChest) and the object is one of the harvestable "ChestPod" baskets, not a real chest. | INTERNAL-ONLY | crates/script/src/player/interact.rs:562 |
| `ps_interact_wait_for_exit` (PS_Interact_WaitForExit) | - | PS_Interact_WaitForExit: wait for the exit key, then stop and leave the interaction. | INTERNAL-ONLY | crates/script/src/player/interact.rs:571 |
| `ps_interact_door` (PS_Interact_Door) | S:70970 | a locked door is unlocked first; then the door swings open or shut, the witnesses are told, the use event is raised and the interaction ends. | INTERNAL-ONLY | crates/script/src/player/interact.rs:672 |
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

### nav (1)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `choose` (gCNavigationMap) | - | gCNavigationMap::DetectFinalTargetAsOffset: among the poses that lie inside the zone the character is in, take the one nearest to the character. | WIRED-NOT-EXERCISED ? | crates/nav/src/agent.rs:198 |

### PS_Interact boulder (1)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `ps_carry_stone` (PS_Interact_CarryStone) | S:77cc0 | take the stone in both hands and walk (movement mode 3); PS_Interact_CarryStone_Loop waits for the exit key and PS_Interact_CarryStone_End puts it dow... | INTERNAL-ONLY | crates/script/src/player/boulder.rs:178 |

### PS_Melee* (1)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `ps_melee_jump_back` (PS_Melee_JumpBack) | S:7fc20 | _AI_JumpBack against the opponent, no alignment. | INTERNAL-ONLY | crates/script/src/player/melee.rs:312 |

### sim (stats, combat, AI, host) (1)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `apply_stat` (the player memory entry points) | Game:0x14efa0 | apply an item modifier event to the hero. Characters that use the NPC formulas have no attribute store and ignore it. | INTERNAL-ONLY | crates/sim/src/actor.rs:420 |

## Ported functions called in play (calls over all scenarios)

| function | calls | scenarios | file |
| --- | ---: | --- | --- |
| `color_factor` | 121947773 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/sim.rs:54 |
| `calc_color` | 121947773 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/sim.rs:489 |
| `tick_particle` | 121791359 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/sim.rs:308 |
| `color_scale` | 117442246 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/scale.rs:80 |
| `blend_factor` | 89376858 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/environment/src/lib.rs:241 |
| `vector_scale` | 71371646 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/scale.rs:62 |
| `calc_frame` | 36384744 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/sim.rs:96 |
| `is_linked` | 23663780 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/stack.rs:152 |
| `can_do` | 7558517 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/combat/moves.rs:232 |
| `update` | 6440386 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/sim.rs:202 |
| `process` | 5344065 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/effect.rs:33 |
| `has_finished` | 5344065 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/effect.rs:49 |
| `float_scale` | 4530510 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/scale.rs:52 |
| `spawn` | 3536364 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/sim.rs:378 |
| `process` | 3078112 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/sim.rs:254 |
| `class_priority` | 2309770 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/focus.rs:161 |
| `is_valid` | 2309770 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/focus.rs:173 |
| `is_playing` | 1988167 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/blend.rs:310 |
| `run_stop_prevention` | 1960323 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/blend.rs:393 |
| `is_enabled` | 1827475 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/sector.rs:145 |
| `process` | 1823669 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/attach.rs:138 |
| `in_range2` | 1724049 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/focus.rs:180 |
| `focus_check` | 1724049 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/focus.rs:354 |
| `angles` | 1723337 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/focus.rs:246 |
| `add_items` | 1719426 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/list.rs:451 |
| `in_angles` | 1691687 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/focus.rs:188 |
| `new` | 1597549 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/stack.rs:82 |
| `routine_time` | 1525825 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/routine/time.rs:44 |
| `create_items` | 1493790 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/list.rs:489 |
| `boost` | 1317029 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/physics/src/relax.rs:8 |
| `at` | 1126912 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/formats/src/weather.rs:35 |
| `at@2` | 1126912 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/formats/src/weather.rs:48 |
| `reset` | 740911 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/sim.rs:167 |
| `priority` | 665064 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/focus.rs:196 |
| `species_can_sidestep` | 522254 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/combat/moves.rs:190 |
| `max_range` | 427912 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/focus.rs:132 |
| `page_flags` | 309143 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/ui/src/manager.rs:89 |
| `find_focus` | 285220 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/focus.rs:383 |
| `clip_fog` | 281602 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/environment/src/lib.rs:392 |
| `political_attitude` | 242356 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/predicates.rs:489 |
| `calculate_rect_indices` | 231009 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/pvs.rs:96 |
| `process` | 231009 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/pvs.rs:257 |
| `item_gold_value` | 225750 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/stack.rs:260 |
| `combined_quality` | 215597 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/stack.rs:254 |
| `score_move` | 207232 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/combat/moves.rs:436 |
| `offer` | 207232 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/combat/moves.rs:653 |
| `get_attitude` | 188985 | bow_faithful, bow_pose, combat_basic, controls_basic, crime_theft, dia | crates/script/src/attitude.rs:171 |
| `outlaw_attitude` | 187786 | bow_faithful, bow_pose, combat_basic, controls_basic, crime_theft, dia | crates/script/src/attitude.rs:17 |
| `has_lower_priority` | 175924 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/focus.rs:331 |
| `start_due_events` | 150991 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/sequencer.rs:642 |
| `environment` | 150918 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/ambient.rs:134 |
| `dominant_environment` | 150918 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/ambient.rs:168 |
| `process` | 150918 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/ambient.rs:221 |
| `update_envelope` | 150918 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/sequencer.rs:818 |
| `update_channels` | 150918 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/sequencer.rs:862 |
| `pick_point` | 150918 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/camera/src/aim.rs:32 |
| `tick` | 150918 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/ui/src/texteffect.rs:239 |
| `water_beds` | 150917 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/ambient_play.rs:168 |
| `bone` | 150917 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/slot.rs:172 |
| `draw_multiplier` | 150917 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/projectile/src/aim.rs:31 |
| `from_state` | 140864 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/environment/src/lib.rs:51 |
| `apply` | 140864 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/environment/src/lib.rs:175 |
| `sun_state` | 140801 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/environment/src/lib.rs:323 |
| `hemisphere_sh` | 140801 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/environment/src/lib.rs:403 |
| `weather_light_terms` | 140801 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/environment/src/lib.rs:426 |
| `look_counts` | 116673 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/input/src/control.rs:113 |
| `cull` | 110290 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/admin.rs:183 |
| `find_stack_index_by_slot` | 105421 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/list.rs:629 |
| `step` | 99610 | doors, npc_archer, shaman_duel, spell_zoo, teleport, walk_to_reddock,  | crates/audio/src/emitter.rs:119 |
| `update_child_dependencies` | 94211 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/entity.rs:389 |
| `find_stack` | 88355 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/list.rs:638 |
| `update_all` | 80094 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/sector.rs:269 |
| `update_processing_range` | 80091 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/runtime/src/core.rs:780 |
| `process` | 80091 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/admin.rs:300 |
| `flush_kills` | 80091 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/admin.rs:455 |
| `recalibrate` | 75022 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/pvs.rs:379 |
| `weight_sphere` | 69546 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/render-rules/src/lib.rs:165 |
| `species_attitude` | 51696 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/attitude.rs:33 |
| `generate_plunder` | 51600 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/treasure.rs:138 |
| `is_in_fist_mode` | 45883 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/predicates.rs:282 |
| `generate_plunder_inventory` | 45838 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/treasure.rs:332 |
| `on_watch_fight` | 45161 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/combat/react.rs:498 |
| `hand_class` | 42685 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/helpers.rs:44 |
| `weapon_mode` | 42638 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/helpers.rs:103 |
| `process` | 41001 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/trigger.rs:932 |
| `is_humanoid` | 40092 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/predicates.rs:21 |
| `can_receive_perception_from` | 39000 | combat_basic, controls_basic, crime_theft, doors, lockpick, loot_chest | crates/script/src/perception.rs:50 |
| `wait_for_flee_loop` | 35183 | routine_zoo, sector_switch, shaman_duel, spell_zoo | crates/script/src/combat/attack.rs:681 |
| `play_slot` | 31035 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/animator.rs:148 |
| `new` | 31035 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/blend.rs:272 |
| `assess_fight_sound` | 29107 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/assess.rs:165 |
| `switch_fade_out` | 27844 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/animator.rs:175 |
| `switch_fade_out` | 27844 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/blend.rs:353 |
| `is_boss` | 25923 | combat_basic, controls_basic, crime_theft, dialog_quest, melee_faithfu | crates/script/src/predicates.rs:328 |
| `on_attack_turn` | 23668 | combat_basic, crime_theft, melee_faithful, npc_orc1, npc_vs_npc, routi | crates/script/src/combat/attack.rs:653 |
| `route` | 23443 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, dia | crates/input/src/player.rs:246 |
| `magic_input` | 23443 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, dia | crates/input/src/player.rs:507 |
| `use_type` | 19922 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/stack.rs:116 |
| `combat_move_length` | 19750 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/combat/range.rs:7 |
| `attack_range` | 19750 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/combat/range.rs:76 |
| `select_combat_move` | 18049 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/combat/select.rs:27 |
| `max_angles` | 15825 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, doo | crates/gameplay/src/focus.rs:145 |
| `follow_time` | 15423 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/predicates.rs:182 |
| `fai_active` | 15033 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/combat/select.rs:67 |
| `dst_volume` | 15016 | combat_basic, controls_basic, dialog_quest, lockpick, melee_faithful,  | crates/audio/src/music.rs:73 |
| `src_volume` | 14976 | combat_basic, controls_basic, dialog_quest, lockpick, melee_faithful,  | crates/audio/src/music.rs:62 |
| `build_situation` | 14542 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/combat/moves.rs:86 |
| `crowd_on_target` | 14538 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/combat/moves.rs:130 |
| `aggression_weight` | 14538 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/combat/moves.rs:147 |
| `on_attack_goto` | 14023 | combat_basic, crime_theft, melee_faithful, npc_orc1, npc_vs_npc, routi | crates/script/src/combat/attack.rs:530 |
| `evaluate` | 13960 | combat_basic, controls_basic, crime_theft, dialog_quest, lockpick, mel | crates/animation/src/face.rs:201 |
| `can_parade_move_of` | 13863 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/damage.rs:598 |
| `pick` | 13846 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/combat/moves.rs:697 |
| `choose_full` | 13789 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/combat/moves.rs:773 |
| `ps_normal_loop` | 12438 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/player/normal.rs:101 |
| `equip_slots` | 11956 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/slot.rs:140 |
| `equip_slots` | 11956 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/stack.rs:122 |
| `packet` | 11463 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/render-rules/src/lib.rs:200 |
| `find_other_target` | 9426 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/combat/select.rs:277 |
| `on_attack` | 8508 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/combat/attack.rs:44 |
| `list_count` | 6056 | trade_click, trade_esc | crates/gameplay/src/trade.rs:23 |
| `pool_percent` | 5474 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/stats.rs:218 |
| `unlink_stack_from_slot` | 5189 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/list.rs:929 |
| `pool` | 4983 | bow_pose, combat_basic, controls_basic, crime_theft, magic_faithful, m | crates/script/src/stats.rs:171 |
| `unlink_default_item_from_slot` | 4974 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/list.rs:841 |
| `link_stack_to_slot` | 4974 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/list.rs:944 |
| `set_transaction_amount` | 4821 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/list.rs:1317 |
| `set_transaction_amount` | 4821 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/stack.rs:278 |
| `equip_stack` | 4506 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/list.rs:1009 |
| `is_equipped_in_primary` | 4506 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/stack.rs:158 |
| `is_equipped_in_alternative` | 4506 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/stack.rs:164 |
| `is_equipped` | 4506 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/stack.rs:170 |
| `attitude_to_player` | 4490 | bow_faithful, bow_pose, combat_basic, controls_basic, crime_theft, dia | crates/script/src/attitude.rs:108 |
| `get_cast_action` | 4085 | bow_pose, magic_faithful, spell_zoo, staff_parade, teleport | crates/script/src/magic/cast.rs:64 |
| `choose_weapon_mode` | 3982 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/combat/weapon.rs:52 |
| `select_weapon_mode` | 3982 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/combat/weapon.rs:163 |
| `is_guard` | 3654 | combat_basic, controls_basic, crime_theft, dialog_quest, melee_faithfu | crates/script/src/predicates.rs:334 |
| `lookup` | 3583 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/efm.rs:97 |
| `read_grid` | 3449 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/formats/src/terrain/vegetation.rs:247 |
| `trade_unit_price` | 3329 | trade_click, trade_esc | crates/sim/src/trade.rs:46 |
| `start` | 3313 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/effect.rs:17 |
| `apply_stats` | 3094 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/list.rs:1427 |
| `hang_around` | 3051 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/routine/work.rs:1827 |
| `must_split_before_linking` | 2944 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/stack.rs:196 |
| `cache_in` | 2919 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/entity.rs:517 |
| `accumulate_key_direction` | 2821 | bow_pose, combat_basic, controls_basic, focus_steer, melee_faithful | crates/gameplay/src/focus.rs:281 |
| `on_attack_go_back` | 2640 | combat_basic, crime_theft, melee_faithful, npc_vs_npc, sector_switch,  | crates/script/src/combat/attack.rs:622 |
| `is_ambient_creature` | 2566 | combat_basic, crime_theft, melee_faithful, routine_zoo, sector_switch, | crates/script/src/predicates.rs:55 |
| `start_stand_up` | 2535 | combat_basic, controls_basic, crime_theft, dialog_quest, melee_faithfu | crates/script/src/combat/commands.rs:149 |
| `can_attack` | 2499 | combat_basic, crime_theft, melee_faithful, sector_switch, shaman_duel, | crates/script/src/perception.rs:83 |
| `assess_target` | 2499 | combat_basic, crime_theft, melee_faithful, sector_switch, shaman_duel, | crates/script/src/perception.rs:95 |
| `set_attack_reason` | 2498 | combat_basic, crime_theft, melee_faithful, sector_switch, shaman_duel, | crates/script/src/crime.rs:39 |
| `memorize_player_crime` | 2498 | combat_basic, crime_theft, melee_faithful, sector_switch, shaman_duel, | crates/script/src/crime.rs:63 |
| `assess_attack` | 2498 | combat_basic, crime_theft, melee_faithful, sector_switch, shaman_duel, | crates/script/src/perception.rs:151 |
| `update` | 2386 | bow_faithful, bow_pose, shaman_duel | crates/projectile/src/aim.rs:154 |
| `add` | 2367 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/attach.rs:45 |
| `remove` | 2367 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/attach.rs:76 |
| `primary` | 2260 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, dia | crates/input/src/player.rs:324 |
| `on_watch_fight_goto` | 2165 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/combat/react.rs:609 |
| `weakness` | 2052 | bow_faithful, bow_pose, combat_basic, controls_basic, crime_theft, mag | crates/script/src/damage.rs:122 |
| `crosshair_size` | 1926 | bow_faithful, bow_pose, shaman_duel | crates/projectile/src/aim.rs:37 |
| `notify_enclave` | 1860 | routine_reentry, routine_zoo, sector_switch, shaman_duel, spell_zoo | crates/script/src/crime.rs:214 |
| `task_for_use_type` | 1632 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/routine/select.rs:68 |
| `select_routine_task` | 1632 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/routine/select.rs:99 |
| `fighters_on` | 1594 | combat_basic, controls_basic, melee_faithful, npc_archer, npc_orc1, np | crates/script/src/combat/attack.rs:136 |
| `find_sounds` | 1582 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/ambient.rs:144 |
| `select_sound` | 1582 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/ambient.rs:157 |
| `update_combat_state` | 1535 | combat_basic, controls_basic, melee_faithful, npc_archer, npc_orc1, np | crates/script/src/combat/attack.rs:151 |
| `continue_routine` | 1511 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/routine/select.rs:366 |
| `ps_magic_loop` | 1438 | bow_pose, magic_faithful, spell_zoo, staff_parade, teleport | crates/script/src/player/magic.rs:113 |
| `speed_cm_s` | 1384 | bow_faithful, bow_pose, magic_faithful, npc_archer, sector_switch, sha | crates/projectile/src/params.rs:339 |
| `can_move` | 1363 | trade_click, trade_esc | crates/gameplay/src/trade.rs:57 |
| `is_disposable` | 1363 | trade_click, trade_esc | crates/gameplay/src/inventory/stack.rs:189 |
| `intro` | 1295 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/blend.rs:116 |
| `extro` | 1295 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/animation/src/blend.rs:125 |
| `observe_loop` | 1269 | combat_basic, controls_basic, routine_zoo, teleport | crates/script/src/combat/react.rs:742 |
| `fai_ranged` | 1264 | npc_archer, sector_switch, shaman_duel, spell_zoo | crates/script/src/combat/select.rs:185 |
| `animation_speed` | 1213 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/combat/commands.rs:263 |
| `level` | 1153 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/stats.rs:24 |
| `change_enclave_status` | 1117 | sector_switch, spell_zoo | crates/script/src/crime.rs:158 |
| `assure_items` | 1106 | craft_anvil, npc_archer, quick_slots, routine_zoo, sector_switch, sham | crates/gameplay/src/inventory/list.rs:526 |
| `fai_passive` | 1061 | sector_switch, spell_zoo | crates/script/src/combat/select.rs:132 |
| `has_dangerous_weapon` | 988 | combat_basic, controls_basic, melee_faithful, routine_zoo, shaman_duel | crates/script/src/predicates.rs:304 |
| `on_react_to_weapon` | 984 | combat_basic, controls_basic, melee_faithful, routine_zoo, teleport | crates/script/src/combat/react.rs:185 |
| `hold_stack_right_hand` | 974 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/list.rs:1104 |
| `power_rank` | 966 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/damage.rs:225 |
| `category` | 927 | bow_faithful, bow_pose, controls_basic, craft_anvil, magic_faithful, m | crates/gameplay/src/inventory/stack.rs:110 |
| `hold_stack_left_hand` | 921 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/list.rs:1122 |
| `hold_stacks` | 921 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/list.rs:1141 |
| `do_logical_damage` | 875 | bow_faithful, bow_pose, combat_basic, controls_basic, crime_theft, mag | crates/script/src/damage.rs:1037 |
| `is_in_parade_mode` | 865 | bow_pose, combat_basic, controls_basic, crime_theft, focus_steer, mele | crates/script/src/damage.rs:642 |
| `level_max` | 845 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/stats.rs:31 |
| `steer` | 786 | bow_faithful, bow_pose, magic_faithful, npc_archer, sector_switch, sha | crates/projectile/src/flight.rs:240 |
| `end_time` | 784 | bow_faithful, bow_pose, combat_basic, magic_faithful, npc_archer, sect | crates/fx/src/efm.rs:75 |
| `start_stumble` | 748 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/combat/commands.rs:189 |
| `begin_melee_command` | 698 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/combat/commands.rs:423 |
| `set_pool` | 686 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/stats.rs:237 |
| `add_stamina_points` | 682 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/stats.rs:327 |
| `can_see` | 660 | combat_basic, crime_theft, doors, lockpick, routine_zoo, shaman_duel,  | crates/script/src/perception.rs:29 |
| `attribute` | 633 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/stats.rs:87 |
| `on_enter_processing_range` | 617 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/trigger.rs:451 |
| `on_cache_in` | 617 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/world/src/trigger.rs:472 |
| `armor_protection` | 609 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/damage.rs:162 |
| `assess_hit` | 609 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/damage.rs:723 |
| `protection` | 609 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/stats.rs:121 |
| `send_perception_fight_sound` | 596 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/assess.rs:233 |
| `shoot` | 590 | bow_faithful, bow_pose, magic_faithful, npc_archer, sector_switch, sha | crates/projectile/src/flight.rs:256 |
| `ps_melee_loop` | 577 | bow_pose, combat_basic, controls_basic, focus_steer, melee_faithful | crates/script/src/player/melee.rs:160 |
| `start_say_aargh` | 576 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/speech.rs:208 |
| `shoot` | 571 | bow_faithful, bow_pose, npc_archer, sector_switch, shaman_duel, spell_ | crates/script/src/player/ranged.rs:35 |
| `start_shoot` | 564 | npc_archer, routine_zoo, sector_switch, shaman_duel, spell_zoo | crates/script/src/combat/commands.rs:171 |
| `npc_arrow` | 563 | npc_archer, sector_switch, shaman_duel, spell_zoo | crates/projectile/src/launch.rs:52 |
| `on_enable` | 550 | menu_save_load, quick_slots, sector_switch | crates/world/src/sector.rs:189 |
| `current_level` | 508 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/stats.rs:45 |
| `assess_murder` | 499 | shaman_duel, spell_zoo | crates/script/src/assess.rs:245 |
| `move_strength` | 483 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/damage.rs:201 |
| `defence_value` | 483 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/damage.rs:260 |
| `can_parade` | 483 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_orc1, n | crates/script/src/damage.rs:648 |
| `start_cast_phase` | 472 | bow_pose, magic_faithful, shaman_duel, spell_zoo, teleport | crates/script/src/magic/cast.rs:147 |
| `assess_warn` | 459 | combat_basic, melee_faithful, routine_zoo | crates/script/src/assess.rs:37 |
| `combat_walk_mode` | 416 | combat_basic, crime_theft, melee_faithful, npc_orc1, npc_vs_npc, routi | crates/script/src/combat/range.rs:104 |
| `fai_magic` | 409 | shaman_duel, spell_zoo | crates/script/src/combat/select.rs:204 |
| `compare` | 395 | ui_screens | crates/gameplay/src/inventory/order.rs:242 |
| `on_watch_fight_go_back` | 370 | combat_basic, controls_basic, crime_theft, melee_faithful, npc_archer, | crates/script/src/combat/react.rs:576 |
| `can_be_killed` | 282 | combat_basic, controls_basic, melee_faithful, npc_vs_npc, sector_switc | crates/script/src/damage.rs:387 |
| `fai_misted` | 282 | spell_zoo | crates/script/src/combat/select.rs:253 |
| `find_skill_stack_index` | 280 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/list.rs:1479 |
| `activate_skill` | 273 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/list.rs:1502 |
| `find_stack_index` | 238 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/gameplay/src/inventory/list.rs:561 |
| `movement_mode_changed` | 227 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/sim/src/moves.rs:67 |
| `set_current_destination_point` | 224 | combat_basic, dialog_quest, menu_save_load, quick_slots, routine_reent | crates/script/src/routine/select.rs:170 |
| `ps_ranged_aim_loop` | 213 | bow_faithful, bow_pose, shaman_duel | crates/script/src/player/ranged.rs:283 |
| `build_lipsync_clip` | 206 | combat_basic, controls_basic, crime_theft, dialog_quest, lockpick, mel | crates/animation/src/face.rs:92 |
| `begin_parade_command` | 195 | combat_basic, controls_basic, melee_faithful, npc_orc1, npc_vs_npc, ro | crates/script/src/combat/commands.rs:559 |
| `assess_use_interact_object` | 188 | lockpick, loot_chest, world_chest | crates/script/src/assess.rs:336 |
| `is_upset` | 166 | combat_basic, crime_theft, melee_faithful, routine_zoo, sector_switch, | crates/script/src/predicates.rs:393 |
| `ps_magic` | 159 | bow_pose, magic_faithful, spell_zoo, staff_parade, teleport | crates/script/src/player/magic.rs:66 |
| `find_fragment` | 157 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/sequencer.rs:440 |
| `select` | 157 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/sequencer.rs:484 |
| `unlink_stack_at` | 157 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, mag | crates/gameplay/src/inventory/list.rs:914 |
| `unapply_stats` | 157 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, mag | crates/gameplay/src/inventory/list.rs:1454 |
| `draw_multiplier` | 149 | bow_faithful, bow_pose, shaman_duel | crates/script/src/player/ranged.rs:24 |
| `ps_ranged_power_aim` | 149 | bow_faithful, bow_pose, shaman_duel | crates/script/src/player/ranged.rs:311 |
| `sleep_loop` | 147 | routine_zoo | crates/script/src/routine/work.rs:1549 |
| `link_default_item_to_slot` | 143 | bow_faithful, bow_pose, controls_basic, craft_anvil, magic_faithful, m | crates/gameplay/src/inventory/list.rs:831 |
| `stack_use_type` | 137 | bow_faithful, bow_pose, controls_basic, craft_anvil, magic_faithful, m | crates/gameplay/src/inventory/list.rs:653 |
| `equip_stack_to_slot` | 137 | bow_faithful, bow_pose, controls_basic, craft_anvil, magic_faithful, m | crates/gameplay/src/inventory/list.rs:990 |
| `right_equip_slot` | 136 | bow_pose, controls_basic, craft_anvil, magic_faithful, melee_faithful, | crates/gameplay/src/inventory/list.rs:1068 |
| `is_equipped_to_slot` | 135 | bow_faithful, bow_pose, controls_basic, craft_anvil, magic_faithful, m | crates/gameplay/src/inventory/stack.rs:183 |
| `follow_distance` | 127 | routine_zoo | crates/script/src/routine/follow.rs:28 |
| `assess_take_item` | 125 | crime_theft, doors | crates/script/src/perception.rs:222 |
| `melee_primary` | 123 | combat_basic, controls_basic, melee_faithful | crates/input/src/player.rs:361 |
| `is_in_parade_pose` | 119 | combat_basic, melee_faithful, npc_vs_npc, sector_switch, spell_zoo | crates/script/src/damage.rs:622 |
| `arena_loop` | 118 | routine_zoo | crates/script/src/combat/misc.rs:68 |
| `exit_time` | 113 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/sequencer.rs:563 |
| `find_transition` | 113 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/sequencer.rs:598 |
| `can_heal_self` | 110 | combat_basic, routine_zoo, spell_zoo | crates/script/src/routine/select.rs:353 |
| `secondary` | 108 | bow_faithful, combat_basic, melee_faithful, staff_parade | crates/input/src/player.rs:447 |
| `quality_bonus` | 101 | combat_basic, controls_basic, melee_faithful, shaman_duel, spell_zoo | crates/script/src/damage.rs:289 |
| `skill_bonus` | 101 | combat_basic, controls_basic, melee_faithful, shaman_duel, spell_zoo | crates/script/src/damage.rs:305 |
| `ps_magic_cast` | 101 | bow_pose, magic_faithful, spell_zoo, teleport | crates/script/src/player/magic.rs:178 |
| `assess_player_zone` | 99 | combat_basic, routine_zoo, shaman_duel | crates/script/src/assess.rs:399 |
| `assess_active_perception` | 98 | combat_basic, routine_zoo, shaman_duel | crates/script/src/assess.rs:463 |
| `assess_player_sneaking` | 98 | combat_basic, routine_zoo, shaman_duel | crates/script/src/perception.rs:290 |
| `assess_player_weapon` | 98 | combat_basic, routine_zoo, shaman_duel | crates/script/src/perception.rs:314 |
| `caption` | 96 | menu_save_load | crates/ui/src/files.rs:94 |
| `set_constraint` | 95 | combat_basic, controls_basic, crime_theft, dialog_quest, melee_faithfu | crates/animation/src/lookat.rs:85 |
| `is_droppable_weapon` | 94 | combat_basic, routine_reentry, routine_zoo, shaman_duel, spell_zoo | crates/script/src/predicates.rs:263 |
| `ps_magic_reload` | 94 | bow_pose, magic_faithful, spell_zoo, teleport | crates/script/src/player/magic.rs:318 |
| `area_damage_tick` | 89 | spell_zoo | crates/script/src/magic/area.rs:324 |
| `assess_defeat` | 88 | combat_basic | crates/script/src/assess.rs:303 |
| `compare_spell` | 86 | ui_screens | crates/gameplay/src/inventory/order.rs:214 |
| `los_for_attack` | 84 | sector_switch, spell_zoo | crates/script/src/combat/select.rs:174 |
| `execute_fragment` | 83 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/sequencer.rs:673 |
| `trigger_follow` | 83 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/sequencer.rs:771 |
| `lock_holds` | 82 | hud_focus | crates/gameplay/src/focus.rs:437 |
| `has_staff_magic_parade` | 79 | shaman_duel, spell_zoo | crates/script/src/damage.rs:675 |
| `can_parade_magic` | 79 | shaman_duel, spell_zoo | crates/script/src/damage.rs:689 |
| `ps_interact_loop` | 79 | craft_anvil, sit_pose, world_fry | crates/script/src/player/interact.rs:540 |
| `is_evil` | 75 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, sector_switch, sha | crates/script/src/predicates.rs:38 |
| `cast_begin` | 75 | bow_pose, magic_faithful, shaman_duel, spell_zoo, teleport | crates/script/src/magic/cast.rs:245 |
| `trigger` | 74 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/audio/src/sequencer.rs:349 |
| `cast_execute` | 74 | bow_pose, magic_faithful, shaman_duel, spell_zoo, teleport | crates/script/src/magic/cast.rs:269 |
| `can_be_poisoned` | 73 | combat_basic, melee_faithful, npc_orc1, npc_vs_npc, sector_switch, sha | crates/script/src/damage.rs:454 |
| `ps_ranged_walk_loop` | 73 | bow_faithful, bow_pose, shaman_duel | crates/script/src/player/ranged.rs:217 |
| `add_mana_points` | 70 | bow_pose, magic_faithful, shaman_duel, spell_zoo, teleport | crates/script/src/stats.rs:312 |
| `start_say_attack_reason` | 68 | combat_basic, crime_theft, melee_faithful, routine_zoo, sector_switch, | crates/script/src/speech.rs:91 |
| `ps_normal` | 62 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/player/normal.rs:72 |
| `on_entity_processing_changed` | 61 | sector_switch | crates/world/src/admin.rs:249 |
| `on_flee_go_back` | 59 | routine_zoo | crates/script/src/combat/react.rs:436 |
| `on_revolution_attack` | 58 | routine_zoo | crates/script/src/combat/misc.rs:168 |
| `on_react_to_damage` | 58 | routine_zoo | crates/script/src/combat/react.rs:87 |
| `on_flee_goto_fleepoint` | 58 | routine_zoo | crates/script/src/combat/react.rs:385 |
| `on_guide_player` | 58 | routine_zoo | crates/script/src/routine/follow.rs:238 |
| `is_gate_guard` | 57 | combat_basic, controls_basic, routine_zoo, shaman_duel, spell_zoo, tel | crates/script/src/predicates.rs:346 |
| `on_arena` | 57 | routine_zoo | crates/script/src/combat/misc.rs:54 |
| `on_react_to_zone` | 57 | routine_zoo | crates/script/src/combat/react.rs:268 |
| `choose_offensive` | 56 | sector_switch, spell_zoo | crates/script/src/combat/moves.rs:807 |
| `threaten_loop` | 53 | routine_zoo, spell_zoo, walk_to_reddock | crates/script/src/combat/react.rs:328 |
| `can_cast` | 52 | bow_pose, magic_faithful, spell_zoo, teleport | crates/script/src/magic/cast.rs:99 |
| `long_work_secs` | 51 | quick_slots, routine_zoo, sleep_bed, spell_zoo, world_routines | crates/script/src/helpers.rs:114 |
| `set_page` | 51 | craft_anvil, dialog_quest, lockpick, loot_chest, menu_save_load, teach | crates/ui/src/manager.rs:199 |
| `reset_all` | 49 | combat_basic, routine_reentry, routine_zoo, shaman_duel, spell_zoo | crates/script/src/down.rs:50 |
| `ps_interact_chest_loop` | 48 | lockpick, loot_chest, world_chest | crates/script/src/player/interact.rs:653 |
| `can_parade_missile` | 47 | npc_archer, sector_switch, shaman_duel | crates/script/src/damage.rs:697 |
| `drop_hand_items` | 47 | combat_basic, routine_reentry, routine_zoo, shaman_duel, spell_zoo | crates/script/src/down.rs:59 |
| `remove_non_combat_items` | 47 | combat_basic, routine_reentry, routine_zoo, shaman_duel, spell_zoo | crates/script/src/down.rs:71 |
| `clean_up_plunder` | 47 | combat_basic, routine_reentry, routine_zoo, shaman_duel, spell_zoo | crates/script/src/down.rs:164 |
| `print` | 46 | combat_basic, craft_anvil, crime_theft, dialog_quest, doors, lockpick, | crates/ui/src/dialogmenu.rs:209 |
| `kill` | 45 | routine_reentry, routine_zoo, shaman_duel, spell_zoo | crates/script/src/down.rs:120 |
| `magic_instant` | 45 | spell_zoo, teleport | crates/script/src/magic/cast.rs:296 |
| `parse` | 43 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/fx/src/efm.rs:107 |
| `on_follow_npc` | 43 | routine_zoo | crates/script/src/routine/follow.rs:228 |
| `rtn_player` | 43 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/script/src/routine/select.rs:480 |
| `from_ini` | 43 | bow_faithful, bow_pose, combat_basic, controls_basic, craft_anvil, cri | crates/ui/src/log.rs:90 |
| `on_follow_player` | 42 | routine_zoo | crates/script/src/routine/follow.rs:185 |
| `execute_transition` | 40 | combat_basic, controls_basic, dialog_quest, lockpick, melee_faithful,  | crates/audio/src/sequencer.rs:732 |
| `on_ransack_body` | 40 | combat_basic, routine_zoo, shaman_duel | crates/script/src/combat/react.rs:927 |
| `on_attack_strafe` | 36 | routine_zoo | crates/script/src/combat/attack.rs:642 |
| `zs_magic_sleep_loop` | 36 | routine_zoo | crates/script/src/magic/area.rs:538 |
| `send_perception_transform` | 36 | spell_zoo | crates/script/src/magic/effects.rs:718 |
| `is_slave` | 34 | combat_basic, shaman_duel | crates/script/src/predicates.rs:352 |
| `is_temporary_friendly_to_player` | 34 | combat_basic, shaman_duel | crates/script/src/predicates.rs:384 |
| `can_burn` | 33 | spell_zoo | crates/script/src/damage.rs:425 |
| `assess_mist` | 33 | spell_zoo | crates/script/src/magic/area.rs:271 |
| `in_state` | 31 | combat_basic, controls_basic, melee_faithful, npc_archer, npc_orc1, sh | crates/audio/src/sequencer.rs:333 |
| `delete_stack` | 30 | combat_basic, craft_anvil, shaman_duel, spell_zoo, world_fry | crates/gameplay/src/inventory/list.rs:715 |
| `zs_stunned_loop` | 30 | routine_zoo | crates/script/src/magic/area.rs:423 |
| `want_to_ransack` | 29 | combat_basic, shaman_duel, spell_zoo | crates/script/src/predicates.rs:423 |
| `start_say_attack_end` | 29 | combat_basic, shaman_duel, spell_zoo | crates/script/src/speech.rs:51 |
| `basket_total` | 28 | trade_click, trade_esc | crates/gameplay/src/trade.rs:45 |
| `is_story_caster` | 25 | shaman_duel, spell_zoo | crates/script/src/combat/weapon.rs:34 |
| `choose_spell` | 25 | shaman_duel, spell_zoo | crates/script/src/combat/weapon.rs:115 |
| `capture` | 24 | craft_anvil, doors, lockpick, loot_chest, menu_save_load, npc_archer,  | crates/runtime/src/save.rs:281 |
| `on_react_to_damage_go_back` | 24 | routine_zoo | crates/script/src/combat/react.rs:126 |
| `ps_ranged_shoot` | 24 | bow_faithful, bow_pose, shaman_duel | crates/script/src/player/ranged.rs:340 |
| `snapshot` | 24 | craft_anvil, doors, lockpick, loot_chest, menu_save_load, npc_archer,  | crates/sim/src/persist.rs:178 |
| `collect` | 24 | craft_anvil, doors, lockpick, loot_chest, menu_save_load, npc_archer,  | crates/world/src/sector.rs:257 |
| `pool_max` | 23 | routine_zoo, spell_zoo | crates/script/src/stats.rs:187 |
| `ps_magic_potion` | 22 | spell_zoo | crates/script/src/player/magic.rs:381 |
| `teach_offers` | 20 | dialog_quest, teach, trade_click, trade_esc | crates/ui/src/journal.rs:129 |
| `spell_missile` | 19 | bow_pose, magic_faithful, shaman_duel, spell_zoo | crates/projectile/src/launch.rs:71 |
| `magic_mist_tick` | 19 | spell_zoo | crates/script/src/magic/area.rs:308 |
| `magic_projectile` | 19 | bow_pose, magic_faithful, shaman_duel, spell_zoo | crates/script/src/magic/cast.rs:308 |
| `is_animal` | 19 | shaman_duel, spell_zoo | crates/script/src/magic/effects.rs:56 |
| `ps_interact_end` | 19 | craft_anvil, lockpick, loot_chest, sit_pose, sleep_bed, spell_zoo, wor | crates/script/src/player/interact.rs:583 |
| `use_stack` | 19 | bow_faithful, bow_pose, shaman_duel, spell_zoo, staff_parade | crates/script/src/player/use_item.rs:56 |
| `is_big` | 18 | spell_zoo | crates/script/src/speech.rs:42 |
| `is_large` | 18 | spell_zoo | crates/script/src/magic/effects.rs:68 |
| `transform_duration` | 18 | spell_zoo | crates/script/src/magic/effects.rs:174 |
| `magic_transform` | 18 | spell_zoo | crates/script/src/magic/effects.rs:736 |
| `transform_back` | 18 | spell_zoo | crates/script/src/magic/effects.rs:794 |
| `summon_party_monster` | 17 | spell_zoo | crates/script/src/magic/effects.rs:566 |
| `ps_ranged_aim_reload` | 16 | bow_faithful, bow_pose, shaman_duel | crates/script/src/player/ranged.rs:434 |
| `drop_slot` | 15 | combat_basic, shaman_duel, spell_zoo | crates/gameplay/src/inventory/list.rs:1184 |
| `drop_stack` | 14 | combat_basic, shaman_duel, spell_zoo | crates/gameplay/src/inventory/list.rs:1158 |
| `confirm` | 14 | craft_anvil, dialog_quest, doors, lockpick, loot_chest, sit_pose, slee | crates/script/src/player/normal.rs:220 |
| `is_deadly_damage` | 13 | combat_basic, shaman_duel, spell_zoo | crates/script/src/damage.rs:393 |
| `deal_enabled` | 12 | trade_click, trade_esc | crates/gameplay/src/trade.rs:198 |
| `send_perception_warn` | 12 | combat_basic, melee_faithful, routine_zoo | crates/script/src/assess.rs:77 |
| `ps_interact` | 12 | craft_anvil, lockpick, loot_chest, sit_pose, sleep_bed, world_boulder, | crates/script/src/player/interact.rs:433 |
| `send_perception_murder` | 11 | shaman_duel, spell_zoo | crates/script/src/assess.rs:295 |
| `can_freeze` | 11 | shaman_duel, spell_zoo | crates/script/src/damage.rs:440 |
| `ambient_action` | 11 | routine_zoo, walk_to_reddock, world_routines | crates/script/src/routine/select.rs:432 |
| `generate_trade` | 10 | trade_click, trade_esc | crates/gameplay/src/treasure.rs:253 |
| `refresh_trade` | 10 | trade_click, trade_esc | crates/gameplay/src/treasure.rs:300 |
| `magic_summon` | 10 | spell_zoo | crates/script/src/magic/cast.rs:353 |
| `spell_duration_bonus` | 10 | spell_zoo | crates/script/src/magic/effects.rs:150 |
| `ps_melee` | 10 | bow_pose, combat_basic, controls_basic, focus_steer, melee_faithful | crates/script/src/player/melee.rs:81 |
| `ps_ranged_aim` | 10 | bow_faithful, bow_pose, shaman_duel | crates/script/src/player/ranged.rs:246 |
| `escape` | 10 | craft_anvil, lockpick, loot_chest, menu_save_load, trade_click, trade_ | crates/ui/src/manager.rs:246 |
| `clear_baskets` | 8 | trade_click, trade_esc | crates/gameplay/src/trade.rs:164 |
| `compare_type` | 8 | ui_screens | crates/gameplay/src/inventory/order.rs:77 |
| `hero_arrow` | 8 | bow_faithful, bow_pose, shaman_duel | crates/projectile/src/launch.rs:30 |
| `is_bothered_by_player_theft_of` | 8 | crime_theft, doors, lockpick, world_chest | crates/script/src/perception.rs:166 |
| `evade_entry` | 8 | routine_zoo | crates/script/src/player/evade.rs:92 |
| `has_guard_news` | 8 | dialog_quest, teach, trade_click, trade_esc | crates/script/src/routine/info.rs:27 |
| `has_boss_news` | 8 | dialog_quest, teach, trade_click, trade_esc | crates/script/src/routine/info.rs:37 |
| `has_heal_info` | 8 | dialog_quest, teach, trade_click, trade_esc | crates/script/src/routine/info.rs:49 |
| `smalltalk_dispatch` | 8 | quick_slots, routine_zoo, sleep_bed, world_routines | crates/script/src/routine/work.rs:1575 |
| `deactivate_skill` | 7 | bow_pose, combat_basic, controls_basic, craft_anvil, melee_faithful, s | crates/gameplay/src/inventory/list.rs:1515 |
| `ps_interact_chest` | 7 | lockpick, loot_chest, world_chest | crates/script/src/player/interact.rs:613 |
| `ps_ranged_walk` | 7 | bow_faithful, bow_pose, shaman_duel | crates/script/src/player/ranged.rs:185 |
| `listen_loop` | 7 | routine_zoo | crates/script/src/routine/work.rs:1488 |
| `compare_template` | 6 | ui_screens | crates/gameplay/src/inventory/order.rs:105 |
| `zs_magic_sleep` | 6 | routine_zoo | crates/script/src/magic/area.rs:507 |
| `evade_loop` | 6 | routine_zoo | crates/script/src/player/evade.rs:127 |
| `can_open` | 6 | lockpick, loot_chest, world_chest | crates/script/src/player/interact.rs:31 |
| `ps_dusk_to_dawn` | 6 | sleep_bed, spell_zoo | crates/script/src/player/interact.rs:773 |
| `ps_ranged` | 6 | bow_faithful, bow_pose, shaman_duel | crates/script/src/player/ranged.rs:142 |
| `reset` | 6 | dialog_quest, teach, trade_click, trade_esc | crates/ui/src/dialogmenu.rs:42 |
| `subtitle_align` | 6 | dialog_quest, teach, trade_click, trade_esc | crates/ui/src/dialogmenu.rs:103 |
| `start` | 6 | dialog_quest, teach, trade_click, trade_esc | crates/ui/src/dialogmenu.rs:133 |
| `set_last_fight_timestamp` | 5 | routine_zoo, sector_switch, shaman_duel | crates/script/src/predicates.rs:366 |
| `cancel_attack` | 5 | routine_zoo, sector_switch, shaman_duel | crates/script/src/combat/attack.rs:178 |
| `ps_boulder` | 5 | world_boulder | crates/script/src/player/boulder.rs:110 |
| `ps_boulder_loop` | 5 | world_boulder | crates/script/src/player/boulder.rs:145 |
| `ps_ghost_loop` | 5 | spell_zoo | crates/script/src/player/magic.rs:657 |
| `holster_from_melee` | 5 | bow_faithful, bow_pose, controls_basic, magic_faithful, melee_faithful | crates/script/src/player/select_weapon.rs:20 |
| `draw_from_normal` | 5 | bow_pose, combat_basic, controls_basic, focus_steer, melee_faithful | crates/script/src/player/select_weapon.rs:39 |
| `select_weapon` | 5 | bow_pose, combat_basic, controls_basic, focus_steer, melee_faithful | crates/script/src/player/select_weapon.rs:52 |
| `on_sleep` | 4 | routine_zoo | crates/script/src/perception.rs:357 |
| `has_crime_refuse` | 4 | dialog_quest, teach, trade_click, trade_esc | crates/script/src/speech.rs:358 |
| `assess_talk` | 4 | dialog_quest, teach, trade_click, trade_esc | crates/script/src/speech.rs:378 |
| `is_game` | 4 | shaman_duel | crates/script/src/magic/effects.rs:62 |
| `ps_ghost` | 4 | spell_zoo | crates/script/src/player/magic.rs:625 |
| `ps_melee_parade` | 4 | combat_basic, melee_faithful | crates/script/src/player/melee.rs:291 |
| `ps_normal_take` | 4 | crime_theft, doors | crates/script/src/player/normal.rs:113 |
| `loot_key_index` | 3 | craft_anvil, loot_chest, world_fry | crates/gameplay/src/trade.rs:280 |
| `compare_damage` | 3 | ui_screens | crates/gameplay/src/inventory/order.rs:89 |
| `compare_value` | 3 | ui_screens | crates/gameplay/src/inventory/order.rs:115 |
| `compare_quality` | 3 | ui_screens | crates/gameplay/src/inventory/order.rs:121 |
| `compare_quick_slot` | 3 | ui_screens | crates/gameplay/src/inventory/order.rs:137 |
| `compare_slot` | 3 | ui_screens | crates/gameplay/src/inventory/order.rs:149 |
| `compare_focus_name` | 3 | ui_screens | crates/gameplay/src/inventory/order.rs:163 |
| `compare_weapon` | 3 | ui_screens | crates/gameplay/src/inventory/order.rs:180 |
| `compare_default` | 3 | ui_screens | crates/gameplay/src/inventory/order.rs:204 |
| `restore` | 3 | menu_save_load, quick_slots | crates/runtime/src/save.rs:310 |
| `send_perception_use_interact_object` | 3 | lockpick, loot_chest, world_chest | crates/script/src/assess.rs:358 |
| `add_enclave_theft_count` | 3 | crime_theft, doors, loot_chest | crates/script/src/crime.rs:126 |
| `ambient_action_secs` | 3 | routine_zoo, walk_to_reddock | crates/script/src/helpers.rs:125 |
| `has_attack_reason_to_kill` | 3 | combat_basic | crates/script/src/predicates.rs:242 |
| `can_handle` | 3 | world_boulder | crates/script/src/player/boulder.rs:75 |
| `unlock` | 3 | lockpick, loot_chest, world_chest | crates/script/src/player/interact.rs:79 |
| `player_sleep` | 3 | sleep_bed, spell_zoo | crates/script/src/player/interact.rs:714 |
| `ps_magic_flame_sword` | 3 | spell_zoo | crates/script/src/player/magic.rs:258 |
| `ps_ranged_reload` | 3 | bow_faithful | crates/script/src/player/ranged.rs:389 |
| `carry_stones` | 3 | routine_zoo | crates/script/src/routine/work.rs:1441 |
| `run_to_attack_point` | 3 | routine_zoo | crates/script/src/routine/work.rs:1627 |
| `apply_snapshot` | 3 | menu_save_load, quick_slots | crates/sim/src/persist.rs:224 |
| `open_menu` | 3 | menu_save_load | crates/ui/src/manager.rs:222 |
| `set_door_status` | 2 | doors, world_doors | crates/gameplay/src/mover.rs:142 |
| `door_status` | 2 | doors, world_doors | crates/gameplay/src/mover.rs:158 |
| `step_basket` | 2 | trade_click, trade_esc | crates/gameplay/src/trade.rs:87 |
| `cancel` | 2 | trade_click, trade_esc | crates/gameplay/src/trade.rs:273 |
| `generate_trade_inventory` | 2 | trade_click, trade_esc | crates/gameplay/src/treasure.rs:355 |
| `refresh_trade_inventory` | 2 | trade_click, trade_esc | crates/gameplay/src/treasure.rs:376 |
| `transfer_items_to` | 2 | trade_click | crates/gameplay/src/inventory/list.rs:1224 |
| `transfer_transaction_items_to` | 2 | trade_click | crates/gameplay/src/inventory/list.rs:1334 |
| `compare_name` | 2 | ui_screens | crates/gameplay/src/inventory/order.rs:174 |
| `send_perception_defeat` | 2 | combat_basic | crates/script/src/assess.rs:328 |
| `unconscious_secs` | 2 | combat_basic | crates/script/src/down.rs:35 |
| `defeat` | 2 | combat_basic | crates/script/src/down.rs:176 |
| `send_perception_take_item` | 2 | crime_theft, doors | crates/script/src/perception.rs:252 |
| `on_eat_body_perception` | 2 | routine_zoo | crates/script/src/combat/react.rs:862 |
| `zs_stunned` | 2 | routine_zoo | crates/script/src/magic/area.rs:399 |
| `zs_stunned_end` | 2 | routine_zoo | crates/script/src/magic/area.rs:434 |
| `zs_freeze_loop` | 2 | routine_zoo | crates/script/src/magic/area.rs:477 |
| `zs_misted` | 2 | routine_zoo | crates/script/src/magic/area.rs:562 |
| `zs_misted_loop` | 2 | routine_zoo | crates/script/src/magic/area.rs:580 |
| `magic_teleport` | 2 | spell_zoo, teleport | crates/script/src/magic/effects.rs:684 |
| `ps_interact_sleep_bed` | 2 | sleep_bed | crates/script/src/player/interact.rs:743 |
| `ps_magic_staff_parade` | 2 | staff_parade | crates/script/src/player/magic.rs:358 |
| `have_a_break` | 2 | routine_zoo | crates/script/src/routine/select.rs:418 |
| `trade_open` | 2 | trade_click, trade_esc | crates/sim/src/trade.rs:77 |
| `time_of_day` | 2 | dialog_quest, ui_screens | crates/ui/src/journal.rs:69 |
| `on_touch` | 2 | triggers | crates/world/src/trigger.rs:335 |
| `can_be_activated` | 2 | triggers | crates/world/src/trigger.rs:570 |
| `activate` | 2 | triggers | crates/world/src/trigger.rs:643 |
| `balance_gold` | 1 | trade_click | crates/gameplay/src/trade.rs:224 |
| `commit` | 1 | trade_click | crates/gameplay/src/trade.rs:258 |
| `left_equip_slot` | 1 | bow_faithful | crates/gameplay/src/inventory/list.rs:1083 |
| `memorize_enclave_crime` | 1 | crime_theft | crates/script/src/crime.rs:89 |
| `pipi_stumble` | 1 | shaman_duel | crates/script/src/damage.rs:1027 |
| `add_hit_points` | 1 | spell_zoo | crates/script/src/stats.rs:304 |
| `on_attack_positioning` | 1 | routine_zoo | crates/script/src/combat/attack.rs:584 |
| `start_parade` | 1 | staff_parade | crates/script/src/combat/commands.rs:138 |
| `start_lie_knock_down` | 1 | combat_basic | crates/script/src/combat/commands.rs:201 |
| `begin_jump_back_command` | 1 | routine_zoo | crates/script/src/combat/commands.rs:594 |
| `arena_enter` | 1 | routine_zoo | crates/script/src/combat/misc.rs:27 |
| `success_enter` | 1 | routine_zoo | crates/script/src/combat/misc.rs:184 |
| `choose_defensive` | 1 | sector_switch | crates/script/src/combat/moves.rs:827 |
| `lee_stand` | 1 | routine_zoo | crates/script/src/combat/react.rs:998 |
| `zs_freeze` | 1 | routine_zoo | crates/script/src/magic/area.rs:460 |
| `zs_decay` | 1 | routine_zoo | crates/script/src/magic/area.rs:592 |
| `can_tame_animal` | 1 | spell_zoo | crates/script/src/magic/effects.rs:86 |
| `can_control` | 1 | spell_zoo | crates/script/src/magic/effects.rs:115 |
| `magic_poison` | 1 | spell_zoo | crates/script/src/magic/effects.rs:223 |
| `magic_cure` | 1 | spell_zoo | crates/script/src/magic/effects.rs:254 |
| `magic_cure_poison` | 1 | spell_zoo | crates/script/src/magic/effects.rs:261 |
| `magic_cure_disease` | 1 | spell_zoo | crates/script/src/magic/effects.rs:269 |
| `magic_heal_other` | 1 | spell_zoo | crates/script/src/magic/effects.rs:277 |
| `magic_tame_animal` | 1 | spell_zoo | crates/script/src/magic/effects.rs:398 |
| `magic_word_of_control` | 1 | spell_zoo | crates/script/src/magic/effects.rs:414 |
| `magic_charm` | 1 | spell_zoo | crates/script/src/magic/effects.rs:424 |
| `magic_bloodlust` | 1 | spell_zoo | crates/script/src/magic/effects.rs:442 |
| `magic_fear` | 1 | spell_zoo | crates/script/src/magic/effects.rs:470 |
| `magic_sleep` | 1 | spell_zoo | crates/script/src/magic/effects.rs:483 |
| `magic_hypnosis` | 1 | spell_zoo | crates/script/src/magic/effects.rs:492 |
| `magic_call_animals` | 1 | spell_zoo | crates/script/src/magic/effects.rs:532 |
| `magic_summon_demon` | 1 | spell_zoo | crates/script/src/magic/effects.rs:597 |
| `magic_summon_skeleton` | 1 | spell_zoo | crates/script/src/magic/effects.rs:603 |
| `magic_summon_golem` | 1 | spell_zoo | crates/script/src/magic/effects.rs:609 |
| `magic_summon_goblin` | 1 | spell_zoo | crates/script/src/magic/effects.rs:615 |
| `magic_summon_army_of_darkness` | 1 | spell_zoo | crates/script/src/magic/effects.rs:632 |
| `magic_summon_army_of_cuteness` | 1 | spell_zoo | crates/script/src/magic/effects.rs:646 |
| `magic_summon_monster` | 1 | spell_zoo | crates/script/src/magic/effects.rs:656 |
| `magic_animal_form` | 1 | spell_zoo | crates/script/src/magic/effects.rs:764 |
| `magic_open_locks` | 1 | spell_zoo | crates/script/src/magic/effects.rs:813 |
| `magic_soul_travel` | 1 | spell_zoo | crates/script/src/magic/effects.rs:828 |
| `magic_telekinesis` | 1 | spell_zoo | crates/script/src/magic/effects.rs:857 |
| `magic_time_bubble` | 1 | spell_zoo | crates/script/src/magic/effects.rs:864 |
| `synthesize` | 1 | craft_anvil | crates/script/src/player/craft.rs:32 |
| `fry_stack` | 1 | world_fry | crates/script/src/player/craft.rs:84 |
| `transform_stack` | 1 | world_fry | crates/script/src/player/craft.rs:104 |
| `ps_magic_summon_abort` | 1 | magic_faithful | crates/script/src/player/magic.rs:247 |
| `close_menu` | 1 | menu_save_load | crates/ui/src/manager.rs:228 |
| `on_untouch` | 1 | triggers | crates/world/src/trigger.rs:350 |
| `deactivate` | 1 | triggers | crates/world/src/trigger.rs:724 |
