//! The damage pipeline: `OnHit -> DoLogicalDamage -> AssessHit` [CA 1, CA 2], parry and
//! stagger classes, kill / defeat decisions and kill XP [CA 3].
//!
//! Entry point: [`resolve_hit`]. It mutates the victim's HP / stamina / mana (the engine
//! applies `DamageAmount` to HP after `AssessHit` [CA 1, I]) and returns a [`HitOutcome`];
//! turning a lethal outcome into `Kill` / `Defeat` bookkeeping is the caller's job
//! ([`crate::sim`]).
use crate::actor::Actor;
use crate::config::{Difficulty, SimConfig};
use crate::item::{ItemDef, Use, WeaponMode};
use crate::rng::Rng;
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

/// Resolve one hit. `attacker` is only read; the victim's resources are updated.
pub fn resolve_hit(
    cfg: &SimConfig,
    rng: &mut Rng,
    attacker: &Actor,
    victim: &mut Actor,
    attack: &Attack,
) -> HitOutcome {
    if let Some(r) = gate(cfg, attacker, victim, attack) {
        return HitOutcome::ignored(r);
    }
    let mut out = assess_hit(cfg, rng, attacker, victim, attack);
    out.perception = Some(match out.result {
        r if r.is_defeat() => PerceptionKind::Defeat,
        r if r.is_kill() => PerceptionKind::Murder,
        _ => PerceptionKind::FightSound,
    });
    out.returns_to_caller = !matches!(out.result.code(), 20 | 21 | 24 | 46);
    victim.last_attacker = victim.current_attacker;
    victim.current_attacker = Some(attacker.id);
    out
}

// ---------------------------------------------------------------------------------
// DoLogicalDamage gate [CA 1.1]

fn gate(cfg: &SimConfig, att: &Actor, vic: &Actor, atk: &Attack) -> Option<IgnoreReason> {
    let _ = cfg;
    if damage_weakness(vic, atk) == 0 {
        return Some(IgnoreReason::Immune);
    }
    if (vic.status | att.status) & status::INVULNERABLE != 0 {
        return Some(IgnoreReason::Invulnerable);
    }
    if matches!(
        vic.ai_mode,
        AiMode::Arena | AiMode::Dead | AiMode::Down | AiMode::Talk
    ) {
        return Some(IgnoreReason::VictimMode);
    }
    if vic.is_player && atk.ctx.gui_open {
        return Some(IgnoreReason::GuiOpen);
    }
    if matches!(vic.ani_state, AniState::Swim | AniState::Dive) {
        return Some(IgnoreReason::Swimming);
    }
    if matches!(
        vic.ani_state,
        AniState::SitKnockDown | AniState::LieKnockDown
    ) {
        return Some(IgnoreReason::KnockedDown);
    }
    if matches!(att.action, Action::PierceAttack | Action::HackAttack)
        && !atk.ctx.victim_is_attacker_target
    {
        return Some(IgnoreReason::OnlyHurtsTarget);
    }
    if !att.is_player
        && !atk.ctx.victim_is_attacker_target
        && matches!(
            atk.ctx.attacker_to_victim,
            Attitude::Friendly | Attitude::Neutral
        )
    {
        return Some(IgnoreReason::AccidentalAlly);
    }
    if att.is_player
        && atk.kind != AttackKind::Melee
        && atk.ctx.victim_friendly_to_player
        && (vic.ai_mode == AiMode::Combat || atk.ctx.victim_to_attacker == Attitude::Friendly)
    {
        return Some(IgnoreReason::FriendlySpell);
    }
    if atk.entity_name.eq_ignore_ascii_case("Spl_DestroyEvil") && !vic.species.is_evil() {
        return Some(IgnoreReason::NotEvil);
    }
    if vic.status & status::MISTED != 0 && atk.ctx.distance > 700.0 {
        return Some(IgnoreReason::TooFarWhileMisted);
    }
    if vic.status & status::SOUL_TRAVEL != 0 {
        return Some(IgnoreReason::GhostForm);
    }
    None
}

/// Damage-type weakness helper (S:7250): 0 immune, 1 normal, 2 weak [CA 2.4].
pub fn damage_weakness(vic: &Actor, atk: &Attack) -> u8 {
    use Species::*;
    let s = vic.species;
    match atk.damage_type {
        DamageType::None => 0,
        DamageType::Impact => {
            if matches!(s, Skeleton | Golem) {
                2
            } else {
                1
            }
        }
        DamageType::Blade | DamageType::Physics => 1,
        DamageType::Missile => {
            let fiery = atk.entity_name.contains("Fire") || atk.entity_name.contains("Explosive");
            match s {
                Golem | FireGolem => 0,
                Skeleton | IceGolem if !fiery => 0,
                _ => 1,
            }
        }
        DamageType::Fire => match s {
            FireVaran | FireGolem | Dragon => 0,
            IceGolem => 2,
            _ => 1,
        },
        DamageType::Ice => {
            if vic.status & status::FROZEN != 0 || s == IceGolem {
                0
            } else if s == FireGolem {
                2
            } else {
                1
            }
        }
        DamageType::Lightning => {
            if s == Golem {
                2
            } else {
                1
            }
        }
    }
}

// ---------------------------------------------------------------------------------
// Strength class helpers [CA 2.3]

/// Perk-based weapon family of the player's hands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    DualOneHand,
    OneHand,
    AxeClass,
    Staff,
    Other,
}

fn family(a: &Actor) -> Family {
    if a.equipment.dual_one_hand() {
        return Family::DualOneHand;
    }
    match a.equipment.right_use() {
        Use::OneH => Family::OneHand,
        Use::TwoH | Use::Axe | Use::Halberd => Family::AxeClass,
        Use::Staff => Family::Staff,
        _ => Family::Other,
    }
}

/// `M(act)` (S:2b3a0) [CA 2.3].
pub fn move_strength(att: &Actor, act: Action) -> i32 {
    let dual = att.equipment.dual_one_hand();
    match act {
        Action::Attack
        | Action::SimpleWhirl
        | Action::WhirlAttack
        | Action::PierceAttack
        | Action::GetUpAttack => 2,
        Action::PowerAttack | Action::SprintAttack => {
            if dual {
                if att.state_position >= 2 { 3 } else { 2 }
            } else {
                4
            }
        }
        Action::HackAttack => 4,
        _ => 0,
    }
}

/// `R(e)` power rank 0..3 (S:2afc0) [CA 2.3]. NPC rank from level (I: current level).
pub fn power_rank(cfg: &SimConfig, e: &Actor) -> i32 {
    if e.uses_npc_formulas() {
        let l = e.current_level(cfg);
        return if l >= 45 {
            3
        } else if l >= 30 {
            2
        } else if l >= 15 {
            1
        } else {
            0
        };
    }
    let (p3, p2) = match family(e) {
        Family::DualOneHand => ("perk_1h1h_secretbonus", "perk_1h1h_2"),
        Family::OneHand => ("perk_1h_3", "perk_1h_2"),
        Family::AxeClass => ("perk_axe_3", "perk_axe_2"),
        Family::Staff => ("perk_staff_3", "perk_staff_2"),
        Family::Other => return 0,
    };
    if e.has_perk(p3) {
        2
    } else if e.has_perk(p2) {
        1
    } else {
        0
    }
}

/// `D(victim)` (S:2b470) [CA 2.3].
pub fn defence_strength(cfg: &SimConfig, vic: &Actor) -> i32 {
    if vic.action == Action::GetUpParade {
        return 2;
    }
    let shield = vic.equipment.has_shield();
    let bonus = if shield && (!vic.is_player || vic.has_perk("perk_shield_2")) {
        1
    } else {
        0
    };
    power_rank(cfg, vic) + bonus
}

// ---------------------------------------------------------------------------------
// Bonuses [CA 2.5]

/// `QualityBonus` (S:6af10) [CA 2.5].
pub fn quality_bonus(b: i32, quality: i32) -> i32 {
    let mut r = 0;
    if quality & 0x100 != 0 {
        r -= b / 2;
    }
    for flag in [0x20, 0x40, 0x80] {
        if quality & flag != 0 {
            r += 10;
        }
    }
    r
}

/// `SkillBonus` (S:2e410), player only [CA 2.5]. "+25" tiers: the `_3` perk sets 25, the
/// `_2` perk adds 25 (I: the two never stack).
pub fn skill_bonus(att: &Actor, vic: &Actor, atk: &Attack, b: i32) -> i32 {
    let tier = |p3: &str, p2: &str| -> i32 {
        if att.has_perk(p3) || att.has_perk(p2) {
            25
        } else {
            0
        }
    };
    let mut bonus = 0;
    match atk.kind {
        AttackKind::Spell => {
            let school = match atk.deity {
                Some(Deity::Innos) => "perk_firemage",
                Some(Deity::Adanos) => "perk_watermage",
                Some(Deity::Beliar) => "perk_darkmage",
                None => "",
            };
            if !school.is_empty() && att.has_perk(school) {
                bonus += b;
            }
        }
        AttackKind::Projectile => {
            match att.equipment.left_use() {
                Use::Bow => {
                    bonus += tier("perk_bow_3", "perk_bow_2");
                    if att.has_perk("perk_hunter_1") && vic.species.is_game_animal() {
                        bonus += b;
                    }
                    if att.has_perk("perk_hunter_2") && vic.species.is_other_animal() {
                        bonus += b;
                    }
                    if att.has_perk("perk_orchunter") && vic.species == Species::Orc {
                        bonus += 25;
                    }
                }
                Use::CrossBow => {
                    bonus += tier("perk_crossbow_3", "perk_crossbow_2");
                    if att.has_perk("perk_orchunter") && vic.species == Species::Orc {
                        bonus += 25;
                    }
                }
                _ => {}
            }
            bonus += physical_slayer(att, vic);
        }
        AttackKind::Melee => {
            bonus += match family(att) {
                Family::DualOneHand => tier("perk_1h1h_secretbonus", "perk_1h1h_2"),
                Family::OneHand => tier("perk_1h_3", "perk_1h_2"),
                Family::AxeClass => tier("perk_axe_3", "perk_axe_2"),
                Family::Staff => tier("perk_staff_3", "perk_staff_2"),
                Family::Other => 0,
            };
            bonus += physical_slayer(att, vic);
        }
    }
    bonus
}

fn physical_slayer(att: &Actor, vic: &Actor) -> i32 {
    let mut b = 0;
    if att.has_perk("perk_orcslayer") && vic.species == Species::Orc {
        b += 25;
    }
    if att.has_perk("perk_paladin") && vic.species.is_evil() {
        b += 25;
    }
    b
}

// ---------------------------------------------------------------------------------
// AssessHit [CA 2]

fn assess_hit(
    cfg: &SimConfig,
    rng: &mut Rng,
    att: &Actor,
    vic: &mut Actor,
    atk: &Attack,
) -> HitOutcome {
    let mut out = HitOutcome::new();
    let act = att.action.normalised(); // [CA 2.1]
    if att.is_player
        && !att.transformed
        && let Some(r) = player_special(cfg, att, vic, atk, act)
    {
        return r;
    }

    // [CA 2.3] strength class v.
    let v = match atk.kind {
        AttackKind::Spell => 2,
        AttackKind::Projectile => {
            if atk.hit_mult >= 0.6 {
                2
            } else {
                0
            }
        }
        AttackKind::Melee => (move_strength(att, att.action) + power_rank(cfg, att)
            - defence_strength(cfg, vic))
        .max(1),
    };
    out.strength_class = v;

    // [CA 2.5] raw damage, weakness, head shot.
    let mut b = (atk.amount as f32 * atk.hit_mult) as i32;
    if damage_weakness(vic, atk) == 2 {
        b *= 2;
        out.weakness_doubled = true;
    }
    if atk.head_hit {
        b *= 2;
        out.head_shot = true;
    }
    let b0 = b;

    let mut dmg;
    if att.is_player && !att.transformed {
        let e = match atk.kind {
            AttackKind::Spell => {
                if atk.spell_projectile {
                    b = 2 * b.min(100);
                }
                if atk.mana_used == 0 {
                    2 * b
                } else {
                    b * att.intelligence(cfg) / 100
                }
            }
            AttackKind::Projectile => att.dexterity(cfg) / 2,
            AttackKind::Melee => att.strength(cfg) / 2,
        };
        b += quality_bonus(b, atk.quality);
        b += skill_bonus(att, vic, atk, b);
        dmg = b + e.max(0);
    } else {
        let strength = att.strength(cfg);
        let l = att.level(cfg);
        let humanoid = att.species.is_humanoid();
        b = if l < 2 || !cfg.extended_content || !humanoid || atk.kind != AttackKind::Melee {
            strength
        } else if !vic.is_player {
            if atk.unarmed { strength / 2 } else { strength }
        } else if atk.unarmed {
            (0.75 * strength as f32) as i32
        } else if att.species == Species::Orc {
            (0.75 * strength as f32 + b0 as f32 / 7.0) as i32
        } else {
            (0.9 * strength as f32 + b0 as f32 / 9.0) as i32
        };
        if cfg.variants.npc_weapon_damage_counts && !atk.unarmed && atk.kind == AttackKind::Melee {
            b += b0;
        }
        // Monster-vs-NPC nerf [CA 2.5].
        if vic.species.is_humanoid() && !humanoid && !vic.is_player {
            b = if att.summoned { b / 2 } else { b / 10 };
        }
        if atk.spell_projectile {
            b *= 2;
        }
        if atk.path_missile {
            b *= 2;
        }
        dmg = b;
    }

    // [CA 2.7] protection.
    let prot = vic.protection(cfg, atk.damage_type);
    let flat =
        vic.is_player && cfg.extended_content && !vic.transformed && atk.kind == AttackKind::Melee;
    if flat {
        let d = dmg - prot;
        dmg = if d < 1 {
            if dmg > 39 || matches!(act, Action::HackAttack | Action::PowerAttack) {
                ((rng.below(2) + 1) * dmg) / 100
            } else {
                0
            }
        } else {
            d
        };
    } else {
        dmg -= prot * dmg / 100;
    }

    // [CA 2.8] attack-move multipliers.
    match act {
        Action::QuickAttack | Action::QuickAttackR | Action::QuickAttackL => dmg /= 2,
        Action::HackAttack => dmg *= 2,
        Action::PowerAttack if att.equipment.dual_one_hand() || att.state_position == 2 => {
            dmg *= 2;
        }
        _ => {}
    }
    dmg = dmg.max(0);

    // [CA 2.9] defence reactions.
    if let Some(done) = defend(cfg, att, vic, atk, v, dmg, &mut out) {
        if done {
            return out;
        }
        // HP went to zero while parrying: fall through to the death branch.
        let result = defeat_result(cfg, att, vic, atk, act, v);
        finish_lethal(vic, result, &mut out);
        return out;
    }

    land_hit(cfg, rng, att, vic, atk, act, v, dmg, &mut out);
    out
}

/// Player-only special cases evaluated before the pipeline [CA 2.2].
fn player_special(
    cfg: &SimConfig,
    att: &Actor,
    vic: &mut Actor,
    atk: &Attack,
    act: Action,
) -> Option<HitOutcome> {
    let mut out = HitOutcome::new();
    // Non-lethal impact projectile on a humanoid.
    if atk.kind == AttackKind::Projectile
        && !atk.spell_projectile
        && vic.species.is_humanoid()
        && atk.damage_type == DamageType::Impact
    {
        let base = (atk.amount as f32 * atk.hit_mult).round() as i32;
        let d = base + skill_bonus(att, vic, atk, base);
        if vic.hp - d <= 0 {
            out.damage = d.max(0);
            out.result = HitResult::LieKnockDown;
            vic.hp = 1;
            out.hp_lost = out.damage;
            return Some(out);
        }
        if atk.hit_mult >= 0.7 {
            out.result = HitResult::SitKnockDown;
            return Some(out);
        }
        return None;
    }
    // Backstab on an unaware routine victim.
    if vic.ai_mode == AiMode::Routine && !atk.ctx.victim_sees_attacker {
        if act == Action::PierceAttack && att.has_perk("perk_assassin") {
            vic.hp = 0;
            out.hp_lost = 0;
            if vic.can_be_killed() {
                out.result = if vic.species.is_humanoid() {
                    HitResult::LiePiercedDead
                } else {
                    HitResult::LieDead
                };
            } else {
                vic.hp = 1;
                out.result = HitResult::LieKnockDown;
            }
            return Some(out);
        }
        if act == Action::PowerAttack
            && atk.damage_type == DamageType::Impact
            && att.has_perk("perk_knockout")
        {
            out.result = HitResult::LieKnockDown;
            let _ = cfg;
            return Some(out);
        }
    }
    None
}

/// Parry / magic parry. Returns `None` when no parry applies, `Some(true)` when the hit
/// is fully handled, `Some(false)` when the parry cost killed the victim [CA 2.9].
fn defend(
    cfg: &SimConfig,
    att: &Actor,
    vic: &mut Actor,
    atk: &Attack,
    v: i32,
    dmg: i32,
    out: &mut HitOutcome,
) -> Option<bool> {
    let stagger = |v: i32| match v {
        i32::MIN..=1 => HitResult::QuickParadeStumble,
        2 => HitResult::ParadeStumble,
        _ => HitResult::HeavyParadeStumble,
    };
    out.damage = dmg;
    match atk.kind {
        AttackKind::Spell if can_parade_magic(vic, atk) => {
            let cost = if cfg.extended_content {
                let f = (1.0 - (vic.intelligence(cfg) - 200) as f32 / 500.0).clamp(0.0, 1.0);
                (dmg as f32 * f) as i32
            } else {
                dmg
            };
            let short = (vic.mana - cost).min(0);
            vic.mana = (vic.mana - cost).max(0);
            vic.hp += short;
            out.mana_spent = cost + short;
            out.hp_lost = -short;
            out.parried = true;
            if vic.hp > 0 {
                out.result = HitResult::MagicParade;
                return Some(true);
            }
            Some(false)
        }
        AttackKind::Projectile if can_parade_missile(vic, atk) => {
            let mut cost = dmg;
            if vic.is_player && vic.has_perk("perk_shield_2") {
                cost = dmg / 2;
            }
            let short = (vic.stamina - cost).min(0);
            vic.stamina = (vic.stamina - cost).max(0);
            vic.hp += short;
            out.stamina_spent = cost + short;
            out.hp_lost = -short;
            out.parried = true;
            if vic.hp > 0 {
                out.result = if v > 0 {
                    HitResult::ParadeStumble
                } else {
                    stagger(v)
                };
                return Some(true);
            }
            Some(false)
        }
        AttackKind::Melee if can_parade(cfg, att, vic, atk) => {
            let mut c = dmg / 2;
            if vic.equipment.has_shield() {
                c -= c / 4;
                if vic.is_player && vic.has_perk("perk_shield_2") {
                    c /= 2;
                }
            }
            out.parried = true;
            if cfg.ai_mode {
                // AIMode: stamina is the only buffer [CA 2.9].
                let spent = c.min(vic.stamina);
                vic.stamina -= spent;
                out.stamina_spent = spent;
                out.result = stagger(v);
                Some(true)
            } else {
                let short = (vic.stamina - c).min(0);
                vic.stamina = (vic.stamina - c).max(0);
                vic.hp += short;
                out.stamina_spent = c + short;
                out.hp_lost = -short;
                if vic.hp > 0 {
                    out.result = stagger(v);
                    Some(true)
                } else {
                    Some(false)
                }
            }
        }
        _ => {
            out.damage = dmg;
            None
        }
    }
}

/// `CanParade` (S:d480) [CA 2.9]. The stamina clause is inferred (S:d640 not decoded).
pub fn can_parade(cfg: &SimConfig, att: &Actor, vic: &Actor, atk: &Attack) -> bool {
    if !vic.is_parading() || vic.action == Action::HackAttack {
        return false;
    }
    if !atk.ctx.attacker_in_victim_fov {
        return false;
    }
    if cfg.ai_mode
        && matches!(
            att.action,
            Action::PierceAttack
                | Action::WhirlAttack
                | Action::FinishingAttack
                | Action::SprintAttack
        )
    {
        return false;
    }
    if cfg.variants.parry_needs_stamina.unwrap_or(true) && vic.stamina <= 0 {
        return false;
    }
    true
}

/// `CanParadeMagic` (S:dd70): magic-parry staff, parade mode, attacker in view [CA 2.9].
pub fn can_parade_magic(vic: &Actor, atk: &Attack) -> bool {
    vic.equipment.right_use() == Use::Staff && vic.is_parading() && atk.ctx.attacker_in_victim_fov
}

/// `CanParadeMissile` (S:de80): shield + one-hander, parade mode, attacker in view [CA 2.9].
pub fn can_parade_missile(vic: &Actor, atk: &Attack) -> bool {
    vic.equipment.shield_and_one_hander() && vic.is_parading() && atk.ctx.attacker_in_victim_fov
}

/// `IsDeadlyDamage` (S:2db80) [CA 2.11].
pub fn is_deadly(att: &Actor, vic: &Actor, atk: &Attack, act: Action) -> bool {
    if !vic.can_be_killed() {
        return false;
    }
    if vic.boss || vic.name.eq_ignore_ascii_case("Xardas") {
        return true;
    }
    match atk.kind {
        AttackKind::Spell => true,
        AttackKind::Projectile => atk.damage_type != DamageType::Impact,
        AttackKind::Melee => {
            let _ = att;
            act == Action::FinishingAttack
                || !vic.species.is_humanoid()
                || matches!(
                    atk.ctx.victim_to_attacker,
                    Attitude::Hostile | Attitude::Panic
                )
                || atk.ctx.attacker_to_victim == Attitude::Hostile
        }
    }
}

fn defeat_result(
    _cfg: &SimConfig,
    att: &Actor,
    vic: &Actor,
    atk: &Attack,
    act: Action,
    v: i32,
) -> HitResult {
    if is_deadly(att, vic, atk, act) {
        HitResult::LieDead
    } else if act == Action::PierceAttack && vic.species.is_humanoid() {
        HitResult::LiePiercedKO
    } else if v >= 3 {
        HitResult::LieKnockOut
    } else {
        HitResult::LieKnockDown
    }
}

/// Knocked-out victims keep 1 HP (the unconscious loop heals them) [CA 9.4, I].
fn finish_lethal(vic: &mut Actor, result: HitResult, out: &mut HitOutcome) {
    out.result = result;
    if result.is_kill() {
        vic.hp = 0;
    } else {
        vic.hp = 1;
    }
}

/// `CanBurn` (S:2d530) [CA 9.7].
pub fn can_burn(vic: &Actor) -> bool {
    use Species::*;
    !matches!(
        vic.species,
        Golem | Demon | Troll | FireGolem | IceGolem | Dragon
    ) && !(vic.is_player && vic.has_perk("perk_resistheat"))
}

/// `CanFreeze` (S:2d7b0) [CA 9.7].
pub fn can_freeze(vic: &Actor) -> bool {
    can_burn(vic) && !(vic.is_player && vic.has_perk("perk_resistcold"))
}

#[allow(clippy::too_many_arguments)]
fn land_hit(
    cfg: &SimConfig,
    rng: &mut Rng,
    att: &Actor,
    vic: &mut Actor,
    atk: &Attack,
    act: Action,
    v: i32,
    mut dmg: i32,
    out: &mut HitOutcome,
) {
    let _ = rng;
    // [CA 2.10] 1. immortal protection.
    if !vic.is_player && !vic.can_be_killed() {
        dmg = dmg.min(30);
        if dmg >= vic.hp {
            dmg = 0;
        }
    }
    // 2. extended-content adjustments.
    if cfg.extended_content && !vic.is_player {
        if atk.ctx.victim_in_arena {
            dmg /= 2;
        }
        if atk.kind == AttackKind::Projectile
            && !atk.spell_projectile
            && cfg.difficulty == Difficulty::Hard
        {
            dmg = (dmg as f32 * 1.2) as i32;
        }
    }
    // 3. apply.
    out.damage = dmg;
    if dmg > 0 {
        vic.hp -= dmg;
        out.hp_lost = dmg;
    }
    if atk.damage_type == DamageType::Fire && can_burn(vic) && dmg > 0 {
        vic.status |= status::BURNING;
        out.status_applied |= status::BURNING;
    }
    // 4. death / knock-out.
    if vic.hp <= 0 {
        let result = defeat_result(cfg, att, vic, atk, act, v);
        finish_lethal(vic, result, out);
        return;
    }
    // 5. survived: stagger.
    if vic.action == Action::SprintAttack {
        out.result = HitResult::NoReaction;
        return;
    }
    if v <= 0 {
        out.result = HitResult::NoReaction; // PipiStumble
        return;
    }
    if matches!(act, Action::PierceAttack | Action::HackAttack)
        && atk.quality & 4 != 0
        && !vic.species.is_evil()
        && !(vic.is_player && vic.has_perk("perk_immunetopoison"))
    {
        vic.status |= status::POISONED;
        out.status_applied |= status::POISONED;
    }
    if atk.quality & 2 != 0 && !(vic.is_player && vic.has_perk("perk_immunetodisease")) {
        vic.status |= status::DISEASED;
        out.status_applied |= status::DISEASED;
    }
    let humanoid = vic.species.is_humanoid();
    out.result = if act == Action::PierceAttack && humanoid {
        HitResult::PierceStumble
    } else if atk.damage_type == DamageType::Ice && can_freeze(vic) {
        out.freeze = 12 * 5.max(dmg / 20);
        vic.status |= status::FROZEN;
        out.status_applied |= status::FROZEN;
        HitResult::NoReaction
    } else if damage_weakness(vic, atk) == 2
        || !humanoid
        || vic.equipment.weapon_mode() == WeaponMode::None
    {
        HitResult::Stumble
    } else if humanoid && v >= 4 && vic.equipment.weapon_mode() == WeaponMode::Melee {
        HitResult::SitKnockDown
    } else if v >= 2 {
        HitResult::Stumble
    } else {
        HitResult::QuickStumble
    };
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
    use crate::item::{ItemDef, Requirement};
    use crate::types::ActorId;

    fn cfg() -> SimConfig {
        SimConfig::default()
    }

    fn hero_with_sword(c: &SimConfig) -> Actor {
        let mut h = Actor::hero(ActorId(0), c);
        let mut s = ItemDef::weapon("It_1H_Sword_01", "1H", "Blade", 50);
        s.requirements.push(Requirement {
            tag: "STR".into(),
            op: 0,
            value: 100,
        });
        h.equipment.equip(s);
        h.action = Action::Attack;
        h
    }

    fn orc(c: &SimConfig) -> Actor {
        let mut o = Actor::npc(ActorId(1), "Orc_01", Species::Orc, 5, 30);
        o.refresh(c);
        o.equipment
            .equip(ItemDef::weapon("It_Axe_Orc_01", "2H", "Blade", 150));
        o
    }

    #[test]
    fn worked_example_hero_hits_orc() {
        // [CA 2.5, 2.7] hero STR 100, sword 50: b = 50, e = STR/2 = 50, dmg = 100;
        // orc LevelMax 30 -> protection 45 percent: 100 - trunc(45*100/100) = 55.
        let c = cfg();
        let hero = hero_with_sword(&c);
        let mut o = orc(&c);
        let mut rng = Rng::new(1);
        let atk = Attack::melee(&hero);
        let out = resolve_hit(&c, &mut rng, &hero, &mut o, &atk);
        assert_eq!(out.damage, 55);
        assert_eq!(o.hp, 600 - 55);
        // v = max(1, M(Attack)=2 + R(hero)=0 - D(orc)=0) = 2 -> Stumble class 26 (orc holds no weapon mode? it does).
        assert_eq!(out.strength_class, 2);
        assert_eq!(out.perception, Some(PerceptionKind::FightSound));
    }

    #[test]
    fn move_multipliers_and_head_shot() {
        let c = cfg();
        let mut hero = hero_with_sword(&c);
        let mut rng = Rng::new(1);
        // Hack attack doubles after protection: 55 * 2 = 110.
        hero.action = Action::HackAttack;
        let mut o = orc(&c);
        let out = resolve_hit(&c, &mut rng, &hero, &mut o, &Attack::melee(&hero));
        assert_eq!(out.damage, 110);
        // Quick attack halves: 55 / 2 = 27.
        hero.action = Action::QuickAttack;
        let mut o = orc(&c);
        let out = resolve_hit(&c, &mut rng, &hero, &mut o, &Attack::melee(&hero));
        assert_eq!(out.damage, 27);
        // Head shot x2 on the raw damage: b = 100, e = 50 -> 150, -45% = 150 - 67 = 83.
        hero.action = Action::Attack;
        let mut o = orc(&c);
        let mut atk = Attack::melee(&hero);
        atk.head_hit = true;
        let out = resolve_hit(&c, &mut rng, &hero, &mut o, &atk);
        assert!(out.head_shot);
        assert_eq!(out.damage, 83);
    }

    #[test]
    fn worked_example_orc_hits_hero_flat_protection() {
        // [CA 2.5] Orc L10 STR 25, weapon b0 = 150: trunc(0.75*25 + 150/7) = 40.
        let c = cfg();
        let mut hero = hero_with_sword(&c);
        let mut o = orc(&c);
        o.action = Action::Attack;
        let mut rng = Rng::new(1);
        let out = resolve_hit(&c, &mut rng, &o, &mut hero, &Attack::melee(&o));
        assert_eq!(out.damage, 40);
        assert_eq!(hero.hp, 160);
        // Armor 60 blade protection -> 12 flat: 40 - 12 = 28.
        let mut armored = hero_with_sword(&c);
        armored.equipment.equip(ItemDef {
            use_type: Some("Body".into()),
            modifiers: vec![crate::item::Modifier {
                tag: "PROT_BLADE".into(),
                op: 0,
                value: 60,
            }],
            ..ItemDef::default()
        });
        let out = resolve_hit(&c, &mut rng, &o, &mut armored, &Attack::melee(&o));
        assert_eq!(out.damage, 28);
        // Vanilla rules: percent mode with 60 protection: 40 -> 40 - 24 = 16 (non-ext NPC uses STR only).
        let v = SimConfig::vanilla();
        let mut hv = hero_with_sword(&v);
        hv.equipment.equip(armored.equipment.body.clone().unwrap());
        let out = resolve_hit(&v, &mut rng, &o, &mut hv, &Attack::melee(&o));
        // vanilla: b = STR = 25, prot 60 percent -> 25 - 15 = 10.
        assert_eq!(out.damage, 10);
    }

    #[test]
    fn flat_protection_chip_damage() {
        // [CA 2.7] dmg < 1: no chip below 40 unless Power/Hack; b > 39 gives 1-2 percent.
        let c = cfg();
        let mut hero = hero_with_sword(&c);
        hero.equipment.equip(ItemDef {
            use_type: Some("Body".into()),
            modifiers: vec![crate::item::Modifier {
                tag: "PROT_IMPACT".into(),
                op: 0,
                value: 400,
            }],
            ..ItemDef::default()
        }); // fists are impact damage: 400 / 5 = 80
        let mut weak = Actor::npc(ActorId(2), "W", Species::Human, 5, 5);
        weak.action = Action::Attack;
        let mut rng = Rng::new(3);
        let out = resolve_hit(&c, &mut rng, &weak, &mut hero, &Attack::melee(&weak));
        assert_eq!(out.damage, 0, "STR 17 minus protection 80 is below 1 -> 0");
        // Hack attack by an L100 human: STR 160, b = 160 (unarmed => 0.75*160=120), flat 120-80 = 40.
        let mut strong = Actor::npc(ActorId(3), "S", Species::Human, 100, 100);
        strong.action = Action::HackAttack;
        let out = resolve_hit(&c, &mut rng, &strong, &mut hero, &Attack::melee(&strong));
        assert_eq!(out.damage, (120 - 80) * 2, "hack doubles");
    }

    #[test]
    fn weakness_table() {
        let c = cfg();
        let sk = Actor::npc(ActorId(5), "Sk", Species::Skeleton, 5, 5);
        let imp = Attack::melee(&Actor::npc(ActorId(6), "x", Species::Human, 1, 1));
        assert_eq!(imp.damage_type, DamageType::Impact);
        assert_eq!(damage_weakness(&sk, &imp), 2);
        let mut arrow = imp.clone();
        arrow.damage_type = DamageType::Missile;
        assert_eq!(damage_weakness(&sk, &arrow), 0);
        arrow.entity_name = "It_Arrow_Fire".into();
        assert_eq!(damage_weakness(&sk, &arrow), 1);
        let golem = Actor::npc(ActorId(7), "G", Species::Golem, 5, 5);
        arrow.entity_name.clear();
        assert_eq!(damage_weakness(&golem, &arrow), 0);
        let mut fire = imp.clone();
        fire.damage_type = DamageType::Fire;
        let ig = Actor::npc(ActorId(8), "IG", Species::IceGolem, 5, 5);
        assert_eq!(damage_weakness(&ig, &fire), 2);
        let dr = Actor::npc(ActorId(9), "D", Species::Dragon, 5, 5);
        assert_eq!(damage_weakness(&dr, &fire), 0);
        // Immune hits are dropped by the gate.
        let mut rng = Rng::new(1);
        let mut golem = golem;
        golem.refresh(&c);
        let hero = hero_with_sword(&c);
        let out = resolve_hit(&c, &mut rng, &hero, &mut golem, &arrow);
        assert_eq!(out.ignored, Some(IgnoreReason::Immune));
        assert_eq!(golem.hp, golem.hp_max(&c));
    }

    #[test]
    fn parry_costs_stamina_only_with_ai_mode() {
        // [CA 2.9] orc hits for 40; hero parries with a shield: c = 20, shield -25% -> 15.
        let c = cfg();
        let mut hero = hero_with_sword(&c);
        hero.equipment.equip(ItemDef {
            name: "It_Shield".into(),
            use_type: Some("Shield".into()),
            ..ItemDef::default()
        });
        hero.action = Action::Parade;
        let mut o = orc(&c);
        o.action = Action::Attack;
        let mut rng = Rng::new(1);
        let out = resolve_hit(&c, &mut rng, &o, &mut hero, &Attack::melee(&o));
        assert!(out.parried);
        assert_eq!(out.stamina_spent, 15);
        assert_eq!(hero.stamina, 85);
        assert_eq!(hero.hp, 200, "AIMode: no HP loss at all");
        // v = max(1, M(Attack)=2 + R(orc L10)=0 - D(hero)=0) = 2 -> ParadeStumble (21);
        // DoLogicalDamage returns 0 for parries.
        assert_eq!(out.result, HitResult::ParadeStumble);
        assert!(!out.returns_to_caller);
        // Pierce and sprint attacks cannot be parried under AIMode.
        o.action = Action::PierceAttack;
        let out = resolve_hit(&c, &mut rng, &o, &mut hero, &Attack::melee(&o));
        assert!(!out.parried);
        // Vanilla: STR-only NPC hit of 25 (no protection): c = 12, shield -25% = 9; with 5
        // stamina left the shortfall of 4 hurts the victim.
        let v = SimConfig::vanilla();
        let mut hv = hero_with_sword(&v);
        hv.equipment.equip(ItemDef {
            use_type: Some("Shield".into()),
            ..ItemDef::default()
        });
        hv.action = Action::Parade;
        hv.stamina = 5;
        o.action = Action::Attack;
        let out = resolve_hit(&v, &mut rng, &o, &mut hv, &Attack::melee(&o));
        assert!(out.parried);
        assert_eq!(hv.stamina, 0);
        assert_eq!(out.hp_lost, 4);
    }

    #[test]
    fn exhausted_parry_fails() {
        let c = cfg();
        let mut hero = hero_with_sword(&c);
        hero.action = Action::Parade;
        hero.stamina = 0;
        let mut o = orc(&c);
        o.action = Action::Attack;
        let mut rng = Rng::new(1);
        let out = resolve_hit(&c, &mut rng, &o, &mut hero, &Attack::melee(&o));
        assert!(!out.parried);
        assert_eq!(hero.hp, 160);
    }

    #[test]
    fn kill_versus_knock_out() {
        // [CA 2.11] a melee defeat of a humanoid is lethal only if either side is Hostile.
        let c = cfg();
        let hero = hero_with_sword(&c);
        let mut rng = Rng::new(1);
        let mut victim = Actor::npc(ActorId(2), "V", Species::Human, 1, 1);
        victim.refresh(&c);
        victim.hp = 5;
        let mut atk = Attack::melee(&hero);
        atk.ctx.victim_to_attacker = Attitude::Neutral;
        atk.ctx.attacker_to_victim = Attitude::Neutral;
        atk.ctx.victim_is_attacker_target = true;
        let out = resolve_hit(&c, &mut rng, &hero, &mut victim, &atk);
        assert!(out.defeated(), "{:?}", out.result);
        assert_eq!(victim.hp, 1);
        assert_eq!(out.perception, Some(PerceptionKind::Defeat));
        // Hostile: dies.
        let mut victim = Actor::npc(ActorId(2), "V", Species::Human, 1, 1);
        victim.refresh(&c);
        victim.hp = 5;
        let out = resolve_hit(&c, &mut rng, &hero, &mut victim, &Attack::melee(&hero));
        assert!(out.killed());
        assert_eq!(out.perception, Some(PerceptionKind::Murder));
        assert_eq!(out.result.code(), 36);
        assert_eq!(victim.hp, 0);
        // Non-humanoids are always killed; strong hits (v >= 3) knock out instead of down.
        let mut wolf = Actor::npc(ActorId(3), "W", Species::Wolf, 1, 1);
        wolf.refresh(&c);
        wolf.hp = 1;
        let mut atk = Attack::melee(&hero);
        atk.ctx.victim_to_attacker = Attitude::Neutral;
        atk.ctx.attacker_to_victim = Attitude::Neutral;
        assert!(resolve_hit(&c, &mut rng, &hero, &mut wolf, &atk).killed());
        // Immortal NPCs are capped at 30 damage and never reach 0.
        let mut imm = Actor::npc(ActorId(4), "Imm", Species::Human, 1, 1);
        imm.refresh(&c);
        imm.immortal = true;
        imm.hp = 20;
        let out = resolve_hit(&c, &mut rng, &hero, &mut imm, &Attack::melee(&hero));
        assert_eq!(out.damage, 0);
        assert_eq!(imm.hp, 20);
    }

    #[test]
    fn pierce_and_strength_classes() {
        // v = max(1, M + R - D). Level 45 orc: R = 3; hero with shield + Perk_Shield_2: D = 1.
        let c = cfg();
        let mut big = Actor::npc(ActorId(1), "Big", Species::Orc, 45, 45);
        big.action = Action::PowerAttack;
        assert_eq!(move_strength(&big, Action::PowerAttack), 4);
        assert_eq!(power_rank(&c, &big), 3);
        let mut hero = hero_with_sword(&c);
        hero.equipment.equip(ItemDef {
            use_type: Some("Shield".into()),
            ..ItemDef::default()
        });
        hero.perks.insert("perk_shield_2".into());
        assert_eq!(defence_strength(&c, &hero), 1);
        // v = 4 + 3 - 1 = 6 -> heavy parry stagger when parried.
        hero.action = Action::Parade;
        let mut rng = Rng::new(1);
        let out = resolve_hit(&c, &mut rng, &big, &mut hero, &Attack::melee(&big));
        assert_eq!(out.strength_class, 6);
        assert_eq!(out.result, HitResult::HeavyParadeStumble);
        // Player rank from perks: dual one-handers with the secret perk -> 2.
        let mut dual = hero_with_sword(&c);
        dual.equipment
            .equip(ItemDef::weapon("S2", "1H", "Blade", 10));
        dual.perks.insert("perk_1h1h_secretbonus".into());
        assert_eq!(power_rank(&c, &dual), 2);
        assert_eq!(move_strength(&dual, Action::PowerAttack), 2);
        dual.state_position = 2;
        assert_eq!(move_strength(&dual, Action::PowerAttack), 3);
    }

    #[test]
    fn stagger_survived_branch() {
        let c = cfg();
        let hero = hero_with_sword(&c);
        let mut rng = Rng::new(1);
        // Pierce vs humanoid -> PierceStumble (34).
        let mut h = hero_with_sword(&c);
        h.action = Action::PierceAttack;
        let mut o = orc(&c);
        let out = resolve_hit(&c, &mut rng, &h, &mut o, &Attack::melee(&h));
        assert_eq!(out.result, HitResult::PierceStumble);
        // Normal attack on a wolf (non-humanoid) -> Stumble 26.
        let mut w = Actor::npc(ActorId(9), "Wolf", Species::Wolf, 20, 20);
        w.refresh(&c);
        let out = resolve_hit(&c, &mut rng, &hero, &mut w, &Attack::melee(&hero));
        assert_eq!(out.result, HitResult::Stumble);
    }

    #[test]
    fn bow_and_perks() {
        // [CA 9.6] draw multiplier: 0.2 at t=0, 1.0 from 1.95 s.
        assert!((bow_hit_multiplier(0.0) - 0.2).abs() < 1e-6);
        assert!((bow_hit_multiplier(1.95) - 1.0).abs() < 1e-6);
        assert!((bow_hit_multiplier(5.0) - 1.0).abs() < 1e-6);
        let c = cfg();
        let mut hero = Actor::hero(ActorId(0), &c);
        hero.equipment.equip(ItemDef {
            name: "It_Bow".into(),
            use_type: Some("Bow".into()),
            ..ItemDef::default()
        });
        hero.perks.insert("perk_bow_2".into());
        let arrow = ItemDef::weapon("It_Arrow", "Arrow", "Missile", 40);
        let mut wolf = Actor::npc(ActorId(9), "Wolf", Species::Wolf, 10, 10);
        wolf.refresh(&c);
        let mut rng = Rng::new(1);
        // b = 40, e = DEX/2 = 50, skill +25 (Bow_2): 40 + 25 + 50 = 115; wolf prot 15% -> 115 - 17 = 98.
        let out = resolve_hit(
            &c,
            &mut rng,
            &hero,
            &mut wolf,
            &Attack::projectile(&arrow, 1.0),
        );
        assert_eq!(out.damage, 98);
        // Half draw: mult 0.6 -> b = 24 + 25 + 50 = 99 -> 99 - 14 = 85; mult >= 0.6 keeps v = 2.
        let mut wolf = Actor::npc(ActorId(9), "Wolf", Species::Wolf, 10, 10);
        wolf.refresh(&c);
        let out = resolve_hit(
            &c,
            &mut rng,
            &hero,
            &mut wolf,
            &Attack::projectile(&arrow, 0.6),
        );
        assert_eq!(out.damage, 85);
    }

    #[test]
    fn spell_damage_and_magic_parry() {
        let c = cfg();
        let mut hero = Actor::hero(ActorId(0), &c);
        hero.base.int = 200;
        hero.perks.insert("perk_firemage".into());
        let mut orc = orc(&c);
        orc.refresh(&c);
        // Fireball 60 damage, 20 mana used: b = 2*min(60,100) = 120; e = 120*200/100 = 240;
        // Firemage +b -> b = 240; dmg = 480; orc fire protection 45% -> 480 - 216 = 264.
        let spell = Attack::spell("Spl_Fireball", DamageType::Fire, 60, 20, Deity::Innos);
        let mut rng = Rng::new(1);
        let out = resolve_hit(&c, &mut rng, &hero, &mut orc, &spell);
        assert_eq!(out.damage, 264);
        // Magic parry [CA 2.9]: victim INT 450 pays f = 1 - 250/500 = 0.5 of 100 damage.
        let mut mage = Actor::hero(ActorId(2), &c);
        mage.base.int = 450;
        mage.equipment.equip(ItemDef {
            use_type: Some("Staff".into()),
            ..ItemDef::default()
        });
        mage.action = Action::Parade;
        mage.mana = 100;
        let mut caster = Actor::npc(ActorId(3), "C", Species::Human, 1, 1);
        caster.refresh(&c);
        // NPC: b = STR 11, spell projectile doubles -> 22; keep it simple: check the cost on a big caster.
        let mut big = Actor::npc(ActorId(4), "Big", Species::Human, 200, 200);
        big.refresh(&c);
        let sp = Attack::spell("Spl_Fireball", DamageType::Fire, 60, 20, Deity::Innos);
        let out = resolve_hit(&c, &mut rng, &big, &mut mage, &sp);
        // big STR = 10 + 300 = 310; spell projectile -> *2 = 620; mage Fire prot 0 -> dmg 620.
        // cost = 620 * 0.5 = 310 > 100 mana -> all 100 mana spent, shortfall 210 hits HP (200).
        assert!(out.parried);
        assert_eq!(out.mana_spent, 100);
        assert_eq!(out.hp_lost, 210);
        assert!(out.killed() || out.defeated());
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

    #[test]
    fn gate_rules() {
        let c = cfg();
        let hero = hero_with_sword(&c);
        let mut rng = Rng::new(1);
        let mut v = orc(&c);
        v.ai_mode = AiMode::Dead;
        assert_eq!(
            resolve_hit(&c, &mut rng, &hero, &mut v, &Attack::melee(&hero)).ignored,
            Some(IgnoreReason::VictimMode)
        );
        let mut v = orc(&c);
        v.ani_state = AniState::LieKnockDown;
        assert_eq!(
            resolve_hit(&c, &mut rng, &hero, &mut v, &Attack::melee(&hero)).ignored,
            Some(IgnoreReason::KnockedDown)
        );
        // An NPC's stray hit on a neutral bystander that is not its target is ignored.
        let mut npc = orc(&c);
        npc.action = Action::Attack;
        let mut by = hero_with_sword(&c);
        let mut atk = Attack::melee(&npc);
        atk.ctx.victim_is_attacker_target = false;
        atk.ctx.attacker_to_victim = Attitude::Neutral;
        assert_eq!(
            resolve_hit(&c, &mut rng, &npc, &mut by, &atk).ignored,
            Some(IgnoreReason::AccidentalAlly)
        );
        // Pierce attacks only hurt the target.
        npc.action = Action::PierceAttack;
        let mut atk = Attack::melee(&npc);
        atk.ctx.victim_is_attacker_target = false;
        assert_eq!(
            resolve_hit(&c, &mut rng, &npc, &mut by, &atk).ignored,
            Some(IgnoreReason::OnlyHurtsTarget)
        );
    }

    #[test]
    fn player_special_cases() {
        let c = cfg();
        let mut rng = Rng::new(1);
        // Assassin backstab on an unaware routine NPC.
        let mut hero = hero_with_sword(&c);
        hero.action = Action::PierceAttack;
        hero.perks.insert("perk_assassin".into());
        let mut v = Actor::npc(ActorId(2), "V", Species::Human, 20, 20);
        v.refresh(&c);
        let mut atk = Attack::melee(&hero);
        atk.ctx.victim_sees_attacker = false;
        let out = resolve_hit(&c, &mut rng, &hero, &mut v, &atk);
        assert_eq!(out.result, HitResult::LiePiercedDead);
        assert_eq!(v.hp, 0);
        // Non-lethal impact projectile knocks a weak humanoid out.
        let mut hero2 = Actor::hero(ActorId(0), &c);
        hero2.equipment.equip(ItemDef {
            use_type: Some("CrossBow".into()),
            ..ItemDef::default()
        });
        let bolt = ItemDef::weapon("It_Bolt_Stun", "Bolt", "Impact", 100);
        let mut weak = Actor::npc(ActorId(3), "W", Species::Human, 1, 1);
        weak.refresh(&c);
        weak.hp = 30;
        let out = resolve_hit(
            &c,
            &mut rng,
            &hero2,
            &mut weak,
            &Attack::projectile(&bolt, 1.0),
        );
        assert_eq!(out.result, HitResult::LieKnockDown);
        assert_eq!(weak.hp, 1);
    }
}
