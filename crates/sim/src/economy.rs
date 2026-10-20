//! Trade prices, pickpocketing, armor reputation requirements, learning-point costs and
//! player progression helpers [CA 8, GAME_RULES 1.6 / 4.1].
use crate::actor::{Actor, PlayerAttrs};
use crate::config::SimConfig;
use crate::item::ItemDef;
use crate::rng::Rng;
use gothic_gameplay::GameState;

/// Direction of a trade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeSide {
    /// The NPC sells to the player.
    PlayerBuys,
    /// The player sells to the NPC.
    PlayerSells,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PriceCtx {
    pub has_barter_perk: bool,
    /// The trading NPC's attitude to the player is Friendly.
    pub npc_friendly: bool,
}

/// `OnPlayerGetGoldCost` (S:6b550) [CA 8.1]: `price = max(1, trunc(value * m))`.
///
/// `m0` = 1.0 when buying, 0.3 when selling (0.6 for furs, skins and valuables); the barter
/// factor is `1 +- 0.1` for `Perk_Barter` and again for a friendly NPC (plus when selling,
/// minus when buying); selling under ExtendedContent subtracts 0.05. Computed in permille so
/// the shipped default (no perks) is exactly 25 percent when selling, 100 percent buying.
pub fn trade_price(cfg: &SimConfig, item: &ItemDef, side: TradeSide, ctx: PriceCtx) -> i32 {
    let selling = side == TradeSide::PlayerSells;
    let special = matches!(
        item.use_type.as_deref(),
        Some("TrophySkin" | "TrophyFur" | "Valuable")
    );
    let m0: i64 = if selling {
        if special { 600 } else { 300 }
    } else {
        1000
    };
    let step: i64 = if selling { 100 } else { -100 };
    let mut b: i64 = 1000;
    if ctx.has_barter_perk {
        b += step;
    }
    if ctx.npc_friendly {
        b += step;
    }
    let mut m = m0 * b / 1000;
    if selling && cfg.extended_content {
        m -= 50;
    }
    ((i64::from(item.value) * m / 1000).max(1)) as i32
}

/// Gold value -> political fame needed to equip a generated faction armor
/// (S:6b020) [CA 8.3].
pub fn armor_fame_requirement(cfg: &SimConfig, gold_value: i32) -> i32 {
    if cfg.extended_content {
        match gold_value {
            i32::MIN..=15_000 => 0,
            15_001..=22_000 => 20,
            22_001..=39_000 => 25,
            39_001..=78_000 => 40,
            78_001..=120_000 => 50,
            _ => 60,
        }
    } else {
        match gold_value {
            i32::MIN..=10_000 => 0,
            10_001..=65_000 => 25,
            _ => 50,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickpocketResult {
    /// Level 45+ without `Perk_PickPocket_3`.
    Impossible,
    /// Level 30+ without `Perk_PickPocket_2`.
    TooHard,
    Success {
        /// Kill-XP-like reward `L/7` under ExtendedContent.
        xp: u64,
    },
    /// The victim gets AttackReason Theft via `AssessTarget`.
    Caught,
}

/// Success threshold: succeed if `rand(100) <= Theft/2 - L + 85` [CA 8.2].
pub fn pickpocket_threshold(theft: i32, target_level_max: i32) -> i32 {
    theft / 2 - target_level_max.min(50) + 85
}

/// Probability (0..=1) that an allowed attempt succeeds: `rand(100)` is `0..=99`.
pub fn pickpocket_probability(theft: i32, target_level_max: i32) -> f32 {
    let t = pickpocket_threshold(theft, target_level_max);
    ((t + 1).clamp(0, 100)) as f32 / 100.0
}

/// `PickPocket` (S:3dd70) [CA 8.2].
pub fn pickpocket(
    cfg: &SimConfig,
    rng: &mut Rng,
    thief: &Actor,
    target: &Actor,
) -> PickpocketResult {
    let l = target.level_max(cfg).min(50);
    if l >= 45 && !thief.has_perk("perk_pickpocket_3") {
        return PickpocketResult::Impossible;
    }
    if l >= 30 && !thief.has_perk("perk_pickpocket_2") {
        return PickpocketResult::TooHard;
    }
    if rng.below(100) <= pickpocket_threshold(thief.theft(cfg), l) {
        PickpocketResult::Success {
            xp: if cfg.extended_content {
                (l / 7) as u64
            } else {
                0
            },
        }
    } else {
        PickpocketResult::Caught
    }
}

/// Learning-point cost of raising an attribute by `points` units: 1:1, doubled under
/// ExtendedContent for STR/DEX/INT when the current value is already >= 250
/// [GAME_RULES 1.3 Teach / 1.6]. HP/MP/SP grow by 4 attribute units per point.
pub fn teach_cost(cfg: &SimConfig, tag: &str, current: i32, points: u32) -> u32 {
    let t = tag.to_ascii_lowercase();
    if cfg.extended_content && matches!(t.as_str(), "str" | "dex" | "int") && current >= 250 {
        points * 2
    } else {
        points
    }
}

/// Teach an attribute from the learning points of `state`; returns false if too few.
pub fn learn_attribute(
    cfg: &SimConfig,
    state: &mut GameState,
    tag: &str,
    current: i32,
    points: u32,
) -> bool {
    let cost = teach_cost(cfg, tag, current, points);
    if state.learning_points < cost {
        return false;
    }
    let m: i32 = if matches!(tag.to_ascii_lowercase().as_str(), "hp" | "mp" | "sp") {
        4
    } else {
        1
    };
    state.learning_points -= cost;
    *state
        .attributes
        .entry(tag.to_ascii_lowercase())
        .or_insert(0) += points as i32 * m;
    true
}

/// Grant `xp` to the player through the gameplay state (levels, +10 LP, `Perk_Learn`).
/// Returns (level before, level after).
pub fn grant_xp(state: &mut GameState, xp: u64) -> (u32, u32) {
    state.add_xp(xp)
}

/// Copy the saved progression (level, attributes, skills) into the hero actor. Base values
/// are the new-game values plus the taught deltas in `state.attributes` [CA 2.13].
pub fn sync_hero(cfg: &SimConfig, hero: &mut Actor, state: &GameState) {
    let base = PlayerAttrs::new_game(cfg);
    let d = |k: &str| state.attributes.get(k).copied().unwrap_or(0);
    hero.level = state.level.max(1) as i32;
    hero.level_max = hero.level;
    hero.base = PlayerAttrs {
        str: base.str + d("str"),
        dex: base.dex + d("dex"),
        int: base.int + d("int"),
        smithing: base.smithing + d("smi"),
        theft: base.theft + d("thf"),
        alchemy: base.alchemy + d("alc"),
        hp_max: base.hp_max + d("hp"),
        mana_max: base.mana_max + d("mp"),
        stamina_max: base.stamina_max + d("sp"),
    };
    hero.perks = state.skills.iter().cloned().collect();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn item(value: i32, use_type: &str) -> ItemDef {
        ItemDef {
            value,
            use_type: Some(use_type.into()),
            ..ItemDef::default()
        }
    }

    #[test]
    fn shipped_trade_prices() {
        // [CA 8.1] shipped flags, no perks: pay 100 percent, receive 25 percent.
        let c = SimConfig::default();
        let sword = item(1000, "1H");
        let none = PriceCtx::default();
        assert_eq!(trade_price(&c, &sword, TradeSide::PlayerBuys, none), 1000);
        assert_eq!(trade_price(&c, &sword, TradeSide::PlayerSells, none), 250);
        // Vanilla: 30 percent.
        assert_eq!(
            trade_price(&SimConfig::vanilla(), &sword, TradeSide::PlayerSells, none),
            300
        );
        // Barter perk: buying -10 percent -> 900; selling 0.3 * 1.1 - 0.05 = 0.28 -> 280.
        let barter = PriceCtx {
            has_barter_perk: true,
            npc_friendly: false,
        };
        assert_eq!(trade_price(&c, &sword, TradeSide::PlayerBuys, barter), 900);
        assert_eq!(trade_price(&c, &sword, TradeSide::PlayerSells, barter), 280);
        // Friendly NPC stacks with Barter: buy 800; sell 0.3*1.2 - 0.05 = 0.31 -> 310.
        let both = PriceCtx {
            has_barter_perk: true,
            npc_friendly: true,
        };
        assert_eq!(trade_price(&c, &sword, TradeSide::PlayerBuys, both), 800);
        assert_eq!(trade_price(&c, &sword, TradeSide::PlayerSells, both), 310);
        // Furs, skins and valuables sell at 0.6 - 0.05 = 55 percent.
        assert_eq!(
            trade_price(&c, &item(100, "TrophyFur"), TradeSide::PlayerSells, none),
            55
        );
        assert_eq!(
            trade_price(&c, &item(100, "Valuable"), TradeSide::PlayerSells, none),
            55
        );
        // Minimum price 1.
        assert_eq!(
            trade_price(&c, &item(1, "None"), TradeSide::PlayerSells, none),
            1
        );
        assert_eq!(
            trade_price(&c, &item(0, "None"), TradeSide::PlayerBuys, none),
            1
        );
    }

    #[test]
    fn armor_thresholds() {
        let c = SimConfig::default();
        let v = SimConfig::vanilla();
        assert_eq!(armor_fame_requirement(&c, 15_000), 0);
        assert_eq!(armor_fame_requirement(&c, 15_001), 20);
        assert_eq!(armor_fame_requirement(&c, 22_000), 20);
        assert_eq!(armor_fame_requirement(&c, 39_000), 25);
        assert_eq!(armor_fame_requirement(&c, 78_000), 40);
        assert_eq!(armor_fame_requirement(&c, 120_000), 50);
        assert_eq!(armor_fame_requirement(&c, 120_001), 60);
        assert_eq!(armor_fame_requirement(&v, 10_000), 0);
        assert_eq!(armor_fame_requirement(&v, 65_000), 25);
        assert_eq!(armor_fame_requirement(&v, 65_001), 50);
    }

    #[test]
    fn pickpocket_chances() {
        let c = SimConfig::default();
        // Theft 40 vs LevelMax 20: threshold 20 - 20 + 85 = 85 -> 86 percent.
        assert_eq!(pickpocket_threshold(40, 20), 85);
        assert!((pickpocket_probability(40, 20) - 0.86).abs() < 1e-6);
        // Level is capped at 50: LevelMax 80 behaves as 50.
        assert_eq!(pickpocket_threshold(40, 80), 55);
        let mut thief = Actor::hero(ActorId(0), &c);
        thief.base.theft = 40;
        let mut rng = Rng::new(9);
        let mut mid = Actor::npc(ActorId(1), "Mid", Species::Human, 10, 20);
        mid.refresh(&c);
        let ok = (0..2000)
            .filter(|_| {
                matches!(
                    pickpocket(&c, &mut rng, &thief, &mid),
                    PickpocketResult::Success { .. }
                )
            })
            .count();
        assert!((1600..1850).contains(&ok), "{ok}");
        // XP L/7 with ExtendedContent.
        match (0..50)
            .map(|_| pickpocket(&c, &mut rng, &thief, &mid))
            .find(|r| matches!(r, PickpocketResult::Success { .. }))
        {
            Some(PickpocketResult::Success { xp }) => assert_eq!(xp, 2),
            other => panic!("{other:?}"),
        }
        // Gates by level and perks.
        let hard = Actor::npc(ActorId(2), "H", Species::Human, 30, 30);
        assert_eq!(
            pickpocket(&c, &mut rng, &thief, &hard),
            PickpocketResult::TooHard
        );
        let elite = Actor::npc(ActorId(3), "E", Species::Human, 45, 45);
        assert_eq!(
            pickpocket(&c, &mut rng, &thief, &elite),
            PickpocketResult::Impossible
        );
        thief.perks.insert("perk_pickpocket_2".into());
        assert!(!matches!(
            pickpocket(&c, &mut rng, &thief, &hard),
            PickpocketResult::TooHard
        ));
    }

    #[test]
    fn teaching_and_progression() {
        let c = SimConfig::default();
        assert_eq!(teach_cost(&c, "STR", 100, 5), 5);
        assert_eq!(teach_cost(&c, "STR", 250, 5), 10);
        assert_eq!(teach_cost(&c, "hp", 300, 5), 5);
        assert_eq!(teach_cost(&SimConfig::vanilla(), "STR", 300, 5), 5);
        let mut st = GameState::new();
        st.learning_points = 7;
        assert!(learn_attribute(&c, &mut st, "STR", 100, 5));
        assert_eq!(st.learning_points, 2);
        assert!(!learn_attribute(&c, &mut st, "hp", 200, 3));
        assert!(learn_attribute(&c, &mut st, "hp", 200, 2));
        assert_eq!(st.attributes["hp"], 8);
        let mut hero = Actor::hero(ActorId(0), &c);
        sync_hero(&c, &mut hero, &st);
        assert_eq!(hero.base.str, 105);
        assert_eq!(hero.base.hp_max, 208);
        // XP / level via the gameplay rules: 500 XP -> level 1 and +10 LP.
        let mut st = GameState::new();
        assert_eq!(grant_xp(&mut st, 500), (0, 1));
        assert_eq!(st.learning_points, 10);
    }
}
