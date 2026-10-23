//! Opt-in bridge from `gothic-audio`: positional channels it spawns (sound effects and ambient
//! emitters, everything with an [`Emitter3d`] and an `AudioPlayer<AudioSource>`) are switched
//! to enhanced voices before `bevy_audio` starts them. Voices (dialogue) stay on the faithful
//! path. The template distances, the channel budget and the group volume of `gothic-audio` are
//! kept: the channel's base volume goes into the playback volume, its distances into an
//! [`EnhancedEmitter`].
//!
//! Decoded sounds are cached per source asset; the first play of a sound decodes it on the
//! main thread (a hitch for long files, acceptable for an opt-in).
use crate::plugin::{EnhancedAudio, EnhancedEmitter, EnhancedSound};
use bevy::audio::{AudioSource, PlaybackMode, PlaybackSettings, Volume};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use gothic_audio::plugin::{AudioSettings, Channel, Emitter3d, VoiceLine};

/// Add after `GothicAudioPlugin` and `EnhancedAudioPlugin`.
pub struct GothicAudioBridgePlugin;

impl Plugin for GothicAudioBridgePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DecodedCache>()
            .init_resource::<BridgeStats>()
            .add_systems(
                PostUpdate,
                switch_channels.before(TransformSystems::Propagate),
            );
    }
}

/// Decoded sounds per `gothic-audio` source (`None`: not decodable, stays faithful).
#[derive(Resource, Default)]
struct DecodedCache(HashMap<AssetId<AudioSource>, Option<Handle<EnhancedSound>>>);

/// How many channels went through the enhanced path (for overlays and tests).
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct BridgeStats {
    pub switched: u64,
    pub undecodable: u64,
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn switch_channels(
    mut commands: Commands,
    audio: Res<EnhancedAudio>,
    settings: Option<Res<AudioSettings>>,
    mut cache: ResMut<DecodedCache>,
    mut stats: ResMut<BridgeStats>,
    sources: Res<Assets<AudioSource>>,
    mut sounds: ResMut<Assets<EnhancedSound>>,
    fresh: Query<
        (
            Entity,
            &AudioPlayer<AudioSource>,
            &PlaybackSettings,
            &Emitter3d,
            Option<&Channel>,
        ),
        (Added<Emitter3d>, Without<VoiceLine>),
    >,
) {
    if !audio.settings.enabled {
        return;
    }
    let (units, rolloff) = settings
        .as_deref()
        .map_or((1.0, 1.0), |s| (s.units_per_meter, s.rolloff_scale));
    for (e, player, pb, em, channel) in &fresh {
        let looped = matches!(pb.mode, PlaybackMode::Loop);
        let id = player.0.id();
        let decoded = cache.0.entry(id).or_insert_with(|| {
            let bytes = sources.get(id)?;
            EnhancedSound::decode(&bytes.bytes, looped)
                .ok()
                .map(|s| sounds.add(s))
        });
        let Some(handle) = decoded.clone() else {
            stats.undecodable += 1;
            continue;
        };
        let Some(sound) = sounds.get(&handle).map(|s| {
            let mut s = s.instance();
            s.looped = looped;
            s
        }) else {
            continue;
        };
        let base = channel.map_or(1.0, |c| c.base);
        commands
            .entity(e)
            .remove::<(AudioPlayer<AudioSource>, PlaybackSettings)>()
            .insert((
                AudioPlayer(sounds.add(sound)),
                PlaybackSettings {
                    volume: Volume::Linear(base.max(0.0)),
                    spatial: false,
                    ..*pb
                },
                EnhancedEmitter {
                    min_distance: em.min_m * units,
                    max_distance: em.max_m * units,
                    rolloff: if em.linear { 1.0 } else { rolloff },
                },
            ));
        stats.switched += 1;
    }
}
