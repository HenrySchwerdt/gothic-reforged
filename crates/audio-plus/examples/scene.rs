//! Offline A/B scene (no game data, no audio device): a synthetic source orbits the listener,
//! passes behind a wall, then the listener walks into a cave. Writes the faithful mix and the
//! enhanced mix as WAVs and measures the cost of each feature in ns per sample.
//!
//! `cargo run --release -p gothic-audio-plus --example scene -- --out <dir> [--secs 12]`
use gothic_audio_plus::{
    EnhancedSettings, EnhancedVoice, Preset, ReverbZone, Shape, VoiceParams, probe, wav,
    zone_params,
};
use std::path::PathBuf;
use std::time::Instant;

const RATE: f32 = 48_000.0;
const BLOCK: usize = 256;

/// A wall from x = 2..2.5, z = -8..-2 (the source passes behind it on its orbit).
fn wall_blocks(a: [f32; 3], b: [f32; 3]) -> bool {
    let (min, max) = ([2.0, -10.0, -8.0], [2.5, 10.0, -2.0]);
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for i in 0..3 {
        let d = b[i] - a[i];
        if d.abs() < 1e-6 {
            if a[i] < min[i] || a[i] > max[i] {
                return false;
            }
            continue;
        }
        let (mut lo, mut hi) = ((min[i] - a[i]) / d, (max[i] - a[i]) / d);
        if lo > hi {
            std::mem::swap(&mut lo, &mut hi);
        }
        t0 = t0.max(lo);
        t1 = t1.min(hi);
        if t0 > t1 {
            return false;
        }
    }
    true
}

/// Inverse-distance rolloff, the shape of the faithful FMOD 3D model (min 1 m).
fn distance_gain(d: f32) -> f32 {
    (1.0 / d.max(1.0)).min(1.0)
}

/// Mono test signal: pitched clicks plus a little noise (wide band, so filters are audible).
fn source(n: usize) -> Vec<f32> {
    let mut seed = 0x1234_5678u32;
    (0..n)
        .map(|i| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let noise = (seed as f32 / u32::MAX as f32 - 0.5) * 0.1;
            let t = i as f32 / RATE;
            let env = (-((t * 4.0).fract()) * 18.0).exp();
            0.5 * env * (t * 660.0 * std::f32::consts::TAU).sin() + noise
        })
        .collect()
}

struct Scene {
    zones: Vec<ReverbZone>,
}

impl Scene {
    /// Listener and emitter at time `t` of `secs`.
    fn at(&self, t: f32, secs: f32) -> ([f32; 3], [f32; 3]) {
        let walk = ((t / secs - 0.6) / 0.3).clamp(0.0, 1.0);
        let listener = [0.0, 1.7, walk * 30.0];
        let a = t * 0.9;
        let emitter = [
            listener[0] + 5.0 * a.sin(),
            1.7,
            listener[2] - 5.0 * a.cos(),
        ];
        (listener, emitter)
    }

    fn params(
        &self,
        listener: [f32; 3],
        emitter: [f32; 3],
        settings: &EnhancedSettings,
    ) -> VoiceParams {
        // The listener faces +z; direction in listener space (x right, z forward).
        let d = [
            emitter[0] - listener[0],
            emitter[1] - listener[1],
            emitter[2] - listener[2],
        ];
        let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        VoiceParams {
            direction: d,
            distance_gain: distance_gain(dist),
            blocking: if settings.enabled && settings.occlusion {
                probe(listener, emitter, 0.6, &wall_blocks)
            } else {
                Default::default()
            },
            reverb: zone_params(listener, &self.zones, Preset::Outdoor.params()),
        }
    }
}

fn render(scene: &Scene, input: &[f32], secs: f32, settings: &EnhancedSettings) -> (Vec<f32>, f64) {
    let mut voice = EnhancedVoice::new(RATE);
    let mut out = vec![0.0; input.len() * 2];
    let mut dsp = 0.0f64;
    for (i, block) in input.chunks(BLOCK).enumerate() {
        let t = (i * BLOCK) as f32 / RATE;
        let (l, e) = scene.at(t, secs);
        let params = scene.params(l, e, settings);
        let start = Instant::now();
        let o = &mut out[i * BLOCK * 2..(i * BLOCK + block.len()) * 2];
        voice.process(block, o, &params, settings);
        dsp += start.elapsed().as_secs_f64();
    }
    (out, dsp * 1e9 / input.len() as f64)
}

fn main() -> std::io::Result<()> {
    let mut out = PathBuf::from("target/audio-scene");
    let mut secs = 12.0f32;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => out = args.next().expect("--out <dir>").into(),
            "--secs" => {
                secs = args
                    .next()
                    .and_then(|s| s.parse().ok())
                    .expect("--secs <n>")
            }
            other => panic!("unknown argument {other}"),
        }
    }
    std::fs::create_dir_all(&out)?;
    let scene = Scene {
        zones: vec![ReverbZone {
            shape: Shape::Box {
                min: [-20.0, -5.0, 20.0],
                max: [20.0, 10.0, 60.0],
            },
            preset: Preset::Cave,
            fade: 4.0,
            priority: 1,
        }],
    };
    let input = source((secs * RATE) as usize);
    let faithful = EnhancedSettings::default();
    let all = EnhancedSettings::all_on();
    let only = |hrtf, occlusion, reverb| EnhancedSettings {
        enabled: true,
        hrtf,
        occlusion,
        reverb,
        ..all
    };
    let runs = [
        ("faithful", faithful),
        ("hrtf", only(true, false, false)),
        ("occlusion", only(false, true, false)),
        ("reverb", only(false, false, true)),
        ("enhanced", all),
    ];
    let mut report = Vec::new();
    for (name, settings) in runs {
        // Best of three for the timing; the output is deterministic.
        let mut best = f64::INFINITY;
        let mut frames = Vec::new();
        for _ in 0..3 {
            let (f, ns) = render(&scene, &input, secs, &settings);
            best = best.min(ns);
            frames = f;
        }
        if name == "faithful" || name == "enhanced" {
            wav::write_stereo(&out.join(format!("{name}.wav")), RATE as u32, &frames)?;
        }
        let peak = frames.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        println!("{name:10} {best:7.1} ns/sample  peak {peak:.3}");
        report.push(format!(
            "  \"{name}\": {{ \"ns_per_sample\": {best:.2}, \"peak\": {peak:.4} }}"
        ));
    }
    std::fs::write(
        out.join("cost.json"),
        format!("{{\n{}\n}}\n", report.join(",\n")),
    )?;
    println!("wrote {}", out.display());
    Ok(())
}
