//! B20: the effect of a spell once it hits (`FuncOnTargetHit`), the potion and rune helpers that
//! share them, and the decision predicates of the school of magic (`CanControl`,
//! `CanTameAnimal`, `CanTransformInto`, the species sets, the duration rules).
//!
//! Every effect has the DLL's signature: `self` is the caster, `other` the target and `stack`
//! the inventory stack of the item. [`call_spell_function`] is the registry the cast pipeline
//! uses to run a function by its template name; names the DLL does not know (a few spell
//! templates point at functions that are not in the script library) do nothing, like a failed
//! `CallScriptFromScript`.

use crate::enums::{
    Action, AiMode, Alignment, AttackReason, DamageType, NpcClass, PartyMemberType, Slot, Species,
};
use crate::host::{EntityId, Pool, ScriptHost};
use crate::magic::api::{Boost, MagicEvent};
use crate::perception::{assess_target, can_attack};
use crate::predicates::{is_boss, is_evil, is_gate_guard};
use crate::routine::select::continue_routine;
use crate::routine::state::State;
use crate::stats;

/// Status bit: burning.
pub const STATUS_BURNING: u32 = 1;
/// Status bit: frozen.
pub const STATUS_FROZEN: u32 = 2;
/// Status bit: poisoned.
pub const STATUS_POISONED: u32 = 4;
/// Status bit: diseased.
pub const STATUS_DISEASED: u32 = 8;
/// Status bit: controlled / misted.
pub const STATUS_CONTROLLED: u32 = 0x40;
/// Status bit: soul-travel ghost body.
pub const STATUS_GHOST: u32 = 0x80;
/// Status bit: soul-travel, out of the body.
pub const STATUS_OUT_OF_BODY: u32 = 0x200;

/// Radius in which `MagicStun` stuns (cm).
pub const STUN_RADIUS: f32 = 1200.0;
/// Radius in which `MagicDispel` clears (cm).
pub const DISPEL_RADIUS: f32 = 2500.0;
/// Radius in which `MagicBloodlust` looks for a victim (cm).
pub const BLOODLUST_RADIUS: f32 = 1000.0;
/// Radius in which `MagicHypnosis` puts people under (cm).
pub const HYPNOSIS_RADIUS: f32 = 2000.0;
/// Radius in which `MagicCallAnimals` summons animals (cm).
pub const CALL_ANIMALS_RADIUS: f32 = 25000.0;
/// Seconds a tamed or controlled spell lasts is not limited: the creature stays until dismissed.
const BASE_DURATION: f32 = 60.0;

// ---- species sets -------------------------------------------------------------------------------

fn raw(s: Species) -> u32 {
    s.raw()
}

/// Port of `IsAnimal` (S:57bd0): the species `CanTameAnimal` and `CanTransformInto` accept.
pub fn is_animal(host: &dyn ScriptHost, e: EntityId) -> bool {
    gothic_portcov::hit!("is_animal");
    host.exists(e) && matches!(raw(host.species(e)), 8..=20 | 22 | 23 | 29 | 33 | 34 | 39)
}

/// Port of `IsGame` (S:57fc0): huntable animals (Boar, Bison, Rhino, Deer).
pub fn is_game(host: &dyn ScriptHost, e: EntityId) -> bool {
    gothic_portcov::hit!("is_game");
    matches!(raw(host.species(e)), 11 | 14 | 15 | 29)
}

/// Port of `IsLarge` (S:57ec0): the large animals (Minecrawler, Sabertooth, Shadowbeast, Bison,
/// Rhino, Ripper, Alligator); transformations into them last half as long.
pub fn is_large(host: &dyn ScriptHost, e: EntityId) -> bool {
    gothic_portcov::hit!("is_large");
    matches!(raw(host.species(e)), 8 | 12..=16 | 20)
}

// ---- gates ---------------------------------------------------------------------------------------

fn player_message(host: &mut dyn ScriptHost, caster: EntityId, key: &'static str) {
    if caster == host.player() {
        host.emit_magic(MagicEvent::Message {
            key,
            subject: caster,
        });
    }
}

/// Port of `CanTameAnimal` (S:5e800): an animal; the dangerous ones (Minecrawler, Sabertooth,
/// Shadowbeast, Ripper, Alligator) need Intelligence 100. The hero is told `Impossible` or
/// `TooHard`.
pub fn can_tame_animal(host: &mut dyn ScriptHost, caster: EntityId, target: EntityId) -> bool {
    gothic_portcov::hit!("can_tame_animal");
    if !host.exists(target) || !is_animal(host, target) {
        player_message(host, caster, "Impossible");
        return false;
    }
    if matches!(raw(host.species(target)), 8 | 12 | 13 | 16 | 20)
        && stats::attribute(host, caster, crate::host::Attribute::Intelligence) < 100
    {
        player_message(host, caster, "TooHard");
        return false;
    }
    true
}

/// Port of `CanTransformInto` (S:5ea30): any animal but a pig; otherwise the hero is told
/// `Impossible`.
pub fn can_transform_into(host: &mut dyn ScriptHost, caster: EntityId, form: EntityId) -> bool {
    gothic_portcov::hit!("can_transform_into");
    if host.exists(form) && is_animal(host, form) && host.species(form) != Species::Pig {
        return true;
    }
    player_message(host, caster, "Impossible");
    false
}

/// Port of `CanControl` (S:549a0) [I for the exact branch order]: not the hero; only an idle
/// target (routine, reaction or sleeping); no boss, no mage class, no gate guard; humans and
/// goblins can never be controlled (`Impossible`), everything else needs Intelligence 200
/// (`TooHard`).
pub fn can_control(host: &mut dyn ScriptHost, caster: EntityId, target: EntityId) -> bool {
    gothic_portcov::hit!("can_control");
    if !host.exists(target) || target == host.player() {
        return false;
    }
    if !matches!(
        host.ai_mode(target),
        AiMode::Routine | AiMode::Reaction | AiMode::Sleep
    ) {
        return false;
    }
    if is_boss(host, target)
        || host.npc_class(target) == NpcClass::Mage
        || is_gate_guard(host, target)
    {
        player_message(host, caster, "Impossible");
        return false;
    }
    if matches!(host.species(target), Species::Human | Species::Goblin) {
        player_message(host, caster, "Impossible");
        return false;
    }
    if stats::attribute(host, caster, crate::host::Attribute::Intelligence) < 200 {
        player_message(host, caster, "TooHard");
        return false;
    }
    true
}

// ---- durations -----------------------------------------------------------------------------------

/// Port of `GetPlayerSpellDurationBonus` (S:580c0): +0.5 (50 percent more) for the hero when he
/// has the perk of the spell's school: `Perk_Firemage` for Innos, `Perk_Watermage` for Adanos,
/// `Perk_Darkmage` for Beliar. 0 otherwise.
pub fn spell_duration_bonus(host: &dyn ScriptHost, caster: EntityId, spell_name: &str) -> f32 {
    gothic_portcov::hit!("spell_duration_bonus");
    if caster != host.player() {
        return 0.0;
    }
    let Some(spell) = host.spell(spell_name) else {
        return 0.0;
    };
    let perk = match spell.deity {
        crate::enums::SpellDeity::Innos => "Perk_Firemage",
        crate::enums::SpellDeity::Adanos => "Perk_Watermage",
        crate::enums::SpellDeity::Beliar => "Perk_Darkmage",
        crate::enums::SpellDeity::None => return 0.0,
    };
    if host.is_skill_active(caster, perk) {
        0.5
    } else {
        0.0
    }
}

/// Port of `GetTransformDuration` (S:582a0): `max(INT - 60, 0) + 60` seconds, halved for a
/// large form; the hero gets x1 (x2 with `Perk_Druid`, plus 0.5 with `Perk_Watermage`).
pub fn transform_duration(host: &dyn ScriptHost, caster: EntityId, form: EntityId) -> f32 {
    gothic_portcov::hit!("transform_duration");
    let int = stats::attribute(host, caster, crate::host::Attribute::Intelligence);
    let mut secs = (int - 60).max(0) as f32 + BASE_DURATION;
    if is_large(host, form) {
        secs *= 0.5;
    }
    if caster == host.player() {
        let mut mult = 1.0;
        if host.is_skill_active(caster, "Perk_Druid") {
            mult = 2.0;
        }
        if host.is_skill_active(caster, "Perk_Watermage") {
            mult += 0.5;
        }
        secs *= mult;
    }
    secs
}

// ---- status effects -------------------------------------------------------------------------------

fn set_status(host: &mut dyn ScriptHost, target: EntityId, mask: u32, on: bool) {
    host.set_status_bits(target, mask, on);
    host.emit_magic(MagicEvent::Status { target, mask, on });
}

/// Port of `MagicBurn` (S:56e60): set the target on fire.
pub fn magic_burn(host: &mut dyn ScriptHost, _caster: EntityId, target: EntityId) {
    gothic_portcov::hit!("magic_burn");
    set_status(host, target, STATUS_BURNING, true);
}

/// Port of `MagicFreeze` (S:56f30): freeze the target (`ZS_Freeze` handles the rest).
pub fn magic_freeze(host: &mut dyn ScriptHost, _caster: EntityId, target: EntityId) {
    gothic_portcov::hit!("magic_freeze");
    set_status(host, target, STATUS_FROZEN, true);
}

/// Port of `MagicPoison` (S:57000): poison a target that `CanBePoisoned`; either way the target
/// holds the caster responsible (`AssessTarget` with reason ReactToDamage).
pub fn magic_poison(host: &mut dyn ScriptHost, caster: EntityId, target: EntityId) {
    gothic_portcov::hit!("magic_poison");
    if crate::damage::can_be_poisoned(host, target) {
        set_status(host, target, STATUS_POISONED, true);
    }
    assess_target(host, target, caster, AttackReason::ReactToDamage);
}

/// Port of `MagicDisease` (S:57160): a diseasable target loses all stamina and falls ill; either
/// way it holds the caster responsible.
pub fn magic_disease(host: &mut dyn ScriptHost, caster: EntityId, target: EntityId) {
    gothic_portcov::hit!("magic_disease");
    if crate::damage::can_be_diseased(host, target) {
        stats::set_pool(host, target, Pool::Stamina, 0);
        set_status(host, target, STATUS_DISEASED, true);
    }
    assess_target(host, target, caster, AttackReason::ReactToDamage);
}

/// Port of `MagicRefresh` (S:572d0): mana to the maximum.
pub fn magic_refresh(host: &mut dyn ScriptHost, caster: EntityId) {
    gothic_portcov::hit!("magic_refresh");
    let missing = stats::pool_max(host, caster, Pool::Mana) - stats::pool(host, caster, Pool::Mana);
    stats::add_mana_points(host, caster, missing);
}

/// Port of `MagicCure` (S:53870): hit points to the maximum.
pub fn magic_cure(host: &mut dyn ScriptHost, caster: EntityId) {
    gothic_portcov::hit!("magic_cure");
    let missing = stats::pool_max(host, caster, Pool::Hit) - stats::pool(host, caster, Pool::Hit);
    stats::add_hit_points(host, caster, missing);
}

/// Port of `MagicCurePoison` (S:53970): clear the poison.
pub fn magic_cure_poison(host: &mut dyn ScriptHost, who: EntityId) {
    gothic_portcov::hit!("magic_cure_poison");
    if host.has_status(who, STATUS_POISONED) {
        set_status(host, who, STATUS_POISONED, false);
    }
}

/// Port of `MagicCureDisease` (S:53a90): clear the disease.
pub fn magic_cure_disease(host: &mut dyn ScriptHost, who: EntityId) {
    gothic_portcov::hit!("magic_cure_disease");
    if host.has_status(who, STATUS_DISEASED) {
        set_status(host, who, STATUS_DISEASED, false);
    }
}

/// Port of `MagicHealOther` (S:56b20): clear poison, disease, fire and ice of the target and
/// restore its hit points to the maximum.
pub fn magic_heal_other(host: &mut dyn ScriptHost, target: EntityId) {
    gothic_portcov::hit!("magic_heal_other");
    if !host.exists(target) {
        return;
    }
    for mask in [
        STATUS_POISONED,
        STATUS_DISEASED,
        STATUS_BURNING,
        STATUS_FROZEN,
    ] {
        set_status(host, target, mask, false);
    }
    let max = stats::pool_max(host, target, Pool::Hit);
    stats::set_pool(host, target, Pool::Hit, max);
}

/// Port of `MagicKeepStamina` (S:58b70): only the hero; the engine parks a "stamina held at
/// maximum" flag on him (it reuses `GuardStatus` and `LastDistToGuardPoint`).
pub fn magic_keep_stamina(host: &mut dyn ScriptHost, who: EntityId) {
    gothic_portcov::hit!("magic_keep_stamina");
    if who == host.player() {
        let level = stats::pool_max(host, who, Pool::Stamina);
        host.set_boost(who, Boost::KeepStamina, 0.0, level);
    }
}

/// Port of `MagicSpeedUp` (S:58a30): only the hero; he runs fast for `(StaminaMax - 200) / 2 + 60`
/// seconds (never less than 60).
pub fn magic_speed_up(host: &mut dyn ScriptHost, who: EntityId) {
    gothic_portcov::hit!("magic_speed_up");
    if who == host.player() {
        let max = stats::pool_max(host, who, Pool::Stamina);
        let secs = ((max - 200) / 2).max(0) as f32 + BASE_DURATION;
        host.set_boost(who, Boost::SpeedUp, secs, 0);
    }
}

/// Port of `MagicStun` (S:58cb0): every character within 12 m, nearest first, that is neither
/// dead nor down, not already stunned, not on guard duty and not in the caster's party gets the
/// `ZS_Stunned` task. The scan ends at the first character beyond the radius.
pub fn magic_stun(host: &mut dyn ScriptHost, caster: EntityId) {
    gothic_portcov::hit!("magic_stun");
    for npc in host.all_npcs_by_distance(caster) {
        if npc == caster || matches!(host.ai_mode(npc), AiMode::Dead | AiMode::Down) {
            continue;
        }
        if host.current_task(npc) == Some(State::Stunned)
            && host.current_state(npc) != Some(State::StunnedEnd)
        {
            continue;
        }
        if host.current_task(npc) == Some(State::Guard) {
            continue;
        }
        let leader = host.party_leader(npc);
        let caster_leader = host.party_leader(caster);
        if leader.is_some() && (leader == caster || leader == caster_leader) {
            continue;
        }
        if host.distance(caster, npc) >= STUN_RADIUS {
            break;
        }
        host.full_stop(npc);
        host.set_task(npc, State::Stunned);
    }
}

/// Port of `MagicDispel` (S:53460): for a damage element (ice 5, fire 4, lightning 6 — the DLL's
/// parameter 4, 5 or 6 picks it) remove the area-spell entities of that element within 25 m
/// and put out the characters burning or frozen there.
pub fn magic_dispel(host: &mut dyn ScriptHost, caster: EntityId, element: DamageType) {
    gothic_portcov::hit!("magic_dispel");
    let centre = host.position(caster);
    host.dispel_areas(centre, DISPEL_RADIUS, element);
    for npc in host.all_npcs() {
        if host.distance(caster, npc) > DISPEL_RADIUS {
            continue;
        }
        match element {
            DamageType::Ice if host.has_status(npc, STATUS_FROZEN) => {
                set_status(host, npc, STATUS_FROZEN, false)
            }
            DamageType::Fire if host.has_status(npc, STATUS_BURNING) => {
                set_status(host, npc, STATUS_BURNING, false)
            }
            _ => {}
        }
    }
}

// ---- minds ----------------------------------------------------------------------------------------

/// Join the caster's party as a controlled creature (tame, word of control): leader, member type,
/// not waiting, follow task.
fn make_controlled(host: &mut dyn ScriptHost, caster: EntityId, target: EntityId) {
    host.set_party_leader(target, caster);
    host.set_party_member_type(target, PartyMemberType::Controlled);
    host.set_party_waiting(target, false);
    host.full_stop(target);
    host.set_task(target, State::FollowPlayer);
}

/// Port of `MagicTameAnimal` (S:54e70): if the animal can be tamed it (and its
/// herd) follows the hero as a controlled member of his party.
pub fn magic_tame_animal(host: &mut dyn ScriptHost, caster: EntityId, target: EntityId) {
    gothic_portcov::hit!("magic_tame_animal");
    if !can_tame_animal(host, caster, target) {
        return;
    }
    host.print_game_message("GO_TameAnimal", &host.name(target), 8);
    for member in host.party_members(target) {
        if member != target {
            host.set_party_leader(member, target);
        }
    }
    make_controlled(host, caster, target);
}

/// Port of `MagicWordOfControl` (S:54c00): a controllable target joins the party.
pub fn magic_word_of_control(host: &mut dyn ScriptHost, caster: EntityId, target: EntityId) {
    gothic_portcov::hit!("magic_word_of_control");
    if !can_control(host, caster, target) {
        return;
    }
    host.print_game_message("GO_Control", &host.name(target), 8);
    make_controlled(host, caster, target);
}

/// Port of `MagicCharm` (S:56770): a target in talk mode forgets every grudge (attitude,
/// attack reasons, last fight, crime) and the charmer is treated as a friend; a boss resists.
pub fn magic_charm(host: &mut dyn ScriptHost, caster: EntityId, target: EntityId) {
    gothic_portcov::hit!("magic_charm");
    if host.ai_mode(target) != AiMode::Talk && !is_boss(host, target) {
        host.set_attack_reason(target, AttackReason::None);
        host.set_last_player_attack_reason(target, AttackReason::None);
        host.set_last_fight_vs_player(target, crate::enums::LastFight::None);
        host.set_last_player_crime(target, crate::enums::PlayerCrime::None);
        host.set_last_fight_timestamp(target, 0.0);
        host.set_current_target(target, EntityId::NONE);
        host.full_stop(target);
        continue_routine(host, target);
    } else {
        assess_target(host, target, caster, AttackReason::ReactToDamage);
    }
}

/// Port of `MagicBloodlust` (S:55cb0): a boss only turns on the caster; others who can fight
/// attack the nearest other fighter within 10 m (reason 17 = Duel) or, with none, the caster.
pub fn magic_bloodlust(host: &mut dyn ScriptHost, caster: EntityId, target: EntityId) {
    gothic_portcov::hit!("magic_bloodlust");
    if is_boss(host, target) {
        assess_target(host, target, caster, AttackReason::ReactToDamage);
        return;
    }
    if host.npc_class(target) == NpcClass::Mage {
        assess_target(host, target, caster, AttackReason::ReactToDamage);
        return;
    }
    for npc in host.all_npcs_by_distance(target) {
        if npc == target || npc == caster {
            continue;
        }
        if host.distance(target, npc) > BLOODLUST_RADIUS {
            break;
        }
        if can_attack(host, npc) && !is_gate_guard(host, npc) {
            host.full_stop(npc);
            assess_target(host, target, npc, AttackReason::Duel);
            return;
        }
    }
    assess_target(host, target, caster, AttackReason::ReactToDamage);
}

/// Port of `MagicFear` (S:55eb0 / S:322f0): a target that can fight (not a boss, not the gate
/// guard) is told by fight sound, stops and flees from the caster.
pub fn magic_fear(host: &mut dyn ScriptHost, caster: EntityId, target: EntityId) {
    gothic_portcov::hit!("magic_fear");
    if is_boss(host, target) || is_gate_guard(host, target) {
        assess_target(host, target, caster, AttackReason::ReactToDamage);
        return;
    }
    host.full_stop(target);
    host.set_current_target(target, caster);
    host.set_task(target, State::Flee);
}

/// Port of `MagicSleep` (S:54670): a target that is not a boss falls under `ZS_MagicSleep`.
pub fn magic_sleep(host: &mut dyn ScriptHost, _caster: EntityId, target: EntityId) {
    gothic_portcov::hit!("magic_sleep");
    if !is_boss(host, target) {
        host.full_stop(target);
        host.set_task(target, State::MagicSleep);
    }
}

/// Port of `MagicHypnosis` (S:543b0): every character within 20 m (nearest first) that is idle
/// (routine, reaction, flee, arena...), not the caster, not in his party and not a boss falls under
/// `ZS_MagicHypnosis`.
pub fn magic_hypnosis(host: &mut dyn ScriptHost, caster: EntityId) {
    gothic_portcov::hit!("magic_hypnosis");
    let caster_leader = host.party_leader(caster);
    for npc in host.all_npcs_by_distance(caster) {
        if npc == caster {
            continue;
        }
        if matches!(
            host.ai_mode(npc),
            AiMode::Reaction
                | AiMode::Combat
                | AiMode::Talk
                | AiMode::Down
                | AiMode::Dead
                | AiMode::PlayerParty
        ) {
            continue;
        }
        if host.distance(caster, npc) >= HYPNOSIS_RADIUS {
            break;
        }
        let leader = host.party_leader(npc);
        if leader.is_some() && (leader == caster || leader == caster_leader) {
            continue;
        }
        if is_boss(host, npc) {
            continue;
        }
        host.emit_magic(MagicEvent::Effect {
            name: "hypnosis".into(),
            on: npc,
        });
        host.full_stop(npc);
        host.set_task(npc, State::MagicHypnosis);
    }
}

/// Port of `MagicCallAnimals` (S:54160): Intelligence over 300 calls three animals, over 200 two,
/// otherwise one. Each is a wild animal within 250 m that can reach the caster, can attack and is
/// not already hunting for him; it is sent after the target with reason 16 (player command).
pub fn magic_call_animals(host: &mut dyn ScriptHost, caster: EntityId, target: EntityId) {
    gothic_portcov::hit!("magic_call_animals");
    let int = stats::attribute(host, caster, crate::host::Attribute::Intelligence);
    let wanted = if int > 300 {
        3
    } else if int > 200 {
        2
    } else {
        1
    };
    let mut called = 0;
    for npc in host.all_npcs_by_distance(caster) {
        if called >= wanted {
            break;
        }
        if npc == caster || npc == target || host.attack_reason(npc) == AttackReason::PlayerCommand
        {
            continue;
        }
        if host.distance(caster, npc) > CALL_ANIMALS_RADIUS / 100.0 * 100.0 {
            break;
        }
        if is_animal(host, npc) && can_attack(host, npc) && host.can_goto(npc, target) {
            assess_target(host, npc, target, AttackReason::PlayerCommand);
            called += 1;
        }
    }
}

// ---- summons ---------------------------------------------------------------------------------------

/// Port of the private helper `SummonPartyMonster` (S:55210): spawn a creature beside the caster,
/// make it a summoned member of his party (dialog-enabled, following, in the hero's political
/// camp) and return it. `NONE` when there is no room (the DLL logs "Could not find spawn pose").
pub fn summon_party_monster(
    host: &mut dyn ScriptHost,
    caster: EntityId,
    template: &str,
) -> EntityId {
    gothic_portcov::hit!("summon_party_monster");
    let npc = host.spawn_npc(template, caster);
    if npc.is_none() {
        return npc;
    }
    host.set_party_leader(npc, caster);
    host.set_party_member_type(npc, PartyMemberType::Summoned);
    host.set_party_waiting(npc, false);
    host.set_dialog_party_enabled(npc, true);
    if caster == host.player() {
        let camp: Alignment = host.alignment(caster);
        host.set_alignment(npc, camp);
    }
    host.full_stop(npc);
    host.set_task(npc, State::FollowPlayer);
    host.emit_magic(MagicEvent::Summoned {
        owner: caster,
        template: template.into(),
        npc,
    });
    npc
}

/// Port of `MagicSummonDemon` (S:554e0).
pub fn magic_summon_demon(host: &mut dyn ScriptHost, caster: EntityId) {
    gothic_portcov::hit!("magic_summon_demon");
    summon_party_monster(host, caster, "Demon");
}

/// Port of `MagicSummonSkeleton` (S:555d0): a sword skeleton.
pub fn magic_summon_skeleton(host: &mut dyn ScriptHost, caster: EntityId) {
    gothic_portcov::hit!("magic_summon_skeleton");
    summon_party_monster(host, caster, "Skeleton_Sword");
}

/// Port of `MagicSummonGolem` (S:556c0).
pub fn magic_summon_golem(host: &mut dyn ScriptHost, caster: EntityId) {
    gothic_portcov::hit!("magic_summon_golem");
    summon_party_monster(host, caster, "Golem");
}

/// Port of `MagicSummonGoblin` (S:557b0).
pub fn magic_summon_goblin(host: &mut dyn ScriptHost, caster: EntityId) {
    gothic_portcov::hit!("magic_summon_goblin");
    summon_party_monster(host, caster, "Goblin");
}

/// Army size by Intelligence: `small` below 250, `mid` from 250 and `big` from 300.
const fn army_size(int: i32, small: u32, mid: u32, big: u32) -> u32 {
    if int >= 300 {
        big
    } else if int >= 250 {
        mid
    } else {
        small
    }
}

/// Port of `MagicSummonArmyOfDarkness` (S:558a0): skeletons and as many zombie slaves, three each below
/// Intelligence 250, four from 250 and five from 300.
pub fn magic_summon_army_of_darkness(host: &mut dyn ScriptHost, caster: EntityId) {
    gothic_portcov::hit!("magic_summon_army_of_darkness");
    let int = stats::attribute(host, caster, crate::host::Attribute::Intelligence);
    let n = army_size(int, 3, 4, 5);
    for _ in 0..n {
        summon_party_monster(host, caster, "Skeleton");
    }
    for _ in 0..n {
        summon_party_monster(host, caster, "SlaveZombie");
    }
}

/// Port of `MagicSummonArmyOfCuteness` (S:56950): rabbits, six below Intelligence 250, seven from 250 and
/// eight from 300.
pub fn magic_summon_army_of_cuteness(host: &mut dyn ScriptHost, caster: EntityId) {
    gothic_portcov::hit!("magic_summon_army_of_cuteness");
    let int = stats::attribute(host, caster, crate::host::Attribute::Intelligence);
    for _ in 0..army_size(int, 6, 7, 8) {
        summon_party_monster(host, caster, "Rabbit");
    }
}

/// Port of `MagicSummonMonster` (S:55a20): the template is what the caster's spell spawns;
/// the hero's version is a `Shadowbeast` when the spell has none. Characters other than the
/// hero keep what the template says.
pub fn magic_summon_monster(host: &mut dyn ScriptHost, caster: EntityId) {
    gothic_portcov::hit!("magic_summon_monster");
    let spell = host.spell(&host.cast_spell(caster));
    let template = spell
        .map(|s| s.spawn)
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "Shadowbeast".into());
    summon_party_monster(host, caster, &template);
}

/// Port of `MagicSummonCompanion` (S:55b90): the creature named by the spell's `Spawn`.
pub fn magic_summon_companion(host: &mut dyn ScriptHost, caster: EntityId) {
    gothic_portcov::hit!("magic_summon_companion");
    let spell = host.spell(&host.cast_spell(caster));
    if let Some(s) = spell.filter(|s| !s.spawn.is_empty()) {
        summon_party_monster(host, caster, &s.spawn);
    }
}

// ---- movement, shape, locks -------------------------------------------------------------------------

/// Port of `MagicTeleport` (S:573d0): the hero puts his hands down and stands; his party comes
/// along, controlled members are released to their routines, every misted character is freed,
/// and he is moved to the waypoint named by the spell without its `Spl_` prefix.
pub fn magic_teleport(host: &mut dyn ScriptHost, caster: EntityId) {
    gothic_portcov::hit!("magic_teleport");
    let spell = host.cast_spell(caster);
    let destination = spell.strip_prefix("Spl_").unwrap_or(&spell).to_string();
    let right = host.find_stack_index(caster, Slot::RightHand);
    host.hold_stacks(caster, right, -1);
    host.set_ani_state(caster, crate::enums::AniState::Stand);
    for member in host.party_members(caster) {
        if member == caster {
            continue;
        }
        match host.party_member_type(member) {
            PartyMemberType::Controlled => {
                host.set_party_leader(member, EntityId::NONE);
                host.full_stop(member);
                continue_routine(host, member);
            }
            PartyMemberType::Summoned | PartyMemberType::None => {}
            _ => host.teleport(member, &destination),
        }
    }
    for npc in host.all_npcs() {
        host.set_status_bits(npc, STATUS_CONTROLLED, false);
    }
    host.teleport(caster, &destination);
    host.emit_magic(MagicEvent::Teleport {
        who: caster,
        to: destination,
    });
}

/// Port of `SendPerceptionTransform` (S:53f40): everybody who can notice the change and had the
/// old body as its current or alternative target now has the new one (the DLL also moves the
/// alignment target, the look-at of the engine, which the host does with `set_look_at`).
pub fn send_perception_transform(host: &mut dyn ScriptHost, old: EntityId, new: EntityId) {
    gothic_portcov::hit!("send_perception_transform");
    for npc in host.all_npcs() {
        if !crate::perception::can_receive_perception_from(host, npc, old, false) {
            continue;
        }
        if host.current_target(npc) == old {
            host.set_current_target(npc, new);
        }
        if host.alternative_target(npc) == old {
            host.set_alternative_target(npc, new);
        }
    }
}

/// Port of `MagicTransform` (S:57880): the hero becomes the creature of the spell's `Spawn`
/// (a druid stone or a potion) for [`transform_duration`] seconds; the hands are emptied
/// (the stone itself stays in the right hand when it is an artefact).
pub fn magic_transform(host: &mut dyn ScriptHost, caster: EntityId) {
    gothic_portcov::hit!("magic_transform");
    if caster != host.player() {
        return;
    }
    let Some(spell) = host.spell(&host.cast_spell(caster)) else {
        return;
    };
    if spell.spawn.is_empty() {
        return;
    }
    let form = host.spawn_npc(&spell.spawn, caster);
    if form.is_none() {
        return;
    }
    let secs = transform_duration(host, caster, form);
    send_perception_transform(host, caster, form);
    host.begin_transform(caster, form, secs);
    host.emit_magic(MagicEvent::Transform {
        who: caster,
        template: spell.spawn,
        secs,
    });
}

/// Port of `MagicAnimalForm` (S:53bb0): another character (the focus) that passes
/// `CanTransformInto` is turned into the spell's animal for the transform duration; the caster
/// puts his hands down and goes back to standing.
pub fn magic_animal_form(host: &mut dyn ScriptHost, caster: EntityId, target: EntityId) {
    gothic_portcov::hit!("magic_animal_form");
    if caster == host.player() && target.is_none() {
        return;
    }
    let Some(spell) = host.spell(&host.cast_spell(caster)) else {
        return;
    };
    if spell.spawn.is_empty() || !can_transform_into(host, caster, target) {
        return;
    }
    let form = host.spawn_npc(&spell.spawn, target);
    if form.is_none() {
        return;
    }
    let secs = transform_duration(host, caster, form);
    send_perception_transform(host, target, form);
    host.begin_transform(target, form, secs);
    let right = host.find_stack_index(caster, Slot::RightHand);
    host.hold_stacks(caster, right, -1);
    host.emit_magic(MagicEvent::Transform {
        who: target,
        template: spell.spawn,
        secs,
    });
}

/// Port of `TransformBack` (S:58610): the transformation ends — the hero (or the animal-formed
/// character) takes his own shape back at the animal's place, the alien body is removed.
pub fn transform_back(host: &mut dyn ScriptHost, who: EntityId) {
    gothic_portcov::hit!("transform_back");
    let form = host.transform_form(who);
    if form.is_some() {
        send_perception_transform(host, form, who);
    }
    host.end_transform(who);
    host.emit_magic(MagicEvent::Transform {
        who,
        template: String::new(),
        secs: 0.0,
    });
    if who == host.player() {
        continue_routine(host, who);
    }
}

/// Port of `MagicOpenLocks` (S:56c40): the focused lock opens when it has no key (the spell
/// opens ordinary locks), or when its difficulty is below 5 and the caster... [I]: a lock of
/// difficulty 1-4 opens, a keyed lock of difficulty 5 or more refuses (`Impossible`).
pub fn magic_open_locks(host: &mut dyn ScriptHost, caster: EntityId, object: EntityId) {
    gothic_portcov::hit!("magic_open_locks");
    match host.lock_difficulty(object) {
        Some(d) if d < 5 => {
            host.unlock_object(object);
            host.emit_magic(MagicEvent::Unlocked { object });
        }
        Some(_) => player_message(host, caster, "Impossible"),
        None => {}
    }
}

/// Port of `MagicSoulTravel` (S:56440): the hero leaves his body: a ghost copy takes his place
/// (status 0x80), he becomes the ghost (status 0x200 marks the real body).
pub fn magic_soul_travel(host: &mut dyn ScriptHost, caster: EntityId) {
    gothic_portcov::hit!("magic_soul_travel");
    if caster != host.player() {
        return;
    }
    host.set_status_bits(caster, STATUS_GHOST, true);
    host.set_status_bits(caster, STATUS_OUT_OF_BODY, true);
    host.emit_magic(MagicEvent::Status {
        target: caster,
        mask: STATUS_GHOST | STATUS_OUT_OF_BODY,
        on: true,
    });
}

/// Port of `ReturnToBody` (S:58470): the soul returns, the ghost marks are cleared.
pub fn return_to_body(host: &mut dyn ScriptHost, who: EntityId) {
    gothic_portcov::hit!("return_to_body");
    host.set_status_bits(who, STATUS_OUT_OF_BODY, false);
    host.set_status_bits(who, STATUS_GHOST, false);
    host.emit_magic(MagicEvent::Status {
        target: who,
        mask: STATUS_GHOST | STATUS_OUT_OF_BODY,
        on: false,
    });
    continue_routine(host, who);
}

/// Port of `MagicTelekinesis` (S:56170): toggles the focused mover (levers) or lifts the
/// focused loose item toward the caster; the host moves it.
pub fn magic_telekinesis(host: &mut dyn ScriptHost, caster: EntityId, target: EntityId) {
    gothic_portcov::hit!("magic_telekinesis");
    host.telekinesis(caster, target);
}

/// Port of `MagicTimeBubble` (S:566b0): the spell's `Smn_Timebubble` entity does the work; the
/// function itself only exists to be called.
pub fn magic_time_bubble(_host: &mut dyn ScriptHost, _caster: EntityId) {
    gothic_portcov::hit!("magic_time_bubble");
}

/// Run a spell function by the name a spell template stores in `FuncOnCast`/`FuncOnTargetHit`.
/// Returns false when the name is not a function of the script library.
pub fn call_spell_function(
    host: &mut dyn ScriptHost,
    name: &str,
    caster: EntityId,
    target: EntityId,
    stack: i32,
) -> bool {
    use crate::magic::cast::{magic_instant, magic_projectile, magic_summon};
    match name {
        "MagicInstant" => magic_instant(host, caster, target, stack),
        "MagicProjectile" => magic_projectile(host, caster, target, stack),
        "MagicSummon" => magic_summon(host, caster, target, stack),
        "MagicBurn" => magic_burn(host, caster, target),
        "MagicFreeze" => magic_freeze(host, caster, target),
        "MagicPoison" => magic_poison(host, caster, target),
        "MagicDisease" => magic_disease(host, caster, target),
        "MagicRefresh" => magic_refresh(host, caster),
        "MagicCure" => magic_cure(host, caster),
        "MagicCurePoison" => magic_cure_poison(host, caster),
        "MagicCureDisease" => magic_cure_disease(host, caster),
        "MagicHealOther" => magic_heal_other(host, target),
        "MagicKeepStamina" => magic_keep_stamina(host, caster),
        "MagicSpeedUp" => magic_speed_up(host, caster),
        "MagicStun" => magic_stun(host, caster),
        "MagicDispel" => magic_dispel(host, caster, DamageType::Ice),
        "MagicTameAnimal" => magic_tame_animal(host, caster, target),
        "MagicWordOfControl" => magic_word_of_control(host, caster, target),
        "MagicCharm" => magic_charm(host, caster, target),
        "MagicBloodlust" => magic_bloodlust(host, caster, target),
        "MagicFear" => magic_fear(host, caster, target),
        "MagicSleep" => magic_sleep(host, caster, target),
        "MagicHypnosis" => magic_hypnosis(host, caster),
        "MagicCallAnimals" => magic_call_animals(host, caster, target),
        "MagicSummonDemon" => magic_summon_demon(host, caster),
        "MagicSummonSkeleton" => magic_summon_skeleton(host, caster),
        "MagicSummonGolem" => magic_summon_golem(host, caster),
        "MagicSummonGoblin" => magic_summon_goblin(host, caster),
        "MagicSummonArmyOfDarkness" => magic_summon_army_of_darkness(host, caster),
        "MagicSummonArmyOfCuteness" => magic_summon_army_of_cuteness(host, caster),
        "MagicSummonMonster" => magic_summon_monster(host, caster),
        "MagicSummonCompanion" => magic_summon_companion(host, caster),
        "MagicTeleport" => magic_teleport(host, caster),
        "MagicTransform" => magic_transform(host, caster),
        "MagicAnimalForm" => magic_animal_form(host, caster, target),
        "MagicOpenLocks" => magic_open_locks(host, caster, target),
        "MagicSoulTravel" => magic_soul_travel(host, caster),
        "MagicTelekinesis" => magic_telekinesis(host, caster, target),
        "MagicTimeBubble" => magic_time_bubble(host, caster),
        _ => return false,
    }
    true
}

/// Is the target evil, for `Spl_DestroyEvil` (kept here next to the other species rules)?
pub fn is_evil_target(host: &dyn ScriptHost, target: EntityId) -> bool {
    host.exists(target) && is_evil(host, target)
}

/// The action a dispel of this element needs from the caster (kept for the state tables).
pub const fn dispel_action() -> Action {
    Action::Cast
}
