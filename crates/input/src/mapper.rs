//! The action mapper: raw device state in, per-logical-key press state out.
//!
//! Port of the session-key half of `eCActionMapper::Process` (Engine:0x10125420) and of the
//! dispatch rule of `eCInputDispatcher::Dispatch` (Engine:0x1001cb00): every frame each logical
//! key with a physical key down (or that was down the frame before) produces one
//! [`GameKeyMessage`]; `IsPressed` / `IsPressedBefore` of the script are its two state bits.
//!
//! The rules, as implemented by the engine:
//!
//! * `before` is a copy of `on` taken at the start of the frame.
//! * The physical keys of a logical key are tried in order; a keyboard key sets `on` when it is
//!   pressed or toggled this frame, so a held key keeps the logical key on whichever of the two
//!   physical keys is down. A mouse pseudo key that occurred this frame ends the scan.
//! * The press time is taken on the first frame `on` is set; while it stays set the duration is
//!   `now - press time`. The release frame does *not* refresh the duration: scripts read the
//!   value of the last frame the key was held.
//! * A mouse axis direction (`X_POS`, `Y_NEG`, `WHEEL_UP`, ...) is on for exactly the frames in
//!   which the mouse moved that way; its weight is the movement. Keyboard keys have weight 1.
use crate::keys::{Binding, DEFAULT_BINDINGS, Device, PhysicalKey, SessionKey, mouse};

/// Number of DirectInput scancodes.
pub const KEY_COUNT: usize = 256;

/// The state of the devices for one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct RawInput {
    /// Keyboard state by scancode (`true` = down).
    pub keys: [bool; KEY_COUNT],
    /// Mouse buttons: left, right, middle, fourth.
    pub buttons: [bool; 4],
    /// Mouse movement of this frame in counts (x right, y down, wheel notches forward).
    pub delta: [i32; 3],
}

impl Default for RawInput {
    fn default() -> Self {
        Self {
            keys: [false; KEY_COUNT],
            buttons: [false; 4],
            delta: [0; 3],
        }
    }
}

impl RawInput {
    /// A frame with only mouse movement.
    pub fn with_delta(delta: [i32; 3]) -> Self {
        Self {
            delta,
            ..Self::default()
        }
    }
    /// Set a keyboard key.
    pub fn set_key(&mut self, scancode: u8, down: bool) {
        self.keys[scancode as usize] = down;
    }
    /// Set a mouse button by offset ([`mouse::LEFT`]..[`mouse::BUTTON4`]).
    pub fn set_button(&mut self, offset: u32, down: bool) {
        if mouse::is_button(offset) {
            self.buttons[(offset - mouse::LEFT) as usize] = down;
        }
    }
}

/// `eSGameKeyMessage`: the state of one logical key in one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GameKeyMessage {
    /// Which logical key.
    pub key: SessionKey,
    /// Weight: 1 for keys, the movement for mouse axes.
    pub weight: f32,
    flags: u8,
    /// Time of the press, application milliseconds.
    pub press_ms: u64,
    /// `DurationPressedMSecs`.
    pub duration_ms: u32,
}

const ON: u8 = 1;
const BEFORE: u8 = 2;
const FROM_MOUSE: u8 = 8;

impl GameKeyMessage {
    /// `IsPressed`.
    pub fn is_pressed(&self) -> bool {
        self.flags & ON != 0
    }
    /// `IsPressedBefore`.
    pub fn was_pressed(&self) -> bool {
        self.flags & BEFORE != 0
    }
    /// The key went down this frame.
    pub fn is_press_edge(&self) -> bool {
        self.is_pressed() && !self.was_pressed()
    }
    /// The key went up this frame.
    pub fn is_release_edge(&self) -> bool {
        !self.is_pressed() && self.was_pressed()
    }
    /// The value came from a mouse (raw counts) instead of a key (scaled by the frame time).
    pub fn from_mouse(&self) -> bool {
        self.flags & FROM_MOUSE != 0
    }
}

#[derive(Debug, Clone)]
struct Logical {
    key: SessionKey,
    physical: Vec<PhysicalKey>,
    flags: u8,
    weight: f32,
    press_ms: u64,
    duration_ms: u32,
}

/// The mapper with its key table.
#[derive(Debug, Clone)]
pub struct ActionMapper {
    keys: Vec<Logical>,
    prev_keys: [bool; KEY_COUNT],
    prev_buttons: [bool; 4],
}

impl Default for ActionMapper {
    fn default() -> Self {
        Self::standard()
    }
}

impl ActionMapper {
    /// The `Controls=Standard` table.
    pub fn standard() -> Self {
        Self::from_bindings(&DEFAULT_BINDINGS)
    }

    /// A custom table (`Controls=Custom`).
    pub fn from_bindings(bindings: &[Binding]) -> Self {
        Self {
            keys: bindings
                .iter()
                .map(|b| Logical {
                    key: b.key,
                    physical: b.physical.iter().flatten().copied().collect(),
                    flags: 0,
                    weight: 0.0,
                    press_ms: 0,
                    duration_ms: 0,
                })
                .collect(),
            prev_keys: [false; KEY_COUNT],
            prev_buttons: [false; 4],
        }
    }

    /// `AssignKey`: replace the physical keys of one logical key.
    pub fn rebind(&mut self, key: SessionKey, physical: &[PhysicalKey]) {
        if let Some(l) = self.keys.iter_mut().find(|l| l.key == key) {
            l.physical = physical.to_vec();
        } else {
            self.keys.push(Logical {
                key,
                physical: physical.to_vec(),
                flags: 0,
                weight: 0.0,
                press_ms: 0,
                duration_ms: 0,
            });
        }
    }

    /// The physical keys bound to a logical key.
    pub fn physical_keys(&self, key: SessionKey) -> &[PhysicalKey] {
        self.keys
            .iter()
            .find(|l| l.key == key)
            .map_or(&[], |l| l.physical.as_slice())
    }

    /// `GetPressedKey` (GameDll:0x1017f2f0): the first physical key that produced an event this
    /// frame, for the "press a key" prompt of the controls menu. Keyboard events win over the
    /// mouse; the mouse reports its axis direction pseudo keys.
    pub fn pressed_key(&self, raw: &RawInput) -> Option<PhysicalKey> {
        for (code, down) in raw.keys.iter().enumerate() {
            if *down && !self.prev_keys[code] {
                return Some(PhysicalKey::key(code as u8));
            }
        }
        for (i, down) in raw.buttons.iter().enumerate() {
            if *down && !self.prev_buttons[i] {
                return Some(PhysicalKey::mouse(mouse::LEFT + i as u32));
            }
        }
        let [dx, dy, dz] = raw.delta;
        if dx != 0 {
            return Some(PhysicalKey::mouse(if dx > 0 {
                mouse::X_POS
            } else {
                mouse::X_NEG
            }));
        }
        if dy != 0 {
            return Some(PhysicalKey::mouse(if dy > 0 {
                mouse::Y_POS
            } else {
                mouse::Y_NEG
            }));
        }
        if dz != 0 {
            return Some(PhysicalKey::mouse(if dz > 0 {
                mouse::WHEEL_UP
            } else {
                mouse::WHEEL_DOWN
            }));
        }
        None
    }

    /// Advance one frame at application time `now_ms` and return the messages the dispatcher
    /// sends: one per logical key that is on or was on the frame before.
    pub fn process(&mut self, raw: &RawInput, now_ms: u64) -> Vec<GameKeyMessage> {
        for l in &mut self.keys {
            // before := on
            l.flags = (l.flags & !BEFORE) | ((l.flags & ON) << 1);
        }
        for i in 0..self.keys.len() {
            self.scan(i, raw, now_ms);
        }
        self.prev_keys = raw.keys;
        self.prev_buttons = raw.buttons;
        self.keys
            .iter()
            .filter(|l| l.flags & (ON | BEFORE) != 0)
            .map(|l| GameKeyMessage {
                key: l.key,
                weight: l.weight,
                flags: l.flags,
                press_ms: l.press_ms,
                duration_ms: l.duration_ms,
            })
            .collect()
    }

    fn scan(&mut self, index: usize, raw: &RawInput, now_ms: u64) {
        let physical = self.keys[index].physical.clone();
        for p in physical {
            let l = &mut self.keys[index];
            match p.device {
                Device::Keyboard => {
                    let code = (p.code & 0xff) as usize;
                    let pressed = raw.keys[code];
                    let toggled = pressed != self.prev_keys[code];
                    if pressed || toggled {
                        l.flags = (l.flags & !ON) | u8::from(pressed);
                        if l.flags & ON != 0 {
                            l.flags &= !FROM_MOUSE;
                            l.weight = 1.0;
                            touch(l, now_ms);
                        }
                    }
                }
                Device::Mouse => {
                    let off = p.code;
                    if mouse::is_button(off) {
                        let i = (off - mouse::LEFT) as usize;
                        let down = raw.buttons[i];
                        // A held button counts as an occurred state on every frame.
                        if down || down != self.prev_buttons[i] {
                            l.flags = (l.flags & !ON) | u8::from(down);
                            if !down {
                                break;
                            }
                            l.flags |= FROM_MOUSE;
                            touch(l, now_ms);
                            break;
                        }
                    } else if let Some(delta) = axis_event(raw, off) {
                        l.flags = (l.flags & !ON) | ON | FROM_MOUSE;
                        touch(l, now_ms);
                        l.weight = delta as f32;
                        break;
                    } else if l.flags & ON != 0 {
                        // The axis did not move: the direction key goes off.
                        l.flags &= !ON;
                        l.weight = 0.0;
                        l.duration_ms = now_ms.saturating_sub(l.press_ms) as u32;
                    }
                }
            }
        }
    }
}

/// The press time and duration update of a key that is on.
fn touch(l: &mut Logical, now_ms: u64) {
    if l.flags & BEFORE == 0 {
        l.press_ms = now_ms;
        l.duration_ms = 0;
    } else {
        l.duration_ms = now_ms.saturating_sub(l.press_ms) as u32;
    }
}

/// `eCMouse::StateOccured`: the movement of a mouse axis direction this frame, if any.
fn axis_event(raw: &RawInput, offset: u32) -> Option<i32> {
    let [dx, dy, dz] = raw.delta;
    match offset {
        mouse::AXIS_X => (dx != 0).then_some(dx),
        mouse::X_POS => (dx > 0).then_some(dx),
        mouse::X_NEG => (dx < 0).then_some(dx),
        mouse::AXIS_Y => (dy != 0).then_some(dy),
        mouse::Y_POS => (dy > 0).then_some(dy),
        mouse::Y_NEG => (dy < 0).then_some(dy),
        mouse::AXIS_Z => (dz != 0).then_some(dz),
        mouse::WHEEL_UP => (dz > 0).then_some(dz),
        mouse::WHEEL_DOWN => (dz < 0).then_some(dz),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dik;

    fn msg(v: &[GameKeyMessage], key: SessionKey) -> Option<GameKeyMessage> {
        v.iter().copied().find(|m| m.key == key)
    }

    #[test]
    fn a_held_key_sends_press_hold_and_release_messages() {
        let mut m = ActionMapper::standard();
        let mut raw = RawInput::default();
        raw.set_key(dik::W, true);
        let a = msg(&m.process(&raw, 1000), SessionKey::Forward).expect("press");
        assert!(a.is_press_edge() && a.duration_ms == 0 && a.weight == 1.0);
        let b = msg(&m.process(&raw, 1016), SessionKey::Forward).expect("hold");
        assert!(b.is_pressed() && b.was_pressed() && b.duration_ms == 16);
        let c = msg(&m.process(&raw, 1033), SessionKey::Forward).expect("hold");
        assert_eq!(c.duration_ms, 33);
        raw.set_key(dik::W, false);
        let d = msg(&m.process(&raw, 1050), SessionKey::Forward).expect("release");
        assert!(d.is_release_edge());
        assert_eq!(
            d.duration_ms, 33,
            "the release frame keeps the last held duration"
        );
        assert!(msg(&m.process(&raw, 1066), SessionKey::Forward).is_none());
    }

    #[test]
    fn either_physical_key_holds_the_logical_key() {
        let mut m = ActionMapper::standard();
        let mut raw = RawInput::default();
        raw.set_key(dik::UP, true);
        assert!(
            msg(&m.process(&raw, 0), SessionKey::Forward)
                .expect("arrow")
                .is_press_edge()
        );
        raw.set_key(dik::W, true);
        let both = msg(&m.process(&raw, 10), SessionKey::Forward).expect("both");
        assert!(both.is_pressed() && both.was_pressed());
        raw.set_key(dik::UP, false);
        let w = msg(&m.process(&raw, 20), SessionKey::Forward).expect("w alone");
        assert!(w.is_pressed(), "W keeps it on after the arrow went up");
    }

    #[test]
    fn mouse_buttons_are_use1_and_use2_and_the_click_length_is_reported() {
        let mut m = ActionMapper::standard();
        let mut raw = RawInput::default();
        raw.set_button(mouse::LEFT, true);
        let press = msg(&m.process(&raw, 5000), SessionKey::Use1).expect("press");
        assert!(press.is_press_edge() && press.from_mouse());
        for t in [5016, 5033, 5050, 5066, 5083] {
            m.process(&raw, t);
        }
        raw.set_button(mouse::LEFT, false);
        let up = msg(&m.process(&raw, 5100), SessionKey::Use1).expect("release");
        assert!(up.is_release_edge());
        assert_eq!(up.duration_ms, 83, "last held frame minus the press");
    }

    #[test]
    fn right_and_middle_buttons_and_the_weapon_key() {
        let mut m = ActionMapper::standard();
        let mut raw = RawInput::default();
        raw.set_button(mouse::RIGHT, true);
        assert!(msg(&m.process(&raw, 0), SessionKey::Use2).is_some());
        raw.set_button(mouse::RIGHT, false);
        raw.set_button(mouse::MIDDLE, true);
        let v = m.process(&raw, 16);
        assert!(
            msg(&v, SessionKey::WeaponMode)
                .expect("middle")
                .is_press_edge()
        );
        let mut raw2 = RawInput::default();
        raw2.set_key(dik::SPACE, true);
        let mut m2 = ActionMapper::standard();
        assert!(
            msg(&m2.process(&raw2, 0), SessionKey::WeaponMode)
                .expect("space")
                .is_press_edge()
        );
    }

    #[test]
    fn mouse_movement_is_a_direction_key_with_the_movement_as_weight() {
        let mut m = ActionMapper::standard();
        let raw = RawInput::with_delta([7, -3, 0]);
        let v = m.process(&raw, 0);
        let right = msg(&v, SessionKey::CameraRight).expect("right");
        assert_eq!(right.weight, 7.0);
        assert!(right.from_mouse());
        let up = msg(&v, SessionKey::LookUp).expect("up");
        assert_eq!(up.weight, -3.0);
        assert!(msg(&v, SessionKey::CameraLeft).is_none());
        // No movement: the keys go off on the next frame.
        let after = m.process(&RawInput::default(), 16);
        let r = msg(&after, SessionKey::CameraRight).expect("release frame");
        assert!(r.is_release_edge());
    }

    #[test]
    fn the_wheel_zooms_and_e_q_turn_the_camera() {
        let mut m = ActionMapper::standard();
        let raw = RawInput::with_delta([0, 0, 1]);
        assert!(msg(&m.process(&raw, 0), SessionKey::ZoomIn).is_some());
        let mut raw = RawInput::default();
        raw.set_key(dik::Q, true);
        let v = m.process(&raw, 16);
        let left = msg(&v, SessionKey::CameraLeft).expect("q");
        assert!(!left.from_mouse() && left.weight == 1.0);
    }

    #[test]
    fn rebinding_moves_a_logical_key_to_another_physical_key() {
        let mut m = ActionMapper::standard();
        m.rebind(SessionKey::Use1, &[PhysicalKey::key(dik::F)]);
        let mut raw = RawInput::default();
        raw.set_button(mouse::LEFT, true);
        assert!(msg(&m.process(&raw, 0), SessionKey::Use1).is_none());
        raw.set_button(mouse::LEFT, false);
        raw.set_key(dik::F, true);
        assert!(msg(&m.process(&raw, 16), SessionKey::Use1).is_some());
    }

    #[test]
    fn the_press_a_key_prompt_reports_the_first_event() {
        let m = ActionMapper::standard();
        let mut raw = RawInput::default();
        raw.set_key(dik::G, true);
        assert_eq!(m.pressed_key(&raw), Some(PhysicalKey::key(dik::G)));
        let raw = RawInput::with_delta([0, 4, 0]);
        assert_eq!(m.pressed_key(&raw), Some(PhysicalKey::mouse(mouse::Y_POS)));
        assert_eq!(m.pressed_key(&RawInput::default()), None);
    }
}
