//! `WeatherStates.g3ws`: weather states (keyframed environment colours over the day plus rain and
//! snow particle parameters) and the weather environments that pick states by probability.
//!
//! Layout (verified on the installation: both sections and the string table are consumed
//! exactly): `GENOMFLE` envelope (`u16 version`, `u32 string table offset`), `GEWS`, `u16 2`,
//! `u32 n`, `n` state objects (accessor header, class entry, property object whose tail is
//! `u16 7`, `u16 name`, then 16 scales), `u32 m`, `m` environments, and at the table offset
//! `0xDEADBEEF`, `u8 1`, `u32 count`, `count x (u16 len, bytes)`.
//!
//! A scale is `u32 version (1)`, `u32 payload size`, `u32 count`, `count x (time, value)` where a
//! colour value is `r g b a` (20 byte entries) and a float value one `f32` (8 byte entries). Time
//! is a fraction of the day in `[0, 1]`.
//!
//! Port of `eCWeatherStates::Read`, `eCColorScale::Read`, `eCFloatScale::Read` and
//! `eCWeatherEnvironment::Read` (Engine.dll:0x1028d8a0 / 0x1015f470 / 0x1015f850 / 0x1028fdf0).
use crate::propstream::Cursor;
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;

const MAX_KEYS: usize = 4096;

/// Keyframes of one colour group over the day.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ColorScale {
    pub keys: Vec<(f32, [f32; 4])>,
}

/// Keyframes of one scalar over the day.
#[derive(Debug, Clone, Default, Serialize)]
pub struct FloatScale {
    pub keys: Vec<(f32, f32)>,
}

impl ColorScale {
    /// Port of `eCColorScale::CalcValue` (Engine.dll:0x1015f2b0): linear interpolation between
    /// neighbouring keys, clamped to the first/last key outside the range; an empty scale is 1.
    pub fn at(&self, t: f32) -> [f32; 4] {
        let Some((i, j, f)) = segment(self.keys.iter().map(|k| k.0), t) else {
            return [1.0; 4];
        };
        let (a, b) = (self.keys[i].1, self.keys[j].1);
        std::array::from_fn(|c| a[c] + (b[c] - a[c]) * f)
    }
}

impl FloatScale {
    /// Port of `eCFloatScale::CalcValue`: same interpolation as [`ColorScale::at`].
    pub fn at(&self, t: f32) -> f32 {
        let Some((i, j, f)) = segment(self.keys.iter().map(|k| k.0), t) else {
            return 1.0;
        };
        self.keys[i].1 + (self.keys[j].1 - self.keys[i].1) * f
    }
}

/// Key pair and blend factor around `t` (keys sorted by time).
fn segment(times: impl Iterator<Item = f32>, t: f32) -> Option<(usize, usize, f32)> {
    let times: Vec<f32> = times.collect();
    if times.is_empty() {
        return None;
    }
    let upper = times.iter().position(|&k| k >= t).unwrap_or(times.len());
    if upper == 0 {
        return Some((0, 0, 0.0));
    }
    if upper == times.len() {
        let last = times.len() - 1;
        return Some((last, last, 0.0));
    }
    let (lo, hi) = (upper - 1, upper);
    let span = times[hi] - times[lo];
    let f = if span > 0.0 {
        (t - times[lo]) / span
    } else {
        0.0
    };
    Some((lo, hi, f))
}

/// The 16 scales of a state, named by what `eCWeatherAdmin::Process` does with them.
#[derive(Debug, Clone, Default, Serialize)]
pub struct StateScales {
    /// Directional light colour (the sun).
    pub sun_diffuse: ColorScale,
    pub sun_specular: ColorScale,
    pub sun_intensity: FloatScale,
    /// Colour multiplied with the sun colour to derive the hemisphere back / sun light terms.
    pub ambient_back: ColorScale,
    /// Hemisphere "general ambient" colour.
    pub ambient_general: ColorScale,
    pub ambient_intensity: FloatScale,
    pub fog_color: ColorScale,
    /// cm.
    pub fog_start: FloatScale,
    pub sky_color: ColorScale,
    pub haze_color: ColorScale,
    /// cm.
    pub fog_end: FloatScale,
    /// Pushed to the skydome next to the cloud colour.
    pub cloud_density: FloatScale,
    pub cloud_color: ColorScale,
    /// Two scalars whose consumer was not identified (single key / 5..20 range).
    pub extra_a: FloatScale,
    pub shadow_density: FloatScale,
    pub extra_b: FloatScale,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct WeatherState {
    pub name: String,
    pub particles_per_second: i32,
    pub speed_min_max: [f32; 2],
    pub size: [f32; 2],
    pub direction_scale: f32,
    pub current_texture_tile: i32,
    pub spawn_point_dizzer: f32,
    pub jitter_power: f32,
    pub jitter_speed: f32,
    pub splash_duration: f32,
    pub splash_texture_tile: i32,
    pub splash_size_scale: [f32; 2],
    pub loop_sample: String,
    pub loop_volume: f32,
    pub scales: StateScales,
}

/// One state entry of an environment: probability and min / max duration (fractions of a day).
#[derive(Debug, Clone, Serialize)]
pub struct EnvironmentEntry {
    pub state: String,
    pub probability: f32,
    pub min_duration: f32,
    pub max_duration: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct WeatherEnvironment {
    pub name: String,
    pub entries: Vec<EnvironmentEntry>,
    pub climate: String,
    pub climate_id: i32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct WeatherData {
    pub states: Vec<WeatherState>,
    pub environments: Vec<WeatherEnvironment>,
}

impl WeatherData {
    pub fn state(&self, name: &str) -> Option<&WeatherState> {
        self.states.iter().find(|s| s.name == name)
    }
    pub fn environment(&self, name: &str) -> Option<&WeatherEnvironment> {
        self.environments.iter().find(|e| e.name == name)
    }
}

fn color_scale(cur: &mut Cursor) -> Result<ColorScale> {
    ensure!(cur.u32()? == 1, "color scale version");
    let size = cur.u32()? as usize;
    if size == 0 {
        return Ok(ColorScale::default());
    }
    let n = cur.u32()? as usize;
    ensure!(n <= MAX_KEYS && n * 20 == size, "color scale size");
    let mut keys = Vec::with_capacity(n);
    for _ in 0..n {
        let t = cur.f32()?;
        keys.push((t, cur.floats::<4>()?));
    }
    Ok(ColorScale { keys })
}

fn float_scale(cur: &mut Cursor) -> Result<FloatScale> {
    ensure!(cur.u32()? == 1, "float scale version");
    let size = cur.u32()? as usize;
    if size == 0 {
        return Ok(FloatScale::default());
    }
    let n = cur.u32()? as usize;
    ensure!(n <= MAX_KEYS && n * 8 == size, "float scale size");
    let mut keys = Vec::with_capacity(n);
    for _ in 0..n {
        keys.push((cur.f32()?, cur.f32()?));
    }
    Ok(FloatScale { keys })
}

fn read_scales(cur: &mut Cursor) -> Result<StateScales> {
    // File order of tail version 7 (see `eCWeatherStates::Read`).
    Ok(StateScales {
        sun_diffuse: color_scale(cur)?,
        sun_specular: color_scale(cur)?,
        sun_intensity: float_scale(cur)?,
        ambient_back: color_scale(cur)?,
        ambient_general: color_scale(cur)?,
        ambient_intensity: float_scale(cur)?,
        fog_color: color_scale(cur)?,
        fog_start: float_scale(cur)?,
        sky_color: color_scale(cur)?,
        haze_color: color_scale(cur)?,
        fog_end: float_scale(cur)?,
        cloud_density: float_scale(cur)?,
        cloud_color: color_scale(cur)?,
        extra_a: float_scale(cur)?,
        shadow_density: float_scale(cur)?,
        extra_b: float_scale(cur)?,
    })
}

fn table(bytes: &[u8], offset: usize) -> Result<Vec<Vec<u8>>> {
    let mut cur = Cursor::with_range(bytes, offset, bytes.len())?;
    ensure!(cur.u32()? == 0xDEAD_BEEF, "string table marker");
    ensure!(cur.u8()? == 1, "string table flag");
    let n = cur.u32()? as usize;
    ensure!(n <= 65_536, "string table size");
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let len = cur.u16()? as usize;
        out.push(cur.take(len)?.to_vec());
    }
    Ok(out)
}

/// Parse `WeatherStates.g3ws`.
pub fn parse(bytes: &[u8]) -> Result<WeatherData> {
    ensure!(
        bytes.len() > 32 && &bytes[..8] == b"GENOMFLE",
        "not a genome file"
    );
    let mut cur = Cursor::new(bytes);
    cur.skip(8)?;
    let _version = cur.u16()?;
    let table_off = cur.u32()? as usize;
    let strings = table(bytes, table_off)?;
    let s = |i: u16| -> Result<String> {
        let b = strings.get(i as usize).context("string index")?;
        // The writer leaves uninitialised bytes after the NUL of some entries.
        let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        Ok(String::from_utf8_lossy(&b[..end]).into_owned())
    };
    ensure!(cur.take(4)? == b"GEWS", "GEWS magic");
    ensure!(cur.u16()? == 2, "weather file version");
    let n = cur.u32()? as usize;
    ensure!(n <= 256, "state count");
    let mut data = WeatherData::default();
    for _ in 0..n {
        ensure!(cur.take(6)? == [1, 0, 1, 1, 0, 1], "accessor");
        let _class = cur.u16()?;
        cur.skip(5)?; // filler `01 00 00 vv 00`
        ensure!(cur.u16()? == 0x53, "object version");
        let size = cur.u32()? as usize;
        ensure!(size <= cur.remaining(), "object size");
        let end = cur.pos + size;
        ensure!(cur.u16()? == 30, "property list version");
        let count = cur.u32()?;
        let mut st = WeatherState::default();
        for _ in 0..count {
            let name = s(cur.u16()?)?;
            let _ty = cur.u16()?;
            ensure!(cur.u16()? == 30, "property version");
            let len = cur.u32()? as usize;
            let p = cur.take(len)?;
            let f = |o: usize| {
                p.get(o..o + 4)
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            };
            let i = |o: usize| {
                p.get(o..o + 4)
                    .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            };
            match name.as_str() {
                "ParticlesPerSecond" => st.particles_per_second = i(0).unwrap_or(0),
                "SpeedMinMax" => st.speed_min_max = [f(0).unwrap_or(0.0), f(4).unwrap_or(0.0)],
                "Size" => st.size = [f(0).unwrap_or(0.0), f(4).unwrap_or(0.0)],
                "DirectionScale" => st.direction_scale = f(0).unwrap_or(0.0),
                "CurrentTextureTile" => st.current_texture_tile = i(0).unwrap_or(0),
                "SpawnPointDizzer" => st.spawn_point_dizzer = f(0).unwrap_or(0.0),
                "JitterPower" => st.jitter_power = f(0).unwrap_or(0.0),
                "JitterSpeed" => st.jitter_speed = f(0).unwrap_or(0.0),
                "SplashDuration" => st.splash_duration = f(0).unwrap_or(0.0),
                "SplashTextureTile" => st.splash_texture_tile = i(0).unwrap_or(0),
                "SplashSizeScale" => {
                    st.splash_size_scale = [f(0).unwrap_or(0.0), f(4).unwrap_or(0.0)]
                }
                "LoopSample" if p.len() == 2 => {
                    st.loop_sample = s(u16::from_le_bytes([p[0], p[1]]))?
                }
                "LoopVolume" => st.loop_volume = f(0).unwrap_or(0.0),
                _ => {}
            }
        }
        ensure!(cur.u16()? == 7, "weather state tail version");
        st.name = s(cur.u16()?)?;
        st.scales = read_scales(&mut cur)?;
        ensure!(cur.pos == end, "weather state not consumed exactly");
        data.states.push(st);
    }
    let m = cur.u32()? as usize;
    ensure!(m <= 256, "environment count");
    for _ in 0..m {
        let ver = cur.u16()?;
        if ver != 3 {
            bail!("environment version {ver}");
        }
        let name = s(cur.u16()?)?;
        ensure!(cur.u8()? == 1, "environment map flag");
        let k = cur.u32()? as usize;
        ensure!(k <= 256, "environment entries");
        let mut entries = Vec::with_capacity(k);
        for _ in 0..k {
            entries.push(EnvironmentEntry {
                state: s(cur.u16()?)?,
                probability: cur.f32()?,
                min_duration: cur.f32()?,
                max_duration: cur.f32()?,
            });
        }
        let climate = s(cur.u16()?)?;
        let climate_id = cur.i32()?;
        data.environments.push(WeatherEnvironment {
            name,
            entries,
            climate,
            climate_id,
        });
    }
    ensure!(cur.pos == table_off, "weather file not consumed exactly");
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_interpolates_and_clamps() {
        let s = FloatScale {
            keys: vec![(0.2, 1.0), (0.4, 3.0)],
        };
        assert_eq!(s.at(0.0), 1.0);
        assert!((s.at(0.3) - 2.0).abs() < 1e-6);
        assert_eq!(s.at(0.9), 3.0);
        assert_eq!(FloatScale::default().at(0.5), 1.0);
        let c = ColorScale {
            keys: vec![(0.0, [0.0; 4]), (1.0, [1.0; 4])],
        };
        assert_eq!(c.at(0.25), [0.25; 4]);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse(b"nope").is_err());
    }
}
