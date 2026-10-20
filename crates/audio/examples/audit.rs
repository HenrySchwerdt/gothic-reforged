//! `cargo run --release -p gothic-audio --example audit -- [--data DIR] [--lang German,English] [--sample N]`
//!
//! Inventory of the audio archives and dialog-key coverage:
//!
//! * file counts by archive / folder class / extension,
//! * for each language: how many `INFO_*` / `SVM_*` string keys (with text in that language) and
//!   how many `Say*` script keys have a voice recording, unmatched examples, voice files that no
//!   key points to,
//! * decode check of every Nth voice file (non-silent, plausible length), `--sample 1` = all.
use anyhow::{Context, Result};
use gothic_audio::{AudioLibrary, pcm};
use gothic_gameplay::GameDatabase;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

const DEFAULT_DATA: &str = "D:/SteamLibrary/steamapps/common/Gothic 3/Data";

fn main() -> Result<()> {
    let mut data = DEFAULT_DATA.to_owned();
    let mut langs = vec!["German".to_owned(), "English".to_owned()];
    let mut sample = 200usize;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--data" => data = it.next().context("--data")?,
            "--lang" => {
                langs = it
                    .next()
                    .context("--lang")?
                    .split(',')
                    .map(str::to_owned)
                    .collect()
            }
            "--sample" => sample = it.next().context("--sample")?.parse()?,
            other => anyhow::bail!("unknown option {other}"),
        }
    }
    let data = PathBuf::from(data);

    // ---- inventory -------------------------------------------------------------------------
    let lib = AudioLibrary::open(&data, "German")?;
    let mut inv: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    lib.with_vfs(|vfs| {
        for (path, slot) in vfs.iter() {
            let ext = path.rsplit('.').next().unwrap_or("");
            if !matches!(ext, "ogg" | "wav" | "xlip") {
                continue;
            }
            let archive = vfs.archive_name(slot).to_ascii_lowercase();
            let top = path.split('/').next().unwrap_or("").to_owned();
            // `Sound.pak` entries have many sub folders: group by first folder only.
            *inv.entry((archive, top, ext.to_owned())).or_default() += 1;
        }
    });
    println!("== audio files visible in the VFS (patch overlays applied) ==");
    for ((archive, top, ext), n) in &inv {
        println!("{n:6}  {archive:<20} {top:<28} .{ext}");
    }
    println!(
        "sample index: {} sounds; sfx emitter templates: {}; music: {} fragments / {} transitions / {} triggers",
        lib.sounds.len(),
        lib.sfx.len(),
        lib.music.fragments.len(),
        lib.music.transitions.len(),
        lib.music.triggers.len()
    );

    // ---- coverage --------------------------------------------------------------------------
    for lang in &langs {
        let lib = AudioLibrary::open(&data, lang)?;
        let db = GameDatabase::load_install(&data, lang)?;
        let li = db
            .strings
            .language_index(lang)
            .with_context(|| format!("no {lang} column"))?;
        println!("\n== {lang}: {} speech recordings ==", lib.speech.len());

        let mut totals: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        let mut missing: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        let mut used: HashSet<String> = HashSet::new();
        for e in &db.strings.entries {
            let family = if e.key.starts_with("INFO_") {
                "INFO_*"
            } else if e.key.starts_with("SVM_") {
                "SVM_*"
            } else {
                continue;
            };
            if e.texts.get(li).is_none_or(|t| t.trim().is_empty()) {
                continue;
            }
            let c = lib.speech.candidates(&e.key);
            let t = totals.entry(family).or_default();
            t.0 += 1;
            if c.is_empty() {
                let m = missing.entry(family).or_default();
                if m.len() < 8 {
                    m.push(e.key.clone());
                }
            } else {
                t.1 += 1;
                used.extend(c.into_iter().map(|f| f.path));
            }
        }
        for (family, (n, hit)) in &totals {
            println!(
                "{family:7} keys with {lang} text: {n:6}  with voice file: {hit:6}  ({:.1} %)",
                100.0 * *hit as f64 / (*n).max(1) as f64
            );
            if let Some(m) = missing.get(family) {
                println!("        e.g. no voice: {}", m.join(", "));
            }
        }

        // Keys actually spoken by info scripts.
        let mut say_keys: HashSet<&str> = HashSet::new();
        let mut cmd_counts: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        for info in &db.infos {
            for s in &info.script {
                if s.command.starts_with("Say") && !s.text.is_empty() {
                    say_keys.insert(&s.text);
                    let c = cmd_counts.entry(s.command.as_str()).or_default();
                    c.0 += 1;
                    if lib.speech.has(&s.text) {
                        c.1 += 1;
                    }
                }
            }
        }
        let hit = say_keys.iter().filter(|k| lib.speech.has(k)).count();
        println!(
            "info-script Say* keys (distinct): {}  with voice file: {hit} ({:.1} %)",
            say_keys.len(),
            100.0 * hit as f64 / say_keys.len().max(1) as f64
        );
        for (c, (n, h)) in &cmd_counts {
            println!("        {c}: {n} steps, {h} with voice");
        }

        // Voice files nobody points to.
        let total_files = lib.speech.len();
        let unused = total_files.saturating_sub(used.len());
        println!("voice files not reached by any string key: {unused} of {total_files}");

        // Decode check.
        let mut paths: Vec<String> = Vec::new();
        lib.with_vfs(|vfs| {
            for (p, _) in vfs.iter() {
                if p.starts_with(&format!("{}_ogg/", lang.to_ascii_lowercase())) {
                    paths.push(p.to_owned());
                }
            }
        });
        paths.sort();
        let (mut ok, mut silent, mut bad, mut secs) = (0, 0, 0, 0.0);
        let mut rates: BTreeMap<(u32, u16), usize> = BTreeMap::new();
        for p in paths.iter().step_by(sample.max(1)) {
            match lib.read(p).and_then(|b| pcm::decode(&b)) {
                Ok(a) => {
                    let s = a.stats();
                    secs += s.duration_secs;
                    *rates.entry((a.sample_rate, a.channels)).or_default() += 1;
                    if s.is_silent() {
                        silent += 1;
                        println!("  silent: {p}");
                    } else {
                        ok += 1;
                    }
                }
                Err(e) => {
                    bad += 1;
                    println!("  decode error {p}: {e:#}");
                }
            }
        }
        println!(
            "decode check (every {sample}th of {} files): {ok} ok, {silent} silent, {bad} failed, {secs:.0} s of audio; (rate, channels): {rates:?}",
            paths.len()
        );
    }
    Ok(())
}
