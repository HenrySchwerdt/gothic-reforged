//! The HUD pages of the play mode: the trade page (`gCHUDPageTrade2`) and the loot-style list
//! page (`gCHUDPageLoot2`, also used for body plunder and the work-station recipes). The
//! layouts are drawn by `gothic-ui` (`crates/ui/src/pages.rs`); this module turns the open
//! window and the inventories into view models and the clicks of the page into core calls.
//!
//! Mouse: a click on an item cell moves one (Shift ten, Ctrl all) into the basket of its side;
//! a click in a basket takes it back out. Keyboard: digits take the n-th loot entry, Enter
//! takes everything (loot) or closes the deal (trade), Esc closes.
//!
//! `G3_SMOKE_UI=trade|loot` (development): open the page by itself once the world is ready,
//! fill the baskets, write `.local/play-trade.png` / `.local/play-loot.png` and quit.
use super::Roster;
use super::interact::{Interaction, LineAction, OpenWindow, WindowKind, open_trade};
use super::objects::WorldObjects;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use gothic_audio::PlaySfx;
use gothic_gameplay::inventory::{Inventory, Stack};
use gothic_gameplay::trade::{self, GOLD_TEMPLATE, ListMode, Step};
use gothic_runtime::{CoreCommand, CoreRes};
use gothic_sim::sim::Sim;
use gothic_sim::types::ActorId;
use gothic_sim::world::WorldQuery;
use gothic_ui::{
    ApplyVideo, HostGate, Ingredient, LootView, Notify, PageEvent, PageRow, PageView, PlayerPanel,
    RecipeRow, RecipeView, Screen, ShowPage, TeachCost, ToastKind, TradeView, UiState,
};

pub fn register(app: &mut App) {
    app.add_systems(
        Update,
        (
            page_events,
            show_pages,
            hold_cursor,
            smoke,
            gui_gate,
            install_teach_cost,
            apply_video,
        )
            .chain(),
    );
}

/// Esc belongs to the world while an interaction window is open; the GUI manager must not open
/// the menu then.
fn gui_gate(inter: Res<Interaction>, mut gate: ResMut<HostGate>) {
    gate.esc_blocked = inter.window.is_some();
}

/// Teachers charge learning points by the game rule of `gothic_sim::economy::teach_cost`.
fn install_teach_cost(core: Res<CoreRes>, mut cost: ResMut<TeachCost>, mut done: Local<bool>) {
    if std::mem::replace(&mut *done, true) {
        return;
    }
    let cfg = core.0.sim.cfg.clone();
    cost.0 = std::sync::Arc::new(move |tag, current, points| {
        gothic_sim::economy::teach_cost(&cfg, tag, current, points)
    });
}

/// Apply the video page: present mode and window size (brightness / gamma ramps need a
/// display-ramp or grading hook that the renderer does not have yet).
fn apply_video(
    mut events: MessageReader<ApplyVideo>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    for ApplyVideo(v) in events.read() {
        let Ok(mut w) = windows.single_mut() else {
            continue;
        };
        w.present_mode = if v.vsync {
            bevy::window::PresentMode::AutoVsync
        } else {
            bevy::window::PresentMode::AutoNoVsync
        };
        w.resolution.set(v.width as f32, v.height as f32);
    }
}

/// The recipes of a work-station window as the recipe page (`gCHUDPageSynth2`).
fn recipe_view(core: &CoreRes, win: &OpenWindow) -> RecipeView {
    let hero = core.0.sim.actor(core.0.hero);
    let name = |t: &str| {
        core.0
            .db
            .text(&format!("FO_{t}"))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| t.replace('_', " "))
    };
    let recipes = win
        .lines
        .iter()
        .enumerate()
        .filter_map(|(key, l)| {
            let LineAction::Craft(id) = l.action else {
                return None;
            };
            let r = core.0.sim.script.recipe_of(id)?;
            let ingredients: Vec<Ingredient> = r
                .ingredients
                .iter()
                .map(|(t, need)| Ingredient {
                    item: t.clone(),
                    name: name(t),
                    need: *need,
                    have: hero.inventory.count(t),
                })
                .collect();
            let skilled = r.required_perk.is_empty() || hero.has_perk(&r.required_perk);
            Some(RecipeRow {
                key,
                product: r.result.clone(),
                name: name(&r.result),
                description: core
                    .0
                    .db
                    .text(&format!("FD_{}", r.result))
                    .unwrap_or_default(),
                craftable: skilled && ingredients.iter().all(|i| i.have >= i.need),
                ingredients,
                requirement: if r.required_perk.is_empty() {
                    String::new()
                } else {
                    name(&r.required_perk)
                },
            })
        })
        .collect();
    RecipeView {
        title: win.title.clone(),
        recipes,
        gold: gold_of(&hero.inventory),
        hint: win.footer.clone(),
    }
}

fn gold_of(inv: &Inventory) -> i64 {
    i64::from(inv.count(GOLD_TEMPLATE))
}

/// The hero's inventory as the right-hand panel.
fn hero_panel(core: &CoreRes) -> PlayerPanel {
    let inv = &core.0.sim.actor(core.0.hero).inventory;
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
                ..default()
            })
            .collect(),
        gold: gold_of(inv),
    }
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

fn trade_view(core: &mut CoreRes, npc: ActorId, hint: &str) -> TradeView {
    let totals = core.0.sim.trade_totals(npc);
    let sim = &core.0.sim;
    let hero = &sim.actor(core.0.hero).inventory;
    let trader = &sim.actor(npc).inventory;
    let vendor_gold = trader
        .stacks()
        .iter()
        .filter(|s| s.template.eq_ignore_ascii_case(GOLD_TEMPLATE))
        .map(|s| i64::from(trade::list_count(s, ListMode::Remaining)))
        .sum();
    let mut player = hero_panel(core);
    player.rows = rows_of(sim, npc, hero, true, ListMode::Remaining);
    TradeView {
        vendor: sim.actor(npc).name.clone(),
        vendor_rows: rows_of(sim, npc, trader, false, ListMode::Remaining),
        vendor_gold,
        player,
        ask_rows: rows_of(sim, npc, trader, false, ListMode::Basket),
        offer_rows: rows_of(sim, npc, hero, true, ListMode::Basket),
        ask_total: totals.ask,
        offer_total: totals.offer,
        deal: totals.deal_enabled(),
        balance: totals.balance_enabled(),
        hint: hint.to_string(),
    }
}

fn loot_view(core: &CoreRes, win: &OpenWindow) -> LootView {
    let rows = win
        .lines
        .iter()
        .enumerate()
        .map(|(i, l)| PageRow {
            item: l.item.clone(),
            count: l.count,
            key: i,
            label: Some(l.text.clone()),
            ..default()
        })
        .collect();
    let (pool_gold, take_all) = match win.kind {
        WindowKind::Station { .. } => (None, false),
        _ => (
            Some(
                win.lines
                    .iter()
                    .filter(|l| l.item.eq_ignore_ascii_case(GOLD_TEMPLATE))
                    .map(|l| i64::from(l.count))
                    .sum(),
            ),
            true,
        ),
    };
    LootView {
        title: win.title.replace('_', " "),
        rows,
        pool_gold,
        take_all,
        player: hero_panel(core),
        hint: String::new(),
    }
}

/// Send the page to the UI whenever the window model changed (and again if another screen
/// took the stage, e.g. a late dialog update).
fn show_pages(
    mut inter: ResMut<Interaction>,
    mut core: ResMut<CoreRes>,
    ui: Res<UiState>,
    mut out: MessageWriter<ShowPage>,
) {
    let Some(win) = inter.window.clone() else {
        if std::mem::take(&mut inter.page_shown) {
            out.write(ShowPage(None));
        }
        return;
    };
    let expected = match win.kind {
        WindowKind::Trade { .. } => Screen::Trade,
        WindowKind::Station { .. } => Screen::Recipe,
        _ => Screen::Loot,
    };
    let lost = inter.page_shown && ui.screen != expected;
    if !win.dirty && !lost {
        return;
    }
    if let Some(w) = inter.window.as_mut() {
        w.dirty = false;
    }
    let view = match win.kind {
        WindowKind::Trade { npc } => PageView::Trade(trade_view(&mut core, npc, &win.footer)),
        WindowKind::Station { .. } => PageView::Recipe(recipe_view(&core, &win)),
        _ => PageView::Loot(loot_view(&core, &win)),
    };
    out.write(ShowPage(Some(view)));
    inter.page_shown = true;
}

#[allow(clippy::too_many_arguments)]
fn page_events(
    mut events: MessageReader<PageEvent>,
    mut inter: ResMut<Interaction>,
    mut core: ResMut<CoreRes>,
    keys: Res<ButtonInput<KeyCode>>,
    mut notify: MessageWriter<Notify>,
    mut sound: MessageWriter<PlaySfx>,
) {
    let mut deal = std::mem::take(&mut inter.trade_deal);
    let mut changed = false;
    let mut hint: Option<String> = None;
    let npc = match inter.window.as_ref().map(|w| &w.kind) {
        Some(WindowKind::Trade { npc }) => Some(*npc),
        _ => None,
    };
    let step = if keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) {
        1_000_000
    } else if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
        10
    } else {
        1
    };
    for e in events.read() {
        match (e, npc) {
            (PageEvent::Close, _) => {
                inter.page_shown = false;
                inter.window_close = true;
            }
            (PageEvent::LootTake { key }, _) => inter.window_key = Some(key + 1),
            (PageEvent::LootTakeAll, _) => inter.window_enter = true,
            (PageEvent::Craft { key }, _) => inter.window_key = Some(key + 1),
            (PageEvent::TradeAdd { player_side, key }, Some(npc)) => {
                let r = core.0.sim.trade_step(npc, *player_side, *key, true, step);
                if r == Step::Refused {
                    hint = Some(refusal(&core, npc, *player_side, *key));
                    sound.write(PlaySfx::new("sfx_inv_deny_01", None));
                }
                changed = true;
            }
            (PageEvent::TradeRemove { player_side, key }, Some(npc)) => {
                core.0.sim.trade_step(npc, *player_side, *key, false, step);
                changed = true;
            }
            (PageEvent::TradeClear { player_side }, Some(npc)) => {
                core.0.sim.trade_clear(npc, *player_side);
                changed = true;
            }
            (PageEvent::TradeBalance, Some(npc)) => {
                core.0.sim.trade_balance(npc);
                changed = true;
            }
            (PageEvent::TradeDeal, Some(_)) => deal = true,
            _ => {}
        }
    }
    let Some(npc) = npc else { return };
    if deal {
        match core.0.sim.trade_deal(npc) {
            Ok(()) => {
                sound.write(PlaySfx::new("sfx_inv_click_02", None));
                notify.write(Notify {
                    text: "Handel abgeschlossen".into(),
                    kind: ToastKind::Info,
                });
                hint = Some(String::new());
            }
            Err(_) => {
                sound.write(PlaySfx::new("sfx_inv_deny_01", None));
            }
        }
        changed = true;
    }
    if changed {
        core.0.sync_inventory();
        if let Some(w) = inter.window.as_mut() {
            w.dirty = true;
            if let Some(h) = hint {
                w.footer = h;
            }
        }
    }
}

/// Text for a move the script refused: faction armor needs fame, the rest is not movable.
fn refusal(core: &CoreRes, npc: ActorId, player_side: bool, key: usize) -> String {
    let inv = if player_side {
        &core.0.sim.actor(core.0.hero).inventory
    } else {
        &core.0.sim.actor(npc).inventory
    };
    let Some(s) = inv.stack(key) else {
        return String::new();
    };
    if !player_side && !core.0.sim.trade_wants_to_sell(npc, s, false) {
        return core
            .0
            .db
            .text("GO_RequiredPoliticalReputation")
            .unwrap_or_else(|| "Dein Ruf reicht dafuer nicht aus".into());
    }
    if s.item().mission_item {
        return "Das ist kein Handelsgut".into();
    }
    String::new()
}

/// The mouse is needed while a page is open; `walk.rs` re-locks it on every click, so it is
/// freed again each frame.
fn hold_cursor(
    inter: Res<Interaction>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mut relock: Local<Option<bool>>,
) {
    let Ok(mut c) = cursor.single_mut() else {
        return;
    };
    if inter.window.is_some() {
        if relock.is_none() {
            *relock = Some(c.grab_mode != CursorGrabMode::None);
        }
        if c.grab_mode != CursorGrabMode::None {
            c.grab_mode = CursorGrabMode::None;
        }
        if !c.visible {
            c.visible = true;
        }
    } else if let Some(was) = relock.take()
        && was
    {
        c.grab_mode = CursorGrabMode::Locked;
        c.visible = false;
    }
}

#[derive(Default)]
struct SmokeState {
    step: u8,
    at: f32,
}

/// `G3_SMOKE_UI` development hook, see the module comment.
#[allow(clippy::too_many_arguments)]
fn smoke(
    mut st: Local<SmokeState>,
    time: Res<Time>,
    roster: Res<Roster>,
    objects: Res<WorldObjects>,
    mut core: ResMut<CoreRes>,
    mut inter: ResMut<Interaction>,
    mut cmd: MessageWriter<CoreCommand>,
    mut pages: MessageWriter<PageEvent>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
) {
    let Ok(mode) = std::env::var("G3_SMOKE_UI") else {
        return;
    };
    let now = time.elapsed_secs();
    if st.step == 0 {
        if now < 8.0 || roster.total == 0 || roster.spawned < roster.total {
            return;
        }
        let Some(npc) = core.0.find_name("Hamlar") else {
            return;
        };
        let hero = core.0.hero;
        // the hero carries some gold for the trade
        core.0.sim.actor_mut(hero).inventory.add("It_Gold", 750);
        core.0.sim.state.inventory.add("It_Gold", 750);
        if mode == "trade" {
            open_trade(&mut core, &objects, &mut inter, &mut cmd, npc);
        } else {
            let lines = {
                let inv = &core.0.sim.actor(npc).inventory;
                inv.stacks()
                    .iter()
                    .enumerate()
                    .map(|(i, s)| super::interact::WindowLine {
                        text: format!("{} x {}", s.amount, s.template),
                        action: LineAction::Take(i),
                        item: s.template.clone(),
                        count: s.amount,
                    })
                    .collect()
            };
            inter.window = Some(OpenWindow {
                kind: WindowKind::Body { actor: npc },
                title: "Beute".into(),
                lines,
                footer: String::new(),
                dirty: true,
            });
        }
        st.step = 1;
        st.at = now;
        return;
    }
    let dt = now - st.at;
    match (st.step, mode.as_str()) {
        (1, "trade") if dt > 1.0 => {
            // two of the first merchant stacks and a balance, like a player would click
            pages.write(PageEvent::TradeAdd {
                player_side: false,
                key: first_key(&core, &inter),
            });
            pages.write(PageEvent::TradeBalance);
            st.step = 2;
            st.at = now;
        }
        (2, "trade") if dt > 1.0 => {
            pages.write(PageEvent::TradeBalance);
            st.step = 3;
            st.at = now;
        }
        (1, "loot") if dt > 2.0 => st.step = 3,
        (3, _) => {
            let name = if mode == "trade" { "trade" } else { "loot" };
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(format!(".local/play-{name}.png")));
            st.step = 4;
            st.at = now;
        }
        (4, _) if dt > 2.0 => {
            exit.write(AppExit::Success);
            st.step = 5;
        }
        _ => {}
    }
}

fn first_key(core: &CoreRes, inter: &Interaction) -> usize {
    let Some(WindowKind::Trade { npc }) = inter.window.as_ref().map(|w| &w.kind) else {
        return 0;
    };
    core.0
        .sim
        .actor(*npc)
        .inventory
        .stacks()
        .iter()
        .position(|s| {
            s.kind == gothic_gameplay::inventory::StackType::Merchandise
                && s.amount > 0
                && s.item_gold_value() > 1
        })
        .unwrap_or(0)
}
