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
