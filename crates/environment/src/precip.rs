//! Precipitation parameters: the `eCPrecipitation_PS` entity of the world and the per-state
//! overrides of `eCWeatherStates` (rain, snow).
//!
//! The world entity (system document) carries the cube, particle budget and the texture
//! (`RainAndSnow.tga`, 2 x 2 tiles); `eCWeatherAdmin::Process` replaces the emitter rate and the
//! look (speed, size, streak length, tile, jitter, splash) with the values of the current weather
//! state. The simulation lives in `gothic_fx::precip`.
use gothic_formats::{
    propstream::{PropValue, PropertySet},
    weather::WeatherState,
};

/// All properties of `eCPrecipitation_PS`.
#[derive(Debug, Clone, PartialEq)]
pub struct PrecipParams {
    pub texture: String,
    /// Edge of the cube around the camera that contains the particles (cm).
    pub cube_size: f32,
    pub max_particles: u32,
    pub particles_per_second: f32,
    pub direction: [f32; 3],
    /// cm per second.
    pub speed_min_max: [f32; 2],
    /// Half extent of a particle (cm).
    pub size: [f32; 2],
    pub max_spawn_points: u32,
    /// Stretch of the streak along the projected fall direction.
    pub direction_scale: f32,
    pub tile_count: [u32; 2],
    pub current_tile: u32,
    /// Random offset added to the spawn position (cm).
    pub spawn_point_dizzer: f32,
    pub jitter_power: f32,
    pub jitter_speed: f32,
    /// Seconds a splash lives; 0 = particles just vanish at the ground.
    pub splash_duration: f32,
    pub splash_tile: u32,
    /// Growth of the splash size per second.
    pub splash_size_scale: [f32; 2],
}

impl Default for PrecipParams {
    /// The values stored in the world.
    fn default() -> Self {
        Self {
            texture: "RainAndSnow.tga".into(),
            cube_size: 1000.0,
            max_particles: 3000,
            particles_per_second: 0.0,
            direction: [0.0, -1.0, 0.0],
            speed_min_max: [800.0, 1000.0],
            size: [0.2, 0.2],
            max_spawn_points: 500,
            direction_scale: 0.0,
            tile_count: [2, 2],
            current_tile: 0,
            spawn_point_dizzer: 20.0,
            jitter_power: 0.0,
            jitter_speed: 0.0,
            splash_duration: 0.0,
            splash_tile: 0,
            splash_size_scale: [1.0, 1.0],
        }
    }
}

impl PrecipParams {
    pub fn from_properties(set: &PropertySet) -> Self {
        let mut p = Self::default();
        let int = |n: &str, d: u32| match set.get(n) {
            Some(PropValue::I32(v)) => (*v).max(0) as u32,
            _ => d,
        };
        let v2 = |n: &str, d: [f32; 2]| match set.get(n) {
            Some(PropValue::Vec2(v)) => *v,
            _ => d,
        };
        if let Some(t) = set.string("Texture") {
            p.texture = t.to_string();
        }
        p.cube_size = set.float("PrecipitationCubeSize").unwrap_or(p.cube_size);
        p.max_particles = int("MaxParticles", p.max_particles);
        p.particles_per_second = int("ParticlesPerSecond", 0) as f32;
        if let Some(PropValue::Vec3(d)) = set.get("Direction") {
            p.direction = *d;
        }
        p.speed_min_max = v2("SpeedMinMax", p.speed_min_max);
        p.size = v2("Size", p.size);
        p.max_spawn_points = int("MaxSpawnPoints", p.max_spawn_points);
        p.direction_scale = set.float("DirectionScale").unwrap_or(0.0);
        p.tile_count = [
            int("TextureTileCountU", 2).max(1),
            int("TextureTileCountV", 2).max(1),
        ];
        p.current_tile = int("CurrentTextureTile", 0);
        p.spawn_point_dizzer = set
            .float("SpawnPointDizzer")
            .unwrap_or(p.spawn_point_dizzer);
        p.jitter_power = set.float("JitterPower").unwrap_or(0.0);
        p.jitter_speed = set.float("JitterSpeed").unwrap_or(0.0);
        p.splash_duration = set.float("SplashDuration").unwrap_or(0.0);
        p.splash_tile = int("SplashTextureTile", 0);
        p.splash_size_scale = v2("SplashSizeScale", p.splash_size_scale);
        p
    }

    /// The emitter as `eCWeatherAdmin::Process` configures it for a weather state: the particle
    /// rate and the look come from the state, the cube and the budget stay.
    pub fn with_state(&self, s: &WeatherState) -> Self {
        Self {
            particles_per_second: s.particles_per_second.max(0) as f32,
            speed_min_max: s.speed_min_max,
            size: s.size,
            direction_scale: s.direction_scale,
            current_tile: s.current_texture_tile.max(0) as u32,
            spawn_point_dizzer: s.spawn_point_dizzer,
            jitter_power: s.jitter_power,
            jitter_speed: s.jitter_speed,
            splash_duration: s.splash_duration,
            splash_tile: s.splash_texture_tile.max(0) as u32,
            splash_size_scale: s.splash_size_scale,
            ..self.clone()
        }
    }

    /// Blend two emitter configurations during a state cross-fade: the rate and the sizes
    /// interpolate, the discrete choices (tiles) follow the dominant state.
    pub fn mix(&self, other: &Self, f: f32) -> Self {
        let l = |a: f32, b: f32| a + (b - a) * f;
        let l2 = |a: [f32; 2], b: [f32; 2]| [l(a[0], b[0]), l(a[1], b[1])];
        let dom = if f < 0.5 { self } else { other };
        Self {
            particles_per_second: l(self.particles_per_second, other.particles_per_second),
            speed_min_max: l2(self.speed_min_max, other.speed_min_max),
            size: l2(self.size, other.size),
            direction_scale: l(self.direction_scale, other.direction_scale),
            spawn_point_dizzer: l(self.spawn_point_dizzer, other.spawn_point_dizzer),
            jitter_power: l(self.jitter_power, other.jitter_power),
            jitter_speed: l(self.jitter_speed, other.jitter_speed),
            splash_duration: l(self.splash_duration, other.splash_duration),
            splash_size_scale: l2(self.splash_size_scale, other.splash_size_scale),
            ..dom.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rainy() -> WeatherState {
        WeatherState {
            name: "Rainy".into(),
            particles_per_second: 3100,
            speed_min_max: [800.0, 1000.0],
            size: [1.0, 1.0],
            direction_scale: 14.0,
            current_texture_tile: 2,
            splash_duration: 0.4,
            splash_texture_tile: 3,
            splash_size_scale: [25.0, 25.0],
            ..Default::default()
        }
    }

    #[test]
    fn state_overrides_rate_and_look_but_not_the_cube() {
        let p = PrecipParams::default().with_state(&rainy());
        assert_eq!(p.particles_per_second, 3100.0);
        assert_eq!(p.current_tile, 2);
        assert_eq!(p.splash_tile, 3);
        assert_eq!(p.cube_size, 1000.0);
        assert_eq!(p.max_particles, 3000);
    }

    #[test]
    fn mix_interpolates_the_rate_and_picks_the_dominant_tile() {
        let dry = PrecipParams::default();
        let wet = dry.with_state(&rainy());
        let half = dry.mix(&wet, 0.5);
        assert_eq!(half.particles_per_second, 1550.0);
        assert_eq!(half.current_tile, wet.current_tile);
        assert_eq!(dry.mix(&wet, 0.2).current_tile, dry.current_tile);
    }
}
