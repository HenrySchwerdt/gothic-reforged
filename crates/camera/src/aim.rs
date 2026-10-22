//! Camera behaviour of the aiming states: the point under the crosshair and the fade of the
//! hero in the special bow mode.
//!
//! The script asks the camera for the point under the screen centre
//! (`PSCamera::GetScreenCenterPickPoint`) in `OnGetAimAngle` (S:83300, the pitch of the aim
//! animation and the crosshair size) and in `Shoot` (S:86450, the direction of the hero's
//! arrow). `PS_Ranged` (S:837e0) switches the camera to the ranged mode and enables the special
//! bow mode ([`crate::CameraState::enable_special_bow_mode`]).
use glam::{Quat, Vec3};

use crate::math::ease;
use crate::world::CameraWorld;

/// Closed-loop two-axis controller for finite camera gestures (QA/replay tools).
///
/// It converts desired angular velocities to mouse counts using active camera gains, then checks
/// actual yaw and pitch plus their measured velocities. The camera's normal input smoothing
/// remains in charge of motion; this type only supplies bounded feedback.
#[derive(Debug, Clone)]
pub struct ScriptedCameraTurn {
    target_yaw: f32,
    target_pitch: f32,
    previous_yaw: f32,
    previous_pitch: f32,
    settled_secs: f32,
}

impl ScriptedCameraTurn {
    /// Capture yaw relative to its initial value and an absolute pitch target.
    pub fn new(start_yaw: f32, yaw_delta: f32, start_pitch: f32, target_pitch: f32) -> Self {
        Self {
            target_yaw: wrap_yaw(start_yaw + yaw_delta),
            target_pitch,
            previous_yaw: wrap_yaw(start_yaw),
            previous_pitch: start_pitch,
            settled_secs: 0.0,
        }
    }

    /// Return X/Y mouse counts and whether both axes stayed settled for 150 ms.
    /// Gains are the active camera's radians-per-count after sensitivity/preset scaling.
    pub fn update(
        &mut self,
        yaw: f32,
        pitch: f32,
        dt: f32,
        yaw_gain: f32,
        pitch_gain: f32,
    ) -> (glam::Vec2, bool) {
        let dt = dt.clamp(0.0, 0.1);
        if dt <= 0.0 || yaw_gain <= 0.0 || pitch_gain <= 0.0 {
            return (glam::Vec2::ZERO, false);
        }

        let yaw = wrap_yaw(yaw);
        let error = angle_delta(yaw, self.target_yaw);
        let yaw_velocity = angle_delta(self.previous_yaw, yaw) / dt;
        let pitch_error = self.target_pitch - pitch;
        let pitch_velocity = (pitch - self.previous_pitch) / dt;
        self.previous_yaw = yaw;
        self.previous_pitch = pitch;

        let settled_limit = 0.5_f32.to_radians();
        let velocity_limit = 2.0_f32.to_radians();
        if error.abs() <= settled_limit
            && yaw_velocity.abs() <= velocity_limit
            && pitch_error.abs() <= settled_limit
            && pitch_velocity.abs() <= velocity_limit
        {
            self.settled_secs += dt;
        } else {
            self.settled_secs = 0.0;
        }
        if self.settled_secs >= 0.15 {
            return (glam::Vec2::ZERO, true);
        }

        // A damped proportional velocity request counters residual camera coast. The speed cap
        // keeps large scripted turns finite; counts scale with dt for frame-rate independence.
        let desired_yaw_velocity = (4.0 * error - 1.4 * yaw_velocity).clamp(-1.5, 1.5);
        let desired_pitch_velocity = (4.0 * pitch_error - 1.4 * pitch_velocity).clamp(-1.5, 1.5);
        let yaw_counts = -desired_yaw_velocity * dt / yaw_gain;
        // Mouse Y turns the player pitch opposite the camera elevation, hence the same sign as X.
        let pitch_counts = -desired_pitch_velocity * dt / pitch_gain;
        (glam::Vec2::new(yaw_counts, pitch_counts), false)
    }
}

fn wrap_yaw(yaw: f32) -> f32 {
    (yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

fn angle_delta(from: f32, to: f32) -> f32 {
    wrap_yaw(to - from)
}

/// Error-diffuses fractional scripted motion into integer mouse counts.
///
/// The game input boundary rounds accumulated mouse motion to whole counts each frame. Keeping
/// the residual here lets small QA corrections accumulate without changing that boundary.
#[derive(Debug, Clone, Copy, Default)]
pub struct MouseCountAccumulator {
    residual: glam::Vec2,
}

impl MouseCountAccumulator {
    /// Convert requested X/Y counts to integer-valued floats suitable for `MouseMotion`.
    pub fn quantize(&mut self, requested: glam::Vec2) -> glam::Vec2 {
        let total = requested + self.residual;
        let rounded = total.round();
        self.residual = total - rounded;
        rounded
    }
}

/// How far the pick ray reaches when nothing is hit, cm [I]: the far clipping plane of the
/// highest view distance option (`DistanceHigh` 10000).
pub const PICK_RANGE: f32 = 10_000.0;
/// The special bow mode fades the hero out over this many seconds (`gCCameraAI_PS::OnProcess`,
/// `DAT_102bc518`).
pub const SPECIAL_BOW_FADE_SECS: f32 = 0.5;

/// The result of a pick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pick {
    /// The point under the crosshair.
    pub point: Vec3,
    /// Distance from the camera, cm.
    pub distance: f32,
    /// The ray ended on geometry (not at the maximum range).
    pub hit: bool,
}

/// Port of `PSCamera::GetScreenCenterPickPoint`: the first static contact of the ray through
/// the screen centre, or the point `PICK_RANGE` away. `world.ray_nearest` answers the static
/// contact; characters on the ray are the host's business (the crosshair target of the hero's
/// focus).
pub fn pick_point(position: Vec3, rotation: Quat, world: &dyn CameraWorld) -> Pick {
    gothic_portcov::hit!("pick_point");
    let dir = rotation * Vec3::NEG_Z;
    match world.ray_nearest(position, dir, PICK_RANGE) {
        Some(d) => Pick {
            point: position + dir * d,
            distance: d,
            hit: true,
        },
        None => Pick {
            point: position + dir * PICK_RANGE,
            distance: PICK_RANGE,
            hit: false,
        },
    }
}

/// The alpha of the hero while the special bow mode runs (`OnProcess`): `ease(1 - 2 t)` with the
/// time in the mode capped at [`SPECIAL_BOW_FADE_SECS`]; zero after half a second.
pub fn special_bow_alpha(elapsed: f32) -> f32 {
    ease(1.0 - 2.0 * elapsed.clamp(0.0, SPECIAL_BOW_FADE_SECS))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CameraConfig;
    use crate::preset::{CameraMode, PresetTable};
    use crate::state::{CameraInput, CameraState, HeroView};
    use crate::world::NoWorld;

    fn run_scripted_turn(
        dts: &[f32],
        start_deg: f32,
        delta_deg: f32,
        start_pitch: f32,
        mode: CameraMode,
    ) -> (f32, f32, f32, f32) {
        let config = CameraConfig::default();
        let mut cam = CameraState::new(config.clone(), PresetTable::default());
        let hero = HeroView::default();
        cam.set_mode(mode, false);
        cam.update(
            &CameraInput {
                dt: 1.0 / 60.0,
                hero,
                ..CameraInput::default()
            },
            &NoWorld,
        );
        cam.set_orbit(start_deg.to_radians(), -start_pitch);
        let seed_dt = dts[0];
        cam.update(
            &CameraInput {
                dt: seed_dt,
                hero,
                look: glam::Vec2::new(-3.0, 15.0),
                ..CameraInput::default()
            },
            &NoWorld,
        );
        let start = cam.view_yaw();
        let target = wrap_yaw(start + delta_deg.to_radians());
        let start_pitch = -cam.orbit_degrees().1.to_radians();
        let mut turn = ScriptedCameraTurn::new(start, delta_deg.to_radians(), start_pitch, -0.05);
        let mut quantizer = MouseCountAccumulator::default();
        let mut settled_at = None;
        let mut elapsed = 0.0;
        let mut counts = glam::Vec2::ZERO;
        let mut last_errors = (f32::NAN, f32::NAN);

        for frame in 0..2400 {
            let dt = dts[frame % dts.len()];
            // Match controls::gather_input: accumulated MouseMotion is rounded to i32 counts.
            let raw_counts = [counts.x.round() as i32, counts.y.round() as i32];
            cam.update(
                &CameraInput {
                    dt,
                    hero,
                    look: glam::Vec2::new(raw_counts[0] as f32, raw_counts[1] as f32),
                    ..CameraInput::default()
                },
                &NoWorld,
            );
            elapsed += dt;
            let yaw = cam.view_yaw();
            let pitch = -cam.orbit_degrees().1.to_radians();
            let (next_counts, settled) =
                turn.update(yaw, pitch, dt, cam.yaw_input_gain(), cam.pitch_input_gain());
            last_errors = (angle_delta(yaw, target), -0.05 - pitch);
            let next_counts = quantizer.quantize(next_counts);
            if settled {
                counts = next_counts;
                settled_at = Some(elapsed);
                break;
            }
            counts = next_counts;
        }

        let settled_at = settled_at.unwrap_or_else(|| {
            panic!(
                "scripted camera turn did not settle within test budget; elapsed={elapsed:.3}s yaw_error_deg={:.3} pitch_error_deg={:.3}",
                last_errors.0.to_degrees(),
                last_errors.1.to_degrees()
            )
        });
        assert!(settled_at <= 4.0, "settled after {settled_at}s");
        let final_yaw = cam.view_yaw();
        assert!(
            angle_delta(final_yaw, target).abs() <= 0.5_f32.to_radians(),
            "final error {} degrees",
            angle_delta(final_yaw, target).to_degrees()
        );

        // Match QA's existing one-second wait after the turn, with no further input.
        let mut velocity = 0.0;
        let mut pitch_velocity = 0.0;
        let mut waited = 0.0;
        let mut wait_frame = 0;
        while waited < 1.0 {
            let dt = dts[wait_frame % dts.len()];
            let previous = cam.view_yaw();
            let previous_pitch = -cam.orbit_degrees().1.to_radians();
            let raw_counts = [counts.x.round() as i32, counts.y.round() as i32];
            cam.update(
                &CameraInput {
                    dt,
                    hero,
                    look: glam::Vec2::new(raw_counts[0] as f32, raw_counts[1] as f32),
                    ..CameraInput::default()
                },
                &NoWorld,
            );
            counts = glam::Vec2::ZERO;
            velocity = angle_delta(previous, cam.view_yaw()) / dt;
            pitch_velocity = (-cam.orbit_degrees().1.to_radians() - previous_pitch) / dt;
            waited += dt;
            wait_frame += 1;
        }
        assert!(
            angle_delta(cam.view_yaw(), target).abs() <= 0.5_f32.to_radians(),
            "one-second wait drifted {} degrees",
            angle_delta(cam.view_yaw(), target).to_degrees()
        );
        assert!(
            velocity.abs() <= 2.0_f32.to_radians(),
            "wait velocity {velocity}"
        );
        assert!(
            pitch_velocity.abs() <= 2.0_f32.to_radians(),
            "wait pitch velocity {pitch_velocity}"
        );
        let final_pitch = -cam.orbit_degrees().1.to_radians();
        assert!(
            (final_pitch + 0.05).abs() <= 0.5_f32.to_radians(),
            "pitch missed old QA target by {} degrees",
            (final_pitch + 0.05).to_degrees()
        );
        assert_eq!(cam.config, config, "gesture must preserve camera settings");
        (cam.view_yaw(), velocity, final_pitch, pitch_velocity)
    }

    #[test]
    fn scripted_yaw_turn_is_settled_and_frame_rate_stable() {
        let schedules = [
            vec![1.0 / 30.0],
            vec![1.0 / 60.0],
            vec![1.0 / 120.0],
            vec![0.009, 0.031, 0.017, 0.026, 0.013, 0.024],
        ];
        for mode in [CameraMode::Normal, CameraMode::Magic] {
            for dts in &schedules {
                let (yaw, _, _, _) = run_scripted_turn(dts, 0.0, 100.0, -0.7, mode);
                assert!((angle_delta(0.0, yaw).to_degrees() - 100.0_f32).abs() < 0.5);
            }
        }
    }

    #[test]
    fn scripted_yaw_turn_handles_opposite_direction_and_angle_wrap() {
        let schedules = [vec![1.0 / 30.0], vec![0.013, 0.027, 0.021, 0.019]];
        for dts in &schedules {
            run_scripted_turn(dts, 170.0, 30.0, -0.7, CameraMode::Magic);
            run_scripted_turn(dts, -170.0, -35.0, -0.7, CameraMode::Magic);
        }
    }

    #[test]
    fn zero_yaw_turn_waits_for_offset_pitch_and_residual_motion() {
        let schedules = [
            vec![1.0 / 30.0],
            vec![1.0 / 60.0],
            vec![1.0 / 120.0],
            vec![0.009, 0.031, 0.017, 0.026, 0.013, 0.024],
        ];
        for dts in &schedules {
            run_scripted_turn(dts, 0.0, 0.0, -0.7, CameraMode::Magic);
        }
    }

    struct Wall(f32);
    impl CameraWorld for Wall {
        fn ray_nearest(&self, from: Vec3, dir: Vec3, len: f32) -> Option<f32> {
            // A wall at z = -wall.
            if dir.z >= 0.0 {
                return None;
            }
            let t = (from.z + self.0) / -dir.z;
            (t >= 0.0 && t <= len).then_some(t)
        }
    }

    #[test]
    fn the_pick_point_is_the_first_wall_or_the_far_point() {
        let rot = Quat::IDENTITY;
        let near = pick_point(Vec3::ZERO, rot, &Wall(1500.0));
        assert!(near.hit && (near.distance - 1500.0).abs() < 1e-3);
        assert!((near.point - Vec3::new(0.0, 0.0, -1500.0)).length() < 1e-3);
        let far = pick_point(Vec3::ZERO, rot, &NoWorld);
        assert!(!far.hit && far.distance == PICK_RANGE);
    }

    #[test]
    fn the_hero_fades_in_half_a_second() {
        assert_eq!(special_bow_alpha(0.0), 1.0);
        assert!((special_bow_alpha(0.25) - 0.5).abs() < 1e-5);
        assert_eq!(special_bow_alpha(0.5), 0.0);
        assert_eq!(special_bow_alpha(3.0), 0.0);
    }

    #[test]
    fn the_camera_fades_the_body_and_keeps_the_weapon_while_aiming() {
        let mut cam = CameraState::new(CameraConfig::default(), PresetTable::default());
        let hero = HeroView {
            position: Vec3::ZERO,
            box_height: 180.0,
            ranged: true,
            ..HeroView::default()
        };
        let input = |dt| CameraInput {
            dt,
            hero,
            ..CameraInput::default()
        };
        cam.set_mode(CameraMode::Ranged, false);
        let before = cam.update(&input(0.05), &NoWorld);
        assert!(!before.items_visible);
        cam.enable_special_bow_mode(true);
        let mut last = before;
        for _ in 0..4 {
            last = cam.update(&input(0.05), &NoWorld);
        }
        assert!(
            last.hero_alpha < 0.9 && last.hero_alpha > 0.0,
            "fading: {}",
            last.hero_alpha
        );
        assert!(last.items_visible, "the bow stays visible");
        for _ in 0..20 {
            last = cam.update(&input(0.05), &NoWorld);
        }
        assert_eq!(last.hero_alpha, 0.0);
        cam.enable_special_bow_mode(false);
        for _ in 0..40 {
            last = cam.update(&input(0.05), &NoWorld);
        }
        assert!(last.hero_alpha > 0.0, "the body is back once the aim ends");
    }
}
