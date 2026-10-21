//! Heading maths in the Gothic convention shared with `gothic-physics` and `gothic-camera`:
//! yaw 0 faces -Z and a positive yaw turns toward -X, `dir_of(yaw) = (-sin, 0, -cos)`.
//! Lengths are centimetres, Y is up.
use glam::Vec3;
use std::f32::consts::{PI, TAU};

pub fn dir_of(yaw: f32) -> Vec3 {
    Vec3::new(-yaw.sin(), 0.0, -yaw.cos())
}

pub fn yaw_of(dir: Vec3) -> f32 {
    (-dir.x).atan2(-dir.z)
}

/// Unit vector to the right of a heading.
pub fn right_of(yaw: f32) -> Vec3 {
    Vec3::new(yaw.cos(), 0.0, -yaw.sin())
}

pub fn angle_diff(from: f32, to: f32) -> f32 {
    (to - from + PI).rem_euclid(TAU) - PI
}

pub fn turn_toward(yaw: f32, goal: f32, max_step: f32) -> f32 {
    yaw + angle_diff(yaw, goal).clamp(-max_step, max_step)
}

pub fn approach(value: f32, goal: f32, max_step: f32) -> f32 {
    value + (goal - value).clamp(-max_step, max_step)
}

/// Ground-plane vector (Y dropped).
pub fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

pub fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaw_round_trips_and_matches_the_controller_convention() {
        for yaw in [-3.0, -1.0, 0.0, 0.7, 2.5] {
            assert!((angle_diff(yaw, yaw_of(dir_of(yaw)))).abs() < 1e-5);
        }
        assert!((dir_of(0.0) - Vec3::NEG_Z).length() < 1e-6);
        assert!((right_of(0.0) - Vec3::X).length() < 1e-6);
    }
}
