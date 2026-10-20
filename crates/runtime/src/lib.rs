//! L2 bridge: owns the [`Core`] inside a Bevy app, ticks it at a fixed 30 Hz and carries
//! [`Command`]s in / [`Event`]s out as Bevy messages. Contains no rendering or UI.
mod core;
mod world;

pub use core::{Command, Core, Event, HERO_REACH, TICK_HZ};
pub use world::PlayWorld;

use bevy::prelude::*;

/// The authoritative game core (a resource; only [`CorePlugin`]'s systems tick it).
#[derive(Resource)]
pub struct CoreRes(pub Core);

/// Host -> core.
#[derive(Message, Debug, Clone)]
pub struct CoreCommand(pub Command);

/// Core -> host (presentation, audio, UI).
#[derive(Message, Debug, Clone)]
pub struct CoreEvent(pub Event);

/// Where the host's quick save (F5) / quick load (F9) commands point.
#[derive(Resource, Clone)]
pub struct SavePath(pub std::path::PathBuf);

impl Default for SavePath {
    fn default() -> Self {
        Self(".local/saves/quick.json".into())
    }
}

/// Insert a [`CoreRes`] before adding this plugin.
pub struct CorePlugin;

impl Plugin for CorePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<CoreCommand>()
            .add_message::<CoreEvent>()
            .init_resource::<SavePath>()
            .insert_resource(Time::<Fixed>::from_hz(f64::from(TICK_HZ)))
            .add_systems(FixedUpdate, tick_core);
    }
}

fn tick_core(
    mut core: ResMut<CoreRes>,
    mut inbox: MessageReader<CoreCommand>,
    mut outbox: MessageWriter<CoreEvent>,
    time: Res<Time<Fixed>>,
) {
    let commands: Vec<Command> = inbox.read().map(|c| c.0.clone()).collect();
    for e in core.0.tick(time.delta_secs(), &commands) {
        outbox.write(CoreEvent(e));
    }
}
