//! Gothic surface, sky and water materials (custom WESL shaders, see `shaders/`).
use bevy::{
    asset::{embedded_asset, load_embedded_asset},
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{
        AsBindGroup, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    },
    shader::{Shader, ShaderRef},
};
use std::sync::OnceLock;

static GOTHIC_FS: OnceLock<Handle<Shader>> = OnceLock::new();
static GOTHIC_PREPASS_FS: OnceLock<Handle<Shader>> = OnceLock::new();
static SKY_FS: OnceLock<Handle<Shader>> = OnceLock::new();
static WATER_FS: OnceLock<Handle<Shader>> = OnceLock::new();

pub struct GothicMaterialsPlugin;

impl Plugin for GothicMaterialsPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/gothic.wesl");
        embedded_asset!(app, "shaders/gothic_prepass.wesl");
        embedded_asset!(app, "shaders/sky.wesl");
        embedded_asset!(app, "shaders/water.wesl");
        let _ = GOTHIC_FS.set(load_embedded_asset!(app, "shaders/gothic.wesl"));
        let _ = GOTHIC_PREPASS_FS.set(load_embedded_asset!(app, "shaders/gothic_prepass.wesl"));
        let _ = SKY_FS.set(load_embedded_asset!(app, "shaders/sky.wesl"));
        let _ = WATER_FS.set(load_embedded_asset!(app, "shaders/water.wesl"));
        app.add_plugins((
            MaterialPlugin::<GothicMaterial>::default(),
            MaterialPlugin::<SkyMaterial>::default(),
            MaterialPlugin::<WaterMaterial>::default(),
        ));
    }
}

pub const F_A1: u32 = 1;
pub const F_B0: u32 = 2;
pub const F_B1: u32 = 4;
pub const F_NA: u32 = 8;
pub const F_NB: u32 = 16;
pub const F_VBLEND: u32 = 32;
pub const F_MASK: u32 = 64;
pub const F_FADE: u32 = 256;
#[allow(dead_code)]
pub const F_UNLIT: u32 = 512;

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct GothicParams {
    pub tint_a: Vec4,
    pub tint_b: Vec4,
    pub scale_a: Vec4,
    pub scale_b: Vec4,
    pub scale_n: Vec4,
    /// x fade start, y fade end (world units), z alpha cutoff, w normal-map Y sign.
    pub misc: Vec4,
    pub flags: u32,
    pub ao_strength: f32,
    pub normal_strength: f32,
    pub roughness: f32,
}

impl Default for GothicParams {
    fn default() -> Self {
        Self {
            tint_a: Vec4::ONE,
            tint_b: Vec4::ONE,
            scale_a: Vec4::ONE,
            scale_b: Vec4::ONE,
            scale_n: Vec4::ONE,
            misc: Vec4::new(0.0, 1.0e9, 0.5, 1.0),
            flags: 0,
            ao_strength: 0.7,
            normal_strength: 1.0,
            roughness: 0.9,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct GothicKey {
    two_sided: bool,
}

impl From<&GothicMaterial> for GothicKey {
    fn from(m: &GothicMaterial) -> Self {
        Self {
            two_sided: m.two_sided,
        }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[bind_group_data(GothicKey)]
pub struct GothicMaterial {
    #[uniform(0)]
    pub params: GothicParams,
    #[texture(1)]
    #[sampler(2)]
    pub a0: Option<Handle<Image>>,
    #[texture(3)]
    pub a1: Option<Handle<Image>>,
    #[texture(4)]
    pub b0: Option<Handle<Image>>,
    #[texture(5)]
    pub b1: Option<Handle<Image>>,
    #[texture(6)]
    pub na: Option<Handle<Image>>,
    #[texture(7)]
    pub nb: Option<Handle<Image>>,
    pub alpha_mode: AlphaMode,
    pub two_sided: bool,
}

impl Material for GothicMaterial {
    fn fragment_shader() -> ShaderRef {
        GOTHIC_FS
            .get()
            .map_or(ShaderRef::Default, |h| ShaderRef::Handle(h.clone()))
    }
    fn prepass_fragment_shader() -> ShaderRef {
        GOTHIC_PREPASS_FS
            .get()
            .map_or(ShaderRef::Default, |h| ShaderRef::Handle(h.clone()))
    }
    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = if key.bind_group_data.two_sided {
            None
        } else {
            Some(Face::Back)
        };
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct SkyParams {
    pub sun_dir: Vec4,
    pub zenith: Vec4,
    pub horizon: Vec4,
    pub ground: Vec4,
    pub sun_color: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct SkyMaterial {
    #[uniform(0)]
    pub params: SkyParams,
}

impl Material for SkyMaterial {
    fn fragment_shader() -> ShaderRef {
        SKY_FS
            .get()
            .map_or(ShaderRef::Default, |h| ShaderRef::Handle(h.clone()))
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct WaterParams {
    pub sun_dir: Vec4,
    pub deep: Vec4,
    pub shallow: Vec4,
    pub sky: Vec4,
    pub sun_color: Vec4,
    /// x wave strength, y normal Y sign, z base opacity.
    pub misc: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct WaterMaterial {
    #[uniform(0)]
    pub params: WaterParams,
    #[texture(1)]
    #[sampler(2)]
    pub n0: Option<Handle<Image>>,
    #[texture(3)]
    pub n1: Option<Handle<Image>>,
}

impl Material for WaterMaterial {
    fn fragment_shader() -> ShaderRef {
        WATER_FS
            .get()
            .map_or(ShaderRef::Default, |h| ShaderRef::Handle(h.clone()))
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}
