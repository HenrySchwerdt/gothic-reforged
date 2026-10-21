//! Player-side part of the in-memory test host ([`crate::player::api::PlayerApi`]).

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::enums::UseType;
use crate::host::EntityId;
use crate::player::api::{
    Book, BoulderClass, FocusKind, Lock, LockStatus, PlayerApi, PlayerEvent, Recipe, SkillCategory,
    VoiceCounter,
};
use crate::testhost::TestWorld;

/// An interact object of the test world.
#[derive(Debug, Clone, Default)]
pub struct TestObject {
    /// Lock, if any.
    pub lock: Option<Lock>,
    /// Door state.
    pub open: bool,
    /// Used up by the player.
    pub used: bool,
    /// Game event raised on use.
    pub event: String,
    /// Owner enclave.
    pub owner: EntityId,
    /// Loot stacks.
    pub loot: Vec<(String, u32)>,
}

/// Player-side state of [`TestWorld`].
#[derive(Debug, Default)]
pub struct PlayerSide {
    /// Objects by entity.
    pub objects: BTreeMap<EntityId, TestObject>,
    /// Voice counters.
    pub counters: HashMap<VoiceCounter, u32>,
    /// Recipes by id.
    pub recipes: BTreeMap<i32, Recipe>,
    /// Item use types.
    pub use_types: HashMap<String, UseType>,
    /// Equipped templates.
    pub equipped: HashSet<String>,
    /// Recipe categories the player holds items for.
    pub recipe_categories: HashSet<SkillCategory>,
    /// A blade weapon is in the bag.
    pub blade: bool,
    /// Events emitted so far.
    pub events: Vec<PlayerEvent>,
    /// Hours advanced.
    pub hours: f32,
    /// Raised game events.
    pub raised: Vec<String>,
    /// Pages read per book.
    pub books: HashMap<Book, i32>,
    /// Weight class per boulder.
    pub boulders: HashMap<EntityId, BoulderClass>,
    /// Alignment target per entity.
    pub alignment: HashMap<EntityId, EntityId>,
    /// Movement constraint per entity (degrees).
    pub constraints: HashMap<EntityId, f32>,
    /// The player's action queue.
    pub queue: std::collections::VecDeque<(u32, EntityId)>,
    /// The melee-mode focus the engine search found.
    pub melee_focus: EntityId,
    /// The focus entity.
    pub focus: EntityId,
    /// Focus settings written by the script: current mode, look-at mode, range rating.
    pub focus_modes: (u32, u32, bool),
    /// First-person camera.
    pub first_person: bool,
    /// Camera mode last set.
    pub camera_mode: u32,
    /// A movement key is held.
    pub movement_key: bool,
    /// `LastWeaponConfig`.
    pub weapon_config: u32,
    /// Azimuth / elevation the engine would report, per `(from, to)`.
    pub aziel: HashMap<(EntityId, EntityId), (f32, f32)>,
    /// `HitDirection` per entity.
    pub hit_dir: HashMap<EntityId, u32>,
}

impl TestWorld {
    /// Create an interact object (a point with object data).
    pub fn add_object(&mut self, use_type: UseType, object: TestObject) -> EntityId {
        let id = self.add_point(use_type, EntityId::NONE);
        self.pl.objects.insert(id, object);
        id
    }
    /// Register a recipe.
    pub fn add_recipe(&mut self, id: i32, recipe: Recipe) {
        self.pl.recipes.insert(id, recipe);
    }
    /// Set the use type of an item template.
    pub fn set_item_use_type(&mut self, template: &str, u: UseType) {
        self.pl.use_types.insert(template.into(), u);
    }
    /// Events emitted for the game layer so far.
    pub fn events(&self) -> &[PlayerEvent] {
        &self.pl.events
    }
    /// The player holds a recipe item of this category.
    pub fn grant_recipes(&mut self, c: SkillCategory) {
        self.pl.recipe_categories.insert(c);
    }
    /// Mutable object data.
    pub fn object_mut(&mut self, e: EntityId) -> &mut TestObject {
        self.pl.objects.entry(e).or_default()
    }
    fn remove_n(&mut self, e: EntityId, template: &str, amount: u32) -> u32 {
        let Some(items) = self.items_mut(e) else {
            return 0;
        };
        let mut removed = 0;
        while removed < amount {
            match items.iter().position(|i| i == template) {
                Some(i) => {
                    items.remove(i);
                    removed += 1;
                }
                None => break,
            }
        }
        removed
    }
}

impl PlayerApi for TestWorld {
    fn set_alignment_target(&mut self, e: EntityId, target: EntityId) {
        self.pl.alignment.insert(e, target);
    }
    fn set_movement_constraints(&mut self, e: EntityId, degrees: f32) {
        self.pl.constraints.insert(e, degrees);
    }
    fn action_queue_head(&self) -> Option<(u32, EntityId)> {
        self.pl.queue.front().copied()
    }
    fn action_queue_push(&mut self, id: u32, entity: EntityId) {
        self.pl.queue.push_back((id, entity));
    }
    fn action_queue_pop(&mut self) {
        self.pl.queue.pop_front();
    }
    fn action_queue_clear(&mut self) {
        self.pl.queue.clear();
    }
    fn melee_focus(&self) -> EntityId {
        self.pl.melee_focus
    }
    fn current_focus(&self) -> EntityId {
        self.pl.focus
    }
    fn set_focus_entity(&mut self, e: EntityId) {
        self.pl.focus = e;
    }
    fn set_focus_current_mode(&mut self, mode: u32) {
        self.pl.focus_modes.0 = mode;
    }
    fn set_focus_look_at_mode(&mut self, mode: u32) {
        self.pl.focus_modes.1 = mode;
    }
    fn set_focus_range_rating(&mut self, on: bool) {
        self.pl.focus_modes.2 = on;
    }
    fn camera_in_first_person(&self) -> bool {
        self.pl.first_person
    }
    fn set_camera_mode(&mut self, mode: u32) {
        self.pl.camera_mode = mode;
    }
    fn movement_key_down(&self) -> bool {
        self.pl.movement_key
    }
    fn last_weapon_config(&self) -> u32 {
        self.pl.weapon_config
    }
    fn set_last_weapon_config(&mut self, v: u32) {
        self.pl.weapon_config = v;
    }
    fn azimuth_elevation(&self, from: EntityId, to: EntityId) -> (f32, f32) {
        self.pl
            .aziel
            .get(&(from, to))
            .copied()
            .unwrap_or((0.0, 0.0))
    }
    fn hit_direction(&self, e: EntityId) -> u32 {
        self.pl.hit_dir.get(&e).copied().unwrap_or(0)
    }
    fn set_hit_direction(&mut self, e: EntityId, dir: u32) {
        self.pl.hit_dir.insert(e, dir);
    }
    fn book_progress(&self, book: Book) -> i32 {
        self.pl.books.get(&book).copied().unwrap_or(0)
    }
    fn set_book_progress(&mut self, book: Book, pages: i32) {
        self.pl.books.insert(book, pages);
    }
    fn boulder_class(&self, object: EntityId) -> BoulderClass {
        self.pl.boulders.get(&object).copied().unwrap_or_default()
    }
    fn voice_counter(&self, c: VoiceCounter) -> u32 {
        self.pl.counters.get(&c).copied().unwrap_or(0)
    }
    fn set_voice_counter(&mut self, c: VoiceCounter, v: u32) {
        self.pl.counters.insert(c, v);
    }
    fn lock(&self, o: EntityId) -> Option<Lock> {
        self.pl.objects.get(&o).and_then(|x| x.lock.clone())
    }
    fn set_lock_status(&mut self, o: EntityId, s: LockStatus) {
        if let Some(l) = self.pl.objects.get_mut(&o).and_then(|x| x.lock.as_mut()) {
            l.status = s;
        }
    }
    fn door_open(&self, o: EntityId) -> bool {
        self.pl.objects.get(&o).is_some_and(|x| x.open)
    }
    fn set_door_open(&mut self, o: EntityId, v: bool) {
        self.object_mut(o).open = v;
    }
    fn used_by_player(&self, o: EntityId) -> bool {
        self.pl.objects.get(&o).is_some_and(|x| x.used)
    }
    fn set_used_by_player(&mut self, o: EntityId, v: bool) {
        self.object_mut(o).used = v;
    }
    fn use_event(&self, o: EntityId) -> String {
        self.pl
            .objects
            .get(&o)
            .map(|x| x.event.clone())
            .unwrap_or_default()
    }
    fn owner_enclave(&self, o: EntityId) -> EntityId {
        self.pl.objects.get(&o).map_or(EntityId::NONE, |x| x.owner)
    }
    fn loot_stacks(&self, o: EntityId) -> i32 {
        self.pl.objects.get(&o).map_or(0, |x| x.loot.len() as i32)
    }
    fn generate_loot(&mut self, o: EntityId) -> i32 {
        self.loot_stacks(o)
    }
    fn focus_kind(&self, e: EntityId) -> FocusKind {
        if self.is_npc(e) {
            FocusKind::Npc
        } else if self.is_item(e) {
            FocusKind::Item
        } else if self.is_point(e) {
            FocusKind::Object
        } else {
            FocusKind::None
        }
    }
    fn lootable(&self, e: EntityId) -> Vec<(String, u32)> {
        let mut out: Vec<(String, u32)> = Vec::new();
        for i in self.items_of(e) {
            match out.iter_mut().find(|(t, _)| *t == i) {
                Some((_, c)) => *c += 1,
                None => out.push((i, 1)),
            }
        }
        out
    }
    fn transfer_item(&mut self, from: EntityId, to: EntityId, template: &str, amount: u32) {
        let moved = self.remove_n(from, template, amount);
        for _ in 0..moved {
            self.give_item(to, template);
        }
    }
    fn item_template(&self, item: EntityId) -> String {
        self.item_name(item)
    }
    fn take_item(&mut self, taker: EntityId, item: EntityId) {
        let name = self.item_name(item);
        self.remove_item_entity(item);
        self.give_item(taker, &name);
    }
    fn add_item(&mut self, e: EntityId, template: &str, amount: u32) {
        for _ in 0..amount {
            self.give_item(e, template);
        }
    }
    fn remove_item(&mut self, e: EntityId, template: &str, amount: u32) -> bool {
        self.remove_n(e, template, amount) == amount
    }
    fn item_count(&self, e: EntityId, template: &str) -> u32 {
        self.items_of(e).iter().filter(|i| *i == template).count() as u32
    }
    fn has_recipes(&self, _e: EntityId, c: SkillCategory) -> bool {
        self.pl.recipe_categories.contains(&c)
    }
    fn has_blade_weapon(&self, _e: EntityId) -> bool {
        self.pl.blade
    }
    fn item_use_type(&self, t: &str) -> UseType {
        self.pl.use_types.get(t).copied().unwrap_or(UseType::None)
    }
    fn equip_item(&mut self, _e: EntityId, t: &str) -> bool {
        self.pl.equipped.insert(t.into());
        true
    }
    fn unequip_item(&mut self, _e: EntityId, t: &str) {
        self.pl.equipped.remove(t);
    }
    fn is_equipped(&self, _e: EntityId, t: &str) -> bool {
        self.pl.equipped.contains(t)
    }
    fn consume_item(&mut self, e: EntityId, t: &str) -> bool {
        self.remove_n(e, t, 1) == 1
    }
    fn recipe(&self, id: i32) -> Option<Recipe> {
        self.pl.recipes.get(&id).cloned()
    }
    fn known_recipes(&self) -> Vec<i32> {
        self.pl.recipes.keys().copied().collect()
    }
    fn raise_game_event(&mut self, name: &str) {
        self.pl.raised.push(name.into());
        self.set_game_event(name);
    }
    fn advance_hours(&mut self, hours: f32) {
        self.pl.hours += hours;
        self.hour = (self.hour + hours).rem_euclid(24.0);
    }
    fn emit(&mut self, event: PlayerEvent) {
        self.pl.events.push(event);
    }
}
