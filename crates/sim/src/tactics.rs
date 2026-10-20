//! Combat AI move selection [CA 9]: ranges, candidate sets, scoring, the shortlist /
//! secondary-list pick driven by level, the potion rule, passive / ranged / magic decisions.
//!
//! What the doc recovered is the *structure* (candidate sets, `CanDo` restrictions, the
//! shortlist window of 35, the pick probability `p = LevelMax`, constants 85/75/70/65/55/45/40
//! and the aggression weight); the per-move scores are not recovered and are marked `[I]`.
use crate::actor::Actor;
use crate::config::{Difficulty, SimConfig};
use crate::item::{Use, WeaponMode};
use crate::rng::Rng;
use crate::types::*;

/// `GetCombatMoveLength` (S:aa270) [CA 9.2]; `None` stands for the engine's `-1`.
pub fn combat_move_length(species: Species, mv: Action) -> Option<f32> {
    if species == Species::Zombie && mv == Action::Attack {
        return Some(150.0);
    }
    match mv {
        Action::Attack
        | Action::SimpleWhirl
        | Action::PierceAttack
        | Action::HackAttack
        | Action::GetUpAttack => Some(120.0),
        Action::PowerAttack | Action::SprintAttack | Action::WhirlAttack => Some(180.0),
        Action::QuickAttack | Action::QuickAttackR | Action::QuickAttackL => Some(60.0),
        _ => None,
    }
}

/// `GetAttackRange` (S:4e940): species base + move length (120 when the length is -1) [CA 9.2].
pub fn attack_range(species: Species, fist_mode: bool, mv: Action) -> f32 {
    use Species::*;
    let quick = mv.is_quick();
    let base = match species {
        Golem | FireGolem | IceGolem | Bloodfly => 110.0,
        Goblin => 140.0,
        Troll | Trex | Dragon => 300.0,
        Minecrawler | Wolf | Ripper => 170.0,
        Scavenger => 115.0,
        Boar => 180.0,
        Sabertooth | Shadowbeast | Varan | Snapper | Alligator | Gargoyle => 200.0,
        Bison | Rhino => 150.0,
        Ogre => 220.0,
        ScorpionKing => 120.0,
        Lurker | SwampLurker => {
            if quick {
                200.0
            } else {
                150.0
            }
        }
        _ => {
            if fist_mode {
                50.0
            } else {
                200.0
            }
        }
    };
    base + combat_move_length(species, mv).unwrap_or(120.0)
}

/// `GetCloseRange`: stop distance of the approach [CA 9.1].
pub fn close_range(species: Species) -> f32 {
    if matches!(species, Species::Troll | Species::Trex | Species::Dragon) {
        600.0
    } else {
        500.0
    }
}

/// `GetFollowTime` (S:10240): how long an NPC chases before giving up [CA 9.1].
/// Wolf 15 s; with AIMode Human / Orc 5 s, others 9 s [I for the non-AIMode value].
pub fn follow_time_secs(cfg: &SimConfig, species: Species) -> f32 {
    if species == Species::Wolf {
        15.0
    } else if cfg.ai_mode && species.is_humanoid() {
        5.0
    } else {
        9.0
    }
}

/// Situation snapshot for scoring [CA 9.3].
#[derive(Debug, Clone)]
pub struct TacticsCtx {
    pub dist: f32,
    pub attack_range: f32,
    /// The target has a strike pending against us.
    pub target_attacking: bool,
    pub target_parading: bool,
    pub los: bool,
    pub target_is_player: bool,
    pub target_is_mage: bool,
    pub arena: bool,
    pub party_condition: bool,
    /// `CanHit`: the minimum hit duration since the last hit taken has elapsed.
    pub can_hit: bool,
    pub current_move: Action,
    /// Exhausted: restrict the choice to the defensive set [CA 9.3, I on the trigger].
    pub defensive: bool,
}

impl TacticsCtx {
    pub fn in_range(&self) -> bool {
        self.dist <= self.attack_range
    }
}

/// Aggression weight `W`: start 5; difficulty easy/normal/hard +20/+10/+0; arena -10; mage
/// target -5; party condition -10; floor 0 [CA 9.3, I on the "minimum 20" clause].
pub fn aggression_weight(cfg: &SimConfig, c: &TacticsCtx) -> i32 {
    let mut w = 5 + match cfg.difficulty {
        Difficulty::Easy => 20,
        Difficulty::Normal => 10,
        Difficulty::Hard => 0,
    };
    if c.arena {
        w -= 10;
    }
    if c.target_is_mage {
        w -= 5;
    }
    if c.party_condition {
        w -= 10;
    }
    w.max(0)
}

/// `CanDo` (S:4f80) restrictions [CA 9.3].
pub fn can_do(a: &Actor, mv: Action, c: &TacticsCtx) -> bool {
    let eq = &a.equipment;
    let right = eq.right_use();
    let humanoid_tactics = a.species.uses_humanoid_tactics();
    let fist = a.equipment.weapon_damage().is_none();
    match mv {
        // Animals only chain sprint / power attacks and approach.
        _ if !humanoid_tactics => {
            matches!(
                mv,
                Action::Fwd | Action::PowerAttack | Action::Attack | Action::Left | Action::Right
            ) && !(mv == Action::Attack
                && matches!(a.species, Species::Lurker | Species::SwampLurker))
        }
        Action::Attack => !matches!(a.species, Species::Lurker | Species::SwampLurker),
        Action::PowerAttack => true,
        Action::QuickAttack => a.species.is_humanoid() && !fist,
        Action::SimpleWhirl => eq.dual_one_hand(),
        Action::WhirlAttack => matches!(right, Use::TwoH | Use::Staff),
        Action::PierceAttack => right == Use::OneH,
        Action::HackAttack => matches!(right, Use::TwoH | Use::Staff | Use::Axe | Use::Halberd),
        Action::Parade => !c.target_parading && eq.weapon_mode() == WeaponMode::Melee,
        Action::JumpBack | Action::Left | Action::Right | Action::Back => !matches!(
            a.species,
            Species::Troll
                | Species::Golem
                | Species::FireGolem
                | Species::IceGolem
                | Species::Goblin
        ),
        Action::Fwd | Action::TurnLeft | Action::TurnRight => true,
        _ => false,
    }
}

/// Score of one candidate [CA 9.3, I]. Constants 85/75/70/65/55/45/40 are the ones found in
/// the binary; their assignment to moves is inferred from the intent of each move.
pub fn score_move(cfg: &SimConfig, a: &Actor, mv: Action, c: &TacticsCtx) -> i32 {
    let mut s = match mv {
        Action::Fwd => {
            if c.in_range() {
                40
            } else {
                85
            }
        }
        Action::Parade => {
            if c.target_attacking && c.los {
                85
            } else {
                40
            }
        }
        Action::JumpBack => {
            if c.target_attacking {
                55
            } else {
                40
            }
        }
        Action::PowerAttack => 75,
        Action::Attack => 70,
        Action::HackAttack | Action::PierceAttack => 65,
        Action::SimpleWhirl | Action::WhirlAttack => 55,
        Action::QuickAttack | Action::QuickAttackL | Action::QuickAttackR => 45,
        Action::Left | Action::Right => 45,
        Action::Back | Action::TurnLeft | Action::TurnRight => 40,
        _ => 0,
    };
    if mv.is_melee_attack() {
        // Not in range: attacking is pointless.
        if !c.in_range() {
            s -= 25;
        }
        // Pierce and whirl cannot be parried under AIMode: prefer them against a parrier.
        if cfg.ai_mode
            && c.target_parading
            && matches!(mv, Action::PierceAttack | Action::WhirlAttack)
        {
            s += 20;
        }
        // Attacking into an incoming strike is worse than defending.
        if c.target_attacking {
            s -= 15;
        }
        // Not enough stamina for the move.
        if crate::vitals::stamina_cost(cfg, mv) > a.stamina {
            s -= 50;
        }
        if !c.can_hit {
            s -= 35;
        }
    }
    if mv == c.current_move && mv != Action::Fwd {
        s -= 35;
    }
    s
}

fn candidates(c: &TacticsCtx) -> &'static [Action] {
    use Action::*;
    const FULL: &[Action] = &[
        Fwd,
        TurnLeft,
        TurnRight,
        PierceAttack,
        PowerAttack,
        WhirlAttack,
        HackAttack,
        SimpleWhirl,
        Attack,
        QuickAttack,
        Parade,
        JumpBack,
        Left,
        Right,
        Back,
    ];
    const DEFENSIVE: &[Action] = &[Parade, JumpBack, Left, Right, Back];
    const OFFENSIVE: &[Action] = &[
        PierceAttack,
        PowerAttack,
        WhirlAttack,
        HackAttack,
        SimpleWhirl,
        Attack,
        QuickAttack,
    ];
    if !c.in_range() {
        &[Fwd]
    } else if c.defensive {
        DEFENSIVE
    } else if c.target_attacking {
        FULL
    } else if c.target_parading {
        OFFENSIVE
    } else {
        FULL
    }
}

/// Pick a move: candidates within 35 of the best form the shortlist, weaker ones the
/// secondary list (kept only when `Random(55) < W`); the shortlist is used with probability
/// `p` percent, otherwise the secondary list [CA 9.3].
pub fn select_combat_move(cfg: &SimConfig, rng: &mut Rng, a: &Actor, c: &TacticsCtx) -> Action {
    let scored: Vec<(Action, i32)> = candidates(c)
        .iter()
        .copied()
        .filter(|m| can_do(a, *m, c))
        .map(|m| (m, score_move(cfg, a, m, c)))
        .collect();
    let Some(best) = scored.iter().map(|x| x.1).max() else {
        return Action::Wait;
    };
    let short: Vec<Action> = scored
        .iter()
        .filter(|x| x.1 >= best - 35)
        .map(|x| x.0)
        .collect();
    let mut second: Vec<Action> = scored
        .iter()
        .filter(|x| x.1 < best - 35)
        .map(|x| x.0)
        .collect();
    if rng.below(55) >= aggression_weight(cfg, c) {
        second.clear();
    }
    // The shortlist only holds moves within 35 of the best; "optimal" picks the very best
    // ones among them, the pick within the shortlist is uniform.
    let optimal = rng.chance_pct(a.optimal_move_pct(cfg));
    let list = if optimal || second.is_empty() {
        &short
    } else {
        &second
    };
    list[rng.below(list.len() as i32) as usize]
}

/// Passive combat state (S:4ffb0): back off below 350, approach above 450, else turn [CA 9.3].
pub fn passive_move(dist: f32) -> Action {
    if dist < 350.0 {
        Action::Back
    } else if dist > 450.0 {
        Action::Fwd
    } else {
        Action::TurnLeft
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangedDecision {
    Approach,
    NewTarget,
    Shoot,
}

/// Ranged (S:50370): D > 2000 approach, no LOS look for a new target, else shoot [CA 9.3].
pub fn ranged_move(dist: f32, los: bool) -> RangedDecision {
    if dist > 2000.0 {
        RangedDecision::Approach
    } else if !los {
        RangedDecision::NewTarget
    } else {
        RangedDecision::Shoot
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MagicDecision {
    Retreat,
    Cast,
    Approach,
    Hold,
}

/// Magic (S:50550): focus spells retreat below 350, cast between 350 and 1175, approach beyond;
/// other spells cast with LOS (80 percent, always for "Xardas") [CA 9.3].
pub fn magic_move(
    rng: &mut Rng,
    dist: f32,
    los: bool,
    focus_spell: bool,
    is_xardas: bool,
) -> MagicDecision {
    if focus_spell {
        return if dist < 350.0 {
            MagicDecision::Retreat
        } else if dist <= 1175.0 {
            MagicDecision::Cast
        } else {
            MagicDecision::Approach
        };
    }
    if !los {
        return MagicDecision::Approach;
    }
    if is_xardas || rng.chance_pct(80) {
        MagicDecision::Cast
    } else {
        MagicDecision::Hold
    }
}

/// The only HP-based behaviour [CA 9.1]: a non-Mage / non-Paladin NPC below 40 percent HP with
/// a health potion, a melee (or no dangerous) weapon and a target farther than 700 drinks.
pub fn wants_potion(cfg: &SimConfig, a: &Actor, dist_to_target: f32) -> bool {
    !a.is_player
        && !matches!(a.class, NpcClass::Mage | NpcClass::Paladin)
        && a.hp * 100 < a.hp_max(cfg) * 40
        && a.inventory.get("it_potion_health").copied().unwrap_or(0) > 0
        && matches!(
            a.equipment.weapon_mode(),
            WeaponMode::Melee | WeaponMode::None
        )
        && dist_to_target > 700.0
}

/// Original `WantToFlee` is a stub (0); the optional variant makes non-boss NPCs flee at low
/// HP without a potion [CA 9.1].
pub fn wants_to_flee(cfg: &SimConfig, a: &Actor) -> bool {
    match cfg.variants.flee_below_hp_pct {
        Some(p) => {
            !a.is_player
                && !a.boss
                && a.hp * 100 < a.hp_max(cfg) * p
                && a.inventory.get("it_potion_health").copied().unwrap_or(0) == 0
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::ItemDef;

    fn ctx(dist: f32, range: f32) -> TacticsCtx {
        TacticsCtx {
            dist,
            attack_range: range,
            target_attacking: false,
            target_parading: false,
            los: true,
            target_is_player: true,
            target_is_mage: false,
            arena: false,
            party_condition: false,
            can_hit: true,
            current_move: Action::None,
            defensive: false,
        }
    }

    fn knight(c: &SimConfig) -> Actor {
        let mut a = Actor::npc(ActorId(1), "K", Species::Human, 20, 20);
        a.refresh(c);
        a.equipment.equip(ItemDef::weapon("S", "1H", "Blade", 50));
        a
    }

    #[test]
    fn ranges() {
        // [CA 9.2] Human base 200 + Attack length 120 = 320; Power 180 -> 380; fists 50.
        assert_eq!(attack_range(Species::Human, false, Action::Attack), 320.0);
        assert_eq!(
            attack_range(Species::Orc, false, Action::PowerAttack),
            380.0
        );
        assert_eq!(
            attack_range(Species::Human, true, Action::QuickAttack),
            110.0
        );
        assert_eq!(attack_range(Species::Troll, false, Action::Attack), 420.0);
        assert_eq!(
            attack_range(Species::Wolf, false, Action::PowerAttack),
            350.0
        );
        assert_eq!(attack_range(Species::Zombie, false, Action::Attack), 350.0);
        assert_eq!(
            attack_range(Species::Lurker, false, Action::QuickAttack),
            260.0
        );
        assert_eq!(attack_range(Species::Lurker, false, Action::Attack), 270.0);
        // Moves without a length use 120.
        assert_eq!(attack_range(Species::Goblin, false, Action::Parade), 260.0);
        assert_eq!(close_range(Species::Troll), 600.0);
        assert_eq!(close_range(Species::Orc), 500.0);
    }

    #[test]
    fn follow_time() {
        let ai = SimConfig::default();
        let van = SimConfig::vanilla();
        assert_eq!(follow_time_secs(&ai, Species::Orc), 5.0);
        assert_eq!(follow_time_secs(&ai, Species::Snapper), 9.0);
        assert_eq!(follow_time_secs(&ai, Species::Wolf), 15.0);
        assert_eq!(follow_time_secs(&van, Species::Human), 9.0);
    }

    #[test]
    fn aggression_weight_rules() {
        let mut cfg = SimConfig::default();
        let mut c = ctx(100.0, 300.0);
        assert_eq!(aggression_weight(&cfg, &c), 15);
        c.arena = true;
        c.target_is_mage = true;
        assert_eq!(aggression_weight(&cfg, &c), 0);
        cfg.difficulty = Difficulty::Easy;
        assert_eq!(aggression_weight(&cfg, &c), 10);
    }

    #[test]
    fn can_do_restrictions() {
        let cfg = SimConfig::default();
        let a = knight(&cfg);
        let c = ctx(100.0, 300.0);
        assert!(can_do(&a, Action::PierceAttack, &c));
        assert!(
            !can_do(&a, Action::HackAttack, &c),
            "hack needs a 2H / axe / halberd"
        );
        assert!(!can_do(&a, Action::WhirlAttack, &c));
        assert!(
            !can_do(&a, Action::SimpleWhirl, &c),
            "needs a one-hander in each hand"
        );
        assert!(can_do(&a, Action::QuickAttack, &c));
        assert!(can_do(&a, Action::Parade, &c));
        let mut parading = c.clone();
        parading.target_parading = true;
        assert!(
            !can_do(&a, Action::Parade, &parading),
            "no parade against a parrier"
        );
        // Dual wield enables SimpleWhirl; a 2H enables Whirl and Hack.
        let mut dual = knight(&cfg);
        dual.equipment
            .equip(ItemDef::weapon("S2", "1H", "Blade", 10));
        assert!(can_do(&dual, Action::SimpleWhirl, &c));
        let mut big = Actor::npc(ActorId(2), "B", Species::Orc, 20, 20);
        big.equipment
            .equip(ItemDef::weapon("H", "2H", "Blade", 100));
        assert!(can_do(&big, Action::HackAttack, &c) && can_do(&big, Action::WhirlAttack, &c));
        assert!(!can_do(&big, Action::PierceAttack, &c));
        // Lurkers cannot plain-attack; trolls do not evade; animals only chain power attacks.
        let lurker = Actor::npc(ActorId(3), "L", Species::Lurker, 5, 5);
        assert!(!can_do(&lurker, Action::Attack, &c));
        assert!(can_do(&lurker, Action::PowerAttack, &c));
        let troll = Actor::npc(ActorId(4), "T", Species::Troll, 5, 5);
        assert!(!can_do(&troll, Action::JumpBack, &c));
        let wolf = Actor::npc(ActorId(5), "W", Species::Wolf, 5, 5);
        assert!(!can_do(&wolf, Action::PierceAttack, &c) && !can_do(&wolf, Action::Parade, &c));
    }

    #[test]
    fn approach_when_out_of_range() {
        let cfg = SimConfig::default();
        let a = knight(&cfg);
        let mut rng = Rng::new(1);
        let far = ctx(900.0, 320.0);
        for _ in 0..50 {
            let m = select_combat_move(&cfg, &mut rng, &a, &far);
            assert_eq!(m, Action::Fwd);
        }
    }

    #[test]
    fn level_is_the_chance_of_an_optimal_move() {
        // [CA 9.3] p = LevelMax percent picks from the shortlist (within 35 of the best);
        // otherwise a weaker move may be chosen (only when Random(55) < W).
        let cfg = SimConfig::default();
        let mut rng = Rng::new(7);
        let mut inc = ctx(150.0, 320.0);
        inc.target_attacking = true;
        let bad_rate = |lvl: i32, rng: &mut Rng| {
            let mut a = knight(&cfg);
            a.level = lvl;
            a.level_max = lvl;
            let best = candidates(&inc)
                .iter()
                .filter(|m| can_do(&a, **m, &inc))
                .map(|m| score_move(&cfg, &a, *m, &inc))
                .max()
                .unwrap();
            (0..4000)
                .filter(|_| {
                    let m = select_combat_move(&cfg, rng, &a, &inc);
                    score_move(&cfg, &a, m, &inc) < best - 35
                })
                .count()
        };
        let low = bad_rate(5, &mut rng);
        let high = bad_rate(100, &mut rng);
        assert_eq!(high, 0, "level 100 never leaves the shortlist");
        // 95 percent non-optimal * 15/55 secondary probability = 26 percent of 4000.
        assert!((700..1400).contains(&low), "low-level bad moves {low}");
    }

    #[test]
    fn mixed_decisions() {
        assert_eq!(passive_move(300.0), Action::Back);
        assert_eq!(passive_move(500.0), Action::Fwd);
        assert_eq!(passive_move(400.0), Action::TurnLeft);
        assert_eq!(ranged_move(2500.0, true), RangedDecision::Approach);
        assert_eq!(ranged_move(1000.0, false), RangedDecision::NewTarget);
        assert_eq!(ranged_move(1000.0, true), RangedDecision::Shoot);
        let mut rng = Rng::new(3);
        assert_eq!(
            magic_move(&mut rng, 300.0, true, true, false),
            MagicDecision::Retreat
        );
        assert_eq!(
            magic_move(&mut rng, 800.0, true, true, false),
            MagicDecision::Cast
        );
        assert_eq!(
            magic_move(&mut rng, 1500.0, true, true, false),
            MagicDecision::Approach
        );
        assert_eq!(
            magic_move(&mut rng, 800.0, true, false, true),
            MagicDecision::Cast
        );
        assert_eq!(
            magic_move(&mut rng, 800.0, false, false, false),
            MagicDecision::Approach
        );
    }

    #[test]
    fn potion_rule_and_flee_variant() {
        let cfg = SimConfig::default();
        let mut a = knight(&cfg);
        a.inventory.insert("it_potion_health".into(), 1);
        a.hp = a.hp_max(&cfg) * 39 / 100;
        assert!(wants_potion(&cfg, &a, 800.0));
        assert!(!wants_potion(&cfg, &a, 600.0), "target too close");
        a.hp = a.hp_max(&cfg) * 40 / 100;
        assert!(
            !wants_potion(&cfg, &a, 800.0),
            "exactly 40 percent is not below"
        );
        a.hp = 1;
        a.class = NpcClass::Mage;
        assert!(!wants_potion(&cfg, &a, 800.0));
        a.class = NpcClass::Warrior;
        a.inventory.clear();
        assert!(!wants_potion(&cfg, &a, 800.0));
        // Original: never flees on HP. Variant: flees at low HP without a potion.
        assert!(!wants_to_flee(&cfg, &a));
        let mut v = cfg.clone();
        v.variants.flee_below_hp_pct = Some(20);
        assert!(wants_to_flee(&v, &a));
        a.boss = true;
        assert!(!wants_to_flee(&v, &a));
    }
}
