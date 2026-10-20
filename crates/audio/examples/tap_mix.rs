//! `tap_mix <tap.tsv> <out.wav> [data dir]`: offline render of a `GR_AUDIO_TAP` log (48 kHz
//! stereo) and loudness / clipping statistics per category and 5 s window. The pan law is
//! rodio's `Spatial` (near ear 0.5 + (|p|+1)/4, far ear 0.5 + (1-|p|)/4) with our ear swap.
use anyhow::Result;
use gothic_audio::{AudioLibrary, pcm};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

const SR: usize = 48_000;

struct Src {
    cat: String,
    pcm: Option<(Vec<f32>, u32, usize)>,
    t0: f32,
    segs: Vec<(f32, f32, f32)>, // (time, gain, pan) changes
    end: Option<f32>,
    skip: f32,
}

fn main() -> Result<()> {
    let mut a = std::env::args().skip(1);
    let tap = a.next().expect("tap.tsv");
    let out = a.next().expect("out.wav");
    let data = a
        .next()
        .unwrap_or_else(|| "D:/SteamLibrary/steamapps/common/Gothic 3/Data".into());
    let lib = AudioLibrary::open(Path::new(&data), "German")?;
    let mut srcs: HashMap<String, Src> = HashMap::new();
    let mut order = Vec::new();
    let mut tmax = 0f32;
    let mut stats: Vec<(f32, f32, f32)> = Vec::new();
    for line in std::fs::read_to_string(&tap)?.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 7 {
            continue;
        }
        let t: f32 = f[0].parse()?;
        tmax = tmax.max(t);
        let (kind, id) = (f[1], f[2]);
        let gain: f32 = f[4].parse()?;
        let pan: f32 = f[5].parse()?;
        match kind {
            "stat" => stats.push((t, gain, pan)),
            "end" => {
                if let Some(s) = srcs.get_mut(id) {
                    s.end = Some(t);
                }
            }
            _ if f[6].starts_with("start") => {
                let pcm = lib
                    .read(f[3])
                    .ok()
                    .and_then(|b| pcm::decode(&b).ok())
                    .map(|p| {
                        let ch = usize::from(p.channels);
                        let v: Vec<f32> =
                            p.samples.iter().map(|&s| f32::from(s) / 32768.0).collect();
                        (v, p.sample_rate, ch)
                    });
                let skip = if kind == "music" {
                    lib.read(f[3])
                        .ok()
                        .and_then(|b| pcm::leading_silence_secs(&b, 60, 30.0).ok())
                        .filter(|&s| s > 0.5)
                        .map_or(0.0, |s| s as f32 - 0.05)
                } else {
                    0.0
                };
                let key = format!("{id}@{t}");
                srcs.insert(
                    id.to_owned(),
                    Src {
                        cat: kind.to_owned(),
                        pcm,
                        t0: t,
                        segs: vec![(t, gain, pan)],
                        end: None,
                        skip,
                    },
                );
                order.push((id.to_owned(), key));
            }
            _ => {
                if let Some(s) = srcs.get_mut(id) {
                    s.segs.push((t, gain, pan));
                }
            }
        }
    }
    let total = ((tmax + 3.0) * SR as f32) as usize;
    let mut mix: HashMap<String, Vec<f32>> = HashMap::new();
    let mut master = vec![0f32; total * 2];
    // ids repeat after despawn only through entity reuse; process in start order.
    for (id, _) in &order {
        let s = &srcs[id];
        let Some((v, rate, ch)) = &s.pcm else {
            continue;
        };
        let frames = v.len() / ch;
        let layer = mix
            .entry(s.cat.clone())
            .or_insert_with(|| vec![0f32; total * 2]);
        let first = (s.t0 * SR as f32) as usize;
        let n_out = ((frames as f32 / *rate as f32 - s.skip).max(0.0) * SR as f32) as usize;
        let mut seg = 0;
        for k in 0..n_out {
            let o = first + k;
            if o >= total {
                break;
            }
            let t = o as f32 / SR as f32;
            if s.end.is_some_and(|e| t > e + 0.05) {
                break;
            }
            while seg + 1 < s.segs.len() && s.segs[seg + 1].0 <= t {
                seg += 1;
            }
            let (_, g, p) = s.segs[seg];
            let spatial = s.cat != "music" && p.abs() > 0.0001
                || (s.cat != "music" && s.segs.iter().any(|x| x.2 != 0.0));
            let (lm, rm) = if spatial {
                (0.5 + (1.0 - p) / 4.0, 0.5 + (1.0 + p) / 4.0)
            } else {
                (1.0, 1.0)
            };
            let pos = (k as f32 / SR as f32 + s.skip) * *rate as f32;
            let i0 = pos as usize;
            if i0 + 1 >= frames {
                break;
            }
            let fr = pos - i0 as f32;
            let samp = |c: usize| v[i0 * ch + c] * (1.0 - fr) + v[(i0 + 1) * ch + c] * fr;
            let (l, r) = if *ch == 1 {
                (samp(0), samp(0))
            } else {
                (samp(0), samp(1))
            };
            layer[o * 2] += l * g * lm;
            layer[o * 2 + 1] += r * g * rm;
        }
    }
    for l in mix.values() {
        for (m, x) in master.iter_mut().zip(l) {
            *m += x;
        }
    }
    let peak = master.iter().fold(0f32, |m, x| m.max(x.abs()));
    let clipped = master.iter().filter(|x| x.abs() > 1.0).count();
    println!(
        "duration {:.1}s peak {peak:.3} clipped samples {clipped} ({:.4}%)",
        total as f32 / SR as f32,
        100.0 * clipped as f32 / master.len() as f32
    );
    let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len().max(1) as f32).sqrt();
    let mut cats: Vec<_> = mix.keys().cloned().collect();
    cats.sort();
    println!(
        "window(s)  {}",
        cats.iter().map(|c| format!("{c:>9}")).collect::<String>() + "    master   (rms dBFS)"
    );
    let w = 5 * SR * 2;
    let db = |x: f32| if x < 1e-6 { -99.0 } else { 20.0 * x.log10() };
    for (i, ch) in master.chunks(w).enumerate() {
        let mut row = format!("{:>4}-{:<4}", i * 5, i * 5 + 5);
        for c in &cats {
            let l = &mix[c];
            let lo = i * w;
            let hi = (lo + w).min(l.len());
            row += &format!("{:9.1}", db(rms(&l[lo..hi])));
        }
        row += &format!("{:9.1}", db(rms(ch)));
        println!("{row}");
    }
    if !stats.is_empty() {
        let maxch = stats.iter().map(|s| s.1).fold(0f32, f32::max);
        let maxv = stats.iter().map(|s| s.2).fold(0f32, f32::max);
        let mean = stats.iter().map(|s| s.1).sum::<f32>() / stats.len() as f32;
        println!("channels: max {maxch} mean {mean:.1}; voices: max {maxv}");
    }
    let mut f = std::io::BufWriter::new(std::fs::File::create(out)?);
    let data_len = (master.len() * 2) as u32;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&(SR as u32).to_le_bytes())?;
    f.write_all(&((SR * 4) as u32).to_le_bytes())?;
    f.write_all(&4u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for x in &master {
        f.write_all(&((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())?;
    }
    Ok(())
}
