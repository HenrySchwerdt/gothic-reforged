//! `SelectCombatMove` (S:4ec30) and the archetype handlers `FAI_Active`, `FAI_Passive`,
//! `FAI_Ranged`, `FAI_Magic`, `FAI_Misted`: which move does the NPC make next?
//!
//! The return value is the `gEAction` of the chosen move; [`Action::None`] means "no decision"
//! (the attack loop then just idles).

use super::moves::{build_situation, can_do, choose_defensive, choose_full, choose_offensive};
use super::range::attack_range;
use crate::enums::Slot;
use crate::enums::{Action, AiMode, AniState, AttackReason, Attitude, Species};
use crate::helpers::{WeaponMode, weapon_mode};
use crate::host::{EntityId, ScriptHost};
use crate::predicates::is_humanoid;
use crate::routine::state::State;

/// Status-effect bit of a mind-controlled ("misted") NPC.
pub const STATUS_MISTED: u32 = 0x40;

/// Weapon mode of `npc` (S:3240 applied to the hand slots).
pub fn npc_weapon_mode(host: &dyn ScriptHost, npc: EntityId) -> WeaponMode {
    weapon_mode(
        host.slot_use_type(npc, Slot::LeftHand),
        host.slot_use_type(npc, Slot::RightHand),
    )
}

/// Port of `SelectCombatMove` (S:4ec30): dispatch by state of mind and weapon.
///
/// Downed or dead fighters make no move; a misted one uses [`fai_misted`]; magic and ranged
/// weapons use their handlers; melee fighters follow `CombatState` (0 passive, 1 active).
pub fn select_combat_move(host: &mut dyn ScriptHost, me: EntityId, target: EntityId) -> Action {
    gothic_portcov::hit!("select_combat_move");
    if matches!(host.ai_mode(me), AiMode::Down | AiMode::Dead) {
        return Action::None;
    }
    if host.has_status(me, STATUS_MISTED) {
        return fai_misted(host, me, target);
    }
    match npc_weapon_mode(host, me) {
        WeaponMode::Magic => fai_magic(host, me, target),
        WeaponMode::Ranged => fai_ranged(host, me, target),
        _ => match host.combat_state(me) {
            0 => fai_passive(host, me, target),
            1 => fai_active(host, me, target),
            _ => Action::None,
        },
    }
}

/// Species that fight with the full tactic table (humanoids and the monsters that wield
/// weapons): Human, Skeleton, Zombie, Demon, Orc, Minecrawler, Ogre, ScorpionKing, Stalker.
pub const fn uses_humanoid_tactics(s: Species) -> bool {
    matches!(
        s,
        Species::Human
            | Species::Skeleton
            | Species::Zombie
            | Species::Demon
            | Species::Orc
            | Species::Minecrawler
            | Species::Ogre
            | Species::ScorpionKing
            | Species::Stalker
    )
}

/// Port of `FAI_Active` (S:4ee00): the engaged fighter.
///
/// A target that sits knocked down is given room (back off below 5 m, approach beyond 6 m,
/// wait between). Weapon-wielding species choose through the move score
/// ([`choose_full`]); everything else (animals) runs [`animal_move`]. The `!AIMode` path of
/// the unpatched game and the Gargoyle are folded into the animal behaviour [I].
pub fn fai_active(host: &mut dyn ScriptHost, me: EntityId, target: EntityId) -> Action {
    gothic_portcov::hit!("fai_active");
    let dist = host.distance(me, target);
    let species = host.species(me);
    let patched = host.setup().ai_mode && species != Species::Gargoyle;
    if patched && host.ani_state(target) == AniState::SitKnockDown {
        if dist >= 500.0 {
            return if dist > 600.0 {
                Action::Fwd
            } else {
                Action::Wait
            };
        }
        return if can_do(host, me, target, Action::JumpBack) {
            Action::JumpBack
        } else {
            Action::Back
        };
    }
    if patched && uses_humanoid_tactics(species) {
        return choose_full(host, me, target).unwrap_or(Action::Wait);
    }
    animal_move(host, me, target)
}

/// The fighting style of animals and monsters without weapon tactics (the long probability tree
/// of S:4ee00 behind the species test): chain sprint into power attack, strike when in reach,
/// otherwise run in. Reconstructed from the branch structure [I].
fn animal_move(host: &mut dyn ScriptHost, me: EntityId, target: EntityId) -> Action {
    let c = build_situation(host, me, target);
    if c.target_is_player
        && (host.current_action(me) == Action::SprintAttack
            || (c.mode != 1 && host.last_hit_timestamp(me) >= 120))
    {
        return Action::SprintAttack;
    }
    if host.current_state(me) == Some(State::AttackSprint) && !c.in_melee_range {
        return Action::Move;
    }
    if c.in_melee_range {
        if host.random(100) < 50 && can_do(host, me, target, Action::PowerAttack) {
            return Action::PowerAttack;
        }
        if can_do(host, me, target, Action::Attack) {
            return Action::Attack;
        }
        if can_do(host, me, target, Action::QuickAttack) {
            return Action::QuickAttack;
        }
        return Action::PowerAttack;
    }
    let dist = host.distance(me, target);
    let reach = attack_range(host, me, target, Action::Attack) as f32;
    if dist > reach + 75.0 && can_do(host, me, target, Action::Move) {
        return Action::Move;
    }
    Action::Fwd
}

/// Port of `FAI_Passive` (S:4ffb0): a fighter that has not been admitted to the front line
/// keeps 3.5 to 4.5 m from its target: back off when closer, approach when farther, otherwise
/// circle (turn left, else turn right, else wait).
///
/// The picks the DLL tries first (defensive when the target swings, offensive when in reach)
/// are evaluated without a situation snapshot and always come back empty, so only the range
/// rule is observable.
pub fn fai_passive(host: &mut dyn ScriptHost, me: EntityId, target: EntityId) -> Action {
    gothic_portcov::hit!("fai_passive");
    let dist = host.distance(me, target);
    let theirs = host.current_action(target);
    if super::moves::is_attack_action(theirs) {
        let reach = attack_range(host, target, me, theirs) as f32;
        if reach >= dist
            && let Some(m) = choose_defensive(host, me, target)
        {
            return m;
        }
    }
    let mine = host.current_action(me);
    let reach = attack_range(host, me, target, mine) as f32;
    if reach >= dist
        && los_for_attack(host, me, target, dist)
        && let Some(m) = choose_offensive(host, me, target)
    {
        return m;
    }
    if dist < 350.0 {
        return Action::Back;
    }
    if dist > 450.0 {
        return Action::Fwd;
    }
    if can_do(host, me, target, Action::TurnLeft) {
        return Action::TurnLeft;
    }
    if can_do(host, me, target, Action::TurnRight) {
        return Action::TurnRight;
    }
    Action::Wait
}

/// Port of S:4e8d0: beyond 2 m the line of sight is checked at knee height, closer by the
/// ordinary line.
fn los_for_attack(host: &dyn ScriptHost, me: EntityId, target: EntityId, dist: f32) -> bool {
    gothic_portcov::hit!("los_for_attack");
    if dist > 200.0 {
        host.has_free_los_knee(me, target)
    } else {
        host.has_free_los(me, target)
    }
}

/// Port of `FAI_Ranged` (S:50370): beyond 20 m run in; without a line of sight look for a new
/// target; then shoot.
pub fn fai_ranged(host: &mut dyn ScriptHost, me: EntityId, target: EntityId) -> Action {
    gothic_portcov::hit!("fai_ranged");
    if host.distance(me, target) > 2000.0 {
        return Action::Fwd;
    }
    if !host.has_free_los(me, target) {
        let other = find_other_target(host, me);
        if other.is_some() {
            let reason = host.attack_reason(me);
            crate::crime::set_attack_reason(host, me, other, reason);
            host.set_current_target(me, other);
            host.set_combat_state(me, 0);
        }
    }
    Action::Shoot
}

/// Port of `FAI_Magic` (S:50550): spells that need no aim (`focus`) retreat inside 3.5 m (eight
/// times in ten), cast between 3.5 and 11.75 m and approach beyond; aimed spells are cast with a
/// line of sight (80 percent of the time, always for Xardas), otherwise the mage looks for
/// another target or approaches.
pub fn fai_magic(host: &mut dyn ScriptHost, me: EntityId, target: EntityId) -> Action {
    gothic_portcov::hit!("fai_magic");
    let dist = host.distance(me, target);
    let min_hit = host.setup().min_hit_duration;
    if host.spell_targets_focus(me) {
        if dist < 350.0 {
            if host.can_evade(me, target, crate::host::dir::BACK) && host.random(10) < 8 {
                return Action::Back;
            }
            return if host.can_hit(me, min_hit) {
                Action::Cast
            } else {
                Action::Wait
            };
        }
        if dist > 2000.0 {
            return Action::Fwd;
        }
        if dist <= 1175.0 {
            return if host.can_hit(me, min_hit) {
                Action::Cast
            } else {
                Action::Wait
            };
        }
        return Action::Fwd;
    }
    if !host.has_free_los_knee(me, target) {
        let other = find_other_target(host, me);
        if other.is_some() {
            let reason = host.attack_reason(me);
            crate::crime::set_attack_reason(host, me, other, reason);
            host.set_current_target(me, other);
            host.set_combat_state(me, 0);
            return Action::Fwd;
        }
        return Action::Fwd;
    }
    let sure = host.setup().extended_content && host.name(me) == "Xardas";
    if (sure || host.random(100) < 80) && host.can_hit(me, min_hit) {
        return Action::Cast;
    }
    Action::Wait
}

/// Port of `FAI_Misted` (S:50a20): a mind-controlled NPC casts or shoots within 7 m and
/// otherwise fights like an engaged melee fighter.
pub fn fai_misted(host: &mut dyn ScriptHost, me: EntityId, target: EntityId) -> Action {
    gothic_portcov::hit!("fai_misted");
    let dist = host.distance(me, target);
    match npc_weapon_mode(host, me) {
        WeaponMode::Magic if dist < 700.0 => return Action::Cast,
        WeaponMode::Ranged if dist < 700.0 => return Action::Shoot,
        _ => {}
    }
    let reach = attack_range(host, me, target, host.current_action(me)) as f32;
    if reach >= dist && los_for_attack(host, me, target, dist) {
        let pick = if host.setup().ai_mode {
            choose_full(host, me, target)
        } else {
            choose_offensive(host, me, target)
        };
        if let Some(m) = pick {
            return m;
        }
    }
    Action::Wait
}

/// Port of the target search helper (S:a070): another enemy worth fighting instead of the
/// current target, `NONE` when there is none.
///
/// A waiting companion looks for nothing. Otherwise the nearest NPC within 25 m that is awake,
/// dry, hostile or predatory toward the searcher, in view and fighting the searcher (or the
/// player when the searcher is a companion) qualifies [I: the DLL walks a 2.7 KB filter chain;
/// this keeps its structure: distance, mode, attitude, line of sight, who it fights].
pub fn find_other_target(host: &mut dyn ScriptHost, me: EntityId) -> EntityId {
    gothic_portcov::hit!("find_other_target");
    if host.party_waiting(me) {
        return EntityId::NONE;
    }
    let current = host.current_target(me);
    let player = host.player();
    let companion = host.party_leader(me) == player && player.is_some();
    let mut best = (f32::MAX, EntityId::NONE);
    for n in host.all_npcs() {
        if n == me || n == current || n == player {
            continue;
        }
        let mode = host.ai_mode(n);
        if matches!(
            mode,
            AiMode::Talk | AiMode::Down | AiMode::Dead | AiMode::Abandoned
        ) {
            continue;
        }
        if host.party_waiting(n) || host.is_swimming(n) {
            continue;
        }
        let d = host.distance(me, n);
        if d >= 2500.0 {
            continue;
        }
        let att = crate::attitude::get_attitude(host, me, n);
        if !matches!(att, Attitude::Hostile | Attitude::Predator) {
            continue;
        }
        if !host.has_free_los(me, n) {
            continue;
        }
        let fights_us = host.current_target(n) == me
            || (companion && host.current_target(n) == player)
            || host.ai_mode(n) == AiMode::Combat && host.current_target(n) == current;
        if !fights_us {
            continue;
        }
        if d < best.0 {
            best = (d, n);
        }
    }
    best.1
}

/// Is the opponent a humanoid (used by the move predicate for runners).
pub fn target_is_humanoid(host: &dyn ScriptHost, target: EntityId) -> bool {
    is_humanoid(host, target)
}

/// Reason ids at or above which `OnAttack` never retargets: Prey, DriveAway, Duel, Arena, Kill.
pub fn reason_keeps_target(r: AttackReason) -> bool {
    matches!(
        r,
        AttackReason::Prey
            | AttackReason::DriveAway
            | AttackReason::Duel
            | AttackReason::Arena
            | AttackReason::Kill
    )
}
