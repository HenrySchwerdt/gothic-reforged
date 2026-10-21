//! `cargo run --release -p gothic-audio --example qa_decode -- [data dir] [scratch dir]`
//!
//! Decode correctness QA: for sampled voice / music / sfx files compare what the playback path
//! really produces (Bevy `AudioSource` -> rodio decoder, f32) and our own `pcm::decode` (i16)
//! against `ffmpeg` (ground truth). Prints one table row per file. Needs `ffmpeg` on PATH.
use anyhow::{Context, Result, ensure};
use bevy::audio::{AudioSource, Decodable, Source};
use gothic_audio::{AudioLibrary, pcm};
use std::path::{Path, PathBuf};
use std::process::Command;

fn ffmpeg(path: &Path, fmt: &str) -> Result<Vec<u8>> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-f", fmt, "-"])
        .output()
        .context("running ffmpeg")?;
    ensure!(out.status.success(), "ffmpeg failed on {}", path.display());
    Ok(out.stdout)
}

fn probe(path: &Path) -> Result<(u32, u32)> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=sample_rate,channels",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let mut it = s.trim().split(',');
    Ok((
        it.next().unwrap_or("0").parse()?,
        it.next().unwrap_or("0").parse()?,
    ))
}

fn main() -> Result<()> {
    let mut a = std::env::args().skip(1);
    let data = a
        .next()
        .unwrap_or_else(|| "D:/SteamLibrary/steamapps/common/Gothic 3/Data".into());
    let scratch = PathBuf::from(a.next().unwrap_or_else(|| ".local/qa-scratch".into()));
    std::fs::create_dir_all(&scratch)?;
    let lib = AudioLibrary::open(Path::new(&data), "German")?;
    let all: Vec<String> = lib
        .with_vfs(|v| v.iter().map(|(p, _)| p.to_owned()).collect())
        .context("vfs")?;
    let pick = |pred: &dyn Fn(&str) -> bool, n: usize| -> Vec<String> {
        let mut v: Vec<String> = all.iter().filter(|p| pred(p)).cloned().collect();
        v.sort();
        let step = (v.len() / n).max(1);
        v.into_iter().step_by(step).take(n).collect()
    };
    let mut files = Vec::new();
    for p in pick(&|p| p.starts_with("german_ogg/") && p.ends_with(".ogg"), 20) {
        files.push(("voice", p));
    }
    for f in lib.music.fragments.iter().step_by(12).take(5) {
        files.push(("music", f.vfs_path()));
    }
    for p in pick(
        &|p| {
            [
                "creature/",
                "foley/",
                "environment/",
                "magic/",
                "voice/",
                "gui/",
                "inventory/",
                "event/",
            ]
            .iter()
            .any(|d| p.starts_with(d))
                && (p.ends_with(".ogg") || p.ends_with(".wav"))
        },
        10,
    ) {
        files.push(("sfx", p));
    }
    println!(
        "kind  file                                            ours(rate/ch/frames)  ffmpeg(rate/ch/frames)  rodio-vs-ff maxdiff  ours-vs-ff maxLSB"
    );
    let mut bad = 0;
    for (kind, p) in files {
        let Ok(bytes) = lib.read(&p) else {
            println!("{kind} {p} unreadable");
            continue;
        };
        let f = scratch.join(p.replace('/', "_"));
        std::fs::write(&f, &*bytes)?;
        let (frate, fch) = probe(&f)?;
        let ff32 = ffmpeg(&f, "f32le")?;
        let ff: Vec<f32> = ff32
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect();
        let ours = pcm::decode(&bytes)?;
        let ff16 = ffmpeg(&f, "s16le")?;
        let ff16: Vec<i16> = ff16
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes(*c))
            .collect();
        let src = AudioSource {
            bytes: bytes.clone(),
        };
        let dec = src.decoder();
        let (rch, rrate) = (dec.channels().get(), dec.sample_rate().get());
        let r: Vec<f32> = dec.collect();
        let n = r.len().min(ff.len());
        let rd = (0..n).map(|i| (r[i] - ff[i]).abs()).fold(0f32, f32::max);
        let m = ours.samples.len().min(ff16.len());
        let od = (0..m)
            .map(|i| (i32::from(ours.samples[i]) - i32::from(ff16[i])).abs())
            .max()
            .unwrap_or(0);
        let ok = u32::from(rch) == fch
            && rrate == frate
            && ours.channels as u32 == fch
            && ours.sample_rate == frate
            && r.len().abs_diff(ff.len()) <= 2 * fch as usize * 1152
            && rd < 2e-3
            && od <= 2;
        if !ok {
            bad += 1;
        }
        println!(
            "{kind:5} {p:48} {}/{}/{}  {}/{}/{}  rodio {}/{}/{} diff {rd:.5}  lsb {od} {}",
            ours.sample_rate,
            ours.channels,
            ours.frames(),
            frate,
            fch,
            ff.len() / fch.max(1) as usize,
            rrate,
            rch,
            r.len() / usize::from(rch).max(1),
            if ok { "OK" } else { "MISMATCH" }
        );
    }
    println!("mismatches: {bad}");
    Ok(())
}
