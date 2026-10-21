//! Audio wiring of the play mode: region and day/night music, fight music exit, ambient
//! climate, footsteps of the hero and of nearby characters, melee sounds and lip-sync for
//! speaking characters.
//!
//! Everything else (voices, mixing, rolloff, channel budget, the music sequencer, ambient
//! one-shots) lives in `gothic_audio`.
use super::events::CombatLog;
use super::{NpcActor, NpcExtra, PlayState, Roster};
use bevy::prelude::*;
use gothic_animation::LipSyncStart;
use gothic_audio::ambient::DayTime;
use gothic_audio::lipsync::LipSync;
use gothic_audio::{
    AmbientControl, AudioAssets, MusicCommand, PlaySfx, VoiceStarted,
};
use gothic_runtime::{CoreEvent, CoreRes, Event};
use gothic_sim::sim::EventKind;
use std::collections::HashMap;

/// NPC footsteps are only played this close to the hero (metres); the original mixes by
/// rolloff, this keeps the number of voices down.
const NPC_STEP_RANGE_M: f32 = 30.0;
const MAX_NPC_STEPPERS: usize = 8;

pub fn register(app: &mut App) {
    app.add_systems(
        Update,
        (
            region_music,
            day_night,
            fight_music_end,
            hero_footsteps,
            npc_footsteps,
            combat_sfx,
            lipsync_for_voices,
            lip_shots,
        ),
    );
}

/// Ardea lies in Myrtana: start its explore theme once.
fn region_music(mut done: Local<bool>, mut music: MessageWriter<MusicCommand>) {
    if !*done {
        *done = true;
        music.write(MusicCommand::SetLocation("myrtana".into()));
    }
}

/// Day and night: the game clock selects the music variant (`SetNight`) and the ambient
/// one-shots (`eEAmbientDayTime`).
fn day_night(
    core: Res<CoreRes>,
    mut control: ResMut<AmbientControl>,
    mut music: MessageWriter<MusicCommand>,
    mut last: Local<Option<bool>>,
) {
    let day_time = DayTime::from_hour(core.0.sim.clock.hour_of_day());
    control.day_time = day_time;
    let night = day_time == DayTime::Night;
    if *last != Some(night) {
        // The very first call only records the state (the location trigger starts the music).
        if last.is_some() {
            music.write(MusicCommand::SetNight(night));
        } else if night {
            music.write(MusicCommand::SetNight(true));
        }
        *last = Some(night);
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

/// One stride counter per walker.
#[derive(Default, Clone, Copy)]
struct Stride {
    prev: Option<Vec3>,
    dist: f32,
    left: bool,
}

impl Stride {
    /// Advances by the movement since the last call; returns the footstep sound to play (gait
    /// from the speed: walk up to 3.2 m/s, run above; strides 1.2 / 1.9 m).
    fn step(&mut self, pos: Vec3, dt: f32) -> Option<String> {
        let prev = self.prev.replace(pos)?;
        let step = Vec3::new(pos.x - prev.x, 0.0, pos.z - prev.z).length();
        let speed = step / dt.max(1e-3);
        if !(0.5..12.0).contains(&speed) {
            self.dist = 0.0;
            return None;
        }
        let (gait, stride) = if speed > 3.2 {
            ("run", 1.9)
        } else {
            ("walk", 1.2)
        };
        self.dist += step;
        if self.dist < stride {
            return None;
        }
        self.dist = 0.0;
        self.left = !self.left;
        let foot = if self.left { "left" } else { "right" };
        Some(format!("eff_step_human_{gait}_{foot}_earth"))
    }
}

/// Hero footsteps: `eff_step_human_{walk|run}_{left|right}_earth_NN`, one per stride.
fn hero_footsteps(
    time: Res<Time>,
    state: Res<PlayState>,
    tf: Query<&GlobalTransform>,
    mut st: Local<Stride>,
    mut sfx: MessageWriter<PlaySfx>,
) {
    let Some(Ok(pos)) = state
        .hero_entity
        .map(|e| tf.get(e).map(GlobalTransform::translation))
    else {
        return;
    };
    if let Some(name) = st.step(pos, time.delta_secs()) {
        sfx.write(PlaySfx::new(name, Some(pos)));
    }
}

/// Footsteps of the nearest walking characters (the brawl capture plays about 750 FX per
/// minute, a third of them steps).
fn npc_footsteps(
    time: Res<Time>,
    state: Res<PlayState>,
    npcs: Query<(Entity, &GlobalTransform, &NpcExtra), With<NpcActor>>,
    tf: Query<&GlobalTransform>,
    mut strides: Local<HashMap<Entity, Stride>>,
    mut sfx: MessageWriter<PlaySfx>,
) {
    let Some(Ok(hero)) = state
        .hero_entity
        .map(|e| tf.get(e).map(GlobalTransform::translation))
    else {
        return;
    };
    let dt = time.delta_secs();
    let mut near: Vec<(f32, Entity, Vec3)> = npcs
        .iter()
        .filter(|(_, _, x)| !x.voice.is_empty())
        .map(|(e, g, _)| (g.translation().distance(hero), e, g.translation()))
        .filter(|(d, _, _)| *d < NPC_STEP_RANGE_M)
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    near.truncate(MAX_NPC_STEPPERS);
    strides.retain(|e, _| near.iter().any(|n| n.1 == *e));
    for (_, e, pos) in near {
        if let Some(name) = strides.entry(e).or_default().step(pos, dt) {
            sfx.write(PlaySfx::new(name, Some(pos)));
        }
    }
}

/// Melee: a weapon whoosh at the attacker for every swing, the impact at the victim (flesh, or
/// metal on metal when the blow was ignored). Names from the FMOD capture of the Ardea brawl.
fn combat_sfx(
    mut events: MessageReader<CoreEvent>,
    core: Res<CoreRes>,
    roster: Res<Roster>,
    state: Res<PlayState>,
    tf: Query<&GlobalTransform>,
    mut sfx: MessageWriter<PlaySfx>,
) {
    let pos_of = |id| {
        let entity = if id == core.0.hero {
            state.hero_entity
        } else {
            roster.by_actor.get(&id).copied()
        };
        entity
            .and_then(|e| tf.get(e).ok())
            .map(GlobalTransform::translation)
    };
    for e in events.read() {
        let Event::Sim(kind) = &e.0 else { continue };
        match kind {
            EventKind::Hit {
                attacker,
                victim,
                damage,
                ignored,
                ..
            } => {
                if let Some(p) = pos_of(*attacker) {
                    sfx.write(PlaySfx::new("sfx_ani_fight_whoosh_normal", Some(p)));
                }
                if let Some(p) = pos_of(*victim) {
                    let name = if ignored.is_some() || *damage == 0 {
                        "sfx_col_fight_wh_me_me_01"
                    } else {
                        "sfx_col_fight_wh_me_fl"
                    };
                    sfx.write(PlaySfx::new(name, Some(p)));
                }
            }
            EventKind::Whiff { attacker, .. } => {
                if let Some(p) = pos_of(*attacker) {
                    sfx.write(PlaySfx::new("sfx_ani_fight_whoosh_normal", Some(p)));
                }
            }
            _ => {}
        }
    }
}

/// A voice line started on a character: decode its `.xlip` and hand the entries to the facial
/// layer (`gothic_animation::face`).
fn lipsync_for_voices(
    mut started: MessageReader<VoiceStarted>,
    assets: Option<Res<AudioAssets>>,
    mut out: MessageWriter<LipSyncStart>,
    shots: Option<Res<LipShots>>,
    mut commands: Commands,
) {
    let Some(assets) = assets else { return };
    for v in started.read() {
        let (Some(entity), Some(path)) = (v.speaker_entity, v.lipsync.as_deref()) else {
            continue;
        };
        let Ok(bytes) = assets.0.read(path) else {
            continue;
        };
        let Ok(lip) = LipSync::parse(&bytes) else {
            continue;
        };
        out.write(LipSyncStart {
            entity,
            frames: lip.frames.iter().map(|f| (f.time, f.viseme)).collect(),
        });
        // `G3_LIPSHOTS=1`: screenshots of the first speaking character at three different
        // mouth shapes (`.local/lipsync-N.png`).
        if shots.is_none() && std::env::var_os("G3_LIPSHOTS").is_some() {
            let times = distinct_mouth_times(&lip);
            if times.len() == 3 {
                commands.insert_resource(LipShots { times, taken: 0 });
            }
        }
    }
}

/// Screenshot schedule for the lip-sync check.
#[derive(Resource)]
struct LipShots {
    times: Vec<f32>,
    taken: usize,
}

/// Three moments (seconds into the line) with different visemes, at least 0.3 s apart.
fn distinct_mouth_times(lip: &LipSync) -> Vec<f32> {
    let mut chosen: Vec<(f32, u32)> = Vec::new();
    for f in &lip.frames {
        if f.time < 0.4 || chosen.iter().any(|c| c.1 == f.viseme) {
            continue;
        }
        if chosen.last().is_none_or(|c| f.time - c.0 >= 0.3) {
            chosen.push((f.time, f.viseme));
        }
        if chosen.len() == 3 {
            break;
        }
    }
    chosen.iter().map(|c| c.0 + 0.02).collect()
}

fn lip_shots(
    shots: Option<ResMut<LipShots>>,
    faces: Query<&gothic_animation::FaceRig>,
    mut commands: Commands,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    let Some(mut shots) = shots else { return };
    // Only while a character is speaking with a facial rig.
    let Some(face) = faces.iter().next() else { return };
    if shots.taken >= shots.times.len() {
        return;
    }
    // The face rig's own clock is the lip-sync clock.
    if face.time() >= shots.times[shots.taken] {
        shots.taken += 1;
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(format!(".local/lipsync-{}.png", shots.taken)));
    }
}
