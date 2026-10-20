//! The "relax" smoothing filter and easing curve shared by the gameplay camera.
pub use gothic_physics::relax::{RELAX_UNIT, boost, relax, relax_tau, relax_vec3};

/// Cosine ease on `[0, 1]` (the clamp is part of the original ease helper).
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    (1.0 - (std::f32::consts::PI * t).cos()) * 0.5
}

/// Linear interpolation between a `[min_range, max_range]` pair.
pub fn lerp2(pair: [f32; 2], t: f32) -> f32 {
    pair[0] + (pair[1] - pair[0]) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_error_has_unit_boost_and_large_error_four() {
        assert!((boost(0.0) - 1.0).abs() < 1e-6);
        assert!((boost(1.0e9) - 4.0).abs() < 1e-3);
    }

    #[test]
    fn relax_distance_time_constant_is_one_and_a_half_seconds() {
        // fRelaxDist 60 -> tau 1.5 s; tiny error keeps boost ~1, so after tau the gap is ~1/e.
        let mut v = 0.0;
        let dt = 1.0 / 120.0;
        for _ in 0..180 {
            v = relax(v, 1.0, dt, 60.0);
        }
        assert!((v - (1.0 - (-1.0f32).exp())).abs() < 0.02, "v={v}");
    }

    #[test]
    fn zero_time_constant_is_unfiltered() {
        assert_eq!(relax(3.0, 9.0, 0.016, 0.0), 9.0);
    }

    #[test]
    fn ease_is_clamped_and_symmetric() {
        assert_eq!(ease(-1.0), 0.0);
        assert!((ease(0.5) - 0.5).abs() < 1e-6);
        assert!((ease(2.0) - 1.0).abs() < 1e-6);
    }
}
