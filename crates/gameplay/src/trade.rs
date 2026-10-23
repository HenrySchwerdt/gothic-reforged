//! The trade basket of the HUD trade page (`gCHUDPageTrade2` and its list controls).
//!
//! Moving an item in the trade window only edits the stack's `TransactionCount` (the basket);
//! gold is another stack (`It_Gold`) whose mark is raised to even out the two baskets; the
//! deal itself is `TransferTransactionItemsTo` in both directions. Prices come from the
//! script (`OnPlayerGetGoldCost`), so every function that needs one takes the price as a
//! closure. Everything here works on plain [`Inventory`] values: the host decides which one
//! belongs to the hero and which to the trader.
use crate::inventory::{FindQuality, Inventory, Stack, StackType, Transfer, TransferContext};

/// Template name of the gold stack.
pub const GOLD_TEMPLATE: &str = "It_Gold";

/// What a trade list shows and totals (field `+0x38` of `gCHUDListCtrlInvTradeStacks`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListMode {
    /// The inventory side: what is left after the marked amount.
    Remaining = 0,
    /// The basket side: the marked amount.
    Basket = 1,
}

/// Port of the number shown in `gCHUDListCtrlInvTradeStacks` (Game:0x100a2100) and summed by
/// the basket total (Game:0x100a1dc0): remaining = amount minus the mark, basket = the mark;
/// a negative mark or one above the amount counts as the whole stack.
pub fn list_count(stack: &Stack, mode: ListMode) -> i32 {
    gothic_portcov::hit!("list_count");
    let mark = if (0..=stack.amount).contains(&stack.transaction) {
        stack.transaction
    } else {
        stack.amount
    };
    match mode {
        ListMode::Remaining => stack.amount - mark,
        ListMode::Basket => mark,
    }
}

/// Price of one item without the script (`max(1, GoldValue)`, the fallback when no script
/// host is attached to the list).
pub fn fallback_price(stack: &Stack) -> i32 {
    stack.item_gold_value().max(1)
}

/// Port of FUN_100a1dc0 (`gCHUDListCtrlInvTradeStacks`, Game:0x100a1dc0): the value of a
/// list, `sum(price(stack) * count)`. (The original skips stacks whose template has no item
/// property set; a stack without resolved data is priced by the closure here.) `price` is the
/// script's `OnPlayerGetGoldCost` for the list's transaction type.
pub fn basket_total(inv: &Inventory, mode: ListMode, price: &dyn Fn(&Stack) -> i32) -> i64 {
    gothic_portcov::hit!("basket_total");
    inv.stacks()
        .iter()
        .map(|s| i64::from(price(s)) * i64::from(list_count(s, mode)))
        .sum()
}

/// Port of FUN_100a1c80 (`gCHUDListCtrlInvTradeStacks`, Game:0x100a1c80): may the stack be
/// moved into the basket? Something has to be left to move; the hero's own list refuses
/// mission items, the trader's list only offers merchandise; equipped / in-use stacks stay.
pub fn can_move(stack: &Stack, mode: ListMode, owner_is_player: bool) -> bool {
    gothic_portcov::hit!("can_move");
    if list_count(stack, mode) < 1 {
        return false;
    }
    if owner_is_player {
        if stack.item().mission_item {
            return false;
        }
    } else if stack.kind != StackType::Merchandise {
        return false;
    }
    stack.is_disposable()
}

/// Outcome of one [`step_basket`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The mark changed and more can follow.
    Moved,
    /// The mark reached its end (everything in the basket / nothing left in it): the key
    /// repeat restarts.
    Done,
    /// The script refused the sale (`OnWantsToSell`) or the index is invalid.
    Refused,
}

/// Port of FUN_100a1fa0 (Game:0x100a1fa0): one press (or key repeat) of the transfer
/// control. On the inventory side the mark grows by `step` (at most to the whole stack, which
/// ends the repeat); on the basket side it shrinks by `step` (to zero ends the repeat).
/// `wants_to_sell` is the script's `OnWantsToSell` check for the inventory side.
pub fn step_basket(
    inv: &mut Inventory,
    idx: usize,
    mode: ListMode,
    step: i32,
    wants_to_sell: &dyn Fn(&Stack) -> bool,
) -> Step {
    gothic_portcov::hit!("step_basket");
    let Some(s) = inv.stack(idx) else {
        return Step::Refused;
    };
    let amount = s.amount;
    let mark = if (0..=amount).contains(&s.transaction) {
        s.transaction
    } else {
        amount
    };
    let (new, done) = match mode {
        ListMode::Remaining => {
            if !wants_to_sell(s) {
                return Step::Refused;
            }
            let n = mark + step;
            if n >= amount {
                (amount, true)
            } else {
                (n, false)
            }
        }
        ListMode::Basket => {
            let n = mark - step;
            if n < 1 { (0, true) } else { (n, false) }
        }
    };
    inv.set_transaction_amount(idx, new);
    if done { Step::Done } else { Step::Moved }
}

/// The key-repeat acceleration of the list controls (`+0x50` / `+0x54`, Game:0x100a1bd0):
/// after every tenth repeat the step grows tenfold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Repeater {
    pub count: i32,
    pub step: i32,
}

impl Default for Repeater {
    fn default() -> Self {
        Self { count: 0, step: 1 }
    }
}

impl Repeater {
    /// A step has been taken (count + 1, as in FUN_100a1fa0).
    pub fn stepped(&mut self) {
        self.count += 1;
    }

    /// The repeat timer fired once more: after a multiple of ten the count moves on and the
    /// step is multiplied by ten.
    pub fn repeat(&mut self) {
        if self.count % 10 == 0 {
            self.count += 1;
            self.step = self.step.saturating_mul(10);
        }
    }

    /// The end of the range was reached or the key was released.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Port of FUN_100a1f20 (Game:0x100a1f20): clear every mark of an inventory.
pub fn clear_baskets(inv: &mut Inventory) {
    gothic_portcov::hit!("clear_baskets");
    for i in 0..inv.len() {
        inv.set_transaction_amount(i, 0);
    }
}

/// Free gold of a gold stack: amount minus the mark already in the basket.
fn free_gold(inv: &Inventory, idx: usize) -> (i32, i32) {
    let s = inv.stack(idx).expect("index from find");
    let mark = if (0..=s.amount).contains(&s.transaction) {
        s.transaction
    } else {
        s.amount
    };
    (s.amount - mark, mark)
}

fn gold_index(inv: &Inventory) -> Option<usize> {
    let i = inv.find_stack_index(GOLD_TEMPLATE, FindQuality::Any, 0, -1);
    (i >= 0).then_some(i as usize)
}

/// What the two baskets are worth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Totals {
    /// Value of the trader's basket (the price to pay).
    pub ask: i64,
    /// Value of the hero's basket.
    pub offer: i64,
}

impl Totals {
    /// Port of the enable rule of FUN_100813d0 (Game:0x100813d0): the deal needs both baskets
    /// filled and the hero's at least as valuable as the trader's.
    pub fn deal_enabled(&self) -> bool {
        gothic_portcov::hit!("deal_enabled");
        self.offer >= self.ask && self.offer > 0 && self.ask > 0
    }

    /// The gold balance button is live while the baskets differ.
    pub fn balance_enabled(&self) -> bool {
        self.offer != self.ask
    }
}

/// Totals of the two baskets (both in [`ListMode::Basket`]).
pub fn totals(
    player: &Inventory,
    trader: &Inventory,
    price_player: &dyn Fn(&Stack) -> i32,
    price_trader: &dyn Fn(&Stack) -> i32,
) -> Totals {
    Totals {
        ask: basket_total(trader, ListMode::Basket, price_trader),
        offer: basket_total(player, ListMode::Basket, price_player),
    }
}

/// Port of FUN_100811a0 (Game:0x100811a0), the gold balance: the poorer basket gets the
/// difference as gold from its owner's gold stack if that much is free. Returns whether a
/// mark was changed.
pub fn balance_gold(
    player: &mut Inventory,
    trader: &mut Inventory,
    price_player: &dyn Fn(&Stack) -> i32,
    price_trader: &dyn Fn(&Stack) -> i32,
) -> bool {
    gothic_portcov::hit!("balance_gold");
    let t = totals(player, trader, price_player, price_trader);
    let diff = t.ask - t.offer;
    let (side, need) = match diff {
        0 => return false,
        // the hero offers less than is asked: the hero adds gold
        d if d > 0 => (&mut *player, d),
        // the hero offers more: the trader adds the change
        d => (&mut *trader, -d),
    };
    let Some(i) = gold_index(side) else {
        return false;
    };
    let (free, mark) = free_gold(side, i);
    let Ok(need) = i32::try_from(need) else {
        return false;
    };
    if need <= free {
        side.set_transaction_amount(i, mark + need);
        true
    } else {
        false
    }
}

/// Port of FUN_1008a0b0 (Game:0x1008a0b0), the deal: the hero's basket goes to the trader as
/// merchandise, the trader's basket to the hero as ordinary stacks. Returns both transfer
/// lists (hero -> trader first).
pub fn commit(
    player: &mut Inventory,
    trader: &mut Inventory,
    ctx_trader: &TransferContext,
    ctx_player: &TransferContext,
) -> (Vec<Transfer>, Vec<Transfer>) {
    gothic_portcov::hit!("commit");
    let sold = player.transfer_transaction_items_to(trader, StackType::Merchandise, ctx_trader);
    let bought = trader.transfer_transaction_items_to(player, StackType::Normal, ctx_player);
    (sold, bought)
}

/// Port of the close handler of the page (FUN_100a1f20 on both lists): abandon the deal.
pub fn cancel(player: &mut Inventory, trader: &mut Inventory) {
    gothic_portcov::hit!("cancel");
    clear_baskets(player);
    clear_baskets(trader);
}

/// Port of gCHUDListCtrlInvLootStacks key handler (Game:0x100a1790): digit keys jump to the
/// n-th stack; `0` is the tenth. Backspace (code 8) selects none.
pub fn loot_key_index(key: char, count: usize) -> Option<usize> {
    gothic_portcov::hit!("loot_key_index");
    let n = key.to_digit(10)? as usize;
    let i = if n == 0 { 9 } else { n - 1 };
    (i < count).then_some(i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{Category, ItemInfo, MapCatalog};

    fn catalog() -> MapCatalog {
        let mut c = MapCatalog::default();
        let mut gold = ItemInfo::new("It_Gold", Category::Misc, 0);
        gold.gold_value = 1;
        c.insert(gold);
        let mut sword = ItemInfo::new("It_Sword", Category::Weapon, 1);
        sword.gold_value = 1000;
        c.insert(sword);
        let mut key = ItemInfo::new("It_Key", Category::Misc, 45);
        key.mission_item = true;
        c.insert(key);
        c.insert(ItemInfo::new("It_Cheap", Category::Misc, 0));
        c
    }

    fn price(s: &Stack) -> i32 {
        // buying costs the value, selling pays a quarter (shipped numbers)
        if s.kind == StackType::Merchandise {
            s.item_gold_value().max(1)
        } else if s.template.eq_ignore_ascii_case(GOLD_TEMPLATE) {
            1
        } else {
            (s.item_gold_value() / 4).max(1)
        }
    }

    fn setup() -> (Inventory, Inventory, MapCatalog) {
        let c = catalog();
        let mut hero = Inventory::new();
        hero.create_items(&c, "It_Gold", 0, 1500);
        hero.create_items(&c, "It_Sword", 0, 2);
        hero.create_items(&c, "It_Key", 0, 1);
        let mut trader = Inventory::new();
        for (t, n) in [("It_Sword", 3), ("It_Gold", 500)] {
            let info = c.item(t);
            trader.add_items(t, info, n, 0, StackType::Merchandise);
        }
        (hero, trader, c)
    }

    use crate::inventory::ItemCatalog;

    #[test]
    fn list_counts_follow_the_mark_rules() {
        let c = catalog();
        let mut inv = Inventory::new();
        inv.create_items(&c, "It_Sword", 0, 5);
        assert_eq!(list_count(inv.stack(0).unwrap(), ListMode::Remaining), 5);
        inv.set_transaction_amount(0, 2);
        let s = inv.stack(0).unwrap();
        assert_eq!(list_count(s, ListMode::Remaining), 3);
        assert_eq!(list_count(s, ListMode::Basket), 2);
        inv.set_transaction_amount(0, -1);
        // a negative mark means the whole stack
        assert_eq!(list_count(inv.stack(0).unwrap(), ListMode::Basket), 5);
    }

    #[test]
    fn stepping_up_clamps_at_the_amount_and_ends_the_repeat() {
        let (mut hero, _, _) = setup();
        let sword = hero.find_stack_index("It_Sword", FindQuality::Any, 0, -1) as usize;
        let ok = |_: &Stack| true;
        assert_eq!(
            step_basket(&mut hero, sword, ListMode::Remaining, 1, &ok),
            Step::Moved
        );
        assert_eq!(hero.transaction_amount(sword), 1);
        assert_eq!(
            step_basket(&mut hero, sword, ListMode::Remaining, 10, &ok),
            Step::Done
        );
        assert_eq!(hero.transaction_amount(sword), 2);
        assert_eq!(
            step_basket(&mut hero, sword, ListMode::Basket, 1, &ok),
            Step::Moved
        );
        assert_eq!(hero.transaction_amount(sword), 1);
        assert_eq!(
            step_basket(&mut hero, sword, ListMode::Basket, 1, &ok),
            Step::Done
        );
        assert_eq!(hero.transaction_amount(sword), 0);
        // a refused sale does not move anything
        let no = |_: &Stack| false;
        assert_eq!(
            step_basket(&mut hero, sword, ListMode::Remaining, 1, &no),
            Step::Refused
        );
        assert_eq!(hero.transaction_amount(sword), 0);
    }

    #[test]
    fn repeat_accelerates_tenfold_after_ten_steps() {
        let mut r = Repeater::default();
        r.repeat(); // count 0 is a multiple of ten
        assert_eq!((r.count, r.step), (1, 10));
        r.repeat(); // 1 is not
        assert_eq!((r.count, r.step), (1, 10));
        r.count = 10;
        r.repeat();
        assert_eq!((r.count, r.step), (11, 100));
        r.reset();
        assert_eq!(r, Repeater::default());
    }

    #[test]
    fn only_permitted_stacks_move() {
        let (hero, trader, _) = setup();
        let key = hero.find_stack_index("It_Key", FindQuality::Any, 0, -1) as usize;
        assert!(!can_move(
            hero.stack(key).unwrap(),
            ListMode::Remaining,
            true
        ));
        assert!(can_move(hero.stack(0).unwrap(), ListMode::Remaining, true));
        assert!(can_move(
            trader.stack(0).unwrap(),
            ListMode::Remaining,
            false
        ));
        let mut plain = trader.clone();
        plain.add_items("It_Cheap", None, 1, 0, StackType::Normal);
        assert!(!can_move(
            plain.stack(2).unwrap(),
            ListMode::Remaining,
            false
        ));
    }

    #[test]
    fn buying_a_sword_balances_with_gold_and_trades() {
        let (mut hero, mut trader, _) = setup();
        let ts = trader.find_stack_index("It_Sword", FindQuality::Any, 0, -1) as usize;
        trader.set_transaction_amount(ts, 1);
        let t = totals(&hero, &trader, &price, &price);
        assert_eq!((t.ask, t.offer), (1000, 0));
        assert!(!t.deal_enabled());
        // the hero adds the missing 1000 gold
        assert!(balance_gold(&mut hero, &mut trader, &price, &price));
        let gi = hero.find_stack_index("It_Gold", FindQuality::Any, 0, -1) as usize;
        assert_eq!(hero.transaction_amount(gi), 1000);
        let t = totals(&hero, &trader, &price, &price);
        assert!(t.deal_enabled() && !t.balance_enabled());
        let (sold, bought) = commit(
            &mut hero,
            &mut trader,
            &TransferContext {
                dst_is_player: false,
                dst_name: "Trader",
                dst_is_npc: true,
                trade_page_open: true,
            },
            &TransferContext {
                trade_page_open: true,
                ..TransferContext::default()
            },
        );
        assert_eq!((sold.len(), bought.len()), (1, 1));
        assert_eq!(hero.count("it_gold"), 500);
        assert_eq!(hero.count("it_sword"), 3);
        // the trader holds the gold as merchandise, not as owned items
        assert_eq!(trader.count("it_gold"), 0);
        assert_eq!(trader.count("it_sword"), 0);
        let merch: i32 = trader
            .stacks()
            .iter()
            .filter(|s| s.template.eq_ignore_ascii_case("It_Gold"))
            .map(|s| s.amount)
            .sum();
        assert_eq!(merch, 1500);
    }

    #[test]
    fn selling_more_than_asked_makes_the_trader_pay_change() {
        let (mut hero, mut trader, _) = setup();
        // the hero offers two swords (2 x 250) and asks for 300 worth of nothing but gold
        let sword = hero.find_stack_index("It_Sword", FindQuality::Any, 0, -1) as usize;
        hero.set_transaction_amount(sword, 2);
        let tg = trader.find_stack_index("It_Gold", FindQuality::Any, 0, -1) as usize;
        trader.set_transaction_amount(tg, 300);
        assert!(balance_gold(&mut hero, &mut trader, &price, &price));
        // ask 300, offer 500: the trader's basket grows by the 200 difference
        assert_eq!(trader.transaction_amount(tg), 500);
        let t = totals(&hero, &trader, &price, &price);
        assert_eq!((t.ask, t.offer), (500, 500));
        assert!(t.deal_enabled());
        // a trader who cannot cover it is not balanced
        trader.set_transaction_amount(tg, 400);
        let sw = hero.find_stack_index("It_Sword", FindQuality::Any, 0, -1) as usize;
        hero.set_transaction_amount(sw, 2);
        let gold = trader.stack(tg).unwrap().amount;
        assert_eq!(gold, 500);
        trader.set_transaction_amount(tg, 100);
        let only = trader.clone();
        let mut poor = Inventory::new();
        poor.add_items(
            "It_Gold",
            only.stack(tg).unwrap().info.clone(),
            50,
            0,
            StackType::Merchandise,
        );
        poor.set_transaction_amount(0, 10);
        assert!(!balance_gold(&mut hero, &mut poor, &price, &price));
    }

    #[test]
    fn cancel_clears_both_baskets() {
        let (mut hero, mut trader, _) = setup();
        hero.set_transaction_amount(0, 3);
        trader.set_transaction_amount(0, 1);
        cancel(&mut hero, &mut trader);
        assert!(hero.stacks().iter().all(|s| s.transaction == 0));
        assert!(trader.stacks().iter().all(|s| s.transaction == 0));
    }

    #[test]
    fn digits_jump_to_the_nth_loot_stack() {
        assert_eq!(loot_key_index('1', 5), Some(0));
        assert_eq!(loot_key_index('5', 5), Some(4));
        assert_eq!(loot_key_index('6', 5), None);
        assert_eq!(loot_key_index('0', 12), Some(9));
        assert_eq!(loot_key_index('x', 12), None);
    }
}
