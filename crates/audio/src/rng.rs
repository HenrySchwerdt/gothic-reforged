//! Small deterministic generator for the audio logic (selection, spawn timing). The original
//! uses the C runtime `rand()`; only the distribution matters, and a seedable generator keeps
//! the tests exact.

#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    pub fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }

    /// Uniform in `[0, 1]` (the original compares `rand() / 32767` against a probability).
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / ((1u32 << 24) - 1) as f32
    }

    /// Uniform in `0..n` (`0` for `n == 0`).
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            self.next_u32() as usize % n
        }
    }

    /// Uniform in `[lo, hi]`; the bounds may come in either order (the shipped ambient table
    /// has `MinSpawnDelay > MaxSpawnDelay` for some environments).
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        lo + (hi - lo) * self.unit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_accepts_swapped_bounds_and_stays_inside() {
        let mut r = Rng::new(7);
        for _ in 0..1000 {
            let v = r.range(12.0, 10.0);
            assert!((10.0..=12.0).contains(&v));
            assert!(r.below(5) < 5);
            assert!((0.0..=1.0).contains(&r.unit()));
        }
        assert_eq!(r.below(0), 0);
    }
}
