//! Host side of the options and HUD feeds of `gothic-ui` (engine port E32-E34): the option
//! resources from `UserOptions.ini`, writing applied pages back, and the per-frame data the HUD
//! pages read (worn items, heading, map position, floating texts).
use super::PlayState;
use super::controls::{InputFrame, user_options_path};
use bevy::prelude::*;
use gothic_runtime::{CoreEvent, CoreRes, Event};
use gothic_sim::sim::EventKind;
use gothic_ui::userini::{merge_section, section_pairs};
use gothic_ui::{
    ApplyControls, AudioOptions, ControlSettings, DifficultyOptions, EquippedItems, FloatingText,
    LogSettings, OptionsWrite, PlayerHeading,
};

pub fn register(app: &mut App) {
    let text = user_options_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let get = |section: &'static str| {
        let pairs = section_pairs(&text, section);
        move |k: &str| {
            pairs
                .iter()
                .find(|(pk, _)| pk.eq_ignore_ascii_case(k))
                .map(|(_, v)| v.clone())
        }
    };
    app.insert_resource(AudioOptions::from_ini(get("Options.Audio")))
        .insert_resource(DifficultyOptions::from_ini(get("Options.Difficulty")))
        .insert_resource(LogSettings::from_ini(get("Log")))
        .add_systems(
            Update,
            (write_options, apply_controls, feed_hud, floating_texts),
        );
}

/// `OptionsWrite`: merge the page into the file (every other line stays).
fn write_options(mut writes: MessageReader<OptionsWrite>) {
    for w in writes.read() {
        let Some(path) = user_options_path() else {
            continue;
        };
        let old = std::fs::read_to_string(&path).unwrap_or_default();
        // A rebound key table must not keep stale entries.
        let strip = (w.section == "Options.Controls").then_some("sessionkey.");
        let new = merge_section(&old, w.section, &w.pairs, strip);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&path, new) {
            eprintln!("options: cannot write {}: {e}", path.display());
        }
    }
}

/// `ApplyControls`: rebuild the action mapper from the new table.
fn apply_controls(
    mut applied: MessageReader<ApplyControls>,
    mut settings: ResMut<ControlSettings>,
    mut frame: ResMut<InputFrame>,
) {
    for a in applied.read() {
        *settings = a.0.clone();
        frame.apply(&a.0);
    }
}

/// Worn weapon / armor for the item tooltips, heading for the compass, position for the map.
fn feed_hud(
    core: Res<CoreRes>,
    state: Res<PlayState>,
    player: Res<crate::walk::Player>,
    mut worn: ResMut<EquippedItems>,
    mut heading: ResMut<PlayerHeading>,
    mut map: ResMut<gothic_ui::map::MapData>,
) {
    let hero = core.0.sim.actor(core.0.hero);
    let mut m = std::collections::HashMap::new();
    for i in hero.equipment.all() {
        match i.category.as_str() {
            "Weapon" => {
                m.entry(1).or_insert_with(|| i.name.clone());
            }
            "Armor" => {
                m.entry(2).or_insert_with(|| i.name.clone());
            }
            _ => {}
        }
    }
    if worn.0 != m {
        worn.0 = m;
    }
    // Written only when it differs: a changed resource wakes the pages that read it.
    if (heading.0 - state.hero_yaw).abs() > 1.0e-3 {
        heading.0 = state.hero_yaw;
    }
    let p = player.ctrl.position;
    let at = Vec2::new(p.x, p.z);
    if map.player.distance(at) > 50.0 {
        map.player = at;
    }
}

/// XP gains and damage as floating text lines (rows 0 and 1 of the effect window).
fn floating_texts(
    mut events: MessageReader<CoreEvent>,
    core: Res<CoreRes>,
    mut out: MessageWriter<FloatingText>,
) {
    let hero = core.0.hero;
    for e in events.read() {
        match &e.0 {
            Event::XpGained { amount, .. } => {
                out.write(FloatingText {
                    row: 0,
                    text: format!("+{amount} EP"),
                    color: Color::srgb(0.9, 0.85, 0.4),
                    flags: gothic_ui::texteffect::flags::CENTER
                        | gothic_ui::texteffect::flags::FADE_IN
                        | gothic_ui::texteffect::flags::FADE_OUT,
                    seconds: 2.5,
                });
            }
            Event::Sim(EventKind::Hit {
                attacker,
                victim,
                damage,
                ignored: None,
                ..
            }) if *damage > 0 && (*attacker == hero || *victim == hero) => {
                let taken = *victim == hero;
                out.write(FloatingText {
                    row: 1,
                    text: format!("{}{damage}", if taken { "-" } else { "" }),
                    color: if taken {
                        Color::srgb(0.85, 0.2, 0.15)
                    } else {
                        Color::srgb(0.95, 0.9, 0.75)
                    },
                    flags: gothic_ui::texteffect::flags::CENTER
                        | gothic_ui::texteffect::flags::FADE_OUT
                        | gothic_ui::texteffect::flags::SHAKE,
                    seconds: 1.0,
                });
            }
            _ => {}
        }
    }
}
