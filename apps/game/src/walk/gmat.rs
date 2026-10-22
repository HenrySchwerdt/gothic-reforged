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
static GOTHIC_VS: OnceLock<Handle<Shader>> = OnceLock::new();
static SKY_FS: OnceLock<Handle<Shader>> = OnceLock::new();
static WATER_FS: OnceLock<Handle<Shader>> = OnceLock::new();

pub struct GothicMaterialsPlugin;

impl Plugin for GothicMaterialsPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/gothic.wesl");
        embedded_asset!(app, "shaders/gothic_prepass.wesl");
        embedded_asset!(app, "shaders/gothic_vertex.wesl");
        embedded_asset!(app, "shaders/sky.wesl");
        embedded_asset!(app, "shaders/water.wesl");
        let _ = GOTHIC_FS.set(load_embedded_asset!(app, "shaders/gothic.wesl"));
        let _ = GOTHIC_PREPASS_FS.set(load_embedded_asset!(app, "shaders/gothic_prepass.wesl"));
        // `GR_NOWIND=1` keeps Bevy's own vertex stage (wind off, for A/B comparisons).
        if std::env::var_os("GR_NOWIND").is_none() {
            let _ = GOTHIC_VS.set(load_embedded_asset!(app, "shaders/gothic_vertex.wesl"));
        }
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
    /// Texture-coordinate animation of the diffuse (`d`) and normal (`n`) samplers, see
    /// `gothic_fx::matadmin::ShaderAnim`: scroll = velocity xy + offset zw, rot = rate, base
    /// angle, centre xy. All zero for static materials.
    pub uv_d_scroll: Vec4,
    pub uv_d_rot: Vec4,
    pub uv_n_scroll: Vec4,
    pub uv_n_rot: Vec4,
    /// Wind: x mode (see [`WindMode`]), y tree height in local units, z leaf card size (m).
    pub wind_mat: Vec4,
    /// Vegetation wave: xy angular wave numbers (1/cm), zw phase rates (rad/s).
    pub veg: Vec4,
    /// xy wave amplitudes (cm), z grass response range (cm), w grass response intensity (cm).
    pub veg_amp: Vec4,
    /// Grass response focus (world metres), w = 1 when active.
    pub player: Vec4,
    /// Packed `eCSpeedTreeWind_PS` parameters, see [`pack_speedtree_wind`].
    pub wind: [Vec4; 7],
}

/// Wind mode of a material (`GothicParams::wind_mat.x`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindMode {
    None = 0,
    /// Ground cover: two travelling waves and the grass response (`ge_vegetation.fx`).
    Vegetation = 1,
    /// SpeedTree bark.
    Tree = 2,
    /// SpeedTree leaf cards: sway plus leaf rocking and rustling.
    TreeLeaves = 3,
}

/// Pack the SpeedTree wind parameters for the vertex shader (layout documented in
/// `shaders/gothic_vertex.wesl`).
pub fn pack_speedtree_wind(w: &gothic_environment::wind::SpeedTreeWind) -> [Vec4; 7] {
    let yaw = w.direction[0].atan2(w.direction[2]);
    [
        Vec4::new(
            w.strength,
            w.gust_period(),
            w.gust_duration[0],
            w.gust_duration[1],
        ),
        Vec4::new(
            w.gust_strength[0],
            w.gust_strength[1],
            w.max_bend_angle.to_radians(),
            w.strength_adjustment_exponent,
        ),
        Vec4::new(
            w.bend_low[0],
            w.bend_high[0],
            w.vibration_angle(0.0),
            w.vibration_angle(1.0),
        ),
        Vec4::new(
            w.vibration_frequency[0],
            w.vibration_frequency[1],
            yaw,
            w.leaf_strength_exponent,
        ),
        Vec4::new(
            w.leaf_rocking[0],
            w.leaf_rocking_high[0],
            w.leaf_rocking_angles[0],
            w.leaf_rocking_angles[1],
        ),
        Vec4::new(
            w.leaf_rustling[0],
            w.leaf_rustling_high[0],
            w.leaf_rustling_angles[0],
            w.leaf_rustling_angles[1],
        ),
        Vec4::new(
            w.leaf_rocking_frequency[0],
            w.leaf_rocking_frequency[1],
            w.leaf_rustling_frequency[0],
            w.leaf_rustling_frequency[1],
        ),
    ]
}

/// World wind constants shared by every wind material: the SpeedTree packet and the vegetation
/// wave / grass response constants.
#[derive(Clone, Copy)]
pub struct WindEnv {
    pub wind: [Vec4; 7],
    pub veg: Vec4,
    pub veg_amp: Vec4,
}

impl WindEnv {
    pub fn new(tree: &gothic_environment::wind::SpeedTreeWind) -> Self {
        use gothic_environment::wind::{
            GRASS_RESPONSE_INTENSITY, GRASS_RESPONSE_RANGE, VegetationWind,
        };
        let v = VegetationWind::original();
        let mut intensity = GRASS_RESPONSE_INTENSITY;
        let mut range = GRASS_RESPONSE_RANGE;
        if std::env::var_os("GR_NOGRASSRESP").is_some() {
            intensity = 0.0;
            range = 1.0;
        }
        Self {
            wind: pack_speedtree_wind(tree),
            veg: Vec4::new(
                v.wave_number[0],
                v.wave_number[1],
                v.phase_rate[0],
                v.phase_rate[1],
            ),
            veg_amp: Vec4::new(v.amplitude[0], v.amplitude[1], range, intensity),
        }
    }
}

fn wind_env_cell() -> &'static std::sync::RwLock<WindEnv> {
    static W: OnceLock<std::sync::RwLock<WindEnv>> = OnceLock::new();
    W.get_or_init(|| {
        std::sync::RwLock::new(WindEnv::new(
            &gothic_environment::wind::SpeedTreeWind::default(),
        ))
    })
}

/// The wind constants new materials start with.
pub fn wind_env() -> WindEnv {
    *wind_env_cell()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Install the world's wind parameters (the `sysdyn` entity) and write them into the registered
/// wind materials.
pub fn set_wind_env(env: WindEnv, materials: &mut Assets<GothicMaterial>) {
    *wind_env_cell()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = env;
    for id in wind_materials() {
        if let Some(mut m) = materials.get_mut(id) {
            m.params.wind = env.wind;
            m.params.veg = env.veg;
            m.params.veg_amp = env.veg_amp;
        }
    }
}

fn wind_registry() -> &'static std::sync::Mutex<Vec<AssetId<GothicMaterial>>> {
    static R: OnceLock<std::sync::Mutex<Vec<AssetId<GothicMaterial>>>> = OnceLock::new();
    R.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// Remember a material that carries wind so the wind systems can reach it without touching the
/// other materials (mutating an asset re-prepares its bind group).
pub fn register_wind_material(id: AssetId<GothicMaterial>) {
    wind_registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(id);
}

/// Snapshot of the registered wind materials.
pub fn wind_materials() -> Vec<AssetId<GothicMaterial>> {
    wind_registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Material admin: copy the animated sampler parameters of a material graph into the uniforms
/// (`eCShaderMaterialAdmin::Process` refreshes the same per-material transforms each frame; the
/// shader evaluates them from the global time instead).
pub fn set_uv_anim(params: &mut GothicParams, anim: &[gothic_formats::xshmat::graph::UvAnim; 2]) {
    use gothic_fx::matadmin::shader_anim;
    if let Some(a) = shader_anim(&anim[0]) {
        params.uv_d_scroll = Vec4::from_array(a.scroll);
        params.uv_d_rot = Vec4::from_array(a.rotate);
    }
    if let Some(a) = shader_anim(&anim[1]) {
        params.uv_n_scroll = Vec4::from_array(a.scroll);
        params.uv_n_rot = Vec4::from_array(a.rotate);
    }
}

/// Original-game light set (gamma-space values, see `docs/research/ORIGINAL_SHADING.md`).
///
/// The environment system (`walk/env.rs`) replaces the value with the day-time dependent packet of
/// the original weather data; materials created later pick up the current one. `sh[0].w` carries
/// the shadow density (1 = full strength).
pub fn light_env() -> (Vec4, [Vec4; 4]) {
    *light_env_cell()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn light_env_cell() -> &'static std::sync::RwLock<(Vec4, [Vec4; 4])> {
    static ENV: OnceLock<std::sync::RwLock<(Vec4, [Vec4; 4])>> = OnceLock::new();
    ENV.get_or_init(|| std::sync::RwLock::new(light_env_raw()))
}

fn char_lights_cell() -> &'static std::sync::RwLock<([Vec4; CHAR_LIGHTS], [Vec4; CHAR_LIGHTS])> {
    static L: OnceLock<std::sync::RwLock<([Vec4; CHAR_LIGHTS], [Vec4; CHAR_LIGHTS])>> =
        OnceLock::new();
    L.get_or_init(|| std::sync::RwLock::new(([Vec4::ZERO; CHAR_LIGHTS], [Vec4::ZERO; CHAR_LIGHTS])))
}

/// The static light set currently used for new character materials.
pub fn char_lights() -> ([Vec4; CHAR_LIGHTS], [Vec4; CHAR_LIGHTS]) {
    *char_lights_cell()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Write the static light set into every character material.
pub fn push_char_lights(
    chars: &mut Assets<CharMaterial>,
    lights: [Vec4; CHAR_LIGHTS],
    mut cols: [Vec4; CHAR_LIGHTS],
) {
    if std::env::var_os("GR_NOCLIGHTS").is_some() {
        cols = [Vec4::ZERO; CHAR_LIGHTS];
    }
    *char_lights_cell()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = (lights, cols);
    for (_, m) in chars.iter_mut() {
        m.params.lights = lights;
        m.params.light_cols = cols;
    }
}

/// Install a new light set for future materials.
pub fn set_light_env(sun: Vec4, sh: [Vec4; 4]) {
    *light_env_cell()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = (sun, sh);
}

/// Write the current light set into every existing material.
pub fn push_light_env(gothic: &mut Assets<GothicMaterial>, chars: &mut Assets<CharMaterial>) {
    let (sun, sh) = light_env();
    for (_, m) in gothic.iter_mut() {
        m.params.sun = sun;
        m.params.sh = sh;
    }
    for (_, m) in chars.iter_mut() {
        m.params.sun = sun;
        m.params.sh = sh;
    }
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
            sh0.extend(1.0),
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
            uv_d_scroll: Vec4::ZERO,
            uv_d_rot: Vec4::ZERO,
            uv_n_scroll: Vec4::ZERO,
            uv_n_rot: Vec4::ZERO,
            wind_mat: Vec4::ZERO,
            veg: wind_env().veg,
            veg_amp: wind_env().veg_amp,
            player: Vec4::ZERO,
            wind: wind_env().wind,
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
    fn vertex_shader() -> ShaderRef {
        GOTHIC_VS
            .get()
            .map_or(ShaderRef::Default, |h| ShaderRef::Handle(h.clone()))
    }
    // The custom shadow/depth prepass still uses a static vertex path.
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
    pub cloud: Vec4,
    /// Moon: xyz direction to the moon, w angular radius (`MoonSize`).
    pub moon: Vec4,
    /// Moon colour (`MoonColor`), w = visibility of the star field 0..1.
    pub moon_color: Vec4,
    /// x: sun billboard half extent (`SunSize`), y: star count / 2000 density scale.
    pub sun_misc: Vec4,
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
    /// Static point lights near the camera (`eCStaticPointLight_PS`): xyz position in metres,
    /// w = 1 / range^2. Colours (x2 like the original packet) in `light_cols`; zero = unused.
    pub lights: [Vec4; CHAR_LIGHTS],
    pub light_cols: [Vec4; CHAR_LIGHTS],
}

/// Static lights evaluated per character fragment.
pub const CHAR_LIGHTS: usize = 3;

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
                        lights: char_lights().0,
                        light_cols: char_lights().1,
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
