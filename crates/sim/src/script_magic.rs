//! Magic side of the script adapter: spell templates, projectiles in flight, area-spell
//! entities, timed boosts and transformations, and the hero's cast and ranged entry points.
//!
//! * Spell numbers come from the spell templates: [`Sim::load_spells`] reads the template export
//!   (`.local/spells.json`, see [`SpellRecord`]), tests register [`SpellInfo`]s directly.
//! * [`MagicApi`] for [`SimHost`] lets the ported cast pipeline (`gothic_script::magic`) launch
//!   projectiles, summon creatures, start area effects and change status bits.
//! * [`Sim::tick_magic`] runs once per step: projectiles arrive (arrows and spell missiles hit
//!   through the normal damage pipeline), area effects tick, boosts and transformations expire.

use std::collections::BTreeMap;

use gothic_script::enums as se;
use gothic_script::host::{EntityId, Pos as SPos};
use gothic_script::magic::api::{
    Boost, CastType, MagicApi, MagicEvent, PathStyle, ProjectileSpec, SpellDamage, SpellHit,
    SpellInfo, SpellType,
};
use gothic_script::magic::area::{AreaEffect, AreaKind};
use gothic_script::player::api::{LockStatus, PlayerApi};
use gothic_script::player::commands;
use gothic_script::routine::state::State;
use serde::Deserialize;

use crate::combat::{Attack, Deity};
use crate::item::ItemDef;
use crate::script_host::{ScriptTables, SimHost, actor_of, entity_of};
use crate::sim::{EventKind, Sim};
use crate::types::{ActorId, DamageType};
use crate::world::{Pos, WorldQuery};

/// Default `ShootVelocity` of a spell missile (the template value of `Mis_Fireball`).
pub const DEFAULT_MISSILE_SPEED: f32 = 100.0;
/// Template speeds are metres per second; positions are centimetres.
const SPEED_UNIT_CM: f32 = 100.0;

/// A projectile on its way.
#[derive(Debug, Clone, PartialEq)]
pub struct Flight {
    /// What was launched.
    pub spec: ProjectileSpec,
    /// Simulation time of the impact.
    pub arrives_ms: u64,
}

/// A running boost of the hero.
#[derive(Debug, Clone, PartialEq)]
pub struct BoostRec {
    /// Who.
    pub who: ActorId,
    /// Which.
    pub boost: Boost,
    /// Simulation time it ends.
    pub until_ms: u64,
    /// Level (kept stamina).
    pub level: i32,
}

/// A running transformation.
#[derive(Debug, Clone, PartialEq)]
pub struct TransformRec {
    /// The transformed character.
    pub who: ActorId,
    /// The body it wears.
    pub form: ActorId,
    /// Simulation time it ends.
    pub until_ms: u64,
}

#[derive(Debug, Clone, Default)]
struct CastRec {
    spell: String,
    target: EntityId,
    mana_used: i32,
    last_spell: String,
}

/// Magic state of a simulation.
#[derive(Debug, Clone, Default)]
pub struct MagicTables {
    /// Spell templates by name.
    pub spells: BTreeMap<String, SpellInfo>,
    /// Projectiles in flight.
    pub flights: Vec<Flight>,
    /// Running area effects.
    pub areas: Vec<AreaEffect>,
    /// Running boosts.
    pub boosts: Vec<BoostRec>,
    /// Running transformations.
    pub transforms: Vec<TransformRec>,
    /// Named teleport destinations (`Teleport_KapDun`).
    pub waypoints: BTreeMap<String, Pos>,
    casts: BTreeMap<u32, CastRec>,
    hit_mult: BTreeMap<u32, f32>,
}

impl ScriptTables {
    /// Register a spell template.
    pub fn add_spell(&mut self, spell: SpellInfo) {
        self.magic.spells.insert(spell.name.clone(), spell);
    }
    /// Register a teleport destination.
    pub fn add_waypoint(&mut self, name: &str, at: Pos) {
        self.magic.waypoints.insert(name.into(), at);
    }
}

/// One entry of the spell template export (`spells.json`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct SpellRecord {
    /// `Spl_*` name.
    pub name: String,
    /// `SpellType` raw.
    pub spell_type: u32,
    /// `SpellDeity` raw (0 Adanos, 1 Beliar, 2 Innos, 3 none).
    pub deity: u32,
    /// `MinManaCost`.
    pub min_mana: i32,
    /// `MaxManaCost`.
    pub max_mana: i32,
    /// `SpellTarget` raw.
    pub target: u32,
    /// `CastType` raw.
    pub cast_type: u32,
    /// `FuncOnCast`.
    pub func_on_cast: String,
    /// `FuncOnTargetHit`.
    pub func_on_hit: String,
    /// `AnimActionCast`.
    pub anim_cast: String,
    /// `AnimActionPowerCast`.
    pub anim_power: String,
    /// Spawned template with its damage entity.
    pub spawn: Option<SpawnRecord>,
}

/// The `Spawn` of a spell template.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct SpawnRecord {
    /// Template name (`Mis_Fireball`), absent when the template is not in the export.
    pub template: Option<String>,
    /// `PSDamage` of it.
    pub damage: Option<DamageRecord>,
    /// `PSProjectile` of it.
    pub projectile: Option<ProjectileRecord>,
}

/// `PSDamage` values.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct DamageRecord {
    /// `DamageType` raw.
    #[serde(rename = "DamageType")]
    pub damage_type: u32,
    /// `DamageAmount`.
    #[serde(rename = "DamageAmount")]
    pub amount: i32,
    /// `DamageManaMultiplier`.
    #[serde(rename = "DamageManaMultiplier")]
    pub mana_mult: f32,
    /// `DamageHitMultiplier`.
    #[serde(rename = "DamageHitMultiplier")]
    pub hit_mult: f32,
}

/// `PSProjectile` values.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ProjectileRecord {
    /// `ShootVelocity`.
    #[serde(rename = "ShootVelocity")]
    pub velocity: f32,
}

fn script_damage_type(raw: u32) -> se::DamageType {
    se::DamageType::from_raw(raw).unwrap_or(se::DamageType::None)
}

impl SpellRecord {
    /// The script-side spell template.
    pub fn to_info(&self) -> SpellInfo {
        let mut s = SpellInfo::new(
            &self.name,
            SpellType::from_raw(self.spell_type),
            CastType::from_raw(self.cast_type),
        );
        s.deity = se::SpellDeity::from_raw(self.deity).unwrap_or(se::SpellDeity::None);
        s.needs_focus = self.target == 1;
        s.min_mana = self.min_mana;
        s.max_mana = self.max_mana;
        s.func_on_cast = self.func_on_cast.clone();
        s.func_on_hit = self.func_on_hit.clone();
        s.anim_cast = self.anim_cast.clone();
        s.anim_power = self.anim_power.clone();
        if let Some(sp) = &self.spawn {
            s.spawn = sp.template.clone().unwrap_or_default();
            s.damage = sp.damage.as_ref().map(|d| SpellDamage {
                damage_type: script_damage_type(d.damage_type),
                amount: d.amount,
                mana_mult: d.mana_mult,
                hit_mult: if d.hit_mult > 0.0 { d.hit_mult } else { 1.0 },
            });
            if let Some(p) = &sp.projectile {
                s.velocity = if p.velocity > 0.0 {
                    p.velocity
                } else {
                    DEFAULT_MISSILE_SPEED
                };
            }
        }
        s
    }
}

/// Read a spell export (`[SpellRecord]`) into script templates.
pub fn parse_spells(json: &str) -> Result<Vec<SpellInfo>, serde_json::Error> {
    let records: Vec<SpellRecord> = serde_json::from_str(json)?;
    Ok(records.iter().map(SpellRecord::to_info).collect())
}

fn deity_of(d: se::SpellDeity) -> Option<Deity> {
    match d {
        se::SpellDeity::Adanos => Some(Deity::Adanos),
        se::SpellDeity::Beliar => Some(Deity::Beliar),
        se::SpellDeity::Innos => Some(Deity::Innos),
        se::SpellDeity::None => None,
    }
}

fn sim_damage_type(t: se::DamageType) -> DamageType {
    match t {
        se::DamageType::None => DamageType::None,
        se::DamageType::Impact => DamageType::Impact,
        se::DamageType::Blade => DamageType::Blade,
        se::DamageType::Missile => DamageType::Missile,
        se::DamageType::Fire => DamageType::Fire,
        se::DamageType::Ice => DamageType::Ice,
        se::DamageType::Lightning => DamageType::Lightning,
        se::DamageType::Physics => DamageType::Physics,
    }
}

fn spell_attack(spell: &str, deity: se::SpellDeity, h: &SpellHit, hit_mult: f32) -> Attack {
    let mut a = Attack::spell(
        spell,
        sim_damage_type(h.damage_type),
        h.amount,
        h.mana_used,
        deity_of(deity).unwrap_or(Deity::Innos),
    );
    a.deity = deity_of(deity);
    a.hit_mult = hit_mult;
    a
}

impl<W: WorldQuery> SimHost<'_, W> {
    fn cast_rec(&mut self, caster: EntityId) -> &mut CastRec {
        self.sim.script.magic.casts.entry(caster.0).or_default()
    }

    fn slot_item(&self, e: EntityId, slot: se::Slot) -> Option<&ItemDef> {
        let a = self.actor(e)?;
        match slot {
            se::Slot::RightHand => a.equipment.right.as_ref(),
            se::Slot::LeftHand | se::Slot::EquipLeftHand => a.equipment.left.as_ref(),
            _ => None,
        }
    }

    fn stack_slot(stack: i32) -> Option<se::Slot> {
        if stack < 1 {
            None
        } else {
            se::Slot::ALL.get(stack as usize - 1).copied()
        }
    }

    fn stack_item(&self, e: EntityId, stack: i32) -> Option<&ItemDef> {
        self.slot_item(e, Self::stack_slot(stack)?)
    }
}

impl<W: WorldQuery> MagicApi for SimHost<'_, W> {
    fn spell(&self, name: &str) -> Option<SpellInfo> {
        self.sim.script.magic.spells.get(name).cloned()
    }
    fn stack_spell(&self, e: EntityId, stack: i32) -> String {
        self.stack_item(e, stack)
            .and_then(|i| i.spell.clone())
            .unwrap_or_default()
    }
    fn stack_amount(&self, e: EntityId, stack: i32) -> i32 {
        let Some(item) = self.stack_item(e, stack) else {
            return 0;
        };
        let name = item.name.clone();
        let held = self
            .actor(e)
            .map_or(0, |a| a.inventory.count(&name) as i32);
        held.max(1)
    }
    fn stack_permanent(&self, e: EntityId, stack: i32) -> bool {
        self.stack_item(e, stack).is_some_and(|i| i.permanent)
    }
    fn stack_category(&self, e: EntityId, stack: i32) -> u32 {
        match self.stack_item(e, stack).map(|i| i.category.as_str()) {
            Some("Weapon") => 1,
            Some("Armor") => 2,
            Some("Artefact") => 3,
            Some("Alchemy") => 4,
            Some("Misc") => 6,
            Some("Written") => 7,
            Some("Spellbook") => 9,
            Some("Skill") => 10,
            _ => 0,
        }
    }
    fn delete_stack_items(&mut self, e: EntityId, stack: i32, n: i32) {
        let Some(slot) = Self::stack_slot(stack) else {
            return;
        };
        let Some(name) = self.slot_item(e, slot).map(|i| i.name.clone()) else {
            return;
        };
        let Some(a) = self.actor_mut(e) else { return };
        let take = (n.max(0) as u32).min(a.inventory.count(&name));
        a.inventory.take(&name, take);
        if a.inventory.count(&name) == 0 {
            match slot {
                se::Slot::RightHand => a.equipment.right = None,
                _ => a.equipment.left = None,
            }
        }
    }
    fn cast_spell(&self, caster: EntityId) -> String {
        self.sim
            .script
            .magic
            .casts
            .get(&caster.0)
            .map(|c| c.spell.clone())
            .unwrap_or_default()
    }
    fn set_cast_spell(&mut self, caster: EntityId, spell: &str) {
        self.cast_rec(caster).spell = spell.into();
    }
    fn spell_target(&self, caster: EntityId) -> EntityId {
        self.sim
            .script
            .magic
            .casts
            .get(&caster.0)
            .map_or(EntityId::NONE, |c| c.target)
    }
    fn set_spell_target(&mut self, caster: EntityId, target: EntityId) {
        self.cast_rec(caster).target = target;
    }
    fn mana_used(&self, caster: EntityId) -> i32 {
        self.sim
            .script
            .magic
            .casts
            .get(&caster.0)
            .map_or(0, |c| c.mana_used)
    }
    fn set_mana_used(&mut self, caster: EntityId, n: i32) {
        self.cast_rec(caster).mana_used = n;
    }
    fn set_last_spell(&mut self, caster: EntityId, spell: &str) {
        self.cast_rec(caster).last_spell = spell.into();
    }
    fn slot_item_damage(&self, e: EntityId, slot: se::Slot) -> Option<(se::DamageType, i32)> {
        let d = self.slot_item(e, slot)?.damage.as_ref()?;
        let ty =
            se::DamageType::from_name(&format!("{:?}", d.kind())).unwrap_or(se::DamageType::None);
        Some((ty, d.amount))
    }
    fn hit_multiplier(&self, e: EntityId) -> f32 {
        self.sim
            .script
            .magic
            .hit_mult
            .get(&e.0)
            .copied()
            .unwrap_or(1.0)
    }
    fn set_hit_multiplier(&mut self, e: EntityId, m: f32) {
        self.sim.script.magic.hit_mult.insert(e.0, m);
    }
    fn spell_damage(&mut self, owner: EntityId, victim: EntityId, hit: SpellHit) {
        let (Some(att), Some(vic)) = (actor_of(owner), actor_of(victim)) else {
            return;
        };
        let deity = hit.deity;
        let attack = spell_attack(&hit.spell, deity, &hit, 1.0);
        self.sim.hit(att, vic, attack);
    }
    fn spawn_projectile(&mut self, p: ProjectileSpec) {
        let now = self.sim.now_ms();
        let from = self.entity_pos(p.owner);
        let to = if p.target.is_some() {
            self.entity_pos(p.target)
        } else {
            from
        };
        let speed = if p.velocity > 0.0 {
            p.velocity
        } else {
            self.sim
                .script
                .magic
                .spells
                .get(&p.spell)
                .map_or(DEFAULT_MISSILE_SPEED, |s| s.velocity)
        };
        let ms = (from.dist(to) / (speed * SPEED_UNIT_CM) * 1000.0) as u64;
        if let (Some(owner), target) = (actor_of(p.owner), actor_of(p.target)) {
            self.sim.push(EventKind::Projectile {
                owner,
                target,
                template: p.template.clone(),
                spell: !p.spell.is_empty(),
            });
        }
        self.sim.script.magic.flights.push(Flight {
            spec: p,
            arrives_ms: now + ms,
        });
    }
    fn spawn_npc(&mut self, template: &str, near: EntityId) -> EntityId {
        let at = self.entity_pos(near);
        let spot = Pos::new(at.x + 150.0, at.y, at.z);
        let id = match self.sim.npc_templates.get(template).cloned() {
            Some(rec) => self.sim.spawn_record(&rec, spot),
            None => {
                let species = crate::types::Species::from_name(
                    template.split('_').next().unwrap_or(template),
                );
                let a = crate::actor::Actor::npc(ActorId(0), template, species, 10, 10);
                self.sim.spawn(a, spot)
            }
        };
        entity_of(id)
    }
    fn spawn_area(&mut self, template: &str, owner: EntityId, spell: &str, at: SPos) {
        let Some(kind) = AreaKind::from_template(template) else {
            return;
        };
        let mana = self.mana_used(owner);
        if matches!(kind, AreaKind::Mist | AreaKind::MistNoAggro) {
            // A new mist ends the old one (`ZS_Mist` sends the others to decay).
            self.sim
                .script
                .magic
                .areas
                .retain(|a| !matches!(a.kind, AreaKind::Mist | AreaKind::MistNoAggro));
        }
        let fx = AreaEffect::new(self, kind, owner, spell, at, mana);
        self.sim.script.magic.areas.push(fx);
    }
    fn remove_entity(&mut self, e: EntityId) {
        if let Some(a) = actor_of(e) {
            self.sim.kill(a, None);
        }
    }
    fn characters_near(&self, centre: SPos, radius: f32) -> Vec<EntityId> {
        let c = Pos::new(centre.x, centre.y, centre.z);
        let mut v: Vec<(EntityId, f32)> = self
            .sim
            .actors
            .iter()
            .filter(|a| a.ai_mode != crate::types::AiMode::Dead)
            .filter_map(|a| {
                let d = self.sim.world.position(a.id)?.dist(c);
                (d <= radius).then_some((entity_of(a.id), d))
            })
            .collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        v.into_iter().map(|(e, _)| e).collect()
    }
    fn template_name(&self, e: EntityId) -> String {
        self.actor(e).map(|a| a.name.clone()).unwrap_or_default()
    }
    fn lock_difficulty(&self, object: EntityId) -> Option<i32> {
        PlayerApi::lock(self, object).map(|l| l.difficulty as i32)
    }
    fn all_npcs_by_distance(&self, from: EntityId) -> Vec<EntityId> {
        let origin = self.entity_pos(from);
        let mut v: Vec<(EntityId, f32)> = self
            .sim
            .actors
            .iter()
            .filter(|a| !a.is_player)
            .filter_map(|a| {
                let d = self.sim.world.position(a.id)?.dist(origin);
                Some((entity_of(a.id), d))
            })
            .collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        v.into_iter().map(|(e, _)| e).collect()
    }
    fn can_goto(&self, _npc: EntityId, _target: EntityId) -> bool {
        true
    }
    fn set_boost(&mut self, who: EntityId, boost: Boost, secs: f32, level: i32) {
        let Some(w) = actor_of(who) else { return };
        let until = self.sim.now_ms() + (secs * 1000.0) as u64;
        self.sim
            .script
            .magic
            .boosts
            .retain(|b| b.boost != boost || b.who != w);
        self.sim.script.magic.boosts.push(BoostRec {
            who: w,
            boost,
            until_ms: until,
            level,
        });
        self.sim.push(EventKind::Magic(MagicEvent::Time {
            scale: if boost == Boost::SpeedUp { 1.5 } else { 1.0 },
            secs,
        }));
    }
    fn dispel_areas(&mut self, centre: SPos, radius: f32, element: se::DamageType) {
        self.sim.script.magic.areas.retain(|fx| {
            let d = fx.at.distance(centre);
            let matches = fx
                .kind
                .config()
                .is_some_and(|c| sim_damage_type(element) == sim_damage_type(c.damage_type));
            !(d <= radius && matches)
        });
    }
    fn teleport(&mut self, who: EntityId, destination: &str) {
        let Some(a) = actor_of(who) else { return };
        if let Some(at) = self.sim.script.magic.waypoints.get(destination).copied() {
            self.sim.world.teleport(a, at);
        }
    }
    fn begin_transform(&mut self, who: EntityId, form: EntityId, secs: f32) {
        let (Some(w), Some(f)) = (actor_of(who), actor_of(form)) else {
            return;
        };
        let until = self.sim.now_ms() + (secs * 1000.0) as u64;
        self.sim.actors[w.0 as usize].transformed = true;
        self.sim.script.magic.transforms.push(TransformRec {
            who: w,
            form: f,
            until_ms: until,
        });
    }
    fn transform_form(&self, who: EntityId) -> EntityId {
        let Some(w) = actor_of(who) else {
            return EntityId::NONE;
        };
        self.sim
            .script
            .magic
            .transforms
            .iter()
            .find(|t| t.who == w)
            .map_or(EntityId::NONE, |t| entity_of(t.form))
    }
    fn end_transform(&mut self, who: EntityId) {
        let Some(w) = actor_of(who) else { return };
        self.sim.actors[w.0 as usize].transformed = false;
        let mut forms = Vec::new();
        self.sim.script.magic.transforms.retain(|t| {
            if t.who == w {
                forms.push(t.form);
                false
            } else {
                true
            }
        });
        for f in forms {
            self.sim.kill(f, None);
        }
    }
    fn unlock_object(&mut self, object: EntityId) {
        PlayerApi::set_lock_status(self, object, LockStatus::Unlocked);
    }
    fn telekinesis(&mut self, _caster: EntityId, _target: EntityId) {}
    fn emit_magic(&mut self, event: MagicEvent) {
        self.sim.push(EventKind::Magic(event));
    }
}

impl<W: WorldQuery> Sim<W> {
    /// Register the spell templates of an export (`spells.json` text).
    pub fn load_spells(&mut self, json: &str) -> Result<usize, serde_json::Error> {
        let spells = parse_spells(json)?;
        let n = spells.len();
        for s in spells {
            self.script.add_spell(s);
        }
        Ok(n)
    }

    /// Advance projectiles, area effects, boosts and transformations by one step.
    pub fn tick_magic(&mut self) {
        let now = self.now_ms();
        let dt = self.cfg.tick_ms;
        // Projectiles that arrive.
        let (arrived, flying): (Vec<_>, Vec<_>) = std::mem::take(&mut self.script.magic.flights)
            .into_iter()
            .partition(|f| f.arrives_ms <= now);
        self.script.magic.flights = flying;
        for f in arrived {
            self.land_projectile(f.spec);
        }
        // Area effects.
        let mut areas = std::mem::take(&mut self.script.magic.areas);
        let mut alive = Vec::new();
        for mut fx in areas.drain(..) {
            let keep = {
                let mut host = SimHost::new(self);
                fx.tick(&mut host, dt)
            };
            if keep {
                alive.push(fx);
            }
        }
        alive.append(&mut self.script.magic.areas);
        self.script.magic.areas = alive;
        // Boosts and transformations.
        self.script
            .magic
            .boosts
            .retain(|b| b.until_ms > now || b.until_ms == 0);
        let ended: Vec<ActorId> = self
            .script
            .magic
            .transforms
            .iter()
            .filter(|t| t.until_ms <= now)
            .map(|t| t.who)
            .collect();
        for who in ended {
            let mut host = SimHost::new(self);
            gothic_script::magic::effects::transform_back(&mut host, entity_of(who));
        }
    }

    /// A projectile reaches its target: an arrow rolls the ranged hit chance, a spell missile
    /// always lands; both go through the damage pipeline.
    fn land_projectile(&mut self, spec: ProjectileSpec) {
        let (Some(att), Some(vic)) = (actor_of(spec.owner), actor_of(spec.target)) else {
            return;
        };
        if self.actor(vic).ai_mode == crate::types::AiMode::Dead {
            return;
        }
        if spec.spell.is_empty() {
            if !self.rng.chance_pct(self.cfg.ranged_hit_pct) {
                self.push(EventKind::Whiff {
                    attacker: att,
                    target: vic,
                    action: crate::types::Action::Shoot,
                });
                return;
            }
            let ammo = self.items.get(&spec.template).cloned().unwrap_or_default();
            let mut attack = Attack::projectile(&ammo, spec.hit_mult);
            attack.amount = spec.amount;
            attack.quality = spec.quality as i32;
            attack.damage_type = sim_damage_type(spec.damage_type);
            self.hit(att, vic, attack);
        } else {
            let hit = SpellHit {
                spell: spec.spell.clone(),
                damage_type: spec.damage_type,
                amount: spec.amount,
                mana_used: spec.mana_used,
                deity: spec.deity,
            };
            let mut attack = spell_attack(&spec.spell, spec.deity, &hit, spec.hit_mult);
            attack.path_missile = spec.path == PathStyle::Missile;
            self.hit(att, vic, attack);
        }
    }

    // ---- the hero's magic and ranged commands ---------------------------------------------------

    /// Select a spell item (rune, scroll, potion) and draw it.
    pub fn player_select_spell(&mut self, template: &str) -> bool {
        let ok = self
            .with_hero(|h, p| commands::select_spell(h, p, template))
            .unwrap_or(false);
        if ok && let Some(h) = self.hero {
            self.actor_mut(h).drawn = true;
        }
        ok
    }

    /// Cast the spell in hand at an actor (or free, `None`); `power` is a long press.
    pub fn player_cast(
        &mut self,
        target: Option<ActorId>,
        power: bool,
    ) -> Result<State, commands::CastError> {
        let t = target.map_or(EntityId::NONE, entity_of);
        self.with_hero(|h, p| commands::cast_spell(h, p, t, power))
            .unwrap_or(Err(commands::CastError::NoSpell))
    }

    /// Let go of a cast in progress.
    pub fn player_abort_cast(&mut self) {
        self.with_hero(|h, p| commands::abort_cast(h, p));
    }

    /// Put the bow or crossbow up.
    pub fn player_ready_ranged(&mut self) -> bool {
        let Some(h) = self.hero else { return false };
        let was = std::mem::replace(&mut self.actor_mut(h).drawn, true);
        let ok = self
            .with_hero(|h, p| commands::ready_ranged(h, p))
            .unwrap_or(false);
        if !ok {
            self.actor_mut(h).drawn = was;
        }
        ok
    }

    /// Aim (true) or lower the weapon (false).
    pub fn player_aim(&mut self, aim: bool) {
        self.with_hero(|h, p| commands::aim_ranged(h, p, aim));
    }

    /// Release the string or pull the trigger at the focus target.
    pub fn player_release(&mut self, target: Option<ActorId>) -> bool {
        let t = target.map_or(EntityId::NONE, entity_of);
        self.with_hero(|h, p| commands::release_ranged(h, p, t))
            .unwrap_or(false)
    }

    /// Offer a guard a bribe; `accepted` is the dialog's verdict on the price.
    pub fn player_bribe(
        &mut self,
        guard: ActorId,
        crime: se::PlayerCrime,
        accepted: bool,
    ) -> Option<gothic_script::routine::dialog::BribeOutcome> {
        let g = entity_of(guard);
        self.with_hero(|h, p| commands::bribe(h, p, g, crime, accepted))
    }
}
