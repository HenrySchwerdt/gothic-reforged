//! Spherical-head HRTF (Brown & Duda 1998 structural model, simplified): interaural time
//! difference by the Woodworth formula as a fractional delay on the far ear, head shadow as a
//! first-order shelving filter per ear, a pinna notch that moves with elevation, and a little
//! high-frequency loss for sources behind. No measured HRIR data is needed.
use crate::dsp::{Biquad, DelayLine, FirstOrder};

/// Head radius (m) and speed of sound (m/s).
pub const HEAD_RADIUS: f32 = 0.0875;
pub const SPEED_OF_SOUND: f32 = 343.0;

/// Direction of a source in listener space: `x` right, `y` up, `z` *forward* (unit vector).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Direction {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Direction {
    pub const FRONT: Direction = Direction {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    };

    pub fn new(x: f32, y: f32, z: f32) -> Self {
        let l = (x * x + y * y + z * z).sqrt();
        if l < 1e-6 {
            Self::FRONT
        } else {
            Self {
                x: x / l,
                y: y / l,
                z: z / l,
            }
        }
    }

    /// Lateral angle (rad, + right): angle from the median plane.
    pub fn lateral(&self) -> f32 {
        self.x.clamp(-1.0, 1.0).asin()
    }

    pub fn elevation(&self) -> f32 {
        self.y.clamp(-1.0, 1.0).asin()
    }
}

/// Woodworth ITD in seconds (positive: the right ear hears first).
pub fn itd(dir: Direction) -> f32 {
    let t = dir.lateral();
    HEAD_RADIUS / SPEED_OF_SOUND * (t + t.sin())
}

/// Head-shadow shelving coefficients for an ear at angle `theta` (rad) between the ear axis
/// and the source: α = 1.05 + 0.95 cos(θ · 180/150) (from ~2 facing the ear to ~0.1 behind it).
fn shadow(theta: f32, sample_rate: f32) -> (f32, f32, f32) {
    let alpha = 1.05 + 0.95 * (theta * 180.0 / 150.0).min(std::f32::consts::PI).cos();
    let w0 = SPEED_OF_SOUND / HEAD_RADIUS;
    // H(s) = (alpha s + 2 w0) / (s + 2 w0) (unity at DC, `alpha` at high frequencies),
    // bilinear transform with k = 2 fs.
    let k = 2.0 * sample_rate;
    let a0 = k + 2.0 * w0;
    let b0 = (alpha * k + 2.0 * w0) / a0;
    let b1 = (-alpha * k + 2.0 * w0) / a0;
    let a1 = (-k + 2.0 * w0) / a0;
    (b0, b1, a1)
}

#[derive(Debug, Clone)]
pub struct Hrtf {
    sample_rate: f32,
    delay: DelayLine,
    left: FirstOrder,
    right: FirstOrder,
    pinna_l: Biquad,
    pinna_r: Biquad,
    back: FirstOrder,
    itd_samples: f32,
}

impl Hrtf {
    pub fn new(sample_rate: f32) -> Self {
        let mut h = Self {
            sample_rate,
            delay: DelayLine::new((0.002 * sample_rate) as usize + 4),
            left: FirstOrder::default(),
            right: FirstOrder::default(),
            pinna_l: Biquad::default(),
            pinna_r: Biquad::default(),
            back: FirstOrder::default(),
            itd_samples: 0.0,
        };
        h.set_direction(Direction::FRONT);
        h
    }

    /// Update the filters for a new direction (call once per block).
    pub fn set_direction(&mut self, dir: Direction) {
        let sr = self.sample_rate;
        self.itd_samples = itd(dir) * sr;
        // Angle between each ear axis (±x) and the source.
        let to_right = dir.x.clamp(-1.0, 1.0).acos();
        let to_left = (-dir.x).clamp(-1.0, 1.0).acos();
        let (b0, b1, a1) = shadow(to_left, sr);
        self.left.set(b0, b1, a1);
        let (b0, b1, a1) = shadow(to_right, sr);
        self.right.set(b0, b1, a1);
        // Pinna notch: ~7 kHz at the horizon, rising with elevation.
        let notch = 7000.0 + 3000.0 * dir.elevation().sin();
        self.pinna_l.set_peaking(notch, 2.0, -6.0, sr);
        self.pinna_r.set_peaking(notch, 2.0, -6.0, sr);
        // Behind (z < 0): up to a 0.6 high shelf cut (same shelving form, alpha < 1).
        let behind = (-dir.z).max(0.0);
        let alpha = 1.0 - 0.4 * behind;
        let w = std::f32::consts::TAU * 3000.0;
        let k = 2.0 * sr;
        let a0 = k + w;
        self.back
            .set((alpha * k + w) / a0, (-alpha * k + w) / a0, (-k + w) / a0);
    }

    /// One mono sample in, one stereo frame out.
    #[inline]
    pub fn process(&mut self, x: f32) -> (f32, f32) {
        let x = self.back.process(x);
        self.delay.push(x);
        // The far ear hears the sound `|itd|` later.
        let (dl, dr) = if self.itd_samples >= 0.0 {
            (self.itd_samples, 0.0)
        } else {
            (0.0, -self.itd_samples)
        };
        let l = self.pinna_l.process(self.left.process(self.delay.read(dl)));
        let r = self
            .pinna_r
            .process(self.right.process(self.delay.read(dr)));
        (l, r)
    }
}
