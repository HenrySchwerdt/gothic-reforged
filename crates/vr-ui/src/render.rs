//! Rendered HUD surfaces: every world-space part (wrist, dialog, subtitles, backpack, wheel,
//! book pages, map) is a Bevy UI tree drawn into its own texture by an offscreen camera and
//! shown on a quad at the part's panel pose. The layout and selection logic stays in the
//! Bevy-free modules; this only draws [`VrHud`] / [`HudData`].
//!
//! The look is synthetic (no game art): dark translucent leather panels with brass borders,
//! parchment for the book and the map. Text uses [`HudFont`] when the host inserts one (the
//! built-in Bevy font has no umlauts); the real UI atlas and fonts can replace the colours and
//! the font once the host loads them (`gothic_ui::UiAssets`).
use crate::backpack::{Backpack, COLS, ROWS, STRIP};
use crate::handheld::{Book, LINE_CHARS, LINES_PER_PAGE};
use crate::hud::{HudData, VrHud};
use crate::panel::Panel;
use crate::plugin::{HudDataRes, VrHudRes};
use crate::wheel::WHEEL_RADIUS;
use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureFormat};
use gothic_vr::Pose;

/// Texture resolution, pixels per metre of panel (capped per side).
const PX_PER_M: f32 = 2400.0;
const MAX_PX: f32 = 1600.0;

const LEATHER: Color = Color::srgba(0.10, 0.08, 0.06, 0.88);
const LEATHER_DARK: Color = Color::srgba(0.05, 0.04, 0.03, 0.92);
const BRASS: Color = Color::srgb(0.78, 0.63, 0.36);
const BRASS_DIM: Color = Color::srgba(0.78, 0.63, 0.36, 0.35);
const TEXT: Color = Color::srgb(0.94, 0.89, 0.79);
const TEXT_DIM: Color = Color::srgb(0.62, 0.57, 0.49);
const PARCHMENT: Color = Color::srgb(0.87, 0.80, 0.65);
const INK: Color = Color::srgb(0.22, 0.15, 0.09);
const HEALTH: Color = Color::srgb(0.78, 0.12, 0.10);
const MANA: Color = Color::srgb(0.20, 0.38, 0.90);
const STAMINA: Color = Color::srgb(0.30, 0.70, 0.28);

/// The drawn parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceKind {
    Wrist,
    Dialog,
    Subtitle,
    Backpack,
    Wheel,
    BookLeft,
    BookRight,
    Map,
}

impl SurfaceKind {
    const ALL: [SurfaceKind; 8] = [
        SurfaceKind::Wrist,
        SurfaceKind::Dialog,
        SurfaceKind::Subtitle,
        SurfaceKind::Backpack,
        SurfaceKind::Wheel,
        SurfaceKind::BookLeft,
        SurfaceKind::BookRight,
        SurfaceKind::Map,
    ];
}

/// One rendered surface: the quad, its texture, the camera and the UI root.
#[derive(Component)]
pub struct Surface {
    pub kind: SurfaceKind,
    image: Handle<Image>,
    material: Handle<StandardMaterial>,
    camera: Entity,
    root: Entity,
    /// Content signature of the last build (rebuild only on change).
    key: String,
    px: UVec2,
}

impl Surface {
    /// The texture the part is drawn into (for previews and screenshots).
    pub fn image(&self) -> &Handle<Image> {
        &self.image
    }
}

/// Font for all HUD text (any face with German glyphs). Without it the Bevy default font is
/// used, which lacks umlauts.
#[derive(Resource, Clone)]
pub struct HudFont(pub Handle<Font>);

/// Marks text spawned by the HUD so [`HudFont`] reaches it.
#[derive(Component)]
struct HudText;

/// Adds the rendered surfaces (needs a renderer; use with `VrUiPlugin { gizmos: false }`).
pub struct VrUiRenderPlugin;

impl Plugin for VrUiRenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_surfaces).add_systems(
            PostUpdate,
            (
                update_surfaces.before(TransformSystems::Propagate),
                apply_font
                    .after(update_surfaces)
                    .before(bevy::ui::UiSystems::Prepare)
                    .run_if(resource_exists::<HudFont>),
            ),
        );
    }
}

fn target_image(px: UVec2) -> Image {
    let mut image = Image::new_target_texture(px.x, px.y, TextureFormat::Bgra8UnormSrgb, None);
    image.asset_usage = RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD;
    image
}

fn spawn_surfaces(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let quad = meshes.add(Rectangle::new(1.0, 1.0));
    for (i, kind) in SurfaceKind::ALL.into_iter().enumerate() {
        let px = UVec2::new(64, 64);
        let image = images.add(target_image(px));
        let camera = commands
            .spawn((
                Camera2d,
                Camera {
                    order: -100 - i as isize,
                    is_active: false,
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    ..default()
                },
                RenderTarget::Image(image.clone().into()),
                Name::new(format!("HudCamera{kind:?}")),
            ))
            .id();
        let root = commands
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                UiTargetCamera(camera),
            ))
            .id();
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(image.clone()),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            double_sided: true,
            ..default()
        });
        commands.spawn((
            Mesh3d(quad.clone()),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            Visibility::Hidden,
            Surface {
                kind,
                image,
                material,
                camera,
                root,
                key: String::new(),
                px,
            },
            Name::new(format!("HudSurface{kind:?}")),
        ));
    }
}

/// Pose, size (m) and alpha of a surface, if it is on show.
fn placement(kind: SurfaceKind, hud: &VrHud) -> Option<(Pose, Vec2, f32)> {
    let p = |panel: &Panel| (panel.pose, panel.size, 1.0);
    match kind {
        SurfaceKind::Wrist => hud.wrist.as_ref().map(p),
        SurfaceKind::Dialog => hud.dialog.as_ref().map(|d| p(&d.panel)),
        SurfaceKind::Backpack => hud.backpack.as_ref().map(|b| p(&b.panel)),
        SurfaceKind::Subtitle => hud.subtitle.as_ref().map(|s| {
            let w = s.lines.iter().map(|l| l.chars().count()).max().unwrap_or(1) as f32
                * s.text_height
                * 0.58
                + s.text_height * 1.6;
            let h = (s.lines.len() as f32 * 1.35 + 1.0) * s.text_height;
            (s.pose, Vec2::new(w, h), s.alpha)
        }),
        SurfaceKind::Wheel => hud.wheel.open.map(|pose| {
            let side = (WHEEL_RADIUS + 0.05) * 2.0;
            (pose, Vec2::splat(side), 1.0)
        }),
        SurfaceKind::BookLeft | SurfaceKind::BookRight => hud.book.as_ref().map(|(_, pose)| {
            let [l, r] = Book::panels(pose);
            p(if kind == SurfaceKind::BookLeft {
                &l
            } else {
                &r
            })
        }),
        SurfaceKind::Map => hud.map.as_ref().map(|(_, panel)| p(panel)),
    }
}

/// Short display name of an item template (`ItMw_Sword_01` -> `Sword 01`, at most 16 characters).
fn item_name(template: &str) -> String {
    let t = template
        .split_once('_')
        .map_or(template, |(_, rest)| rest)
        .replace('_', " ");
    if t.chars().count() > 16 {
        t.chars().take(15).chain(std::iter::once('.')).collect()
    } else {
        t
    }
}

/// Text size for `s` at most `size`, shrunk to fit `width` pixels (about 0.58 em per glyph).
fn fit(s: &str, size: f32, width: f32) -> f32 {
    size.min(width / (s.chars().count().max(1) as f32 * 0.58))
}

fn content_key(kind: SurfaceKind, hud: &VrHud, data: &HudData) -> String {
    match kind {
        SurfaceKind::Wrist => {
            let f = data.vitals.fractions();
            format!("{:.2}{:.2}{:.2}", f[0], f[1], f[2])
        }
        SurfaceKind::Dialog => hud.dialog.as_ref().map_or_else(String::new, |d| {
            format!("{:?}{:?}{:?}", d.options, d.rows(), d.list.selected())
        }),
        SurfaceKind::Subtitle => hud
            .subtitle
            .as_ref()
            .map_or_else(String::new, |s| format!("{:?}{:?}", s.lines, s.off_view)),
        SurfaceKind::Backpack => hud.backpack.as_ref().map_or_else(String::new, |b| {
            format!("{}{:?}{}", b.page, b.hovered, data.inventory.rows.len())
        }),
        SurfaceKind::Wheel => format!("{:?}{}", hud.wheel.hovered, data.quick_slots.len()),
        SurfaceKind::BookLeft | SurfaceKind::BookRight => {
            hud.book.as_ref().map_or_else(String::new, |(b, _)| {
                format!("{}{}", b.spread, b.lines.len())
            })
        }
        SurfaceKind::Map => hud.map.as_ref().map_or_else(String::new, |(m, _)| {
            format!("{:?}{:?}{:?}", m.markers, m.player, hud.map_label)
        }),
    }
}

fn text(s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(s),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
        HudText,
    )
}

fn apply_font(font: Res<HudFont>, mut texts: Query<&mut TextFont, Added<HudText>>) {
    for mut t in &mut texts {
        t.font = FontSource::Handle(font.0.clone());
    }
}

/// The framed background every leather panel shares.
fn frame(px: UVec2) -> Node {
    let b = (px.x.min(px.y) as f32 * 0.012).max(2.0);
    Node {
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        border: UiRect::all(Val::Px(b)),
        border_radius: BorderRadius::all(Val::Px(b * 4.0)),
        padding: UiRect::all(Val::Px(b * 3.0)),
        flex_direction: FlexDirection::Column,
        ..default()
    }
}

fn build(
    kind: SurfaceKind,
    hud: &VrHud,
    data: &HudData,
    px: UVec2,
    parent: &mut ChildSpawnerCommands,
) {
    let w = px.x as f32;
    let h = px.y as f32;
    match kind {
        SurfaceKind::Wrist => {
            let f = data.vitals.fractions();
            let vals = [data.vitals.health, data.vitals.mana, data.vitals.stamina];
            parent
                .spawn((frame(px), BackgroundColor(LEATHER), BorderColor::all(BRASS)))
                .with_children(|p| {
                    for (i, (label, color)) in [("HP", HEALTH), ("MP", MANA), ("SP", STAMINA)]
                        .into_iter()
                        .enumerate()
                    {
                        p.spawn(Node {
                            flex_grow: 1.0,
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(w * 0.03),
                            ..default()
                        })
                        .with_children(|row| {
                            row.spawn(text(label, h * 0.17, BRASS));
                            row.spawn((
                                Node {
                                    flex_grow: 1.0,
                                    height: Val::Percent(55.0),
                                    border: UiRect::all(Val::Px(2.0)),
                                    border_radius: BorderRadius::all(Val::Px(h * 0.04)),
                                    ..default()
                                },
                                BackgroundColor(LEATHER_DARK),
                                BorderColor::all(BRASS_DIM),
                            ))
                            .with_children(|track| {
                                track.spawn((
                                    Node {
                                        width: Val::Percent(f[i] * 100.0),
                                        height: Val::Percent(100.0),
                                        border_radius: BorderRadius::all(Val::Px(h * 0.03)),
                                        ..default()
                                    },
                                    BackgroundColor(color),
                                ));
                            });
                            row.spawn(text(format!("{:.0}", vals[i].0), h * 0.15, TEXT));
                        });
                    }
                });
        }
        SurfaceKind::Dialog => {
            let Some(d) = &hud.dialog else { return };
            let rows: Vec<usize> = d.rows().collect();
            let row_h = h / rows.len().max(1) as f32;
            parent
                .spawn((frame(px), BackgroundColor(LEATHER), BorderColor::all(BRASS)))
                .with_children(|p| {
                    for i in rows {
                        let sel = d.list.selected() == Some(i);
                        p.spawn((
                            Node {
                                flex_grow: 1.0,
                                align_items: AlignItems::Center,
                                padding: UiRect::horizontal(Val::Px(row_h * 0.3)),
                                border_radius: BorderRadius::all(Val::Px(row_h * 0.15)),
                                ..default()
                            },
                            BackgroundColor(if sel { BRASS } else { Color::NONE }),
                        ))
                        .with_children(|r| {
                            r.spawn(text(
                                format!("{}. {}", i + 1, d.options[i]),
                                row_h * 0.42,
                                if sel { INK } else { TEXT },
                            ));
                        });
                    }
                });
        }
        SurfaceKind::Subtitle => {
            let Some(s) = &hud.subtitle else { return };
            let line_h = h / (s.lines.len() as f32 * 1.35 + 1.0);
            parent
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(Val::Px(line_h * 0.4)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
                ))
                .with_children(|p| {
                    for (k, l) in s.lines.iter().enumerate() {
                        let mut line = l.clone();
                        if k == 0 {
                            match s.off_view {
                                Some(crate::subtitles::Side::Left) => line = format!("< {line}"),
                                Some(crate::subtitles::Side::Right) => line = format!("{line} >"),
                                None => {}
                            }
                        }
                        p.spawn(text(line, line_h, TEXT));
                    }
                });
        }
        SurfaceKind::Backpack => {
            let Some(bp) = &hud.backpack else { return };
            let grid_h = h * (ROWS as f32 * crate::backpack::CELL)
                / (ROWS as f32 * crate::backpack::CELL + STRIP);
            let cell = (w / COLS as f32).min(grid_h / ROWS as f32);
            let pages = Backpack::pages(&data.inventory.rows);
            parent
                .spawn((frame(px), BackgroundColor(LEATHER), BorderColor::all(BRASS)))
                .with_children(|p| {
                    p.spawn(Node {
                        height: Val::Px(grid_h * 0.92),
                        flex_wrap: FlexWrap::Wrap,
                        align_content: AlignContent::FlexStart,
                        ..default()
                    })
                    .with_children(|g| {
                        let items: Vec<_> = bp.visible(&data.inventory).collect();
                        for c in 0..COLS * ROWS {
                            let item = items.get(c).map(|(_, r)| *r);
                            let hovered = item.is_some_and(|r| {
                                bp.hovered == Some(crate::backpack::PanelHit::Item(r.key))
                            });
                            g.spawn((Node {
                                width: Val::Percent(100.0 / COLS as f32),
                                height: Val::Percent(100.0 / ROWS as f32),
                                padding: UiRect::all(Val::Px(cell * 0.05)),
                                ..default()
                            },))
                                .with_children(|slot| {
                                    slot.spawn((
                                        Node {
                                            width: Val::Percent(100.0),
                                            height: Val::Percent(100.0),
                                            border: UiRect::all(Val::Px(cell * 0.03)),
                                            border_radius: BorderRadius::all(Val::Px(cell * 0.12)),
                                            flex_direction: FlexDirection::Column,
                                            justify_content: JustifyContent::SpaceBetween,
                                            padding: UiRect::all(Val::Px(cell * 0.06)),
                                            ..default()
                                        },
                                        BackgroundColor(LEATHER_DARK),
                                        BorderColor::all(if hovered { BRASS } else { BRASS_DIM }),
                                    ))
                                    .with_children(|s| {
                                        if let Some(r) = item {
                                            let name = item_name(&r.item);
                                            s.spawn(text(
                                                name.clone(),
                                                fit(&name, cell * 0.15, cell * 0.8),
                                                TEXT,
                                            ));
                                            if r.count > 1 {
                                                s.spawn(text(
                                                    format!("x{}", r.count),
                                                    cell * 0.16,
                                                    BRASS,
                                                ));
                                            }
                                        }
                                    });
                                });
                        }
                    });
                    p.spawn(Node {
                        flex_grow: 1.0,
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(w * 0.04)),
                        ..default()
                    })
                    .with_children(|s| {
                        let size = h - grid_h;
                        s.spawn(text("<", size * 0.45, BRASS));
                        s.spawn(text(
                            format!(
                                "{} Gold   Seite {}/{}",
                                data.inventory.gold,
                                bp.page + 1,
                                pages
                            ),
                            size * 0.32,
                            TEXT,
                        ));
                        s.spawn(text(">", size * 0.45, BRASS));
                    });
                });
        }
        SurfaceKind::Wheel => {
            let n = hud.wheel.slots.max(1);
            let side = w;
            let r = side * WHEEL_RADIUS / ((WHEEL_RADIUS + 0.05) * 2.0);
            let slot = side * 0.16;
            parent.spawn(Node::default()).with_children(|p| {
                // Ring backdrop.
                p.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(side * 0.5 - r - slot * 0.6),
                        top: Val::Px(side * 0.5 - r - slot * 0.6),
                        width: Val::Px((r + slot * 0.6) * 2.0),
                        height: Val::Px((r + slot * 0.6) * 2.0),
                        border: UiRect::all(Val::Px(3.0)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.05, 0.04, 0.03, 0.55)),
                    BorderColor::all(BRASS_DIM),
                ));
                for i in 0..n {
                    let a = i as f32 / n as f32 * std::f32::consts::TAU;
                    let (x, y) = (side * 0.5 + a.sin() * r, side * 0.5 - a.cos() * r);
                    let filled = data.quick_slots.get(i).and_then(|s| s.as_ref());
                    let hov = hud.wheel.hovered == Some(i);
                    p.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(x - slot * 0.5),
                            top: Val::Px(y - slot * 0.5),
                            width: Val::Px(slot),
                            height: Val::Px(slot),
                            border: UiRect::all(Val::Px(slot * 0.06)),
                            border_radius: BorderRadius::MAX,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            flex_direction: FlexDirection::Column,
                            ..default()
                        },
                        BackgroundColor(if hov { BRASS } else { LEATHER }),
                        BorderColor::all(if filled.is_some() { BRASS } else { BRASS_DIM }),
                    ))
                    .with_children(|s| {
                        let key = (i + 1) % 10;
                        s.spawn(text(
                            key.to_string(),
                            slot * 0.22,
                            if hov { INK } else { TEXT_DIM },
                        ));
                        if let Some(q) = filled {
                            let name = item_name(&q.item);
                            s.spawn(text(
                                name.clone(),
                                fit(&name, slot * 0.13, slot * 0.82),
                                if hov { INK } else { TEXT },
                            ));
                        }
                    });
                }
            });
        }
        SurfaceKind::BookLeft | SurfaceKind::BookRight => {
            let Some((book, _)) = &hud.book else { return };
            let page = book.spread + usize::from(kind == SurfaceKind::BookRight);
            let line_h = h / (LINES_PER_PAGE as f32 + 3.0);
            parent
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(w * 0.08)),
                        border: UiRect::all(Val::Px(w * 0.012)),
                        ..default()
                    },
                    BackgroundColor(PARCHMENT),
                    BorderColor::all(Color::srgb(0.55, 0.42, 0.26)),
                ))
                .with_children(|p| {
                    if kind == SurfaceKind::BookLeft {
                        p.spawn(text("Tagebuch", line_h * 1.1, INK));
                    } else {
                        p.spawn(text(" ", line_h * 1.1, INK));
                    }
                    // LINE_CHARS must fit the inner width (about 0.55 em per glyph).
                    let size = (line_h * 0.62).min(w * 0.84 / (LINE_CHARS as f32 * 0.58));
                    for l in book.page(page) {
                        p.spawn(text(l.clone(), size, INK));
                    }
                    p.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    if page >= book.pages() {
                        return;
                    }
                    p.spawn(text(
                        format!("{} / {}", page + 1, book.pages()),
                        line_h * 0.55,
                        Color::srgb(0.45, 0.35, 0.22),
                    ));
                });
        }
        SurfaceKind::Map => {
            let Some((map, _)) = &hud.map else { return };
            parent
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        border: UiRect::all(Val::Px(w * 0.012)),
                        ..default()
                    },
                    BackgroundColor(PARCHMENT),
                    BorderColor::all(Color::srgb(0.55, 0.42, 0.26)),
                ))
                .with_children(|p| {
                    // Faint grid.
                    for k in 1..8 {
                        let f = k as f32 / 8.0 * 100.0;
                        for vertical in [true, false] {
                            p.spawn((
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: Val::Percent(if vertical { f } else { 0.0 }),
                                    top: Val::Percent(if vertical { 0.0 } else { f }),
                                    width: if vertical {
                                        Val::Px(1.5)
                                    } else {
                                        Val::Percent(100.0)
                                    },
                                    height: if vertical {
                                        Val::Percent(100.0)
                                    } else {
                                        Val::Px(1.5)
                                    },
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(0.45, 0.33, 0.2, 0.25)),
                            ));
                        }
                    }
                    p.spawn(text(map.sheet.name.clone(), w * 0.05, INK));
                    let dot = w * 0.025;
                    for (label, uv) in &map.markers {
                        let hl = hud.map_label.as_deref() == Some(label.as_str());
                        p.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(uv.x * w - dot * 0.5),
                                top: Val::Px(uv.y * h - dot * 0.5),
                                width: Val::Px(dot),
                                height: Val::Px(dot),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(INK),
                        ));
                        p.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(uv.x * w + dot),
                                top: Val::Px(uv.y * h - dot),
                                padding: UiRect::horizontal(Val::Px(dot * 0.3)),
                                ..default()
                            },
                            BackgroundColor(if hl { BRASS } else { Color::NONE }),
                        ))
                        .with_children(|t| {
                            t.spawn(text(label.clone(), w * (if hl { 0.05 } else { 0.04 }), INK));
                        });
                    }
                    if let Some(uv) = map.player {
                        let s = dot * 1.6;
                        p.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(uv.x * w - s * 0.5),
                                top: Val::Px(uv.y * h - s * 0.5),
                                width: Val::Px(s),
                                height: Val::Px(s),
                                border: UiRect::all(Val::Px(s * 0.2)),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(HEALTH),
                            BorderColor::all(PARCHMENT),
                        ));
                    }
                });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn update_surfaces(
    mut commands: Commands,
    hud: Res<VrHudRes>,
    data: Res<HudDataRes>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cameras: Query<&mut Camera>,
    mut surfaces: Query<(&mut Surface, &mut Transform, &mut Visibility)>,
) {
    for (mut s, mut t, mut vis) in &mut surfaces {
        let shown = placement(s.kind, &hud.0);
        let active = shown.is_some();
        if let Ok(mut cam) = cameras.get_mut(s.camera) {
            cam.is_active = active;
        }
        let Some((pose, size, alpha)) = shown else {
            *vis = Visibility::Hidden;
            if !s.key.is_empty() {
                s.key.clear();
                commands.entity(s.root).despawn_children();
            }
            continue;
        };
        *vis = Visibility::Visible;
        t.translation = pose.position;
        t.rotation = pose.rotation;
        t.scale = size.extend(1.0);
        if let Some(mut m) = materials.get_mut(&s.material) {
            m.base_color = Color::srgba(1.0, 1.0, 1.0, alpha);
        }
        let scale = (PX_PER_M).min(MAX_PX / size.x.max(size.y).max(1e-3));
        let px = (size * scale).max(Vec2::splat(16.0)).round().as_uvec2();
        let resize = px.x.abs_diff(s.px.x) > 8 || px.y.abs_diff(s.px.y) > 8;
        if resize && let Some(mut img) = images.get_mut(&s.image) {
            img.resize(Extent3d {
                width: px.x,
                height: px.y,
                depth_or_array_layers: 1,
            });
            s.px = px;
            s.key.clear();
        }
        let key = content_key(s.kind, &hud.0, &data.0);
        if key != s.key {
            s.key = key;
            let (kind, px) = (s.kind, s.px);
            commands
                .entity(s.root)
                .despawn_children()
                .with_children(|p| {
                    build(kind, &hud.0, &data.0, px, p);
                });
        }
    }
}
