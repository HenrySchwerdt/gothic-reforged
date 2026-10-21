//! Stereo rig math (Bevy-free): per-eye poses from the head pose and the interpupillary
//! distance, and the viewport split used by the desktop stereo preview.
//!
//! With a headset the OpenXR runtime reports eye poses and asymmetric FOVs itself and the
//! renderer draws both views into a multiview swapchain; the desktop fallback reproduces the
//! same two-camera layout side by side so the stereo path is exercised without hardware.
use crate::input::Pose;
use glam::{UVec2, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Eye {
    Left,
    Right,
}

impl Eye {
    pub const BOTH: [Eye; 2] = [Eye::Left, Eye::Right];

    pub fn sign(self) -> f32 {
        match self {
            Eye::Left => -1.0,
            Eye::Right => 1.0,
        }
    }
}

/// How the desktop window shows the rig.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StereoMode {
    /// One camera at the head (comfortable to debug).
    #[default]
    Mono,
    /// Left and right eye side by side.
    SideBySide,
}

/// Average adult IPD in metres.
pub const DEFAULT_IPD: f32 = 0.063;

/// Eye pose: half the IPD to the side of the head pose, same orientation.
pub fn eye_pose(head: &Pose, eye: Eye, ipd: f32) -> Pose {
    let ipd = if ipd.is_finite() {
        ipd.clamp(0.0, 0.1)
    } else {
        DEFAULT_IPD
    };
    Pose::new(
        head.transform_point(Vec3::X * (eye.sign() * ipd * 0.5)),
        head.rotation,
    )
}

/// Viewport (position, size) of an eye in a window of `size` physical pixels.
pub fn eye_viewport(mode: StereoMode, eye: Eye, size: UVec2) -> (UVec2, UVec2) {
    match mode {
        StereoMode::Mono => (UVec2::ZERO, size.max(UVec2::ONE)),
        StereoMode::SideBySide => {
            let half = (size.x / 2).max(1);
            let x = if eye == Eye::Left { 0 } else { half };
            (UVec2::new(x, 0), UVec2::new(half, size.y.max(1)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    #[test]
    fn eyes_are_ipd_apart_along_head_right() {
        let head = Pose::new(Vec3::new(0.0, 1.7, 0.0), Quat::from_rotation_y(1.0));
        let l = eye_pose(&head, Eye::Left, DEFAULT_IPD);
        let r = eye_pose(&head, Eye::Right, DEFAULT_IPD);
        assert!((l.position.distance(r.position) - DEFAULT_IPD).abs() < 1e-6);
        assert!((r.position - l.position).normalize().dot(head.right()) > 0.999);
        let bad = eye_pose(&head, Eye::Left, f32::NAN);
        assert!(bad.position.is_finite());
    }

    #[test]
    fn side_by_side_splits_the_window() {
        let size = UVec2::new(1920, 1080);
        assert_eq!(
            eye_viewport(StereoMode::Mono, Eye::Right, size),
            (UVec2::ZERO, size)
        );
        let (lp, ls) = eye_viewport(StereoMode::SideBySide, Eye::Left, size);
        let (rp, rs) = eye_viewport(StereoMode::SideBySide, Eye::Right, size);
        assert_eq!((lp.x, ls.x, rp.x, rs.x), (0, 960, 960, 960));
        let (_, tiny) = eye_viewport(StereoMode::SideBySide, Eye::Left, UVec2::ZERO);
        assert!(tiny.x >= 1 && tiny.y >= 1);
    }
}
