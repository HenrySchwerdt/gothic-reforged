//! Bounded wind oscillator phases for the renderer.
//!
//! `GlobalsUniform.time` wraps hourly in Bevy. This clock integrates shared world rates in f64,
//! wraps oscillator phases, and evaluates gust envelopes from f64 elapsed time. Integer gust slots
//! preserve the existing `hash11` sequence for the supported day-long run (slot IDs remain exactly
//! representable as f32 through 2^24 slots).

use crate::wind::{SpeedTreeWind, VegetationWind, hash11};

#[derive(Clone, Copy, Debug)]
pub struct WindClockConfig {
    vegetation_rate: [f64; 2],
    tree_hz: f64,
    leaf_hz: [f64; 2],
    gust_period: f64,
    gust_duration: [f64; 2],
    gust_strength: [f64; 2],
}

impl WindClockConfig {
    pub fn new(wind: &SpeedTreeWind, vegetation: VegetationWind) -> Self {
        let s = wind.strength.clamp(0.0, 1.0);
        let lerp = |a: f32, b: f32| a as f64 + (b as f64 - a as f64) * s as f64;
        Self {
            vegetation_rate: [
                vegetation.phase_rate[0] as f64,
                vegetation.phase_rate[1] as f64,
            ],
            tree_hz: (lerp(wind.vibration_frequency[0], wind.vibration_frequency[1]) / 100.0)
                .clamp(0.1, 4.0),
            leaf_hz: [
                lerp(
                    wind.leaf_rocking_frequency[0],
                    wind.leaf_rocking_frequency[1],
                ) / 100.0,
                lerp(
                    wind.leaf_rustling_frequency[0],
                    wind.leaf_rustling_frequency[1],
                ) / 100.0,
            ],
            gust_period: 60.0 / wind.gust_frequency.max(0.1) as f64,
            gust_duration: [wind.gust_duration[0] as f64, wind.gust_duration[1] as f64],
            gust_strength: [wind.gust_strength[0] as f64, wind.gust_strength[1] as f64],
        }
    }
}

#[derive(Default, Debug)]
pub struct WindClock {
    elapsed: f64,
    // Vegetation phases are radians; SpeedTree phases are cycles.
    phases: [f64; 5],
    previous: [f32; 6],
    last_current: [f32; 6],
}

impl WindClock {
    /// Returns four vec4 records: current phases+gust, then previous-frame phases+gust.
    /// Each sample packs vegetation A/B radians, tree cycles, leaf-rock cycles, then
    /// leaf-rustle cycles and gust strength.
    pub fn advance(&mut self, dt: f64, config: WindClockConfig) -> [f32; 16] {
        let dt = dt.max(0.0);
        self.previous = self.last_current;
        self.elapsed += dt;
        let rates = [
            config.vegetation_rate[0],
            config.vegetation_rate[1],
            config.tree_hz,
            config.leaf_hz[0],
            config.leaf_hz[1],
        ];
        for (i, (phase, rate)) in self.phases.iter_mut().zip(rates).enumerate() {
            let period = if i < 2 { std::f64::consts::TAU } else { 1.0 };
            *phase = (*phase + rate * dt).rem_euclid(period);
        }
        let current = [
            self.phases[0] as f32,
            self.phases[1] as f32,
            self.phases[2] as f32,
            self.phases[3] as f32,
            self.phases[4] as f32,
            gust_strength(self.elapsed, config),
        ];
        self.last_current = current;
        [
            current[0],
            current[1],
            current[2],
            current[3],
            current[4],
            current[5],
            0.0,
            0.0,
            self.previous[0],
            self.previous[1],
            self.previous[2],
            self.previous[3],
            self.previous[4],
            self.previous[5],
            0.0,
            0.0,
        ]
    }

    pub fn elapsed(&self) -> f64 {
        self.elapsed
    }
}

fn gust_strength(elapsed: f64, config: WindClockConfig) -> f32 {
    let period = config.gust_period.max(0.01);
    let slot = (elapsed / period).floor().max(0.0) as i64;
    let mut gust = 0.0f64;
    for s in [slot - 1, slot] {
        let slot_f32 = s as f32;
        let start = (s as f64 + hash11(slot_f32) as f64) * period;
        let duration = config.gust_duration[0]
            + (config.gust_duration[1] - config.gust_duration[0]) * hash11(slot_f32 + 17.0) as f64;
        let strength = config.gust_strength[0]
            + (config.gust_strength[1] - config.gust_strength[0]) * hash11(slot_f32 + 41.0) as f64;
        let u = (elapsed - start) / duration.max(0.01);
        if u > 0.0 && u < 1.0 {
            gust = gust.max(strength * (std::f64::consts::PI * u).sin());
        }
    }
    gust as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> (SpeedTreeWind, WindClockConfig) {
        let wind = SpeedTreeWind::default();
        let config = WindClockConfig::new(&wind, VegetationWind::original());
        (wind, config)
    }

    fn rates(config: WindClockConfig) -> [f64; 5] {
        [
            config.vegetation_rate[0],
            config.vegetation_rate[1],
            config.tree_hz,
            config.leaf_hz[0],
            config.leaf_hz[1],
        ]
    }

    fn assert_shader_phase(actual: f32, expected: f64, index: usize) {
        let phase_rad = if index < 2 {
            actual as f64
        } else {
            actual as f64 * std::f64::consts::TAU
        };
        let expected_rad = if index < 2 {
            expected
        } else {
            expected * std::f64::consts::TAU
        };
        assert!((phase_rad.sin() - expected_rad.sin()).abs() < 2e-5);
        assert!((phase_rad.cos() - expected_rad.cos()).abs() < 2e-5);
    }

    #[test]
    fn bounded_phases_cross_hour_and_day_without_reset() {
        let (_, config) = defaults();
        for boundary in [3_600.0, 86_400.0] {
            let mut clock = WindClock::default();
            let before_dt = boundary - 0.01;
            let first = clock.advance(before_dt, config);
            let mut expected = [0.0; 5];
            for (i, (phase, rate)) in expected.iter_mut().zip(rates(config)).enumerate() {
                let period = if i < 2 { std::f64::consts::TAU } else { 1.0 };
                *phase = (rate * before_dt).rem_euclid(period);
                assert_shader_phase(first[i], *phase, i);
            }
            let dt = 0.02;
            let second = clock.advance(dt, config);
            for (i, (phase, rate)) in expected.iter_mut().zip(rates(config)).enumerate() {
                let period = if i < 2 { std::f64::consts::TAU } else { 1.0 };
                *phase = (*phase + rate * dt).rem_euclid(period);
                assert_shader_phase(second[i], *phase, i);
            }
        }
    }

    #[test]
    fn gust_hash_sequence_matches_legacy_cpu_at_early_and_day_adjacent_slots() {
        let (wind, config) = defaults();
        let day_slot = (86_400.0 / config.gust_period).floor() as i64;
        for slot in [0i64, 1, 17, 512, day_slot - 2, day_slot - 1, day_slot] {
            let mut best = (0.0f32, 0.0f64);
            for sample_idx in 0..2_000 {
                let t = (slot as f64 + sample_idx as f64 / 2_000.0) * config.gust_period;
                let legacy = wind.gust(t as f32);
                if legacy > best.0 {
                    best = (legacy, t);
                }
            }
            assert!(best.0 > 0.02, "no nonzero legacy gust in slot {slot}");
            let sample = gust_strength(best.1, config);
            assert!(
                (sample - best.0).abs() < 0.06,
                "gust mismatch at slot {slot}: {sample} vs {}",
                best.0
            );
        }
    }

    #[test]
    fn gust_envelope_is_smooth_across_hour_and_day_boundaries() {
        let (_, config) = defaults();
        let epsilon = 1.0 / 60.0;
        for boundary in [3_600.0, 86_400.0] {
            let before = gust_strength(boundary - epsilon, config);
            let after = gust_strength(boundary + epsilon, config);
            assert!(
                (after - before).abs() < 0.02,
                "gust envelope jumped at {boundary}s: {before} -> {after}"
            );
        }
    }

    #[test]
    fn current_and_previous_gust_records_are_distinct_frame_samples() {
        let (wind, config) = defaults();
        let mut clock = WindClock::default();
        let first = clock.advance(0.25, config);
        let mut changed_wind = wind.clone();
        changed_wind.strength = 0.8;
        changed_wind.vibration_frequency = [25.0, 125.0];
        changed_wind.leaf_rocking_frequency = [70.0, 210.0];
        changed_wind.gust_strength = [0.2, 0.6];
        let changed = WindClockConfig::new(&changed_wind, VegetationWind::original());
        let second = clock.advance(0.5, changed);
        assert_eq!(&second[8..14], &first[0..6]);
        let expected = rates(changed);
        for (i, rate) in expected.into_iter().enumerate() {
            let period = if i < 2 { std::f64::consts::TAU } else { 1.0 };
            assert_shader_phase(
                second[i],
                (first[i] as f64 + rate * 0.5).rem_euclid(period),
                i,
            );
        }
        assert!((clock.elapsed() - 0.75).abs() < f64::EPSILON);
    }
}
