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

/// Closed-loop yaw controller for finite camera gestures (QA/replay tools).
///
/// It converts a desired angular velocity to mouse counts using the active camera gain, then
/// checks the actual yaw and its measured velocity. The camera's normal input smoothing remains
/// in charge of motion; this type only supplies bounded feedback.
#[derive(Debug, Clone)]
pub struct ScriptedYawTurn {
    target_yaw: f32,
    previous_yaw: f32,
    settled_secs: f32,
}

impl ScriptedYawTurn {
    /// Capture one target relative to the yaw at the start of a gesture.
    pub fn new(start_yaw: f32, yaw_delta: f32) -> Self {
        Self {
            target_yaw: wrap_yaw(start_yaw + yaw_delta),
            previous_yaw: wrap_yaw(start_yaw),
            settled_secs: 0.0,
        }
    }

    /// Return mouse counts for this frame and whether yaw/error stayed settled for 150 ms.
    /// `gain` is the active camera's radians-per-count value after sensitivity and preset scaling.
    pub fn update(&mut self, yaw: f32, dt: f32, gain: f32) -> (f32, bool) {
        let dt = dt.clamp(0.0, 0.1);
        if dt <= 0.0 || gain <= 0.0 {
            return (0.0, false);
        }

        let yaw = wrap_yaw(yaw);
        let error = angle_delta(yaw, self.target_yaw);
        let velocity = angle_delta(self.previous_yaw, yaw) / dt;
        self.previous_yaw = yaw;

        if error.abs() <= 0.5_f32.to_radians() && velocity.abs() <= 2.0_f32.to_radians() {
            self.settled_secs += dt;
        } else {
            self.settled_secs = 0.0;
        }
        if self.settled_secs >= 0.15 {
            return (0.0, true);
        }

        // A damped proportional velocity request counters residual camera coast. The speed cap
        // keeps large scripted turns finite; counts scale with dt for frame-rate independence.
        let desired_velocity = (4.0 * error - 1.4 * velocity).clamp(-1.5, 1.5);
        (-desired_velocity * dt / gain, false)
    }
}

fn wrap_yaw(yaw: f32) -> f32 {
    (yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

fn angle_delta(from: f32, to: f32) -> f32 {
    wrap_yaw(to - from)
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
        mode: CameraMode,
    ) -> (f32, f32) {
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
        cam.set_orbit(start_deg.to_radians(), 0.0);
        let start = cam.view_yaw();
        let target = wrap_yaw(start + delta_deg.to_radians());
        let mut turn = ScriptedYawTurn::new(start, delta_deg.to_radians());
        let mut settled_at = None;
        let mut elapsed = 0.0;
        let mut counts = 0.0;

        for frame in 0..2400 {
            let dt = dts[frame % dts.len()];
            cam.update(
                &CameraInput {
                    dt,
                    hero,
                    look: glam::Vec2::new(counts, 0.0),
                    ..CameraInput::default()
                },
                &NoWorld,
            );
            elapsed += dt;
            let yaw = cam.view_yaw();
            let (next_counts, settled) = turn.update(yaw, dt, cam.yaw_input_gain());
            if settled {
                settled_at = Some(elapsed);
                break;
            }
            counts = next_counts;
        }

        let settled_at = settled_at.expect("scripted camera turn settles within 4 seconds");
        assert!(settled_at <= 4.0, "settled after {settled_at}s");
        let final_yaw = cam.view_yaw();
        assert!(
            angle_delta(final_yaw, target).abs() <= 0.5_f32.to_radians(),
            "final error {} degrees",
            angle_delta(final_yaw, target).to_degrees()
        );

        // Match QA's existing one-second wait after the turn, with no further input.
        let mut velocity = 0.0;
        let mut waited = 0.0;
        let mut wait_frame = 0;
        while waited < 1.0 {
            let dt = dts[wait_frame % dts.len()];
            let previous = cam.view_yaw();
            cam.update(
                &CameraInput {
                    dt,
                    hero,
                    ..CameraInput::default()
                },
                &NoWorld,
            );
            velocity = angle_delta(previous, cam.view_yaw()) / dt;
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
        assert_eq!(cam.config, config, "gesture must preserve camera settings");
        (cam.view_yaw(), velocity)
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
                let (yaw, _) = run_scripted_turn(dts, 0.0, 100.0, mode);
                assert!((angle_delta(0.0, yaw).to_degrees() - 100.0_f32).abs() < 0.5);
            }
        }
    }

    #[test]
    fn scripted_yaw_turn_handles_opposite_direction_and_angle_wrap() {
        let schedules = [vec![1.0 / 30.0], vec![0.013, 0.027, 0.021, 0.019]];
        for dts in &schedules {
            run_scripted_turn(dts, 170.0, 30.0, CameraMode::Magic);
            run_scripted_turn(dts, -170.0, -35.0, CameraMode::Magic);
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
