//! `cargo run -p gothic-audio --example say -- <dialog key> [options]`
//!
//! Looks up the voice recording of a string key (`INFO_...` dialog line or `SVM_...` stock
//! line), prints the subtitle and decoded audio statistics, then plays it through the default
//! output device. Silence is detected from the decoded samples (RMS), since playback cannot be
//! heard by the tool.
//!
//! Options: `--lang German|English` (default German), `--speaker HINT`, `--data DIR`,
//! `--no-play` (decode and report only), `--spatial` (voice comes from an NPC 4 m to the right).
use anyhow::{Context, Result, bail};
use bevy::app::Update;
use bevy::audio::{AudioSink, AudioSinkPlayback, SpatialAudioSink, SpatialListener};
use bevy::ecs::message::MessageReader;
use bevy::ecs::message::Messages;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::ResMut;
use bevy::transform::components::{GlobalTransform, Transform};
use gothic_audio::{
    AudioLibrary, GothicAudioPlugin, PlayVoice, VoiceFinished, VoiceMissing, VoiceStarted,
    headless_app, pcm,
};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const DEFAULT_DATA: &str = "D:/SteamLibrary/steamapps/common/Gothic 3/Data";

fn main() -> Result<()> {
    let mut key = None;
    let mut spatial = false;
    let (mut lang, mut speaker, mut data, mut play) =
        ("German".to_owned(), None, DEFAULT_DATA.to_owned(), true);
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--lang" => lang = it.next().context("--lang needs a value")?,
            "--speaker" => speaker = Some(it.next().context("--speaker needs a value")?),
            "--data" => data = it.next().context("--data needs a value")?,
            "--no-play" => play = false,
            "--spatial" => spatial = true,
            s if s.starts_with("--") => bail!("unknown option {s}"),
            _ => key = Some(a),
        }
    }
    let key = key
        .context("usage: say <dialog key> [--lang German|English] [--speaker HINT] [--no-play]")?;
    let lib = AudioLibrary::open(&PathBuf::from(&data), &lang)?;
    println!(
        "language {lang}: {} speech recordings indexed",
        lib.speech.len()
    );
    let candidates = lib.speech.candidates(&key);
    let Some(file) = lib.speech.lookup(&key, speaker.as_deref()) else {
        bail!("no {lang} voice recording for key {key:?}");
    };
    println!("key       {key}  ({} recording(s))", candidates.len());
    for c in &candidates {
        println!("  candidate {}  [speaker {}]", c.path, c.speaker);
    }
    println!("chosen    {}  [speaker {}]", file.path, file.speaker);
    println!("lipsync   {}", file.lipsync.as_deref().unwrap_or("-"));
    println!(
        "subtitle  {}",
        lib.subtitle(&key).unwrap_or_else(|| "(no text)".into())
    );

    let bytes = lib.read(&file.path)?;
    let audio = pcm::decode(&bytes)?;
    let st = audio.stats();
    println!(
        "decoded   {:.2} s, {} Hz, {} ch, {} frames, rms {:.4}, peak {:.3} -> {}",
        st.duration_secs,
        audio.sample_rate,
        audio.channels,
        st.frames,
        st.rms,
        st.peak,
        if st.is_silent() {
            "SILENT"
        } else {
            "non-silent"
        }
    );
    if st.is_silent() {
        bail!("decoded audio is silent");
    }
    if !play {
        return Ok(());
    }

    let mut app = headless_app(GothicAudioPlugin::new(lib));
    app.add_systems(Update, report);
    app.init_resource::<Done>();
    let mut req = PlayVoice::new(key.clone());
    req.speaker_hint.clone_from(&speaker);
    let speaker_hint = speaker;
    if spatial {
        // Gothic world units are centimetres: 400 units = 4 m to the right of the listener.
        let npc = app
            .world_mut()
            .spawn((
                Transform::from_xyz(400.0, 0.0, 0.0),
                GlobalTransform::default(),
            ))
            .id();
        app.world_mut()
            .spawn((SpatialListener::new(0.2), Transform::default()));
        req = PlayVoice::spatial(key.clone(), npc);
        req.speaker_hint.clone_from(&speaker_hint);
    }
    app.world_mut()
        .resource_mut::<Messages<PlayVoice>>()
        .write(req);
    let limit = Instant::now() + Duration::from_secs_f64(st.duration_secs + 4.0);
    let mut sink_seen = false;
    let mut max_pos = Duration::ZERO;
    while !app.world().resource::<Done>().0 && Instant::now() < limit {
        app.update();
        let mut q = app.world_mut().query::<&AudioSink>();
        for s in q.iter(app.world()) {
            sink_seen = true;
            max_pos = max_pos.max(s.position());
        }
        let mut q = app.world_mut().query::<&SpatialAudioSink>();
        for s in q.iter(app.world()) {
            sink_seen = true;
            max_pos = max_pos.max(s.position());
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    println!(
        "playback  output device sink {}; furthest position {:.2} s",
        if sink_seen {
            "present"
        } else {
            "ABSENT (no audio device?)"
        },
        max_pos.as_secs_f64()
    );
    Ok(())
}

#[derive(Resource, Default)]
struct Done(bool);

fn report(
    mut started: MessageReader<VoiceStarted>,
    mut finished: MessageReader<VoiceFinished>,
    mut missing: MessageReader<VoiceMissing>,
    mut done: ResMut<Done>,
) {
    for s in started.read() {
        println!(
            "started   {} ({:?} s) subtitle: {}",
            s.path,
            s.duration_secs,
            s.subtitle.as_deref().unwrap_or("-")
        );
    }
    for f in finished.read() {
        println!("finished  {}", f.key);
        done.0 = true;
    }
    for m in missing.read() {
        println!("missing   {}", m.key);
        done.0 = true;
    }
}
