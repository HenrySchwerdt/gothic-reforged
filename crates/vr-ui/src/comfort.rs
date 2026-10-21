//! Comfort rules for world-space UI, after Meta's VR UI guidance: sizes in *dmm*
//! (distance-independent millimetres: 1 dmm = 1 mm seen from 1 m, i.e. an angle), panels at
//! arm's length or a little further, content inside a comfortable head-turn cone and rather
//! below than above the eye line, panels upright and facing the eyes.
//!
//! The numbers are this crate's defaults, chosen inside the ranges that guidance gives; tune
//! them on a headset.
use glam::{Quat, Vec3};
use gothic_vr::Pose;
use std::ops::RangeInclusive;

/// Smallest text height, dmm.
pub const MIN_TEXT_DMM: f32 = 14.0;
/// Body text height, dmm.
pub const BODY_TEXT_DMM: f32 = 20.0;
/// Smallest pointer / touch target, dmm.
pub const MIN_TARGET_DMM: f32 = 22.0;
/// Panels the hands touch directly (inventory, quick slots), m from the eyes.
pub const TOUCH_DISTANCE: RangeInclusive<f32> = 0.45..=0.7;
/// Panels used with the laser (dialog options, subtitles), m from the eyes.
pub const POINT_DISTANCE: RangeInclusive<f32> = 0.9..=2.5;
/// Comfortable head turn left/right for content, radians.
pub const MAX_YAW: f32 = 30.0 * std::f32::consts::PI / 180.0;
/// Comfortable pitch range: a little above, more below the eye line, radians.
pub const MAX_PITCH_UP: f32 = 15.0 * std::f32::consts::PI / 180.0;
pub const MAX_PITCH_DOWN: f32 = 40.0 * std::f32::consts::PI / 180.0;

/// Size in metres of `dmm` seen from `distance` metres.
pub fn dmm(dmm: f32, distance: f32) -> f32 {
    dmm * 0.001 * distance
}

/// Size in dmm of `metres` seen from `distance`.
pub fn to_dmm(metres: f32, distance: f32) -> f32 {
    if distance <= 0.0 {
        f32::INFINITY
    } else {
        metres / distance * 1000.0
    }
}

/// Yaw (positive = left) and pitch (positive = up) of `p` relative to the head's view.
pub fn view_angles(head: &Pose, p: Vec3) -> (f32, f32) {
    let local = head.inverse().transform_point(p);
    let flat = (local.x * local.x + local.z * local.z).sqrt();
    ((-local.x).atan2(-local.z), local.y.atan2(flat))
}

/// Whether `p` is inside the comfortable viewing cone of `head`.
pub fn in_comfort_cone(head: &Pose, p: Vec3) -> bool {
    let (yaw, pitch) = view_angles(head, p);
    yaw.abs() <= MAX_YAW && (-MAX_PITCH_DOWN..=MAX_PITCH_UP).contains(&pitch)
}

/// Move `p` along the line from the eye so its distance is inside `range`.
pub fn clamp_distance(eye: Vec3, p: Vec3, range: &RangeInclusive<f32>) -> Vec3 {
    let d = p - eye;
    let len = d.length();
    if len < 1e-4 {
        return eye + Vec3::NEG_Z * *range.start();
    }
    eye + d / len * len.clamp(*range.start(), *range.end())
}

/// Upright rotation whose +Z faces `eye` from `pos` (yaw only, so text never rolls).
pub fn facing(pos: Vec3, eye: Vec3) -> Quat {
    let d = eye - pos;
    if d.x.abs() + d.z.abs() < 1e-5 {
        return Quat::IDENTITY;
    }
    Quat::from_rotation_y(d.x.atan2(d.z))
}

/// A point in front of the head at `distance`, `pitch` radians below the eye line, along the
/// head's yaw (ignores head roll and pitch, so the content does not swim with the head).
pub fn in_front(head: &Pose, distance: f32, pitch_down: f32) -> Vec3 {
    let yaw = head.yaw();
    let fwd = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    head.position + (fwd * pitch_down.cos() - Vec3::Y * pitch_down.sin()) * distance
}
