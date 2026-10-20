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
}

impl BloomPost {
    pub fn original() -> Self {
        let f = |k: &str, d: f32| {
            std::env::var(k)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(d)
        };
        Self {
            bloom: Vec4::new(
                f("GR_BLOOM_T", 0.59),
                f("GR_BLOOM_I", 1.7),
                f("GR_BLOOM_A", 2.75),
                f("GR_BLOOM_STEP", 2.5),
            ),
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
