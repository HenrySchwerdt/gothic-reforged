//! The save / load page (`gCHUDMenuFiles2`): rows the host lists from its save directory,
//! the commands the page sends back, and the caption format of a slot.
//!
//! The UI never touches the file system: the host answers [`crate::MenuChoice`] for Load /
//! Save with a [`SlotList`], and carries out [`FileCommand`]s. Rules (what may be saved, the
//! slot file layout) stay in `gothic_runtime::save`.
use crate::journal::{clock_from_ms, format_clock};
use bevy::prelude::*;

/// Kind of a save slot as the list shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    Quick,
    Auto,
    Manual,
}

/// One listed save.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotRow {
    /// Host key of the slot (its file name); echoed in commands.
    pub key: String,
    pub name: String,
    pub kind: SlotKind,
    /// Seconds since the Unix epoch.
    pub saved_unix: u64,
    pub game_ms: u64,
    pub play_ms: u64,
    pub region: String,
    pub level: u32,
}

/// Host -> UI: the slots to list (newest first) and whether saving is allowed right now
/// (`gCSession::IsSaveAllowed`).
#[derive(Message, Clone, Debug, Default)]
pub struct SlotList {
    pub rows: Vec<SlotRow>,
    pub can_save: bool,
}

/// UI -> host.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub enum FileCommand {
    Load {
        key: String,
    },
    /// `key: None` writes a new slot, `Some` overwrites that one.
    Save {
        key: Option<String>,
    },
    Delete {
        key: String,
    },
    /// The list was opened or a command finished: send a fresh [`SlotList`].
    Refresh,
}

/// Tag of the confirm boxes of this page (`file_overwrite:<key>`, `file_delete:<key>`).
pub const CONFIRM_OVERWRITE: &str = "file_overwrite";
pub const CONFIRM_DELETE: &str = "file_delete";

/// Answer of a confirm box of this page as a command (`None` for other boxes or "no").
pub fn confirm_command(tag: &str, yes: bool) -> Option<FileCommand> {
    if !yes {
        return None;
    }
    let (kind, key) = tag.split_once(':')?;
    let key = key.to_string();
    match kind {
        CONFIRM_OVERWRITE => Some(FileCommand::Save { key: Some(key) }),
        CONFIRM_DELETE => Some(FileCommand::Delete { key }),
        _ => None,
    }
}

/// Which buttons are live for the selected row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Buttons {
    pub load: bool,
    pub save: bool,
    pub delete: bool,
}

pub fn buttons(selected: Option<&SlotRow>, can_save: bool) -> Buttons {
    Buttons {
        load: selected.is_some(),
        // a new slot can always be written; overwriting needs the row
        save: can_save,
        // quick and auto saves are managed by the game
        delete: selected.is_some_and(|r| r.kind == SlotKind::Manual),
    }
}

/// Port of the caption build in gCHUDMenuFiles2 (Game:0x1008ddf0): the slot name (quick saves
/// get the fixed quick-save label, region prefixes are stripped), then the game clock from the
/// localized `HUD_File_SaveGame_Format`.
pub fn caption(row: &SlotRow, quick_label: &str, clock_format: &str) -> (String, String) {
    let title = if row.kind == SlotKind::Quick {
        quick_label.to_string()
    } else if row.name.is_empty() {
        row.region.replace("G3_World_01_", "")
    } else {
        row.name.replace("G3_World_01_", "")
    };
    let (day, hour, minute) = clock_from_ms(row.game_ms);
    (title, format_clock(clock_format, 1, day, hour, minute))
}

/// `h:mm` play time.
pub fn play_time(ms: u64) -> String {
    let m = ms / 60_000;
    format!("{}:{:02}", m / 60, m % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(kind: SlotKind, name: &str) -> SlotRow {
        SlotRow {
            key: format!("{name}.g3rs"),
            name: name.into(),
            kind,
            saved_unix: 1,
            game_ms: 86_400_000 * 2 + 3_600_000 * 14 + 60_000 * 5,
            play_ms: 3_725_000,
            region: "G3_World_01_Ardea".into(),
            level: 3,
        }
    }

    #[test]
    fn buttons_follow_selection_and_slot_kind() {
        assert_eq!(
            buttons(None, true),
            Buttons {
                load: false,
                save: true,
                delete: false
            }
        );
        let m = row(SlotKind::Manual, "slot_01");
        assert_eq!(
            buttons(Some(&m), false),
            Buttons {
                load: true,
                save: false,
                delete: true
            }
        );
        let a = row(SlotKind::Auto, "auto_0");
        assert!(!buttons(Some(&a), true).delete);
    }

    #[test]
    fn captions_use_quick_label_region_strip_and_clock() {
        let (t, c) = caption(
            &row(SlotKind::Quick, "quick"),
            "Schnellspeichern",
            "$(day). Tag $(hour):$(minute)",
        );
        assert_eq!(t, "Schnellspeichern");
        assert_eq!(c, "3. Tag 14:05");
        let mut r = row(SlotKind::Manual, "");
        r.name.clear();
        assert_eq!(caption(&r, "q", "$(day). Tag").0, "Ardea");
    }

    #[test]
    fn confirm_tags_map_to_commands() {
        assert_eq!(
            confirm_command("file_delete:slot_02.g3rs", true),
            Some(FileCommand::Delete {
                key: "slot_02.g3rs".into()
            })
        );
        assert_eq!(
            confirm_command("file_overwrite:a", true),
            Some(FileCommand::Save {
                key: Some("a".into())
            })
        );
        assert_eq!(confirm_command("file_delete:a", false), None);
        assert_eq!(confirm_command("quit", true), None);
    }

    #[test]
    fn play_time_formats_hours_and_minutes() {
        assert_eq!(play_time(3_725_000), "1:02");
        assert_eq!(play_time(0), "0:00");
    }
}
