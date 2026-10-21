//! Sun and moon directions from the hour (Y up, -Z north, +X east, like Bevy's defaults).
use serde::{Deserialize, Serialize};
use std::f32::consts::{PI, TAU};

/// Parameters of the sky path. The defaults are placeholders, not the original values.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SkyPath {
    /// Observer latitude in degrees (higher = lower noon sun, longer twilight).
    pub latitude_deg: f32,
    /// Sun declination in degrees (season: + summer, - winter).
    pub sun_declination_deg: f32,
    /// Moon declination in degrees.
    pub moon_declination_deg: f32,
    /// Hours the moon trails the anti-sun position (phase offset).
    pub moon_lag_hours: f32,
    /// Rotation of the whole sky around the up axis in degrees (where north is in the world).
    pub north_yaw_deg: f32,
}

impl Default for SkyPath {
    fn default() -> Self {
        Self {
            latitude_deg: 40.0,
            sun_declination_deg: 15.0,
            moon_declination_deg: -5.0,
            moon_lag_hours: 1.5,
            north_yaw_deg: 0.0,
        }
    }
}

/// Unit vector pointing from the ground towards a body at `hour` (12 = transit / noon).
pub fn body_direction(
    hour: f64,
    latitude_deg: f32,
    declination_deg: f32,
    north_yaw_deg: f32,
) -> [f32; 3] {
    let h = ((hour.rem_euclid(24.0) as f32) - 12.0) / 24.0 * TAU;
    let (phi, delta) = (latitude_deg.to_radians(), declination_deg.to_radians());
    // North-east-up frame: celestial pole P = (0, sin phi, cos phi) towards north, meridian
    // M = (0, cos phi, -sin phi) towards south-up, east E = (1, 0, 0).
    let east = -delta.cos() * h.sin();
    let up = delta.sin() * phi.sin() + delta.cos() * h.cos() * phi.cos();
    let north = delta.sin() * phi.cos() - delta.cos() * h.cos() * phi.sin();
    // World: x east, y up, -z north; then the sky's yaw.
    let (x, z) = (east, -north);
    let (s, c) = north_yaw_deg.to_radians().sin_cos();
    [x * c - z * s, up, x * s + z * c]
}

impl SkyPath {
    pub fn sun(&self, hour: f64) -> [f32; 3] {
        body_direction(
            hour,
            self.latitude_deg,
            self.sun_declination_deg,
            self.north_yaw_deg,
        )
    }

    pub fn moon(&self, hour: f64) -> [f32; 3] {
        body_direction(
            hour + 12.0 - f64::from(self.moon_lag_hours),
            self.latitude_deg,
            self.moon_declination_deg,
            self.north_yaw_deg,
        )
    }
}

/// Elevation of a direction in degrees.
pub fn elevation_deg(dir: [f32; 3]) -> f32 {
    dir[1].clamp(-1.0, 1.0).asin() * 180.0 / PI
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noon_high_midnight_low_east_morning_west_evening() {
        let p = SkyPath::default();
        let noon = elevation_deg(p.sun(12.0));
        assert!((noon - (90.0 - 40.0 + 15.0)).abs() < 1e-3, "{noon}");
        assert!(elevation_deg(p.sun(0.0)) < -30.0);
        assert!(p.sun(9.0)[0] > 0.0, "morning sun in the east");
        assert!(p.sun(15.0)[0] < 0.0, "afternoon sun in the west");
        assert!(
            p.sun(12.0)[2] > 0.0,
            "noon sun due south (+z) in the north hemisphere"
        );
        for h in 0..48 {
            let d = p.sun(f64::from(h) * 0.5);
            let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            assert!((len - 1.0).abs() < 1e-5);
        }
        assert!(
            elevation_deg(p.moon(0.0)) > 0.0,
            "the moon is up at midnight"
        );
    }
}
