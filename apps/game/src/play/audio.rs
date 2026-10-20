//! Audio wiring of the play mode: region music, fight music exit and hero footsteps.
//!
//! Everything else (voices, mixing, rolloff, channel budget) lives in `gothic_audio`.
use super::PlayState;
use super::events::CombatLog;
use bevy::prelude::*;
use gothic_audio::{MusicCommand, PlaySfx};

pub fn register(app: &mut App) {
    app.add_systems(Update, (region_music, fight_music_end, hero_footsteps));
}

/// Ardea lies in Myrtana: start its explore theme once.
fn region_music(mut done: Local<bool>, mut music: MessageWriter<MusicCommand>) {
    if !*done {
        *done = true;
        music.write(MusicCommand::SetLocation("myrtana".into()));
    }
}

/// The fight theme ends after 12 s without a change of the combat target (the hero's
/// `combat_focus` is refreshed with every swing).
fn fight_music_end(
    time: Res<Time>,
    state: Res<PlayState>,
    mut log: ResMut<CombatLog>,
    mut last: Local<(Option<(u32, f32)>, f32)>,
    mut music: MessageWriter<MusicCommand>,
) {
    let now = time.elapsed_secs();
    let cur = state.combat_focus.map(|(a, t)| (a.0, t));
    if cur != last.0 {
        last.0 = cur;
        last.1 = now;
    }
    if !log.fight {
        last.1 = now;
    } else if now - last.1 > 12.0 {
        log.fight = false;
        music.write(MusicCommand::SetSituation("explore".into()));
    }
}

/// Hero footsteps: `eff_step_human_{walk|run}_{left|right}_earth_NN`, one per stride.
fn hero_footsteps(
    time: Res<Time>,
    state: Res<PlayState>,
    tf: Query<&GlobalTransform>,
    mut st: Local<(Option<Vec3>, f32, bool)>,
    mut sfx: MessageWriter<PlaySfx>,
) {
    let Some(Ok(pos)) = state
        .hero_entity
        .map(|e| tf.get(e).map(GlobalTransform::translation))
    else {
        return;
    };
    let dt = time.delta_secs().max(1e-3);
    if let Some(prev) = st.0 {
        let step = Vec3::new(pos.x - prev.x, 0.0, pos.z - prev.z).length();
        let speed = step / dt;
        if speed > 0.5 && speed < 12.0 {
            let (gait, stride) = if speed > 3.2 {
                ("run", 1.9)
            } else {
                ("walk", 1.2)
            };
            st.1 += step;
            if st.1 >= stride {
                st.1 = 0.0;
                st.2 = !st.2;
                let foot = if st.2 { "left" } else { "right" };
                sfx.write(PlaySfx::new(
                    format!("eff_step_human_{gait}_{foot}_earth"),
                    Some(pos),
                ));
            }
        } else {
            st.1 = 0.0;
        }
    }
    st.0 = Some(pos);
}
