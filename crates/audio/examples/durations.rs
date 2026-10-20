//! `durations [data dir] < names.txt > name<TAB>seconds`: playback length of original sound
//! names as FMOD logged them (`SVM_X_German.wav`, `eff_step_*.wav`, `*.wav` music) or VFS paths.
use anyhow::Result;
use gothic_audio::{AudioLibrary, pcm};
use std::io::BufRead;
use std::path::Path;

fn main() -> Result<()> {
    let data = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "D:/SteamLibrary/steamapps/common/Gothic 3/Data".into());
    let lib = AudioLibrary::open(Path::new(&data), "German")?;
    for line in std::io::stdin().lock().lines() {
        let name = line?;
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let low = name.to_ascii_lowercase();
        let stem = low.rsplit_once('.').map_or(low.as_str(), |(s, _)| s);
        let stem = stem.rsplit('/').next().unwrap_or(stem);
        let cands = [
            format!("german_ogg/{stem}.ogg"),
            format!("fragments/{stem}.ogg"),
            format!("transitions/{stem}.ogg"),
        ];
        let path = cands
            .iter()
            .find(|c| lib.read(c).is_ok())
            .cloned()
            .or_else(|| lib.sounds.find(stem).map(str::to_owned));
        let dur = path.and_then(|p| lib.read(&p).ok()).and_then(|b| {
            pcm::ogg_duration_secs(&b)
                .map(f64::from)
                .or_else(|| pcm::decode(&b).ok().map(|p| p.stats().duration_secs))
        });
        println!(
            "{name}\t{}",
            dur.map_or("-".to_owned(), |d| format!("{d:.3}"))
        );
    }
    Ok(())
}
