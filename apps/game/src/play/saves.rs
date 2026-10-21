//! Save game glue of the play mode: manual slots (F6 save, F10 load the newest), the main menu's
//! "Spiel laden" / "Spiel speichern" entries, autosaves when the hero enters a new region, and
//! the re-synchronisation of the world's loose items after a load. F5 / F9 (quick slot) live in
//! `hero.rs`; the file format and slot rules are `gothic_runtime::save`.
use super::controls::{Act, just_pressed};
use super::objects::WorldObjects;
use bevy::prelude::*;
use gothic_runtime::save::{MANUAL_SLOTS, SaveDir};
use gothic_runtime::{Command, CoreCommand, CoreEvent, CoreRes, Event, SavePath};
use gothic_ui::{
    FileCommand, MenuChoice, MenuEntry, SlotKind as UiSlotKind, SlotList, SlotRow, UiState,
};

/// Size of a region cell in centimetres (placeholder until the world regions are ported).
const REGION_CELL_CM: f32 = 20_000.0;
/// Minimum real time between two autosaves.
const AUTOSAVE_COOLDOWN_SECS: f32 = 120.0;

pub fn register(app: &mut App) {
    app.add_systems(
        Update,
        (
            slot_keys,
            menu_entries,
            file_commands,
            autosave,
            resync_world_after_load,
        ),
    );
}

fn dir_of(save: &SavePath) -> SaveDir {
    SaveDir(
        save.0
            .parent()
            .map_or_else(|| ".local/saves".into(), ToOwned::to_owned),
    )
}

/// First free manual slot, else the oldest one.
fn next_manual(dir: &SaveDir) -> std::path::PathBuf {
    let used = dir.list();
    (1..=MANUAL_SLOTS)
        .map(|n| dir.manual(n))
        .find(|p| !used.iter().any(|u| &u.path == p))
        .unwrap_or_else(|| {
            used.iter()
                .rev()
                .find(|u| u.header.kind == gothic_runtime::save::SlotKind::Manual)
                .map_or_else(|| dir.manual(1), |u| u.path.clone())
        })
}

fn slot_keys(
    keys: Res<ButtonInput<KeyCode>>,
    save: Res<SavePath>,
    mut commands: MessageWriter<CoreCommand>,
) {
    let dir = dir_of(&save);
    if just_pressed(&keys, Act::Save) {
        commands.write(CoreCommand(Command::Save(next_manual(&dir))));
    }
    if just_pressed(&keys, Act::Load)
        && let Some(newest) = dir.list().into_iter().next()
    {
        commands.write(CoreCommand(Command::Load(newest.path)));
    }
}

/// The menu's "Beenden" leaves the game; Load / Save open the files page of the UI, which talks
/// to [`file_commands`].
fn menu_entries(mut menu: MessageReader<MenuChoice>, mut exit: MessageWriter<AppExit>) {
    for m in menu.read() {
        if m.0 == MenuEntry::Quit {
            exit.write(AppExit::Success);
        }
    }
}

fn slot_rows(dir: &SaveDir) -> Vec<SlotRow> {
    dir.list()
        .into_iter()
        .filter_map(|s| {
            let key = s.path.file_name()?.to_string_lossy().into_owned();
            Some(SlotRow {
                key,
                name: s.header.name.clone(),
                kind: match s.header.kind {
                    gothic_runtime::save::SlotKind::Quick => UiSlotKind::Quick,
                    gothic_runtime::save::SlotKind::Auto => UiSlotKind::Auto,
                    gothic_runtime::save::SlotKind::Manual => UiSlotKind::Manual,
                },
                saved_unix: s.header.saved_unix,
                game_ms: s.header.game_ms,
                play_ms: s.header.play_ms,
                region: s.header.region.clone(),
                level: s.header.hero_level,
            })
        })
        .collect()
}

/// A slot key is a bare file name of the save directory (no separators, no parent hops).
fn slot_path(dir: &SaveDir, key: &str) -> Option<std::path::PathBuf> {
    let ok = !key.is_empty()
        && !key.contains(['/', '\\'])
        && key != ".."
        && key.ends_with(&format!(".{}", gothic_runtime::save::EXT));
    ok.then(|| dir.0.join(key))
}

/// Answers of the files page: list the slots, load, save, delete.
fn file_commands(
    mut files: MessageReader<FileCommand>,
    save: Res<SavePath>,
    core: Res<CoreRes>,
    mut ui: ResMut<UiState>,
    mut list: MessageWriter<SlotList>,
    mut commands: MessageWriter<CoreCommand>,
) {
    let dir = dir_of(&save);
    let can_save = !core.0.dialog_open();
    ui.menu_can_save = can_save;
    for f in files.read() {
        match f {
            FileCommand::Refresh => {}
            FileCommand::Load { key } => {
                if let Some(p) = slot_path(&dir, key) {
                    commands.write(CoreCommand(Command::Load(p)));
                }
            }
            FileCommand::Save { key } => {
                if !can_save {
                    continue;
                }
                let path = key
                    .as_deref()
                    .and_then(|k| slot_path(&dir, k))
                    .unwrap_or_else(|| next_manual(&dir));
                commands.write(CoreCommand(Command::Save(path)));
            }
            FileCommand::Delete { key } => {
                // only numbered manual slots can be removed from the page
                let manual = key.starts_with("slot_");
                if let Some(p) = slot_path(&dir, key).filter(|_| manual) {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
        list.write(SlotList {
            rows: slot_rows(&dir),
            can_save,
        });
    }
}

/// Autosave (rotating slots) when the hero enters another region cell.
fn autosave(
    time: Res<Time>,
    core: Res<CoreRes>,
    save: Res<SavePath>,
    mut commands: MessageWriter<CoreCommand>,
    mut last: Local<Option<(i32, i32)>>,
    mut cooldown: Local<f32>,
) {
    *cooldown -= time.delta_secs();
    let Some(&(p, _)) = core.0.sim.world.actors.get(&core.0.hero) else {
        return;
    };
    let cell = (
        (p.x / REGION_CELL_CM).floor() as i32,
        (p.z / REGION_CELL_CM).floor() as i32,
    );
    if *last == Some(cell) {
        return;
    }
    let first = last.is_none();
    *last = Some(cell);
    commands.write(CoreCommand(Command::SetHost {
        key: "region".into(),
        value: format!("cell_{}_{}", cell.0, cell.1),
    }));
    if first || *cooldown > 0.0 || core.0.dialog_open() {
        return;
    }
    *cooldown = AUTOSAVE_COOLDOWN_SECS;
    commands.write(CoreCommand(Command::Save(dir_of(&save).next_autosave())));
}

/// After a load the loose items that were picked up (in the loaded game) disappear and the ones
/// that are lying there again (in the loaded game) come back.
fn resync_world_after_load(
    mut events: MessageReader<CoreEvent>,
    core: Res<CoreRes>,
    mut objects: ResMut<WorldObjects>,
    mut commands: Commands,
) {
    if !events.read().any(|e| matches!(e.0, Event::Loaded { .. })) {
        return;
    }
    for e in objects.list.iter_mut().filter(|e| e.def.is_item) {
        let Some(entity) = e.entity else { continue };
        let Some(taken) = core.0.sim.script.loose_item_taken(entity) else {
            continue;
        };
        e.taken = taken;
        if taken {
            if let Some(v) = e.visual.take() {
                commands.entity(v).despawn();
            }
            e.visual_tried = true;
        } else if e.visual.is_none() {
            e.visual_tried = false;
        }
    }
}
