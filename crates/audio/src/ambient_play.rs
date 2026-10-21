//! Bevy hosts of the ambient system: random one-shots (`gCAmbientSequencer`), the three water
//! beds, world emitters (`eCAudioEmitter_PS`) and the effect command `PlaySound`.
use crate::ambient::{AmbientSequencer, Climate, DayTime, WaterBeds, dominant_environment};
use crate::emitter::{EmitterAction, EmitterConfig, EmitterState, select_sample};
use crate::mix::Category;
use crate::plugin::{
    AudioAssets, AudioRng, AudioSettings, Channel, Emitter3d, PlaySfx, SfxCache, SfxSound,
    group_gain, spatial_scale, volume,
};
use crate::rng::Rng;
use bevy::asset::{Assets, Handle};
use bevy::audio::{
    AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, PlaybackSettings, SpatialListener,
};
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::prelude::*;
use bevy::math::Vec3;
use bevy::time::Time;
use bevy::transform::components::{GlobalTransform, Transform};

/// What the host tells the ambient system about the world (the original reads the session:
/// climate, time of day and the ambient volumes around the camera).
#[derive(Resource, Debug, Clone)]
pub struct AmbientControl {
    pub enabled: bool,
    pub climate: Climate,
    pub day_time: DayTime,
    /// Number of ambient volumes of each environment type the camera is inside of.
    pub counters: [u32; 6],
    /// Camera distance to the ocean, cm (`None` = no ocean in range).
    pub ocean_distance_cm: Option<f32>,
    /// Normalised river distance (`None` = no river).
    pub river: Option<f32>,
}

impl Default for AmbientControl {
    fn default() -> Self {
        Self {
            enabled: true,
            climate: Climate::Mediterranean,
            day_time: DayTime::Day,
            counters: [0; 6],
            ocean_distance_cm: None,
            river: None,
        }
    }
}

#[derive(Resource)]
pub struct AmbientSeq(pub AmbientSequencer);

/// Statistics of the ambient system (smoke reports, tuning against the capture).
#[derive(Resource, Default, Debug)]
pub struct AmbientStats {
    pub spawned: u32,
    pub missing: u32,
    pub by_sample: Vec<(String, u32)>,
}

/// A random ambient one-shot.
#[derive(Component)]
pub struct AmbientShot;

/// One of the looped water beds (0 coast, 1 ocean waves, 2 river).
#[derive(Component)]
pub struct WaterBed(pub u8);

const BED_FILES: [&str; 3] = ["water_coast_01", "water_oceanwaves_01", "water_river_01"];

/// Distance ring (metres) around the listener where random one-shots are placed. The original
/// uses a random ambient emitter entity of the world; without placed emitters this keeps the
/// sound outside the minimum distance (5 m) at plausible tree-top height.
const RING_M: (f32, f32) = (8.0, 40.0);
const RING_HEIGHT_M: (f32, f32) = (2.0, 12.0);

fn sound_handle(
    lib: &crate::library::AudioLibrary,
    cache: &mut SfxCache,
    sources: &mut Assets<AudioSource>,
    path: &str,
) -> Option<Handle<AudioSource>> {
    if let Some(h) = cache.0.get(path) {
        return Some(h.clone());
    }
    let bytes = lib.read(path).ok()?;
    let h = sources.add(AudioSource { bytes });
    cache.0.insert(path.to_owned(), h.clone());
    Some(h)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn ambient_update(
    time: Res<Time>,
    assets: Res<AudioAssets>,
    settings: Res<AudioSettings>,
    control: Res<AmbientControl>,
    mut seq: ResMut<AmbientSeq>,
    mut stats: ResMut<AmbientStats>,
    mut rng: ResMut<AudioRng>,
    mut cache: ResMut<SfxCache>,
    mut sources: ResMut<Assets<AudioSource>>,
    listener: Query<&GlobalTransform, With<SpatialListener>>,
    shots: Query<(), With<AmbientShot>>,
    mut commands: Commands,
) {
    if !control.enabled {
        return;
    }
    let lib = &assets.0;
    let Some(ear) = listener.iter().next().map(GlobalTransform::translation) else {
        return;
    };
    let s = &mut seq.0;
    s.climate = control.climate as u8;
    s.day_time = control.day_time as u8;
    s.environment = dominant_environment(&control.counters);
    let Some(spawn) = s.process(&lib.ambient, time.delta_secs(), shots.iter().count()) else {
        return;
    };
    let sound = &lib.ambient.sounds[spawn.sound];
    let Some(path) = lib
        .sounds
        .resolve(&sound.sample, rng.next())
        .map(str::to_owned)
    else {
        stats.missing += 1;
        return;
    };
    let Some(handle) = sound_handle(lib, &mut cache, &mut sources, &path) else {
        stats.missing += 1;
        return;
    };
    let mut r = Rng::new(rng.next() as u64 | 1);
    let angle = r.range(0.0, std::f32::consts::TAU);
    let dist = r.range(RING_M.0, RING_M.1);
    let height = r.range(RING_HEIGHT_M.0, RING_HEIGHT_M.1);
    let at = ear + Vec3::new(angle.cos() * dist, height, angle.sin() * dist);
    let base = group_gain(&settings, Category::Ambient) * sound.volume;
    commands.spawn((
        SfxSound,
        AmbientShot,
        AudioPlayer::new(handle),
        PlaybackSettings::DESPAWN
            .with_volume(volume(base))
            .with_spatial(true)
            .with_spatial_scale(spatial_scale(&settings)),
        Transform::from_translation(at),
        Channel {
            category: Category::Ambient,
            base,
            audibility: base,
            name: path,
        },
        Emitter3d {
            min_m: sound.min_distance * 0.01,
            max_m: sound.max_distance * 0.01,
            linear: false,
        },
    ));
    stats.spawned += 1;
    let name = sound.sample.clone();
    match stats.by_sample.iter_mut().find(|(n, _)| *n == name) {
        Some((_, c)) => *c += 1,
        None => stats.by_sample.push((name, 1)),
    }
}

/// Port of gCAmbientSequencer::UpdateWaterSounds (Game:0x101bab90): 2D looped beds whose volume
/// ramps with the distance to the ocean / river.
#[allow(clippy::too_many_arguments)]
pub(crate) fn water_beds(
    assets: Res<AudioAssets>,
    settings: Res<AudioSettings>,
    control: Res<AmbientControl>,
    mut cache: ResMut<SfxCache>,
    mut sources: ResMut<Assets<AudioSource>>,
    mut beds: Query<(Entity, &WaterBed, Option<&mut AudioSink>, &mut Channel)>,
    mut commands: Commands,
) {
    let gains = if control.enabled {
        WaterBeds::at(control.ocean_distance_cm, control.river)
    } else {
        WaterBeds::default()
    };
    let wanted = [gains.coast, gains.waves, gains.river];
    let lib = &assets.0;
    let group = group_gain(&settings, Category::Ambient);
    let mut have = [false; 3];
    for (e, bed, sink, mut ch) in &mut beds {
        let g = wanted[usize::from(bed.0)];
        if g <= 0.001 {
            commands.entity(e).despawn();
            continue;
        }
        have[usize::from(bed.0)] = true;
        ch.base = g * group;
        ch.audibility = ch.base;
        if let Some(mut sink) = sink {
            sink.set_volume(volume(ch.base));
        }
    }
    for (i, &g) in wanted.iter().enumerate() {
        if g <= 0.001 || have[i] {
            continue;
        }
        let Some(path) = lib.sounds.resolve(BED_FILES[i], 0).map(str::to_owned) else {
            continue;
        };
        let Some(handle) = sound_handle(lib, &mut cache, &mut sources, &path) else {
            continue;
        };
        commands.spawn((
            SfxSound,
            WaterBed(i as u8),
            AudioPlayer::new(handle),
            PlaybackSettings::LOOP.with_volume(volume(g * group)),
            Channel {
                category: Category::Ambient,
                base: g * group,
                audibility: g * group,
                name: path,
            },
        ));
    }
}

/// A world sound emitter: the `eCAudioEmitter_PS` property set on an entity with a transform.
#[derive(Component, Debug)]
pub struct AudioEmitterPs {
    pub config: EmitterConfig,
    state: EmitterState,
}

impl AudioEmitterPs {
    pub fn new(config: EmitterConfig) -> Self {
        Self {
            config,
            state: EmitterState::default(),
        }
    }
}

/// The sound an emitter currently plays (child of the emitter).
#[derive(Component)]
pub struct EmitterVoice;

/// Emitters farther than this from the listener are not processed (metres). At the default
/// minimum distance of 10 m the inverse law leaves 4 percent there.
const EMITTER_RANGE_M: f32 = 250.0;

#[allow(clippy::too_many_arguments)]
pub(crate) fn emitter_update(
    time: Res<Time>,
    assets: Res<AudioAssets>,
    settings: Res<AudioSettings>,
    mut rng: ResMut<AudioRng>,
    mut cache: ResMut<SfxCache>,
    mut sources: ResMut<Assets<AudioSource>>,
    listener: Query<&GlobalTransform, With<SpatialListener>>,
    mut emitters: Query<(
        Entity,
        &GlobalTransform,
        &mut AudioEmitterPs,
        Option<&Children>,
    )>,
    voices: Query<(), With<EmitterVoice>>,
    mut commands: Commands,
) {
    let Some(ear) = listener.iter().next().map(GlobalTransform::translation) else {
        return;
    };
    let lib = &assets.0;
    let mut r = Rng::new(rng.next() as u64 | 1);
    for (entity, gt, mut em, children) in &mut emitters {
        let d = (gt.translation() - ear).length() / settings.units_per_meter.max(1.0);
        if d > EMITTER_RANGE_M {
            continue;
        }
        let playing = children.is_some_and(|c| c.iter().any(|e| voices.contains(e)));
        let em = &mut *em;
        let action = em
            .state
            .step(&em.config, time.delta_secs(), playing, &mut r);
        match action {
            EmitterAction::None => {}
            EmitterAction::Kill => commands.entity(entity).despawn(),
            EmitterAction::Spawn { looped } => {
                let cfg = &em.config;
                let Some(path) = lib
                    .sounds
                    .resolve(&cfg.sample, rng.next())
                    .map(str::to_owned)
                else {
                    continue;
                };
                let Some(handle) = sound_handle(lib, &mut cache, &mut sources, &path) else {
                    continue;
                };
                let base = group_gain(&settings, Category::Ambient) * cfg.volume;
                let mut pb = if looped {
                    PlaybackSettings::LOOP
                } else {
                    PlaybackSettings::DESPAWN
                };
                pb = pb
                    .with_volume(volume(base))
                    .with_spatial(true)
                    .with_spatial_scale(spatial_scale(&settings));
                commands.spawn((
                    SfxSound,
                    EmitterVoice,
                    AudioPlayer::new(handle),
                    pb,
                    Transform::default(),
                    ChildOf(entity),
                    Channel {
                        category: Category::Ambient,
                        base,
                        audibility: base,
                        name: path,
                    },
                    Emitter3d {
                        min_m: cfg.min_distance * 0.01,
                        max_m: cfg.max_distance * 0.01,
                        linear: false,
                    },
                ));
            }
        }
    }
}

/// Effect command `gCEffectCommandPlaySound` (Game:0x100582c0): a weighted sample list with a
/// possible silent outcome, played in the FX group at the entity position.
#[derive(Message, Debug, Clone)]
pub struct PlayEffectSound {
    /// `(sample, weight)` entries of the command.
    pub samples: Vec<(String, f32)>,
    pub position: Option<Vec3>,
    pub volume: f32,
    pub min_distance_cm: f32,
    pub max_distance_cm: f32,
}

pub(crate) fn effect_sounds(
    mut reader: MessageReader<PlayEffectSound>,
    mut rng: ResMut<AudioRng>,
    mut out: MessageWriter<PlaySfx>,
) {
    let mut r = Rng::new(rng.next() as u64 | 1);
    for cmd in reader.read() {
        let weights: Vec<f32> = cmd.samples.iter().map(|s| s.1).collect();
        let Some(i) = select_sample(&weights, &mut r) else {
            continue;
        };
        let mut sfx = PlaySfx::new(cmd.samples[i].0.clone(), cmd.position);
        sfx.volume = cmd.volume;
        sfx.distances_cm = Some((cmd.min_distance_cm, cmd.max_distance_cm));
        out.write(sfx);
    }
}
