//! Small deterministic xorshift64* generator (the engine uses `rand()`; any uniform source
//! matches its behaviour, determinism makes the simulation testable).
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform unit vector (`FUN_1014f2d0` in the engine: a random direction).
    pub fn direction(&mut self) -> [f32; 3] {
        loop {
            let v = [
                self.unit() * 2.0 - 1.0,
                self.unit() * 2.0 - 1.0,
                self.unit() * 2.0 - 1.0,
            ];
            let l2 = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
            if (1.0e-4..=1.0).contains(&l2) {
                let l = l2.sqrt();
                return [v[0] / l, v[1] / l, v[2] / l];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_is_in_range_and_deterministic() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..1000 {
            let x = a.unit();
            assert!((0.0..1.0).contains(&x));
            assert_eq!(x, b.unit());
        }
    }

    #[test]
    fn directions_are_unit_length() {
        let mut r = Rng::new(3);
        for _ in 0..100 {
            let d = r.direction();
            let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            assert!((l - 1.0).abs() < 1e-4);
        }
    }
}
