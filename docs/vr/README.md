# VR mode (PC VR first)

Issues: #8 (epic), #2 VR-01, #3 VR-02, #4 VR-03, #5 VR-04, #6 VR-05, #7 VR-06, #20 VR-09, #46 VR-10, #47 VR-11, #48 VR-12.

Everything here is developed and tested with **synthetic content only** (greybox boxes,
capsule dummies, procedural weapons). No Gothic 3 assets, data or decompiled code are used.
Integration with the real game data (skeleton mapping, item stats, the damage formula) is a
local maintainer step; the hooks for it are listed below.

## Crates

| Crate | Issue | Contents |
| --- | --- | --- |
| `crates/vr` (`gothic-vr`) | VR-01, VR-02 | XR input model (head, two controllers, buttons, analog hysteresis), backend seam, desktop fallback (keyboard/mouse headset emulator, input scripts), stereo rig, controller buttons → core `Command`s, comfort locomotion driving `gothic_physics::CharacterController`, full-body IK, greybox level, debug rendering |
| `crates/vr-physics` (`gothic-vr-physics`) | VR-03 | avian3d 0.8, physics hands (strength-limited PD force/torque), grip points, two-handed grabs, throwing, procedural props, headless test rig |
| `crates/vr-melee` (`gothic-vr-melee`) | VR-04 | swept blade/blunt/pierce contact model, stabs (stick/pull-out), block/parry, `HitEvent`, hitzones, training dummy |
| `crates/vr-npc` (`gothic-vr-npc`) | VR-05 | active ragdoll (balance, stagger, fall, get-up), combat AI (telegraphs, attack slots, blocking, recoil), Bevy plugin |
| `crates/vr-interact` (`gothic-vr-interact`) | VR-11 | point / grab / lockpick / drink / spell ring + casting / bow / crouch / dodge / hop mapped onto the core's `Command`s, `CoreApi` + stub core, Bevy plugin |
| `crates/vr-ui` (`gothic-vr-ui`) | VR-10 | world-space HUD over `gothic-ui`'s page state: wrist vitals, quick-slot wheel, backpack inventory, dialog options beside the NPC, journal book, held map, world-locked subtitles; comfort rules; drawn panels (UI to texture) and a gizmo greybox view |
| `crates/vr-bridge` (`gothic-vr-bridge`) | VR-09 | adapters to `gothic-physics` / `gothic-runtime`: units, level chunks as avian trimeshes, runtime actors as melee proxies, hits as `Command::Attack`, 30 Hz core ticking beside 90 Hz physics (see `PHYSICS.md`) |
| `crates/vr-archery` (`gothic-vr-archery`) | VR-06 | quiver/nock/draw/release, ballistic arrows that stick, aim assist, damage hook |
| `apps/vr-sandbox` (`gothic-vr-sandbox`) | all | greybox level with everything above |

The mechanics are Bevy-free modules (`input`, `desktop`, `locomotion`, `body`, `pd`, `grab`,
`contact`, `ragdoll`, `ai`, `bow`, `ballistics`) with thin Bevy/avian plugins on top, per
`docs/ARCHITECTURE.md`. Units: metres in tracking space, the Bevy world and avian; the Gothic
character controller and its `CollisionWorld` stay in centimetres (`gothic_vr::CM_PER_M`).

## Running

```sh
cargo run -p gothic-vr --example greybox                 # VR-01/02 demo, desktop fallback
cargo run -p gothic-vr --example greybox -- --stereo     # side-by-side eye views
cargo run -p gothic-vr --example greybox -- --headless   # scripted CI run (exit code)
cargo run -p gothic-vr-sandbox                           # everything: props, dummies, duel, range
cargo run -p gothic-vr-sandbox -- --peaceful --strength 120
cargo run -p gothic-vr-sandbox -- --headless             # scripted CI duel (exit code)
cargo run -p gothic-vr-npc --example exec_duel           # VR-12: original-AI intents vs scripted player

cargo run -p gothic-vr-interact --example interact_greybox # VR-11: every mapping against a stub core (exit code)
cargo run -p gothic-vr-ui --example hud_greybox          # VR-10 HUD with fake data (--headless: exit code)
```

On Linux the windowed builds use X11 (`libxkbcommon-x11` is needed at runtime). Without a GPU,
Mesa's lavapipe works (`VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json`, e.g. under
`xvfb-run`); `--screenshot out.png [--screenshot-frame N]` captures a frame and exits.

### Desktop debug controls

| Input | Emulates |
| --- | --- |
| W A S D | left thumbstick (move, head-relative) |
| Q / E | right thumbstick X (snap turn 30°) |
| T (hold, release) | right thumbstick forward: teleport arc, release to jump |
| right mouse + move | head yaw/pitch |
| mouse move | active hand in the view plane — fast mouse moves are sword swings |
| mouse wheel | active hand depth (pull back toward / over the shoulder) |
| Tab | switch the active hand |
| G / F | toggle grip of the active / other hand |
| left mouse | trigger of the active hand |
| Space / R | A (jump) / B (draw weapon command) |
| Left Shift | left stick click (sprint) |
| C | crouch (lowers the head: sneaking) |
| Esc | menu button |
| sandbox: 1 / 2 | active hand to the quiver / right hand to the bow string |
| sandbox: H, F1 | NPC hitzones, stereo |

### With a headset

Not possible in this build yet: no OpenXR integration for Bevy 0.20 has been released
(`bevy_mod_openxr` 0.6 targets Bevy 0.19). `GOTHIC_VR_BACKEND=openxr` is recognised and logs
that it falls back to the desktop backend. The backend seam is ready: a headset backend only
writes `gothic_vr::PendingXrFrame` (head + controllers in tracking space) before
`VrSet::Input` and drives the `XrEyeCamera`s' projection/targets. Details and the steps to
enable it in `docs/vr/OPENXR.md`.

## Tests

```sh
cargo test -p gothic-vr -p gothic-vr-physics -p gothic-vr-melee -p gothic-vr-npc -p gothic-vr-archery -p gothic-vr-interact -p gothic-vr-ui -p gothic-vr-bridge
```

All tests are headless and deterministic (fixed 90 Hz steps; avian runs inside a
`MinimalPlugins` app via `gothic_vr_physics::testing::Rig`). In CI they run in the separate
`vr` job on Windows and Linux, together with the two headless demos; see `docs/CI.md` for
costs and caching.

| Area | Tests |
| --- | --- |
| input / desktop / bindings / rig | hysteresis, edges, sanitising NaN, emulator reach, scripts, commands, eye poses |
| locomotion (greybox stairs/slopes) | head/hand-relative, walk/run/sprint, snap pivot about the head, smooth turn + vignette, crouch → sneak, seated lift, stairs, 25° vs 60° ramp, wall pushes the view back, teleport valid/invalid, determinism |
| body IK | recorded pose fixtures (`crates/vr/tests/fixtures/poses`): bone lengths, reach, feet on the floor, one foot at a time, idle look-around, turn in place, crouch, walking, reach/swing |
| physics hands | PD follow, walls stop the hand, grab/snap/carry, lag vs weight and strength, two hands, throw, stuck-hand snap, determinism |
| melee | fast swing cuts, slow swing harmless, waggle, flat = blunt, mace, no tunnelling, self-hits, re-hit cooldown, stab sticks / wiggle / pulls out, slow poke, block protects, counter-swing parries, determinism; plus physics hand + sword vs dummy |
| NPC | ragdoll stands, collapses, light hit staggers and recovers, heavy hit fells and gets up, push and grab, hitzones; AI telegraphs on the right side, edge-first strikes, attack slots, distance, blocking, recoil, disabled when down; headless duel (NPC hits player, player hit staggers NPC, overhead blocked → recoil) |
| execution layer (VR-12) | sim action numbering → moves, commanded humanoid attacks only on an order, Parade → guard, attack slots (3 attackers, 2 slots), quadruped bite → `AiHitEvent` with the original action, guard in the path → blocked + recoil, humanoid clash → recoil, melee hit carries the action (once per attack), knockdown → get up |
| archery | quiver over the shoulder only, nocking, draw power, small draw drops, off-axis loses the arrow, damage hook, ballistic range, drag, orientation, wall sticking, ricochet, body hit + carried along, no tunnelling, aim assist, determinism; physics range (10 m hit at full draw, half draw falls short) |
| interaction mapping (VR-11) | point + trigger talks (`Confirm`) and misses off-axis, recipe board crafts, grab picks up / loots / opens by the handle / unlocks with the key, lockpick combination opens, neutral needed between turns, wrong turn breaks, no combination lets the script roll, no pick / letting go aborts, potion at the mouth used once, spell ring selection, cast aimed from the hand, power cast on a long hold, grip aborts, bow ready / aim / shoot at the aimed NPC, easing the string lowers it, real crouch toggles sneak, steps dodge in the head frame, hop jumps, menu blocks, stub core answers, plugin sends `CoreCommand` |
| HUD (VR-10) | dmm sizes, distance clamp, upright facing, comfort cone; panel laser / touch / back-face, rows, grid, wrapping; wrist shows only turned to the eyes and readable, bar fills, low-health pulse; wheel direction, deadzone, release, belt; backpack over the shoulder, touch distance, cell size, items, paging; dialog beside the NPC at laser distance, row size, laser select + confirm, scroll, touch, close NPC pushed out; subtitles above the speaker at body text size, off-view fallback with side, narrator; book pages and swipes, page text size; map markers, hidden / outside markers, laser labels; HUD: backpack page + one press per touch + laser, dialog page hides the vitals and closes, one row per flick, wheel uses filled slots only, journal book in the off hand, plugin messages |

## Integration hooks for the real game

* **Damage**: melee produces `gothic_vr_melee::HitEvent` / `HitMessage`
  `{ attacker, victim, bone, damage_type (Edge/Blunt/Point), impact_speed, point, direction, quality }`;
  archery additionally `ArrowHitMessage { damage }` from the `ArrowDamage` trait (default
  `(ammo_damage + bow_damage) * draw_factor`). Feed these into the original damage formula.
* **Strength**: `gothic_vr_physics::VrStrength` (Gothic `STR`) limits arm force and torque
  (`pd::arm_limits`).
* **Skeleton**: `gothic_vr::body::Joint` / `gothic_vr_melee::Bone` are a generic humanoid;
  map them to Gothic skeleton bones for rendering, hitzones and ragdoll proportions.
* **Commands**: `gothic_vr::VrCommands` maps buttons to `gothic_runtime::Command`s and can
  send `HeroTransform` every frame (`send_hero_transform`).
* **Weapons**: replace `props::*` specs and `profiles::profile_for` with data from the item
  templates (mass, length, grip, blade/point geometry).

## Execution layer for the original AI (VR-12)

`gothic_vr_npc::exec` keeps the original AI in charge of **when**, **whom** and **which move**
and only decides **how** the move is executed physically. The host feeds one `AiIntent
{ target, action }` per decision (`AiAction::from_sim` takes the `gothic_sim::types::Action`
value) into an `Executor` per NPC:

| Original action | Humanoid (`BodyKind::Humanoid`) | Quadruped |
| --- | --- | --- |
| Attack, QuickAttack, GetUpAttack | `Swing` alternating right/left | `Bite` |
| QuickAttackR / QuickAttackL | `Swing(FromRight / FromLeft)` | `Bite` |
| PowerAttack, HackAttack, JumpAttack, FinishingAttack | `Swing(Overhead)` | `Lunge` (Hack, Finishing: `Bite`) |
| PierceAttack, SprintAttack, RamAttack | `Swing(Thrust)` | `Lunge` |
| SimpleWhirl, WhirlAttack | `Swing` alternating | `Swipe` |
| Parade | guard (`Phase::Block`) | – |

* Every attack is telegraphed (`ExecEvent::Telegraph { vr_move, lead }`) and needs an attack
  slot (`AttackSlots`, 1–2 per target); without one the NPC waits (`WaitingForSlot`).
* Humanoids run the existing `CombatAi` in commanded mode (`autonomous = false`, `order`), so
  the active ragdoll and the physical weapon work as before; the host forwards the melee
  model's contacts: `Executor::on_melee_hit(..) -> Option<AiHitEvent>` (once per attack) and
  `Executor::on_clash(recoil)` (block / parry → recoil).
* Quadrupeds (`Species::is_humanoid() == false`) use a kinematic `QuadBody`: the mouth or paw
  sweeps against the target's hitzones and guard blade (`TargetBody`), a guard in the path
  blocks and the animal recoils; `on_hit_taken(true, ..)` knocks it down and it gets up.
* `AiHitEvent { attacker, victim, action, vr_move, bone, damage_type, point, direction,
  impact_speed }` carries the original action, so the original damage formula can apply
  its move strength (quick attacks, power attacks, ...). Nothing here computes damage.

Not done yet (needs the game): reading the intents from `gothic-sim` per frame, mapping
entity ids, the Bevy plugin wiring of `Executor` next to `VrNpc`, and a ragdoll body for
quadrupeds (the kinematic body is a stand-in).

## Interaction mapping onto core commands (VR-11)

`gothic_vr_interact::InteractMapper` turns tracked poses and buttons into the same
`gothic_runtime::Command`s the keyboard host sends; the core stays the only authority over
reach, skills, locks, mana and inventory.

| VR action | Command |
| --- | --- |
| point at a character / object / item + trigger (main hand) | `Confirm { focus }` (actor `n` → `n + 1`) |
| point at a recipe on a work station's board + trigger | `Craft { recipe }` |
| grab an item / a body | `PickUp { item }` / `QuickLoot { body }` |
| grab a door or chest handle | `UseInteractObject { object }` |
| grab a locked handle with the key | `Lockpick { object, success: None }` |
| grab a locked handle with a lockpick, roll the wrist left/right (back to neutral between turns) | `Lockpick { success: Some(true) }` for the combination, `Some(false)` on a wrong turn, `None` on the first turn when the host has no combination |
| hold a consumable at the mouth for 0.4 s | `UseItem { template }` |
| hold the off hand's secondary button, put the main hand at a slot of the ring, release | `SelectSpell { template }` |
| spell in hand: trigger tap / hold ≥ 0.8 s, aimed with the hand | `CastSpell { target, power }`; grip during the charge: `AbortCast` |
| bow equipped: off-hand grip / nock with the main grip at the bow and draw ≥ 35 cm / let go | `ReadyRanged` / `AimRanged { aim: true }` / `ReleaseRanged { target }`; easing the draw below 25 cm: `AimRanged { aim: false }` |
| head below 72 % / above 82 % of the standing height | `ToggleSneak` |
| quick head step (> 2 m/s) | `Dodge { direction }` (head frame) |
| head rising > 1.6 m/s | `Jump` (the button binding stays) |

Integration hooks:

* `VrInteractPlugin` (after `VrPlugin`) reads `VrInputRes`, applies `VrPlayer`'s locomotion
  offset and writes `CoreCommand` messages next to the button mapper.
* The host fills `InteractTargets` with what is near the hero: NPCs from the actor list,
  items and objects with the ids from `Event::ItemRegistered` / `ObjectRegistered`, lock data
  (`LockSpec`: locked, key, combination if known), recipe board slots of the active work
  station. `InteractHero` carries lockpick count, quick-slot spells, the equipped bow, the
  consumables in the physics hands and whether a window is open.
* `CoreApi` is implemented for the real `gothic_runtime::Core` and for `StubCore` (demo and
  tests only; it does not model the original rules).
* The host corrects `InteractMapper::sneaking` if sneak is toggled another way.

Not done (needs the game): the target registry from real world entities, highlighting the
focused target, haptics, and the physics hands holding the grabbed item instead of `PickUp`
moving it straight into the inventory.
## World-space HUD (VR-10)

`gothic_vr_ui::VrHud` turns the flat UI's state into diegetic world-space parts. Which parts
are up follows `gothic_ui::GuiManager` (page stack, `HudFlags`): the dialog page hides the
vitals like the flat HUD hides the bars, the inventory page is the backpack, the quest page
is the book. Data comes in as the same view models the flat UI uses (`HudData`:
`QuickSlot`s, `PlayerPanel`, `OptionList` via the dialog, `Subtitle`, `QuestRow`s,
`MapSheet` / `MapMarker`s); actions go out as `HudAction` (`UseQuickSlot`, `InventoryItem`,
`ChooseOption`, `Page`).

| Part | Where / how |
| --- | --- |
| vitals | inside of the off-hand wrist, shown when turned to the eyes (≤ 40°, readable size) and the page allows bars; low health pulses |
| quick slots | hold the off hand's X/A: ring of the slots (12 cm) at the main hand facing the eyes, slot 0 on top, release on a filled slot; pressed at the hip it is the belt |
| inventory | main-hand grip over the shoulder opens a 5×4 grid (6 cm cells) 55 cm in front, 25° below the eye line; fingertip press (once per touch) or laser + trigger; paging strip; over the shoulder again closes |
| dialog | options panel beside the speaking NPC (clear of the body, pulled towards the player), 0.9–2.5 m, rows 36 dmm; laser + trigger or fingertip, thumbstick flicks scroll |
| subtitles | above the speaker's head facing the player at 20 dmm text, wrapped to 42 characters; speaker out of view: in front with a side marker; narrator: in front |
| journal | quest page: an open book in the off hand, swipe across the spine to turn the spread |
| map | a sheet in the off hand with the player and the markers (`calc_uv`), the laser shows labels |

Comfort defaults (`comfort.rs`, after Meta's VR UI guidance, tune on a headset): text ≥ 14
dmm (body 20), targets ≥ 22 dmm, touch panels 0.45–0.7 m, laser panels 0.9–2.5 m, content
within ±30° yaw, 15° up / 40° down, panels upright and facing the eyes, never head-locked
except the off-view subtitle fallback.

Integration hooks: add `VrUiPlugin` after `VrPlugin`; the host fills `HudDataRes` from the
same sources the flat UI reads, mirrors `HudAction::Page` into the flat UI / core GUI state
and executes the other actions (use item, choose option). `VrUiPlugin { gizmos: false }`
keeps only the logic.

Drawn panels: `VrUiRenderPlugin` (with `VrUiPlugin { gizmos: false }`) renders each part
as a Bevy UI tree into its own texture (offscreen camera, rebuilt only when its content
changes) on an unlit, alpha-blended quad at the part's panel pose. The look is synthetic:
dark leather panels with brass borders, parchment for the book and the map, coloured
vitals bars. Insert `HudFont(handle)` for text with umlauts (Bevy's built-in font has
none); the demo loads a system font (`--font <file>` to choose). `--dump-panels <dir>`
saves each panel's texture, `--screenshot` the whole view. Not done: the game's own UI
atlas, fonts and item icons (names stand in for icons), haptics, the trade / crafting
pages, and a check on a headset (texture resolution, legibility, comfort).
