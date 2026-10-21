//! DirectInput keyboard scancodes (`DIK_*`), the key space of the original `eCKeyboard`.
//!
//! The session key table of `Game.dll` stores physical keys as these codes
//! (`CreateDefaultKeys`: `0x11` is W, `200` is the up arrow, ...). `Ini/controls.ini` names
//! them for the custom bindings; [`from_name`] / [`name`] translate between the two, and
//! [`from_key_code_name`] maps the `Debug` name of a Bevy `KeyCode` to the scancode so a host
//! does not need an own table.

pub const ESCAPE: u8 = 0x01;
pub const D1: u8 = 0x02;
pub const D2: u8 = 0x03;
pub const D3: u8 = 0x04;
pub const D4: u8 = 0x05;
pub const D5: u8 = 0x06;
pub const D6: u8 = 0x07;
pub const D7: u8 = 0x08;
pub const D8: u8 = 0x09;
pub const D9: u8 = 0x0A;
pub const D0: u8 = 0x0B;
pub const MINUS: u8 = 0x0C;
pub const EQUALS: u8 = 0x0D;
pub const BACK: u8 = 0x0E;
pub const TAB: u8 = 0x0F;
pub const Q: u8 = 0x10;
pub const W: u8 = 0x11;
pub const E: u8 = 0x12;
pub const R: u8 = 0x13;
pub const T: u8 = 0x14;
pub const Y: u8 = 0x15;
pub const U: u8 = 0x16;
pub const I: u8 = 0x17;
pub const O: u8 = 0x18;
pub const P: u8 = 0x19;
pub const LBRACKET: u8 = 0x1A;
pub const RBRACKET: u8 = 0x1B;
pub const RETURN: u8 = 0x1C;
pub const LCONTROL: u8 = 0x1D;
pub const A: u8 = 0x1E;
pub const S: u8 = 0x1F;
pub const D: u8 = 0x20;
pub const F: u8 = 0x21;
pub const G: u8 = 0x22;
pub const H: u8 = 0x23;
pub const J: u8 = 0x24;
pub const K: u8 = 0x25;
pub const L: u8 = 0x26;
pub const SEMICOLON: u8 = 0x27;
pub const APOSTROPHE: u8 = 0x28;
pub const GRAVE: u8 = 0x29;
pub const LSHIFT: u8 = 0x2A;
pub const BACKSLASH: u8 = 0x2B;
pub const Z: u8 = 0x2C;
pub const X: u8 = 0x2D;
pub const C: u8 = 0x2E;
pub const V: u8 = 0x2F;
pub const B: u8 = 0x30;
pub const N: u8 = 0x31;
pub const M: u8 = 0x32;
pub const COMMA: u8 = 0x33;
pub const PERIOD: u8 = 0x34;
pub const SLASH: u8 = 0x35;
pub const RSHIFT: u8 = 0x36;
pub const MULTIPLY: u8 = 0x37;
pub const LMENU: u8 = 0x38;
pub const SPACE: u8 = 0x39;
pub const CAPITAL: u8 = 0x3A;
pub const F1: u8 = 0x3B;
pub const F2: u8 = 0x3C;
pub const F3: u8 = 0x3D;
pub const F4: u8 = 0x3E;
pub const F5: u8 = 0x3F;
pub const F6: u8 = 0x40;
pub const F7: u8 = 0x41;
pub const F8: u8 = 0x42;
pub const F9: u8 = 0x43;
pub const F10: u8 = 0x44;
pub const NUMLOCK: u8 = 0x45;
pub const SCROLL: u8 = 0x46;
pub const NUMPAD7: u8 = 0x47;
pub const NUMPAD8: u8 = 0x48;
pub const NUMPAD9: u8 = 0x49;
pub const SUBTRACT: u8 = 0x4A;
pub const NUMPAD4: u8 = 0x4B;
pub const NUMPAD5: u8 = 0x4C;
pub const NUMPAD6: u8 = 0x4D;
pub const ADD: u8 = 0x4E;
pub const NUMPAD1: u8 = 0x4F;
pub const NUMPAD2: u8 = 0x50;
pub const NUMPAD3: u8 = 0x51;
pub const NUMPAD0: u8 = 0x52;
pub const DECIMAL: u8 = 0x53;
pub const F11: u8 = 0x57;
pub const F12: u8 = 0x58;
pub const F13: u8 = 0x64;
pub const F14: u8 = 0x65;
pub const F15: u8 = 0x66;
pub const NUMPADENTER: u8 = 0x9C;
pub const RCONTROL: u8 = 0x9D;
pub const DIVIDE: u8 = 0xB5;
pub const RMENU: u8 = 0xB8;
pub const PAUSE: u8 = 0xC5;
pub const HOME: u8 = 0xC7;
pub const UP: u8 = 0xC8;
pub const PRIOR: u8 = 0xC9;
pub const LEFT: u8 = 0xCB;
pub const RIGHT: u8 = 0xCD;
pub const END: u8 = 0xCF;
pub const DOWN: u8 = 0xD0;
pub const NEXT: u8 = 0xD1;
pub const INSERT: u8 = 0xD2;
pub const DELETE: u8 = 0xD3;
pub const LWIN: u8 = 0xDB;
pub const RWIN: u8 = 0xDC;
/// `DIK_SLEEP`: the code the shipped "Abbrechen" binding names, a key no keyboard has.
pub const SLEEP: u8 = 0xDF;

/// The key names of `Ini/controls.ini` (the list in its header) with their scancodes.
pub const NAMES: &[(&str, u8)] = &[
    ("ESC", ESCAPE),
    ("F1", F1),
    ("F2", F2),
    ("F3", F3),
    ("F4", F4),
    ("F5", F5),
    ("F6", F6),
    ("F7", F7),
    ("F8", F8),
    ("F9", F9),
    ("F10", F10),
    ("F11", F11),
    ("F12", F12),
    ("F13", F13),
    ("F14", F14),
    ("F15", F15),
    ("PAUSE", PAUSE),
    ("INSERT", INSERT),
    ("HOME", HOME),
    ("PAGE UP", PRIOR),
    ("DELETE", DELETE),
    ("END", END),
    ("PAGE DOWN", NEXT),
    ("CURSOR UP", UP),
    ("CURSOR LEFT", LEFT),
    ("CURSOR DOWN", DOWN),
    ("CURSOR RIGHT", RIGHT),
    ("NUMLOCK", NUMLOCK),
    ("SCROLL", SCROLL),
    ("NUM DIV", DIVIDE),
    ("NUM MUL", MULTIPLY),
    ("NUM SUB", SUBTRACT),
    ("NUM ADD", ADD),
    ("NUM ENTER", NUMPADENTER),
    ("NUM DEL", DECIMAL),
    ("NUM 0", NUMPAD0),
    ("NUM 1", NUMPAD1),
    ("NUM 2", NUMPAD2),
    ("NUM 3", NUMPAD3),
    ("NUM 4", NUMPAD4),
    ("NUM 5", NUMPAD5),
    ("NUM 6", NUMPAD6),
    ("NUM 7", NUMPAD7),
    ("NUM 8", NUMPAD8),
    ("NUM 9", NUMPAD9),
    ("BACKSPACE", BACK),
    ("TAB", TAB),
    ("RETURN", RETURN),
    ("CAPS LOCK", CAPITAL),
    ("LEFT SHIFT", LSHIFT),
    ("RIGHT SHIFT", RSHIFT),
    ("LEFT CTRL", LCONTROL),
    ("LEFT OS KEY", LWIN),
    ("LEFT ALT", LMENU),
    ("SPACE", SPACE),
    ("RIGHT ALT", RMENU),
    ("RIGHT OS KEY", RWIN),
    ("RIGHT CTRL", RCONTROL),
    ("GRAVE", GRAVE),
    ("1", D1),
    ("2", D2),
    ("3", D3),
    ("4", D4),
    ("5", D5),
    ("6", D6),
    ("7", D7),
    ("8", D8),
    ("9", D9),
    ("0", D0),
    ("MINUS", MINUS),
    ("EQUALS", EQUALS),
    ("Q", Q),
    ("W", W),
    ("E", E),
    ("R", R),
    ("T", T),
    ("Y", Y),
    ("U", U),
    ("I", I),
    ("O", O),
    ("P", P),
    ("LEFT BRACKET", LBRACKET),
    ("RIGHT BRACKET", RBRACKET),
    ("A", A),
    ("S", S),
    ("D", D),
    ("F", F),
    ("G", G),
    ("H", H),
    ("J", J),
    ("K", K),
    ("L", L),
    ("SEMICOLON", SEMICOLON),
    ("APOSTROPHE", APOSTROPHE),
    ("BACKSLASH", BACKSLASH),
    ("Z", Z),
    ("X", X),
    ("C", C),
    ("V", V),
    ("B", B),
    ("N", N),
    ("M", M),
    ("COMMA", COMMA),
    ("PERIOD", PERIOD),
    ("SLASH", SLASH),
];

/// Scancode of a `controls.ini` key name (case-insensitive).
pub fn from_name(name: &str) -> Option<u8> {
    let name = name.trim();
    NAMES
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, c)| *c)
}

/// The `controls.ini` name of a scancode.
pub fn name(code: u8) -> Option<&'static str> {
    NAMES.iter().find(|(_, c)| *c == code).map(|(n, _)| *n)
}

/// Scancode for the `Debug` name of a Bevy `KeyCode` (`"KeyW"`, `"ArrowUp"`, `"ShiftLeft"`,
/// `"Digit1"`, `"Numpad0"`, ...). Keys without a DirectInput counterpart give `None`.
pub fn from_key_code_name(name: &str) -> Option<u8> {
    if let Some(letter) = name.strip_prefix("Key")
        && letter.len() == 1
    {
        return from_name(letter);
    }
    if let Some(digit) = name.strip_prefix("Digit")
        && digit.len() == 1
    {
        return from_name(digit);
    }
    if let Some(n) = name.strip_prefix("Numpad") {
        return Some(match n {
            "0" => NUMPAD0,
            "1" => NUMPAD1,
            "2" => NUMPAD2,
            "3" => NUMPAD3,
            "4" => NUMPAD4,
            "5" => NUMPAD5,
            "6" => NUMPAD6,
            "7" => NUMPAD7,
            "8" => NUMPAD8,
            "9" => NUMPAD9,
            "Add" => ADD,
            "Subtract" => SUBTRACT,
            "Multiply" => MULTIPLY,
            "Divide" => DIVIDE,
            "Decimal" => DECIMAL,
            "Enter" => NUMPADENTER,
            _ => return None,
        });
    }
    if let Some(n) = name.strip_prefix('F')
        && let Ok(n) = n.parse::<u8>()
        && (1..=15).contains(&n)
    {
        return from_name(&format!("F{n}"));
    }
    Some(match name {
        "Escape" => ESCAPE,
        "Minus" => MINUS,
        "Equal" => EQUALS,
        "Backspace" => BACK,
        "Tab" => TAB,
        "BracketLeft" => LBRACKET,
        "BracketRight" => RBRACKET,
        "Enter" => RETURN,
        "ControlLeft" => LCONTROL,
        "ControlRight" => RCONTROL,
        "Semicolon" => SEMICOLON,
        "Quote" => APOSTROPHE,
        "Backquote" => GRAVE,
        "ShiftLeft" => LSHIFT,
        "ShiftRight" => RSHIFT,
        "Backslash" | "IntlBackslash" => BACKSLASH,
        "Comma" => COMMA,
        "Period" => PERIOD,
        "Slash" => SLASH,
        "AltLeft" => LMENU,
        "AltRight" => RMENU,
        "Space" => SPACE,
        "CapsLock" => CAPITAL,
        "NumLock" => NUMLOCK,
        "ScrollLock" => SCROLL,
        "Pause" => PAUSE,
        "Home" => HOME,
        "ArrowUp" => UP,
        "PageUp" => PRIOR,
        "ArrowLeft" => LEFT,
        "ArrowRight" => RIGHT,
        "End" => END,
        "ArrowDown" => DOWN,
        "PageDown" => NEXT,
        "Insert" => INSERT,
        "Delete" => DELETE,
        "SuperLeft" => LWIN,
        "SuperRight" => RWIN,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_of_the_ini_resolve_both_ways() {
        assert_eq!(from_name("cursor up"), Some(UP));
        assert_eq!(from_name("LEFT CTRL"), Some(LCONTROL));
        assert_eq!(name(0x39), Some("SPACE"));
        assert_eq!(name(0xDF), None);
        for (n, c) in NAMES {
            assert_eq!(from_name(n), Some(*c), "{n}");
        }
    }

    #[test]
    fn bevy_key_codes_map_to_scancodes() {
        assert_eq!(from_key_code_name("KeyW"), Some(W));
        assert_eq!(from_key_code_name("Digit0"), Some(D0));
        assert_eq!(from_key_code_name("ArrowDown"), Some(DOWN));
        assert_eq!(from_key_code_name("Numpad0"), Some(NUMPAD0));
        assert_eq!(from_key_code_name("NumpadAdd"), Some(ADD));
        assert_eq!(from_key_code_name("AltLeft"), Some(LMENU));
        assert_eq!(from_key_code_name("F9"), Some(F9));
        assert_eq!(from_key_code_name("MediaPlay"), None);
    }
}
