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
| gameplay/inventory/order | `compare`, `compare_*` | unsorted stack lists in the inventory / trade pages | `trade_click`, `hud_focus` (compare 24k) |
| world/trigger | `on_cache_in`, `on_enter_processing_range` | none (nobody called the range hooks) | `triggers` QA: 133 each |
| world/sector | `update_all`, `collect` | `sync_sectors` loop that re-implemented `UpdateSectors`; `capture()` now writes `CollectSectors` into the save image | `sector_switch`, `menu_save_load` QA |
| render rules | `lights::packet` (and `weight_sphere`) | `lights::select` + hand packing in `walk/env.rs` for the character light set | `startup_sanity` |

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
* `audio::emitter::{step, select_sample, voice_path}`: world `eCAudioEmitter_PS` entities are not created (`AudioEmitterPs`
  has no spawner); sfx templates use the same property set but go through `PlayEffectSound`. `lipsync::viseme_for_label`
  is for `.lip` label text, the game reads the binary `.xlip` frames.
* `audio::sequencer::{set_accept_triggers, execute_stop}`: music trigger rows carry `DisableTriggers` (parsed, not applied)
  and a stop trigger is never raised; the original's caller of `SetAcceptTriggers` still has to be found (REA pass).

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
