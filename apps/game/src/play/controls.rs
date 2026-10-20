//! Key and mouse bindings of the play mode.
//!
//! Source: the installed game's own documents. `Ini/controls.ini` of the installation holds only
//! the `[DebugControls]` (F1..F12, Ctrl+... developer keys) and an empty `[SessionKeys]` section;
//! the player keys are compiled into `Game.dll` and listed in the manual
//! (`Docs/G3_Manual.pdf`, section 2.3 "Tastenbelegung" and the key card on the last page) and
//! in the action list of `Ini/events.ini`:
//!
//! | Key | Action (manual) |
//! | --- | --- |
//! | W A S D | walk / strafe (`Vorwaerts`, `Rueckwaerts`, `Strafe Links/Rechts`) |
//! | Q E | turn left / right (`Spieler Rotation Links/Rechts`) |
//! | Shift | run (`Laufen`) -> sprint gait |
//! | Ctrl | sneak (`Schleichen`) |
//! | Alt | jump (`Springen`) |
//! | Space | draw the equipped weapon (`Ausgeruestete Waffe ziehen`; the key glyph is missing in the PDF text layer, the wide space-bar picture is [I]) |
//! | 1 .. 0 | wear or use the item in quick slot 1..10 (`QuickUse 0..9`) |
//! | M / B / I / C / L | documents / spell book / inventory / character / quest log |
//! | Esc | game menu |
//! | F5 F6 F9 F10 | quick save / save / quick load / load |
//! | Print | screenshot |
//!
//! Mouse (manual, "Attacken (Mausbelegung)" and the bow / crossbow chapter): short left click =
//! quick attack, normal click = standard attack, long click = power attack, hold right = parry,
//! hold right + short click = quick attack out of the parry, hold right + normal click = thrust
//! (finishing blow on a downed enemy), jump + left click = jump attack; bow: hold left to draw,
//! release to shoot; crossbow: one click. Spells use the left button the same way as the bow.
//!
//! Not documented anywhere in the installation and therefore marked [U]: the confirm / use key
//! (the manual lists no key; `events.ini` has `Benutzen 1` / `Benutzen 2` / `Bestaetigen`, taken
//! here as the left mouse button while no weapon is drawn, plus Enter), the loot key
//! (`Pluendern`), take (`Gegenstand nehmen`) and the click-length limits between quick / normal /
//! power attack (the ported rule only separates tap < 200 ms and hold, `melee::TAP_MS`).
use bevy::prelude::*;

/// A key action of the table above.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    Forward,
    Back,
    StrafeLeft,
    StrafeRight,
    TurnLeft,
    TurnRight,
    Sprint,
    Sneak,
    Jump,
    DrawWeapon,
    Documents,
    SpellBook,
    Inventory,
    Character,
    QuestLog,
    Menu,
    QuickSave,
    Save,
    QuickLoad,
    Load,
    Confirm,
}

/// Keys that trigger an action.
pub fn keys_of(a: Act) -> &'static [KeyCode] {
    match a {
        Act::Forward => &[KeyCode::KeyW],
        Act::Back => &[KeyCode::KeyS],
        Act::StrafeLeft => &[KeyCode::KeyA],
        Act::StrafeRight => &[KeyCode::KeyD],
        Act::TurnLeft => &[KeyCode::KeyQ],
        Act::TurnRight => &[KeyCode::KeyE],
        Act::Sprint => &[KeyCode::ShiftLeft, KeyCode::ShiftRight],
        Act::Sneak => &[KeyCode::ControlLeft, KeyCode::ControlRight],
        Act::Jump => &[KeyCode::AltLeft, KeyCode::AltRight],
        Act::DrawWeapon => &[KeyCode::Space],
        Act::Documents => &[KeyCode::KeyM],
        Act::SpellBook => &[KeyCode::KeyB],
        Act::Inventory => &[KeyCode::KeyI],
        Act::Character => &[KeyCode::KeyC],
        Act::QuestLog => &[KeyCode::KeyL],
        Act::Menu => &[KeyCode::Escape],
        Act::QuickSave => &[KeyCode::F5],
        Act::Save => &[KeyCode::F6],
        Act::QuickLoad => &[KeyCode::F9],
        Act::Load => &[KeyCode::F10],
        Act::Confirm => &[KeyCode::Enter, KeyCode::NumpadEnter],
    }
}

pub fn pressed(keys: &ButtonInput<KeyCode>, a: Act) -> bool {
    keys.any_pressed(keys_of(a).iter().copied())
}

pub fn just_pressed(keys: &ButtonInput<KeyCode>, a: Act) -> bool {
    keys.any_just_pressed(keys_of(a).iter().copied())
}

/// The quick slot (0-based) whose number key went down this frame (`1` = slot 0, `0` = slot 9).
pub fn quick_slot_pressed(keys: &ButtonInput<KeyCode>) -> Option<usize> {
    const DIGITS: [KeyCode; 10] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
        KeyCode::Digit0,
    ];
    DIGITS.iter().position(|k| keys.just_pressed(*k))
}

/// Turn rate of the Q / E keys: `TurnSpeedMax` of the `PC_Hero` movement template
/// (`PHYSICS_MOVEMENT.md`: 200 deg/s).
pub const TURN_RATE: f32 = 200.0 * std::f32::consts::PI / 180.0;

/// Length of a left click (seconds) up to which it is a quick attack: `melee::TAP_MS` of the
/// ported `OnPlayerAction` rule.
pub const TAP_SECS: f32 = 0.2;
/// Length from which a left click is a power attack [U]: the manual names three kinds (short,
/// normal, long) but gives no limits.
pub const LONG_SECS: f32 = 0.55;
/// Two presses of the same direction key within this time are a dodge [U]: no dodge key is
/// documented.
pub const DOUBLE_TAP_SECS: f32 = 0.25;
