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
        embedded_asset!(app, "shaders/char.wesl");
        embedded_asset!(app, "shaders/char_prepass.wesl");
        let _ = CHAR_FS.set(load_embedded_asset!(app, "shaders/char.wesl"));
        let _ = CHAR_PREPASS_FS.set(load_embedded_asset!(app, "shaders/char_prepass.wesl"));
        app.add_systems(Update, convert_standard);
        app.add_plugins((
            MaterialPlugin::<CharMaterial>::default(),
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
    /// Original lighting model (ORIGINAL_SHADING.md): sun diffuse colour (rgb, gamma space),
    /// w = lightmap gain.
    pub sun: Vec4,
    /// Linear spherical-harmonic ambient: sh[0] constant, sh[1..3] x/y/z coefficients (rgb).
    pub sh: [Vec4; 4],
}

/// Original-game light set (gamma-space values, see `docs/research/ORIGINAL_SHADING.md`).
pub fn light_env() -> (Vec4, [Vec4; 4]) {
    static ENV: OnceLock<(Vec4, [Vec4; 4])> = OnceLock::new();
    *ENV.get_or_init(light_env_raw)
}

fn light_env_raw() -> (Vec4, [Vec4; 4]) {
    let f = |k: &str, d: f32| -> f32 {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(d)
    };
    let sun_i = f("GR_SUN", 1.9);
    let amb_i = f("GR_AMB", 1.3);
    let sun = Vec4::new(1.0 * sun_i, 0.92 * sun_i, 0.78 * sun_i, 4.0);
    let sh0 = Vec3::new(0.30, 0.35, 0.42) * amb_i;
    let sh_y = Vec3::new(0.12, 0.14, 0.20) * amb_i;
    let sh_x = Vec3::new(0.03, 0.02, 0.0) * amb_i;
    // The environment carries an AmbientBackLightColor next to AmbientGeneralColor (see the
    // eCWeatherZone_PS properties): light arriving from the side opposite the sun that fills the
    // shaded half of every object. It enters the 2-band SH as back * (-sun_dir) . n.
    let s = Quat::from_euler(EulerRot::YXZ, super::SUN_ROT.0, super::SUN_ROT.1, 0.0) * Vec3::Z;
    let back_i = f("GR_BACK", 0.75);
    let back = Vec3::new(f("GR_BACKR", 1.0), f("GR_BACKG", 0.92), f("GR_BACKB", 0.80)) * back_i;
    (
        sun,
        [
            sh0.extend(0.0),
            (sh_x - back * s.x).extend(0.0),
            (sh_y - back * s.y).extend(0.0),
            (-back * s.z).extend(0.0),
        ],
    )
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
            sun: light_env().0,
            sh: light_env().1,
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
    #[texture(1)]
    #[sampler(2)]
    pub clouds: Option<Handle<Image>>,
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

// ---- characters / props: StandardMaterial entities are converted to this -------------------

static CHAR_FS: OnceLock<Handle<Shader>> = OnceLock::new();
static CHAR_PREPASS_FS: OnceLock<Handle<Shader>> = OnceLock::new();

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct CharParams {
    pub tint: Vec4,
    /// x alpha cutoff (0 none), y rim start, z rim end, w rim intensity (ge3.INI Render.RimLight*).
    pub misc: Vec4,
    pub sun: Vec4,
    pub sh: [Vec4; 4],
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct CharMaterial {
    #[uniform(0)]
    pub params: CharParams,
    #[texture(1)]
    #[sampler(2)]
    pub tex: Option<Handle<Image>>,
    pub alpha_mode: AlphaMode,
}

impl Material for CharMaterial {
    fn fragment_shader() -> ShaderRef {
        CHAR_FS
            .get()
            .map_or(ShaderRef::Default, |h| ShaderRef::Handle(h.clone()))
    }
    fn prepass_fragment_shader() -> ShaderRef {
        CHAR_PREPASS_FS
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
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// Swaps newly spawned `StandardMaterial` entities (characters, items) onto [`CharMaterial`].
fn convert_standard(
    mut commands: Commands,
    added: Query<
        (Entity, &MeshMaterial3d<StandardMaterial>),
        Added<MeshMaterial3d<StandardMaterial>>,
    >,
    standard: Res<Assets<StandardMaterial>>,
    mut converted: ResMut<Assets<CharMaterial>>,
    mut cache: Local<std::collections::HashMap<AssetId<StandardMaterial>, Handle<CharMaterial>>>,
) {
    if std::env::var_os("GR_NOCHAR").is_some() {
        return;
    }
    for (e, m) in &added {
        let Some(src) = standard.get(&m.0) else {
            continue;
        };
        let h = cache
            .entry(m.0.id())
            .or_insert_with(|| {
                let (sun, sh) = light_env();
                let cutoff = match src.alpha_mode {
                    AlphaMode::Mask(c) => c.max(0.05),
                    _ => 0.0,
                };
                converted.add(CharMaterial {
                    params: CharParams {
                        tint: src.base_color.to_linear().to_vec4(),
                        misc: Vec4::new(cutoff, 0.65, 2.2, 1.0),
                        sun,
                        sh,
                    },
                    tex: src.base_color_texture.clone(),
                    alpha_mode: src.alpha_mode,
                })
            })
            .clone();
        commands
            .entity(e)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert(MeshMaterial3d(h));
    }
}
