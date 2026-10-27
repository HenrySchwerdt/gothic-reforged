//! Magic-side part of the in-memory test host ([`crate::magic::api::MagicApi`]).

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::enums::{DamageType, Slot, Species};
use crate::host::{EntityApi, EntityId, InventoryApi, Pos, WorldApi};
use crate::magic::api::{Boost, MagicApi, MagicEvent, ProjectileSpec, SpellHit, SpellInfo};
use crate::testhost::TestWorld;

/// Magic state of [`TestWorld`].
#[derive(Debug, Default)]
pub struct MagicSide {
    /// Spell templates by name.
    pub spells: BTreeMap<String, SpellInfo>,
    /// `PSInteraction.Spell` per caster.
    pub cast_spell: HashMap<EntityId, String>,
    /// `PSNpc.SpellTarget` per caster.
    pub spell_target: HashMap<EntityId, EntityId>,
    /// `PSNpc.ManaUsed` per caster.
    pub mana_used: HashMap<EntityId, i32>,
    /// `PSNpc.LastSpell` per caster.
    pub last_spell: HashMap<EntityId, String>,
    /// Item counts of spell stacks (default 1).
    pub amounts: HashMap<(EntityId, i32), i32>,
    /// Permanent stacks (runes).
    pub permanent: HashSet<(EntityId, i32)>,
    /// Projectiles launched.
    pub projectiles: Vec<ProjectileSpec>,
    /// Events emitted.
    pub events: Vec<MagicEvent>,
    /// Creatures spawned `(template, id)`.
    pub spawned: Vec<(String, EntityId)>,
    /// Area entities spawned `(template, owner, spell, at)`.
    pub areas: Vec<(String, EntityId, String, Pos)>,
    /// Spell hits dealt `(owner, victim, hit)`.
    pub hits: Vec<(EntityId, EntityId, SpellHit)>,
    /// Boosts started.
    pub boosts: Vec<(EntityId, Boost, f32, i32)>,
    /// Teleports `(who, destination)`.
    pub teleports: Vec<(EntityId, String)>,
    /// Transformations started `(who, form, secs)` and ended.
    pub transforms: Vec<(EntityId, EntityId, f32)>,
    /// Transformations ended.
    pub transforms_ended: Vec<EntityId>,
    /// Locks by object: difficulty.
    pub locks: HashMap<EntityId, i32>,
    /// Objects unlocked.
    pub unlocked: Vec<EntityId>,
    /// Entities removed.
    pub removed: Vec<EntityId>,
    /// Area elements dispelled.
    pub dispelled: Vec<DamageType>,
    /// Damage entities of the items in slots.
    pub slot_damage: HashMap<(EntityId, Slot), (DamageType, i32)>,
    /// Hit multiplier per character.
    pub hit_mult: HashMap<EntityId, f32>,
    /// Telekinesis calls.
    pub telekinesis: Vec<(EntityId, EntityId)>,
}

impl TestWorld {
    /// Register a spell template.
    pub fn add_spell(&mut self, spell: SpellInfo) {
        self.mg.spells.insert(spell.name.clone(), spell);
    }

    /// Magic events emitted so far.
    pub fn magic_events(&self) -> &[MagicEvent] {
        &self.mg.events
    }

    /// Mark the stack of a slot as permanent (a rune) or consumable (a scroll).
    pub fn set_permanent(&mut self, e: EntityId, slot: Slot, permanent: bool) {
        let key = (e, slot as i32 + 1);
        if permanent {
            self.mg.permanent.insert(key);
        } else {
            self.mg.permanent.remove(&key);
        }
    }

    fn slot_of(stack: i32) -> Slot {
        Slot::ALL[(stack - 1).clamp(0, Slot::ALL.len() as i32 - 1) as usize]
    }
}

impl MagicApi for TestWorld {
    fn spell(&self, name: &str) -> Option<SpellInfo> {
        self.mg.spells.get(name).cloned()
    }
    fn stack_spell(&self, e: EntityId, stack: i32) -> String {
        if stack < 1 {
            return String::new();
        }
        self.slot_spell_name(e, Self::slot_of(stack))
    }
    fn item_spell(&self, _template: &str) -> String {
        String::new()
    }
    fn stack_amount(&self, e: EntityId, stack: i32) -> i32 {
        self.mg.amounts.get(&(e, stack)).copied().unwrap_or(1)
    }
    fn stack_permanent(&self, e: EntityId, stack: i32) -> bool {
        self.mg.permanent.contains(&(e, stack))
    }
    fn stack_category(&self, e: EntityId, stack: i32) -> u32 {
        if stack < 1 {
            return 0;
        }
        self.slot_category(e, Self::slot_of(stack))
    }
    fn delete_stack_items(&mut self, e: EntityId, stack: i32, n: i32) {
        let left = self.stack_amount(e, stack) - n;
        self.mg.amounts.insert((e, stack), left.max(0));
        if left <= 0 {
            self.clear_slot(e, Self::slot_of(stack));
        }
    }
    fn cast_spell(&self, caster: EntityId) -> String {
        self.mg.cast_spell.get(&caster).cloned().unwrap_or_default()
    }
    fn set_cast_spell(&mut self, caster: EntityId, spell: &str) {
        self.mg.cast_spell.insert(caster, spell.into());
    }
    fn spell_target(&self, caster: EntityId) -> EntityId {
        self.mg
            .spell_target
            .get(&caster)
            .copied()
            .unwrap_or(EntityId::NONE)
    }
    fn set_spell_target(&mut self, caster: EntityId, target: EntityId) {
        self.mg.spell_target.insert(caster, target);
    }
    fn mana_used(&self, caster: EntityId) -> i32 {
        self.mg.mana_used.get(&caster).copied().unwrap_or(0)
    }
    fn set_mana_used(&mut self, caster: EntityId, n: i32) {
        self.mg.mana_used.insert(caster, n);
    }
    fn set_last_spell(&mut self, caster: EntityId, spell: &str) {
        self.mg.last_spell.insert(caster, spell.into());
    }
    fn slot_item_damage(&self, e: EntityId, slot: Slot) -> Option<(DamageType, i32)> {
        self.mg.slot_damage.get(&(e, slot)).copied()
    }
    fn hit_multiplier(&self, e: EntityId) -> f32 {
        self.mg.hit_mult.get(&e).copied().unwrap_or(1.0)
    }
    fn set_hit_multiplier(&mut self, e: EntityId, m: f32) {
        self.mg.hit_mult.insert(e, m);
    }
    fn spell_damage(&mut self, owner: EntityId, victim: EntityId, hit: SpellHit) {
        self.mg.hits.push((owner, victim, hit));
    }
    fn spawn_projectile(&mut self, p: ProjectileSpec) {
        self.mg.projectiles.push(p);
    }
    fn spawn_npc(&mut self, template: &str, near: EntityId) -> EntityId {
        let species = Species::from_name(template).unwrap_or(Species::Human);
        let id = self.spawn_npc(template, species, 5, 5);
        let at = EntityApi::position(self, near);
        self.set_position(id, at);
        self.mg.spawned.push((template.into(), id));
        id
    }
    fn spawn_area(&mut self, template: &str, owner: EntityId, spell: &str, at: Pos) {
        self.mg
            .areas
            .push((template.into(), owner, spell.into(), at));
    }
    fn remove_entity(&mut self, e: EntityId) {
        self.mg.removed.push(e);
    }
    fn characters_near(&self, centre: Pos, radius: f32) -> Vec<EntityId> {
        let mut v: Vec<(EntityId, f32)> = self
            .npc_ids()
            .into_iter()
            .map(|id| (id, EntityApi::position(self, id).distance(centre)))
            .filter(|(_, d)| *d <= radius)
            .collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        v.into_iter().map(|(id, _)| id).collect()
    }
    fn template_name(&self, e: EntityId) -> String {
        EntityApi::name(self, e)
    }
    fn lock_difficulty(&self, object: EntityId) -> Option<i32> {
        self.mg.locks.get(&object).copied()
    }
    fn all_npcs_by_distance(&self, from: EntityId) -> Vec<EntityId> {
        let mut v: Vec<(EntityId, f32)> = WorldApi::all_npcs(self)
            .into_iter()
            .map(|id| (id, WorldApi::distance(self, from, id)))
            .collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        v.into_iter().map(|(id, _)| id).collect()
    }
    fn can_goto(&self, _npc: EntityId, _target: EntityId) -> bool {
        true
    }
    fn set_boost(&mut self, who: EntityId, boost: Boost, secs: f32, level: i32) {
        self.mg.boosts.push((who, boost, secs, level));
    }
    fn dispel_areas(&mut self, _centre: Pos, _radius: f32, element: DamageType) {
        self.mg.dispelled.push(element);
    }
    fn teleport(&mut self, who: EntityId, destination: &str) {
        self.mg.teleports.push((who, destination.into()));
    }
    fn begin_transform(&mut self, who: EntityId, form: EntityId, secs: f32) {
        self.mg.transforms.push((who, form, secs));
        self.set_transformed(who, true);
    }
    fn transform_form(&self, who: EntityId) -> EntityId {
        self.mg
            .transforms
            .iter()
            .rev()
            .find(|t| t.0 == who && !self.mg.transforms_ended.contains(&who))
            .map_or(EntityId::NONE, |t| t.1)
    }
    fn end_transform(&mut self, who: EntityId) {
        self.mg.transforms_ended.push(who);
        self.set_transformed(who, false);
    }
    fn unlock_object(&mut self, object: EntityId) {
        self.mg.locks.remove(&object);
        self.mg.unlocked.push(object);
    }
    fn telekinesis(&mut self, caster: EntityId, target: EntityId) {
        self.mg.telekinesis.push((caster, target));
    }
    fn emit_magic(&mut self, event: MagicEvent) {
        self.mg.events.push(event);
    }
}
