//! Bevy plugin: voice-over, sound effects and the music state machine.
//!
//! Input is message based (`MessageWriter<PlayVoice>` etc.), output messages report subtitles
//! and finished lines. Audio bytes come from the VFS and are handed to Bevy's rodio backend as
//! `AudioSource`s (Ogg Vorbis / WAV decode happens in rodio).
use crate::library::AudioLibrary;
use crate::mix::{self, Category, Tap};
use crate::music::{EXPLORE, NextFragment};
use crate::pcm;
use bevy::app::{App, Plugin, Update};
use bevy::asset::{Assets, Handle};
use bevy::audio::{
    AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, PlaybackMode, PlaybackSettings,
    SpatialAudioSink, SpatialListener, SpatialScale, Volume,
};
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::prelude::*;
use bevy::math::Vec3;
use bevy::time::Time;
use bevy::transform::components::{GlobalTransform, Transform};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

fn is_bark(key: &str) -> bool {
    key.len() > 4 && key[..4].eq_ignore_ascii_case("SVM_")
}

/// FMOD `set3DMinMaxDistance` of every voice in the original (500 / 1 000 000 cm).
pub const VOICE_MIN_M: f32 = 5.0;
pub const VOICE_MAX_M: f32 = 10_000.0;

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
    /// `Fx` (footsteps, hits; priority 3) or `Ambient` (birds, water; priority 4).
    pub category: Category,
}

impl PlaySfx {
    pub fn new(name: impl Into<String>, position: Option<Vec3>) -> Self {
        Self {
            name: name.into(),
            position,
            volume: 1.0,
            category: Category::Fx,
        }
    }
    pub fn ambient(mut self) -> Self {
        self.category = Category::Ambient;
        self
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
    /// Ambient emitter group (birds, water, wind, fire).
    pub ambient: f32,
    /// FMOD `System::set3DSettings` rolloff scale (`1.0` in the original).
    pub rolloff_scale: f32,
    /// FMOD software channel budget (`System::init(64, ..)` in the original).
    pub max_channels: usize,
    /// Simultaneous stock-voice barks (`SVM_*`); the original capture peaks at 9.
    pub max_barks: usize,
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
            // Group volumes captured from the original (`Sound.*Volume` of ge3.INI / 255, applied
            // through FMOD channel groups): see AUDIO_FORMATS.md "Original mix".
            master: 1.0,
            voice: 1.0,
            music: 56.0 / 255.0,
            sfx: 205.0 / 255.0,
            ambient: 1.0,
            rolloff_scale: 1.0,
            max_channels: crate::mix::MAX_CHANNELS,
            max_barks: 9,
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

/// A sound occupying a (virtual) FMOD channel: group, priority, loudness and a name for the tap.
#[derive(Component, Debug)]
pub struct Channel {
    pub category: Category,
    /// Group x user volume x template volume (no distance).
    pub base: f32,
    /// `base` x distance rolloff, refreshed every frame.
    pub audibility: f32,
    pub name: String,
}

/// FMOD 3D attributes of a channel: distances in metres (original data is centimetres / 100).
#[derive(Component, Debug)]
pub struct Emitter3d {
    pub min_m: f32,
    pub max_m: f32,
    pub linear: bool,
}

#[derive(Resource, Default)]
struct TapRes(Option<Tap>);

/// World units per rodio spatial unit. rodio attenuates by 1/d^2 beyond one unit (and its ear
/// formula assumes unit-scale worlds); making one unit 1000 m keeps that term clamped at 1 so
/// the pan is rodio's and the distance law is FMOD's (`mix::inverse_gain`, applied in
/// `rolloff_3d`).
const SPATIAL_UNIT_M: f32 = 1000.0;

fn spatial_scale(settings: &AudioSettings) -> SpatialScale {
    SpatialScale::new(1.0 / (settings.units_per_meter * SPATIAL_UNIT_M))
}

fn group_gain(s: &AudioSettings, c: Category) -> f32 {
    s.master
        * match c {
            Category::Voice => s.voice,
            Category::Music => s.music,
            Category::Fx => s.sfx,
            Category::Ambient => s.ambient,
        }
}

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
            .insert_resource(TapRes(Tap::from_env()))
            .init_resource::<SfxCache>()
            .init_resource::<MusicState>()
            .add_systems(
                Update,
                (
                    fix_listener_ears,
                    play_voices,
                    track_voices,
                    play_sfx,
                    music_commands,
                    music_update,
                    music_fade,
                    rolloff_3d,
                    limit_channels,
                    tap_system,
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
    mut finished: MessageWriter<VoiceFinished>,
    playing: Query<&VoiceLine>,
) {
    let lib = &assets.0;
    // Stock-voice barks (`SVM_*`): one at a time per speaker, and at most `MAX_BARKS` at once
    // (the original capture peaks at 9 simultaneous voices in a 66-NPC brawl).
    let mut barks: Vec<Option<Entity>> = playing
        .iter()
        .filter(|v| is_bark(&v.key))
        .map(|v| v.speaker_entity)
        .collect();
    for req in reader.read() {
        if is_bark(&req.key)
            && (barks.len() >= settings.max_barks
                || req.speaker_entity.is_some_and(|e| barks.contains(&Some(e))))
        {
            finished.write(VoiceFinished {
                entity: Entity::PLACEHOLDER,
                key: req.key.clone(),
                speaker_entity: req.speaker_entity,
            });
            continue;
        }
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
        let base = group_gain(&settings, Category::Voice) * req.volume;
        let mut pb = PlaybackSettings::ONCE.with_volume(volume(base));
        if spatial_parent.is_some() {
            pb = pb
                .with_spatial(true)
                .with_spatial_scale(spatial_scale(&settings));
        }
        if is_bark(&req.key) {
            barks.push(req.speaker_entity);
        }
        let mut e = commands.spawn((
            VoiceLine {
                key: req.key.clone(),
                speaker_entity: req.speaker_entity,
                age: 0.0,
                expected: duration.unwrap_or(30.0),
            },
            Channel {
                category: Category::Voice,
                base,
                audibility: base,
                name: file.path.clone(),
            },
            AudioPlayer::new(handle),
            pb,
            Transform::default(),
        ));
        if let Some(parent) = spatial_parent {
            // FMOD: min 500 cm, max 1 000 000 cm for every voice (captured).
            e.insert((
                ChildOf(parent),
                Emitter3d {
                    min_m: VOICE_MIN_M,
                    max_m: VOICE_MAX_M,
                    linear: false,
                },
            ));
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
        let base = group_gain(&settings, req.category) * gain;
        let mut pb = PlaybackSettings::DESPAWN.with_volume(volume(base));
        let mut transform = Transform::default();
        let mut e = commands.spawn((SfxSound, AudioPlayer::new(handle)));
        e.insert(Channel {
            category: req.category,
            base,
            audibility: base,
            name: path.clone(),
        });
        if let Some(p) = req.position {
            pb = pb
                .with_spatial(true)
                .with_spatial_scale(spatial_scale(&settings));
            transform.translation = p;
            // Template distances are centimetres; fall back to the captured footstep / ambient
            // defaults (min 150 cm resp. 500 cm, effectively unbounded max).
            let (dmin, dmax) = match def {
                Some(d) if d.min_distance > 0.0 => {
                    (d.min_distance, d.max_distance.max(d.min_distance))
                }
                _ if req.category == Category::Ambient => (500.0, 1.0e6),
                _ => (150.0, 1.0e9),
            };
            e.insert(Emitter3d {
                min_m: dmin * 0.01,
                max_m: dmax * 0.01,
                linear: false,
            });
        }
        e.insert((pb, transform));
    }
}

/// rodio's `Spatial` source computes the ear gain of the ear *farther* from the emitter as the
/// larger one (verified in `rodio-0.22 source/spatial.rs`: `left_diff_modifier` grows with
/// `left_dist - right_dist`), so with Bevy's default ears a voice on the right is louder in the
/// left channel. Swapping the ear offsets of every listener corrects it.
fn fix_listener_ears(mut listeners: Query<&mut SpatialListener, Added<SpatialListener>>) {
    for mut l in &mut listeners {
        let l = &mut *l;
        std::mem::swap(&mut l.left_ear_offset, &mut l.right_ear_offset);
    }
}

/// FMOD distance attenuation on top of rodio's pan: `volume = base x rolloff(distance)`.
fn rolloff_3d(
    settings: Res<AudioSettings>,
    listener: Query<&GlobalTransform, With<SpatialListener>>,
    mut q: Query<(
        &GlobalTransform,
        &mut Channel,
        Option<&Emitter3d>,
        Option<&mut SpatialAudioSink>,
    )>,
) {
    let ear = listener.iter().next().map(GlobalTransform::translation);
    for (gt, mut ch, em, sink) in &mut q {
        let (Some(em), Some(ear)) = (em, ear) else {
            ch.audibility = ch.base;
            continue;
        };
        let d = (gt.translation() - ear).length() / settings.units_per_meter;
        let r = if em.linear {
            mix::linear_gain(d, em.min_m, em.max_m)
        } else {
            mix::inverse_gain(d, em.min_m, em.max_m, settings.rolloff_scale)
        };
        ch.audibility = ch.base * r;
        if let Some(mut sink) = sink {
            sink.set_volume(volume(ch.audibility));
        }
    }
}

/// FMOD software channel budget: with more than `max_channels` sounds the least important
/// (priority, then quietest) are stopped, like FMOD virtualising them.
fn limit_channels(
    settings: Res<AudioSettings>,
    q: Query<(Entity, &Channel, Option<&VoiceLine>)>,
    mut commands: Commands,
    mut finished: MessageWriter<VoiceFinished>,
) {
    let n = q.iter().count();
    if n <= settings.max_channels {
        return;
    }
    let mut all: Vec<_> = q.iter().collect();
    all.sort_by(|a, b| {
        (b.1.category.priority(), -b.1.audibility)
            .partial_cmp(&(a.1.category.priority(), -a.1.audibility))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (e, _, voice) in all.into_iter().take(n - settings.max_channels) {
        if let Some(v) = voice {
            finished.write(VoiceFinished {
                entity: e,
                key: v.key.clone(),
                speaker_entity: v.speaker_entity,
            });
        }
        commands.entity(e).despawn();
    }
}

#[derive(Default)]
struct TapState {
    /// Spawned this frame: their transform is not propagated yet, log on the next one.
    pending: Vec<Entity>,
    live: HashMap<Entity, (f32, f32)>,
    music: HashMap<Entity, f32>,
    next_stat: f32,
}

/// `GR_AUDIO_TAP=path`: log start / gain change / end of every voice, sfx and music track.
#[allow(clippy::too_many_arguments)]
fn tap_system(
    tap: Res<TapRes>,
    time: Res<Time>,
    settings: Res<AudioSettings>,
    assets: Res<AudioAssets>,
    mut st: Local<TapState>,
    listener: Query<&GlobalTransform, With<SpatialListener>>,
    chans: Query<(Entity, &GlobalTransform, &Channel, Option<&Emitter3d>)>,
    tracks: Query<(Entity, &MusicTrack)>,
) {
    let Some(tap) = &tap.0 else { return };
    let t = time.elapsed_secs();
    let lis = listener.iter().next();
    let mut seen = Vec::new();
    for (e, gt, ch, em) in &chans {
        seen.push(e);
        let pan = match (lis, em) {
            (Some(l), Some(_)) => {
                let d = (gt.translation() - l.translation()).normalize_or_zero();
                d.dot(*l.right())
            }
            _ => 0.0,
        };
        let id = e.to_bits();
        match st.live.get(&e) {
            None if !st.pending.contains(&e) => {
                st.pending.push(e);
            }
            None => {
                st.pending.retain(|&p| p != e);
                let dist = lis.map_or(0.0, |l| (gt.translation() - l.translation()).length())
                    / settings.units_per_meter;
                tap.log(
                    t,
                    ch.category.name(),
                    id,
                    &ch.name,
                    ch.audibility,
                    pan,
                    &format!("start d={dist:.2}"),
                );
                st.live.insert(e, (ch.audibility, pan));
            }
            Some(&(g, p)) => {
                if (ch.audibility - g).abs() > 0.02 * g.max(0.05) || (pan - p).abs() > 0.1 {
                    tap.log(
                        t,
                        ch.category.name(),
                        id,
                        &ch.name,
                        ch.audibility,
                        pan,
                        "gain",
                    );
                    st.live.insert(e, (ch.audibility, pan));
                }
            }
        }
    }
    let gone: Vec<Entity> = st
        .live
        .keys()
        .filter(|e| !seen.contains(e))
        .copied()
        .collect();
    for e in gone {
        st.live.remove(&e);
        tap.log(t, "end", e.to_bits(), "", 0.0, 0.0, "");
    }
    let lib = &assets.0;
    let mut mseen = Vec::new();
    for (e, tr) in &tracks {
        mseen.push(e);
        let f = &lib.music.fragments[tr.fragment];
        let g = tr.gain.sqrt() * f.volume * group_gain(&settings, Category::Music);
        let last = st.music.get(&e).copied();
        if last.is_none_or(|l| (l - g).abs() > 0.01) {
            let ex = if last.is_none() { "start" } else { "gain" };
            tap.log(t, "music", e.to_bits(), &f.vfs_path(), g, 0.0, ex);
            st.music.insert(e, g);
        }
    }
    let mgone: Vec<Entity> = st
        .music
        .keys()
        .filter(|e| !mseen.contains(e))
        .copied()
        .collect();
    for e in mgone {
        st.music.remove(&e);
        tap.log(t, "end", e.to_bits(), "", 0.0, 0.0, "");
    }
    if t >= st.next_stat {
        st.next_stat = t + 1.0;
        let nv = chans
            .iter()
            .filter(|c| c.2.category == Category::Voice)
            .count();
        tap.log(
            t,
            "stat",
            0,
            "",
            chans.iter().count() as f32,
            nv as f32,
            "channels,voices",
        );
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
