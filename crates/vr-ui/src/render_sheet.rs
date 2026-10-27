//! Drawing of [`Sheet`]s (the world windows) into a Bevy UI tree, in the original UI's look:
//! the original view backgrounds, item icons from the `g3.igl` atlases, body text `#E0C495`
//! in the body font (Times New Roman Bold), headings in the heading font, dark teal list fills.
//! Without `UiAssets` (tests, no installation) it falls back to flat panels.
use crate::hud::HudData;
use crate::render::{HudHeading, fit, text};
use crate::sheet::{ItemKind, Sheet, SheetItem};
use bevy::prelude::*;
use gothic_ui::assets::UiAssets;
use gothic_ui::atlas::{MARK_QUEST, Pic};
use gothic_ui::style::{BORDER, CELL, CELL_HOVER, SEL, TEXT, TEXT_DIM, TEXT_HI};

const FLAT: Color = Color::srgba(0.05, 0.12, 0.12, 0.93);
const TEAL: Color = Color::srgba(0.0, 0.07, 0.04, 0.92);
const TEAL_ON: Color = Color::srgba(0.07, 0.22, 0.12, 0.95);
const INK: Color = Color::srgb(0.22, 0.15, 0.09);
const HOT: Color = Color::srgba(1.0, 0.85, 0.45, 0.30);

fn pic_node(a: Option<&UiAssets>, pic: Pic, node: Node) -> Option<impl Bundle + use<>> {
    let a = a?;
    let pic = a.resolve(pic);
    let image = a.page(pic.page)?.clone();
    Some((
        node,
        ImageNode {
            image,
            rect: Some(Rect::new(pic.x, pic.y, pic.x + pic.w, pic.y + pic.h)),
            ..default()
        },
    ))
}

fn heading_text(s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (text(s, size, color), HudHeading)
}

/// Draw a sheet into `parent` (a UI root of `px` pixels).
pub fn build_sheet(
    sheet: &Sheet,
    hovered: Option<usize>,
    px: UVec2,
    assets: Option<&UiAssets>,
    data: &HudData,
    parent: &mut ChildSpawnerCommands,
) {
    let (w, h) = (px.x as f32, px.y as f32);
    let k = w / sheet.design.x; // texture pixels per design pixel
    let abs = |r: crate::sheet::Rect| Node {
        position_type: PositionType::Absolute,
        left: Val::Px(r.x * w),
        top: Val::Px(r.y * h),
        width: Val::Px(r.w * w),
        height: Val::Px(r.h * h),
        ..default()
    };
    let icon_pic = |it: &SheetItem| -> Option<Pic> {
        if it.icon.is_empty() {
            return None;
        }
        if it.icon == "quest" {
            return Some(MARK_QUEST);
        }
        let file = data.icons.get(&it.icon)?;
        assets?.item_pic(file, !it.enabled)
    };
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        })
        .with_children(|root| {
            for (pic, r) in &sheet.backdrops {
                let n = abs(*r);
                match pic_node(assets, *pic, n.clone()) {
                    Some(b) => {
                        root.spawn(b);
                    }
                    None => {
                        root.spawn((
                            Node {
                                border: UiRect::all(Val::Px(2.0)),
                                ..n
                            },
                            BackgroundColor(FLAT),
                            BorderColor::all(BORDER),
                        ));
                    }
                }
            }
            for (idx, it) in sheet.items.iter().enumerate() {
                let r = it.rect;
                let (rw, rh) = (r.w * w, r.h * h);
                let hot = hovered == Some(idx) && it.interactive();
                let size = it.size * k;
                let col = match (it.ink, it.enabled, hot) {
                    (true, true, true) => Color::srgb(0.45, 0.20, 0.05),
                    (true, true, false) => INK,
                    (true, false, _) => Color::srgba(0.22, 0.15, 0.09, 0.45),
                    (false, false, _) => TEXT_DIM,
                    (false, true, true) => TEXT_HI,
                    (false, true, false) => TEXT,
                };
                match it.kind {
                    ItemKind::Title => {
                        root.spawn(Node {
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..abs(r)
                        })
                        .with_child(heading_text(it.text.clone(), size, if it.ink { INK } else { TEXT_HI }));
                    }
                    ItemKind::Paragraph => {
                        root.spawn(Node {
                            align_items: AlignItems::Center,
                            overflow: Overflow::clip(),
                            ..abs(r)
                        })
                        .with_child(text(it.text.clone(), size, col));
                    }
                    ItemKind::Rule => {
                        root.spawn((abs(r), BackgroundColor(BORDER)));
                    }
                    ItemKind::Bar(f) => {
                        root.spawn((abs(r), BackgroundColor(TEAL), BorderColor::all(BORDER)))
                            .with_child((
                                Node {
                                    width: Val::Percent(f * 100.0),
                                    height: Val::Percent(100.0),
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(0.62, 0.09, 0.07)),
                            ));
                    }
                    ItemKind::Image(name) => {
                        // The painted world map: shown at 0.886 and shifted so the compass rose
                        // sits where the original puts it, clipped to its control.
                        let map = name == "g3_hud_view_world_back.tga";
                        let n = abs(r);
                        match assets.and_then(|a| a.named_pic(name)) {
                            Some(p) if map => {
                                root.spawn(Node { overflow: Overflow::clip(), ..n })
                                    .with_children(|c| {
                                        let (sc, ox, oy) = (0.886 * k, -35.0 * k, -9.0 * k);
                                        let node = Node {
                                            position_type: PositionType::Absolute,
                                            left: Val::Px(ox),
                                            top: Val::Px(oy),
                                            width: Val::Px(p.w * sc),
                                            height: Val::Px(p.h * sc),
                                            ..default()
                                        };
                                        if let Some(b) = pic_node(assets, p, node) {
                                            c.spawn(b);
                                        }
                                    });
                            }
                            Some(p) => {
                                if let Some(b) = pic_node(assets, p, n.clone()) {
                                    root.spawn(b);
                                }
                            }
                            None => {
                                root.spawn((n, BackgroundColor(TEAL)));
                            }
                        }
                    }
                    ItemKind::Marker => {
                        if let Some(p) = assets.and_then(|a| a.named_pic(&it.text))
                            && let Some(b) = pic_node(assets, p, abs(r))
                        {
                            root.spawn(b);
                        }
                    }
                    ItemKind::Button | ItemKind::Tab => {
                        let bg = if it.selected {
                            TEAL_ON
                        } else if hot {
                            Color::srgba(0.24, 0.16, 0.10, 1.0)
                        } else if it.enabled {
                            Color::srgba(0.24, 0.16, 0.10, 0.80)
                        } else {
                            Color::srgba(0.24, 0.16, 0.10, 0.30)
                        };
                        let bg = if it.kind == ItemKind::Tab && !it.selected { TEAL } else { bg };
                        root.spawn((
                            Node {
                                border: UiRect::all(Val::Px((2.0 * k).max(1.0))),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..abs(r)
                            },
                            BackgroundColor(bg),
                            BorderColor::all(if hot { TEXT_HI } else { BORDER }),
                        ))
                        .with_child(heading_text(it.text.clone(), size.min(rh * 0.6), col));
                    }
                    ItemKind::Cell => {
                        root.spawn((
                            Node {
                                border: UiRect::all(Val::Px((1.0 * k).max(1.0))),
                                ..abs(r)
                            },
                            BackgroundColor(if hot { CELL_HOVER } else { CELL }),
                            BorderColor::all(if hot { TEXT_HI } else { Color::NONE }),
                        ))
                        .with_children(|c| {
                            let icon = icon_pic(it);
                            if let Some(b) = icon.and_then(|p| {
                                pic_node(
                                    assets,
                                    p,
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: Val::Px(0.0),
                                        top: Val::Px(0.0),
                                        width: Val::Percent(100.0),
                                        height: Val::Percent(100.0),
                                        ..default()
                                    },
                                )
                            }) {
                                c.spawn(b);
                            } else {
                                // No atlas: the item's short name in the cell.
                                c.spawn(text(it.text.chars().take(7).collect::<String>(), 11.0 * k, col));
                            }
                            if it.count > 1 {
                                c.spawn(Node {
                                    position_type: PositionType::Absolute,
                                    right: Val::Px(2.0 * k),
                                    bottom: Val::Px(0.0),
                                    ..default()
                                })
                                .with_child(text(it.count.to_string(), 13.0 * k, TEXT_HI));
                            }
                        });
                    }
                    ItemKind::Label | ItemKind::Row => {
                        let bg = if it.selected {
                            SEL
                        } else if hot {
                            HOT
                        } else {
                            Color::NONE
                        };
                        root.spawn((
                            Node {
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(6.0 * k),
                                overflow: Overflow::clip(),
                                ..abs(r)
                            },
                            BackgroundColor(bg),
                        ))
                        .with_children(|row| {
                            let icon = icon_pic(it);
                            if let Some(b) = icon.and_then(|p| {
                                pic_node(
                                    assets,
                                    p,
                                    Node {
                                        width: Val::Px(rh.min(22.0 * k)),
                                        height: Val::Px(rh.min(22.0 * k)),
                                        flex_shrink: 0.0,
                                        ..default()
                                    },
                                )
                            }) {
                                row.spawn(b);
                            }
                            let name = if it.count > 1 { format!("{}  x{}", it.text, it.count) } else { it.text.clone() };
                            row.spawn(Node {
                                flex_grow: 1.0,
                                overflow: Overflow::clip(),
                                ..default()
                            })
                            .with_child(text(name.clone(), fit(&name, size, rw * 0.72), col));
                            if !it.value.is_empty() {
                                row.spawn(text(it.value.clone(), size, col));
                            }
                        });
                    }
                }
            }
            // Name and price of the hovered cell / row.
            if let (Some(info), Some(i)) = (sheet.info, hovered) {
                let it = &sheet.items[i];
                if !it.text.is_empty() {
                    let line = if it.value.is_empty() { it.text.clone() } else { format!("{}  ({})", it.text, it.value) };
                    root.spawn(Node {
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..abs(info)
                    })
                    .with_child(text(line, 16.0 * k, TEXT_HI));
                }
            }
            // Scroll hints: a chevron where a list continues.
            for reg in &sheet.regions {
                let (up, down) = (reg.top > 0, reg.top + reg.visible < reg.total);
                for (on, y, ch) in [(up, reg.rect.y - 0.03, "▲"), (down, reg.rect.y + reg.rect.h, "▼")] {
                    if on {
                        root.spawn(Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px((reg.rect.x + reg.rect.w) * w - 14.0 * k),
                            top: Val::Px(y * h),
                            ..default()
                        })
                        .with_child(text(ch, 12.0 * k, TEXT));
                    }
                }
            }
        });
}
