//! Cheap lit material for Quest (see shaders/quest.wesl).
use bevy::{
    asset::{embedded_asset, load_embedded_asset},
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError},
    shader::{Shader, ShaderRef},
};
use gothic_xr_quest::knob_f32;
use std::sync::OnceLock;

static QUEST_FS: OnceLock<Handle<Shader>> = OnceLock::new();

pub struct QuestMaterialPlugin;

impl Plugin for QuestMaterialPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/quest.wesl");
        let _ = QUEST_FS.set(load_embedded_asset!(app, "shaders/quest.wesl"));
        app.add_plugins(MaterialPlugin::<QuestMaterial>::default());
    }
}

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct QParams {
    pub tint: Vec4,
    pub uv_scale: Vec4,
    pub uv_scale_b: Vec4,
    pub tint_b: Vec4,
    pub sun_dir: Vec4,
    pub sun_col: Vec4,
    pub sh: [Vec4; 4],
    pub fog_col: Vec4,
    pub misc: Vec4,
    pub uv_scale_d: Vec4,
    pub flags: Vec4,
}

/// Sun rotation of the walkable prototype (apps/game walk.rs SUN_ROT).
pub const SUN_ROT: (f32, f32) = (0.9, -0.85);

/// Original-game light set with the calibrated constants of `apps/game/src/walk/gmat.rs::light_env`
/// (weather keyframes are not available on the headset).
pub fn light_env() -> (Vec3, Vec3, [Vec4; 4]) {
    let sun_i = knob_f32("sun", 1.9);
    let amb_i = knob_f32("amb", 1.3);
    let sun_col = Vec3::new(1.0, 0.92, 0.78) * sun_i;
    let sh0 = Vec3::new(0.30, 0.35, 0.42) * amb_i;
    let sh_y = Vec3::new(0.12, 0.14, 0.20) * amb_i;
    let sh_x = Vec3::new(0.03, 0.02, 0.0) * amb_i;
    let sun_dir = Quat::from_euler(EulerRot::YXZ, SUN_ROT.0, SUN_ROT.1, 0.0) * Vec3::Z;
    let back = Vec3::new(1.0, 0.92, 0.80) * knob_f32("back", 0.75);
    (
        sun_col,
        sun_dir,
        [
            sh0.extend(1.0),
            (sh_x - back * sun_dir.x).extend(0.0),
            (sh_y - back * sun_dir.y).extend(0.0),
            (-back * sun_dir.z).extend(0.0),
        ],
    )
}

/// Haze/fog colour in gamma space (also the clear colour).
pub fn fog_color() -> Vec3 {
    Vec3::new(0.55, 0.64, 0.76)
}

impl QParams {
    pub fn new(tint: Vec3, uv_scale: Vec2, cutoff: f32) -> Self {
        let (sun_col, sun_dir, sh) = light_env();
        Self {
            tint: tint.extend(1.0),
            uv_scale: Vec4::new(uv_scale.x, uv_scale.y, 0.0, 0.0),
            uv_scale_b: Vec4::new(1.0, 1.0, 0.0, 0.0),
            tint_b: Vec4::ONE,
            sun_dir: sun_dir.extend(0.0),
            sun_col: sun_col.extend(0.0),
            sh,
            fog_col: fog_color().extend(1.0),
            misc: Vec4::new(cutoff, 0.0, knob_f32("fog_start", 120.0), knob_f32("fog_end", 420.0)),
            uv_scale_d: Vec4::ONE,
            flags: Vec4::ZERO,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct QKey {
    two_sided: bool,
}

impl From<&QuestMaterial> for QKey {
    fn from(m: &QuestMaterial) -> Self {
        Self { two_sided: m.two_sided }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[bind_group_data(QKey)]
pub struct QuestMaterial {
    #[uniform(0)]
    pub params: QParams,
    #[texture(1)]
    #[sampler(2)]
    pub tex: Option<Handle<Image>>,
    #[texture(3)]
    pub tex_b: Option<Handle<Image>>,
    #[texture(4)]
    pub detail_a: Option<Handle<Image>>,
    #[texture(5)]
    pub detail_b: Option<Handle<Image>>,
    pub two_sided: bool,
}

impl Material for QuestMaterial {
    fn fragment_shader() -> ShaderRef {
        QUEST_FS.get().map_or(ShaderRef::Default, |h| ShaderRef::Handle(h.clone()))
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
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = if key.bind_group_data.two_sided { None } else { Some(Face::Back) };
        Ok(())
    }
}
