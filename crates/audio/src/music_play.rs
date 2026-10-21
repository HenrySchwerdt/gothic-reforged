//! Bevy host of the music [`Sequencer`]: turns [`MusicCommand`]s into triggers, steps the
//! sequencer every frame and executes its actions on `bevy_audio` sinks.
use crate::library::AudioLibrary;
use crate::mix::Category;
use crate::pcm;
use crate::plugin::{AudioAssets, AudioSettings, MusicCommand, MusicState, group_gain, volume};
use crate::sequencer::{Action, Channel, ExitTime, STINGER_CHANNEL, Sequencer, Trigger};
use bevy::asset::Assets;
use bevy::audio::{
    AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, PlaybackMode, PlaybackSettings, Volume,
};
use bevy::ecs::prelude::*;
use bevy::time::Time;

/// The sequencer instance of the app.
#[derive(Resource)]
pub struct MusicSeq(pub Sequencer);

/// A playing music channel (fragment or stinger) driven by the sequencer.
#[derive(Component, Debug)]
pub struct MusicTrack {
    pub channel: Channel,
    /// Fragment index, or the transition index for a stinger.
    pub fragment: usize,
    pub stinger: bool,
    /// Sequencer level: fragment volume times the stinger envelope.
    pub level: f32,
    /// Fade multiplier, moving towards `target` over `fade_secs`.
    pub gain: f32,
    pub target: f32,
    pub fade_secs: f32,
    /// Plain cross-fades (no stinger) keep constant loudness; stinger envelopes are linear.
    pub equal_power: bool,
    /// Where in the sample playback started, seconds.
    pub start_secs: f32,
}

impl MusicTrack {
    /// Linear amplitude before the group volume.
    pub fn amplitude(&self) -> f32 {
        let g = if self.equal_power {
            self.gain.sqrt()
        } else {
            self.gain
        };
        self.level * g
    }
}

/// Maps the high-level commands onto sequencer triggers; table entries (`Loc_*`, `Sit_*`) carry
/// the original timing values.
fn trigger_for(lib: &AudioLibrary, cmd: &MusicCommand) -> Option<Trigger> {
    let table = |id: &str| lib.music.trigger(id).map(Trigger::from_table);
    Some(match cmd {
        MusicCommand::Trigger(id) => table(id)?,
        MusicCommand::SetLocation(l) => {
            table(&format!("Loc_{l}")).unwrap_or_else(|| Trigger::partial(l, "", ""))
        }
        MusicCommand::SetSituation(s) => table(&format!("Sit_{s}")).unwrap_or_else(|| Trigger {
            situation: s.clone(),
            ..Trigger::default()
        }),
        MusicCommand::SetNight(n) => Trigger::partial("", "", if *n { "night" } else { "day" }),
        MusicCommand::Stop => Trigger {
            stop: true,
            exit: ExitTime::Immediately,
            ..Trigger::default()
        },
    })
}

fn stem_of(sample: &str) -> String {
    sample
        .strip_suffix(".wav")
        .unwrap_or(sample)
        .to_ascii_lowercase()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn music_update(
    time: Res<Time>,
    mut reader: MessageReader<MusicCommand>,
    assets: Res<AudioAssets>,
    settings: Res<AudioSettings>,
    mut seq: ResMut<MusicSeq>,
    mut state: ResMut<MusicState>,
    mut sources: ResMut<Assets<AudioSource>>,
    mut tracks: Query<(Entity, &mut MusicTrack)>,
    mut commands: Commands,
) {
    let lib: &AudioLibrary = &assets.0;
    let mut actions = Vec::new();
    for cmd in reader.read() {
        state.apply(lib, cmd);
        if let Some(t) = trigger_for(lib, cmd) {
            actions.extend(seq.0.trigger(&lib.music, &t));
        }
    }
    seq.0.crossfade_secs = settings.crossfade_secs;
    actions.extend(seq.0.advance(&lib.music, time.delta_secs().min(0.25)));
    for action in actions {
        apply(
            lib,
            &settings,
            &mut seq.0,
            action,
            &mut sources,
            &mut tracks,
            &mut commands,
        );
    }
    let st = seq.0.state();
    if state.location != st.location {
        state.location.clone_from(&st.location);
    }
    if state.situation != st.situation {
        state.situation.clone_from(&st.situation);
    }
    state.night = st.day_time == "night";
    state.now_playing = seq
        .0
        .current_fragment()
        .map(|f| stem_of(&lib.music.fragments[f].sample));
    state.stopped = !seq.0.is_playing() && seq.0.pending_events() == 0;
}

fn apply(
    lib: &AudioLibrary,
    settings: &AudioSettings,
    seq: &mut Sequencer,
    action: Action,
    sources: &mut Assets<AudioSource>,
    tracks: &mut Query<(Entity, &mut MusicTrack)>,
    commands: &mut Commands,
) {
    match action {
        Action::Start {
            channel,
            fragment,
            start_ms,
            volume: level,
            fade_in_secs,
            skip_silence,
        } => {
            let f = &lib.music.fragments[fragment];
            let Ok(bytes) = lib.read(&f.vfs_path()) else {
                bevy::log::warn!("music fragment {} missing", f.vfs_path());
                return;
            };
            for (e, t) in tracks.iter() {
                if t.channel == channel && !t.stinger && t.target > 0.0 {
                    commands.entity(e).despawn();
                }
            }
            let start = if start_ms > 0.0 {
                Some(std::time::Duration::from_secs_f64(start_ms / 1000.0))
            } else if skip_silence && settings.skip_lead_silence {
                let skip = pcm::leading_silence_secs(&bytes, 60, 30.0)
                    .ok()
                    .filter(|&s| s > 0.5)
                    .map(|s| std::time::Duration::from_secs_f64((s - 0.05).max(0.0)));
                if let Some(d) = skip {
                    seq.seek(channel, d.as_secs_f64() * 1000.0);
                }
                skip
            } else {
                None
            };
            let handle = sources.add(AudioSource { bytes });
            commands.spawn((
                MusicTrack {
                    channel,
                    fragment,
                    stinger: false,
                    level,
                    gain: if fade_in_secs > 0.0 { 0.0 } else { 1.0 },
                    target: 1.0,
                    fade_secs: fade_in_secs.max(0.01),
                    equal_power: fade_in_secs > 0.0,
                    start_secs: start.map_or(0.0, |d| d.as_secs_f32()),
                },
                AudioPlayer::new(handle),
                PlaybackSettings {
                    mode: PlaybackMode::Once,
                    volume: Volume::Linear(0.0),
                    start_position: start,
                    ..PlaybackSettings::ONCE
                },
            ));
        }
        Action::StartStinger {
            transition,
            volume: level,
        } => {
            let t = &lib.music.transitions[transition];
            let Ok(bytes) = lib.read(&t.vfs_path()) else {
                return;
            };
            let handle = sources.add(AudioSource { bytes });
            commands.spawn((
                MusicTrack {
                    channel: STINGER_CHANNEL,
                    fragment: transition,
                    stinger: true,
                    level,
                    gain: 1.0,
                    target: 1.0,
                    fade_secs: 0.01,
                    equal_power: false,
                    start_secs: 0.0,
                },
                AudioPlayer::new(handle),
                PlaybackSettings {
                    mode: PlaybackMode::Once,
                    volume: Volume::Linear(0.0),
                    ..PlaybackSettings::ONCE
                },
            ));
        }
        Action::Volume {
            channel,
            volume: level,
        } => {
            for (_, mut t) in tracks.iter_mut() {
                if t.channel == channel && !t.stinger && t.target > 0.0 {
                    t.level = level;
                }
            }
        }
        Action::Stop { channel, fade_secs } => {
            for (e, mut t) in tracks.iter_mut() {
                if t.channel != channel || t.target <= 0.0 {
                    continue;
                }
                if fade_secs <= 0.0 {
                    commands.entity(e).despawn();
                } else {
                    t.target = 0.0;
                    t.fade_secs = fade_secs;
                    t.equal_power = !t.stinger;
                }
            }
        }
    }
}

/// Moves the fade multipliers and sets the sink volumes (group volume from `ge3.INI`).
pub(crate) fn music_fade(
    time: Res<Time>,
    settings: Res<AudioSettings>,
    mut tracks: Query<(Entity, &mut MusicTrack, Option<&mut AudioSink>)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    let group = group_gain(&settings, Category::Music);
    for (entity, mut t, sink) in &mut tracks {
        let step = dt / t.fade_secs.max(0.01);
        t.gain = if t.gain < t.target {
            (t.gain + step).min(t.target)
        } else {
            (t.gain - step).max(t.target)
        };
        if t.target <= 0.0 && t.gain <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        if let Some(mut sink) = sink {
            sink.set_volume(volume(t.amplitude() * group));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::test_library;

    #[test]
    fn commands_map_to_triggers() {
        let lib = AudioLibrary::for_tests(test_library());
        let t = trigger_for(&lib, &MusicCommand::SetLocation("myrtana".into())).unwrap();
        assert_eq!(t.location, "Myrtana"); // the table row `Loc_Myrtana`
        let t = trigger_for(&lib, &MusicCommand::SetLocation("braga".into())).unwrap();
        assert_eq!((t.location.as_str(), t.situation.as_str()), ("braga", ""));
        let t = trigger_for(&lib, &MusicCommand::SetSituation("fight".into())).unwrap();
        assert_eq!(t.situation, "fight");
        let t = trigger_for(&lib, &MusicCommand::SetNight(true)).unwrap();
        assert_eq!(t.day_time, "night");
        assert!(trigger_for(&lib, &MusicCommand::Stop).unwrap().stop);
        assert!(trigger_for(&lib, &MusicCommand::Trigger("nope".into())).is_none());
    }

    #[test]
    fn plain_crossfades_keep_loudness_and_stinger_fades_are_linear() {
        let mut t = MusicTrack {
            channel: 0,
            fragment: 0,
            stinger: false,
            level: 1.0,
            gain: 0.25,
            target: 1.0,
            fade_secs: 1.0,
            equal_power: true,
            start_secs: 0.0,
        };
        assert!((t.amplitude() - 0.5).abs() < 1e-6);
        t.equal_power = false;
        assert!((t.amplitude() - 0.25).abs() < 1e-6);
    }
}
