//! Interaction prompt (name of the NPC in front of the camera) and the control hint.
use super::{Mode, PlayState, Roster};
use bevy::prelude::*;
use gothic_runtime::CoreRes;
use gothic_ui::{UiAssets, UiState};

#[derive(Component)]
struct Prompt;

pub fn register(app: &mut App) {
    app.add_systems(Startup, spawn).add_systems(Update, update);
}

fn spawn(mut commands: Commands, assets: Option<Res<UiAssets>>) {
    let font = assets.map(|a| a.font.clone());
    let mk = |size: f32| TextFont {
        font: font.clone().map(Into::into).unwrap_or_default(),
        font_size: FontSize::Px(size),
        ..default()
    };
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            bottom: px(120),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|p| {
            p.spawn((
                Prompt,
                Text::new(""),
                mk(26.0),
                TextColor(Color::srgb(0.95, 0.88, 0.62)),
                TextShadow::default(),
            ));
        });
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(14),
            top: px(10),
            ..default()
        },
        Text::new("WASD Bewegen   Maus Blick   E Sprechen   1/0 Schwert   LMT Angriff   L Tagebuch   I Inventar   C Charakter   F5/F9 Speichern/Laden"),
        mk(15.0),
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.55)),
    ));
}

fn update(
    state: Res<PlayState>,
    core: Res<CoreRes>,
    roster: Res<Roster>,
    ui: Res<UiState>,
    mut q: Query<&mut Text, With<Prompt>>,
) {
    let Ok(mut t) = q.single_mut() else { return };
    let want = match state.focus {
        Some(id) if state.mode == Mode::Explore && !ui.is_open() && !state.drawn => {
            let name = &core.0.sim.actor(id).name;
            let shown = roster.display.get(name).unwrap_or(name);
            format!("{shown}\n[E] Sprechen")
        }
        _ => String::new(),
    };
    if t.0 != want {
        t.0 = want;
    }
}
