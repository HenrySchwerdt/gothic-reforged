//! B07: the damage pipeline (`DoLogicalDamage`, `AssessHit` and the rules around it) and
//! experience (`GiveXP`).
//!
//! The engine hands the script a *damage entity* (weapon, projectile, spell); everything the
//! rules read from it is collected in [`HitEvent`] by the host. All numbers follow
//! COMBAT_AI_RULES section 2 as re-verified against the decompiled bodies; the shipped
//! `ExtendedContent` and `AIMode` flags are read from [`crate::config::EngineSetup`].

use crate::attitude::get_attitude;
use crate::enums::{
    Action, AiMode, AniState, Attitude, DamageType, NpcType, PartyMemberType, Slot, Species,
    SpellDeity, UseType,
};
use crate::helpers::{WeaponMode, weapon_mode};
use crate::host::{AiCommand, Attribute, EntityId, Pool, Protection, ScriptHost};
use crate::predicates::{
    is_ambient_creature, is_boss, is_evil, is_humanoid, is_in_fist_mode, species_is_evil,
};
use crate::routine::state::State;
use crate::stats;

/// What kind of damage entity hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitKind {
    /// A weapon swing or a natural attack.
    Melee,
    /// An arrow or bolt.
    Projectile,
    /// A projectile spell (fire bolt, ice block, ...).
    SpellProjectile,
    /// A spell that damages without a projectile (area ticks, instant spells).
    SpellDirect,
}

impl HitKind {
    /// `S:2af20`-style test: the damage comes from a spell.
    pub const fn is_spell(self) -> bool {
        matches!(self, Self::SpellProjectile | Self::SpellDirect)
    }
}

/// The facts of one hit that live on the damage entity.
#[derive(Debug, Clone)]
pub struct HitEvent {
    /// Who is hit (`Self` of the callback).
    pub victim: EntityId,
    /// The owner of the damage entity (`GetOwner`), `NONE` for environmental damage.
    pub attacker: EntityId,
    /// What hit.
    pub kind: HitKind,
    /// `PSDamage.DamageType`.
    pub damage_type: DamageType,
    /// `PSDamage.DamageAmount`.
    pub amount: i32,
    /// `PSDamage.DamageHitMultiplier`.
    pub hit_mult: f32,
    /// `PSDamage.ManaUsed`.
    pub mana_used: i32,
    /// The projectile's path style is Missile (3).
    pub path_missile: bool,
    /// The collision shape touched a bone named `_Head`.
    pub head_shot: bool,
    /// Name of the damage entity (checked for "Fire"/"Explosive" on missiles, `Spl_*` for spells).
    pub entity_name: String,
    /// Deity of the spell, if any.
    pub spell_deity: Option<SpellDeity>,
    /// `PSItem.Quality` bits of the weapon.
    pub weapon_quality: u32,
    /// Template name of the attacker's weapon (`Fist` for unarmed).
    pub weapon_name: String,
    /// The hit touched the victim at one of its bones (vs. only its volume), for logging.
    pub touched_shape: bool,
}

impl HitEvent {
    /// A melee hit with the given damage.
    pub fn melee(
        victim: EntityId,
        attacker: EntityId,
        damage_type: DamageType,
        amount: i32,
    ) -> Self {
        Self {
            victim,
            attacker,
            kind: HitKind::Melee,
            damage_type,
            amount,
            hit_mult: 1.0,
            mana_used: 0,
            path_missile: false,
            head_shot: false,
            entity_name: String::new(),
            spell_deity: None,
            weapon_quality: 0,
            weapon_name: String::new(),
            touched_shape: false,
        }
    }
}

/// Result of [`assess_hit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HitOutcome {
    /// The `gEAction` the hit resolves to (`Action::None` = no reaction).
    pub action: Action,
    /// Damage written to `PSDamageReceiver.DamageAmount`.
    pub damage: i32,
}

/// Weakness classes of S:7250.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weakness {
    /// Immune.
    Immune,
    /// Normal damage.
    Normal,
    /// Double damage.
    Weak,
}

/// Port of the damage-type weakness helper (S:7250): immune, normal or weak.
///
/// Impact doubles against Skeleton and Golem; Missile cannot hurt Golem, FireGolem and (unless
/// the damage entity is called "...Fire..." or "...Explosive...") Skeleton and IceGolem; Fire
/// cannot hurt FireVaran, FireGolem, Dragon and doubles against IceGolem; Ice cannot hurt a
/// frozen victim (status 2) or an IceGolem and doubles against FireGolem; Lightning doubles
/// against Golem. A victim without an NPC property set or a hit without a damage type is
/// immune.
pub fn weakness(host: &dyn ScriptHost, victim: EntityId, ev: &HitEvent) -> Weakness {
    gothic_portcov::hit!("weakness");
    use DamageType as D;
    use Species as S;
    if !host.exists(victim) || ev.damage_type == D::None {
        return Weakness::Immune;
    }
    let s = host.species(victim);
    let fiery = ev.entity_name.contains("Fire") || ev.entity_name.contains("Explosive");
    match ev.damage_type {
        D::Impact if matches!(s, S::Skeleton | S::Golem) => Weakness::Weak,
        D::Missile => match s {
            S::Golem | S::FireGolem => Weakness::Immune,
            S::Skeleton | S::IceGolem if !fiery => Weakness::Immune,
            _ => Weakness::Normal,
        },
        D::Fire => match s {
            S::FireVaran | S::FireGolem | S::Dragon => Weakness::Immune,
            S::IceGolem => Weakness::Weak,
            _ => Weakness::Normal,
        },
        D::Ice => match s {
            _ if host.has_status(victim, 2) => Weakness::Immune,
            S::IceGolem => Weakness::Immune,
            S::FireGolem => Weakness::Weak,
            _ => Weakness::Normal,
        },
        D::Lightning if s == S::Golem => Weakness::Weak,
        _ => Weakness::Normal,
    }
}

/// Port of `GetProtection` (S:2de50): the victim's protection against a damage type, with the
/// armor perks applied.
///
/// Physics and None use Impact. Without `ExtendedContent` a robe with `Perk_LightArmor` doubles
/// the value, other armor with `Perk_HeavyArmor` adds a quarter, and everything caps at 90.
/// With `ExtendedContent` (shipped) the chest armor's `PROT_<TYPE>` modifier adds 20 percent
/// (light armor, robe) or 5 percent (heavy armor, non-robe), uncapped.
pub fn armor_protection(host: &dyn ScriptHost, victim: EntityId, ty: DamageType) -> i32 {
    gothic_portcov::hit!("armor_protection");
    let prot = match ty {
        DamageType::Blade => Protection::Blades,
        DamageType::Missile => Protection::Missile,
        DamageType::Fire => Protection::Fire,
        DamageType::Ice => Protection::Ice,
        DamageType::Lightning => Protection::Lightning,
        _ => Protection::Impact,
    };
    let base = stats::protection(host, victim, prot);
    let robe = host.chest_is_robe(victim);
    let light = host.is_skill_active(victim, "Perk_LightArmor");
    let heavy = host.is_skill_active(victim, "Perk_HeavyArmor");
    if host.setup().extended_content {
        let modifier = host.chest_protection_modifier(victim, prot);
        if robe && light {
            base + modifier * 20 / 100
        } else if !robe && heavy {
            base + modifier * 5 / 100
        } else {
            base
        }
    } else if robe && light {
        (base * 2).min(90)
    } else if !robe && heavy {
        (base + base / 4).min(90)
    } else {
        base.min(90)
    }
}

/// Port of `M(action)` (S:2b3a0): how much a move contributes to the hit strength class.
fn move_strength(host: &dyn ScriptHost, attacker: EntityId, act: Action) -> i32 {
    gothic_portcov::hit!("move_strength");
    match act {
        Action::Attack => 2,
        Action::PowerAttack | Action::SprintAttack => {
            let dual = host.slot_use_type(attacker, Slot::LeftHand) == UseType::OneH
                && host.slot_use_type(attacker, Slot::RightHand) == UseType::OneH;
            if dual {
                if host.state_position(attacker) >= 2 {
                    3
                } else {
                    2
                }
            } else {
                4
            }
        }
        Action::SimpleWhirl | Action::WhirlAttack | Action::PierceAttack | Action::GetUpAttack => 2,
        Action::HackAttack => 4,
        _ => 0,
    }
}

/// Port of `R(entity)` (S:2afc0): power rank 0..=3. The player's comes from the equipped weapon
/// perks, an NPC's from its level (>= 45: 3, >= 30: 2, >= 15: 1).
pub fn power_rank(host: &dyn ScriptHost, e: EntityId) -> i32 {
    gothic_portcov::hit!("power_rank");
    if e == host.player() && !host.is_transformed(e) {
        let left = host.slot_use_type(e, Slot::LeftHand);
        let right = host.slot_use_type(e, Slot::RightHand);
        let skill = |name: &str| host.is_skill_active(e, name);
        let rank = |top: &str, mid: &str| {
            if skill(top) {
                2
            } else if skill(mid) {
                1
            } else {
                0
            }
        };
        return match (left, right) {
            (UseType::OneH, UseType::OneH) => rank("Perk_1H1H_SecretBonus", "Perk_1H1H_2"),
            (_, UseType::OneH) => rank("Perk_1H_3", "Perk_1H_2"),
            (_, UseType::Axe | UseType::Halberd | UseType::TwoH) => {
                rank("Perk_Axe_3", "Perk_Axe_2")
            }
            (_, UseType::Staff) => rank("Perk_Staff_3", "Perk_Staff_2"),
            _ => 0,
        };
    }
    match stats::level(host, e) {
        45.. => 3,
        30..=44 => 2,
        15..=29 => 1,
        _ => 0,
    }
}

/// Port of `D(victim)` (S:2b470): defence value: 2 while getting up with a parade, otherwise the
/// power rank plus one for a shield (NPCs always, the player only with `Perk_Shield_2`).
fn defence_value(host: &dyn ScriptHost, victim: EntityId) -> i32 {
    gothic_portcov::hit!("defence_value");
    if host.current_action(victim) == Action::GetUpParade {
        return 2;
    }
    let shield = host.slot_use_type(victim, Slot::LeftHand) == UseType::Shield;
    let counts =
        shield && (victim != host.player() || host.is_skill_active(victim, "Perk_Shield_2"));
    power_rank(host, victim) + i32::from(counts)
}

/// Hit strength class `v` that drives stagger and knock-down (COMBAT_AI_RULES 2.3).
pub fn hit_strength(host: &dyn ScriptHost, ev: &HitEvent, attacker_action: Action) -> i32 {
    if ev.kind.is_spell() {
        return 2;
    }
    if ev.kind == HitKind::Projectile {
        return if ev.hit_mult >= 0.6 { 2 } else { 0 };
    }
    if ev.attacker.is_none() {
        return 0;
    }
    (move_strength(host, ev.attacker, attacker_action) + power_rank(host, ev.attacker)
        - defence_value(host, ev.victim))
    .max(1)
}

/// Port of the weapon-quality bonus (S:6af10): a damaged weapon (0x100) loses half the base, each
/// of the bonus flags 0x20, 0x40, 0x80 adds 10.
pub fn quality_bonus(quality: u32, base: i32) -> i32 {
    gothic_portcov::hit!("quality_bonus");
    let mut b = 0;
    if quality & 0x100 != 0 {
        b -= base / 2;
    }
    for flag in [0x20, 0x40, 0x80] {
        if quality & flag != 0 {
            b += 10;
        }
    }
    b
}

/// Port of `GetPlayerSkillDamageBonus` (S:2e410): the player's perk bonus on a hit of `base`
/// damage against `victim`. Only for the player; the two Hunter perks (game versus other
/// animals, S:57fc0/57bd0) are not ported.
pub fn skill_bonus(
    host: &dyn ScriptHost,
    attacker: EntityId,
    victim: EntityId,
    ev: &HitEvent,
    base: i32,
) -> i32 {
    gothic_portcov::hit!("skill_bonus");
    if attacker != host.player() {
        return 0;
    }
    let skill = |n: &str| host.is_skill_active(attacker, n);
    let mut bonus = 0;
    if ev.kind.is_spell() {
        let school = match ev.spell_deity {
            Some(SpellDeity::Innos) => skill("Perk_Firemage"),
            Some(SpellDeity::Adanos) => skill("Perk_Watermage"),
            Some(SpellDeity::Beliar) => skill("Perk_Darkmage"),
            _ => false,
        };
        return if school { base } else { 0 };
    }
    let tiered = |top: &str, mid: &str, bonus: &mut i32| {
        if skill(top) {
            *bonus = 25;
        } else if skill(mid) {
            *bonus += 25;
        }
    };
    let left = host.slot_use_type(attacker, Slot::LeftHand);
    let right = host.slot_use_type(attacker, Slot::RightHand);
    if ev.kind == HitKind::Projectile {
        match (left, right) {
            (UseType::Bow, _) => tiered("Perk_Bow_3", "Perk_Bow_2", &mut bonus),
            (UseType::CrossBow, _) => tiered("Perk_Crossbow_3", "Perk_Crossbow_2", &mut bonus),
            _ => {}
        }
    } else {
        match (left, right) {
            (UseType::OneH, UseType::OneH) => {
                tiered("Perk_1H1H_SecretBonus", "Perk_1H1H_2", &mut bonus)
            }
            (_, UseType::OneH) => tiered("Perk_1H_3", "Perk_1H_2", &mut bonus),
            (_, UseType::Axe | UseType::Halberd | UseType::TwoH) => {
                tiered("Perk_Axe_3", "Perk_Axe_2", &mut bonus)
            }
            (_, UseType::Staff) => tiered("Perk_Staff_3", "Perk_Staff_2", &mut bonus),
            _ => {}
        }
    }
    let species = host.species(victim);
    if species == Species::Orc
        && (skill("Perk_OrcSlayer") || (ev.kind == HitKind::Projectile && skill("Perk_OrcHunter")))
    {
        bonus += 25;
    }
    if skill("Perk_Paladin") && species_is_evil(species) && ev.kind != HitKind::Projectile {
        bonus += 25;
    }
    bonus
}

/// Is the attacker's hand pair a one-hander in each hand (dual wield)?
fn dual_wield(host: &dyn ScriptHost, e: EntityId) -> bool {
    host.slot_use_type(e, Slot::LeftHand) == UseType::OneH
        && host.slot_use_type(e, Slot::RightHand) == UseType::OneH
}

/// Port of `CanBeKilled` (S:2da00): essential NPCs cannot be killed [I].
pub fn can_be_killed(host: &dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("can_be_killed");
    !host.is_immortal(npc)
}

/// Port of `IsDeadlyDamage` (S:2db80): lethal or merely knocking out?
///
/// Killable victims die from bosses' hits... precisely: a victim that is a Boss or named
/// "Xardas", any spell, any non-Impact projectile, and every projectile against a non-humanoid
/// is lethal; Impact projectiles against humanoids never are. Melee is lethal when there is no
/// attacker, the attacker finishes (`FinishingAttack`), the victim is not humanoid, or either
/// side is hostile (the victim sees the attacker as Hostile/Panic, the attacker the victim as
/// Hostile). Otherwise the victim is only knocked out.
pub fn is_deadly_damage(host: &dyn ScriptHost, ev: &HitEvent, attacker_action: Action) -> bool {
    gothic_portcov::hit!("is_deadly_damage");
    let victim = ev.victim;
    if !can_be_killed(host, victim) {
        return false;
    }
    if is_boss(host, victim) || host.name(victim) == "Xardas" || ev.kind.is_spell() {
        return true;
    }
    if ev.kind == HitKind::Projectile {
        return ev.damage_type != DamageType::Impact || !is_humanoid(host, victim);
    }
    if ev.attacker.is_none()
        || attacker_action == Action::FinishingAttack
        || !is_humanoid(host, victim)
    {
        return true;
    }
    matches!(
        get_attitude(host, victim, ev.attacker),
        Attitude::Hostile | Attitude::Panic
    ) || get_attitude(host, ev.attacker, victim) == Attitude::Hostile
}

/// Port of `CanBurn` (S:2d530): fire damage ignites unless the victim is a Golem, Demon, Troll,
/// FireGolem, IceGolem or Dragon or (the player) has `Perk_ResistHeat`.
pub fn can_burn(host: &dyn ScriptHost, victim: EntityId) -> bool {
    gothic_portcov::hit!("can_burn");
    !matches!(
        host.species(victim),
        Species::Golem
            | Species::Demon
            | Species::Troll
            | Species::FireGolem
            | Species::IceGolem
            | Species::Dragon
    ) && !host.is_skill_active(victim, "Perk_ResistHeat")
}

/// Port of `CanFreeze` (S:2d7b0): same exclusions as [`can_burn`] with `Perk_ResistCold`.
pub fn can_freeze(host: &dyn ScriptHost, victim: EntityId) -> bool {
    gothic_portcov::hit!("can_freeze");
    !matches!(
        host.species(victim),
        Species::Golem
            | Species::Demon
            | Species::Troll
            | Species::FireGolem
            | Species::IceGolem
            | Species::Dragon
    ) && !host.is_skill_active(victim, "Perk_ResistCold")
}

/// Port of `CanBePoisoned` (S:2d9b0 family): not for evil victims or with `Perk_ImmuneToPoison`.
pub fn can_be_poisoned(host: &dyn ScriptHost, victim: EntityId) -> bool {
    gothic_portcov::hit!("can_be_poisoned");
    !is_evil(host, victim) && !host.is_skill_active(victim, "Perk_ImmuneToPoison")
}

/// Port of `CanBeDiseased`: not with `Perk_ImmuneToDisease`.
pub fn can_be_diseased(host: &dyn ScriptHost, victim: EntityId) -> bool {
    gothic_portcov::hit!("can_be_diseased");
    !host.is_skill_active(victim, "Perk_ImmuneToDisease")
}

/// Status bit: burning.
pub const STATUS_BURNING: u32 = 1;
/// Status bit: frozen.
pub const STATUS_FROZEN: u32 = 2;
/// Status bit: poisoned.
pub const STATUS_POISONED: u32 = 4;
/// Status bit: diseased.
pub const STATUS_DISEASED: u32 = 8;

/// Bonus that mana paid for a spell buys: `ManaUsed == 0` doubles the base, otherwise
/// `base * INT / 100`.
fn spell_attribute_bonus(host: &dyn ScriptHost, attacker: EntityId, ev: &HitEvent, b: i32) -> i32 {
    if ev.mana_used == 0 {
        2 * b
    } else {
        b * stats::attribute(host, attacker, Attribute::Intelligence) / 100
    }
}

/// Raw and attacker-dependent damage before protection (COMBAT_AI_RULES 2.5).
fn raw_damage(host: &dyn ScriptHost, ev: &HitEvent, strength_class: i32) -> i32 {
    let _ = strength_class;
    let mut b = (ev.amount as f32 * ev.hit_mult) as i32;
    if weakness(host, ev.victim, ev) == Weakness::Weak {
        b *= 2;
    }
    if ev.head_shot {
        b *= 2;
    }
    let b0 = b;
    let attacker = ev.attacker;
    let player = host.player();
    let setup = host.setup();
    let mut dmg;
    if attacker == player && !host.is_transformed(attacker) {
        if ev.kind == HitKind::SpellProjectile {
            b = 2 * b.min(100);
        }
        let e = if ev.kind.is_spell() {
            spell_attribute_bonus(host, attacker, ev, b)
        } else if ev.kind == HitKind::Projectile {
            stats::attribute(host, attacker, Attribute::Dexterity) / 2
        } else {
            stats::attribute(host, attacker, Attribute::Strength) / 2
        }
        .max(0);
        b += quality_bonus(ev.weapon_quality, b);
        b += skill_bonus(host, attacker, ev.victim, ev, b);
        dmg = b + e;
    } else if attacker.is_none() || !host.exists(attacker) {
        dmg = b;
    } else {
        let strength = stats::attribute(host, attacker, Attribute::Strength);
        let humanoid = is_humanoid(host, attacker);
        let level = host.level_raw(attacker);
        dmg = if level < 2 || !setup.extended_content || !humanoid || ev.kind != HitKind::Melee {
            strength
        } else if ev.victim != player {
            if ev.weapon_name == "Fist" {
                strength / 2
            } else {
                strength
            }
        } else if ev.weapon_name == "Fist" {
            (strength as f32 * 0.75) as i32
        } else if host.species(attacker) == Species::Orc {
            (strength as f32 * 0.75 + b0 as f32 / 7.0) as i32
        } else {
            (strength as f32 * 0.9 + b0 as f32 / 9.0) as i32
        };
        if is_humanoid(host, ev.victim) && !humanoid && ev.victim != player {
            let summoned = host.party_member_type(attacker) == PartyMemberType::Summoned;
            dmg = if summoned { dmg / 2 } else { dmg / 10 };
        }
        if ev.kind.is_spell() {
            dmg *= 2;
        }
    }
    if ev.kind == HitKind::SpellProjectile && ev.path_missile {
        dmg *= 2;
    }
    dmg
}

/// Apply protection (COMBAT_AI_RULES 2.7): percent mode for NPC victims and projectiles/spells,
/// flat mode (`dmg - Prot`, with a chip of 1-2 percent when the hit is strong) for the player
/// hit by melee under `ExtendedContent`.
fn apply_protection(
    host: &mut dyn ScriptHost,
    ev: &HitEvent,
    dmg: i32,
    attacker_action: Action,
) -> i32 {
    let prot = armor_protection(host, ev.victim, ev.damage_type);
    let flat = ev.victim == host.player()
        && host.setup().extended_content
        && !host.is_transformed(ev.victim)
        && !ev.kind.is_spell()
        && ev.kind != HitKind::Projectile;
    if !flat {
        return dmg - prot * dmg / 100;
    }
    let mut d = dmg - prot;
    if d < 1
        && (dmg > 39
            || matches!(
                attacker_action,
                Action::HackAttack | Action::PowerAttack | Action::SprintAttack
            ))
    {
        d = (host.random(2) + 1) * dmg / 100;
    }
    d
}

/// Attack-move multipliers (COMBAT_AI_RULES 2.8).
fn apply_move(host: &dyn ScriptHost, ev: &HitEvent, dmg: i32, act: Action) -> i32 {
    let d = match act {
        Action::PowerAttack | Action::SprintAttack => {
            if dual_wield(host, ev.attacker) || host.state_position(ev.attacker) == 2 {
                dmg * 2
            } else {
                dmg
            }
        }
        Action::QuickAttack | Action::QuickAttackR | Action::QuickAttackL => dmg / 2,
        Action::HackAttack => dmg * 2,
        _ => dmg,
    };
    d.max(0)
}

/// Port of `CanParadeMoveOf` (S:d640): can the victim block this *kind* of move?
///
/// Two fighters need matching fist modes; a fist-fighting victim cannot parry weapons. Heavy
/// moves (Pierce, Hack, and with `AIMode` also Power) can only be parried with sword and shield
/// in hand.
pub fn can_parade_move_of(host: &dyn ScriptHost, victim: EntityId, attacker: EntityId) -> bool {
    gothic_portcov::hit!("can_parade_move_of");
    let attacker_fist = is_in_fist_mode(host, attacker);
    let victim_fist = is_in_fist_mode(host, victim);
    if attacker_fist {
        return is_humanoid(host, attacker) && victim_fist && is_humanoid(host, victim);
    }
    if victim_fist {
        return false;
    }
    let shield_pair = host.slot_use_type(victim, Slot::LeftHand) == UseType::Shield
        && host.slot_use_type(victim, Slot::RightHand) == UseType::OneH;
    match host.current_action(attacker) {
        Action::PowerAttack => !host.setup().ai_mode || shield_pair,
        Action::PierceAttack | Action::HackAttack => shield_pair,
        _ => true,
    }
}

/// Port of `IsInParadePose` (S:d980): is the victim's body pose compatible with its hands?
/// Sword and shield need primary pose 2, other fighting pairs pose 1 or 2, everything else pose
/// 1 [U: the hand pairs of the second and third table rows are assumed, see PORT_NOTES].
pub fn is_in_parade_pose(host: &dyn ScriptHost, e: EntityId) -> bool {
    gothic_portcov::hit!("is_in_parade_pose");
    let left = host.slot_use_type(e, Slot::LeftHand);
    let right = host.slot_use_type(e, Slot::RightHand);
    let pose = host.primary_pose(e);
    if left == UseType::Shield && right == UseType::OneH {
        pose == 2
    } else if matches!(
        (left, right),
        (UseType::OneH, UseType::OneH) | (UseType::Torch, UseType::OneH)
    ) {
        pose == 1 || pose == 2
    } else {
        pose == 1
    }
}

/// Port of `IsInParadeMode` (S:d870): in the `Parade` pose and [`is_in_parade_pose`].
pub fn is_in_parade_mode(host: &dyn ScriptHost, e: EntityId) -> bool {
    gothic_portcov::hit!("is_in_parade_mode");
    host.ani_state(e) == AniState::Parade && is_in_parade_pose(host, e)
}

/// Port of `CanParade` (S:d480): can the victim block this attacker's melee hit?
///
/// [`can_parade_move_of`], parade mode, the victim not in a `HackAttack` itself, with `AIMode`
/// the attacker not in a Pierce/Whirl/Finishing/Sprint move, and the attacker in the victim's
/// field of view.
pub fn can_parade(host: &dyn ScriptHost, victim: EntityId, attacker: EntityId) -> bool {
    gothic_portcov::hit!("can_parade");
    if !can_parade_move_of(host, victim, attacker) || !is_in_parade_mode(host, victim) {
        return false;
    }
    if host.current_action(victim) == Action::HackAttack {
        return false;
    }
    if host.setup().ai_mode
        && matches!(
            host.current_action(attacker),
            Action::PierceAttack
                | Action::WhirlAttack
                | Action::FinishingAttack
                | Action::SprintAttack
        )
    {
        return false;
    }
    host.is_in_fov(victim, attacker)
}

/// Port of `HasStaffMagicParade` (S:dc20): a staff in the right hand and (the player) the
/// `Perk_StaffMagic` skill or (an NPC) the Mage class.
pub fn has_staff_magic_parade(host: &dyn ScriptHost, e: EntityId) -> bool {
    gothic_portcov::hit!("has_staff_magic_parade");
    if host.slot_use_type(e, Slot::RightHand) != UseType::Staff {
        return false;
    }
    if e == host.player() {
        host.is_skill_active(e, "Perk_StaffMagic")
    } else {
        host.npc_class(e) == crate::enums::NpcClass::Mage
    }
}

/// Port of `CanParadeMagic` (S:dd70): staff magic parry, parade mode, attacker in view.
pub fn can_parade_magic(host: &dyn ScriptHost, victim: EntityId, attacker: EntityId) -> bool {
    gothic_portcov::hit!("can_parade_magic");
    has_staff_magic_parade(host, victim)
        && is_in_parade_mode(host, victim)
        && host.is_in_fov(victim, attacker)
}

/// Port of `CanParadeMissile` (S:de80): sword and shield in hand, parade mode, attacker in view.
pub fn can_parade_missile(host: &dyn ScriptHost, victim: EntityId, attacker: EntityId) -> bool {
    gothic_portcov::hit!("can_parade_missile");
    host.slot_use_type(victim, Slot::LeftHand) == UseType::Shield
        && host.slot_use_type(victim, Slot::RightHand) == UseType::OneH
        && is_in_parade_mode(host, victim)
        && host.is_in_fov(victim, attacker)
}

/// Stagger result of a parried hit by strength class `v`.
fn parry_stagger(v: i32) -> Action {
    match v {
        i32::MIN..=1 => Action::QuickParadeStumble,
        2 => Action::ParadeStumble,
        _ => Action::HeavyParadeStumble,
    }
}

fn parry_task(action: Action) -> State {
    if action == Action::HeavyParadeStumble {
        State::HeavyParadeStumble
    } else {
        State::ParadeStumble
    }
}

/// Port of `AssessHit` (S:2b580): turn a hit into damage and a reaction.
///
/// Order: bookkeeping (last/current attacker, Sprint becomes Power); the player's non-lethal
/// impact projectile and backstab special cases; strength class; raw damage and attacker bonuses;
/// protection; move multipliers; the three parries (magic, missile, melee); immortality cap;
/// arena and hard-mode adjustments; and finally death, knock-out or stagger. The reaction state
/// is set on the victim and the `gEAction` is returned (`Action::None` for "no reaction").
pub fn assess_hit(host: &mut dyn ScriptHost, ev: &HitEvent) -> HitOutcome {
    gothic_portcov::hit!("assess_hit");
    let victim = ev.victim;
    let attacker = ev.attacker;
    let player = host.player();
    host.set_damage_amount(victim, 0);
    let previous = host.current_attacker(victim);
    host.set_last_attacker(victim, previous);
    host.set_current_attacker(victim, attacker);
    let mut act = if attacker.is_some() {
        host.current_action(attacker)
    } else {
        Action::None
    };
    if matches!(act, Action::SprintAttack | Action::PowerAttack) {
        if victim == player {
            host.set_last_hit_timestamp(victim, 0);
        }
        host.set_current_action(attacker, Action::PowerAttack);
        act = Action::PowerAttack;
    }
    let hp = stats::pool(host, victim, Pool::Hit);
    // Player-only special cases.
    if attacker == player {
        if ev.kind == HitKind::Projectile
            && is_humanoid(host, victim)
            && ev.damage_type == DamageType::Impact
        {
            let d = (ev.amount as f32 * ev.hit_mult) as i32;
            let d = d + skill_bonus(host, attacker, victim, ev, d);
            if hp - d <= 0 {
                if d > 0 {
                    host.set_damage_amount(victim, d);
                }
                host.full_stop(victim);
                host.set_task(victim, State::Unconscious);
                return HitOutcome {
                    action: Action::LieKnockOut,
                    damage: d.max(0),
                };
            }
            if ev.hit_mult >= 0.7 {
                if d > 0 {
                    host.set_damage_amount(victim, d);
                }
                host.set_task(victim, State::SitKnockDown);
                return HitOutcome {
                    action: Action::SitKnockDown,
                    damage: d.max(0),
                };
            }
        }
        if host.ai_mode(victim) == AiMode::Routine && !host.is_in_fov(victim, attacker) {
            if act == Action::PierceAttack && host.is_skill_active(attacker, "Perk_Assassin") {
                stats::set_pool(host, victim, Pool::Hit, 0);
                host.full_stop(victim);
                if can_be_killed(host, victim) {
                    if is_humanoid(host, victim) {
                        host.set_task(victim, State::PiercedDead);
                        return HitOutcome {
                            action: Action::LiePiercedDead,
                            damage: hp,
                        };
                    }
                    host.set_task(victim, State::RagDollDead);
                    return HitOutcome {
                        action: Action::LieDead,
                        damage: hp,
                    };
                }
                host.set_task(victim, State::LieKnockDown);
                return HitOutcome {
                    action: Action::LieKnockDown,
                    damage: hp,
                };
            }
            if matches!(act, Action::PowerAttack | Action::SprintAttack)
                && ev.damage_type == DamageType::Impact
                && host.is_skill_active(attacker, "Perk_KnockOut")
            {
                host.full_stop(victim);
                host.set_task(victim, State::LieKnockDown);
                return HitOutcome {
                    action: Action::LieKnockDown,
                    damage: 0,
                };
            }
        }
    }
    let v = hit_strength(host, ev, act);
    let raw = raw_damage(host, ev, v);
    let mut dmg = apply_protection(host, ev, raw, act);
    dmg = apply_move(host, ev, dmg, act);

    // Parries.
    if attacker.is_some() {
        if ev.kind.is_spell() && can_parade_magic(host, victim, attacker) {
            let mut cost = dmg;
            if host.setup().extended_content {
                let int = stats::attribute(host, victim, Attribute::Intelligence) as f32;
                let f = (1.0 - (int - 200.0) / 500.0).clamp(0.0, 1.0);
                cost = (dmg as f32 * f) as i32;
            }
            stats::add_mana_points(host, victim, -cost);
            let mana = stats::pool(host, victim, Pool::Mana);
            let short = (mana - cost).min(0);
            let _ = short;
            let left = stats::pool(host, victim, Pool::Mana);
            let shortfall = (left).min(0);
            if shortfall < 0 {
                stats::add_hit_points(host, victim, shortfall);
            }
            if stats::pool(host, victim, Pool::Hit) > 0 {
                host.set_task(victim, State::ParadeStumble);
                return HitOutcome {
                    action: Action::MagicParade,
                    damage: dmg,
                };
            }
        } else if ev.kind == HitKind::Projectile && can_parade_missile(host, victim, attacker) {
            let mut cost = dmg;
            if victim == player && host.is_skill_active(victim, "Perk_Shield_2") {
                cost /= 2;
            }
            let before = stats::pool(host, victim, Pool::Stamina);
            stats::add_stamina_points(host, victim, -cost);
            let short = (before - cost).min(0);
            if short < 0 {
                stats::add_hit_points(host, victim, short);
            }
            if stats::pool(host, victim, Pool::Hit) > 0 {
                let action = if v > 0 {
                    Action::ParadeStumble
                } else {
                    parry_stagger(v)
                };
                host.set_task(victim, parry_task(action));
                return HitOutcome {
                    action,
                    damage: dmg,
                };
            }
        } else if ev.kind == HitKind::Melee && can_parade(host, victim, attacker) {
            let mut c = dmg / 2;
            if host.slot_use_type(victim, Slot::LeftHand) == UseType::Shield {
                c -= c / 4;
            }
            if victim == player && host.is_skill_active(victim, "Perk_Shield_2") {
                c /= 2;
            }
            let before = stats::pool(host, victim, Pool::Stamina);
            stats::add_stamina_points(host, victim, -c);
            if !host.setup().ai_mode {
                let short = (before - c).min(0);
                if short < 0 {
                    stats::add_hit_points(host, victim, short);
                }
            }
            if stats::pool(host, victim, Pool::Hit) > 0 {
                let action = parry_stagger(v);
                host.set_task(victim, parry_task(action));
                return HitOutcome {
                    action,
                    damage: dmg,
                };
            }
        }
    }
    land_hit(host, ev, dmg, v, act)
}

/// COMBAT_AI_RULES 2.10: the hit lands.
fn land_hit(
    host: &mut dyn ScriptHost,
    ev: &HitEvent,
    mut dmg: i32,
    v: i32,
    act: Action,
) -> HitOutcome {
    let victim = ev.victim;
    let player = host.player();
    let hp = stats::pool(host, victim, Pool::Hit);
    if victim != player && !can_be_killed(host, victim) {
        dmg = dmg.min(30);
        if dmg > hp {
            dmg = 0;
        }
    }
    if host.setup().extended_content && victim != player {
        if host.attack_reason(victim) == crate::enums::AttackReason::Arena {
            dmg /= 2;
        }
        if ev.kind == HitKind::Projectile && host.difficulty() == crate::config::Difficulty::Hard {
            dmg = (dmg as f32 * 1.2) as i32;
        }
    }
    if dmg > 0 {
        host.set_damage_amount(victim, dmg);
    }
    if ev.damage_type == DamageType::Fire
        && can_burn(host, victim)
        && dmg > 0
        && burn_applies(host, ev, hp, dmg)
    {
        host.set_status_bits(victim, STATUS_BURNING, true);
    }
    if hp - dmg <= 0 {
        host.full_stop(victim);
        return if is_deadly_damage(host, ev, act) {
            host.set_task(victim, State::RagDollDead);
            HitOutcome {
                action: Action::LieDead,
                damage: dmg,
            }
        } else if act == Action::PierceAttack && is_humanoid(host, victim) {
            host.set_task(victim, State::PiercedKO);
            HitOutcome {
                action: Action::LiePiercedKO,
                damage: dmg,
            }
        } else if v >= 3 {
            host.set_task(victim, State::Unconscious);
            HitOutcome {
                action: Action::LieKnockOut,
                damage: dmg,
            }
        } else {
            host.set_task(victim, State::LieKnockDown);
            HitOutcome {
                action: Action::LieKnockDown,
                damage: dmg,
            }
        };
    }
    if host.current_action(victim) == Action::SprintAttack {
        return HitOutcome {
            action: Action::None,
            damage: dmg,
        };
    }
    if v <= 0 {
        pipi_stumble(host, victim);
        return HitOutcome {
            action: Action::None,
            damage: dmg,
        };
    }
    if matches!(act, Action::PierceAttack | Action::HackAttack)
        && can_be_poisoned(host, victim)
        && ev.weapon_quality & 4 != 0
    {
        host.set_status_bits(victim, STATUS_POISONED, true);
    }
    if ev.weapon_quality & 2 != 0 && can_be_diseased(host, victim) {
        host.set_status_bits(victim, STATUS_DISEASED, true);
    }
    let humanoid = is_humanoid(host, victim);
    let melee_ready = weapon_mode(
        host.slot_use_type(victim, Slot::LeftHand),
        host.slot_use_type(victim, Slot::RightHand),
    ) == WeaponMode::Melee;
    let reaction = if act == Action::PierceAttack && humanoid {
        (State::PierceStumble, Action::PierceStumble)
    } else if ev.damage_type == DamageType::Ice && can_freeze(host, victim) {
        host.set_status_bits(victim, STATUS_FROZEN, true);
        host.set_task(victim, State::Freeze);
        host.set_task_position(victim, (12 * (dmg / 20).max(5)) as u32);
        return HitOutcome {
            action: Action::None,
            damage: dmg,
        };
    } else if weakness(host, victim, ev) == Weakness::Weak || !humanoid || !melee_ready && !humanoid
    {
        (State::Stumble, Action::Stumble)
    } else if humanoid && v >= 4 && melee_ready {
        (State::SitKnockDown, Action::SitKnockDown)
    } else if v >= 2 {
        (State::Stumble, Action::Stumble)
    } else {
        (State::QuickStumble, Action::QuickStumble)
    };
    host.set_task(victim, reaction.0);
    HitOutcome {
        action: reaction.1,
        damage: dmg,
    }
}

/// Burning rules (COMBAT_AI_RULES 9.7): a Missile-path fire projectile, fire-quality ammo (`mult
/// >= 0.5`) or a fire weapon (quality bit 8) when the hit leaves under 100 hit points.
fn burn_applies(host: &dyn ScriptHost, ev: &HitEvent, hp: i32, dmg: i32) -> bool {
    ev.path_missile
        || (ev.kind == HitKind::Projectile && ev.hit_mult >= 0.5)
        || (ev.weapon_quality & 8 != 0 && hp - dmg < 100)
        || ev.kind.is_spell() && host.species(ev.victim) != Species::Golem
}

/// Port of `PipiStumble` (S:22f70): the minimal hit reaction. A victim outside combat stops what
/// it does and forgets its move; the effect/animation part is host work.
pub fn pipi_stumble(host: &mut dyn ScriptHost, victim: EntityId) {
    gothic_portcov::hit!("pipi_stumble");
    if host.ai_mode(victim) != AiMode::Combat {
        host.full_stop(victim);
        host.set_current_action(victim, Action::None);
    }
}

/// Port of `DoLogicalDamage` (S:b4f50): the gate in front of [`assess_hit`].
///
/// Returns `None` when the hit is ignored. Gate order: weakness (immune), the invulnerability cheat for the
/// player, status 0x80 on either side, victim AI mode Arena/Dead/Down/Talk, the player victim
/// with a GUI page open, swimming victims, knocked-down victims, Pierce/Hack against a
/// non-target, an NPC attacker's accidental hit on a Friendly or Neutral non-target, the
/// `Spl_DestroyEvil` spell against non-evil victims, status 0x40 beyond 700, and spell or
/// projectile damage of the player against friends. Otherwise [`assess_hit`] runs and the
/// perception broadcast of its result is returned with it.
pub fn do_logical_damage(
    host: &mut dyn ScriptHost,
    ev: &HitEvent,
) -> Option<(HitOutcome, Broadcast)> {
    gothic_portcov::hit!("do_logical_damage");
    let victim = ev.victim;
    let attacker = ev.attacker;
    let player = host.player();
    if weakness(host, victim, ev) == Weakness::Immune {
        return None;
    }
    if victim == player && host.cheat_god() {
        return None;
    }
    if host.has_status(victim, 0x80) || (attacker.is_some() && host.has_status(attacker, 0x80)) {
        return None;
    }
    if matches!(
        host.ai_mode(victim),
        AiMode::Arena | AiMode::Dead | AiMode::Down | AiMode::Talk
    ) {
        return None;
    }
    if (victim == player && host.gui_page_open()) || host.is_swimming(victim) {
        return None;
    }
    if matches!(
        host.ani_state(victim),
        AniState::SitKnockDown | AniState::LieKnockDown
    ) {
        return None;
    }
    let act = if attacker.is_some() {
        host.current_action(attacker)
    } else {
        Action::None
    };
    let victim_is_target = attacker.is_some() && host.current_target(attacker) == victim;
    if attacker.is_some() {
        if matches!(act, Action::PierceAttack | Action::HackAttack) && !victim_is_target {
            return None;
        }
        if attacker != player
            && host.npc_type(attacker) != NpcType::Friend
            && !victim_is_target
            && matches!(
                get_attitude(host, attacker, victim),
                Attitude::Friendly | Attitude::Neutral
            )
        {
            return None;
        }
    }
    if ev.entity_name == "Spl_DestroyEvil" && !is_evil(host, victim) {
        return None;
    }
    if host.has_status(victim, 0x40)
        && attacker.is_some()
        && host.distance(victim, attacker) > 700.0
    {
        return None;
    }
    if attacker == player && (ev.kind.is_spell() || ev.kind == HitKind::Projectile) {
        let friendly = get_attitude(host, victim, player) == Attitude::Friendly;
        if friendly {
            return None;
        }
    }
    let outcome = assess_hit(host, ev);
    let broadcast = match outcome.action {
        Action::LieKnockDown | Action::LieKnockOut | Action::LiePiercedKO => Broadcast::Defeat,
        Action::Die | Action::LieDead | Action::LiePiercedDead => Broadcast::Murder,
        _ => Broadcast::FightSound,
    };
    Some((outcome, broadcast))
}

/// The perception broadcast that follows a hit (`SendPerception*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Broadcast {
    /// Non-lethal defeat.
    Defeat,
    /// Killed.
    Murder,
    /// Ordinary hit.
    FightSound,
}

/// `DoLogicalDamage`'s return value: 0 for the parry results (QuickParadeStumble, ParadeStumble,
/// HeavyParadeStumble, MagicParade), 1 for everything else.
pub fn logical_damage_result(action: Action) -> u32 {
    u32::from(!matches!(
        action,
        Action::QuickParadeStumble
            | Action::ParadeStumble
            | Action::HeavyParadeStumble
            | Action::MagicParade
    ))
}

/// Cumulative experience needed to reach level `level + 1` (`GetXPNext`, S:4e800):
/// `250 * (L + 1) * (L + 2)`.
pub fn xp_next(level: u32) -> i64 {
    250 * i64::from(level + 1) * i64::from(level + 2)
}

/// Port of `GiveXP` (S:4e160): experience for `receiver`.
///
/// `amount == -1` means "a kill or defeat of `source`": ambient creatures give 10, Pig and Cow 20,
/// everything else `max(10 * level, 50)` (without `ExtendedContent`: ambient `max(10 L, 25)`, others
/// `max(10 L, 50)`), scaled by `XPModifier`: `5 * floor(mod * base / 500 + 0.5)`. A source that is
/// the world entity (or none) pays `5 * amount`; any other explicit amount is paid as is.
/// Nothing is paid when the source is the player, for summoned receivers, or to a receiver that
/// is neither the player nor in the player's party. Levels are gained while the XP total reaches
/// [`xp_next`]; each level gives 10 learning points (11 with `Perk_Learn`). Returns the XP paid.
pub fn give_xp(
    host: &mut dyn ScriptHost,
    source: EntityId,
    receiver: EntityId,
    amount: i32,
) -> i64 {
    gothic_portcov::hit!("give_xp");
    let player = host.player();
    if source == player {
        return 0;
    }
    if receiver != player && host.party_leader(receiver) != player {
        return 0;
    }
    if host.party_member_type(receiver) == PartyMemberType::Summoned {
        return 0;
    }
    let xp: i64 = if source.is_none() || !host.exists(source) {
        5 * i64::from(amount)
    } else if amount == -1 {
        let level = stats::level(host, source) as i64;
        let ext = host.setup().extended_content;
        let base = if ext {
            if is_ambient_creature(host, source) {
                10
            } else if matches!(host.species(source), Species::Pig | Species::Cow) {
                20
            } else {
                (10 * level).max(50)
            }
        } else if is_ambient_creature(host, source) {
            (10 * level).max(25)
        } else {
            (10 * level).max(50)
        };
        let modifier = i64::from(host.setup().xp_modifier);
        5 * ((modifier * base) as f64 / 500.0 + 0.5).floor() as i64
    } else {
        i64::from(amount)
    };
    let total = host.player_xp() + xp;
    host.set_player_xp(total);
    let mut level = host.player_level();
    while total >= xp_next(level) {
        level += 1;
        let learn = if host.is_skill_active(player, "Perk_Learn") {
            11
        } else {
            10
        };
        host.add_learning_points(learn);
        host.set_player_level(level);
    }
    xp
}

/// Queue the voice-less combat reaction sound hook: used by hosts that animate hits.
pub fn hit_reaction_command(action: Action) -> Option<AiCommand> {
    match action {
        Action::None => None,
        a => Some(AiCommand::AniStateAction(a)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::Alignment;
    #[allow(unused_imports)]
    use crate::host::{EntityApi, InventoryApi, NpcApi, ScriptCtx, StatsApi, WorldApi};
    use crate::testhost::TestWorld;

    fn duel() -> (TestWorld, EntityId, EntityId) {
        let mut w = TestWorld::new();
        let p = w.player();
        w.pm.attributes.insert(Attribute::Strength, 100);
        w.pm.attributes.insert(Attribute::Dexterity, 100);
        w.pm.attributes.insert(Attribute::Intelligence, 100);
        let npc = w.spawn_npc("Orc", Species::Orc, 10, 10);
        w.set_alignment(npc, Alignment::Orc);
        w.set_alignment(p, Alignment::Nrd);
        (w, p, npc)
    }

    #[test]
    fn weakness_table() {
        let mut w = TestWorld::new();
        let mk = |w: &mut TestWorld, s| w.spawn_npc("x", s, 1, 1);
        let skel = mk(&mut w, Species::Skeleton);
        let golem = mk(&mut w, Species::Golem);
        let ice = mk(&mut w, Species::IceGolem);
        let fire = mk(&mut w, Species::FireGolem);
        let man = mk(&mut w, Species::Human);
        let ev = |ty: DamageType, name: &str| {
            let mut e = HitEvent::melee(EntityId::NONE, EntityId::NONE, ty, 10);
            e.entity_name = name.into();
            e
        };
        let check = |w: &TestWorld, who, ty, name: &str| {
            let mut e = ev(ty, name);
            e.victim = who;
            weakness(w, who, &e)
        };
        assert_eq!(check(&w, skel, DamageType::Impact, ""), Weakness::Weak);
        assert_eq!(check(&w, golem, DamageType::Impact, ""), Weakness::Weak);
        assert_eq!(check(&w, man, DamageType::Impact, ""), Weakness::Normal);
        assert_eq!(check(&w, golem, DamageType::Missile, ""), Weakness::Immune);
        assert_eq!(check(&w, skel, DamageType::Missile, ""), Weakness::Immune);
        assert_eq!(
            check(&w, skel, DamageType::Missile, "It_Arrow_Fire"),
            Weakness::Normal
        );
        assert_eq!(
            check(&w, ice, DamageType::Missile, "Explosive"),
            Weakness::Normal
        );
        assert_eq!(check(&w, fire, DamageType::Fire, ""), Weakness::Immune);
        assert_eq!(check(&w, ice, DamageType::Fire, ""), Weakness::Weak);
        assert_eq!(check(&w, ice, DamageType::Ice, ""), Weakness::Immune);
        assert_eq!(check(&w, fire, DamageType::Ice, ""), Weakness::Weak);
        assert_eq!(check(&w, golem, DamageType::Lightning, ""), Weakness::Weak);
        assert_eq!(check(&w, man, DamageType::None, ""), Weakness::Immune);
        w.set_status(man, 2);
        assert_eq!(
            check(&w, man, DamageType::Ice, ""),
            Weakness::Immune,
            "frozen victims"
        );
    }

    #[test]
    fn quality_bonus_flags() {
        assert_eq!(quality_bonus(0, 100), 0);
        assert_eq!(quality_bonus(0x100, 100), -50);
        assert_eq!(quality_bonus(0x20 | 0x40 | 0x80, 100), 30);
        assert_eq!(quality_bonus(0x100 | 0x20, 40), -10);
    }

    #[test]
    fn hit_strength_classes() {
        let (mut w, p, npc) = duel();
        let mut ev = HitEvent::melee(npc, p, DamageType::Blade, 10);
        // Spells and projectiles have fixed classes.
        ev.kind = HitKind::SpellDirect;
        assert_eq!(hit_strength(&w, &ev, Action::None), 2);
        ev.kind = HitKind::Projectile;
        ev.hit_mult = 0.6;
        assert_eq!(hit_strength(&w, &ev, Action::None), 2);
        ev.hit_mult = 0.59;
        assert_eq!(hit_strength(&w, &ev, Action::None), 0);
        // Melee: move + rank - defence, at least 1.
        ev.kind = HitKind::Melee;
        assert_eq!(hit_strength(&w, &ev, Action::HackAttack), 4);
        assert_eq!(hit_strength(&w, &ev, Action::QuickAttack), 1);
        // Orc level 10: rank 0; level 30 victim with a shield: rank 2 + 1.
        let tank = w.spawn_npc("Tank", Species::Human, 30, 30);
        w.set_slot(tank, Slot::LeftHand, UseType::Shield, 1);
        let mut ev2 = HitEvent::melee(tank, p, DamageType::Blade, 10);
        ev2.kind = HitKind::Melee;
        assert_eq!(
            hit_strength(&w, &ev2, Action::PowerAttack),
            1,
            "4 - 3 -> clamped to 1"
        );
        // Dual wielding weakens the power attack.
        w.set_slot(p, Slot::LeftHand, UseType::OneH, 1);
        w.set_slot(p, Slot::RightHand, UseType::OneH, 1);
        assert_eq!(hit_strength(&w, &ev, Action::PowerAttack), 2);
        // Without an attacker nothing is strong.
        ev.attacker = EntityId::NONE;
        assert_eq!(hit_strength(&w, &ev, Action::None), 0);
    }

    #[test]
    fn power_ranks() {
        let (mut w, p, npc) = duel();
        assert_eq!(power_rank(&w, npc), 0);
        for (lvl, rank) in [(14, 0), (15, 1), (29, 1), (30, 2), (45, 3)] {
            let n = w.spawn_npc("n", Species::Human, lvl, lvl);
            assert_eq!(power_rank(&w, n), rank, "level {lvl}");
        }
        w.set_slot(p, Slot::RightHand, UseType::OneH, 1);
        assert_eq!(power_rank(&w, p), 0);
        w.grant_skill(p, "Perk_1H_2");
        assert_eq!(power_rank(&w, p), 1);
        w.grant_skill(p, "Perk_1H_3");
        assert_eq!(power_rank(&w, p), 2);
    }

    #[test]
    fn player_melee_damage() {
        let (mut w, p, npc) = duel();
        w.set_slot(p, Slot::RightHand, UseType::OneH, 1);
        w.set_current_action_direct(p, Action::Attack);
        w.set_target(p, npc);
        let ev = HitEvent::melee(npc, p, DamageType::Blade, 40);
        // 40 + STR/2 (50) = 90, minus the Orc's protection min(80, 1.5 * 10) = 15 percent.
        let out = assess_hit(&mut w, &ev);
        assert_eq!(out.damage, 90 - 90 * 15 / 100);
        assert_eq!(w.damage_amount(npc), out.damage);
        assert_eq!(w.current_attacker(npc), p);
        // Quick attacks do half, hack attacks double.
        w.set_current_action_direct(p, Action::QuickAttack);
        assert_eq!(assess_hit(&mut w, &ev).damage, (90 - 90 * 15 / 100) / 2);
        w.set_current_action_direct(p, Action::HackAttack);
        assert_eq!(assess_hit(&mut w, &ev).damage, (90 - 90 * 15 / 100) * 2);
    }

    #[test]
    fn perks_and_quality_add_to_player_damage() {
        let (mut w, p, npc) = duel();
        w.set_slot(p, Slot::RightHand, UseType::OneH, 1);
        w.set_current_action_direct(p, Action::Attack);
        w.grant_skill(p, "Perk_1H_2");
        w.grant_skill(p, "Perk_OrcSlayer");
        let mut ev = HitEvent::melee(npc, p, DamageType::Blade, 40);
        ev.weapon_quality = 0x20;
        // b = 40 + 10 (quality) + 50 (two perks) = 100; + STR/2 = 150.
        let out = assess_hit(&mut w, &ev);
        assert_eq!(out.damage, 150 - 150 * 15 / 100);
    }

    #[test]
    fn npc_attackers_use_strength() {
        let (mut w, p, _) = duel();
        let orc = w.spawn_npc("Grunt", Species::Orc, 20, 20);
        w.set_alignment(orc, Alignment::Orc);
        // STR = 10 + 1.5 * 20 = 40. Against the player: 0.75 * 40 + b0/7 (b0 = 50 / 7 = 7).
        let mut ev = HitEvent::melee(p, orc, DamageType::Blade, 50);
        ev.weapon_name = "ItMw_Axt".into();
        w.set_current_action_direct(orc, Action::Attack);
        w.pm.protection.insert(Protection::Blades, 0);
        let out = assess_hit(&mut w, &ev);
        assert_eq!(out.damage, 37, "flat mode: 30 + 7, no protection");
        // A human uses 0.9 * STR + b0/9.
        let man = w.spawn_npc("Man", Species::Human, 20, 20);
        w.set_current_action_direct(man, Action::Attack);
        let ev = HitEvent::melee(p, man, DamageType::Blade, 45);
        assert_eq!(assess_hit(&mut w, &ev).damage, 36 + 5);
        // Unarmed against the player: 0.75 STR; against an NPC half STR.
        let mut fist = HitEvent::melee(p, man, DamageType::Impact, 5);
        fist.weapon_name = "Fist".into();
        assert_eq!(assess_hit(&mut w, &fist).damage, 30);
        let victim = w.spawn_npc("V", Species::Human, 1, 1);
        let mut fist2 = HitEvent::melee(victim, man, DamageType::Impact, 5);
        fist2.weapon_name = "Fist".into();
        w.set_alignment(victim, Alignment::Nrd);
        // Level 1 victim: protection min(80, 1.5) = 1 percent: 20 - 0.
        assert_eq!(assess_hit(&mut w, &fist2).damage, 20);
    }

    #[test]
    fn flat_protection_and_chip_damage_for_the_player() {
        let (mut w, p, _) = duel();
        let orc = w.spawn_npc("Grunt", Species::Orc, 2, 2);
        w.set_current_action_direct(orc, Action::Attack);
        w.pm.protection.insert(Protection::Blades, 500); // 500 / 5 = 100 -> capped at 80
        let ev = HitEvent::melee(p, orc, DamageType::Blade, 50);
        // STR 13: 0.75 * 13 + 50/7 = 9 + 7 -> 16; flat protection 80 -> below one, no chip (< 40).
        assert_eq!(assess_hit(&mut w, &ev).damage, 0);
        // Heavy blows chip through for 1-2 percent.
        w.set_current_action_direct(orc, Action::HackAttack);
        w.rolls = vec![1];
        let big = HitEvent::melee(p, orc, DamageType::Blade, 50);
        let out = assess_hit(&mut w, &big);
        assert_eq!(out.damage, 0, "a 2 percent chip of 16 rounds to nothing");
    }

    #[test]
    fn head_shots_and_weak_victims_double() {
        let (mut w, p, _) = duel();
        let skel = w.spawn_npc("Skel", Species::Skeleton, 1, 1);
        w.set_slot(p, Slot::RightHand, UseType::OneH, 1);
        w.set_current_action_direct(p, Action::Attack);
        let mut ev = HitEvent::melee(skel, p, DamageType::Impact, 20);
        // 20 * 2 (weak) = 40; + STR/2 = 90; protection 1 percent.
        assert_eq!(assess_hit(&mut w, &ev).damage, 90);
        ev.head_shot = true;
        w.dr_set_pool(skel, Pool::Hit, 400);
        assert_eq!(assess_hit(&mut w, &ev).damage, 130 - 130 / 100);
    }

    #[test]
    fn killing_blow_and_knock_out() {
        let (mut w, p, _) = duel();
        let man = w.spawn_npc("Man", Species::Human, 1, 1);
        w.set_alignment(man, Alignment::Orc);
        w.set_slot(p, Slot::RightHand, UseType::OneH, 1);
        w.set_current_action_direct(p, Action::Attack);
        w.dr_set_pool(man, Pool::Hit, 5);
        let ev = HitEvent::melee(man, p, DamageType::Blade, 100);
        // Hostile victim: lethal.
        let out = assess_hit(&mut w, &ev);
        assert_eq!(out.action, Action::LieDead);
        assert_eq!(w.current_task(man), Some(State::RagDollDead));
        // A friendly victim is only knocked down in a weak hit.
        let friend = w.spawn_npc("Friend", Species::Human, 1, 1);
        w.set_alignment(friend, Alignment::Nrd);
        w.dr_set_pool(friend, Pool::Hit, 5);
        let ev = HitEvent::melee(friend, p, DamageType::Blade, 100);
        w.set_current_action_direct(p, Action::QuickAttack);
        let out = assess_hit(&mut w, &ev);
        assert_eq!(out.action, Action::LieKnockDown, "v < 3 knocks down");
        // A pierce kills nobody friendly: Pierced KO.
        let friend2 = w.spawn_npc("Friend2", Species::Human, 1, 1);
        w.set_alignment(friend2, Alignment::Nrd);
        w.dr_set_pool(friend2, Pool::Hit, 5);
        w.set_current_action_direct(p, Action::PierceAttack);
        let ev = HitEvent::melee(friend2, p, DamageType::Blade, 100);
        assert_eq!(assess_hit(&mut w, &ev).action, Action::LiePiercedKO);
        assert_eq!(w.current_task(friend2), Some(State::PiercedKO));
        // Finishing attacks always kill.
        let friend3 = w.spawn_npc("Friend3", Species::Human, 1, 1);
        w.set_alignment(friend3, Alignment::Nrd);
        w.dr_set_pool(friend3, Pool::Hit, 5);
        w.set_current_action_direct(p, Action::FinishingAttack);
        let ev = HitEvent::melee(friend3, p, DamageType::Blade, 100);
        assert_eq!(assess_hit(&mut w, &ev).action, Action::LieDead);
    }

    #[test]
    fn essential_npcs_survive() {
        let (mut w, p, _) = duel();
        let boss = w.spawn_npc("Essential", Species::Human, 1, 1);
        w.set_immortal(boss, true);
        w.set_slot(p, Slot::RightHand, UseType::OneH, 1);
        w.set_current_action_direct(p, Action::Attack);
        let ev = HitEvent::melee(boss, p, DamageType::Blade, 500);
        w.dr_set_pool(boss, Pool::Hit, 20);
        let out = assess_hit(&mut w, &ev);
        assert_eq!(
            out.damage, 0,
            "capped at 30, then zeroed when still above hit points"
        );
        w.dr_set_pool(boss, Pool::Hit, 200);
        let out = assess_hit(&mut w, &ev);
        assert_eq!(out.damage, 30);
        assert!(!is_deadly_damage(&w, &ev, Action::Attack));
    }

    #[test]
    fn staggers_by_strength() {
        let (mut w, p, _) = duel();
        let man = w.spawn_npc("Man", Species::Human, 1, 1);
        w.set_alignment(man, Alignment::Orc);
        w.dr_set_pool(man, Pool::Hit, 10_000);
        w.dr_set_pool_max(man, Pool::Hit, 10_000);
        w.set_slot(p, Slot::RightHand, UseType::OneH, 1);
        let ev = HitEvent::melee(man, p, DamageType::Blade, 10);
        w.set_current_action_direct(p, Action::QuickAttack);
        assert_eq!(assess_hit(&mut w, &ev).action, Action::QuickStumble);
        w.set_current_action_direct(p, Action::Attack);
        assert_eq!(assess_hit(&mut w, &ev).action, Action::Stumble);
        w.set_current_action_direct(p, Action::PierceAttack);
        assert_eq!(assess_hit(&mut w, &ev).action, Action::PierceStumble);
        // Heavy hit on a humanoid with a melee weapon: knocked down.
        w.set_slot(man, Slot::RightHand, UseType::OneH, 1);
        w.set_current_action_direct(p, Action::HackAttack);
        assert_eq!(assess_hit(&mut w, &ev).action, Action::SitKnockDown);
        // Animals just stumble.
        let wolf = w.spawn_npc("Wolf", Species::Wolf, 1, 1);
        w.dr_set_pool(wolf, Pool::Hit, 10_000);
        let ev = HitEvent::melee(wolf, p, DamageType::Blade, 10);
        assert_eq!(assess_hit(&mut w, &ev).action, Action::Stumble);
    }

    #[test]
    fn ice_freezes_and_fire_burns() {
        let (mut w, p, _) = duel();
        let man = w.spawn_npc("Man", Species::Human, 1, 1);
        w.dr_set_pool(man, Pool::Hit, 10_000);
        let mut ev = HitEvent::melee(man, p, DamageType::Ice, 100);
        ev.kind = HitKind::SpellDirect;
        let out = assess_hit(&mut w, &ev);
        assert_eq!(out.action, Action::None);
        assert_eq!(w.current_task(man), Some(State::Freeze));
        assert!(w.has_status(man, STATUS_FROZEN));
        assert_eq!(w.task_position(man) as i32, 12 * (out.damage / 20).max(5));
        let man2 = w.spawn_npc("Man2", Species::Human, 1, 1);
        w.dr_set_pool(man2, Pool::Hit, 10_000);
        let mut ev = HitEvent::melee(man2, p, DamageType::Fire, 100);
        ev.kind = HitKind::SpellProjectile;
        ev.path_missile = true;
        assert!(assess_hit(&mut w, &ev).damage > 0);
        assert!(w.has_status(man2, STATUS_BURNING));
        let golem = w.spawn_npc("Golem", Species::Golem, 1, 1);
        assert!(!can_burn(&w, golem) && !can_freeze(&w, golem));
        w.grant_skill(man2, "Perk_ResistHeat");
        assert!(!can_burn(&w, man2) && can_freeze(&w, man2));
    }

    #[test]
    fn the_gate() {
        let (mut w, p, npc) = duel();
        w.set_slot(p, Slot::RightHand, UseType::OneH, 1);
        w.set_current_action_direct(p, Action::Attack);
        w.set_target(p, npc);
        let ev = HitEvent::melee(npc, p, DamageType::Blade, 40);
        let (out, bc) = do_logical_damage(&mut w, &ev).expect("passes");
        assert_eq!(bc, Broadcast::FightSound);
        assert!(out.damage > 0);
        assert_eq!(logical_damage_result(out.action), 1);
        // Dead, down, talking victims; ghosts; swimmers; knocked-down victims.
        for mode in [AiMode::Dead, AiMode::Down, AiMode::Talk, AiMode::Arena] {
            w.set_ai_mode(npc, mode);
            assert!(do_logical_damage(&mut w, &ev).is_none(), "{mode:?}");
        }
        w.set_ai_mode(npc, AiMode::Routine);
        w.set_status(npc, 0x80);
        assert!(do_logical_damage(&mut w, &ev).is_none());
        w.set_status(npc, 0);
        w.swimming = true;
        assert!(do_logical_damage(&mut w, &ev).is_none());
        w.swimming = false;
        w.set_ani_state(npc, AniState::LieKnockDown);
        assert!(do_logical_damage(&mut w, &ev).is_none());
        w.set_ani_state(npc, AniState::Stand);
        // Pierce and hack only hurt the target.
        w.set_current_action_direct(p, Action::PierceAttack);
        w.set_target(p, EntityId::NONE);
        assert!(do_logical_damage(&mut w, &ev).is_none());
        w.set_target(p, npc);
        assert!(do_logical_damage(&mut w, &ev).is_some());
        // Invulnerable player.
        w.set_current_action_direct(npc, Action::Attack);
        w.set_target(npc, p);
        let hit_p = HitEvent::melee(p, npc, DamageType::Blade, 10);
        assert!(do_logical_damage(&mut w, &hit_p).is_some());
        w.god = true;
        assert!(do_logical_damage(&mut w, &hit_p).is_none());
        w.god = false;
        w.gui_open = true;
        assert!(do_logical_damage(&mut w, &hit_p).is_none());
        w.gui_open = false;
        // An allied NPC's stray hit on a non-target is ignored.
        let ally = w.spawn_npc("Ally", Species::Human, 5, 5);
        w.set_alignment(ally, Alignment::Orc);
        let stray = HitEvent::melee(ally, npc, DamageType::Blade, 10);
        w.set_target(npc, p);
        assert!(do_logical_damage(&mut w, &stray).is_none());
        // Destroy Evil only hurts evil.
        let mut spell = HitEvent::melee(ally, p, DamageType::Fire, 50);
        spell.kind = HitKind::SpellDirect;
        spell.entity_name = "Spl_DestroyEvil".into();
        assert!(do_logical_damage(&mut w, &spell).is_none());
        // The player's projectiles do not hit friends.
        let friend = w.spawn_npc("Friend", Species::Human, 5, 5);
        w.set_leader(friend, p, PartyMemberType::Party, false);
        let mut arrow = HitEvent::melee(friend, p, DamageType::Missile, 50);
        arrow.kind = HitKind::Projectile;
        assert!(do_logical_damage(&mut w, &arrow).is_none());
    }

    #[test]
    fn broadcast_follows_the_result() {
        let (mut w, p, _) = duel();
        let man = w.spawn_npc("Man", Species::Human, 1, 1);
        w.set_alignment(man, Alignment::Orc);
        w.set_slot(p, Slot::RightHand, UseType::OneH, 1);
        w.set_current_action_direct(p, Action::Attack);
        w.set_target(p, man);
        w.dr_set_pool(man, Pool::Hit, 1);
        let ev = HitEvent::melee(man, p, DamageType::Blade, 100);
        let (out, bc) = do_logical_damage(&mut w, &ev).unwrap();
        assert_eq!(out.action, Action::LieDead);
        assert_eq!(bc, Broadcast::Murder);
        for (a, r) in [
            (Action::QuickParadeStumble, 0),
            (Action::ParadeStumble, 0),
            (Action::HeavyParadeStumble, 0),
            (Action::MagicParade, 0),
            (Action::Stumble, 1),
            (Action::LieDead, 1),
            (Action::None, 1),
        ] {
            assert_eq!(logical_damage_result(a), r, "{a:?}");
        }
    }

    #[test]
    fn melee_parry_costs_stamina_and_staggers() {
        let (mut w, p, orc) = duel();
        // The player parries with sword and shield.
        w.set_slot(p, Slot::LeftHand, UseType::Shield, 1);
        w.set_slot(p, Slot::RightHand, UseType::OneH, 1);
        w.set_ani_state(p, AniState::Parade);
        w.set_primary_pose(p, 2);
        w.pm.pools.insert(Pool::Stamina, (100, 100));
        w.set_current_action_direct(orc, Action::Attack);
        w.set_alignment(orc, Alignment::Orc);
        let ev = HitEvent::melee(p, orc, DamageType::Blade, 40);
        assert!(can_parade(&w, p, orc));
        let hp_before = stats::pool(&w, p, Pool::Hit);
        let out = assess_hit(&mut w, &ev);
        assert_eq!(out.action, Action::ParadeStumble);
        assert_eq!(
            stats::pool(&w, p, Pool::Hit),
            hp_before,
            "AIMode: stamina is the buffer"
        );
        assert!(stats::pool(&w, p, Pool::Stamina) < 100);
        assert_eq!(w.current_task(p), Some(State::ParadeStumble));
        // Heavy attacks cannot be parried with AIMode unless sword and shield: still fine here,
        // but a finishing attack cannot.
        w.set_current_action_direct(orc, Action::FinishingAttack);
        assert!(!can_parade(&w, p, orc));
        // Wrong pose: no parry.
        w.set_current_action_direct(orc, Action::Attack);
        w.set_primary_pose(p, 1);
        assert!(!can_parade(&w, p, orc));
        // Without AIMode the stamina shortfall hurts.
        w.set_primary_pose(p, 2);
        w.setup.ai_mode = false;
        w.pm.pools.insert(Pool::Stamina, (0, 100));
        // Player stamina cannot go negative; the shortfall is computed from the value before.
        let hp = stats::pool(&w, p, Pool::Hit);
        assess_hit(&mut w, &ev);
        assert!(stats::pool(&w, p, Pool::Hit) < hp);
    }

    #[test]
    fn parry_move_rules() {
        let (mut w, p, orc) = duel();
        w.set_slot(p, Slot::RightHand, UseType::OneH, 1);
        w.set_current_action_direct(orc, Action::PierceAttack);
        assert!(!can_parade_move_of(&w, p, orc), "pierce needs a shield");
        w.set_slot(p, Slot::LeftHand, UseType::Shield, 1);
        assert!(can_parade_move_of(&w, p, orc));
        w.set_current_action_direct(orc, Action::PowerAttack);
        assert!(can_parade_move_of(&w, p, orc));
        w.set_slot(p, Slot::LeftHand, UseType::None, 0);
        assert!(
            !can_parade_move_of(&w, p, orc),
            "power attacks need a shield with AIMode"
        );
        w.setup.ai_mode = false;
        assert!(can_parade_move_of(&w, p, orc));
        w.set_current_action_direct(orc, Action::Attack);
        // Fist fighters only parry fists.
        w.set_slot(orc, Slot::RightHand, UseType::Fist, 0);
        assert!(!can_parade_move_of(&w, p, orc));
        w.set_slot(p, Slot::RightHand, UseType::Fist, 0);
        assert!(can_parade_move_of(&w, p, orc));
    }

    #[test]
    fn magic_parry_uses_mana() {
        let (mut w, p, orc) = duel();
        w.set_slot(p, Slot::RightHand, UseType::Staff, 1);
        w.grant_skill(p, "Perk_StaffMagic");
        w.set_ani_state(p, AniState::Parade);
        w.set_primary_pose(p, 1);
        w.pm.pools.insert(Pool::Mana, (100, 100));
        w.pm.attributes.insert(Attribute::Intelligence, 450);
        let mut ev = HitEvent::melee(p, orc, DamageType::Fire, 100);
        ev.kind = HitKind::SpellDirect;
        assert!(can_parade_magic(&w, p, orc));
        let out = assess_hit(&mut w, &ev);
        assert_eq!(out.action, Action::MagicParade);
        // INT 450: f = 1 - 250/500 = 0.5; spell hit is doubled by the NPC branch first.
        assert!(stats::pool(&w, p, Pool::Mana) < 100);
        assert_eq!(logical_damage_result(out.action), 0);
    }

    #[test]
    fn xp_formula_and_levels() {
        let (mut w, p, _) = duel();
        let man = w.spawn_npc("Bandit", Species::Human, 12, 12);
        w.set_player_level(1);
        // 10 * 12 = 120; 100 percent: 5 * floor(120 / 5 + 0.5) = 120.
        assert_eq!(give_xp(&mut w, man, p, -1), 120);
        assert_eq!(w.player_xp(), 120);
        // Ambient creatures give 10, pigs 20, weak humans 50.
        let bug = w.spawn_npc("Bug", Species::Meatbug, 1, 1);
        let pig = w.spawn_npc("Pig", Species::Pig, 1, 1);
        let rat = w.spawn_npc("Rat", Species::Human, 1, 1);
        assert_eq!(give_xp(&mut w, bug, p, -1), 10);
        assert_eq!(give_xp(&mut w, pig, p, -1), 20);
        assert_eq!(give_xp(&mut w, rat, p, -1), 50);
        // Explicit amounts and the world entity.
        assert_eq!(give_xp(&mut w, EntityId::NONE, p, 3), 15);
        assert_eq!(give_xp(&mut w, man, p, 7), 7);
        // The player never pays himself; outsiders and summons get nothing.
        assert_eq!(give_xp(&mut w, p, p, -1), 0);
        let stranger = w.spawn_npc("Stranger", Species::Human, 1, 1);
        assert_eq!(give_xp(&mut w, man, stranger, -1), 0);
        w.set_leader(stranger, p, PartyMemberType::Summoned, false);
        assert_eq!(give_xp(&mut w, man, stranger, -1), 0);
        w.set_leader(stranger, p, PartyMemberType::Party, false);
        assert_eq!(
            give_xp(&mut w, man, stranger, -1),
            120,
            "party members collect for the player"
        );
    }

    #[test]
    fn xp_modifier_and_vanilla_rules() {
        let (mut w, p, _) = duel();
        let man = w.spawn_npc("Bandit", Species::Human, 12, 12);
        w.setup.xp_modifier = 200;
        assert_eq!(give_xp(&mut w, man, p, -1), 240);
        w.setup.xp_modifier = 100;
        w.setup.extended_content = false;
        let bug = w.spawn_npc("Bug", Species::Meatbug, 1, 1);
        assert_eq!(give_xp(&mut w, bug, p, -1), 25);
        let pig = w.spawn_npc("Pig", Species::Pig, 1, 1);
        assert_eq!(give_xp(&mut w, pig, p, -1), 50);
    }

    #[test]
    fn level_ups_grant_learning_points() {
        let (mut w, p, _) = duel();
        w.set_player_level(1);
        assert_eq!(xp_next(1), 250 * 2 * 3);
        assert_eq!(xp_next(0), 500);
        let man = w.spawn_npc("Big", Species::Human, 100, 100);
        // 1000 xp reaches XPNext(1) = 1500? no: 1000 < 1500, level stays.
        give_xp(&mut w, man, p, -1);
        assert_eq!(w.player_level(), 1);
        assert_eq!(w.learning_points, 0);
        // A second kill crosses XPNext(1) = 1500 (and maybe more).
        give_xp(&mut w, man, p, -1);
        assert!(w.player_level() >= 2);
        assert!(w.learning_points >= 10);
        w.learning_points = 0;
        w.set_player_xp(0);
        w.set_player_level(1);
        w.grant_skill(p, "Perk_Learn");
        give_xp(&mut w, EntityId::NONE, p, 300);
        assert_eq!(
            w.player_level(),
            2,
            "1500 xp reaches level 2, not 3 (XPNext(2) = 3000)"
        );
        assert_eq!(w.learning_points, 11);
    }

    #[test]
    fn backstab_and_knock_out_perks() {
        let (mut w, p, _) = duel();
        let guard = w.spawn_npc("Guard", Species::Human, 5, 5);
        w.set_alignment(guard, Alignment::Orc);
        w.fov = false;
        w.set_slot(p, Slot::RightHand, UseType::OneH, 1);
        w.set_current_action_direct(p, Action::PierceAttack);
        w.grant_skill(p, "Perk_Assassin");
        let ev = HitEvent::melee(guard, p, DamageType::Blade, 10);
        let out = assess_hit(&mut w, &ev);
        assert_eq!(out.action, Action::LiePiercedDead);
        assert_eq!(w.current_task(guard), Some(State::PiercedDead));
        assert_eq!(w.dr_pool(guard, Pool::Hit), 0);
        let wolf = w.spawn_npc("Wolf", Species::Wolf, 5, 5);
        let ev = HitEvent::melee(wolf, p, DamageType::Blade, 10);
        assert_eq!(assess_hit(&mut w, &ev).action, Action::LieDead);
        let rat = w.spawn_npc("Essential", Species::Human, 5, 5);
        w.set_immortal(rat, true);
        let ev = HitEvent::melee(rat, p, DamageType::Blade, 10);
        assert_eq!(assess_hit(&mut w, &ev).action, Action::LieKnockDown);
        // Knock-out perk with an impact power attack from behind.
        let other = w.spawn_npc("Other", Species::Human, 5, 5);
        w.set_current_action_direct(p, Action::PowerAttack);
        w.grant_skill(p, "Perk_KnockOut");
        let ev = HitEvent::melee(other, p, DamageType::Impact, 10);
        let out = assess_hit(&mut w, &ev);
        assert_eq!(out.action, Action::LieKnockDown);
        assert_eq!(out.damage, 0);
    }

    #[test]
    fn non_lethal_impact_projectiles_knock_out() {
        let (mut w, p, _) = duel();
        let man = w.spawn_npc("Man", Species::Human, 1, 1);
        w.set_alignment(man, Alignment::Orc);
        w.dr_set_pool(man, Pool::Hit, 20);
        let mut ev = HitEvent::melee(man, p, DamageType::Impact, 100);
        ev.kind = HitKind::Projectile;
        ev.hit_mult = 1.0;
        let out = assess_hit(&mut w, &ev);
        assert_eq!(out.action, Action::LieKnockOut);
        assert_eq!(w.current_task(man), Some(State::Unconscious));
        let tough = w.spawn_npc("Tough", Species::Human, 50, 50);
        w.set_alignment(tough, Alignment::Orc);
        let mut ev = HitEvent::melee(tough, p, DamageType::Impact, 100);
        ev.kind = HitKind::Projectile;
        ev.hit_mult = 0.8;
        let out = assess_hit(&mut w, &ev);
        assert_eq!(out.action, Action::SitKnockDown);
    }
}
