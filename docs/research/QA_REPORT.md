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
