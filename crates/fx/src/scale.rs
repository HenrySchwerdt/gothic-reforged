//! Keyframed scale curves of `eCParticle_PS` (`eCFloatScale`, `eCVectorScale`, `eCColorScale`).
//! Keys are `[time, value...]` with time in 0..1, interpolated linearly; outside the key range
//! the nearest key holds. An empty curve is the neutral value 1.
use gothic_formats::props::tails::ParticleCurves;

/// Fraction of a looped parameter: `(repeats + 1) * t` wrapped into 0..1 (the `(float)(int)`
/// truncation of `CalcSizeFactor`, `CalcRevolutionFactor`, `CalcFrame`).
pub fn looped(t: f32, repeats: f32) -> f32 {
    let x = (repeats + 1.0) * t;
    x - x.trunc()
}

/// Segment `(lo, hi)` of the keys around `x`, as walked by the original `CalcValue` loops.
fn locate(times: impl Iterator<Item = f32>, len: usize, x: f32) -> (usize, usize) {
    let mut lo = 0;
    let mut hi = 0;
    let mut found = false;
    for (i, t) in times.enumerate() {
        if t == x {
            lo = i;
            hi = i;
            found = true;
            break;
        }
        if x < t {
            lo = i.saturating_sub(1);
            hi = i;
            found = true;
            break;
        }
        lo = i;
    }
    if !found {
        hi = len; // walked off the end: hold the last key
    }
    if hi == len {
        hi = lo;
    }
    (lo, hi)
}

fn lerp_index<const N: usize>(keys: &[[f32; N]], x: f32) -> (usize, usize, f32) {
    let (lo, hi) = locate(keys.iter().map(|k| k[0]), keys.len(), x);
    let span = keys[hi][0] - keys[lo][0];
    if lo == hi || span == 0.0 {
        (lo, lo, 0.0)
    } else {
        (lo, hi, (x - keys[lo][0]) / span)
    }
}

/// Port of eCFloatScale::CalcValue (Engine:0x1015f750): the keyed float at `x`.
pub fn float_scale(keys: &[[f32; 2]], x: f32) -> f32 {
    if keys.is_empty() {
        return 1.0;
    }
    let (lo, hi, f) = lerp_index(keys, x);
    keys[lo][1] + (keys[hi][1] - keys[lo][1]) * f
}

/// Port of eCVectorScale::CalcValue (Engine:0x101600c0) incl. the linear and binary search
/// variants (they differ only in how the segment is found).
pub fn vector_scale(keys: &[[f32; 4]], x: f32) -> [f32; 3] {
    match keys.len() {
        0 => [1.0; 3],
        1 => [keys[0][1], keys[0][2], keys[0][3]],
        _ => {
            let (lo, hi, f) = lerp_index(keys, x);
            let mut out = [0.0; 3];
            for (c, o) in out.iter_mut().enumerate() {
                *o = keys[lo][c + 1] + (keys[hi][c + 1] - keys[lo][c + 1]) * f;
            }
            out
        }
    }
}

/// Port of eCColorScale::CalcValue (Engine:0x1015f2b0): the keyed rgba at `x`.
pub fn color_scale(keys: &[[f32; 5]], x: f32) -> [f32; 4] {
    if keys.is_empty() {
        return [1.0; 4];
    }
    let (lo, hi, f) = lerp_index(keys, x);
    let mut out = [0.0; 4];
    for (c, o) in out.iter_mut().enumerate() {
        *o = keys[lo][c + 1] + (keys[hi][c + 1] - keys[lo][c + 1]) * f;
    }
    out
}

/// The curves of one emitter (copied out of the decoded tail).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Curves {
    pub frame: Vec<[f32; 2]>,
    pub revolution: Vec<[f32; 4]>,
    pub velocity: Vec<[f32; 4]>,
    pub color: Vec<[f32; 5]>,
    pub size: Vec<[f32; 4]>,
}

impl From<&ParticleCurves> for Curves {
    fn from(c: &ParticleCurves) -> Self {
        Self {
            frame: c.frame.clone(),
            revolution: c.revolution.clone(),
            velocity: c.velocity.clone(),
            color: c.color.clone(),
            size: c.size.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_curves_are_neutral() {
        assert_eq!(float_scale(&[], 0.3), 1.0);
        assert_eq!(vector_scale(&[], 0.3), [1.0; 3]);
        assert_eq!(color_scale(&[], 0.3), [1.0; 4]);
    }

    #[test]
    fn interpolates_and_holds_the_ends() {
        let k = [[0.0, 0.0], [0.5, 10.0], [1.0, 20.0]];
        assert_eq!(float_scale(&k, 0.25), 5.0);
        assert_eq!(float_scale(&k, 0.5), 10.0);
        assert_eq!(float_scale(&k, 0.75), 15.0);
        assert_eq!(float_scale(&k, 2.0), 20.0);
        let late = [[0.2, 4.0], [0.8, 8.0]];
        assert_eq!(float_scale(&late, 0.0), 4.0);
    }

    #[test]
    fn single_key_vector_is_constant() {
        assert_eq!(vector_scale(&[[0.5, 1.0, 2.0, 3.0]], 0.9), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn colour_blends_all_channels() {
        let k = [[0.0, 1.0, 0.0, 0.0, 1.0], [1.0, 0.0, 1.0, 0.0, 0.0]];
        assert_eq!(color_scale(&k, 0.5), [0.5, 0.5, 0.0, 0.5]);
    }

    #[test]
    fn repeats_wrap_the_parameter() {
        assert_eq!(looped(0.5, 0.0), 0.5);
        assert!((looped(0.75, 1.0) - 0.5).abs() < 1e-6);
    }
}
