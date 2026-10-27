//! Drawing of [`Sheet`]s (the world windows) into a Bevy UI tree; used by `render.rs`.
use crate::render::{
    BRASS, BRASS_DIM, HEALTH, LEATHER, LEATHER_DARK, MANA, STAMINA, TEXT, TEXT_DIM, fit, frame,
    text,
};
use crate::sheet::{ItemKind, Sheet};
use bevy::prelude::*;

/// Draw a sheet: absolutely positioned items on a leather panel. Sizes derive from the
/// sheet's dmm height so text keeps its angular size at any texture resolution.
pub fn build_sheet(
    sheet: &Sheet,
    hovered: Option<usize>,
    px: UVec2,
    parent: &mut ChildSpawnerCommands,
) {
    let (w, h) = (px.x as f32, px.y as f32);
    let ppd = h / sheet.size_dmm.y.max(1.0);
    parent
        .spawn((frame(px), BackgroundColor(LEATHER), BorderColor::all(BRASS)))
        .with_children(|root| {
            for (k, it) in sheet.items.iter().enumerate() {
                let r = it.rect;
                let (rw, rh) = (r.w * w, r.h * h);
                let hot = hovered == Some(k) && it.interactive();
                let pos = |extra: Node| Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(r.x * w),
                    top: Val::Px(r.y * h),
                    width: Val::Px(rw),
                    height: Val::Px(rh),
                    ..extra
                };
                let col = if it.enabled { TEXT } else { TEXT_DIM };
                match it.kind {
                    ItemKind::Title => {
                        root.spawn(pos(Node {
                            align_items: AlignItems::Center,
                            ..default()
                        }))
                        .with_child(text(it.text.clone(), 30.0 * ppd, BRASS));
                    }
                    ItemKind::Paragraph => {
                        root.spawn(pos(Node {
                            align_items: AlignItems::Center,
                            ..default()
                        }))
                        .with_child(text(
                            it.text.clone(),
                            (rh * 0.78).min(22.0 * ppd),
                            TEXT,
                        ));
                    }
                    ItemKind::Rule => {
                        root.spawn((
                            pos(Node {
                                height: Val::Px(2.0),
                                ..default()
                            }),
                            BackgroundColor(BRASS_DIM),
                        ));
                    }
                    ItemKind::Bar(f) => {
                        let c = if it.text.starts_with('L') {
                            HEALTH
                        } else if it.text.starts_with('M') {
                            MANA
                        } else {
                            STAMINA
                        };
                        root.spawn((
                            pos(Node {
                                border: UiRect::all(Val::Px(2.0)),
                                ..default()
                            }),
                            BackgroundColor(LEATHER_DARK),
                            BorderColor::all(BRASS_DIM),
                        ))
                        .with_children(|b| {
                            b.spawn((
                                Node {
                                    width: Val::Percent(f * 100.0),
                                    height: Val::Percent(100.0),
                                    ..default()
                                },
                                BackgroundColor(c),
                            ));
                            b.spawn(Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(8.0),
                                height: Val::Percent(100.0),
                                align_items: AlignItems::Center,
                                ..default()
                            })
                            .with_child(text(
                                format!("{}  {}", it.text, it.value),
                                (rh * 0.62).min(20.0 * ppd),
                                TEXT,
                            ));
                        });
                    }
                    ItemKind::Button | ItemKind::Tab => {
                        let bg = if hot {
                            Color::srgba(0.45, 0.34, 0.14, 0.95)
                        } else if it.selected {
                            Color::srgba(0.30, 0.22, 0.10, 0.95)
                        } else {
                            LEATHER_DARK
                        };
                        root.spawn((
                            pos(Node {
                                border: UiRect::all(Val::Px(2.0)),
                                border_radius: BorderRadius::all(Val::Px(rh * 0.2)),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            }),
                            BackgroundColor(bg),
                            BorderColor::all(if it.enabled { BRASS } else { BRASS_DIM }),
                        ))
                        .with_child(text(it.text.clone(), (rh * 0.5).min(22.0 * ppd), col));
                    }
                    ItemKind::Label | ItemKind::Row | ItemKind::Cell => {
                        let bg = if hot {
                            Color::srgba(0.45, 0.34, 0.14, 0.7)
                        } else if it.selected {
                            Color::srgba(0.30, 0.22, 0.10, 0.8)
                        } else {
                            Color::NONE
                        };
                        let size = (rh * 0.58).min(22.0 * ppd);
                        root.spawn((
                            pos(Node {
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(size * 0.5),
                                padding: UiRect::horizontal(Val::Px(size * 0.4)),
                                border_radius: BorderRadius::all(Val::Px(size * 0.3)),
                                ..default()
                            }),
                            BackgroundColor(bg),
                        ))
                        .with_children(|row| {
                            if !it.icon.is_empty() {
                                row.spawn((
                                    Node {
                                        width: Val::Px(rh * 0.7),
                                        height: Val::Px(rh * 0.7),
                                        border: UiRect::all(Val::Px(2.0)),
                                        flex_shrink: 0.0,
                                        ..default()
                                    },
                                    BackgroundColor(LEATHER_DARK),
                                    BorderColor::all(BRASS_DIM),
                                ));
                            }
                            let name = if it.count > 1 {
                                format!("{}  x{}", it.text, it.count)
                            } else {
                                it.text.clone()
                            };
                            row.spawn(Node {
                                flex_grow: 1.0,
                                ..default()
                            })
                            .with_child(text(name.clone(), fit(&name, size, rw * 0.7), col));
                            if !it.value.is_empty() {
                                row.spawn(text(it.value.clone(), size, TEXT_DIM));
                            }
                        });
                    }
                }
            }
            // Scroll hints: a chevron where a list continues (the clipped row is the main cue).
            for reg in &sheet.regions {
                let (up, down) = (reg.top > 0, reg.top + reg.visible < reg.total);
                for (on, y, ch) in [
                    (up, reg.rect.y - 0.035, "^"),
                    (down, reg.rect.y + reg.rect.h, "v"),
                ] {
                    if on {
                        root.spawn(Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px((reg.rect.x + reg.rect.w * 0.5) * w),
                            top: Val::Px(y * h),
                            ..default()
                        })
                        .with_child(text(ch, 20.0 * ppd, BRASS));
                    }
                }
            }
        });
}
