//! Wrist vitals: health, mana and stamina bars on the inside of the off-hand wrist, shown
//! when the player turns the wrist towards the eyes (the HUD's `bars` flag still applies).
use crate::comfort;
use crate::panel::Panel;
use glam::{Quat, Vec2, Vec3};
use gothic_vr::{Hand, Pose};
use serde::{Deserialize, Serialize};

/// Current and maximum of the three bars.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Vitals {
    pub health: (f32, f32),
    pub mana: (f32, f32),
    pub stamina: (f32, f32),
}

impl Vitals {
    /// Bar fills 0..1 in the order health, mana, stamina.
    pub fn fractions(&self) -> [f32; 3] {
        [self.health, self.mana, self.stamina].map(|(cur, max)| {
            if max > 0.0 && cur.is_finite() {
                (cur / max).clamp(0.0, 1.0)
            } else {
                0.0
            }
        })
    }
}

/// Size of the wrist panel, m (three 1 cm bars with gaps).
pub const WRIST_SIZE: Vec2 = Vec2::new(0.08, 0.04);
/// The panel shows when its face points at the eyes within this angle.
pub const SHOW_ANGLE: f32 = 40.0 * std::f32::consts::PI / 180.0;
/// Health below this fraction pulses.
pub const LOW_HEALTH: f32 = 0.25;

/// The panel on the inside of `hand`'s wrist (controller grip pose: -Z forward along the
/// fingers, +X to the right). The face points away from the palm side of the forearm.
pub fn wrist_panel(grip: &Pose, hand: Hand) -> Panel {
    // Inside of the wrist: towards the thumb side, a little behind the grip.
    let local = Pose::new(
        Vec3::new(-hand.sign() * 0.035, 0.0, 0.07),
        Quat::from_rotation_y(-hand.sign() * std::f32::consts::FRAC_PI_2)
            * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
    );
    Panel::new(grip.mul(&local), WRIST_SIZE)
}

/// Whether the wrist is turned to the eyes and close enough to read.
pub fn wrist_visible(panel: &Panel, eye: Vec3) -> bool {
    let to_eye = eye - panel.pose.position;
    let d = to_eye.length();
    d > 0.1
        && d < 0.8
        && panel.normal().angle_between(to_eye) <= SHOW_ANGLE
        && comfort::to_dmm(WRIST_SIZE.y / 3.0, d) >= comfort::MIN_TEXT_DMM
}

/// Brightness of the health bar for the low-health pulse at time `t` (1 = steady).
pub fn health_pulse(health: f32, t: f32) -> f32 {
    if health < LOW_HEALTH {
        0.6 + 0.4 * (t * std::f32::consts::TAU * 1.5).cos().abs()
    } else {
        1.0
    }
}
