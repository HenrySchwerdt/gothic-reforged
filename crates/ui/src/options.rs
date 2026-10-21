//! The video options page (`gCHUDMenuOptionsVideo2`): the settings it edits and their
//! `UserOptions.ini` spelling (`[Options.Video]`), so a real user file loads and saves.
//!
//! Sliders run -1..=1 around the neutral 0 and are scaled by the `[Options.Gamma]` constants
//! of `ge3.INI` when they reach the display ramp. The host applies [`VideoOptions`] to the
//! window and camera ([`ApplyVideo`]); this module owns no engine state.
use bevy::prelude::*;

/// Section of the file.
pub const SECTION: &str = "Options.Video";
/// Scaling constants of `[Options.Gamma]` in the shipped `ge3.INI`.
pub const BRIGHTNESS_SCALING: f32 = 0.2;
pub const CONTRAST_SCALING: f32 = 0.075;
pub const CHANNEL_SCALING: f32 = 0.3;

/// Display modes offered when the host does not list the adapter's.
pub const DEFAULT_MODES: &[(u32, u32)] = &[
    (1024, 768),
    (1280, 720),
    (1280, 1024),
    (1600, 900),
    (1920, 1080),
    (2560, 1440),
    (3840, 2160),
];

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct VideoOptions {
    pub width: u32,
    pub height: u32,
    pub refresh: u32,
    pub brightness: f32,
    pub contrast: f32,
    /// Red, green, blue.
    pub gamma: [f32; 3],
    pub vsync: bool,
}

impl Default for VideoOptions {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            refresh: 60,
            brightness: 0.0,
            contrast: 0.0,
            gamma: [0.0; 3],
            vsync: true,
        }
    }
}

/// One of the five sliders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slider {
    Brightness,
    Contrast,
    Red,
    Green,
    Blue,
}

/// UI -> host: the player pressed Apply.
#[derive(Message, Clone, Debug)]
pub struct ApplyVideo(pub VideoOptions);

impl VideoOptions {
    pub fn slider(&self, s: Slider) -> f32 {
        match s {
            Slider::Brightness => self.brightness,
            Slider::Contrast => self.contrast,
            Slider::Red => self.gamma[0],
            Slider::Green => self.gamma[1],
            Slider::Blue => self.gamma[2],
        }
    }

    /// Set a slider, clamped to -1..=1 and snapped to 1/20 steps like the page's slider.
    pub fn set_slider(&mut self, s: Slider, v: f32) {
        let v = ((v.clamp(-1.0, 1.0) * 20.0).round() / 20.0).clamp(-1.0, 1.0);
        match s {
            Slider::Brightness => self.brightness = v,
            Slider::Contrast => self.contrast = v,
            Slider::Red => self.gamma[0] = v,
            Slider::Green => self.gamma[1] = v,
            Slider::Blue => self.gamma[2] = v,
        }
    }

    /// Display-ramp offsets (brightness, contrast, red, green, blue) after scaling.
    pub fn ramp_offsets(&self) -> [f32; 5] {
        [
            self.brightness * BRIGHTNESS_SCALING,
            self.contrast * CONTRAST_SCALING,
            self.gamma[0] * CHANNEL_SCALING,
            self.gamma[1] * CHANNEL_SCALING,
            self.gamma[2] * CHANNEL_SCALING,
        ]
    }

    /// Key / value pairs for `[Options.Video]`, in the file's order and number format.
    pub fn to_ini(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Resolution.Width", self.width.to_string()),
            ("Resolution.Height", self.height.to_string()),
            ("Resolution.RefreshRate", self.refresh.to_string()),
            ("Brightness", format!("{:.6}", self.brightness)),
            ("Contrast", format!("{:.6}", self.contrast)),
            ("GammaRed", format!("{:.6}", self.gamma[0])),
            ("GammaGreen", format!("{:.6}", self.gamma[1])),
            ("GammaBlue", format!("{:.6}", self.gamma[2])),
            ("VSync", self.vsync.to_string()),
        ]
    }

    /// Read the section; missing or malformed keys keep the default.
    pub fn from_ini(get: impl Fn(&str) -> Option<String>) -> Self {
        let d = Self::default();
        let num = |k: &str, def: u32| get(k).and_then(|v| v.trim().parse().ok()).unwrap_or(def);
        let flt = |k: &str| {
            get(k)
                .and_then(|v| v.trim().parse::<f32>().ok())
                .filter(|v| v.is_finite())
                .map_or(0.0, |v| v.clamp(-1.0, 1.0))
        };
        Self {
            width: num("Resolution.Width", d.width).clamp(640, 16_384),
            height: num("Resolution.Height", d.height).clamp(480, 16_384),
            refresh: num("Resolution.RefreshRate", d.refresh).clamp(24, 1000),
            brightness: flt("Brightness"),
            contrast: flt("Contrast"),
            gamma: [flt("GammaRed"), flt("GammaGreen"), flt("GammaBlue")],
            vsync: get("VSync").map_or(d.vsync, |v| v.trim().eq_ignore_ascii_case("true")),
        }
    }

    /// Index of the listed mode the options currently use (or the closest by pixel count).
    pub fn mode_index(&self, modes: &[(u32, u32)]) -> usize {
        let want = i64::from(self.width) * i64::from(self.height);
        modes
            .iter()
            .enumerate()
            .min_by_key(|(_, (w, h))| (i64::from(*w) * i64::from(*h) - want).abs())
            .map_or(0, |(i, _)| i)
    }
}

/// Parse a flat `[section]` / `key=value` ini text and return `section`'s keys.
pub fn read_section(text: &str, section: &str) -> impl Fn(&str) -> Option<String> {
    let mut inside = false;
    let mut kv = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if let Some(name) = l.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            inside = name.eq_ignore_ascii_case(section);
        } else if inside && let Some((k, v)) = l.split_once('=') {
            kv.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    move |key: &str| {
        let key = key.to_ascii_lowercase();
        kv.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "[Options.Difficulty]\nDifficulty=Medium\n[Options.Video]\nResolution.Width=1920\nResolution.Height=1080\nResolution.RefreshRate=144\nBrightness=0.250000\nContrast=-0.500000\nGammaRed=0.000000\nGammaGreen=3.0\nGammaBlue=bad\nVSync=false\n[Options.Details]\nPerformance=High\n";

    #[test]
    fn reads_the_video_section_of_a_user_file() {
        let v = VideoOptions::from_ini(read_section(SAMPLE, SECTION));
        assert_eq!((v.width, v.height, v.refresh), (1920, 1080, 144));
        assert_eq!(v.brightness, 0.25);
        assert_eq!(v.contrast, -0.5);
        assert_eq!(v.gamma, [0.0, 1.0, 0.0], "clamped / malformed values");
        assert!(!v.vsync);
    }

    #[test]
    fn ini_round_trip_keeps_key_spelling_and_values() {
        let v = VideoOptions {
            width: 2560,
            height: 1440,
            refresh: 120,
            brightness: -0.25,
            contrast: 0.5,
            gamma: [0.1, 0.0, -0.1],
            vsync: true,
        };
        let text = format!(
            "[{SECTION}]\n{}",
            v.to_ini()
                .iter()
                .map(|(k, val)| format!("{k}={val}\n"))
                .collect::<String>()
        );
        assert!(text.contains("Brightness=-0.250000\n"));
        assert!(text.contains("VSync=true\n"));
        assert_eq!(VideoOptions::from_ini(read_section(&text, SECTION)), v);
    }

    #[test]
    fn missing_section_gives_defaults() {
        assert_eq!(
            VideoOptions::from_ini(read_section("[Other]\nA=1\n", SECTION)),
            VideoOptions::default()
        );
    }

    #[test]
    fn sliders_snap_clamp_and_scale() {
        let mut v = VideoOptions::default();
        v.set_slider(Slider::Brightness, 0.27);
        assert_eq!(v.brightness, 0.25);
        v.set_slider(Slider::Blue, 5.0);
        assert_eq!(v.slider(Slider::Blue), 1.0);
        let r = v.ramp_offsets();
        assert!((r[0] - 0.05).abs() < 1e-6 && (r[4] - 0.3).abs() < 1e-6);
    }

    #[test]
    fn nearest_mode_is_found() {
        let v = VideoOptions {
            width: 1900,
            height: 1060,
            ..VideoOptions::default()
        };
        assert_eq!(DEFAULT_MODES[v.mode_index(DEFAULT_MODES)], (1920, 1080));
    }
}
