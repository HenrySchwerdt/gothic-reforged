//! Player side of the script adapter: interact objects, recipes and the driver that runs the
//! hero's ported `PS_*` states ([`gothic_script::player`]) inside the simulation.
//!
//! Objects (chests, doors, work stations, beds) are interact points with extra data
//! ([`ObjectData`]); register them with [`ScriptTables::add_object`]. The game layer issues
//! player actions through the `Sim::player_*` methods; [`Sim::drive_player`] (called every tick)
//! steps the hero's current state and turns the queued AI commands into short busy times.

use std::collections::{BTreeMap, BTreeSet};

use gothic_script::enums as se;
use gothic_script::host::{AiCommand, EntityId, NpcApi};
use gothic_script::player::api::{
    FocusKind, Lock, LockStatus, PlayerApi, PlayerEvent, Recipe, SkillCategory, VoiceCounter,
};
use gothic_script::player::commands;
use gothic_script::player::evade::Direction;
use gothic_script::player::normal::Confirm;
use gothic_script::routine::state::State;

use crate::script_host::{
    KIND_ITEM, KIND_POINT, ScriptTables, SimHost, actor_of, entity_of, table_index,
};
use crate::sim::{EventKind, Sim};
use crate::types::ActorId;
use crate::world::{Pos, WorldQuery};

/// Data of an interact object.
#[derive(Debug, Clone, Default)]
pub struct ObjectData {
    /// Display name (shrines are told apart by it).
    pub name: String,
    /// Lock, if any.
    pub lock: Option<Lock>,
    /// Door state.
    pub open: bool,
    /// Used up by the player (ore, treasure, bookstands).
    pub used: bool,
    /// Game event raised when used.
    pub event: String,
    /// Owner enclave (`NONE` for nobody).
    pub owner: EntityId,
    /// Contents of a container as `(template, amount)`.
    pub loot: Vec<(String, u32)>,
}

/// Player-side script state of a simulation.
#[derive(Debug, Clone, Default)]
pub struct PlayerTables {
    pub(crate) objects: BTreeMap<u32, ObjectData>,
    pub(crate) counters: [u32; 2],
    recipes: BTreeMap<i32, Recipe>,
    recipe_items: BTreeMap<String, SkillCategory>,
    blade_items: BTreeSet<String>,
    pub(crate) books: [i32; 3],
    boulders: BTreeMap<u32, gothic_script::player::api::BoulderClass>,
    /// The player's action queue (`DAT_10118d54` of the script DLL): `(gEAction id, focus)`.
    pub(crate) queue: std::collections::VecDeque<(u32, EntityId)>,
    /// `PSFocus::GetFocusEntity(melee)`: the engine's focus search, fed by the host each frame.
    pub melee_focus: EntityId,
    /// `PSFocus::GetFocusEntity()`.
    pub focus: EntityId,
    /// `PSFocus.CurrentMode`, `FocusLookAtMode`, `EnableRangeRating` as the script wrote them.
    pub focus_modes: (u32, u32, bool),
    /// The camera is in first person (`PSCamera::IsInFirstPerson`), fed by the host.
    pub first_person: bool,
    /// The camera mode the script asked for (`PSCamera::SetMode`).
    pub camera_mode: u32,
    /// A movement key is held (`DAT_10118aac`), fed by the host.
    pub movement_key: bool,
    /// `PSPlayerMemory.LastWeaponConfig`.
    pub weapon_config: u32,
    /// The AI instructions the hero's states queued and the engine has not started yet.
    pub(crate) pending: std::collections::VecDeque<AiCommand>,
    /// The instruction the hero is executing.
    pub(crate) exec: HeroExec,
    /// Sequence number of the last instruction handed to the host.
    pub(crate) seq: u32,
    /// The host plays the hero's animations and reports back ([`Sim::hero_move_hit`],
    /// [`Sim::hero_exec_done`], [`Sim::hero_hold_swap`]). Without it (headless runs) combat
    /// moves have no clip: a `Hit` step lands at once and every instruction ends immediately.
    pub host_drives: bool,
}

/// The instruction of the hero that is running.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) enum HeroExec {
    #[default]
    Idle,
    /// `sAIWaitInstr` and the placeholder durations of animations the host does not drive.
    Wait { until_ms: u64 },
    /// `sAICombatMoveInstr`, waiting for the host.
    Move {
        seq: u32,
        action: se::Action,
        hit_phase: bool,
        target: Option<ActorId>,
        deadline_ms: u64,
    },
    /// `_AI_HoldInventoryItems`, waiting for the host.
    Hold {
        seq: u32,
        right: i32,
        left: i32,
        swapped: bool,
        deadline_ms: u64,
    },
}

/// A host that never answers must not freeze the hero: an instruction ends by itself after this.
const EXEC_TIMEOUT_MS: u64 = 8_000;

impl ScriptTables {
    /// Register an interact object at `pos` and return its entity.
    pub fn add_object(&mut self, use_type: se::UseType, pos: Pos, data: ObjectData) -> EntityId {
        let id = self.add_point(use_type, EntityId::NONE, pos);
        self.player.objects.insert(id.0, data);
        id
    }

    /// Register a recipe.
    pub fn add_recipe(&mut self, id: i32, recipe: Recipe) {
        self.player.recipes.insert(id, recipe);
    }

    /// Declare a recipe item (a scroll or book that makes `HasRecipes` true for a category).
    pub fn add_recipe_item(&mut self, template: &str, category: SkillCategory) {
        self.player
            .recipe_items
            .insert(template.to_ascii_lowercase(), category);
    }

    /// Declare an item template as a blade weapon (`HasBladeWeapon`).
    pub fn add_blade_item(&mut self, template: &str) {
        self.player
            .blade_items
            .insert(template.to_ascii_lowercase());
    }

    /// Set the weight class of an orc boulder object.
    pub fn set_boulder(
        &mut self,
        object: EntityId,
        kind: gothic_script::player::api::BoulderClass,
    ) {
        self.player.boulders.insert(object.0, kind);
    }

    /// A registered recipe.
    pub fn recipe_of(&self, id: i32) -> Option<&Recipe> {
        self.player.recipes.get(&id)
    }

    /// Object data.
    pub fn object(&self, object: EntityId) -> Option<&ObjectData> {
        self.player.objects.get(&object.0)
    }

    /// Take the contents of a container: stack `index`, or everything. Returns what was taken.
    pub fn take_loot(&mut self, object: EntityId, index: Option<usize>) -> Vec<(String, u32)> {
        let Some(o) = self.player.objects.get_mut(&object.0) else {
            return Vec::new();
        };
        match index {
            Some(i) if i < o.loot.len() => vec![o.loot.remove(i)],
            Some(_) => Vec::new(),
            None => std::mem::take(&mut o.loot),
        }
    }

    /// Object data (mutable).
    pub fn object_mut(&mut self, object: EntityId) -> Option<&mut ObjectData> {
        self.player.objects.get_mut(&object.0)
    }
}

fn counter_index(c: VoiceCounter) -> usize {
    match c {
        VoiceCounter::Refuse => 0,
        VoiceCounter::Aargh => 1,
    }
}

impl<W: WorldQuery> PlayerApi for SimHost<'_, W> {
    fn dialog_has_info(&self, npc: EntityId) -> bool {
        match (self.sim.dialog_info, actor_of(npc)) {
            (Some((a, has)), Some(n)) if a == n => has,
            _ => true,
        }
    }
    fn book_progress(&self, book: gothic_script::player::api::Book) -> i32 {
        self.sim.script.player.books[book as usize]
    }
    fn set_book_progress(&mut self, book: gothic_script::player::api::Book, pages: i32) {
        self.sim.script.player.books[book as usize] = pages;
    }
    fn boulder_class(&self, object: EntityId) -> gothic_script::player::api::BoulderClass {
        self.sim
            .script
            .player
            .boulders
            .get(&object.0)
            .copied()
            .unwrap_or_default()
    }
    fn voice_counter(&self, c: VoiceCounter) -> u32 {
        self.sim.script.player.counters[counter_index(c)]
    }
    fn set_voice_counter(&mut self, c: VoiceCounter, v: u32) {
        self.sim.script.player.counters[counter_index(c)] = v;
    }
    fn lock(&self, object: EntityId) -> Option<Lock> {
        self.sim.script.player.objects.get(&object.0)?.lock.clone()
    }
    fn set_lock_status(&mut self, object: EntityId, status: LockStatus) {
        if let Some(l) = self
            .sim
            .script
            .player
            .objects
            .get_mut(&object.0)
            .and_then(|o| o.lock.as_mut())
        {
            l.status = status;
        }
    }
    fn door_open(&self, object: EntityId) -> bool {
        self.sim
            .script
            .player
            .objects
            .get(&object.0)
            .is_some_and(|o| o.open)
    }
    fn set_door_open(&mut self, object: EntityId, open: bool) {
        self.sim
            .script
            .player
            .objects
            .entry(object.0)
            .or_default()
            .open = open;
    }
    fn used_by_player(&self, object: EntityId) -> bool {
        self.sim
            .script
            .player
            .objects
            .get(&object.0)
            .is_some_and(|o| o.used)
    }
    fn set_used_by_player(&mut self, object: EntityId, v: bool) {
        self.sim
            .script
            .player
            .objects
            .entry(object.0)
            .or_default()
            .used = v;
    }
    fn use_event(&self, object: EntityId) -> String {
        self.sim
            .script
            .player
            .objects
            .get(&object.0)
            .map(|o| o.event.clone())
            .unwrap_or_default()
    }
    fn owner_enclave(&self, thing: EntityId) -> EntityId {
        self.sim
            .script
            .player
            .objects
            .get(&thing.0)
            .map_or(EntityId::NONE, |o| o.owner)
    }
    fn loot_stacks(&self, object: EntityId) -> i32 {
        self.sim
            .script
            .player
            .objects
            .get(&object.0)
            .map_or(0, |o| o.loot.len() as i32)
    }
    fn generate_loot(&mut self, object: EntityId) -> i32 {
        self.loot_stacks(object)
    }
    fn focus_kind(&self, e: EntityId) -> FocusKind {
        if actor_of(e).is_some_and(|a| (a.0 as usize) < self.sim.actors.len()) {
            FocusKind::Npc
        } else if table_index(e, KIND_ITEM).is_some() {
            FocusKind::Item
        } else if table_index(e, KIND_POINT).is_some() {
            FocusKind::Object
        } else {
            FocusKind::None
        }
    }
    fn lootable(&self, npc: EntityId) -> Vec<(String, u32)> {
        self.actor(npc)
            .map(|a| {
                a.inventory
                    .iter()
                    .filter(|(_, n)| **n > 0)
                    .map(|(k, n)| (k.clone(), *n))
                    .collect()
            })
            .unwrap_or_default()
    }
    fn transfer_item(&mut self, from: EntityId, to: EntityId, template: &str, amount: u32) {
        let moved = {
            let Some(a) = self.actor_mut(from) else {
                return;
            };
            let moved = a.inventory.count(template).min(amount);
            a.inventory.take(template, moved);
            moved
        };
        if moved > 0 {
            PlayerApi::add_item(self, to, template, moved);
        }
    }
    fn item_template(&self, item: EntityId) -> String {
        table_index(item, KIND_ITEM)
            .and_then(|i| self.sim.script.items.get(i))
            .map(|i| i.name.clone())
            .unwrap_or_default()
    }
    fn take_item(&mut self, taker: EntityId, item: EntityId) {
        let name = self.item_template(item);
        if let Some(i) = table_index(item, KIND_ITEM).and_then(|i| self.sim.script.items.get_mut(i))
        {
            i.dropped = true;
        }
        if !name.is_empty() {
            PlayerApi::add_item(self, taker, &name, 1);
        }
    }
    fn add_item(&mut self, e: EntityId, template: &str, amount: u32) {
        let info = self.sim.items.info(template);
        if let Some(a) = self.actor_mut(e) {
            a.inventory.add_items(
                template,
                info,
                amount.min(i32::MAX as u32) as i32,
                0,
                gothic_gameplay::inventory::StackType::Normal,
            );
        }
    }
    fn remove_item(&mut self, e: EntityId, template: &str, amount: u32) -> bool {
        let Some(a) = self.actor_mut(e) else {
            return false;
        };
        let have = a.inventory.count(template);
        if have == 0 {
            return amount == 0;
        }
        if have < amount {
            a.inventory.take(template, have);
            return false;
        }
        a.inventory.take(template, amount);
        true
    }
    fn item_count(&self, e: EntityId, template: &str) -> u32 {
        self.actor(e).map_or(0, |a| a.inventory.count(template))
    }
    fn has_recipes(&self, e: EntityId, category: SkillCategory) -> bool {
        self.actor(e).is_some_and(|a| {
            a.inventory.iter().any(|(k, n)| {
                *n > 0
                    && self
                        .sim
                        .script
                        .player
                        .recipe_items
                        .get(&k.to_ascii_lowercase())
                        .is_some_and(|c| *c == category)
            })
        })
    }
    fn has_blade_weapon(&self, e: EntityId) -> bool {
        self.actor(e).is_some_and(|a| {
            a.inventory.iter().any(|(k, n)| {
                *n > 0
                    && self
                        .sim
                        .script
                        .player
                        .blade_items
                        .contains(&k.to_ascii_lowercase())
            }) || a.equipment.all().any(|i| {
                i.damage
                    .as_ref()
                    .is_some_and(|d| d.damage_type.eq_ignore_ascii_case("Blade"))
            })
        })
    }
    fn item_use_type(&self, template: &str) -> se::UseType {
        self.sim
            .items
            .get(template)
            .and_then(|d| d.use_type.as_deref())
            .and_then(se::UseType::from_name)
            .unwrap_or(se::UseType::None)
    }
    fn equip_item(&mut self, e: EntityId, template: &str) -> bool {
        match actor_of(e) {
            Some(id) => {
                // A spell potion is held in the right hand like a rune (the cast reads the spell
                // of the item in that hand); other potions are not equipment.
                let potion = self.sim.items.get(template).and_then(|d| {
                    (d.spell.is_some()
                        && d.use_type
                            .as_deref()
                            .is_some_and(|u| u.eq_ignore_ascii_case("potion")))
                    .then(|| d.clone())
                });
                if let Some(d) = potion {
                    self.sim.actor_mut(id).equipment.right = Some(d);
                    return true;
                }
                self.sim.equip(id, template).is_ok()
            }
            None => false,
        }
    }
    fn unequip_item(&mut self, e: EntityId, template: &str) {
        if let Some(a) = self.actor_mut(e) {
            let eq = &mut a.equipment;
            for slot in [&mut eq.left, &mut eq.right, &mut eq.body] {
                if slot
                    .as_ref()
                    .is_some_and(|i| i.name.eq_ignore_ascii_case(template))
                {
                    *slot = None;
                }
            }
            eq.accessories
                .retain(|i| !i.name.eq_ignore_ascii_case(template));
        }
    }
    fn is_equipped(&self, e: EntityId, template: &str) -> bool {
        self.actor(e).is_some_and(|a| {
            a.equipment
                .all()
                .any(|i| i.name.eq_ignore_ascii_case(template))
        })
    }
    fn consume_item(&mut self, e: EntityId, template: &str) -> bool {
        if !PlayerApi::remove_item(self, e, template, 1) {
            return false;
        }
        let heal = self.sim.items.get(template).map_or(0, |d| d.heal_amount());
        let Some(id) = actor_of(e) else { return true };
        let max = self.sim.actors[id.0 as usize].hp_max(&self.sim.cfg);
        let a = &mut self.sim.actors[id.0 as usize];
        let before = a.hp;
        a.hp = (a.hp + heal).min(max);
        let healed = a.hp - before;
        if healed > 0 {
            self.sim.push(EventKind::Potion { actor: id, healed });
        }
        true
    }
    fn recipe(&self, id: i32) -> Option<Recipe> {
        self.sim.script.player.recipes.get(&id).cloned()
    }
    fn known_recipes(&self) -> Vec<i32> {
        self.sim.script.player.recipes.keys().copied().collect()
    }
    fn raise_game_event(&mut self, name: &str) {
        self.sim.state.events.insert(name.to_string());
    }
    fn advance_hours(&mut self, hours: f32) {
        self.sim.clock.game_ms += (hours * 3_600_000.0) as u64;
        self.sim.state.hours = self.sim.clock.total_hours();
    }
    fn set_alignment_target(&mut self, e: EntityId, target: EntityId) {
        if let Some(s) = self.script_mut(e) {
            s.alignment_target = target;
        }
    }
    fn set_movement_constraints(&mut self, e: EntityId, degrees: f32) {
        if let Some(s) = self.script_mut(e) {
            s.movement_constraint = degrees;
        }
    }
    fn action_queue_head(&self) -> Option<(u32, EntityId)> {
        self.sim.script.player.queue.front().copied()
    }
    fn action_queue_push(&mut self, id: u32, entity: EntityId) {
        self.sim.script.player.queue.push_back((id, entity));
    }
    fn action_queue_pop(&mut self) {
        self.sim.script.player.queue.pop_front();
    }
    fn action_queue_clear(&mut self) {
        self.sim.script.player.queue.clear();
    }
    fn melee_focus(&self) -> EntityId {
        self.sim.script.player.melee_focus
    }
    fn current_focus(&self) -> EntityId {
        self.sim.script.player.focus
    }
    fn set_focus_entity(&mut self, e: EntityId) {
        self.sim.script.player.focus = e;
    }
    fn set_focus_current_mode(&mut self, mode: u32) {
        self.sim.script.player.focus_modes.0 = mode;
    }
    fn set_focus_look_at_mode(&mut self, mode: u32) {
        self.sim.script.player.focus_modes.1 = mode;
    }
    fn set_focus_range_rating(&mut self, on: bool) {
        self.sim.script.player.focus_modes.2 = on;
    }
    fn camera_in_first_person(&self) -> bool {
        self.sim.script.player.first_person
    }
    fn set_camera_mode(&mut self, mode: u32) {
        self.sim.script.player.camera_mode = mode;
    }
    fn movement_key_down(&self) -> bool {
        self.sim.script.player.movement_key
    }
    fn last_weapon_config(&self) -> u32 {
        self.sim.script.player.weapon_config
    }
    fn set_last_weapon_config(&mut self, v: u32) {
        self.sim.script.player.weapon_config = v;
    }
    fn azimuth_elevation(&self, from: EntityId, to: EntityId) -> (f32, f32) {
        let (Some(a), Some(b)) = (actor_of(from), actor_of(to)) else {
            return (0.0, 0.0);
        };
        let (pa, pb) = (self.sim.pos(a), self.sim.pos(b));
        let heading = self.sim.world.facing(a);
        let mut azi = pa.yaw_to(pb) - heading;
        while azi > std::f32::consts::PI {
            azi -= std::f32::consts::TAU;
        }
        while azi < -std::f32::consts::PI {
            azi += std::f32::consts::TAU;
        }
        let flat = ((pb.x - pa.x).powi(2) + (pb.z - pa.z).powi(2)).sqrt();
        let elev = (pb.y - pa.y).atan2(flat.max(1e-3));
        (azi.to_degrees(), elev.to_degrees())
    }
    fn hit_direction(&self, e: EntityId) -> u32 {
        self.script(e).hit_direction
    }
    fn set_hit_direction(&mut self, e: EntityId, dir: u32) {
        if let Some(s) = self.script_mut(e) {
            s.hit_direction = dir;
        }
    }
    fn emit(&mut self, event: PlayerEvent) {
        // A window the script opens holds the world (`gui_page_open`) until the host closes it.
        match &event {
            PlayerEvent::Window(_) => self.sim.gui_open = true,
            PlayerEvent::CloseWindow => self.sim.gui_open = false,
            _ => {}
        }
        self.sim.push(EventKind::Player(event));
    }
}

impl<W: WorldQuery> Sim<W> {
    fn hero_entity(&self) -> Option<EntityId> {
        self.hero.map(entity_of)
    }

    pub(crate) fn with_hero<R>(
        &mut self,
        f: impl FnOnce(&mut SimHost<'_, W>, EntityId) -> R,
    ) -> Option<R> {
        let hero = self.hero_entity()?;
        let mut host = SimHost::new(self);
        host.self_entity = hero;
        Some(f(&mut host, hero))
    }

    /// Reactions the ported predicates report for a talk with `npc` (`HasGuardNews`,
    /// `HasBossNews`, `HasHealInfo`, S:bcf00 / S:bd050 / S:bd2c0): the dialog engine offers the
    /// `EnclaveCrime`, `PoliticalCrime` and `Heal` infos only then.
    pub fn dialog_triggers(&mut self, npc: ActorId) -> std::collections::BTreeSet<String> {
        use gothic_script::routine::info::{has_boss_news, has_guard_news, has_heal_info};
        let mut set = std::collections::BTreeSet::new();
        let e = entity_of(npc);
        let host = SimHost::new(self);
        if has_guard_news(&host, e) {
            set.insert("EnclaveCrime".to_string());
        }
        if has_boss_news(&host, e) {
            set.insert("PoliticalCrime".to_string());
        }
        if has_heal_info(&host, e) {
            set.insert("Heal".to_string());
        }
        set
    }

    /// The confirm key on the focused entity (actors are `EntityId(n + 1)`, objects and items
    /// come from [`ScriptTables`]).
    pub fn player_confirm(&mut self, focus: EntityId) -> Option<Confirm> {
        self.with_hero(|h, p| commands::press_confirm(h, p, focus))
    }

    /// Use an interact object.
    pub fn player_use_object(&mut self, object: EntityId) {
        self.with_hero(|h, p| commands::use_object(h, p, object));
    }

    /// Leave a persistent interaction.
    pub fn player_end_interaction(&mut self) {
        self.with_hero(|h, p| commands::end_interaction(h, p));
    }

    /// Pick a loose item up.
    pub fn player_pick_up(&mut self, item: EntityId) {
        self.with_hero(|h, p| commands::pick_up(h, p, item));
    }

    /// Open a lock with key/lockpick; `minigame` is the UI's result when it ran one.
    pub fn player_lockpick(&mut self, object: EntityId, minigame: Option<bool>) -> bool {
        self.with_hero(|h, p| commands::lockpick(h, p, object, minigame))
            .unwrap_or(false)
    }

    /// Make a recipe.
    pub fn player_craft(&mut self, recipe: i32) -> Option<gothic_script::player::craft::Synth> {
        self.with_hero(|h, p| commands::craft(h, p, recipe))
    }

    /// Use an inventory item.
    pub fn player_use_item(
        &mut self,
        template: &str,
    ) -> Option<gothic_script::player::use_item::UseOutcome> {
        self.with_hero(|h, p| commands::use_item(h, p, template))
    }

    /// Toggle sneaking.
    pub fn player_toggle_sneak(&mut self) {
        self.with_hero(|h, p| commands::toggle_sneak(h, p));
    }

    /// Jump.
    pub fn player_jump(&mut self) {
        self.with_hero(|h, p| commands::jump(h, p));
    }

    /// Dodge.
    pub fn player_dodge(&mut self, direction: Direction) -> bool {
        self.with_hero(|h, p| commands::dodge(h, p, direction))
            .unwrap_or(false)
    }

    /// The facts the key handlers read before they queue an attack (`CanDo`, the weapon classes).
    pub fn player_control_facts(&mut self) -> gothic_script::player::melee::ControlFacts {
        self.with_hero(|h, p| gothic_script::player::melee::control_facts(h, p))
            .unwrap_or_default()
    }

    /// The hero's script state names for the key router: task, state, seconds in the state.
    pub fn hero_script_state(&self) -> (Option<State>, Option<State>, f32) {
        let Some(h) = self.hero else {
            return (None, None, 0.0);
        };
        let s = self.script.npc(h);
        (s.task, s.state, s.state_time)
    }

    /// A key action of the hero (`OnPlayerGameKeyPressed` queues a `gEAction` id with the focus
    /// entity); the loop state of the current task reads it (`PS_Melee_Loop`).
    pub fn player_push_action(&mut self, id: u32, focus: Option<ActorId>) {
        let focus = focus.map_or(EntityId::NONE, entity_of);
        self.with_hero(|h, _| commands::push_action(h, id, focus));
    }

    /// `FullStop` + `SetState` of the hero: the riposte out of the guard and the chains of
    /// `OnPlayerAction` switch states directly.
    pub fn player_enter_state(&mut self, state: State) {
        self.with_hero(|h, p| commands::enter_state(h, p, state));
    }

    /// The weapon key in `PS_Normal_Loop` (`OnPlayerSelectWeapon`).
    pub fn player_select_weapon(&mut self) -> bool {
        self.with_hero(|h, p| commands::select_weapon(h, p))
            .unwrap_or(false)
    }

    /// The host's per-frame view of the hero's control layer: the melee-mode focus search
    /// (`PSFocus::GetFocusEntity(melee)`), the plain focus, the camera and the movement key
    /// latch.
    pub fn set_hero_control(
        &mut self,
        melee_focus: Option<ActorId>,
        focus: Option<ActorId>,
        first_person: bool,
        movement_key: bool,
    ) {
        let t = &mut self.script.player;
        t.melee_focus = melee_focus.map_or(EntityId::NONE, entity_of);
        t.focus = focus.map_or(EntityId::NONE, entity_of);
        t.first_person = first_person;
        t.movement_key = movement_key;
    }

    /// What the hero's script wants the body to do: the alignment target and the movement
    /// constraint (`SetAlignmentTarget`, `SetMovementConstraints`).
    pub fn hero_alignment(&self) -> (Option<ActorId>, f32) {
        let Some(h) = self.hero else {
            return (None, 0.0);
        };
        let s = self.script.npc(h);
        (actor_of(s.alignment_target), s.movement_constraint)
    }

    /// The hero's current target (`PSNpc::GetCurrentTarget`) as `PS_Melee_Loop` wrote it.
    pub fn hero_current_target(&self) -> Option<ActorId> {
        let h = self.hero?;
        self.actors[h.0 as usize].target
    }

    /// The camera mode the hero's script asked for (`PSCamera::SetMode`) and the focus modes.
    pub fn hero_script_modes(&self) -> (u32, (u32, u32, bool)) {
        let t = &self.script.player;
        (t.camera_mode, t.focus_modes)
    }

    /// The `OnHit` moment of a `Hit` clip (60 % of its length, `sAICombatMoveItlLoop`): the
    /// blow lands on the move's target if `seq` is the instruction in flight.
    pub fn hero_move_hit(&mut self, seq: u32) {
        let HeroExec::Move {
            seq: s,
            hit_phase: true,
            target: Some(target),
            ..
        } = self.script.player.exec.clone()
        else {
            return;
        };
        if s != seq {
            return;
        }
        self.hero_strike(target);
    }

    /// The host finished the instruction `seq` (its clips ended, or none was found).
    pub fn hero_exec_done(&mut self, seq: u32) {
        let done = match &self.script.player.exec {
            HeroExec::Move { seq: s, .. } | HeroExec::Hold { seq: s, .. } => *s == seq,
            _ => false,
        };
        if done {
            self.script.player.exec = HeroExec::Idle;
        }
    }

    /// The `Begin` clip of a hold ended: the item moves between the back slot and the hand
    /// (`PSInventory::HoldStacks`).
    pub fn hero_hold_swap(&mut self, seq: u32) {
        let HeroExec::Hold {
            seq: s,
            right,
            left,
            swapped: false,
            ..
        } = self.script.player.exec.clone()
        else {
            return;
        };
        if s != seq {
            return;
        }
        self.with_hero(|h, p| gothic_script::host::InventoryApi::hold_stacks(h, p, right, left));
        if let HeroExec::Hold { swapped, .. } = &mut self.script.player.exec {
            *swapped = true;
        }
    }

    /// The blow of the hero's current move: the reach test and the damage of
    /// `Sim::resolve_strike` against the move's target.
    fn hero_strike(&mut self, target: ActorId) {
        let Some(h) = self.hero else { return };
        let action = self.actors[h.0 as usize].action;
        self.resolve_strike(
            h.0 as usize,
            crate::brain::Strike {
                at_ms: 0,
                action,
                target,
            },
        );
    }

    /// Take everything from a body.
    pub fn player_quick_loot(&mut self, body: EntityId) -> usize {
        self.with_hero(|h, p| gothic_script::player::normal::quick_loot(h, p, body))
            .unwrap_or(0)
    }

    /// Step the hero: the instructions the hero's states queued run one after the other (the
    /// engine's AI instruction queue); when none is left the current state runs again and
    /// queues the next ones. A combat move or a hold waits for the host's animation
    /// ([`Sim::hero_move_hit`], [`Sim::hero_exec_done`], [`Sim::hero_hold_swap`]); a wait and the
    /// placeholder animations of the other commands keep the hero busy for their length.
    pub fn drive_player(&mut self) {
        let now = self.now_ms();
        let Some(hero) = self.hero else { return };
        {
            // The hero's states keep time like the NPCs' (a bow's draw is read from it).
            let dt = self.dt();
            let s = self.script.npc_mut(hero);
            s.state_time += dt;
            s.task_time += dt;
        }
        let ent = entity_of(hero);
        // 1. The instruction in flight, then the queued ones.
        for _ in 0..16 {
            match self.script.player.exec.clone() {
                HeroExec::Idle => {}
                HeroExec::Wait { until_ms } => {
                    if now < until_ms {
                        return;
                    }
                    self.script.player.exec = HeroExec::Idle;
                }
                HeroExec::Move { deadline_ms, .. } | HeroExec::Hold { deadline_ms, .. } => {
                    if now < deadline_ms {
                        return;
                    }
                    self.script.player.exec = HeroExec::Idle;
                }
            }
            match self.script.player.pending.pop_front() {
                Some(cmd) => self.start_hero_instruction(hero, cmd, now),
                None => break,
            }
            if self.script.player.exec != HeroExec::Idle {
                return;
            }
        }
        if !self.script.player.pending.is_empty() {
            return;
        }
        // 2. Nothing left to run: the state decides what comes next.
        let cmds = {
            let mut host = SimHost::new(self);
            host.self_entity = ent;
            if host.current_task(ent).is_none() {
                // A new game starts in `PS_Normal`.
                host.set_task(ent, State::PsNormal);
            }
            if !host.current_state(ent).is_some_and(|s| {
                let n = s.name();
                n.starts_with("PS_") || n.starts_with("BS_") || n.starts_with("ZS_Evade")
            }) {
                return;
            }
            commands::run(&mut host, ent);
            host.sim.script.take_commands_for(hero)
        };
        self.script.player.pending.extend(cmds);
    }

    /// Start one queued instruction of the hero.
    fn start_hero_instruction(&mut self, hero: ActorId, cmd: AiCommand, now: u64) {
        let ent = entity_of(hero);
        let crossbow = self
            .actor(hero)
            .equipment
            .left
            .as_ref()
            .is_some_and(|i| i.use_kind() == crate::item::Use::CrossBow);
        let wait = |ms: u64| HeroExec::Wait { until_ms: now + ms };
        let drives = self.script.player.host_drives;
        let exec = match cmd {
            AiCommand::AddStamina(delta) => {
                let mut host = SimHost::new(self);
                host.self_entity = ent;
                gothic_script::stats::add_stamina_points(&mut host, ent, delta);
                HeroExec::Idle
            }
            AiCommand::CombatMove {
                action,
                phase,
                speed,
                pose,
                target,
            } => {
                let target = actor_of(target);
                let hit_phase = phase == gothic_script::host::MovePhase::Hit;
                if drives {
                    self.script.player.seq += 1;
                    let seq = self.script.player.seq;
                    self.push(EventKind::HeroMove {
                        seq,
                        action,
                        phase,
                        speed,
                        pose,
                        target,
                    });
                    HeroExec::Move {
                        seq,
                        action,
                        hit_phase,
                        target,
                        deadline_ms: now + EXEC_TIMEOUT_MS,
                    }
                } else {
                    // Headless: no clip, so the blow lands at once and the move ends.
                    if hit_phase && let Some(t) = target {
                        self.hero_strike(t);
                    }
                    HeroExec::Idle
                }
            }
            AiCommand::HoldInventoryStacks { right, left } => {
                if drives {
                    self.script.player.seq += 1;
                    let seq = self.script.player.seq;
                    self.push(EventKind::HeroHold {
                        seq,
                        draw: right >= 0 || left >= 0,
                    });
                    HeroExec::Hold {
                        seq,
                        right,
                        left,
                        swapped: false,
                        deadline_ms: now + EXEC_TIMEOUT_MS,
                    }
                } else {
                    let mut host = SimHost::new(self);
                    host.self_entity = ent;
                    gothic_script::host::InventoryApi::hold_stacks(&mut host, ent, right, left);
                    HeroExec::Idle
                }
            }
            AiCommand::Action { action, secs } => wait(hero_action_ms(action, crossbow, secs)),
            AiCommand::Wait { secs } => wait(secs.max(0) as u64 * 1000),
            AiCommand::WaitMs { ms } => wait(u64::from(ms)),
            AiCommand::ChangeAction | AiCommand::StandUp => wait(400),
            AiCommand::AniStateAction(_) => wait(500),
            AiCommand::TakeItem { .. } => wait(800),
            AiCommand::SayLine { key } => {
                self.speak(hero, key.to_string());
                HeroExec::Idle
            }
            _ => wait(200),
        };
        self.script.player.exec = exec;
    }

    /// Has the hero returned to the idle `PS_Normal_Loop` (or no state)?
    pub fn player_idle(&self) -> bool {
        let Some(h) = self.hero else { return true };
        matches!(
            self.script.npc(h).state,
            None | Some(
                State::PsNormalLoop
                    | State::PsMeleeLoop
                    | State::PsMagicLoop
                    | State::PsRangedWalk
                    | State::PsRangedWalkLoop
                    | State::PsRangedAimLoop,
            )
        )
    }
}

/// Milliseconds an animation action keeps the hero busy: the length of the clip that plays
/// (`gothic_projectile::aim::clips`, measured from `_compiledAnimation.pak`). The ported script
/// passes a placeholder of one second; the draw of a bow and the cast would otherwise lag a full
/// second behind the key.
fn hero_action_ms(action: gothic_script::enums::Action, crossbow: bool, secs: i32) -> u64 {
    use gothic_projectile::aim::clips;
    use gothic_script::enums::Action;
    let s = match action {
        Action::Aim if crossbow => clips::crossbow::RAISE,
        Action::Aim => clips::bow::RAISE,
        // The arrow leaves at once (the bow has no `Hit` clip); the recovery follows the shot.
        Action::Shoot => 0.0,
        Action::Reload if crossbow => clips::crossbow::RELOAD_BEGIN + clips::crossbow::RELOAD_END,
        Action::Reload => {
            clips::bow::RELOAD_BEGIN + clips::bow::RELOAD_RAISE + clips::bow::RELOAD_END
        }
        Action::Cock => clips::crossbow::COCK,
        // The missile leaves when the raise clip ends.
        Action::Cast => clips::cast::CAST.0,
        Action::QuickCast => clips::cast::QUICK.0,
        Action::PowerCast => clips::cast::POWER.0,
        _ => return secs.max(0) as u64 * 1000,
    };
    (s * 1000.0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SimConfig;
    use crate::item::ItemDb;
    use crate::world::Grid2dWorld;

    fn sim() -> Sim<Grid2dWorld> {
        let mut s = Sim::new(SimConfig::default(), Grid2dWorld::new(), ItemDb::default());
        s.spawn_hero(Pos::flat(0.0, 0.0));
        s
    }

    fn play(s: &mut Sim<Grid2dWorld>, ticks: usize) -> Vec<crate::sim::Event> {
        for _ in 0..ticks {
            s.step();
        }
        s.drain_events()
    }

    #[test]
    fn the_hero_opens_a_door_through_the_ported_states() {
        let mut s = sim();
        let door = s
            .script
            .add_object(se::UseType::Door, Pos::default(), ObjectData::default());
        s.player_use_object(door);
        let ev = play(&mut s, 400);
        assert!(ev.iter().any(|e| matches!(
            &e.kind,
            EventKind::Player(PlayerEvent::DoorChanged { open: true, .. })
        )));
        assert!(s.player_idle());
    }

    #[test]
    fn a_locked_chest_needs_its_key_and_theft_is_voiced() {
        let mut s = sim();
        let chest = s.script.add_object(
            se::UseType::Chest,
            Pos::default(),
            ObjectData {
                lock: Some(Lock {
                    status: LockStatus::Locked,
                    key: "It_Key_A".into(),
                    key_amount: 1,
                    difficulty: 5,
                }),
                loot: vec![("It_Gold".into(), 3)],
                ..Default::default()
            },
        );
        s.player_use_object(chest);
        let ev = play(&mut s, 200);
        assert!(
            ev.iter()
                .any(|e| matches!(&e.kind, EventKind::Voice { key, .. } if key == "NeedKey"))
        );
        let hero = s.hero.expect("hero");
        s.actor_mut(hero).inventory.insert("It_Key_A", 1);
        s.player_use_object(chest);
        let ev = play(&mut s, 400);
        assert!(
            ev.iter()
                .any(|e| matches!(&e.kind, EventKind::Player(PlayerEvent::Window(_))))
        );
    }
}
