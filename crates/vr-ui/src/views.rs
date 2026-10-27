//! View models of the VR-only windows and the builders that lay every window out as a
//! [`Sheet`] in the **original UI's design space** (`g3.gui.res`, 1024x768 pixels, see
//! `docs/research/UI_ORIGINAL.md`): the original view backgrounds (`gothic_ui::atlas`) sit
//! behind the items, so a window looks like the flat game screen and is just floated in the
//! world (about 1000 dmm wide, see `docs/vr/VR_GAME_SYSTEMS.md` for the comfort numbers).
//!
//! The trade / loot / recipe / document models are `gothic-ui`'s own (`TradeView`, `LootView`,
//! `RecipeView`, `DocumentView`), so the desktop and VR hosts feed the same data; the quest
//! log and character models are small copies of what the flat journal / character pages compute
//! from the `GameState` (`gothic-vr-bridge::feed` builds them). Fixed texts come from
//! [`Labels`]: the original loc-table keys where the flat UI has them, the flat UI's own
//! fallbacks (real German, with umlauts) otherwise.
use crate::panel::wrap;
use crate::sheet::{ItemKind, Rect, Sheet, SheetAct, SheetItem};
use crate::wrist::Vitals;
use glam::Vec2;
use gothic_ui::atlas as art;
use gothic_ui::{DocumentView, LootView, PageEvent, PageRow, RecipeView, TradeView};

/// Width of every window in dmm (angle): about 58 degrees at any distance, inside the +-30
/// degree comfort cone edge to edge.
pub const WINDOW_DMM: f32 = 1000.0;
/// Where windows float, m from the eyes (laser range, outside Meta's 0.5-0.8 m "middle
/// distance").
pub const WINDOW_DISTANCE: f32 = 1.25;

/// Fixed texts. `HUD_*` keys are the original loc-table keys the flat UI reads; the rest are the
/// flat UI's literal strings (screens.rs), which are the original's German.
#[derive(Debug, Clone, PartialEq)]
pub struct Labels {
    pub close: String,
    pub craft: String,
    pub gold: String,
    pub gold_balance: String,
    pub no_quest: String,
    pub no_recipes: String,
    pub quest_tabs: [String; 3],
    pub ingredients: String,
    pub requirements: String,
    pub take_all: String,
    pub trade: String,
    pub quest_log: String,
    pub empty: String,
    pub clear: String,
    pub experience: String,
    pub level: String,
    pub xp: String,
    pub next_level: String,
    pub learning_points: String,
    pub attributes: String,
    pub armor_protection: String,
    pub protection: [String; 6],
    pub skills: String,
    pub no_skills: String,
    pub worn: String,
    pub you_get: String,
    pub you_give: String,
    pub page: String,
}

impl Default for Labels {
    fn default() -> Self {
        let s = |t: &str| t.to_string();
        Self {
            close: s("Schließen"),
            craft: s("Herstellen"),
            gold: s("Gold"),
            gold_balance: s("Gold-Ausgleich"),
            no_quest: s("Keine Mission ausgewählt"),
            no_recipes: s("Keine Rezepte bekannt"),
            quest_tabs: [s("Aktuell"), s("Erfolgreich"), s("Gescheitert")],
            ingredients: s("Zutaten"),
            requirements: s("Erforderliche Kenntnisse"),
            take_all: s("Alles nehmen"),
            trade: s("Tauschen"),
            quest_log: s("Alle Missionen"),
            empty: s("(leer)"),
            clear: s("Leeren"),
            experience: s("Erfahrung"),
            level: s("Stufe"),
            xp: s("Erfahrung"),
            next_level: s("Nächste Stufe"),
            learning_points: s("Lernpunkte"),
            attributes: s("Attribute"),
            armor_protection: s("Rüstungsschutz"),
            protection: [
                s("Schutz vor Klingen"),
                s("Schutz vor Zerschmettern"),
                s("Schutz vor Geschossen"),
                s("Schutz vor Feuer"),
                s("Schutz vor Eis"),
                s("Schutz vor Energie"),
            ],
            skills: s("Fertigkeiten"),
            no_skills: s("Noch keine Fertigkeiten erlernt."),
            worn: s("Getragen"),
            you_get: s("Du bekommst"),
            you_give: s("Du gibst"),
            page: s("Seite"),
        }
    }
}

impl Labels {
    /// The labels with every original loc-table entry applied (`text` = `GameDatabase::text`).
    pub fn from_text(text: &dyn Fn(&str) -> Option<String>) -> Self {
        let mut l = Self::default();
        let get = |k: &str| text(k).filter(|s| !s.trim().is_empty());
        let mut set = |dst: &mut String, k: &str| {
            if let Some(v) = get(k) {
                *dst = v;
            }
        };
        set(&mut l.close, "HUD_Close");
        set(&mut l.craft, "HUD_Craft");
        set(&mut l.gold, "HUD_GoldAmount");
        set(&mut l.gold_balance, "HUD_GoldBalance");
        set(&mut l.no_quest, "HUD_NoQuestSelected");
        set(&mut l.no_recipes, "HUD_NoRecipes");
        set(&mut l.quest_tabs[0], "HUD_Quest_Running");
        set(&mut l.quest_tabs[1], "HUD_Quest_Success");
        set(&mut l.quest_tabs[2], "HUD_Quest_Failed");
        set(&mut l.ingredients, "HUD_Recipe_Ingredients");
        set(&mut l.requirements, "HUD_Recipe_Requirements");
        set(&mut l.take_all, "HUD_TakeAll");
        set(&mut l.trade, "HUD_Trade");
        l
    }
}

/// What a builder needs besides the view: item names and the labels.
pub struct Ctx<'a> {
    pub name: &'a dyn Fn(&str) -> String,
    pub labels: &'a Labels,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestEntry {
    pub name: String,
    pub title: String,
    /// 0 running, 1 succeeded, 2 failed / cancelled (`gothic_ui::journal::quest_tab`).
    pub tab: usize,
    /// Log lines, newest first.
    pub log: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestLogView {
    pub quests: Vec<QuestEntry>,
    /// "1. Tag, 12:23".
    pub clock: String,
    /// Hour of the day (picks the sky strip).
    pub hour: u32,
    /// "Ruf in Nordmar" and its value.
    pub reputation: Vec<(String, i32)>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CharacterView {
    pub level: u32,
    pub xp: u64,
    pub xp_next: u64,
    pub learning_points: u32,
    pub vitals: Vitals,
    /// Localised names (`ATTRIB_<tag>`) and values: strength, hunting, lore, smithing, theft,
    /// alchemy.
    pub attributes: Vec<(String, i32)>,
    /// The six protection values (blade, blunt, missile, fire, ice, energy).
    pub protection: Vec<i32>,
    /// (group 0..7 as in the flat UI: Kampf, Jagd, Magie, Schmieden, Diebstahl, Alchemie,
    /// Sonstige; skill name).
    pub skills: Vec<(usize, String)>,
    /// (slot, item template).
    pub equipment: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WindowView {
    Trade(TradeView),
    Loot(LootView),
    Recipe(RecipeView),
    Document(DocumentView),
    Quests(QuestLogView),
    Character(CharacterView),
}

impl WindowView {
    pub fn name(&self) -> &'static str {
        match self {
            WindowView::Trade(_) => "trade",
            WindowView::Loot(_) => "loot",
            WindowView::Recipe(_) => "recipe",
            WindowView::Document(_) => "document",
            WindowView::Quests(_) => "quests",
            WindowView::Character(_) => "character",
        }
    }
}

/// Per-window UI state (never touches the game).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WindowUi {
    pub tab: usize,
    pub selected: Option<usize>,
    /// Top row of each scroll region, in region order.
    pub scroll: Vec<usize>,
    /// Document spread.
    pub page: usize,
}

impl WindowUi {
    fn top(&self, region: usize) -> usize {
        self.scroll.get(region).copied().unwrap_or(0)
    }
}

/// `ItMw_Sword_01` -> `Sword 01` when the host has no localised name.
pub fn guess_name(template: &str) -> String {
    template
        .split_once('_')
        .map_or(template, |(_, rest)| rest)
        .replace('_', " ")
}

pub fn build(view: &WindowView, ui: &WindowUi, cx: &Ctx) -> Sheet {
    match view {
        WindowView::Trade(v) => trade(v, ui, cx),
        WindowView::Loot(v) => loot(v, ui, cx),
        WindowView::Recipe(v) => recipe(v, ui, cx),
        WindowView::Document(v) => document(v, ui, cx),
        WindowView::Quests(v) => quests(v, ui, cx),
        WindowView::Character(v) => character(v, ui, cx),
    }
}

fn heading(s: &mut Sheet, text: &str, x: f32, y: f32, w: f32) {
    let r = s.px(x, y, w, 30.0);
    s.push(SheetItem::new(r, ItemKind::Title, text).size(22.0));
}

fn close(s: &mut Sheet, x: f32, y: f32, cx: &Ctx) {
    let r = s.px(x, y, 30.0, 28.0);
    s.push(
        SheetItem::new(r, ItemKind::Button, "×")
            .value(cx.labels.close.clone())
            .size(22.0)
            .act(SheetAct::Close),
    );
}

/// "name .... value" row of the character panels.
fn kv(s: &mut Sheet, x: f32, y: f32, w: f32, k: &str, v: String) {
    let r = s.px(x + 24.0, y, w - 46.0, 20.0);
    s.push(SheetItem::new(r, ItemKind::Label, k).value(v));
}

/// A grid of 48 px icon cells (the original's inventory cells) in a scroll region.
#[allow(clippy::too_many_arguments)]
fn grid(
    s: &mut Sheet,
    (x, y): (f32, f32),
    cols: usize,
    rows: usize,
    list: &[PageRow],
    top: usize,
    cx: &Ctx,
    act: impl Fn(&PageRow) -> Option<SheetAct>,
) {
    const CELL: f32 = 48.0;
    let rect = s.px(x, y, CELL * cols as f32, CELL * rows as f32);
    let total = list.len().div_ceil(cols).max(1);
    let (region, range) = s.region(rect, total, CELL, top);
    for (k, r) in range.enumerate() {
        for c in 0..cols {
            let Some(row) = list.get(r * cols + c) else {
                break;
            };
            let rc = s.px(x + CELL * c as f32, y + CELL * k as f32, CELL - 2.0, CELL - 2.0);
            let mut it = SheetItem::new(rc, ItemKind::Cell, row.label.clone().unwrap_or_else(|| (cx.name)(&row.item)))
                .icon(row.item.clone(), row.count)
                .enabled(!row.locked);
            if row.price > 0 {
                it.value = format!("{} {}", row.price, cx.labels.gold);
            }
            it.act = act(row);
            it.region = Some(region);
            s.push(it);
        }
    }
}

fn gold_line(s: &mut Sheet, x: f32, y: f32, w: f32, g: i64, cx: &Ctx) {
    let r = s.px(x, y, w, 26.0);
    s.push(SheetItem::new(r, ItemKind::Label, &cx.labels.gold).value(g.to_string()));
}

fn trade(v: &TradeView, ui: &WindowUi, cx: &Ctx) -> Sheet {
    let l = cx.labels;
    let mut s = Sheet::new(format!("{}: {}", l.trade, v.vendor), Vec2::new(981.0, 722.0), WINDOW_DMM, WINDOW_DISTANCE);
    s.backdrop(art::TRADE_VENDOR, 0.0, 0.0, 303.0, 493.0);
    s.backdrop(art::TRADE_PLAYER, 677.0, 0.0, 304.0, 493.0);
    s.backdrop(art::TRANSACTION, 0.0, 508.0, 980.0, 213.0);
    heading(&mut s, &v.vendor, 24.0, 12.0, 255.0);
    heading(&mut s, "", 701.0, 12.0, 255.0);
    grid(&mut s, (26.0, 66.0), 5, 8, &v.vendor_rows, ui.top(0), cx, |r| {
        Some(SheetAct::Page(PageEvent::TradeAdd { player_side: false, key: r.key }))
    });
    grid(&mut s, (703.0, 88.0), 5, 7, &v.player.rows, ui.top(1), cx, |r| {
        Some(SheetAct::Page(PageEvent::TradeAdd { player_side: true, key: r.key }))
    });
    gold_line(&mut s, 24.0, 445.0, 255.0, v.vendor_gold, cx);
    gold_line(&mut s, 701.0, 445.0, 255.0, v.player.gold, cx);
    // Baskets in the transaction strip: what the hero gets (left), what he gives (right).
    // The totals sit in the two teal bars of the strip.
    let bar = |s: &mut Sheet, x: f32, total: i64| {
        let r = s.px(x, 672.0, 250.0, 22.0);
        s.push(SheetItem::new(r, ItemKind::Label, &l.gold).value(total.to_string()));
    };
    bar(&mut s, 70.0, v.ask_total);
    bar(&mut s, 652.0, v.offer_total);
    let r = s.px(24.0, 520.0, 300.0, 24.0);
    s.push(SheetItem::new(r, ItemKind::Label, &l.you_get));
    let r = s.px(657.0, 520.0, 300.0, 24.0);
    s.push(SheetItem::new(r, ItemKind::Label, &l.you_give));
    grid(&mut s, (30.0, 550.0), 6, 2, &v.ask_rows, 0, cx, |r| {
        Some(SheetAct::Page(PageEvent::TradeRemove { player_side: false, key: r.key }))
    });
    grid(&mut s, (663.0, 550.0), 6, 2, &v.offer_rows, 0, cx, |r| {
        Some(SheetAct::Page(PageEvent::TradeRemove { player_side: true, key: r.key }))
    });
    let btn = |s: &mut Sheet, y: f32, text: &str, on: bool, act: SheetAct| {
        let r = s.px(396.0, y, 188.0, 34.0);
        s.push(SheetItem::new(r, ItemKind::Button, text).enabled(on).act(act));
    };
    btn(&mut s, 536.0, &l.gold_balance, v.balance, SheetAct::Page(PageEvent::TradeBalance));
    btn(&mut s, 576.0, &l.trade, v.deal, SheetAct::Page(PageEvent::TradeDeal));
    btn(&mut s, 616.0, &l.clear, true, SheetAct::Page(PageEvent::TradeClear { player_side: true }));
    s.info = Some(s.px(330.0, 656.0, 320.0, 26.0));
    if !v.hint.is_empty() {
        let r = s.px(330.0, 690.0, 320.0, 24.0);
        s.push(SheetItem::new(r, ItemKind::Label, &v.hint));
    }
    close(&mut s, 940.0, 10.0, cx);
    s
}

fn loot(v: &LootView, ui: &WindowUi, cx: &Ctx) -> Sheet {
    let l = cx.labels;
    let mut s = Sheet::new(v.title.clone(), Vec2::new(634.0, 638.0), WINDOW_DMM * 0.65, WINDOW_DISTANCE);
    s.backdrop(art::LOOT_POOL, 0.0, 0.0, 312.0, 638.0);
    s.backdrop(art::INVENTORY, 322.0, 0.0, 312.0, 638.0);
    heading(&mut s, &v.title, 24.0, 12.0, 264.0);
    grid(&mut s, (26.0, 66.0), 5, 10, &v.rows, ui.top(0), cx, |r| {
        Some(SheetAct::Page(PageEvent::LootTake { key: r.key }))
    });
    if v.rows.is_empty() {
        let r = s.px(26.0, 66.0, 260.0, 26.0);
        s.push(SheetItem::new(r, ItemKind::Label, &l.empty));
    }
    if let Some(g) = v.pool_gold {
        gold_line(&mut s, 24.0, 584.0, 258.0, g, cx);
    }
    if v.take_all {
        let r = s.px(86.0, 604.0, 140.0, 26.0);
        s.push(
            SheetItem::new(r, ItemKind::Button, &l.take_all)
                .enabled(!v.rows.is_empty())
                .act(SheetAct::Page(PageEvent::LootTakeAll)),
        );
    }
    // The hero's own inventory next to it (read only: items are taken *into* it).
    grid(&mut s, (348.0, 85.0), 5, 10, &v.player.rows, ui.top(1), cx, |_| None);
    gold_line(&mut s, 346.0, 584.0, 258.0, v.player.gold, cx);
    s.info = Some(s.px(26.0, 618.0, 580.0, 20.0));
    close(&mut s, 595.0, 8.0, cx);
    s
}

fn recipe(v: &RecipeView, ui: &WindowUi, cx: &Ctx) -> Sheet {
    let l = cx.labels;
    let mut s = Sheet::new(v.title.clone(), Vec2::new(712.0, 638.0), WINDOW_DMM * 0.72, WINDOW_DISTANCE);
    s.backdrop(art::RECIPE, 0.0, 0.0, 333.0, 507.0);
    s.backdrop(art::SYNTH_RECIPE, 343.0, 0.0, 367.0, 638.0);
    heading(&mut s, &v.title, 363.0, 12.0, 300.0);
    let sel = ui.selected.unwrap_or(0).min(v.recipes.len().saturating_sub(1));
    let list = s.px(363.0, 56.0, 330.0, 520.0);
    let (region, range) = s.region(list, v.recipes.len(), 30.0, ui.top(0));
    for (k, i) in range.enumerate() {
        let r = &v.recipes[i];
        let rc = s.px(363.0, 56.0 + 30.0 * k as f32, 330.0, 30.0);
        let mut it = SheetItem::new(rc, ItemKind::Row, &r.name)
            .icon(r.product.clone(), 0)
            .selected(i == sel)
            .enabled(r.craftable)
            .act(SheetAct::Select(i));
        it.region = Some(region);
        s.push(it);
    }
    if v.recipes.is_empty() {
        let r = s.px(363.0, 56.0, 330.0, 30.0);
        s.push(SheetItem::new(r, ItemKind::Label, &l.no_recipes));
    }
    gold_line(&mut s, 357.0, 596.0, 330.0, v.gold, cx);
    if let Some(r) = v.recipes.get(sel) {
        let t = s.px(20.0, 14.0, 293.0, 30.0);
        s.push(SheetItem::new(t, ItemKind::Title, &r.name).size(22.0).ink());
        for (k, line) in wrap(&r.description, 36).into_iter().take(5).enumerate() {
            let rc = s.px(26.0, 54.0 + 20.0 * k as f32, 285.0, 20.0);
            s.push(SheetItem::new(rc, ItemKind::Paragraph, line).ink());
        }
        let h = s.px(26.0, 170.0, 285.0, 24.0);
        s.push(SheetItem::new(h, ItemKind::Label, &l.ingredients).size(18.0).ink());
        for (k, ing) in r.ingredients.iter().take(6).enumerate() {
            let name = if ing.name.is_empty() { (cx.name)(&ing.item) } else { ing.name.clone() };
            let rc = s.px(26.0, 198.0 + 28.0 * k as f32, 285.0, 26.0);
            s.push(
                SheetItem::new(rc, ItemKind::Label, name)
                    .value(format!("{}/{}", ing.have, ing.need))
                    .icon(ing.item.clone(), 0)
                    .enabled(ing.have >= ing.need)
                    .ink(),
            );
        }
        if !r.requirement.is_empty() {
            let h = s.px(26.0, 372.0, 285.0, 24.0);
            s.push(SheetItem::new(h, ItemKind::Label, &l.requirements).size(18.0).ink());
            let rc = s.px(26.0, 398.0, 285.0, 22.0);
            s.push(SheetItem::new(rc, ItemKind::Label, &r.requirement).ink());
        }
        let rc = s.px(60.0, 446.0, 213.0, 38.0);
        s.push(
            SheetItem::new(rc, ItemKind::Button, &l.craft)
                .enabled(r.craftable)
                .ink()
                .act(SheetAct::Page(PageEvent::Craft { key: r.key })),
        );
    }
    close(&mut s, 672.0, 10.0, cx);
    s
}

/// Lines per document page.
pub const DOC_LINES: usize = 17;

fn doc_chars(v: &DocumentView) -> usize {
    if v.is_book { 34 } else { 38 }
}

/// Number of two-page spreads (book) or pages (letter) of a document.
pub fn document_spreads(v: &DocumentView) -> usize {
    let n = doc_chars(v);
    let lines = wrap(&v.left, n).len().max(wrap(&v.right, n).len());
    lines.div_ceil(DOC_LINES).max(1)
}

fn document(v: &DocumentView, ui: &WindowUi, cx: &Ctx) -> Sheet {
    let (w, h, pic) = if v.is_book { (601.0, 504.0, art::BOOK) } else { (333.0, 507.0, art::LETTER) };
    let mut s = Sheet::new(v.title.clone(), Vec2::new(w, h), if v.is_book { 760.0 } else { 440.0 }, 0.9);
    s.backdrop(pic, 0.0, 0.0, w, h);
    let spreads = document_spreads(v);
    let page = ui.page.min(spreads - 1);
    let cols: Vec<(&String, f32)> = if v.is_book { vec![(&v.left, 44.0), (&v.right, 318.0)] } else { vec![(&v.left, 34.0)] };
    let cw = if v.is_book { 240.0 } else { 268.0 };
    for (text, x) in cols {
        for (k, line) in wrap(text, doc_chars(v)).into_iter().skip(page * DOC_LINES).take(DOC_LINES).enumerate() {
            let r = s.px(x, 52.0 + 22.0 * k as f32, cw, 22.0);
            s.push(SheetItem::new(r, ItemKind::Paragraph, line).size(15.0).ink());
        }
    }
    let t = s.px(44.0, 12.0, w - 110.0, 30.0);
    s.push(SheetItem::new(t, ItemKind::Title, &v.title).size(if v.is_book { 22.0 } else { 18.0 }).ink());
    let by = h - 44.0;
    let r = s.px(40.0, by, 56.0, 30.0);
    s.push(SheetItem::new(r, ItemKind::Button, "‹").size(22.0).ink().enabled(page > 0).act(SheetAct::Turn(-1)));
    let r = s.px(w / 2.0 - 50.0, by, 100.0, 30.0);
    s.push(SheetItem::new(r, ItemKind::Label, format!("{} {}/{}", cx.labels.page, page + 1, spreads)).ink());
    let r = s.px(w - 96.0, by, 56.0, 30.0);
    s.push(SheetItem::new(r, ItemKind::Button, "›").size(22.0).ink().enabled(page + 1 < spreads).act(SheetAct::Turn(1)));
    close(&mut s, w - 44.0, 8.0, cx);
    s
}

/// Quests of `tab` (indices into `v.quests`).
pub fn quests_in_tab(v: &QuestLogView, tab: usize) -> Vec<usize> {
    v.quests
        .iter()
        .enumerate()
        .filter(|(_, q)| q.tab == tab)
        .map(|(i, _)| i)
        .collect()
}

fn quests(v: &QuestLogView, ui: &WindowUi, cx: &Ctx) -> Sheet {
    let l = cx.labels;
    let mut s = Sheet::new(l.quest_log.clone(), Vec2::new(937.0, 638.0), WINDOW_DMM, WINDOW_DISTANCE);
    s.backdrop(art::QUESTS, 0.0, 0.0, 937.0, 638.0);
    heading(&mut s, &l.quest_log, 0.0, 6.0, 937.0);
    let map = s.px(38.0, 46.0, 442.0, 392.0);
    s.push(SheetItem::new(map, ItemKind::Image("g3_hud_view_world_back.tga"), ""));
    for (t, name) in l.quest_tabs.iter().enumerate() {
        let r = s.px(503.0 + 139.0 * t as f32, 44.0, 138.0, 44.0);
        s.push(
            SheetItem::new(r, ItemKind::Tab, name.clone())
                .selected(ui.tab == t)
                .act(SheetAct::Tab(t)),
        );
    }
    let ids = quests_in_tab(v, ui.tab);
    let list = s.px(508.0, 98.0, 392.0, 176.0);
    let (region, range) = s.region(list, ids.len(), 26.0, ui.top(0));
    let sel = ui.selected.filter(|i| ids.contains(i)).or(ids.first().copied());
    for (k, &q) in ids[range].iter().enumerate() {
        let r = s.px(508.0, 98.0 + 26.0 * k as f32, 392.0, 26.0);
        let mut it = SheetItem::new(r, ItemKind::Row, &v.quests[q].title)
            .icon("quest", 0)
            .selected(Some(q) == sel)
            .act(SheetAct::Select(q));
        it.region = Some(region);
        s.push(it);
    }
    let det = s.px(508.0, 300.0, 410.0, 302.0);
    match sel.map(|q| &v.quests[q]) {
        None => {
            let r = Rect::new(det.x, det.y + det.h * 0.45, det.w, 24.0 / 638.0);
            s.push(SheetItem::new(r, ItemKind::Label, &l.no_quest).value(""));
        }
        Some(q) => {
            let r = s.px(520.0, 308.0, 380.0, 26.0);
            s.push(SheetItem::new(r, ItemKind::Title, &q.title).size(18.0));
            let lines: Vec<String> = q
                .log
                .iter()
                .flat_map(|e| {
                    let mut w = wrap(e, 44);
                    w.push(String::new());
                    w
                })
                .collect();
            let body = s.px(520.0, 340.0, 384.0, 258.0);
            let (region, range) = s.region(body, lines.len(), 22.0, ui.top(1));
            for (k, line) in lines[range].iter().enumerate() {
                let r = s.px(520.0, 340.0 + 22.0 * k as f32, 384.0, 22.0);
                let mut it = SheetItem::new(r, ItemKind::Paragraph, line);
                it.region = Some(region);
                s.push(it);
            }
        }
    }
    for (k, (name, fame)) in v.reputation.iter().take(6).enumerate() {
        kv(&mut s, 20.0, 468.0 + 22.0 * k as f32, 290.0, name, fame.to_string());
    }
    let sky = ["g3_hud_view_quests_time_dawn.tga", "g3_hud_view_quests_time_day.tga", "g3_hud_view_quests_time_dusk.tga", "g3_hud_view_quests_time_night.tga"]
        [gothic_ui::journal::time_of_day(v.hour)];
    let r = s.px(330.0, 460.0, 158.0, 34.0);
    s.push(SheetItem::new(r, ItemKind::Image(sky), ""));
    let r = s.px(330.0, 496.0, 158.0, 26.0);
    s.push(SheetItem::new(r, ItemKind::Label, &v.clock).size(15.0));
    close(&mut s, 898.0, 8.0, cx);
    s
}

fn character(v: &CharacterView, _ui: &WindowUi, cx: &Ctx) -> Sheet {
    let l = cx.labels;
    let mut s = Sheet::new("Charakter", Vec2::new(932.0, 638.0), WINDOW_DMM, WINDOW_DISTANCE);
    s.backdrop(art::CHARACTER, 0.0, 0.0, 312.0, 638.0);
    s.backdrop(art::SKILLS, 311.0, 0.0, 621.0, 638.0);
    heading(&mut s, &l.experience, 24.0, 8.0, 260.0);
    kv(&mut s, 0.0, 44.0, 312.0, &l.level, v.level.to_string());
    kv(&mut s, 0.0, 68.0, 312.0, &l.xp, v.xp.to_string());
    kv(&mut s, 0.0, 92.0, 312.0, &l.next_level, v.xp_next.to_string());
    kv(&mut s, 0.0, 116.0, 312.0, &l.learning_points, v.learning_points.to_string());
    heading(&mut s, &l.attributes, 24.0, 164.0, 260.0);
    let mut y = 196.0;
    for (name, val) in &v.attributes {
        kv(&mut s, 0.0, y, 312.0, name, val.to_string());
        y += 23.0;
    }
    for (name, (cur, max)) in [
        ("Lebensenergie", v.vitals.health),
        ("Ausdauer", v.vitals.stamina),
        ("Mana", v.vitals.mana),
    ] {
        kv(&mut s, 0.0, y, 312.0, name, format!("{cur:.0} / {max:.0}"));
        y += 23.0;
    }
    heading(&mut s, &l.armor_protection, 24.0, 428.0, 260.0);
    for (i, name) in l.protection.iter().enumerate() {
        let y = [464.0, 488.0, 512.0, 532.0, 556.0, 580.0][i];
        kv(&mut s, 0.0, y, 312.0, name, v.protection.get(i).copied().unwrap_or(0).to_string());
    }
    // The seven headed boxes of the skills view (`g3.gui.res`), origin (311, 0).
    let groups: [(&str, (f32, f32), (f32, f32, f32, f32)); 7] = [
        ("Kampf", (32.0, 8.0), (12.0, 40.0, 288.0, 144.0)),
        ("Jagd", (32.0, 200.0), (12.0, 232.0, 288.0, 96.0)),
        ("Magie", (32.0, 348.0), (12.0, 380.0, 288.0, 96.0)),
        ("Schmieden", (32.0, 488.0), (12.0, 532.0, 288.0, 48.0)),
        ("Diebstahl", (328.0, 12.0), (320.0, 44.0, 288.0, 96.0)),
        ("Alchemie", (328.0, 156.0), (320.0, 184.0, 288.0, 96.0)),
        ("Sonstige", (328.0, 300.0), (320.0, 344.0, 288.0, 48.0)),
    ];
    for (g, (name, h, r)) in groups.into_iter().enumerate() {
        heading(&mut s, name, 311.0 + h.0, h.1, 260.0);
        let list: Vec<&String> = v.skills.iter().filter(|(k, _)| *k == g).map(|(_, n)| n).collect();
        for (j, n) in list.into_iter().take((r.3 / 20.0) as usize).enumerate() {
            let rc = s.px(311.0 + r.0 + 8.0, r.1 + 20.0 * j as f32, r.2 - 16.0, 20.0);
            s.push(SheetItem::new(rc, ItemKind::Label, n));
        }
    }
    if v.skills.is_empty() {
        let rc = s.px(631.0, 444.0, 288.0, 24.0);
        s.push(SheetItem::new(rc, ItemKind::Label, &l.no_skills).enabled(false));
    }
    let h = s.px(631.0, 410.0, 288.0, 24.0);
    s.push(SheetItem::new(h, ItemKind::Label, &l.worn).size(18.0));
    for (k, (slot, item)) in v.equipment.iter().take(4).enumerate() {
        let r = s.px(631.0, 490.0 + 24.0 * k as f32, 288.0, 24.0);
        s.push(SheetItem::new(r, ItemKind::Label, slot).value((cx.name)(item)).icon(item.clone(), 0));
    }
    close(&mut s, 894.0, 8.0, cx);
    s
}

/// Skill group of a skill key as the flat character page sorts them (Kampf, Jagd, Magie,
/// Schmieden, Diebstahl, Alchemie, Sonstige).
pub fn skill_group(key: &str) -> usize {
    let k = key.to_ascii_lowercase();
    let has = |w: &[&str]| w.iter().any(|x| k.contains(x));
    if has(&["sword", "1h", "2h", "bow", "crossbow", "combat", "weapon", "shield", "fight"]) {
        0
    } else if has(&["hunt", "sneak", "sprint", "animal", "pelt", "claw", "teeth", "trophy", "skin"]) {
        1
    } else if has(&["magic", "spell", "rune", "mana", "scroll", "summon"]) {
        2
    } else if has(&["smith", "forge", "blade", "armor"]) {
        3
    } else if has(&["thief", "lock", "pick", "steal", "pocket", "theft"]) {
        4
    } else if has(&["alch", "potion", "herb", "brew"]) {
        5
    } else {
        6
    }
}
