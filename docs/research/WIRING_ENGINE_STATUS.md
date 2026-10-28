# Engine-side wiring status (gameplay / world / animation / audio / render rules / fx)

Branch `day/cov-engine`. Method as in `WIRING_COVERAGE.md` (portcov build, `--play --qa <scenario>`, merge of the
per-scenario dumps). Baseline of this branch (origin/dev 2d23b95) for the engine areas: gameplay 39 called / 123 never,
world 18 / 46, animation 10 / 38, audio 20 / 6, render rules 1 / 6, fx 17 / 2.

## Wired in this round (each verified by a QA run with hit counts)

| Area | Port | Replaces (glue) | Evidence |
| --- | --- | --- | --- |
| gameplay/mover | `Mover::{open, close, set_door_status, door_status, motion_stopped}` | `doors.rs` flag `open`; solid body now follows `Mover::collides()` (closed only), the open/close motion end calls `motion_stopped` | `doors` QA: open 1, close 1, motion_stopped 2 |
| animation/attach | `AttachmentSet::{add, remove, process}` | per-mesh `Attachment{joint}` component + `attachment_system`; `weapon_mount_system` re-adds the attachment with the other bone (`AddAttachment` replaces the entry of the same entity) | every QA that spawns a character: add 466, process 172k |
| gameplay/treasure | `generate_plunder`, `generate_plunder_inventory` | `Treasure::plunder` host re-implementation (own roll, no `half_to_full`, no trophy / mining rules) | chests at world spawn, bodies lazily when first looted (`ensure_plunder`, hero perks of that moment); NPC `Gear.give` no longer carries plunder |
| gameplay/trade | `loot_key_index` | digit key arithmetic in `interact.rs` | `loot_chest` QA |
| gameplay/focus | `lock_holds` | not wired by this branch: the merger keeps its own lock in hero.rs (player focus code is not mine) | - |
| world/trigger | `on_cache_in`, `on_enter_processing_range` | none (nobody called the range hooks) | `triggers` QA: 133 each |
| world/sector | `update_all`, `collect` | `sync_sectors` loop that re-implemented `UpdateSectors`; `capture()` now writes `CollectSectors` into the save image | `sector_switch`, `menu_save_load` QA |
| render rules | `lights::packet` (and `weight_sphere`) | `lights::select` + hand packing in `walk/env.rs` for the character light set | `startup_sanity` |

| gameplay/inventory | `equip_stack`, `unequip_stack`, `hold_stacks`, `hold_stack_{right,left}_hand`, `equip_stack_to_slot`, `link_stack_to_slot`, `unlink_stack_from_slot`, `right/left_equip_slot`, `apply_stats`, `unapply_stats`, `find_stack*`, stack `is_equipped*` | none: worn items were never in the stack list | `Sim::mirror_equipment`: the stack list follows `Equipment` / the drawn flag (direction Equipment -> links, not yet derived). Apply / unapply modifier events are dropped (Equipment's sums already count them); ammunition stays a plain count (`take` only removes unlinked stacks). 56 inventory functions hit across the QA set |

## Not reachable with the current host (needs a feature first, or is covered by an inverted form)

Evidence is code or document, not a guess. "Needs" names the host feature that would reach it.

* `render-rules::lod::{area_factor, lod_input, lod_stage}`, `cull::small_object_fade`: the game applies the closed-form
  inverses `stage_distances` / `distance_band` as Bevy `VisibilityRange` margins (the GPU evaluates the area rule per
  view, distance-based); unit tests `stage_distances_invert_the_stage_rule` and `distance_band_matches_the_area_rule`
  pin the equivalence. Calling the forward form per entity per frame would only add CPU work.
* `render-rules::lights::intensity`: the baked vertex term of `GetIntensity`; the instance shader uses `attenuation`.
* `world::dynamic::{decay, process_decay, is_auto_kill_enabled}`, `entity::{clone_entity, clone_subgraph, move_to_node,
  update_parent_dependencies, cache_out, contains_parent, set_temporary}`, `template::{spawn, patch_entity,
  patch_instances, delete_template, ref_template, split_template_path}`, `proxy::{resolve, parse, execute}`,
  `scene::{entity_by_part_name, count_by_part_name, trigger_targets}`: the runtime mirrors only entities it needs into the
  scene (`Scene::spawn_dynamic`); there are no `eCEntityPropertySet` objects, property proxies or template instances, and
  `triggers.rs` documents that the shipped worlds hold one trigger-family class (`gCTouchDamage_PS`). Needs: a property-set
  layer in the runtime (editor-grade) which the game never uses.
* `world::trigger::{on_intersect, on_trigger, on_untrigger, on_damage, trigger_by_name, filter, list_activate}`: no
  `eCTriggerList_PS` / `eCEventFilter_PS` / `gCEventScript_PS` in the parsed documents (same header comment).
  `on_damage` could be wired to hits on a `gCTouchDamage_PS` entity if one is ever damaged by the hero.
* `world::zones::*` (9): J8 of `WIRING_GLUE_SCAN.md`: negative zones and dynamic collision circles do not exist for NPC
  pathing / avoidance. Owned by the NPC / nav agent (`day/cov-npc`); the port is ready (`blocked_at`, `blocked_along`).
* `audio::emitter::{select_sample, voice_path}`: reached only when a spawned emitter fires inside the QA window, and the
  `(voice)` effect command; `step` is wired (see dump findings). `lipsync::viseme_for_label`
  is for `.lip` label text, the game reads the binary `.xlip` frames.
* `audio::sequencer::set_accept_triggers`: only the console command calls it in the original (see dump findings); `execute_stop`
  runs on a stop trigger, which the game never raises.

## Dump findings (`.local/research/engine/Game`, replaces the failed REA attempt)

* `gCMusicSequencer::SetAcceptTriggers` has exactly one caller: `gCSession::OnConsoleCommand` (Game, console command that
  toggles the music trigger acceptance). No game code calls it, so `set_accept_triggers` is a console / debug function and is
  **dead for play by design**; `DisableTriggers` of a trigger row is not read by it.
* `gCInventory_PS::OnSortInventory` (0x100b49e0) does not sort: it calls the listeners (`this+0x38` list, vfn +0xc) and
  purges them. `gCInventoryStack::Compare` (0x100c2460) has no static caller and no data reference in the dump (it is reached
  through a function pointer / template instantiation that the decompilation does not show), `SetSortIndex` has no caller.
  So **where the original sorts the stack lists is not recoverable here**; the UI sort added earlier was removed again
  (cite-or-reject). The `order::compare*` family stays unreached until a caller is found.
* Worn state: the original has no separate equipment record. Worn items are stacks linked to `gCInventorySlot`s
  (`Slot::RightHand` ... `Helmet`), the scripts ask `FindStackIndex(slot)` / `HoldStacks`. So the stack list is the single
  source of truth there. Our `Equipment` is a second model with different semantics (it keeps the weapon while holstered and
  `npc_stow_melee` / `npc_ready_melee` stash hand contents in `brain.melee_kit`); `Sim::mirror_equipment` keeps the stack list
  consistent but the flip (Equipment derived) means re-expressing `_AI_SelectWeapon`'s swaps, `drop_slot`, `unequip_stack`
  (script_host), magic `remove_item` and persist as `hold_stacks` / `unequip_stack` calls first. Not done: needs its own
  parity pass over the combat QA (10 mutation sites listed by `grep "equipment\.\(left\|right\|body\) ="`).
* World audio emitters: `eCAudioEmitter_PS` is created by the property-set factory on load (`Create` 0x100724c0, driven by
  `OnProcess` 0x10057ef0). The worlds hold 149 such sets (property survey: 149 `eEAudioEmitterMode` values); the new
  `play/emitters.rs` reads them (cache `.local/world_emitters.json`) and spawns `AudioEmitterPs` entities within 300 m of the
  hero, so `EmitterState::step` runs in play (`world_walk`: 4679 hits).

## Equipment flip, look-at, inventory order (this round)

* `Equipment` is derived from the stack links (`crates/sim/src/worn.rs`: `wear`, `hold`, `unwear`, `derive_equipment`);
  mirror and `brain.melee_kit` are gone. Hands win when a hand slot is occupied, else the carry slots (`BackRight`,
  `BackLeft`, `Bow`); a bow in the left hand brings the quiver (a count). Fixture items without a stack keep their place.
* Head look-at: `Entity::SetLookAtTarget` (`ScriptHost::set_look_at`, called by the ported combat scripts) now sets
  `Actor::look_at`; the dialog partner looks at the hero; `play/look_at.rs` drives `lookat::LookAtSet` (default limits of
  `ImportLookAtConstraints`, no `.xlac` ships in the animation archive) and `Animator::look` applies the head rotation after
  the clips. QA `look_at`.
* Inventory page order: the list control shows the stack array filtered by category (`gCHUDListCtrlInvStacks`
  0x100a1570: category 0 hidden, filter field +0x2c); where the array gets sorted is not found in the dump (`Compare` has no
  static caller). The page now orders stacks with the ported `Compare` chain; this reproduces `.local/ref/orig-inventory.png`
  (sword 1, bow 3, arrows 4 in the weapon tab) once the hero stacks carry their quick slots.
* `spell_zoo` fails on `It_Spell_Flamesword` (events Phase, Weapon; no Cast) on the build before the flip as well.
* Not done: replacing the `anim.rs` overlay glue by the `Animator` slot functions. Slot layers have no bone mask, the overlay
  (upper body) and aim blends need one first.

## Open (feature work, not dead)

* Inventory bridge: `Equipment` (sim) and the stack list (`Inventory`) are two truths. 95 functions of
  `inventory/{list,stack,order}.rs` wait on equip / hold / drop / split / merge / learn flows that the UI and the sim do
  through `Equipment` or by template count. Plan: derive `Equipment` from the linked stacks (`equip_stack`,
  `hold_stacks`, `apply_stats`), drop from the inventory page (`drop_items_from_stack`), skills from teachers
  (`learn_skill`, `can_learn_skill`).
* `gameplay::stats` (14): `Actor::stat_of` / `apply_stat` already use it for inventory stat events, which only arrive with
  the bridge above.
* Animation wrapper: `Animator::{stop_slot, stop_at_loop_end, set_target_weight, set_blend_mode, is_slot_playing}`,
  `body_parts`, `lookat::*`, `visual::*`, `process::{process_plan, step}`: replace the `anim.rs` overlay glue by
  `VisualAnimation`; head look-at from focus / dialog partner is the visible win.
* QA: `hud_focus` should press the lock key and assert `PlayState::lock`; a `doors` assertion that the door mover passes
  Opening -> Open; plunder scenario for a trophy animal with and without `Perk_TrophyTeeth`.
