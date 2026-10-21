//! Sky objects of `eCSkydome_PS` / `eCSkyObject`: the properties of the world's skydome entity,
//! the sun / moon billboards and the star field.
//!
//! `eCSkyObject` (Engine.dll:0x1026e4c0..) owns a list of camera-centred billboards; the
//! skydome fills it with `StarCount` stars (random positions from `rand()/32767`, size
//! 0.01 .. 0.02), the sun (`SunSize`) and the moon (`MoonSize`) and draws them additively before
//! the dome ([`ENGINE_MAP.md`] 4.2). The brightness of the stars is the alpha of the tint the
//! weather admin pushes; the weather data carries no such channel (all colour alphas are 1), so
//! the visibility below follows the sky brightness instead [I].
use gothic_formats::propstream::{PropValue, PropertySet};

/// Properties of the world's `eCSkydome_PS` entity (the `sysdyn` document).
#[derive(Debug, Clone, PartialEq)]
pub struct SkydomeParams {
    pub star_texture: String,
    pub star_count: u32,
    pub sun_color: [f32; 3],
    pub sun_texture: String,
    /// Half extent of the sun billboard on the unit dome.
    pub sun_size: f32,
    pub moon_color: [f32; 3],
    pub moon_texture: String,
    pub moon_size: f32,
    pub lens_flare_texture: String,
    pub lens_flare_size: f32,
    pub cloud_texture: [String; 2],
    pub cloud_color: [f32; 3],
    pub cloud_direction: [f32; 2],
    pub cloud_size: [f32; 2],
}

impl Default for SkydomeParams {
    /// The values stored in the world.
    fn default() -> Self {
        Self {
            star_texture: "Star.tga".into(),
            star_count: 2000,
            sun_color: [1.0; 3],
            sun_texture: "UNSUN5.TGA".into(),
            sun_size: 0.1,
            moon_color: [0.8, 0.737_254_9, 0.733_333_35],
            moon_texture: "MOON.tga".into(),
            moon_size: 0.07,
            lens_flare_texture: "sun_00.tga".into(),
            lens_flare_size: 0.2,
            cloud_texture: ["Wolkentest_01.tga".into(), "Wolkentest_02.tga".into()],
            cloud_color: [0.5; 3],
            cloud_direction: [0.1, 0.0],
            cloud_size: [0.6, 0.4],
        }
    }
}

impl SkydomeParams {
    pub fn from_properties(set: &PropertySet) -> Self {
        let mut p = Self::default();
        let string = |n: &str, d: &str| set.string(n).map_or_else(|| d.to_string(), str::to_string);
        let color = |n: &str, d: [f32; 3]| match set.get(n) {
            Some(PropValue::ColorRgb(c)) | Some(PropValue::Vec3(c)) => *c,
            _ => d,
        };
        p.star_texture = string("StarTexture", &p.star_texture);
        p.star_count = match set.get("StarCount") {
            Some(PropValue::I32(n)) => (*n).max(0) as u32,
            _ => p.star_count,
        };
        p.sun_color = color("SunColor", p.sun_color);
        p.sun_texture = string("SunTexture", &p.sun_texture);
        p.sun_size = set.float("SunSize").unwrap_or(p.sun_size);
        p.moon_color = color("MoonColor", p.moon_color);
        p.moon_texture = string("MoonTexture", &p.moon_texture);
        p.moon_size = set.float("MoonSize").unwrap_or(p.moon_size);
        p.lens_flare_texture = string("LensFlareTextures", &p.lens_flare_texture);
        p.lens_flare_size = set.float("LensFlareSize").unwrap_or(p.lens_flare_size);
        p.cloud_texture = [
            string("CloudTexture1", &p.cloud_texture[0]),
            string("CloudTexture2", &p.cloud_texture[1]),
        ];
        p.cloud_color = color("CloudColor", p.cloud_color);
        if let Some(PropValue::Vec2(v)) = set.get("CloudDirection") {
            p.cloud_direction = *v;
        }
        p.cloud_size = [
            set.float("CloudSize1").unwrap_or(p.cloud_size[0]),
            set.float("CloudSize2").unwrap_or(p.cloud_size[1]),
        ];
        p
    }
}

/// Smallest and largest star half extent (`rand() / 32767 * 0.01 + 0.01`).
pub const STAR_SIZE: [f32; 2] = [0.01, 0.02];

/// Direction of the moon for a sun state: on its own arc at night (the light direction), opposite
/// the sun by day.
pub fn moon_direction(sun: &crate::SunState) -> [f32; 3] {
    if sun.day {
        [-sun.to_sun[0], -sun.to_sun[1], -sun.to_sun[2]]
    } else {
        sun.to_light
    }
}

/// Star visibility 0..1 from the sky colour: full at night, gone once the sky is bright [I].
pub fn star_visibility(sky: [f32; 3]) -> f32 {
    let lum = 0.3 * sky[0] + 0.59 * sky[1] + 0.11 * sky[2];
    (1.0 - (lum - 0.08) / 0.20).clamp(0.0, 1.0)
}

/// Deterministic star list for the CPU side (tests, tools): unit directions on the upper
/// hemisphere and half extents. The viewer evaluates an equivalent cell hash on the GPU.
pub fn star_field(count: u32, seed: u64) -> Vec<([f32; 3], f32)> {
    let mut s = seed | 1;
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 40) as f32 / (1u64 << 24) as f32
    };
    (0..count)
        .map(|_| {
            let y = next();
            let phi = next() * std::f32::consts::TAU;
            let r = (1.0 - y * y).max(0.0).sqrt();
            let size = STAR_SIZE[0] + next() * (STAR_SIZE[1] - STAR_SIZE[0]);
            ([r * phi.cos(), y, r * phi.sin()], size)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_world_entity() {
        let p = SkydomeParams::default();
        assert_eq!(p.star_count, 2000);
        assert_eq!(p.moon_size, 0.07);
        assert_eq!(p.sun_size, 0.1);
    }

    #[test]
    fn stars_are_above_the_horizon_and_sized_like_the_original() {
        let stars = star_field(2000, 7);
        assert_eq!(stars.len(), 2000);
        for (d, size) in stars {
            let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            assert!((n - 1.0).abs() < 1e-3 && d[1] >= 0.0);
            assert!((STAR_SIZE[0]..=STAR_SIZE[1]).contains(&size));
        }
    }

    #[test]
    fn stars_fade_out_with_daylight() {
        assert_eq!(star_visibility([0.02, 0.08, 0.15]), 1.0);
        assert_eq!(star_visibility([0.3, 0.5, 0.75]), 0.0);
        let mid = star_visibility([0.1, 0.15, 0.3]);
        assert!(mid > 0.0 && mid < 1.0);
    }

    #[test]
    fn the_moon_is_opposite_the_sun_by_day() {
        let day = crate::sun_state(0.5, 0.9);
        let moon = moon_direction(&day);
        let dot: f32 = (0..3).map(|i| moon[i] * day.to_sun[i]).sum();
        assert!((dot + 1.0).abs() < 1e-4);
        let night = crate::sun_state(0.0, 0.9);
        assert_eq!(moon_direction(&night), night.to_light);
    }
}
