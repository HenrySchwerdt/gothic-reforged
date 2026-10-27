//! Host interface of the magic port (B14 barriers, B17 casting, B20 spells, B21 spell states) and
//! the plain data of a spell.
//!
//! Spell numbers are not script knowledge: mana costs, the cast type, the spawned projectile and
//! its damage are properties of the spell and projectile templates (`PSMagic`, `PSDamage`). The
//! host loads them (the simulation from its template export) and answers [`MagicApi::spell`].

use crate::enums::{DamageType, SpellDeity};
use crate::host::{EntityId, Pos};

/// `gESpellType`: how a cast turns into an effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SpellType {
    /// A projectile flies to the target (`MagicProjectile`).
    #[default]
    Projectile,
    /// The spell acts on the target at once (`MagicInstant`).
    Target,
    /// An entity is spawned (`MagicSummon`).
    Summon,
}

impl SpellType {
    /// From the raw template value.
    pub fn from_raw(raw: u32) -> Self {
        match raw {
            1 => Self::Target,
            2 => Self::Summon,
            _ => Self::Projectile,
        }
    }
}

/// `gECastType`: which cast animation family a spell uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CastType {
    /// One quick motion without charging (`QuickCast`).
    #[default]
    Immediate,
    /// Can be charged for a power cast (needs the maximum mana).
    Power,
    /// The long regular summon motion.
    Regular,
}

impl CastType {
    /// From the raw template value.
    pub fn from_raw(raw: u32) -> Self {
        match raw {
            1 => Self::Power,
            2 => Self::Regular,
            _ => Self::Immediate,
        }
    }
}

/// Damage entity of the projectile a spell spawns (`PSDamage` of the `Mis_*`/`Smn_*` template).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpellDamage {
    /// `DamageType`.
    pub damage_type: DamageType,
    /// `DamageAmount`.
    pub amount: i32,
    /// `DamageManaMultiplier`.
    pub mana_mult: f32,
    /// `DamageHitMultiplier`.
    pub hit_mult: f32,
}

/// A spell template (`PSMagic`) together with the damage of what it spawns.
#[derive(Debug, Clone, PartialEq)]
pub struct SpellInfo {
    /// Entity name (`Spl_Fireball`).
    pub name: String,
    /// `SpellType`.
    pub spell_type: SpellType,
    /// `SpellDeity`.
    pub deity: SpellDeity,
    /// `CastType`.
    pub cast_type: CastType,
    /// `SpellTarget`: true when the spell needs the focus entity (raw value 1) [I].
    pub needs_focus: bool,
    /// `MinManaCost`.
    pub min_mana: i32,
    /// `MaxManaCost`.
    pub max_mana: i32,
    /// `FuncOnCast`: script function replacing the type dispatch, empty for none.
    pub func_on_cast: String,
    /// `FuncOnTargetHit`: script function applying the effect, empty for none.
    pub func_on_hit: String,
    /// Template named by `Spawn` (projectile, summoned effect or creature).
    pub spawn: String,
    /// Damage entity of the spawned template, if it has one.
    pub damage: Option<SpellDamage>,
    /// `AnimActionCast`.
    pub anim_cast: String,
    /// `AnimActionPowerCast`.
    pub anim_power: String,
    /// `ShootVelocity` of the spawned missile (metres per second in the template).
    pub velocity: f32,
    /// `AutoReload`: the spell is held again after a cast.
    pub auto_reload: bool,
    /// `ReloadDelaySeconds`.
    pub reload_secs: f32,
}

impl SpellInfo {
    /// A spell with the given identity and no spawn (builder for hosts and tests).
    pub fn new(name: &str, spell_type: SpellType, cast_type: CastType) -> Self {
        Self {
            name: name.into(),
            spell_type,
            deity: SpellDeity::None,
            cast_type,
            needs_focus: false,
            min_mana: 0,
            max_mana: 0,
            func_on_cast: String::new(),
            func_on_hit: String::new(),
            spawn: String::new(),
            damage: None,
            anim_cast: String::new(),
            anim_power: String::new(),
            velocity: 100.0,
            auto_reload: true,
            reload_secs: 0.2,
        }
    }
}

/// How a spawned projectile finds its way (`gEProjectilePath`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathStyle {
    /// Straight line along the aim direction (value 0).
    Line,
    /// Ballistic flight (value 2, the player's arrows).
    Physics,
    /// Homing on the target (value 3).
    Missile,
}

/// Spell projectile or arrow to launch.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileSpec {
    /// Shooter (`PSInteraction.Owner`).
    pub owner: EntityId,
    /// Entity aimed at (`NONE` for free flight along the aim).
    pub target: EntityId,
    /// Spell name, empty for ammunition.
    pub spell: String,
    /// Template spawned (`Mis_Fireball`, `It_Arrow`).
    pub template: String,
    /// `PSDamage.ManaUsed`.
    pub mana_used: i32,
    /// `PSDamage.DamageHitMultiplier`.
    pub hit_mult: f32,
    /// Damage type of the missile.
    pub damage_type: DamageType,
    /// Damage amount (arrows: ammunition plus bow).
    pub amount: i32,
    /// Path style.
    pub path: PathStyle,
    /// Launch speed, `PSProjectile.ShootVelocity` (arrows: draw multiplier times 200).
    pub velocity: f32,
    /// `Spl_*` deity, for the damage rules.
    pub deity: SpellDeity,
    /// `PSItem.Quality` bits of the ammunition (fire, poison, ...); 0 for spells.
    pub quality: u32,
}

/// What the game layer is told about magic (effects, summons, status changes, messages).
#[derive(Debug, Clone, PartialEq)]
pub enum MagicEvent {
    /// A cast phase began or ended for the caster.
    Phase {
        /// The caster.
        caster: EntityId,
        /// The spell.
        spell: String,
        /// 0 reset, 1 hold, 2 load, 3 abort, 4 power, 5 cast.
        phase: u8,
    },
    /// The spell was cast (after the cast phase ran its effect).
    Cast {
        /// The caster.
        caster: EntityId,
        /// The spell.
        spell: String,
        /// Target entity, `NONE` if free.
        target: EntityId,
        /// Mana the cast cost.
        mana_used: i32,
    },
    /// A visual effect should play on an entity (`EffectSystem::StartEffect`).
    Effect {
        /// Effect name.
        name: String,
        /// Entity it plays on.
        on: EntityId,
    },
    /// A creature was summoned.
    Summoned {
        /// The summoner.
        owner: EntityId,
        /// Template name.
        template: String,
        /// The new creature.
        npc: EntityId,
    },
    /// A status bit changed (burning 1, frozen 2, poisoned 4, diseased 8, controlled 0x40...).
    Status {
        /// The affected character.
        target: EntityId,
        /// Status bit mask.
        mask: u32,
        /// Set or cleared.
        on: bool,
    },
    /// The hero changed shape or returned to his own.
    Transform {
        /// Who.
        who: EntityId,
        /// Template of the new shape; empty when the transformation ended.
        template: String,
        /// Duration in seconds, 0 when it ended.
        secs: f32,
    },
    /// The hero was teleported.
    Teleport {
        /// Who.
        who: EntityId,
        /// Destination name (`Teleport_KapDun`).
        to: String,
    },
    /// A HUD message (`TooHard`, `Impossible`, `OnlyPlayer`).
    Message {
        /// Message key.
        key: &'static str,
        /// The character the message is about.
        subject: EntityId,
    },
    /// An area spell entity began.
    Area {
        /// Which spell state (`ZS_Firerain`, `ZS_Heatwave`...).
        state: &'static str,
        /// Owner of the spell.
        owner: EntityId,
        /// Centre of the effect.
        at: Pos,
    },
    /// A chest, door or mover was opened by a spell.
    Unlocked {
        /// The object.
        object: EntityId,
    },
    /// The player's time was sped up or bubbled.
    Time {
        /// Time scale the world should run at.
        scale: f32,
        /// For how long in seconds.
        secs: f32,
    },
    /// A flame sword (or other weapon spell) is up or down.
    Weapon {
        /// Owner.
        owner: EntityId,
        /// Active.
        on: bool,
    },
}

/// One hit of a spell that does not travel as a projectile (area ticks, waves).
#[derive(Debug, Clone, PartialEq)]
pub struct SpellHit {
    /// The spell (`Spl_Firerain`).
    pub spell: String,
    /// Damage type.
    pub damage_type: DamageType,
    /// Damage amount.
    pub amount: i32,
    /// Mana the cast paid (scales spell damage).
    pub mana_used: i32,
    /// Deity of the spell.
    pub deity: SpellDeity,
}

/// A timed boost of the hero that the host keeps ticking (the engine parks them in unrelated
/// properties, see `MagicSpeedUp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Boost {
    /// Faster movement for the given seconds.
    SpeedUp,
    /// Stamina held at the given level.
    KeepStamina,
}

/// Everything the magic code needs besides the other host traits.
pub trait MagicApi {
    /// The spell template of a name.
    fn spell(&self, name: &str) -> Option<SpellInfo>;
    /// Spell of the item on a stack (`PSItem.Spell`), empty if it has none.
    fn stack_spell(&self, e: EntityId, stack: i32) -> String;
    /// Spell of an item template (`PSItem.Spell`), empty if it has none.
    fn item_spell(&self, template: &str) -> String;
    /// Amount of items on a stack.
    fn stack_amount(&self, e: EntityId, stack: i32) -> i32;
    /// Is the item of the stack permanent (runes), i.e. never used up?
    fn stack_permanent(&self, e: EntityId, stack: i32) -> bool;
    /// `PSItem.Category` of the stack (4 = Alchemy).
    fn stack_category(&self, e: EntityId, stack: i32) -> u32;
    /// Remove items from a stack (a scroll is used up).
    fn delete_stack_items(&mut self, e: EntityId, stack: i32, n: i32);

    /// `PSInteraction.Spell` of the caster: the spell being cast, empty if none.
    fn cast_spell(&self, caster: EntityId) -> String;
    /// Set the spell being cast.
    fn set_cast_spell(&mut self, caster: EntityId, spell: &str);
    /// `PSNpc.SpellTarget`.
    fn spell_target(&self, caster: EntityId) -> EntityId;
    /// Set the spell target.
    fn set_spell_target(&mut self, caster: EntityId, target: EntityId);
    /// `PSNpc.ManaUsed`: mana paid for the cast in progress.
    fn mana_used(&self, caster: EntityId) -> i32;
    /// Set the mana paid.
    fn set_mana_used(&mut self, caster: EntityId, n: i32);
    /// `PSNpc.LastSpell`.
    fn set_last_spell(&mut self, caster: EntityId, spell: &str);

    /// Apply the damage of a spell hit to a victim (the damage pipeline of B07).
    fn spell_damage(&mut self, owner: EntityId, victim: EntityId, hit: SpellHit);
    /// Damage entity of the item in a slot (the arrow, the bow): type and amount.
    fn slot_item_damage(&self, e: EntityId, slot: crate::enums::Slot) -> Option<(DamageType, i32)>;
    /// `PSDamage.DamageHitMultiplier` of a character (the draw strength of its next shot).
    fn hit_multiplier(&self, e: EntityId) -> f32;
    /// Set the hit multiplier.
    fn set_hit_multiplier(&mut self, e: EntityId, m: f32);
    /// Launch a spell projectile or an arrow.
    fn spawn_projectile(&mut self, p: ProjectileSpec);
    /// Spawn a creature from an NPC template next to `near`; `NONE` if there is no room.
    fn spawn_npc(&mut self, template: &str, near: EntityId) -> EntityId;
    /// Spawn an area-spell entity (a `Smn_*` template) running its state at `at`.
    fn spawn_area(&mut self, template: &str, owner: EntityId, spell: &str, at: Pos);
    /// Remove an entity (`Entity::Decay`/`Kill` of a summon).
    fn remove_entity(&mut self, e: EntityId);
    /// Characters within `radius` cm of `centre`, nearest first (`GetEntitiesByDistance`).
    fn characters_near(&self, centre: Pos, radius: f32) -> Vec<EntityId>;
    /// Template of a character (`Wolf`), for transformations and summon checks.
    fn template_name(&self, e: EntityId) -> String;
    /// Difficulty of the lock of an interact object, `None` if it is not locked.
    fn lock_difficulty(&self, object: EntityId) -> Option<i32>;
    /// All characters, nearest to `from` first.
    fn all_npcs_by_distance(&self, from: EntityId) -> Vec<EntityId>;
    /// Can `npc` walk to `target` (`Entity::CanGoto`)?
    fn can_goto(&self, npc: EntityId, target: EntityId) -> bool;
    /// Start a timed boost on the hero (`secs` for a speed-up, `level` for kept stamina).
    fn set_boost(&mut self, who: EntityId, boost: Boost, secs: f32, level: i32);
    /// Remove the area-spell entities of one damage element within `radius` of `centre`.
    fn dispel_areas(&mut self, centre: Pos, radius: f32, element: DamageType);
    /// Move a character to a named waypoint.
    fn teleport(&mut self, who: EntityId, destination: &str);
    /// `Entity::StartTransform`: `who` takes the body of `form` for `secs` seconds.
    fn begin_transform(&mut self, who: EntityId, form: EntityId, secs: f32);
    /// The body `who` currently wears (`NONE` when not transformed).
    fn transform_form(&self, who: EntityId) -> EntityId;
    /// `Entity::EndTransform`: give the original body back and remove the animal's.
    fn end_transform(&mut self, who: EntityId);
    /// Unlock a locked object (`PSLock.Status = Unlocked`).
    fn unlock_object(&mut self, object: EntityId);
    /// Lift or toggle the focused object toward the caster.
    fn telekinesis(&mut self, caster: EntityId, target: EntityId);
    /// Tell the game layer something.
    fn emit_magic(&mut self, event: MagicEvent);
}
