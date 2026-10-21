//! The character control state and the mouse/keyboard look.
//!
//! Port of `gCCharacterControl_PS::OnAction` (GameDll:0x100de480) for the part the movement
//! reads (which movement keys are down, the pressed-key properties) and of the look handling of
//! `gCCameraAI_PS::OnAction` (GameDll:0x1002b6c0).
use crate::keys::SessionKey;
use crate::mapper::GameKeyMessage;

/// Keyboard camera rotation speed, "mouse counts" per second (`gCCameraAI_PS::OnAction`, ids 9
/// and 10: `weight * 550 * dt`).
pub const KEY_YAW_RATE: f32 = 550.0;
/// Keyboard pitch speed in counts per second (ids 7 and 8: `weight * 50 * dt`).
pub const KEY_PITCH_RATE: f32 = 50.0;

/// The `PSCharacterControl` properties and the four movement keys.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CharacterControl {
    /// `PressedKey`: the last key message.
    pub pressed_key: Option<SessionKey>,
    /// `IsPressed`.
    pub is_pressed: bool,
    /// `IsPressedBefore`.
    pub is_pressed_before: bool,
    /// `DurationPressedMSecs`.
    pub duration_ms: u32,
    forward: bool,
    backward: bool,
    left: bool,
    right: bool,
    jump: bool,
    duck: bool,
}

/// The four ground movement keys of one frame, for the movement's wish direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MoveKeys {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
}

impl MoveKeys {
    /// The wish in the camera's ground frame: `x` right, `y` forward, each axis -1, 0 or 1
    /// (opposite keys cancel). The movement normalises the sum.
    pub fn axes(self) -> (f32, f32) {
        let x = f32::from(u8::from(self.right)) - f32::from(u8::from(self.left));
        let y = f32::from(u8::from(self.forward)) - f32::from(u8::from(self.backward));
        (x, y)
    }
    /// Any key down.
    pub fn any(self) -> bool {
        self.forward || self.backward || self.left || self.right
    }
}

impl CharacterControl {
    /// `OnAction`: record a key message. `session_running` is the session state gate of the
    /// original (`ms_pCurrentSession[0xd1] == 0 || [0xd2] == 1`): while a menu or load screen
    /// pauses the game the movement keys are not stored.
    pub fn on_action(&mut self, m: &GameKeyMessage, session_running: bool) {
        self.pressed_key = Some(m.key);
        self.is_pressed = m.is_pressed();
        self.is_pressed_before = m.was_pressed();
        self.duration_ms = m.duration_ms;
        if !session_running {
            return;
        }
        let on = m.is_pressed();
        match m.key {
            SessionKey::Forward => self.forward = on,
            SessionKey::Backward => self.backward = on,
            SessionKey::StrafeLeft => self.left = on,
            SessionKey::StrafeRight => self.right = on,
            SessionKey::Jump => self.jump = on,
            SessionKey::Duck => self.duck = on,
            _ => {}
        }
    }

    /// Forget every key (after a pause or a load).
    pub fn release_all(&mut self) {
        *self = Self::default();
    }

    /// The movement keys.
    pub fn move_keys(&self) -> MoveKeys {
        MoveKeys {
            forward: self.forward,
            backward: self.backward,
            left: self.left,
            right: self.right,
        }
    }
    /// The jump key is down.
    pub fn jump_down(&self) -> bool {
        self.jump
    }
    /// The duck key is down.
    pub fn duck_down(&self) -> bool {
        self.duck
    }
}

/// How far the camera is asked to turn this frame, in mouse counts (x right, y down), the unit
/// the camera's sensitivity and smoothing options work on.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Look {
    pub x: f32,
    pub y: f32,
}

/// Port of the look handling of `gCCameraAI_PS::OnAction`: ids 9/10 (camera right/left) and 7/8
/// (look up/down). A mouse source contributes its raw counts; a key contributes its rate times
/// the frame time (`+550 dt` right, `-550 dt` left). `invert_x` / `invert_y` flip the axes
/// (`MouseInvertX/Y`).
pub fn look_counts(messages: &[GameKeyMessage], dt: f32, invert_x: bool, invert_y: bool) -> Look {
    let mut look = Look::default();
    for m in messages.iter().filter(|m| m.is_pressed()) {
        let (axis_x, sign, rate) = match m.key {
            SessionKey::CameraRight => (true, 1.0, KEY_YAW_RATE),
            SessionKey::CameraLeft => (true, -1.0, KEY_YAW_RATE),
            SessionKey::LookUp => (false, 1.0, KEY_PITCH_RATE),
            SessionKey::LookDown => (false, -1.0, KEY_PITCH_RATE),
            _ => continue,
        };
        let v = if m.from_mouse() {
            m.weight
        } else {
            sign * m.weight * rate * dt
        };
        if axis_x {
            look.x += v;
        } else {
            look.y += v;
        }
    }
    if invert_x {
        look.x = -look.x;
    }
    if invert_y {
        look.y = -look.y;
    }
    look
}

/// The wheel and zoom keys of a frame: notches, positive zooms in (`gCCameraAI_PS::OnAction`
/// ids 17 and 18).
pub fn zoom_notches(messages: &[GameKeyMessage]) -> f32 {
    messages
        .iter()
        .filter(|m| m.is_pressed())
        .map(|m| match m.key {
            SessionKey::ZoomIn => m.weight.max(1.0),
            SessionKey::ZoomOut => -m.weight.abs().max(1.0),
            _ => 0.0,
        })
        .sum()
}

/// The `[Options.Controls]` section of `UserOptions.ini` (`gCGameApp::BuildUserOptions`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlOptions {
    /// `MouseSensitivityX/Y`, `0..=1`.
    pub sensitivity: [f32; 2],
    /// `MouseSmoothingX/Y`, `0..=1`.
    pub smoothing: [f32; 2],
    /// `MouseInvertX/Y`.
    pub invert: [bool; 2],
    /// `AltCamera`.
    pub alt_camera: bool,
    /// `CamLookaroundInverse`.
    pub lookaround_inverse: bool,
    /// `Controls=Custom`: the session keys come from the INI, not from the standard table.
    pub custom: bool,
}

impl Default for ControlOptions {
    /// The values a fresh install writes.
    fn default() -> Self {
        Self {
            sensitivity: [0.5, 0.5],
            smoothing: [0.5, 0.5],
            invert: [false, false],
            alt_camera: false,
            lookaround_inverse: true,
            custom: false,
        }
    }
}

impl ControlOptions {
    /// Read the keys of the section (`key`, `value` pairs, any case).
    pub fn from_pairs<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut o = Self::default();
        let num = |v: &str| v.trim().parse::<f32>().ok();
        let flag = |v: &str| v.trim().eq_ignore_ascii_case("true") || v.trim() == "1";
        for (k, v) in pairs {
            match k.trim().to_ascii_lowercase().as_str() {
                "mousesensitivityx" => o.sensitivity[0] = num(v).unwrap_or(o.sensitivity[0]),
                "mousesensitivityy" => o.sensitivity[1] = num(v).unwrap_or(o.sensitivity[1]),
                "mousesmoothingx" => o.smoothing[0] = num(v).unwrap_or(o.smoothing[0]),
                "mousesmoothingy" => o.smoothing[1] = num(v).unwrap_or(o.smoothing[1]),
                "mouseinvertx" => o.invert[0] = flag(v),
                "mouseinverty" => o.invert[1] = flag(v),
                "altcamera" => o.alt_camera = flag(v),
                "camlookaroundinverse" => o.lookaround_inverse = flag(v),
                "controls" => o.custom = v.trim().eq_ignore_ascii_case("custom"),
                _ => {}
            }
        }
        o
    }

    /// The keys of `[Options.Controls]` in the file's order and number format (the `Controls`
    /// line included; the custom key pairs are separate, see `custom_bindings_to_ini`).
    pub fn to_pairs(&self) -> Vec<(&'static str, String)> {
        let flag = |b: bool| b.to_string();
        vec![
            ("MouseSensitivityX", format!("{:.6}", self.sensitivity[0])),
            ("MouseSensitivityY", format!("{:.6}", self.sensitivity[1])),
            ("MouseInvertX", flag(self.invert[0])),
            ("MouseInvertY", flag(self.invert[1])),
            ("MouseSmoothingX", format!("{:.6}", self.smoothing[0])),
            ("MouseSmoothingY", format!("{:.6}", self.smoothing[1])),
            ("AltCamera", flag(self.alt_camera)),
            ("CamLookaroundInverse", flag(self.lookaround_inverse)),
            (
                "Controls",
                if self.custom { "Custom" } else { "Standard" }.to_string(),
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dik;
    use crate::mapper::{ActionMapper, RawInput};

    #[test]
    fn the_movement_keys_follow_the_press_state() {
        let mut m = ActionMapper::standard();
        let mut cc = CharacterControl::default();
        let mut raw = RawInput::default();
        raw.set_key(dik::W, true);
        raw.set_key(dik::A, true);
        for msg in m.process(&raw, 0) {
            cc.on_action(&msg, true);
        }
        let k = cc.move_keys();
        assert!(k.forward && k.left && !k.right && !k.backward);
        assert_eq!(k.axes(), (-1.0, 1.0));
        raw.set_key(dik::W, false);
        for msg in m.process(&raw, 16) {
            cc.on_action(&msg, true);
        }
        assert!(!cc.move_keys().forward && cc.move_keys().left);
    }

    #[test]
    fn opposite_keys_cancel_and_a_paused_session_stores_nothing() {
        let k = MoveKeys {
            forward: true,
            backward: true,
            ..MoveKeys::default()
        };
        assert_eq!(k.axes(), (0.0, 0.0));
        let mut cc = CharacterControl::default();
        let mut m = ActionMapper::standard();
        let mut raw = RawInput::default();
        raw.set_key(dik::W, true);
        for msg in m.process(&raw, 0) {
            cc.on_action(&msg, false);
        }
        assert!(!cc.move_keys().any());
        assert!(cc.is_pressed, "the property is stored anyway");
    }

    #[test]
    fn mouse_look_passes_raw_counts_and_keys_use_their_rates() {
        let mut m = ActionMapper::standard();
        let raw = RawInput::with_delta([12, -5, 0]);
        let l = look_counts(&m.process(&raw, 0), 1.0 / 60.0, false, false);
        assert_eq!((l.x, l.y), (12.0, -5.0));
        let mut m = ActionMapper::standard();
        let mut raw = RawInput::default();
        raw.set_key(dik::E, true);
        let l = look_counts(&m.process(&raw, 0), 0.1, false, false);
        assert!((l.x - 55.0).abs() < 1e-4, "E turns right at 550 counts/s");
        let mut raw = RawInput::default();
        raw.set_key(dik::Q, true);
        let l = look_counts(&m.process(&raw, 16), 0.1, false, false);
        assert!((l.x + 55.0).abs() < 1e-4);
        let raw = RawInput::with_delta([0, 4, 0]);
        let inv = look_counts(
            &ActionMapper::standard().process(&raw, 0),
            0.016,
            false,
            true,
        );
        assert_eq!(inv.y, -4.0);
    }

    #[test]
    fn the_wheel_zooms() {
        let mut m = ActionMapper::standard();
        let raw = RawInput::with_delta([0, 0, 2]);
        assert_eq!(zoom_notches(&m.process(&raw, 0)), 2.0);
        let raw = RawInput::with_delta([0, 0, -1]);
        assert_eq!(zoom_notches(&m.process(&raw, 16)), -1.0);
        // The shipped table lists the numpad plus before the wheel; the mapper's scan clears a key
        // whose later mouse axis did not move, so the numpad key alone does not zoom (the
        // original behaves the same way, see `eCActionMapper::Process`).
        let mut raw = RawInput::default();
        raw.set_key(dik::ADD, true);
        assert_eq!(
            zoom_notches(&ActionMapper::standard().process(&raw, 0)),
            0.0
        );
    }

    #[test]
    fn the_installed_user_options_are_read() {
        let o = ControlOptions::from_pairs([
            ("MouseSensitivityX", "0.500000"),
            ("MouseSensitivityY", "0.250000"),
            ("MouseInvertX", "false"),
            ("MouseInvertY", "true"),
            ("MouseSmoothingX", "0.500000"),
            ("MouseSmoothingY", "0.500000"),
            ("AltCamera", "false"),
            ("CamLookaroundInverse", "true"),
            ("Controls", "Standard"),
        ]);
        assert_eq!(o.sensitivity, [0.5, 0.25]);
        assert_eq!(o.invert, [false, true]);
        assert!(o.lookaround_inverse && !o.alt_camera && !o.custom);
        assert_eq!(ControlOptions::default().smoothing, [0.5, 0.5]);
    }

    #[test]
    fn control_options_write_what_they_read() {
        let o = ControlOptions {
            sensitivity: [0.25, 0.75],
            smoothing: [0.0, 1.0],
            invert: [true, false],
            alt_camera: true,
            lookaround_inverse: false,
            custom: true,
        };
        let pairs = o.to_pairs();
        let back = ControlOptions::from_pairs(pairs.iter().map(|(k, v)| (*k, v.as_str())));
        assert_eq!(back, o);
        assert_eq!(pairs[0], ("MouseSensitivityX", "0.250000".to_string()));
    }
}
