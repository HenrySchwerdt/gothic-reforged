//! Combat-move selection for humanoid fighters: `CanDo` (S:4f80), the situation snapshot
//! (S:7460, S:6b30, S:74e0), the move score (S:57d0), the shortlist bookkeeping (S:6560, S:63c0)
//! and the drivers (S:67c0, S:6e60, S:7030).
//!
//! The DLL keeps the snapshot and the candidate lists in one static record; here it is the
//! [`Situation`] value built per decision. The score is a decision table: the snapshot picks a
//! branch, the move picks a class inside the branch (the DLL's jump tables, kept as the `SW_*`
//! arrays), and the class adds a constant (45, 55, 65, 70, 75 or the aggression weight `W`).

use super::range::attack_range;
use crate::config::Difficulty;
use crate::damage::can_parade_move_of;
use crate::enums::{Action, AiMode, AniState, AttackReason, NpcClass, Slot, Species, UseType};
use crate::helpers::{WeaponMode, weapon_mode};
use crate::host::{EntityId, ScriptHost, dir};
use crate::predicates::is_humanoid;
use crate::stats;

/// Margin of the "inside melee range" flag of the snapshot (centimetres).
const NEAR_MARGIN: f32 = 25.0;

/// Moves the DLL's table at S:4f50 calls attacks: Attack, Power, the three quick moves, simple
/// whirl, sprint, whirl, pierce and hack.
pub const fn is_attack_action(a: Action) -> bool {
    matches!(
        a,
        Action::Attack
            | Action::PowerAttack
            | Action::QuickAttack
            | Action::QuickAttackR
            | Action::QuickAttackL
            | Action::SimpleWhirl
            | Action::SprintAttack
            | Action::WhirlAttack
            | Action::PierceAttack
            | Action::HackAttack
    )
}

/// The situation snapshot of one decision (the DLL's static `sCombatCtx`).
#[derive(Debug, Clone)]
pub struct Situation {
    /// The opponent is mid-attack (`+0x40`).
    pub target_attacking: bool,
    /// Free line of sight to the opponent (`+0x41`).
    pub los: bool,
    /// The opponent is the player (`+0x42`).
    pub target_is_player: bool,
    /// Inside melee range minus 25 (`+0x43`).
    pub in_melee_range: bool,
    /// The opponent's attack currently reaches me (`+0x44`).
    pub reached: bool,
    /// Snapshot was taken (both entities exist) (`+0x45`).
    pub ready: bool,
    /// The player faces me (`+0x46`).
    pub facing_me: bool,
    /// The opponent is a mage (`+0x47`).
    pub target_is_mage: bool,
    /// 1: several attackers on one target (approach), 2: engaged one-on-one (`+0x48`).
    pub mode: u8,
    /// Shortlist length limit (`+0x38`).
    pub max_keep: i32,
    /// Aggression weight `W` (`+0x3c`).
    pub weight: i32,
}

impl Situation {
    /// The state after S:7460: nothing known yet.
    pub fn blank() -> Self {
        Self {
            target_attacking: false,
            los: true,
            target_is_player: false,
            in_melee_range: false,
            reached: false,
            ready: false,
            facing_me: false,
            target_is_mage: false,
            mode: 3,
            max_keep: 3,
            weight: 5,
        }
    }
}

/// Port of the snapshot builder (S:6b30 with the weight of S:74e0, preceded by the reset
/// S:7460).
pub fn build_situation(host: &mut dyn ScriptHost, me: EntityId, target: EntityId) -> Situation {
    let mut c = Situation::blank();
    if me.is_none() || target.is_none() {
        return c;
    }
    c.ready = true;
    let dist = host.distance(me, target);
    c.mode = 2;
    c.target_is_mage = host.npc_class(target) == NpcClass::Mage;
    c.target_is_player = target == host.player();
    let target_action = host.current_action(target);
    c.target_attacking = is_attack_action(target_action);
    if c.target_attacking {
        let reach = attack_range(host, target, me, target_action) as f32;
        c.reached = reach >= dist;
    }
    let melee = attack_range(host, me, target, Action::Attack) as f32 - NEAR_MARGIN;
    c.in_melee_range = melee >= dist;
    if crowd_on_target(host, target) {
        c.mode = 1;
    }
    if c.mode == 1 {
        let mine = if c.target_is_player {
            host.focus_entity() == me || host.current_attacker(target) == me
        } else {
            host.current_target(target) == me
        };
        if mine {
            c.mode = 2;
        }
    }
    if host.is_in_fov(target, me) && c.target_is_player {
        c.facing_me = true;
    }
    if !host.has_free_los(me, target) {
        c.los = false;
    }
    c.weight = aggression_weight(host, me, &c);
    c
}

/// Port of S:19a0: more than one NPC fights `target` at the same time.
fn crowd_on_target(host: &dyn ScriptHost, target: EntityId) -> bool {
    let player = host.player();
    let count = host
        .all_npcs()
        .into_iter()
        .filter(|&n| {
            n != player
                && host.combat_state(n) == 1
                && host.ai_mode(n) == AiMode::Combat
                && host.current_target(n) == target
        })
        .count();
    count > 1
}

/// Port of the aggression weight (S:74e0): 5, plus 20/10/0 for easy/normal/hard difficulty,
/// five more (minimum 20) for level-1 fighters or minus 5 above level 30 and again above 70,
/// minus 10 in the arena, minus 5 against a mage, minus 10 for a companion of the player.
pub fn aggression_weight(host: &dyn ScriptHost, me: EntityId, c: &Situation) -> i32 {
    let mut w = 5;
    w += match host.difficulty() {
        Difficulty::Easy => 20,
        Difficulty::Normal => 10,
        Difficulty::Hard => 0,
    };
    let level = host.level_raw(me);
    if level < 2 {
        w += 5;
        w = w.max(20);
    } else {
        if level > 30 {
            w -= 5;
        }
        if level > 70 {
            w -= 5;
        }
    }
    if host.attack_reason(me) == AttackReason::Arena {
        w -= 10;
    }
    if c.target_is_mage {
        w -= 5;
    }
    let player = host.player();
    if player.is_some() && host.party_leader(me) == player {
        w -= 10;
    }
    w.max(0)
}

fn species_is_foot_fighter(s: Species) -> bool {
    matches!(
        s,
        Species::Human | Species::Skeleton | Species::Zombie | Species::Orc
    )
}

/// Port of S:1fa0: species that can sidestep.
fn species_can_sidestep(s: Species) -> bool {
    use Species as S;
    matches!(
        s,
        S::Human
            | S::Skeleton
            | S::Zombie
            | S::Golem
            | S::Demon
            | S::Orc
            | S::Goblin
            | S::Troll
            | S::Minecrawler
            | S::Varan
            | S::FireVaran
            | S::Bloodfly
            | S::Ogre
            | S::FireGolem
            | S::IceGolem
            | S::ScorpionKing
            | S::Stalker
    )
}

/// Use types the stack check S:30c0 accepts as two-handed weapons.
const fn is_two_hander(u: UseType) -> bool {
    matches!(
        u,
        UseType::TwoH | UseType::Staff | UseType::Halberd | UseType::Axe
    )
}

/// Tools the swing check S:3140 accepts (broom, rake, shovel, ...).
const fn is_swing_tool(u: UseType) -> bool {
    matches!(
        u,
        UseType::Broom | UseType::Rake | UseType::Shovel | UseType::Pickaxe
    )
}

/// Port of `CanDo` (S:4f80): may `me` perform move `mv` against `target` right now?
///
/// Attack moves need `CanHit(MinHitDuration)` for NPCs; each move has its own equipment and
/// species rules (a pierce needs a one-hander in the right hand, a whirl a two-hander, a
/// simple whirl a one-hander in each hand, ...).
pub fn can_do(host: &dyn ScriptHost, me: EntityId, target: EntityId, mv: Action) -> bool {
    if me != host.player()
        && is_attack_action(mv)
        && !host.can_hit(me, host.setup().min_hit_duration)
    {
        return false;
    }
    let species = host.species(me);
    let right = host.slot_use_type(me, Slot::RightHand);
    let left = host.slot_use_type(me, Slot::LeftHand);
    use Action as A;
    match mv {
        A::Attack => {
            !(host.setup().ai_mode && matches!(species, Species::Lurker | Species::SwampLurker))
        }
        A::PowerAttack | A::Fwd => true,
        A::QuickAttack | A::QuickAttackR | A::QuickAttackL => {
            use Species as S;
            match species {
                S::Human
                | S::Skeleton
                | S::Zombie
                | S::Demon
                | S::Orc
                | S::Goblin
                | S::Ogre
                | S::Stalker => !(left == UseType::None && right == UseType::Fist),
                S::Troll
                | S::Minecrawler
                | S::Scavenger
                | S::Sabertooth
                | S::Lurker
                | S::Snapper
                | S::Trex
                | S::SwampLurker
                | S::ScorpionKing => true,
                _ => false,
            }
        }
        A::SimpleWhirl => left == UseType::OneH && right == UseType::OneH,
        A::TurnLeft | A::TurnRight => {
            if matches!(
                species,
                Species::Troll
                    | Species::Golem
                    | Species::IceGolem
                    | Species::FireGolem
                    | Species::Goblin
            ) {
                return false;
            }
            species_can_sidestep(species)
                && host.can_evade(
                    me,
                    target,
                    if mv == A::TurnLeft {
                        dir::STRAFE_LEFT
                    } else {
                        dir::STRAFE_RIGHT
                    },
                )
        }
        A::Left | A::Right => {
            species_can_sidestep(species)
                && host.can_evade(
                    me,
                    target,
                    if mv == A::Left {
                        dir::STRAFE_LEFT
                    } else {
                        dir::STRAFE_RIGHT
                    },
                )
        }
        A::WhirlAttack => species_is_foot_fighter(species) && is_two_hander(right),
        A::PierceAttack => species_is_foot_fighter(species) && right == UseType::OneH,
        A::HackAttack => {
            species_is_foot_fighter(species) && (is_two_hander(right) || is_swing_tool(right))
        }
        A::FinishingAttack => {
            species_is_foot_fighter(species)
                && (is_two_hander(right) || is_swing_tool(right) || right == UseType::OneH)
        }
        A::Parade => {
            if !matches!(
                species,
                Species::Human
                    | Species::Skeleton
                    | Species::Zombie
                    | Species::Orc
                    | Species::Minecrawler
            ) {
                return false;
            }
            if target.is_none() {
                return true;
            }
            if host.ani_state(target) == AniState::Parade {
                return false;
            }
            can_parade_move_of(host, me, target)
        }
        A::JumpBack => {
            if species_is_foot_fighter(species) {
                return false;
            }
            host.can_evade(me, target, dir::BACK)
        }
        A::Back => host.can_evade(me, target, dir::BACK),
        A::Move => {
            if !host.setup().ai_mode {
                return false;
            }
            use Species as S;
            let allowed = !matches!(
                species,
                S::Human
                    | S::Skeleton
                    | S::Zombie
                    | S::Demon
                    | S::Orc
                    | S::Minecrawler
                    | S::Wolf
                    | S::Bloodfly
            );
            if !allowed {
                return false;
            }
            if target.is_some() && is_humanoid(host, target) {
                return false;
            }
            weapon_mode(left, right) == WeaponMode::Melee
        }
        _ => false,
    }
}

// ---- score tables (the DLL's jump tables, indexed by move id - 1 unless noted) ----------------

const SW_A: [u8; 53] = [
    0, 3, 0, 3, 3, 0, 1, 1, 3, 0, 3, 3, 3, 3, 3, 1, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3,
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 2,
];
const SW_B: [u8; 14] = [0, 1, 0, 0, 0, 0, 2, 2, 2, 0, 1, 2, 1, 1];
const SW_C: [u8; 53] = [
    0, 2, 0, 2, 2, 0, 1, 1, 2, 0, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1,
];
const SW_D: [u8; 52] = [
    0, 0, 2, 2, 2, 2, 1, 1, 2, 0, 0, 2, 0, 0, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1,
];
const SW_E: [u8; 49] = [
    0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0,
];
const SW_F: [u8; 54] = [
    0, 0, 4, 4, 4, 4, 4, 4, 4, 1, 1, 1, 4, 2, 2, 2, 2, 3, 2, 3, 3, 3, 4, 4, 1, 4, 4, 3, 4, 4, 4, 4,
    4, 2, 2, 2, 4, 4, 4, 4, 4, 2, 2, 2, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0,
];
const SW_G: [u8; 53] = [
    0, 2, 0, 2, 2, 0, 1, 1, 2, 0, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1,
];
const SW_H: [u8; 52] = [
    0, 0, 2, 2, 2, 2, 1, 1, 2, 2, 0, 2, 2, 0, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1,
];
const SW_I: [u8; 46] = [
    0, 0, 3, 3, 1, 3, 3, 1, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3,
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 2,
];
const SW_J: [u8; 53] = [
    0, 0, 0, 2, 2, 2, 1, 1, 2, 0, 0, 2, 2, 0, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1,
];
const SW_L: [u8; 49] = [
    0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0,
];
const SW_M: [u8; 53] = [
    0, 5, 1, 5, 5, 1, 2, 2, 5, 3, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 4,
];

/// Look a jump table up; indices past the end take the DLL's out-of-range class `fallback`.
fn class(table: &[u8], index: i32, fallback: u8) -> u8 {
    if index < 0 {
        return fallback;
    }
    table.get(index as usize).copied().unwrap_or(fallback)
}

/// The move is one of the first fourteen ids and an attack (`idx <= 0xd && sub_4f50`).
fn is_attack_id(mv: Action) -> bool {
    mv.raw() <= 14 && is_attack_action(mv)
}

/// Port of the move score (S:57d0, the `AIMode` branch): how attractive is `mv` in `c`?
/// Zero means "not a candidate".
///
/// Out of melee range only approaching scores (85). In reach the score starts at -35 for a
/// repeated move or an attack right after one, then the branch picked by the engagement mode
/// and by what the opponent is doing adds 45 to 75 (or `W`) per move class.
pub fn score_move(
    host: &dyn ScriptHost,
    me: EntityId,
    target: EntityId,
    c: &mut Situation,
    mv: Action,
) -> i32 {
    if !can_do(host, me, target, mv) {
        return 0;
    }
    if !host.setup().ai_mode || host.species(me) == Species::Gargoyle {
        return legacy_score(host, mv);
    }
    if !c.ready {
        return 0;
    }
    let idx = mv.raw() as i32 - 1;
    let target_action = host.current_action(target);
    // Out of reach: only approaching (or, against a mage, backing off) makes sense.
    let normal_scoring =
        c.target_attacking && c.reached && c.mode != 1 && (!c.target_is_player || c.facing_me);
    if !normal_scoring && !c.in_melee_range {
        if c.target_is_mage && c.mode == 1 && can_do(host, me, target, Action::Back) {
            return if mv == Action::Back { 85 } else { 0 };
        }
        return if mv == Action::Fwd { 85 } else { 0 };
    }
    let mut s = 0;
    if (is_attack_id(mv) && !host.can_hit(me, 30)) || mv == host.current_action(me) {
        s = -35;
    }
    let w = c.weight;
    match c.mode {
        2 => {
            if c.target_attacking {
                if c.reached && (!c.target_is_player || c.facing_me) {
                    c.max_keep = c.max_keep.max(12);
                    match class(&SW_A, idx, 3) {
                        0 => {
                            if c.in_melee_range {
                                s += 65;
                            }
                        }
                        1 => s += 65,
                        2 => s += w + 45,
                        _ => {
                            if (is_attack_id(mv) && c.in_melee_range) || mv == Action::Fwd {
                                s += w;
                            }
                        }
                    }
                } else if c.los {
                    if !c.in_melee_range {
                        return s;
                    }
                    c.max_keep = c.max_keep.max(12);
                    let ta = target_action.raw() as i32 - 1;
                    match class(&SW_B, ta, 2) {
                        0 => match class(&SW_C, idx, 2) {
                            0 => s += 75,
                            1 => s += w,
                            _ => {
                                if is_attack_id(mv) {
                                    s += 45 + w;
                                }
                            }
                        },
                        1 => match class(&SW_D, idx, 2) {
                            0 => s += 75,
                            1 => s += w,
                            _ => {
                                if is_attack_id(mv) {
                                    s += w + 45;
                                }
                            }
                        },
                        _ => {
                            let m = mv.raw();
                            if m == 7 || m == 8 {
                                if !can_do(host, me, target, Action::Attack) {
                                    s += 55;
                                }
                                s += w;
                            } else if m == 0x34 {
                                s += w;
                            } else if is_attack_id(mv) {
                                s += 65;
                            }
                        }
                    }
                } else {
                    c.max_keep = c.max_keep.max(4);
                    if class(&SW_E, mv.raw() as i32 - 7, 1) == 0 {
                        s += 75;
                    }
                }
            } else if c.los {
                if !c.in_melee_range {
                    return s;
                }
                c.max_keep = c.max_keep.max(12);
                match class(&SW_F, target_action.raw() as i32 - 7, 4) {
                    0 => match class(&SW_J, idx, 2) {
                        0 => s += 75,
                        1 => s += w,
                        _ => {
                            if is_attack_id(mv) {
                                s += w + 65;
                            }
                        }
                    },
                    1 => match class(&SW_I, mv.raw() as i32 - 7, 3) {
                        0 => s += 45 + w,
                        1 => s += 75,
                        2 => s += w,
                        _ => {
                            if is_attack_id(mv) {
                                s += w + 45;
                            }
                        }
                    },
                    2 => match class(&SW_G, idx, 2) {
                        0 => s += 70,
                        1 => s += w,
                        _ => {
                            if is_attack_id(mv) {
                                s += w + 45;
                            }
                        }
                    },
                    3 => match class(&SW_H, idx, 2) {
                        0 => s += 75,
                        1 => s += w,
                        _ => {
                            if is_attack_id(mv) {
                                s += w + 45;
                            }
                        }
                    },
                    _ => {
                        let m = mv.raw();
                        if m == 7 || m == 8 {
                            if !can_do(host, me, target, Action::Attack) {
                                s += 55;
                            }
                            s += w;
                        } else if is_attack_id(mv) {
                            s += 65;
                        }
                    }
                }
            } else if class(&SW_L, mv.raw() as i32 - 7, 1) == 0 {
                s += 55;
            }
        }
        1 => {
            if c.los {
                if !c.in_melee_range {
                    return 0;
                }
                c.max_keep = c.max_keep.max(12);
                match class(&SW_M, idx, 5) {
                    0 => s += 65,
                    1 => s += 55,
                    2 => {
                        s += w + 40;
                        if !c.facing_me && c.target_is_player {
                            s -= 10;
                        }
                    }
                    3 => s += 45,
                    4 => s += w,
                    _ => {
                        if is_attack_id(mv) {
                            s += 45;
                        }
                    }
                }
            } else {
                let m = mv.raw();
                if (7..=8).contains(&m) || m == 0x35 {
                    s += 65;
                }
            }
        }
        _ => return 0,
    }
    s
}

/// The score of the unpatched game (`!AIMode`, and Gargoyles): `50 + player level / 2` for
/// the preferred moves. A stand-in [I]; the shipped configuration never takes it.
fn legacy_score(host: &dyn ScriptHost, mv: Action) -> i32 {
    let base = 50 + stats::level(host, host.player()) as i32 / 2;
    match mv {
        Action::Fwd | Action::Attack | Action::PowerAttack => base,
        _ => 0,
    }
}

/// The ordered candidate lists of one decision (S:6560 adds, S:63c0 finishes).
#[derive(Debug, Default, Clone)]
pub struct Shortlist {
    moves: Vec<Action>,
    scores: Vec<i32>,
    low: Vec<Action>,
    best: i32,
}

impl Shortlist {
    /// Port of S:6560: offer a scored move. Moves above the weight and within 35 of the best
    /// join the ordered shortlist; weaker ones wait in the secondary list.
    pub fn offer(&mut self, weight: i32, mv: Action, score: i32) {
        if score <= 0 {
            return;
        }
        if score >= self.best - 35 {
            if score <= weight {
                self.low.push(mv);
                return;
            }
            if self.moves.is_empty() {
                self.moves.push(mv);
                self.scores.push(score);
                self.best = score;
                return;
            }
            match self.scores.iter().position(|&s| s < score) {
                Some(i) => {
                    self.scores.insert(i, score);
                    self.moves.insert(i, mv);
                    if i == 0 {
                        self.best = score;
                        while let Some(&last) = self.scores.last() {
                            if last < self.best - 35 {
                                self.scores.pop();
                                self.moves.pop();
                            } else {
                                break;
                            }
                        }
                    }
                }
                None => {
                    self.scores.push(score);
                    self.moves.push(mv);
                }
            }
        } else if score <= weight {
            self.low.push(mv);
        }
    }

    /// Port of S:63c0: trim to `max_keep`, with probability `W/55` mix the weak moves in at
    /// the best score, then weight the bag (best 3, middle 2, lowest 1) and pick uniformly.
    pub fn pick(mut self, host: &mut dyn ScriptHost, c: &Situation) -> Option<Action> {
        let use_low = host.random(55) < c.weight;
        let mut i = c.max_keep.max(0) as usize;
        // The DLL trims only the move list (the score list keeps its length), so the pairs
        // shift when more than `max_keep` moves qualified.
        while i < self.moves.len() {
            self.moves.remove(i);
            i += 1;
        }
        if use_low {
            let best = self.best;
            for &m in &self.low {
                self.moves.insert(0, m);
                self.scores.insert(0, best);
            }
        }
        if self.scores.is_empty() {
            return None;
        }
        let lowest = *self.scores.last().unwrap_or(&0);
        let mut bag = Vec::new();
        for (m, s) in self.moves.iter().zip(&self.scores) {
            let weight = if *s == self.best {
                3
            } else if *s != lowest {
                2
            } else {
                1
            };
            for _ in 0..weight {
                bag.push(*m);
            }
        }
        let n = host.random(bag.len() as i32) as usize;
        bag.get(n).copied()
    }
}

/// The candidate order of S:67c0.
const FULL_SET: [Action; 15] = [
    Action::Fwd,
    Action::TurnLeft,
    Action::TurnRight,
    Action::PierceAttack,
    Action::PowerAttack,
    Action::WhirlAttack,
    Action::HackAttack,
    Action::SimpleWhirl,
    Action::Attack,
    Action::QuickAttack,
    Action::Parade,
    Action::JumpBack,
    Action::Left,
    Action::Right,
    Action::Back,
];

fn choose_from(
    host: &mut dyn ScriptHost,
    me: EntityId,
    target: EntityId,
    set: &[Action],
) -> (Option<Action>, Situation) {
    let mut c = build_situation(host, me, target);
    let mut list = Shortlist::default();
    for &mv in set {
        let s = score_move(host, me, target, &mut c, mv);
        list.offer(c.weight, mv, s);
    }
    let picked = list.pick(host, &c);
    (picked, c)
}

/// Port of S:67c0: choose among the full candidate set; with nothing chosen and no line of
/// sight the NPC backs off (if it can) or walks forward.
pub fn choose_full(host: &mut dyn ScriptHost, me: EntityId, target: EntityId) -> Option<Action> {
    let (picked, c) = choose_from(host, me, target, &FULL_SET);
    if picked.is_some() {
        return picked;
    }
    if !c.los && c.ready {
        return Some(if host.can_evade(me, target, dir::BACK) {
            Action::Back
        } else {
            Action::Fwd
        });
    }
    None
}

/// The chance in percent that the pickers S:6e60/S:7030 return their choice: the NPC's
/// `GetLevelMax` (Zombies 85), 10 less on easy, 10 more on hard, clamped to `0..=100`
/// (S:71b0).
pub fn skill_percent(host: &dyn ScriptHost, me: EntityId) -> i32 {
    let mut p = stats::level_max(host, me) as i32;
    if host.species(me) == Species::Zombie {
        p = 85;
    }
    match host.difficulty() {
        Difficulty::Easy => p -= 10,
        Difficulty::Hard => p += 10,
        Difficulty::Normal => {}
    }
    p.clamp(0, 100)
}

/// Port of S:6e60: the offensive-only picker (Pierce, Power, Whirl, Hack, SimpleWhirl, Attack,
/// Quick), skill-gated.
pub fn choose_offensive(
    host: &mut dyn ScriptHost,
    me: EntityId,
    target: EntityId,
) -> Option<Action> {
    const SET: [Action; 7] = [
        Action::PierceAttack,
        Action::PowerAttack,
        Action::WhirlAttack,
        Action::HackAttack,
        Action::SimpleWhirl,
        Action::Attack,
        Action::QuickAttack,
    ];
    gated(host, me, target, &SET)
}

/// Port of S:7030: the defensive-only picker (Parade, JumpBack, Left, Right, Back), skill-gated.
pub fn choose_defensive(
    host: &mut dyn ScriptHost,
    me: EntityId,
    target: EntityId,
) -> Option<Action> {
    const SET: [Action; 5] = [
        Action::Parade,
        Action::JumpBack,
        Action::Left,
        Action::Right,
        Action::Back,
    ];
    gated(host, me, target, &SET)
}

fn gated(
    host: &mut dyn ScriptHost,
    me: EntityId,
    target: EntityId,
    set: &[Action],
) -> Option<Action> {
    // The DLL re-initialises the record and does not take the snapshot before scoring here
    // (S:6b30 is not called), so every score is zero and the pickers yield nothing; the skill
    // gate is rolled all the same.
    let mut c = Situation::blank();
    let mut list = Shortlist::default();
    for &mv in set {
        let s = score_move(host, me, target, &mut c, mv);
        list.offer(c.weight, mv, s);
    }
    let picked = list.pick(host, &c);
    let p = skill_percent(host, me);
    if host.random(100) < p { picked } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::NpcApi;
    use crate::testhost::TestWorld;

    /// Two armed fighters `dist` cm apart (reach of the orc's plain attack: 320).
    fn duel(dist: f32) -> (TestWorld, EntityId, EntityId) {
        let mut w = TestWorld::new();
        let a = w.spawn_npc("Orc", Species::Orc, 10, 30);
        let b = w.spawn_npc("Hero", Species::Human, 10, 30);
        for e in [a, b] {
            w.set_slot(e, Slot::RightHand, UseType::OneH, 1);
            w.set_ai_mode(e, AiMode::Combat);
            w.set_combat_state(e, 1);
        }
        w.set_distance(a, b, dist);
        w.set_target(a, b);
        w.set_target(b, a);
        (w, a, b)
    }

    fn scores(w: &mut TestWorld, a: EntityId, b: EntityId) -> Vec<(Action, i32)> {
        let mut c = build_situation(w, a, b);
        FULL_SET
            .iter()
            .map(|&m| (m, score_move(w, a, b, &mut c, m)))
            .collect()
    }

    fn score_of(list: &[(Action, i32)], m: Action) -> i32 {
        list.iter().find(|(a, _)| *a == m).map_or(-1, |(_, s)| *s)
    }

    #[test]
    fn only_approaching_scores_out_of_reach() {
        let (mut w, a, b) = duel(600.0);
        let s = scores(&mut w, a, b);
        assert_eq!(score_of(&s, Action::Fwd), 85);
        assert!(
            s.iter()
                .filter(|(m, _)| *m != Action::Fwd)
                .all(|(_, v)| *v == 0)
        );
        assert_eq!(choose_full(&mut w, a, b), Some(Action::Fwd));
    }

    #[test]
    fn an_idle_target_in_reach_invites_every_attack_equally() {
        let (mut w, a, b) = duel(250.0);
        let s = scores(&mut w, a, b);
        assert_eq!(score_of(&s, Action::Attack), 65);
        assert_eq!(score_of(&s, Action::PowerAttack), 65);
        assert_eq!(score_of(&s, Action::PierceAttack), 65);
        assert_eq!(score_of(&s, Action::QuickAttack), 65);
        assert_eq!(score_of(&s, Action::Parade), 0, "nothing to parry");
        assert_eq!(score_of(&s, Action::WhirlAttack), 0, "needs a two-hander");
        assert_eq!(score_of(&s, Action::Fwd), 0);
        let m = choose_full(&mut w, a, b).expect("a move");
        assert!(is_attack_action(m), "{m:?}");
    }

    #[test]
    fn repeating_a_move_costs_35() {
        let (mut w, a, b) = duel(250.0);
        w.set_current_action_direct(a, Action::Attack);
        let s = scores(&mut w, a, b);
        assert_eq!(score_of(&s, Action::Attack), 30);
        assert_eq!(score_of(&s, Action::PowerAttack), 65);
    }

    #[test]
    fn a_swinging_enemy_in_reach_is_parried_or_answered() {
        let (mut w, a, b) = duel(250.0);
        w.set_current_action_direct(b, Action::Attack);
        let s = scores(&mut w, a, b);
        let weight = build_situation(&mut w, a, b).weight;
        assert_eq!(score_of(&s, Action::Parade), 65);
        assert_eq!(score_of(&s, Action::Attack), 65);
        assert_eq!(score_of(&s, Action::Back), weight + 45);
        assert_eq!(score_of(&s, Action::Fwd), weight);
    }

    #[test]
    fn the_aggression_weight_follows_difficulty_and_level() {
        let (mut w, a, b) = duel(250.0);
        let c = build_situation(&mut w, a, b);
        assert_eq!(c.weight, 5 + 10);
        w.difficulty = Difficulty::Hard;
        assert_eq!(build_situation(&mut w, a, b).weight, 5);
        w.difficulty = Difficulty::Easy;
        assert_eq!(build_situation(&mut w, a, b).weight, 25);
        w.set_attack_reason(a, AttackReason::Arena);
        assert_eq!(build_situation(&mut w, a, b).weight, 15);
    }

    #[test]
    fn equipment_gates_the_moves() {
        let (mut w, a, b) = duel(250.0);
        assert!(can_do(&w, a, b, Action::PierceAttack));
        assert!(!can_do(&w, a, b, Action::SimpleWhirl));
        assert!(!can_do(&w, a, b, Action::WhirlAttack));
        assert!(!can_do(&w, a, b, Action::HackAttack));
        assert!(!can_do(&w, a, b, Action::JumpBack), "orcs never jump back");
        w.set_slot(a, Slot::LeftHand, UseType::OneH, 1);
        assert!(can_do(&w, a, b, Action::SimpleWhirl));
        w.set_slot(a, Slot::RightHand, UseType::TwoH, 1);
        w.set_slot(a, Slot::LeftHand, UseType::None, 0);
        assert!(can_do(&w, a, b, Action::WhirlAttack));
        assert!(can_do(&w, a, b, Action::HackAttack));
        assert!(!can_do(&w, a, b, Action::PierceAttack));
        // Fists cannot quick-attack; a parading opponent cannot be parried.
        w.set_slot(a, Slot::RightHand, UseType::Fist, 0);
        assert!(!can_do(&w, a, b, Action::QuickAttack));
        w.set_ani_state(b, AniState::Parade);
        assert!(!can_do(&w, a, b, Action::Parade));
        // An NPC that struck a moment ago may not attack again.
        w.hit_blocked.insert(a);
        assert!(!can_do(&w, a, b, Action::Attack));
    }

    #[test]
    fn shortlist_keeps_moves_within_35_of_the_best() {
        let mut l = Shortlist::default();
        l.offer(10, Action::Attack, 60);
        l.offer(10, Action::PowerAttack, 75);
        l.offer(10, Action::Back, 20);
        l.offer(10, Action::Left, 5);
        assert_eq!(l.moves, vec![Action::PowerAttack, Action::Attack]);
        assert_eq!(l.scores, vec![75, 60]);
        // 20 is more than 35 below 75 and above the weight: dropped. 5 is below W: kept apart.
        assert_eq!(l.low, vec![Action::Left]);
    }

    #[test]
    fn the_skill_gate_uses_level_max_and_difficulty() {
        let (mut w, a, _) = duel(250.0);
        assert_eq!(skill_percent(&w, a), 30);
        w.difficulty = Difficulty::Hard;
        assert_eq!(skill_percent(&w, a), 50, "level max 40 plus 10");
        w.difficulty = Difficulty::Easy;
        assert_eq!(skill_percent(&w, a), 10, "level max 20 minus 10");
    }
}
