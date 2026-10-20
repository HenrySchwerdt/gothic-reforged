//! Screens of the original UI, rebuilt from `g3.gui.res`: every view is an atlas panel at the
//! position/size stored in the resource file (1024x768 design space) with the original's
//! text rectangles on top. The whole screen is respawned when `UiState::dirty` is set.
use crate::atlas::*;
use crate::hud::glyph;
use crate::style::*;
use crate::*;
use gothic_gameplay::model::{Cmd, QuestStatus};
use gothic_gameplay::state::{GOLD_ITEM, LogKind};
use gothic_gameplay::transcript::quest_title;

#[derive(Component)]
pub(crate) struct ScreenRoot;

struct Cx<'a> {
    a: &'a UiAssets,
    s: &'a GameSession,
    ui: &'a UiState,
    vitals: PlayerVitals,
}

impl Cx<'_> {
    fn t(&self, key: &str, fallback: &str) -> String {
        self.s.db.text(key).unwrap_or_else(|| fallback.to_string())
    }
    fn scroll_y(&self, name: &'static str) -> f32 {
        self.ui.scroll.get(name).copied().unwrap_or(0.0)
    }
    fn font(&self) -> &Handle<Font> {
        &self.a.font
    }
}

pub(crate) fn rebuild(
    mut commands: Commands,
    mut ui: ResMut<UiState>,
    session: Res<GameSession>,
    assets: Res<UiAssets>,
    vitals: Res<PlayerVitals>,
    roots: Query<Entity, With<ScreenRoot>>,
    scrolls: Query<(&ScrollArea, &ScrollPosition)>,
) {
    if !ui.dirty {
        return;
    }
    ui.dirty = false;
    let mut saved = Vec::new();
    for (a, p) in &scrolls {
        saved.push((a.0, p.y));
    }
    for (k, v) in saved {
        ui.scroll.insert(k, v);
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    let cx = Cx {
        a: &assets,
        s: &session,
        ui: &ui,
        vitals: *vitals,
    };
    if ui.screen == Screen::None {
        return;
    }
    // full-window root; the 1024x768 stage is centred inside it
    commands
        .spawn((
            ScreenRoot,
            GlobalZIndex(10),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|root| {
            root.spawn(Node {
                width: px(1024),
                height: px(768),
                flex_shrink: 0.0,
                ..default()
            })
            .with_children(|p| match ui.screen {
                Screen::None => {}
                Screen::Dialog => dialog(p, &cx),
                Screen::Journal => journal(p, &cx),
                Screen::Inventory => inventory(p, &cx),
                Screen::Character => character(p, &cx),
                Screen::Trade => trade(p, &cx),
            });
        });
}

// ------------------------------------------------------------------------------ pieces
fn heading(p: &mut P, cx: &Cx, s: impl Into<String>, r: (f32, f32, f32, f32)) {
    label(p, &cx.a.heading, s, r, 22.0, TEXT_HI, Al::Center);
}

fn body(p: &mut P, cx: &Cx, s: impl Into<String>, r: (f32, f32, f32, f32), al: Al, c: Color) {
    label(p, cx.font(), s, r, 16.0, c, al);
}

/// "name ....... value" row of the character panels (label x=24, value column 166..290).
fn kv_row(p: &mut P, cx: &Cx, ox: f32, oy: f32, y: f32, k: &str, v: String) {
    body(p, cx, k, (ox + 24.0, oy + y, 202.0, 20.0), Al::Left, TEXT);
    body(p, cx, v, (ox + 166.0, oy + y, 124.0, 20.0), Al::Right, TEXT);
}

fn frame_box(bg: Color) -> impl Bundle {
    (BackgroundColor(bg), BorderColor::all(BORDER))
}

/// Right-hand page strip (exit + five page buttons) at 946,76.
fn sidebar(p: &mut P, cx: &Cx, active: Option<Screen>) {
    let (ox, oy) = (946.0, 76.0);
    spawn_pic(p, cx.a, SIDEBAR, ox, oy, None);
    let pages: [(Pic, Option<Screen>); 5] = [
        (PAGE_INVENTORY, Some(Screen::Inventory)),
        (PAGE_CHARACTER, Some(Screen::Character)),
        (PAGE_DOCUMENTS, None),
        (PAGE_QUESTS, Some(Screen::Journal)),
        (PAGE_QUILL, None),
    ];
    let slot = |p: &mut P, pic: Pic, y: f32, act: Option<Action>, on: bool| {
        let mut n = abs(ox + 8.0, oy + y, 46.0, 46.0);
        n.border = UiRect::all(px(2));
        n.align_items = AlignItems::Center;
        n.justify_content = JustifyContent::Center;
        let bg = if on {
            Color::srgba(0.25, 0.42, 0.42, 0.95)
        } else {
            Color::srgba(0.05, 0.14, 0.15, 0.92)
        };
        let mut e = match act {
            Some(a) => p.spawn((button(a, bg, Color::srgba(0.16, 0.30, 0.30, 0.95)), n)),
            None => p.spawn((BackgroundColor(bg), n)),
        };
        e.insert(BorderColor::all(if on { TEXT } else { BORDER }));
        e.with_children(|b| {
            b.spawn(Node {
                width: px(pic.w.min(38.0)),
                height: px(pic.h.min(38.0)),
                ..default()
            })
            .with_children(|i| {
                spawn_pic(
                    i,
                    cx.a,
                    pic,
                    0.0,
                    0.0,
                    Some((pic.w.min(38.0), pic.h.min(38.0))),
                );
            });
        });
    };
    slot(p, PAGE_EXIT, 48.0, Some(Action::Close), false);
    for (k, (pic, scr)) in pages.into_iter().enumerate() {
        slot(
            p,
            pic,
            144.0 + 63.0 * k as f32,
            scr.map(Action::Open),
            scr.is_some() && scr == active,
        );
    }
}

/// Item grid: 48px cells, five per row, scrollable (26,85 inside the inventory-style views).
#[allow(clippy::too_many_arguments)]
fn item_grid(
    p: &mut P,
    cx: &Cx,
    rect: (f32, f32, f32, f32),
    scroll: &'static str,
    list: &[(String, String, u32)],
    sel: Option<&str>,
    mk: impl Fn(&str) -> Action,
) {
    let (x, y, _w, h) = rect;
    let cols = 5usize;
    p.spawn((
        Node {
            flex_wrap: FlexWrap::Wrap,
            align_content: AlignContent::FlexStart,
            overflow: Overflow::scroll_y(),
            ..abs(x, y, 48.0 * cols as f32, h)
        },
        scroll_area(scroll, cx.scroll_y(scroll)),
    ))
    .with_children(|g| {
        let cells = (list.len().div_ceil(cols) * cols).max(cols * 10);
        for i in 0..cells {
            let mut n = Node {
                width: px(44),
                height: px(44),
                margin: UiRect::all(px(2)),
                border: UiRect::all(px(1)),
                flex_shrink: 0.0,
                ..default()
            };
            match list.get(i) {
                None => {
                    g.spawn((
                        n,
                        BackgroundColor(CELL),
                        BorderColor::all(Color::srgba(0.5, 0.4, 0.28, 0.45)),
                    ));
                }
                Some((_, key, count)) => {
                    let is_sel = sel == Some(key.as_str());
                    n.justify_content = JustifyContent::Center;
                    n.align_items = AlignItems::Center;
                    let cat = cx.s.items.get(key).map_or(6, |i| i.category);
                    let gl = glyph(cat);
                    g.spawn((
                        button(
                            mk(key),
                            if is_sel {
                                Color::srgba(0.25, 0.38, 0.55, 0.9)
                            } else {
                                CELL
                            },
                            CELL_HOVER,
                        ),
                        n,
                        BorderColor::all(if is_sel { TEXT } else { BORDER }),
                    ))
                    .with_children(|c| {
                        c.spawn(Node {
                            width: px(gl.w),
                            height: px(gl.h),
                            ..default()
                        })
                        .with_children(|i| {
                            spawn_pic(i, cx.a, gl, 0.0, 0.0, None);
                        });
                        if *count > 1 {
                            label(
                                c,
                                cx.font(),
                                count.to_string(),
                                (10.0, 29.0, 30.0, 12.0),
                                11.0,
                                TEXT,
                                Al::Right,
                            );
                        }
                    });
                }
            }
        }
    });
    // scroll bar track (decoration; the wheel scrolls)
    let mut t = abs(x + 48.0 * cols as f32 + 4.0, y, 14.0, h);
    t.border = UiRect::all(px(1));
    p.spawn((t, frame_box(Color::srgba(0.08, 0.06, 0.05, 0.9))));
}

fn item_list(
    cx: &Cx,
    inv: &std::collections::BTreeMap<String, u32>,
    cat: Option<i32>,
) -> Vec<(String, String, u32)> {
    let mut v: Vec<_> = inv
        .iter()
        .filter(|(k, n)| k.as_str() != GOLD_ITEM && **n > 0)
        .filter(|(k, _)| cat.is_none_or(|c| item_category(cx.s, k) == c))
        .map(|(k, n)| (item_name(&cx.s.db, k), k.clone(), *n))
        .collect();
    v.sort();
    v
}

fn item_category(s: &GameSession, name: &str) -> i32 {
    match s.items.get(name).map(|i| i.category) {
        Some(0) | None => 6,
        Some(c) => c,
    }
}

/// Character view (left): progress, attributes, protection - at 14,16.
fn character_view(p: &mut P, cx: &Cx) {
    let (ox, oy) = (14.0, 16.0);
    let st = &cx.s.state;
    let db = &cx.s.db;
    spawn_pic(p, cx.a, CHARACTER, ox, oy, None);
    let next = GameState::xp_next(st.level);
    heading(p, cx, "Erfahrung", (ox + 24.0, oy + 8.0, 260.0, 33.0));
    kv_row(p, cx, ox, oy, 44.0, "Stufe", st.level.to_string());
    kv_row(p, cx, ox, oy, 68.0, "Erfahrung", st.xp.to_string());
    kv_row(p, cx, ox, oy, 92.0, "Nächste Stufe", next.to_string());
    kv_row(
        p,
        cx,
        ox,
        oy,
        116.0,
        "Lernpunkte",
        st.learning_points.to_string(),
    );
    heading(p, cx, "Attribute", (ox + 24.0, oy + 164.0, 260.0, 25.0));
    let attr = |tag: &str| {
        st.attributes
            .get(&tag.to_ascii_lowercase())
            .copied()
            .unwrap_or(0)
            .to_string()
    };
    let mut y = 196.0;
    for tag in ["STR", "HNT", "INT", "SMT", "THF", "ALC"] {
        let name = if tag == "HNT" {
            "Jagdgeschick".to_string()
        } else {
            attrib_name(db, tag)
        };
        kv_row(p, cx, ox, oy, y, &name, attr(tag));
        y += 23.0;
    }
    let v = cx.vitals;
    for (name, cur, max) in [
        ("Lebensenergie", v.hp, v.hp_max),
        ("Ausdauer", v.sp, v.sp_max),
        ("Mana", v.mp, v.mp_max),
    ] {
        kv_row(p, cx, ox, oy, y, name, format!("{cur:.0} / {max:.0}"));
        y += 23.0;
    }
    heading(
        p,
        cx,
        "Rüstungsschutz",
        (ox + 24.0, oy + 428.0, 260.0, 25.0),
    );
    for (i, (name, tag)) in [
        ("Schutz vor Klingen", "prot_blade"),
        ("Schutz vor Zerschmettern", "prot_impact"),
        ("Schutz vor Geschossen", "prot_missile"),
        ("Schutz vor Feuer", "prot_fire"),
        ("Schutz vor Eis", "prot_ice"),
        ("Schutz vor Energie", "prot_lightning"),
    ]
    .into_iter()
    .enumerate()
    {
        let y = [464.0, 488.0, 512.0, 532.0, 556.0, 580.0][i];
        kv_row(
            p,
            cx,
            ox,
            oy,
            y,
            name,
            st.attributes.get(tag).copied().unwrap_or(0).to_string(),
        );
    }
}

// --------------------------------------------------------------------------- inventory
const INV_TABS: [(&str, Option<i32>, Pic); 5] = [
    ("Waffen", Some(1), GLYPH_WEAPON),
    ("Rüstung", Some(2), GLYPH_ARMOR),
    ("Schriften", Some(7), GLYPH_SCROLL),
    ("Alchemie", Some(4), GLYPH_POTION),
    ("Sonstiges", None, GLYPH_MISC),
];

fn in_tab(cx: &Cx, name: &str, tab: usize) -> bool {
    let c = item_category(cx.s, name);
    match INV_TABS[tab].1 {
        Some(k) => c == k,
        None => !matches!(c, 1 | 2 | 4 | 7),
    }
}

fn tab_strip(p: &mut P, cx: &Cx, rect: (f32, f32), selected: usize) {
    for (k, (_, _, g)) in INV_TABS.iter().enumerate() {
        let mut n = abs(rect.0 + 50.0 * k as f32, rect.1, 46.0, 26.0);
        n.border = UiRect::all(px(2));
        n.align_items = AlignItems::Center;
        n.justify_content = JustifyContent::Center;
        let on = k == selected;
        p.spawn((
            button(
                Action::InvTab(k),
                if on {
                    Color::srgba(0.12, 0.30, 0.18, 0.95)
                } else {
                    Color::srgba(0.02, 0.10, 0.06, 0.92)
                },
                Color::srgba(0.12, 0.26, 0.18, 0.95),
            ),
            n,
            BorderColor::all(if on { TEXT } else { BORDER }),
        ))
        .with_children(|b| {
            b.spawn(Node {
                width: px(g.w.min(26.0)),
                height: px(g.h.min(18.0)),
                ..default()
            })
            .with_children(|i| {
                spawn_pic(i, cx.a, *g, 0.0, 0.0, Some((g.w.min(26.0), g.h.min(18.0))));
            });
        });
    }
}

fn item_view(p: &mut P, cx: &Cx, key: &str, actions: Vec<(String, Action)>) {
    let (ox, oy) = (336.0, 16.0);
    let db = &cx.s.db;
    let info = cx.s.items.get(key);
    spawn_pic(p, cx.a, ITEM, ox, oy, None);
    heading(
        p,
        cx,
        item_name(db, key),
        (ox + 12.0, oy + 8.0, 264.0, 28.0),
    );
    if let Some(d) = db.text(&format!("ITEMDESC_{key}")).filter(|d| d != "-") {
        p.spawn((
            Node {
                overflow: Overflow::clip(),
                ..abs(ox + 90.0, oy + 40.0, 180.0, 66.0)
            },
            children![txt_wrap(cx.font(), d, 14.0, TEXT)],
        ));
    }
    let Some(info) = info else {
        return;
    };
    let gl = glyph(info.category);
    spawn_pic(
        p,
        cx.a,
        gl,
        ox + 16.0 + (52.0 - gl.w) / 2.0,
        oy + 40.0 + (52.0 - gl.h) / 2.0,
        None,
    );
    let mut y = 112.0;
    let mut row = |p: &mut P, k: String, v: String, c: Color| {
        body(p, cx, k, (ox + 18.0, oy + y, 170.0, 20.0), Al::Left, TEXT);
        body(p, cx, v, (ox + 160.0, oy + y, 108.0, 20.0), Al::Right, c);
        y += 22.0;
    };
    if let Some((amount, ty)) = &info.damage {
        let ty = match ty.as_str() {
            "Blade" => "Klinge",
            "Impact" => "Zerschmettern",
            "Missile" => "Geschoss",
            "Fire" => "Feuer",
            "Ice" => "Eis",
            "Lightning" => "Energie",
            other => other,
        };
        row(p, "Schaden".into(), format!("{amount} ({ty})"), TEXT);
    }
    for (tag, v) in info.modifiers.iter().take(4) {
        row(p, attrib_name(db, tag), format!("{v:+}"), TEXT);
    }
    for (tag, need) in info.requirements.iter().take(3) {
        let have =
            cx.s.state
                .attributes
                .get(&tag.to_ascii_lowercase())
                .copied()
                .unwrap_or(0);
        row(
            p,
            format!("Benötigt {}", attrib_name(db, tag)),
            need.to_string(),
            if have >= *need { TEXT } else { TEXT_BAD },
        );
    }
    row(p, "Wert".into(), info.value.to_string(), TEXT);
    for (i, (name, a)) in actions.into_iter().enumerate() {
        let mut n = abs(ox + 70.0 + 0.0 * i as f32, oy + 308.0, 148.0, 30.0);
        n.border = UiRect::all(px(2));
        n.align_items = AlignItems::Center;
        n.justify_content = JustifyContent::Center;
        p.spawn((
            button(
                a,
                Color::srgba(0.12, 0.08, 0.05, 0.95),
                Color::srgba(0.30, 0.20, 0.10, 0.95),
            ),
            n,
            BorderColor::all(BORDER),
        ))
        .with_child(txt(cx.font(), name, 16.0, TEXT));
    }
}

fn txt_wrap(font: &Handle<Font>, s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(s),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

fn inventory(p: &mut P, cx: &Cx) {
    let st = &cx.s.state;
    let tab = cx.ui.inv_tab.min(INV_TABS.len() - 1);
    let (ox, oy) = (634.0, 16.0);
    character_view(p, cx);
    let list: Vec<_> = item_list(cx, &st.inventory, None)
        .into_iter()
        .filter(|(_, k, _)| in_tab(cx, k, tab))
        .collect();
    let sel = cx
        .ui
        .inv_sel
        .clone()
        .filter(|s| list.iter().any(|(_, k, _)| k == s));
    spawn_pic(p, cx.a, INVENTORY, ox, oy, None);
    heading(p, cx, INV_TABS[tab].0, (ox + 26.0, oy + 12.0, 258.0, 25.0));
    tab_strip(p, cx, (ox + 30.0, oy + 46.0), tab);
    item_grid(
        p,
        cx,
        (ox + 26.0, oy + 85.0, 258.0, 482.0),
        "items",
        &list,
        sel.as_deref(),
        |k| Action::SelectItem(k.to_string()),
    );
    body(
        p,
        cx,
        cx.t("HUD_GoldAmount", "Gold"),
        (ox + 32.0, oy + 580.0, 100.0, 20.0),
        Al::Left,
        TEXT,
    );
    body(
        p,
        cx,
        st.gold().to_string(),
        (ox + 98.0, oy + 580.0, 180.0, 20.0),
        Al::Right,
        TEXT,
    );
    if let Some(k) = &sel {
        let equip =
            cx.s.items
                .get(k)
                .is_some_and(|i| matches!(i.category, 1 | 2));
        let label = if equip { "Anlegen" } else { "Benutzen" };
        item_view(p, cx, k, vec![(label.into(), Action::UseItem(k.clone()))]);
    }
    sidebar(p, cx, Some(Screen::Inventory));
}

// ---------------------------------------------------------------------------- character
fn skill_group(key: &str) -> usize {
    let k = key.to_ascii_lowercase();
    let has = |w: &[&str]| w.iter().any(|x| k.contains(x));
    if has(&[
        "sword", "1h", "2h", "bow", "crossbow", "combat", "weapon", "shield", "fight",
    ]) {
        0 // Kampf
    } else if has(&[
        "hunt", "sneak", "sprint", "animal", "pelt", "claw", "teeth", "trophy", "skin",
    ]) {
        1 // Jagd
    } else if has(&["magic", "spell", "rune", "mana", "scroll", "summon"]) {
        2 // Magie
    } else if has(&["smith", "forge", "blade", "armor"]) {
        3 // Schmieden
    } else if has(&["thief", "lock", "pick", "steal", "pocket", "theft"]) {
        4 // Diebstahl
    } else if has(&["alch", "potion", "herb", "brew"]) {
        5 // Alchemie
    } else {
        6 // Sonstige
    }
}

fn character(p: &mut P, cx: &Cx) {
    let st = &cx.s.state;
    let db = &cx.s.db;
    character_view(p, cx);
    let (ox, oy) = (325.0, 16.0);
    spawn_pic(p, cx.a, SKILLS, ox, oy, None);
    // (heading, heading rect, list rect) from the resource file
    let groups = [
        ("Kampf", (32.0, 8.0), (12.0, 40.0, 288.0, 144.0)),
        ("Jagd", (32.0, 200.0), (12.0, 232.0, 288.0, 96.0)),
        ("Magie", (32.0, 348.0), (12.0, 380.0, 288.0, 96.0)),
        ("Schmieden", (32.0, 488.0), (12.0, 532.0, 288.0, 48.0)),
        ("Diebstahl", (328.0, 12.0), (320.0, 44.0, 288.0, 96.0)),
        ("Alchemie", (328.0, 156.0), (320.0, 184.0, 288.0, 96.0)),
        ("Sonstige", (328.0, 300.0), (320.0, 344.0, 288.0, 48.0)),
    ];
    let mut buckets: Vec<Vec<String>> = vec![Vec::new(); groups.len()];
    for k in &st.skills {
        let name = db
            .text(&format!("FO_it_{k}"))
            .or_else(|| db.text(&format!("FO_{k}")))
            .unwrap_or_else(|| k.clone());
        buckets[skill_group(k)].push(name);
    }
    for (i, (name, h, r)) in groups.into_iter().enumerate() {
        heading(p, cx, name, (ox + h.0, oy + h.1, 260.0, 25.0));
        for (j, s) in buckets[i].iter().take((r.3 / 20.0) as usize).enumerate() {
            body(
                p,
                cx,
                s.clone(),
                (ox + r.0 + 8.0, oy + r.1 + 20.0 * j as f32, r.2 - 16.0, 20.0),
                Al::Left,
                TEXT,
            );
        }
    }
    if st.skills.is_empty() {
        body(
            p,
            cx,
            "Noch keine Fertigkeiten erlernt.",
            (ox + 320.0, oy + 444.0, 288.0, 40.0),
            Al::Center,
            TEXT_DIM,
        );
    }
    sidebar(p, cx, Some(Screen::Character));
}

// ------------------------------------------------------------------------------ journal
fn quest_tab_of(s: QuestStatus) -> Option<usize> {
    match s {
        QuestStatus::Running => Some(0),
        QuestStatus::Success | QuestStatus::Won => Some(1),
        QuestStatus::Failed
        | QuestStatus::Lost
        | QuestStatus::Obsolete
        | QuestStatus::Cancelled => Some(2),
        QuestStatus::Open => None,
    }
}

fn dialog_excerpt(db: &gothic_gameplay::GameDatabase, info: &str) -> Vec<String> {
    let Some(id) = db.info_id(info) else {
        return Vec::new();
    };
    let info = db.info(id);
    info.script
        .iter()
        .filter(|s| Cmd::parse(&s.command) == Some(Cmd::Say))
        .filter_map(|s| {
            let t = db.text(&s.text)?;
            let player = s.entity1.eq_ignore_ascii_case("player")
                || s.entity1.eq_ignore_ascii_case("pc_hero");
            Some(format!("{}: {t}", if player { "Du" } else { "Gegenüber" }))
        })
        .collect()
}

fn journal(p: &mut P, cx: &Cx) {
    let db = &cx.s.db;
    let st = &cx.s.state;
    let (ox, oy) = (14.0, 16.0);
    spawn_pic(p, cx.a, QUESTS, ox, oy, None);
    heading(p, cx, "Alle Missionen", (ox + 32.0, oy + 8.0, 872.0, 30.0));
    // world map (cropped to the 442x392 view)
    let mut map = WORLD_MAP;
    map.h = 514.0;
    spawn_pic(p, cx.a, map, ox + 40.0, oy + 48.0, Some((438.0, 388.0)));
    // quest tabs
    let names = [
        cx.t("HUD_Quest_Running", "Aktuell"),
        cx.t("HUD_Quest_Success", "Erfolgreich"),
        cx.t("HUD_Quest_Failed", "Gescheitert"),
    ];
    for (i, n) in names.iter().enumerate() {
        let on = cx.ui.quest_tab == i;
        let mut nd = abs(ox + 503.0 + 140.0 * i as f32, oy + 44.0, 136.0, 40.0);
        nd.border = UiRect::all(px(2));
        nd.align_items = AlignItems::Center;
        nd.justify_content = JustifyContent::Center;
        p.spawn((
            button(
                Action::QuestTab(i),
                if on {
                    Color::srgba(0.05, 0.25, 0.12, 0.95)
                } else {
                    Color::srgba(0.0, 0.07, 0.04, 0.95)
                },
                Color::srgba(0.07, 0.22, 0.12, 0.95),
            ),
            nd,
            BorderColor::all(BORDER),
        ))
        .with_child(txt(
            cx.font(),
            n.clone(),
            16.0,
            if on { TEXT_HI } else { TEXT },
        ));
    }
    let mut list: Vec<(String, String)> = st
        .quests
        .iter()
        .filter(|(_, q)| q.status.and_then(quest_tab_of) == Some(cx.ui.quest_tab))
        .map(|(n, _)| (quest_title(db, n), n.clone()))
        .collect();
    list.sort();
    let selected = cx
        .ui
        .quest_sel
        .clone()
        .filter(|s| list.iter().any(|(_, n)| n == s));
    p.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            overflow: Overflow::scroll_y(),
            ..abs(ox + 508.0, oy + 98.0, 392.0, 176.0)
        },
        scroll_area("quests", cx.scroll_y("quests")),
    ))
    .with_children(|l| {
        for (title, name) in &list {
            let sel = selected.as_deref() == Some(name.as_str());
            l.spawn((
                button(
                    Action::SelectQuest(name.clone()),
                    if sel { SEL } else { Color::NONE },
                    SEL,
                ),
                Node {
                    flex_shrink: 0.0,
                    padding: UiRect::axes(px(4), px(1)),
                    column_gap: px(4),
                    align_items: AlignItems::Center,
                    ..default()
                },
            ))
            .with_children(|r| {
                r.spawn(Node {
                    width: px(18),
                    height: px(18),
                    ..default()
                })
                .with_children(|i| {
                    spawn_pic(i, cx.a, MARK_QUEST, 0.0, 0.0, None);
                });
                r.spawn(txt(
                    cx.font(),
                    title.clone(),
                    15.0,
                    if sel { TEXT_HI } else { TEXT },
                ));
            });
        }
    });
    // detail
    let detail = (ox + 508.0, oy + 300.0, 410.0, 302.0);
    match &selected {
        None => body(
            p,
            cx,
            cx.t("HUD_NoQuestSelected", "Keine Mission ausgewählt"),
            detail,
            Al::Center,
            TEXT,
        ),
        Some(name) => {
            p.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6),
                    overflow: Overflow::scroll_y(),
                    padding: UiRect::axes(px(8), px(4)),
                    ..abs(detail.0, detail.1, 392.0, detail.3)
                },
                scroll_area("quest_detail", cx.scroll_y("quest_detail")),
            ))
            .with_children(|d| {
                d.spawn(txt(cx.font(), quest_title(db, name), 18.0, TEXT_HI));
                for e in st.log.iter().filter(|e| &e.quest == name) {
                    match &e.kind {
                        LogKind::Status(s) => {
                            let t = match s {
                                QuestStatus::Running => "Aufgabe begonnen",
                                QuestStatus::Success | QuestStatus::Won => "Aufgabe erfüllt",
                                QuestStatus::Failed => "Aufgabe gescheitert",
                                _ => continue,
                            };
                            d.spawn(txt(cx.font(), format!("- {t} -"), 14.0, TEXT_DIM));
                        }
                        LogKind::Text(key) => {
                            if let Some(t) = db.text(key) {
                                d.spawn(txt_wrap(cx.font(), t, 15.0, TEXT));
                            }
                        }
                        LogKind::Dialog(info) => {
                            for l in dialog_excerpt(db, info) {
                                d.spawn(txt_wrap(cx.font(), l, 14.0, TEXT_DIM));
                            }
                        }
                    }
                }
            });
        }
    }
    // reputation
    let mut fame: Vec<(String, i32)> = st
        .fame
        .iter()
        .map(|(k, v)| (fame_name(db, k), *v))
        .collect();
    if fame.is_empty() {
        fame = [
            "Ruf in Nordmar",
            "Ruf bei den Orks",
            "Ruf bei den Rebellen",
            "Ruf beim Waldvolk",
            "Ruf bei den Assassinen",
            "Ruf bei den Nomaden",
        ]
        .into_iter()
        .map(|s| (s.to_string(), 0))
        .collect();
    }
    for (i, (n, v)) in fame.iter().take(6).enumerate() {
        let y = 464.0 + 22.0 * i as f32;
        body(
            p,
            cx,
            n.clone(),
            (ox + 42.0, oy + y, 200.0, 25.0),
            Al::Left,
            TEXT,
        );
        body(
            p,
            cx,
            v.to_string(),
            (ox + 206.0, oy + y, 100.0, 25.0),
            Al::Right,
            TEXT,
        );
    }
    // clock
    spawn_pic(p, cx.a, TIME_SKY, ox + 330.0, oy + 460.0, None);
    spawn_pic(p, cx.a, TIME_LABEL, ox + 330.0, oy + 493.0, None);
    let h = st.hours;
    body(
        p,
        cx,
        format!("{}. Tag, {:02}:00", h / 24 % 365 + 1, h % 24),
        (ox + 330.0, oy + 493.0, 158.0, 34.0),
        Al::Center,
        TEXT,
    );
    sidebar(p, cx, Some(Screen::Journal));
}

fn fame_name(db: &gothic_gameplay::GameDatabase, key: &str) -> String {
    match key.split_once(':') {
        Some(("pol", tag)) => db.text(&format!("HUD_{tag}")),
        Some(("enc", name)) => db.text(&format!("GO_Enclave_{name}")),
        _ => None,
    }
    .unwrap_or_else(|| key.to_string())
}

// -------------------------------------------------------------------------------- dialog
fn dialog(p: &mut P, cx: &Cx) {
    let Some(view) = cx.ui.dialog.as_ref() else {
        return;
    };
    // subtitle: the latest spoken line (the original shows no name header)
    if let Some(l) = view.lines.last() {
        spawn_pic(p, cx.a, TALK, 227.0, 100.0, None);
        p.spawn((
            Node {
                overflow: Overflow::clip(),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..abs(227.0 + 19.0, 100.0 + 9.0, 532.0, 62.0)
            },
            children![(
                txt_wrap(
                    cx.font(),
                    l.text.clone(),
                    15.0,
                    if l.player { TEXT_HI } else { TEXT }
                ),
                TextLayout::justify(Justify::Center),
            )],
        ));
    }
    spawn_pic(p, cx.a, DIALOG, 22.0, 534.0, None);
    p.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            overflow: Overflow::scroll_y(),
            ..abs(22.0 + 60.0, 534.0 + 22.0, 858.0, 169.0)
        },
        scroll_area("dialog", cx.scroll_y("dialog")),
    ))
    .with_children(|m| {
        if view.menu.is_empty() {
            m.spawn((
                button(Action::Close, Color::NONE, SEL),
                Node {
                    padding: UiRect::axes(px(14), px(2)),
                    flex_shrink: 0.0,
                    ..default()
                },
                children![txt(cx.font(), "Weiter", 16.0, TEXT)],
            ));
        }
        for (i, c) in view.menu.iter().enumerate() {
            m.spawn((
                button(Action::Choose(i), Color::NONE, SEL),
                Node {
                    padding: UiRect::axes(px(14), px(2)),
                    flex_shrink: 0.0,
                    ..default()
                },
                children![txt(cx.font(), c.label.replace('\n', " "), 16.0, TEXT)],
            ));
        }
    });
}

// ---------------------------------------------------------------------------------- trade
/// Layout only: trade rules (price factors, stock refresh) are not decoded yet.
fn trade(p: &mut P, cx: &Cx) {
    let Some((npc, stock)) = cx.ui.trade.as_ref() else {
        return;
    };
    let st = &cx.s.state;
    let mine = item_list(cx, &st.inventory, None);
    let mut theirs: Vec<(String, String, u32)> = stock
        .iter()
        .filter(|(k, n)| k != GOLD_ITEM && *n > 0)
        .map(|(k, n)| (item_name(&cx.s.db, k), k.clone(), *n))
        .collect();
    theirs.sort();
    let sel = cx.ui.inv_sel.clone();
    // vendor (left)
    spawn_pic(p, cx.a, TRADE_VENDOR, 14.0, 16.0, None);
    heading(
        p,
        cx,
        npc.replace('_', " "),
        (14.0 + 26.0, 16.0 + 8.0, 252.0, 25.0),
    );
    item_grid(
        p,
        cx,
        (14.0 + 22.0, 16.0 + 44.0, 260.0, 384.0),
        "trade_theirs",
        &theirs,
        sel.as_deref(),
        |k| Action::Trade(k.to_string(), true),
    );
    // player (right)
    spawn_pic(p, cx.a, TRADE_PLAYER, 691.0, 16.0, None);
    heading(
        p,
        cx,
        "Dein Inventar",
        (691.0 + 25.0, 16.0 + 8.0, 252.0, 25.0),
    );
    tab_strip(p, cx, (691.0 + 25.0, 16.0 + 48.0), cx.ui.inv_tab.min(4));
    item_grid(
        p,
        cx,
        (691.0 + 22.0, 16.0 + 92.0, 260.0, 336.0),
        "trade_mine",
        &mine,
        sel.as_deref(),
        |k| Action::Trade(k.to_string(), false),
    );
    body(
        p,
        cx,
        "Gold",
        (691.0 + 33.0, 16.0 + 444.0, 100.0, 25.0),
        Al::Left,
        TEXT,
    );
    body(
        p,
        cx,
        st.gold().to_string(),
        (691.0 + 117.0, 16.0 + 444.0, 152.0, 25.0),
        Al::Right,
        TEXT,
    );
    // transaction strip
    spawn_pic(p, cx.a, TRANSACTION, 14.0, 524.0, None);
    body(
        p,
        cx,
        "Klick auf einen Gegenstand: kaufen / verkaufen",
        (14.0 + 200.0, 524.0 + 40.0, 580.0, 24.0),
        Al::Center,
        TEXT_DIM,
    );
    let mut n = abs(14.0 + 414.0, 524.0 + 128.0, 152.0, 34.0);
    n.border = UiRect::all(px(2));
    n.align_items = AlignItems::Center;
    n.justify_content = JustifyContent::Center;
    p.spawn((
        button(
            Action::Close,
            Color::srgba(0.12, 0.08, 0.05, 0.95),
            Color::srgba(0.30, 0.20, 0.10, 0.95),
        ),
        n,
        BorderColor::all(BORDER),
        children![txt(cx.font(), "Schliessen", 16.0, TEXT)],
    ));
}
