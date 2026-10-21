//! `music_walk [DATA_DIR]`: runs the music sequencer offline (no audio device) against the real
//! music table and prints the action timeline of a scripted walk, for comparison with the
//! captured FMOD trace (`.local/research/audio/fmod-trace.jsonl`).
//!
//! Script: the story cue `revolution` starts at 0 s and loops through its follow trigger; at
//! 150 s the location becomes `myrtana` exploring, at 260 s a fight starts, at 330 s exploring
//! again, at 400 s the day turns to night.
use anyhow::Result;
use gothic_audio::sequencer::{Action, Entrance, Sequencer, Trigger};
use gothic_audio::{AudioLibrary, MusicLibrary};

fn describe(lib: &MusicLibrary, a: &Action) -> String {
    match a {
        Action::Start {
            channel,
            fragment,
            start_ms,
            fade_in_secs,
            skip_silence,
            ..
        } => format!(
            "start  ch{channel} {} at {:.1}s fade {fade_in_secs}s skip {skip_silence}",
            lib.fragments[*fragment].sample,
            start_ms / 1000.0
        ),
        Action::StartStinger { transition, .. } => {
            format!("stinger {}", lib.transitions[*transition].sample)
        }
        Action::Volume { .. } => String::new(),
        Action::Stop { channel, fade_secs } => format!("stop   ch{channel} fade {fade_secs}s"),
    }
}

fn main() -> Result<()> {
    let data = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "D:/SteamLibrary/steamapps/common/Gothic 3/Data".into());
    let lib = AudioLibrary::open(std::path::Path::new(&data), "German")?;
    let music = &lib.music;
    let mut seq = Sequencer::new(music, 7);
    let begin = |loc: &str, sit: &str, day: &str| Trigger {
        entrance: Entrance::Begin,
        ..Trigger::partial(loc, sit, day)
    };
    let script: Vec<(f32, Trigger)> = vec![
        (0.0, begin("myrtana", "revolution", "day")),
        (150.0, begin("", "explore", "")),
        (260.0, {
            let mut t = begin("", "fight", "");
            t.exit = gothic_audio::sequencer::ExitTime::NextExit;
            t
        }),
        (330.0, begin("", "explore", "")),
        (400.0, Trigger::partial("", "", "night")),
    ];
    let (mut t, dt, mut next) = (0.0f32, 0.05f32, 0usize);
    let (mut stingers, mut starts) = (0, 0);
    while t < 520.0 {
        let mut actions = Vec::new();
        while next < script.len() && script[next].0 <= t {
            actions.extend(seq.trigger(music, &script[next].1));
            println!("{t:7.1}s  trigger {:?}", script[next].1.situation);
            next += 1;
        }
        actions.extend(seq.advance(music, dt));
        for a in &actions {
            match a {
                Action::StartStinger { .. } => stingers += 1,
                Action::Start { .. } => starts += 1,
                _ => {}
            }
            let d = describe(music, a);
            if !d.is_empty() {
                println!("{t:7.1}s  {d}");
            }
        }
        t += dt;
    }
    println!("fragments started {starts}, stingers {stingers}");
    for l in &seq.log {
        println!("{l}");
    }
    Ok(())
}
