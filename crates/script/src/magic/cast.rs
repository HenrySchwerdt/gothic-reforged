//! The casting pipeline: which cast animation a spell uses (`GetCastAction`), whether it can be
//! cast at all, the six cast phases of `_AI_StartCastPhase` (mana accounting, consuming the
//! scroll) and the three dispatchers `MagicProjectile`, `MagicInstant` and `MagicSummon` that
//! turn a finished cast into an effect.
//!
//! Mana is the player's; an NPC's pool is a constant 1000 and `AddManaPoints` ignores it (B01), so
//! the same code serves both. Scrolls are *not* free: the DLL skips the mana payment only for
//! items of the Alchemy category (potions), runes and scrolls cost their `MinManaCost`.

use crate::enums::{Action, Slot, Species};
use crate::host::{EntityId, Pool, Pos, ScriptHost};
use crate::magic::api::{
    CastType, MagicEvent, PathStyle, ProjectileSpec, SpellDamage, SpellInfo, SpellType,
};
use crate::magic::effects;
use crate::predicates::is_evil;
use crate::speech::start_say_doesnt_work;
use crate::stats;

/// The phase argument of `_AI_StartCastPhase`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// 0: forget spell, target and mana paid; stop the effects.
    Reset,
    /// 1: the spell is held in the hand (selected, not yet charged).
    Hold,
    /// 2: the load phase pays `MinManaCost`.
    Load,
    /// 3: aborted: the mana paid comes back.
    Abort,
    /// 4: power charge pays the rest up to `MaxManaCost`.
    Power,
    /// 5: the cast itself.
    Cast,
}

impl Phase {
    /// Raw argument value.
    pub const fn raw(self) -> u8 {
        match self {
            Self::Reset => 0,
            Self::Hold => 1,
            Self::Load => 2,
            Self::Abort => 3,
            Self::Power => 4,
            Self::Cast => 5,
        }
    }
}

/// `StatsApi`-free alchemy category id (`PSItem.Category`: potions).
pub const CATEGORY_ALCHEMY: u32 = 4;

/// The spell of the item in the caster's right hand (its stack is `find_stack_index(RightHand)`).
pub fn hand_spell(host: &dyn ScriptHost, caster: EntityId) -> Option<SpellInfo> {
    let name = host.slot_spell_name(caster, Slot::RightHand);
    if name.is_empty() {
        None
    } else {
        host.spell(&name)
    }
}

/// Port of `GetCastAction` (S:52a10): the animation action of the spell in hand. `None` without
/// a spell; `FlameSword` for `Spl_Flamesword`; Immediate spells are a `QuickCast`, Regular ones
/// use the `Summon` motion; Power spells are a `PowerCast` when the caster has the maximum mana
/// (the god cheat pays for everything; Ice Golems never check; a Goblin with `ExtendedContent`
/// never charges), otherwise a plain `Cast`.
pub fn get_cast_action(host: &dyn ScriptHost, caster: EntityId) -> Action {
    let Some(spell) = hand_spell(host, caster) else {
        return Action::None;
    };
    if spell.name == "Spl_Flamesword" {
        return Action::FlameSword;
    }
    match spell.cast_type {
        CastType::Immediate => Action::QuickCast,
        CastType::Regular => Action::Summon,
        CastType::Power => {
            let species = host.species(caster);
            if species != Species::IceGolem {
                let god = caster == host.player() && host.cheat_god();
                if stats::pool(host, caster, Pool::Mana) < spell.max_mana && !god {
                    return Action::Cast;
                }
                if species == Species::Goblin && host.setup().extended_content {
                    return Action::Cast;
                }
            }
            Action::PowerCast
        }
    }
}

/// Daylight hours in which `Spl_DuskToDawn` refuses to work (the two bounds in the DLL's data).
const DAYLIGHT: (f32, f32) = (6.0, 20.0);

/// Port of the private cast check at S:798d0 that every `PS_Magic_*Cast` state runs first.
/// Characters other than the hero always may. The hero needs `MinManaCost` mana (or the god
/// cheat); a spell that targets the focus needs a focus unless it is a power spell; `Spl_AnimalForm`
/// needs a focus; `Spl_DuskToDawn` does not work in daylight; `Spl_DestroyEvil` needs an evil
/// target. A refusal makes the grunt `StartSayDoesntWork` (and a fizzle effect); a lack of mana
/// only fizzles.
pub fn can_cast(host: &mut dyn ScriptHost, caster: EntityId, target: EntityId) -> bool {
    let Some(spell) = hand_spell(host, caster) else {
        return false;
    };
    if caster != host.player() {
        return true;
    }
    let god = host.cheat_god();
    let fizzle = |host: &mut dyn ScriptHost| {
        host.emit_magic(MagicEvent::Effect {
            name: "eff_magic_fizzle_01".into(),
            on: caster,
        });
    };
    if stats::pool(host, caster, Pool::Mana) < spell.min_mana && !god {
        fizzle(host);
        return false;
    }
    let refuse = |host: &mut dyn ScriptHost| {
        start_say_doesnt_work(host, caster);
        fizzle(host);
        false
    };
    if target.is_none() && spell.needs_focus && spell.cast_type != CastType::Power {
        return refuse(host);
    }
    if spell.name == "Spl_AnimalForm" && target.is_none() && spell.needs_focus {
        return refuse(host);
    }
    if spell.name == "Spl_DuskToDawn" {
        let t = host.world_hour();
        if DAYLIGHT.0 < t && t < DAYLIGHT.1 {
            return refuse(host);
        }
    }
    if spell.name == "Spl_DestroyEvil" && (target.is_none() || !is_evil(host, target)) {
        return refuse(host);
    }
    true
}

/// Port of `_AI_StartCastPhase` (S:39760) for a spell on `stack` of the caster's inventory.
///
/// * `Hold` records spell, target and `LastSpell` and clears `ManaUsed`.
/// * `Load` (once, while `ManaUsed` is zero) records them again, sets `ManaUsed = MinManaCost` and
///   pays it unless the item is an Alchemy item.
/// * `Power` sets `ManaUsed = MaxManaCost` and pays the difference.
/// * `Abort` refunds `ManaUsed` and resets.
/// * `Cast` calls the spell's `FuncOnCast` (or the dispatcher of its type), then uses up one
///   non-permanent item.
/// * `Reset` clears the bookkeeping.
pub fn start_cast_phase(
    host: &mut dyn ScriptHost,
    caster: EntityId,
    target: EntityId,
    stack: i32,
    phase: Phase,
) {
    let name = host.stack_spell(caster, stack);
    let spell = host.spell(&name);
    host.emit_magic(MagicEvent::Phase {
        caster,
        spell: name.clone(),
        phase: phase.raw(),
    });
    let reset = |host: &mut dyn ScriptHost| {
        host.set_cast_spell(caster, "");
        host.set_spell_target(caster, EntityId::NONE);
        host.set_mana_used(caster, 0);
    };
    let Some(spell) = spell else {
        reset(host);
        return;
    };
    match phase {
        Phase::Reset => reset(host),
        Phase::Hold => {
            host.set_spell_target(caster, target);
            host.set_cast_spell(caster, &name);
            host.set_last_spell(caster, &name);
            host.set_mana_used(caster, 0);
        }
        Phase::Load => {
            if host.mana_used(caster) == 0 {
                host.set_spell_target(caster, target);
                host.set_cast_spell(caster, &name);
                host.set_last_spell(caster, &name);
                host.set_mana_used(caster, spell.min_mana);
                if host.stack_category(caster, stack) != CATEGORY_ALCHEMY {
                    stats::add_mana_points(host, caster, -spell.min_mana);
                }
            }
        }
        Phase::Abort => {
            let paid = host.mana_used(caster);
            stats::add_mana_points(host, caster, paid);
            reset(host);
        }
        Phase::Power => {
            host.set_spell_target(caster, target);
            host.set_cast_spell(caster, &name);
            host.set_last_spell(caster, &name);
            host.set_mana_used(caster, spell.max_mana);
            stats::add_mana_points(host, caster, spell.min_mana - spell.max_mana);
        }
        Phase::Cast => {
            let consumable = !host.stack_permanent(caster, stack);
            let used = host.mana_used(caster);
            let tgt = host.spell_target(caster);
            host.emit_magic(MagicEvent::Cast {
                caster,
                spell: name.clone(),
                target: tgt,
                mana_used: used,
            });
            if consumable {
                host.delete_stack_items(caster, stack, 1);
            }
            let func = if spell.func_on_cast.is_empty() {
                match spell.spell_type {
                    SpellType::Projectile => "MagicProjectile",
                    SpellType::Target => "MagicInstant",
                    SpellType::Summon => "MagicSummon",
                }
                .to_string()
            } else {
                spell.func_on_cast.clone()
            };
            effects::call_spell_function(host, &func, caster, tgt, stack);
        }
    }
}

/// How a cast proceeds in the animation: `_AI_Cast_Begin` loads (and for a power cast charges),
/// `_AI_Cast_Execute` casts and resets. Both are run back to back by the host, which plays the
/// animation between them. Returns the mana the cast cost.
///
/// Port of `_AI_Cast` (S:38710), `_AI_Cast_Begin` (S:38ae0) and `_AI_Cast_Execute` (S:39180).
pub fn run_cast(host: &mut dyn ScriptHost, caster: EntityId, power: bool) -> i32 {
    let stack = host.find_stack_index(caster, Slot::RightHand);
    if stack < 0 {
        return 0;
    }
    let target = {
        let t = host.current_target(caster);
        if t.is_some() { t } else { host.focus_entity() }
    };
    let name = host.stack_spell(caster, stack);
    if host.cast_spell(caster) != name {
        start_cast_phase(host, caster, target, stack, Phase::Reset);
        start_cast_phase(host, caster, target, stack, Phase::Hold);
    }
    start_cast_phase(host, caster, target, stack, Phase::Load);
    if power {
        start_cast_phase(host, caster, target, stack, Phase::Power);
    }
    let paid = host.mana_used(caster);
    start_cast_phase(host, caster, target, stack, Phase::Cast);
    start_cast_phase(host, caster, EntityId::NONE, stack, Phase::Reset);
    paid
}

/// Port of `MagicInstant` (S:52c40): the spell acts at once; its `FuncOnTargetHit` is called with
/// the caster as `self` and the target as `other`.
pub fn magic_instant(host: &mut dyn ScriptHost, caster: EntityId, target: EntityId, stack: i32) {
    let Some(spell) = host.spell(&host.cast_spell(caster)) else {
        return;
    };
    if !spell.func_on_hit.is_empty() {
        effects::call_spell_function(host, &spell.func_on_hit, caster, target, stack);
    }
}

/// Port of `MagicProjectile` (S:52db0): spawn the spell's missile at the caster, owned by him and
/// aimed at the target (a power cast, one that paid the maximum mana, homes on it; otherwise it
/// flies straight), and hand it the
/// mana that was paid, which scales its damage.
pub fn magic_projectile(
    host: &mut dyn ScriptHost,
    caster: EntityId,
    target: EntityId,
    _stack: i32,
) {
    let Some(spell) = host.spell(&host.cast_spell(caster)) else {
        return;
    };
    let damage = spell.damage.unwrap_or(SpellDamage {
        damage_type: crate::enums::DamageType::None,
        amount: 0,
        mana_mult: 1.0,
        hit_mult: 1.0,
    });
    let mana_used = host.mana_used(caster);
    let powered = spell.cast_type == CastType::Power
        && spell.max_mana > spell.min_mana
        && mana_used >= spell.max_mana;
    let path = if target.is_some() && powered {
        PathStyle::Missile
    } else {
        PathStyle::Line
    };
    host.spawn_projectile(ProjectileSpec {
        owner: caster,
        target,
        spell: spell.name.clone(),
        template: spell.spawn.clone(),
        mana_used,
        hit_mult: damage.hit_mult,
        damage_type: damage.damage_type,
        amount: damage.amount,
        path,
        velocity: spell.velocity,
        deity: spell.deity,
        quality: 0,
    });
}

/// Port of `MagicSummon` (S:530e0): spawn the spell's entity at the target's pose (or beside the
/// caster), owned by the caster, then call `FuncOnTargetHit`. A second `Smn_Light` replaces the
/// first.
pub fn magic_summon(host: &mut dyn ScriptHost, caster: EntityId, target: EntityId, stack: i32) {
    let Some(spell) = host.spell(&host.cast_spell(caster)) else {
        return;
    };
    if !spell.spawn.is_empty() {
        let at: Pos = if target.is_some() {
            host.position(target)
        } else {
            host.position(caster)
        };
        host.spawn_area(&spell.spawn, caster, &spell.name, at);
    }
    if !spell.func_on_hit.is_empty() {
        effects::call_spell_function(host, &spell.func_on_hit, caster, target, stack);
    }
}
