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
* `spell_zoo`: `PS_Magic_FlameSword` (S:7b780) conjures a weapon: Phase(Load) and `MagicEvent::Weapon`, no Cast event, so the
  scenario now expects `Weapon` for `It_Spell_Flamesword` (passes).
* Overlay glue (`anim.rs`) not replaced by the slot functions, evidence: the original wrapper has no body-part mask. `PlayMotion`
  takes an `eSMotionDesc` (fade, mode, speed, loops, weight; no mask) and `SetBlendMode` (0x10049d10) only sets the instance's mix
  flag (+0x4c) for mode 2. The stock hit-reaction clips bind 59 of 67 rig slots, 11 of them below the spine (measured), so a
  non-additive slot layer would take the legs over; whether the original's mix flag is an additive layer or something else is
  EMotionFX behaviour that is not in the dump. The upper-body mask of `Rig::upper_body_mask` stays an [I] host rule until that is
  settled; adding masks to slot layers would be inventing an API the original does not have.

## Never called after the full QA run (55 scenarios, flaky ones re-run singly)

* gameplay (75 of 162): `inventory/list.rs` 37 (drop / split / merge / transfer / learn / skill / default-item / armor-set
  functions: they need UI flows: drop and split from the inventory page, teacher learning through the stack list, trade transfer
  through `transfer_stack_to`), `inventory/stack.rs` 13, `inventory/order.rs` 3, `stats.rs` 14 (now reachable through
  `consume_item` -> `ConsumeItems` -> `apply_stat`; hit once a potion is used in a scenario), `mover.rs` 4 (`doors` runs them; the
  full run lost them to a flaky door focus), `slot.rs` 2, `item.rs` 1, `script.rs` 1.
* world (48 of 64): see the "not reachable" section (property-set layer, trigger lists, zones owned by the NPC agent); `sector.rs`
  `enable_layer_activation`, `add_layer`, `contains_entity`, `enable_context` need layer documents the runtime does not build.
* Rune swap bug (merged day/cov-npc + flip): `derive_equipment` put an item whose stack was used up back over the item in the
  hand (fixture rule); fixtures now only fill a free place. Regression test `a_used_up_rune_does_not_come_back_over_the_next_one`.
  `spell_zoo` after the merge: effects of Poison, Sleep, IceBomb, SummonSkeleton are the right ones; DestroyEvil and DuskToDawn
  fizzle (`eff_magic_fizzle_01`): their casting conditions (evil target, darkness) are not met in the scenario.

## Callers of the inventory functions in the original (Game dump)

Callers found with `index.json` (static call sites only):

| Function | Original callers | Consequence for the port |
| --- | --- | --- |
| `DropSlot` 0x100b4530, `DropItemsFromSlot` 0x100b4630, `SplitStack`, `MergeStacks`, `SetStackAmount`, `TransferStackTo`, `TransferAllTo`, `PerformTransaction`, `SetStackHotKey`, `FindStackIndexByHotKey`, `CanEquip` | none (script natives: `PSInventory.*` called by the scripts) | reached through the script host (`InventoryApi`), not from a screen. `drop_slot` now runs through `Inventory::drop_slot` (the hand's stack leaves, the item is a loose item at the feet): hit by the `Down` script flow (spell_zoo, 9 hits) |
| `TransferItemsTo` 0x100b6e20 | loot list `gCHUDListCtrlInvLootStacks` (vfn_72, `FUN_100a0bc0`), `gCInfo_PS::Give` (dialog `Give`), the transaction transfer | loot and `Give` (dialog) already use `transfer_items_to`; `Give` is a dialog script op |
| `LearnSkill` / `LearnSpell` | `gCInfo_PS::OnEndInfo` (the info ends: teaching is an info's script op) | not a screen: the dialog engine (`gameplay::dialog`) would call it; our teach path uses `economy::learn_attribute` for attributes only |
| `LearnStack` | `LearnSkill`, `LearnSpell`, `gCSession::Teach` (console command `Teach`) | console only besides the two above |
| `CanLearnSkill` | `gCInfo_PS::Execute` (the info's condition for teaching) | dialog condition |

So the remaining inventory functions have no inventory / trade / teach screen caller in the original: the screens call
`TransferItemsTo`, the trade list `FUN_100a1fa0` and `GetTransactionAmount`; everything else is script or dialog driven.

Dialog `Give` / `Teach` (not changed, owned by the NPC / dialog bridge): `gameplay::script::Ctx::transfer` moves items with the
count-level calls `item_count` / `add_item` / `take_item` / `npc.inventory.add` of the `GameState`; the original's
`gCInfo_PS::Give` calls `TransferItemsTo`. Switching needs both sides as real stack lists with item data
(the game state's inventories are count-only until `rebind`) and the quest-manager notification
(`Transfer::notify`, ported) to replace `questmgr::on_receive_item`; learning (`LearnSkill`, `CanLearnSkill`) has no skill
stacks in the state yet. Left for the dialog owner.

## EMotionFX in the dump (mix mode)

EMotionFX is linked statically into Engine.dll: `MotionLayerSystem` (7 functions), `MotionSystem` (2), `SkeletalMotionInstance` (6),
`MotionInstance` (1) are present, mostly as unnamed `vfn_*` / `FUN_102e..` functions. Found: `MotionLayerSystem` add-instance
0x102ed2b0 inserts the instance into a list ordered by its priority field (+0x40) and clamps its target weight, the ramp is
`(target - current) / fade` when the fade is above zero (this is what `blend::insertion_index` and `Instance::new` already do).
The per-frame pass is found: `MotionLayerSystem` slot 6 (0x102ed520, the update) calls `FUN_102ed620`, which runs for every
node record (400 bytes) `FUN_102fb5b0(layer, node)`: it samples the layer's instance for the node (instance vfn +0xc; a
layer without a track for the node contributes nothing, or the node's bind pose when the instance's flag +0x57 is clear and a
predicate holds), recurses into the next layer and combines: **if the instance's mix flag (+0x4c, set by `SetBlendMode` mode 2)
is 1 and a lower layer exists, the layer is additive: its difference from the node's bind pose (position, rotation and scale
deltas) is scaled by the layer weight (+0x28) and added onto the lower result**; otherwise the two are blended by the weight.
So the original has no body-part mask: an overlay motion in mix mode adds its delta from the bind pose, and a clip that
barely moves the legs leaves the walk alone. That is what `Instance::set_blend_mode(2)` + `Animator::evaluate`'s `add_into(clip,
rest, ...)` implement. The host's `Overlay` + `upper_body_mask` is therefore a stand-in for this: replacing it by
`play_slot` + `set_blend_mode(slot, 2)` is the faithful path, with the visible risk that the hit clips' leg deltas (stumbles) now
show; it needs a QA with before/after screenshots and was not done in this round.

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
