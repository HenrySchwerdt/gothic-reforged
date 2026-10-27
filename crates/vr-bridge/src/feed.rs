//! The core's state as the view models the VR UI shows, and the VR UI's window actions as
//! calls on the core. Everything here is a projection: no rule is decided in this module.
//! Trade prices, baskets and the deal are `gothic_sim` (`trade_*`), recipes are the script
//! host's, the quest log is `GameState`; VR only chooses the layout (`gothic-vr-ui`).
//!
//! The desktop host does the same in `apps/game/src/play/windows.rs`; the two stay in step
//! because both call the same `Sim` functions and the same `gothic-ui` view models.
use gothic_gameplay::dialog::Choice;
use gothic_gameplay::inventory::{Inventory, Stack};
use gothic_gameplay::state::{GameState, LogKind};
use gothic_gameplay::trade::{self, GOLD_TEMPLATE, ListMode, Step};
use gothic_gameplay::transcript::quest_title;
use gothic_runtime::{Command, Core};
use gothic_sim::sim::Sim;
use gothic_sim::types::ActorId;
use gothic_sim::world::WorldQuery;
use gothic_ui::journal::{clock_from_ms, format_clock, quest_rows, status_line};
use gothic_ui::{
    Ingredient, LootView, PageEvent, PageRow, PlayerPanel, RecipeRow, RecipeView, TradeView,
};
use gothic_vr_ui::{CharacterView, QuestEntry, QuestLogView, Vitals, WindowAction};
use std::collections::HashMap;

/// Localised name of an item template (`FO_<template>`), the template without prefix and
/// underscores when the loc table has none.
pub fn item_name(core: &Core, template: &str) -> String {
    core.db
        .text(&format!("FO_{template}"))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| gothic_vr_ui::views::guess_name(template))
}

/// Fixed window texts from the original loc table (`HUD_*` keys), flat UI fallbacks otherwise.
pub fn labels(core: &Core) -> gothic_vr_ui::views::Labels {
    gothic_vr_ui::views::Labels::from_text(&|k| core.db.text(k))
}

/// Icon file name (`IT_1H_OrcSlayer_01.tga`) of every template, from the item catalog the flat UI
/// loads (`ItemCatalog`), for `HudData::icons`.
pub fn icons<'a>(
    catalog: &gothic_ui::ItemCatalog,
    templates: impl IntoIterator<Item = &'a str>,
) -> HashMap<String, String> {
    templates
        .into_iter()
        .filter_map(|t| Some((t.to_string(), catalog.get(t)?.icon.clone()?)))
        .collect()
}

/// Names of every template in `templates`, for `HudData::names`.
pub fn names<'a>(
    core: &Core,
    templates: impl IntoIterator<Item = &'a str>,
) -> HashMap<String, String> {
    templates
        .into_iter()
        .map(|t| (t.to_string(), item_name(core, t)))
        .collect()
}

/// Health, mana and stamina of the hero.
pub fn vitals(core: &Core) -> Vitals {
    let hero = core.sim.actor(core.hero);
    Vitals {
        health: (hero.hp.max(0) as f32, hero.hp_max(&core.sim.cfg) as f32),
        mana: (hero.mana as f32, hero.mana_max() as f32),
        stamina: (hero.stamina as f32, hero.stamina_max(&core.sim.cfg) as f32),
    }
}

fn gold_of(inv: &Inventory) -> i64 {
    i64::from(inv.count(GOLD_TEMPLATE))
}

/// The hero's inventory as rows (`key` = stack index, the key `UseItem` is looked up by).
pub fn hero_panel(core: &Core) -> PlayerPanel {
    let inv = &core.sim.actor(core.hero).inventory;
    PlayerPanel {
        title: String::new(),
        rows: inv
            .stacks()
            .iter()
            .enumerate()
            .filter(|(_, s)| s.amount > 0)
            .map(|(i, s)| PageRow {
                item: s.template.clone(),
                count: s.amount,
                key: i,
                ..Default::default()
            })
            .collect(),
        gold: gold_of(inv),
    }
}

/// `HudAction::InventoryItem(key)` -> the command that uses / equips / drinks the item
/// (`OnPlayerUseStack`; weapons and armor are worn by the same script).
pub fn use_stack(core: &Core, key: usize) -> Option<Command> {
    core.sim
        .actor(core.hero)
        .inventory
        .stack(key)
        .map(|s| Command::UseItem {
            template: s.template.clone(),
        })
}

fn rows_of<W: WorldQuery>(
    sim: &Sim<W>,
    npc: ActorId,
    inv: &Inventory,
    player_side: bool,
    mode: ListMode,
) -> Vec<PageRow> {
    let ctx = sim.trade_price_ctx(npc).unwrap_or_default();
    inv.stacks()
        .iter()
        .enumerate()
        .filter(|(_, s)| player_side || Sim::<W>::trader_offers(s))
        .filter_map(|(i, s): (usize, &Stack)| {
            let count = trade::list_count(s, mode);
            (count > 0).then(|| PageRow {
                item: s.template.clone(),
                count,
                price: sim.trade_unit_price(ctx, s, player_side),
                key: i,
                locked: mode == ListMode::Remaining && !trade::can_move(s, mode, player_side),
                label: None,
            })
        })
        .collect()
}

/// The trade page with `npc` (open it with `Sim::trade_open` first, as the desktop host does).
pub fn trade_view(core: &mut Core, npc: ActorId, hint: &str) -> TradeView {
    let totals = core.sim.trade_totals(npc);
    let sim = &core.sim;
    let hero = &sim.actor(core.hero).inventory;
    let trader = &sim.actor(npc).inventory;
    let vendor_gold = trader
        .stacks()
        .iter()
        .filter(|s| s.template.eq_ignore_ascii_case(GOLD_TEMPLATE))
        .map(|s| i64::from(trade::list_count(s, ListMode::Remaining)))
        .sum();
    TradeView {
        vendor: sim.actor(npc).name.replace('_', " "),
        vendor_rows: rows_of(sim, npc, trader, false, ListMode::Remaining),
        vendor_gold,
        player: PlayerPanel {
            title: String::new(),
            rows: rows_of(sim, npc, hero, true, ListMode::Remaining),
            gold: gold_of(hero),
        },
        ask_rows: rows_of(sim, npc, trader, false, ListMode::Basket),
        offer_rows: rows_of(sim, npc, hero, true, ListMode::Basket),
        ask_total: totals.ask,
        offer_total: totals.offer,
        deal: totals.deal_enabled(),
        balance: totals.balance_enabled(),
        hint: hint.to_string(),
    }
}

/// What a trade page event did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TradeOutcome {
    /// The view must be rebuilt.
    pub changed: bool,
    /// The deal went through.
    pub dealt: bool,
    /// The move was refused (play the deny sound).
    pub refused: bool,
}

/// Apply a trade `PageEvent` exactly like the desktop host (`step` = how many per click).
pub fn apply_trade_event(core: &mut Core, npc: ActorId, e: &PageEvent, step: i32) -> TradeOutcome {
    let mut out = TradeOutcome::default();
    match e {
        PageEvent::TradeAdd { player_side, key } => {
            out.refused = core.sim.trade_step(npc, *player_side, *key, true, step) == Step::Refused;
            out.changed = true;
        }
        PageEvent::TradeRemove { player_side, key } => {
            core.sim.trade_step(npc, *player_side, *key, false, step);
            out.changed = true;
        }
        PageEvent::TradeClear { player_side } => {
            core.sim.trade_clear(npc, *player_side);
            out.changed = true;
        }
        PageEvent::TradeBalance => {
            core.sim.trade_balance(npc);
            out.changed = true;
        }
        PageEvent::TradeDeal => {
            out.dealt = core.sim.trade_deal(npc).is_ok();
            out.refused = !out.dealt;
            out.changed = true;
        }
        _ => {}
    }
    if out.changed {
        core.sync_inventory();
    }
    out
}

/// One line of a container or body: `(template, amount, text)`.
pub type LootLine = (String, i32, String);

/// A chest / body window; `lines` in the order `Command::TakeLoot { index }` expects.
pub fn loot_view(core: &Core, title: &str, lines: &[LootLine], with_gold: bool) -> LootView {
    LootView {
        title: title.replace('_', " "),
        rows: lines
            .iter()
            .enumerate()
            .map(|(key, (item, count, text))| PageRow {
                item: item.clone(),
                count: *count,
                key,
                label: (!text.is_empty()).then(|| text.clone()),
                ..Default::default()
            })
            .collect(),
        pool_gold: with_gold.then(|| {
            lines
                .iter()
                .filter(|l| l.0.eq_ignore_ascii_case(GOLD_TEMPLATE))
                .map(|l| i64::from(l.1))
                .sum()
        }),
        take_all: with_gold,
        player: hero_panel(core),
        hint: String::new(),
    }
}

/// `LootTake` / `LootTakeAll` of container `object` -> `Command::TakeLoot`.
pub fn loot_command(object: u32, e: &PageEvent) -> Option<Command> {
    match e {
        PageEvent::LootTake { key } => Some(Command::TakeLoot {
            object,
            index: Some(*key),
        }),
        PageEvent::LootTakeAll => Some(Command::TakeLoot {
            object,
            index: None,
        }),
        _ => None,
    }
}

/// The recipes of a work station (`ids` = registered recipe ids; `key` of a row = its index
/// in `ids`, which `PageEvent::Craft` echoes).
pub fn recipe_view(core: &Core, title: &str, ids: &[i32], footer: &str) -> RecipeView {
    let hero = core.sim.actor(core.hero);
    let recipes = ids
        .iter()
        .enumerate()
        .filter_map(|(key, id)| {
            let r = core.sim.script.recipe_of(*id)?;
            let ingredients: Vec<Ingredient> = r
                .ingredients
                .iter()
                .map(|(t, need)| Ingredient {
                    item: t.clone(),
                    name: item_name(core, t),
                    need: *need,
                    have: hero.inventory.count(t),
                })
                .collect();
            let skilled = r.required_perk.is_empty() || hero.has_perk(&r.required_perk);
            Some(RecipeRow {
                key,
                product: r.result.clone(),
                name: item_name(core, &r.result),
                description: core
                    .db
                    .text(&format!("FD_{}", r.result))
                    .unwrap_or_default(),
                craftable: skilled && ingredients.iter().all(|i| i.have >= i.need),
                ingredients,
                requirement: if r.required_perk.is_empty() {
                    String::new()
                } else {
                    item_name(core, &r.required_perk)
                },
            })
        })
        .collect();
    RecipeView {
        title: title.to_string(),
        recipes,
        gold: gold_of(&hero.inventory),
        hint: footer.to_string(),
    }
}

/// `PageEvent::Craft { key }` -> `Command::Craft` of recipe `ids[key]`.
pub fn craft_command(ids: &[i32], e: &PageEvent) -> Option<Command> {
    match e {
        PageEvent::Craft { key } => ids.get(*key).map(|id| Command::Craft { recipe: *id }),
        _ => None,
    }
}

/// Text of a log entry (`AddQuestLog` key -> localised text, dialog excerpt, status line).
fn log_text(core: &Core, kind: &LogKind) -> Option<String> {
    match kind {
        LogKind::Status(_) => status_line(kind).map(String::from),
        LogKind::Text(key) => Some(core.db.text(key).unwrap_or_else(|| key.clone())),
        LogKind::Dialog(t) => Some(t.clone()),
    }
}

/// The quest log: every quest that has started, its tab and its log (newest first).
pub fn quest_log(core: &Core) -> QuestLogView {
    let st: &GameState = core.state();
    let mut quests = Vec::new();
    for tab in 0..3 {
        for row in quest_rows(st, tab, |n| quest_title(&core.db, n)) {
            let log = st
                .log
                .iter()
                .rev()
                .filter(|e| e.quest == row.name)
                .filter_map(|e| log_text(core, &e.kind))
                .collect();
            quests.push(QuestEntry {
                name: row.name,
                title: row.title,
                tab,
                log,
            });
        }
    }
    let (day, hour, minute) = clock_from_ms(st.hours.saturating_mul(3_600_000));
    let fmt = core.db.text("HUD_Time").unwrap_or_default();
    let reputation = st
        .fame
        .iter()
        .filter(|(_, v)| **v != 0)
        .map(|(k, v)| {
            (
                k.split_once(':').map_or(k.as_str(), |(_, n)| n).to_string(),
                *v,
            )
        })
        .collect();
    QuestLogView {
        quests,
        clock: format_clock(&fmt, 0, day, hour, minute),
        hour,
        with_quest: locations_with_running_quest(core),
        reputation,
    }
}

/// Attributes, protection, skills and equipment of the hero (the values the flat character
/// page shows: `GameState::attributes`, `ATTRIB_<tag>` names).
pub fn character(core: &Core) -> CharacterView {
    let hero = core.sim.actor(core.hero);
    let st = core.state();
    let val = |tag: &str| st.attributes.get(&tag.to_ascii_lowercase()).copied().unwrap_or(0);
    let attributes = ["STR", "HNT", "INT", "SMT", "THF", "ALC"]
        .into_iter()
        .map(|tag| {
            let name = if tag == "HNT" {
                "Jagdgeschick".to_string()
            } else {
                core.db.text(&format!("ATTRIB_{tag}")).unwrap_or_else(|| tag.to_string())
            };
            (name, val(tag))
        })
        .collect();
    let protection = [
        "prot_blade",
        "prot_impact",
        "prot_missile",
        "prot_fire",
        "prot_ice",
        "prot_lightning",
    ]
    .into_iter()
    .map(val)
    .collect();
    let eq = &hero.equipment;
    let equipment = [
        ("Rechte Hand", &eq.right),
        ("Linke Hand", &eq.left),
        ("Körper", &eq.body),
    ]
    .into_iter()
    .filter_map(|(slot, it)| it.as_ref().map(|i| (slot.to_string(), i.name.clone())))
    .collect();
    CharacterView {
        level: st.level,
        xp: st.xp,
        xp_next: GameState::xp_next(st.level),
        learning_points: st.learning_points,
        vitals: vitals(core),
        attributes,
        protection,
        skills: st
            .skills
            .iter()
            .map(|k| {
                let name = core
                    .db
                    .text(&format!("FO_it_{k}"))
                    .or_else(|| core.db.text(&format!("FO_{k}")))
                    .unwrap_or_else(|| k.replace('_', " "));
                (gothic_vr_ui::views::skill_group(k), name)
            })
            .collect(),
        equipment,
    }
}

/// Dialog options from a `MenuChanged` event (`Command::Choose { index }` takes the index).
pub fn dialog_options(menu: &[Choice]) -> Vec<String> {
    menu.iter().map(|c| c.label.clone()).collect()
}

/// What a window action turned into.
#[derive(Debug, Clone)]
pub enum Applied {
    /// Core commands to send with the next `Core::tick`.
    Commands(Vec<Command>),
    /// The trade page changed (rebuild it with [`trade_view`]).
    Trade(TradeOutcome),
    /// A save-page command: the host lists / deletes files and maps the rest with
    /// [`file_command`].
    File(gothic_ui::FileCommand),
    /// The window closes; the host sends `GuiOpen(false)` / `EndInteraction`.
    Close,
    Nothing,
}

/// The window the host has open (decides how page events map to the core).
#[derive(Debug, Clone, PartialEq)]
pub enum OpenWindow {
    Trade { npc: ActorId },
    Loot { object: u32 },
    Station { recipes: Vec<i32> },
    Other,
}

/// Turn a window action into core calls. Trade moves happen right here (`Sim` calls, like the
/// desktop); everything else is a `Command` for the next tick.
pub fn apply_action(core: &mut Core, open: &OpenWindow, a: &WindowAction, step: i32) -> Applied {
    match (a, open) {
        (WindowAction::Close, _) | (WindowAction::Page(PageEvent::Close), _) => Applied::Close,
        (WindowAction::File(c), _) => {
            Applied::File(c.clone())
        }
        (WindowAction::Wait(h), _) => Applied::Commands(vec![wait_command(*h)]),
        (WindowAction::UseItem(t), _) => Applied::Commands(vec![Command::UseItem {
            template: t.clone(),
        }]),
        (WindowAction::Page(e), OpenWindow::Trade { npc }) => {
            Applied::Trade(apply_trade_event(core, *npc, e, step))
        }
        (WindowAction::Page(e), OpenWindow::Loot { object }) => {
            loot_command(*object, e).map_or(Applied::Nothing, |c| Applied::Commands(vec![c]))
        }
        (WindowAction::Page(e), OpenWindow::Station { recipes }) => {
            craft_command(recipes, e).map_or(Applied::Nothing, |c| Applied::Commands(vec![c]))
        }
        _ => Applied::Nothing,
    }
}

/// The save / load page from the host's slot list (`gothic_ui::SlotRow`s, newest first).
pub fn files_view(
    core: &Core,
    rows: &[gothic_ui::SlotRow],
    can_save: bool,
    save_mode: bool,
) -> gothic_vr_ui::FilesView {
    let quick = core
        .db
        .text("HUD_QuickSave")
        .unwrap_or_else(|| "Schnellspeicherung".into());
    let fmt = core.db.text("HUD_File_SaveGame_Format").unwrap_or_default();
    gothic_vr_ui::FilesView {
        save_mode,
        can_save,
        rows: rows
            .iter()
            .map(|r| {
                let (title, clock) = gothic_ui::files::caption(r, &quick, &fmt);
                gothic_vr_ui::FileRowView {
                    key: r.key.clone(),
                    title,
                    clock,
                    manual: r.kind == gothic_ui::SlotKind::Manual,
                }
            })
            .collect(),
    }
}

/// A save-page button as the core command (`dir` = the host's save directory). New slots are
/// named `manual_<unix>.json` (the core classifies the slot by the file name); deleting a file is
/// the host's file-system business (`None`).
pub fn file_command(dir: &std::path::Path, c: &gothic_ui::FileCommand, unix: u64) -> Option<Command> {
    match c {
        gothic_ui::FileCommand::Load { key } => Some(Command::Load(dir.join(key))),
        gothic_ui::FileCommand::Save { key: Some(k) } => Some(Command::Save(dir.join(k))),
        gothic_ui::FileCommand::Save { key: None } => {
            Some(Command::Save(dir.join(format!("manual_{unix}.json"))))
        }
        _ => None,
    }
}

/// The map location a quest belongs to, from the original quest template: `Quest::folder` is the
/// editor folder (`Varant/Braga/`), whose segments name the town; the first segment (from the
/// innermost) that is one of the quest page's location buttons (`gothic_ui::map::LOCATIONS`) wins.
pub fn quest_location(core: &Core, quest: &str) -> Option<String> {
    let q = core.db.quest_by_name(quest)?;
    q.folder
        .split(['/', '\\'])
        .rev()
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase)
        .find(|seg| gothic_ui::map::LOCATIONS.iter().any(|(n, _, _)| *n == seg))
}

/// Lower-case names of the locations with a running quest (`QuestLogView::with_quest`, the
/// "Haupt" filter of the quest map).
pub fn locations_with_running_quest(core: &Core) -> Vec<String> {
    let mut v: Vec<String> = core
        .state()
        .quests
        .iter()
        .filter(|(_, q)| q.status == Some(gothic_gameplay::model::QuestStatus::Running))
        .filter_map(|(n, _)| quest_location(core, n))
        .collect();
    v.sort();
    v.dedup();
    v
}

/// The command for the wait picker / any wait: the core's own `PlayerSleep` port advances the
/// clock, restores the hero and reports `Slept` (no state is poked from the host).
pub fn wait_command(hours: u32) -> Command {
    Command::Sleep {
        hours: Some(hours as f32),
    }
}

/// `WindowAction::Wait` as the command (`None` for anything else).
pub fn wait_action(a: &WindowAction) -> Option<Command> {
    match a {
        WindowAction::Wait(h) => Some(wait_command(*h)),
        _ => None,
    }
}
