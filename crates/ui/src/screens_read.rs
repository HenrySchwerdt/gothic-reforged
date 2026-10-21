//! Work-station recipe page (`gCHUDPageSynth2` + `gCHUDViewRecipe2`) and the readable
//! documents (`gCHUDViewBook2`, `gCHUDViewLetter2`). Rectangles are the control placements of
//! `g3.gui.res` (1024x768 design space); the recipe card sits at (+17,+65) in the left panel.
use crate::atlas::*;
use crate::hud::item_icon;
use crate::pages::{DocumentView, RecipeRow};
use crate::reading;
use crate::screens::{Box4, Cx, body, gold_line, heading, row_grid, txt_wrap};
use crate::screens_menu::menu_btn;
use crate::style::*;
use crate::*;

/// Ink of the parchment views.
const INK: Color = Color::srgb(0.17, 0.10, 0.05);
const INK_BAD: Color = Color::srgb(0.55, 0.08, 0.05);

fn ink_heading(p: &mut P, cx: &Cx, s: impl Into<String>, r: Box4) {
    label(p, &cx.a.heading, s, r, 22.0, INK, Al::Center);
}

const PANEL: (f32, f32) = (14.0, 16.0);
/// Card origin: panel + (17,65).
const CARD: (f32, f32) = (PANEL.0 + 17.0, PANEL.1 + 65.0);

/// Colour of an ingredient line: short ones are red.
fn ingredient_color(have: u32, need: u32) -> Color {
    if have >= need { INK } else { INK_BAD }
}

pub(crate) fn recipe_page(p: &mut P, cx: &Cx) {
    let Some(PageView::Recipe(v)) = cx.ui.page.as_ref() else {
        return;
    };
    spawn_pic(p, cx.a, SYNTH_RECIPE, PANEL.0, PANEL.1, None);
    let sel = cx.ui.recipe_sel.min(v.recipes.len().saturating_sub(1));
    match v.recipes.get(sel) {
        Some(r) => recipe_card(p, cx, r),
        None => {
            spawn_pic(p, cx.a, RECIPE, CARD.0, CARD.1, None);
            body(
                p,
                cx,
                cx.t("HUD_NoRecipes", "Keine Rezepte bekannt"),
                (CARD.0 + 6.0, CARD.1 + 200.0, 318.0, 30.0),
                Al::Center,
                TEXT_DIM,
            );
        }
    }

    // amount box between the panels: the craft button
    let craft = v
        .recipes
        .get(sel)
        .filter(|r| r.craftable)
        .map(|r| Action::Page(PageEvent::Craft { key: r.key }));
    menu_btn(
        p,
        cx,
        &cx.t("HUD_Craft", "Herstellen"),
        (394.0, 320.0, 236.0, 36.0),
        craft,
    );
    menu_btn(
        p,
        cx,
        &cx.t("HUD_Close", "Schliessen"),
        (394.0, 366.0, 236.0, 36.0),
        Some(Action::Close),
    );
    if !v.hint.is_empty() {
        body(
            p,
            cx,
            v.hint.clone(),
            (384.0, 412.0, 256.0, 40.0),
            Al::Center,
            TEXT_DIM,
        );
    }

    // right: the known recipes as an icon list
    let (ox, oy) = (634.0, 16.0);
    spawn_pic(p, cx.a, INVENTORY, ox, oy, None);
    heading(p, cx, v.title.clone(), (ox + 26.0, oy + 12.0, 260.0, 25.0));
    let rows: Vec<PageRow> = v
        .recipes
        .iter()
        .enumerate()
        .map(|(i, r)| PageRow {
            item: r.product.clone(),
            key: i,
            label: Some(r.name.clone()),
            ..default()
        })
        .collect();
    let refs: Vec<&PageRow> = rows.iter().collect();
    let pick = |r: &PageRow| Action::SelectRecipe(r.key);
    row_grid(
        p,
        cx,
        (ox + 26.0, oy + 85.0, 260.0, 480.0),
        "page_recipes",
        &refs,
        5,
        Some(&pick),
    );
    gold_line(p, cx, (ox + 34.0, oy + 578.0), 84.0, v.gold);
}

/// The recipe card: name, icon + description, ingredients with what the hero holds, and the
/// skill the recipe needs (`IDD_VIEW_RECIPE_2`).
fn recipe_card(p: &mut P, cx: &Cx, r: &RecipeRow) {
    let (x, y) = CARD;
    spawn_pic(p, cx.a, RECIPE, x, y, None);
    ink_heading(p, cx, r.name.clone(), (x + 6.0, y + 28.0, 318.0, 25.0));
    let (gl, full) = item_icon(cx.a, cx.s.items.get(&r.product), false);
    if full {
        spawn_pic(p, cx.a, gl, x + 38.0, y + 60.0, Some((64.0, 68.0)));
    } else {
        spawn_pic(p, cx.a, gl, x + 38.0, y + 60.0 + (68.0 - gl.h) / 2.0, None);
    }
    p.spawn((
        Node {
            overflow: Overflow::clip(),
            ..abs(x + 114.0, y + 60.0, 172.0, 72.0)
        },
        children![txt_wrap(cx.font(), r.description.clone(), 14.0, INK)],
    ));
    body(
        p,
        cx,
        cx.t("HUD_Recipe_Ingredients", "Zutaten"),
        (x + 6.0, y + 295.0, 318.0, 29.0),
        Al::Center,
        INK,
    );
    for (i, ing) in r.ingredients.iter().take(3).enumerate() {
        body(
            p,
            cx,
            format!("{}x {}", ing.need, ing.name),
            (x + 38.0, y + 331.0 + 24.0 * i as f32, 170.0, 22.0),
            Al::Left,
            ingredient_color(ing.have, ing.need),
        );
        body(
            p,
            cx,
            format!("{}/{}", ing.have, ing.need),
            (x + 210.0, y + 331.0 + 24.0 * i as f32, 76.0, 22.0),
            Al::Right,
            ingredient_color(ing.have, ing.need),
        );
    }
    if !r.requirement.is_empty() {
        body(
            p,
            cx,
            cx.t("HUD_Recipe_Requirements", "Erforderliche Kenntnisse"),
            (x + 6.0, y + 408.0, 318.0, 25.0),
            Al::Center,
            INK,
        );
        body(
            p,
            cx,
            r.requirement.clone(),
            (x + 38.0, y + 440.0, 248.0, 36.0),
            Al::Left,
            if r.craftable { INK } else { INK_BAD },
        );
    }
}

// -------------------------------------------------------------------------------- documents
/// Lines per page of a book column (382 px at 20 px per line).
const BOOK_LINES: usize = 19;
const BOOK_FONT: f32 = 15.0;

pub(crate) fn document_page(p: &mut P, cx: &Cx) {
    let Some(PageView::Document(d)) = cx.ui.page.as_ref() else {
        return;
    };
    spawn_pic(p, cx.a, DOCUMENT, PANEL.0, PANEL.1, None);
    if d.is_book {
        book(p, cx, d);
    } else {
        letter(p, cx, d);
    }
    menu_btn(
        p,
        cx,
        &cx.t("HUD_Close", "Schliessen"),
        (257.0, 608.0, 120.0, 32.0),
        Some(Action::Close),
    );
}

fn column(p: &mut P, cx: &Cx, text: String, r: Box4) {
    p.spawn((
        Node {
            overflow: Overflow::clip(),
            ..abs(r.0, r.1, r.2, r.3)
        },
        children![txt_wrap(cx.font(), text, BOOK_FONT, INK)],
    ));
}

fn book(p: &mut P, cx: &Cx, d: &DocumentView) {
    let (ox, oy) = (24.0, 83.0);
    spawn_pic(p, cx.a, BOOK, ox, oy, None);
    ink_heading(p, cx, d.title.clone(), (ox + 36.0, oy + 33.0, 244.0, 44.0));
    let pages = reading::book_pages(&d.left, &d.right, 244.0, BOOK_FONT, BOOK_LINES);
    let spreads = reading::spreads(pages.len());
    let spread = cx.ui.spread.min(spreads - 1);
    let page = |i: usize| pages.get(i).map(|l| l.join("\n")).unwrap_or_default();
    column(
        p,
        cx,
        page(spread * 2),
        (ox + 36.0, oy + 89.0, 244.0, 382.0),
    );
    column(
        p,
        cx,
        page(spread * 2 + 1),
        (ox + 326.0, oy + 33.0, 244.0, 438.0),
    );
    if spreads > 1 {
        menu_btn(
            p,
            cx,
            "<",
            (ox + 36.0, oy + 474.0, 28.0, 24.0),
            (spread > 0).then_some(Action::BookSpread(-1)),
        );
        menu_btn(
            p,
            cx,
            ">",
            (ox + 542.0, oy + 474.0, 28.0, 24.0),
            (spread + 1 < spreads).then_some(Action::BookSpread(1)),
        );
        body(
            p,
            cx,
            format!("{} / {}", spread + 1, spreads),
            (ox + 250.0, oy + 474.0, 100.0, 24.0),
            Al::Center,
            INK,
        );
    }
}

fn letter(p: &mut P, cx: &Cx, d: &DocumentView) {
    let (ox, oy) = (158.0, 81.0);
    spawn_pic(p, cx.a, LETTER, ox, oy, None);
    ink_heading(p, cx, d.title.clone(), (ox + 11.0, oy + 33.0, 312.0, 25.0));
    p.spawn((
        Node {
            overflow: Overflow::scroll_y(),
            ..abs(ox + 20.0, oy + 71.0, 288.0, 414.0)
        },
        scroll_area("letter", cx.scroll_y("letter")),
        children![txt_wrap(
            cx.font(),
            d.left.replace("\\n", "\n"),
            BOOK_FONT,
            INK
        )],
    ));
}
