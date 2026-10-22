//! Wind: the SpeedTree wind entity and the vegetation wave/grass-response rules.
//!
//! Two independent systems exist in the original:
//!
//! * `eCSpeedTreeWind_PS` (Engine.dll:0x10208200..): the parameter block of the SpeedTree wind
//!   (one entity in the world, the `sysdyn` document) that `Advance` (0x10218ab0) feeds to the
//!   SpeedTree runtime every frame with the wind direction (the entity's forward axis). The
//!   runtime itself is a closed SDK, so only the *parameters* are recoverable; the sway model
//!   below ([`SpeedTreeWind::tree_offset`], [`SpeedTreeWind::leaf_offset`]) is a re-implementation
//!   of the documented behaviour (strength interpolates between a low and a high parameter set,
//!   gusts add strength) and its mapping is **inferred**.
//! * `eCVegetationAdmin::SetWind` (0x1027a1c0) with the vegetation vertex shader
//!   (`ge_vegetation.fx`): ground cover sways with two travelling waves; the shader constants
//!   are verified. Per vertex weight = `((y - y_min) / height)^2 * WindStrength`
//!   (`eCVegetation_GridNode::CacheInGeometry`, 0x1027d830). The *grass response* pushes
//!   vertices away from the player (`Render.GrassResponseRange/Intensity` of `ge3.INI`).
//!
//! Everything is a plain function of time so the GPU copy of these functions can be checked
//! against the tests here.
use std::f32::consts::{PI, TAU};

/// Port of the defaults of `eCSpeedTreeWind_PS::PostInitializeProperties` (0x10218be0) and the
/// values stored in the world's `sysdyn` entity (identical). Property names are the serialized
/// ones (`WindStrange` is the engine's own spelling of the strength).
#[derive(Debug, Clone, PartialEq)]
pub struct SpeedTreeWind {
    /// `WindStrange`: base strength 0..1.
    pub strength: f32,
    pub bend_low: [f32; 3],
    pub bend_high: [f32; 3],
    pub vibration_low: [f32; 3],
    pub vibration_high: [f32; 3],
    pub vibration_frequency: [f32; 2],
    /// Degrees.
    pub vibration_angles: [f32; 2],
    /// Degrees.
    pub max_bend_angle: f32,
    pub strength_adjustment_exponent: f32,
    pub gust_strength: [f32; 2],
    /// Seconds.
    pub gust_duration: [f32; 2],
    pub gust_frequency: f32,
    pub gust_control: [f32; 3],
    pub leaf_strength_exponent: f32,
    pub leaf_rocking: [f32; 3],
    pub leaf_rocking_high: [f32; 3],
    pub leaf_rocking_frequency: [f32; 2],
    pub leaf_rocking_angles: [f32; 2],
    pub leaf_rustling: [f32; 3],
    pub leaf_rustling_high: [f32; 3],
    pub leaf_rustling_frequency: [f32; 2],
    pub leaf_rustling_angles: [f32; 2],
    /// Wind direction (the entity's forward axis; the world entity is an identity matrix).
    pub direction: [f32; 3],
}

impl Default for SpeedTreeWind {
    fn default() -> Self {
        Self {
            strength: 0.1,
            bend_low: [0.2, 0.2, 0.0],
            bend_high: [0.5, 0.5, 0.0],
            vibration_low: [1.0, 0.01, 0.0],
            vibration_high: [1.0, 0.05, 0.0],
            vibration_frequency: [10.0, 1000.0],
            vibration_angles: [2.0, 5.0],
            max_bend_angle: 20.0,
            strength_adjustment_exponent: 1.0,
            gust_strength: [0.1, 0.5],
            gust_duration: [0.1, 3.0],
            gust_frequency: 30.0,
            gust_control: [3.0, 0.001, 0.0],
            leaf_strength_exponent: 1.0,
            leaf_rocking: [0.2, 0.01, 0.0],
            leaf_rocking_high: [4.0, 1.0, 1.0],
            leaf_rocking_frequency: [100.0, 300.0],
            leaf_rocking_angles: [0.5, 1.0],
            leaf_rustling: [0.5, 0.05, 0.0],
            leaf_rustling_high: [4.0, 1.0, 1.0],
            leaf_rustling_frequency: [100.0, 500.0],
            leaf_rustling_angles: [0.5, 1.0],
            direction: [0.0, 0.0, 1.0],
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Stateless hash to `[0, 1)` shared with the shader (`fract(sin(x * 127.1 + 311.7) * 43758.5453)`).
pub fn hash11(x: f32) -> f32 {
    let v = (x * 127.1 + 311.7).sin() * 43_758.547;
    v - v.floor()
}

impl SpeedTreeWind {
    /// Read the serialized property set of an `eCSpeedTreeWind_PS` entity; missing properties
    /// keep their defaults.
    pub fn from_properties(set: &gothic_formats::propstream::PropertySet) -> Self {
        use gothic_formats::propstream::PropValue;
        let mut w = Self::default();
        let f = |n: &str, d: f32| set.float(n).unwrap_or(d);
        let v3 = |n: &str, d: [f32; 3]| match set.get(n) {
            Some(PropValue::Vec3(v)) => *v,
            _ => d,
        };
        let v2 = |n: &str, d: [f32; 2]| match set.get(n) {
            Some(PropValue::Vec2(v)) => *v,
            _ => d,
        };
        w.strength = f("WindStrange", w.strength);
        w.bend_low = v3("BendLow", w.bend_low);
        w.bend_high = v3("BendHigh", w.bend_high);
        w.vibration_low = v3("VibrationLow", w.vibration_low);
        w.vibration_high = v3("VibrationHigh", w.vibration_high);
        w.vibration_frequency = v2("VibrationFrequency", w.vibration_frequency);
        w.vibration_angles = v2("VibrationAngles", w.vibration_angles);
        w.max_bend_angle = f("MaxBendAngle", w.max_bend_angle);
        w.strength_adjustment_exponent =
            f("StrengthAdjustmentExponent", w.strength_adjustment_exponent);
        w.gust_strength = v2("GustStrength", w.gust_strength);
        w.gust_duration = v2("GustDuration", w.gust_duration);
        w.gust_frequency = f("GustFrequency", w.gust_frequency);
        w.gust_control = v3("GustControl", w.gust_control);
        w.leaf_strength_exponent = f("LeafStrengthExponent", w.leaf_strength_exponent);
        w.leaf_rocking = v3("LeafRocking", w.leaf_rocking);
        w.leaf_rocking_high = v3("LeafRockingHigh", w.leaf_rocking_high);
        w.leaf_rocking_frequency = v2("LeafRockingFrequency", w.leaf_rocking_frequency);
        w.leaf_rocking_angles = v2("LeafRockingAngles", w.leaf_rocking_angles);
        w.leaf_rustling = v3("LeafRustling", w.leaf_rustling);
        w.leaf_rustling_high = v3("LeafRustlingHigh", w.leaf_rustling_high);
        w.leaf_rustling_frequency = v2("LeafRustlingFrequency", w.leaf_rustling_frequency);
        w.leaf_rustling_angles = v2("LeafRustlingAngles", w.leaf_rustling_angles);
        w
    }

    /// Length of one gust slot in seconds: `GustFrequency` is read as gusts per minute [I].
    pub fn gust_period(&self) -> f32 {
        60.0 / self.gust_frequency.max(0.1)
    }

    /// Strength added by gusts at time `t` (seconds): every slot starts one gust at a random
    /// moment with a random duration and strength inside the property ranges, shaped by a
    /// half-sine; overlapping gusts do not add up (the strongest wins) [I].
    pub fn gust(&self, t: f32) -> f32 {
        let period = self.gust_period();
        let slot = (t / period).floor();
        let mut g: f32 = 0.0;
        for k in [-1.0, 0.0] {
            let s = slot + k;
            let start = (s + hash11(s)) * period;
            let dur = lerp(
                self.gust_duration[0],
                self.gust_duration[1],
                hash11(s + 17.0),
            )
            .max(0.01);
            let str = lerp(
                self.gust_strength[0],
                self.gust_strength[1],
                hash11(s + 41.0),
            );
            let u = (t - start) / dur;
            if u > 0.0 && u < 1.0 {
                g = g.max(str * (PI * u).sin());
            }
        }
        g
    }

    /// Wind strength 0..1 at time `t` (base plus gust).
    pub fn strength_at(&self, t: f32) -> f32 {
        (self.strength + self.gust(t)).clamp(0.0, 1.0)
    }

    /// Branch bend angle (radians) for strength `s`: `MaxBendAngle * bend(s) * s^exp`.
    pub fn bend_angle(&self, s: f32) -> f32 {
        let bend = lerp(self.bend_low[0], self.bend_high[0], s);
        (self.max_bend_angle * bend * s.max(0.0).powf(self.strength_adjustment_exponent))
            .to_radians()
    }

    /// Trunk/branch vibration angle (radians) at strength `s`: the angle range interpolated
    /// together with the amplitude factor of `VibrationLow/High` (x 10) [I].
    pub fn vibration_angle(&self, s: f32) -> f32 {
        let a = |i: usize, amp: f32| self.vibration_angles[i].to_radians() * amp * 10.0;
        lerp(a(0, self.vibration_low[1]), a(1, self.vibration_high[1]), s)
    }

    /// Vibration frequency in Hz (`VibrationFrequency` is in hundredths of a hertz [I]).
    pub fn vibration_hz(&self, s: f32) -> f32 {
        (lerp(self.vibration_frequency[0], self.vibration_frequency[1], s) / 100.0).clamp(0.1, 4.0)
    }

    /// Base strength at which oscillator frequencies are sampled. Gusts still change wind
    /// amplitude, but keeping their frequency fixed prevents elapsed-time phase drift.
    fn base_strength(&self) -> f32 {
        self.strength.clamp(0.0, 1.0)
    }

    fn tree_phase_cycles(&self, t: f32, phase: f32) -> f32 {
        self.vibration_hz(self.base_strength()) * t + phase
    }

    fn leaf_frequencies_hz(&self) -> (f32, f32) {
        let s = self.base_strength();
        (
            lerp(self.leaf_rocking_frequency[0], self.leaf_rocking_frequency[1], s) / 100.0,
            lerp(self.leaf_rustling_frequency[0], self.leaf_rustling_frequency[1], s) / 100.0,
        )
    }

    fn leaf_phase_cycles(&self, t: f32, phase: f32, hz: f32) -> f32 {
        hz * t + phase
    }

    /// Horizontal sway of a branch point with height weight `weight` (`(y / H)^2`) on a tree of
    /// world height `height` metres, along the wind direction. `phase` is a per-tree random
    /// phase in `[0, 1)`. Gusts change bend and vibration amplitude; oscillator frequency uses
    /// the base strength so phase rate stays continuous.
    pub fn tree_offset(&self, t: f32, weight: f32, height: f32, phase: f32) -> f32 {
        let s = self.strength_at(t);
        let bend = self.bend_angle(s).sin();
        let osc = 0.75 + 0.25 * (TAU * self.tree_phase_cycles(t, phase)).sin();
        let vib = self.vibration_angle(s).sin()
            * (TAU * self.tree_phase_cycles(t * 3.0, phase * 7.0)).sin();
        let off = height * weight * (bend * osc + vib);
        off.clamp(-0.35 * height * weight, 0.35 * height * weight)
    }

    /// Leaf flutter amplitude (radians) at strength `s`: rocking plus rustling terms
    /// (`LeafRocking*` / `LeafRustling*`, interpolated low -> high with the leaf exponent) [I].
    pub fn leaf_angles(&self, s: f32) -> (f32, f32) {
        let k = s.max(0.0).powf(self.leaf_strength_exponent);
        let rock = lerp(self.leaf_rocking[0], self.leaf_rocking_high[0], k)
            / self.leaf_rocking_high[0].max(1e-3);
        let rust = lerp(self.leaf_rustling[0], self.leaf_rustling_high[0], k)
            / self.leaf_rustling_high[0].max(1e-3);
        (
            lerp(self.leaf_rocking_angles[0], self.leaf_rocking_angles[1], k) * rock,
            lerp(
                self.leaf_rustling_angles[0],
                self.leaf_rustling_angles[1],
                k,
            ) * rust,
        )
    }

    /// Displacement (metres) of a leaf-card vertex along the card normal at time `t`;
    /// `leaf_size` is the card edge in metres and `phase` a per-vertex random phase. Gusts
    /// change flutter amplitude; oscillator frequencies use base strength for continuous phase.
    pub fn leaf_offset(&self, t: f32, leaf_size: f32, phase: f32) -> f32 {
        let s = self.strength_at(t);
        let (rock, rust) = self.leaf_angles(s);
        let (f_rock, f_rust) = self.leaf_frequencies_hz();
        leaf_size
            * 0.5
            * (rock * (TAU * self.leaf_phase_cycles(t, phase, f_rock)).sin()
                + rust * (TAU * self.leaf_phase_cycles(t, phase * 3.0, f_rust)).sin())
    }
}

/// Port of `eCVegetationAdmin::SetWind` (0x1027a1c0) with the arguments of
/// `eCVegetationAdmin::Invalidate` (0x10279e10): wavelengths 2000 / 500 cm, amplitudes 30 / 10 cm,
/// phase speeds 0.1 / 0.3 cm per millisecond.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VegetationWind {
    /// `vecAmplitude` (cm).
    pub amplitude: [f32; 2],
    /// `vecWaveLength` as angular wave numbers `2 pi / wavelength` (1/cm).
    pub wave_number: [f32; 2],
    /// `SetWindPhaseConstant`: radians per second (`wave_number * speed * 1000`).
    pub phase_rate: [f32; 2],
}

impl VegetationWind {
    /// `SetWind(wave_length_a, wave_length_b, amp_a, amp_b, speed_a, speed_b)`.
    pub fn set_wind(wl: [f32; 2], amp: [f32; 2], speed: [f32; 2]) -> Self {
        let k = [TAU / wl[0], TAU / wl[1]];
        Self {
            amplitude: amp,
            wave_number: k,
            phase_rate: [k[0] * speed[0] * 1000.0, k[1] * speed[1] * 1000.0],
        }
    }

    /// The constructor defaults of the vegetation admin.
    pub fn original() -> Self {
        Self::set_wind([2000.0, 500.0], [30.0, 10.0], [0.1, 0.3])
    }

    /// Horizontal displacement (cm) of a vertex at world `(x, z)` (cm) with wind weight
    /// `weight` at time `t` (seconds): the vertex shader block "APPLAY WIND".
    pub fn offset(&self, x: f32, z: f32, weight: f32, t: f32) -> [f32; 2] {
        if weight <= 0.0 {
            return [0.0, 0.0];
        }
        let p = [t * self.phase_rate[0], t * self.phase_rate[1]];
        let [a, b] = self.amplitude;
        let [k1, k2] = self.wave_number;
        let dz = weight * (a * (z * k1 + p[0]).cos() + b * (z * k2 + p[1]).sin());
        let dx = weight * (a * (x * k1 + p[0]).sin() + b * (x * k2 + p[1]).cos());
        [dx, dz]
    }
}

/// Wind weight of a brush-mesh vertex: `((y - y_min) / height)^2 * WindStrength`
/// (`eCVegetation_GridNode::CacheInGeometry`). Zero strength disables the wind of the mesh.
pub fn vegetation_weight(y: f32, y_min: f32, height: f32, wind_strength: f32) -> f32 {
    if wind_strength <= 0.0 || height <= 0.0 {
        return 0.0;
    }
    let h = (y - y_min) / height;
    h * h * wind_strength
}

/// `Render.GrassResponseRange` / `Intensity` of `ge3.INI` (cm).
pub const GRASS_RESPONSE_RANGE: f32 = 130.0;
pub const GRASS_RESPONSE_INTENSITY: f32 = 50.0;

/// The grass response block of `ge_vegetation.fx`: the vertex (cm, wind weight > 0) is pushed
/// horizontally away from a point `range` above the player with a linear fall-off. Returns the
/// `(dx, dz)` displacement in cm.
pub fn grass_response(vertex: [f32; 3], player: [f32; 3], range: f32, intensity: f32) -> [f32; 2] {
    let d = [
        vertex[0] - player[0],
        vertex[1] - (player[1] + range),
        vertex[2] - player[2],
    ];
    let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if dist <= 1e-4 {
        return [0.0, 0.0];
    }
    let fall = (range - dist.min(range)) / range;
    [
        d[0] / dist * fall * intensity,
        d[2] / dist * fall * intensity,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_world_entity() {
        let w = SpeedTreeWind::default();
        assert_eq!(w.strength, 0.1);
        assert_eq!(w.max_bend_angle, 20.0);
        assert_eq!(w.gust_strength, [0.1, 0.5]);
        assert_eq!(w.leaf_rocking_frequency, [100.0, 300.0]);
    }

    #[test]
    fn set_wind_constants() {
        let v = VegetationWind::original();
        assert!((v.wave_number[0] - TAU / 2000.0).abs() < 1e-9);
        assert!((v.phase_rate[0] - 0.314_159_27).abs() < 1e-5);
        assert!((v.phase_rate[1] - 3.769_911).abs() < 1e-4);
        assert_eq!(v.amplitude, [30.0, 10.0]);
    }

    #[test]
    fn vegetation_wave_is_bounded_by_the_amplitudes() {
        let v = VegetationWind::original();
        for i in 0..200 {
            let [dx, dz] = v.offset(i as f32 * 37.0, i as f32 * 91.0, 1.0, i as f32 * 0.31);
            assert!(dx.abs() <= 40.0 + 1e-3 && dz.abs() <= 40.0 + 1e-3);
        }
        assert_eq!(v.offset(10.0, 10.0, 0.0, 3.0), [0.0, 0.0]);
        // The x term at t = 0 is a * sin(x * k1) + b * cos(x * k2).
        let [dx, _] = v.offset(500.0, 0.0, 0.5, 0.0);
        let want = 0.5 * (30.0 * (500.0 * TAU / 2000.0).sin() + 10.0 * (500.0 * TAU / 500.0).cos());
        assert!((dx - want).abs() < 1e-4);
    }

    #[test]
    fn vegetation_weight_is_squared_height_times_strength() {
        assert_eq!(vegetation_weight(0.0, 0.0, 100.0, 1.0), 0.0);
        assert!((vegetation_weight(50.0, 0.0, 100.0, 0.8) - 0.2).abs() < 1e-6);
        assert_eq!(vegetation_weight(50.0, 0.0, 100.0, 0.0), 0.0);
    }

    #[test]
    fn grass_response_falls_off_with_distance() {
        // Directly beside the player at the push height: full intensity away from the player.
        let [dx, dz] = grass_response([10.0, 130.0, 0.0], [0.0, 0.0, 0.0], 130.0, 50.0);
        assert!((dx - 50.0 * (130.0 - 10.0) / 130.0).abs() < 1e-3 && dz.abs() < 1e-6);
        // Outside the range nothing moves.
        let out = grass_response([300.0, 0.0, 0.0], [0.0, 0.0, 0.0], 130.0, 50.0);
        assert_eq!(out, [0.0, 0.0]);
        // Pushes away on the other side too.
        let [dx2, _] = grass_response([-10.0, 130.0, 0.0], [0.0, 0.0, 0.0], 130.0, 50.0);
        assert!(dx2 < 0.0);
    }

    #[test]
    fn gusts_stay_inside_the_property_ranges() {
        let w = SpeedTreeWind::default();
        let mut seen = false;
        for i in 0..4000 {
            let g = w.gust(i as f32 * 0.05);
            assert!((0.0..=0.5 + 1e-6).contains(&g));
            seen |= g > 0.05;
        }
        assert!(seen, "some gust must occur in 200 s");
        let s = w.strength_at(12.3);
        assert!((0.1..=1.0).contains(&s));
    }

    #[test]
    fn stronger_wind_bends_more() {
        let w = SpeedTreeWind::default();
        assert!(w.bend_angle(0.6) > w.bend_angle(0.1));
        assert!(w.bend_angle(0.1) > 0.0);
        let calm = w.tree_offset(0.0, 1.0, 10.0, 0.0).abs();
        assert!(calm < 0.35 * 10.0);
        // The leaf flutter scales with the leaf size.
        assert!(w.leaf_offset(0.37, 0.4, 0.2).abs() <= w.leaf_offset(0.37, 0.8, 0.2).abs() + 1e-9);
    }

    #[test]
    fn speedtree_oscillator_phase_stays_continuous_over_long_runtimes() {
        let mut w = SpeedTreeWind::default();
        w.strength = 0.2;
        w.gust_strength = [0.2, 0.2];
        w.gust_duration = [1.0, 1.0];
        w.gust_frequency = 30.0;
        // A two-second slot and one-second gust give repeatable, bounded transitions. Keep the
        // original default oscillator frequency ranges so the bound covers the real shader setup.

        let frame = 1.0 / 60.0;
        // Evaluate actual public offsets on the first frame at/after each onset and the last
        // frame before each end. Divide by the representable f32 interval: at 50 h, one 60 Hz
        // frame rounds to 15.625 ms because elapsed time is stored as f32.
        for hours in [0.0, 1.0, 10.0, 50.0] {
            let around = hours * 3600.0;
            let period = w.gust_period();
            let first_slot = (around / period).floor() as i32;
            // Multiple actual gust boundaries keep the regression from passing because one
            // particular phase happens to alias the bad frequency(s) * t expression.
            for gust_slot in first_slot..first_slot + 16 {
                let s = gust_slot as f32;
                let start = (s + hash11(s)) * period;
                if start < around {
                    continue;
                }
                let end = start + w.gust_duration[0];
                for (t0, t1) in [(start, start + frame), (end - frame, end)] {
                    let dt = t1 - t0;
                    assert!(dt > 0.0, "f32 time must advance across the sampled frame");

                    let tree_speed = (w.tree_offset(t1, 1.0, 8.0, 0.25)
                        - w.tree_offset(t0, 1.0, 8.0, 0.25))
                    .abs()
                        / dt;
                    let leaf_speed = (w.leaf_offset(t1, 1.0, 0.17)
                        - w.leaf_offset(t0, 1.0, 0.17))
                    .abs()
                        / dt;

                    // At base strength 0.2, branch phase is 2.08 Hz and its 3x vibration is
                    // 6.24 Hz. For a 1 m leaf, the configured flutter amplitudes are 0.144 m
                    // (rocking) and 0.18 m (rustling), each scaled by 0.5 in the sum. Their
                    // maximum smooth derivative, including the one-second gust amplitude
                    // change, is below 3.6 m/s. At 50 h the phase-cycle f32 ULP is at most
                    // 0.125 cycles for branch vibration and 0.03125 for leaves; those add up
                    // to conservative 12 / 7 m/s long-runtime bounds over the representable
                    // frame interval. At startup, where phase quantization is negligible, the
                    // tighter 5 / 4.5 m/s bounds check the smooth gust-boundary envelope. The
                    // old frequency(s) * elapsed_time formula exceeds these limits as t*f'
                    // grows; checking 16 consecutive gust slots avoids a lucky phase alias.
                    let (tree_limit, leaf_limit) = if hours == 0.0 {
                        (5.0, 4.5)
                    } else {
                        (12.0, 7.0)
                    };
                    assert!(
                        tree_speed < tree_limit,
                        "tree wind jumped {tree_speed} m/s at {hours} h"
                    );
                    assert!(
                        leaf_speed < leaf_limit,
                        "leaf wind jumped {leaf_speed} m/s at {hours} h"
                    );
                }
            }
        }
    }
}
