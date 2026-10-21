//! Orbit camera while locked on. The Gothic camera solver (`gothic-camera`) keeps owning the
//! boom, collision and presets; lock-on only steers its orbit angles toward the target through
//! `CameraState::set_orbit` and suppresses free mouse look while it is active.
use crate::math::{angle_diff, flat, yaw_of};
use glam::Vec3;
use gothic_camera::CameraState;

/// Seconds the orbit takes to cover most of a turn toward the target.
const FOLLOW_TAU: f32 = 0.12;
/// Elevation above the horizon for a level target (rad).
const BASE_ELEVATION: f32 = 0.34;

#[derive(Clone, Copy, Debug, Default)]
pub struct LockCam {
    /// A recenter swing (lock pressed with nothing to lock) is running.
    recenter: f32,
}

/// Yaw and elevation that put the target ahead of the hero.
pub fn orbit_for(hero: Vec3, target: Vec3) -> (f32, f32) {
    let d = target - hero;
    let yaw = yaw_of(flat(d));
    let dist = flat(d).length().max(1.0);
    // Look slightly down on close targets so both bodies stay in frame, up for distant ones.
    let elev =
        (BASE_ELEVATION + (d.y / dist) * -0.4 - (300.0 / dist).min(0.5) * 0.15).clamp(0.05, 0.6);
    (yaw, elev)
}

impl LockCam {
    pub fn recenter(&mut self) {
        self.recenter = 0.35;
    }

    /// Steers the camera for one frame. Call before `CameraState::update` with the look input
    /// zeroed. `hero_yaw` is used for the recenter swing.
    pub fn drive(
        &mut self,
        cam: &mut CameraState,
        hero: Vec3,
        target: Option<Vec3>,
        hero_yaw: f32,
        dt: f32,
    ) {
        let (cur_yaw, cur_elev) = (cam.view_yaw(), cam.orbit_degrees().1.to_radians());
        let k = 1.0 - (-dt / FOLLOW_TAU).exp();
        let goal = match target {
            Some(t) => Some(orbit_for(hero, t)),
            None if self.recenter > 0.0 => {
                self.recenter -= dt;
                Some((hero_yaw, 0.25))
            }
            None => None,
        };
        let Some((yaw, elev)) = goal else {
            return;
        };
        let new_yaw = cur_yaw + angle_diff(cur_yaw, yaw) * k;
        let new_elev = cur_elev + (elev - cur_elev) * k;
        cam.set_orbit(new_yaw, new_elev);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gothic_camera::{CameraConfig, PresetTable};

    #[test]
    fn orbit_points_from_hero_to_target() {
        let (yaw, _) = orbit_for(Vec3::ZERO, Vec3::new(0.0, 0.0, -500.0));
        assert!(yaw.abs() < 1e-4);
        let (yaw, _) = orbit_for(Vec3::ZERO, Vec3::new(-500.0, 0.0, 0.0));
        assert!((yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
    }

    #[test]
    fn camera_converges_on_the_target_direction() {
        let mut cam = CameraState::new(CameraConfig::default(), PresetTable::default());
        let mut lc = LockCam::default();
        let hero = Vec3::ZERO;
        let target = Vec3::new(-400.0, 0.0, -400.0);
        for _ in 0..120 {
            lc.drive(&mut cam, hero, Some(target), 0.0, 1.0 / 60.0);
        }
        let (want, _) = orbit_for(hero, target);
        assert!(angle_diff(cam.view_yaw(), want).abs() < 0.02);
    }
}
