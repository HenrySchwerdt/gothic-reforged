//! Palette, text/button helpers and small interaction systems.
use crate::assets::UiAssets;
use crate::atlas::Pic;
use crate::{Act, Action};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button, observe};
use gothic_gameplay::GameDatabase;

/// Body text colour sampled from the original screens.
pub const TEXT: Color = Color::srgb(0.878, 0.769, 0.584);
pub const TEXT_DIM: Color = Color::srgb(0.60, 0.53, 0.40);
pub const TEXT_HI: Color = Color::srgb(1.0, 0.97, 0.88);
pub const TEXT_BAD: Color = Color::srgb(0.85, 0.30, 0.25);
pub const BORDER: Color = Color::srgb(0.62, 0.52, 0.36);
pub const SEL: Color = Color::srgba(1.0, 0.85, 0.45, 0.16);
pub const CELL: Color = Color::srgba(0.17, 0.12, 0.09, 0.78);
pub const CELL_HOVER: Color = Color::srgba(0.32, 0.24, 0.15, 0.9);

pub type P<'a> = ChildSpawnerCommands<'a>;

/// Absolutely positioned box in the 1024x768 design space.
pub fn abs(x: f32, y: f32, w: f32, h: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(x),
        top: px(y),
        width: px(w),
        height: px(h),
        ..default()
    }
}

/// Atlas picture at its native (or the given) size; flat fallback when the atlas is missing.
pub fn spawn_pic<'a>(
    p: &'a mut P,
    a: &UiAssets,
    pic: Pic,
    x: f32,
    y: f32,
    size: Option<(f32, f32)>,
) -> EntityCommands<'a> {
    let (w, h) = size.unwrap_or((pic.w, pic.h));
    let node = abs(x, y, w, h);
    match a.page(pic.page) {
        Some(img) => p.spawn((
            node,
            ImageNode {
                image: img.clone(),
                rect: Some(Rect::new(pic.x, pic.y, pic.x + pic.w, pic.y + pic.h)),
                ..default()
            },
        )),
        None => p.spawn((node, BackgroundColor(Color::srgba(0.1, 0.12, 0.13, 0.85)))),
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Al {
    Left,
    Center,
    Right,
}

/// Text in a fixed box (design-space pixels); vertically centred.
#[allow(clippy::too_many_arguments)]
pub fn label(
    p: &mut P,
    font: &Handle<Font>,
    s: impl Into<String>,
    (x, y, w, h): (f32, f32, f32, f32),
    size: f32,
    color: Color,
    al: Al,
) {
    let mut n = abs(x, y, w, h);
    n.align_items = AlignItems::Center;
    n.justify_content = match al {
        Al::Left => JustifyContent::FlexStart,
        Al::Center => JustifyContent::Center,
        Al::Right => JustifyContent::FlexEnd,
    };
    n.overflow = Overflow::clip();
    p.spawn(n).with_child((
        Text::new(s),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
        TextLayout::new_with_justify(match al {
            Al::Left => Justify::Left,
            Al::Center => Justify::Center,
            Al::Right => Justify::Right,
        }),
    ));
}

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
