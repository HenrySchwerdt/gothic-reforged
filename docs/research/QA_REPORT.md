# QA report: input-level playtests of `--play`

Harness: `gothic-reforged --play --qa <scenario>` (`apps/game/src/play/qa/`). It injects the
same Bevy messages the OS backend produces (`KeyboardInput`, `MouseButtonInput`, `MouseMotion`,
`CursorMoved`, and the `WindowEvent` wrappers that `bevy_picking` reads), so the real hotkeys,
the hero input system and the UI picking run unmodified. Assertions read the core state
(`CoreRes`), the UI state and the interaction state. Output per scenario:
`.local/qa/<scenario>/NN_<step>.png`, `result.txt`, and the process exit code (0 PASS, 1 FAIL);
the last stdout line is `QA RESULT <scenario> PASS|FAIL`. `.local/qa_run.sh <scenario>` runs one,
`.local/qa_all.sh a b c` a list (gitignored helper scripts, build: `.local/qa_build.sh`).

What is injected and what is set up directly: all movement, aiming (mouse motion), menu / page
clicks, dialog selection (arrow keys + Enter), attacks (left / right button), dodge (double tap),
save / load keys are real input. Direct state changes are limited to marked `setup:` steps
(teleports, granting gold / items / a perk the quests or trainers would give, binding a spell
rune to a quick slot, raising the trade request that a `DoTrade` dialog command raises) and are
logged as `note: setup: ...`.

## Results (final run)

| Scenario | Result | Bug found | Fix |
| --- | --- | --- | --- |
| (all) startup | FAIL -> fixed | `--play` / `--smoke` stalled in "Loading" for 400 s with 30 of 99 characters spawned: flyers (`Seagull_SkyFlock`, 69 characters) hover far above any collision so `spawn_npcs` never found "ground below" and the readiness check waited for them forever | `npcs.rs`: characters with a resident tile and nothing within reach below spawn at their original height; `pending_in_ring()` helper so readiness only waits for characters inside the streamed ring. All 99 spawn by 5 s, ready at ~8 s incl. the 3 s settle |
| startup_sanity | PASS | The player's own `PC_Hero` template entity was spawned as a roster character, a second body exactly on the hero (visible as two fused humans in every screenshot, 99 actors instead of 98) | `npcs.rs` `select_roster`: skip `PC_Hero` |
| startup_sanity | PASS | (check) nobody targets the hero and hp stays 200 for 30 s; 86 creatures within 100 m, 16 in the 90 degree view cone within 60 m (6 raiders within 25 m); original nav graph (30 zones, 28 corridors) loaded and routes hero -> Hamlar | nav graph wired in `nav.rs` (worker thread, `SharedNav::set_graph`, origin-mirrored map) |
| menu_save_load | PASS | none in the game (harness: first clicks did nothing because picking reads `WindowEvent`, fixed in the harness) | Esc -> menu -> Spiel speichern -> Neuer Spielstand -> Speichern; state changed; Esc -> Spiel laden -> slot row -> Laden; gold / items / quests / hero pose equal to the saved state. The title menu has no "quit to menu" entry (play starts a new game directly) |
| trade_click | PASS | Hamlar's start dialog has no trade entry and the rebel traders are story-disabled at new game, so no merchant can be reached by dialog | trade page opened through the same host request a `DoTrade` command raises (marked setup); then real clicks on a vendor cell, "Gold-Ausgleich", "Tauschen": gold 623 -> 443, +1 stack; Esc closes |
| trade_esc | PASS | one earlier run left `Screen::Trade` up with the window gone after Enter + Esc (not reproducible again) | `windows.rs show_pages`: a Loot / Recipe / Trade screen without a window for 3 frames is closed by `ShowPage(None)` (the `page_shown` flag could be cleared by the UI's own Esc path) |
| loot_chest | PASS | `Screen::Loot` stayed up after Esc in 2 of 6 runs (same race as above) | same fix |
| craft_anvil | PASS | `Screen::Recipe` stayed up after Esc once (same race) | same fix; click "Herstellen" crafts (`Crafted` event) |
| dialog_quest | PASS (flaky walk) | none in the game; the harness walk to a second NPC in the crowd failed twice (hero blocked by NPC bodies, harness has no local avoidance) so only Diego is used | Diego dialog by arrow keys + Enter; a quest is running afterwards (0 -> 1), journal opens with L and closes with L |
| ui_screens | PASS | `gui_open` of the combat context was never set for I / C / L / menu: the hero could be hurt behind an open page (`Command::GuiOpen` was only sent by trade / loot windows). Also the mouse cursor stayed grabbed and hidden while I / C / L were open, so those pages could not be clicked | `hud.rs sync_gui_open` sends `GuiOpen` on every open / close; `windows.rs hold_cursor` frees the cursor for any open screen |
| character page (teach) | PASS | every attribute and protection value on the character page was 0 (the page reads `GameState.attributes`, the live values are on the hero actor) | `events.rs mirror_attributes` writes str / hnt / int / smt / thf / alc / prot_* from the hero actor |
| combat_basic | PASS | the dodge (double tap of a direction key) sent `Command::Dodge` but nothing moved or animated the hero | `hero.rs dodge_motion`: 280 cm slide in 0.6 s in the dodge direction through the controller (plays `Evade<Dir>` on the overlay when the animation set has such a clip, none is known) |
| combat_basic | PASS | note: a level-1 hero is down to 1 hp within ~10 s against one raider warrior (4 to 9 hits on the hero, 3 hits on the orc for 3 to 9 clicks) | not a bug by itself (raiders are meant to be avoided at game start); tracked, see below |
| magic_bow | PASS | none | quick slot 6 fireball (setup bind), hold / release left: missile; quick slot 3: bow readied, hold 1.2 s, release: arrow |
| swim | PASS | none functionally; visual: hard-edged light polygons on the seabed through the water, hero swims with a static spread-arms pose | not fixed (renderer / animation) |
| routine_reentry | PASS (weak) | tracked characters stay alive and on their feet after the hero leaves 180 m for 25 s and comes back; entity transforms equal the sim positions (worst 0.00 m). No routine change was exercised: 25 s are 5 game minutes and routines switch on the hour | needs a clock-skip command to test a real routine hand-over |
| lockpick | PASS | a refused interaction (`MissingPerk`) left `Interaction::active` set | `interact.rs`: `Refused` clears `active` when nothing was started; with `Perk_LockPick_1` the chest opens (`LockPicked`, loot window) |
| sit_pose | PASS | none by state (`holding_pose`, released by W); visual check inconclusive (camera inside the bench) | |
| quick_slots | PASS | none | F5 writes `quick.g3rs`, F9 restores it; F6 writes a slot, F10 loads the newest |
| teach | PASS | Hamlar's combat teaching entry leads to a submenu with only "...zurueck" (nothing to buy at level 0, as the learning points are 0 before the setup) | |
| doors | N/A | there is nothing to wire: the world has 6 `gCDoor_PS` / `gCMover_PS` entities in total (28770 interactive entities scanned with `object-dump --radius 900000`), the nearest 711 m from the start (`G3_Object_Interact_Animated_Door_01`), none in Ardea. The mover state machine stays untested in the running game | none |
| autosave | n/a | the autosave toast read "Gespeichert / geladen: .local/saves\auto_1.g3rs" over the pages | `events.rs`: autosaves are silent, other saves say "Spiel gespeichert" |

## magic_bow and basic controls (observations, no redesign)

The mechanics fire (fireball: `FxLog.missiles` 0 -> 1 after quick slot 6 + hold/release left;
bow: quick slot 3, hold 1.2 s, release: `arrows` 0 -> 1, quiver 50 -> 49), but what is seen is
wrong and matches the user's "cursed" report:
* after the shot the hero's hand keeps glowing (the spell glow is never removed when the bow is
  put up) and no bow mesh is visible in the left hand (`.local/qa/magic_bow/02_arrow.png`);
* the camera stays in the plain follow mode while aiming (no zoom / over-the-shoulder aim), the
  reticle is absent, and the shot goes along the hero's facing, not the screen centre;
* attack timing: a click shorter than 0.2 s is a quick attack, but the swing is queued when the
  button is *released*, so the hit lands about one click-length plus the swing time late;
  clicking every 1 s gave 3 hits on the orc for 3 to 9 clicks, the orc's 4 to 9 hits took the
  hero from 200 to 1 hp;
* turning: the camera owns yaw (mouse), the hero body turns after it at the ported turn rate,
  so quick 180 degree mouse moves leave him facing away for a moment and melee (which uses the
  body heading, `hero_yaw`) misses; WASD is camera relative as in the original;
* the double-tap dodge (`[U]`, no original key) is easy to trigger by accident when strafing.
* `--combat souls` (`souls_basic`): draw with quick slot 1, Space rolls (340 cm, PASS), a held
  left button lands a light attack (orc 600 -> 593, PASS), Tab does not visibly lock on in
  `Player.face` (gate active, weapon drawn; not verified further, owner to check).

## Remaining known bugs / gaps

* New game shows `Stufe 0` on the character page and in save captions (the original starts at
  a level; `GameState.level` is 0 in the new-game state).
* Hero is down to 1 hp quickly against one raider warrior; combat balance at level 1 and the
  parry (hold right) effect have no assertion yet (only "no stuck state").
* `Evade<Dir>` clips: the dodge plays no animation (no clip name known in the motion catalog).
* The mouse look still leaks up to ~0.03 rad during the frames in which a full screen opens.
* Trading with a merchant by dialog is untestable at game start (rebel traders are
  story-disabled; Hamlar offers none).
* Interaction poses (bench / bed) are asserted by state only; seat height / alignment is not.
* Routine hand-over (work -> relax -> sleep points on the hour) is not covered: needs a
  game-clock skip command. NPC routines and chases use graph paths only through
  `SharedNav::path`; there is no counter that proves a route came from the original graph
  rather than the grid fallback.
* `--smoke` (the older showcase script) still uses the same loading condition; it benefits from
  the same fix but was not re-run.
* Swimming art: faceted seabed through the water; static swim pose.
* Flaky under load (other builds running on the machine): `combat_basic` dodge check (4 cm
  moved once, stamina likely spent by the preceding parry), `dialog_quest` NPC walks.
* Magic: the spell book (B) and documents (M) screens do not exist, so spells can only be bound
  by the harness' `setup` step.

## Open world (2026-10-21): scenarios `sector_switch`, `teleport`, `walk_to_reddock`, `triggers`, `world_walk`, `perf_idle`

Run with `.local/ow_run.sh <scenario>...` (same binary, `GR_NOVSYNC=1` for unthrottled frame times).

| Scenario | Result | What it proves / bug found |
| --- | --- | --- |
| sector_switch | PASS | New game: `Ardea_NPC_01` layer shown (24 bodies), `NPC_02` none. The info `FILLER938` (owner Javier, `SetSectorStatus G3_Myrtana_01_Ardea_NPC_02 true` and `..._NPC_01 false`) is executed through `gothic_gameplay::script::execute_info`: both `SectorSwitched` events arrive, NPC_01 bodies leave, 37 NPC_02 bodies appear; clearing `state.sectors` (older save) restores the authored layers. **Bug found and fixed**: the script spelling `G3_Myrtana_01_Ardea_NPC_02` and the registered directory name differ in case; `Core::sync_sectors` kept both as separate wanted entries and flipped the sector every tick (1000+ events per run). Names are lower-cased there now |
| teleport | PASS (frame times informational) | Teleporter stone `It_Teleport_Faring` in quick slot 7, select, hold / release left (cast): `Spl_Teleport_Faring` -> `MagicTeleport` -> `MagicEvent::Teleport` -> hero controller placed at `Teleport_Faring` (723 m away), the tiles stream in, 27 Faring bodies shown, Ardea's 18 gone |
| walk_to_reddock | PASS | Hero runs from Ardea to the densest group of Reddock characters (about 190 m, `walk_to` with detours; 9 m left at the end), 24 Reddock bodies shown, core actors `Brenton`, `Gelford`, `Javier` exist, Ardea bodies removed. Screenshots `01_ardea`, `02_reddock`, `03_reddock_market` |
| triggers | PASS | The hero capsule touches the nearest `gCTouchDamage_PS` trigger (a loose `It_1H_Sword_01`, 20 m away): `OnTouchDamage` fires once, a second visit is held back by `MaxCountPerEntity = 1`, the counters are in the core's save image (`TriggerSnapshot`) |
| world_walk | informational | Teleport hops Reddock, Kap Dun, Gotha, Montera with 25 s walks: 186 bodies created, 142 removed, 186 core actors at the end |
| perf_idle | informational | reference: standing in Ardea |

Frame times (1280x720, unthrottled, machine shared with compiling agents, so only ratios mean something):
standing in Ardea p50 <= 17 ms, p95 <= 22 ms (includes the 27 initial body builds); walking Ardea -> Reddock
p50 <= 10 ms, p95 <= 13 ms, p99 <= 20 ms over 9 600 frames (threshold of the scenario 24 ms for a loaded machine; the 12 ms goal
was missed by 1 ms on a loaded machine); the four-city hop run p95 <= 24 ms, p99 <= 53 ms. The residual
hitches are cold body builds: `ActorSpawner::spawn` decodes mesh, skeleton and clips on the main thread, avg
4 ms, up to 40-80 ms for a look that was never built. The roster systems themselves are cheap (candidate scan
avg 25-35 us, max 0.4 ms); the spawn pass is rate-limited (3 ms budget, after a pass over 3 ms the next waits
twice as long). A background asset-build thread in `gothic-animation` would remove the rest.

Findings:
* World content: 10 253 characters in 68 layers, 9 692 in enabled layers at the new game. Characters are
  created in the core when they first enter the 4000 box around the hero and then stay (position, health,
  inventory); only the body is dropped beyond 4250.
* The shipped world documents contain **no** `eCTrigger_PS` / `eCTriggerList_PS` / `eCEventFilter_PS` /
  `gCEventScript_PS` property sets (scan of 2 517 parsed documents, `ow_scan`). The only trigger-family set is
  `gCTouchDamage_PS` on 698 loose weapons / arrows with the script function `OnTouchDamage`, which is not
  ported (no `OnTouchDamage` in `gothic-script`): the trigger fires and is recorded, no damage follows.
  The shape is approximated by a 60 cm sphere (no shape data in the documents).
* The hero is moved by the host (`HeroTransform` every frame), so a teleport must move the controller; the
  core's own `teleport` only moves party members (waypoints `Teleport_<City>` are registered with the core).
* Not covered: routine catch-up when a frozen character re-enters the range (see PORT_NOTES, simulation),
  quest NPCs far away exist in the core only after the hero came near them once.

## Input, bow and magic wiring (engine ports E50..E52 in `--play`)

Wired: `crates/input` (Controls=Standard table, mapper timings, key router, `UserOptions.ini`
`[Options.Controls]` via `gothic_ui::ControlSettings`, `ApplyControls` rebuilds the mapper),
`crates/projectile` (shots through `Sim::shot_views`, `set_hero_aim`), `crates/camera` aim pick.
Debug switches: `GR_INPUT_DEBUG=1` (messages and orders), `GR_RANGED_DEBUG=1` (hero state
changes, shot spawn / km/h / impact / gone).

| Scenario | Result | Findings / fixes |
| --- | --- | --- |
| controls_basic | PASS | Q / E turn the camera only (49 deg camera, 0 deg body in 0.6 s), Shift walks (run 400, walk 150 cm/s), a double tap is no dodge, the left button without a weapon is the use key and attacks nothing |
| magic_bow | PASS (counters and projected-position checks) | Bugs fixed on the way: key `6` is `Quick6` = slot 6 (`QuickUse n` is named after its key, key 0 = slot 10); the click that captures the cursor was also a game click; `aim_at` of the harness oscillated (gain); the sim held every hero action for a placeholder second (`AiCommand::Action secs 1`): the arrow left 1 s after the release, the missile 1 s after the key. Now the raise clip lengths of `projectile::aim::clips` (Aim 0.4 s, cast 0.8 / quick 0.16 / power 0.92 s), the shot spawns at once and recovery follows (0.4 s bow, 0.6 s crossbow, Cast_Hit + Cast_Recover); hand glow: `StopEffects` now ends every emitter of the effect (looping glow stayed after the spell was put away); shots are drawn from `shot_views` every frame (spell: `..._projectile_01` effect on the moving entity, arrow: `Arrow` mesh), impacts start `..._impact_01` / target effects and sounds |
| shaman_duel | see the log | orc shaman from the world data, hero with the bow; prints state / mana every 10 s |
| hud_focus | see the log | focus label (`gothic_ui::FocusLabel`) |

Visual assertions (harness `visual_watch`): projected shot positions inside the screen during
flight (fireball 7 frames at 7 m, arrow while flying and stuck), the hero's body parts visible in
every aim / cast frame, the bow actor visible inside the screen. Open: bow actor orientation /
offset at `Slot_LeftHand_Weapon`, crosshair texture (`G3_HUD_CrosssHair.tga`), camera numbers
and clip names of the aim, to be set from the capture of the original (`.local/ref/ranged_magic`).

### Decompile / trace findings used for the aim (2026-10-21)

* `PS_Ranged` (Script_Game 0x100837e0) calls `PSCamera::EnableSpecialBowMode`; the runtime trace
  of the original (`.local/ref/ranged_magic/trace*.jsonl`, `Cam.SpecialBow`) shows it is always
  called with `on = 0` and no other function of Game.dll or the script calls it: the special bow
  mode is never switched on, the hero stays visible while aiming (the earlier port note was
  wrong). `play/cam.rs` keeps it off (`SPECIAL_BOW_HIDES_HERO`).
* Camera: `PS_Ranged` -> `SetMode(Ranged)`, trace `req=2`: wished distance 300, fov 60.6.
* Crosshair: `PS_Ranged_Aim` -> `GUI2::ShowCrossHair(true)` (fade 200 ms,
  `AnimateWindow(AW_BLEND)`, Game 0x1007d970), `gCHUDCrossHair` is a 32x32 static (minimum 8x8)
  with `G3_HUD_CrosssHair.tga` (`FUN_100a6b30`), `SetCrossHairSize` resizes and centres it
  (`FUN_100a6bf0`); the tile is `g3_hud_icons_00` rect 405,138 40x40 of `g3.igl`.
* Motions: `Hero_Stand_Bow_Arrow_P0_Hold_Begin/Hold_End_N_Fwd` on drawing the bow
  (`HoldRight_*` for melee weapons), aim clips `..._Aim_Raise_O_Fwd` (slot 4) and `_O_Down`
  (slot 5), as ported.
* Slots: `gCInventorySlot::GetBone` (ported in `gameplay::inventory::slot`): bow = left hand
  (`Slot_LeftHand_Weapon`, back `Slot_Bow`), arrow = right hand; the script holds a spell rune in
  the RIGHT-hand stack (`PS_Magic`: `HoldInventoryStacks{right, left: -1}`, `cast.rs` reads
  `Slot::RightHand`). The user remembers the cast from the left hand; the decompiled script says
  right. Not changed; to be decided with the animation data (which arm the `Cast_*` clips move).
* Spell targets: spells with `SpellTarget = focus` take the focus entity (`PS_Magic_Loop`), all
  others fly free along the forward axis; the host now passes the focus entity (not the nearest
  character) to `CastSpell` / `ReleaseRanged`.
* Open: the traces never reached an aim (the hero was knocked out); the projectile spawn bone,
  aim camera offsets and the bow actor's slot offset (now a quarter turn about X, chosen by
  `bow_pose` against the four axis turns [I]) are not verified against the original.

### Cast hand, spawn point, perf (follow-up)

* Cast clip data (`crates/animation/examples/cast_probe.rs`, rotation travel of the arm chains):
  `Hero_Stand_None_Cast_P0_Cast_Raise` (0.80 s) moves the left arm 609 deg and the right 173 deg;
  `Cast_Hit` (0.28 s) the right arm 325 deg and the left 159 deg; `Cast_Recover` left 492 / right 374.
  The stance token is `None_Cast` (left hand none, right hand cast), the rune sits in the right-hand
  slot (`PS_Magic`), the `eff_magic_*` effects link to the owner entity (`TargetEntity`, no bone in
  g3.efm). So the wind-up is left-hand dominated and the throw (`Hit`, when the missile leaves) is the
  right hand: the missile now starts at `Slot_RightHand_Weapon` (`Sim::set_hero_hand`).
* Perf: per-frame writes of change-detected resources removed (`CoreRes` for the aim, `PlayerHeading`
  / `MapData` of the UI feed, crosshair UI nodes while hidden, bow visibility). Not measured with the
  `trace` feature here.
* YouTube reference: `yt-dlp` is not installed and was not installed; the only original reference is
  the user's screenshot (`.local/ref/orig-bow-aim-user.png`), compared in `.local/qa/compare_ranged_still.png`.
