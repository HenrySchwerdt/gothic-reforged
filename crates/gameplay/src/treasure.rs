//! Treasure sets: the contents of plunder, chests and trader stock (`gCTreasureSet_PS` and the
//! `Generate*InvFromTreasureSets` methods of `gCInventory_PS`).
//!
//! A character or container names up to five treasure sets (`TreasureSet1..5`); each set has
//! a distribution that decides how its stacks are copied into the inventory. The original draws
//! with the C runtime's `rand()`; here every draw goes through [`Dice`] so tests (and the host)
//! can supply their own sequence.
use crate::inventory::{FindQuality, Inventory, ItemCatalog, StackType};
use std::collections::HashMap;

/// `gETreasureDistribution`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distribution {
    Plunder = 0,
    Unique = 1,
    Trophy = 2,
    Weaponry = 3,
    TradeGenerate = 4,
    TradeRefresh = 5,
    Mining = 6,
    Pickpocket = 7,
    TradeNotRandom = 8,
}

impl Distribution {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "plunder" => Self::Plunder,
            "unique" => Self::Unique,
            "trophy" => Self::Trophy,
            "weaponry" => Self::Weaponry,
            "trade_generate" => Self::TradeGenerate,
            "trade_refresh" => Self::TradeRefresh,
            "mining" => Self::Mining,
            "pickpocket" => Self::Pickpocket,
            "trade_notrandom" => Self::TradeNotRandom,
            _ => return None,
        })
    }
}

/// Source of random numbers (`rand() % n`).
pub trait Dice {
    /// A number in `0..n` (`n >= 1`).
    fn below(&mut self, n: u32) -> u32;
}

impl<F: FnMut(u32) -> u32> Dice for F {
    fn below(&mut self, n: u32) -> u32 {
        self(n)
    }
}

/// Small deterministic generator for the host (xorshift).
pub struct Roll(pub u64);

impl Dice for Roll {
    fn below(&mut self, n: u32) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        if n == 0 {
            0
        } else {
            ((self.0 >> 11) % u64::from(n)) as u32
        }
    }
}

/// One treasure set template: its distribution, the stack count range and its own stacks.
#[derive(Debug, Clone)]
pub struct TreasureSet {
    pub distribution: Distribution,
    pub min_stacks: i32,
    pub max_stacks: i32,
    /// The set's inventory (`gCInventory_PS` of the set entity). `Unique` sets mark what they
    /// already handed out in the stacks' transaction counts.
    pub stacks: Inventory,
}

/// All sets by lower-case name.
#[derive(Debug, Clone, Default)]
pub struct TreasureBook {
    sets: HashMap<String, TreasureSet>,
}

impl TreasureBook {
    pub fn insert(&mut self, name: &str, set: TreasureSet) {
        self.sets.insert(name.to_ascii_lowercase(), set);
    }

    pub fn get(&self, name: &str) -> Option<&TreasureSet> {
        self.sets.get(&name.to_ascii_lowercase())
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut TreasureSet> {
        self.sets.get_mut(&name.to_ascii_lowercase())
    }

    pub fn len(&self) -> usize {
        self.sets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }
}

/// What the generators ask of the hero.
pub trait HeroSkills {
    /// Is the skill (`Perk_TrophyTeeth`, `Perk_Mining`, ...) active? `None` when there is no
    /// hero at all.
    fn is_skill_active(&self, skill: &str) -> Option<bool>;
}

/// No hero: trophies are skipped, mining is not doubled.
pub struct NoHero;

impl HeroSkills for NoHero {
    fn is_skill_active(&self, _: &str) -> Option<bool> {
        None
    }
}

/// `n` items drawn between half of `n` and `n` (`rand() % (n - n/2 + 1) + n/2`).
fn half_to_full(n: i32, dice: &mut dyn Dice) -> i32 {
    if n < 2 {
        return n;
    }
    let low = n / 2;
    low + dice.below((n - low + 1) as u32) as i32
}

const USE_TROPHY_TEETH: u32 = 30;
const USE_TROPHY_SKIN: u32 = 41;
const USE_TROPHY_FUR: u32 = 50;

/// Port of gCTreasureSet_PS::GeneratePlunderInventory (Game:0x101b4d90): copy stacks of the
/// set into `target` as treasure stacks. Returns false for an empty set or a distribution this
/// method does not handle (weaponry, trader sets, pickpocket).
pub fn generate_plunder(
    set: &mut TreasureSet,
    target: &mut Inventory,
    dice: &mut dyn Dice,
    hero: &dyn HeroSkills,
) -> bool {
    gothic_portcov::hit!("generate_plunder");
    let n = set.stacks.len();
    if n == 0 {
        return false;
    }
    match set.distribution {
        Distribution::Plunder => {
            let (min, max) = (set.min_stacks, set.max_stacks);
            let count = if min == max {
                max.max(1)
            } else {
                let (lo, hi) = if max < min { (max, min) } else { (min, max) };
                lo + dice.below((hi - lo + 1) as u32) as i32
            };
            for _ in 0..count.max(0) {
                let i = if n > 1 {
                    dice.below(n as u32) as usize
                } else {
                    0
                };
                let s = set.stacks.stack(i).expect("index below len");
                let amount = half_to_full(s.amount, dice);
                target.add_items(
                    &s.template,
                    s.info.clone(),
                    amount,
                    s.combined_quality(),
                    StackType::Treasure,
                );
            }
            true
        }
        Distribution::Unique => {
            // the cheapest stack that was not handed out yet, else any
            let mut best: Option<(usize, i32)> = None;
            for (i, s) in set.stacks.stacks().iter().enumerate() {
                if s.transaction == 0
                    && s.info.is_some()
                    && best.is_none_or(|(_, v)| s.item_gold_value() < v)
                {
                    best = Some((i, s.item_gold_value()));
                }
            }
            let i = best.map_or_else(
                || {
                    if n > 1 {
                        dice.below(n as u32) as usize
                    } else {
                        0
                    }
                },
                |b| b.0,
            );
            let s = set.stacks.stack(i).expect("index below len");
            let amount = half_to_full(s.amount, dice);
            target.add_items(
                &s.template,
                s.info.clone(),
                amount,
                s.combined_quality(),
                StackType::Treasure,
            );
            set.stacks.set_transaction_amount(i, amount);
            true
        }
        Distribution::Trophy => {
            for s in set.stacks.stacks() {
                let perk = match s.use_type() {
                    USE_TROPHY_TEETH => Some("Perk_TrophyTeeth"),
                    USE_TROPHY_SKIN => Some("Perk_TrophySkin"),
                    USE_TROPHY_FUR => Some("Perk_TrophyFur"),
                    _ => None,
                };
                if let Some(perk) = perk
                    && hero.is_skill_active(perk) != Some(true)
                {
                    continue;
                }
                target.add_items(
                    &s.template,
                    s.info.clone(),
                    s.amount,
                    s.combined_quality(),
                    StackType::Treasure,
                );
            }
            true
        }
        Distribution::Mining => {
            let double = hero.is_skill_active("Perk_Mining") == Some(true);
            for s in set.stacks.stacks() {
                let amount = if double { s.amount * 2 } else { s.amount };
                target.add_items(
                    &s.template,
                    s.info.clone(),
                    amount,
                    s.quality,
                    StackType::Treasure,
                );
            }
            true
        }
        _ => false,
    }
}

/// Port of gCTreasureSet_PS::GenerateTradeInventory (Game:0x101b51f0): stock of a trader.
/// `TradeGenerate` draws `min..=max` random stacks (with replacement, half to full amount);
/// `TradeNotRandom` copies every stack, adding the template's own quality bits. Other
/// distributions do nothing.
pub fn generate_trade(set: &TreasureSet, target: &mut Inventory, dice: &mut dyn Dice) -> bool {
    gothic_portcov::hit!("generate_trade");
    match set.distribution {
        Distribution::TradeGenerate => {
            let n = set.stacks.len();
            let mut count = set.min_stacks.max(0) as u32;
            if count < set.max_stacks.max(0) as u32 {
                count += dice.below(set.max_stacks as u32 - count + 1);
            }
            if n == 0 || count == 0 {
                return true;
            }
            for _ in 0..count {
                let i = dice.below(n as u32) as usize;
                let s = set.stacks.stack(i).expect("index below len");
                let amount = half_to_full(s.amount, dice);
                target.add_items(
                    &s.template,
                    s.info.clone(),
                    amount,
                    s.combined_quality(),
                    StackType::Merchandise,
                );
            }
            true
        }
        Distribution::TradeNotRandom => {
            for s in set.stacks.stacks() {
                let quality = s.quality | s.item().quality;
                target.add_items(
                    &s.template,
                    s.info.clone(),
                    s.amount,
                    quality,
                    StackType::Merchandise,
                );
            }
            true
        }
        _ => true,
    }
}

/// Port of gCTreasureSet_PS::RefreshTradeInventory (Game:0x101b53d0): top the stock of a
/// `TradeRefresh` set up again; a stack that is missing or has fallen below half of the
/// set's amount is re-drawn (at least one item).
pub fn refresh_trade(set: &TreasureSet, target: &mut Inventory, dice: &mut dyn Dice) -> bool {
    gothic_portcov::hit!("refresh_trade");
    if set.distribution != Distribution::TradeRefresh {
        return true;
    }
    for s in set.stacks.stacks() {
        if s.info.is_none() {
            continue;
        }
        let q = s.combined_quality();
        let have = target.find_stack_index(&s.template, FindQuality::Exact, q, -1);
        let low = have < 0
            || target
                .stack(have as usize)
                .is_some_and(|t| t.amount < s.amount >> 1);
        if low {
            let amount = half_to_full(s.amount, dice).max(1);
            target.add_items(
                &s.template,
                s.info.clone(),
                amount,
                q,
                StackType::Merchandise,
            );
        }
    }
    true
}

/// Port of gCInventory_PS::GeneratePlunderInvFromTreasureSets (Game:0x100b71f0): once per
/// inventory, run every named set. Not ported: the 7 percent bonus `It_Carrot` for one
/// species. Returns true (also when the plunder was generated before).
pub fn generate_plunder_inventory(
    inv: &mut Inventory,
    book: &mut TreasureBook,
    dice: &mut dyn Dice,
    hero: &dyn HeroSkills,
) -> bool {
    gothic_portcov::hit!("generate_plunder_inventory");
    if inv.generated_plunder {
        return true;
    }
    let names = inv.treasure_sets.clone();
    for name in names.iter().filter(|n| !n.is_empty()) {
        if let Some(set) = book.get_mut(name) {
            generate_plunder(set, inv, dice, hero);
        }
    }
    inv.generated_plunder = true;
    true
}

/// Port of gCInventory_PS::GenerateTradeInvFromTreasureSets (Game:0x100b7340): stock a
/// trader once.
pub fn generate_trade_inventory(
    inv: &mut Inventory,
    book: &TreasureBook,
    dice: &mut dyn Dice,
) -> bool {
    gothic_portcov::hit!("generate_trade_inventory");
    if inv.generated_trade {
        return true;
    }
    let names = inv.treasure_sets.clone();
    for name in names.iter().filter(|n| !n.is_empty()) {
        if let Some(set) = book.get(name) {
            generate_trade(set, inv, dice);
        }
    }
    inv.generated_trade = true;
    true
}

/// Port of gCInventory_PS::RefreshTradeInvFromTreasureSets (Game:0x100b7400).
pub fn refresh_trade_inventory(
    inv: &mut Inventory,
    book: &TreasureBook,
    dice: &mut dyn Dice,
) -> bool {
    gothic_portcov::hit!("refresh_trade_inventory");
    let names = inv.treasure_sets.clone();
    for name in names.iter().filter(|n| !n.is_empty()) {
        if let Some(set) = book.get(name) {
            refresh_trade(set, inv, dice);
        }
    }
    true
}

/// Build a set from `(template, amount)` pairs (data loaders and tests).
pub fn set_from_pairs(
    cat: &dyn ItemCatalog,
    distribution: Distribution,
    min_stacks: i32,
    max_stacks: i32,
    pairs: &[(String, u32)],
) -> TreasureSet {
    let mut stacks = Inventory::new();
    for (t, n) in pairs {
        stacks.create_items(cat, t, 0, *n as i32);
    }
    TreasureSet {
        distribution,
        min_stacks,
        max_stacks,
        stacks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{Category, ItemInfo, MapCatalog};

    fn cat() -> MapCatalog {
        let mut c = MapCatalog::default();
        for (n, v, ut) in [
            ("It_Apple", 5, 0),
            ("It_Gem", 100, 0),
            ("It_Sword", 900, 2),
            ("It_Fur", 40, USE_TROPHY_FUR),
            ("It_Gold", 1, 0),
            ("It_Ore", 8, 0),
        ] {
            let mut i = ItemInfo::new(n, Category::Misc, ut);
            i.gold_value = v;
            c.insert(i);
        }
        c
    }

    /// Dice that replay a list of results (each reduced into the asked range).
    fn script(vals: &[u32]) -> impl Dice {
        let mut it = vals.iter().copied();
        move |n: u32| it.next().expect("dice exhausted") % n
    }

    struct Hero(&'static [&'static str]);
    impl HeroSkills for Hero {
        fn is_skill_active(&self, s: &str) -> Option<bool> {
            Some(self.0.contains(&s))
        }
    }

    fn pairs(v: &[(&str, u32)]) -> Vec<(String, u32)> {
        v.iter().map(|(a, b)| (a.to_string(), *b)).collect()
    }

    #[test]
    fn plunder_draws_the_count_then_stacks_and_half_to_full_amounts() {
        let c = cat();
        let mut set = set_from_pairs(
            &c,
            Distribution::Plunder,
            1,
            3,
            &pairs(&[("It_Apple", 6), ("It_Gem", 1)]),
        );
        let mut inv = Inventory::new();
        // count: 1 + (2 % 3) = 3; draws: stack 0 amount 3 + (3 % 4) = 6;
        // stack 1 (single item, no amount draw); stack 0 again 3 + (0 % 4) = 3
        let mut d = script(&[2, 0, 3, 1, 0, 0]);
        assert!(generate_plunder(&mut set, &mut inv, &mut d, &NoHero));
        assert_eq!(inv.count("it_apple"), 9);
        assert_eq!(inv.count("it_gem"), 1);
        assert!(inv.stacks().iter().all(|s| s.kind == StackType::Treasure));
    }

    #[test]
    fn equal_min_max_uses_the_value_and_zero_means_one() {
        let c = cat();
        let mut set = set_from_pairs(&c, Distribution::Plunder, 0, 0, &pairs(&[("It_Gem", 1)]));
        let mut inv = Inventory::new();
        let mut d = script(&[]);
        assert!(generate_plunder(&mut set, &mut inv, &mut d, &NoHero));
        assert_eq!(inv.count("it_gem"), 1);
        let mut set = set_from_pairs(&c, Distribution::Plunder, 3, 3, &pairs(&[("It_Gem", 1)]));
        let mut inv = Inventory::new();
        generate_plunder(&mut set, &mut inv, &mut script(&[]), &NoHero);
        assert_eq!(inv.count("it_gem"), 3);
    }

    #[test]
    fn empty_sets_and_other_distributions_fail() {
        let c = cat();
        let mut empty = set_from_pairs(&c, Distribution::Plunder, 1, 1, &[]);
        assert!(!generate_plunder(
            &mut empty,
            &mut Inventory::new(),
            &mut script(&[]),
            &NoHero
        ));
        let mut weap = set_from_pairs(&c, Distribution::Weaponry, 1, 1, &pairs(&[("It_Sword", 1)]));
        assert!(!generate_plunder(
            &mut weap,
            &mut Inventory::new(),
            &mut script(&[]),
            &NoHero
        ));
    }

    #[test]
    fn unique_hands_out_the_cheapest_unmarked_stack_once() {
        let c = cat();
        let mut set = set_from_pairs(
            &c,
            Distribution::Unique,
            1,
            1,
            &pairs(&[("It_Gem", 1), ("It_Apple", 1)]),
        );
        let mut inv = Inventory::new();
        generate_plunder(&mut set, &mut inv, &mut script(&[]), &NoHero);
        assert_eq!((inv.count("it_apple"), inv.count("it_gem")), (1, 0));
        generate_plunder(&mut set, &mut inv, &mut script(&[]), &NoHero);
        assert_eq!((inv.count("it_apple"), inv.count("it_gem")), (1, 1));
    }

    #[test]
    fn trophies_need_the_perk_and_mining_doubles() {
        let c = cat();
        let mut set = set_from_pairs(
            &c,
            Distribution::Trophy,
            0,
            0,
            &pairs(&[("It_Fur", 2), ("It_Apple", 1)]),
        );
        let mut inv = Inventory::new();
        generate_plunder(&mut set, &mut inv, &mut script(&[]), &Hero(&[]));
        assert_eq!((inv.count("it_fur"), inv.count("it_apple")), (0, 1));
        let mut inv = Inventory::new();
        generate_plunder(
            &mut set,
            &mut inv,
            &mut script(&[]),
            &Hero(&["Perk_TrophyFur"]),
        );
        assert_eq!(inv.count("it_fur"), 2);
        let mut mine = set_from_pairs(&c, Distribution::Mining, 0, 0, &pairs(&[("It_Ore", 3)]));
        let mut inv = Inventory::new();
        generate_plunder(
            &mut mine,
            &mut inv,
            &mut script(&[]),
            &Hero(&["Perk_Mining"]),
        );
        assert_eq!(inv.count("it_ore"), 6);
    }

    #[test]
    fn trader_stock_is_merchandise_and_generated_once() {
        let c = cat();
        let mut book = TreasureBook::default();
        book.insert(
            "TS_Trader",
            set_from_pairs(
                &c,
                Distribution::TradeNotRandom,
                0,
                0,
                &pairs(&[("It_Sword", 2), ("It_Gold", 300)]),
            ),
        );
        let mut inv = Inventory::new();
        inv.treasure_sets[0] = "ts_trader".into();
        generate_trade_inventory(&mut inv, &book, &mut script(&[]));
        assert!(inv.generated_trade);
        assert_eq!(inv.len(), 2);
        assert!(
            inv.stacks()
                .iter()
                .all(|s| s.kind == StackType::Merchandise)
        );
        // merchandise does not count as owned
        assert_eq!(inv.count("it_sword"), 0);
        // a second call changes nothing
        generate_trade_inventory(&mut inv, &book, &mut script(&[]));
        assert_eq!(inv.stacks().iter().map(|s| s.amount).sum::<i32>(), 302);
    }

    #[test]
    fn random_trade_sets_draw_stacks_with_replacement() {
        let c = cat();
        let set = set_from_pairs(
            &c,
            Distribution::TradeGenerate,
            2,
            2,
            &pairs(&[("It_Apple", 4), ("It_Gem", 1)]),
        );
        let mut inv = Inventory::new();
        // count 2 (min == max, no draw); stack 0 amount 2 + 1 = 3; stack 1 amount 1
        let mut d = script(&[0, 1, 1]);
        generate_trade(&set, &mut inv, &mut d);
        assert_eq!(inv.stacks().iter().map(|s| s.amount).sum::<i32>(), 4);
    }

    #[test]
    fn refresh_restocks_missing_and_low_stacks() {
        let c = cat();
        let set = set_from_pairs(
            &c,
            Distribution::TradeRefresh,
            0,
            0,
            &pairs(&[("It_Apple", 10), ("It_Gem", 4)]),
        );
        let mut inv = Inventory::new();
        inv.add_items("It_Gem", c.item("It_Gem"), 3, 0, StackType::Merchandise);
        // the apple is missing: 5 + (2 % 6) = 7; the gem stack holds 3 >= 4 / 2: untouched
        refresh_trade(&set, &mut inv, &mut script(&[2]));
        assert_eq!(
            inv.stacks()
                .iter()
                .find(|s| s.template == "It_Apple")
                .unwrap()
                .amount,
            7
        );
        assert_eq!(
            inv.stacks()
                .iter()
                .find(|s| s.template == "It_Gem")
                .unwrap()
                .amount,
            3
        );
    }

    #[test]
    fn distribution_names() {
        assert_eq!(
            Distribution::from_name("Trade_Generate"),
            Some(Distribution::TradeGenerate)
        );
        assert_eq!(Distribution::from_name("nope"), None);
    }
}
