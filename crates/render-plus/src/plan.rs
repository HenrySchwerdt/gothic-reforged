//! What a profile means for a camera and the sun, as plain data. The plugin turns a
//! [`Plan`] into Bevy components; keeping the decisions here makes them testable without a
//! GPU.
use crate::profile::{Effect, RenderProfile, Ssao, Ssr, Volumetric};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    pub ssao: Option<Ssao>,
    pub taa: bool,
    /// CAS strength (only together with TAA).
    pub sharpening: Option<f32>,
    pub volumetric: Option<Volumetric>,
    /// PCSS light size for the sun.
    pub sun_soft_size: Option<f32>,
    /// Contact shadow ray length.
    pub contact_shadows: Option<f32>,
    /// Temporal shadow filtering (only with TAA, it is noisy without).
    pub temporal_shadow_filter: bool,
    pub ssr: Option<Ssr>,
    /// SSAO, TAA and deferred shading all need MSAA off in Bevy 0.20.
    pub msaa_off: bool,
    pub deferred: bool,
    pub depth_prepass: bool,
    pub normal_prepass: bool,
    pub motion_prepass: bool,
}

impl Plan {
    /// `suppressed`: effects the budget governor has switched off for now.
    pub fn new(profile: &RenderProfile, suppressed: &[Effect]) -> Self {
        let on = |e: Effect| profile.is_on(e) && !suppressed.contains(&e);
        let mut p = Plan::default();
        if on(Effect::Ssao) {
            p.ssao = Some(Ssao {
                radius: profile.ssao.radius.max(0.01),
                ..profile.ssao
            });
        }
        if on(Effect::Taa) {
            p.taa = true;
            if profile.taa.sharpening > 0.0 {
                p.sharpening = Some(profile.taa.sharpening.min(1.0));
            }
        }
        if on(Effect::Volumetric) {
            let v = profile.volumetric;
            p.volumetric = Some(Volumetric {
                steps: v.steps.max(1),
                ambient: v.ambient.max(0.0),
                jitter: v.jitter.max(0.0),
                density: v.density.max(0.0),
                ..v
            });
        }
        if on(Effect::SoftShadows) {
            let s = profile.soft_shadows;
            p.sun_soft_size = Some(s.sun_size.max(0.0));
            if s.contact {
                p.contact_shadows = Some(s.contact_length.max(0.01));
            }
            p.temporal_shadow_filter = p.taa;
        }
        if on(Effect::Ssr) {
            p.ssr = Some(Ssr {
                steps: profile.ssr.steps.max(1),
                max_roughness: profile.ssr.max_roughness.clamp(0.05, 1.0),
                ..profile.ssr
            });
            p.deferred = true;
        }
        p.msaa_off = p.ssao.is_some() || p.taa || p.deferred;
        p.depth_prepass = p.ssao.is_some() || p.taa || p.contact_shadows.is_some() || p.deferred;
        p.normal_prepass = p.ssao.is_some();
        p.motion_prepass = p.taa;
        p
    }

    pub fn is_empty(&self) -> bool {
        *self == Plan::default()
    }

    /// Effects actually active in this plan.
    pub fn active(&self) -> Vec<Effect> {
        let mut v = Vec::new();
        if self.ssao.is_some() {
            v.push(Effect::Ssao);
        }
        if self.taa {
            v.push(Effect::Taa);
        }
        if self.volumetric.is_some() {
            v.push(Effect::Volumetric);
        }
        if self.sun_soft_size.is_some() {
            v.push(Effect::SoftShadows);
        }
        if self.ssr.is_some() {
            v.push(Effect::Ssr);
        }
        v
    }
}
