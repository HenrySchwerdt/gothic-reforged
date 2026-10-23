//! Ground cover (grass, herbs, ferns, stones, branches: the `eCVegetation_PS` grids of the world documents) for the
//! Ardea scene on Quest.
//!
//! Data: `veg/<cx>_<cz>.gveg` per Ardea cell, written by `examples/pack_veg.rs` from the user's own game files (placement
//! records of the original grid: mesh, position, rotation, scale, colour, wind strength). The brush meshes are the cooked
//! `.gmsh` files already used by the static props.
//!
//! Runtime (nothing here is one entity per plant):
//! * The scene is split into `CHUNK_CM` squares. A chunk merges all its instances into one mesh per material
//!   (alpha-tested, no blending, one draw per material and chunk), built on the async compute pool.
//! * Distance rules come from the original: view range = `VegetationViewRange.fViewDistance_*`
//!   (`eCVegetationAdmin::SetDefaultViewRange`, 0x100afbe0; render-rules `vegetation::view_range`), dissolve start =
//!   0.65..0.75 x view range (`eCVegetation_PS::OnRender`, 0x1027e8a0), dissolve end = view range, culling sphere =
//!   view range (`eCVegetation_Grid::Render`, 0x1027c9a0). The dissolve itself runs per pixel in `veg.wesl`.
//! * Mesh LoD: the cooked LoD index ranges of the brush meshes, stage thresholds from `render-rules::lod::stage_distances`
//!   (`eCRenderContextMesh::CalculateLoDStage`, 0x101d9080), selected per chunk from the distance of its nearest point
//!   (5 % hysteresis); a chunk is rebuilt (off thread) when its stage changes and swapped in without a gap.
//! * Lifetime: a chunk is built when its nearest point is closer than `LOAD_FACTOR` x view range, hidden (no draw) beyond
//!   the view range and despawned (meshes dropped, GPU memory freed) beyond `UNLOAD_FACTOR` x view range.
//! * Frustum culling per chunk part through explicit `Aabb`s.
//!
//! Quest profile values that are NOT from the original are named constants / knobs below and listed in
//! .local/quest/HANDOFF.md with the measured numbers that justify them.
use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::{RenderAssetUsages, embedded_asset, load_embedded_asset};
use bevy::camera::primitives::Aabb;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::{Shader, ShaderRef};
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use gothic_cook::material::{KIND_WATER, MaterialRec, NONE_U32};
use gothic_cook::mesh::MeshData;
use gothic_cook::{AssetId, Cache};
use gothic_xr_quest::{XrEye, knob, knob_f32};

use crate::ardea::{Ardea, TriCount, WorldRoot};
use crate::locomotion::CM;
use crate::qmat;

// --- Quest profile (deviations from the original, see module docs) ---------------------------------------------------
/// Chunk edge (cm). Original nodes are 1000 cm (`GridNodeSize`) and are drawn one by one; batching 2.5 x 2.5 nodes per
/// draw trades culling granularity for draw calls on the Quest (2 views = 2x draws in Bevy).
pub const CHUNK_CM: f32 = 2500.0;
/// `VegetationViewRange` preset used on Quest: 3 = Low (4000 cm), the original's own lowest quality preset
/// (`VegetationViewRange.fViewDistance_Low`). `debug.gr.veg_quality` 0..3 selects VeryHigh..Low for experiments.
pub const QUEST_QUALITY: u8 = 3;
/// Dissolve start as a fraction of the view range: the original draws a random value in [0.65, 0.75]; the middle.
pub const FADE_START_FRACTION: f32 = 0.70;
/// A chunk is built once its nearest point is closer than this x view range (lead time for the async build).
pub const LOAD_FACTOR: f32 = 1.10;
/// A chunk is despawned beyond this x view range. LOAD < UNLOAD gives the hysteresis against thrashing.
pub const UNLOAD_FACTOR: f32 = 1.35;
/// Metres above the ground at which the eyes are, to find the feet for the grass response (desktop uses 165 cm).
const EYE_HEIGHT_M: f32 = 1.65;
/// Wind + grass response displacement bound (cm) added to chunk bounds: wave amplitudes 30 + 10 (`Invalidate`) and the
/// response intensity 50.
const WIND_PAD_CM: f32 = 100.0;
/// Quest deviation: fraction of instances kept in chunks beyond `FAR_START` x view range. The brush meshes of this data
/// set have no mesh LoD stages (verified: identical index counts for LoD 0..2), so this is the only geometric LoD lever.
/// 1.0 = off (original behaviour); not lowered until a device measurement justifies it (`debug.gr.veg_far_density`).
pub const FAR_DENSITY: f32 = 1.0;
pub const FAR_START: f32 = 0.5;
const MAX_BUILDS_IN_FLIGHT: usize = 2;
const REC: usize = 52;

static VEG_SHADER: std::sync::OnceLock<Handle<Shader>> = std::sync::OnceLock::new();

/// Wind / response constants shared by all vegetation materials (`eCVegetationAdmin::Invalidate`, ge3.INI).
fn wind_consts() -> (Vec4, Vec4) {
    use gothic_environment::wind::{
        GRASS_RESPONSE_INTENSITY, GRASS_RESPONSE_RANGE, VegetationWind,
    };
    let v = VegetationWind::original();
    (
        Vec4::new(
            v.wave_number[0],
            v.wave_number[1],
            v.phase_rate[0],
            v.phase_rate[1],
        ),
        Vec4::new(
            v.amplitude[0],
            v.amplitude[1],
            GRASS_RESPONSE_RANGE,
            GRASS_RESPONSE_INTENSITY,
        ),
    )
}

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct VParams {
    pub tint: Vec4,
    pub uv_scale: Vec4,
    pub sun_dir: Vec4,
    pub sun_col: Vec4,
    pub sh: [Vec4; 4],
    pub fog_col: Vec4,
    pub misc: Vec4,
    pub fog: Vec4,
    pub veg: Vec4,
    pub veg_amp: Vec4,
    pub player: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct VegMaterial {
    #[uniform(0)]
    pub params: VParams,
    #[texture(1)]
    #[sampler(2)]
    pub tex: Option<Handle<Image>>,
}

impl Material for VegMaterial {
    fn vertex_shader() -> ShaderRef {
        VEG_SHADER
            .get()
            .map_or(ShaderRef::Default, |h| ShaderRef::Handle(h.clone()))
    }
    fn fragment_shader() -> ShaderRef {
        VEG_SHADER
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
        // Foliage cards are two-sided (desktop: `two_sided = true` for every ground cover material).
        descriptor.primitive.cull_mode = None::<Face>;
        Ok(())
    }
}

// --- data ------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Inst {
    mesh: u64,
    wind: f32,
    color: u32,
    pos: Vec3,
    rot: Quat,
    scale: [f32; 2],
}

fn parse_gveg(bytes: &[u8]) -> Vec<Inst> {
    if bytes.len() < 8 || &bytes[..4] != b"GVG1" {
        return Vec::new();
    }
    let n = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let f = |b: &[u8], o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    bytes[8..]
        .chunks_exact(REC)
        .take(n)
        .map(|r| Inst {
            mesh: u64::from_le_bytes(r[0..8].try_into().unwrap()),
            wind: f(r, 8),
            color: u32::from_le_bytes(r[12..16].try_into().unwrap()),
            pos: Vec3::new(f(r, 16), f(r, 20), f(r, 24)),
            rot: Quat::from_xyzw(f(r, 28), f(r, 32), f(r, 36), f(r, 40)).normalize(),
            scale: [f(r, 44), f(r, 48)],
        })
        .collect()
}

/// One submesh of a brush mesh with its resolved material.
struct BrushSub {
    mat_key: u64,
    rec: MaterialRec,
}

struct Brush {
    md: MeshData,
    subs: Vec<Option<BrushSub>>,
    y_min: f32,
    height: f32,
    radius: f32,
    tris0: u32,
    /// Some submesh has LoD index ranges that really differ (the grass / herb / stone brush meshes have none).
    has_lod: bool,
}

struct Brushes {
    map: HashMap<u64, Brush>,
}

fn mat_key(rec: &MaterialRec) -> u64 {
    use std::hash::{Hash, Hasher};
    let l = rec.layers[0];
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (l.tex[0], rec.mask_reference).hash(&mut h);
    for f in [
        l.tint[0],
        l.tint[1],
        l.tint[2],
        l.scale[0][0],
        l.scale[0][1],
    ] {
        f.to_bits().hash(&mut h);
    }
    h.finish()
}

fn load_brush(cache: &Cache, id: u64) -> Option<Brush> {
    let md = cache.load_mesh(AssetId(id)).ok()?;
    let hdr = md.header();
    let names = md.material_names();
    let mut tris0 = 0;
    let mut has_lod = false;
    let subs = md
        .submeshes()
        .iter()
        .enumerate()
        .map(|(i, s)| {
            if names
                .get(i)
                .is_some_and(|n| n.to_ascii_lowercase().contains("world_lowpoly"))
            {
                return None;
            }
            let rec = cache.material(s.material)?;
            if rec.kind == KIND_WATER {
                return None;
            }
            tris0 += md.indices(s, 0).len() as u32 / 3;
            has_lod |= md.indices(s, 0).len() != md.indices(s, 2).len();
            Some(BrushSub {
                mat_key: mat_key(&rec),
                rec,
            })
        })
        .collect();
    let (lo, hi) = (Vec3::from(hdr.bounds_min), Vec3::from(hdr.bounds_max));
    Some(Brush {
        md,
        subs,
        y_min: lo.y,
        height: hi.y - lo.y,
        radius: (hi - lo).length() * 0.5,
        tris0,
        has_lod,
    })
}

// --- chunk build (worker thread) -------------------------------------------------------------------------------------

#[derive(Default)]
struct PartBuf {
    rec: MaterialRec,
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
    lo: Vec3,
    hi: Vec3,
}

struct ChunkBuild {
    parts: Vec<(u64, PartBuf)>,
    instances: u32,
}

fn h01(i: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B1);
    x ^= x >> 15;
    x = x.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 13;
    (x & 0xFFFF) as f32 / 65536.0
}

/// Merges the instances of one chunk into one buffer per material at mesh LoD `level`.
fn build_chunk(brushes: &Brushes, insts: &[Inst], level: usize, density: f32) -> ChunkBuild {
    let mut parts: HashMap<u64, PartBuf> = HashMap::new();
    let mut scratch: Vec<u32> = Vec::new();
    let mut count = 0;
    for (n, inst) in insts.iter().enumerate() {
        if density < 1.0 && h01(n as u32) >= density {
            continue;
        }
        let Some(b) = brushes.map.get(&inst.mesh) else {
            continue;
        };
        count += 1;
        // The engine scales (x, y, x): `scale[1]` is the height (`eCVegetation_GridNode::CacheInGeometry`).
        let xf = Mat4::from_scale_rotation_translation(
            Vec3::new(inst.scale[0], inst.scale[1], inst.scale[0]),
            inst.rot,
            inst.pos,
        );
        let tint = [
            ((inst.color >> 16) & 255) as f32 / 255.0,
            ((inst.color >> 8) & 255) as f32 / 255.0,
            (inst.color & 255) as f32 / 255.0,
        ];
        for (si, s) in b.md.submeshes().iter().enumerate() {
            let Some(bs) = &b.subs[si] else { continue };
            let part = parts.entry(bs.mat_key).or_insert_with(|| PartBuf {
                rec: bs.rec,
                lo: Vec3::MAX,
                hi: Vec3::MIN,
                ..default()
            });
            let vs = b.md.submesh_vertices(s);
            let idx =
                b.md.indices(s, level.min((s.lod_count as usize).max(1) - 1));
            scratch.clear();
            scratch.resize(vs.len(), u32::MAX);
            for k in 0..idx.len() {
                let vi = idx.get(k) as usize;
                if scratch[vi] == u32::MAX {
                    let v = &vs[vi];
                    scratch[vi] = part.pos.len() as u32;
                    let wp = xf.transform_point3(Vec3::from(v.pos));
                    part.lo = part.lo.min(wp);
                    part.hi = part.hi.max(wp);
                    part.pos.push(wp.to_array());
                    // Ground cover shades like the ground it stands on: blend the normal toward up (desktop loader).
                    let n = xf
                        .transform_vector3(Vec3::from(v.normal))
                        .normalize_or_zero();
                    part.nrm
                        .push((Vec3::Y + n * 0.1).normalize_or_zero().to_array());
                    part.uv.push(v.uv);
                    let w = gothic_environment::wind::vegetation_weight(
                        v.pos[1], b.y_min, b.height, inst.wind,
                    );
                    part.col.push([
                        v.color[0] as f32 / 255.0 * tint[0],
                        v.color[1] as f32 / 255.0 * tint[1],
                        v.color[2] as f32 / 255.0 * tint[2],
                        w,
                    ]);
                }
                part.idx.push(scratch[vi]);
            }
        }
    }
    let mut parts: Vec<_> = parts
        .into_iter()
        .filter(|(_, p)| !p.idx.is_empty())
        .collect();
    parts.sort_by_key(|(k, _)| *k);
    ChunkBuild {
        parts,
        instances: count,
    }
}

// --- resources / streaming -------------------------------------------------------------------------------------------

#[derive(Component)]
pub struct VegChunkRoot;

struct Resident {
    entity: Entity,
    level: u8,
    visible: bool,
    tris: u32,
    parts: u32,
}

struct Chunk {
    insts: Arc<Vec<Inst>>,
    lo: Vec3,
    hi: Vec3,
    /// Weighted mean brush radius (cm): input of the mesh LoD stage distances.
    radius: f32,
    stage_d: Vec<f32>,
    /// Stage distances come from real mesh LoD ranges (else: density thinning stage, see `FAR_DENSITY`).
    mesh_lod: bool,
    resident: Option<Resident>,
    task: Option<(Task<ChunkBuild>, u8)>,
    want: u8,
}

#[derive(Default, Clone, Debug)]
pub struct VegStats {
    pub chunks: usize,
    pub chunks_visible: usize,
    pub building: usize,
    pub instances: u64,
    pub tris: u64,
    pub tris_visible: u64,
    pub draws_visible: u32,
    pub builds: u64,
    pub despawns: u64,
    pub max_spawn_ms: f32,
}

#[derive(Resource)]
pub struct Veg {
    root: Entity,
    cache: Arc<Cache>,
    dir: std::path::PathBuf,
    brushes: Arc<Brushes>,
    cells: HashMap<(i32, i32), bool>,
    chunks: HashMap<(i32, i32), Chunk>,
    mats: HashMap<u64, Handle<VegMaterial>>,
    pub view_range: f32,
    pub fade_start: f32,
    density: f32,
    lod_scale: f32,
    far_density: f32,
    pub stats: VegStats,
    /// Viewer position (cm) the last update used.
    pub viewer: Vec3,
}

pub struct VegPlugin;

impl Plugin for VegPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/veg.wesl");
        let _ = VEG_SHADER.set(load_embedded_asset!(app, "shaders/veg.wesl"));
        app.add_plugins(MaterialPlugin::<VegMaterial>::default());
        let Some(dir) = crate::ardea::data_dir() else {
            return;
        };
        if !dir.join("veg").is_dir() {
            log::warn!(
                "[veg] no veg/ directory in {} (run examples/pack_veg)",
                dir.display()
            );
            return;
        }
        let Ok(cache) = Cache::open(&dir) else { return };
        let quality = knob_f32("veg_quality", QUEST_QUALITY as f32) as u8;
        let view_range = gothic_render_rules::vegetation::view_range(quality);
        let fade_start = gothic_render_rules::vegetation::fade_start(
            view_range,
            (FADE_START_FRACTION - 0.65) / 0.10,
        );
        // Brush meshes: every mesh id referenced by any veg file, read once.
        let mut cells = HashMap::new();
        let mut ids: std::collections::BTreeSet<u64> = Default::default();
        if let Ok(rd) = std::fs::read_dir(dir.join("veg")) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let Some(stem) = name.strip_suffix(".gveg") else {
                    continue;
                };
                let Some((a, b)) = stem.split_once('_') else {
                    continue;
                };
                let (Ok(cx), Ok(cz)) = (a.parse::<i32>(), b.parse::<i32>()) else {
                    continue;
                };
                cells.insert((cx, cz), false);
                if let Ok(bytes) = std::fs::read(e.path()) {
                    for r in bytes.get(8..).unwrap_or(&[]).chunks_exact(REC) {
                        ids.insert(u64::from_le_bytes(r[0..8].try_into().unwrap()));
                    }
                }
            }
        }
        let mut map = HashMap::new();
        for id in ids {
            if let Some(b) = load_brush(&cache, id) {
                map.insert(id, b);
            }
        }
        if knob("veg_dump").is_some() {
            for (id, b) in &map {
                let t = |l: usize| {
                    b.md.submeshes()
                        .iter()
                        .map(|s| b.md.indices(s, l).len() as u32 / 3)
                        .sum::<u32>()
                };
                log::info!(
                    "[veg] brush {id:016x} radius {:.0} cm tris lod0/1/2 = {}/{}/{}",
                    b.radius,
                    t(0),
                    t(1),
                    t(2)
                );
            }
        }
        log::info!(
            "[veg] {} cells with ground cover, {} brush meshes, view range {view_range} cm (fade from {fade_start})",
            cells.len(),
            map.len()
        );
        let root = app
            .world_mut()
            .query_filtered::<Entity, With<WorldRoot>>()
            .iter(app.world())
            .next();
        let Some(root) = root else {
            log::warn!("[veg] Ardea world root missing (add VegPlugin after ardea::build)");
            return;
        };
        app.insert_resource(Veg {
            root,
            cache: Arc::new(cache),
            dir,
            brushes: Arc::new(Brushes { map }),
            cells,
            chunks: HashMap::new(),
            mats: HashMap::new(),
            view_range,
            fade_start,
            density: knob_f32("veg_density", 1.0).clamp(0.0, 1.0),
            lod_scale: knob_f32("lod_scale", 1.0),
            far_density: knob_f32("veg_far_density", FAR_DENSITY).clamp(0.05, 1.0),
            stats: VegStats::default(),
            viewer: Vec3::splat(1e9),
        });
        app.add_systems(
            Update,
            (veg_update, veg_report)
                .chain()
                .run_if(resource_exists::<Veg>),
        );
    }
}

/// Nearest distance from `p` to the box.
fn box_dist(p: Vec3, lo: Vec3, hi: Vec3) -> f32 {
    let d = (lo - p).max(p - hi).max(Vec3::ZERO);
    d.length()
}

impl Veg {
    /// Resident chunks / instances / triangles per band of nearest-point distance (cm) from the viewer: `(lo, hi, ..)`.
    pub fn bands(&self, edges: &[f32]) -> Vec<(f32, f32, usize, u64, u64)> {
        let mut out: Vec<(f32, f32, usize, u64, u64)> = edges
            .iter()
            .enumerate()
            .map(|(i, e)| (*e, edges.get(i + 1).copied().unwrap_or(f32::MAX), 0, 0, 0))
            .collect();
        for c in self.chunks.values() {
            let Some(r) = &c.resident else { continue };
            let d = box_dist(self.viewer, c.lo, c.hi);
            if let Some(b) = out.iter_mut().find(|b| d >= b.0 && d < b.1) {
                b.2 += 1;
                b.3 += c.insts.len() as u64;
                b.4 += r.tris as u64;
            }
        }
        out
    }

    fn material(
        &mut self,
        rec: &MaterialRec,
        a: &mut Ardea,
        mats: &mut Assets<VegMaterial>,
        images: &mut Assets<Image>,
    ) -> Handle<VegMaterial> {
        let key = mat_key(rec);
        if let Some(h) = self.mats.get(&key) {
            return h.clone();
        }
        let l = rec.layers[0];
        let tex = a.texture(l.tex[0], images);
        let uvs = tex.as_ref().map_or(Vec2::ONE, |t| t.1);
        let (sun_col, sun_dir, sh) = qmat::light_env();
        let fog = qmat::fog_color();
        let masked = rec.mask_reference != NONE_U32;
        // Ground cover is always alpha tested (desktop: forced mask, 0.4 when the material has none or a tiny reference).
        let cutoff = if masked && rec.mask_reference as f32 / 255.0 >= 0.05 {
            rec.mask_reference as f32 / 255.0
        } else {
            0.4
        };
        let (veg, amp) = wind_consts();
        let m = VegMaterial {
            params: VParams {
                tint: if tex.is_some() {
                    Vec4::new(
                        l.tint[0].max(0.05),
                        l.tint[1].max(0.05),
                        l.tint[2].max(0.05),
                        1.0,
                    )
                } else {
                    Vec4::new(0.5, 0.5, 0.5, 1.0)
                },
                uv_scale: Vec4::new(l.scale[0][0] * uvs.x, l.scale[0][1] * uvs.y, 0.0, 0.0),
                sun_dir: sun_dir.extend(0.0),
                sun_col: sun_col.extend(0.0),
                sh,
                fog_col: fog.extend(1.0),
                misc: Vec4::new(cutoff, self.fade_start * 0.01, self.view_range * 0.01, 0.0),
                fog: Vec4::new(
                    knob_f32("fog_start", 120.0),
                    knob_f32("fog_end", 420.0),
                    0.0,
                    0.0,
                ),
                veg,
                veg_amp: amp,
                player: Vec4::new(EYE_HEIGHT_M, 0.0, 0.0, 0.0),
            },
            tex: Some(tex.map(|t| t.0).unwrap_or_else(|| a.white_image())),
        };
        let h = mats.add(m);
        self.mats.insert(key, h.clone());
        h
    }
}

fn level_for(c: &Chunk, d: f32, cur: Option<u8>) -> u8 {
    let n = c.stage_d.len();
    let raw = c.stage_d.iter().filter(|e| d > **e).count().min(n);
    let Some(cur) = cur else { return raw as u8 };
    // 5 % hysteresis around each boundary (same as the static props).
    let cur_i = cur as usize;
    if raw > cur_i && cur_i < n && d < c.stage_d[cur_i] * 1.05 {
        return cur;
    }
    if raw < cur_i && cur_i > 0 && d > c.stage_d[cur_i - 1] * 0.95 {
        return cur;
    }
    raw as u8
}

#[allow(clippy::too_many_arguments)]
fn veg_update(
    mut commands: Commands,
    mut v: ResMut<Veg>,
    mut a: ResMut<Ardea>,
    eyes: Query<&GlobalTransform, With<XrEye>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<VegMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut vis: Query<&mut Visibility>,
) {
    let (mut n, mut sum) = (0, Vec3::ZERO);
    for e in &eyes {
        sum += e.translation();
        n += 1;
    }
    if n == 0 {
        return;
    }
    let viewer = sum / n as f32 * CM;
    v.viewer = viewer;
    let (view, load_r, unload_r) = (
        v.view_range,
        v.view_range * LOAD_FACTOR,
        v.view_range * UNLOAD_FACTOR,
    );

    // 1. Cell data on demand (one cell per frame): the chunks of a cell exist while the viewer is within unload range.
    let cell_keys: Vec<(i32, i32)> = v.cells.keys().copied().collect();
    for (cx, cz) in cell_keys {
        // Gothic cell (cx, cz) covers x in [cx, cx+1) * 10000 in Gothic space = (-(cx+1)*10000, -cx*10000] in Bevy.
        let lo = Vec3::new(-(cx as f32 + 1.0) * 10_000.0, -1e6, cz as f32 * 10_000.0);
        let hi = Vec3::new(-(cx as f32) * 10_000.0, 1e6, (cz as f32 + 1.0) * 10_000.0);
        let d = box_dist(viewer, lo, hi);
        let loaded = v.cells[&(cx, cz)];
        if !loaded && d < load_r {
            let bytes =
                std::fs::read(v.dir.join(format!("veg/{cx}_{cz}.gveg"))).unwrap_or_default();
            let insts = parse_gveg(&bytes);
            let mut by_chunk: HashMap<(i32, i32), Vec<Inst>> = HashMap::new();
            for i in insts {
                if v.brushes.map.contains_key(&i.mesh) {
                    by_chunk
                        .entry((
                            (i.pos.x / CHUNK_CM).floor() as i32,
                            (i.pos.z / CHUNK_CM).floor() as i32,
                        ))
                        .or_default()
                        .push(i);
                }
            }
            let (lod_scale, far, view) = (v.lod_scale, v.far_density, v.view_range);
            for (k, list) in by_chunk {
                let (mut lo, mut hi, mut rsum) = (Vec3::MAX, Vec3::MIN, 0.0);
                for i in &list {
                    let r = v.brushes.map[&i.mesh].radius * i.scale[1].max(i.scale[0]);
                    lo = lo.min(i.pos - Vec3::new(r, 0.0, r));
                    hi = hi.max(i.pos + Vec3::new(r, 2.0 * r, r));
                    rsum += v.brushes.map[&i.mesh].radius;
                }
                let radius = rsum / list.len() as f32;
                let mesh_lod = list.iter().any(|i| v.brushes.map[&i.mesh].has_lod);
                let stage_d = if mesh_lod {
                    gothic_render_rules::lod::stage_distances(radius, 3, lod_scale, 1.0)
                } else if far < 1.0 {
                    vec![view * FAR_START]
                } else {
                    Vec::new()
                };
                let pad = Vec3::splat(WIND_PAD_CM);
                v.chunks.insert(
                    k,
                    Chunk {
                        stage_d,
                        mesh_lod,
                        insts: Arc::new(list),
                        lo: lo - pad,
                        hi: hi + pad,
                        radius,
                        resident: None,
                        task: None,
                        want: 0,
                    },
                );
            }
            v.cells.insert((cx, cz), true);
            break;
        }
        if loaded && d > unload_r {
            // Drop the cell's chunk data once none of its chunks is resident or building.
            let mut busy = false;
            let keys: Vec<(i32, i32)> = v
                .chunks
                .iter()
                .filter(|(_, c)| {
                    let m = (c.lo + c.hi) * 0.5;
                    m.x > lo.x && m.x <= hi.x && m.z >= lo.z && m.z < hi.z
                })
                .map(|(k, c)| {
                    busy |= c.resident.is_some() || c.task.is_some();
                    *k
                })
                .collect();
            if !busy {
                for k in keys {
                    v.chunks.remove(&k);
                }
                v.cells.insert((cx, cz), false);
            }
        }
    }

    // 2. Poll finished builds (one spawn per frame bounds the upload cost).
    let t0 = std::time::Instant::now();
    let mut spawned = false;
    let keys: Vec<(i32, i32)> = v
        .chunks
        .iter()
        .filter(|(_, c)| c.task.is_some())
        .map(|(k, _)| *k)
        .collect();
    for k in keys {
        if spawned {
            break;
        }
        let Some((mut task, level)) = v.chunks.get_mut(&k).and_then(|c| c.task.take()) else {
            continue;
        };
        let Some(build) = check_ready(&mut task) else {
            v.chunks.get_mut(&k).unwrap().task = Some((task, level));
            continue;
        };
        let (d, root) = (box_dist(viewer, v.chunks[&k].lo, v.chunks[&k].hi), v.root);
        // Built for a chunk that has since gone out of range: drop it.
        if d > unload_r {
            continue;
        }
        let holder = commands
            .spawn((
                VegChunkRoot,
                Transform::IDENTITY,
                Visibility::Inherited,
                ChildOf(root),
            ))
            .id();
        let (mut tris, mut nparts) = (0u32, 0u32);
        for (_, part) in build.parts {
            let mat = {
                let rec = part.rec;
                v.material(&rec, &mut a, &mut mats, &mut images)
            };
            let t = part.idx.len() as u32 / 3;
            tris += t;
            nparts += 1;
            let mut m = Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::RENDER_WORLD,
            );
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, part.pos);
            m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, part.nrm);
            m.insert_attribute(Mesh::ATTRIBUTE_UV_0, part.uv);
            m.insert_attribute(Mesh::ATTRIBUTE_COLOR, part.col);
            m.insert_indices(if m.count_vertices() <= u16::MAX as usize {
                Indices::U16(part.idx.iter().map(|i| *i as u16).collect())
            } else {
                Indices::U32(part.idx)
            });
            let pad = Vec3::splat(WIND_PAD_CM);
            commands.spawn((
                Mesh3d(meshes.add(m)),
                MeshMaterial3d(mat),
                Transform::IDENTITY,
                Aabb::from_min_max(part.lo - pad, part.hi + pad),
                TriCount(t),
                ChildOf(holder),
            ));
        }
        let c = v.chunks.get_mut(&k).unwrap();
        if let Some(old) = c.resident.take() {
            commands.entity(old.entity).despawn();
        }
        c.resident = Some(Resident {
            entity: holder,
            level,
            visible: true,
            tris,
            parts: nparts,
        });
        v.stats.builds += 1;
        let _ = build.instances;
        spawned = true;
    }
    if spawned {
        v.stats.max_spawn_ms = v.stats.max_spawn_ms.max(t0.elapsed().as_secs_f32() * 1e3);
    }

    // 3. Per chunk: despawn, visibility, wanted LoD, new builds (nearest first).
    let mut in_flight = v.chunks.values().filter(|c| c.task.is_some()).count();
    let mut starts: Vec<((i32, i32), f32, u8)> = Vec::new();
    let mut despawn: Vec<(i32, i32)> = Vec::new();
    for (k, c) in v.chunks.iter_mut() {
        let d = box_dist(viewer, c.lo, c.hi);
        if c.resident.is_some() {
            if d > unload_r {
                despawn.push(*k);
                continue;
            }
            let show = d < view;
            let cur = c.resident.as_ref().map(|r| r.level);
            let w = if c.stage_d.is_empty() {
                0
            } else {
                level_for(c, d, cur)
            };
            if let Some(r) = &mut c.resident {
                if show != r.visible {
                    r.visible = show;
                    if let Ok(mut vi) = vis.get_mut(r.entity) {
                        *vi = if show {
                            Visibility::Inherited
                        } else {
                            Visibility::Hidden
                        };
                    }
                }
                if show {
                    if w != r.level && c.task.is_none() {
                        starts.push((*k, d, w));
                    }
                }
            }
            c.want = w;
        } else if c.task.is_none() && d < load_r {
            c.want = level_for(c, d, None);
            starts.push((*k, d, c.want));
        }
    }
    for k in despawn {
        let c = v.chunks.get_mut(&k).unwrap();
        if let Some(r) = c.resident.take() {
            commands.entity(r.entity).despawn();
            v.stats.despawns += 1;
        }
    }
    starts.sort_by(|a, b| a.1.total_cmp(&b.1));
    let pool = AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    for (k, _d, level) in starts {
        if in_flight >= MAX_BUILDS_IN_FLIGHT {
            break;
        }
        let c = &v.chunks[&k];
        let thin = if !c.mesh_lod && level >= 1 {
            v.far_density
        } else {
            1.0
        };
        let (brushes, insts, density) = (v.brushes.clone(), c.insts.clone(), v.density * thin);
        let task =
            pool.spawn(async move { build_chunk(&brushes, &insts, level as usize, density) });
        v.chunks.get_mut(&k).unwrap().task = Some((task, level));
        in_flight += 1;
    }
    let _ = &v.cache;
}

fn veg_report(
    mut v: ResMut<Veg>,
    time: Res<Time>,
    mut acc: Local<f32>,
    parts: Query<(&ViewVisibility, &TriCount, &ChildOf), With<MeshMaterial3d<VegMaterial>>>,
) {
    let mut s = VegStats {
        builds: v.stats.builds,
        despawns: v.stats.despawns,
        max_spawn_ms: v.stats.max_spawn_ms,
        ..default()
    };
    for c in v.chunks.values() {
        if let Some(r) = &c.resident {
            s.chunks += 1;
            s.chunks_visible += r.visible as usize;
            s.tris += r.tris as u64;
        }
        s.building += c.task.is_some() as usize;
        s.instances += c.resident.is_some() as u64 * c.insts.len() as u64;
    }
    for (vv, t, _) in &parts {
        if vv.get() {
            s.tris_visible += t.0 as u64;
            s.draws_visible += 1;
        }
    }
    v.stats = s;
    *acc += time.delta_secs();
    if *acc < 2.0 {
        return;
    }
    *acc = 0.0;
    let s = &v.stats;
    log::info!(
        "[veg] chunks={} visible={} building={} instances={} tris={} visible_tris={} visible_draws={} builds={} despawns={} max_spawn_ms={:.2}",
        s.chunks,
        s.chunks_visible,
        s.building,
        s.instances,
        s.tris,
        s.tris_visible,
        s.draws_visible,
        s.builds,
        s.despawns,
        s.max_spawn_ms
    );
}

/// `debug.gr.veg=0` disables the ground cover (default on).
pub fn enabled() -> bool {
    knob("veg").as_deref() != Some("0")
}
