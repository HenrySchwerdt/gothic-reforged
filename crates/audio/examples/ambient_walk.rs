//! `ambient_walk [CLIMATE ENVIRONMENT DAYTIME SECONDS] [DATA_DIR]`: runs the ambient sequencer
//! offline against the real `g3.ams` with the real sample lengths and prints how often each
//! sound fires, for comparison with the FMOD capture of the original (about 29 bird one-shots
//! per minute at the Ardea start: climate 0, environment 0, day).
use anyhow::Result;
use gothic_audio::ambient::AmbientSequencer;
use gothic_audio::{AudioLibrary, pcm};
use std::collections::BTreeMap;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let num = |i: usize, default: u8| args.get(i).and_then(|a| a.parse().ok()).unwrap_or(default);
    let (climate, environment, day_time) = (num(0, 0), num(1, 0), num(2, 1));
    let seconds: f32 = args.get(3).and_then(|a| a.parse().ok()).unwrap_or(600.0);
    let data = args.get(4).map_or(
        "D:/SteamLibrary/steamapps/common/Gothic 3/Data",
        String::as_str,
    );
    let lib = AudioLibrary::open(std::path::Path::new(data), "German")?;
    let mut seq = AmbientSequencer::new(11);
    seq.climate = climate;
    seq.environment = environment;
    seq.day_time = day_time;
    // Real length of every ambient sample.
    let length = |i: usize| -> f32 {
        let s = &lib.ambient.sounds[i];
        lib.sounds
            .resolve(&s.sample, 0)
            .and_then(|p| lib.read(p).ok())
            .and_then(|b| pcm::ogg_duration_secs(&b))
            .unwrap_or(2.0) as f32
    };
    let (mut t, dt) = (0.0f32, 0.016f32);
    let mut playing: Vec<f32> = Vec::new();
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    let mut times = Vec::new();
    while t < seconds {
        playing.retain(|&e| e > t);
        if let Some(sp) = seq.process(&lib.ambient, dt, playing.len()) {
            playing.push(t + length(sp.sound));
            *counts
                .entry(lib.ambient.sounds[sp.sound].sample.clone())
                .or_default() += 1;
            times.push(t);
        }
        t += dt;
    }
    let total: u32 = counts.values().sum();
    println!(
        "climate {climate} environment {environment} daytime {day_time}: {total} one-shots in {seconds:.0} s = {:.1} per minute",
        total as f32 / seconds * 60.0
    );
    let gaps: Vec<f32> = times.windows(2).map(|w| w[1] - w[0]).collect();
    if !gaps.is_empty() {
        let mean = gaps.iter().sum::<f32>() / gaps.len() as f32;
        let (min, max) = gaps
            .iter()
            .fold((f32::MAX, 0.0f32), |a, &g| (a.0.min(g), a.1.max(g)));
        println!("gap between spawns: mean {mean:.2} s, min {min:.2}, max {max:.2}");
    }
    for (name, n) in &counts {
        println!("  {name:24} {n}");
    }
    Ok(())
}
