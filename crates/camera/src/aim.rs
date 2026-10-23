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
