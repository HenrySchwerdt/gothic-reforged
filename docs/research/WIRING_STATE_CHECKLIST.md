# Per-state behaviour checklist: original vs. our running game

Static audit (2026-10-21): part A = hero states PS_Normal .. PS_Interact*, PS_HUD*, locomotion, camera, focus; part B = PS_Melee*, PS_Ranged*, PS_Magic*, lock-on, hit reactions. Companion of `WIRING_COVERAGE.md` (runtime call counts) and `WIRING_GLUE_SCAN.md`. Counts: part A YES 56 / PARTIAL 32 / NO 22 (movement rows updated 2026-10-21, branch night/p4-move); part B YES 45 / PARTIAL 38 / NO 22. Line numbers drift while other agents edit `hero.rs`, `ranged.rs`, `cam.rs`.

# Wiring audit: hero states PS_Normal .. PS_Interact*, PS_HUD*, engine locomotion / camera / input

Scope: what the original does per player state (own words; sources `.local/research/script_game/{player_state,callback,player_interface,ai_command}`, `.local/research/engine/Game`, docs/research/PHYSICS_MOVEMENT.md, SCRIPT_PORT_PLAN.md, ENGINE_MAP.md) versus what the RUNNING play mode (`apps/game/src/play/**`, `apps/game/src/walk.rs`, `crates/runtime`, the sim drive loop) really does. Line numbers are as read on 2026-10-21; other agents are editing hero.rs / ranged.rs / animation, so lines may drift by a few.

Legend: YES = wired and I read the call chain; PARTIAL = part of it, or an approximation; NO = not done in the running game (a ported function may exist that nothing calls); N/A = original has no such behaviour (not counted).
Path prefixes: `P/` = apps/game/src/play/, `W/` = apps/game/src/walk.rs, `PH/` = crates/physics/src/, `S/` = crates/script/src/, `SIM/` = crates/sim/src/, `RT/` = crates/runtime/src/.
`Name (S:addr)` = script DLL RVA, `G:addr` = Game.dll engine RVA.

## 0. How the hero is driven today (read first)

* Body motion is NOT run by the original script states. Each frame `P/hero.rs::input` builds a camera-relative wish from the movement keys (hero.rs:354-361), picks a gait (hero.rs:363-370) and `W/player_motion` calls the ported `CharacterController::step_command` (walk.rs:1363). Heading, turn rate, acceleration, slopes, slide, jump, fall, swim are the ported engine (`PH/controller.rs`, `PH/movement.rs`).
* Script states (`PS_*`) run inside the sim via `Sim::drive_player` (SIM/script_player.rs, `drive_player`) and only for the commands the host sends: `Confirm`, `UseInteractObject`, `PickUp`, `ToggleSneak`, `Jump`, `Dodge`, `MeleeMove`, `Parade`, `UseItem`, bow and magic commands (RT/core.rs:760-826). Only the DATA effects of those states (inventory, stamina, events, busy time) reach the screen; their physical effects (walk to the object, stop, align, tool props, fall, swim) are not reproduced.
* `HeroTransform` carries position and yaw to the sim. Since 2026-10-21 (branch night/p4-move) the mode machine is also wired: `P/move_states.rs` forwards every `ModeChange` (`drain_mode_changes`) and the landing of `StepReport` as `Command::HeroModeChanged` -> `Sim::movement_mode_changed` (SIM/moves.rs, port of `OnMovementModeChanged` S:adbf0), which applies fall damage, `Perk_Acrobatic`, `PS_Swim` and the ani states, and answers `Event::ModeChanged` (heavy landing / dive-in clips). The hero's body state (`Command::HeroMovement`) feeds the stamina tick (`IsSprinting`, jumping).
* `second_tick` regenerates with the hero's body state (`Sim::hero_move`): the sprint / jump variants of `stamina_tick` (SIM/vitals.rs:83, S:b0520) are selected; the interaction variant (-1) is still never passed.

---------------------------------------------------------------------------------------------------

## 1. PS_Normal / PS_Normal_Loop and engine locomotion (`gCCharacterControl_PS`, `gCCharacterMovement_PS`)

Original `PS_Normal` (S:80cc0), on entry: AIMode routine; player may swim (`TreatWaterAsSolid` off); camera mode normal; `_AI_ChangeAction`; `_AI_HoldInventoryItems` (put the held weapon away); two one-off tutorial pop-ups (`TUT_AfterFriends`, `TUT_AfterLester`); focus entity cleared, focus mode 2, range rating off, look-at and alignment target cleared; `_AI_StandUp`; then idle in `PS_Normal_Loop` (S:815f0). A drawn weapon hands over to `PS_Melee` / `PS_Ranged` / `PS_Magic`. `PS_Normal_Loop` reacts to messages (jump, sneak, use, quick slots, confirm, hold).

| behaviour (original) | our game | evidence / ported fn to use |
|---|---|---|
| Movement keys -> camera-relative wish direction (flatten axes, sum pressed keys, normalise; `gCCharacterControl_PS::ProcessMovements`, G:ddd40) | YES | P/controls.rs `InputFrame::move_axes`; hero.rs:354-361; walk.rs:1338-1357 uses `movement::wish_direction` (PH/movement.rs:507) |
| Ground speeds by gait: run 400, walk 150, sneak 280 cm/s, strafe/back 280, directional max by angle (`GetCurrentMaxSpeedOfDir`, G:eb800) | YES | PH/controller.rs:605 `directional_max`, PH/profile.rs:209; `step_command` controller.rs:503-570 |
| Acceleration 1000 cm/s^2, instant stop without wish (`ProcessControledTranslation`, G:e2410) | YES | PH/controller.rs:728-745 `ground_step` |
| Turn model: heading follows the wish with `TurnSpeedMax` 200 deg/s (x0.5 sprinting) (`CalcNewRotationAngleAndVelocity`, G:e49f0) | YES | PH/controller.rs:531-544, PH/movement.rs:547 `step_turn` |
| Walk modifier: held = walk gait; the original latches walk only when the key goes down while moving (`OnPlayerWalk`, S:638b0 tests `IsStanding` / `IsBraking`) | PARTIAL | held / caps-toggle approximation P/controls.rs:166-171, hero.rs:363-369; `Order::Walk` is dropped (hero.rs:600). Port: `PlayerInput::route` Walk branch (crates/input/src/player.rs:239) |
| Sprint gait (mode 5, 600 cm/s, `Move_Sprint_*` clips exist, stamina -10 per tick, -5 with `Perk_Sprinter`, `IsSprinting` restored after a pick-up) | PARTIAL | everything below the trigger is wired: the mode machine and `step_command` run `Gait::Sprint` (600 cm/s), the locomotion mixer plays `Move_Sprint_*`, `Sim::hero_move` makes `IsSprinting` true for the stamina tick (`stamina_tick`, S:b0520: -10 / -5 Sprinter) and `Command::HeroMovement` carries it (P/move_states.rs). What *starts* a sprint for the player is [U]: `OnPlayerWalk` (S:638b0) only latches the walk gait, `gCCharacterControl_PS` has no sprint rule, NPCs sprint through `ZS_Attack_Sprint`; the play layer therefore never requests it |
| Strafe / back-pedal clips (`Move_*_Left/Right/Back`) from the filtered local velocity (`CalcAniVelocity`, G:e1cd0) | YES | hero.rs:998-1010 `local_ani_velocity`; crates/animation/src/locomotion.rs mixer |
| Turn-in-place clips `Move_Turn90` | PARTIAL | only when a dialog / combat target gives a turn error (hero.rs:1011-1025, 1058-1062); free standing turns have none |
| Jump key: `PS_Normal_Jump` pays the jump stamina, `_AI_Jump`, back to loop | YES | hero.rs:620-624 -> `Command::Jump` -> `Sim::player_jump` -> `ps_jump` (S/player/normal.rs:147) + `start_jump`; physics take-off PH/controller.rs:691. `CanJump` (movement-mode test) is approximated by `grounded` (hero.rs:421) |
| Jump arc: 600 cm/s up, ends at vy < 0.5, air steering (`JumpSteerScaleFactor`) | YES | PH/controller.rs:691-698, `air_step` |
| Jump out of a slide (`OnSlide` -> `PS_Slide_Jump`, `_AI_Jump`) | YES | `OnSlide` (S:af0e0): jump message after 0.6 s of sliding (float at S:0x101074e8) -> `_AI_Jump` (mode 6, 600 cm/s): `CharacterController::slide_time` / `SLIDE_JUMP_DELAY`, PH/controller.rs `sub_step`; QA `p4_slide` (ignored at 0.45 s, jumps after 0.8 s) |
| Walk off a ledge higher than StepHeight -> fall mode, fall steering | YES | PH/controller.rs:752-754 `start_fall`, `air_step` |
| `ZS_Fall` on entering fall mode (`OnMovementModeChanged`: force pose, fall animation) | YES | every mode change of the machine reaches `Sim::movement_mode_changed` (SIM/moves.rs, `Command::HeroModeChanged`, P/move_states.rs `after_motion`): `ZS_Fall` -> sim ani state `Fall`; the `Fall/Loop` clip plays by velocity and the camera goes to Fall mode (P/cam.rs:93) |
| Landing clips `Fall_End` / `Fall_Recover`; heavy landing `Fall_Hit` (`ZS_Land`, `ZS_HeavyLand`) | YES | `MoveKind::Land` (End / Recover) for free landings; a landing that cost hit points plays `Fall_Hit` (`MoveKind::HeavyLand`, `ZS_HeavyLand` S:ae9b0) and the hero cannot move until the clip ends (P/move_states.rs `before_motion`); QA `p4_fall`: 250 cm = `Fall_End`, 1104 / 1500 cm = `Fall_Hit`, 0 cm walked while it plays |
| Fall damage `floor(160*(t-0.9)^2)`, halved by `Perk_Acrobatic` (+ `ZS_Acrobat`), lethal -> `ZS_RagDollDead`, immune: ghost / god mode / bloodfly | YES | `Sim::movement_mode_changed` (SIM/moves.rs, port of `OnMovementModeChanged` S:adbf0) -> `Sim::land` -> `vitals::fall_damage`; fall time = `Landing::fall_time` (restarts at the jump apex). A landing on ground too steep to stand goes to `ZS_Slide` with no damage (S:adbf0 `LAB_100ae3d9`). QA `p4_fall`: lost hp == formula for 0.70 / 1.50 / 1.76 s. Not done: the `ZS_Acrobat` clip (none in `hero-motions.txt`), ghost status 0x80 immunity (hero never has it) |
| `Perk_Acrobatic` learned by falling into water (`GO_Learned`) | YES | falling more than 7.45 s (double at S:0x10107658) into deep water teaches it; the `GO_Learned` message is queued (`script.messages`, which the HUD does not show yet); QA `p4_swim` (11500 cm drop) |
| Slope speed scale, steep limit 49 deg, slide start (`CanWalkOnFloor`, `CanSlide`, `CalcGroundSlopeTransScale`) | YES | PH/controller.rs:733-744, 764-771, 813-819 |
| Slide mode physics (800 cm/s, inertia 0.4) | YES | PH/controller.rs:821-848 |
| Slide animation: `Slide_Begin` / `Slide_End` clips, `ZS_Slide`, `ZS_Slide_Loop` | YES | `MoveKind::Slide` / `SlideEnd` (ANIM/locomotion.rs); QA `p4_slide` sees both clips. The slide moves along the slope at `SlideSpeedMax` (horizontal part x cos(angle)) |
| Step height 65, `PutToGround` foot filter | YES | PH/controller.rs:750-763, `ground_follow_factor` PH/movement.rs:631 |
| Moving platforms (`ContributeFloorMovements`), stuck check (`CheckCharacterGotStuck`) | NO | PHYSICS_MOVEMENT.md section 6 open items |
| Hero vs NPC capsule separation (`CanBePushedWhileIdle` false) | YES | P/nav.rs:80-99 (`crowd::separate`) |
| Climb / ladder / ledge grab | N/A | no climb state in script_game, no ladder / climb string in the engine dumps or hero-motions: Gothic 3 has none |
| Footsteps per stride, gait, foot, floor material (`EFF_Step_<Human|Orc|Default>_<Walk|Run|Sprint>_<L|R>_<material>`, `FUN_100dd320`, clip phase 0.6 / 0.1) | PARTIAL | P/audio.rs:106-130: distance based, `earth` material hard-coded, no sprint, no jump / land sounds, no water steps |
| Head look-at toward the focus entity (`PropertyFocusLookAtMode`, `Entity::SetLookAtTarget` in PS_Normal) | NO | `animation::lookat::LookAtSet` (crates/animation/src/lookat.rs) has no user (grep over apps/ and crates/) |
| `_AI_HoldInventoryItems` when drawing / holstering (Hold_Begin / End clips) | YES | hero.rs:627-649 |
| Entering PS_Normal: camera normal, `_AI_StandUp`, focus reset | PARTIAL | camera normal by `wanted_mode` (P/cam.rs:86-113); no stand-up transition |
| Tutorial pop-ups (`_AI_HUD_OpenTutorial`) | NO | no caller in apps/ |
| `PS_StopForMenu` / `PS_QuickUse`: stop the hero (`StopMovement`) before menus and quick items | PARTIAL | menus zero wish and keys (hero.rs:336-347, controls.rs:243); `UseItem` has no stop-and-wait step |

### 1b. PS_Normal_Loop messages: confirm key, take, loot

| behaviour (original) | our game | evidence / ported fn to use |
|---|---|---|
| Confirm on a loose item -> `PS_Normal_Take`: stop, `_AI_TakeItem` (walk to it, bend), theft counted unless dropped (`AddEnclaveTheftCount`) | PARTIAL | hero.rs:666-681 -> `ps_normal_take` (S/player/normal.rs:111) -> `PickedUp`; sim gives 800 ms busy (SIM/script_player.rs `AiCommand::TakeItem`), but no bend / `PickGround` clip, no walk-to; the item mesh despawns on the event (P/interact.rs, `PickedUp` arm) |
| Confirm on a body -> `PS_GonnaLoot`, loot window, `NothingToGet` voice, merchandise excluded | YES | `confirm` / `ps_gonna_loot` (S/player/normal.rs:139, 217); `Confirm::Loot` -> window (P/interact.rs, `Confirm::Loot` arm) |
| Quick-loot-all key (`TakeAll`, session key 44) | NO | `Sim::player_quick_loot` exists (SIM/script_player.rs:502) but nothing binds `SessionKey::TakeAll` (grep); only Enter inside the window |
| Confirm on interact object -> `PS_Interact` | YES | `Confirm::Interact` -> `UseInteractObject` (P/interact.rs) |
| Confirm on a living NPC -> `AssessTalk` -> `ZS_Talk` (+ refusal voices) | YES | `Confirm::Talk` (P/interact.rs); `assess_talk` (S/speech.rs) |
| Confirm on a sleeping NPC with `Perk_MasterThief` -> robbery | NO | `Confirm::Rob` only prints "noch nicht umgesetzt" (P/interact.rs) |
| `HUD_ChestLocked`, `HUD_NotDuringBattle` messages | PARTIAL | locked chest speaks via `Refusal` (S/player/interact.rs `refuse`); `HUD_NotDuringBattle` has no port |

---------------------------------------------------------------------------------------------------

## 2. PS_Sneak (`PS_Normal_Sneak`, `PS_Melee_Sneak`, `PS_Ranged_Sneak`, `PS_Magic_Sneak`, `OnPlayerSneak` S:63a00)

Original: the duck key press enqueues the sneak message (humans); the state toggles the ani state Sneak / Stand, sets the movement mode to walking and returns to the loop of the weapon mode; speed 0.7 x 400 = 280; perception rules for a sneaking player; crossbow sneak = walk loop.

| behaviour (original) | our game | evidence / ported fn to use |
|---|---|---|
| Duck key = toggle (message on press) | YES | `PlayerInput::route` Duck (crates/input/src/player.rs:246) -> hero.rs:492 -> `Command::ToggleSneak` (hero.rs:617-619) -> `ps_sneak` (S/player/normal.rs:169) |
| Sneak gait speed, `Move_Sneak` / Sneak pose clips | YES | hero.rs:352-364 (`Gait::Sneak`), hero.rs:1050 `input.sneak`; locomotion pose "Sneak" (locomotion.rs:383) |
| Perception: sneaking player hidden from the poll below 1000, `AssessPlayerSneaking` | YES | SIM/perception.rs:69 reads `ani_state == Sneak` |
| Sneak while a weapon is drawn (melee / bow / magic variants, crossbow sneak loop) | PARTIAL | same toggle path (`back_to_loop` picks the weapon-mode loop); clip coverage of drawn sneak poses unverified; `PS_Ranged_CrossBowSneak_Loop` ported (S/player/ranged.rs:267) |
| Sneak ends on entering water / interaction | YES | `PS_Swim` sets the swim ani state (leaves `Sneak`), `ZS_Fall` the fall state; the stand state returns when the water is left (QA `p4_swim`) |

---------------------------------------------------------------------------------------------------

## 3. PS_Swim / PS_Swim_Loop / ZS_DiveIn / diving (`callback/PS_Swim.c`, `ZS_DiveIn.c`, `OnMovementModeChanged`)

Original: entering swim mode (`OnMovementModeChanged`) cuts the current task and starts `ZS_DiveIn` (drop-in-water effect `eff_ani_dropinwater_01`, dive-in clip) and then `PS_Swim` for the hero (NPCs die as ragdolls). `PS_Swim`: clear focus, disable status effects, stop the burning effect, put all held items away (`HoldStacks(-1,-1)`), delete a burning torch from the hand, set the swim ani state, then `PS_Swim_Loop`. Swim depth 155; leaving the water continues the routine.

| behaviour (original) | our game | evidence / ported fn to use |
|---|---|---|
| Swim mode deeper than `SwimDepth`, surface following, dive offset | YES | physics PH/controller.rs `swim_step`; the water level is the highest water-mesh surface (lakes, rivers: `gCWaterZone_PS` carriers, tile triangles in `WaterSurfaces`, W/walk.rs) under the hero, else the sea level; diving with the `Up` / `Down` session keys (jump / duck, INI names `Up` / `Down`) while in the water (P/move_states.rs). Breath / drowning: [U] (no rule found in the script DLL) |
| Swim / dive camera modes | YES | P/cam.rs:90-92 from `ctrl.machine.mode()` |
| Swim animation (`Hero_Swim_*_Move_Walk/Run/Stand`, `Ambient_Loop` exist) | YES | `BodyPose::Swim` / `Wade` select the `Hero_Swim_*` / `Hero_Wade_*` clips (ANIM/locomotion.rs `resolve_motion`); QA `p4_swim` sees `Hero_Wade` while wading and `Hero_Swim_..._Move_Walk` while swimming |
| `PS_Swim`: items away, torch deleted, focus cleared, effects stopped | PARTIAL | S:af700: swim ani state (sim), `HoldStacks(-1,-1)` = weapon put away (`SetDrawn(false)`); QA `p4_swim` (drawn before, holstered in the water). Not done: torch deletion, focus clear, effect stop |
| `ZS_DiveIn`: splash effect + dive-in clip | PARTIAL | the dive-in clip (`Hero_Swim_..._Fall_Hit`, `MoveKind::DiveIn`) plays after a fall into water; no `eff_ani_dropinwater_01` splash effect |
| AniState Swim / Dive on the sim actor (NPCs ignore swimmers) | YES | `Sim::movement_mode_changed` sets `Swim` on `PS_Swim` / `ZS_DiveIn`, back to `Stand` on leaving the water (`Dive` is not set: the script has no diving state) |
| Wading: water footsteps, shallow-water effects, `GetWaterDepth` | NO | no floor-material feed (P/audio.rs:128 hard-codes `earth`) |
| Dive-in after a fall (damage replaced, Acrobatic) | YES | no damage for a fall that ends in deep water, `ZS_DiveIn` task; QA `p4_swim` (1500 cm: hp unchanged) |

---------------------------------------------------------------------------------------------------

## 4. PS_Evade / dodge (`ZS_EvadeLeft/Right/Forward/Backward`, `ZS_EvadeLoop`; hero trigger = `PS_Melee_JumpBack` message)

Original: pay stamina; start a root-motion evade clip with the character's own movement switched off; four ray probes at knee and chest height keep it out of walls; at the end of the clip restore walking, put the body to ground, return to `PS_Melee_Loop`. The player layer has no double-tap dodge (P/controls.rs header).

| behaviour (original) | our game | evidence / ported fn to use |
|---|---|---|
| Jump-back combo -> `ZS_EvadeBackward` | YES | `msg::JUMP_BACK` -> `Command::Dodge{Backward}` (hero.rs:531-535) -> `Sim::player_dodge` -> `evade::start` (S/player/evade.rs:77) |
| Left / right / forward evade for the player | NO | `Command::Dodge` is only sent for Backward (hero.rs:531); the other `Direction`s are unreachable from input |
| Stamina payment (10 [I]) and refusal at 0 stamina | YES | `evade_entry` (S/player/evade.rs:95); host guard hero.rs:77 |
| Root-motion evade clip | NO | `queue_steps(.. (clip, "Hit"))` (hero.rs:81-90) but no `Evade*` / `JumpBack` name exists in .local/hero-motions.txt, so no clip; a fixed 280 cm slide in 0.6 s instead (hero.rs:56-57 [I]) |
| Wall probes (`CombatApi::can_evade`) | PARTIAL | `can_evade` returns true (SIM/script_host.rs:1907); wall safety only through `ctrl.nudge` collision (hero.rs:97) |
| Movement lock during the evade, restore walking after | NO | walk input keeps driving the controller while the nudge runs (`dodge_motion` does not zero `scripted_wish`) |
| Return to `PS_Melee_Loop` after the clip | YES | `evade_loop` (S/player/evade.rs:129), busy time via `drive_player` |

---------------------------------------------------------------------------------------------------

## 5. PS_Interact (generic, S:6da40), `PS_Interact_Loop`, `_End`, `_WaitForExit`

Original `PS_Interact`: per use type check requirements and print refusals (`HUD_MissingItem_Interact`, `HUD_MissingSkill`, `HUD_NotDuringBattle`, recipe headers, `DoesntWork_00`, god choice `Innos/Adanos/Beliar_chosen`); set movement constraints; clear the alignment target; if the hero is still moving wait 500 ms; `StopMovement`; register `OnInteract_Loop` as local callback; `_AI_Goto_Ext` to the object's interact point (chests: separate branch when sneaking); then switch into the object's state. `PS_Interact_Loop`: wait while a HUD page is open or the pose is held, with random idle variants (`OnInteract_Loop`). `PS_Interact_End`: put tools away, change action, `_AI_StandUp`, re-enable collision with the object, merge stacks, continue routine.

| behaviour (original) | our game | evidence / ported fn to use |
|---|---|---|
| Requirement checks + refusal messages | YES | `check` / `ps_interact` (S/player/interact.rs:372, 434); toasts P/interact.rs (`Refused` arm, `refusal_text`). Missing: `HUD_NotDuringBattle` |
| Only objects in front and within range can be used | YES | `find_focus` hero.rs:852-938 (see section 8) |
| Stop movement, wait until still, then use | YES | timed uses (a `Posed` hero that is not in a held seat / bed pose) zero the wish and the jump (P/move_states.rs `before_motion`); held poses still end on a movement key. The 500 ms wait for a moving hero is not modelled (the controller stops at once) |
| `_AI_Goto_Ext`: walk to the object's interact point and align | NO | `ps_interact` emits no goto and the host never moves / rotates the hero to the point: the pose plays where he stands (nothing sets `ctrl.position` from an object; grep over apps/) |
| Tool props in the hands (hammer, pickaxe, saw, drum sticks, shovel, scoop, stick / meat, sword on the anvil) | NO | `AiCommand::HoldInventoryStacks` is only 400 ms of busy time (SIM/script_player.rs, `drive_player`); no mesh attached (only sword / arrow / bow mounts exist: hero.rs:1072-1119) |
| Pose clips Begin -> Loop -> End for stations and seats | YES | P/pose.rs `build` / `pose_system`, chosen in P/interact.rs `pose_for` + `InteractStarted` |
| Random idle variants while using (`OnInteract_Loop`) | NO | one looping ambient clip |
| Camera interaction mode | PARTIAL | `CameraMode::Interaction` only while a `Posed` component exists (P/cam.rs:96-98); window-only stations release the pose at once so the camera stays normal. Original `SetMode` args differ from the combat states [U, stack args] |
| `PS_Interact_End`: stand-up, collision back, merge stacks, weapon holstered | PARTIAL | `InteractEnded` releases the pose (exit clip) and sets `drawn=false` (P/interact.rs); stack merge in the sim; no collision toggle (hero never collided with objects) |
| `PS_Interact_WaitForExit` held poses until an exit request | YES | `EndInteraction` on move key / use / confirm (hero.rs:608-614) -> `player_end_interaction` |
| Stamina -1 per tick while interacting | NO | `Movement::Interacting` (SIM/vitals.rs:93) never passed |
| Single-use objects (`UsedByPlayer`: bookstand, stoneplate, ore, treasure) | YES | `station_state` (S/player/interact.rs:504); `used` flag from data (P/objects.rs:172) |

### 5b. Per object state

| state (S addr) | original behaviour | our game | evidence / gap |
|---|---|---|---|
| `PS_Interact_Chest` (S:70210) + `_Loop` | `CanOpen`; `_AI_Unlock` with key / lockpick (random fail, perks, `PickBroke`), kneel-open clip, treasure sets generated, witnesses told, loot window, loop closes when the page is closed and the last stack taken | YES | `ps_interact_chest` + `unlock` + loop (S/player/interact.rs:78, 618, 655); loot window and sounds P/interact.rs (`Window(Loot)`, lock events). Gap: the chest lid / basket object animation is not played |
| `PS_Interact_Door` (S:70970) | unlock if locked, toggle open / closed with `eff_ani_door_open_01`, `OpenDoor` clip | PARTIAL | state machine + `DoorChanged` (S/player/interact.rs:674); play shows only a toast (P/interact.rs `DoorChanged` arm); the door entity does not swing and its collision never opens (no door code in apps/) |
| `PS_Interact_SitBench / SitStool / SitThrone` | hold items, sit pose, wait for exit, sitting HP regen | YES | STATIONS table (S/player/interact.rs) + pose (P/pose.rs `POSE_ACTIONS`); no alignment to the seat (see goto row); regen via `ani_state.is_sitting()` (SIM/vitals.rs) |
| `PS_Interact_SleepBed` (S:75d40) / `SleepGround` -> `PS_DuskToDawn` (S:66fc0) | torch away, lie down, slow the clock (x0.3), movement constraints, camera mode, run to 9:00, `PlayerSleep` restores HP / stamina / mana, stand up | PARTIAL | `ps_dusk_to_dawn` (S/player/interact.rs:762) jumps the clock via `player_sleep`; `PlayerEvent::TimeScale` is ignored (P/interact.rs `TimeScale(_) => {}`), no fade, no camera / constraint step; only a toast on `Slept` |
| `PS_Interact_Anvil / Forge / GrindStone / Cauldron / Alchemy` | tools in hand, change action, open smithing / sharpen / cooking / alchemy window | PARTIAL | smithing, alchemy, cooking windows YES (P/interact.rs `station_window`); `Sharpen` (grindstone) has no page: toast "Seite noch nicht umgesetzt" + `GuiOpen(false)`; forge pose YES; anvil sound YES |
| `PS_Interact_Barbecue / Campfire` | frying (`GUI2::OpenFry`), campfire = sit-ground pose + stick / meat | PARTIAL | state ported, `Fry` window not implemented (same toast), pose YES |
| `PS_Interact_Bookstand / Stoneplate` | change action, play the book-open clip on the object, `BS_*` book states (pages, learning), single use | PARTIAL | `BookRead` event becomes a toast only (P/interact.rs); no book page UI, no object animation (`Entity::StartPlayAni`) |
| `PS_Interact_PickOre / SawLog / OrcDrum / Waterpipe / DrinkWaterBarrel / PirateTreasure` | tool in hands, timed action loop (6 / 8 / 8 / 10 / 4 / 8 s), treasure sets into the inventory, single use for ore and treasure | PARTIAL | timed pose via `pose_for`; yields via script; no tool prop, no ore-vein / ground state change, drink effect unverified |
| `PS_Interact_Shrine` / `PS_Interact_Talk` (S:72600, S:78210) | kneel, start the dialog with the object | PARTIAL | `ps_interact_talk` ported (S/player/interact.rs:791); kneel pose YES; the dialog window path of an object talk is not wired (only `Confirm::Talk` for NPCs) |
| `PS_Interact_TiltOrcBoulder / HoldOrcBoulder / LiftOrcBoulder` (+ loops, `OB_Light/Medium/Heavy/Faring`) | three-stage strength-gated boulder interaction, `TooHeavy` voice, `Faring_Boulder_Success` event | NO | ported (S/player/boulder.rs:75-229) but `Sim::set_boulder` (SIM/script_player.rs:89) is never called and `AddObject` data carries no boulder class (P/objects.rs:166-176); no clips wired (`TiltOrcBoulder`, `HoldOrcBoulder_Begin`, `LiftOrcBoulder_Begin` exist) |
| `PS_Interact_CarryStone` / `_Loop` / `_End` | take `It_CarryFront_Stone`, carry (walk mode), drop | NO | ported (S/player/boulder.rs:175); nothing registers the use type, no carry prop / movement lock |
| Lock picking | G3 has no minigame (random roll in the script) | YES | `unlock` inside the chest / door states (S/player/interact.rs:84); `Command::Lockpick` is never sent and not needed |

---------------------------------------------------------------------------------------------------

## 6. PS_HUD*, PS_QuickUse, PS_StopForMenu, dialog side of the hero

Original `PS_HUD`: AIMode talk, camera mode set (args 5, false), alignment target cleared, opens the page of the requested kind (map, book, letter, skills, spells, quests, inventory, trade, dialog, loot, alchemy, cooking, smithing, bless, poison, sharpen, fry) and arranges the stacks; `PS_HUD_Loop` waits for the page to close; `PS_HUD_Equip / UnEquip / Use` run `_AI_EquipInventoryWeapons`, `_AI_HoldInventoryItems`, `_AI_UseInventoryItem` and return to the loop.

| behaviour (original) | our game | evidence / ported fn to use |
|---|---|---|
| Open inventory / quests / skills / spells / map / book / letter pages | YES | `gothic_ui` GUI manager + `UiState`; hero input blocked while open (controls.rs:243, hero.rs:336-347) |
| Hero cannot be hurt while a page is open (`gui_open`) | YES | P/hud.rs:19-32 -> `Command::GuiOpen` (RT/core.rs:808) |
| Hero stops and has no alignment target while a menu is open | PARTIAL | wish zeroed (instant-stop controller); no alignment state |
| Camera inventory mode in `PS_HUD` | NO | `wanted_mode` (P/cam.rs:86-113) has no Inventory branch; `gui_open` only blocks zoom |
| Equip / unequip stack with Hold clips, `LastWeaponConfig` | PARTIAL | `Command::UseItem` -> `player_use_item` (S/player/use_item.rs); weapon draw / holster plays Hold_Begin / End (hero.rs:627-649); window equip has no Hold clip, `LastWeaponConfig` unused |
| Use item (`PS_HUD_Use`, `_AI_UseInventoryItem`, eat / drink clips) | PARTIAL | effects YES (`UseOutcome`), consume animation absent |
| Quick slots 1-0 (`OnPlayerUseQuickSlot`) including spell select, bow ready, weapon draw | PARTIAL | hero.rs:536-539 + `quick_use` 727-821 is a host re-implementation, not the ported handler |
| Potion keys (`OnPlayerPotionQuickUse`, `PS_QuickUse`) | PARTIAL | `potion_of` name heuristic (hero.rs:256-274), no `PS_QuickUse` stop / animation |
| Missing-requirement message (`PrintMissingRequirements`) | PARTIAL | UI crate prints some; select-weapon / quick-slot requirement branches not routed |
| `OnReturnFromMenu`, `OnResetPlayer`, `OnPlayerScriptFinished` | NO | no callers |
| `IsFirstPersonCameraAllowed` (S:aaef0) | NO | ported (S/callbacks.rs:68); the toggle is unconditional (P/cam.rs:228-230) |
| Dialog: hero turns to the partner, camera dialog director | YES | `face_target` hero.rs:167-168; P/cam.rs:176-214 |

---------------------------------------------------------------------------------------------------

## 7. Camera behaviour per state (`PSCamera::SetMode` callers: PS_Normal, Melee, Ranged, Magic, Interact, HUD, Ghost, DuskToDawn; `gCCameraAI_PS`)

| behaviour (original) | our game | evidence / ported fn to use |
|---|---|---|
| Follow camera presets (Normal / Melee / Ranged / Magic ...), mouse look, zoom, wall collision | YES | `gothic_camera::CameraState` driven by P/cam.rs `play_camera`; presets from `Camera_PC_Camera.tple` |
| Mode by state: normal unarmed, melee / ranged / magic drawn, fall, swim, dive, death, interaction | YES | `wanted_mode` P/cam.rs:86-113 |
| Inventory mode in `PS_HUD` / interaction pages | NO | section 6 |
| Dialog shots, look-back key, first-person toggle | YES | P/cam.rs:176-235 |
| Temporary first person (`enable_temp_first_person`), first-person-allowed guard | NO | no caller / see section 6 |
| Special bow camera (hero fades while aiming) | PARTIAL | implemented, disabled by `SPECIAL_BOW_HIDES_HERO = false` (cam.rs:20, 247-256) |
| Movement keys relative to the view | YES | cam.rs writes `player.yaw`; `player_motion` builds `rot` from `player.yaw` (walk.rs ~1264-1270) |
| Teleport / cinematic handling | YES | cam.rs:130-141, 187-190 |

---------------------------------------------------------------------------------------------------

## 8. Focus, targeting, auto-turn, input layer

| behaviour (original) | our game | evidence / ported fn to use |
|---|---|---|
| Focus entity search by class / range / view window / priority (`gCFocus_PS::FindFocusEntity`, G:6da20) | YES | `find_focus` (crates/gameplay/src/focus.rs:326) in hero.rs:852-938 |
| Script focus priority (`GetFocusEntityPriority`), high-priority interactions, real line of sight | PARTIAL | candidates carry `script_priority: 0.0`, `high_priority: false`, `los: true` (hero.rs:875-885) [I] |
| Target lock key (`Target locken`, `focus::lock_holds`) | NO | `SessionKey::Lock` has no consumer in apps/; `lock_holds` (crates/gameplay/src/focus.rs:380) unused |
| Focus name label while unarmed, hidden when drawn | YES | P/hud.rs:46-86 |
| Hero turns toward: dialog partner, fight target (2.5 s), aim direction while a bow / spell is drawn (camera forward), camera heading for 0.8-1 s after a melee strike | PARTIAL | `face_target` hero.rs:150-182 (the "turn to the aim direction while drawing the bow" case is lines 173-180, consumed as `face` by `step_command`, walk.rs:1359); melee auto-alignment is an approximation of the original combat rotation (`EnableCombatRotationFromSPU`) [I] |
| Aim body orientation (side-on aim pose) | PARTIAL | cosmetic quarter turn `aim_blend` (hero.rs:1031); another agent edits this |
| Key router (`OnPlayerGameKeyPressed` S:61640, action / secondary / jump / sneak / select-weapon / quick slot) | YES | `PlayerInput::route` fed at hero.rs:430-433; handlers hero.rs:488-606 |
| `OnPlayerSelectWeapon` weapon cycling by `LastWeaponConfig` | PARTIAL | the weapon key just toggles `drawn` (hero.rs:627-649) |
| Toggle-HUD key, skip talk line, `ConfirmTransformed` orders | NO | dropped at hero.rs:600-604 |
| Click that only captures the cursor is not a game click | YES | P/controls.rs:196-199 |

---------------------------------------------------------------------------------------------------

## 9. Tally

Produced by `grep -c "| YES |"` etc. over this file; the caller summary quotes these numbers (N/A rows are excluded).

YES 44, PARTIAL 33, NO 33 (N/A 1). (The aim-turn row is PARTIAL: see the combat part, section 0; the first audit pass had it as YES.)


---------------------------------------------------------------------------------------------------

# Wiring audit: combat / magic player states (PS_Melee*, PS_Ranged*, PS_Magic*, focus, hits)

Scope: what the ORIGINAL does (behaviours described in my own words from `.local/research/script_game/**`,
`.local/research/engine/**` and the docs) versus what the RUNNING GAME (`apps/game/src/play/**`,
`crates/runtime`, `crates/sim`, driven through the ported `crates/script/src/player/*`) does today.
Working tree of 2026-10-21 (hero.rs / ranged.rs / cam.rs are being edited by other agents, line numbers may drift).
Verdict column: YES / PARTIAL / NO (first word counts). `file:line` = code I read. "port" = ported function that exists.
Address notation: `S:xxxx` = Script_Game.dll RVA, `G:` Game.dll, `E:` Engine.dll. [I] = inferred, [U] = unverified.

Two layers (so the evidence is readable):
* **sim layer**: `Command` -> `crates/runtime/src/core.rs:785-828` -> `Sim::player_*` (`crates/sim/src/script_player.rs:485-570`,
  `script_magic.rs:660-721`) -> `crates/script/src/player/commands.rs` -> the ported `PS_*` states
  (`crates/script/src/player/{melee,ranged,magic,normal}.rs`), stepped by `Sim::drive_player`.
* **host layer**: `apps/game/src/play/{hero,ranged,cam,anim,events}.rs`: input routing, animation overlays, camera, crosshair.
  Many things live ONLY here and never touch the ported script state.

---------------------------------------------------------------------------------------------------

## 0. THE ANSWER: why the hero does not turn toward the aim direction while drawing the bow

**Original mechanism (engine, not script).** No `PS_Ranged*` state turns the body. The turn comes from three pieces:

1. Script `PS_Ranged` (S:837e0) calls the helper `sub_20b0` (S:20b0). For a humanoid species (`sub_1fa0`: species 0-8, 0x12, 0x16, 0x17,
   0x29, 0x30-0x33) it calls `PSCharacterControl::SetMovementConstraints(0.0)`; for anything else it passes the large constant
   `_DAT_101074cc`. The same helper is used by `PS_Magic_Loop` and `PS_HUD`. The melee attack/parade states and the magic cast states pass
   `_DAT_101074cc` (a "blocked" value). `PS_Ranged` also clears `SetLookAtTarget` and `SetAlignmentTarget`.
2. `gCCharacterControl_PS::ProcessMovements` (G:ddd40; port plan E01/E63) runs every frame for the player: unless the gCSession free-fly flag
   (`+0xd1`) is on, it calls `gCCharacterMovement_PS::SetAlignmentLookAt(camera forward on the ground plane)` when the movement-constraint float
   (`+0x5c`) is 0, and `SetAlignmentLookAt(0)` when the constraint is large (for the in-between values it compares the constraint with the
   entity's azimuth to the camera).
3. `gCCharacterMovement_PS::ProcessControledRotation` (E1 0x100e3000): when the AlignmentLookAt vector (`+0x354`) is non-zero it calls
   `ProcessControledRotationLookAt` (G:e4060), which rotates the body toward that direction with the turn model
   (`CalcNewRotationAngleAndVelocity`, TurnSpeedMax 200 deg/s) INDEPENDENT of the wish direction; the locomotion mixer then plays turn-in-place /
   strafe clips from the angle error (`+0x10c`).
   So in the original, with the bow (or a spell) up, the hero's body is continuously aligned to the CAMERA forward direction, and WASD
   strafes/backs relative to that. While melee attacks or casts run, the constraint is set to the blocked value so the body stops following the camera
   (melee instead uses `SetAlignmentTarget(focus)` -> `ProcessControledRotationToTarget`).
   [I on the exact threshold semantics of the in-between constraint values; V on the constraint 0 / blocked split and on who calls the helper.]

**Ported function(s).** None of `SetMovementConstraints` / `SetAlignmentLookAt` / `SetAlignmentTarget` is ported: the ScriptHost has only
`set_look_at` with an empty default body (`crates/script/src/host.rs:785-786`); `ps_ranged` (`crates/script/src/player/ranged.rs:93-113`) does not touch
constraints, alignment, camera mode, crosshair or auto-aim; no port of `ProcessMovements`' look-at rule or of `ProcessControledRotationLookAt` exists
(`grep -rn "alignment\|constraint" crates` finds nothing relevant). The movement crate has the TURN model
(`crates/physics/src/controller.rs:512-550`, `step_command` with `cmd.face`), the equivalent of engine part (3).

**What the game does instead (host glue).** `apps/game/src/play/hero.rs:150-182` `face_target` sets `player.face` (consumed at
`apps/game/src/walk.rs:1359` -> `step_command` -> turn model):
* priority 1 dialog partner;
* **priority 2: `state.combat_focus` within 2.5 s -> face the focused NPC (hero.rs:169-172)**; `combat_focus` is refreshed every frame the weapon is
  drawn and any target exists (hero.rs:471-474), for bow and spells too (`target = target_npc`, hero.rs:465-470);
* priority 3: weapon drawn and weapon mode Ranged/Magic (or `face_until` after a melee click) -> face camera yaw `player.yaw` (hero.rs:173-181).
So the hero faces the camera only when no NPC is focused; with an NPC focused (and for 2.5 s after) it faces the NPC, not the crosshair.
Further gaps that make it look like "no turn":
* the model is rotated an extra quarter turn while aiming (`aim_blend`, hero.rs:1026-1032), a visual hack with no original counterpart that hides what
  the body really does;
* no turn-in-place clip: `input.turn_error` is computed only for dialog and combat_focus targets (hero.rs:1013-1025, 1057-1063), never for the
  camera-follow case, so the body rotates while the legs stand;
* the facing is gated on `state.drawn && weapon_mode()` (hero.rs:173-177), not on the script state / constraint; the melee "stop following the camera
  while swinging" rule is replaced by `face_until` (0.8 s toward camera yaw, hero.rs:574, 694).

**Fix direction.** Port `ProcessMovements`' look-at rule as a pure function (inputs: constraint value, camera forward, entity azimuth, free-fly flag)
into `crates/physics` next to `step_turn`; add `set_movement_constraints` / `set_alignment_target` to ScriptHost and call them from `ps_ranged`,
`ps_magic_loop`, `ps_magic_cast`, the melee states exactly as the originals do; feed `MoveCommand.face` and `LocomotionInput.turn_error` from that
result and delete the three-branch glue in `face_target` for ranged/magic.

---------------------------------------------------------------------------------------------------

## 1. PS_Melee*

Common original rule for the attack/parade states (`PS_Melee_Attack`, `_PowerAttack`, `_QuickAttack`, `_SimpleWhirl`, `_Parade`; `HackAttack` / `Pierce` /
`Whirl` / `Finishing` do not do the alignment part): if the camera is not in first person, `SetAlignmentTarget(current target)` +
`SetMovementConstraints(blocked)`; then run the matching `_AI_*` command; then back to `PS_Melee_Loop`.

### PS_Melee (S:7e280), weapon draw

| behaviour (original) | our game | evidence / ported fn to use |
|---|---|---|
| back to PS_Normal if not in melee weapon mode | YES | port `PS_Melee` `crates/script/src/player/melee.rs:57-64`; run via `normal.rs:77-80` and sim `drive_player` |
| clear focus entity, `PSCamera::SetMode(Melee)` | PARTIAL | camera Melee mode is chosen from `drawn && weapon_mode`, not by the state: `play/cam.rs:86-110`, `cam.rs:189-195`; focus clear: no |
| `_AI_HoldInventoryItems` = draw animation while the item moves into the hand (events inside the clip) | PARTIAL | host plays `Hold_Begin/End` ("HoldRight") overlay `hero.rs:626-640`; the weapon appears via `Command::SetDrawn` at once (`core.rs:760`), not at the clip event |
| first-fight tutorial `TUT_Fight` | NO | no caller |
| clear LookAt/Alignment target | NO | not ported (section 0) |

### PS_Melee_Loop (S:7e750): focus selection, alignment, input queue

| behaviour | our game | evidence / ported fn |
|---|---|---|
| each pass pick the combat target: focus entity of melee mode, else an entity named by the queued key message if inside an azimuth/elevation window, else the current target if within a distance; write focus + `CurrentTarget` | PARTIAL | focus: `hero.rs:852-940` `focus()` uses port `find_focus` (`crates/gameplay/src/focus.rs:324`); BUT the melee swing target is `nearest_in_front(HERO_REACH)` (hero.rs:465-470, 824-847), not the focus; no `CurrentTarget` bookkeeping |
| `SetAlignmentTarget(target)` + movement constraint so the body turns to the target in a fight | PARTIAL | glue: `face_target` toward `combat_focus` for 2.5 s (hero.rs:169-172) and toward camera yaw 0.8 s after a click (hero.rs:574, 694); no port of `ProcessControledRotationToTarget` (G:e3fb0) |
| no target -> clear alignment target | NO | n/a |
| key messages -> next state: Attack, PowerAttack, QuickAttack, SimpleWhirl, WhirlAttack, PierceAttack, HackAttack, FinishingAttack, Parade, JumpBack, Jump, Sneak | PARTIAL | router `crates/input/src/player.rs:286-377` (port `OnPlayerAction` / `OnPlayerSecondaryAction`); play maps only QuickAttack, PowerAttack, Pierce (parry + hold), Parade, JumpBack, Jump, Sneak (hero.rs:514-540, 567-600). Attack, SimpleWhirl, Whirl, Hack, Finishing are never emitted (`grep -in "whirl\|hack\|finishing" apps/game/src/play/*.rs crates/input/src/*.rs` empty) |
| movement keys work in the loop (strafe/footwork with body toward target) | PARTIAL | camera-relative wish (hero.rs:352-360) + directional speed limits (`controller.rs:560-570`); body orientation is the glue above |
| loop ends (-> PS_Normal) when the melee weapon is not in hand | YES | `ps_melee_loop` `melee.rs:67-76` |

### PS_Melee_Attack / PowerAttack / QuickAttack / SimpleWhirl / HackAttack / PierceAttack / WhirlAttack / FinishingAttack

| behaviour | our game | evidence / ported fn |
|---|---|---|
| pay stamina per move (B09 table); the hero pays like NPCs | YES | port `start_move` `crates/script/src/combat/commands.rs:94-112` via `commands::melee_move` (`player/commands.rs:111-125`) <- `Command::MeleeMove` (core.rs:792) |
| hit window opens after the move's time (Attack/SimpleWhirl 0.25 s, Quick 0.10, Pierce 0.8, Whirl 0.25, Hack in parade pose) with reach check and `Whiff` | NO | NPCs do it: `sim/ai_exec.rs:440-470` `begin_swing`, `sim.rs:1141-1165` `resolve_strike`, `vitals::hit_window_secs`. The HERO: `drive_player` calls `melee_hit(hero, target)` immediately when the queued `AiCommand::Attack` is processed (`script_player.rs:545-552`), 600 ms busy; no window, no reach test, no Whiff. Use `vitals::hit_window_secs` + `begin_swing` path for the hero |
| animation speed multiplier (weapon base 0.6/0.7/1.0, Power/Hack 1.5, SimpleWhirl 1.3, Parade 2.3 for the player, tired reductions) | NO | overlays always `rate: 1.0` (`play/anim.rs:77-83`); rule in `COMBAT_AI_RULES.md` 9.5 (S:42a0) is not ported anywhere |
| swing clip per move | PARTIAL | `hero.rs:686-722` `melee_swing` picks Attack/Quick L,R/Power/Pierce by hand; SimpleWhirl, Whirl, Hack, Finishing have no mapping |
| damage details (Quick x0.5, Hack/Power x2, head shot, backstab Pierce, KO perk) | YES | sim `Attack::melee` + `resolve_hit` (ported `AssessHit`), `sim.rs:604-660` |
| `SetAlignmentTarget` + constraint during the swing (body stays on the target) | NO | see PS_Melee_Loop; host holds `face_until` toward camera yaw |
| light attack starts on key PRESS, power attack when held > 100 ms with wind-up clip | PARTIAL | `Order::MeleeAttackKey` hero.rs:567-600: swing is queued on RELEASE (QA_REPORT.md lines 47-50 document the late hit); wind-up clip `PowerAttack/Raise` after `CHARGE_HOLD_MS` |
| combo chaining: attack key during PowerAttack after a threshold -> WhirlAttack; during Finishing/Pierce -> QuickAttack; Hack -> Whirl; tap < 200 ms -> Quick (L/R chosen by `FUN_10004220`), extended-content variants | NO | chain logic is in `OnPlayerAction` (`.local/research/script_game/player_interface/OnPlayerAction.c`); the input crate only forwards `MeleeAttackKey`; `melee::move_for_press` (`melee.rs:35-41`) is the only rule used (tap/hold only) |
| FinishingAttack kills a downed enemy | PARTIAL | NPC path `ai_exec.rs:440-449`; hero can never request it |
| jump attack / sprint attack | NO | hero.rs:587-589 comment "ported player states have no jump attack" |

### PS_Melee_Parade (S:7f960)

| behaviour | our game | evidence / ported fn |
|---|---|---|
| raise guard: `_AI_Parade`, Action Parade, AniState Parade, alignment to the opponent | PARTIAL | `commands::parade` -> `ps_melee_parade` (`melee.rs:101-113`, `start_parade` commands.rs:141-147); only when a target exists (hero.rs:514-519); no alignment |
| guard pose animation | NO | no clip queued for the hero's guard (hero.rs:514-520; nothing named Parade in `play/anim.rs`) |
| guard held while right button held, released on release/tap (`OnPlayerSecondaryAction`) | YES | router `player.rs:330-377`; hero.rs:340, 436, 550 clear `parrying`. [U: sim `Action::Parade` is reset only by the next move] |
| parry rules (CanParade, stamina buffer with AIMode, stumble codes) | YES | sim `combat.rs:930-960` + `resolve_hit` |
| riposte: tap in parade -> QuickAttack | YES | `Order::EnterState("PS_Melee_QuickAttack")` hero.rs:547-558 |
| parry spark / sound | YES | `fx.rs:212-216` |

### PS_Melee_Jump / PS_Melee_JumpBack / PS_Melee_Sneak

| behaviour | our game | evidence / ported fn |
|---|---|---|
| Jump: `_AI_Jump` (stamina) + jump | PARTIAL | `Command::Jump` (hero.rs:620-624) -> `ps_jump` (`normal.rs:182-200`) pays stamina; the physical jump is decided separately by `walk.rs` (`player.scripted_jump`, hero.rs:621) |
| JumpBack: `_AI_JumpBack` (stamina 10), hop away from the opponent | PARTIAL | hero maps `msg::JUMP_BACK` to `Command::Dodge{Backward}` (hero.rs:531-535) = the evade roll (`dodge_motion` hero.rs:60-101, 280 cm / 0.6 s [I]), NOT `ps_melee_jump_back` (`melee.rs:117-130`), which play never reaches |
| Sneak: `_AI_Parade` + toggle + back to PS_Melee_Loop | PARTIAL | `ps_sneak` (`normal.rs:203-215`) toggles AniState and returns to the loop; parade part not done |

---------------------------------------------------------------------------------------------------

## 2. PS_Ranged*

### PS_Ranged (S:837e0): entry

| behaviour | our game | evidence / ported fn |
|---|---|---|
| keep focus only if AIMode was Combat; set AIMode Combat | PARTIAL | port sets Combat (`ranged.rs:99-101`), no focus handling |
| `PSCamera::SetMode(Ranged)` (preset dist 300, fov ~60.6 per runtime trace) | YES | by weapon mode: `cam.rs:86-110`; presets `crates/camera/src/preset.rs:389-395` |
| `EnableTempFirstPerson(false)`, `EnableSpecialBowMode(false)` (trace: always off) | YES | `cam.rs:20, 251` `SPECIAL_BOW_HIDES_HERO=false`; port `crates/camera/src/state.rs:230-246` |
| `LastWeaponConfig`, focus look-at mode, enable range rating | NO | no host counterpart |
| copy ranged focus entity to focus (`SetFocusEntity(GetFocusEntity(mode 3))`) | PARTIAL | `focus()` runs `find_focus` with `FocusMode::Ranged` (hero.rs:925-934) but only fills `state.target` for the host [I: script focus injected by host] |
| `SetLookAtTarget(None)`, `SetAlignmentTarget(None)`, movement constraint 0 (humanoid): body follows the camera | PARTIAL | glue only, section 0 |
| hold bow + ammo: `_AI_HoldInventoryItems` | YES | `ps_ranged` `HoldInventoryStacks` ranged.rs:106-111; `Command::ReadyRanged` core.rs:820 |
| tutorial `TUT_Bow` once | NO | not wired |
| crossbow goes straight to `PS_Ranged_Aim_Loop` (multiplier 1, crosshair + auto-aim on); bow -> `PS_Ranged_Walk` | PARTIAL | port always -> `PsRangedWalk` (ranged.rs:112); crossbow reaches aim via the Aim key |
| undrawn bow multiplier 0.2, crossbow 1.0 | YES | `ranged.rs:99-101` (`MIN_DRAW`) |

### PS_Ranged_Walk / PS_Ranged_Walk_Loop (S:83f80, S:84080)

| behaviour | our game | evidence / ported fn |
|---|---|---|
| crosshair off, auto-aim off | YES | host derives from state: `ranged.rs:224-237` (`aiming` only for Aim/PowerAim/AimLoop), fade-out `ranged.rs:377-412` |
| ranged focus each tick | PARTIAL | see above |
| message switch: AIM -> Aim; SHOOT -> Shoot (crossbow) / Aim (bow); RELOAD; jump; sneak; quick slots; potions | PARTIAL | port `crates/input/src/player.rs:471-` `ranged_walk_loop`; play uses `PlayerInput::route` and maps `msg::AIM/SHOOT/ABORT_ATTACK/JUMP/SNEAK` (hero.rs:493-507); **`msg::RELOAD` has no match arm (hero.rs:489-545)**: the reload key / empty-crossbow click does nothing, the crossbow cannot be re-cocked by key |
| walking with the weapon ready: camera-relative strafing, body on the camera heading | PARTIAL | wish OK; body facing is glue (section 0) |

### PS_Ranged_Aim (S:844a0), PS_Ranged_PowerAim (S:84830), PS_Ranged_Aim_Loop (S:84d60): the aim

| behaviour | our game | evidence / ported fn |
|---|---|---|
| action Aim, AniState Stand | YES | port `ps_ranged_aim` ranged.rs:129-153 |
| `GUI2::ShowCrossHair(true)`, 200 ms fade, 32x32 (min 8x8) `G3_HUD_CrosssHair.tga` | YES | `play/ranged.rs:61, 73-128, 224-237, 377-412` (atlas lookup logs "not in the loaded UI atlases" when absent, ranged.rs:84-88) |
| crosshair size `1 - clamp(d,100,4000)/4000` from the pick distance (`OnGetAimAngle` -> `SetCrossHairSize`) | YES | port `gothic_projectile::aim::crosshair_size`, used ranged.rs:392, 406 |
| `EnableAutoAiming(true)` -> engine `UpdateAim`: aim sine = dot(up, normalize(screen-centre pick - weapon)) (2.0 without ammo); Fwd clip slot 4 weight `1-abs(angle)/90`, Up/Down clip slot 5 | YES | `ranged.rs:338, 423-488`: `pick_point` (port `PSCamera::GetScreenCenterPickPoint`, `crates/camera/src/aim.rs:36`), `hero_aim_sine`, `AimPitch`, `clip_name`, upper-body mask |
| clip phases: Aim_Raise 0.4 s (crossbow 0.6), PowerRaise 2.0 s with effect `eff_ani_fight_bow_raise_01`, then hold | YES | `ranged.rs:305-326, 431-488`; durations `projectile::aim::clips` (`script_player.rs:659-677` `hero_action_ms`) |
| bow string animation (`..._Cock`) follows draw time | YES | `ranged.rs:576-733` `bow_sync` (`anim.seek(state_time)`) |
| draw multiplier `0.2 + 0.8*min(1,t/1.95)` per 50 ms tick | YES | port `draw_multiplier` (`script/player/ranged.rs:25`), `ps_ranged_power_aim` 168-174 |
| bow: PowerAim -> Aim_Loop after the 2.0 s raise clip | PARTIAL | port stays in `PsRangedPowerAim` forever (ranged.rs:168-174, never sets `PsRangedAimLoop`); clip phase stays `PowerRaise` past its end instead of the `Aim` hold clip |
| Aim_Loop keys: shoot; abort (0x27) -> Reload with `HoldStack(-1)` (arrow put away); sneak (crossbow); quick slots/potions; 0x44 | PARTIAL | router has `ranged_aim_loop` / `power_aim_loop`; play maps only SHOOT/ABORT; abort goes to `PsRangedWalk` (`commands::aim_ranged(false)`, commands.rs:213-226) without un-holding the ammo |
| body turn to the aim yaw (camera) while aiming | PARTIAL | section 0 |
| movement while aiming | YES | original does not stop movement (`sAIPlayAimStart` only drives overlay slots 4/5, no `EnableMovementFromSPU`); play does not restrict it [I] |
| aim camera: Ranged preset, no zoom, special bow mode off | YES | see PS_Ranged; QA_REPORT.md "Decompile / trace findings" |
| right button while drawn gives up the draw | PARTIAL | router `ABORT_ATTACK` [I] (`player.rs:373-377`) -> `AimRanged{false}` hero.rs:504-507 |

### PS_Ranged_Shoot (S:850e0) and `Shoot` (S:86450)

| behaviour | our game | evidence / ported fn |
|---|---|---|
| release: arrow at once (no Hit clip for the bow), effect `eff_ani_fight_bow_shoot_01`, raise glow stops | YES | port `ps_ranged_shoot` ranged.rs:177-213; glow stop ranged.rs:309-327; effect + sound `fx.rs:175-205` |
| player shot: ballistic (path Physics), speed `mult * 200` km/h, aimed at the screen-centre pick, origin = right-hand slot bone | YES | port `shoot` `script/player/ranged.rs:40-82`, `projectile::launch::hero_arrow`; host feeds `set_hero_aim` every frame (ranged.rs:356-372) |
| damage = ammo + bow damage; fire ammo needs mult >= 0.5; head shot x2 | YES | ranged.rs:60-62; sim hit path |
| consume one arrow/bolt; `LastOneGone` speech + `LastWeaponConfig` reset + back to PS_Normal when the quiver is dry | PARTIAL | consumption YES (ranged.rs:76-80); speech and reset NO (port goes to `PsRangedAimReload` when empty, ranged.rs:197-199) |
| after the shot: crossbow -> Walk (reload by key); bow -> `PS_Ranged_Aim_Reload` (re-nock Reload_Begin/Raise/End 0.28+0.4+0.28 s, then PowerAim again if the aim key is still held, Shoot if released, else Walk) | NO | port: bow -> `PsRangedWalk` (ranged.rs:201-207), crossbow -> `PsRangedReload` (cock 2.8 s) then Aim; Aim_Reload is conflated with Reload (ranged.rs:219-264). [I: type id 6 = crossbow]. Holding the aim key after a shot does not re-nock / re-aim |
| `Aim_Recover` 0.4 s (0.6 crossbow), right hand empty | YES | ranged.rs:206 WaitMs + overlay `Aim/Recover` (`play/ranged.rs:535-540`) |
| arrow flight / impact / stuck / fade | YES | `Sim::shot_views` -> `play/ranged.rs`, `fx.rs`; crate `projectile` |

### PS_Ranged_Reload (S:859a0) / PS_Ranged_Aim_Reload (S:85550)

| behaviour | our game | evidence / ported fn |
|---|---|---|
| crossbow cock 2.8 s; bow Reload clips | PARTIAL | state runs (ranged.rs:219-264) but is reachable only from Shoot, no key (see Walk_Loop) |
| hold ammo again; aim (crossbow) / walk (bow) | PARTIAL | `HoldInventoryStacks` ranged.rs:243-252 |

### PS_Ranged_Jump / PS_Ranged_Sneak / PS_Ranged_CrossBowSneak_Loop

| behaviour | our game | evidence / ported fn |
|---|---|---|
| jump: `_AI_Jump`, state ends, the task default (`PS_Ranged` -> Walk) resumes | NO | **bug**: `back_to_loop` (`normal.rs:218-227`) sends the ranged hero to `PsRangedAimLoop` after any jump or sneak toggle; the host treats AimLoop as aiming (`play/ranged.rs:224-237`): crosshair, aim camera clips and `set_hero_aim` appear after a jump/crouch with the bow, crosshair-off of Walk is lost |
| sneak: crosshair + auto-aim off; bow -> `PS_Ranged_Walk_Loop`, crossbow -> `PS_Ranged_CrossBowSneak_Loop` (re-enters `PS_Ranged`) | PARTIAL | `ps_sneak` toggles AniState only (normal.rs:203-215); `ps_ranged_crossbow_sneak` (ranged.rs:268-270) is not reached from `ToggleSneak` (`commands::toggle_sneak` -> `PsRangedSneak`) |

---------------------------------------------------------------------------------------------------

## 3. PS_Magic*

### PS_Magic (S:79e20), PS_Magic_Loop (S:7a590)

| behaviour | our game | evidence / ported fn |
|---|---|---|
| back to PS_Normal without a spell; Combat mode; remember `LastSpell`; hold the rune/scroll in the RIGHT hand; stand up | YES | port `ps_magic` `magic.rs:47-70`, `commands::select_spell` commands.rs:153-165 <- `Command::SelectSpell` <- `quick_use` hero.rs:803-813 |
| `PSCamera::SetMode(Magic)` | YES | by weapon mode `cam.rs:106-108` |
| clear look-at / alignment, focus none | PARTIAL | section 0; `ps_magic` clears the target (magic.rs:57) |
| tutorial `TUT_Spell` | NO | not wired |
| loop each tick: spells with `SpellTarget = focus` take the focused character, others none; focus window of mode 4 | YES | `ps_magic_loop` magic.rs:76-90 (`needs_focus`); host focus: `focus()` with `FocusMode::Magic` hero.rs:925-930 |
| `SetAlignmentTarget(None)` + constraint 0 without focus (body follows the camera); with focus alignment to it | PARTIAL | glue `face_target` (section 0) |
| input: cast key -> Cast / PowerCast / QuickCast / Summon / FlameSword by `GetCastAction`; right button + staff -> `PS_Magic_StaffParade`; hold right button -> put away; jump; sneak | PARTIAL | router `magic_input` (`input/player.rs:380-440`); play maps CAST/POWER_CAST/QUICK_CAST/SUMMON to `CastSpell` (hero.rs:508-513) and `commands::cast_spell` picks the state by `get_cast_action` (commands.rs:168-195) |
| right button + staff -> `PS_Magic_StaffParade`, shield effect `eff_magic_parade_shield_01` | NO | `msg::PARADE` always becomes `Command::Parade` -> `PsMeleeParade` (commands.rs:129-132); port `ps_magic_staff_parade` (`magic.rs:256-275`) has no caller |

### PS_Magic_Cast / PowerCast / QuickCast / Summon (+Abort), FlameSword

| behaviour | our game | evidence / ported fn |
|---|---|---|
| can-cast check: mana >= MinManaCost, focus needed, special `Spl_*` refusals, "doesn't work" grunt + fizzle effect | YES | port `can_cast` `script/magic/cast.rs:99-143` via `ps_magic_cast` magic.rs:96-110 |
| `SetAlignmentTarget(target)` + blocked constraint (not in first person): body turns to the spell target during the cast | PARTIAL | glue only (`combat_focus` faces the NPC for 2.5 s); no look-at port |
| `_AI_Cast`: Begin (load, pay MinManaCost unless Alchemy; power: pay up to MaxManaCost) -> animation -> Execute (spawn `MagicProjectile` / `MagicInstant` / `MagicSummon`) -> `Cast_Hit` -> `Cast_Recover` | PARTIAL | `run_cast` (`cast.rs:244-266`) does load + power + cast + reset in ONE step after the raise clip, so the mana is spent when the missile leaves, not when the cast starts; Hit/Recover clips queued by `play/ranged.rs:516-545` |
| cast timings: staff cast 0.8/0.28/0.84, quick 0.16/0.2/0.36, power 0.92/0.24/0.76 | YES | `script_player.rs:668-672`, `magic.rs:117-123` |
| projectile launch from the casting hand, speed from the spell, target = focus or free along forward | YES | `make_launch` (`sim/shots.rs`, `spell_missile`), `set_hero_hand` `play/ranged.rs:349-352` |
| release earlier than 0.7 s aborts the cast | YES | `Order::EnterState("PS_Magic_Loop")` hero.rs:560-564 -> `AbortCast` -> `ps_magic_summon_abort` (magic.rs:149-155) [refund is a no-op because the mana is paid later] |
| after the cast: `PS_Magic_Reload` (reset, AutoReload check, hold again, reload delay) | YES | `ps_magic_reload` magic.rs:218-252 |
| `Spl_SoulTravel` -> `PS_Ghost`, `Spl_DuskToDawn` -> `PS_DuskToDawn` | YES | magic.rs:127-141 (host presentation of ghost mode not audited here) |
| Summon, FlameSword (conjure `It_1H_MagicFlameSword_01`, scroll consumed) | YES | magic.rs:96, 159-213 (sim); visual of the sword not audited |
| crosshair for spells | YES | original shows none for spells; play shows none (`aiming` is bow only) |

### PS_Magic_Jump / _Sneak / _Reload / _StaffParade(_Loop) / PS_MagicPotion

| behaviour | our game | evidence / ported fn |
|---|---|---|
| Magic_Jump: `_AI_Jump` | PARTIAL | `ps_jump` returns the mage to `PsMagicLoop` (normal.rs:218-227); physical jump separate |
| Magic_Sneak: toggle then `PS_Magic_Loop` | YES | `ps_sneak` + `back_to_loop` |
| StaffParade / StaffParade_Loop (cast keys leave it) | NO | unreachable (see PS_Magic_Loop) |
| PS_MagicPotion: stop, hands down, Drink clip, cast the potion's spell as quick cast, continue routine | NO | `magic.rs:278-318` exists but nothing sets `State::PsMagicPotion` (`grep PsMagicPotion crates apps` = definition only); potions go through `Command::UseItem` -> `use_stack` with no drink pose in play |

---------------------------------------------------------------------------------------------------

## 4. Lock-on / focus, hits, down states, NPC combat as the player sees it

| behaviour (original) | our game | evidence / ported fn |
|---|---|---|
| focus selection by mode (ranges, azimuth/elevation windows, priorities, range rating, tie-breaks) | YES | `hero.rs:852-940` + port `find_focus` (`crates/gameplay/src/focus.rs:324-376`) |
| lock key (id 39, R / End): `gCFocus_PS::OnAction` toggles `SetFocusLocked`; a locked focus is kept while alive within `FocusLockMaxDistance` (2000) | NO | `SessionKey::Lock` exists (`input/keys.rs:75`) but `PlayerInput::route` ignores it and `controls.rs` never reads it; port `lock_holds` (`focus.rs:380`) has no caller outside tests. (Opt-in `--combat souls` has its own lock-on in `play/souls.rs`.) |
| in combat mode the movement keys steer the focus direction vector (`OnAction` cases 1-4) to pick the next target left/right | NO | no port |
| `HardFocusMaxAzi/Elev`, `HighlightFocusInFight`, `GetCrossHairEntity` | NO | focus is shown as a text label only; no combat highlight |
| victim reaction by hit result: Stumble / QuickStumble / ParadeStumble / PierceStumble / knockdown, hero movement locked during the stagger | PARTIAL | results computed in sim (`combat.rs:200-260`); visuals: every hit with `damage > 0` queues the same `QuickStumble` overlay for hero and NPCs (`events.rs:328-352`, `anim.rs:129-131`); the sim's stagger lock (`busy_until_ms`) is read only by NPC brains, not by `walk.rs`/`hero.rs` |
| parried blow: ParadeStumble clip on the attacker, weapon-clash effect | PARTIAL | spark/sound `fx.rs:212-216`; no stumble clip by result |
| down / knocked out / dead hero: death camera, fall clip, controls blocked, wake-up timers | YES | `events.rs:366-390` (`hero_down`, `play_fall`), `cam.rs:87-89`, `controls.rs:243`; timers `vitals.rs:148-165` |
| `OnHit` -> `DoLogicalDamage` -> `AssessHit` | YES | sim `hit()` `sim.rs:604`, ported rules |
| fall damage / `OnMovementModeChanged` | NO | `PHYSICS_MOVEMENT.md` section 4: "Not wired: fall damage" |
| NPC attack timing: hit windows, stamina, reach, whiff | YES | `ai_exec.rs:440-470`, `sim.rs:1141-1165` |
| NPC attack animation per move | PARTIAL | `events.rs:343-351, 371`: always `queue_swing` = Attack Raise + Hit regardless of the move |
| NPC parade pose / parry visual | NO | no overlay for NPCs in `Action::Parade` |
| NPC stumble / knockdown / get-up visuals by `HitResult` | PARTIAL | only QuickStumble and `play_fall` for dead/defeated |
| music situation on fight (`HandleMusicSituation`) | YES | `events.rs:355-361` |
| NPC archers / casters: aim point 120 cm above target origin, line flight, Raise/Hit/Recover on the launch event | YES | `launch::npc_aim_point`, `play/ranged.rs:517-545` |

---------------------------------------------------------------------------------------------------

## 5. Ad-hoc glue that overrides an original mechanism (to remove while wiring)

1. `hero.rs:1026-1032` quarter turn of the hero model while aiming (replaces the engine rotation and the real aim-pose offset).
2. `hero.rs:824-847` `nearest_in_front` (melee target) instead of the focus entity + `CurrentTarget` of `PS_Melee_Loop`.
3. `hero.rs:686-722` `melee_swing` hand-picks clip names; the hit is immediate (`script_player.rs:545-552`).
4. `hero.rs:531-535` `JUMP_BACK` mapped to the evade roll (`dodge_motion`, invented 280 cm / 0.6 s).
5. `cam.rs:86-110` camera mode from weapon mode instead of the state's `SetMode`.
6. `play/anim.rs` constant overlay rate (no S:42a0 multipliers).
7. `normal.rs:218-227` `back_to_loop` returns a ranged hero to the AimLoop (port bug visible through `ranged_state`).
8. `hero.rs:169-172` focus-NPC facing overrides camera facing for bow and spells.
