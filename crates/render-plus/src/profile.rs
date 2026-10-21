//! The render-plus profile: which effects are on and how hard they work. Plain data (serde), so
//! it can live in the settings file. `RenderProfile::default()` is the faithful look:
//! every effect off.
use serde::{Deserialize, Serialize};

/// One switchable effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Effect {
    /// Screen-space ambient occlusion (Bevy's GTAO-style SSAO).
    Ssao,
    /// Temporal anti-aliasing plus contrast-adaptive sharpening.
    Taa,
    /// Volumetric fog with light shafts from the sun.
    Volumetric,
    /// Soft (PCSS) sun shadows, temporal shadow filtering and contact shadows.
    SoftShadows,
    /// Screen-space reflections (forces deferred shading on the camera).
    Ssr,
}

impl Effect {
    pub const ALL: [Effect; 5] = [
        Effect::Ssao,
        Effect::Taa,
        Effect::Volumetric,
        Effect::SoftShadows,
        Effect::Ssr,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Effect::Ssao => "ssao",
            Effect::Taa => "taa",
            Effect::Volumetric => "volumetric",
            Effect::SoftShadows => "soft_shadows",
            Effect::Ssr => "ssr",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SsaoQuality {
    Low,
    #[default]
    Medium,
    High,
    Ultra,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ssao {
    pub enabled: bool,
    pub quality: SsaoQuality,
    /// Sample radius in view-space metres.
    pub radius: f32,
}

impl Default for Ssao {
    fn default() -> Self {
        Self {
            enabled: false,
            quality: SsaoQuality::Medium,
            radius: 0.75,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Taa {
    pub enabled: bool,
    /// Contrast-adaptive sharpening strength after TAA (0 = off, 1 = max).
    pub sharpening: f32,
}

impl Default for Taa {
    fn default() -> Self {
        Self {
            enabled: false,
            sharpening: 0.5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Volumetric {
    pub enabled: bool,
    /// Ray-march steps per pixel.
    pub steps: u32,
    /// Ambient light the fog scatters (keep it near the scene ambient).
    pub ambient: f32,
    /// Ray start jitter in metres (hides banding when TAA is on).
    pub jitter: f32,
    /// Density of the fog volume the plugin keeps around the camera (0 = none; hosts can
    /// place their own `FogVolume`s instead).
    pub density: f32,
    /// Horizontal and vertical size of that volume in metres.
    pub extent: [f32; 2],
}

impl Default for Volumetric {
    fn default() -> Self {
        Self {
            enabled: false,
            steps: 48,
            ambient: 0.05,
            jitter: 0.5,
            density: 0.03,
            extent: [300.0, 60.0],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoftShadows {
    pub enabled: bool,
    /// Angular size of the sun as a light size (PCSS penumbra), only with the `pcss` feature.
    pub sun_size: f32,
    /// Screen-space contact shadows for small-scale contact (feet, props on the ground).
    pub contact: bool,
    /// Contact shadow ray length in metres.
    pub contact_length: f32,
}

impl Default for SoftShadows {
    fn default() -> Self {
        Self {
            enabled: false,
            sun_size: 0.6,
            contact: true,
            contact_length: 0.3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ssr {
    pub enabled: bool,
    /// Linear ray-march steps.
    pub steps: u32,
    /// Surfaces rougher than this get no SSR.
    pub max_roughness: f32,
}

impl Default for Ssr {
    fn default() -> Self {
        Self {
            enabled: false,
            steps: 16,
            max_roughness: 0.45,
        }
    }
}

/// Frame-time budget: when the smoothed frame time stays above `target_ms`, the governor
/// switches effects off in `shed_order`, and back on once there is room again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Budget {
    pub enabled: bool,
    pub target_ms: f32,
    /// First entry is dropped first.
    pub shed_order: Vec<Effect>,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            enabled: false,
            target_ms: 16.6,
            shed_order: vec![
                Effect::Ssr,
                Effect::Volumetric,
                Effect::SoftShadows,
                Effect::Ssao,
                Effect::Taa,
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preset {
    /// Everything off: the original look.
    Faithful,
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "render", derive(bevy::prelude::Resource))]
#[serde(default)]
pub struct RenderProfile {
    pub ssao: Ssao,
    pub taa: Taa,
    pub volumetric: Volumetric,
    pub soft_shadows: SoftShadows,
    pub ssr: Ssr,
    pub budget: Budget,
}

impl RenderProfile {
    pub fn preset(preset: Preset) -> Self {
        let mut p = Self::default();
        match preset {
            Preset::Faithful => {}
            Preset::Low => {
                p.taa.enabled = true;
                p.ssao.enabled = true;
                p.ssao.quality = SsaoQuality::Low;
            }
            Preset::Medium => {
                p.taa.enabled = true;
                p.ssao.enabled = true;
                p.soft_shadows.enabled = true;
                p.volumetric.enabled = true;
                p.volumetric.steps = 32;
            }
            Preset::High => {
                for e in Effect::ALL {
                    p.set(e, true);
                }
                p.ssao.quality = SsaoQuality::High;
                p.volumetric.steps = 64;
                p.ssr.steps = 24;
            }
        }
        if preset != Preset::Faithful {
            p.budget.enabled = true;
        }
        p
    }

    pub fn is_on(&self, e: Effect) -> bool {
        match e {
            Effect::Ssao => self.ssao.enabled,
            Effect::Taa => self.taa.enabled,
            Effect::Volumetric => self.volumetric.enabled,
            Effect::SoftShadows => self.soft_shadows.enabled,
            Effect::Ssr => self.ssr.enabled,
        }
    }

    pub fn set(&mut self, e: Effect, on: bool) {
        match e {
            Effect::Ssao => self.ssao.enabled = on,
            Effect::Taa => self.taa.enabled = on,
            Effect::Volumetric => self.volumetric.enabled = on,
            Effect::SoftShadows => self.soft_shadows.enabled = on,
            Effect::Ssr => self.ssr.enabled = on,
        }
    }

    /// Only `effect` on (the A/B runs of the demo), other settings kept.
    pub fn only(&self, effect: Effect) -> Self {
        let mut p = self.clone();
        for e in Effect::ALL {
            p.set(e, e == effect);
        }
        p
    }

    pub fn is_faithful(&self) -> bool {
        Effect::ALL.iter().all(|&e| !self.is_on(e))
    }
}
