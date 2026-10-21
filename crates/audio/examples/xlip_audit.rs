//! `xlip_audit [DATA_DIR]`: decodes every `.xlip` of the installation and reports the ranges.
use anyhow::Result;
use gothic_audio::lipsync::LipSync;

fn main() -> Result<()> {
    let data = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "D:/SteamLibrary/steamapps/common/Gothic 3/Data".into());
    let lib = gothic_audio::AudioLibrary::open(std::path::Path::new(&data), "German")?;
    let paths: Vec<String> = lib
        .with_vfs(|v| {
            v.iter()
                .map(|(p, _)| p.to_owned())
                .filter(|p| p.ends_with(".xlip"))
                .collect()
        })
        .unwrap_or_default();
    let (mut ok, mut bad, mut max_v, mut frames, mut empty) = (0, 0, 0, 0usize, 0);
    let (mut min_gap, mut max_gap, mut longest) = (f32::MAX, 0.0f32, 0.0f32);
    let mut hist = [0usize; 16];
    for p in &paths {
        match lib.read(p).and_then(|b| LipSync::parse(&b)) {
            Ok(l) => {
                ok += 1;
                empty += usize::from(l.frames.is_empty());
                frames += l.frames.len();
                longest = longest.max(l.duration());
                for f in &l.frames {
                    max_v = max_v.max(f.viseme);
                    hist[(f.viseme as usize).min(15)] += 1;
                }
                for w in l.frames.windows(2) {
                    let g = w[1].time - w[0].time;
                    min_gap = min_gap.min(g);
                    max_gap = max_gap.max(g);
                }
            }
            Err(e) => {
                bad += 1;
                if bad < 4 {
                    eprintln!("{p}: {e}");
                }
            }
        }
    }
    println!(
        "files {} ok {ok} bad {bad} empty {empty} frames {frames}",
        paths.len()
    );
    println!("max viseme {max_v} longest {longest:.1}s gap {min_gap:.3}..{max_gap:.3}s");
    println!("viseme histogram {hist:?}");
    Ok(())
}
