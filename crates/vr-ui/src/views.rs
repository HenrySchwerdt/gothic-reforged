//! View models of the VR-only windows and the builders that lay every window out as a
//! [`Sheet`]. The trade / loot / recipe / document models are `gothic-ui`'s own
//! (`TradeView`, `LootView`, `RecipeView`, `DocumentView`), so the desktop and VR hosts feed
//! the same data; the quest log and character models are small copies of what the flat
//! journal / skills pages compute from the `GameState` (`gothic-vr-bridge::feed` builds them).
use crate::panel::wrap;
use crate::sheet::{ItemKind, Rect, Sheet, SheetAct, SheetItem};
use crate::wrist::Vitals;
use glam::Vec2;
use gothic_ui::{
    DocumentView, LootView, PageEvent, PageRow, PlayerPanel, RecipeView, TradeView,
};

/// Row height of lists, dmm (laser targets; `comfort::MIN_TARGET_DMM` is 22).
pub const ROW_DMM: f32 = 38.0;
/// Where windows float, m from the eyes (laser range, outside Meta's 0.5-0.8 m "middle
/// distance").
pub const WINDOW_DISTANCE: f32 = 1.25;

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
    pub reputation: Vec<(String, i32)>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CharacterView {
    pub level: u32,
    pub xp: u64,
    pub xp_next: u64,
    pub learning_points: u32,
    pub vitals: Vitals,
    pub attributes: Vec<(String, i32)>,
    pub protection: Vec<(String, i32)>,
    /// (skill name, learned).
    pub skills: Vec<(String, bool)>,
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

/// Display name of an item template.
pub type Namer<'a> = &'a dyn Fn(&str) -> String;

/// `ItMw_Sword_01` -> `Sword 01` when the host has no localised name.
pub fn guess_name(template: &str) -> String {
    template
        .split_once('_')
        .map_or(template, |(_, rest)| rest)
        .replace('_', " ")
}

fn title(sheet: &mut Sheet, text: &str) {
    sheet.push(SheetItem::new(Rect::new(0.03, 0.015, 0.8, 0.07), ItemKind::Title, text));
    let close = SheetItem::new(Rect::new(0.90, 0.015, 0.07, 0.07), ItemKind::Button, "X")
        .act(SheetAct::Close);
    sheet.push(close);
}

fn gold(sheet: &mut Sheet, rect: Rect, g: i64) {
    sheet.push(SheetItem::new(rect, ItemKind::Label, "Gold").value(g.to_string()));
}

/// A scrolling list of rows; `act(row)` gives the action of each row.
fn rows(
    sheet: &mut Sheet,
    rect: Rect,
    list: &[PageRow],
    top: usize,
    namer: Namer,
    act: impl Fn(&PageRow) -> SheetAct,
) {
    let (region, range) = sheet.region(rect, list.len(), ROW_DMM, top);
    let n = sheet.regions[region].visible;
    for (k, row) in list[range].iter().enumerate() {
        let name = row.label.clone().unwrap_or_else(|| namer(&row.item));
        let mut it = SheetItem::new(rect.row(k, n), ItemKind::Row, name)
            .icon(row.item.clone(), row.count)
            .enabled(!row.locked)
            .act(act(row));
        if row.price > 0 {
            it.value = format!("{} g", row.price);
        }
        it.region = Some(region);
        sheet.push(it);
    }
}

pub fn build(view: &WindowView, ui: &WindowUi, namer: Namer) -> Sheet {
    match view {
        WindowView::Trade(v) => trade(v, ui, namer),
        WindowView::Loot(v) => loot(v, ui, namer),
        WindowView::Recipe(v) => recipe(v, ui, namer),
        WindowView::Document(v) => document(v, ui),
        WindowView::Quests(v) => quests(v, ui),
        WindowView::Character(v) => character(v, ui, namer),
    }
}

fn trade(v: &TradeView, ui: &WindowUi, namer: Namer) -> Sheet {
    let mut s = Sheet::new(
        format!("Handel: {}", v.vendor),
        Vec2::new(1000.0, 600.0),
        WINDOW_DISTANCE,
    );
    let t = s.title.clone();
    title(&mut s, &t);
    let (l, m, r) = (
        Rect::new(0.02, 0.17, 0.35, 0.68),
        Rect::new(0.39, 0.10, 0.22, 0.75),
        Rect::new(0.63, 0.17, 0.35, 0.68),
    );
    s.push(SheetItem::new(Rect::new(0.02, 0.10, 0.35, 0.06), ItemKind::Label, &v.vendor));
    s.push(SheetItem::new(Rect::new(0.63, 0.10, 0.35, 0.06), ItemKind::Label, "Du"));
    rows(&mut s, l, &v.vendor_rows, ui.top(0), namer, |r| {
        SheetAct::Page(PageEvent::TradeAdd { player_side: false, key: r.key })
    });
    rows(&mut s, r, &v.player.rows, ui.top(1), namer, |r| {
        SheetAct::Page(PageEvent::TradeAdd { player_side: true, key: r.key })
    });
    // Baskets: what you get above, what you give below.
    let ask = Rect::new(m.x, m.y + 0.06, m.w, 0.22);
    let offer = Rect::new(m.x, m.y + 0.38, m.w, 0.22);
    s.push(SheetItem::new(Rect::new(m.x, m.y, m.w, 0.06), ItemKind::Label, "Du bekommst").value(format!("{} g", v.ask_total)));
    rows(&mut s, ask, &v.ask_rows, 0, namer, |r| {
        SheetAct::Page(PageEvent::TradeRemove { player_side: false, key: r.key })
    });
    s.push(SheetItem::new(Rect::new(m.x, m.y + 0.31, m.w, 0.06), ItemKind::Label, "Du gibst").value(format!("{} g", v.offer_total)));
    rows(&mut s, offer, &v.offer_rows, 0, namer, |r| {
        SheetAct::Page(PageEvent::TradeRemove { player_side: true, key: r.key })
    });
    s.push(
        SheetItem::new(Rect::new(m.x, m.y + 0.62, m.w, 0.07), ItemKind::Button, "Ausgleichen")
            .enabled(v.balance)
            .act(SheetAct::Page(PageEvent::TradeBalance)),
    );
    s.push(
        SheetItem::new(Rect::new(m.x, m.y + 0.70, m.w, 0.07), ItemKind::Button, "Handeln")
            .enabled(v.deal)
            .act(SheetAct::Page(PageEvent::TradeDeal)),
    );
    gold(&mut s, Rect::new(0.02, 0.87, 0.35, 0.06), v.vendor_gold);
    gold(&mut s, Rect::new(0.63, 0.87, 0.35, 0.06), v.player.gold);
    s.push(
        SheetItem::new(Rect::new(0.39, 0.885, 0.22, 0.07), ItemKind::Button, "Leeren")
            .act(SheetAct::Page(PageEvent::TradeClear { player_side: true })),
    );
    if !v.hint.is_empty() {
        s.push(SheetItem::new(Rect::new(0.02, 0.94, 0.35, 0.05), ItemKind::Label, &v.hint));
    }
    s
}

fn loot(v: &LootView, ui: &WindowUi, namer: Namer) -> Sheet {
    let mut s = Sheet::new(v.title.clone(), Vec2::new(640.0, 560.0), WINDOW_DISTANCE);
    title(&mut s, &v.title);
    rows(
        &mut s,
        Rect::new(0.03, 0.12, 0.94, 0.66),
        &v.rows,
        ui.top(0),
        namer,
        |r| SheetAct::Page(PageEvent::LootTake { key: r.key }),
    );
    if v.rows.is_empty() {
        s.push(SheetItem::new(Rect::new(0.03, 0.12, 0.94, 0.07), ItemKind::Label, "(leer)"));
    }
    if let Some(g) = v.pool_gold {
        gold(&mut s, Rect::new(0.03, 0.80, 0.45, 0.06), g);
    }
    if v.take_all {
        s.push(
            SheetItem::new(Rect::new(0.52, 0.80, 0.45, 0.10), ItemKind::Button, "Alles nehmen")
                .enabled(!v.rows.is_empty())
                .act(SheetAct::Page(PageEvent::LootTakeAll)),
        );
    }
    if !v.hint.is_empty() {
        s.push(SheetItem::new(Rect::new(0.03, 0.92, 0.94, 0.06), ItemKind::Label, &v.hint));
    }
    s
}

fn recipe(v: &RecipeView, ui: &WindowUi, namer: Namer) -> Sheet {
    let mut s = Sheet::new(v.title.clone(), Vec2::new(960.0, 560.0), WINDOW_DISTANCE);
    title(&mut s, &v.title);
    let sel = ui.selected.unwrap_or(0).min(v.recipes.len().saturating_sub(1));
    let list = Rect::new(0.52, 0.12, 0.46, 0.74);
    let (region, range) = s.region(list, v.recipes.len(), ROW_DMM, ui.top(0));
    let n = s.regions[region].visible;
    for (k, i) in range.enumerate() {
        let r = &v.recipes[i];
        let mut it = SheetItem::new(list.row(k, n), ItemKind::Row, &r.name)
            .icon(r.product.clone(), 0)
            .selected(i == sel)
            .enabled(true)
            .act(SheetAct::Select(i));
        if !r.craftable {
            it.value = "-".into();
        }
        it.region = Some(region);
        s.push(it);
    }
    if let Some(r) = v.recipes.get(sel) {
        s.push(SheetItem::new(Rect::new(0.03, 0.12, 0.45, 0.07), ItemKind::Title, &r.name).icon(r.product.clone(), 0));
        for (k, line) in wrap(&r.description, 34).into_iter().take(4).enumerate() {
            s.push(SheetItem::new(Rect::new(0.03, 0.20 + 0.055 * k as f32, 0.45, 0.055), ItemKind::Paragraph, line));
        }
        for (k, ing) in r.ingredients.iter().take(6).enumerate() {
            let name = if ing.name.is_empty() { namer(&ing.item) } else { ing.name.clone() };
            let mut it = SheetItem::new(Rect::new(0.03, 0.44 + 0.065 * k as f32, 0.45, 0.065), ItemKind::Label, name)
                .value(format!("{}/{}", ing.have, ing.need))
                .icon(ing.item.clone(), 0);
            it.enabled = ing.have >= ing.need;
            s.push(it);
        }
        if !r.requirement.is_empty() {
            s.push(SheetItem::new(Rect::new(0.03, 0.80, 0.45, 0.06), ItemKind::Label, &r.requirement));
        }
        s.push(
            SheetItem::new(Rect::new(0.03, 0.88, 0.45, 0.10), ItemKind::Button, "Herstellen")
                .enabled(r.craftable)
                .act(SheetAct::Page(PageEvent::Craft { key: r.key })),
        );
    } else {
        s.push(SheetItem::new(Rect::new(0.03, 0.12, 0.45, 0.07), ItemKind::Label, "Keine Rezepte"));
    }
    gold(&mut s, Rect::new(0.52, 0.88, 0.46, 0.06), v.gold);
    s
}

/// Lines per document page.
pub const DOC_LINES: usize = 12;
/// Characters per line.
pub const DOC_CHARS: usize = 34;

/// Number of two-page spreads of a document.
pub fn document_spreads(v: &DocumentView) -> usize {
    let lines = wrap(&v.left, DOC_CHARS).len().max(wrap(&v.right, DOC_CHARS).len());
    lines.div_ceil(DOC_LINES).max(1)
}

fn document(v: &DocumentView, ui: &WindowUi) -> Sheet {
    let w = if v.is_book { 760.0 } else { 420.0 };
    let mut s = Sheet::new(v.title.clone(), Vec2::new(w, 480.0), 0.9);
    title(&mut s, &v.title);
    let spreads = document_spreads(v);
    let page = ui.page.min(spreads - 1);
    let cols: Vec<&String> = if v.is_book { vec![&v.left, &v.right] } else { vec![&v.left] };
    let cw = 0.94 / cols.len() as f32;
    for (c, text) in cols.into_iter().enumerate() {
        let lines = wrap(text, DOC_CHARS);
        for (k, line) in lines.into_iter().skip(page * DOC_LINES).take(DOC_LINES).enumerate() {
            let r = Rect::new(0.03 + cw * c as f32, 0.12 + 0.062 * k as f32, cw - 0.02, 0.062);
            s.push(SheetItem::new(r, ItemKind::Paragraph, line));
        }
    }
    s.push(
        SheetItem::new(Rect::new(0.03, 0.90, 0.2, 0.08), ItemKind::Button, "<")
            .enabled(page > 0)
            .act(SheetAct::Turn(-1)),
    );
    s.push(SheetItem::new(Rect::new(0.40, 0.90, 0.2, 0.08), ItemKind::Label, format!("{}/{}", page + 1, spreads)));
    s.push(
        SheetItem::new(Rect::new(0.77, 0.90, 0.2, 0.08), ItemKind::Button, ">")
            .enabled(page + 1 < spreads)
            .act(SheetAct::Turn(1)),
    );
    s
}

pub const QUEST_TABS: [&str; 3] = ["Aktuell", "Erfolgreich", "Gescheitert"];

/// Quests of the tab of `ui` (indices into `v.quests`).
pub fn quests_in_tab(v: &QuestLogView, tab: usize) -> Vec<usize> {
    v.quests
        .iter()
        .enumerate()
        .filter(|(_, q)| q.tab == tab)
        .map(|(i, _)| i)
        .collect()
}

fn quests(v: &QuestLogView, ui: &WindowUi) -> Sheet {
    let mut s = Sheet::new("Tagebuch", Vec2::new(1000.0, 620.0), WINDOW_DISTANCE);
    title(&mut s, "Tagebuch");
    for (t, name) in QUEST_TABS.iter().enumerate() {
        let n = quests_in_tab(v, t).len();
        s.push(
            SheetItem::new(Rect::new(0.03 + 0.17 * t as f32, 0.10, 0.165, 0.07), ItemKind::Tab, format!("{name} {n}"))
                .selected(ui.tab == t)
                .act(SheetAct::Tab(t)),
        );
    }
    let ids = quests_in_tab(v, ui.tab);
    let list = Rect::new(0.03, 0.19, 0.46, 0.60);
    let (region, range) = s.region(list, ids.len(), ROW_DMM, ui.top(0));
    let n = s.regions[region].visible;
    let sel = ui.selected.filter(|i| ids.contains(i)).or(ids.first().copied());
    for (k, &q) in ids[range].iter().enumerate() {
        let mut it = SheetItem::new(list.row(k, n), ItemKind::Row, &v.quests[q].title)
            .selected(Some(q) == sel)
            .act(SheetAct::Select(q));
        it.region = Some(region);
        s.push(it);
    }
    if ids.is_empty() {
        s.push(SheetItem::new(list.row(0, n), ItemKind::Label, "Keine Aufgaben"));
    }
    // Detail: the log of the selected quest, wrapped, newest first.
    let det = Rect::new(0.52, 0.19, 0.45, 0.60);
    match sel.map(|q| &v.quests[q]) {
        None => {
            s.push(SheetItem::new(det.row(0, 8), ItemKind::Label, "Keine Mission ausgewaehlt"));
        }
        Some(q) => {
            s.push(SheetItem::new(det.row(0, 12), ItemKind::Title, &q.title));
            let lines: Vec<String> = q.log.iter().flat_map(|l| {
                let mut w = wrap(l, 40);
                w.push(String::new());
                w
            }).collect();
            let body = Rect::new(det.x, det.y + det.h / 12.0, det.w, det.h * 11.0 / 12.0);
            let (region, range) = s.region(body, lines.len(), 30.0, ui.top(1));
            let n = s.regions[region].visible;
            for (k, line) in lines[range].iter().enumerate() {
                let mut it = SheetItem::new(body.row(k, n), ItemKind::Paragraph, line);
                it.region = Some(region);
                s.push(it);
            }
        }
    }
    s.push(SheetItem::new(Rect::new(0.03, 0.82, 0.46, 0.06), ItemKind::Label, &v.clock));
    for (k, (name, fame)) in v.reputation.iter().take(2).enumerate() {
        s.push(SheetItem::new(Rect::new(0.03 + 0.24 * k as f32, 0.89, 0.22, 0.06), ItemKind::Label, name).value(fame.to_string()));
    }
    s
}

fn character(v: &CharacterView, ui: &WindowUi, namer: Namer) -> Sheet {
    let mut s = Sheet::new("Charakter", Vec2::new(1000.0, 620.0), WINDOW_DISTANCE);
    title(&mut s, "Charakter");
    let left = Rect::new(0.03, 0.11, 0.30, 0.8);
    let mut y = 0;
    let mut line = |s: &mut Sheet, k: &str, val: String| {
        s.push(SheetItem::new(left.row(y, 14), ItemKind::Label, k).value(val));
        y += 1;
    };
    line(&mut s, "Stufe", v.level.to_string());
    line(&mut s, "Erfahrung", v.xp.to_string());
    line(&mut s, "Naechste Stufe", v.xp_next.to_string());
    line(&mut s, "Lernpunkte", v.learning_points.to_string());
    for (k, (label, (cur, max), frac)) in [
        ("Leben", v.vitals.health, v.vitals.fractions()[0]),
        ("Mana", v.vitals.mana, v.vitals.fractions()[1]),
        ("Ausdauer", v.vitals.stamina, v.vitals.fractions()[2]),
    ]
    .into_iter()
    .enumerate()
    {
        let r = left.row(5 + k, 14);
        s.push(SheetItem::new(r, ItemKind::Bar(frac), label).value(format!("{cur:.0}/{max:.0}")));
    }
    let mid = Rect::new(0.36, 0.11, 0.28, 0.8);
    for (k, (name, val)) in v.attributes.iter().chain(v.protection.iter()).take(14).enumerate() {
        s.push(SheetItem::new(mid.row(k, 14), ItemKind::Label, name).value(val.to_string()));
    }
    let right = Rect::new(0.67, 0.11, 0.30, 0.5);
    let (region, range) = s.region(right, v.skills.len(), 30.0, ui.top(0));
    let n = s.regions[region].visible;
    for (k, (name, learned)) in v.skills[range].iter().enumerate() {
        let mut it = SheetItem::new(right.row(k, n), ItemKind::Label, name).enabled(*learned);
        it.region = Some(region);
        s.push(it);
    }
    let eq = Rect::new(0.67, 0.64, 0.30, 0.27);
    for (k, (slot, item)) in v.equipment.iter().take(6).enumerate() {
        s.push(SheetItem::new(eq.row(k, 6), ItemKind::Label, slot).value(namer(item)));
    }
    s
}

/// Convenience for tests and hosts: the inventory list of a [`PlayerPanel`] as plain text rows.
pub fn panel_rows(p: &PlayerPanel, namer: Namer) -> Vec<String> {
    p.rows.iter().map(|r| format!("{} x{}", namer(&r.item), r.count)).collect()
}
