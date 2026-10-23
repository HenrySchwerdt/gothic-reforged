//! One-way gate from initial world loading to interactive play.
//!
//! Streaming, simulation and placement continue while hardware input is blocked. Keys pressed
//! during loading are discarded; play starts only after the initial cell, hero and script are
//! ready and all startup keys have been released.
use super::{HeroActor, PlayState};
use crate::walk::{Collision, Player, Stream, tile_chunk_id};
use bevy::prelude::*;
use gothic_runtime::CoreRes;
use gothic_sim::script::routine::state::State;

#[derive(Resource, Debug, Default)]
pub(crate) struct StartupReadiness {
    phase: Phase,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Loading,
    AwaitInputRelease,
    Playing,
    Failed(&'static str),
}

impl StartupReadiness {
    pub(crate) fn blocks_hardware(&self) -> bool {
        self.phase != Phase::Playing
    }

    pub(crate) fn is_playing(&self) -> bool {
        self.phase == Phase::Playing
    }
}

/// Install only for the play app. The walk viewer and deterministic camera/wind-shot probes keep
/// their existing startup and input behavior.
pub(super) fn register(app: &mut App, args: &[String]) {
    if args.iter().any(|arg| {
        matches!(arg.as_str(), "--bench-camera" | "--wind-shot-times" | "--fly")
    }) {
        return;
    }

    app.insert_resource(StartupReadiness::default())
        .add_systems(Startup, spawn_overlay)
        .add_systems(
            Update,
            update
                .after(crate::walk::player_motion)
                .before(crate::walk::update_camera),
        );
}

fn update(
    stream: Res<Stream>,
    collision: Res<Collision>,
    player: Res<Player>,
    play: Res<PlayState>,
    core: Res<CoreRes>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    hero_entities: Query<
        (),
        (
            With<HeroActor>,
            With<gothic_animation::runtime::Animator>,
            With<gothic_animation::runtime::CharacterComposition>,
        ),
    >,
    mut readiness: ResMut<StartupReadiness>,
    mut overlay: Query<(&mut Text, &mut Visibility), With<StartupOverlay>>,
) {
    if readiness.phase == Phase::Playing {
        return;
    }
    if matches!(readiness.phase, Phase::Failed(_)) {
        if let Ok((mut text, mut visibility)) = overlay.single_mut() {
            if let Phase::Failed(message) = readiness.phase {
                text.0 = message.into();
            }
            *visibility = Visibility::Inherited;
        }
        return;
    }

    let cell = stream.settings.cell_at(player.ctrl.position);
    let initial_cell_ready = collision.0.has_chunk(tile_chunk_id(cell));
    let visual_ready = play
        .hero_entity
        .is_some_and(|hero| hero_entities.get(hero).is_ok());
    let script_ready = core.0.sim.hero_script_state().1 == Some(State::PsNormalLoop);
    let ready = initial_cell_ready && player.placed && visual_ready && script_ready;

    // Startup failures have concrete causes: the loader has finished but the hero could not be
    // spawned, a loaded start cell has no ground, or the living hero was defeated before control
    // became available. Keep the input gate closed and show an honest status instead of waiting
    // forever or healing/pausing the simulation.
    let failure = if state_hero_down(&play, &core) {
        Some("Held ist kampfunfähig — Esc öffnet das Menü")
    } else if stream.load_error.is_some() {
        Some("Welt konnte nicht geladen werden — Esc öffnet das Menü")
    } else if stream.ready && play.hero_entity.is_none() {
        Some("Held konnte nicht geladen werden — Esc öffnet das Menü")
    } else if stream.ready && initial_cell_ready && !player.placed {
        Some("Startboden fehlt — Esc öffnet das Menü")
    } else if stream.ready && play.hero_entity.is_some() && !visual_ready {
        Some("Heldenmodell fehlt — Esc öffnet das Menü")
    } else {
        None
    };
    if let Some(message) = failure {
        readiness.phase = Phase::Failed(message);
    } else {
        readiness.phase = advance(readiness.phase, ready, inputs_released(&keyboard, &mouse));
    }

    if readiness.phase == Phase::Playing {
        eprintln!("play: initial world and hero ready; input enabled");
    }

    if let Ok((mut text, mut visibility)) = overlay.single_mut() {
        match readiness.phase {
            Phase::Loading => {
                text.0 = "Welt wird geladen …".into();
                *visibility = Visibility::Inherited;
            }
            Phase::AwaitInputRelease => {
                text.0 = "Welt bereit — Tasten loslassen".into();
                *visibility = Visibility::Inherited;
            }
            Phase::Playing => *visibility = Visibility::Hidden,
            Phase::Failed(message) => {
                text.0 = message.into();
                *visibility = Visibility::Inherited;
            }
        }
    }
}

fn state_hero_down(play: &PlayState, core: &CoreRes) -> bool {
    play.hero_down || core.0.sim.actor(core.0.hero).hp <= 0
}

fn inputs_released(keyboard: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>) -> bool {
    keyboard.get_pressed().is_empty() && mouse.get_pressed().is_empty()
}

fn advance(phase: Phase, ready: bool, released: bool) -> Phase {
    match phase {
        Phase::Loading if ready => Phase::AwaitInputRelease,
        Phase::AwaitInputRelease if released => Phase::Playing,
        Phase::Loading | Phase::AwaitInputRelease | Phase::Playing | Phase::Failed(_) => phase,
    }
}

/// A uniquely owned UI node lets the loading gate hide only its own notice. The shared
/// ScreenOutput/PrintScreenEnd channel also carries quest and game notifications.
#[derive(Component)]
struct StartupOverlay;

fn spawn_overlay(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(0.0),
            right: Val::Percent(0.0),
            top: px(24),
            justify_content: JustifyContent::Center,
            padding: UiRect::axes(px(12), px(8)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.72)),
        GlobalZIndex(10_001),
        Visibility::Hidden,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(18.0),
            ..default()
        },
        TextColor(Color::WHITE),
        StartupOverlay,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_input_remains_blocked_until_latched_play() {
        let mut readiness = StartupReadiness::default();
        assert!(readiness.blocks_hardware());
        readiness.phase = advance(readiness.phase, false, true);
        assert_eq!(readiness.phase, Phase::Loading);

        readiness.phase = advance(readiness.phase, true, false);
        assert_eq!(readiness.phase, Phase::AwaitInputRelease);
        assert!(readiness.blocks_hardware());

        readiness.phase = advance(readiness.phase, true, false);
        assert_eq!(readiness.phase, Phase::AwaitInputRelease);
        readiness.phase = advance(readiness.phase, false, true);
        assert_eq!(readiness.phase, Phase::Playing);
        assert!(!readiness.blocks_hardware());
        // Readiness is latched; later streaming work cannot pause ordinary play.
        readiness.phase = advance(readiness.phase, false, false);
        assert_eq!(readiness.phase, Phase::Playing);
        assert!(!readiness.blocks_hardware());
    }
}
