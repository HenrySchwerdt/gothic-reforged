//! View models and events of the host-driven HUD pages: the trade page
//! (`gCHUDPageTrade2`) and the loot page (`gCHUDPageLoot2`, also used for the work-station
//! recipe lists).
//!
//! The UI only draws what the host puts into a [`PageView`] and reports clicks as
//! [`PageEvent`]; every rule (prices, baskets, what may be moved) lives in
//! `gothic_gameplay::trade` and the host that owns the inventories.
use bevy::prelude::*;

/// One icon cell of a list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageRow {
    /// Item template (icon, name and category are looked up by it).
    pub item: String,
    /// Number shown in the cell corner.
    pub count: i32,
    /// Price of one item (0 = none shown).
    pub price: i32,
    /// Host key of the entry (stack index, loot index, recipe id).
    pub key: usize,
    /// Greyed out: cannot be moved.
    pub locked: bool,
    /// Text that replaces the item name in the hint line (recipes).
    pub label: Option<String>,
}

/// The right-hand inventory panel of both pages.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerPanel {
    /// Title above the category tabs (the host leaves it empty for the tab name).
    pub title: String,
    pub rows: Vec<PageRow>,
    pub gold: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TradeView {
    pub vendor: String,
    pub vendor_rows: Vec<PageRow>,
    pub vendor_gold: i64,
    pub player: PlayerPanel,
    /// Basket of the vendor (what the hero gets) and of the hero (what he gives).
    pub ask_rows: Vec<PageRow>,
    pub offer_rows: Vec<PageRow>,
    pub ask_total: i64,
    pub offer_total: i64,
    /// The deal button is live.
    pub deal: bool,
    /// The gold balance button is live.
    pub balance: bool,
    /// One-line message under the buttons (refusals).
    pub hint: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LootView {
    pub title: String,
    pub rows: Vec<PageRow>,
    /// Gold in the pool, `None` hides the gold line (work stations).
    pub pool_gold: Option<i64>,
    /// Show the "take all" button.
    pub take_all: bool,
    pub player: PlayerPanel,
    pub hint: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PageView {
    Trade(TradeView),
    Loot(LootView),
}

/// Host -> UI: show (or, with `None`, close) a page.
#[derive(Message, Clone, Debug)]
pub struct ShowPage(pub Option<PageView>);

/// UI -> host: what the player did on a page.
#[derive(Message, Clone, Debug, PartialEq)]
pub enum PageEvent {
    /// Move one more of a stack into the basket (`player_side`: the hero's inventory list).
    TradeAdd {
        player_side: bool,
        key: usize,
    },
    /// Take one back out of a basket (`player_side`: the hero's basket).
    TradeRemove {
        player_side: bool,
        key: usize,
    },
    /// Empty a basket (trash button).
    TradeClear {
        player_side: bool,
    },
    TradeBalance,
    TradeDeal,
    /// A loot entry was picked.
    LootTake {
        key: usize,
    },
    LootTakeAll,
    /// The page was closed (button, Esc, or the hotkey).
    Close,
}
