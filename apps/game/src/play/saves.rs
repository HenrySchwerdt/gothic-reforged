//! Save game glue of the play mode: manual slots (F6 save, F10 load the newest), the main menu's
//! "Spiel laden" / "Spiel speichern" entries, autosaves when the hero enters a new region, and
//! the re-synchronisation of the world's loose items after a load. F5 / F9 (quick slot) live in
//! `hero.rs`; the file format and slot rules are `gothic_runtime::save`.
use super::controls::{Act, just_pressed};
use super::objects::WorldObjects;
use bevy::prelude::*;
use gothic_runtime::save::{MANUAL_SLOTS, SaveDir};
use gothic_runtime::{Command, CoreCommand, CoreEvent, CoreRes, Event, SavePath};
use gothic_ui::{MenuChoice, MenuEntry};

/// Size of a region cell in centimetres (placeholder until the world regions are ported).
const REGION_CELL_CM: f32 = 20_000.0;
/// Minimum real time between two autosaves.
const AUTOSAVE_COOLDOWN_SECS: f32 = 120.0;

pub fn register(app: &mut App) {
    app.add_systems(
        Update,
        (slot_keys, menu_entries, autosave, resync_world_after_load),
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

fn menu_entries(
    mut menu: MessageReader<MenuChoice>,
    save: Res<SavePath>,
    mut commands: MessageWriter<CoreCommand>,
) {
    let dir = dir_of(&save);
    for m in menu.read() {
        match m.0 {
            MenuEntry::SaveGame => {
                commands.write(CoreCommand(Command::Save(next_manual(&dir))));
            }
            MenuEntry::LoadGame => {
                if let Some(newest) = dir.list().into_iter().next() {
                    commands.write(CoreCommand(Command::Load(newest.path)));
                }
            }
            _ => {}
        }
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
