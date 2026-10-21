//! Desktop fallback: keyboard and mouse emulate a headset and two controllers, so every VR
//! system can be developed, debugged and run in CI without hardware.
//!
//! The emulator is Bevy-free: [`DesktopInput`] is a plain snapshot of the keys/mouse of one
//! frame (filled by the Bevy plugin or by a script) and [`DesktopEmulator::frame`] turns it
//! into an [`XrFrame`]. Default bindings (see `docs/vr/README.md`):
//!
//! | input | emulates |
//! | --- | --- |
//! | W/A/S/D | left thumbstick (locomotion) |
//! | Q/E | right thumbstick X (turn) |
//! | T | right thumbstick forward (teleport aim) |
//! | right mouse + move | head yaw/pitch |
//! | mouse move | active hand in the view plane (swings!) |
//! | mouse wheel | active hand depth |
//! | Tab | switch active hand |
//! | left mouse | trigger of the active hand |
//! | G | toggle grip of the active hand |
//! | F | toggle grip of the other hand |
//! | Space / R | right A / B |
//! | Left Shift | left stick click (sprint) |
//! | C | crouch (lowers the head) |
//! | Esc | menu |
use crate::input::{ControllerState, Hand, Pose, XrFrame};
use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// Keyboard/mouse snapshot of one frame. Mouse deltas are in pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct DesktopInput {
    /// x = right, y = forward, each -1..1.
    pub move_axes: Vec2,
    /// -1 (turn left) .. 1 (turn right).
    pub turn_axis: f32,
    pub teleport: bool,
    /// Mouse motion while the look button is held.
    pub look_delta: Vec2,
    /// Mouse motion while the look button is not held (moves the active hand).
    pub hand_delta: Vec2,
    pub wheel: f32,
    pub trigger: bool,
    pub toggle_grip: bool,
    pub toggle_other_grip: bool,
    pub switch_hand: bool,
    pub primary: bool,
    pub secondary: bool,
    pub sprint: bool,
    pub crouch: bool,
    pub menu: bool,
}

/// Tunables of the emulator.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DesktopSettings {
    /// Radians per pixel.
    pub look_sensitivity: f32,
    /// Metres per pixel.
    pub hand_sensitivity: f32,
    /// Metres per wheel notch.
    pub wheel_step: f32,
    pub eye_height: f32,
    pub crouch_eye_height: f32,
    /// Metres per second the head moves when crouching/standing.
    pub crouch_speed: f32,
}

impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            look_sensitivity: 0.003,
            hand_sensitivity: 0.0015,
            wheel_step: 0.05,
            eye_height: 1.7,
            crouch_eye_height: 1.05,
            crouch_speed: 2.0,
        }
    }
}

/// Reach box of an emulated hand relative to the head, in the head-yaw frame.
const HAND_MIN: Vec3 = Vec3::new(-0.75, -0.9, -0.85);
const HAND_MAX: Vec3 = Vec3::new(0.75, 0.45, -0.15);

/// Emulated headset and controllers.
#[derive(Debug, Clone, PartialEq)]
pub struct DesktopEmulator {
    pub settings: DesktopSettings,
    pub yaw: f32,
    pub pitch: f32,
    pub eye_height: f32,
    pub active: Hand,
    /// Hand positions relative to the head in the head-yaw frame.
    pub hand_offsets: [Vec3; 2],
    pub grip: [bool; 2],
    previous: Option<XrFrame>,
}

impl Default for DesktopEmulator {
    fn default() -> Self {
        Self::new(DesktopSettings::default())
    }
}

impl DesktopEmulator {
    pub fn new(settings: DesktopSettings) -> Self {
        Self {
            settings,
            yaw: 0.0,
            pitch: 0.0,
            eye_height: settings.eye_height,
            active: Hand::Right,
            hand_offsets: [Self::rest(Hand::Left), Self::rest(Hand::Right)],
            grip: [false; 2],
            previous: None,
        }
    }

    /// Resting position of a hand relative to the head.
    pub fn rest(hand: Hand) -> Vec3 {
        Vec3::new(0.22 * hand.sign(), -0.4, -0.4)
    }

    /// Advance by one frame.
    pub fn frame(&mut self, input: &DesktopInput, dt: f32) -> XrFrame {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        let s = self.settings;
        if input.switch_hand {
            self.active = self.active.other();
        }
        if input.toggle_grip {
            self.grip[self.active.index()] ^= true;
        }
        if input.toggle_other_grip {
            self.grip[self.active.other().index()] ^= true;
        }
        if input.look_delta.is_finite() {
            self.yaw -= input.look_delta.x * s.look_sensitivity;
            self.pitch = (self.pitch - input.look_delta.y * s.look_sensitivity).clamp(-1.4, 1.4);
        }
        let target = if input.crouch {
            s.crouch_eye_height
        } else {
            s.eye_height
        };
        let step = s.crouch_speed * dt;
        self.eye_height += (target - self.eye_height).clamp(-step, step);

        let a = self.active.index();
        if input.hand_delta.is_finite() && input.wheel.is_finite() {
            let d = Vec3::new(
                input.hand_delta.x * s.hand_sensitivity,
                -input.hand_delta.y * s.hand_sensitivity,
                -input.wheel * s.wheel_step,
            );
            self.hand_offsets[a] = (self.hand_offsets[a] + d).clamp(HAND_MIN, HAND_MAX);
        }

        let yaw_rot = Quat::from_rotation_y(self.yaw);
        let head = Pose::new(
            Vec3::new(0.0, self.eye_height, 0.0),
            yaw_rot * Quat::from_rotation_x(self.pitch),
        );
        let mut frame = XrFrame {
            head,
            hands: [ControllerState::default(); 2],
        };
        for hand in Hand::BOTH {
            let off = self.hand_offsets[hand.index()];
            // Raised hands tilt the "blade" up, lowered hands point it forward/down, and a hand
            // pushed to the side rolls a little: a mouse swing then sweeps a weapon's edge.
            let tilt = ((off.y + 0.4) * 1.6).clamp(-0.6, 1.2);
            let roll = -off.x * 0.8;
            let rotation = yaw_rot * Quat::from_rotation_x(tilt) * Quat::from_rotation_z(roll);
            let c = frame.hand_mut(hand);
            c.pose = Some(Pose::new(head.position + yaw_rot * off, rotation));
            c.grip = if self.grip[hand.index()] { 1.0 } else { 0.0 };
            if hand == self.active {
                c.trigger = if input.trigger { 1.0 } else { 0.0 };
            }
        }
        let left = frame.hand_mut(Hand::Left);
        left.thumbstick = clamp_axes(input.move_axes);
        left.stick_click = input.sprint;
        left.menu = input.menu;
        let right = frame.hand_mut(Hand::Right);
        right.thumbstick = Vec2::new(
            input.turn_axis.clamp(-1.0, 1.0),
            if input.teleport { 1.0 } else { 0.0 },
        );
        right.primary = input.primary;
        right.secondary = input.secondary;
        if dt > 0.0
            && let Some(prev) = self.previous
        {
            for hand in Hand::BOTH {
                if let (Some(a), Some(b)) = (frame.hand(hand).pose, prev.hand(hand).pose) {
                    frame.hand_mut(hand).linear_velocity = Some((a.position - b.position) / dt);
                }
            }
        }
        self.previous = Some(frame);
        frame
    }
}

fn clamp_axes(v: Vec2) -> Vec2 {
    if !v.is_finite() {
        return Vec2::ZERO;
    }
    if v.length_squared() > 1.0 {
        v.normalize()
    } else {
        v
    }
}

/// A list of `(frames, input)` steps replayed one frame at a time (CI / headless demos).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InputScript {
    pub steps: Vec<(u32, DesktopInput)>,
    #[serde(skip)]
    cursor: (usize, u32),
}

impl InputScript {
    pub fn new(steps: Vec<(u32, DesktopInput)>) -> Self {
        Self {
            steps,
            cursor: (0, 0),
        }
    }

    /// The input of the next frame, `None` when the script is over.
    pub fn next_input(&mut self) -> Option<DesktopInput> {
        loop {
            let (step, done) = &mut self.cursor;
            let (count, input) = self.steps.get(*step)?;
            if *done < *count {
                *done += 1;
                return Some(*input);
            }
            *step += 1;
            *done = 0;
        }
    }

    pub fn total_frames(&self) -> u32 {
        self.steps.iter().map(|(n, _)| n).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 90.0;

    #[test]
    fn hands_follow_head_yaw() {
        let mut e = DesktopEmulator::default();
        let f0 = e.frame(&DesktopInput::default(), DT);
        let r0 = f0.hand(Hand::Right).pose.unwrap().position;
        assert!(r0.x > 0.0 && r0.z < 0.0);
        // Turn the head 90 degrees to the left: the right hand is now in front (-X side).
        let px = std::f32::consts::FRAC_PI_2 / e.settings.look_sensitivity;
        let f1 = e.frame(
            &DesktopInput {
                look_delta: Vec2::new(-px, 0.0),
                ..Default::default()
            },
            DT,
        );
        let r1 = f1.hand(Hand::Right).pose.unwrap().position;
        assert!(r1.x < 0.0, "{r1}");
        assert!((f1.head.yaw() - std::f32::consts::FRAC_PI_2).abs() < 1e-3);
    }

    #[test]
    fn mouse_moves_only_the_active_hand_and_reports_velocity() {
        let mut e = DesktopEmulator::default();
        let f0 = e.frame(&DesktopInput::default(), DT);
        let f1 = e.frame(
            &DesktopInput {
                hand_delta: Vec2::new(100.0, 0.0),
                ..Default::default()
            },
            DT,
        );
        let dr = f1.hand(Hand::Right).pose.unwrap().position
            - f0.hand(Hand::Right).pose.unwrap().position;
        assert!((dr.x - 0.15).abs() < 1e-4);
        assert_eq!(
            f1.hand(Hand::Left).pose.unwrap().position,
            f0.hand(Hand::Left).pose.unwrap().position
        );
        let v = f1.hand(Hand::Right).linear_velocity.unwrap();
        assert!((v.x - 0.15 / DT).abs() < 1e-2);
    }

    #[test]
    fn hand_stays_in_reach() {
        let mut e = DesktopEmulator::default();
        let f = e.frame(
            &DesktopInput {
                hand_delta: Vec2::new(1e6, -1e6),
                wheel: -1e3,
                ..Default::default()
            },
            DT,
        );
        let off = f.hand(Hand::Right).pose.unwrap().position - f.head.position;
        assert!(off.cmple(HAND_MAX + 1e-4).all() && off.cmpge(HAND_MIN - 1e-4).all());
    }

    #[test]
    fn grip_toggle_switch_and_crouch() {
        let mut e = DesktopEmulator::default();
        let f = e.frame(
            &DesktopInput {
                toggle_grip: true,
                trigger: true,
                ..Default::default()
            },
            DT,
        );
        assert_eq!(f.hand(Hand::Right).grip, 1.0);
        assert_eq!(f.hand(Hand::Right).trigger, 1.0);
        assert_eq!(f.hand(Hand::Left).grip, 0.0);
        let f = e.frame(
            &DesktopInput {
                switch_hand: true,
                toggle_grip: true,
                ..Default::default()
            },
            DT,
        );
        assert_eq!(f.hand(Hand::Left).grip, 1.0);
        assert_eq!(f.hand(Hand::Right).grip, 1.0, "toggle persists");
        let crouch = DesktopInput {
            crouch: true,
            ..Default::default()
        };
        let mut f = f;
        for _ in 0..90 {
            f = e.frame(&crouch, DT);
        }
        assert!((f.head.position.y - e.settings.crouch_eye_height).abs() < 1e-4);
    }

    #[test]
    fn script_replays_in_order() {
        let a = DesktopInput {
            primary: true,
            ..Default::default()
        };
        let mut s = InputScript::new(vec![(2, a), (0, a), (1, DesktopInput::default())]);
        assert_eq!(s.total_frames(), 3);
        assert!(s.next_input().unwrap().primary);
        assert!(s.next_input().unwrap().primary);
        assert!(!s.next_input().unwrap().primary);
        assert!(s.next_input().is_none());
    }
}
