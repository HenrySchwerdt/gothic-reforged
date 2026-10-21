//! Deterministic DSP tests on synthetic signals.
use gothic_audio_plus::dsp::equal_power;
use gothic_audio_plus::hrtf::{Direction, Hrtf, itd};
use gothic_audio_plus::reverb::zone_params;
use gothic_audio_plus::{
    Blocking, EnhancedSettings, EnhancedVoice, Preset, Reverb, ReverbParams, ReverbZone, Shape,
    VoiceParams, probe,
};

const SR: f32 = 48_000.0;

fn noise(n: usize, seed: u64) -> Vec<f32> {
    let mut s = seed | 1;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            ((s >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
        })
        .collect()
}

fn sine(freq: f32, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| (std::f32::consts::TAU * freq * i as f32 / SR).sin())
        .collect()
}

fn rms(x: impl Iterator<Item = f32>) -> f32 {
    let (s, n) = x.fold((0.0f64, 0usize), |(s, n), v| (s + f64::from(v * v), n + 1));
    (s / n.max(1) as f64).sqrt() as f32
}

fn db(a: f32, b: f32) -> f32 {
    20.0 * (a / b).log10()
}

fn hrtf_out(dir: Direction, input: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let mut h = Hrtf::new(SR);
    h.set_direction(dir);
    input.iter().map(|&x| h.process(x)).unzip()
}

#[test]
fn interaural_time_difference_follows_the_side() {
    let right = Direction::new(1.0, 0.0, 0.0);
    let t = itd(right);
    assert!((0.0006..0.0007).contains(&t), "max ITD {t}");
    assert!(itd(Direction::FRONT).abs() < 1e-7);
    assert!((itd(Direction::new(-1.0, 0.0, 0.0)) + t).abs() < 1e-7);
    // Impulse from the right: the right ear's response starts earlier.
    let mut imp = vec![0.0; 256];
    imp[0] = 1.0;
    let (l, r) = hrtf_out(right, &imp);
    let onset = |x: &[f32]| x.iter().position(|v| v.abs() > 0.05).unwrap();
    let lag = onset(&l) as f32 - onset(&r) as f32;
    assert!(
        (lag / SR - t).abs() < 2.0 / SR,
        "lag {lag} samples vs {}",
        t * SR
    );
}

#[test]
fn head_shadow_is_frequency_dependent() {
    let right = Direction::new(1.0, 0.0, 0.0);
    let ild = |freq: f32| {
        let (l, r) = hrtf_out(right, &sine(freq, 9600));
        db(
            rms(r[4800..].iter().copied()),
            rms(l[4800..].iter().copied()),
        )
    };
    let low = ild(200.0);
    let high = ild(8000.0);
    assert!(low < 3.0, "little shadow at 200 Hz: {low} dB");
    assert!(high > 8.0, "strong shadow at 8 kHz: {high} dB");
    // Front: both ears equal.
    let (l, r) = hrtf_out(Direction::FRONT, &noise(48_000, 1));
    assert!(db(rms(r.into_iter()), rms(l.into_iter())).abs() < 0.1);
}

#[test]
fn sources_behind_are_duller_than_in_front() {
    let hf = |dir: Direction| {
        let (l, _) = hrtf_out(dir, &sine(6000.0, 9600));
        rms(l[4800..].iter().copied())
    };
    assert!(hf(Direction::new(0.0, 0.0, -1.0)) < hf(Direction::FRONT) * 0.8);
}

#[test]
fn occlusion_is_quieter_and_duller_monotonically() {
    let input = noise(48_000, 2);
    let settings = EnhancedSettings {
        reverb: false,
        hrtf: false,
        ..EnhancedSettings::all_on()
    };
    let run = |occ: f32, freq: Option<f32>| {
        let mut v = EnhancedVoice::new(SR);
        let x = match freq {
            Some(f) => sine(f, 48_000),
            None => input.clone(),
        };
        let mut out = vec![0.0; 2 * x.len()];
        let params = VoiceParams {
            blocking: Blocking {
                occlusion: occ,
                obstruction: occ,
            },
            ..VoiceParams::default()
        };
        v.process(&x, &mut out, &params, &settings);
        rms(out[24_000..].iter().copied())
    };
    let levels: Vec<f32> = [0.0, 0.25, 0.5, 1.0]
        .iter()
        .map(|&o| run(o, None))
        .collect();
    assert!(levels.windows(2).all(|w| w[1] < w[0]), "{levels:?}");
    // Blocked: highs lose more than lows.
    let lows = db(run(1.0, Some(200.0)), run(0.0, Some(200.0)));
    let highs = db(run(1.0, Some(6000.0)), run(0.0, Some(6000.0)));
    assert!(highs < lows - 10.0, "lows {lows} dB, highs {highs} dB");
}

#[test]
fn probe_tells_obstruction_from_occlusion() {
    // A thin wall at z = 5 between listener (z 0) and emitter (z 10), 1 m wide in x.
    let wall = |a: [f32; 3], b: [f32; 3]| {
        let t = (5.0 - a[2]) / (b[2] - a[2]);
        if !(0.0..=1.0).contains(&t) {
            return false;
        }
        let x = a[0] + (b[0] - a[0]) * t;
        let y = a[1] + (b[1] - a[1]) * t;
        x.abs() < 0.5 && y.abs() < 5.0
    };
    let narrow = probe([0.0, 1.0, 0.0], [0.0, 1.0, 10.0], 2.0, &wall);
    assert_eq!(narrow.obstruction, 1.0);
    assert!(
        narrow.occlusion < 0.6,
        "sound goes around a narrow wall: {narrow:?}"
    );
    let big_wall = |a: [f32; 3], b: [f32; 3]| (a[2] - 5.0) * (b[2] - 5.0) < 0.0;
    let full = probe([0.0, 1.0, 0.0], [0.0, 1.0, 10.0], 2.0, &big_wall);
    assert_eq!(
        full,
        Blocking {
            obstruction: 1.0,
            occlusion: 1.0
        }
    );
    let free = probe([0.0, 1.0, 0.0], [0.0, 1.0, 10.0], 2.0, &|_, _| false);
    assert_eq!(free, Blocking::default());
}

fn tail_energy(preset: Preset, from: f32, to: f32) -> f32 {
    let mut r = Reverb::new(SR);
    r.set_params(preset.params());
    let n = (to * SR) as usize;
    let mut e = 0.0f64;
    for i in 0..n {
        let (l, rr) = r.process(if i == 0 { 1.0 } else { 0.0 });
        if i as f32 >= from * SR {
            e += f64::from(l * l + rr * rr);
        }
    }
    e as f32
}

#[test]
fn bigger_spaces_ring_longer() {
    let late = |p| tail_energy(p, 0.8, 1.5) / tail_energy(p, 0.0, 0.3).max(1e-12);
    let (outdoor, indoor, cave) = (
        late(Preset::Outdoor),
        late(Preset::Indoor),
        late(Preset::Cave),
    );
    assert!(
        outdoor < indoor && indoor < cave,
        "{outdoor} {indoor} {cave}"
    );
    // Every tail dies out.
    assert!(tail_energy(Preset::Cave, 9.0, 10.0) < tail_energy(Preset::Cave, 0.0, 1.0) * 1e-3);
}

#[test]
fn zones_blend_smoothly_and_priority_wins() {
    let outdoor = Preset::Outdoor.params();
    let zones = [
        ReverbZone {
            shape: Shape::Box {
                min: [-50.0, -10.0, -50.0],
                max: [50.0, 20.0, 50.0],
            },
            preset: Preset::Forest,
            fade: 5.0,
            priority: 0,
        },
        ReverbZone {
            shape: Shape::Sphere {
                center: [0.0, 0.0, 0.0],
                radius: 10.0,
            },
            preset: Preset::Cave,
            fade: 4.0,
            priority: 1,
        },
    ];
    assert_eq!(zone_params([200.0, 0.0, 0.0], &zones, outdoor), outdoor);
    assert_eq!(
        zone_params([30.0, 0.0, 0.0], &zones, outdoor),
        Preset::Forest.params()
    );
    assert_eq!(
        zone_params([0.0, 0.0, 0.0], &zones, outdoor),
        Preset::Cave.params()
    );
    // Walking from outside into the cave: wet level changes continuously.
    let mut prev = zone_params([60.0, 0.0, 0.0], &zones, outdoor).wet;
    for i in 0..600 {
        let x = 60.0 - i as f32 * 0.1;
        let w = zone_params([x, 0.0, 0.0], &zones, outdoor).wet;
        assert!((w - prev).abs() < 0.02, "jump at x = {x}");
        prev = w;
    }
    let mid = ReverbParams::lerp(outdoor, Preset::Cave.params(), 0.5);
    assert!(
        (mid.room_size - (outdoor.room_size + Preset::Cave.params().room_size) / 2.0).abs() < 1e-6
    );
}

#[test]
fn disabled_is_the_plain_equal_power_pan() {
    let input = noise(4800, 3);
    let mut v = EnhancedVoice::new(SR);
    let mut out = vec![0.0; 2 * input.len()];
    let params = VoiceParams {
        direction: [0.5, 0.0, 0.5],
        distance_gain: 0.5,
        blocking: Blocking {
            occlusion: 1.0,
            obstruction: 1.0,
        },
        reverb: Preset::Cave.params(),
    };
    // Settle the parameter smoothers (20 ms) first, then compare.
    for _ in 0..5 {
        v.process(&input, &mut out, &params, &EnhancedSettings::default());
    }
    let (pl, pr) = equal_power(0.5f32.atan2(0.5).sin());
    for (i, &x) in input.iter().enumerate() {
        assert!((out[2 * i] - x * 0.5 * pl).abs() < 1e-3);
        assert!((out[2 * i + 1] - x * 0.5 * pr).abs() < 1e-3);
    }
}

#[test]
fn long_noise_stays_finite_and_runs_are_identical() {
    let input = noise(48_000 * 5, 4);
    let render = || {
        let mut v = EnhancedVoice::new(SR);
        let mut out = vec![0.0; 2 * input.len()];
        let mut params = VoiceParams {
            reverb: Preset::Cave.params(),
            ..VoiceParams::default()
        };
        for (k, block) in input.chunks(480).enumerate() {
            let a = k as f32 * 0.05;
            params.direction = [a.sin(), 0.2, a.cos()];
            params.blocking.occlusion = (k % 40) as f32 / 40.0;
            let o = &mut out[k * 960..k * 960 + block.len() * 2];
            v.process(block, o, &params, &EnhancedSettings::all_on());
        }
        out
    };
    let a = render();
    assert!(a.iter().all(|x| x.is_finite() && x.abs() < 8.0));
    let b = render();
    assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
}
