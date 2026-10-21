//! Small, deterministic DSP building blocks (f32, no allocation while processing).

/// One-pole low-pass (`y += a (x - y)`).
#[derive(Debug, Clone, Copy, Default)]
pub struct OnePole {
    a: f32,
    z: f32,
}

impl OnePole {
    pub fn new(cutoff: f32, sample_rate: f32) -> Self {
        let mut f = Self { a: 1.0, z: 0.0 };
        f.set_cutoff(cutoff, sample_rate);
        f
    }

    pub fn set_cutoff(&mut self, cutoff: f32, sample_rate: f32) {
        let c = cutoff.clamp(10.0, sample_rate * 0.49);
        self.a = 1.0 - (-std::f32::consts::TAU * c / sample_rate).exp();
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.z += self.a * (x - self.z);
        self.z
    }

    pub fn reset(&mut self) {
        self.z = 0.0;
    }
}

/// First-order IIR section `y = b0 x + b1 x1 - a1 y1`.
#[derive(Debug, Clone, Copy, Default)]
pub struct FirstOrder {
    pub b0: f32,
    pub b1: f32,
    pub a1: f32,
    x1: f32,
    y1: f32,
}

impl FirstOrder {
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 - self.a1 * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }

    pub fn set(&mut self, b0: f32, b1: f32, a1: f32) {
        self.b0 = b0;
        self.b1 = b1;
        self.a1 = a1;
    }
}

/// Biquad (RBJ cookbook), direct form I.
#[derive(Debug, Clone, Copy, Default)]
pub struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    x: [f32; 2],
    y: [f32; 2],
}

impl Biquad {
    /// Peaking EQ at `freq` with `gain_db` and quality `q`.
    pub fn peaking(freq: f32, q: f32, gain_db: f32, sample_rate: f32) -> Self {
        let mut b = Self::default();
        b.set_peaking(freq, q, gain_db, sample_rate);
        b
    }

    pub fn set_peaking(&mut self, freq: f32, q: f32, gain_db: f32, sample_rate: f32) {
        let a = 10f32.powf(gain_db / 40.0);
        let w = std::f32::consts::TAU * freq.clamp(20.0, sample_rate * 0.45) / sample_rate;
        let (sn, cs) = w.sin_cos();
        let alpha = sn / (2.0 * q.max(0.05));
        let a0 = 1.0 + alpha / a;
        self.b = [
            (1.0 + alpha * a) / a0,
            (-2.0 * cs) / a0,
            (1.0 - alpha * a) / a0,
        ];
        self.a = [(-2.0 * cs) / a0, (1.0 - alpha / a) / a0];
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// Delay line with a fractional (linearly interpolated) read.
#[derive(Debug, Clone)]
pub struct DelayLine {
    buf: Vec<f32>,
    pos: usize,
}

impl DelayLine {
    pub fn new(max_samples: usize) -> Self {
        Self {
            buf: vec![0.0; max_samples.max(2)],
            pos: 0,
        }
    }

    #[inline]
    pub fn push(&mut self, x: f32) {
        self.buf[self.pos] = x;
        self.pos = (self.pos + 1) % self.buf.len();
    }

    /// The sample written `delay` samples ago (0 = the latest).
    #[inline]
    pub fn read(&self, delay: f32) -> f32 {
        let n = self.buf.len();
        let d = delay.clamp(0.0, (n - 2) as f32);
        let i = d.floor() as usize;
        let frac = d - i as f32;
        let at = |k: usize| self.buf[(self.pos + n - 1 - k) % n];
        at(i) * (1.0 - frac) + at(i + 1) * frac
    }
}

/// Exponential smoothing of control values (avoids zipper noise on parameter jumps).
#[derive(Debug, Clone, Copy)]
pub struct Smoothed {
    pub value: f32,
    coef: f32,
}

impl Smoothed {
    pub fn new(value: f32, time: f32, rate: f32) -> Self {
        Self {
            value,
            coef: 1.0 - (-1.0 / (time.max(1e-4) * rate)).exp(),
        }
    }

    #[inline]
    pub fn next(&mut self, target: f32) -> f32 {
        self.value += (target - self.value) * self.coef;
        self.value
    }
}

#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Equal-power stereo pan (`pan` -1 left .. 1 right), the plain spatialisation.
pub fn equal_power(pan: f32) -> (f32, f32) {
    let a = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    (a.cos(), a.sin())
}
