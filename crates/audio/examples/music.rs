//! `cargo run -p gothic-audio --example music -- [options]`
//!
//! Plays the Myrtana exploration theme (Ardea lies in Myrtana, trigger `Loc_Myrtana`) for 10 s
//! through the music state machine, after decoding the fragment and checking the decoded length
//! against the `Duration` stored in the music table.
//!
//! Options: `--secs N` (default 10), `--location L` (default Myrtana), `--night`,
//! `--fight-at S` (switch to `Sit_Fight` after S seconds to exercise the cross-fade), `--data DIR`.
use anyhow::{Context, Result, bail};
use bevy::audio::{AudioSink, AudioSinkPlayback};
use bevy::ecs::message::Messages;
use gothic_audio::{
    AudioLibrary, GothicAudioPlugin, MusicCommand, MusicState, MusicTrack, headless_app, pcm,
};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const DEFAULT_DATA: &str = "D:/SteamLibrary/steamapps/common/Gothic 3/Data";

fn main() -> Result<()> {
    let (mut secs, mut loc, mut night, mut fight_at, mut data) = (
        10.0f64,
        "Myrtana".to_owned(),
        false,
        None::<f64>,
        DEFAULT_DATA.to_owned(),
    );
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--secs" => secs = it.next().context("--secs")?.parse()?,
            "--location" => loc = it.next().context("--location")?,
            "--night" => night = true,
            "--fight-at" => fight_at = Some(it.next().context("--fight-at")?.parse()?),
            "--data" => data = it.next().context("--data")?,
            other => bail!("unknown option {other}"),
        }
    }
    let lib = AudioLibrary::open(&PathBuf::from(&data), "German")?;
    println!(
        "music table: {} fragments, {} transitions, {} triggers; locations: {}",
        lib.music.fragments.len(),
        lib.music.transitions.len(),
        lib.music.triggers.len(),
        lib.music.locations().join(", ")
    );
    let frag = lib
        .music
        .resolve(&loc, "explore", night, "myrtana")
        .context("no explore fragment")?
        .clone();
    println!(
        "fragment  {} (table duration {} ms)",
        frag.vfs_path(),
        frag.duration_ms
    );
    let bytes = lib.read(&frag.vfs_path())?;
    let t0 = Instant::now();
    let audio = pcm::decode(&bytes)?;
    let st = audio.stats();
    println!(
        "decoded   {:.2} s, {} Hz, {} ch, rms {:.4}, peak {:.3} -> {} ({:.1} s to decode, {} KiB ogg)",
        st.duration_secs,
        audio.sample_rate,
        audio.channels,
        st.rms,
        st.peak,
        if st.is_silent() {
            "SILENT"
        } else {
            "non-silent"
        },
        t0.elapsed().as_secs_f64(),
        bytes.len() / 1024
    );
    if let Some(h) = pcm::ogg_duration_secs(&bytes) {
        println!("header    {h:.2} s (granule position)");
    }
    println!(
        "table says {:.2} s; difference {:+.2} s",
        f64::from(frag.duration_ms) / 1000.0,
        st.duration_secs - f64::from(frag.duration_ms) / 1000.0
    );
    if st.is_silent() {
        bail!("decoded music is silent");
    }
    // Every fragment starts with ~10 s of digital silence; the plugin skips it by default.
    let lead = pcm::leading_silence_secs(&bytes, 60, 30.0)?;
    println!("lead-in   {lead:.2} s of silence before the first audible sample (skipped on play)");
    // RMS per second of the played window: shows the audio is music, not silence.
    let per = audio.sample_rate as usize * audio.channels as usize;
    let skip = (((lead - 0.05).max(0.0) * f64::from(audio.sample_rate)) as usize)
        * audio.channels as usize;
    let windows: Vec<String> = audio.samples[skip.min(audio.samples.len())..]
        .chunks(per)
        .take(secs.ceil() as usize)
        .map(|c| {
            let ms = c
                .iter()
                .map(|&s| (f64::from(s) / 32768.0).powi(2))
                .sum::<f64>()
                / c.len() as f64;
            format!("{:.3}", ms.sqrt())
        })
        .collect();
    println!("rms/sec   {} (from t = {lead:.2} s)", windows.join(" "));

    let mut app = headless_app(GothicAudioPlugin::new(lib));
    {
        let mut m = app.world_mut().resource_mut::<Messages<MusicCommand>>();
        m.write(MusicCommand::SetNight(night));
        m.write(MusicCommand::SetLocation(loc));
    }
    let start = Instant::now();
    let mut switched = false;
    let mut max_pos = Duration::ZERO;
    let mut sinks = 0;
    let mut peak_tracks = 0;
    while start.elapsed().as_secs_f64() < secs {
        app.update();
        if let Some(t) = fight_at
            && !switched
            && start.elapsed().as_secs_f64() >= t
        {
            switched = true;
            app.world_mut()
                .resource_mut::<Messages<MusicCommand>>()
                .write(MusicCommand::Trigger("Sit_Fight".into()));
            println!(
                "t={:.1}s  -> Sit_Fight (cross-fade)",
                start.elapsed().as_secs_f64()
            );
        }
        let mut q = app.world_mut().query::<(&MusicTrack, Option<&AudioSink>)>();
        let tracks: Vec<_> = q
            .iter(app.world())
            .map(|(_, s)| s.map(|s| s.position()))
            .collect();
        peak_tracks = peak_tracks.max(tracks.len());
        for p in tracks.into_iter().flatten() {
            sinks += 1;
            max_pos = max_pos.max(p);
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    let state = app.world().resource::<MusicState>();
    println!(
        "state     now_playing {:?}, location {:?}, situation {:?}",
        state.now_playing, state.location, state.situation
    );
    println!(
        "playback  max simultaneous tracks {peak_tracks}; sink samples {sinks}; furthest position {:.2} s -> {}",
        max_pos.as_secs_f64(),
        if sinks > 0 {
            "output sink active"
        } else {
            "NO SINK (no audio device?)"
        }
    );
    Ok(())
}
