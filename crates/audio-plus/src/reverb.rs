//! Reverb: a Freeverb-style network (8 damped combs + 4 allpasses per channel, stereo spread)
//! with presets, and reverb zones that blend presets by listener position.
use crate::dsp::DelayLine;
use serde::{Deserialize, Serialize};

/// Reverb character.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ReverbParams {
    /// 0..1, longer tail with larger values.
    pub room_size: f32,
    /// 0..1, high-frequency damping of the tail.
    pub damping: f32,
    /// Wet level.
    pub wet: f32,
    /// Seconds before the tail starts.
    pub pre_delay: f32,
    /// Stereo width 0..1.
    pub width: f32,
}

impl ReverbParams {
    pub const NONE: ReverbParams = ReverbParams {
        room_size: 0.0,
        damping: 1.0,
        wet: 0.0,
        pre_delay: 0.0,
        width: 1.0,
    };

    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        if t <= 0.0 {
            return a;
        }
        if t >= 1.0 {
            return b;
        }
        let f = |x: f32, y: f32| x + (y - x) * t;
        Self {
            room_size: f(a.room_size, b.room_size),
            damping: f(a.damping, b.damping),
            wet: f(a.wet, b.wet),
            pre_delay: f(a.pre_delay, b.pre_delay),
            width: f(a.width, b.width),
        }
    }
}

/// Built-in presets (placeholders to tune by ear; not taken from the original).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Preset {
    Outdoor,
    Forest,
    Indoor,
    Hall,
    Cave,
}

impl Preset {
    pub fn params(self) -> ReverbParams {
        match self {
            Preset::Outdoor => ReverbParams {
                room_size: 0.25,
                damping: 0.7,
                wet: 0.06,
                pre_delay: 0.0,
                width: 1.0,
            },
            Preset::Forest => ReverbParams {
                room_size: 0.45,
                damping: 0.75,
                wet: 0.12,
                pre_delay: 0.01,
                width: 1.0,
            },
            Preset::Indoor => ReverbParams {
                room_size: 0.6,
                damping: 0.45,
                wet: 0.22,
                pre_delay: 0.008,
                width: 0.8,
            },
            Preset::Hall => ReverbParams {
                room_size: 0.82,
                damping: 0.3,
                wet: 0.3,
                pre_delay: 0.02,
                width: 1.0,
            },
            Preset::Cave => ReverbParams {
                room_size: 0.93,
                damping: 0.2,
                wet: 0.42,
                pre_delay: 0.03,
                width: 1.0,
            },
        }
    }
}

/// Where a preset applies.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    Sphere { center: [f32; 3], radius: f32 },
    Box { min: [f32; 3], max: [f32; 3] },
}

impl Shape {
    /// Distance inside the shape (> 0 inside, < 0 outside).
    pub fn depth(&self, p: [f32; 3]) -> f32 {
        match *self {
            Shape::Sphere { center, radius } => {
                let d = (0..3)
                    .map(|i| (p[i] - center[i]).powi(2))
                    .sum::<f32>()
                    .sqrt();
                radius - d
            }
            Shape::Box { min, max } => (0..3)
                .map(|i| (p[i] - min[i]).min(max[i] - p[i]))
                .fold(f32::INFINITY, f32::min),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ReverbZone {
    pub shape: Shape,
    pub preset: Preset,
    /// Distance over which the zone fades in at its border (m).
    pub fade: f32,
    /// Higher wins where zones overlap (a cave inside a forest).
    pub priority: i32,
}

/// Reverb at `listener`: zones by descending priority, each blended over what lies beneath
/// it by its border fade; `base` outside every zone.
pub fn zone_params(listener: [f32; 3], zones: &[ReverbZone], base: ReverbParams) -> ReverbParams {
    let mut sorted: Vec<&ReverbZone> = zones.iter().collect();
    sorted.sort_by_key(|z| z.priority);
    let mut out = base;
    for z in sorted {
        let w = (z.shape.depth(listener) / z.fade.max(1e-3)).clamp(0.0, 1.0);
        if w > 0.0 {
            out = ReverbParams::lerp(out, z.preset.params(), w);
        }
    }
    out
}

const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
const SPREAD: usize = 23;

#[derive(Debug, Clone)]
struct Comb {
    buf: Vec<f32>,
    pos: usize,
    store: f32,
}

impl Comb {
    #[inline]
    fn process(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let y = self.buf[self.pos];
        self.store = y * (1.0 - damp) + self.store * damp;
        self.buf[self.pos] = x + self.store * feedback;
        self.pos = (self.pos + 1) % self.buf.len();
        y
    }
}

#[derive(Debug, Clone)]
struct Allpass {
    buf: Vec<f32>,
    pos: usize,
}

impl Allpass {
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let b = self.buf[self.pos];
        let y = -x + b;
        self.buf[self.pos] = x + b * 0.5;
        self.pos = (self.pos + 1) % self.buf.len();
        y
    }
}

/// Stereo Freeverb. Mono in (the send), stereo wet out.
#[derive(Debug, Clone)]
pub struct Reverb {
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
    pre: DelayLine,
    sample_rate: f32,
    params: ReverbParams,
}

impl Reverb {
    pub fn new(sample_rate: f32) -> Self {
        let scale = sample_rate / 44_100.0;
        let len = |n: usize, ch: usize| (((n + ch * SPREAD) as f32 * scale) as usize).max(2);
        let combs = [0, 1].map(|ch| {
            COMBS
                .iter()
                .map(|&n| Comb {
                    buf: vec![0.0; len(n, ch)],
                    pos: 0,
                    store: 0.0,
                })
                .collect()
        });
        let allpasses = [0, 1].map(|ch| {
            ALLPASSES
                .iter()
                .map(|&n| Allpass {
                    buf: vec![0.0; len(n, ch)],
                    pos: 0,
                })
                .collect()
        });
        Self {
            combs,
            allpasses,
            pre: DelayLine::new((0.1 * sample_rate) as usize),
            sample_rate,
            params: ReverbParams::NONE,
        }
    }

    pub fn set_params(&mut self, p: ReverbParams) {
        self.params = p;
    }

    pub fn params(&self) -> ReverbParams {
        self.params
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> (f32, f32) {
        let p = self.params;
        self.pre.push(x);
        let input = self.pre.read(p.pre_delay * self.sample_rate) * 0.015;
        let feedback = 0.7 + 0.28 * p.room_size.clamp(0.0, 1.0);
        let damp = p.damping.clamp(0.0, 1.0) * 0.4;
        let mut out = [0.0f32; 2];
        for (ch, o) in out.iter_mut().enumerate() {
            let mut acc = 0.0;
            for c in &mut self.combs[ch] {
                acc += c.process(input, feedback, damp);
            }
            for a in &mut self.allpasses[ch] {
                acc = a.process(acc);
            }
            *o = acc;
        }
        let w1 = p.wet * (p.width / 2.0 + 0.5);
        let w2 = p.wet * ((1.0 - p.width) / 2.0);
        (out[0] * w1 + out[1] * w2, out[1] * w1 + out[0] * w2)
    }
}
