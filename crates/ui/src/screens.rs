//! Screen builders: the whole screen is respawned when `UiState::dirty` is set.
use crate::style::*;
use crate::*;
use gothic_gameplay::model::{Cmd, QuestStatus};
use gothic_gameplay::state::{GOLD_ITEM, LogKind};
use gothic_gameplay::transcript::quest_title;

type P<'a> = ChildSpawnerCommands<'a>;

#[derive(Component)]
pub(crate) struct ScreenRoot;

struct Cx<'a> {
    font: &'a Handle<Font>,
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
        font: &assets.font,
        s: &session,
        ui: &ui,
        vitals: *vitals,
    };
    match ui.screen {
        Screen::None => {}
        Screen::Dialog => dialog(&mut commands, &cx),
        Screen::Journal => journal(&mut commands, &cx),
        Screen::Inventory => inventory(&mut commands, &cx),
        Screen::Character => character(&mut commands, &cx),
        Screen::Trade => trade(&mut commands, &cx),
    }
}

fn node_col() -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        ..default()
    }
}

fn separator(p: &mut P) {
    p.spawn((
        Node {
            width: percent(100),
            height: px(2),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(GOLD_DIM),
    ));
}

/// Centered overlay window with title, optional tab buttons and a close button.
fn window(
    commands: &mut Commands,
    cx: &Cx,
    title: &str,
    tabs: Vec<(String, bool, Action)>,
    body: impl FnOnce(&mut P),
) {
    let font = cx.font;
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
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.62)),
        ))
        .with_children(|r| {
            r.spawn((
                Node {
                    width: px(1100),
                    height: px(640),
                    flex_direction: FlexDirection::Column,
                    border: UiRect::all(px(2)),
                    padding: UiRect::all(px(16)),
                    row_gap: px(10),
                    ..default()
                },
                BackgroundColor(BG),
                BorderColor::all(GOLD_DIM),
            ))
            .with_children(|p| {
                p.spawn(Node {
                    width: percent(100),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    flex_shrink: 0.0,
                    ..default()
                })
                .with_children(|h| {
                    h.spawn(txt(font, title, 32.0, GOLD));
                    h.spawn(Node {
                        column_gap: px(8),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|t| {
                        for (label, sel, act) in tabs {
                            t.spawn((
                                button(act, if sel { BG_SEL } else { BG_ROW }, BG_HOVER),
                                Node {
                                    padding: UiRect::axes(px(14), px(6)),
                                    border: UiRect::all(px(1)),
                                    ..default()
                                },
                                BorderColor::all(if sel { GOLD } else { GOLD_DIM }),
                            ))
                            .with_child(txt(font, label, 18.0, if sel { GOLD } else { PARCH }));
                        }
                        t.spawn((
                            button(Action::Close, BG_ROW, BG_HOVER),
                            Node {
                                width: px(34),
                                height: px(34),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(px(1)),
                                margin: UiRect::left(px(10)),
                                ..default()
                            },
                            BorderColor::all(GOLD_DIM),
                        ))
                        .with_child(txt(font, "X", 18.0, PARCH));
                    });
                });
                separator(p);
                p.spawn(Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    width: percent(100),
                    column_gap: px(18),
                    ..default()
                })
                .with_children(body);
            });
        });
}

fn bar(p: &mut P, w: Val, h: f32, frac: f32, color: Color) {
    p.spawn((
        Node {
            width: w,
            height: px(h),
            border: UiRect::all(px(1)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
        BorderColor::all(GOLD_DIM),
    ))
    .with_children(|b| {
        b.spawn((
            Node {
                width: percent(frac.clamp(0.0, 1.0) * 100.0),
                height: percent(100),
                ..default()
            },
            BackgroundColor(color),
        ));
    });
}

fn kv(p: &mut P, cx: &Cx, k: &str, v: String, vc: Color) {
    p.spawn(Node {
        width: percent(100),
        justify_content: JustifyContent::SpaceBetween,
        ..default()
    })
    .with_children(|r| {
        r.spawn(txt(cx.font, k, 19.0, DIM));
        r.spawn(txt(cx.font, v, 19.0, vc));
    });
}

// ---------------------------------------------------------------------------- dialog
fn dialog(commands: &mut Commands, cx: &Cx) {
    let Some(view) = cx.ui.dialog.as_ref() else {
        return;
    };
    let font = cx.font;
    let npc = view.session.npc.replace('_', " ");
    commands
        .spawn((
            ScreenRoot,
            GlobalZIndex(10),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            },
        ))
        .with_children(|r| {
            // top letterbox bar with the partner's name
            r.spawn((
                Node {
                    width: percent(100),
                    height: px(76),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(BLACK_BAR),
            ))
            .with_child(txt(font, npc, 34.0, GOLD));
            // bottom bar: subtitles + choices
            r.spawn((
                Node {
                    width: percent(100),
                    height: px(330),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(px(120), px(14)),
                    row_gap: px(8),
                    ..default()
                },
                BackgroundColor(BLACK_BAR),
            ))
            .with_children(|b| {
                // subtitles: the most recent lines
                b.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    height: px(126),
                    flex_shrink: 0.0,
                    overflow: Overflow::clip(),
                    justify_content: JustifyContent::FlexEnd,
                    ..default()
                })
                .with_children(|s| {
                    let n = view.lines.len();
                    for l in &view.lines[n.saturating_sub(4)..] {
                        let color = if l.player { BLUE } else { PARCH };
                        let who = if l.speaker.is_empty() {
                            String::new()
                        } else {
                            format!("{}: ", l.speaker)
                        };
                        s.spawn((
                            Text::new(who),
                            TextFont {
                                font: font.clone().into(),
                                font_size: FontSize::Px(22.0),
                                ..default()
                            },
                            TextColor(if l.player { BLUE } else { GOLD }),
                        ))
                        .with_child(span(font, l.text.clone(), 22.0, color));
                    }
                });
                separator(b);
                if view.menu.is_empty() {
                    b.spawn((
                        button(Action::Close, BG_ROW, BG_HOVER),
                        Node {
                            padding: UiRect::axes(px(16), px(8)),
                            align_self: AlignSelf::Center,
                            border: UiRect::all(px(1)),
                            ..default()
                        },
                        BorderColor::all(GOLD_DIM),
                    ))
                    .with_child(txt(font, "Weiter  [Leertaste]", 20.0, GOLD));
                } else {
                    b.spawn((
                        Node {
                            flex_direction: FlexDirection::Column,
                            flex_grow: 1.0,
                            min_height: px(0),
                            row_gap: px(3),
                            overflow: Overflow::scroll_y(),
                            ..default()
                        },
                        scroll_area("dialog", cx.scroll_y("dialog")),
                    ))
                    .with_children(|m| {
                        for (i, c) in view.menu.iter().enumerate() {
                            let key = if i < 9 {
                                format!("{}.", i + 1)
                            } else if i == 9 {
                                "0.".to_string()
                            } else {
                                "-".to_string()
                            };
                            m.spawn((
                                button(Action::Choose(i), Color::NONE, BG_HOVER),
                                Node {
                                    padding: UiRect::axes(px(10), px(3)),
                                    column_gap: px(10),
                                    flex_shrink: 0.0,
                                    ..default()
                                },
                            ))
                            .with_children(|row| {
                                row.spawn((
                                    Node {
                                        width: px(34),
                                        ..default()
                                    },
                                    children![txt(font, key, 21.0, GOLD)],
                                ));
                                row.spawn(txt(font, c.label.replace('\n', " "), 21.0, PARCH));
                            });
                        }
                    });
                }
            });
        });
}

// ---------------------------------------------------------------------------- journal
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
            Some(format!(
                "{}: {t}",
                if player { "Du" } else { "Gegenüber" }
            ))
        })
        .collect()
}

fn journal(commands: &mut Commands, cx: &Cx) {
    let db = &cx.s.db;
    let st = &cx.s.state;
    let tab_names = [
        cx.t("HUD_Quest_Running", "Aktuell"),
        cx.t("HUD_Quest_Success", "Erfolgreich"),
        cx.t("HUD_Quest_Failed", "Gescheitert"),
    ];
    let tabs = tab_names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), cx.ui.quest_tab == i, Action::QuestTab(i)))
        .collect();
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
        .filter(|s| list.iter().any(|(_, n)| n == s))
        .or_else(|| list.first().map(|(_, n)| n.clone()));
    let font = cx.font;
    window(
        commands,
        cx,
        &cx.t("HUD_Quests", "Missionen"),
        tabs,
        |p| {
            p.spawn((
                Node {
                    width: px(360),
                    height: percent(100),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(3),
                    overflow: Overflow::scroll_y(),
                    flex_shrink: 0.0,
                    ..default()
                },
                scroll_area("quests", cx.scroll_y("quests")),
            ))
            .with_children(|l| {
                if list.is_empty() {
                    l.spawn(txt(font, cx.t("HUD_NoQuests", "Keine Missionen vorhanden"), 19.0, DIM));
                }
                for (title, name) in &list {
                    let sel = selected.as_deref() == Some(name.as_str());
                    l.spawn((
                        button(
                            Action::SelectQuest(name.clone()),
                            if sel { BG_SEL } else { BG_ROW },
                            BG_HOVER,
                        ),
                        Node {
                            padding: UiRect::axes(px(10), px(6)),
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ))
                    .with_child(txt(font, title.clone(), 19.0, if sel { GOLD } else { PARCH }));
                }
            });
            p.spawn((
                Node {
                    height: percent(100),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(8),
                    padding: UiRect::all(px(8)),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                scroll_area("quest_detail", cx.scroll_y("quest_detail")),
            ))
            .with_children(|d| {
                let Some(name) = &selected else {
                    d.spawn(txt(font, cx.t("HUD_NoQuestSelected", "Keine Mission ausgewählt"), 19.0, DIM));
                    return;
                };
                d.spawn(txt(font, quest_title(db, name), 28.0, GOLD));
                if let Some(q) = db.quest_by_name(name) {
                    let mut meta = Vec::new();
                    if !q.folder.is_empty() {
                        meta.push(q.folder.trim_end_matches('/').replace('/', " / "));
                    }
                    if q.experience_points > 0 {
                        meta.push(format!("Belohnung: {} EP", q.experience_points));
                    }
                    if !meta.is_empty() {
                        d.spawn(txt(font, meta.join("   -   "), 16.0, DIM));
                    }
                }
                separator(d);
                let mut any = false;
                for e in st.log.iter().filter(|e| &e.quest == name) {
                    match &e.kind {
                        LogKind::Status(s) => {
                            let t = match s {
                                QuestStatus::Running => "Aufgabe begonnen",
                                QuestStatus::Success | QuestStatus::Won => "Aufgabe erfüllt",
                                QuestStatus::Failed => "Aufgabe gescheitert",
                                _ => continue,
                            };
                            d.spawn(txt(font, format!("- {t} -"), 17.0, GOLD_DIM));
                            any = true;
                        }
                        LogKind::Text(key) => {
                            if let Some(t) = db.text(key) {
                                d.spawn(txt(font, t, 19.0, PARCH));
                                any = true;
                            }
                        }
                        LogKind::Dialog(info) => {
                            for l in dialog_excerpt(db, info) {
                                d.spawn(txt(font, l, 18.0, DIM));
                                any = true;
                            }
                        }
                    }
                }
                if !any {
                    d.spawn(txt(font, "Noch keine Einträge.", 18.0, DIM));
                }
            });
        },
    );
}

// ---------------------------------------------------------------------------- inventory
const INV_TABS: [(&str, Option<i32>); 9] = [
    ("Alle", None),
    ("Waffen", Some(1)),
    ("Rüstung", Some(2)),
    ("Artefakte", Some(3)),
    ("Alchemie", Some(4)),
    ("Sonstiges", Some(6)),
    ("Schriften", Some(7)),
    ("Zauber", Some(9)),
    ("Fertigk.", Some(10)),
];

fn category_name(c: i32) -> &'static str {
    match c {
        1 => "Waffe",
        2 => "Rüstung",
        3 => "Artefakt",
        4 => "Alchemie",
        7 => "Schrift",
        9 => "Zauberbuch",
        10 => "Fertigkeit",
        _ => "Gegenstand",
    }
}

fn item_category(s: &GameSession, name: &str) -> i32 {
    match s.items.get(name).map(|i| i.category) {
        Some(0) | None => 6,
        Some(c) => c,
    }
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

fn item_rows(p: &mut P, cx: &Cx, list: &[(String, String, u32)], sel: Option<&str>, mk: impl Fn(&str) -> Action) {
    let font = cx.font;
    if list.is_empty() {
        p.spawn(txt(font, "Nichts vorhanden.", 19.0, DIM));
    }
    for (title, key, n) in list {
        let is_sel = sel == Some(key.as_str());
        let value = cx.s.items.get(key).map_or(0, |i| i.value);
        p.spawn((
            button(mk(key), if is_sel { BG_SEL } else { BG_ROW }, BG_HOVER),
            Node {
                padding: UiRect::axes(px(10), px(5)),
                justify_content: JustifyContent::SpaceBetween,
                flex_shrink: 0.0,
                column_gap: px(8),
                ..default()
            },
        ))
        .with_children(|r| {
            let label = if *n > 1 {
                format!("{title}  x{n}")
            } else {
                title.clone()
            };
            r.spawn(txt(font, label, 19.0, if is_sel { GOLD } else { PARCH }));
            r.spawn(txt(font, value.to_string(), 17.0, DIM));
        });
    }
}

fn item_details(p: &mut P, cx: &Cx, key: &str, actions: Vec<(String, Action)>) {
    let font = cx.font;
    let db = &cx.s.db;
    let info = cx.s.items.get(key);
    p.spawn(txt(font, item_name(db, key), 28.0, GOLD));
    let cat = info.map_or(6, |i| if i.category == 0 { 6 } else { i.category });
    p.spawn(txt(font, category_name(cat), 16.0, DIM));
    separator(p);
    if let Some(d) = db.text(&format!("ITEMDESC_{key}")).filter(|d| d != "-") {
        p.spawn(txt(font, d, 19.0, PARCH));
    }
    let Some(info) = info else {
        return;
    };
    p.spawn(node_col()).with_children(|k| {
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
            kv(k, cx, "Schaden", format!("{amount} ({ty})"), PARCH);
        }
        for (tag, v) in &info.modifiers {
            kv(k, cx, &attrib_name(db, tag), format!("{v:+}"), GREEN);
        }
        for (tag, need) in &info.requirements {
            let have = cx
                .s
                .state
                .attributes
                .get(&tag.to_ascii_lowercase())
                .copied()
                .unwrap_or(0);
            kv(
                k,
                cx,
                &format!("Benötigt {}", attrib_name(db, tag)),
                need.to_string(),
                if have >= *need { PARCH } else { RED },
            );
        }
        kv(k, cx, "Wert", format!("{} Gold", info.value), GOLD);
    });
    if !actions.is_empty() {
        p.spawn(Node {
            column_gap: px(10),
            margin: UiRect::top(px(10)),
            ..default()
        })
        .with_children(|r| {
            for (label, a) in actions {
                r.spawn((
                    button(a, BG_ROW, BG_HOVER),
                    Node {
                        padding: UiRect::axes(px(16), px(7)),
                        border: UiRect::all(px(1)),
                        ..default()
                    },
                    BorderColor::all(GOLD),
                ))
                .with_child(txt(font, label, 19.0, GOLD));
            }
        });
    }
}

fn inventory(commands: &mut Commands, cx: &Cx) {
    let st = &cx.s.state;
    let tabs = INV_TABS
        .iter()
        .enumerate()
        .map(|(i, (n, _))| (n.to_string(), cx.ui.inv_tab == i, Action::InvTab(i)))
        .collect();
    let cat = INV_TABS[cx.ui.inv_tab.min(INV_TABS.len() - 1)].1;
    let list = item_list(cx, &st.inventory, cat);
    let sel = cx
        .ui
        .inv_sel
        .clone()
        .filter(|s| list.iter().any(|(_, k, _)| k == s))
        .or_else(|| list.first().map(|(_, k, _)| k.clone()));
    let font = cx.font;
    window(commands, cx, &cx.t("HUD_SessionKey_InventoryMode", "Inventar"), tabs, |p| {
        p.spawn(Node {
            width: px(470),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|l| {
            l.spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            })
            .with_children(|g| {
                g.spawn(txt(font, cx.t("HUD_GoldAmount", "Gold"), 21.0, GOLD));
                g.spawn(txt(font, st.gold().to_string(), 21.0, GOLD));
            });
            l.spawn((
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(3),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                scroll_area("items", cx.scroll_y("items")),
            ))
            .with_children(|rows| {
                item_rows(rows, cx, &list, sel.as_deref(), |k| {
                    Action::SelectItem(k.to_string())
                });
            });
        });
        p.spawn((
            Node {
                flex_grow: 1.0,
                height: percent(100),
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                padding: UiRect::all(px(8)),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            scroll_area("item_detail", cx.scroll_y("item_detail")),
        ))
        .with_children(|d| {
            if let Some(k) = &sel {
                let info = cx.s.items.get(k);
                let equip = info.is_some_and(|i| matches!(i.category, 1 | 2));
                let label = if equip { "Anlegen" } else { "Benutzen" };
                item_details(d, cx, k, vec![(label.into(), Action::UseItem(k.clone()))]);
            }
        });
    });
}

// ---------------------------------------------------------------------------- character
const ATTRS: [&str; 6] = ["STR", "DEX", "INT", "SMT", "ALC", "THF"];

fn fame_name(db: &gothic_gameplay::GameDatabase, key: &str) -> String {
    match key.split_once(':') {
        Some(("pol", tag)) => db.text(&format!("HUD_{tag}")),
        Some(("enc", name)) => db.text(&format!("GO_Enclave_{name}")),
        _ => None,
    }
    .unwrap_or_else(|| key.to_string())
}

fn character(commands: &mut Commands, cx: &Cx) {
    let st = &cx.s.state;
    let db = &cx.s.db;
    let font = cx.font;
    let prev = if st.level == 0 {
        0
    } else {
        GameState::xp_next(st.level - 1)
    };
    let next = GameState::xp_next(st.level);
    let frac = (st.xp.saturating_sub(prev)) as f32 / (next - prev).max(1) as f32;
    window(commands, cx, "Charakter", Vec::new(), |p| {
        p.spawn(Node {
            width: px(500),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            row_gap: px(5),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|l| {
            l.spawn(txt(font, "Held", 30.0, GOLD));
            kv(l, cx, "Stufe", st.level.to_string(), PARCH);
            kv(l, cx, "Erfahrung", format!("{} / {}", st.xp, next), PARCH);
            bar(l, percent(100), 10.0, frac, GOLD);
            kv(l, cx, "Lernpunkte", st.learning_points.to_string(), GREEN);
            kv(l, cx, "Gold", st.gold().to_string(), GOLD);
            l.spawn(Node {
                height: px(8),
                ..default()
            });
            let v = cx.vitals;
            for (name, cur, max, c) in [
                (attrib_name(db, "HP"), v.hp, v.hp_max, RED),
                (attrib_name(db, "MP"), v.mp, v.mp_max, BLUE),
                (attrib_name(db, "SP"), v.sp, v.sp_max, GREEN),
            ] {
                kv(l, cx, &name, format!("{:.0} / {:.0}", cur, max), PARCH);
                bar(l, percent(100), 8.0, if max > 0.0 { cur / max } else { 0.0 }, c);
            }
            l.spawn(Node {
                height: px(8),
                ..default()
            });
            separator(l);
            for tag in ATTRS {
                let v = st.attributes.get(&tag.to_ascii_lowercase()).copied().unwrap_or(0);
                kv(l, cx, &attrib_name(db, tag), v.to_string(), PARCH);
            }
        });
        p.spawn(Node {
            flex_grow: 1.0,
            height: percent(100),
            flex_direction: FlexDirection::Column,
            row_gap: px(5),
            ..default()
        })
        .with_children(|r| {
            r.spawn(txt(font, "Fertigkeiten", 26.0, GOLD));
            r.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(3),
                    max_height: px(250),
                    overflow: Overflow::scroll_y(),
                    flex_shrink: 0.0,
                    ..default()
                },
                scroll_area("perks", cx.scroll_y("perks")),
            ))
            .with_children(|s| {
                if st.skills.is_empty() {
                    s.spawn(txt(font, "Noch keine erlernt.", 18.0, DIM));
                }
                for k in &st.skills {
                    let name = db
                        .text(&format!("FO_it_{k}"))
                        .or_else(|| db.text(&format!("FO_{k}")))
                        .unwrap_or_else(|| k.clone());
                    s.spawn((
                        Node {
                            padding: UiRect::axes(px(10), px(4)),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        BackgroundColor(BG_ROW),
                        children![txt(font, name, 19.0, PARCH)],
                    ));
                }
            });
            r.spawn(Node {
                height: px(6),
                ..default()
            });
            r.spawn(txt(font, "Ansehen", 26.0, GOLD));
            if st.fame.is_empty() {
                r.spawn(txt(font, "Noch unbekannt.", 18.0, DIM));
            }
            for (k, v) in &st.fame {
                kv(r, cx, &fame_name(db, k), v.to_string(), PARCH);
                bar(r, percent(100), 6.0, *v as f32 / 100.0, GOLD_DIM);
            }
        });
    });
}

// ---------------------------------------------------------------------------- trade
/// Layout only: trade rules (price factors, stock refresh) are not decoded yet.
fn trade(commands: &mut Commands, cx: &Cx) {
    let Some((npc, stock)) = cx.ui.trade.as_ref() else {
        return;
    };
    let font = cx.font;
    let st = &cx.s.state;
    let mine = item_list(cx, &st.inventory, None);
    let theirs: Vec<(String, String, u32)> = {
        let mut v: Vec<_> = stock
            .iter()
            .filter(|(k, n)| k != GOLD_ITEM && *n > 0)
            .map(|(k, n)| (item_name(&cx.s.db, k), k.clone(), *n))
            .collect();
        v.sort();
        v
    };
    let sel = cx.ui.inv_sel.clone();
    window(
        commands,
        cx,
        &format!("Handel - {}", npc.replace('_', " ")),
        Vec::new(),
        |p| {
            for (title, list, buy, scroll) in [
                (
                    cx.t("HUD_Trade", "Tauschen"),
                    &mine,
                    false,
                    "trade_mine",
                ),
                (npc.replace('_', " "), &theirs, true, "trade_theirs"),
            ] {
                p.spawn(Node {
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    height: percent(100),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6),
                    ..default()
                })
                .with_children(|c| {
                    c.spawn(Node {
                        justify_content: JustifyContent::SpaceBetween,
                        ..default()
                    })
                    .with_children(|h| {
                        h.spawn(txt(font, if buy { title.clone() } else { "Dein Inventar".into() }, 23.0, GOLD));
                        if !buy {
                            h.spawn(txt(font, format!("{} Gold", st.gold()), 21.0, GOLD));
                        }
                    });
                    c.spawn((
                        Node {
                            flex_grow: 1.0,
                            min_height: px(0),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(3),
                            overflow: Overflow::scroll_y(),
                            ..default()
                        },
                        scroll_area(scroll, cx.scroll_y(scroll)),
                    ))
                    .with_children(|rows| {
                        item_rows(rows, cx, list, sel.as_deref(), |k| {
                            Action::Trade(k.to_string(), buy)
                        });
                    });
                    c.spawn(txt(
                        font,
                        if buy {
                            "Klick: kaufen (Preis = Warenwert)"
                        } else {
                            "Klick: verkaufen (Preis = Warenwert)"
                        },
                        15.0,
                        DIM,
                    ));
                });
            }
        },
    );
}
