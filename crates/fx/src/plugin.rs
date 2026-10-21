//! Bevy plugin: loads the effect data on a worker thread, instantiates the world's effect sites
//! near the camera, plays effect command sequences (`SpawnEffect` messages and `gCEffect_PS`
//! entities) and draws every particle through one dynamic mesh per texture/blend batch.
//!
//! Space: entity transforms are Bevy metres in the mirrored, origin-relative world (the space
//! the world loader builds in); simulation and billboards work in the same space in
//! centimetres (`CM` converts).
use crate::draw::{Quad, View, emitter_quads};
use crate::effect::EffectPlayer;
use crate::efm::{CommandKind, EffectMap, Link};
use crate::params::{DrawStyle, EmitterParams};
use crate::rng::Rng;
use crate::sim::Emitter;
use crate::space::{to_view, to_view_local};
use crate::templates::{EmitterSet, FxTemplates, TemplateEmitter};
use crate::world::{ScanArea, WorldFx, scan};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::{
    asset::{RenderAssetUsages, embedded_asset, load_embedded_asset},
    camera::visibility::NoFrustumCulling,
    mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology, VertexAttributeValues},
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{
        AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
        TextureDimension, TextureFormat,
    },
    shader::{Shader, ShaderRef},
};
use gothic_formats::vfs::VirtualFileSystem;
use gothic_formats::ximg::Ximg;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::Instant;

/// Centimetres to Bevy metres.
const CM: f32 = 0.01;
/// Sites activate inside this distance of the camera and deactivate beyond the second one.
const ACTIVATE_M: f32 = 80.0;
const DEACTIVATE_M: f32 = 100.0;
/// An emitter behind the camera (or farther than this) is paused; after `UNSEEN_RESET_S` seconds
/// unseen it drops its particles and starts afresh when it comes back
/// (`eCParticle_PS::SecondsBeforeInactive`, `DoProcessV1` Engine:0x1014fcc0).
const VISIBLE_M: f32 = 70.0;
const UNSEEN_RESET_S: f32 = 3.0;

static PARTICLE_FS: OnceLock<Handle<Shader>> = OnceLock::new();

/// Start an effect of the effect map at a place (the engine's `gCEffectSystem` start).
#[derive(Message, Clone, Debug)]
pub struct SpawnEffect {
    /// Effect map name (`eff_magic_fireball_impact_01`).
    pub name: String,
    /// Position in Bevy metres.
    pub position: Vec3,
    pub rotation: Quat,
    /// Follow this entity (the effect becomes its child; `position` is then local).
    pub follow: Option<Entity>,
    /// A permanent effect never finishes by itself; stop it with [`StopEffects`].
    pub permanent: bool,
    /// Groups effects so they can be stopped together (a caster, a missile).
    pub key: Option<u64>,
}

/// Stop every effect started with this key: auto-kill emitters stop emitting and fade out.
#[derive(Message, Clone, Copy, Debug)]
pub struct StopEffects {
    pub key: u64,
}

/// Group key of a dynamic effect owner.
#[derive(Component, Clone, Copy)]
struct FxGroup(u64);

/// Plugin configuration.
pub struct FxPlugin {
    pub data_dir: PathBuf,
    /// World origin of the mirrored space (engine cm).
    pub origin: Vec3,
    /// Effect sites within this engine distance (cm) of the origin are loaded.
    pub radius: f32,
}

/// Counters for the smoke report.
#[derive(Resource, Default, Debug, Clone)]
pub struct FxStats {
    pub library_ms: u64,
    pub templates: usize,
    pub effects: usize,
    pub sites: usize,
    pub active_sites: usize,
    pub emitters: usize,
    pub particles: usize,
    pub batches: usize,
    /// CPU time of the last mesh build (ms).
    pub frame_ms: f32,
    /// CPU time of the last emitter simulation step (ms).
    pub update_ms: f32,
    pub spawned: u32,
}

struct TexRaw {
    width: u32,
    height: u32,
    fourcc: [u8; 4],
    mips: Vec<Vec<u8>>,
}

struct Library {
    templates: FxTemplates,
    effects: EffectMap,
    textures: HashMap<String, TexRaw>,
    world: WorldFx,
    millis: u64,
}

#[derive(Resource)]
struct Loading(Mutex<mpsc::Receiver<Library>>);

#[derive(Resource)]
struct Ready(Arc<Library>);

#[derive(Resource)]
struct Origin(Vec3);

#[derive(Resource, Default)]
struct Textures(HashMap<String, Handle<Image>>);

#[derive(Resource, Default)]
struct Batches {
    materials: HashMap<BatchKey, Handle<FxMaterial>>,
    entities: HashMap<BatchKey, (Entity, Handle<Mesh>)>,
}

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/particle.wesl");
        let _ = PARTICLE_FS.set(load_embedded_asset!(app, "shaders/particle.wesl"));
        let (tx, rx) = mpsc::channel();
        let (dir, origin, radius) = (self.data_dir.clone(), self.origin, self.radius);
        let _ = std::thread::Builder::new()
            .name("fx-library".into())
            .spawn(move || {
                if let Some(lib) = load_library(&dir, origin, radius) {
                    let _ = tx.send(lib);
                }
            });
        app.add_plugins(MaterialPlugin::<FxMaterial>::default())
            .add_message::<SpawnEffect>()
            .add_message::<StopEffects>()
            .insert_resource(Loading(Mutex::new(rx)))
            .insert_resource(Origin(self.origin))
            .init_resource::<Textures>()
            .init_resource::<Batches>()
            .init_resource::<FxStats>()
            .add_systems(
                Update,
                (poll_library, site_activation, run_effects)
                    .chain()
                    .run_if(|r: Option<Res<Ready>>| r.is_some()),
            )
            .add_systems(
                PostUpdate,
                (update_emitters, build_batches)
                    .chain()
                    .after(bevy::transform::TransformSystems::Propagate)
                    .run_if(|r: Option<Res<Ready>>| r.is_some()),
            );
        // The library arrives later; `poll_library` must also run before it exists.
        app.add_systems(
            Update,
            poll_library.run_if(|r: Option<Res<Ready>>| r.is_none()),
        );
    }
}

fn load_library(dir: &std::path::Path, origin: Vec3, radius: f32) -> Option<Library> {
    let t = Instant::now();
    let mut vfs = VirtualFileSystem::open_data_dir(dir).ok()?;
    let templates = FxTemplates::load(&mut vfs).ok()?;
    let effects = vfs
        .resolve_name("g3.efm")
        .and_then(|s| vfs.read(s).ok())
        .and_then(|b| EffectMap::parse(&b).ok())
        .unwrap_or_default();
    let world = scan(
        &mut vfs,
        &ScanArea {
            center: origin,
            radius,
            // The sector documents of Ardea plus the global dynamic system and the bird layers.
            document: &|p| p.contains("ardea") || p.contains("sysdyn"),
        },
    );
    let mut stems: Vec<String> = Vec::new();
    let mut note = |tex: &str| {
        let stem = texture_stem(tex);
        if !stem.is_empty() && !stems.contains(&stem) {
            stems.push(stem);
        }
    };
    for e in &world.emitters {
        note(&e.params.texture);
    }
    for set in templates.iter_sets() {
        for e in &set.emitters {
            note(&e.params.texture);
        }
    }
    let mut textures = HashMap::new();
    for stem in stems {
        let key = format!("{stem}.ximg");
        let Some(slot) = vfs.resolve_name(&key) else {
            continue;
        };
        let Ok(bytes) = vfs.read(slot) else { continue };
        let Ok(img) = Ximg::parse(&bytes) else {
            continue;
        };
        textures.insert(
            stem,
            TexRaw {
                width: img.width,
                height: img.height,
                fourcc: img.fourcc,
                mips: img.mips().iter().map(|m| m.to_vec()).collect(),
            },
        );
    }
    Some(Library {
        templates,
        effects,
        textures,
        world,
        millis: t.elapsed().as_millis() as u64,
    })
}

/// `VFX_Env_Torch_01.tga` -> `vfx_env_torch_01`; empty for materials (`.xshmat`, distortion).
fn texture_stem(tex: &str) -> String {
    let lower = tex.replace('\\', "/").to_ascii_lowercase();
    if lower.ends_with(".xshmat") {
        return String::new();
    }
    let file = lower.rsplit('/').next().unwrap_or(&lower);
    file.rsplit_once('.').map_or(file, |(s, _)| s).to_owned()
}

// ---------------------------------------------------------------------------------------------
// Material
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct FxParams {
    /// x alpha reference (0 = none), y fog enabled, z additive blend.
    pub misc: Vec4,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct FxMatKey {
    depth_write: bool,
}

impl From<&FxMaterial> for FxMatKey {
    fn from(m: &FxMaterial) -> Self {
        Self {
            depth_write: m.depth_write,
        }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[bind_group_data(FxMatKey)]
pub struct FxMaterial {
    #[uniform(0)]
    pub params: FxParams,
    #[texture(1)]
    #[sampler(2)]
    pub texture: Option<Handle<Image>>,
    pub alpha_mode: AlphaMode,
    pub depth_write: bool,
}

impl Material for FxMaterial {
    fn fragment_shader() -> ShaderRef {
        PARTICLE_FS
            .get()
            .map_or(ShaderRef::Default, |h| ShaderRef::Handle(h.clone()))
    }
    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
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
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(key.bind_group_data.depth_write);
        }
        Ok(())
    }
}

/// Everything that makes two emitters share a draw call.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct BatchKey {
    texture: String,
    style: u8,
    alpha_ref: u8,
    fog: bool,
    depth_write: bool,
}

impl BatchKey {
    fn of(p: &EmitterParams) -> Option<Self> {
        let texture = texture_stem(&p.texture);
        if texture.is_empty() || p.draw_style == DrawStyle::Invisible {
            return None;
        }
        Some(Self {
            texture,
            style: p.draw_style as u8,
            alpha_ref: if p.alpha_test {
                p.alpha_reference.max(1)
            } else {
                0
            },
            fog: !p.disable_fogging,
            depth_write: p.depth_write,
        })
    }

    fn alpha_mode(&self) -> AlphaMode {
        match self.style {
            6 => AlphaMode::Add,
            2 | 5 => AlphaMode::Multiply,
            _ => AlphaMode::Blend,
        }
    }
}

fn texture_image(t: &TexRaw) -> Option<Image> {
    let format = match &t.fourcc {
        b"DXT1" => TextureFormat::Bc1RgbaUnormSrgb,
        b"DXT3" => TextureFormat::Bc2RgbaUnormSrgb,
        b"DXT5" => TextureFormat::Bc3RgbaUnormSrgb,
        _ => return None,
    };
    if !t.width.is_multiple_of(4) || !t.height.is_multiple_of(4) {
        return None;
    }
    let data: Vec<u8> = t.mips.iter().flatten().copied().collect();
    let mut image = Image::new(
        Extent3d {
            width: t.width,
            height: t.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = t.mips.len() as u32;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    Some(image)
}

// ---------------------------------------------------------------------------------------------
// Components and systems
// ---------------------------------------------------------------------------------------------

/// A running emitter entity (its `GlobalTransform` is the emitter frame).
#[derive(Component)]
pub struct FxEmitter {
    emitter: Emitter,
    key: BatchKey,
    /// Seconds the emitter has been out of sight (`SecondsBeforeInactive`).
    unseen: f32,
}

/// An emitter that outlives its effect owner's hierarchy but not the owner entity itself.
#[derive(Component)]
struct FxOwner(Entity);

/// A static world site: instantiated when the camera comes close, removed when it leaves.
#[derive(Component)]
enum FxSite {
    Emitter(Arc<EmitterParams>),
    Effect {
        name: String,
        permanent: bool,
        /// Offset in the entity frame (mirrored cm).
        offset: Vec3,
    },
}

/// Marks a site that currently has runtime state.
#[derive(Component)]
struct FxActive;

/// Runtime of an effect: the command player and the entities its commands spawned.
#[derive(Component)]
struct FxEffect {
    player: EffectPlayer,
    spawned: Vec<(Entity, bool)>,
    /// A dynamic effect is despawned when finished; a site's runtime is only dropped.
    owner_dies: bool,
    seed: u64,
}

fn poll_library(
    loading: Option<Res<Loading>>,
    ready: Option<Res<Ready>>,
    origin: Res<Origin>,
    mut commands: Commands,
    mut stats: ResMut<FxStats>,
) {
    if ready.is_some() {
        return;
    }
    let Some(loading) = loading else { return };
    let Ok(lib) = loading.0.lock().map(|r| r.try_recv()) else {
        return;
    };
    let Ok(lib) = lib else { return };
    stats.library_ms = lib.millis;
    stats.templates = lib.templates.len();
    stats.effects = lib.effects.len();
    let mut sites = 0;
    for e in &lib.world.emitters {
        let m = to_view(&e.matrix, origin.0);
        commands.spawn((
            FxSite::Emitter(Arc::new(e.params.mirrored_x())),
            site_transform(&m),
            Visibility::default(),
            Name::new(e.name.clone()),
        ));
        sites += 1;
        if std::env::var_os("GR_FX_DEBUG").is_some() {
            info!(
                "fx site emitter {} at {:?} m",
                e.name,
                m.w_axis.truncate() * CM
            );
        }
    }
    for e in &lib.world.effects {
        if e.probability < 1.0
            && (hash(&e.name, e.matrix.w_axis) % 1000) as f32 / 1000.0 > e.probability
        {
            continue;
        }
        let m = to_view(&e.matrix, origin.0);
        commands.spawn((
            FxSite::Effect {
                name: e.effect.clone(),
                permanent: e.is_static,
                offset: Vec3::new(-e.offset.x, e.offset.y, e.offset.z),
            },
            site_transform(&m),
            Visibility::default(),
            Name::new(e.name.clone()),
        ));
        sites += 1;
        if std::env::var_os("GR_FX_DEBUG").is_some() {
            info!(
                "fx site effect {} {} at {:?} m",
                e.name,
                e.effect,
                m.w_axis.truncate() * CM
            );
        }
    }
    stats.sites = sites;
    commands.insert_resource(Ready(Arc::new(lib)));
    commands.remove_resource::<Loading>();
}

fn hash(name: &str, at: Vec4) -> u64 {
    let mut h: u64 = 1469598103934665603;
    for b in name
        .bytes()
        .chain(at.x.to_bits().to_le_bytes())
        .chain(at.z.to_bits().to_le_bytes())
    {
        h = (h ^ b as u64).wrapping_mul(1099511628211);
    }
    h
}

/// A view-space cm matrix as a Bevy transform in metres.
fn site_transform(m: &Mat4) -> Transform {
    let (_, rot, pos) = m.to_scale_rotation_translation();
    Transform::from_translation(pos * CM).with_rotation(rot)
}

fn camera_position(
    cams: &Query<(&Camera, &GlobalTransform), With<Camera3d>>,
) -> Option<GlobalTransform> {
    cams.iter().find(|(c, _)| c.is_active).map(|(_, g)| *g)
}

/// The half-FOV tangents of the active perspective camera, widened by a margin.
fn frustum_tangents(
    cams: &Query<(&Camera, &GlobalTransform, &Projection), With<Camera3d>>,
) -> Option<(GlobalTransform, Option<(f32, f32)>)> {
    let (_, g, p) = cams.iter().find(|(c, _, _)| c.is_active)?;
    let tan = match p {
        Projection::Perspective(p) => {
            let ty = (p.fov * 0.5).tan() * 1.1;
            Some((ty * p.aspect_ratio, ty))
        }
        _ => None,
    };
    Some((*g, tan))
}

#[allow(clippy::too_many_arguments)]
fn site_activation(
    mut commands: Commands,
    cams: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    sites: Query<(Entity, &FxSite, &GlobalTransform, Has<FxActive>)>,
    lib: Res<Ready>,
    mut stats: ResMut<FxStats>,
    effects: Query<&FxEffect>,
) {
    let Some(cam) = camera_position(&cams) else {
        return;
    };
    let eye = cam.translation();
    let mut active = 0;
    for (entity, site, gt, is_active) in &sites {
        let d = gt.translation().distance(eye);
        if is_active {
            if d > DEACTIVATE_M {
                // Drop the runtime: the children (attached emitters) go with it, independent
                // emitters are despawned explicitly.
                if let Ok(fx) = effects.get(entity) {
                    for (e, _) in &fx.spawned {
                        if let Ok(mut c) = commands.get_entity(*e) {
                            c.despawn();
                        }
                    }
                }
                let mut c = commands.entity(entity);
                c.remove::<(FxActive, FxEffect, FxEmitter)>();
                c.despawn_related::<Children>();
            } else {
                active += 1;
            }
            continue;
        }
        if d > ACTIVATE_M {
            continue;
        }
        active += 1;
        let seed = hash(
            "site",
            Vec4::new(gt.translation().x, 0.0, gt.translation().z, 0.0),
        );
        match site {
            FxSite::Emitter(params) => {
                if let Some(key) = BatchKey::of(params) {
                    commands.entity(entity).insert((
                        FxActive,
                        FxEmitter {
                            emitter: Emitter::new(params.clone(), seed),
                            key,
                            unseen: 0.0,
                        },
                    ));
                }
            }
            FxSite::Effect {
                name, permanent, ..
            } => {
                let Some(seq) = lib.0.effects.lookup(name) else {
                    continue;
                };
                commands.entity(entity).insert((
                    FxActive,
                    FxEffect {
                        player: EffectPlayer::start(Arc::new(seq.clone()), *permanent),
                        spawned: Vec::new(),
                        owner_dies: false,
                        seed,
                    },
                ));
            }
        }
    }
    stats.active_sites = active;
}

/// Where and how the emitters of one template appear.
struct Placement<'a> {
    owner: Entity,
    owner_gt: &'a GlobalTransform,
    offset_cm: Vec3,
    link: Link,
    auto_kill: bool,
}

/// Spawn the emitters of one template as children of / next to the owner.
fn spawn_set(
    commands: &mut Commands,
    set: &EmitterSet,
    at: &Placement,
    rng: &mut Rng,
    spawned: &mut Vec<(Entity, bool)>,
) {
    for TemplateEmitter { params, local, .. } in &set.emitters {
        let Some(key) = BatchKey::of(params) else {
            continue;
        };
        let local = to_view_local(local);
        let (_, lrot, lpos) = local.to_scale_rotation_translation();
        let local_tf = Transform::from_translation((lpos + at.offset_cm) * CM).with_rotation(lrot);
        let emitter = Emitter::new(Arc::new(params.mirrored_x()), rng.next_u64());
        let bundle = (
            FxEmitter {
                emitter,
                key,
                unseen: 0.0,
            },
            Visibility::default(),
        );
        let entity = match at.link {
            Link::Independent => {
                let world = at.owner_gt.compute_transform() * local_tf;
                commands.spawn((bundle, world, FxOwner(at.owner))).id()
            }
            Link::TargetEntity | Link::TargetBone => {
                commands.spawn((bundle, local_tf, ChildOf(at.owner))).id()
            }
        };
        spawned.push((entity, at.auto_kill));
    }
}

/// Effect owners: the entity, its frame, the runtime, the site it came from and its group.
type OwnerData = (
    Entity,
    &'static GlobalTransform,
    &'static mut FxEffect,
    Option<&'static FxSite>,
    Option<&'static FxGroup>,
);

#[allow(clippy::too_many_arguments)]
fn run_effects(
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<Ready>,
    mut requests: MessageReader<SpawnEffect>,
    mut stops: MessageReader<StopEffects>,
    mut owners: Query<OwnerData>,
    mut emitters: Query<(&mut FxEmitter, &GlobalTransform), Without<FxEffect>>,
    mut stats: ResMut<FxStats>,
) {
    for r in requests.read() {
        let Some(seq) = lib.0.effects.lookup(&r.name) else {
            continue;
        };
        let tf = Transform::from_translation(r.position).with_rotation(r.rotation);
        let mut e = commands.spawn((
            tf,
            Visibility::default(),
            FxGroup(r.key.unwrap_or(0)),
            FxEffect {
                player: EffectPlayer::start(Arc::new(seq.clone()), r.permanent),
                spawned: Vec::new(),
                owner_dies: true,
                seed: hash(&r.name, Vec4::new(r.position.x, 0.0, r.position.z, 0.0)),
            },
        ));
        if let Some(f) = r.follow {
            e.insert(ChildOf(f));
        }
        stats.spawned += 1;
    }
    let dt = time.delta_secs();
    let stopped: Vec<u64> = stops.read().map(|s| s.key).collect();
    for (entity, gt, mut fx, site, key) in &mut owners {
        let stop = key.is_some_and(|k| k.0 != 0 && stopped.contains(&k.0));
        let due: Vec<_> = fx.player.process(dt).into_iter().cloned().collect();
        let site_offset = match site {
            Some(FxSite::Effect { offset, .. }) => *offset,
            _ => Vec3::ZERO,
        };
        let mut rng = Rng::new(fx.seed);
        let mut spawned = std::mem::take(&mut fx.spawned);
        for c in due {
            let CommandKind::SpawnEntity {
                template: Some(guid),
                offset,
                link,
                auto_kill,
                ..
            } = &c.kind
            else {
                continue;
            };
            let Some(set) = lib.0.templates.by_guid(guid) else {
                continue;
            };
            let off = Vec3::new(
                -(offset[0][0] + (offset[1][0] - offset[0][0]) * rng.unit()),
                offset[0][1] + (offset[1][1] - offset[0][1]) * rng.unit(),
                offset[0][2] + (offset[1][2] - offset[0][2]) * rng.unit(),
            );
            let at = Placement {
                owner: entity,
                owner_gt: gt,
                offset_cm: site_offset + off,
                link: *link,
                auto_kill: *auto_kill,
            };
            spawn_set(&mut commands, set, &at, &mut rng, &mut spawned);
            fx.seed = rng.next_u64();
        }
        fx.spawned = spawned;
        if fx.player.has_finished() || stop {
            // Auto-kill emitters stop emitting (their particles run out); the rest live on.
            for (e, auto) in &fx.spawned {
                // An explicit stop (`StopEffects`: the spell in the hand is put away) ends
                // every emitter of the effect, looping glow included.
                if (*auto || stop)
                    && let Ok((mut em, _)) = emitters.get_mut(*e)
                {
                    em.emitter.stop();
                }
            }
            if fx.owner_dies {
                // Spawnees that outlive the owner keep their place in the world.
                for (e, _) in &fx.spawned {
                    if let Ok((_, egt)) = emitters.get(*e) {
                        commands
                            .entity(*e)
                            .remove::<ChildOf>()
                            .insert(egt.compute_transform());
                    }
                }
                commands.entity(entity).despawn();
            } else {
                commands.entity(entity).remove::<FxEffect>();
            }
        }
    }
}

fn update_emitters(
    mut commands: Commands,
    time: Res<Time>,
    cams: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut q: Query<(Entity, &GlobalTransform, &mut FxEmitter, Option<&FxOwner>)>,
    all: Query<()>,
    mut stats: ResMut<FxStats>,
) {
    let t0 = Instant::now();
    let dt = time.delta_secs().min(0.25);
    let cam = camera_position(&cams).map(|c| (c.translation(), c.rotation() * Vec3::NEG_Z));
    for (entity, gt, mut fx, owner) in &mut q {
        if let Some((eye, forward)) = &cam {
            let to = gt.translation() - *eye;
            let d = to.length();
            // Out of range, or behind the eye farther than a few metres: nobody sees it.
            let seen = d < 4.0 || (d < VISIBLE_M && to.dot(*forward) > -0.25 * d);
            if !seen {
                fx.unseen += dt;
                if fx.unseen > UNSEEN_RESET_S {
                    fx.emitter.reset();
                }
                continue;
            }
            fx.unseen = 0.0;
        }
        // An emitter whose owner is gone runs out instead of emitting forever.
        if owner.is_some_and(|o| !all.contains(o.0)) {
            fx.emitter.stop();
        }
        let mut m = gt.to_matrix();
        m.w_axis = (m.w_axis.truncate() / CM).extend(1.0);
        fx.emitter.set_transform(m);
        fx.emitter.update(dt);
        if fx.emitter.is_finished() {
            commands.entity(entity).despawn();
        }
    }
    stats.update_ms = t0.elapsed().as_secs_f32() * 1000.0;
}

/// Mesh transforms the batcher writes each frame.
type MeshFrame = (&'static mut Transform, &'static mut GlobalTransform);

#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
fn build_batches(
    mut commands: Commands,
    cams: Query<(&Camera, &GlobalTransform, &Projection), With<Camera3d>>,
    emitters: Query<&FxEmitter>,
    lib: Res<Ready>,
    mut textures: ResMut<Textures>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FxMaterial>>,
    mut batches: ResMut<Batches>,
    mut stats: ResMut<FxStats>,
    mut scratch: Local<HashMap<BatchKey, Vec<Quad>>>,
    mut visibilities: Query<&mut Visibility>,
    mut log: Local<(u32, f32, f32)>,
    mut frames: Query<MeshFrame, (With<Mesh3d>, Without<Camera3d>)>,
) {
    let t0 = Instant::now();
    let Some((cam, tan_half)) = frustum_tangents(&cams) else {
        return;
    };
    let mut view = View::new(cam.translation() / CM, cam.rotation());
    view.tan_half = tan_half;
    for v in scratch.values_mut() {
        v.clear();
    }
    let (mut n_emitters, mut n_particles) = (0, 0);
    for fx in &emitters {
        // Paused (unseen) emitters keep their particles but draw nothing.
        if fx.unseen > 0.0 {
            continue;
        }
        n_emitters += 1;
        n_particles += fx.emitter.particles.len();
        emitter_quads(
            &fx.emitter,
            &view,
            scratch.entry(fx.key.clone()).or_default(),
        );
    }
    let mut live = 0;
    for (key, quads) in scratch.iter_mut() {
        let present = batches.entities.get(key).map(|(e, _)| *e);
        if quads.is_empty() {
            if let Some(e) = present
                && let Ok(mut v) = visibilities.get_mut(e)
            {
                *v = Visibility::Hidden;
            }
            continue;
        }
        live += 1;
        let (entity, handle) = match batches.entities.get(key) {
            Some((e, h)) => (*e, h.clone()),
            None => {
                let tex = textures.0.entry(key.texture.clone()).or_insert_with(|| {
                    lib.0
                        .textures
                        .get(&key.texture)
                        .and_then(texture_image)
                        .map(|i| images.add(i))
                        .unwrap_or_default()
                });
                let mat = batches
                    .materials
                    .entry(key.clone())
                    .or_insert_with(|| {
                        materials.add(FxMaterial {
                            params: FxParams {
                                misc: Vec4::new(
                                    key.alpha_ref as f32 / 255.0,
                                    if key.fog { 1.0 } else { 0.0 },
                                    if key.style == 6 { 1.0 } else { 0.0 },
                                    0.0,
                                ),
                            },
                            texture: Some(tex.clone()),
                            alpha_mode: key.alpha_mode(),
                            depth_write: key.depth_write,
                        })
                    })
                    .clone();
                let mesh = meshes.add(empty_mesh());
                let e = commands
                    .spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(mat),
                        Transform::IDENTITY,
                        Visibility::Visible,
                        NoFrustumCulling,
                        Name::new(format!("fx batch {}", key.texture)),
                    ))
                    .id();
                batches.entities.insert(key.clone(), (e, mesh.clone()));
                (e, mesh)
            }
        };
        // Vertices are relative to the centroid so the transparent sort key is the batch's
        // own position.
        let mut centroid = Vec3::ZERO;
        for q in quads.iter() {
            centroid += (q.corners[0] + q.corners[2]) * 0.5;
        }
        centroid /= quads.len() as f32;
        if std::env::var_os("GR_FX_LAG").is_some()
            && let Ok((_, g)) = frames.get(entity)
        {
            // Rendered frame uses the propagated (previous) transform.
            info!(
                "fxlag {} quads {} new centroid {:?} rendered {:?} error {:.4} m",
                key.texture,
                quads.len(),
                centroid * CM,
                g.translation(),
                (centroid * CM - g.translation()).length()
            );
        }
        if let Some(mut mesh) = meshes.get_mut(&handle) {
            fill_mesh(&mut mesh, quads, centroid);
        }
        if let Ok(mut v) = visibilities.get_mut(entity) {
            *v = Visibility::Visible;
        }
        // The vertices are relative to this frame's centroid, so the transform that places them
        // must be this frame's too. This system runs after transform propagation: writing only
        // `Transform` (as a deferred insert) rendered every batch with the previous frame's
        // centroid, which swings all flames of the batch by the centroid's per-frame change.
        let at = batch_transform(centroid);
        if let Ok((mut t, mut g)) = frames.get_mut(entity) {
            *t = at;
            *g = GlobalTransform::from(at);
        } else {
            commands.entity(entity).insert(at);
        }
    }
    stats.emitters = n_emitters;
    stats.particles = n_particles;
    stats.batches = live;
    stats.frame_ms = t0.elapsed().as_secs_f32() * 1000.0;
    // Running average, reported every 600 frames while anything is simulated.
    log.0 += 1;
    log.1 += stats.update_ms;
    log.2 += stats.frame_ms;
    if log.0 >= 600 {
        if n_emitters > 0 {
            let n = log.0 as f32;
            info!(
                "fx: {} emitters, {} particles, {} batches, {} active sites of {}, avg update {:.3} ms + build {:.3} ms",
                n_emitters,
                n_particles,
                live,
                stats.active_sites,
                stats.sites,
                log.1 / n,
                log.2 / n
            );
            if std::env::var_os("GR_FX_DEBUG").is_some() {
                for (key, quads) in scratch.iter() {
                    info!(
                        "fx:   {:?} style {} quads {}",
                        key.texture,
                        key.style,
                        quads.len()
                    );
                }
            }
        }
        *log = (0, 0.0, 0.0);
    }
}

/// Transform of a batch whose vertices are relative to `centroid` (cm).
fn batch_transform(centroid: Vec3) -> Transform {
    Transform::from_translation(centroid * CM)
}

pub fn empty_mesh() -> Mesh {
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new());
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, Vec::<[f32; 3]>::new());
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, Vec::<[f32; 2]>::new());
    m.insert_attribute(Mesh::ATTRIBUTE_UV_1, Vec::<[f32; 2]>::new());
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new());
    m.insert_indices(Indices::U32(Vec::new()));
    m
}

/// Rewrite the batch mesh in place: four vertices per quad, reusing the attribute buffers.
pub fn fill_mesh(mesh: &mut Mesh, quads: &[Quad], centroid: Vec3) {
    let n = quads.len() * 4;
    let corner_uv = |r: [f32; 4], i: usize| match i {
        0 => [r[0], r[1]],
        1 => [r[2], r[1]],
        2 => [r[2], r[3]],
        _ => [r[0], r[3]],
    };
    if let Some(VertexAttributeValues::Float32x3(v)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        v.clear();
        v.reserve(n);
        for q in quads {
            for c in &q.corners {
                v.push(((*c - centroid) * CM).to_array());
            }
        }
    }
    if let Some(VertexAttributeValues::Float32x3(v)) = mesh.attribute_mut(Mesh::ATTRIBUTE_NORMAL) {
        v.clear();
        v.reserve(n);
        for q in quads {
            // Unit normal whose x is the frame blend weight.
            let w = q.blend.clamp(0.0, 1.0);
            let normal = [w, (1.0 - w * w).sqrt(), 0.0];
            v.extend_from_slice(&[normal; 4]);
        }
    }
    if let Some(VertexAttributeValues::Float32x2(v)) = mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0) {
        v.clear();
        v.reserve(n);
        for q in quads {
            v.extend((0..4).map(|i| corner_uv(q.uv_a, i)));
        }
    }
    if let Some(VertexAttributeValues::Float32x2(v)) = mesh.attribute_mut(Mesh::ATTRIBUTE_UV_1) {
        v.clear();
        v.reserve(n);
        for q in quads {
            v.extend((0..4).map(|i| corner_uv(q.uv_b, i)));
        }
    }
    if let Some(VertexAttributeValues::Float32x4(v)) = mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR) {
        v.clear();
        v.reserve(n);
        for q in quads {
            v.extend_from_slice(&[q.color; 4]);
        }
    }
    // The index pattern depends only on the quad count.
    if let Some(Indices::U32(idx)) = mesh.indices_mut()
        && idx.len() != quads.len() * 6
    {
        idx.clear();
        for k in 0..quads.len() as u32 {
            let b = k * 4;
            idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_transform_is_the_mirrored_engine_frame_in_metres() {
        // Engine entity at (1000, 200, -300) cm yawed 90 degrees about +y; origin shifts x/z.
        let engine = Mat4::from_translation(Vec3::new(1000.0, 200.0, -300.0))
            * Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let origin = Vec3::new(400.0, 50.0, -100.0);
        let tf = site_transform(&to_view(&engine, origin));
        // Position: (origin.x - x, y - origin.y, z - origin.z) in metres.
        assert!((tf.translation - Vec3::new(-6.0, 1.5, -2.0)).length() < 1e-4);
        // The entity's engine +x axis (a point 100 cm along it) lands where the loader's point
        // map puts the engine-space point, so a torch's flame offset stays on the torch.
        let engine_pt = engine.transform_point3(Vec3::new(100.0, 0.0, 0.0));
        let want = crate::space::point_to_view(engine_pt, origin) * CM;
        let got = tf.transform_point(Vec3::new(-1.0, 0.0, 0.0));
        assert!((got - want).length() < 1e-4, "{got:?} vs {want:?}");
    }

    #[test]
    fn batch_transform_places_centroid_relative_vertices() {
        // A vertex stored as (corner - centroid) * CM lands on corner * CM under the transform:
        // the transform and the vertices must come from the same centroid.
        let centroid = Vec3::new(5120.0, 340.0, -870.0);
        let corner = Vec3::new(5150.0, 360.0, -860.0);
        let local = (corner - centroid) * CM;
        let world = batch_transform(centroid).transform_point(local);
        assert!((world - corner * CM).length() < 1e-5);
        let moved = batch_transform(centroid + Vec3::new(30.0, 0.0, 0.0)).translation;
        assert!(((moved - batch_transform(centroid).translation).x - 0.3).abs() < 1e-4);
    }
}
