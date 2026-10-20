//! Palette, text/button helpers and small interaction systems.
use crate::{Act, Action};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button, observe};
use gothic_gameplay::GameDatabase;

pub const GOLD: Color = Color::srgb(0.80, 0.66, 0.36);
pub const GOLD_DIM: Color = Color::srgb(0.46, 0.37, 0.20);
pub const PARCH: Color = Color::srgb(0.90, 0.85, 0.72);
pub const DIM: Color = Color::srgb(0.63, 0.57, 0.46);
pub const RED: Color = Color::srgb(0.85, 0.30, 0.25);
pub const GREEN: Color = Color::srgb(0.50, 0.78, 0.38);
pub const BLUE: Color = Color::srgb(0.38, 0.55, 0.88);
pub const BG: Color = Color::srgba(0.045, 0.040, 0.035, 0.96);
pub const BG_ROW: Color = Color::srgba(0.12, 0.105, 0.085, 0.80);
pub const BG_HOVER: Color = Color::srgba(0.30, 0.22, 0.10, 0.92);
pub const BG_SEL: Color = Color::srgba(0.40, 0.29, 0.11, 0.95);
pub const BLACK_BAR: Color = Color::srgba(0.0, 0.0, 0.0, 0.93);

#[derive(Component)]
pub struct HoverStyle {
    pub normal: Color,
    pub hover: Color,
}

/// Marks a scrollable node; the name keys the remembered scroll offset across rebuilds.
#[derive(Component)]
pub struct ScrollArea(pub &'static str);

pub fn txt(font: &Handle<Font>, s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(s),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
        bevy::text::LineHeight::RelativeToFont(1.3),
    )
}

pub fn span(font: &Handle<Font>, s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        TextSpan::new(s),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

/// Clickable node: add a `Node` yourself.
pub fn button(action: Action, normal: Color, hover: Color) -> impl Bundle {
    (
        Button,
        Hovered::default(),
        HoverStyle { normal, hover },
        BackgroundColor(normal),
        observe(move |_: On<Activate>, mut w: MessageWriter<Act>| {
            w.write(Act(action.clone()));
        }),
    )
}

pub fn scroll_area(name: &'static str, y: f32) -> impl Bundle {
    (
        ScrollArea(name),
        Hovered::default(),
        ScrollPosition(Vec2::new(0.0, y)),
    )
}

pub fn hover_style(mut q: Query<(&Hovered, &HoverStyle, &mut BackgroundColor), Changed<Hovered>>) {
    for (h, s, mut bg) in &mut q {
        bg.0 = if h.get() { s.hover } else { s.normal };
    }
}

pub fn scroll_wheel(
    mut wheel: MessageReader<MouseWheel>,
    mut q: Query<(&Hovered, &mut ScrollPosition, &ComputedNode), With<ScrollArea>>,
) {
    for w in wheel.read() {
        let dy = match w.unit {
            MouseScrollUnit::Line => w.y * 36.0,
            MouseScrollUnit::Pixel => w.y,
        };
        for (h, mut pos, node) in &mut q {
            if h.get() {
                let max = ((node.content_size().y - node.size().y) * node.inverse_scale_factor())
                    .max(0.0);
                pos.y = (pos.y - dy).clamp(0.0, max);
            }
        }
    }
}

pub fn item_name(db: &GameDatabase, item: &str) -> String {
    db.text(&format!("FO_{item}"))
        .or_else(|| db.text(item))
        .unwrap_or_else(|| item.to_string())
}

/// `ATTRIB_STR` -> localized name, falling back to the tag.
pub fn attrib_name(db: &GameDatabase, tag: &str) -> String {
    db.text(&format!("ATTRIB_{tag}"))
        .unwrap_or_else(|| tag.to_string())
}
