//! Surface state for materials: wetness (darker, glossier ground) and snow cover.
use crate::weather::{SurfaceRates, WeatherProfile};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct Surface {
    /// 0 = dry, 1 = soaked.
    pub wetness: f32,
    /// 0 = bare, 1 = covered.
    pub snow_cover: f32,
}

impl Surface {
    /// `sun_up` 0..1: how much the sun helps drying.
    pub fn update(&mut self, rates: &SurfaceRates, w: &WeatherProfile, sun_up: f32, dt: f32) {
        let dry = rates.drying * (0.5 + 0.5 * sun_up.clamp(0.0, 1.0)) * (1.0 - w.rain);
        self.wetness = (self.wetness + (w.rain * rates.wetting - dry) * dt).clamp(0.0, 1.0);
        let thaw = rates.thawing * (1.0 - w.snow);
        self.snow_cover = (self.snow_cover + (w.snow * rates.snowing - thaw) * dt).clamp(0.0, 1.0);
    }
}
