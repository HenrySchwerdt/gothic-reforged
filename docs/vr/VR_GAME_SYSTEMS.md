# VR game systems: every Gothic 3 UI and system as a VR interaction

Goal: the whole game is playable in VR (Meta Quest, Bevy 0.20), not only combat. This file maps
each original UI / system (`docs/research/UI_ORIGINAL.md`, `GAME_RULES.md`, the desktop host in
`apps/game/src/play/*`, the core `Command` / `Event` API in `crates/runtime`) to a VR form, the
comfort rules behind it, what is implemented and tested, and how a host wires it in.

Hard rule: **no game logic in VR.** VR only issues the same `Command`s (or the same `Sim`
trade calls and `gothic-ui` `PageEvent`s the desktop host applies) and renders the same
`Event`s / view models. Prices, availability of dialog infos, quest state, recipes, lock rules,
reach and damage stay in the core.

## 1. Comfort rules (cited)

All numbers live in `crates/vr-ui/src/comfort.rs` and `views.rs`; they follow Meta's guidance,
verified against the Meta Horizon OS developer docs (via the `metavr` docs tools, 2026-10-27):

| Rule | Value used | Source |
| --- | --- | --- |
| Content the eyes fixate for long (menus, GUIs) at least 0.5 m away; 1 m is comfortable | windows at 1.25 m, book/letter 0.9 m | Display > Comfortable viewing distances (`design/display.md`) |
| Avoid traditional HUDs; put information in the environment / avatar (watch, map in hand); reticles drawn on the target; no overlay that is "behind" scene objects | wrist vitals, held map and book, subtitles and options world-locked next to the NPC, focus names above the NPC | Display > Displaying information in fully immersive experiences |
| Direct touch 42-46 cm; raycast 0.8-3 m; **avoid the 0.5-0.8 m middle distance**; show a mode indicator | pokeable panels (backpack) at 0.45 m (`comfort::TOUCH_DISTANCE` 0.40-0.46); windows and dialog at >= 0.9 m; a poke is accepted on windows too, so reaching works without a mode switch (hover look = gold row, poke = press; a distinct reticle for the laser is the Quest app's job) | Hands UI best practices; Spatial UI inside interactive 3D scenes (`design/hands-ui-best-practices.md`, `design/hands-3d-best-practices.md`) |
| Hit target >= 48 dp = about 22 mm at touch distance, >= 2.5-3 deg angular, 12 mm spacing | `MIN_TARGET_DMM = 22`, rows 38 dmm, buttons >= 60 dmm | Hands UI best practices > Sizing and density |
| Keep angular size when a panel changes distance | all sizes in dmm (`comfort::dmm`) | same, "Comfortable distance range" |
| No scroll bars; scroll by swipe anywhere on the content; show a clipped item as the cue | stick flick / index swipe on the list under the pointer, chevron + half-visible row | same, "UI scrolling" |
| **Do not anchor menus to the (moving) wrist**; spawning from the wrist is fine if the panel is then static in world space | vitals are a glance read-out (a watch, allowed); every *interactive* window (tablet, trade, loot, ...) spawns in front of the player and stays world-locked | same, "Do not anchor menus to the user's wrist" |
| Panels upright, face the user, roll fixed | `windows::place`, `comfort::facing` | Spatial UI inside interactive 3D scenes > Constrain panel rotation |
| Hands have no haptics: every poke / pinch / grab needs visual **and** audio feedback; different hover looks for grabbable 3D objects versus clickable UI | gold hover for UI rows, a different (outline) highlight for grabbable objects is the host's job; the host plays `sfx_inv_click_02`, `sfx_inv_deny_01` exactly as the desktop does | Hands UI best practices > Audio and visual feedback; role confusion |
| Critical elements in the immediate line of sight; comfort cone | windows centred on the head's horizontal heading, 12 cm below eye line | Display; `comfort::in_comfort_cone` |

Text: body 20 dmm, minimum 14 dmm, rows 38 dmm; German umlauts need the host's `HudFont`.

## 2. Interaction vocabulary

| Gesture | Meaning |
| --- | --- |
| Point + trigger (laser from the main hand) | talk, choose, press a UI row/button, pick up at range, confirm on the focus |
| Fingertip poke (only on panels inside arm's reach) | same as trigger on that panel |
| Grab (grip) | pick up, open a lid, take an item out of a chest, hold a book, hold the lockpick |
| Stick flick up/down on the pointing hand over a list | scroll that list; left/right turns document pages |
| Off-hand palm turned to the eyes | vitals (read-only watch) |
| Off-hand `X/A` held, main hand moves to a slot, release | quick-slot wheel |
| Main-hand grip over the shoulder | backpack |
| Off-hand `Y` / main `B` | open the **tablet** (quest log / character) in front of the player; again closes any window |
| `B/Y` (main) or `X/A` (off) on an open window | close |

Desktop emulation (`gothic-vr` headset emulator) covers all of these with mouse + keys, so every
window is reachable in the host tests.

## 3. System by system

"Core API" is what the host sends / reads. Status: **done** = implemented + host tested in this
branch; **layout** = sheet + builder + tests, the host event wiring is in the Integration section;
**plan** = specified, not built.

| # | Original system (UI_ORIGINAL) | VR form | Core API | Status |
| --- | --- | --- | --- | --- |
| 1 | HUD bars health / endurance / mana (3 framed bars, 807,688) | **Wrist watch** on the off hand, shows when the palm faces the eyes; low-health pulse. Damage vignette is not used (comfort) | `feed::vitals(core)` -> `HudData.vitals` | done (`wrist.rs`) |
| 2 | Quick bar 10 slots | **Quick-slot wheel** at the main hand, opened with the off-hand button; keys 1-0 map to wheel slots | `HudAction::UseQuickSlot(i)` -> `Command::UseItem` / `SelectSpell` per slot; `PlayerEvent::QuickSlot` | done (`wheel.rs`) |
| 3 | Compass / heading | **Wrist compass ring** is part of the watch (plan: heading from `PlayerHeading`); the held map shows the player marker | heading from hero yaw | plan |
| 4 | Focus name (enemy name above NPC, focus bar) | Name **floats above the NPC's head** at laser distance, health bar under it; only for the pointed/looked-at NPC | `Command::HeroControl { focus }`, `actor` hp via `Sim` | done (`tags.rs`: `HudData.focus`, original attitude colours, health bar, name always at most 16 cm high) |
| 5 | Dialog: bottom band, options, NPC subtitle | **Options panel beside the speaking NPC**, selected by laser + trigger or a fingertip; stick flick scrolls; **subtitle world-locked at the speaker's head** (never head-locked); the NPC's voice plays 3D | `Event::DialogStarted / LineSpoken / MenuChanged / DialogEnded` in, `Command::Talk`, `Choose { index }`, `EndDialog` out; `feed::dialog_options` | done (`dialog.rs`, `subtitles.rs`, `vr-bridge::dialog_flow`: lines in order with voice + subtitle at the speaker, options only after the last line, `Choose`, trigger skips a line) |
| 6 | Quest log page (tabs Aktuell / Erfolgreich / Gescheitert, list, detail, reputation, clock) | **Tablet**: spawned in front of the player on `Y`, world-locked; 3 tabs, list left, log of the selected quest right, clock and reputation under it. (The existing held *journal book* remains as the diegetic alternative for the same data.) | read-only from `GameState` + `db.text`; `feed::quest_log` | done (`views::quests`, `feed::quest_log`) |
| 7 | World map with quest markers | **Held map sheet** in the off hand (already built), laser shows marker labels; zoom = bring it closer. Quest-category markers from `MapMarker` | `MapData`, hero position | done (`handheld.rs`) |
| 8 | Inventory (5x10 grid, categories, gold) | **Backpack** over the shoulder: grab-or-poke grid in arm's reach (0.4-0.46 m); item hover shows name and stats; press = use/equip/drink, drop item out of the panel = `Drop` (plan) | `HudAction::InventoryItem(key)` -> `feed::use_stack` -> `Command::UseItem`; gold from `Inventory::count(GOLD)` | done (`backpack.rs`) |
| 9 | Equipment on the character view | **On the body**: weapon on hip/back (grab to draw = `SetDrawn`/`SelectWeapon`), shield on the arm, armor shown on the avatar; the Character tab lists the slots | `Command::SetDrawn`, `SelectWeapon`; `hero.equipment` | layout done (`views::character`), body-slots belong to `vr-melee`/Quest app |
| 10 | Character page: level, XP, attributes, protection, skills | **Tablet tab "Charakter"**: three columns (level / attributes + protection / skills + worn items), vitals bars | `feed::character` from `GameState`, `Actor` | done |
| 11 | Skills & learning from teachers | Teacher dialog options (`Teach`) are normal dialog rows with cost shown by the core's `teach_cost`; learning is a dialog `Choose`. No separate page | dialog `Choice.label` carries the text; `TeachCost` | done through dialog |
| 12 | Loot / chest window, body plunder, take all | **Chests open by hand** (grab the lid; `UseInteractObject`), then a **loot window** floats over the chest; or grab an item out and the window shows the rest. Bodies: crouch + hold over them = `QuickLoot`, or poke "Alles nehmen" | `Command::UseInteractObject`, `TakeLoot { object, index }`, `QuickLoot { body }`; `feed::loot_view` / `loot_command` | done (`views::loot`, `feed.rs`) |
| 13 | Trade page (vendor / player lists, baskets, balance, deal) | **Trade window** at 1.25 m: vendor left, player right, baskets in the middle, "Ausgleichen" and "Handeln". Optional: the merchant's counter holds real props (plan) | `Sim::trade_step / trade_clear / trade_balance / trade_deal`; prices `trade_unit_price`; `feed::trade_view`, `apply_trade_event` | done (`views::trade`, `feed.rs`) |
| 14 | Crafting: smithing anvil, alchemy, cooking cauldron, fry, sharpen, poison, bless | Walk to the station (`UseInteractObject` -> `PlayerEvent::Window`), the **recipe window** floats over it; hammering at the anvil is a physical mini-gesture that only *times* the `Craft` command (plan: hits counted, never changes the result) | `PlayerEvent::Window(Smithing/Alchemy/Cooking/...)`, `Command::Craft { recipe }`, `Event::CraftResult`; `feed::recipe_view`, `craft_command` | done: window (`views::recipe`, original recipe card + list art) and the hammer gesture (`vr-interact::craft_gestures::Hammer`: counted blows on the anvil face, then `Command::Craft`; the result is still the core's) |
| 15 | Lockpicking (bend-and-turn minigame) | **Two hands at the lock**: off hand holds the tension wrench, main hand turns the pick; the existing `vr-interact::lockpick` physical turn decides `success`, the core still rolls the skill when `None` | `Command::Lockpick { object, success }`, `Event::Player(LockPicked)` | done (`lockpick.rs` one hand, `lockpick_two.rs` two hands: tension wrench with load / over-torque, pick turns only under load) |
| 16 | Reading books / letters / notes | Book or letter **held in the off hand** (page turn by main-hand swipe) or, for long texts, the **document window** at 0.9 m with page flicks; skill books fire `PlayerEvent::BookRead` | `PlayerEvent::Window(Book / Letter)`, book text from the host | done (`views::document`, `handheld.rs`) |
| 17 | Sleeping / waiting / bed | Lie down = `UseInteractObject(bed)`; the screen fades (comfort), time scale from `PlayerEvent::TimeScale`; "Wait" is a wrist-clock gesture (plan) | `PlayerEvent::Slept`, `TimeScale` | done: sleep fade (`fade.rs`) and the wait picker (`views::wait` on the original confirm box, `WindowAction::Wait(hours)`, `feed::wait_hours`) |
| 18 | Pickpocketing | Sneak up behind (crouch), reach into the pouch at the NPC's belt: grab = the core's `PickPocket` info/`Command::Confirm`; sound-and-visibility meter on the wrist | `Confirmed`, dialog `PickPocket` | done: gesture (`craft_gestures::Pickpocket`, `Success` -> the host starts the core's `PickPocket` info) and the noise meter as a 4th bar on the watch (`HudData.noise` = `Pickpocket::noise()`) |
| 19 | Sneaking indicator | Real **crouch** (head below threshold) toggles sneak; a small eye glyph on the watch, lit/dim by `Event` detection state | `Command::ToggleSneak` (done in `vr-interact`) | done (toggle), indicator plan |
| 20 | Save / load / main menu / options | **Menu tablet** (`Menu` button) in front of the player, same world-lock rules; options map to `UserOptions.ini` pages through `gothic-ui` | `Command::Save(path)`, `Load(path)`, `Event::Saved / Loaded` | done: main menu on the original banner (`views::menu`, `MenuEntry`), save / load page, VR comfort options page (`views::options`, `VrComfortOptions`: snap / smooth turn, snap angle, vignette + strength, height, handedness, subtitles) |
| 21 | Floating texts (XP, damage), game messages | World-space text popping from the NPC / the hero's chest, short and non-blocking; game messages as a one-line toast under the subtitles | `Event::XpGained`, `Sim(Hit)`, `Notice` | done: floating texts (`tags::Floaters`) and game-message toasts (`HudData.messages`, world-locked, re-hung only after a 35 degree head turn) |
| 22 | Spells: rune/scroll select, cast | Off-hand spell ring, casting hand aims (already in `vr-interact`) | `SelectSpell`, `CastSpell`, `AbortCast` | done |
| 23 | Bow / crossbow | Draw the string by hand (`vr-archery`), `ReadyRanged / AimRanged / ReleaseRanged` | done |
| 24 | Weapon draw / melee | Grab the weapon from the hip; physical blows -> `Command::Attack` via `vr-bridge::actors` | done |
| 25 | Bribe / guard crime dialog | Normal dialog rows | `Command::Bribe`, `Event::Bribed` | done through dialog |

## 4. Data flow

```
Core::tick(dt, commands) -> Events ──► host (Quest app / desktop VR) ──► feed::* ──► HudData ──► VrHud
                                                                                      │
       ◄── Commands / Sim trade calls ◄── feed::apply_action ◄── WindowActionMsg / HudActionMsg
```

* `gothic-vr-ui` is Bevy-light and core-free: it consumes `HudData` (including
  `HudData.window: Option<WindowView>`) and emits `HudActionMsg` and `WindowActionMsg`.
* `gothic-vr-bridge::feed` is the only place that knows both sides.
* Everything the player can do with a window is a `gothic_ui::PageEvent` (trade, loot, craft)
  or `UseItem` / `Close`, i.e. exactly what the flat UI produces, so a window opened in VR and
  on the desktop behave identically.

## 5. Windows (sheets)

`vr-ui::sheet` / `views` / `windows` describe a window as a list of items in panel
coordinates (labels, rows, icon cells, buttons, tabs, bars, wrapped paragraphs) plus scroll
regions. The layout is Bevy-free and tested (`tests/windows.rs`): no overlapping targets,
every interactive item at least `MIN_TARGET_DMM`, text sizes in dmm, scroll clamps, pointer
hit tests, trade flows against the real `Sim`. `render_sheet.rs` draws a sheet with Bevy UI into
a texture on a quad (one extra `SurfaceKind::Window`).

| Window | Builder | Size (dmm) | Distance |
| --- | --- | --- | --- |
| Trade | `views::trade` | 1000 x 600 | 1.25 m |
| Loot / plunder | `views::loot` | 640 x 560 | 1.25 m |
| Recipes | `views::recipe` | 960 x 560 | 1.25 m |
| Document | `views::document` | 760 (book) / 420 (letter) x 480 | 0.9 m |
| Quest log | `views::quests` | 1000 x 620 | 1.25 m |
| Character | `views::character` | 1000 x 620 | 1.25 m |

Windows are world-locked when they appear (`windows::place`: horizontal heading of the head,
12 cm below the eye line, facing the eyes). Closing when the player walks away (e.g. > 4 m) is host policy (`EndInteraction`), not built here.

## 6. Integration (for the Quest agent; do not edit this from `vr-ui`)

1. Dependencies of `apps/quest`: `gothic-vr-ui`, `gothic-vr-bridge` (already in the workspace).
2. Plugins: `VrUiPlugin { gizmos: false }` after `VrPlugin`; on a renderer
   `VrUiRenderPlugin` and insert `HudFont`. The `Surface` for windows is created by
   `VrUiRenderPlugin` (kind `Window`), no extra setup.
3. Each frame (or on `state_rev` change) fill `HudDataRes`:
   ```rust
   data.vitals = feed::vitals(&core);
   data.inventory = feed::hero_panel(&core);
   data.names.extend(feed::names(&core, data.inventory.rows.iter().map(|r| r.item.as_str())));
   ```
   Dialogue: keep a `DialogFlow`; `flow.on_event(&event)` for every core event, `flow.tick(dt)` each
   frame and `dialog_flow::apply_to_hud(&out, &mut data, npc_head, &head_of)` for each output
   (`Say` also starts the 3D voice: `PlayVoice::spatial(key, speaker)`); a trigger press on the
   subtitle calls `flow.finish_line()`; `HudAction::ChooseOption(i)` -> `flow.choose(i)` ->
   `(Command::Choose, out)`; the leave key -> `flow.leave()` (`EndDialog`). Focus: set
   `data.focus` (name from the focus entity, `FocusAttitude`, health) every frame, `None` to clear;
   XP / damage: `data.floaters.push(text, rgb, world_pos, secs)` from `Event::XpGained` / `Sim(Hit)`.
   Save / load: `WindowView::Files(feed::files_view(..))`; `Applied::File(c)` -> `feed::file_command`.
   Sleep: `SleepFade::start(hours)` on `PlayerEvent::Slept`, advance the clock when `tick` returns
   the hours. Craft / lock / pocket gestures: feed `Hammer::step`, `TwoHandLock::step`,
   `Pickpocket::step` with the tracked hands and send the resulting commands.
4. Windows: set `data.window = Some(WindowView::Trade(feed::trade_view(&mut core, npc, hint)))`
   when the desktop logic would open the trade page (`Interaction.window`); likewise
   `Loot(feed::loot_view(..))`, `Recipe(feed::recipe_view(..))`, `Document(..)`. The tablet:
   on `Secondary` of the off hand toggle `data.window = Some(WindowView::Quests(feed::quest_log(&core)))`
   (second press -> `Character`, third -> close); refresh it when `core.state_rev` changes.
5. Actions: read `MessageReader<WindowActionMsg>` and
   `feed::apply_action(&mut core, &open_window, &a.0, step)`:
   `Applied::Commands(c)` -> add to the next `Core::tick`; `Applied::Trade(o)` -> rebuild the
   trade view (`o.dealt` plays the deal sound); `Applied::Close` -> `data.window = None` and
   send `Command::GuiOpen(false)` / `EndInteraction`. `HudActionMsg::InventoryItem(key)` ->
   `feed::use_stack(&core, key)`; `ChooseOption(i)` -> `Command::Choose { index: i }`;
   `UseQuickSlot(i)` -> the same code as the desktop quick bar.
6. Open the trade with `Sim::trade_open` first exactly like
   `apps/game/src/play/interact.rs::open_trade` (treasure sets from the host's object data).

Look: the windows are drawn in the original's design space on the original view backgrounds, with
item icons from `g3.igl` (`UiAssets::item_pic`), body font Times New Roman Bold and heading font
Times New Roman Bold Italic (`HudFont` / `HudHeadingFont`), text `#E0C495`. The host loads them like
the flat UI (`gothic_ui::assets::load_install_assets`) and inserts the `UiAssets` resource; without
it the windows fall back to flat panels. Fixed texts come from the original loc table
(`feed::labels` = `Labels::from_text`, the `HUD_*` keys the flat UI reads), otherwise the flat UI's
own German strings (with umlauts); item names are `FO_<template>`, attribute names `ATTRIB_<tag>`.
Compare renders: `.local/quest/vrui/compare_quests.png`, `compare_character.png`.

Known gaps: item icons for templates without an `icon` in the catalog fall back to a short name;
the options page edits `VrComfortOptions` but the Quest app has to read `snap_turn`, `snap_degrees`,
`vignette*` and `height_cm` in its locomotion (the HUD already reads handedness and subtitles);
the wrist watch compass and the options for the original (flat) pages are not built.
