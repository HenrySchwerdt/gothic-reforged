//! Wind systems: the grass response (`Render.GrassResponseRange/Intensity` of `ge3.INI`) uses the
//! hero position from the shared clock buffer; bounded shared phases feed the GPU wind shader.
use super::{CM, Player, gmat};
use bevy::prelude::*;

pub struct WindPlugin;

impl Plugin for WindPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, advance_wind_clock);
        app.add_systems(
            Update,
            grass_focus.run_if(|bench: Res<crate::perf::BenchWorld>| {
                bench.camera_route_target_elapsed_seconds.is_none()
            }),
        );
        app.add_systems(
            Update,
            grass_focus.after(super::WalkUpdateSet).run_if(
                |bench: Res<crate::perf::BenchWorld>| {
                    bench.camera_route_target_elapsed_seconds.is_some()
                },
            ),
        );
    }
}

fn advance_wind_clock(
    time: Res<Time>,
    mut frame: ResMut<gmat::WindClockFrame>,
    mut clock: Local<gothic_environment::wind_clock::WindClock>,
) {
    let values = clock.advance(time.delta_secs_f64(), gmat::wind_env().clock);
    frame.values[..values.len()].copy_from_slice(&values);
}

/// Write the shared grass-response focus into the clock buffer.
fn grass_focus(
    player: Res<Player>,
    mut frame: ResMut<gmat::WindClockFrame>,
    mut last: Local<Option<Vec3>>,
) {
    if std::env::var_os("GR_NOGRASSFOCUS").is_some() {
        return;
    }
    let pos = super::observer_cm(&player) * CM;
    if last.is_some_and(|l| l.distance_squared(pos) < 0.02 * 0.02) {
        return;
    }
    *last = Some(pos);
    frame.values[16..20].copy_from_slice(&pos.extend(1.0).to_array());
    if std::env::var_os("GR_WINDLOG").is_some() {
        eprintln!(
            "grass focus update {pos:?}, {} wind materials",
            gmat::wind_materials().len()
        );
    }
}
