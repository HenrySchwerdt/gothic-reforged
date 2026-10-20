//! HUD: vitals bars, compass strip and notification toasts.
use crate::style::*;
use crate::*;

#[derive(Component)]
pub(crate) struct HudRoot;
#[derive(Component, Clone, Copy)]
pub(crate) enum HudBar {
    Hp,
    Mp,
    Sp,
}
#[derive(Component)]
pub(crate) struct ToastRoot;
#[derive(Component)]
pub(crate) struct Toast(Timer);
#[derive(Component)]
pub(crate) struct CompassMark(f32);

const TOAST_SECONDS: f32 = 4.5;
const MAX_TOASTS: usize = 6;
const COMPASS_W: f32 = 480.0;

pub(crate) fn spawn_hud(mut commands: Commands, assets: Res<UiAssets>) {
    let font = &assets.font;
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
            // vitals, bottom left
            r.spawn(Node {
                position_type: PositionType::Absolute,
                left: px(24),
                bottom: px(22),
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                ..default()
            })
            .with_children(|b| {
                for (kind, color) in [
                    (HudBar::Hp, Color::srgb(0.72, 0.14, 0.12)),
                    (HudBar::Mp, Color::srgb(0.20, 0.36, 0.78)),
                    (HudBar::Sp, Color::srgb(0.78, 0.66, 0.22)),
                ] {
                    b.spawn((
                        Node {
                            width: px(260),
                            height: px(16),
                            border: UiRect::all(px(2)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
                        BorderColor::all(GOLD_DIM),
                    ))
                    .with_child((
                        kind,
                        Node {
                            width: percent(100),
                            height: percent(100),
                            ..default()
                        },
                        BackgroundColor(color),
                    ));
                }
            });
            // compass, top center
            r.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: px(14),
                    left: percent(50),
                    margin: UiRect::left(px(-COMPASS_W / 2.0)),
                    width: px(COMPASS_W),
                    height: px(34),
                    border: UiRect::axes(px(0), px(2)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
                BorderColor::all(GOLD_DIM),
            ))
            .with_children(|c| {
                for (i, label) in ["N", "NO", "O", "SO", "S", "SW", "W", "NW"]
                    .iter()
                    .enumerate()
                {
                    let major = i % 2 == 0;
                    c.spawn((
                        CompassMark(i as f32 * 45.0),
                        Node {
                            position_type: PositionType::Absolute,
                            top: px(if major { 2 } else { 6 }),
                            left: px(-100),
                            width: px(40),
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        children![txt(
                            font,
                            *label,
                            if major { 22.0 } else { 16.0 },
                            if i == 0 {
                                RED
                            } else if major {
                                GOLD
                            } else {
                                DIM
                            },
                        )],
                    ));
                }
                c.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(50),
                        bottom: px(0),
                        width: px(2),
                        height: px(8),
                        ..default()
                    },
                    BackgroundColor(PARCH),
                ));
            });
        });
    // toasts stay visible during dialogs, so they live outside the HUD root
    commands.spawn((
        ToastRoot,
        GlobalZIndex(20),
        Node {
            position_type: PositionType::Absolute,
            right: px(24),
            bottom: px(30),
            width: px(360),
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            align_items: AlignItems::FlexEnd,
            ..default()
        },
    ));
}

pub(crate) fn update_hud(
    ui: Res<UiState>,
    vitals: Res<PlayerVitals>,
    heading: Res<PlayerHeading>,
    mut root: Query<&mut Visibility, With<HudRoot>>,
    mut bars: Query<(&HudBar, &mut Node), Without<CompassMark>>,
    mut marks: Query<(&CompassMark, &mut Node), Without<HudBar>>,
) {
    for mut v in &mut root {
        *v = if ui.screen != Screen::None {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
    for (b, mut n) in &mut bars {
        let (cur, max) = match b {
            HudBar::Hp => (vitals.hp, vitals.hp_max),
            HudBar::Mp => (vitals.mp, vitals.mp_max),
            HudBar::Sp => (vitals.sp, vitals.sp_max),
        };
        n.width = percent(if max > 0.0 {
            (cur / max).clamp(0.0, 1.0) * 100.0
        } else {
            0.0
        });
    }
    let yaw = heading.0.to_degrees();
    for (m, mut n) in &mut marks {
        let delta = (m.0 - yaw + 540.0).rem_euclid(360.0) - 180.0;
        let x = COMPASS_W / 2.0 + delta * 3.0 - 20.0;
        n.left = px(if delta.abs() < 85.0 { x } else { -200.0 });
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
        let color = match n.kind {
            ToastKind::Info => PARCH,
            ToastKind::Quest => GOLD,
            ToastKind::Xp => GREEN,
            ToastKind::Item => Color::srgb(0.75, 0.85, 0.95),
        };
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
                Node {
                    padding: UiRect::axes(px(14), px(7)),
                    border: UiRect::left(px(3)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.03, 0.03, 0.025, 0.88)),
                BorderColor::all(color),
                children![txt(&assets.font, n.text.clone(), 20.0, color)],
            ))
            .id();
        commands.entity(root).add_child(t);
    }
}
