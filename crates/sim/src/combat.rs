//! The damage pipeline: `OnHit -> DoLogicalDamage -> AssessHit` [CA 1, CA 2], parry and
//! stagger classes, kill / defeat decisions and kill XP [CA 3].
//!
//! Entry point: [`resolve_hit`]. It mutates the victim's HP / stamina / mana (the engine
//! applies `DamageAmount` to HP after `AssessHit` [CA 1, I]) and returns a [`HitOutcome`];
//! turning a lethal outcome into `Kill` / `Defeat` bookkeeping is the caller's job
//! ([`crate::sim`]).
use crate::actor::Actor;
use crate::config::SimConfig;
use crate::item::{ItemDef, Use};
use crate::types::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackKind {
    Melee,
    Projectile,
    Spell,
}

/// Spell deity (`Innos` 2, `Adanos` 0, `Beliar` 1) [CA 2.5].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deity {
    Adanos,
    Beliar,
    Innos,
}

/// Situational inputs that the world / AI layer supplies [CA 1.1, 2.2, 2.9, 2.11].
#[derive(Debug, Clone)]
pub struct HitContext {
    /// Attacker-to-victim distance (for the `0x40` far-hit rule).
    pub distance: f32,
    /// The attacker is inside the victim's field of view (parry conditions).
    pub attacker_in_victim_fov: bool,
    /// The victim has the attacker in its field of view (backstab rule).
    pub victim_sees_attacker: bool,
    /// The victim is the attacker's current target.
    pub victim_is_attacker_target: bool,
    /// `GetAttitude(victim -> attacker)`.
    pub victim_to_attacker: Attitude,
    /// `GetAttitude(attacker -> victim)`.
    pub attacker_to_victim: Attitude,
    /// A GUI page is open while the player is the victim.
    pub gui_open: bool,
    /// The victim's attack reason is Arena (duel).
    pub victim_in_arena: bool,
    /// `AttitudeToPlayer2 == Friendly` for the friendly-fire gate of player projectiles.
    pub victim_friendly_to_player: bool,
}

impl Default for HitContext {
    /// An ordinary face-to-face fight between hostile parties.
    fn default() -> Self {
        Self {
            distance: 100.0,
            attacker_in_victim_fov: true,
            victim_sees_attacker: true,
            victim_is_attacker_target: true,
            victim_to_attacker: Attitude::Hostile,
            attacker_to_victim: Attitude::Hostile,
            gui_open: false,
            victim_in_arena: false,
            victim_friendly_to_player: false,
        }
    }
}

/// The `PSDamage` data of the damage entity plus the situation.
#[derive(Debug, Clone)]
pub struct Attack {
    pub kind: AttackKind,
    pub damage_type: DamageType,
    /// `DamageAmount`.
    pub amount: i32,
    /// `DamageHitMultiplier` (bow draw, ...) [CA 9.6].
    pub hit_mult: f32,
    /// `ManaUsed` for spells [CA 2.5].
    pub mana_used: i32,
    /// The damage entity touched a `_Head` bone [CA 2.5].
    pub head_hit: bool,
    /// Weapon name is "Fist" (unarmed) [CA 2.5].
    pub unarmed: bool,
    /// Name of the damage entity (arrow, spell, weapon) for the `Fire` / `Explosive`
    /// and `Spl_DestroyEvil` rules [CA 2.4, 1.1].
    pub entity_name: String,
    pub spell_projectile: bool,
    /// Projectile `PathStyle == Missile` [CA 2.5].
    pub path_missile: bool,
    pub deity: Option<Deity>,
    /// `PSItem.Quality` flags of the weapon [CA 2.5].
    pub quality: i32,
    pub ctx: HitContext,
}

impl Attack {
    fn base(kind: AttackKind, ty: DamageType, amount: i32) -> Self {
        Self {
            kind,
            damage_type: ty,
            amount,
            hit_mult: 1.0,
            mana_used: 0,
            head_hit: false,
            unarmed: false,
            entity_name: String::new(),
            spell_projectile: false,
            path_missile: false,
            deity: None,
            quality: 0,
            ctx: HitContext::default(),
        }
    }

    /// A melee swing with whatever the attacker holds in the weapon hand; unarmed NPCs use
    /// their natural-attack block or `Fist` (10 impact) [CA 2.5].
    pub fn melee(attacker: &Actor) -> Self {
        if let Some(w) = attacker.equipment.weapon_damage() {
            let mut a = Self::base(AttackKind::Melee, w.kind(), w.amount);
            a.hit_mult = w.hit_multiplier;
            a.quality = attacker.equipment.weapon_quality();
            a.entity_name = attacker
                .equipment
                .weapon_name()
                .unwrap_or_default()
                .to_string();
            a.unarmed = matches!(
                attacker.equipment.right_use(),
                Use::Fist | Use::PhysicalFist
            );
            return a;
        }
        let d = attacker.natural_damage.clone().unwrap_or_default();
        let (ty, amount) = if d.amount > 0 {
            (d.kind(), d.amount)
        } else {
            (DamageType::Impact, 10)
        };
        let mut a = Self::base(AttackKind::Melee, ty, amount);
        a.hit_mult = if d.amount > 0 { d.hit_multiplier } else { 1.0 };
        a.entity_name = "Fist".into();
        a.unarmed = true;
        a
    }

    /// Arrow or bolt: damage of the ammunition, `mult` from the draw time [CA 9.6].
    pub fn projectile(ammo: &ItemDef, mult: f32) -> Self {
        let (ty, amount) = ammo
            .damage
            .as_ref()
            .map_or((DamageType::Missile, 0), |d| (d.kind(), d.amount));
        let mut a = Self::base(AttackKind::Projectile, ty, amount);
        a.hit_mult = mult;
        a.quality = ammo.quality;
        a.entity_name = ammo.name.clone();
        a
    }

    pub fn spell(name: &str, ty: DamageType, amount: i32, mana_used: i32, deity: Deity) -> Self {
        let mut a = Self::base(AttackKind::Spell, ty, amount);
        a.mana_used = mana_used;
        a.entity_name = name.into();
        a.spell_projectile = true;
        a.deity = Some(deity);
        a
    }

    pub fn with_ctx(mut self, ctx: HitContext) -> Self {
        self.ctx = ctx;
        self
    }
}

/// `DamageHitMultiplier` of a bow after `t` seconds of drawing: `0.2 + 0.8*min(1, t/1.95)`
/// [CA 9.6].
pub fn bow_hit_multiplier(held_secs: f32) -> f32 {
    0.2 + 0.8 * (held_secs / 1.95).min(1.0)
}

/// Why `DoLogicalDamage` dropped the hit [CA 1.1].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IgnoreReason {
    Immune,
    Invulnerable,
    VictimMode,
    GuiOpen,
    Swimming,
    KnockedDown,
    OnlyHurtsTarget,
    AccidentalAlly,
    FriendlySpell,
    NotEvil,
    TooFarWhileMisted,
    GhostForm,
}

/// `gEAction` results of `AssessHit` [CA 2.9, 2.10].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitResult {
    Ignored,
    /// 0: no reaction (also frozen / sprint-attack-uninterrupted).
    NoReaction,
    QuickParadeStumble,
    ParadeStumble,
    HeavyParadeStumble,
    QuickStumble,
    Stumble,
    SitKnockDown,
    LieKnockDown,
    LieKnockOut,
    PierceStumble,
    LieDead,
    LiePiercedKO,
    LiePiercedDead,
    MagicParade,
}

impl HitResult {
    /// The `gEAction` number [CA 1.1].
    pub fn code(self) -> i32 {
        match self {
            Self::Ignored | Self::NoReaction => 0,
            Self::QuickParadeStumble => 20,
            Self::ParadeStumble => 21,
            Self::HeavyParadeStumble => 24,
            Self::QuickStumble => 25,
            Self::Stumble => 26,
            Self::SitKnockDown => 29,
            Self::LieKnockDown => 32,
            Self::LieKnockOut => 33,
            Self::PierceStumble => 34,
            Self::LieDead => 36,
            Self::LiePiercedKO => 37,
            Self::LiePiercedDead => 38,
            Self::MagicParade => 46,
        }
    }
    /// Lethal results (broadcast as murder) [CA 1.1].
    pub fn is_kill(self) -> bool {
        matches!(self, Self::LieDead | Self::LiePiercedDead)
    }
    /// Non-lethal defeat (broadcast as defeat) [CA 1.1].
    pub fn is_defeat(self) -> bool {
        matches!(
            self,
            Self::LieKnockDown | Self::LieKnockOut | Self::LiePiercedKO
        )
    }
    pub fn is_parry(self) -> bool {
        matches!(
            self,
            Self::QuickParadeStumble
                | Self::ParadeStumble
                | Self::HeavyParadeStumble
                | Self::MagicParade
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerceptionKind {
    Defeat,
    Murder,
    FightSound,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HitOutcome {
    pub ignored: Option<IgnoreReason>,
    pub result: HitResult,
    /// Final damage figure after protection and move multipliers.
    pub damage: i32,
    /// HP actually removed from the victim.
    pub hp_lost: i32,
    pub stamina_spent: i32,
    pub mana_spent: i32,
    /// Hit strength class `v` [CA 2.3].
    pub strength_class: i32,
    pub parried: bool,
    pub head_shot: bool,
    pub weakness_doubled: bool,
    pub status_applied: u32,
    /// Frozen duration units `12 * max(5, dmg/20)` [CA 2.10].
    pub freeze: i32,
    pub perception: Option<PerceptionKind>,
    /// Return value of `DoLogicalDamage`: 0 for parry results [CA 1.1].
    pub returns_to_caller: bool,
}

impl HitOutcome {
    fn new() -> Self {
        Self {
            ignored: None,
            result: HitResult::NoReaction,
            damage: 0,
            hp_lost: 0,
            stamina_spent: 0,
            mana_spent: 0,
            strength_class: 0,
            parried: false,
            head_shot: false,
            weakness_doubled: false,
            status_applied: 0,
            freeze: 0,
            perception: None,
            returns_to_caller: false,
        }
    }
    /// A hit `DoLogicalDamage` dropped.
    pub(crate) fn ignored_hit() -> Self {
        Self::ignored(IgnoreReason::VictimMode)
    }
    /// The outcome of an assessed hit (`AssessHit` result and damage).
    pub(crate) fn ported(result: HitResult, damage: i32) -> Self {
        Self {
            result,
            damage,
            ..Self::new()
        }
    }
    fn ignored(r: IgnoreReason) -> Self {
        Self {
            ignored: Some(r),
            result: HitResult::Ignored,
            ..Self::new()
        }
    }
    pub fn killed(&self) -> bool {
        self.result.is_kill()
    }
    pub fn defeated(&self) -> bool {
        self.result.is_defeat()
    }
}

// ---------------------------------------------------------------------------------
// Kill XP [CA 3]

/// XP awarded for killing / defeating `victim` with `GiveXP(amount = -1)` [CA 3].
/// `xp = 5 * floor(XPModifier * base / 500 + 0.5)`.
pub fn kill_xp(cfg: &SimConfig, victim: &Actor) -> u64 {
    let l = victim.level(cfg) as i64;
    let base = if cfg.extended_content {
        if victim.species.is_ambient() {
            10
        } else if matches!(victim.species, Species::Pig | Species::Cow) {
            20
        } else {
            (10 * l).max(50)
        }
    } else if victim.species.is_ambient() {
        (10 * l).max(25)
    } else {
        (10 * l).max(50)
    };
    let m = i64::from(cfg.xp_percent());
    (5 * ((m * base + 250) / 500)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SimConfig {
        SimConfig::default()
    }

    #[test]
    fn kill_xp_examples() {
        // [CA 3] ExtendedContent: max(10*L, 50) then 5*floor(mod*base/500 + .5).
        let c = cfg();
        let mk = |sp, l| Actor::npc(ActorId(1), "X", sp, l, l);
        assert_eq!(kill_xp(&c, &mk(Species::Orc, 10)), 100);
        assert_eq!(kill_xp(&c, &mk(Species::Orc, 2)), 50);
        assert_eq!(kill_xp(&c, &mk(Species::Seagull, 30)), 10);
        assert_eq!(kill_xp(&c, &mk(Species::Pig, 3)), 20);
        // Vanilla ambient minimum 25: base max(10*L,25) -> L=1 -> 25.
        let v = SimConfig::vanilla();
        assert_eq!(kill_xp(&v, &mk(Species::Rabbit, 1)), 25);
        // XPModifier 200: base 100 -> 5*floor(200*100/500 + 0.5) = 5*40 = 200.
        let c2 = SimConfig {
            xp_modifier: 200,
            ..cfg()
        };
        assert_eq!(kill_xp(&c2, &mk(Species::Orc, 10)), 200);
        // XPModifier above 200 resets to 100.
        let c3 = SimConfig {
            xp_modifier: 500,
            ..cfg()
        };
        assert_eq!(kill_xp(&c3, &mk(Species::Orc, 10)), 100);
    }
}
