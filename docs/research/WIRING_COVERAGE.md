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

* 782 instrumented ported functions: **262 called, 520 never called** (66%); of the 520, **144 DEAD**, **284 INTERNAL-ONLY**, **92 WIRED-NOT-EXERCISED**.
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
| P1-3 | **(fixed night P2, NPC_COMBAT_AI.md)** NPC combat animation is late and uniform: swing clip starts at the damage moment, every hit plays QuickStumble regardless of `HitResult`, no parry pose, no wind-up for NPC archers/casters | `events.rs:344-363` (C6, C7, D5, G4): `routines.rs` shows only 4 of ~20 `AiCommand`s | Emit a "swing started" event from `begin_swing`; choose stumble/knockdown clips from `Hit.result` via `combat::commands::start_stumble/start_lie_knock_down/start_get_up`; play `AiCommand::{Attack,Parade,Shoot,Cast,Stumble,Turn*}` |
| P1-4 | **(speeds and LOS fixed night P2, NPC_COMBAT_AI.md section 3)** NPCs run too fast (flat 550 cm/s, play sprint clips) and see through walls | `sim.rs:40-41 RUN_SPEED/WALK_SPEED`, `MovementProfile::human_npc()` unused (G1); `PlayWorld::line_of_sight` always true (G6) | Use `physics::profile` (run 400, walk 150, sneak 280) and species/template values; hand the collision world to `PlayWorld` for LOS and `is_outdoors` |
| P1-5 | **Spells other than the fireball do nothing visible**: summons show a toast, status effects (freeze, burn, sleep), animal forms, area spells, magic parade (staff), flame sword | `script/magic/effects.rs` 47 of 48 and `magic/area.rs` 14 of 14 never called; `MagicEvent::{Summoned,Status,Transform,Area}` fall through `_ => {}` in `fx.rs:294`; `PS_Magic_StaffParade`, `PS_MagicPotion`, `PS_Magic_FlameSword` unreachable (E5, SC magic) | Present `MagicEvent`s (spawn summon bodies, status visuals); map `msg::PARADE` with a staff to `PS_Magic_StaffParade` |
| P1-6 | **(fixed night P2: `_AI_SelectWeapon` port gives mages their spell, NPC_COMBAT_AI.md)** Shaman never casts (QA `shaman_duel` FAIL: casts 0, mana unspent) | not investigated here; NPC cast path (`ZS_Attack_Cast`, `AiCommand::Cast`) | Debug the QA failure first, it is an existing regression |
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

Generated by `scripts/portcov/` (crate `gothic-portcov`, cargo feature `port-coverage` of `gothic-reforged`). `scripts/portcov/instrument.py` inserts `gothic_portcov::hit!("<fn>")` at the top of every function whose doc comment says `Port of <Name> (<Dll>:<addr>)`; the macro expands to nothing without the feature. Each QA scenario was run as `gothic-reforged --play --qa <scenario>` with the feature on, and the call counts of all runs were merged (`scripts/portcov/run_all.sh`, `scripts/portcov/report.py`). **Called** means executed at least once by the running game in any scenario (unit tests do not count; a function reached only from another never-used function counts as never called only if it is itself never executed). Scenarios run: `bow_pose`, `combat_basic`, `controls_basic`, `craft_anvil`, `dialog_quest`, `doors`, `hud_focus`, `lockpick`, `loot_chest`, `magic_bow`, `menu_save_load`, `perf_idle`, `quick_slots`, `routine_reentry`, `sector_switch`, `shaman_duel`, `sit_pose`, `souls_basic`, `souls_er`, `startup_sanity`, `swim`, `teach`, `teleport`, `trade_click`, `trade_esc`, `triggers`, `ui_screens`, `walk_to_reddock`, `world_walk`.

## Totals

* Doc blocks with `Port of`: 924; instrumentable functions: 782 (+0 not instrumented, see the end); other items (tables, consts, structs, impls; not countable): 142.
* **Called in play: 262**, **never called: 520** (66% of instrumented functions).
* Of the never-called: **144 DEAD** (no production reference anywhere: nobody could call them), **284 internal-only** (referenced only inside their own crate, entry point never reached), **92 wired-not-exercised** (referenced from another crate, but no QA scenario reaches the call; long-running routines, rare events). A trailing "?" marks names shared by several functions (reference count is a name match, so the status is a heuristic).
* Script states (ZS_/PS_/BS_, data tables interpreted by `run_state`): 65 of 375 entered at least once.

## By subsystem

| Subsystem / original state family | called | never called |
| --- | ---: | ---: |
| gameplay (inventory, trade, mover, treasure, focus, stats) | 39 | 123 |
| spell effects / casting | 7 | 62 |
| script helpers / predicates / stats | 28 | 51 |
| world (entity admin, PVS, sectors, triggers, zones) | 18 | 46 |
| ZS_Attack / combat AI | 25 | 42 |
| animation (EMotionFX wrapper, blending, attachments) | 10 | 38 |
| damage pipeline | 5 | 38 |
| ZS_* routine tasks | 7 | 35 |
| PS_Magic* | 4 | 13 |
| ui | 14 | 11 |
| perception / crime | 12 | 10 |
| down / knock-out | 0 | 9 |
| PS_Interact* | 7 | 7 |
| audio sequencers | 20 | 6 |
| render rules | 1 | 6 |
| PS_Interact boulder | 0 | 4 |
| input / player control | 5 | 3 |
| PS_Ranged* (bow, crossbow) | 7 | 3 |
| fx / particles | 17 | 2 |
| PS_Interact craft | 1 | 2 |
| PS_Evade | 0 | 2 |
| PS_Melee* | 3 | 2 |
| PS_Normal / Sneak / Swim | 5 | 2 |
| nav | 0 | 1 |
| projectiles | 8 | 1 |
| sim (stats, combat, AI, host) | 4 | 1 |
| camera (PSCamera) | 1 | 0 |
| environment / weather | 6 | 0 |
| formats | 3 | 0 |
| physics / movement | 1 | 0 |
| runtime | 3 | 0 |
| PS_HUD use item | 1 | 0 |

## Script states never entered in play

**PS_** (77 of 101): `PS_DuskToDawn`, `PS_DuskToDawn_Loop`, `PS_Ghost`, `PS_GhostSummoner`, `PS_GhostSummoner_Loop`, `PS_Ghost_Jump`, `PS_Ghost_Loop`, `PS_GonnaLoot`, `PS_HUD`, `PS_HUD_Equip`, `PS_HUD_Loop`, `PS_HUD_UnEquip`, `PS_HUD_Use`, `PS_Interact_Alchemy`, `PS_Interact_Barbecue`, `PS_Interact_Bookstand`, `PS_Interact_Campfire`, `PS_Interact_CarryStone`, `PS_Interact_CarryStone_End`, `PS_Interact_CarryStone_Loop`, `PS_Interact_Cauldron`, `PS_Interact_Door`, `PS_Interact_DrinkWaterBarrel`, `PS_Interact_Forge`, `PS_Interact_GrindStone`, `PS_Interact_HoldOrcBoulder`, `PS_Interact_HoldOrcBoulder_Loop`, `PS_Interact_LiftOrcBoulder`, `PS_Interact_LiftOrcBoulder_Loop`, `PS_Interact_OrcDrum`, `PS_Interact_PickOre`, `PS_Interact_PirateTreasure`, `PS_Interact_SawLog`, `PS_Interact_Shrine`, `PS_Interact_SitStool`, `PS_Interact_SitThrone`, `PS_Interact_SleepBed`, `PS_Interact_SleepGround`, `PS_Interact_Stoneplate`, `PS_Interact_Talk`, `PS_Interact_TiltOrcBoulder`, `PS_Interact_TiltOrcBoulder_Loop`, `PS_Interact_UseWaterpipe`, `PS_Interact_WaitForExit`, `PS_MagicPotion`, `PS_Magic_Cast`, `PS_Magic_FlameSword`, `PS_Magic_Jump`, `PS_Magic_QuickCast`, `PS_Magic_Sneak`, `PS_Magic_StaffParade`, `PS_Magic_StaffParade_Loop`, `PS_Magic_Summon_Abort`, `PS_Melee_Attack`, `PS_Melee_FinishingAttack`, `PS_Melee_HackAttack`, `PS_Melee_Jump`, `PS_Melee_JumpBack`, `PS_Melee_Parade`, `PS_Melee_PierceAttack`, `PS_Melee_PowerAttack`, `PS_Melee_SimpleWhirl`, `PS_Melee_Sneak`, `PS_Melee_WhirlAttack`, `PS_Normal_Take`, `PS_QuickUse`, `PS_Ranged_Aim_Loop`, `PS_Ranged_Aim_Reload`, `PS_Ranged_CrossBowSneak_Loop`, `PS_Ranged_Jump`, `PS_Ranged_Reload`, `PS_Ranged_Sneak`, `PS_Ranged_Walk_Loop`, `PS_Slide_Jump`, `PS_StopForMenu`, `PS_Swim`, `PS_Swim_Loop`

**ZS_** (225 of 266): `ZS_Acrobat`, `ZS_Alchemy`, `ZS_Alchemy_Alchemy`, `ZS_Alchemy_Bookstand`, `ZS_Alchemy_Cauldron`, `ZS_Ambient_Loop`, `ZS_Ambient_Sleep`, `ZS_Ambient_Sleep_Loop`, `ZS_AnimalSleep`, `ZS_AnimalSleep_SleepGround`, `ZS_Applaud`, `ZS_Arena`, `ZS_Arena_Loop`, `ZS_Attack_Arena_End`, `ZS_Attack_Cast`, `ZS_Attack_ChangePosition`, `ZS_Attack_DrinkPotion`, `ZS_Attack_FinishingAttack`, `ZS_Attack_JumpBack`, `ZS_Attack_Parade`, `ZS_Attack_Shoot`, `ZS_Attack_SimpleWhirl`, `ZS_Attack_StrafeLeft`, `ZS_Attack_StrafeRight`, `ZS_Attack_WaitForDialog`, `ZS_Attack_WaitOnShore`, `ZS_Barbecue`, `ZS_Bard`, `ZS_Boss_Stand`, `ZS_Boss_Throne`, `ZS_Break`, `ZS_Break_Bench`, `ZS_Break_Chest`, `ZS_Break_Pee`, `ZS_Break_SitGround`, `ZS_Break_Stand`, `ZS_Break_Stool`, `ZS_Break_WaterBarrel`, `ZS_CarryStones`, `ZS_Cook`, `ZS_Dead`, `ZS_Dead_Loop`, `ZS_Deadlock`, `ZS_Decay`, `ZS_Digger`, `ZS_Digger_DigGround`, `ZS_Digger_PickGround`, `ZS_Digger_PickOre`, `ZS_Digger_WaterBarrel`, `ZS_DiveIn`, `ZS_EatBody`, `ZS_EatBody_Loop`, `ZS_EvadeBackward`, `ZS_EvadeForward`, `ZS_EvadeLeft`, `ZS_EvadeLoop`, `ZS_EvadeRight`, `ZS_Fall`, `ZS_Farmer`, `ZS_Farmer_FieldWork`, `ZS_Farmer_RakeField`, `ZS_Firerain`, `ZS_Firerain_Loop`, `ZS_Flee`, `ZS_Flee_GoBack`, `ZS_Flee_GotoFleePoint`, `ZS_Flee_ObserveFollower`, `ZS_FollowNPC`, `ZS_FollowNPC_Loop`, `ZS_FollowPlayer`, `ZS_FollowPlayer_Loop`, `ZS_Freeze`, `ZS_Freeze_Loop`, `ZS_FrostWave_Loop`, `ZS_Frostwave`, `ZS_Goblin`, `ZS_Goblin_Banana`, `ZS_Goblin_FryMeat`, `ZS_Goblin_SitGround`, `ZS_Goblin_Smalltalk`, `ZS_GotoPlayerZone`, `ZS_Grind`, `ZS_Guard`, `ZS_Guard_Loop`, `ZS_GuidePlayer`, `ZS_GuidePlayer_Loop`, `ZS_GuidePlayer_Wait`, `ZS_Hailstorm`, `ZS_Hailstorm_FadeIn`, `ZS_Hailstorm_FadeIn_Loop`, `ZS_Hailstorm_FadeOut_Loop`, `ZS_Hailstorm_Loop`, `ZS_HangAround_Bench`, `ZS_HangAround_Pee`, `ZS_HangAround_Stool`, `ZS_HangAround_Waterpipe`, `ZS_HealSelf`, `ZS_HealSelf_Cast`, `ZS_HeatWave_Loop`, `ZS_Heatwave`, `ZS_HeavyLand`, `ZS_HeavyParadeStumble`, `ZS_Herd`, `ZS_Herd_DrinkWater`, `ZS_Herd_EatGround`, `ZS_IceBomb`, `ZS_IceBomb_Loop`, `ZS_Idiot`, `ZS_Land`, `ZS_Lee_Teleport`, `ZS_LieKnockDown`, `ZS_Light`, `ZS_Light_Loop`, `ZS_Lightning`, `ZS_Lightning_Loop`, `ZS_Listen`, `ZS_Listen_Loop`, `ZS_Lumberjack`, `ZS_MagicHypnosis`, `ZS_MagicHypnosis_Loop`, `ZS_MagicSleep`, `ZS_MagicSleep_Loop`, `ZS_Meteor`, `ZS_Meteor_Loop`, `ZS_Mist`, `ZS_Mist_End`, `ZS_Mist_Loop`, `ZS_Mist_NoAggro`, `ZS_Mist_NoAggro_Loop`, `ZS_Misted`, `ZS_Misted_Loop`, `ZS_Monster`, `ZS_Monster_Roam`, `ZS_NavEval`, `ZS_NavEval_Loop`, `ZS_ObserveWeapon`, `ZS_ObserveZonePlayer`, `ZS_OrcParty`, `ZS_OrcParty_OrcBoulder`, `ZS_OrcParty_OrcDance`, `ZS_OrcParty_OrcDrum`, `ZS_OrcParty_Spectator`, `ZS_ParadeStumble`, `ZS_PierceStumble`, `ZS_PiercedDead`, `ZS_PiercedKO`, `ZS_Pray`, `ZS_Pray_Shrine`, `ZS_Preach`, `ZS_Preach_AutoPreach`, `ZS_Preach_SVM`, `ZS_QuickStumble`, `ZS_RagDollDead`, `ZS_RagDollDead_Loop`, `ZS_ReactToDamage`, `ZS_ReactToDamage_End`, `ZS_ReactToDamage_GoBack`, `ZS_ReactToDamage_Loop`, `ZS_ReactToFootsteps`, `ZS_ReactToSneak`, `ZS_ReactToWeapon_WaitForAttack`, `ZS_ReactToZone`, `ZS_ReactToZone_Loop`, `ZS_RevolutionAttack`, `ZS_RevolutionAttack_Loop`, `ZS_RevolutionSuccess`, `ZS_RunToAttackPoint`, `ZS_SitKnockDown`, `ZS_SitKnockDown_GetUpAttack`, `ZS_SitKnockDown_GetUpParade`, `ZS_SitKnockDown_Loop`, `ZS_SlaveBabe`, `ZS_SlaveBabe_CleanFloor`, `ZS_SlaveBabe_Dance`, `ZS_SlaveBabe_FanBoss`, `ZS_SlaveBabe_SweepFloor`, `ZS_Sleep`, `ZS_Sleep_Bed`, `ZS_Sleep_Ground`, `ZS_Sleep_Loop`, `ZS_Slide`, `ZS_Slide_Loop`, `ZS_Smith`, `ZS_Smith_Anvil`, `ZS_Smith_Cool`, `ZS_Smith_Forge`, `ZS_Smith_GrindStone`, `ZS_StandUp`, `ZS_Study`, `ZS_Study_Bookshelf`, `ZS_Study_Bookstand`, `ZS_Stumble`, `ZS_Stunned`, `ZS_Stunned_End`, `ZS_Stunned_Loop`, `ZS_Timebubble`, `ZS_Timebubble_Loop`, `ZS_Trader`, `ZS_Trader_Bookshelf`, `ZS_Trader_Bookstand`, `ZS_Trader_Chest`, `ZS_Trader_Stand`, `ZS_Trader_SweepFloor`, `ZS_TravelToDestination`, `ZS_TravelToDestination_Loop`, `ZS_Unconscious`, `ZS_Unconscious_End`, `ZS_Unconscious_Loop`, `ZS_WaitForDawn`, `ZS_WaitForFreeDestination`, `ZS_WaitForFreeDestination_Loop`, `ZS_WaitForMeteor`, `ZS_WaitForPlayer`, `ZS_WaitForPlayer_Loop`, `ZS_WatchFight`, `ZS_WatchFight_GoBack`, `ZS_WatchFight_Goto`, `ZS_WatchFight_Loop`, `ZS_WatchFight_Spectator`, `ZS_WatchFight_Spectator_Loop`, `ZS_WatchFight_WaitForFlee`, `ZS_WatchFight_WaitForFlee_Loop`, `ZS_Woodwork`, `ZS_Woodwork_Repair`, `ZS_Woodwork_SawLog`

**BS_** (8 of 8): `BS_Alchemy`, `BS_AncientWisdom`, `BS_BookOfFlood`, `BS_BookOfRhobar`, `BS_BookOfZuben`, `BS_Chronicles`, `BS_NothingNew`, `BS_Smith`

Entered: `PS_Interact` (9), `PS_Interact_Anvil` (4), `PS_Interact_Chest` (5), `PS_Interact_Chest_Loop` (48), `PS_Interact_End` (12), `PS_Interact_Loop` (40), `PS_Interact_SitBench` (4), `PS_Magic` (9), `PS_Magic_Loop` (172), `PS_Magic_PowerCast` (4), `PS_Magic_Reload` (6), `PS_Magic_Summon` (2), `PS_Melee` (5), `PS_Melee_Loop` (241), `PS_Melee_QuickAttack` (14), `PS_Normal` (11), `PS_Normal_Jump` (10), `PS_Normal_Loop` (1067), `PS_Normal_Sneak` (2), `PS_Ranged` (5), `PS_Ranged_Aim` (14), `PS_Ranged_PowerAim` (928), `PS_Ranged_Shoot` (12), `PS_Ranged_Walk` (150), `ZS_Ambient` (32), `ZS_Attack` (7180), `ZS_Attack_Attack` (82), `ZS_Attack_End` (87), `ZS_Attack_GoBack` (42), `ZS_Attack_Goto` (93), `ZS_Attack_HackAttack` (36), `ZS_Attack_Loop` (7186), `ZS_Attack_PierceAttack` (19), `ZS_Attack_PowerAttack` (32), `ZS_Attack_QuickAttack` (70), `ZS_Attack_Sprint` (1114), `ZS_Attack_TurnLeft` (71), `ZS_Attack_TurnRight` (65), `ZS_Attack_Wait` (775), `ZS_Attack_WaitForFlee` (7), `ZS_Attack_WaitForFlee_Loop` (2366), `ZS_Attack_WhirlAttack` (21), `ZS_Boss` (6), `ZS_Boss_Pace` (6), `ZS_FetchWeapon` (20), `ZS_HangAround` (831), `ZS_HangAround_DoNothing` (17), `ZS_HangAround_FryMeat` (8), `ZS_HangAround_SitGround` (1490), `ZS_HangAround_Stand` (39), `ZS_ObserveSuspect` (16), `ZS_ObserveSuspect_Loop` (520), `ZS_RansackBody` (31), `ZS_ReactToWeapon` (54), `ZS_ReactToWeapon_End` (15), `ZS_ReactToWeapon_WaitForSecondWarn` (914), `ZS_Smalltalk` (4), `ZS_Smalltalk_Leader` (6), `ZS_Smalltalk_Leader_Loop` (2), `ZS_Smalltalk_Partner` (5), `ZS_Smalltalk_Partner_Loop` (24), `ZS_Talk` (2), `ZS_Talk_Loop` (185), `ZS_Threaten` (99), `ZS_Threaten_Loop` (68)

## Ported functions never called in play (by subsystem)

### gameplay (inventory, trade, mover, treasure, focus, stats) (123)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `lock_holds` (the lock test at the top of FindFocusEntity) | - | the lock test at the top of FindFocusEntity: a locked focus is kept while the entity lives and stays within FocusLockMaxDistance. | DEAD | crates/gameplay/src/focus.rs:378 |
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
| `compare_type` (gCInventoryStack) | Game:0x100c3050 | ascending rank of the use type; out-of-range use types count as None, negative ranks as "last". | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:77 |
| `compare_damage` (gCInventoryStack) | Game:0x100c2df0 | higher damage first. | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:89 |
| `compare_protection` (gCInventoryStack) | Game:0x100c2f10 | higher PROT_BLADE first. | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:95 |
| `compare_template` (gCInventoryStack) | Game:0x100c2a10 | case-insensitive name. | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:105 |
| `compare_value` (gCInventoryStack) | Game:0x100c2b00 | more valuable first. | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:115 |
| `compare_quality` (gCInventoryStack) | Game:0x100c28d0 | compare the quality bits without the damaged flag; at equal bits an undamaged item sorts before a damaged one. | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:121 |
| `compare_quick_slot` (gCInventoryStack) | Game:0x100c2980 | assigned quick slots first, slot 0 counts as 10, unassigned last. | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:137 |
| `compare_slot` (gCInventoryStack) | Game:0x100c29d0 | linked slot order, unlinked stacks after slot 18. | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:149 |
| `compare_focus_name` (gCInventoryStack) | Game:0x100c2c00 | localised focus name, then the localised description. | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:163 |
| `compare_name` (gCInventoryStack) | Game:0x100c2b70 | plain template name. | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:174 |
| `compare_weapon` (gCInventoryStack) | Game:0x100c25c0 | gCInventoryStack::CompareWeapon (Game:0x100c25c0). | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:180 |
| `compare_armor` (gCInventoryStack) | Game:0x100c2610 | gCInventoryStack::CompareArmor (Game:0x100c2610). | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:192 |
| `compare_default` (gCInventoryStack) | Game:0x100c2880 | gCInventoryStack::CompareDefault (Game:0x100c2880). | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:204 |
| `compare_spell` (gCInventoryStack) | Game:0x100c2780 | by the spell template, then name. | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:214 |
| `compare_recipe` (gCInventoryStack) | Game:0x100c26f0 | recipe order, then name. | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:229 |
| `compare` (gCInventoryStack) | Game:0x100c2460 | categories in enum order, inside a category the category-specific chain (weapons, armor, artefacts, alchemy, documents, spells); everything else by lo... | INTERNAL-ONLY | crates/gameplay/src/inventory/order.rs:242 |
| `execute_script` (gCItem_PS) | Game:0x100b9df0 | scripts guarded by a SetGameEvent line run only once per event; otherwise every line runs in order. (The original returns false in both cases and sets... | DEAD | crates/gameplay/src/inventory/script.rs:214 |
| `is_display_slot` (gCInventory_PS) | Game:0x100b34e0 | slots whose item is shown on the character (carry slots, armor, jewellery and the body parts below Helmet). | DEAD | crates/gameplay/src/inventory/slot.rs:97 |
| `equip_slots` (gCInventorySlot) | Game:0x100bd2a0 | Returns the primary and the alternative carry slot of an item with this use type; Slot::None when it has none. | INTERNAL-ONLY ? | crates/gameplay/src/inventory/slot.rs:140 |
| `is_skinned_slot` (gCInventory_PS) | Game:0x100b50b0 | the item in this slot replaces a skinned body part. | DEAD | crates/gameplay/src/inventory/slot.rs:216 |
| `category` (gCInventorySlot) | - | gCInventorySlot::GetCategory. | WIRED-NOT-EXERCISED | crates/gameplay/src/inventory/stack.rs:110 |
| `equip_slots` (gCInventorySlot) | - | primary and alternative slot. | INTERNAL-ONLY ? | crates/gameplay/src/inventory/stack.rs:122 |
| `is_permanent` (gCInventorySlot) | Game:0x100bcfd0 | spells and skills, or items flagged Permanent. | INTERNAL-ONLY ? | crates/gameplay/src/inventory/stack.rs:128 |
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
| `split` (gCInventoryStack) | Game:0x100c09b0 | cut n items off into a new unlinked stack. None unless 0 < n < amount. | WIRED-NOT-EXERCISED | crates/gameplay/src/inventory/stack.rs:240 |
| `item_gold_value` (gCInventoryStack) | Game:0x100c0630 | gCInventoryStack::GetItemGoldValue (Game:0x100c0630). | WIRED-NOT-EXERCISED | crates/gameplay/src/inventory/stack.rs:260 |
| `stack_gold_value` (gCInventoryStack) | Game:0x100c0660 | gCInventoryStack::GetStackGoldValue (Game:0x100c0660). | DEAD | crates/gameplay/src/inventory/stack.rs:266 |
| `is_empty` (gCInventoryStack) | Game:0x100c03b0 | gCInventoryStack::IsEmpty (Game:0x100c03b0). | WIRED-NOT-EXERCISED ? | crates/gameplay/src/inventory/stack.rs:272 |
| `transfer_amount` (gCInventoryStack) | Game:0x100c0ce0 | the marked amount, everything when the mark is negative, never more than the stack holds. | DEAD | crates/gameplay/src/inventory/stack.rs:284 |
| `transfer_remainder` (gCInventoryStack) | Game:0x100c0d00 | what stays behind after the transfer. | DEAD | crates/gameplay/src/inventory/stack.rs:295 |
| `shows_normal_icon` (gCInventoryStack) | Game:0x100c0a40 | the normal icon once the stack is learned or activated, the grey one before. | INTERNAL-ONLY | crates/gameplay/src/inventory/stack.rs:306 |
| `icon_file_names` (gCInventoryStack) | Game:0x100c0ae0 | (normal, grey) icon file names. An empty template icon is replaced by category_default(category) for both. A grey variant (_Grey before the extension)... | DEAD | crates/gameplay/src/inventory/stack.rs:313 |
| `open` (gCMover_PS) | Game:0x1a7780 | ignored while already opening. A closing mover reverses from where it is; instant jumps to the open pose. | WIRED-NOT-EXERCISED ? | crates/gameplay/src/mover.rs:58 |
| `close` (gCMover_PS) | Game:0x1a7a70 | gCMover_PS::Close (Game:0x1a7a70). | WIRED-NOT-EXERCISED | crates/gameplay/src/mover.rs:86 |
| `toggle` (gCMover_PS) | Game:0x1a7130 | open what is closed or closing, close what is open or opening. | WIRED-NOT-EXERCISED | crates/gameplay/src/mover.rs:113 |
| `motion_stopped` (gCMover_PS) | Game:0x1a7240 | the host calls it when the motion's play speed has dropped to zero (it ran to its end). With rest == 0 the finished motions end in the opposite state ... | DEAD | crates/gameplay/src/mover.rs:123 |
| `set_door_status` (gCDoor_PS) | Game:0xcc020 | Status: a door whose status is 0 is open, otherwise closed. | DEAD | crates/gameplay/src/mover.rs:142 |
| `door_status` (gCDoor_PS) | Game:0xcbf80 | the door status for an open flag (0 = open, 1 = closed). | DEAD | crates/gameplay/src/mover.rs:158 |
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
| `loot_key_index` (gCHUDListCtrlInvLootStacks key handler) | Game:0x100a1790 | digit keys jump to the n-th stack; 0 is the tenth. Backspace (code 8) selects none. | DEAD | crates/gameplay/src/trade.rs:280 |
| `generate_plunder` (gCTreasureSet_PS) | Game:0x101b4d90 | copy stacks of the set into target as treasure stacks. Returns false for an empty set or a distribution this method does not handle (weaponry, trader ... | WIRED-NOT-EXERCISED ? | crates/gameplay/src/treasure.rs:138 |
| `generate_plunder_inventory` (gCInventory_PS) | Game:0x100b71f0 | once per inventory, run every named set. Not ported: the 7 percent bonus It_Carrot for one species. Returns true (also when the plunder was generated ... | DEAD | crates/gameplay/src/treasure.rs:332 |

### spell effects / casting (62)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `assess_mist` (AssessMist) | S:5d040 | and AssessMistNoAggro: a character that can notice the caster and is neither fleeing nor asleep reacts to the mist. One already fighting only stops. F... | INTERNAL-ONLY | crates/script/src/magic/area.rs:271 |
| `magic_mist_tick` (MagicMistTick) | S:5c820 | and MagicMistTickNoAggro: characters inside the mist that are not yet under it get the status bit and assess the mist; those that left it lose the bit... | INTERNAL-ONLY | crates/script/src/magic/area.rs:308 |
| `area_damage_tick` (MagicAreaDamageTick) | S:5a0a0 | Candidates are the characters within the radius (nearest first) other than the owner and his party; entities without a character property are skipped.... | INTERNAL-ONLY | crates/script/src/magic/area.rs:324 |
| `zs_stunned` (ZS_Stunned) | S:5dd70 | the victim stumbles and stands dazed; ZS_Stunned_Loop waits out [STUN_SECS] and ZS_Stunned_End stands up and picks the routine (or the fight) up again... | INTERNAL-ONLY | crates/script/src/magic/area.rs:399 |
| `zs_stunned_loop` (ZS_Stunned_Loop) | S:5e300 | wait until the stun is over, then ZS_Stunned_End. | INTERNAL-ONLY | crates/script/src/magic/area.rs:423 |
| `zs_stunned_end` (ZS_Stunned_End) | S:5e4d0 | stand up; a victim who knows its attacker assesses him (AssessAgressor), otherwise the routine continues. | INTERNAL-ONLY | crates/script/src/magic/area.rs:434 |
| `zs_freeze` (ZS_Freeze) | S:58fd0 | the victim stops where it is, its time stands still and the status bit is set (the length was stored in the task position by the hit). | INTERNAL-ONLY | crates/script/src/magic/area.rs:460 |
| `zs_freeze_loop` (ZS_Freeze_Loop) | S:59160 | when the ice melts the victim thaws and turns on whoever froze it (or resumes its routine). | INTERNAL-ONLY | crates/script/src/magic/area.rs:477 |
| `zs_magic_sleep` (ZS_MagicSleep) | S:59350, S:599d0 | and ZS_MagicHypnosis (S:599d0): the victim drops everything it fights with, stands, then lies down (AniStateAction 0x54) in sleep mode. | INTERNAL-ONLY | crates/script/src/magic/area.rs:507 |
| `zs_magic_sleep_loop` (ZS_MagicSleep_Loop) | S:59720, S:59da0 | and ZS_MagicHypnosis_Loop (S:59da0): lie there; after a second the sleeper may wake. A sleeper wakes after 30 s (the hero after 5 s) by the sleep spel... | INTERNAL-ONLY | crates/script/src/magic/area.rs:538 |
| `zs_misted` (ZS_Misted) | S:5d1e0 | a character under the mist goes into reaction mode, stands up and loses its destination; the loop lets it go as soon as the mist bit is gone. | INTERNAL-ONLY | crates/script/src/magic/area.rs:562 |
| `zs_misted_loop` (ZS_Misted_Loop) | S:5d410 | while the 0x40 bit holds the character stands; afterwards the routine continues. | INTERNAL-ONLY | crates/script/src/magic/area.rs:580 |
| `zs_decay` (ZS_Decay) | S:5a4d0 | the entity plays its end effect and is removed. | INTERNAL-ONLY | crates/script/src/magic/area.rs:592 |
| `zs_nothing` (ZS_WaitForMeteor) | S:5a000 | and ZS_WaitForDawn: the DLL's bodies are empty. | INTERNAL-ONLY | crates/script/src/magic/area.rs:599 |
| `magic_summon` (MagicSummon) | S:530e0 | spawn the spell's entity at the target's pose (or beside the caster), owned by the caster, then call FuncOnTargetHit. A second Smn_Light replaces the ... | INTERNAL-ONLY | crates/script/src/magic/cast.rs:327 |
| `is_animal` (IsAnimal) | S:57bd0 | the species CanTameAnimal and CanTransformInto accept. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:56 |
| `is_game` (IsGame) | S:57fc0 | huntable animals (Boar, Bison, Rhino, Deer). | DEAD | crates/script/src/magic/effects.rs:62 |
| `is_large` (IsLarge) | S:57ec0 | the large animals (Minecrawler, Sabertooth, Shadowbeast, Bison, Rhino, Ripper, Alligator); transformations into them last half as long. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:68 |
| `can_tame_animal` (CanTameAnimal) | S:5e800 | an animal; the dangerous ones (Minecrawler, Sabertooth, Shadowbeast, Ripper, Alligator) need Intelligence 100. The hero is told Impossible or TooHard. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:86 |
| `can_transform_into` (CanTransformInto) | S:5ea30 | any animal but a pig; otherwise the hero is told Impossible. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:104 |
| `can_control` (CanControl) | S:549a0 | [I for the exact branch order]: not the hero; only an idle target (routine, reaction or sleeping); no boss, no mage class, no gate guard; humans and g... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:115 |
| `spell_duration_bonus` (GetPlayerSpellDurationBonus) | S:580c0 | +0.5 (50 percent more) for the hero when he has the perk of the spell's school: Perk_Firemage for Innos, Perk_Watermage for Adanos, Perk_Darkmage for ... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:150 |
| `transform_duration` (GetTransformDuration) | S:582a0 | max(INT - 60, 0) + 60 seconds, halved for a large form; the hero gets x1 (x2 with Perk_Druid, plus 0.5 with Perk_Watermage). | INTERNAL-ONLY | crates/script/src/magic/effects.rs:174 |
| `magic_burn` (MagicBurn) | S:56e60 | set the target on fire. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:203 |
| `magic_freeze` (MagicFreeze) | S:56f30 | freeze the target (ZS_Freeze handles the rest). | INTERNAL-ONLY | crates/script/src/magic/effects.rs:209 |
| `magic_poison` (MagicPoison) | S:57000 | poison a target that CanBePoisoned; either way the target holds the caster responsible (AssessTarget with reason ReactToDamage). | INTERNAL-ONLY | crates/script/src/magic/effects.rs:215 |
| `magic_disease` (MagicDisease) | S:57160 | a diseasable target loses all stamina and falls ill; either way it holds the caster responsible. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:225 |
| `magic_refresh` (MagicRefresh) | S:572d0 | mana to the maximum. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:236 |
| `magic_cure` (MagicCure) | S:53870 | hit points to the maximum. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:243 |
| `magic_cure_poison` (MagicCurePoison) | S:53970 | clear the poison. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:250 |
| `magic_cure_disease` (MagicCureDisease) | S:53a90 | clear the disease. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:258 |
| `magic_heal_other` (MagicHealOther) | S:56b20 | clear poison, disease, fire and ice of the target and restore its hit points to the maximum. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:266 |
| `magic_keep_stamina` (MagicKeepStamina) | S:58b70 | only the hero; the engine parks a "stamina held at maximum" flag on him (it reuses GuardStatus and LastDistToGuardPoint). | INTERNAL-ONLY | crates/script/src/magic/effects.rs:285 |
| `magic_speed_up` (MagicSpeedUp) | S:58a30 | only the hero; he runs fast for (StaminaMax - 200) / 2 + 60 seconds (never less than 60). | INTERNAL-ONLY | crates/script/src/magic/effects.rs:295 |
| `magic_stun` (MagicStun) | S:58cb0 | every character within 12 m, nearest first, that is neither dead nor down, not already stunned, not on guard duty and not in the caster's party gets t... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:306 |
| `magic_dispel` (MagicDispel) | S:53460 | for a damage element (ice 5, fire 4, lightning 6 — the DLL's parameter 4, 5 or 6 picks it) remove the area-spell entities of that element within 25 m ... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:336 |
| `magic_tame_animal` (MagicTameAnimal) | S:54e70 | if the animal can be tamed it (and its herd) follows the hero as a controlled member of his party. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:371 |
| `magic_word_of_control` (MagicWordOfControl) | S:54c00 | a controllable target joins the party. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:387 |
| `magic_charm` (MagicCharm) | S:56770 | a target in talk mode forgets every grudge (attitude, attack reasons, last fight, crime) and the charmer is treated as a friend; a boss resists. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:397 |
| `magic_bloodlust` (MagicBloodlust) | S:55cb0 | a boss only turns on the caster; others who can fight attack the nearest other fighter within 10 m (reason 17 = Duel) or, with none, the caster. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:415 |
| `magic_fear` (MagicFear) | S:322f0, S:55eb0 | a target that can fight (not a boss, not the gate guard) is told by fight sound, stops and flees from the caster. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:443 |
| `magic_sleep` (MagicSleep) | S:54670 | a target that is not a boss falls under ZS_MagicSleep. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:456 |
| `magic_hypnosis` (MagicHypnosis) | S:543b0 | every character within 20 m (nearest first) that is idle (routine, reaction, flee, arena...), not the caster, not in his party and not a boss falls un... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:465 |
| `magic_call_animals` (MagicCallAnimals) | S:54160 | Intelligence over 300 calls three animals, over 200 two, otherwise one. Each is a wild animal within 250 m that can reach the caster, can attack and i... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:505 |
| `summon_party_monster` (the private helper SummonPartyMonster) | S:55210 | spawn a creature beside the caster, make it a summoned member of his party (dialog-enabled, following, in the hero's political camp) and return it. NO... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:539 |
| `magic_summon_demon` (MagicSummonDemon) | S:554e0 | MagicSummonDemon (S:554e0). | INTERNAL-ONLY | crates/script/src/magic/effects.rs:570 |
| `magic_summon_skeleton` (MagicSummonSkeleton) | S:555d0 | a sword skeleton. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:576 |
| `magic_summon_golem` (MagicSummonGolem) | S:556c0 | MagicSummonGolem (S:556c0). | INTERNAL-ONLY | crates/script/src/magic/effects.rs:582 |
| `magic_summon_goblin` (MagicSummonGoblin) | S:557b0 | MagicSummonGoblin (S:557b0). | INTERNAL-ONLY | crates/script/src/magic/effects.rs:588 |
| `magic_summon_army_of_darkness` (MagicSummonArmyOfDarkness) | S:558a0 | skeletons and as many zombie slaves, three each below Intelligence 250, four from 250 and five from 300. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:605 |
| `magic_summon_army_of_cuteness` (MagicSummonArmyOfCuteness) | S:56950 | rabbits, six below Intelligence 250, seven from 250 and eight from 300. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:619 |
| `magic_summon_monster` (MagicSummonMonster) | S:55a20 | the template is what the caster's spell spawns; the hero's version is a Shadowbeast when the spell has none. Characters other than the hero keep what ... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:629 |
| `magic_summon_companion` (MagicSummonCompanion) | S:55b90 | the creature named by the spell's Spawn. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:642 |
| `send_perception_transform` (SendPerceptionTransform) | S:53f40 | everybody who can notice the change and had the old body as its current or alternative target now has the new one (the DLL also moves the alignment ta... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:687 |
| `magic_transform` (MagicTransform) | S:57880 | the hero becomes the creature of the spell's Spawn (a druid stone or a potion) for [transform_duration] seconds; the hands are emptied (the stone itse... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:705 |
| `magic_animal_form` (MagicAnimalForm) | S:53bb0 | another character (the focus) that passes CanTransformInto is turned into the spell's animal for the transform duration; the caster puts his hands dow... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:733 |
| `transform_back` (TransformBack) | S:58610 | the transformation ends — the hero (or the animal-formed character) takes his own shape back at the animal's place, the alien body is removed. | WIRED-NOT-EXERCISED | crates/script/src/magic/effects.rs:763 |
| `magic_open_locks` (MagicOpenLocks) | S:56c40 | the focused lock opens when it has no key (the spell opens ordinary locks), or when its difficulty is below 5 and the caster... [I]: a lock of difficu... | INTERNAL-ONLY | crates/script/src/magic/effects.rs:782 |
| `magic_soul_travel` (MagicSoulTravel) | S:56440 | the hero leaves his body: a ghost copy takes his place (status 0x80), he becomes the ghost (status 0x200 marks the real body). | INTERNAL-ONLY | crates/script/src/magic/effects.rs:797 |
| `return_to_body` (ReturnToBody) | S:58470 | the soul returns, the ghost marks are cleared. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:813 |
| `magic_telekinesis` (MagicTelekinesis) | S:56170 | toggles the focused mover (levers) or lifts the focused loose item toward the caster; the host moves it. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:826 |
| `magic_time_bubble` (MagicTimeBubble) | S:566b0 | the spell's Smn_Timebubble entity does the work; the function itself only exists to be called. | INTERNAL-ONLY | crates/script/src/magic/effects.rs:833 |

### script helpers / predicates / stats (51)

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
| `ambient_action_secs` (the duration helper) | - | ambient creatures idle rand(11) + 10 seconds, every other NPC rand(31) + 30; 10 s with the debug byte. ambient is IsAmbientCreature. | INTERNAL-ONLY | crates/script/src/helpers.rs:125 |
| `routine_point` (the routine-point helper) | - | the NPC's working, relaxing or sleeping point depending on GetRoutineTime (0 sleeping, 2 relaxing, otherwise working). | WIRED-NOT-EXERCISED ? | crates/script/src/helpers.rs:148 |
| `use_object` (the use-object accessor) | - | the interaction object currently found for npc for a use type (the scanner S:7c90 fills the table), or NONE. | WIRED-NOT-EXERCISED ? | crates/script/src/helpers.rs:156 |
| `is_undead` (IsUndead) | S:112f0 | Skeleton or Zombie. | WIRED-NOT-EXERCISED ? | crates/script/src/predicates.rs:27 |
| `is_evil` (IsEvil) | S:10aa0 | undead, Golem, Demon, Gargoyle, FireGolem, IceGolem or ScorpionKing. | WIRED-NOT-EXERCISED ? | crates/script/src/predicates.rs:38 |
| `can_sleep` (CanSleep) | - | species that use sleeping points. | DEAD | crates/script/src/predicates.rs:86 |
| `can_threaten` (CanThreaten) | - | species that can play the threaten reaction (Goblin through Bloodfly, SwampLurker, Ogre, Stalker, Dragon). | INTERNAL-ONLY | crates/script/src/predicates.rs:116 |
| `can_eat_ground` (CanEatGround) | - | grazing and scavenging species. | DEAD | crates/script/src/predicates.rs:148 |
| `can_swim` (CanSwim) | S:bd420 | a stub that always returns false in the shipped script. | DEAD | crates/script/src/predicates.rs:176 |
| `has_attack_reason_to_watch` (HasAttackReasonToWatch) | S:10550 | reasons 17..=18 and 20 (Duel, Arena, Kill). | DEAD | crates/script/src/predicates.rs:203 |
| `zone_attitude_to_player` (GetZoneAttitudeToPlayer) | - | how the owner of zone regards the player.  Public zones owned by an enclave: security level Political compares the player's political fame of the owne... | INTERNAL-ONLY | crates/script/src/predicates.rs:212 |
| `is_droppable_weapon` (IsDroppableWeapon) | S:10970 | an item of non-zero category whose use type is a one-hander, two-hander, bow, crossbow, shield, staff, torch, halberd or axe. | INTERNAL-ONLY | crates/script/src/predicates.rs:263 |
| `set_last_fight_timestamp` (SetLastFightTimestamp) | S:113d0 | when other is the player, stamp the NPC's LastFightTimestamp with the current play time. | WIRED-NOT-EXERCISED ? | crates/script/src/predicates.rs:366 |
| `is_big` (IsBig) | S:57de0 | trolls, trexes and dragons. | INTERNAL-ONLY | crates/script/src/speech.rs:42 |
| `start_say_attack_end` (StartSayAttackEnd) | S:50cf0 | the remark after a fight ends. Humanoid opponents that died get "Victory", knocked-down ones a line that depends on why the fight started; monsters ge... | DEAD | crates/script/src/speech.rs:51 |
| `start_say_attack_reason` (StartSayAttackReason) | S:512d0 | the shout at the start of a fight. An upset NPC always says "IGetYouStill"; otherwise the line follows the attack reason, with separate choices for mo... | DEAD | crates/script/src/speech.rs:91 |
| `start_say_defeat_comment` (StartSayDefeatComment) | S:51040 | a bystander comments on a knocked-out fighter. Only fights the bystander is not part of get a remark, and then only 30% of the time. | INTERNAL-ONLY | crates/script/src/speech.rs:143 |
| `start_say_confirm` (StartSayConfirm) | S:51680 | party members have three variants, everybody else "Sure". | DEAD | crates/script/src/speech.rs:173 |
| `start_say_follow` (StartSayFollow) | S:518f0 | the companion's reaction to "follow me" (only when the player gives the order). | DEAD | crates/script/src/speech.rs:184 |
| `start_say_holding_position` (StartSayHoldingPosition) | S:517c0 | "Sure" for NPCs without party voices and for party members of type 3 (a hired fighter), "PartyWait" for the other companions. | DEAD | crates/script/src/speech.rs:195 |
| `start_say_aargh` (StartSayAargh) | S:528a0 | the pain grunt, rotating AARGH_00..02. The first call says _00; afterwards the counter moves to 0 or 2 (or 1/2 right after the first) by a coin flip s... | DEAD | crates/script/src/speech.rs:208 |
| `start_say_refuse_talk` (StartSayRefuseTalk) | S:52280 | a sleeping NPC says "SleepRefuse"; everybody else rotates through three refusals (what they are busy with, a polite excuse, a brush-off). The second a... | INTERNAL-ONLY | crates/script/src/speech.rs:262 |
| `start_say_crime_refuse` (StartSayCrimeRefuse) | S:524c0 | an NPC that knows about a crime of the player refuses to talk; after a theft the three steps are "DontStealAnything", "TheftRefuse", "GetLost", otherw... | INTERNAL-ONLY | crates/script/src/speech.rs:294 |
| `start_say_zone_refuse` (StartSayZoneRefuse) | S:52640 | an NPC in a zone hostile to the player refuses to talk. In a locked-down (Enclave-level) zone the first step names the owning enclave ("NoFriendOfORC"... | INTERNAL-ONLY | crates/script/src/speech.rs:310 |
| `start_say_doesnt_work` (StartSayDoesntWork) | S:6f660 | the player's grunt when an object cannot be used. | INTERNAL-ONLY | crates/script/src/speech.rs:339 |
| `start_say_nothing_to_get` (StartSayNothingToGet) | S:6f560 | the player's comment on an emptied container. | INTERNAL-ONLY | crates/script/src/speech.rs:346 |
| `boss_news_plan` (_AI_SayBossNews) | S:3c490 | the line plan of a boss telling the player about the crimes against his faction. After the opener "BadNews" the lines depend on the level; at level 3 ... | INTERNAL-ONLY | crates/script/src/speech.rs:486 |
| `crime_investigation_plan` (_AI_SayCrimeInvestigation) | S:3be50 | the question an investigator asks and the answer. crime 1 is a murder, 2 a theft; step 1 asks, 2 accuses. The follow-up (reached when the player had s... | DEAD | crates/script/src/speech.rs:530 |
| `guard_news_plan` (_AI_SayGuardNews) | S:3a3c0 | as a line plan [I]. The guard stops the player ("Halt"), names the crime he knows (murders/thefts counts of the enclave: "KnownMurder" or "KnownTheft"... | DEAD | crates/script/src/speech.rs:553 |
| `level` (GetLevel) | S:36cb0 | the NPC's Level property, +10 on hard and -10 on easy (minimum 1). The player is never adjusted. | WIRED-NOT-EXERCISED ? | crates/script/src/stats.rs:24 |
| `level_max` (GetLevelMax) | S:36db0 | LevelMax with the same difficulty shift as [level]. | WIRED-NOT-EXERCISED ? | crates/script/src/stats.rs:31 |
| `current_level` (GetCurrentLevel) | S:36b40 | Player: the Level property, or (LevelMax + Level) / 2 (minimum 1) while transformed. Everyone else: min(GetLevel + Level, GetLevelMax). The decompiled... | WIRED-NOT-EXERCISED ? | crates/script/src/stats.rs:45 |
| `diff_to_player_level` (GetDiffToPlayerLevel) | S:36eb0 | CurrentLevel(npc) - player level, 0 for the player. | DEAD | crates/script/src/stats.rs:68 |
| `attribute` (GetStrength) | S:33540, S:33630 | GetDexterity (S:33630), GetIntelligence (S:33720), GetAlchemy (S:33810), GetSmithing (S:33900), GetTheft (S:339f0).  Player: the player-memory base va... | WIRED-NOT-EXERCISED ? | crates/script/src/stats.rs:87 |
| `set_attribute` (SetStrength) | S:33ae0, S:33ba0 | SetDexterity (S:33ba0), SetIntelligence (S:33c60), SetAlchemy (S:33d20), SetSmithing (S:33de0), SetTheft (S:33ea0): player only. | DEAD | crates/script/src/stats.rs:100 |
| `add_attribute` (AddStrength) | S:33f60, S:34060 | AddDexterity (S:34060), AddIntelligence (S:34160), AddAlchemy (S:34260), AddSmithing (S:34360), AddTheft (S:34460): adds to the player's base value an... | INTERNAL-ONLY | crates/script/src/stats.rs:110 |
| `protection` (GetProtectionBlade) | S:34560, S:34690 | GetProtectionImpact (S:34690), GetProtectionMissile (S:347c0), GetProtectionFire (S:348c0), GetProtectionIce (S:34a10), GetProtectionLightning (S:34b6... | WIRED-NOT-EXERCISED ? | crates/script/src/stats.rs:121 |
| `set_protection` (SetProtectionBlades) | S:34c60, S:34d20 | SetProtectionImpact (S:34d20), SetProtectionMissile (S:34de0), SetProtectionFire (S:34ea0), SetProtectionIce (S:34f60), SetProtectionLightning (S:3502... | DEAD | crates/script/src/stats.rs:150 |
| `add_protection` (AddProtectionBlades) | S:350e0, S:351a0 | ...Impact (S:351a0), ...Missile (S:35260), ...Fire (S:35320), ...Ice (S:353e0), ...Lightning (S:354a0): player only. | DEAD | crates/script/src/stats.rs:161 |
| `pool_max` (GetHitPointsMax) | S:35660, S:35e10 | GetManaPointsMax (S:35e10), GetStaminaPointsMax (S:36440).  NPC hit points: 1 for ambient creatures, else max(100, 20 * LevelMax), 0 without a damage ... | INTERNAL-ONLY | crates/script/src/stats.rs:187 |
| `set_pool_max` (SetHitPointsMax) | S:359f0, S:36080 | SetManaPointsMax (S:36080), SetStaminaPointsMax (S:367a0): writes the maximum and re-clamps the current value (HitPointsMax also floors the current va... | INTERNAL-ONLY | crates/script/src/stats.rs:271 |
| `add_hit_points` (AddHitPoints) | S:35b50 | SetHitPoints(GetHitPoints + delta). No god-mode rule here. | INTERNAL-ONLY | crates/script/src/stats.rs:304 |
| `add_pool_max` (AddHitPointsMax) | S:35c20, S:36260 | AddManaPointsMax (S:36260), AddStaminaPointsMax (S:36a20): the player's maximum grows (and the gain is printed as HP/MP/SP); no-op for NPCs. | DEAD | crates/script/src/stats.rs:341 |

### world (entity admin, PVS, sectors, triggers, zones) (46)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `decay` (eCDynamicEntity) | Engine:0x1022a1b0 | states are the decay states of the entity's property sets after asking each to decay. An entity without property sets is done at once. | DEAD | crates/world/src/dynamic.rs:27 |
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
| `collect` (gCSectorPersistence_PS) | Game:0x101a18f0 | the save image. | WIRED-NOT-EXERCISED | crates/world/src/sector.rs:257 |
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
| `on_enter_processing_range` (eCTrigger_PS) | Engine:0x1007e120 | triggers that react to the processing range fire for their own entity. | DEAD | crates/world/src/trigger.rs:451 |
| `on_cache_in` (eCTrigger_PS) | Engine:0x1007e160 | eCTrigger_PS::OnCacheIn (Engine:0x1007e160). | DEAD | crates/world/src/trigger.rs:472 |
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

### ZS_Attack / combat AI (42)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `fighters_on` (S) | - | are engaged in combat against target. | INTERNAL-ONLY | crates/script/src/combat/attack.rs:136 |
| `update_combat_state` (S) | - | S:14f0: may the NPC step up to the front line? A fighter already engaged with a target stays engaged; otherwise it joins while fewer than the allowed ... | INTERNAL-ONLY | crates/script/src/combat/attack.rs:151 |
| `cancel_attack` (CancelAttack) | S:1a4d0 | give the fight up. A fight against the player is recorded as cancelled; the NPC returns to its routine. | INTERNAL-ONLY | crates/script/src/combat/attack.rs:178 |
| `on_attack_positioning` (OnAttack_Positioning) | S:16190 | walk in until the distance drops under 5 m (or a second passes); closer than that, circle toward the free side first. | INTERNAL-ONLY | crates/script/src/combat/attack.rs:579 |
| `on_attack_strafe` (OnAttack_Strafe) | S:16a10 | strafe for 0.6 s. | INTERNAL-ONLY | crates/script/src/combat/attack.rs:637 |
| `on_ai_move` (the OnAI_* animation-event callbacks) | S:38580, S:41e30 | the first call after the hit window opened moves the stage counter to 1 (damage strip on). Returns true exactly then. A hack counts a second stage (St... | DEAD | crates/script/src/combat/commands.rs:114 |
| `start_parade` (_AI_Parade) | S:46810, S:46d80 | and OnAI_Parade (S:46d80): the guard goes up (Action = Parade, pose Parade) and holds. Returns the seconds the pose lasts before the callback lowers i... | INTERNAL-ONLY | crates/script/src/combat/commands.rs:138 |
| `start_stand_up` (_AI_StandUp) | S:4a950 | leave any sitting, lying or kneeling pose. Returns whether an animation has to play. | DEAD | crates/script/src/combat/commands.rs:149 |
| `start_shoot` (_AI_Shoot) | S:4a710 | shooting consumes one arrow or bolt from the stack in the off hand; returns whether a projectile was available (PSInventory::HasItems). | DEAD | crates/script/src/combat/commands.rs:171 |
| `start_die` (_AI_Die) | S:3e510 | the dying NPC plays Die and is out of the fight. | DEAD | crates/script/src/combat/commands.rs:183 |
| `start_stumble` (_AI_Stumble, _AI_ParadeStumble, _AI_QuickStumb) | S:43490, S:46f80 | the hit reaction starts (Action = the stumble); the host plays it for the length it knows. | DEAD | crates/script/src/combat/commands.rs:189 |
| `start_lie_knock_down` (_AI_LieKnockDown) | S:467c0 | the knocked-down NPC lies on the ground. | DEAD | crates/script/src/combat/commands.rs:197 |
| `start_get_up` (_AI_GetUpAttack / _AI_GetUpParade) | S:41bd0, S:41f20 | a knocked-down fighter attacks or parries on its way up; the pose becomes standing and the action the get-up move. | DEAD | crates/script/src/combat/commands.rs:203 |
| `start_reload` (_AI_Reload) | S:4a4f0 | reloading a crossbow restores the action to Reload. | DEAD | crates/script/src/combat/commands.rs:218 |
| `arena_enter` (ZS_Arena) | S:119a0 | first half: the fighter walks to its arena start point in AI mode Arena. | INTERNAL-ONLY | crates/script/src/combat/misc.rs:27 |
| `on_arena` (OnArena) | S:11e60 | once the player stands in the arena zone the fighter attacks him with reason Arena. | INTERNAL-ONLY | crates/script/src/combat/misc.rs:54 |
| `arena_loop` (ZS_Arena_Loop) | S:11f40 | an arena that is no longer running ends the bout: the fighter says "YouLeftTheArena", the player gets the warning message and the NPC leaves via ZS_At... | INTERNAL-ONLY | crates/script/src/combat/misc.rs:68 |
| `wait_free_loop` (ZS_WaitForFreeDestination_Loop) | S:42db0 | every second look whether the player has moved more than 1.5 m away from the destination point; then the routine continues. | INTERNAL-ONLY | crates/script/src/combat/misc.rs:126 |
| `on_revolution_attack` (OnRevolutionAttack) | S:61090 | the first enemy that shows up is attacked with reason Revolution. | INTERNAL-ONLY | crates/script/src/combat/misc.rs:168 |
| `success_enter` (ZS_RevolutionSuccess) | S:611e0 | the winners put everything away, face the player and applaud. | INTERNAL-ONLY | crates/script/src/combat/misc.rs:184 |
| `end_goto` (EndGoto) | S:42880 | act on how a goto instruction ended.  Arrival clears "can be pushed while idle". A blocked destination (the player is in the way) makes the NPC wait (... | DEAD | crates/script/src/combat/misc.rs:248 |
| `handle_controlled_return_home` (HandleControlledReturnHome) | S:42bc0 | a controlled (mind-controlled) creature whose master is not the player any more and that stands on its destination becomes a plain creature again (Par... | DEAD | crates/script/src/combat/misc.rs:279 |
| `on_wait_for_player` (OnWaitForPlayer) | S:86d50 | a plain perception poll. | INTERNAL-ONLY | crates/script/src/combat/misc.rs:294 |
| `choose_offensive` (S) | - | S:6e60: the offensive-only picker (Pierce, Power, Whirl, Hack, SimpleWhirl, Attack, Quick), skill-gated. | INTERNAL-ONLY | crates/script/src/combat/moves.rs:807 |
| `choose_defensive` (S) | - | S:7030: the defensive-only picker (Parade, JumpBack, Left, Right, Back), skill-gated. | INTERNAL-ONLY | crates/script/src/combat/moves.rs:827 |
| `close_range` (GetCloseRange) | S:4eb70 | stop distance of the approach (600 for the big monsters, else 500). | WIRED-NOT-EXERCISED ? | crates/script/src/combat/range.rs:90 |
| `on_react_to_damage` (OnReactToDamage) | S:20af0 | poll the surroundings; give up when the target is farther than 7 m (30 m for archers and mages) or after ten seconds. | INTERNAL-ONLY | crates/script/src/combat/react.rs:87 |
| `on_react_to_damage_go_back` (OnReactToDamage_GoBack) | S:20f90 | stop walking once within 3 m. | INTERNAL-ONLY | crates/script/src/combat/react.rs:126 |
| `on_react_to_zone` (OnReactToZone) | S:221c0 | the trespasser within 5 m is told to leave after 4 s ("ISaidGetOutOfHere", enclave zones) and attacked with reason Intruder after 8 s; leaving the gua... | INTERNAL-ONLY | crates/script/src/combat/react.rs:268 |
| `on_flee_goto_fleepoint` (OnFlee_GotoFleePoint) | S:1dff0 | every half second check whether the flee point is reached (5 m); then flee again from the pursuer, or watch for a follower. | INTERNAL-ONLY | crates/script/src/combat/react.rs:385 |
| `on_flee_go_back` (OnFlee_GoBack) | S:1e470 | once the pursuer is within 20 m watch it. | INTERNAL-ONLY | crates/script/src/combat/react.rs:436 |
| `on_watch_fight` (OnWatchFight) | S:27e40 | a fight where somebody is dead or down ends the show (applause for the player's victory); a fight that left the arena zone is ignored. | INTERNAL-ONLY | crates/script/src/combat/react.rs:498 |
| `on_watch_fight_go_back` (OnWatchFight_GoBack) | S:286b0 | stop once the fight is 6 m away again. | INTERNAL-ONLY | crates/script/src/combat/react.rs:576 |
| `on_watch_fight_goto` (OnWatchFight_Goto) | S:28a60 | walk toward the fight until 6 m away; chasing longer than GetFollowTime makes the spectator wait for the fighters to flee. | INTERNAL-ONLY | crates/script/src/combat/react.rs:609 |
| `cheer_fight` (CheerFight) | S:28bb0 | a spectator's voice reaction to a seen hit. Friends of the hit fighter groan ("Ooh"), the others cheer; heavy reactions get "HeavyHit". Returns the li... | INTERNAL-ONLY | crates/script/src/combat/react.rs:677 |
| `on_eat_body_perception` (OnEatBody_Perception) | S:1d5a0 | a meal that is disturbed by a hostile creature within 5 m is defended (AssessTarget with reason DriveAway) against up to five candidates. | INTERNAL-ONLY | crates/script/src/combat/react.rs:862 |
| `lee_stand` (ZS_Lee_Teleport) | S:25c00 | Lee stands up, takes the Vengard temple teleport rune and casts it, then goes back to his routine. | INTERNAL-ONLY | crates/script/src/combat/react.rs:998 |
| `fai_passive` (FAI_Passive) | S:4ffb0 | a fighter that has not been admitted to the front line keeps 3.5 to 4.5 m from its target: back off when closer, approach when farther, otherwise circ... | INTERNAL-ONLY | crates/script/src/combat/select.rs:132 |
| `los_for_attack` (S) | - | S:4e8d0: beyond 2 m the line of sight is checked at knee height, closer by the ordinary line. | INTERNAL-ONLY | crates/script/src/combat/select.rs:174 |
| `fai_ranged` (FAI_Ranged) | S:50370 | beyond 20 m run in; without a line of sight look for a new target; then shoot. | INTERNAL-ONLY | crates/script/src/combat/select.rs:185 |
| `fai_magic` (FAI_Magic) | S:50550 | spells that need no aim (focus) retreat inside 3.5 m (eight times in ten), cast between 3.5 and 11.75 m and approach beyond; aimed spells are cast wit... | INTERNAL-ONLY | crates/script/src/combat/select.rs:204 |
| `fai_misted` (FAI_Misted) | S:50a20 | a mind-controlled NPC casts or shoots within 7 m and otherwise fights like an engaged melee fighter. | INTERNAL-ONLY | crates/script/src/combat/select.rs:253 |

### animation (EMotionFX wrapper, blending, attachments) (38)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `stop_slot` (eCWrapper_emfx2Actor) | Engine:0x10047910 | Zero fade removes the instance, otherwise it fades out like SwitchFadeOut. | DEAD | crates/animation/src/animator.rs:174 |
| `stop_at_loop_end` (eCWrapper_emfx2Actor) | Engine:0x100479c0 | eCWrapper_emfx2Actor::StopAtLoopEnd (Engine:0x100479c0). | INTERNAL-ONLY ? | crates/animation/src/animator.rs:185 |
| `set_target_weight` (eCWrapper_emfx2Actor) | Engine:0x10049ca0 | eCWrapper_emfx2Actor::SetTargetWeight (Engine:0x10049ca0). | INTERNAL-ONLY ? | crates/animation/src/animator.rs:193 |
| `set_blend_mode` (eCWrapper_emfx2Actor) | Engine:0x10049d10 | eCWrapper_emfx2Actor::SetBlendMode (Engine:0x10049d10). | INTERNAL-ONLY ? | crates/animation/src/animator.rs:201 |
| `is_slot_playing` (eCWrapper_emfx2Actor) | Engine:0x1004a040 | eCWrapper_emfx2Actor::IsActorPlaying (Engine:0x1004a040). | DEAD | crates/animation/src/animator.rs:225 |
| `add` (eCVisualAnimation_PS) | Engine:0x10030da0 | any persistent attachment of the same entity is removed first, then the new one is appended, so adding twice never duplicates and the last bone wins. | WIRED-NOT-EXERCISED ? | crates/animation/src/attach.rs:45 |
| `add_once` (eCVisualAnimation_PS) | Engine:0x10030f50 | like [add] but the attachment is placed a single time. The entity's own rendering and processing are switched off by the caller (the item is drawn thr... | DEAD | crates/animation/src/attach.rs:59 |
| `remove` (eCVisualAnimation_PS) | Engine:0x10030e20 | removes the persistent attachment of entity, keeping the order of the others. | WIRED-NOT-EXERCISED ? | crates/animation/src/attach.rs:76 |
| `remove_once` (eCVisualAnimation_PS) | Engine:0x10030fe0 | eCVisualAnimation_PS::RemoveOnceAttachment (Engine:0x10030fe0). | DEAD | crates/animation/src/attach.rs:89 |
| `clear` (ClearAttachments) | Engine:0x10030bc0 | ClearAttachments (Engine:0x10030bc0). | WIRED-NOT-EXERCISED ? | crates/animation/src/attach.rs:101 |
| `clear_once` (ClearOnceAttachments) | Engine:0x10030cb0 | ClearOnceAttachments (Engine:0x10030cb0). | DEAD | crates/animation/src/attach.rs:107 |
| `relink` (RelinkAttachments) | Engine:0x10031110 | forgets the cached bone nodes (the skeleton changed, for instance after a head was attached). | DEAD | crates/animation/src/attach.rs:113 |
| `process` (the attachment part of OnProcess) | Engine:0x10032d40 | the one-shot list is flushed first (each entry placed once and dropped), then every persistent attachment is placed at actor_world * node_world * offs... | WIRED-NOT-EXERCISED ? | crates/animation/src/attach.rs:138 |
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

### damage pipeline (38)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `assess_warn` (AssessWarn) | S:318c0 | a warning shout makes calm bystanders pay attention.  Receivers that are not in Routine mode, friend-types, the player's companions, strangers farther... | INTERNAL-ONLY | crates/script/src/assess.rs:37 |
| `send_perception_warn` (SendPerceptionWarn) | S:32bd0 | SendPerceptionWarn (S:32bd0). | INTERNAL-ONLY | crates/script/src/assess.rs:77 |
| `assess_cmd_attack` (AssessCmdAttack) | S:29e70 | a companion attacks what the player focuses. | INTERNAL-ONLY | crates/script/src/assess.rs:85 |
| `send_perception_cmd_attack` (SendPerceptionCmdAttack) | S:31ff0 | every companion of the player hears the order. | DEAD | crates/script/src/assess.rs:96 |
| `assess_cmd_follow` (AssessCmdFollow) | S:56a80 | a companion that waits stops waiting and follows again. | INTERNAL-ONLY | crates/script/src/assess.rs:107 |
| `send_perception_cmd_follow` (SendPerceptionCmdFollow) | S:32170 | SendPerceptionCmdFollow (S:32170). | DEAD | crates/script/src/assess.rs:118 |
| `assess_fight_sound` (AssessFightSound) | S:2a510 | a fight between attacker and victim is heard.  Fighters, fleeing, talking and boss NPCs, waiting companions and arena fights are not moved; others wit... | INTERNAL-ONLY | crates/script/src/assess.rs:165 |
| `send_perception_fight_sound` (SendPerceptionFightSound) | S:322f0 | SendPerceptionFightSound (S:322f0). | DEAD | crates/script/src/assess.rs:233 |
| `assess_murder` (AssessMurder) | S:2eee0 | murderer killed victim where witness can tell.  Fleeing NPCs are out; within 20 m of either (and with a free line of sight to one of them) the witness... | INTERNAL-ONLY | crates/script/src/assess.rs:245 |
| `send_perception_murder` (SendPerceptionMurder) | S:32720 | SendPerceptionMurder (S:32720). | DEAD | crates/script/src/assess.rs:295 |
| `assess_defeat` (AssessDefeat) | S:29fb0 | a knock-out is seen within 20 m with a free line of sight; humanoid bystanders friendly to the loser who were not fighting it comment on it (StartSayD... | INTERNAL-ONLY | crates/script/src/assess.rs:303 |
| `send_perception_defeat` (SendPerceptionDefeat) | S:32600 | SendPerceptionDefeat (S:32600). | DEAD | crates/script/src/assess.rs:328 |
| `assess_player_enter_zone` (AssessPlayerEnterZone) | S:30000 | the player stepped into a zone with an owner within 7 m of the witness: a calm witness goes and observes (ZS_ObserveZonePlayer). | INTERNAL-ONLY | crates/script/src/assess.rs:370 |
| `send_perception_enter_zone` (SendPerceptionEnterZone) | S:33170 | SendPerceptionEnterZone (S:33170). | DEAD | crates/script/src/assess.rs:391 |
| `send_perception_preach` (SendPerceptionPreach) | S:32cf0 | calm humanoids within 10 m of a preaching NPC stop and listen (ZS_Listen). | DEAD | crates/script/src/assess.rs:429 |
| `weakness` (the damage-type weakness helper) | - | immune, normal or weak.  Impact doubles against Skeleton and Golem; Missile cannot hurt Golem, FireGolem and (unless the damage entity is called "...F... | INTERNAL-ONLY | crates/script/src/damage.rs:122 |
| `armor_protection` (GetProtection) | S:2de50 | the victim's protection against a damage type, with the armor perks applied.  Physics and None use Impact. Without ExtendedContent a robe with Perk_Li... | INTERNAL-ONLY | crates/script/src/damage.rs:162 |
| `move_strength` (M) | S:2b3a0 |  (S:2b3a0): how much a move contributes to the hit strength class. | WIRED-NOT-EXERCISED ? | crates/script/src/damage.rs:201 |
| `power_rank` (R) | S:2afc0 |  (S:2afc0): power rank 0..=3. The player's comes from the equipped weapon perks, an NPC's from its level (>= 45: 3, >= 30: 2, >= 15: 1). | WIRED-NOT-EXERCISED ? | crates/script/src/damage.rs:225 |
| `defence_value` (D) | S:2b470 |  (S:2b470): defence value: 2 while getting up with a parade, otherwise the power rank plus one for a shield (NPCs always, the player only with Perk_Sh... | INTERNAL-ONLY | crates/script/src/damage.rs:260 |
| `quality_bonus` (the weapon-quality bonus) | S:6af10 | a damaged weapon (0x100) loses half the base, each of the bonus flags 0x20, 0x40, 0x80 adds 10. | WIRED-NOT-EXERCISED ? | crates/script/src/damage.rs:289 |
| `skill_bonus` (GetPlayerSkillDamageBonus) | S:2e410, S:57fc0 | the player's perk bonus on a hit of base damage against victim. Only for the player; the two Hunter perks (game versus other animals, S:57fc0/57bd0) a... | WIRED-NOT-EXERCISED ? | crates/script/src/damage.rs:305 |
| `can_be_killed` (CanBeKilled) | S:2da00 | essential NPCs cannot be killed [I]. | WIRED-NOT-EXERCISED ? | crates/script/src/damage.rs:376 |
| `is_deadly_damage` (IsDeadlyDamage) | S:2db80 | lethal or merely knocking out?  Killable victims die from bosses' hits... precisely: a victim that is a Boss or named "Xardas", any spell, any non-Imp... | INTERNAL-ONLY | crates/script/src/damage.rs:382 |
| `can_burn` (CanBurn) | S:2d530 | fire damage ignites unless the victim is a Golem, Demon, Troll, FireGolem, IceGolem or Dragon or (the player) has Perk_ResistHeat. | WIRED-NOT-EXERCISED ? | crates/script/src/damage.rs:414 |
| `can_freeze` (CanFreeze) | S:2d7b0 | same exclusions as [can_burn] with Perk_ResistCold. | WIRED-NOT-EXERCISED ? | crates/script/src/damage.rs:429 |
| `can_be_poisoned` (CanBePoisoned) | S:2d9b0 | not for evil victims or with Perk_ImmuneToPoison. | INTERNAL-ONLY | crates/script/src/damage.rs:443 |
| `can_be_diseased` (CanBeDiseased) | - | CanBeDiseased: not with Perk_ImmuneToDisease. | INTERNAL-ONLY | crates/script/src/damage.rs:449 |
| `is_in_parade_pose` (IsInParadePose) | - | is the victim's body pose compatible with its hands? Sword and shield need primary pose 2, other fighting pairs pose 1 or 2, everything else pose 1 [U... | INTERNAL-ONLY | crates/script/src/damage.rs:611 |
| `is_in_parade_mode` (IsInParadeMode) | - | in the Parade pose and [is_in_parade_pose]. | INTERNAL-ONLY | crates/script/src/damage.rs:631 |
| `can_parade` (CanParade) | - | can the victim block this attacker's melee hit?  [can_parade_move_of], parade mode, the victim not in a HackAttack itself, with AIMode the attacker no... | WIRED-NOT-EXERCISED ? | crates/script/src/damage.rs:637 |
| `has_staff_magic_parade` (HasStaffMagicParade) | - | a staff in the right hand and (the player) the Perk_StaffMagic skill or (an NPC) the Mage class. | INTERNAL-ONLY | crates/script/src/damage.rs:664 |
| `can_parade_magic` (CanParadeMagic) | - | staff magic parry, parade mode, attacker in view. | WIRED-NOT-EXERCISED ? | crates/script/src/damage.rs:678 |
| `can_parade_missile` (CanParadeMissile) | - | sword and shield in hand, parade mode, attacker in view. | WIRED-NOT-EXERCISED ? | crates/script/src/damage.rs:686 |
| `assess_hit` (AssessHit) | S:2b580 | turn a hit into damage and a reaction.  Order: bookkeeping (last/current attacker, Sprint becomes Power); the player's non-lethal impact projectile an... | WIRED-NOT-EXERCISED ? | crates/script/src/damage.rs:712 |
| `pipi_stumble` (PipiStumble) | S:22f70 | the minimal hit reaction. A victim outside combat stops what it does and forgets its move; the effect/animation part is host work. | INTERNAL-ONLY | crates/script/src/damage.rs:1016 |
| `do_logical_damage` (DoLogicalDamage) | S:b4f50 | the gate in front of [assess_hit].  Returns None when the hit is ignored. Gate order: weakness (immune), the invulnerability cheat for the player, sta... | DEAD | crates/script/src/damage.rs:1026 |
| `give_xp` (GiveXP) | S:4e160 | experience for receiver.  amount == -1 means "a kill or defeat of source": ambient creatures give 10, Pig and Cow 20, everything else max(10 * level, ... | WIRED-NOT-EXERCISED ? | crates/script/src/damage.rs:1141 |

### ZS_* routine tasks (35)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `guard_attack_reason` (GetGuardAttackReason) | S:3d050 | why a guard attacks somebody whose crime it knows: a murderer is fought to the death (reason Revolution, 19), a thief with reason Theft, anybody else ... | INTERNAL-ONLY | crates/script/src/routine/dialog.rs:14 |
| `excuse` (_AI_Excuse) | S:3d130 | the player excuses a crime to a guard.  Needs Perk_Excuse. Crimes up to Attack always work; for theft and murder the guard rolls Random(130): above 11... | WIRED-NOT-EXERCISED ? | crates/script/src/routine/dialog.rs:38 |
| `bribe` (_AI_Bribe) | S:3d7d0 | the player offers a guard money to forget a crime.  Accepted (the dialog took the gold): the guard says LetsForgetThis, ThisTimeYoureLucky and NoMoreT... | WIRED-NOT-EXERCISED ? | crates/script/src/routine/dialog.rs:102 |
| `pick_pocket` (PickPocket) | S:3dd70 | one attempt. A caught thief is attacked (reason Theft); a successful one counts toward the victim enclave's thefts unless ExtendedContent is on and th... | DEAD | crates/script/src/routine/dialog.rs:183 |
| `follow_distance` (FUN_10012d40) | S:12d40 | the follow distance of npc in the player's party.  Companions that are waiting or already in ZS_WaitForPlayer do not count; the remaining members are ... | WIRED-NOT-EXERCISED ? | crates/script/src/routine/follow.rs:28 |
| `on_follow_player` (OnFollowPlayer) | S:12f50 | the per-tick callback of ZS_FollowPlayer.  The companion stops walking once it is within its follow distance. If the player swims or dives it switches... | DEAD | crates/script/src/routine/follow.rs:185 |
| `on_follow_npc` (OnFollowNPC) | S:13790 | the per-tick callback of ZS_FollowNPC: stop walking once within the follow distance of the leader. | DEAD | crates/script/src/routine/follow.rs:228 |
| `on_guide_player` (OnGuidePlayer) | S:13d60 | the per-tick callback of ZS_GuidePlayer: when the player lags more than 10 m behind (out of sight, or farther from the destination than the guide), st... | DEAD | crates/script/src/routine/follow.rs:238 |
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
| `can_heal_self` (CanHealSelf) | S:a66b0 | the NPC leading the player can always heal; Mages and Paladins heal by spell; everyone else needs an It_Potion_Health. | INTERNAL-ONLY | crates/script/src/routine/select.rs:353 |
| `have_a_break` (HaveABreak) | S:8c390 | used at the end of work states. An NPC whose current point belongs to an anchor goes straight back to ContinueRoutine; an NPC working a free-standing ... | INTERNAL-ONLY | crates/script/src/routine/select.rs:418 |
| `ambient_action` (GetAmbientAction) | S:884c0 | the idle action of a grazing animal.  Scavenger, Bison, Rhino and Pig eat the ground with 80 percent, Boar, Deer and Cow with 50 percent; every other ... | INTERNAL-ONLY | crates/script/src/routine/select.rs:432 |
| `send_to_arena` (SendToArena) | S:a7f80 | stop what the NPC does and, if it is within processing range, restart its routine (the arena task takes over through SelectRoutineTask).  The DLL refu... | DEAD | crates/script/src/routine/select.rs:450 |
| `rtn_monster` (Rtn_Monster) | S:86a10 | entry for monsters: just resume the routine. | DEAD | crates/script/src/routine/select.rs:467 |
| `rtn_dead` (Rtn_Dead) | S:86a80 | entry for dead NPCs: AI mode Dead and ZS_Dead. | DEAD | crates/script/src/routine/select.rs:473 |
| `rtn_player` (Rtn_Player) | S:86ba0 | entry for the player entity. Resets AI mode and Ransacked, then picks PS_Ghost (status bit 0x80), else by weapon mode PS_Melee, PS_Ranged, PS_Magic or... | DEAD | crates/script/src/routine/select.rs:480 |
| `travelling_walk_mode` (GetTravellingWalkMode) | S:a9c00 | 2 (fast) for Minecrawler, Scavenger, Wolf, Bison, Rhino and Snapper, 1 otherwise. | DEAD | crates/script/src/routine/time.rs:82 |
| `carry_stones` (ZS_CarryStones) | S:8e8c0 | stones are carried from a take point to a drop point; a drop-point trip holds the stone, a pick-up trip walks empty-handed. Ends with ContinueRoutine. | INTERNAL-ONLY | crates/script/src/routine/work.rs:1441 |
| `listen_loop` (ZS_Listen_Loop) | S:97ec0 | with 50 percent chance listen for a medium time; the state restarts. | INTERNAL-ONLY | crates/script/src/routine/work.rs:1488 |
| `sleep_loop` (ZS_Sleep_Loop) | S:9f500 | poll once a minute; stay asleep while the player talks to the NPC, otherwise wake when the routine time is no longer "sleep" (saying the "Awake" voice... | INTERNAL-ONLY | crates/script/src/routine/work.rs:1549 |
| `run_to_attack_point` (ZS_RunToAttackPoint) | S:9c910 | draw the weapon, run to the assigned point, then mill about (ZS_HangAround). Without a point the NPC mills about immediately. | INTERNAL-ONLY | crates/script/src/routine/work.rs:1627 |

### PS_Magic* (13)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `ps_magic_summon_abort` (PS_Magic_Summon_Abort) | S:7c2f0 | the hero lets go of a summon spell in the middle of the cast; the mana paid comes back (cast phase 3). | INTERNAL-ONLY | crates/script/src/player/magic.rs:150 |
| `ps_magic_flame_sword` (PS_Magic_FlameSword) | S:7b780 | load the spell, conjure the flame sword into the hand (AssureItems with flag 8, hands: nothing right, the sword left), use the scroll up and let the r... | INTERNAL-ONLY | crates/script/src/player/magic.rs:160 |
| `ps_magic_staff_parade` (PS_Magic_StaffParade) | S:7c630, S:7c8c0 | and PS_Magic_StaffParade_Loop (S:7c8c0): with the staff the mage parries (_AI_Parade); the loop leaves it when a cast key comes. | INTERNAL-ONLY | crates/script/src/player/magic.rs:260 |
| `ps_magic_potion` (PS_MagicPotion) | S:7d870 | the hero stops, puts his hands down, drinks (the spell potions of the animal forms and the like) and the potion's spell is cast as a quick cast. | INTERNAL-ONLY | crates/script/src/player/magic.rs:283 |
| `bs_book` (BS_BookOfFlood) | S:65180, S:656f0 | BS_BookOfRhobar (S:656f0) and BS_BookOfZuben: after half a second the hero reads two pages aloud (BookOf<Name>_<page>, the page counter advances by tw... | INTERNAL-ONLY | crates/script/src/player/magic.rs:391 |
| `bs_ancient_wisdom` (BS_AncientWisdom) | S:65080 | the bookstand of the ancient wisdom picks the book the hero has not finished: Flood, then Rhobar, then Zuben, and when all are read BS_NothingNew. | INTERNAL-ONLY | crates/script/src/player/magic.rs:437 |
| `bs_nothing_new` (BS_NothingNew) | S:64da0 | half a second, then one of three remarks (NothingNew_00..02), and on with the routine. | INTERNAL-ONLY | crates/script/src/player/magic.rs:449 |
| `bs_skill_book` (BS_Alchemy) | S:66a00 | and BS_Smith: after half a second a remark (AlchemyBook, SmithBook) and +3 Alchemy or Smithing (+5 without ExtendedContent); the stand is used up. | INTERNAL-ONLY | crates/script/src/player/magic.rs:471 |
| `bs_chronicles` (BS_Chronicles) | S:66190 | the Dargoth chronicles are read once; the game event Dargoth_Chronicles marks it. The five remarks and the quest hooks of the DLL are story data [U]; ... | INTERNAL-ONLY | crates/script/src/player/magic.rs:500 |
| `ps_ghost` (PS_Ghost) | S:673f0 | the soul has left the body. The hero changes action, puts his hands down, loses his focus and stands up, then waits in PS_Ghost_Loop. | INTERNAL-ONLY | crates/script/src/player/magic.rs:522 |
| `ps_ghost_loop` (PS_Ghost_Loop) | S:67900 | the ghost looks at whatever is in focus; a jump key sends him into PS_Ghost_Jump. | INTERNAL-ONLY | crates/script/src/player/magic.rs:554 |
| `ps_ghost_jump` (PS_Ghost_Jump) | S:67a70 | one jump, then back to the loop. | INTERNAL-ONLY | crates/script/src/player/magic.rs:564 |
| `ps_ghost_summoner` (PS_GhostSummoner) | S:67c80 | and PS_GhostSummoner_Loop: the body the soul left behind is abandoned: hands down, standing, idle. | INTERNAL-ONLY | crates/script/src/player/magic.rs:586 |

### ui (11)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `stop` (gCGUIManager) | Game:0x1007c520 | gCGUIManager::StopSay (Game:0x1007c520): fade out. | WIRED-NOT-EXERCISED ? | crates/ui/src/dialogmenu.rs:147 |
| `calc_uv` (gCMap_PS) | Game:0x101a42e0 | normalised picture coordinates of a world position, None for a degenerate sheet. | INTERNAL-ONLY | crates/ui/src/map.rs:25 |
| `contains` (gCMap_PS) | Game:0x101a4240 | gCMap_PS::Contains (Game:0x101a4240). | WIRED-NOT-EXERCISED ? | crates/ui/src/map.rs:39 |
| `around` (the rectangle step of gCMap_PS) | Game:0x101a43d0 | the square around the bounding box of points (x, z), side max(dx, dz) * (scale + margin), centred on the box. scale is the gSMapGenerate scale; the en... | WIRED-NOT-EXERCISED ? | crates/ui/src/map.rs:48 |
| `view_window` (the window computation of gCHUDStaticMap's paint) | Game:0x100a87d0 | the map control shows: view_px is the control's client size, tex_px the picture's size, zoom 1 = pixel for pixel when the picture is larger than the c... | INTERNAL-ONLY | crates/ui/src/map.rs:110 |
| `item_rows` (gCHUDTableItemStats) | Game:0x100a3bb0 | row building: the table of info. With against (the worn item) each damage / bonus / value row carries the difference. in_inventory adds the "in invent... | INTERNAL-ONLY | crates/ui/src/stats.rs:104 |
| `print` (gCScreenOutputCtrl) | Game:0x10077630 | replace the text, show the window and start the timer. | WIRED-NOT-EXERCISED ? | crates/ui/src/texteffect.rs:193 |
| `print_end` (gCScreenOutputCtrl) | Game:0x10077610 | gCScreenOutputCtrl::PrintScreenEnd (Game:0x10077610). | INTERNAL-ONLY | crates/ui/src/texteffect.rs:208 |
| `quest_success` (gCScreenOutputCtrl) | Game:0x10077c20 | the quest title in white. | DEAD | crates/ui/src/texteffect.rs:214 |
| `quest_failed` (gCScreenOutputCtrl) | Game:0x10077cc0 | gCScreenOutputCtrl::PrintScreen_Quest_Failed (Game:0x10077cc0). | DEAD | crates/ui/src/texteffect.rs:221 |
| `info_say` (gCScreenOutputCtrl) | Game:0x10077880 | the spoken line, white for the player and yellow for everybody else. | DEAD | crates/ui/src/texteffect.rs:227 |

### perception / crime (10)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `get_player_crime` (GetPlayerCrime) | S:36fd0 | the crime the NPC remembers. | DEAD | crates/script/src/crime.rs:33 |
| `memorize_enclave_crime` (MemorizeEnclaveCrime) | S:60670 | report a crime to the witness' enclave.  Only Theft and Murder reach the enclave, never below what it already knows, theft only through a boss or guar... | INTERNAL-ONLY | crates/script/src/crime.rs:89 |
| `change_enclave_status` (ChangeEnclaveStatus) | S:602d0 | switch an enclave between Routine, Attack and Flee.  Returns false when nothing changes. Any change away from **Attack** is a pardon: the player's fam... | INTERNAL-ONLY | crates/script/src/crime.rs:158 |
| `is_audible` (FUN_10009100) | - | "is audible": the entity is neither sneaking (Sneak or Parade pose) nor standing still. | INTERNAL-ONLY | crates/script/src/perception.rs:43 |
| `assess_take_item` (AssessTakeItem) | S:30620 | witness saw taker pick up item.  A witness that was already after the taker stops and resumes its routine. If it is not fleeing, the item was not drop... | INTERNAL-ONLY | crates/script/src/perception.rs:222 |
| `send_perception_take_item` (SendPerceptionTakeItem) | S:32c00 | offer the theft to every NPC that can perceive the taker. | DEAD | crates/script/src/perception.rs:252 |
| `call_help` (CallHelp) | S:332e0 | caller was wronged by culprit; guards that are friendly toward the caller, not friendly toward the culprit and within 10 m of the culprit attack it wi... | WIRED-NOT-EXERCISED ? | crates/script/src/perception.rs:263 |
| `on_sleep` (OnSleep) | S:86dc0 | the callback of a sleeping NPC: wake into ZS_ReactToFootsteps when the player, whom the sleeper does not like, is audible, in clear view, within 10 m ... | DEAD | crates/script/src/perception.rs:357 |
| `on_animal_sleep` (OnAnimalSleep) | S:89f20 | the callback of a sleeping animal: the player wakes it by coming close unless hidden. With Perk_Sneak_2 a sneaking player (or one in PS_Ranged) is nev... | DEAD | crates/script/src/perception.rs:384 |
| `on_gate_guard` (OnGateGuard) | S:86c80 | the local callback of a gate guard: returns true once per [GATE_GUARD_POLL_SECS] of state time, when the caller should run the active perception scan. | DEAD | crates/script/src/perception.rs:415 |

### down / knock-out (9)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `unconscious_secs` (FUN_10007790) | - | seconds a defeated NPC stays down.  5 s for the player and the player's party, 20 s after an arena defeat, 30 s otherwise. | WIRED-NOT-EXERCISED ? | crates/script/src/down.rs:35 |
| `reset_all` (ResetAll + ResetInteraction) | S:247f0, S:24c20 | drop every effect and interaction of the character. The engine part (animation, collision group, look-at) is the host's reset_character; the script-vi... | INTERNAL-ONLY | crates/script/src/down.rs:50 |
| `drop_hand_items` (DropHandItems) | S:24d10 | droppable weapons in the hands fall to the ground, the hands are emptied. | INTERNAL-ONLY | crates/script/src/down.rs:59 |
| `remove_non_combat_items` (RemoveNonCombatItems) | S:24ef0 | put away whatever is in the hands unless it is a weapon pose (tool-idle, melee, ranged or magic ready). | INTERNAL-ONLY | crates/script/src/down.rs:71 |
| `kill` (Kill) | S:1bc30 | victim dies, killer is the character that did it.  Stamps the last fight, resets effects and interactions, drops hand items, sets AI mode Dead, dissol... | WIRED-NOT-EXERCISED ? | crates/script/src/down.rs:120 |
| `clean_up_plunder` (CleanUpPlunderInv) | S:1c380 | a dead Demon keeps nothing; everything but a summoned creature gets its plunder regenerated from the treasure sets. | INTERNAL-ONLY | crates/script/src/down.rs:164 |
| `defeat` (Defeat) | S:27510 | victim is knocked out by attacker.  Duel, arena, drive-away or reasonless fights end the victim's poisoning. Same cleanup as [kill] but AI mode Down. ... | WIRED-NOT-EXERCISED ? | crates/script/src/down.rs:176 |
| `fall_damage` (the fall-damage rule of OnMovementModeChanged) | S:adbf0 | floor(160 * (t - 0.9)^2) for a fall of t seconds, 0 up to 0.9 s. | WIRED-NOT-EXERCISED ? | crates/script/src/down.rs:318 |
| `land_from_fall` (the landing rule of OnMovementModeChanged) | S:adbf0 | apply fall damage after t seconds in the ZS_Fall state.  Nothing happens for ghosts (status 0x80), the god-mode player and Bloodflies. Perk_Acrobatic ... | DEAD | crates/script/src/down.rs:329 |

### PS_Interact* (7)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `is_basket` (IsBasket) | S:782e0 | the hero just opened a chest (action OpenChest) and the object is one of the harvestable "ChestPod" baskets, not a real chest. | INTERNAL-ONLY | crates/script/src/player/interact.rs:562 |
| `ps_interact_wait_for_exit` (PS_Interact_WaitForExit) | - | PS_Interact_WaitForExit: wait for the exit key, then stop and leave the interaction. | INTERNAL-ONLY | crates/script/src/player/interact.rs:571 |
| `ps_interact_door` (PS_Interact_Door) | S:70970 | a locked door is unlocked first; then the door swings open or shut, the witnesses are told, the use event is raised and the interaction ends. | INTERNAL-ONLY | crates/script/src/player/interact.rs:672 |
| `player_sleep` (PlayerSleep) | S:76440 | advance the world by hours (playing time follows) and refresh hit points, stamina and mana to their maximum. | INTERNAL-ONLY | crates/script/src/player/interact.rs:711 |
| `ps_interact_sleep_bed` (PS_Interact_SleepBed) | S:75d40 | a torch in hand is put away, the hero lies down and the night starts (PS_DuskToDawn). | INTERNAL-ONLY | crates/script/src/player/interact.rs:733 |
| `ps_dusk_to_dawn` (PS_DuskToDawn + PS_DuskToDawn_Loop) | S:66fc0, S:671d0 | remember the hour, let the clock run until it is morning, then restore the hero (PlayerSleep) and get up. | INTERNAL-ONLY | crates/script/src/player/interact.rs:763 |
| `ps_interact_talk` (PS_Interact_Shrine) | S:72600, S:78210 | and PS_Interact_Talk (S:78210): kneel (shrine) and start the conversation with the object. When nobody answers the interaction ends. | INTERNAL-ONLY | crates/script/src/player/interact.rs:789 |

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

### PS_Interact boulder (4)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `can_handle` (OB_Light) | S:777b0, S:778e0 | OB_Medium (S:778e0), OB_Heavy (S:77a10) and OB_Faring_Boulder (S:77b40): the strength check of the stage the hero asks for. A weaker hero says TooHeav... | INTERNAL-ONLY | crates/script/src/player/boulder.rs:75 |
| `ps_boulder` (PS_Interact_TiltOrcBoulder) | S:765b0, S:76c70 | PS_Interact_HoldOrcBoulder (S:76c70) and PS_Interact_LiftOrcBoulder (S:77210): hands empty (tilting), the strength check of the stage (failure: back t... | INTERNAL-ONLY | crates/script/src/player/boulder.rs:110 |
| `ps_boulder_loop` (the three boulder loops) | S:76950 | hold the pose; the use key asks for the next stage, the exit key ends the interaction. A lift pays the first-time Strength bonus. | INTERNAL-ONLY | crates/script/src/player/boulder.rs:145 |
| `ps_carry_stone` (PS_Interact_CarryStone) | S:77cc0 | take the stone in both hands and walk (movement mode 3); PS_Interact_CarryStone_Loop waits for the exit key and PS_Interact_CarryStone_End puts it dow... | INTERNAL-ONLY | crates/script/src/player/boulder.rs:178 |

### input / player control (3)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `ranged_walk_loop` (the message switch of PS_Ranged_Walk_Loop) | S:84080 | the hero walks with the weapon ready. crossbow: the left hand holds a crossbow. | DEAD | crates/input/src/player.rs:472 |
| `ranged_aim_loop` (the message switch of PS_Ranged_Aim_Loop) | S:84d60 | the weapon is up. | DEAD | crates/input/src/player.rs:492 |
| `power_aim_loop` (the message switch of OnPowerAim_Loop) | S:84b90 | the callback of the drawn bow: the abort message gives up the draw, the shoot message looses the arrow. | DEAD | crates/input/src/player.rs:504 |

### PS_Ranged* (bow, crossbow) (3)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `ps_ranged_aim_loop` (PS_Ranged_Aim_Loop) | S:84d60 | hold the aim and wait for shoot or reload. | INTERNAL-ONLY | crates/script/src/player/ranged.rs:161 |
| `ps_ranged_reload` (PS_Ranged_Reload) | S:85550, S:859a0 | and PS_Ranged_Aim_Reload (S:85550): cock the crossbow (Cock overlay) or pull a fresh arrow, hold the ammunition again and aim (crossbow) or walk (bow)... | INTERNAL-ONLY | crates/script/src/player/ranged.rs:224 |
| `ps_ranged_crossbow_sneak` (PS_Ranged_CrossBowSneak_Loop) | S:86320 | sneaking with a crossbow is the walk loop. | INTERNAL-ONLY | crates/script/src/player/ranged.rs:276 |

### fx / particles (2)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `scroller` (eCTexCoordSrcScroller) | Engine:0x102628f0 | the offset is the fractional part of direction * rate * time (wrapping keeps float precision over long sessions), added to the authored ScrollOffset. | INTERNAL-ONLY | crates/fx/src/matadmin.rs:47 |
| `trigger` (eCParticle_PS) | Engine:0x1014ef20 | queue count extra particles. | WIRED-NOT-EXERCISED ? | crates/fx/src/sim.rs:187 |

### PS_Interact craft (2)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `fry_stack` (OnPlayerFryStack) | S:6a740 | fry amount pieces of raw meat. Items that cannot be fried are left alone; returns the number fried. | INTERNAL-ONLY | crates/script/src/player/craft.rs:84 |
| `transform_stack` (OnPlayerTransformStack) | S:6a8d0 | a transform request on the barbecue is frying (the only transformation the hero's HUD offers); the stacks are rearranged afterwards by the host. | DEAD | crates/script/src/player/craft.rs:104 |

### PS_Evade (2)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `evade_entry` (ZS_EvadeLeft) | S:401d0, S:405b0 | ZS_EvadeRight (S:405b0), ZS_EvadeForward (S:40970) and ZS_EvadeBackward (S:40d10): pay the stamina, play the dodge and hand over to ZS_EvadeLoop. A do... | INTERNAL-ONLY | crates/script/src/player/evade.rs:92 |
| `evade_loop` (ZS_EvadeLoop) | S:3fb00 | the motion runs; once it is over the character walks again and returns to PS_Melee_Loop (the hero) or to its previous task (everybody else). | INTERNAL-ONLY | crates/script/src/player/evade.rs:127 |

### PS_Melee* (2)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `ps_melee_parade` (PS_Melee_Parade) | S:7f960 | raise the guard against the opponent. | INTERNAL-ONLY | crates/script/src/player/melee.rs:103 |
| `ps_melee_jump_back` (PS_Melee_JumpBack) | S:7fc20 | hop away from the opponent (stamina per the B09 table). | INTERNAL-ONLY | crates/script/src/player/melee.rs:120 |

### PS_Normal / Sneak / Swim (2)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `ps_normal_take` (PS_Normal_Take) | S:82240 | stop, take the item (_AI_TakeItem) and count a theft when it did not lie there as a dropped item (AddEnclaveTheftCount: only secured zones and enclave... | INTERNAL-ONLY | crates/script/src/player/normal.rs:113 |
| `ps_gonna_loot` (PS_GonnaLoot) | S:83120 | stop and open the loot window of the focused body. | INTERNAL-ONLY | crates/script/src/player/normal.rs:142 |

### nav (1)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `choose` (gCNavigationMap) | - | gCNavigationMap::DetectFinalTargetAsOffset: among the poses that lie inside the zone the character is in, take the one nearest to the character. | WIRED-NOT-EXERCISED ? | crates/nav/src/agent.rs:198 |

### projectiles (1)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `npc_arrow` (the NPC branch of Shoot) | - | the NPC branch of Shoot: a straight flight along the archer's aim with the focus as target, at the template's ShootVelocity. | WIRED-NOT-EXERCISED | crates/projectile/src/launch.rs:52 |

### sim (stats, combat, AI, host) (1)

| function | original (Dll:addr) | what the original does | wiring | file |
| --- | --- | --- | --- | --- |
| `apply_stat` (the player memory entry points) | Game:0x14efa0 | apply an item modifier event to the hero. Characters that use the NPC formulas have no attribute store and ignore it. | INTERNAL-ONLY | crates/sim/src/actor.rs:417 |

## Ported functions called in play (calls over all scenarios)

| function | calls | scenarios | file |
| --- | ---: | --- | --- |
| `color_factor` | 48416920 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/fx/src/sim.rs:54 |
| `calc_color` | 48416920 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/fx/src/sim.rs:489 |
| `tick_particle` | 48307249 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/fx/src/sim.rs:308 |
| `color_scale` | 46535263 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/fx/src/scale.rs:80 |
| `blend_factor` | 37639430 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/environment/src/lib.rs:241 |
| `vector_scale` | 27842465 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/fx/src/scale.rs:62 |
| `calc_frame` | 13642227 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/fx/src/sim.rs:96 |
| `update` | 2594717 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/fx/src/sim.rs:202 |
| `process` | 2430516 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/fx/src/effect.rs:33 |
| `has_finished` | 2430516 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/fx/src/effect.rs:49 |
| `float_scale` | 1881657 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/fx/src/scale.rs:52 |
| `spawn` | 1501566 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/fx/src/sim.rs:378 |
| `process` | 1286905 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/fx/src/sim.rs:254 |
| `is_enabled` | 851242 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/world/src/sector.rs:145 |
| `routine_time` | 810455 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/script/src/routine/time.rs:44 |
| `is_playing` | 680808 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/animation/src/blend.rs:310 |
| `run_stop_prevention` | 673035 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/animation/src/blend.rs:393 |
| `boost` | 549449 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/physics/src/relax.rs:8 |
| `reset` | 485479 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/fx/src/sim.rs:167 |
| `at` | 473696 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/formats/src/weather.rs:35 |
| `at@2` | 473696 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/formats/src/weather.rs:48 |
| `class_priority` | 259484 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/gameplay/src/focus.rs:161 |
| `is_valid` | 259484 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/gameplay/src/focus.rs:173 |
| `is_linked` | 227837 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/gameplay/src/inventory/stack.rs:152 |
| `get_attitude` | 168910 | combat_basic, controls_basic, dialog_quest, sector_switch, shaman_duel | crates/script/src/attitude.rs:171 |
| `outlaw_attitude` | 163396 | combat_basic, controls_basic, dialog_quest, sector_switch, shaman_duel | crates/script/src/attitude.rs:17 |
| `political_attitude` | 162689 | combat_basic, controls_basic, sector_switch, shaman_duel, souls_er, sw | crates/script/src/predicates.rs:489 |
| `in_range2` | 147747 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/gameplay/src/focus.rs:180 |
| `focus_check` | 147747 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/gameplay/src/focus.rs:295 |
| `angles` | 135356 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/gameplay/src/focus.rs:246 |
| `page_flags` | 134838 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/ui/src/manager.rs:89 |
| `in_angles` | 130650 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/gameplay/src/focus.rs:188 |
| `priority` | 121474 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, hud | crates/gameplay/src/focus.rs:196 |
| `clip_fog` | 118288 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/environment/src/lib.rs:392 |
| `calculate_rect_indices` | 100079 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/world/src/pvs.rs:96 |
| `process` | 100079 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/world/src/pvs.rs:257 |
| `start_due_events` | 63914 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/audio/src/sequencer.rs:642 |
| `environment` | 63879 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/audio/src/ambient.rs:134 |
| `dominant_environment` | 63879 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/audio/src/ambient.rs:168 |
| `process` | 63879 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/audio/src/ambient.rs:221 |
| `water_beds` | 63879 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/audio/src/ambient_play.rs:168 |
| `update_envelope` | 63879 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/audio/src/sequencer.rs:818 |
| `update_channels` | 63879 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/audio/src/sequencer.rs:862 |
| `pick_point` | 63879 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/camera/src/aim.rs:32 |
| `bone` | 63879 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/gameplay/src/inventory/slot.rs:172 |
| `draw_multiplier` | 63879 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/projectile/src/aim.rs:31 |
| `tick` | 63879 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/ui/src/texteffect.rs:239 |
| `cull` | 60753 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/world/src/admin.rs:183 |
| `can_do` | 60127 | combat_basic, sector_switch, shaman_duel, walk_to_reddock | crates/script/src/combat/moves.rs:232 |
| `score_move` | 59820 | combat_basic, sector_switch, shaman_duel, walk_to_reddock | crates/script/src/combat/moves.rs:436 |
| `offer` | 59820 | combat_basic, sector_switch, shaman_duel, walk_to_reddock | crates/script/src/combat/moves.rs:653 |
| `from_state` | 59212 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/environment/src/lib.rs:51 |
| `apply` | 59212 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/environment/src/lib.rs:175 |
| `sun_state` | 59144 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/environment/src/lib.rs:323 |
| `hemisphere_sh` | 59144 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/environment/src/lib.rs:403 |
| `max_range` | 57150 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/gameplay/src/focus.rs:132 |
| `find_focus` | 57150 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/gameplay/src/focus.rs:324 |
| `look_counts` | 52793 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/input/src/control.rs:113 |
| `update_child_dependencies` | 40929 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/world/src/entity.rs:389 |
| `update_processing_range` | 36224 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/runtime/src/core.rs:667 |
| `process` | 36224 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/world/src/admin.rs:300 |
| `flush_kills` | 36224 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/world/src/admin.rs:455 |
| `recalibrate` | 32665 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/world/src/pvs.rs:379 |
| `route` | 32455 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, hud | crates/input/src/player.rs:207 |
| `magic_input` | 32455 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, hud | crates/input/src/player.rs:385 |
| `weight_sphere` | 32256 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/render-rules/src/lib.rs:165 |
| `species_attitude` | 20465 | combat_basic, controls_basic, sector_switch, shaman_duel, souls_er, sw | crates/script/src/attitude.rs:33 |
| `process` | 18979 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/world/src/trigger.rs:932 |
| `has_lower_priority` | 17952 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, hud | crates/gameplay/src/focus.rs:272 |
| `species_can_sidestep` | 15952 | combat_basic, sector_switch, shaman_duel, walk_to_reddock | crates/script/src/combat/moves.rs:190 |
| `is_in_fist_mode` | 13573 | combat_basic, sector_switch, shaman_duel, swim, walk_to_reddock | crates/script/src/predicates.rs:282 |
| `attitude_to_player` | 10361 | combat_basic, controls_basic, dialog_quest, sector_switch, souls_er, t | crates/script/src/attitude.rs:108 |
| `is_boss` | 9750 | combat_basic, controls_basic, sector_switch, souls_er, teach, teleport | crates/script/src/predicates.rs:328 |
| `is_guard` | 9750 | combat_basic, controls_basic, sector_switch, souls_er, teach, teleport | crates/script/src/predicates.rs:334 |
| `play_slot` | 9390 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/animation/src/animator.rs:134 |
| `new` | 9390 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/animation/src/blend.rs:272 |
| `hand_class` | 8384 | bow_pose, combat_basic, craft_anvil, lockpick, loot_chest, magic_bow,  | crates/script/src/helpers.rs:44 |
| `weapon_mode` | 8384 | bow_pose, combat_basic, craft_anvil, lockpick, loot_chest, magic_bow,  | crates/script/src/helpers.rs:103 |
| `new` | 8088 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/gameplay/src/inventory/stack.rs:82 |
| `combat_move_length` | 7825 | combat_basic, sector_switch, shaman_duel, swim, walk_to_reddock | crates/script/src/combat/range.rs:7 |
| `attack_range` | 7825 | combat_basic, sector_switch, shaman_duel, swim, walk_to_reddock | crates/script/src/combat/range.rs:76 |
| `switch_fade_out` | 7773 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/animation/src/animator.rs:161 |
| `switch_fade_out` | 7773 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/animation/src/blend.rs:353 |
| `add_items` | 7218 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/gameplay/src/inventory/list.rs:436 |
| `select_combat_move` | 5777 | combat_basic, sector_switch, shaman_duel, swim, walk_to_reddock | crates/script/src/combat/select.rs:27 |
| `fai_active` | 5777 | combat_basic, sector_switch, shaman_duel, swim, walk_to_reddock | crates/script/src/combat/select.rs:67 |
| `on_attack` | 5318 | combat_basic, sector_switch, shaman_duel, swim, walk_to_reddock | crates/script/src/combat/attack.rs:44 |
| `find_other_target` | 5318 | combat_basic, sector_switch, shaman_duel, swim, walk_to_reddock | crates/script/src/combat/select.rs:277 |
| `build_situation` | 5102 | combat_basic, sector_switch, shaman_duel, swim, walk_to_reddock | crates/script/src/combat/moves.rs:86 |
| `crowd_on_target` | 5102 | combat_basic, sector_switch, shaman_duel, swim, walk_to_reddock | crates/script/src/combat/moves.rs:130 |
| `aggression_weight` | 5102 | combat_basic, sector_switch, shaman_duel, swim, walk_to_reddock | crates/script/src/combat/moves.rs:147 |
| `list_count` | 4735 | trade_click, trade_esc | crates/gameplay/src/trade.rs:23 |
| `evaluate` | 4377 | controls_basic, dialog_quest, lockpick, sector_switch, teach, teleport | crates/animation/src/face.rs:201 |
| `can_parade_move_of` | 3988 | combat_basic, sector_switch, shaman_duel, walk_to_reddock | crates/script/src/damage.rs:587 |
| `pick` | 3988 | combat_basic, sector_switch, shaman_duel, walk_to_reddock | crates/script/src/combat/moves.rs:697 |
| `choose_full` | 3988 | combat_basic, sector_switch, shaman_duel, walk_to_reddock | crates/script/src/combat/moves.rs:773 |
| `crosshair_size` | 3926 | bow_pose, magic_bow, shaman_duel | crates/projectile/src/aim.rs:37 |
| `on_attack_goto` | 3896 | combat_basic, sector_switch, shaman_duel, walk_to_reddock | crates/script/src/combat/attack.rs:525 |
| `follow_time` | 3892 | combat_basic, sector_switch, shaman_duel, walk_to_reddock | crates/script/src/predicates.rs:182 |
| `update` | 3855 | bow_pose, magic_bow, shaman_duel | crates/projectile/src/aim.rs:154 |
| `can_receive_perception_from` | 3653 | lockpick, loot_chest, sector_switch, walk_to_reddock | crates/script/src/perception.rs:50 |
| `set_attack_reason` | 3515 | sector_switch, walk_to_reddock | crates/script/src/crime.rs:39 |
| `memorize_player_crime` | 3515 | sector_switch, walk_to_reddock | crates/script/src/crime.rs:63 |
| `can_attack` | 3515 | sector_switch, walk_to_reddock | crates/script/src/perception.rs:83 |
| `assess_target` | 3515 | sector_switch, walk_to_reddock | crates/script/src/perception.rs:95 |
| `assess_attack` | 3515 | sector_switch, walk_to_reddock | crates/script/src/perception.rs:151 |
| `is_ambient_creature` | 3515 | sector_switch, walk_to_reddock | crates/script/src/predicates.rs:55 |
| `notify_enclave` | 3514 | sector_switch | crates/script/src/crime.rs:214 |
| `dst_volume` | 3374 | combat_basic, dialog_quest, shaman_duel, swim, walk_to_reddock, world_ | crates/audio/src/music.rs:73 |
| `src_volume` | 3365 | combat_basic, dialog_quest, shaman_duel, swim, walk_to_reddock, world_ | crates/audio/src/music.rs:62 |
| `trade_unit_price` | 2710 | trade_click, trade_esc | crates/sim/src/trade.rs:46 |
| `on_attack_turn` | 2525 | combat_basic, sector_switch, shaman_duel, walk_to_reddock | crates/script/src/combat/attack.rs:648 |
| `wait_for_flee_loop` | 2366 | sector_switch, shaman_duel | crates/script/src/combat/attack.rs:676 |
| `max_angles` | 2353 | bow_pose, combat_basic, controls_basic, craft_anvil, loot_chest, magic | crates/gameplay/src/focus.rs:145 |
| `pool_percent` | 2233 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/script/src/stats.rs:218 |
| `read_grid` | 2226 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/formats/src/terrain/vegetation.rs:247 |
| `cache_in` | 1971 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/world/src/entity.rs:517 |
| `lookup` | 1769 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/fx/src/efm.rs:97 |
| `start` | 1756 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/fx/src/effect.rs:17 |
| `primary` | 1489 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, loc | crates/input/src/player.rs:285 |
| `can_see` | 1164 | combat_basic, lockpick, sector_switch, shaman_duel, walk_to_reddock | crates/script/src/perception.rs:29 |
| `ps_normal_loop` | 1067 | craft_anvil, lockpick, loot_chest, magic_bow, sit_pose, walk_to_reddoc | crates/script/src/player/normal.rs:101 |
| `can_move` | 1012 | trade_click, trade_esc | crates/gameplay/src/trade.rs:57 |
| `is_disposable` | 1012 | trade_click, trade_esc | crates/gameplay/src/inventory/stack.rs:189 |
| `draw_multiplier` | 928 | bow_pose, magic_bow, shaman_duel | crates/script/src/player/ranged.rs:24 |
| `ps_ranged_power_aim` | 928 | bow_pose, magic_bow, shaman_duel | crates/script/src/player/ranged.rs:172 |
| `has_dangerous_weapon` | 901 | combat_basic, souls_er, teleport, walk_to_reddock | crates/script/src/predicates.rs:304 |
| `on_react_to_weapon` | 901 | combat_basic, souls_er, teleport, walk_to_reddock | crates/script/src/combat/react.rs:185 |
| `on_attack_go_back` | 896 | combat_basic, sector_switch, shaman_duel, walk_to_reddock | crates/script/src/combat/attack.rs:617 |
| `hang_around` | 831 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/script/src/routine/work.rs:1827 |
| `find_sounds` | 721 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/audio/src/ambient.rs:144 |
| `select_sound` | 721 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/audio/src/ambient.rs:157 |
| `set_transaction_amount` | 569 | trade_click, trade_esc | crates/gameplay/src/inventory/list.rs:1302 |
| `set_transaction_amount` | 569 | trade_click, trade_esc | crates/gameplay/src/inventory/stack.rs:278 |
| `observe_loop` | 520 | souls_er, teleport | crates/script/src/combat/react.rs:742 |
| `task_for_use_type` | 460 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/script/src/routine/select.rs:68 |
| `select_routine_task` | 460 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/script/src/routine/select.rs:99 |
| `continue_routine` | 410 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/script/src/routine/select.rs:366 |
| `is_humanoid` | 397 | combat_basic, sector_switch, shaman_duel, walk_to_reddock | crates/script/src/predicates.rs:21 |
| `ps_melee_loop` | 241 | combat_basic, walk_to_reddock, world_walk | crates/script/src/player/melee.rs:68 |
| `ps_magic_loop` | 172 | bow_pose, magic_bow, teleport | crates/script/src/player/magic.rs:74 |
| `ps_ranged_walk` | 150 | bow_pose, magic_bow, shaman_duel | crates/script/src/player/ranged.rs:119 |
| `assess_use_interact_object` | 138 | lockpick, loot_chest | crates/script/src/assess.rs:336 |
| `assess_active_perception` | 118 | combat_basic, sector_switch, shaman_duel, walk_to_reddock | crates/script/src/assess.rs:463 |
| `is_upset` | 118 | combat_basic, sector_switch, shaman_duel, walk_to_reddock | crates/script/src/predicates.rs:393 |
| `on_ransack_body` | 118 | combat_basic, sector_switch, shaman_duel, walk_to_reddock | crates/script/src/combat/react.rs:927 |
| `assess_player_zone` | 116 | combat_basic, shaman_duel, walk_to_reddock | crates/script/src/assess.rs:399 |
| `assess_player_sneaking` | 116 | combat_basic, shaman_duel, walk_to_reddock | crates/script/src/perception.rs:290 |
| `assess_player_weapon` | 116 | combat_basic, shaman_duel, walk_to_reddock | crates/script/src/perception.rs:314 |
| `speed_cm_s` | 113 | bow_pose, magic_bow, shaman_duel | crates/projectile/src/params.rs:339 |
| `steer` | 99 | bow_pose, magic_bow, shaman_duel | crates/projectile/src/flight.rs:240 |
| `combat_walk_mode` | 93 | combat_basic, sector_switch, shaman_duel, walk_to_reddock | crates/script/src/combat/range.rs:104 |
| `create_items` | 75 | craft_anvil, menu_save_load, quick_slots, trade_click, trade_esc | crates/gameplay/src/inventory/list.rs:474 |
| `find_fragment` | 73 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/audio/src/sequencer.rs:440 |
| `select` | 73 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/audio/src/sequencer.rs:484 |
| `threaten_loop` | 68 | walk_to_reddock | crates/script/src/combat/react.rs:328 |
| `secondary` | 66 | combat_basic, walk_to_reddock | crates/input/src/player.rs:326 |
| `on_entity_processing_changed` | 61 | sector_switch | crates/world/src/admin.rs:249 |
| `caption` | 50 | menu_save_load | crates/ui/src/files.rs:94 |
| `ps_interact_chest_loop` | 48 | lockpick, loot_chest | crates/script/src/player/interact.rs:653 |
| `set_page` | 47 | controls_basic, craft_anvil, dialog_quest, lockpick, loot_chest, menu_ | crates/ui/src/manager.rs:199 |
| `is_gate_guard` | 45 | combat_basic, shaman_duel, souls_er, teleport | crates/script/src/predicates.rs:346 |
| `exit_time` | 44 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/audio/src/sequencer.rs:563 |
| `find_transition` | 44 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/audio/src/sequencer.rs:598 |
| `is_slave` | 40 | combat_basic, shaman_duel | crates/script/src/predicates.rs:352 |
| `is_temporary_friendly_to_player` | 40 | combat_basic, shaman_duel | crates/script/src/predicates.rs:384 |
| `ps_interact_loop` | 40 | craft_anvil, sit_pose | crates/script/src/player/interact.rs:540 |
| `execute_fragment` | 38 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/audio/src/sequencer.rs:673 |
| `trigger_follow` | 38 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/audio/src/sequencer.rs:771 |
| `intro` | 37 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/animation/src/blend.rs:116 |
| `extro` | 37 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/animation/src/blend.rs:125 |
| `trigger` | 35 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/audio/src/sequencer.rs:349 |
| `combined_quality` | 32 | trade_click, trade_esc | crates/gameplay/src/inventory/stack.rs:254 |
| `pool` | 30 | bow_pose, combat_basic, magic_bow, teleport, walk_to_reddock, world_wa | crates/script/src/stats.rs:171 |
| `move_for_press` | 30 | combat_basic, walk_to_reddock | crates/script/src/player/melee.rs:32 |
| `parse` | 29 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/fx/src/efm.rs:107 |
| `from_ini` | 29 | bow_pose, combat_basic, controls_basic, craft_anvil, dialog_quest, doo | crates/ui/src/log.rs:90 |
| `build_lipsync_clip` | 28 | controls_basic, dialog_quest, lockpick, sector_switch, teach, teleport | crates/animation/src/face.rs:92 |
| `basket_total` | 24 | trade_click, trade_esc | crates/gameplay/src/trade.rs:45 |
| `teach_offers` | 24 | controls_basic, dialog_quest, souls_er, teach, trade_click, trade_esc | crates/ui/src/journal.rs:129 |
| `find_stack_index` | 23 | trade_click, trade_esc | crates/gameplay/src/inventory/list.rs:546 |
| `start_cast_phase` | 23 | bow_pose, magic_bow, teleport | crates/script/src/magic/cast.rs:147 |
| `want_to_ransack` | 20 | combat_basic, sector_switch, shaman_duel, walk_to_reddock | crates/script/src/predicates.rs:423 |
| `capture` | 16 | craft_anvil, lockpick, loot_chest, menu_save_load, quick_slots, routin | crates/runtime/src/save.rs:279 |
| `snapshot` | 16 | craft_anvil, lockpick, loot_chest, menu_save_load, quick_slots, routin | crates/sim/src/persist.rs:178 |
| `print` | 16 | combat_basic, craft_anvil, dialog_quest, lockpick, loot_chest, menu_sa | crates/ui/src/dialogmenu.rs:209 |
| `set_pool` | 15 | combat_basic, walk_to_reddock, world_walk | crates/script/src/stats.rs:237 |
| `add_stamina_points` | 15 | combat_basic, walk_to_reddock, world_walk | crates/script/src/stats.rs:327 |
| `has_attack_reason_to_kill` | 14 | combat_basic, walk_to_reddock | crates/script/src/predicates.rs:242 |
| `ps_ranged_aim` | 14 | bow_pose, magic_bow, shaman_duel | crates/script/src/player/ranged.rs:131 |
| `ps_interact_end` | 12 | craft_anvil, lockpick, loot_chest, sit_pose | crates/script/src/player/interact.rs:583 |
| `ps_ranged_shoot` | 12 | bow_pose, magic_bow, shaman_duel | crates/script/src/player/ranged.rs:182 |
| `end_time` | 11 | bow_pose, magic_bow, sector_switch, shaman_duel | crates/fx/src/efm.rs:75 |
| `ps_normal` | 11 | craft_anvil, lockpick, loot_chest, magic_bow, sit_pose, walk_to_reddoc | crates/script/src/player/normal.rs:72 |
| `confirm` | 11 | controls_basic, craft_anvil, dialog_quest, lockpick, loot_chest, sit_p | crates/script/src/player/normal.rs:218 |
| `deal_enabled` | 10 | trade_click, trade_esc | crates/gameplay/src/trade.rs:198 |
| `generate_trade` | 10 | trade_click, trade_esc | crates/gameplay/src/treasure.rs:253 |
| `refresh_trade` | 10 | trade_click, trade_esc | crates/gameplay/src/treasure.rs:300 |
| `ps_jump` | 10 | walk_to_reddock, world_walk | crates/script/src/player/normal.rs:151 |
| `reset` | 10 | controls_basic, dialog_quest, souls_er, teach, trade_click, trade_esc | crates/ui/src/dialogmenu.rs:42 |
| `execute_transition` | 9 | combat_basic, dialog_quest, shaman_duel, swim, walk_to_reddock, world_ | crates/audio/src/sequencer.rs:732 |
| `ps_interact` | 9 | craft_anvil, lockpick, loot_chest, sit_pose | crates/script/src/player/interact.rs:433 |
| `ps_magic` | 9 | bow_pose, magic_bow, teleport | crates/script/src/player/magic.rs:44 |
| `escape` | 9 | craft_anvil, lockpick, loot_chest, menu_save_load, trade_click, trade_ | crates/ui/src/manager.rs:246 |
| `clear_baskets` | 8 | trade_click, trade_esc | crates/gameplay/src/trade.rs:164 |
| `assure_items` | 8 | craft_anvil, menu_save_load, quick_slots | crates/gameplay/src/inventory/list.rs:511 |
| `shoot` | 8 | bow_pose, magic_bow, shaman_duel | crates/projectile/src/flight.rs:256 |
| `subtitle_align` | 8 | controls_basic, dialog_quest, teach, trade_click, trade_esc | crates/ui/src/dialogmenu.rs:103 |
| `start` | 8 | controls_basic, dialog_quest, teach, trade_click, trade_esc | crates/ui/src/dialogmenu.rs:133 |
| `use_stack` | 7 | bow_pose, magic_bow, shaman_duel | crates/script/src/player/use_item.rs:56 |
| `in_state` | 6 | combat_basic, shaman_duel, swim, walk_to_reddock | crates/audio/src/sequencer.rs:333 |
| `hero_arrow` | 6 | bow_pose, magic_bow, shaman_duel | crates/projectile/src/launch.rs:30 |
| `has_crime_refuse` | 6 | controls_basic, dialog_quest, souls_er, teach, trade_click, trade_esc | crates/script/src/speech.rs:358 |
| `assess_talk` | 6 | controls_basic, dialog_quest, souls_er, teach, trade_click, trade_esc | crates/script/src/speech.rs:378 |
| `ps_magic_cast` | 6 | bow_pose, magic_bow, teleport | crates/script/src/player/magic.rs:94 |
| `ps_magic_reload` | 6 | bow_pose, magic_bow, teleport | crates/script/src/player/magic.rs:220 |
| `shoot` | 6 | bow_pose, magic_bow, shaman_duel | crates/script/src/player/ranged.rs:35 |
| `add_mana_points` | 5 | bow_pose, magic_bow, teleport | crates/script/src/stats.rs:312 |
| `start_jump` | 5 | walk_to_reddock, world_walk | crates/script/src/combat/commands.rs:158 |
| `can_open` | 5 | lockpick, loot_chest | crates/script/src/player/interact.rs:31 |
| `ps_interact_chest` | 5 | lockpick, loot_chest | crates/script/src/player/interact.rs:613 |
| `ps_melee` | 5 | walk_to_reddock, world_walk | crates/script/src/player/melee.rs:55 |
| `ps_ranged` | 5 | bow_pose, magic_bow, shaman_duel | crates/script/src/player/ranged.rs:92 |
| `smalltalk_dispatch` | 4 | menu_save_load, quick_slots | crates/script/src/routine/work.rs:1575 |
| `enable` | 4 | sector_switch | crates/world/src/sector.rs:180 |
| `on_enable` | 4 | sector_switch | crates/world/src/sector.rs:189 |
| `restore` | 3 | menu_save_load, quick_slots | crates/runtime/src/save.rs:304 |
| `get_cast_action` | 3 | bow_pose, magic_bow, teleport | crates/script/src/magic/cast.rs:64 |
| `can_cast` | 3 | bow_pose, magic_bow, teleport | crates/script/src/magic/cast.rs:99 |
| `run_cast` | 3 | bow_pose, magic_bow, teleport | crates/script/src/magic/cast.rs:240 |
| `apply_snapshot` | 3 | menu_save_load, quick_slots | crates/sim/src/persist.rs:224 |
| `open_menu` | 3 | menu_save_load | crates/ui/src/manager.rs:222 |
| `step_basket` | 2 | trade_click, trade_esc | crates/gameplay/src/trade.rs:87 |
| `cancel` | 2 | trade_click, trade_esc | crates/gameplay/src/trade.rs:273 |
| `generate_trade_inventory` | 2 | trade_click, trade_esc | crates/gameplay/src/treasure.rs:355 |
| `refresh_trade_inventory` | 2 | trade_click, trade_esc | crates/gameplay/src/treasure.rs:376 |
| `transfer_items_to` | 2 | trade_click | crates/gameplay/src/inventory/list.rs:1209 |
| `transfer_transaction_items_to` | 2 | trade_click | crates/gameplay/src/inventory/list.rs:1319 |
| `use_type` | 2 | trade_click, trade_esc | crates/gameplay/src/inventory/stack.rs:116 |
| `spell_missile` | 2 | bow_pose, magic_bow | crates/projectile/src/launch.rs:71 |
| `send_perception_use_interact_object` | 2 | lockpick, loot_chest | crates/script/src/assess.rs:358 |
| `long_work_secs` | 2 | menu_save_load, quick_slots | crates/script/src/helpers.rs:114 |
| `magic_projectile` | 2 | bow_pose, magic_bow | crates/script/src/magic/cast.rs:282 |
| `unlock` | 2 | lockpick, loot_chest | crates/script/src/player/interact.rs:79 |
| `ps_sneak` | 2 | walk_to_reddock | crates/script/src/player/normal.rs:174 |
| `set_current_destination_point` | 2 | menu_save_load, quick_slots | crates/script/src/routine/select.rs:170 |
| `trade_open` | 2 | trade_click, trade_esc | crates/sim/src/trade.rs:77 |
| `time_of_day` | 2 | dialog_quest, ui_screens | crates/ui/src/journal.rs:69 |
| `on_touch` | 2 | triggers | crates/world/src/trigger.rs:335 |
| `can_be_activated` | 2 | triggers | crates/world/src/trigger.rs:570 |
| `activate` | 2 | triggers | crates/world/src/trigger.rs:643 |
| `balance_gold` | 1 | trade_click | crates/gameplay/src/trade.rs:224 |
| `commit` | 1 | trade_click | crates/gameplay/src/trade.rs:258 |
| `delete_stack` | 1 | craft_anvil | crates/gameplay/src/inventory/list.rs:700 |
| `add_enclave_theft_count` | 1 | loot_chest | crates/script/src/crime.rs:126 |
| `is_bothered_by_player_theft_of` | 1 | lockpick | crates/script/src/perception.rs:166 |
| `magic_instant` | 1 | teleport | crates/script/src/magic/cast.rs:270 |
| `magic_teleport` | 1 | teleport | crates/script/src/magic/effects.rs:653 |
| `synthesize` | 1 | craft_anvil | crates/script/src/player/craft.rs:32 |
| `close_menu` | 1 | menu_save_load | crates/ui/src/manager.rs:228 |
| `on_untouch` | 1 | triggers | crates/world/src/trigger.rs:350 |
| `deactivate` | 1 | triggers | crates/world/src/trigger.rs:724 |

## Movement states re-run (2026-10-22, branch night/p4-move)

`port-coverage` build, scenarios `p4_jump`, `p4_fall`, `p4_slide`, `p4_swim`, `controls_basic`, `world_walk`, `sit_pose`, `craft_anvil`, `combat_basic` (dumps merged; `scripts/portcov/report.py` was not re-run, the numbers below are read from the dumps). Newly called by the running game:

| function | calls | caller |
|---|---|---|
| `Sim::movement_mode_changed` (`OnMovementModeChanged`, S:adbf0) | 118 | `Command::HeroModeChanged` from `P/move_states.rs::after_motion` |
| `ps_jump` (`PS_Normal_Jump`) | 7 | jump key (`p4_jump`, `p4_slide`) |
| `ps_normal`, `ps_normal_loop`, `ps_sneak`, `confirm` | 13 / 2219 / 1 / 3 | as before |
| `on_slide_jump` (`OnSlide` -> `PS_Slide_Jump`, `CharacterController::sub_step`) | see `p4_slide` | jump key after 0.6 s of sliding |

Still never called and now duplicates of the wired path: `down::fall_damage`, `down::land_from_fall` (S:adbf0 landing rule on the `ScriptHost`) and `physics::fall_damage` as an entry point; the running game applies the same rule through `Sim::land` -> `vitals::fall_damage` (it needs the simulation's kill path for the hero). The movement mode machine and controller functions (`gCCharacterMovement_PS`) carry no `Port of ... (Dll:addr)` doc line and are not in the manifest; their behaviour is covered by the `p4_*` scenarios instead (state entered, clip, hit points, positions).
