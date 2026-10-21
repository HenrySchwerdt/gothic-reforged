//! Original Gothic 3 bloom (`Bloom=1`, Performance=High): `shaders/post.wesl`.
use bevy::{
    asset::{embedded_asset, load_embedded_asset},
    core_pipeline::fullscreen_material::{FullscreenMaterial, FullscreenMaterialPlugin},
    prelude::*,
    render::{RenderApp, extract_component::ExtractComponent, render_resource::ShaderType},
    shader::{Shader, ShaderRef},
};
use std::sync::OnceLock;

static POST_FS: OnceLock<Handle<Shader>> = OnceLock::new();

pub struct PostPlugin;

impl Plugin for PostPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/post.wesl");
        let _ = POST_FS.set(load_embedded_asset!(app, "shaders/post.wesl"));
        app.add_plugins(FullscreenMaterialPlugin::<BloomPost>::default());
    }
}

/// Bloom parameters: x threshold, y intensity (ge3.INI Render.BloomThreshold / Intensity),
/// z aberration (Render.BloomAberration), w blur tap spacing in pixels.
#[derive(Component, ExtractComponent, Clone, Copy, ShaderType, Default)]
#[extract_app(RenderApp)]
pub struct BloomPost {
    pub bloom: Vec4,
    /// x exposure (0 disables the HDR path), y gamma, z vignette, w blue-shift threshold
    /// (`Render.HDRExposure / HDRGamma / HDRVignette / HDRBlueShiftThreshold`).
    pub hdr: Vec4,
    /// x blue-shift intensity (`Render.HDRBlueShiftIntensity`).
    pub hdr2: Vec4,
}

impl BloomPost {
    pub fn original() -> Self {
        let f = |k: &str, d: f32| {
            std::env::var(k)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(d)
        };
        // The HDR exposure curve (Bloom=2) over-exposes the original light values in our scene (checked
        // against the reference screenshot); the LDR bloom chain matches. `GR_HDR=1` enables it.
        let hdr = std::env::var_os("GR_HDR").is_some();
        // HDR mode (Bloom=2) uses the glow constants instead of the bloom ones.
        let (t, i, a) = if hdr {
            (0.69, 3.0, 2.75)
        } else {
            (0.59, 1.7, 2.75)
        };
        Self {
            bloom: Vec4::new(
                f("GR_BLOOM_T", t),
                f("GR_BLOOM_I", i),
                f("GR_BLOOM_A", a),
                f("GR_BLOOM_STEP", 2.5),
            ),
            hdr: if hdr {
                Vec4::new(f("GR_EXPOSURE", 2.85), 0.60, 0.30, 4.0)
            } else {
                Vec4::ZERO
            },
            hdr2: Vec4::new(1.0, 0.0, 0.0, 0.0),
        }
    }
}

impl FullscreenMaterial for BloomPost {
    fn fragment_shader() -> ShaderRef {
        POST_FS
            .get()
            .map_or(ShaderRef::Default, |h| ShaderRef::Handle(h.clone()))
    }
}
