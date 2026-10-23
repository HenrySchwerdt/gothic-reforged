# Startup and New Game lifecycle gaps

Source audit at `0260a73f7780892360863e19c8bb88b5e8585cc4` (2026-10-23). No source edits or runtime checks were made for this audit.

## Current behavior

- With no arguments, `apps/game/src/main.rs:25-35` appends `--play`; `walk::run` then calls `play::install` (`apps/game/src/walk.rs:520-521`). `play::install` immediately creates `new_game_state()` and `Core::new` (`apps/game/src/play/mod.rs:324-333, 403-418`), seeds the start kit, and sets the player to `HERO_START` (`459-465`). `UiState` defaults to `Screen::None` (`crates/ui/src/lib.rs:129-132, 395-400`), so this launches directly into the active world. The main menu is opened with Escape (`crates/ui/src/logic.rs:356-373`); there is no original intro/startup flow.
- The main-menu button is present at `crates/ui/src/screens.rs:1195-1211`. Selecting it emits `MenuChoice(NewGame)`, but `crates/ui/src/logic.rs:436-443` has no NewGame branch and `apps/game/src/play/saves.rs:72-79` handles only Quit. Repository search finds no other `MenuEntry::NewGame` handler. It neither resets the authoritative `Core`/player state nor closes the menu.
- Loading is wired for Reforged saves: LoadGame opens the files page and requests a refresh (`crates/ui/src/logic.rs:232-240, 436-443`); the selected row produces `FileCommand::Load` (`559-570`); the host resolves its slot and sends `CoreCommand::Load` (`apps/game/src/play/saves.rs:105-132`); `Core` loads and restores the snapshot (`crates/runtime/src/core.rs:1056-1062`, `crates/runtime/src/save.rs:304-333`, `crates/sim/src/persist.rs:224-255`). Loaded events resynchronize the hero controller position and loose items (`apps/game/src/play/events.rs:413-423`, `apps/game/src/play/saves.rs:192-218`).
- That path only lists `.g3rs` files (`crates/runtime/src/save.rs:39-41, 245-256`). The save module explicitly states that the original `.g3savcp` binary layout is not reproduced (`crates/runtime/src/save.rs:1-11`). This is Reforged save/load, not original Gothic save compatibility.
- One restoration gap: `Core::loaded_event` reports hero position and yaw (`crates/runtime/src/core.rs:1300-1313`), but `misc_events` ignores the event yaw and only moves the player controller (`apps/game/src/play/events.rs:413-417`). Camera orientation therefore remains at its pre-load value.

## Lifecycle constraints for the next implementation

Treat New Game as a coordinated session transition, not a UI dismissal. The authoritative game state is `Core.sim.state`; the running session also owns simulation actors/world, RNG and game clock, triggers, sector flags, and host presentation data. The Bevy host mirrors this through `Player`, `PlayState`, camera state, `Roster` actor bindings, NPC visuals, `WorldObjects`, trigger resources, and UI/session caches (`apps/game/src/play/mod.rs:129-157, 485-508`). Replacing only `GameState`, clearing a dialog, or moving the player leaves old actor, inventory, quest, trigger, and presentation state attached.

There is no general session reset API: `Core::new` constructs a core; `Core::after_restore` only clears dialog/strike transients and synchronizes inventory (`crates/runtime/src/core.rs:525-555, 1281-1289`). Save restoration is not a reset substitute. The pause path is also input-only: `InputFrame.blocked` suppresses player input when a UI screen is open (`apps/game/src/play/controls.rs:247-252, 308-319`), while `CorePlugin` continues calling `Core::tick` on every fixed update (`crates/runtime/src/lib.rs:39-57`). Do not use a process exit/restart as an in-app New Game transition.

Minimum coherent ownership for New Game:

1. Route the menu choice to one host lifecycle command/state transition, and decide when startup enters the title/menu versus creating a session. Do not construct the active default session before the user’s startup choice if the intended flow is a title menu.
2. Build a single canonical fresh-session baseline from the existing startup defaults (`new_game_state`, start kit, perks/equipment, world clock, hero spawn/pose). Reset or replace `Core` coherently, including simulation actor IDs/state, RNG/clock, triggers, sectors, and host data.
3. Reconcile host mirrors in the same transition: hero controller and camera, `PlayState` and quick slots, NPC roster actor/visual bindings, dynamic world objects, interaction windows, transient queues, and UI/dialog/session mirrors. Preserve loaded static terrain/assets where safe; respawn/rebind dynamic world state from the canonical authored data.
4. Keep Load as a separate restore transition. Apply the save atomically enough that decode/validation failure leaves the current session intact; after success, apply both loaded position and yaw to the host camera/player and reconcile actor/object/trigger mirrors.

## Acceptance paths

- Launch with no arguments: verify the chosen startup policy explicitly (title/menu before session creation, or documented direct-new-game). A title path must not create a second hidden default session first.
- Change quest status, inventory, XP/skills, NPC flags/health, world-object state, and player pose; choose New Game; assert the canonical fresh values and authored NPC/world state return, the old dialog/window closes, and no stale actor/visual binding remains.
- Save a modified session, change it again, choose Load and select that slot; assert persisted quest/player/inventory/actor/object state, hero position **and yaw**, and trigger/sector mirrors are restored. A corrupt or unsupported slot must report failure without partially replacing the active session.
- Keep format claims separate: test Reforged `.g3rs` load/save here; do not claim original `.g3savcp` import until its parser/compatibility path exists and has its own fixtures.
