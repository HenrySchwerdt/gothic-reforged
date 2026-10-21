//! Seeded, platform-independent RNG (SplitMix64); every mod gets its own stream.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// Stream for one mod: the world seed mixed with a hash of the mod id (FNV-1a).
    pub fn for_mod(seed: u64, id: &str) -> Self {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in id.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        let mut r = Self(seed ^ h);
        r.next_u64();
        r
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform integer in `lo..=hi` (bounds swapped if reversed).
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        let span = (hi as i128 - lo as i128 + 1) as u128;
        (lo as i128 + (u128::from(self.next_u64()) % span) as i128) as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_in_range() {
        let mut a = Rng::for_mod(7, "m");
        let mut b = Rng::for_mod(7, "m");
        let mut c = Rng::for_mod(7, "n");
        let va: Vec<i64> = (0..100).map(|_| a.range(-3, 3)).collect();
        let vb: Vec<i64> = (0..100).map(|_| b.range(-3, 3)).collect();
        let vc: Vec<i64> = (0..100).map(|_| c.range(-3, 3)).collect();
        assert_eq!(va, vb);
        assert_ne!(va, vc);
        assert!(va.iter().all(|v| (-3..=3).contains(v)));
        assert!(
            (0..1000)
                .map(|_| a.next_f64())
                .all(|f| (0.0..1.0).contains(&f))
        );
    }
}
