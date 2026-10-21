//! Keyframe tables over the 24-hour day: sorted `(hour, value)` pairs, sampled with linear or
//! smoothstep interpolation, wrapping from the last key of a day to the first of the next.
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};

/// Values that can be interpolated.
pub trait Lerp: Copy {
    fn lerp(a: Self, b: Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(a: f32, b: f32, t: f32) -> f32 {
        a + (b - a) * t
    }
}

/// Linear-space RGB.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Rgb(pub f32, pub f32, pub f32);

impl Rgb {
    pub const BLACK: Rgb = Rgb(0.0, 0.0, 0.0);
    pub const WHITE: Rgb = Rgb(1.0, 1.0, 1.0);

    pub fn splat(v: f32) -> Self {
        Self(v, v, v)
    }

    /// Relative luminance (Rec. 709).
    pub fn luminance(self) -> f32 {
        0.2126 * self.0 + 0.7152 * self.1 + 0.0722 * self.2
    }

    pub fn scale(self, s: f32) -> Self {
        Self(self.0 * s, self.1 * s, self.2 * s)
    }

    pub fn plus(self, o: Self) -> Self {
        Self(self.0 + o.0, self.1 + o.1, self.2 + o.2)
    }

    /// Towards grey of the same luminance by `amount` (0..1).
    pub fn desaturate(self, amount: f32) -> Self {
        Self::lerp(self, Self::splat(self.luminance()), amount)
    }

    pub fn to_array(self) -> [f32; 3] {
        [self.0, self.1, self.2]
    }
}

impl Lerp for Rgb {
    fn lerp(a: Rgb, b: Rgb, t: f32) -> Rgb {
        Rgb(
            f32::lerp(a.0, b.0, t),
            f32::lerp(a.1, b.1, t),
            f32::lerp(a.2, b.2, t),
        )
    }
}

/// How keys blend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Interp {
    #[default]
    Linear,
    /// Smoothstep between keys (no kinks at the keys).
    Smooth,
    /// Hold each key's value until the next key.
    Step,
}

impl Interp {
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::Smooth => t * t * (3.0 - 2.0 * t),
            Self::Step => 0.0,
        }
    }
}

/// Wrap an hour into `[0, 24)`.
pub fn wrap_hour(h: f64) -> f32 {
    h.rem_euclid(24.0) as f32
}

/// A cyclic keyframe curve over the day. Serialized as a list of `(hour, value)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<(f32, T)>", into = "Vec<(f32, T)>")]
#[serde(bound(
    serialize = "T: Serialize + Clone",
    deserialize = "T: Deserialize<'de>"
))]
pub struct Curve<T> {
    keys: Vec<(f32, T)>,
}

impl<T> TryFrom<Vec<(f32, T)>> for Curve<T> {
    type Error = anyhow::Error;

    fn try_from(keys: Vec<(f32, T)>) -> Result<Self> {
        Self::new(keys)
    }
}

impl<T> From<Curve<T>> for Vec<(f32, T)> {
    fn from(c: Curve<T>) -> Self {
        c.keys
    }
}

impl<T> Curve<T> {
    /// Keys must be non-empty, inside `[0, 24)` and strictly increasing.
    pub fn new(keys: Vec<(f32, T)>) -> Result<Self> {
        ensure!(!keys.is_empty(), "a curve needs at least one key");
        for (i, (h, _)) in keys.iter().enumerate() {
            if !(0.0..24.0).contains(h) || !h.is_finite() {
                bail!("key hour {h} outside [0, 24)");
            }
            if i > 0 && keys[i - 1].0 >= *h {
                bail!("key hours must increase ({} then {h})", keys[i - 1].0);
            }
        }
        Ok(Self { keys })
    }

    pub fn constant(value: T) -> Self {
        Self {
            keys: vec![(0.0, value)],
        }
    }

    pub fn keys(&self) -> &[(f32, T)] {
        &self.keys
    }
}

impl<T: Lerp> Curve<T> {
    /// The value at `hour` (any real number; wrapped into the day).
    pub fn sample(&self, hour: f64, interp: Interp) -> T {
        let h = wrap_hour(hour);
        let k = &self.keys;
        if k.len() == 1 {
            return k[0].1;
        }
        // First key after h; the segment is (i - 1, i), wrapping around midnight.
        let i = k.partition_point(|(kh, _)| *kh <= h);
        let (a, b) = if i == 0 || i == k.len() {
            (k.len() - 1, 0)
        } else {
            (i - 1, i)
        };
        let (ha, hb) = (k[a].0, k[b].0);
        let span = if hb > ha { hb - ha } else { hb + 24.0 - ha };
        let mut d = h - ha;
        if d < 0.0 {
            d += 24.0;
        }
        T::lerp(k[a].1, k[b].1, interp.apply(d / span))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_keys() {
        assert!(Curve::<f32>::new(vec![]).is_err());
        assert!(Curve::new(vec![(5.0, 1.0f32), (5.0, 2.0)]).is_err());
        assert!(Curve::new(vec![(24.0, 1.0f32)]).is_err());
        assert!(Curve::new(vec![(-1.0, 1.0f32)]).is_err());
        let c: Curve<f32> = ron::from_str("[(0.0, 1.0), (12.0, 3.0)]").unwrap();
        assert_eq!(c.sample(6.0, Interp::Linear), 2.0);
        assert!(ron::from_str::<Curve<f32>>("[(12.0, 1.0), (6.0, 3.0)]").is_err());
    }
}
