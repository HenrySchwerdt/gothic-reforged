//! Tick the game core at its own rate in `Update`.
//!
//! `gothic_runtime::CorePlugin` ticks the core in `FixedUpdate` and sets `Time<Fixed>` to
//! 30 Hz, while avian and the VR hands, melee and archery run their fixed steps at the
//! headset rate (90 Hz). One app cannot have both fixed rates, so a VR app uses this plugin
//! instead: same messages (`CoreCommand` in, `CoreEvent` out), same 30 Hz core steps, driven
//! by an accumulator over the frame time.
use bevy::prelude::*;
use gothic_runtime::{Command, CoreCommand, CoreEvent, CoreRes, SavePath, TICK_HZ};

/// Most core steps per frame (a long hitch does not make the core race).
const MAX_STEPS: u32 = 5;

#[derive(Resource, Default)]
struct Accumulator {
    secs: f32,
    queued: Vec<Command>,
}

pub struct CoreTickPlugin;

impl Plugin for CoreTickPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<CoreCommand>()
            .add_message::<CoreEvent>()
            .init_resource::<SavePath>()
            .init_resource::<Accumulator>()
            .add_systems(
                Update,
                tick_core
                    .run_if(resource_exists::<CoreRes>)
                    .after(gothic_vr::VrSet::Sync),
            );
    }
}

fn tick_core(
    time: Res<Time>,
    mut acc: ResMut<Accumulator>,
    mut core: ResMut<CoreRes>,
    mut inbox: MessageReader<CoreCommand>,
    mut outbox: MessageWriter<CoreEvent>,
) {
    let step = 1.0 / TICK_HZ;
    acc.queued.extend(inbox.read().map(|c| c.0.clone()));
    acc.secs = (acc.secs + time.delta_secs()).min(step * MAX_STEPS as f32);
    while acc.secs >= step {
        acc.secs -= step;
        let commands = std::mem::take(&mut acc.queued);
        for e in core.0.tick(step, &commands) {
            outbox.write(CoreEvent(e));
        }
    }
}
