//! Bevy plugin: voice-over, sound effects and the music state machine.
//!
//! Input is message based (`MessageWriter<PlayVoice>` etc.), output messages report subtitles
//! and finished lines. Audio bytes come from the VFS and are handed to Bevy's rodio backend as
//! `AudioSource`s (Ogg Vorbis / WAV decode happens in rodio).
use crate::library::AudioLibrary;
use crate::music::{EXPLORE, NextFragment};
use crate::pcm;
use bevy::app::{App, Plugin, Update};
use bevy::asset::{Assets, Handle};
use bevy::audio::{
    AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, PlaybackMode, PlaybackSettings,
    SpatialAudioSink, SpatialScale, Volume,
};
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::prelude::*;
use bevy::math::Vec3;
use bevy::time::Time;
use bevy::transform::components::{GlobalTransform, Transform};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Play the recording of a dialog (`INFO_*`) or stock-voice (`SVM_*`) string key.
#[derive(Message, Debug, Clone)]
pub struct PlayVoice {
    pub key: String,
    /// Entity the voice comes from. With `spatial`, the voice becomes its child and follows it.
    pub speaker_entity: Option<Entity>,
    /// Preferred voice when a key has several recordings (`player_pc_hero`, a voice class such as
    /// `hum_fat_bearish`, or an NPC name).
    pub speaker_hint: Option<String>,
    pub spatial: bool,
    /// Linear gain on top of master x voice volume.
    pub volume: f32,
}

impl PlayVoice {
    /// Non-positional line (dialog screen, narrator).
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            speaker_entity: None,
            speaker_hint: None,
            spatial: false,
            volume: 1.0,
        }
    }
    /// Positional line emitted by `speaker`.
    pub fn spatial(key: impl Into<String>, speaker: Entity) -> Self {
        Self {
            speaker_entity: Some(speaker),
            spatial: true,
            ..Self::new(key)
        }
    }
    /// Stock voice line (`SaySVM` script command): `voice` is the NPC voice class or name,
    /// `event` the command's `ID1` (`ThoughtSo`, `Teach_Axe_3_01`).
    pub fn svm(voice: &str, event: &str) -> Self {
        Self::new(format!("SVM_{voice}_{event}"))
    }
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.speaker_hint = Some(hint.into());
        self
    }
}

/// Play a sound effect by emitter-template name (`SFX_Env_Waterfall_03`), sample stem or path.
#[derive(Message, Debug, Clone)]
pub struct PlaySfx {
    pub name: String,
    /// World position; `None` plays non-positionally.
    pub position: Option<Vec3>,
    pub volume: f32,
}

impl PlaySfx {
    pub fn new(name: impl Into<String>, position: Option<Vec3>) -> Self {
        Self {
            name: name.into(),
            position,
            volume: 1.0,
        }
    }
}

/// Drive the music state machine. Ids and names are case-insensitive.
#[derive(Message, Debug, Clone, PartialEq)]
pub enum MusicCommand {
    /// A `gCMusicTrigger` id from the music table (`Loc_Myrtana`, `Sit_Fight`, `Stop`, ...).
    Trigger(String),
    /// Region theme (`myrtana`, `varant`, `dungeon`, ...).
    SetLocation(String),
    /// `explore`, `fight` or a story cue (`innos`, `beliar`, `revolution`, ...).
    SetSituation(String),
    SetNight(bool),
    /// Fade out and stay silent until the next location/situation/trigger command.
    Stop,
}

/// Emitted when a voice line starts; carries the subtitle for the UI.
#[derive(Message, Debug, Clone)]
pub struct VoiceStarted {
    pub entity: Entity,
    pub key: String,
    pub speaker_entity: Option<Entity>,
    pub subtitle: Option<String>,
    pub path: String,
    pub lipsync: Option<String>,
    pub duration_secs: Option<f32>,
}

#[derive(Message, Debug, Clone)]
pub struct VoiceFinished {
    pub entity: Entity,
    pub key: String,
    pub speaker_entity: Option<Entity>,
}

/// A requested key has no recording in the selected language.
#[derive(Message, Debug, Clone)]
pub struct VoiceMissing {
    pub key: String,
}

#[derive(Resource, Clone)]
pub struct AudioSettings {
    pub master: f32,
    pub voice: f32,
    pub music: f32,
    pub sfx: f32,
    /// World units per audio meter. Gothic units are centimetres.
    pub units_per_meter: f32,
    pub crossfade_secs: f32,
    /// Region whose music is the fallback for unknown locations.
    pub default_region: String,
    /// Skip the silent first ~10 s every music fragment starts with, so a theme is audible as
    /// soon as it is triggered. Whether the original engine waits through it is [U].
    pub skip_lead_silence: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            master: 1.0,
            voice: 1.0,
            music: 0.6,
            sfx: 1.0,
            units_per_meter: 100.0,
            crossfade_secs: 3.0,
            default_region: "myrtana".into(),
            skip_lead_silence: true,
        }
    }
}

#[derive(Resource, Clone)]
pub struct AudioAssets(pub Arc<AudioLibrary>);

/// Observable music state. `now_playing` is the sample stem of the loudest track.
#[derive(Resource, Debug, Clone)]
pub struct MusicState {
    pub location: String,
    pub situation: String,
    pub night: bool,
    pub stopped: bool,
    pub now_playing: Option<String>,
    dirty: bool,
    current: Option<usize>,
}

impl Default for MusicState {
    fn default() -> Self {
        Self {
            location: String::new(),
            situation: EXPLORE.into(),
            night: false,
            stopped: true,
            now_playing: None,
            dirty: false,
            current: None,
        }
    }
}

impl MusicState {
    /// Apply a command; returns whether the wanted music may have changed.
    pub fn apply(&mut self, lib: &AudioLibrary, cmd: &MusicCommand) -> bool {
        match cmd {
            MusicCommand::Trigger(id) => match lib.music.trigger(id) {
                Some(t) if t.is_stop => self.stopped = true,
                Some(t) => {
                    self.stopped = false;
                    if !t.location.is_empty() {
                        self.location = t.location.to_ascii_lowercase();
                    }
                    if !t.situation.is_empty() {
                        self.situation = t.situation.to_ascii_lowercase();
                    }
                }
                None => return false,
            },
            MusicCommand::SetLocation(l) => {
                self.stopped = false;
                self.location = l.to_ascii_lowercase();
            }
            MusicCommand::SetSituation(s) => {
                self.stopped = false;
                self.situation = s.to_ascii_lowercase();
            }
            MusicCommand::SetNight(n) => self.night = *n,
            MusicCommand::Stop => self.stopped = true,
        }
        self.dirty = true;
        true
    }
}

#[derive(Component, Debug)]
pub struct VoiceLine {
    pub key: String,
    pub speaker_entity: Option<Entity>,
    age: f32,
    expected: f32,
}

#[derive(Component, Debug)]
pub struct SfxSound;

#[derive(Component, Debug)]
pub struct MusicTrack {
    /// Index into `MusicLibrary::fragments`.
    pub fragment: usize,
    pub gain: f32,
    pub target: f32,
    pub fade_secs: f32,
    /// Plays once (variation chain / story cue) instead of looping.
    pub one_shot: bool,
}

#[derive(Resource)]
struct AudioRng(u64);

impl AudioRng {
    fn next(&mut self) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as usize
    }
}

#[derive(Resource, Default)]
struct SfxCache(HashMap<String, Handle<AudioSource>>);

/// Registers messages, resources and systems. Add Bevy's `AudioPlugin` (part of
/// `DefaultPlugins`) yourself; headless tools also need `AssetPlugin` and `TimePlugin`.
pub struct GothicAudioPlugin {
    pub library: Arc<AudioLibrary>,
    pub settings: AudioSettings,
}

impl GothicAudioPlugin {
    pub fn new(library: Arc<AudioLibrary>) -> Self {
        Self {
            library,
            settings: AudioSettings::default(),
        }
    }

    pub fn open(data_dir: &Path, language: &str) -> anyhow::Result<Self> {
        Ok(Self::new(AudioLibrary::open(data_dir, language)?))
    }
}

impl Plugin for GothicAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PlayVoice>()
            .add_message::<PlaySfx>()
            .add_message::<MusicCommand>()
            .add_message::<VoiceStarted>()
            .add_message::<VoiceFinished>()
            .add_message::<VoiceMissing>()
            .insert_resource(AudioAssets(self.library.clone()))
            .insert_resource(self.settings.clone())
            .insert_resource(AudioRng(0x9E37_79B9_7F4A_7C15))
            .init_resource::<SfxCache>()
            .init_resource::<MusicState>()
            .add_systems(
                Update,
                (
                    play_voices,
                    track_voices,
                    play_sfx,
                    music_commands,
                    music_update,
                    music_fade,
                )
                    .chain(),
            );
    }
}

fn volume(v: f32) -> Volume {
    Volume::Linear(v.max(0.0))
}

#[allow(clippy::too_many_arguments)]
fn play_voices(
    mut reader: MessageReader<PlayVoice>,
    assets: Res<AudioAssets>,
    settings: Res<AudioSettings>,
    mut sources: ResMut<Assets<AudioSource>>,
    globals: Query<&GlobalTransform>,
    mut commands: Commands,
    mut started: MessageWriter<VoiceStarted>,
    mut missing: MessageWriter<VoiceMissing>,
) {
    let lib = &assets.0;
    for req in reader.read() {
        let Some(file) = lib.speech.lookup(&req.key, req.speaker_hint.as_deref()) else {
            missing.write(VoiceMissing {
                key: req.key.clone(),
            });
            continue;
        };
        let Ok(bytes) = lib.read(&file.path) else {
            missing.write(VoiceMissing {
                key: req.key.clone(),
            });
            continue;
        };
        let duration = pcm::ogg_duration_secs(&bytes);
        let handle = sources.add(AudioSource { bytes });
        let spatial_parent = req
            .speaker_entity
            .filter(|&e| req.spatial && globals.contains(e));
        let mut pb = PlaybackSettings::ONCE
            .with_volume(volume(settings.master * settings.voice * req.volume));
        if spatial_parent.is_some() {
            pb = pb
                .with_spatial(true)
                .with_spatial_scale(SpatialScale::new(1.0 / settings.units_per_meter));
        }
        let mut e = commands.spawn((
            VoiceLine {
                key: req.key.clone(),
                speaker_entity: req.speaker_entity,
                age: 0.0,
                expected: duration.unwrap_or(30.0),
            },
            AudioPlayer::new(handle),
            pb,
            Transform::default(),
        ));
        if let Some(parent) = spatial_parent {
            e.insert(ChildOf(parent));
        }
        started.write(VoiceStarted {
            entity: e.id(),
            key: req.key.clone(),
            speaker_entity: req.speaker_entity,
            subtitle: lib.subtitle(&req.key),
            path: file.path,
            lipsync: file.lipsync,
            duration_secs: duration,
        });
    }
}

fn track_voices(
    time: Res<Time>,
    mut voices: Query<(
        Entity,
        &mut VoiceLine,
        Option<&AudioSink>,
        Option<&SpatialAudioSink>,
    )>,
    mut commands: Commands,
    mut finished: MessageWriter<VoiceFinished>,
) {
    for (entity, mut line, sink, spatial) in &mut voices {
        line.age += time.delta_secs();
        let done = match (sink, spatial) {
            (Some(s), _) => s.empty(),
            (_, Some(s)) => s.empty(),
            // No sink yet: either loading, or there is no output device. Give up after the
            // expected length so headless runs still emit `VoiceFinished`.
            _ => line.age > line.expected + 1.0,
        };
        if done {
            finished.write(VoiceFinished {
                entity,
                key: line.key.clone(),
                speaker_entity: line.speaker_entity,
            });
            commands.entity(entity).despawn();
        }
    }
}

fn play_sfx(
    mut reader: MessageReader<PlaySfx>,
    assets: Res<AudioAssets>,
    settings: Res<AudioSettings>,
    mut cache: ResMut<SfxCache>,
    mut rng: ResMut<AudioRng>,
    mut sources: ResMut<Assets<AudioSource>>,
    mut commands: Commands,
) {
    let lib = &assets.0;
    for req in reader.read() {
        let def = lib.sfx.get(&req.name);
        let sample = def.map_or(req.name.as_str(), |d| d.sample.as_str());
        let Some(path) = lib
            .sounds
            .resolve(sample, rng.next())
            .or_else(|| lib.sounds.resolve(&req.name, rng.next()))
            .map(str::to_owned)
        else {
            bevy::log::debug!("sfx {:?} not found", req.name);
            continue;
        };
        let handle = match cache.0.get(&path) {
            Some(h) => h.clone(),
            None => {
                let Ok(bytes) = lib.read(&path) else { continue };
                let h = sources.add(AudioSource { bytes });
                cache.0.insert(path.clone(), h.clone());
                h
            }
        };
        let gain = def.map_or(1.0, |d| d.volume) * req.volume;
        let mut pb =
            PlaybackSettings::DESPAWN.with_volume(volume(settings.master * settings.sfx * gain));
        let mut transform = Transform::default();
        if let Some(p) = req.position {
            pb = pb
                .with_spatial(true)
                .with_spatial_scale(SpatialScale::new(1.0 / settings.units_per_meter));
            transform.translation = p;
        }
        commands.spawn((SfxSound, AudioPlayer::new(handle), pb, transform));
    }
}

fn music_commands(
    mut reader: MessageReader<MusicCommand>,
    assets: Res<AudioAssets>,
    mut state: ResMut<MusicState>,
) {
    for cmd in reader.read() {
        state.apply(&assets.0, cmd);
    }
}

fn start_track(
    commands: &mut Commands,
    sources: &mut Assets<AudioSource>,
    lib: &AudioLibrary,
    fragment: usize,
    fade_secs: f32,
    skip_lead_silence: bool,
) -> bool {
    let f = &lib.music.fragments[fragment];
    let Ok(bytes) = lib.read(&f.vfs_path()) else {
        bevy::log::warn!("music fragment {} missing", f.vfs_path());
        return false;
    };
    let one_shot = !matches!(lib.music.next_after(f), NextFragment::Repeat);
    let skip = if skip_lead_silence {
        pcm::leading_silence_secs(&bytes, 60, 30.0)
            .ok()
            .filter(|&s| s > 0.5)
            .map(|s| std::time::Duration::from_secs_f64((s - 0.05).max(0.0)))
    } else {
        None
    };
    let handle = sources.add(AudioSource { bytes });
    let mode = if one_shot {
        PlaybackMode::Once
    } else {
        PlaybackMode::Loop
    };
    commands.spawn((
        MusicTrack {
            fragment,
            gain: 0.0,
            target: 1.0,
            fade_secs,
            one_shot,
        },
        AudioPlayer::new(handle),
        PlaybackSettings {
            mode,
            volume: Volume::Linear(0.0),
            start_position: skip,
            ..PlaybackSettings::ONCE
        },
    ));
    true
}

fn music_update(
    assets: Res<AudioAssets>,
    settings: Res<AudioSettings>,
    mut state: ResMut<MusicState>,
    mut sources: ResMut<Assets<AudioSource>>,
    mut tracks: Query<(&mut MusicTrack, Option<&AudioSink>)>,
    mut commands: Commands,
) {
    let lib = &assets.0;
    // A finished one-shot (variation chain or story cue) advances the chain or ends the cue.
    let mut next: Option<Option<usize>> = None; // Some(Some(i)) = play i, Some(None) = cue over
    for (mut t, sink) in &mut tracks {
        if t.one_shot && t.target > 0.0 && sink.is_some_and(|s| s.empty()) {
            t.target = 0.0;
            t.gain = 0.0;
            let f = &lib.music.fragments[t.fragment];
            next = Some(match lib.music.next_after(f) {
                NextFragment::Play(n) => {
                    lib.music.fragments.iter().position(|x| std::ptr::eq(x, n))
                }
                _ => None,
            });
        }
    }
    match next {
        Some(Some(i)) => {
            if start_track(
                &mut commands,
                &mut sources,
                lib,
                i,
                0.05,
                settings.skip_lead_silence,
            ) {
                state.current = Some(i);
                state.now_playing = Some(stem_of(&lib.music.fragments[i].sample));
            }
        }
        Some(None) => {
            // Story cue finished: return to exploring.
            state.situation = EXPLORE.into();
            state.current = None;
            state.dirty = true;
        }
        None => {}
    }
    if !state.dirty {
        return;
    }
    state.dirty = false;
    let want = if state.stopped {
        None
    } else {
        lib.music
            .resolve(
                &state.location,
                &state.situation,
                state.night,
                &settings.default_region,
            )
            .and_then(|f| lib.music.fragments.iter().position(|x| std::ptr::eq(x, f)))
    };
    // An alternate variation of the playing group counts as the same music.
    let same = match (want, state.current) {
        (Some(w), Some(c)) => {
            let (a, b) = (&lib.music.fragments[w], &lib.music.fragments[c]);
            a.location == b.location && a.situation == b.situation && a.day_time == b.day_time
        }
        (None, None) => true,
        _ => false,
    };
    if same {
        return;
    }
    for (mut t, _) in &mut tracks {
        t.target = 0.0;
        t.fade_secs = settings.crossfade_secs;
    }
    state.current = None;
    state.now_playing = None;
    if let Some(i) = want
        && start_track(
            &mut commands,
            &mut sources,
            lib,
            i,
            settings.crossfade_secs,
            settings.skip_lead_silence,
        )
    {
        state.current = Some(i);
        state.now_playing = Some(stem_of(&lib.music.fragments[i].sample));
    }
}

fn stem_of(sample: &str) -> String {
    sample
        .strip_suffix(".wav")
        .unwrap_or(sample)
        .to_ascii_lowercase()
}

fn music_fade(
    time: Res<Time>,
    assets: Res<AudioAssets>,
    settings: Res<AudioSettings>,
    mut tracks: Query<(Entity, &mut MusicTrack, Option<&mut AudioSink>)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
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
            // Equal-power curve so the cross-fade keeps constant loudness.
            let base = assets.0.music.fragments[t.fragment].volume;
            sink.set_volume(volume(
                t.gain.sqrt() * base * settings.master * settings.music,
            ));
        }
    }
}

/// Minimal windowless app (time, assets, transforms, audio output) for tools and tests; the
/// caller steps it with `app.update()`.
pub fn headless_app(plugin: GothicAudioPlugin) -> App {
    use bevy::app::{PluginsState, TaskPoolPlugin};
    use bevy::asset::AssetPlugin;
    use bevy::audio::AudioPlugin;
    use bevy::diagnostic::FrameCountPlugin;
    use bevy::time::TimePlugin;
    use bevy::transform::TransformPlugin;
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        FrameCountPlugin,
        TimePlugin,
        AssetPlugin::default(),
        TransformPlugin,
        AudioPlugin::default(),
        plugin,
    ));
    while app.plugins_state() == PluginsState::Adding {
        bevy::tasks::tick_global_task_pools_on_main_thread();
    }
    app.finish();
    app.cleanup();
    app
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::test_library;

    fn lib() -> AudioLibrary {
        AudioLibrary::for_tests(test_library())
    }

    #[test]
    fn trigger_and_commands_update_state() {
        let l = lib();
        let mut s = MusicState::default();
        assert!(s.stopped);
        assert!(s.apply(&l, &MusicCommand::Trigger("loc_myrtana".into())));
        assert!(!s.stopped && s.location == "myrtana" && s.dirty);
        assert!(!s.apply(&l, &MusicCommand::Trigger("Loc_Nowhere".into())));
        s.apply(&l, &MusicCommand::SetSituation("FIGHT".into()));
        assert_eq!(s.situation, "fight");
        s.apply(&l, &MusicCommand::Stop);
        assert!(s.stopped);
        s.apply(&l, &MusicCommand::SetLocation("Varant".into()));
        assert!(!s.stopped && s.location == "varant");
    }

    #[test]
    fn rng_is_nonconstant() {
        let mut r = AudioRng(1);
        assert_ne!(r.next(), r.next());
    }
}
