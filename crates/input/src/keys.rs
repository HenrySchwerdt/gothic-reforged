//! Session keys: the logical keys of the player and their default bindings.
//!
//! Port of `gCSessionKeys::CreateDefaultKeys` (GameDll:0x1017f690, the `Controls=Standard`
//! table; ids and device/offset pairs read from the code), `CreateINIKeys` (0x10182500, the ids
//! and names of the `Controls=Custom` table) and `GetPressedKey` (0x1017f2f0, the numbering of the
//! mouse pseudo keys). The [`DEFAULT_BINDINGS`] are what the shipped game does when the
//! installation has no `SessionKey.*` entries, which is the case for every stock install.
use crate::dik;

/// The logical keys, `gESessionKey`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum SessionKey {
    Forward = 1,
    Backward = 2,
    StrafeLeft = 3,
    StrafeRight = 4,
    /// Turn the character right (`Spieler Rotation Rechts`); unbound by default.
    TurnRight = 5,
    /// Turn the character left; unbound by default.
    TurnLeft = 6,
    /// Look up (mouse up). INI name `TurnUp`.
    LookUp = 7,
    /// Look down (mouse down). INI name `TurnDown`.
    LookDown = 8,
    /// Rotate the camera right (mouse right, `E`). INI name `RotateCamRight`.
    CameraRight = 9,
    /// Rotate the camera left (mouse left, `Q`). INI name `RotateCamLeft`.
    CameraLeft = 10,
    /// Walk while held (`Gehen`, shift). INI name `Slow/Fast`.
    Walk = 11,
    /// Toggle walking (`Gehen Toggle`, caps lock). INI name `WalkToggle`.
    WalkToggle = 12,
    /// Jump (`Springen`, alt). INI name `Up`.
    Jump = 13,
    /// Duck / sneak (`Ducken`, ctrl). INI name `Down`.
    Duck = 14,
    /// `Aktion 1`, left mouse button. INI name `Use1`.
    Use1 = 15,
    /// `Aktion 2`, right mouse button. INI name `Use2`.
    Use2 = 16,
    /// Zoom in (`Zoomen +`, wheel forward). INI name `Plus`.
    ZoomIn = 17,
    /// Zoom out. INI name `Minus`.
    ZoomOut = 18,
    Quick0 = 19,
    Quick1 = 20,
    Quick2 = 21,
    Quick3 = 22,
    Quick4 = 23,
    Quick5 = 24,
    Quick6 = 25,
    Quick7 = 26,
    Quick8 = 27,
    Quick9 = 28,
    /// `Bestätigen`.
    Confirm = 29,
    /// `Abbrechen`; the stock binding is a code no key sends.
    Abort = 30,
    /// `Inventar öffnen`.
    Inventory = 31,
    /// `Quest-Log`. INI name `JournalLog`.
    JournalLog = 32,
    /// `Character-Screen`. INI name `CharScreen`.
    CharScreen = 33,
    /// `Magie Buch`. INI name `MagBook`.
    MagicBook = 34,
    /// `Karte`. INI name `Maps`.
    Maps = 35,
    QuickLoad = 36,
    QuickSave = 37,
    /// `Waffe ziehen` (space, middle mouse). INI name `WeaponMode`.
    WeaponMode = 38,
    /// `Target locken`.
    Lock = 39,
    /// `Camera Look` (look back). INI name `LookBack`.
    LookBack = 40,
    /// `First Person Camera`. INI name `1stPerson`.
    FirstPerson = 41,
    ResetCamera = 42,
    ToggleHud = 43,
    TakeAll = 44,
    UsePotionHp = 45,
    UsePotionMp = 46,
    UsePotionSp = 47,
}

impl SessionKey {
    /// All keys, in id order.
    pub const ALL: [SessionKey; 47] = {
        use SessionKey::*;
        [
            Forward,
            Backward,
            StrafeLeft,
            StrafeRight,
            TurnRight,
            TurnLeft,
            LookUp,
            LookDown,
            CameraRight,
            CameraLeft,
            Walk,
            WalkToggle,
            Jump,
            Duck,
            Use1,
            Use2,
            ZoomIn,
            ZoomOut,
            Quick0,
            Quick1,
            Quick2,
            Quick3,
            Quick4,
            Quick5,
            Quick6,
            Quick7,
            Quick8,
            Quick9,
            Confirm,
            Abort,
            Inventory,
            JournalLog,
            CharScreen,
            MagicBook,
            Maps,
            QuickLoad,
            QuickSave,
            WeaponMode,
            Lock,
            LookBack,
            FirstPerson,
            ResetCamera,
            ToggleHud,
            TakeAll,
            UsePotionHp,
            UsePotionMp,
            UsePotionSp,
        ]
    };

    /// From the numeric id.
    pub fn from_id(id: u16) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| *k as u16 == id)
    }

    /// The id the script sees in `PSCharacterControl.PressedKey`.
    pub fn id(self) -> u16 {
        self as u16
    }

    /// The name used by `SessionKey.<Name>.Key1.*` in a custom `UserOptions.ini`
    /// (`CreateINIKeys`). `None` for the two turn keys the INI list lacks.
    pub fn ini_name(self) -> Option<&'static str> {
        use SessionKey::*;
        Some(match self {
            Forward => "Forward",
            Backward => "Backward",
            StrafeLeft => "StrafeLeft",
            StrafeRight => "StrafeRight",
            LookUp => "TurnUp",
            LookDown => "TurnDown",
            CameraRight => "RotateCamRight",
            CameraLeft => "RotateCamLeft",
            Walk => "Slow/Fast",
            WalkToggle => "WalkToggle",
            Jump => "Up",
            Duck => "Down",
            Use1 => "Use1",
            Use2 => "Use2",
            ZoomIn => "Plus",
            ZoomOut => "Minus",
            Quick0 => "Quick0",
            Quick1 => "Quick1",
            Quick2 => "Quick2",
            Quick3 => "Quick3",
            Quick4 => "Quick4",
            Quick5 => "Quick5",
            Quick6 => "Quick6",
            Quick7 => "Quick7",
            Quick8 => "Quick8",
            Quick9 => "Quick9",
            Confirm => "Confirm",
            Inventory => "Inv",
            JournalLog => "JournalLog",
            CharScreen => "CharScreen",
            MagicBook => "MagBook",
            Maps => "Maps",
            QuickLoad => "QuickLoad",
            QuickSave => "QuickSave",
            WeaponMode => "WeaponMode",
            Lock => "Lock",
            LookBack => "LookBack",
            FirstPerson => "1stPerson",
            ResetCamera => "ResetCamera",
            ToggleHud => "ToggleHUD",
            TakeAll => "TakeAll",
            UsePotionHp => "UsePotionHP",
            UsePotionMp => "UsePotionMP",
            UsePotionSp => "UsePotionSP",
            TurnRight | TurnLeft | Abort => return None,
        })
    }

    /// From an INI name.
    pub fn from_ini_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|k| k.ini_name().is_some_and(|n| n.eq_ignore_ascii_case(name)))
    }

    /// The quick slot (0-based) a `Quick0..9` key selects.
    pub fn quick_slot(self) -> Option<u8> {
        let id = self as u16;
        (19..=28).contains(&id).then(|| (id - 19) as u8)
    }
}

/// Mouse pseudo keys (`eEMouseOffset`): the raw axes and buttons and the direction states
/// `GetPressedKey` derives from them.
pub mod mouse {
    /// Horizontal movement.
    pub const AXIS_X: u32 = 0;
    /// Vertical movement.
    pub const AXIS_Y: u32 = 1;
    /// Wheel.
    pub const AXIS_Z: u32 = 2;
    /// Left button.
    pub const LEFT: u32 = 3;
    /// Right button.
    pub const RIGHT: u32 = 4;
    /// Middle button.
    pub const MIDDLE: u32 = 5;
    /// Fourth button.
    pub const BUTTON4: u32 = 6;
    /// Moved right (positive x delta).
    pub const X_POS: u32 = 7;
    /// Moved left.
    pub const X_NEG: u32 = 8;
    /// Moved down (positive y delta).
    pub const Y_POS: u32 = 9;
    /// Moved up.
    pub const Y_NEG: u32 = 10;
    /// Wheel forward (away from the user).
    pub const WHEEL_UP: u32 = 11;
    /// Wheel backward.
    pub const WHEEL_DOWN: u32 = 12;

    /// Is this offset one of the buttons (3 to 6)?
    pub fn is_button(offset: u32) -> bool {
        (LEFT..=BUTTON4).contains(&offset)
    }
}

/// The device of a physical key (`eCPhysicalKey` +0x10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Device {
    Keyboard = 0,
    Mouse = 1,
}

/// A physical key: a keyboard scancode or a mouse offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysicalKey {
    pub device: Device,
    /// DirectInput scancode ([`dik`]) or mouse offset ([`mouse`]).
    pub code: u32,
}

impl PhysicalKey {
    /// A keyboard key.
    pub const fn key(code: u8) -> Self {
        Self {
            device: Device::Keyboard,
            code: code as u32,
        }
    }
    /// A mouse pseudo key.
    pub const fn mouse(offset: u32) -> Self {
        Self {
            device: Device::Mouse,
            code: offset,
        }
    }
}

/// A default binding: up to two physical keys for one logical key.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Binding {
    pub key: SessionKey,
    /// The name shown in the options (German).
    pub label: &'static str,
    pub physical: [Option<PhysicalKey>; 2],
}

const fn k(code: u8) -> Option<PhysicalKey> {
    Some(PhysicalKey::key(code))
}
const fn m(offset: u32) -> Option<PhysicalKey> {
    Some(PhysicalKey::mouse(offset))
}

/// The `Controls=Standard` bindings, in the order `CreateDefaultKeys` registers them.
pub const DEFAULT_BINDINGS: [Binding; 45] = {
    use SessionKey::*;
    [
        Binding {
            key: Forward,
            label: "Vorwärts",
            physical: [k(dik::UP), k(dik::W)],
        },
        Binding {
            key: Backward,
            label: "Rückwärts",
            physical: [k(dik::DOWN), k(dik::S)],
        },
        Binding {
            key: StrafeLeft,
            label: "Strafe Links",
            physical: [k(dik::LEFT), k(dik::A)],
        },
        Binding {
            key: StrafeRight,
            label: "Strafe Rechts",
            physical: [k(dik::RIGHT), k(dik::D)],
        },
        Binding {
            key: CameraRight,
            label: "Camera Rotation Rechts",
            physical: [m(mouse::X_POS), k(dik::E)],
        },
        Binding {
            key: CameraLeft,
            label: "Camera Rotation Links",
            physical: [m(mouse::X_NEG), k(dik::Q)],
        },
        Binding {
            key: LookUp,
            label: "Rotation Hoch",
            physical: [m(mouse::Y_NEG), None],
        },
        Binding {
            key: LookDown,
            label: "Rotation Runter",
            physical: [m(mouse::Y_POS), None],
        },
        Binding {
            key: Walk,
            label: "Gehen",
            physical: [k(dik::RSHIFT), k(dik::LSHIFT)],
        },
        Binding {
            key: WalkToggle,
            label: "Gehen Toggle",
            physical: [k(dik::CAPITAL), None],
        },
        Binding {
            key: Jump,
            label: "Springen",
            physical: [k(dik::NUMPAD0), k(dik::LMENU)],
        },
        Binding {
            key: Duck,
            label: "Ducken",
            physical: [k(dik::RCONTROL), k(dik::LCONTROL)],
        },
        Binding {
            key: Use1,
            label: "Aktion 1",
            physical: [m(mouse::LEFT), None],
        },
        Binding {
            key: Use2,
            label: "Aktion 2",
            physical: [m(mouse::RIGHT), None],
        },
        Binding {
            key: ZoomIn,
            label: "Zoomen +",
            physical: [k(dik::ADD), m(mouse::WHEEL_UP)],
        },
        Binding {
            key: ZoomOut,
            label: "Zoomen -",
            physical: [k(dik::SUBTRACT), m(mouse::WHEEL_DOWN)],
        },
        Binding {
            key: ResetCamera,
            label: "Reset Camera",
            physical: [k(dik::NUMPAD4), None],
        },
        Binding {
            key: FirstPerson,
            label: "First Person Camera",
            physical: [k(dik::DECIMAL), k(dik::T)],
        },
        Binding {
            key: LookBack,
            label: "Camera Look",
            physical: [k(dik::NUMPAD2), None],
        },
        Binding {
            key: Quick0,
            label: "QuickUse 0",
            physical: [k(dik::D0), None],
        },
        Binding {
            key: Quick1,
            label: "QuickUse 1",
            physical: [k(dik::D1), None],
        },
        Binding {
            key: Quick2,
            label: "QuickUse 2",
            physical: [k(dik::D2), None],
        },
        Binding {
            key: Quick3,
            label: "QuickUse 3",
            physical: [k(dik::D3), None],
        },
        Binding {
            key: Quick4,
            label: "QuickUse 4",
            physical: [k(dik::D4), None],
        },
        Binding {
            key: Quick5,
            label: "QuickUse 5",
            physical: [k(dik::D5), None],
        },
        Binding {
            key: Quick6,
            label: "QuickUse 6",
            physical: [k(dik::D6), None],
        },
        Binding {
            key: Quick7,
            label: "QuickUse 7",
            physical: [k(dik::D7), None],
        },
        Binding {
            key: Quick8,
            label: "QuickUse 8",
            physical: [k(dik::D8), None],
        },
        Binding {
            key: Quick9,
            label: "QuickUse 9",
            physical: [k(dik::D9), None],
        },
        Binding {
            key: Abort,
            label: "Abbrechen",
            physical: [k(dik::SLEEP), None],
        },
        Binding {
            key: Inventory,
            label: "Inventar öffnen",
            physical: [k(dik::I), k(dik::BACK)],
        },
        Binding {
            key: WeaponMode,
            label: "Waffe ziehen",
            physical: [k(dik::SPACE), m(mouse::MIDDLE)],
        },
        Binding {
            key: JournalLog,
            label: "Quest-Log",
            physical: [k(dik::L), None],
        },
        Binding {
            key: CharScreen,
            label: "Character-Screen",
            physical: [k(dik::C), None],
        },
        Binding {
            key: MagicBook,
            label: "Magie Buch",
            physical: [k(dik::B), None],
        },
        Binding {
            key: Maps,
            label: "Karte",
            physical: [k(dik::M), None],
        },
        Binding {
            key: Lock,
            label: "Target locken",
            physical: [k(dik::R), k(dik::END)],
        },
        Binding {
            key: ToggleHud,
            label: "Toggle HUD",
            physical: [k(dik::SCROLL), None],
        },
        Binding {
            key: TakeAll,
            label: "Take all",
            physical: [k(dik::U), None],
        },
        Binding {
            key: UsePotionHp,
            label: "Use Potion HP",
            physical: [k(dik::NUMPAD7), None],
        },
        Binding {
            key: UsePotionMp,
            label: "Use Potion MP",
            physical: [k(dik::NUMPAD8), None],
        },
        Binding {
            key: UsePotionSp,
            label: "Use Potion SP",
            physical: [k(dik::NUMPAD9), None],
        },
        Binding {
            key: Confirm,
            label: "Bestätigen",
            physical: [k(dik::RETURN), None],
        },
        Binding {
            key: QuickSave,
            label: "Schnellspeichern",
            physical: [k(dik::F5), None],
        },
        Binding {
            key: QuickLoad,
            label: "Schnellladen",
            physical: [k(dik::F9), None],
        },
    ]
};

/// Look up one binding of the default table.
pub fn default_binding(key: SessionKey) -> Option<&'static Binding> {
    DEFAULT_BINDINGS.iter().find(|b| b.key == key)
}

/// The custom bindings of `UserOptions.ini` (`Controls=Custom`): `SessionKey.<Name>.Key1.Type`,
/// `.Key1.Offset`, `.Key2.Type`, `.Key2.Offset`. A key the INI does not mention is unbound
/// (`CreateINIKeys` registers all 47 names from the setup struct). Type is `0`/`Keyboard` or
/// `1`/`Mouse` [I: the spelling of the values is not visible in the shipped INI].
pub fn parse_custom_bindings<'a>(
    entries: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<Binding> {
    use std::collections::HashMap;
    let map: HashMap<String, &str> = entries
        .into_iter()
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim()))
        .collect();
    let physical = |name: &str, n: u8| -> Option<PhysicalKey> {
        let base = format!("sessionkey.{}.key{n}", name.to_ascii_lowercase());
        let ty = map.get(&format!("{base}.type"))?;
        let off = map.get(&format!("{base}.offset"))?.parse::<u32>().ok()?;
        match ty.to_ascii_lowercase().as_str() {
            "0" | "keyboard" => Some(PhysicalKey {
                device: Device::Keyboard,
                code: off,
            }),
            "1" | "mouse" => Some(PhysicalKey {
                device: Device::Mouse,
                code: off,
            }),
            _ => None,
        }
    };
    SessionKey::ALL
        .iter()
        .filter_map(|key| {
            let name = key.ini_name()?;
            let physical = [physical(name, 1), physical(name, 2)];
            Some(Binding {
                key: *key,
                label: "",
                physical,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ids_are_the_session_key_enumeration() {
        assert_eq!(SessionKey::Forward.id(), 1);
        assert_eq!(SessionKey::Use1.id(), 15);
        assert_eq!(SessionKey::Use2.id(), 16);
        assert_eq!(SessionKey::WeaponMode.id(), 38);
        assert_eq!(SessionKey::UsePotionSp.id(), 47);
        for (i, k) in SessionKey::ALL.iter().enumerate() {
            assert_eq!(k.id() as usize, i + 1, "{k:?} sits at its id");
            assert_eq!(SessionKey::from_id(k.id()), Some(*k));
        }
    }

    #[test]
    fn the_standard_table_binds_the_original_keys() {
        use SessionKey::*;
        let phys = |k: SessionKey| -> Vec<PhysicalKey> {
            default_binding(k)
                .expect("bound")
                .physical
                .iter()
                .flatten()
                .copied()
                .collect()
        };
        assert_eq!(
            phys(Forward),
            vec![PhysicalKey::key(dik::UP), PhysicalKey::key(dik::W)]
        );
        assert_eq!(phys(Use1), vec![PhysicalKey::mouse(mouse::LEFT)]);
        assert_eq!(phys(Use2), vec![PhysicalKey::mouse(mouse::RIGHT)]);
        assert_eq!(
            phys(WeaponMode),
            vec![
                PhysicalKey::key(dik::SPACE),
                PhysicalKey::mouse(mouse::MIDDLE)
            ]
        );
        assert_eq!(
            phys(Jump),
            vec![PhysicalKey::key(dik::NUMPAD0), PhysicalKey::key(dik::LMENU)]
        );
        assert_eq!(
            phys(Walk),
            vec![PhysicalKey::key(dik::RSHIFT), PhysicalKey::key(dik::LSHIFT)]
        );
        assert_eq!(
            phys(Duck),
            vec![
                PhysicalKey::key(dik::RCONTROL),
                PhysicalKey::key(dik::LCONTROL)
            ]
        );
        assert_eq!(
            phys(CameraRight),
            vec![PhysicalKey::mouse(mouse::X_POS), PhysicalKey::key(dik::E)]
        );
        assert_eq!(
            phys(ZoomIn),
            vec![
                PhysicalKey::key(dik::ADD),
                PhysicalKey::mouse(mouse::WHEEL_UP)
            ]
        );
        assert_eq!(phys(Quick0), vec![PhysicalKey::key(dik::D0)]);
        assert_eq!(phys(Quick9), vec![PhysicalKey::key(dik::D9)]);
        assert_eq!(phys(QuickSave), vec![PhysicalKey::key(dik::F5)]);
        assert_eq!(phys(QuickLoad), vec![PhysicalKey::key(dik::F9)]);
        assert_eq!(phys(Confirm), vec![PhysicalKey::key(dik::RETURN)]);
        // Q/E turn the camera, not the hero; the turn keys are unbound.
        assert!(default_binding(TurnLeft).is_none() && default_binding(TurnRight).is_none());
    }

    #[test]
    fn the_standard_table_has_45_entries_with_unique_keys() {
        let mut seen = std::collections::BTreeSet::new();
        for b in &DEFAULT_BINDINGS {
            assert!(seen.insert(b.key), "{:?} twice", b.key);
        }
        assert_eq!(seen.len(), 45);
    }

    #[test]
    fn ini_names_round_trip() {
        for k in SessionKey::ALL {
            if let Some(n) = k.ini_name() {
                assert_eq!(SessionKey::from_ini_name(n), Some(k));
            }
        }
        assert_eq!(
            SessionKey::from_ini_name("slow/fast"),
            Some(SessionKey::Walk)
        );
        assert_eq!(SessionKey::Quick3.quick_slot(), Some(3));
        assert_eq!(SessionKey::Use1.quick_slot(), None);
    }

    #[test]
    fn custom_bindings_read_both_keys() {
        let entries = [
            ("SessionKey.Forward.Key1.Type", "0"),
            ("SessionKey.Forward.Key1.Offset", "17"),
            ("SessionKey.Forward.Key2.Type", "Mouse"),
            ("SessionKey.Forward.Key2.Offset", "11"),
        ];
        let b = parse_custom_bindings(entries);
        let fwd = b
            .iter()
            .find(|b| b.key == SessionKey::Forward)
            .expect("forward");
        assert_eq!(fwd.physical[0], Some(PhysicalKey::key(dik::W)));
        assert_eq!(fwd.physical[1], Some(PhysicalKey::mouse(mouse::WHEEL_UP)));
        let back = b
            .iter()
            .find(|b| b.key == SessionKey::Backward)
            .expect("backward");
        assert_eq!(back.physical, [None, None]);
    }
}
