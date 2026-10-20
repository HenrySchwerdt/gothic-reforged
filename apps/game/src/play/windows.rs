//! The loot / plunder / recipe list of the play mode.
//!
//! PLACEHOLDER: the original shows these as HUD pages built from `g3.gui.res` (loot page with
//! item icons and "Nimm alles"); here the entries are a plain text list so the ported player
//! states (`PS_Interact_Chest`, `PS_Interact_Anvil`, ...) can be used end to end. The texts that
//! exist in the string table (`HUD_TakeAll`, `HUD_NoIngredients`, ...) come from it.
use super::interact::Interaction;
use bevy::prelude::*;
use gothic_ui::UiAssets;

#[derive(Component)]
struct WindowRoot;

pub fn register(app: &mut App) {
    app.add_systems(Update, sync_window);
}

fn sync_window(
    mut commands: Commands,
    mut inter: ResMut<Interaction>,
    roots: Query<Entity, With<WindowRoot>>,
    assets: Option<Res<UiAssets>>,
) {
    let Some(win) = inter.window.as_mut() else {
        for e in &roots {
            commands.entity(e).despawn();
        }
        return;
    };
    if !win.dirty {
        return;
    }
    win.dirty = false;
    for e in &roots {
        commands.entity(e).despawn();
    }
    let font = assets.map(|a| a.font.clone());
    let text_font = |size: f32| TextFont {
        font: font.clone().map(Into::into).unwrap_or_default(),
        font_size: FontSize::Px(size),
        ..default()
    };
    let title = win.title.clone();
    let footer = win.footer.clone();
    let lines: Vec<String> = win
        .lines
        .iter()
        .enumerate()
        .map(|(i, l)| format!("{}  {}", i + 1, l.text))
        .collect();
    commands
        .spawn((
            WindowRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            ZIndex(20),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(22)),
                    row_gap: px(6),
                    min_width: px(420),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.04, 0.03, 0.88)),
            ))
            .with_children(|p| {
                p.spawn((
                    Text::new(title),
                    text_font(30.0),
                    TextColor(Color::srgb(0.95, 0.85, 0.55)),
                ));
                if lines.is_empty() {
                    p.spawn((
                        Text::new("-"),
                        text_font(22.0),
                        TextColor(Color::srgb(0.8, 0.8, 0.8)),
                    ));
                }
                for l in lines {
                    p.spawn((
                        Text::new(l),
                        text_font(22.0),
                        TextColor(Color::srgb(0.92, 0.9, 0.85)),
                    ));
                }
                p.spawn((
                    Text::new(footer),
                    text_font(16.0),
                    TextColor(Color::srgba(1.0, 1.0, 1.0, 0.55)),
                ));
            });
        });
}
