//! HUD as in the original: round compass (bottom left), 10-slot quick bar (bottom centre),
//! three framed bars (bottom right: health, endurance, mana), focus bar (top centre) and the
//! plain "game messages" text in the screen middle. Positions are the ones in `g3.gui.res`.
use crate::atlas::*;
use crate::style::*;
use crate::*;

#[derive(Component)]
pub(crate) struct HudRoot;
#[derive(Component, Clone, Copy)]
pub(crate) enum HudBar {
    Hp,
    Sp,
    Mp,
    Focus,
}
#[derive(Component)]
pub(crate) struct SlotsRoot;
#[derive(Component)]
pub(crate) struct FocusRoot;
#[derive(Component)]
pub(crate) struct Rose;
#[derive(Component)]
pub(crate) struct ToastRoot;
#[derive(Component)]
pub(crate) struct Toast(Timer);

const TOAST_SECONDS: f32 = 4.5;
const MAX_TOASTS: usize = 6;
const BAR_W: f32 = 201.0;
const BAR_H: f32 = 19.0;

/// Item bound to a quick slot (key 1..0).
#[derive(Clone, Debug)]
pub struct QuickSlot {
    pub item: String,
    pub count: u32,
}

/// The ten quick slots shown in the bottom bar; the host fills it.
#[derive(Resource, Clone, Debug, Default)]
pub struct QuickSlots(pub Vec<Option<QuickSlot>>);

/// Health fraction (0..=1) of the creature in focus; `None` hides the top bar.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct FocusHealth(pub Option<f32>);

fn bar_fill(color: Color, kind: HudBar) -> impl Bundle {
    (
        kind,
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            top: px(4),
            width: px(176),
            height: px(12),
            ..default()
        },
        BackgroundColor(color),
    )
}

pub(crate) fn spawn_hud(mut commands: Commands, assets: Res<UiAssets>) {
    let a = assets.clone();
    commands
        .spawn((
            HudRoot,
            GlobalZIndex(5),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
        ))
        .with_children(|r| {
            // compass: brass wheel with the star rose turning in it
            let mut n = abs(0.0, 0.0, 100.0, 100.0);
            n.left = px(16);
            n.top = Val::Auto;
            n.bottom = px(8);
            r.spawn(n).with_children(|c| {
                spawn_pic(c, &a, COMPASS_WHEEL, 8.0, 8.0, Some((84.0, 84.0)));
                let mut rose = spawn_pic(c, &a, COMPASS_ROSE, 20.0, 20.0, Some((60.0, 60.0)));
                rose.insert(Rose);
            });
            // quick bar
            let mut n = abs(0.0, 0.0, 520.0, 56.0);
            n.left = percent(50);
            n.margin = UiRect::left(px(-260));
            n.top = Val::Auto;
            n.bottom = px(24);
            r.spawn(n).with_children(|c| {
                spawn_pic(c, &a, QUICKSLOTS, 0.0, 0.0, Some((520.0, 56.0)));
                c.spawn((SlotsRoot, abs(0.0, 0.0, 520.0, 56.0)));
            });
            // bars
            for (i, (kind, color)) in [
                (HudBar::Hp, Color::srgb(0.74, 0.07, 0.07)),
                (HudBar::Sp, Color::srgb(0.76, 0.62, 0.18)),
                (HudBar::Mp, Color::srgb(0.07, 0.46, 0.70)),
            ]
            .into_iter()
            .enumerate()
            {
                let mut n = abs(0.0, 0.0, BAR_W, BAR_H);
                n.left = Val::Auto;
                n.right = px(16);
                n.top = Val::Auto;
                n.bottom = px(61.0 - 20.0 * i as f32);
                r.spawn(n).with_children(|c| {
                    spawn_pic(c, &a, STATUSBAR, 0.0, 0.0, None);
                    c.spawn(bar_fill(color, kind));
                });
            }
            // focus bar
            let mut n = abs(0.0, 0.0, BAR_W, BAR_H);
            n.left = percent(50);
            n.margin = UiRect::left(px(-BAR_W / 2.0));
            n.top = px(10);
            r.spawn((FocusRoot, n, Visibility::Hidden))
                .with_children(|c| {
                    spawn_pic(c, &a, STATUSBAR, 0.0, 0.0, None);
                    c.spawn(bar_fill(Color::srgb(0.74, 0.07, 0.07), HudBar::Focus));
                });
        });
    // game messages (plain text, centre of the screen); they stay visible during dialogs
    commands.spawn((
        ToastRoot,
        GlobalZIndex(20),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            width: percent(100),
            top: px(308),
            flex_direction: FlexDirection::Column,
            row_gap: px(2),
            align_items: AlignItems::Center,
            ..default()
        },
    ));
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update_hud(
    mut commands: Commands,
    ui: Res<UiState>,
    vitals: Res<PlayerVitals>,
    heading: Res<PlayerHeading>,
    focus: Res<FocusHealth>,
    slots: Res<QuickSlots>,
    session: Option<Res<GameSession>>,
    assets: Res<UiAssets>,
    mut root: Query<&mut Visibility, (With<HudRoot>, Without<FocusRoot>)>,
    mut bars: Query<(&HudBar, &mut Node), Without<SlotsRoot>>,
    mut rose: Query<&mut UiTransform, With<Rose>>,
    mut focus_root: Query<&mut Visibility, With<FocusRoot>>,
    slot_root: Query<(Entity, Option<&Children>), With<SlotsRoot>>,
    mut last_slots: Local<u32>,
) {
    // the full-width dialog / trade panels replace the HUD, every other page keeps it
    for mut v in &mut root {
        *v = if matches!(ui.screen, Screen::Dialog | Screen::Trade) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
    for (b, mut n) in &mut bars {
        let f = match b {
            HudBar::Hp => frac(vitals.hp, vitals.hp_max),
            HudBar::Mp => frac(vitals.mp, vitals.mp_max),
            HudBar::Sp => frac(vitals.sp, vitals.sp_max),
            HudBar::Focus => focus.0.unwrap_or(0.0).clamp(0.0, 1.0),
        };
        n.width = px(176.0 * f);
    }
    for mut v in &mut focus_root {
        *v = if focus.0.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    for mut t in &mut rose {
        t.rotation = Rot2::radians(-heading.0);
    }
    // rebuild the slot row when the slots change (or on the first frames, once the root exists)
    let need = slots.is_changed() || *last_slots < 3;
    *last_slots += 1;
    if !need {
        return;
    }
    for (root, kids) in &slot_root {
        for k in kids.into_iter().flatten() {
            commands.entity(*k).despawn();
        }
        commands.entity(root).with_children(|c| {
            for i in 0..10 {
                let x = 22.0 + 48.0 * i as f32;
                let slot = slots.0.get(i).and_then(Option::as_ref);
                let mut cell = abs(x + 2.0, 7.0, 44.0, 44.0);
                cell.border = UiRect::all(px(1));
                c.spawn((
                    cell,
                    BackgroundColor(Color::srgba(0.24, 0.17, 0.12, 0.92)),
                    BorderColor::all(Color::srgba(0.55, 0.45, 0.30, 0.8)),
                ));
                if let Some(s) = slot {
                    let cat = session
                        .as_ref()
                        .and_then(|g| g.items.get(&s.item))
                        .map_or(6, |i| i.category);
                    let g = glyph(cat);
                    spawn_pic(
                        c,
                        &assets,
                        g,
                        x + (48.0 - g.w) / 2.0,
                        5.0 + (48.0 - g.h) / 2.0,
                        None,
                    );
                }
                label(
                    c,
                    &assets.font,
                    if i == 9 {
                        "10".to_string()
                    } else {
                        format!("{}", i + 1)
                    },
                    (x + 3.0, 6.0, 20.0, 12.0),
                    11.0,
                    TEXT,
                    Al::Left,
                );
                if let Some(s) = slot.filter(|s| s.count > 1) {
                    label(
                        c,
                        &assets.font,
                        s.count.to_string(),
                        (x + 14.0, 38.0, 30.0, 12.0),
                        11.0,
                        TEXT,
                        Al::Right,
                    );
                }
            }
        });
    }
}

fn frac(cur: f32, max: f32) -> f32 {
    if max > 0.0 {
        (cur / max).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Item category -> glyph of the icon atlas (the per-item icon cells are not indexed in gui.pak).
pub(crate) fn glyph(category: i32) -> Pic {
    match category {
        1 => GLYPH_WEAPON,
        2 => GLYPH_ARMOR,
        4 => GLYPH_POTION,
        7 => GLYPH_SCROLL,
        9 => GLYPH_BOOK,
        3 => GLYPH_ANVIL,
        _ => GLYPH_MISC,
    }
}

pub(crate) fn toasts(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<UiAssets>,
    mut msgs: MessageReader<Notify>,
    root: Query<Entity, With<ToastRoot>>,
    mut live: Query<(Entity, &mut Toast)>,
) {
    for (e, mut t) in &mut live {
        if t.0.tick(time.delta()).is_finished() {
            commands.entity(e).despawn();
        }
    }
    let Ok(root) = root.single() else {
        return;
    };
    let mut count = live.iter().count();
    for n in msgs.read() {
        if count >= MAX_TOASTS {
            if let Some((e, _)) = live.iter().next() {
                commands.entity(e).despawn();
            }
            count -= 1;
        }
        count += 1;
        let t = commands
            .spawn((
                Toast(Timer::from_seconds(TOAST_SECONDS, TimerMode::Once)),
                Node::default(),
                children![(
                    Text::new(n.text.clone()),
                    TextFont {
                        font: assets.font.clone().into(),
                        font_size: FontSize::Px(18.0),
                        ..default()
                    },
                    TextColor(TEXT),
                    TextShadow::default(),
                )],
            ))
            .id();
        commands.entity(root).add_child(t);
    }
}
