//! The simulation side of the trade page: prices through the ported `OnPlayerGetGoldCost`,
//! the basket moves on the hero's and the trader's [`Inventory`], and the deal.
//!
//! The rules are `gothic_gameplay::trade`; this module supplies what they ask the script for
//! (prices and `OnWantsToSell`) and owns the two inventories.
use crate::attitude::AttitudeCtx;
use crate::economy::{self, PriceCtx, TradeSide};
use crate::item::ItemDef;
use crate::sim::Sim;
use crate::types::ActorId;
use crate::world::WorldQuery;
use gothic_gameplay::inventory::{Inventory, Stack, StackType, TransferContext};
use gothic_gameplay::trade::{self, GOLD_TEMPLATE, ListMode, Step, Totals};
use gothic_gameplay::treasure::{self, Dice, TreasureBook};

/// Use type of body armor (`gEUseType::Body`).
const USE_BODY: u32 = 114;

/// Why a deal did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DealError {
    NoHero,
    /// The baskets do not allow a deal (empty basket or the hero offers less than asked).
    NotAllowed,
}

/// Which price a list uses.
fn side_of(player_side: bool) -> TradeSide {
    if player_side {
        TradeSide::PlayerSells
    } else {
        TradeSide::PlayerBuys
    }
}

impl<W: WorldQuery> Sim<W> {
    /// Facts the price script reads: the barter perk and the trader's attitude.
    pub fn trade_price_ctx(&self, npc: ActorId) -> Option<PriceCtx> {
        let hero = self.hero?;
        Some(PriceCtx {
            has_barter_perk: self.actor(hero).has_perk("perk_barter"),
            npc_friendly: self.attitude(npc, hero) == crate::types::Attitude::Friendly,
        })
    }

    /// Port of the `OnPlayerGetGoldCost` call of the list controls: the price of one item of
    /// `stack` on the trader's list (`PlayerBuys`) or the hero's list (`PlayerSells`). Gold
    /// is worth one gold; unknown templates fall back to their template value.
    pub fn trade_unit_price(&self, ctx: PriceCtx, stack: &Stack, player_side: bool) -> i32 {
        if stack.template.eq_ignore_ascii_case(GOLD_TEMPLATE) {
            return 1;
        }
        match self.items.get(&stack.template) {
            Some(def) => economy::trade_price(&self.cfg, def, side_of(player_side), ctx),
            None => trade::fallback_price(stack),
        }
    }

    /// `OnWantsToSell` (S:6b020): faction body armor from a trader needs political fame with
    /// the trader's party, depending on its gold value [CA 8.3].
    pub fn trade_wants_to_sell(&self, npc: ActorId, stack: &Stack, player_side: bool) -> bool {
        if player_side || stack.use_type() != USE_BODY {
            return true;
        }
        let ctx = AttitudeCtx {
            cfg: &self.cfg,
            chapter: self.chapter,
            fame: &self.state.fame,
            enclaves: &self.enclaves,
            player_alignment: self.player_alignment,
        };
        let need = economy::armor_fame_requirement(&self.cfg, stack.item_gold_value());
        ctx.political_fame(self.actor(npc).alignment) >= need
    }

    /// Port of the page opening (`BeginTrade`): stock the trader once from its trade sets and
    /// top the refresh sets up (Game:0x100b7340 / 0x100b7400). All marks start empty.
    pub fn trade_open(&mut self, npc: ActorId, book: &TreasureBook, dice: &mut dyn Dice) {
        let inv = &mut self.actors[npc.0 as usize].inventory;
        treasure::generate_trade_inventory(inv, book, dice);
        treasure::refresh_trade_inventory(inv, book, dice);
        trade::clear_baskets(inv);
        if let Some(h) = self.hero {
            trade::clear_baskets(&mut self.actors[h.0 as usize].inventory);
        }
    }

    /// Run `f` with the hero's and the trader's inventory (both taken out of the actors for
    /// the call).
    fn with_inventories<R>(
        &mut self,
        npc: ActorId,
        f: impl FnOnce(&Self, &mut Inventory, &mut Inventory) -> R,
    ) -> Option<R> {
        let hero = self.hero?;
        if hero == npc {
            return None;
        }
        let mut hi = std::mem::take(&mut self.actors[hero.0 as usize].inventory);
        let mut ti = std::mem::take(&mut self.actors[npc.0 as usize].inventory);
        let r = f(self, &mut hi, &mut ti);
        self.actors[hero.0 as usize].inventory = hi;
        self.actors[npc.0 as usize].inventory = ti;
        Some(r)
    }

    /// Value of both baskets (port of the totals of FUN_100813d0).
    pub fn trade_totals(&mut self, npc: ActorId) -> Totals {
        let Some(ctx) = self.trade_price_ctx(npc) else {
            return Totals::default();
        };
        self.with_inventories(npc, |s, hi, ti| {
            trade::totals(hi, ti, &|st| s.trade_unit_price(ctx, st, true), &|st| {
                s.trade_unit_price(ctx, st, false)
            })
        })
        .unwrap_or_default()
    }

    /// One click on a stack. `add`: move `step` more into the basket from the list of that
    /// side; otherwise take `step` back out of the basket. Moves are refused for stacks that
    /// [`trade::can_move`] or `OnWantsToSell` do not allow.
    pub fn trade_step(
        &mut self,
        npc: ActorId,
        player_side: bool,
        idx: usize,
        add: bool,
        step: i32,
    ) -> Step {
        self.with_inventories(npc, |s, hi, ti| {
            let inv = if player_side { hi } else { ti };
            let mode = if add {
                ListMode::Remaining
            } else {
                ListMode::Basket
            };
            let Some(st) = inv.stack(idx) else {
                return Step::Refused;
            };
            if !trade::can_move(st, mode, player_side) {
                return Step::Refused;
            }
            trade::step_basket(inv, idx, mode, step, &|st| {
                s.trade_wants_to_sell(npc, st, player_side)
            })
        })
        .unwrap_or(Step::Refused)
    }

    /// Empty one basket (trash button, FUN_100a1f20).
    pub fn trade_clear(&mut self, npc: ActorId, player_side: bool) {
        self.with_inventories(npc, |_, hi, ti| {
            trade::clear_baskets(if player_side { hi } else { ti });
        });
    }

    /// The gold balance button (FUN_100811a0). True when a gold mark changed.
    pub fn trade_balance(&mut self, npc: ActorId) -> bool {
        let Some(ctx) = self.trade_price_ctx(npc) else {
            return false;
        };
        self.with_inventories(npc, |s, hi, ti| {
            trade::balance_gold(hi, ti, &|st| s.trade_unit_price(ctx, st, true), &|st| {
                s.trade_unit_price(ctx, st, false)
            })
        })
        .unwrap_or(false)
    }

    /// The deal (FUN_1008a0b0) when the baskets allow it. The trader's gold is merchandise
    /// like everything else it received.
    pub fn trade_deal(&mut self, npc: ActorId) -> Result<(), DealError> {
        if self.hero.is_none() {
            return Err(DealError::NoHero);
        }
        if !self.trade_totals(npc).deal_enabled() {
            return Err(DealError::NotAllowed);
        }
        let name = self.actor(npc).name.clone();
        self.with_inventories(npc, |_, hi, ti| {
            trade::commit(
                hi,
                ti,
                &TransferContext {
                    dst_is_player: false,
                    dst_name: &name,
                    dst_is_npc: true,
                    trade_page_open: true,
                },
                &TransferContext {
                    trade_page_open: true,
                    ..TransferContext::default()
                },
            );
        });
        Ok(())
    }

    /// Closing the page (FUN_100a1f20 on both lists): the baskets are abandoned.
    pub fn trade_cancel(&mut self, npc: ActorId) {
        self.with_inventories(npc, |_, hi, ti| trade::cancel(hi, ti));
    }

    /// Take one stack (`Some(idx)`) or everything from a body's inventory into the hero's
    /// (loot page). Equipped / in-use stacks stay. Returns `(template, amount)` taken.
    pub fn loot_body(&mut self, body: ActorId, idx: Option<usize>) -> Vec<(String, i32)> {
        self.with_inventories(body, |_, hi, bi| {
            let ctx = TransferContext::default();
            let mut taken = Vec::new();
            let mut take = |bi: &mut Inventory, i: usize| -> bool {
                let Some(s) = bi.stack(i) else { return false };
                let (t, n) = (s.template.clone(), s.amount);
                if bi
                    .transfer_stack_to(hi, i, StackType::Normal, &ctx)
                    .is_some()
                {
                    taken.push((t, n));
                    true
                } else {
                    false
                }
            };
            match idx {
                Some(i) => {
                    take(bi, i);
                }
                None => {
                    let mut i = 0;
                    while i < bi.len() {
                        // a moved stack shifts the rest down
                        if !take(bi, i) {
                            i += 1;
                        }
                    }
                }
            }
            taken
        })
        .unwrap_or_default()
    }

    /// The item definition behind a trade-page row.
    pub fn trade_item(&self, template: &str) -> Option<&ItemDef> {
        self.items.get(template)
    }

    /// Merchandise filter used by the page: the trader's list shows only what it offers.
    pub fn trader_offers(stack: &Stack) -> bool {
        stack.kind == StackType::Merchandise && stack.amount > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::Actor;
    use crate::config::SimConfig;
    use crate::item::{ItemDb, ItemDef};
    use crate::types::Species;
    use crate::world::{Grid2dWorld, Pos};
    use gothic_gameplay::inventory::{FindQuality, MapCatalog};

    fn sword() -> ItemDef {
        ItemDef {
            name: "It_1H_Sword_01".into(),
            category: "Weapon".into(),
            use_type: Some("1H".into()),
            value: 1000,
            quality: 0,
            robe: false,
            damage: None,
            requirements: Vec::new(),
            required_skills: Vec::new(),
            modifiers: Vec::new(),
            spell: None,
            permanent: false,
            mission_item: false,
            activate_skills: Vec::new(),
            skill: None,
        }
    }

    fn world() -> (Sim<Grid2dWorld>, ActorId, ActorId) {
        let mut db = ItemDb::default();
        db.insert(sword());
        let mut gold = sword();
        gold.name = "It_Gold".into();
        gold.value = 1;
        gold.category = "Misc".into();
        gold.use_type = None;
        db.insert(gold);
        let mut sim = Sim::new(SimConfig::default(), Grid2dWorld::new(), db);
        let hero = sim.spawn_hero(Pos::default());
        let smith = sim.spawn(
            Actor::npc(ActorId(0), "Smith", Species::Human, 5, 5),
            Pos::flat(100.0, 0.0),
        );
        (sim, hero, smith)
    }

    #[test]
    fn a_sword_is_bought_with_the_shipped_prices() {
        let (mut sim, hero, smith) = world();
        let cat = MapCatalog::default();
        let _ = cat;
        let info = |s: &Sim<Grid2dWorld>, t: &str| {
            s.items.get(t).map(|d| std::sync::Arc::new(d.to_info()))
        };
        let (sw, gd) = (info(&sim, "It_1H_Sword_01"), info(&sim, "It_Gold"));
        sim.actors[smith.0 as usize].inventory.add_items(
            "It_1H_Sword_01",
            sw,
            1,
            0,
            StackType::Merchandise,
        );
        sim.actors[hero.0 as usize]
            .inventory
            .add_items("It_Gold", gd, 1500, 0, StackType::Normal);
        let ti = sim.actors[smith.0 as usize].inventory.find_stack_index(
            "It_1H_Sword_01",
            FindQuality::Any,
            0,
            -1,
        ) as usize;
        assert_eq!(sim.trade_step(smith, false, ti, true, 1), Step::Done);
        // the smith is friendly to a fresh hero: 1000 * 0.9
        assert_eq!(sim.trade_totals(smith).ask, 900);
        // the hero adds the 900 gold and the deal opens
        assert!(sim.trade_balance(smith));
        assert!(sim.trade_totals(smith).deal_enabled());
        sim.trade_deal(smith).unwrap();
        let hi = &sim.actors[hero.0 as usize].inventory;
        assert_eq!((hi.count("it_1h_sword_01"), hi.count("it_gold")), (1, 600));
        // no second deal without baskets
        assert_eq!(sim.trade_deal(smith), Err(DealError::NotAllowed));
    }

    #[test]
    fn selling_pays_the_barter_price_and_the_smith_gives_change() {
        let (mut sim, hero, smith) = world();
        let info = |s: &Sim<Grid2dWorld>, t: &str| {
            s.items.get(t).map(|d| std::sync::Arc::new(d.to_info()))
        };
        let (sw, gd) = (info(&sim, "It_1H_Sword_01"), info(&sim, "It_Gold"));
        sim.actors[hero.0 as usize].inventory.add_items(
            "It_1H_Sword_01",
            sw,
            1,
            0,
            StackType::Normal,
        );
        sim.actors[smith.0 as usize].inventory.add_items(
            "It_Gold",
            gd,
            400,
            0,
            StackType::Merchandise,
        );
        let hi = sim.actors[hero.0 as usize].inventory.find_stack_index(
            "It_1H_Sword_01",
            FindQuality::Any,
            0,
            -1,
        ) as usize;
        assert_eq!(sim.trade_step(smith, true, hi, true, 1), Step::Done);
        let t = sim.trade_totals(smith);
        // friendly smith: 1000 * (0.3 + 0.03) - 0.05 per unit value = 280
        assert_eq!((t.offer, t.ask), (280, 0));
        // the trader basket is empty: the balance puts the 280 gold into it
        assert!(sim.trade_balance(smith));
        sim.trade_deal(smith).unwrap();
        let hi = &sim.actors[hero.0 as usize].inventory;
        assert_eq!((hi.count("it_1h_sword_01"), hi.count("it_gold")), (0, 280));
    }

    #[test]
    fn the_hero_cannot_sell_what_he_does_not_hold_or_buy_non_merchandise() {
        let (mut sim, hero, smith) = world();
        let info = sim
            .items
            .get("It_1H_Sword_01")
            .map(|d| std::sync::Arc::new(d.to_info()));
        sim.actors[smith.0 as usize].inventory.add_items(
            "It_1H_Sword_01",
            info,
            1,
            0,
            StackType::Normal,
        );
        assert_eq!(sim.trade_step(smith, false, 0, true, 1), Step::Refused);
        assert_eq!(sim.trade_step(smith, true, 5, true, 1), Step::Refused);
        let _ = hero;
    }
}

#[cfg(test)]
mod stat_tests {
    use super::*;
    use crate::config::SimConfig;
    use crate::item::{ItemDb, ItemDef, Modifier};
    use crate::world::{Grid2dWorld, Pos};
    use gothic_gameplay::inventory::StackType;
    use std::sync::Arc;

    fn def(name: &str, use_type: &str, mods: &[(&str, i32, i32)]) -> ItemDef {
        ItemDef {
            name: name.into(),
            category: if use_type == "Ring" {
                "Artefact"
            } else {
                "Food"
            }
            .into(),
            use_type: Some(use_type.into()),
            value: 10,
            quality: 0,
            robe: false,
            damage: None,
            requirements: Vec::new(),
            required_skills: Vec::new(),
            modifiers: mods
                .iter()
                .map(|(t, op, v)| Modifier {
                    tag: (*t).into(),
                    op: *op,
                    value: *v,
                })
                .collect(),
            spell: None,
            permanent: false,
            mission_item: false,
            activate_skills: Vec::new(),
            skill: None,
        }
    }

    /// Worn equipment feeds temporary modifiers into the hero's stats, taking it off removes
    /// them; consumables change values for good.
    #[test]
    fn inventory_events_reach_the_hero_stats() {
        let mut db = ItemDb::default();
        db.insert(def(
            "It_Ring_Might",
            "Ring",
            &[("STR", 0, 10), ("HP", 1, 50), ("PROT_BLADE", 0, 5)],
        ));
        db.insert(def("It_Potion_Heal", "Potion", &[("HP", 2, 30)]));
        let mut sim = Sim::new(SimConfig::default(), Grid2dWorld::new(), db);
        let hero = sim.spawn_hero(Pos::default());
        let (str0, hp_max0) = {
            let h = sim.actor(hero);
            (h.base.str, h.base.hp_max)
        };
        let ring = Arc::new(sim.items.get("It_Ring_Might").unwrap().to_info());
        let potion = Arc::new(sim.items.get("It_Potion_Heal").unwrap().to_info());
        let inv = &mut sim.actors[hero.0 as usize].inventory;
        let r = inv.add_items("It_Ring_Might", Some(ring), 1, 0, StackType::Normal) as usize;
        let p = inv.add_items("It_Potion_Heal", Some(potion), 1, 0, StackType::Normal) as usize;
        assert!(inv.equip_stack(r));
        sim.step();
        let cfg = sim.cfg.clone();
        let h = sim.actor(hero);
        assert_eq!(h.strength(&cfg), str0 + 10);
        assert_eq!(h.hp_max(&cfg), hp_max0 + 50);
        // the value moved with the maximum: 200 + 200 * 50 / 200 = 250
        assert_eq!(h.hp, 250);
        assert_eq!(h.stats.modifier("PROT_BLADE"), 5);
        // unequip takes everything back
        let inv = &mut sim.actors[hero.0 as usize].inventory;
        assert!(inv.unequip_stack(r));
        sim.step();
        let h = sim.actor(hero);
        assert_eq!(h.strength(&cfg), str0);
        assert_eq!(h.hp_max(&cfg), hp_max0);
        assert_eq!(h.hp, hp_max0);
        assert!(h.stats.is_empty());
        // a health potion (+30 percent of the maximum) heals for good
        sim.actors[hero.0 as usize].hp = 50;
        let inv = &mut sim.actors[hero.0 as usize].inventory;
        assert!(inv.consume_items(p, 1).ok);
        sim.step();
        assert_eq!(sim.actor(hero).hp, 50 + hp_max0 * 30 / 100);
    }
}

#[cfg(test)]
mod loot_tests {
    use super::*;
    use crate::actor::Actor;
    use crate::config::SimConfig;
    use crate::item::{ItemDb, ItemDef};
    use crate::types::Species;
    use crate::world::{Grid2dWorld, Pos};

    fn plain(name: &str) -> ItemDef {
        ItemDef {
            name: name.into(),
            category: "Misc".into(),
            use_type: None,
            value: 3,
            quality: 0,
            robe: false,
            damage: None,
            requirements: Vec::new(),
            required_skills: Vec::new(),
            modifiers: Vec::new(),
            spell: None,
            permanent: false,
            mission_item: false,
            activate_skills: Vec::new(),
            skill: None,
        }
    }

    #[test]
    fn plunder_moves_one_stack_or_all_into_the_hero_inventory() {
        let mut db = ItemDb::default();
        db.insert(plain("It_Apple"));
        db.insert(plain("It_Bone"));
        let mut sim = Sim::new(SimConfig::default(), Grid2dWorld::new(), db);
        let hero = sim.spawn_hero(Pos::default());
        let body = sim.spawn(
            Actor::npc(ActorId(0), "Wolf", Species::Human, 1, 1),
            Pos::flat(50.0, 0.0),
        );
        for (t, n) in [("It_Apple", 3), ("It_Bone", 2)] {
            let info = sim.items.get(t).map(|d| std::sync::Arc::new(d.to_info()));
            sim.actors[body.0 as usize]
                .inventory
                .add_items(t, info, n, 0, StackType::Normal);
        }
        assert_eq!(
            sim.loot_body(body, Some(1)),
            vec![("It_Bone".to_string(), 2)]
        );
        assert_eq!(sim.actor(hero).inventory.count("it_bone"), 2);
        assert_eq!(sim.actor(body).inventory.count("it_bone"), 0);
        let all = sim.loot_body(body, None);
        assert_eq!(all, vec![("It_Apple".to_string(), 3)]);
        assert!(sim.actor(body).inventory.is_empty());
        assert_eq!(sim.actor(hero).inventory.count("it_apple"), 3);
    }
}
