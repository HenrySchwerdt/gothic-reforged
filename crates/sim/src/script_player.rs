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
    objects: BTreeMap<u32, ObjectData>,
    counters: [u32; 2],
    recipes: BTreeMap<i32, Recipe>,
    recipe_items: BTreeMap<String, SkillCategory>,
    blade_items: BTreeSet<String>,
    /// The hero is busy (an animation plays) until this simulation time.
    pub busy_until_ms: u64,
    books: [i32; 3],
    boulders: BTreeMap<u32, gothic_script::player::api::BoulderClass>,
}

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
            Some(id) => self.sim.equip(id, template).is_ok(),
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

    /// Start a melee move against an actor.
    pub fn player_melee(&mut self, action: se::Action, target: ActorId) -> bool {
        self.with_hero(|h, p| commands::melee_move(h, p, action, entity_of(target)))
            .unwrap_or(false)
    }

    /// Raise the guard against an actor (`PS_Melee_Parade`).
    pub fn player_parade(&mut self, target: ActorId) {
        self.with_hero(|h, p| commands::parade(h, p, entity_of(target)));
    }

    /// Take everything from a body.
    pub fn player_quick_loot(&mut self, body: EntityId) -> usize {
        self.with_hero(|h, p| gothic_script::player::normal::quick_loot(h, p, body))
            .unwrap_or(0)
    }

    /// Step the hero's current state and execute what it queued. Called once per tick; an
    /// animation (an `Action`, a wait) keeps the hero busy for its length.
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
        if now < self.script.player.busy_until_ms {
            return;
        }
        let ent = entity_of(hero);
        let cmds = {
            let mut host = SimHost::new(self);
            host.self_entity = ent;
            if !host.current_state(ent).is_some_and(|s| {
                let n = s.name();
                n.starts_with("PS_") || n.starts_with("BS_") || n.starts_with("ZS_Evade")
            }) {
                return;
            }
            commands::run(&mut host, ent);
            host.sim.script.take_commands_for(hero)
        };
        let mut busy = 0u64;
        for cmd in cmds {
            busy += match cmd {
                AiCommand::Action { secs, .. } | AiCommand::Wait { secs } => {
                    secs.max(0) as u64 * 1000
                }
                AiCommand::WaitMs { ms } => u64::from(ms),
                AiCommand::ChangeAction
                | AiCommand::StandUp
                | AiCommand::HoldInventoryStacks { .. } => 400,
                AiCommand::AniStateAction(_) => 500,
                AiCommand::TakeItem { .. } => 800,
                AiCommand::SayLine { key } => {
                    self.speak(hero, key.to_string());
                    0
                }
                AiCommand::Attack { target, .. } => {
                    if let Some(t) = actor_of(target) {
                        self.melee_hit(hero, t);
                    }
                    600
                }
                _ => 200,
            };
        }
        self.script.player.busy_until_ms = now + busy;
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
                    | State::PsRangedAimLoop,
            )
        )
    }
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
