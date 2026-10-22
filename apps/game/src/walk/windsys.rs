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
    bench: Res<crate::perf::BenchWorld>,
    wind_shot: Option<Res<super::WindShotControl>>,
    mut frame: ResMut<gmat::WindClockFrame>,
    mut clock: Local<gothic_environment::wind_clock::WindClock>,
    mut probe: Local<WindClockProbeState>,
    mut shot: Local<WindShotClockState>,
) {
    if !probe.checked {
        probe.checked = true;
        match wind_clock_probe_start() {
            Ok(start) => probe.start_seconds = start,
            Err(message) => eprintln!("wind-clock probe FAIL: {message}"),
        }
    }

    let dt = time.delta_secs_f64();
    let config = gmat::wind_env().clock;
    frame.probe = gmat::WindClockProbeFrame {
        finished: probe.finished,
        ..default()
    };

    if let Some(control) = wind_shot.as_deref()
        && let Some(index) = control.selected
    {
        if shot.applied_index != Some(index) {
            let elapsed = control.times[index];
            let previous_elapsed = super::wind_shots::previous_sample_time(elapsed);
            let mut synthetic = gothic_environment::wind_clock::WindClock::default();
            // Seed the prior render sample, then advance one 60 Hz step to the requested sample.
            // Only the two oscillator records are replaced; grass focus remains camera-driven.
            synthetic.advance(previous_elapsed, config);
            let values = synthetic.advance(elapsed - previous_elapsed, config);
            frame.values[..16].copy_from_slice(&values);
            shot.values = Some(values);
            *clock = synthetic;
            shot.applied_index = Some(index);
            eprintln!(
                "wind-shot selected: shot {} requested synthetic elapsed {:.9}s; current={:?}; previous={:?}",
                index + 1,
                elapsed,
                &values[..8],
                &values[8..16]
            );
        } else if let Some(values) = shot.values {
            frame.values[..16].copy_from_slice(&values);
        }
        return;
    }

    if let Some(start) = probe.start_seconds
        && !probe.finished
        && bench.camera_route_target_elapsed_seconds.is_some()
    {
        let boundary = if start < 10_000.0 { 3_600.0 } else { 86_400.0 };
        if !probe.started {
            // Explicit synthetic start is confined to the finite, warmed-up camera benchmark.
            // Reset startup accumulation, then seed one exact frame for previous-frame continuity.
            *clock = gothic_environment::wind_clock::WindClock::default();
            let values = clock.advance(start, config);
            frame.values[..values.len()].copy_from_slice(&values);
            frame.probe = gmat::WindClockProbeFrame {
                active: true,
                continuity_ok: true,
                finished: false,
                capture: gmat::WindClockProbeCapture::Seed,
                elapsed_seconds: clock.elapsed(),
                delta_seconds: 0.0,
            };
            probe.last_current = Some(values[..8].try_into().expect("eight wind record values"));
            probe.started = true;
            eprintln!(
                "wind-clock probe: synthetic-time GPU boundary test starts at {start:.1}s; normal Time/delta remains unchanged"
            );
            return;
        }

        let values = clock.advance(dt, config);
        frame.values[..values.len()].copy_from_slice(&values);
        let continuity_ok = probe
            .last_current
            .is_none_or(|previous| values[8..16] == previous);
        if !continuity_ok && !probe.continuity_failure_reported {
            eprintln!(
                "wind-clock probe FAIL: encoded previous sample differs from last current sample"
            );
            probe.continuity_failure_reported = true;
        }
        probe.last_current = Some(values[..8].try_into().expect("eight wind record values"));
        let elapsed = clock.elapsed();
        let capture = if !probe.before_captured && elapsed >= boundary - 0.25 && elapsed < boundary
        {
            probe.before_captured = true;
            gmat::WindClockProbeCapture::BeforeBoundary
        } else if elapsed >= boundary {
            probe.finished = true;
            gmat::WindClockProbeCapture::AfterBoundary
        } else {
            gmat::WindClockProbeCapture::None
        };
        frame.probe = gmat::WindClockProbeFrame {
            active: true,
            continuity_ok,
            finished: probe.finished,
            capture,
            elapsed_seconds: elapsed,
            delta_seconds: dt,
        };
        return;
    }

    let values = clock.advance(dt, config);
    frame.values[..values.len()].copy_from_slice(&values);
}

#[derive(Default)]
struct WindShotClockState {
    applied_index: Option<usize>,
    values: Option<[f32; 16]>,
}

#[derive(Default)]
struct WindClockProbeState {
    checked: bool,
    start_seconds: Option<f64>,
    started: bool,
    before_captured: bool,
    finished: bool,
    last_current: Option<[f32; 8]>,
    continuity_failure_reported: bool,
}

fn wind_clock_probe_start() -> Result<Option<f64>, String> {
    let Ok(value) = std::env::var("GR_WIND_CLOCK_PROBE_START") else {
        return Ok(None);
    };
    let args: Vec<String> = std::env::args().collect();
    if !args.iter().any(|arg| arg == "--bench-camera") {
        return Err("GR_WIND_CLOCK_PROBE_START is allowed only with finite --bench-camera".into());
    }
    let duration = args
        .windows(2)
        .find(|pair| pair[0] == "--bench")
        .and_then(|pair| pair[1].parse::<f64>().ok());
    if !duration.is_some_and(|seconds| seconds.is_finite() && seconds >= 4.0) {
        return Err(
            "GR_WIND_CLOCK_PROBE_START requires --bench of at least 4 finite seconds".into(),
        );
    }
    let start = value
        .parse::<f64>()
        .map_err(|_| "GR_WIND_CLOCK_PROBE_START must be 3599.0 or 86399.0".to_string())?;
    if start != 3_599.0 && start != 86_399.0 {
        return Err("GR_WIND_CLOCK_PROBE_START must be exactly 3599.0 or 86399.0".into());
    }
    Ok(Some(start))
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
