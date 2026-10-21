//! `gothic-audio`'s positional channels switch to enhanced voices (synthetic WAV, no device).
#![cfg(feature = "plugin")]
use bevy::audio::{AudioSource, PlaybackSettings, SpatialListener};
use bevy::prelude::*;
use gothic_audio::Category;
use gothic_audio::plugin::{AudioSettings, Channel, Emitter3d};
use gothic_audio_plus::bridge::{BridgeStats, GothicAudioBridgePlugin};
use gothic_audio_plus::plugin::{
    EmitterBlocking, EnhancedAudio, EnhancedAudioPlugin, EnhancedEmitter, EnhancedSound,
};
use gothic_audio_plus::{Blocking, EnhancedSettings};

/// 0.1 s of a 440 Hz sine, 16-bit mono WAV.
fn wav() -> Vec<u8> {
    let rate = 22_050u32;
    let data: Vec<u8> = (0..rate / 10)
        .flat_map(|i| {
            let s = (i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin();
            ((s * 12_000.0) as i16).to_le_bytes()
        })
        .collect();
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend((36 + data.len() as u32).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(rate.to_le_bytes());
    b.extend((rate * 2).to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend((data.len() as u32).to_le_bytes());
    b.extend(data);
    b
}

fn make_app(enabled: bool) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), TransformPlugin))
        .add_plugins(bevy::audio::AudioPlugin::default())
        .add_plugins((EnhancedAudioPlugin, GothicAudioBridgePlugin))
        .insert_resource(AudioSettings::default())
        .insert_resource(EnhancedAudio {
            settings: EnhancedSettings {
                enabled,
                ..EnhancedSettings::all_on()
            },
            ..Default::default()
        });
    app.world_mut()
        .spawn((SpatialListener::new(0.2), Transform::default()));
    app
}

fn spawn_sfx(app: &mut App, bytes: Vec<u8>) -> Entity {
    let h = app
        .world_mut()
        .resource_mut::<Assets<AudioSource>>()
        .add(AudioSource {
            bytes: bytes.into(),
        });
    app.world_mut()
        .spawn((
            AudioPlayer::new(h),
            PlaybackSettings::DESPAWN.with_spatial(true),
            Transform::from_xyz(300.0, 0.0, 0.0),
            Channel {
                category: Category::Fx,
                base: 0.7,
                audibility: 0.7,
                name: "synthetic".into(),
            },
            Emitter3d {
                min_m: 1.5,
                max_m: 40.0,
                linear: false,
            },
        ))
        .id()
}

#[test]
fn positional_channels_switch_to_enhanced_voices() {
    let mut app = make_app(true);
    let a = spawn_sfx(&mut app, wav());
    let b = spawn_sfx(&mut app, wav());
    app.update();
    let w = app.world();
    for e in [a, b] {
        assert!(w.get::<AudioPlayer<AudioSource>>(e).is_none());
        let player = w
            .get::<AudioPlayer<EnhancedSound>>(e)
            .expect("enhanced player");
        let pb = w.get::<PlaybackSettings>(e).unwrap();
        assert!(!pb.spatial, "the enhanced chain pans itself");
        assert!(
            matches!(pb.volume, bevy::audio::Volume::Linear(v) if (v - 0.7).abs() < 1e-6),
            "group / template volume kept"
        );
        let em = w.get::<EnhancedEmitter>(e).unwrap();
        // Template metres in world units (gothic-audio's default: 100 per metre).
        assert_eq!((em.min_distance, em.max_distance), (150.0, 4000.0));
        assert!(
            w.resource::<Assets<EnhancedSound>>()
                .get(&player.0)
                .is_some()
        );
    }
    // Two plays of one sound: one decode, two voices with their own parameters.
    let pa = &w.get::<AudioPlayer<EnhancedSound>>(a).unwrap().0;
    let pb = &w.get::<AudioPlayer<EnhancedSound>>(b).unwrap().0;
    assert_ne!(pa.id(), pb.id());
    assert_eq!(w.resource::<BridgeStats>().switched, 2);
}

#[test]
fn undecodable_and_disabled_stay_faithful() {
    let mut app = make_app(true);
    let junk = spawn_sfx(&mut app, b"not audio".to_vec());
    app.update();
    assert!(app.world().get::<AudioPlayer<AudioSource>>(junk).is_some());
    assert_eq!(app.world().resource::<BridgeStats>().undecodable, 1);

    let mut off = make_app(false);
    let e = spawn_sfx(&mut off, wav());
    off.update();
    assert!(off.world().get::<AudioPlayer<AudioSource>>(e).is_some());
}

#[test]
fn host_blocking_reaches_the_voice() {
    let mut app = make_app(true);
    let e = spawn_sfx(&mut app, wav());
    app.update();
    let blocked = Blocking {
        obstruction: 1.0,
        occlusion: 0.5,
    };
    app.world_mut()
        .entity_mut(e)
        .insert(EmitterBlocking(blocked));
    app.update();
    let h = app
        .world()
        .get::<AudioPlayer<EnhancedSound>>(e)
        .unwrap()
        .0
        .clone();
    let params = app
        .world()
        .resource::<Assets<EnhancedSound>>()
        .get(&h)
        .unwrap()
        .params();
    let p = *params.read().unwrap();
    assert_eq!(p.blocking, blocked);
    assert!(
        p.direction[0] > 0.9,
        "emitter to the right: {:?}",
        p.direction
    );
}
