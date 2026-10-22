//! Background loader: turns world tiles into CPU-ready render + collision payloads.
//!
//! Coordinate conversion (see docs/research/TERRAIN_FORMAT.md, XCMSH_LAYOUT.md):
//! Gothic 3 is a left-handed, Y-up, clockwise-front D3D title; 1 unit = 1 cm. Bevy is
//! right-handed. We mirror X (`F = diag(-1,1,1)`), which flips handedness *and* the
//! winding at once, so source index order is kept and front faces stay front. Instance
//! matrices (row-vector convention) become `F * M * F`; a fixed origin offset keeps f32
//! coordinates small. Everything this module emits is in centimetres relative to the
//! origin; the renderer applies a uniform 0.01 scale.
mod trees;

use anyhow::{Context, Result};
use bevy::math::{Mat4, Quat, Vec3};
use gothic_formats::{
    terrain::{parse_cell_name, spt::Spt, vegetation},
    vfs::VirtualFileSystem,
    xcmsh::Xcmsh,
    ximg::Ximg,
    xlmsh::Xlmsh,
    xnvmsh::Xnvmsh,
    xshmat::{TextureRole, Xshmat, graph::MaterialGraph},
};
use gothic_importer::world_index::WorldIndex;
pub use gothic_render_rules::water::{is_confirmed_ocean_resource, is_river_depth_resource};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::Path,
    sync::{
        Arc,
        mpsc::{Receiver, Sender, channel},
    },
    thread,
};

/// Tile edge in centimetres (25 m), in origin-relative, mirrored space.
pub const TILE_CM: f32 = 2500.0;
/// Brightness multiplier of the (dark) SpeedTree composite map and bark textures.
const TREE_LEAF_TINT: f32 = 0.42;
const TREE_BARK_TINT: f32 = 2.0;
pub type TileKey = (i32, i32);

pub struct TexData {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub fourcc: [u8; 4],
    pub mips: Vec<Vec<u8>>,
}

/// One colour layer: `tex[0] * tex[1] * 2` (the second texture optional), uv scales per texture.
#[derive(Clone)]
pub struct LayerData {
    pub tex: [Option<Arc<TexData>>; 2],
    pub scale: [[f32; 2]; 2],
    pub tint: [f32; 3],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatKind {
    Surface,
    Water,
    /// Water shader without samplers. Legitimate sea surfaces also use this form; source
    /// identity is required before treating it as a depth proxy.
    WaterUntextured,
}

impl MatKind {
    pub fn is_water(self) -> bool {
        matches!(self, Self::Water | Self::WaterUntextured)
    }
}

pub struct MatData {
    pub id: u64,
    pub name: String,
    pub kind: MatKind,
    pub blend_mode: Option<u32>,
    pub mask_reference: Option<u8>,
    /// One layer, or two when `vertex_blend` (layer 1 weighted by the vertex alpha).
    pub layers: Vec<LayerData>,
    pub vertex_blend: bool,
    /// Normal map per layer with its uv scale (water: the first two normal maps).
    pub normals: [Option<(Arc<TexData>, [f32; 2])>; 2],
    /// Texture paths of the layers (lower case), used for vegetation placement.
    pub layer_names: Vec<String>,
    /// Texture-coordinate animation of the first diffuse and the first normal sampler
    /// (scroller / rotator nodes of the material graph, see `gothic_fx::matadmin`).
    pub anim: [gothic_formats::xshmat::graph::UvAnim; 2],
    /// SpeedTree height in local units (wind weight normalisation); 0 for other materials.
    pub wind_height: f32,
}

/// GPU-ready geometry in local (mirrored) space; shared between instances.
pub struct GeomData {
    pub id: u64,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// rgb: baked ambient occlusion, a: layer blend weight.
    pub colors: Vec<[f32; 4]>,
    pub tangents: Vec<[f32; 4]>,
    /// Baked static-light colour per vertex (`xlmp`), packed `[r, b]` (g = sqrt(r*b)); empty = none.
    pub light: Vec<[f32; 2]>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PartKind {
    Static,
    /// Per-tile, per-material merged static props by object size (distance-culled at 90 / 200 m).
    MergedSmall,
    MergedMid,
    MergedBig,
    Terrain,
    Water,
    /// Scattered ground cover: alpha tested, two sided, distance faded, casts no shadow.
    Foliage,
    /// SpeedTree leaf cards and frond ribbons (alpha tested, two sided, near LOD).
    Tree,
    /// SpeedTree bark tubes (opaque, near LOD).
    TreeBark,
    /// SpeedTree whole-tree billboard (far LOD, cross-faded with the near parts).
    TreeFar,
    /// Far-LOD bark of species without a billboard (reduced real geometry).
    TreeFarBark,
}

pub struct Part {
    pub geom: Arc<GeomData>,
    pub material: Option<Arc<MatData>>,
    /// Source placed-mesh identity from the world index. Shared by all subparts of one entity.
    /// Present only on water parts; source identity distinguishes confirmed ocean meshes.
    pub source: Option<Arc<PartSource>>,
    /// Vertex-derived sea height for source-confirmed planar ocean parts, in metres.
    pub ocean_plane_y_m: Option<f32>,
    /// Origin-relative cm transform (mirrored space).
    pub transform: Mat4,
    pub kind: PartKind,
    /// Mesh LoD stage this part belongs to (`eCRenderContextMesh::CalculateLoDStage`); `None`
    /// for geometry without LoD meshes.
    pub lod: Option<LodPart>,
}

#[derive(Debug)]
pub struct PartSource {
    pub resource: Arc<str>,
    pub entity_id: Arc<str>,
}

fn planar_ocean_y_m(positions: &[[f32; 3]], transform: Mat4) -> Option<f32> {
    gothic_render_rules::water::planar_water_height_m(positions.iter().map(|position| {
        transform.transform_point3(Vec3::from(*position)).y
    }))
}

/// One stage of a placed mesh that has several LoD meshes (`.xlmsh`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodPart {
    /// 0 = full detail.
    pub stage: u8,
    /// Number of stages (LoD meshes) of the visual.
    pub stages: u8,
    /// Bounding sphere radius of the placed visual (cm).
    pub radius_cm: f32,
}

pub struct TilePayload {
    pub key: TileKey,
    pub parts: Vec<Part>,
    /// Collision triangles, cm, origin-relative mirrored space.
    pub collision: Vec<[Vec3; 3]>,
    pub terrain_collision_from_nav: bool,
    pub stats: TileStats,
}

#[derive(Default, Clone, Copy)]
pub struct TileStats {
    pub statics: usize,
    pub statics_with_collision_mesh: usize,
    pub trees: usize,
}

pub struct VegPayload {
    pub key: TileKey,
    pub parts: Vec<Part>,
    pub instances: usize,
}

pub enum Request {
    Tile(TileKey),
    Vegetation(TileKey),
    /// A texture by stem (e.g. sky clouds).
    Texture(String),
    /// The weather states and every weather zone of the world.
    Weather,
}
pub enum Response {
    Weather(Box<WeatherPayload>),
    Ready {
        records: usize,
        millis: u128,
        /// Placed meshes per PVS cell (cell index, count): the world data that drives the
        /// residency budget.
        cell_weights: Vec<((i32, i32), u32)>,
    },
    Tile(Box<TilePayload>),
    Vegetation(Box<VegPayload>),
    Texture(String, Option<Arc<TexData>>),
    Failed(String),
}

pub struct LoaderHandle {
    pub requests: Sender<Request>,
    pub responses: Receiver<Response>,
}

/// Gothic-space origin the viewer subtracts (cm).
#[derive(Clone, Copy)]
pub struct Origin(pub Vec3);

impl Origin {
    /// Gothic world point -> origin-relative mirrored point.
    pub fn point(&self, p: Vec3) -> Vec3 {
        Vec3::new(self.0.x - p.x, p.y - self.0.y, p.z - self.0.z)
    }
}

pub fn spawn_loader(data_dir: std::path::PathBuf, origin: Origin) -> LoaderHandle {
    let (req_tx, req_rx) = channel::<Request>();
    let (resp_tx, resp_rx) = channel::<Response>();
    thread::Builder::new()
        .name("world-loader".into())
        .spawn(move || {
            match Loader::new(&data_dir, origin) {
                Ok(mut loader) => {
                    let _ = resp_tx.send(Response::Ready {
                        records: loader.index.len(),
                        millis: loader.startup_ms,
                        cell_weights: loader
                            .index
                            .pvs_cell_weights(gothic_world::pvs::MAIN_CELL_SIZE),
                    });
                    while let Ok(request) = req_rx.recv() {
                        match request {
                            Request::Tile(key) => match loader.load_tile(key) {
                                Ok(p) => {
                                    if resp_tx.send(Response::Tile(Box::new(p))).is_err() {
                                        break;
                                    }
                                }
                                Err(e) => {
                                    let _ = resp_tx
                                        .send(Response::Failed(format!("tile {key:?}: {e:#}")));
                                    // An empty payload keeps the streaming bookkeeping moving.
                                    let _ = resp_tx.send(Response::Tile(Box::new(TilePayload {
                                        key,
                                        parts: vec![],
                                        collision: vec![],
                                        terrain_collision_from_nav: false,
                                        stats: TileStats::default(),
                                    })));
                                }
                            },
                            Request::Texture(stem) => {
                                let t = loader.texture(&stem);
                                if resp_tx.send(Response::Texture(stem, t)).is_err() {
                                    break;
                                }
                            }
                            Request::Weather => {
                                let p = loader.load_weather();
                                if resp_tx.send(Response::Weather(Box::new(p))).is_err() {
                                    break;
                                }
                            }
                            Request::Vegetation(key) => {
                                let p = loader.load_vegetation(key);
                                if resp_tx.send(Response::Vegetation(Box::new(p))).is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = resp_tx.send(Response::Failed(format!("loader start: {e:#}")));
                }
            }
        })
        .expect("spawn world loader thread");
    LoaderHandle {
        requests: req_tx,
        responses: resp_rx,
    }
}

/// A weather zone with the matrix that maps Gothic world cm into its local frame.
pub struct ZoneInstance {
    pub zone: gothic_environment::Zone,
    pub world_to_local: Mat4,
}

pub struct WeatherPayload {
    pub data: Option<gothic_formats::weather::WeatherData>,
    pub zones: Vec<ZoneInstance>,
    /// Every `eCStaticPointLight_PS`: world position, colour and range (Gothic cm).
    pub lights: Vec<StaticLightRaw>,
    /// The world's `eCSpeedTreeWind_PS` (system entity), if present.
    pub wind: Option<gothic_environment::wind::SpeedTreeWind>,
    /// The world's `eCPrecipitation_PS` (system entity), if present.
    pub precipitation: Option<gothic_environment::precip::PrecipParams>,
    /// The world's `eCSkydome_PS` (system entity), if present.
    pub skydome: Option<gothic_environment::sky::SkydomeParams>,
}

pub struct StaticLightRaw {
    pub position: Vec3,
    pub color: [f32; 3],
    pub range: f32,
}

/// One authored vegetation instance (Gothic space, cm).
struct VegInstance {
    mesh: String,
    position: Vec3,
    rotation: Quat,
    scale: [f32; 2],
    color: u32,
    /// `eCVegetation_Mesh::WindStrength` of the brush mesh.
    wind: f32,
}

struct TerrainSub {
    material: String,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    tangents: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

struct MeshEntry {
    parts: Vec<(Arc<GeomData>, Option<Arc<MatData>>)>,
    /// Submesh index of each part (empty submeshes are skipped) and the lightmap name stem.
    sub_index: Vec<usize>,
    stem: String,
    /// Coarser LoD meshes of an `.xlmsh` (stage 1, 2, ...), loaded with the same rules.
    lods: Vec<Arc<MeshEntry>>,
    /// Bounding sphere radius of the full-detail parts (local units).
    radius: f32,
}

impl MeshEntry {
    fn new(
        parts: Vec<(Arc<GeomData>, Option<Arc<MatData>>)>,
        sub_index: Vec<usize>,
        stem: String,
    ) -> Self {
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for (g, _) in &parts {
            for p in &g.positions {
                lo = lo.min(Vec3::from(*p));
                hi = hi.max(Vec3::from(*p));
            }
        }
        let radius = if lo.x <= hi.x {
            (hi - lo).length() * 0.5
        } else {
            0.0
        };
        Self {
            parts,
            sub_index,
            stem,
            lods: Vec::new(),
            radius,
        }
    }

    fn triangles(&self) -> usize {
        self.parts.iter().map(|(g, _)| g.indices.len() / 3).sum()
    }
}

struct CellInfo {
    #[allow(dead_code)]
    number: u32,
    mesh: String,
    sector: (f32, f32),
}

struct TreeNearVariant {
    bark: Option<Arc<GeomData>>,
    cards: Option<Arc<GeomData>>,
}

struct TreeKind {
    variants: Vec<TreeNearVariant>,
    far_bark: Option<Arc<GeomData>>,
    far: Arc<GeomData>,
    bark_mat: Arc<MatData>,
    card_mat: Arc<MatData>,
    /// Trunk collision cylinder in cm (radius, height), unscaled.
    trunk: (f32, f32),
}

struct Loader {
    vfs: VirtualFileSystem,
    index: WorldIndex,
    origin: Origin,
    startup_ms: u128,
    cells: Vec<CellInfo>,
    cells_loaded: HashSet<usize>,
    nav_loaded: HashSet<usize>,
    terrain: HashMap<TileKey, Vec<TerrainSub>>,
    tile_cells: HashMap<TileKey, BTreeSet<usize>>,
    nav_tiles: HashMap<TileKey, Vec<[Vec3; 3]>>,
    meshes: HashMap<String, Option<Arc<MeshEntry>>>,
    materials: HashMap<String, Option<Arc<MatData>>>,
    textures: HashMap<String, Option<Arc<TexData>>>,
    collision_meshes: HashMap<String, Option<Arc<Vec<[Vec3; 3]>>>>,
    /// Parsed vegetation documents (`CStat` sector -> trees), by document path.
    tree_docs: HashMap<String, Vec<vegetation::TreeEntity>>,
    /// Painted vegetation grid instances (`eCVegetation_Grid`), by document path.
    veg_docs: HashMap<String, Arc<Vec<VegInstance>>>,
    tree_kinds: HashMap<String, Option<Arc<TreeKind>>>,
    cstat_docs: Vec<(String, (f32, f32))>,
    next_id: u64,
}

fn sector_of(document: &str) -> Option<(f32, f32)> {
    // ..._x85000y0z-15000_cstat.node
    let at = document.rfind("_x")?;
    let rest = &document[at + 2..];
    let (x, rest) = rest.split_once('y')?;
    let (_, rest) = rest.split_once('z')?;
    let z: String = rest
        .chars()
        .take_while(|c| *c == '-' || c.is_ascii_digit())
        .collect();
    Some((x.parse().ok()?, z.parse().ok()?))
}

pub fn tile_of(p: Vec3) -> TileKey {
    (
        (p.x / TILE_CM).floor() as i32,
        (p.z / TILE_CM).floor() as i32,
    )
}

fn flip() -> Mat4 {
    Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0))
}

fn stem_of(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.rsplit_once('.')
        .map_or(name, |(s, _)| s)
        .to_ascii_lowercase()
}

/// xorshift64* generator; tiles seed it from their key so scattering is deterministic.
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1)
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
}

impl Loader {
    fn new(data_dir: &Path, origin: Origin) -> Result<Self> {
        let start = std::time::Instant::now();
        let mut vfs = VirtualFileSystem::open_data_dir(data_dir).context("open Data directory")?;
        let index = WorldIndex::build(&mut vfs).context("build world index")?;
        let mut cells = Vec::new();
        let mut seen = HashSet::new();
        for r in index.records() {
            if let Some((_, number, ext)) = parse_cell_name(&r.resource)
                && ext == "xcmsh"
                && seen.insert((r.resource.to_ascii_lowercase(), number))
                && let Some(sector) = sector_of(&r.document)
            {
                cells.push(CellInfo {
                    number,
                    mesh: r.resource.to_string(),
                    sector,
                });
            }
        }
        let cstat_docs = vfs
            .iter()
            .filter(|(p, _)| p.ends_with("_cstat.node"))
            .filter_map(|(p, _)| Some((p.to_string(), sector_of(p)?)))
            .collect();
        Ok(Self {
            vfs,
            index,
            origin,
            startup_ms: start.elapsed().as_millis(),
            cells,
            cells_loaded: HashSet::new(),
            nav_loaded: HashSet::new(),
            terrain: HashMap::new(),
            tile_cells: HashMap::new(),
            nav_tiles: HashMap::new(),
            meshes: HashMap::new(),
            materials: HashMap::new(),
            textures: HashMap::new(),
            collision_meshes: HashMap::new(),
            tree_docs: HashMap::new(),
            veg_docs: HashMap::new(),
            tree_kinds: HashMap::new(),
            cstat_docs,
            next_id: 1,
        })
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Gothic-space centre of a tile.
    fn tile_center_gothic(&self, key: TileKey) -> Vec3 {
        let b = Vec3::new(
            (key.0 as f32 + 0.5) * TILE_CM,
            0.0,
            (key.1 as f32 + 0.5) * TILE_CM,
        );
        Vec3::new(self.origin.0.x - b.x, 0.0, b.z + self.origin.0.z)
    }

    fn ensure_cells_near(&mut self, center: Vec3) {
        // The sector a cell is listed in is only near its geometry (cells are ~20 000 wide).
        let wanted: Vec<usize> = self
            .cells
            .iter()
            .enumerate()
            .filter(|(i, c)| {
                !self.cells_loaded.contains(i)
                    && (c.sector.0 - center.x).abs() <= 30_000.0
                    && (c.sector.1 - center.z).abs() <= 30_000.0
            })
            .map(|(i, _)| i)
            .collect();
        for i in wanted {
            self.cells_loaded.insert(i);
            if let Err(e) = self.bin_cell(i) {
                eprintln!("terrain cell {}: {e:#}", self.cells[i].mesh);
            }
        }
    }

    fn bin_cell(&mut self, cell: usize) -> Result<()> {
        let slot = self
            .vfs
            .resolve_name(&self.cells[cell].mesh)
            .context("cell mesh not found")?;
        let mesh = Xcmsh::parse(&self.vfs.read(slot)?)?;
        // Cell entities have an identity world matrix (TERRAIN_FORMAT.md).
        let origin = self.origin;
        let mut per_tile: HashMap<(TileKey, usize), TerrainSub> = HashMap::new();
        for (si, sub) in mesh.submeshes.iter().enumerate() {
            let n = sub.positions.len();
            let world: Vec<Vec3> = sub
                .positions
                .iter()
                .map(|p| origin.point(Vec3::from(*p)))
                .collect();
            let mut remap: HashMap<(TileKey, u32), u32> = HashMap::new();
            for t in sub.indices.as_chunks::<3>().0 {
                if t.iter().any(|&i| i as usize >= n) {
                    continue;
                }
                let c = (world[t[0] as usize] + world[t[1] as usize] + world[t[2] as usize]) / 3.0;
                let key = tile_of(c);
                let entry = per_tile.entry((key, si)).or_insert_with(|| TerrainSub {
                    material: sub.material.clone(),
                    positions: vec![],
                    normals: vec![],
                    uvs: vec![],
                    colors: vec![],
                    tangents: vec![],
                    indices: vec![],
                });
                for &i in t {
                    let out = *remap.entry((key, i)).or_insert_with(|| {
                        let k = i as usize;
                        let o = entry.positions.len() as u32;
                        entry.positions.push(world[k].to_array());
                        let nrm = sub.normals.get(k).copied().unwrap_or([0.0, 1.0, 0.0]);
                        entry.normals.push([-nrm[0], nrm[1], nrm[2]]);
                        entry
                            .uvs
                            .push(sub.uvs.get(k).copied().unwrap_or([0.0, 0.0]));
                        entry.colors.push(vertex_color(sub, k));
                        entry.tangents.push(vertex_tangent(sub, k));
                        o
                    });
                    entry.indices.push(out);
                }
            }
        }
        for ((key, _), sub) in per_tile {
            self.terrain.entry(key).or_default().push(sub);
            self.tile_cells.entry(key).or_default().insert(cell);
        }
        Ok(())
    }

    fn bin_nav(&mut self, cell: usize) -> Result<()> {
        let name = self.cells[cell]
            .mesh
            .to_ascii_lowercase()
            .replace(".xcmsh", ".xnvmsh");
        let slot = self.vfs.resolve_name(&name).context("xnvmsh not found")?;
        let nav = Xnvmsh::parse(&self.vfs.read(slot)?)?;
        for t in nav.triangles_cm() {
            let v = t.map(|p| self.origin.point(Vec3::from(p)));
            let c = (v[0] + v[1] + v[2]) / 3.0;
            self.nav_tiles.entry(tile_of(c)).or_default().push(v);
        }
        Ok(())
    }

    fn texture(&mut self, stem: &str) -> Option<Arc<TexData>> {
        let key = format!("{}.ximg", stem.to_ascii_lowercase());
        if let Some(t) = self.textures.get(&key) {
            return t.clone();
        }
        let remastered = gothic_formats::remaster::Profile::global()
            .and_then(|p| p.texture(stem))
            .map(|t| (t.width, t.height, t.fourcc, t.mips));
        let loaded = remastered.or_else(|| {
            let slot = self.vfs.resolve_name(&key)?;
            let bytes = self.vfs.read(slot).ok()?;
            let img = Ximg::parse(&bytes).ok()?;
            Some((
                img.width,
                img.height,
                img.fourcc,
                img.mips().iter().map(|m| m.to_vec()).collect::<Vec<_>>(),
            ))
        });
        let id = self.id();
        let tex = loaded.map(|(width, height, fourcc, mips)| {
            Arc::new(TexData {
                id,
                width,
                height,
                fourcc,
                mips,
            })
        });
        self.textures.insert(key, tex.clone());
        tex
    }

    fn texture_path(&mut self, path: &str) -> Option<Arc<TexData>> {
        self.texture(&stem_of(path))
    }

    fn material(&mut self, name: &str) -> Option<Arc<MatData>> {
        let key = name.to_ascii_lowercase();
        if let Some(m) = self.materials.get(&key) {
            return m.clone();
        }
        let bytes = self
            .vfs
            .resolve_name(&key)
            .and_then(|slot| self.vfs.read(slot).ok());
        let mat = bytes.and_then(|bytes| {
            let x = Xshmat::parse(&bytes).ok()?;
            let recipe = MaterialGraph::parse(&bytes).ok().and_then(|g| g.surface());
            let id = self.id();
            let mut layers = Vec::new();
            let mut layer_names = Vec::new();
            let mut normals: [Option<(Arc<TexData>, [f32; 2])>; 2] = [None, None];
            let mut vertex_blend = false;
            let mut anim: [gothic_formats::xshmat::graph::UvAnim; 2] = Default::default();
            if let Some(r) = &recipe {
                vertex_blend = r.vertex_blend;
                anim[0] = r
                    .layers
                    .first()
                    .and_then(|l| l.textures.iter().find(|t| !t.anim.is_empty()))
                    .map(|t| t.anim.clone())
                    .unwrap_or_default();
                anim[1] = r
                    .normals
                    .first()
                    .and_then(|n| n.as_ref())
                    .map(|t| t.anim.clone())
                    .unwrap_or_default();
                for l in &r.layers {
                    let mut tex: [Option<Arc<TexData>>; 2] = [None, None];
                    let mut scale = [[1.0f32; 2]; 2];
                    for (i, t) in l.textures.iter().take(2).enumerate() {
                        tex[i] = self.texture_path(&t.path);
                        scale[i] = remaster_scale(&t.path, t.scale);
                    }
                    layer_names.push(
                        l.textures
                            .iter()
                            .map(|t| t.path.to_ascii_lowercase())
                            .collect::<Vec<_>>()
                            .join("|"),
                    );
                    layers.push(LayerData {
                        tex,
                        scale,
                        tint: l.tint,
                    });
                }
                for (i, n) in r.normals.iter().take(2).enumerate() {
                    if let Some(t) = n
                        && let Some(tex) = self.texture_path(&t.path)
                    {
                        normals[i] = Some((tex, remaster_scale(&t.path, t.scale)));
                    }
                }
            } else if let Some(d) = x.texture(TextureRole::Diffuse) {
                layers.push(LayerData {
                    tex: [self.texture(d.stem()), None],
                    scale: [[1.0; 2]; 2],
                    tint: [1.0; 3],
                });
                layer_names.push(d.path.to_ascii_lowercase());
                if let Some(n) = x.texture(TextureRole::Normal)
                    && let Some(tex) = self.texture(n.stem())
                {
                    normals[0] = Some((tex, [1.0, 1.0]));
                }
            }
            let lname = key.as_str();
            let water = layers.is_empty() && (lname.contains("water") || x.water_shader);
            let water_depth = water
                && x.water_shader
                && x.diffuse().is_none()
                && x.texture(TextureRole::Normal).is_none();
            if water {
                let mut found = x
                    .textures
                    .iter()
                    .filter(|t| t.role == TextureRole::Normal)
                    .map(|t| t.stem().to_string())
                    .collect::<Vec<_>>();
                found.dedup();
                for (i, stem) in found.iter().take(2).enumerate() {
                    if let Some(tex) = self.texture(stem) {
                        normals[i] = Some((tex, [1.0, 1.0]));
                    }
                }
            }
            if layers.is_empty() && !water {
                return None;
            }
            layers.retain(|l| l.tex[0].is_some());
            if layers.len() < 2 {
                vertex_blend = false;
            }
            Some(Arc::new(MatData {
                id,
                name: key.clone(),
                kind: if water_depth {
                    MatKind::WaterUntextured
                } else if water {
                    MatKind::Water
                } else {
                    MatKind::Surface
                },
                blend_mode: x.blend_mode,
                mask_reference: x.mask_reference,
                layers,
                vertex_blend,
                normals,
                layer_names,
                anim,
                wind_height: 0.0,
            }))
        });
        self.materials.insert(key, mat.clone());
        mat
    }

    /// Resolve an `.xcmsh` (or `.xlmsh` level 0) resource into shared mirrored geometry.
    fn mesh(&mut self, resource: &str, depth: u8) -> Option<Arc<MeshEntry>> {
        let key = resource.rsplit(['/', '\\']).next()?.to_ascii_lowercase();
        if let Some(m) = self.meshes.get(&key) {
            return m.clone();
        }
        let entry = self.mesh_uncached(&key, depth);
        self.meshes.insert(key, entry.clone());
        entry
    }

    fn mesh_uncached(&mut self, key: &str, depth: u8) -> Option<Arc<MeshEntry>> {
        if let Some(entry) = self.remaster_mesh(key) {
            return Some(entry);
        }
        let slot = self.vfs.resolve_name(key)?;
        let bytes = self.vfs.read(slot).ok()?;
        if key.ends_with(".xlmsh") {
            if depth > 0 {
                return None;
            }
            let lod = Xlmsh::parse(&bytes).ok()?;
            let level = lod.levels.iter().find(|l| l.source_index == 0)?;
            let first = self.mesh(&level.mesh.clone(), depth + 1)?;
            // Coarser stages: only worth the extra entities for meshes with real geometry.
            let min_tris: usize = std::env::var("GR_LODMINTRI")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1500);
            if std::env::var_os("GR_NOLOD").is_some()
                || lod.levels.len() < 2
                || first.triangles() < min_tris
            {
                return Some(first);
            }
            let mut entry = MeshEntry::new(
                first.parts.clone(),
                first.sub_index.clone(),
                first.stem.clone(),
            );
            for l in lod.levels.iter().filter(|l| l.source_index > 0).take(4) {
                if let Some(coarse) = self.mesh(&l.mesh.clone(), depth + 1) {
                    entry.lods.push(coarse);
                }
            }
            return Some(Arc::new(entry));
        }
        let mesh = Xcmsh::parse(&bytes).ok()?;
        let mut parts = Vec::new();
        let mut sub_index = Vec::new();
        for (si, sub) in mesh.submeshes.iter().enumerate() {
            if sub.positions.is_empty() || sub.indices.is_empty() {
                continue;
            }
            sub_index.push(si);
            let n = sub.positions.len();
            let positions: Vec<[f32; 3]> =
                sub.positions.iter().map(|p| [-p[0], p[1], p[2]]).collect();
            let normals: Vec<[f32; 3]> = if sub.normals.len() == n {
                sub.normals.iter().map(|n| [-n[0], n[1], n[2]]).collect()
            } else {
                vec![[0.0, 1.0, 0.0]; n]
            };
            let uvs = if sub.uvs.len() == n {
                sub.uvs.clone()
            } else {
                vec![[0.0, 0.0]; n]
            };
            let geom = Arc::new(GeomData {
                id: self.id(),
                positions,
                normals,
                uvs,
                indices: sub.indices.clone(),
                colors: (0..n).map(|k| vertex_color(sub, k)).collect(),
                tangents: (0..n).map(|k| vertex_tangent(sub, k)).collect(),
                light: vec![],
            });
            let material = self.material(&sub.material);
            parts.push((geom, material));
        }
        let stem = key.rsplit_once('.').map_or(key, |(s, _)| s).to_string();
        Some(Arc::new(MeshEntry::new(parts, sub_index, stem)))
    }

    /// Replacement geometry from the local remaster profile (`GR_REMASTER`, see
    /// `gothic_formats::remaster`), LOD 0, mirrored like original meshes. Submeshes with their
    /// own textures get a plain one-layer material; the others inherit the named original one.
    fn remaster_mesh(&mut self, key: &str) -> Option<Arc<MeshEntry>> {
        use gothic_formats::remaster::{OverrideKind, Profile};
        let ov = Profile::global()?.mesh(OverrideKind::Mesh, key)?;
        let mut parts = Vec::new();
        for sub in &ov.submeshes {
            let n = sub.positions.len();
            let Some(indices) = sub.lods.first().filter(|l| !l.is_empty()) else {
                continue;
            };
            let geom = Arc::new(GeomData {
                id: self.id(),
                positions: sub.positions.iter().map(|p| [-p[0], p[1], p[2]]).collect(),
                normals: sub.normals.iter().map(|v| [-v[0], v[1], v[2]]).collect(),
                uvs: sub.uvs.clone(),
                indices: indices.clone(),
                colors: vec![[1.0; 4]; n],
                tangents: sub
                    .tangents
                    .iter()
                    .map(|t| [-t[0], t[1], t[2], -t[3]])
                    .collect(),
                light: vec![],
            });
            let material = if sub.diffuse.is_empty() {
                self.material(&sub.material)
            } else {
                let diffuse = self.texture(&sub.diffuse);
                let normal = (!sub.normal.is_empty())
                    .then(|| self.texture(&sub.normal))
                    .flatten();
                let id = self.id();
                diffuse.map(|d| {
                    Arc::new(MatData {
                        id,
                        name: format!("remaster:{}", sub.diffuse),
                        kind: MatKind::Surface,
                        blend_mode: (sub.blend == 1).then_some(1),
                        mask_reference: None,
                        layers: vec![LayerData {
                            tex: [Some(d), None],
                            scale: [[1.0; 2]; 2],
                            tint: [1.0; 3],
                        }],
                        vertex_blend: false,
                        normals: [normal.map(|t| (t, [1.0, 1.0])), None],
                        layer_names: vec![sub.diffuse.to_ascii_lowercase()],
                        anim: Default::default(),
                        wind_height: 0.0,
                    })
                })
            };
            parts.push((geom, material));
        }
        if parts.is_empty() {
            return None;
        }
        let stem = key.rsplit_once('.').map_or(key, |(s, _)| s).to_string();
        // No original lightmap block matches the new geometry.
        let sub_index = vec![usize::MAX; parts.len()];
        Some(Arc::new(MeshEntry::new(parts, sub_index, stem)))
    }

    /// Per part: a copy of the geometry carrying the baked static-light colour of this placed
    /// instance (`Lightmaps.pak`, matched by mesh stem + entity GUID), for vertices that receive
    /// light. `None` entries (and an empty result) keep the shared geometry.
    fn instance_light(&mut self, mesh: &MeshEntry, entity_id: &str) -> Vec<Option<Arc<GeomData>>> {
        if std::env::var_os("GR_NOLM").is_some() || entity_id.len() < 32 {
            return Vec::new();
        }
        let id: Vec<u8> = (0..16)
            .filter_map(|i| u8::from_str_radix(&entity_id[i * 2..i * 2 + 2], 16).ok())
            .collect();
        let Some(name) = gothic_formats::xlmp::lightmap_file_name(&mesh.stem, &id) else {
            return Vec::new();
        };
        let Some(slot) = self.vfs.resolve_name(&name) else {
            return Vec::new();
        };
        let Ok(bytes) = self.vfs.read(slot) else {
            return Vec::new();
        };
        let Ok(lm) = gothic_formats::xlmp::Xlmp::parse_opts(&bytes, false) else {
            return Vec::new();
        };
        let mut out = vec![None; mesh.parts.len()];
        for (pi, (geom, _)) in mesh.parts.iter().enumerate() {
            let Some(block) = mesh.sub_index.get(pi).and_then(|&si| lm.blocks.get(si)) else {
                continue;
            };
            if block.light.len() != geom.positions.len() || !block.has_vertex_light() {
                continue;
            }
            let light = (0..block.light.len())
                .map(|k| {
                    let [r, _, b] = block.light_rgb(k).unwrap_or_default();
                    [r, b]
                })
                .collect();
            out[pi] = Some(Arc::new(GeomData {
                id: self.id(),
                positions: geom.positions.clone(),
                normals: geom.normals.clone(),
                uvs: geom.uvs.clone(),
                indices: geom.indices.clone(),
                colors: geom.colors.clone(),
                tangents: geom.tangents.clone(),
                light,
            }));
        }
        out
    }

    /// The game's own collision mesh for a render resource (`<stem>_COL.xnvmsh`), as mirrored
    /// local-space triangles in centimetres.
    fn collision_mesh(&mut self, resource: &str, depth: u8) -> Option<Arc<Vec<[Vec3; 3]>>> {
        let base = resource.rsplit(['/', '\\']).next()?.to_ascii_lowercase();
        let stem = base.rsplit_once('.').map_or(base.as_str(), |(s, _)| s);
        let stem = stem.to_string();
        if let Some(c) = self.collision_meshes.get(&stem) {
            return c.clone();
        }
        if let Some(ov) = gothic_formats::remaster::Profile::global()
            .and_then(|p| p.mesh(gothic_formats::remaster::OverrideKind::Mesh, &base))
            .filter(|m| !m.collision.is_empty())
        {
            let tris: Vec<[Vec3; 3]> = ov
                .collision
                .iter()
                .map(|t| t.map(|p| Vec3::new(-p[0], p[1], p[2])))
                .collect();
            let result = Some(Arc::new(tris));
            self.collision_meshes.insert(stem, result.clone());
            return result;
        }
        let mut result = None;
        let mut candidates = vec![format!("{stem}_col.xnvmsh")];
        if let Some(s) = stem.strip_suffix("_lowpoly") {
            candidates.push(format!("{s}_col.xnvmsh"));
        }
        for c in candidates {
            if let Some(slot) = self.vfs.resolve_name(&c)
                && let Ok(bytes) = self.vfs.read(slot)
                && let Ok(nav) = Xnvmsh::parse(&bytes)
            {
                let tris: Vec<[Vec3; 3]> = nav
                    .triangles_cm()
                    .into_iter()
                    .map(|t| t.map(|p| Vec3::new(-p[0], p[1], p[2])))
                    .collect();
                if !tris.is_empty() {
                    result = Some(Arc::new(tris));
                    break;
                }
            }
        }
        if result.is_none() && base.ends_with(".xlmsh") && depth == 0 {
            // LOD container: use the collision of its first level.
            if let Some(slot) = self.vfs.resolve_name(&base)
                && let Ok(bytes) = self.vfs.read(slot)
                && let Ok(lod) = Xlmsh::parse(&bytes)
                && let Some(level) = lod.levels.iter().find(|l| l.source_index == 0)
            {
                result = self.collision_mesh(&level.mesh.clone(), depth + 1);
            }
        }
        self.collision_meshes.insert(stem, result.clone());
        result
    }

    fn load_tile(&mut self, key: TileKey) -> Result<TilePayload> {
        let center = self.tile_center_gothic(key);
        self.ensure_cells_near(center);
        let mut parts = Vec::new();
        let mut collision: Vec<[Vec3; 3]> = Vec::new();
        let mut stats = TileStats::default();

        // Terrain render tiles.
        let subs = self.terrain.get(&key).map(|v| v.len()).unwrap_or(0);
        let mut render_tris: Vec<[Vec3; 3]> = Vec::new();
        for i in 0..subs {
            let (mat_name, geom) = {
                let s = &self.terrain[&key][i];
                for t in s.indices.as_chunks::<3>().0 {
                    render_tris.push([0, 1, 2].map(|k| Vec3::from(s.positions[t[k] as usize])));
                }
                (
                    s.material.clone(),
                    (
                        s.positions.clone(),
                        s.normals.clone(),
                        s.uvs.clone(),
                        s.colors.clone(),
                        s.tangents.clone(),
                        s.indices.clone(),
                    ),
                )
            };
            let material = self.material(&mat_name);
            parts.push(Part {
                lod: None,
                source: None,
                ocean_plane_y_m: None,
                geom: Arc::new(GeomData {
                    id: self.id(),
                    positions: geom.0,
                    normals: geom.1,
                    uvs: geom.2,
                    colors: geom.3,
                    tangents: geom.4,
                    light: vec![],
                    indices: geom.5,
                }),
                material,
                transform: Mat4::IDENTITY,
                kind: PartKind::Terrain,
            });
        }

        // Real collision mesh for the terrain (xnvmsh), lazily per source cell.
        let cells: Vec<usize> = self
            .tile_cells
            .get(&key)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default();
        for c in cells {
            if self.nav_loaded.insert(c)
                && let Err(e) = self.bin_nav(c)
            {
                eprintln!("collision cell {}: {e:#}", self.cells[c].mesh);
            }
        }
        let from_nav = if let Some(nav) = self.nav_tiles.get(&key) {
            collision.extend_from_slice(nav);
            true
        } else {
            collision.extend(render_tris);
            false
        };

        // Static scenery: entities whose origin falls in this tile.
        let radius = TILE_CM * 0.75;
        let hits: Vec<(String, [[f32; 4]; 4], String)> = self
            .index
            .query_xz([center.x, 0.0, center.z], radius)
            .into_iter()
            .filter(|r| parse_cell_name(&r.resource).is_none())
            .map(|r| (r.resource.to_string(), r.world_matrix, r.entity_id.clone()))
            .collect();
        let f = flip();
        let origin = self.origin;
        for (resource, matrix, entity_id) in hits {
            let m = Mat4::from_cols_array_2d(&matrix);
            if !m.is_finite() {
                continue;
            }
            let t = m.w_axis.truncate();
            if tile_of(origin.point(t)) != key {
                continue;
            }
            let Some(mesh) = self.mesh(&resource, 0) else {
                continue;
            };
            let river_depth_proxy = is_river_depth_resource(&resource)
                && mesh.parts.iter().any(|(_, material)| {
                    material
                        .as_ref()
                        .is_some_and(|m| m.kind == MatKind::WaterUntextured)
                });
            let source = mesh
                .parts
                .iter()
                .any(|(_, material)| material.as_ref().is_some_and(|m| m.kind.is_water()))
                .then(|| {
                    Arc::new(PartSource {
                        resource: Arc::from(resource.as_str()),
                        entity_id: Arc::from(entity_id.as_str()),
                    })
                });
            let transform =
                Mat4::from_translation(Vec3::new(origin.0.x, -origin.0.y, -origin.0.z)) * f * m * f;
            // Translation of F*M*F is (-tx, ty, tz); the origin shift above makes it
            // origin-relative: (ox - tx, ty - oy, tz - oz).
            let mut any_solid = false;
            let mut water_only = true;
            let lit = self.instance_light(&mesh, &entity_id);
            let stages = (1 + mesh.lods.len()).min(5) as u8;
            let lod_info = |stage: u8| {
                (stages > 1).then(|| {
                    let sc = transform.to_scale_rotation_translation().0;
                    LodPart {
                        stage,
                        stages,
                        radius_cm: mesh.radius * sc.abs().max_element(),
                    }
                })
            };
            for (pi, (geom, material)) in mesh.parts.iter().enumerate() {
                let geom = lit.get(pi).and_then(Option::as_ref).unwrap_or(geom);
                // This depth-hull source is proxy geometry, not visible water geometry.
                if river_depth_proxy
                    && material
                        .as_ref()
                        .is_some_and(|m| m.kind == MatKind::WaterUntextured)
                {
                    continue;
                }
                // Distant-LOD proxy submeshes (low-poly world atlas) overlap the real geometry.
                if material.as_ref().is_some_and(|m| {
                    m.name.contains("world_lowpoly")
                        || m.layer_names.iter().any(|n| n.contains("world_lowpoly"))
                }) {
                    continue;
                }
                let is_water = material.as_ref().is_some_and(|m| m.kind.is_water());
                let solid = !is_water
                    && material
                        .as_ref()
                        .is_none_or(|m| !matches!(m.blend_mode, Some(2 | 7 | 8)));
                any_solid |= solid;
                water_only &= is_water;
                let ocean_plane_y_m = if is_water
                    && source.as_ref().is_some_and(|s| is_confirmed_ocean_resource(&s.resource))
                {
                    planar_ocean_y_m(&geom.positions, transform)
                } else {
                    None
                };
                parts.push(Part {
                    lod: if is_water { None } else { lod_info(0) },
                    source: if is_water { source.clone() } else { None },
                    ocean_plane_y_m,
                    geom: geom.clone(),
                    material: material.clone(),
                    transform,
                    kind: if is_water {
                        PartKind::Water
                    } else {
                        PartKind::Static
                    },
                });
            }
            // Coarser stages, switched by the area factor rule (distance bands in the viewer).
            for (li, coarse) in mesh.lods.iter().enumerate().take(4) {
                let stage = li as u8 + 1;
                for (pi, (geom, material)) in coarse.parts.iter().enumerate() {
                    if material.as_ref().is_some_and(|m| {
                        m.kind.is_water()
                            || m.name.contains("world_lowpoly")
                            || m.layer_names.iter().any(|n| n.contains("world_lowpoly"))
                    }) {
                        continue;
                    }
                    let geom = match lit.get(pi).and_then(Option::as_ref) {
                        Some(fine) => Arc::new(transfer_light(geom, fine)),
                        None => geom.clone(),
                    };
                    parts.push(Part {
                        lod: lod_info(stage),
                        source: None,
                        ocean_plane_y_m: None,
                        geom,
                        material: material.clone(),
                        transform,
                        kind: PartKind::Static,
                    });
                }
            }
            if water_only {
                continue;
            }
            stats.statics += 1;
            if !river_depth_proxy {
                if let Some(col) = self.collision_mesh(&resource, 0) {
                    stats.statics_with_collision_mesh += 1;
                    collision.extend(
                        col.iter()
                            .map(|tri| tri.map(|p| transform.transform_point3(p))),
                    );
                } else if any_solid && architectural(&resource) {
                    // No authored collision mesh: large architecture falls back to its render mesh.
                    for (geom, material) in &mesh.parts {
                        let solid = material
                            .as_ref()
                            .is_none_or(|m| m.kind == MatKind::Surface && m.blend_mode != Some(2));
                        if solid && geom.indices.len() <= 40_000 {
                            for tri in geom.indices.as_chunks::<3>().0 {
                                collision.push([0, 1, 2].map(|k| {
                                    transform
                                        .transform_point3(Vec3::from(geom.positions[tri[k] as usize]))
                                }));
                            }
                        }
                    }
                }
            }
        }

        // SpeedTree placements.
        self.add_trees(key, center, &mut parts, &mut collision, &mut stats);
        let next_id = self.next_id;
        self.next_id += merge_statics(&mut parts, next_id + 1);

        Ok(TilePayload {
            key,
            parts,
            collision,
            terrain_collision_from_nav: from_nav,
            stats,
        })
    }

    fn add_trees(
        &mut self,
        key: TileKey,
        center: Vec3,
        parts: &mut Vec<Part>,
        collision: &mut Vec<[Vec3; 3]>,
        stats: &mut TileStats,
    ) {
        let docs: Vec<String> = self
            .cstat_docs
            .iter()
            .filter(|(_, s)| {
                (s.0 - center.x).abs() <= 5000.0 + TILE_CM
                    && (s.1 - center.z).abs() <= 5000.0 + TILE_CM
            })
            .map(|(p, _)| p.clone())
            .collect();
        let origin = self.origin;
        let f = flip();
        for doc in docs {
            if !self.tree_docs.contains_key(&doc) {
                let list = self
                    .vfs
                    .resolve(&doc)
                    .and_then(|slot| self.vfs.read(slot).ok())
                    .and_then(|bytes| vegetation::extract(&bytes).ok())
                    .map(|v| v.trees)
                    .unwrap_or_default();
                self.tree_docs.insert(doc.clone(), list);
            }
            let list = self.tree_docs[&doc].clone();
            for tree in list {
                let m = Mat4::from_cols_array_2d(&tree.matrix);
                if !m.is_finite() {
                    continue;
                }
                let t = m.w_axis.truncate();
                if tile_of(origin.point(t)) != key {
                    continue;
                }
                let Some(kind) = self.tree_kind(&tree.spt) else {
                    continue;
                };
                // The world matrix carries the per-instance scale; the species size is baked in.
                let transform =
                    Mat4::from_translation(Vec3::new(origin.0.x, -origin.0.y, -origin.0.z))
                        * f
                        * m
                        * f;
                stats.trees += 1;
                if std::env::var_os("GR_TREE_LOG").is_some() {
                    let (sc, _, ps) = transform.to_scale_rotation_translation();
                    eprintln!(
                        "TREE {} scale {:.2} pos {:.0} {:.0} {:.0}",
                        tree.spt, sc.y, ps.x, ps.y, ps.z
                    );
                }
                let variant = &kind.variants[(trees::hash_str(&format!("{:.0}{:.0}", t.x, t.z))
                    % kind.variants.len() as u64)
                    as usize];
                if let Some(g) = &variant.bark {
                    parts.push(Part {
                        lod: None,
                        source: None,
                        ocean_plane_y_m: None,
                        geom: g.clone(),
                        material: Some(kind.bark_mat.clone()),
                        transform,
                        kind: PartKind::TreeBark,
                    });
                }
                if let Some(g) = &variant.cards {
                    parts.push(Part {
                        lod: None,
                        source: None,
                        ocean_plane_y_m: None,
                        geom: g.clone(),
                        material: Some(kind.card_mat.clone()),
                        transform,
                        kind: PartKind::Tree,
                    });
                }
                if let Some(g) = &kind.far_bark {
                    parts.push(Part {
                        lod: None,
                        source: None,
                        ocean_plane_y_m: None,
                        geom: g.clone(),
                        material: Some(kind.bark_mat.clone()),
                        transform,
                        kind: PartKind::TreeFarBark,
                    });
                }
                parts.push(Part {
                    lod: None,
                    source: None,
                    ocean_plane_y_m: None,
                    geom: kind.far.clone(),
                    material: Some(kind.card_mat.clone()),
                    transform,
                    kind: PartKind::TreeFar,
                });
                // Trunk collision from the .spt cylinder: an eight-sided prism.
                let (scale, _, pos) = transform.to_scale_rotation_translation();
                let r = kind.trunk.0 * scale.x;
                let h = (kind.trunk.1 * scale.y).min(600.0);
                for i in 0..8 {
                    let a0 = i as f32 * std::f32::consts::TAU / 8.0;
                    let a1 = (i + 1) as f32 * std::f32::consts::TAU / 8.0;
                    let p0 = pos + Vec3::new(a0.cos() * r, 0.0, a0.sin() * r);
                    let p1 = pos + Vec3::new(a1.cos() * r, 0.0, a1.sin() * r);
                    let up = Vec3::Y * h;
                    collision.push([p0, p1, p1 + up]);
                    collision.push([p0, p1 + up, p0 + up]);
                }
            }
        }
    }

    fn tree_kind(&mut self, spt_name: &str) -> Option<Arc<TreeKind>> {
        let key = spt_name.to_ascii_lowercase();
        if let Some(k) = self.tree_kinds.get(&key) {
            return k.clone();
        }
        let kind = (|| {
            let slot = self.vfs.resolve_name(&key)?;
            let spt = Spt::parse(&self.vfs.read(slot).ok()?).ok()?;
            let atlas_stem = stem_of(spt.composite.as_ref()?);
            let atlas = self.texture(&atlas_stem)?;
            let bark_tex = self.texture(&stem_of(&spt.bark));
            let mut next = 0u64;
            let species = {
                let mut ids = Vec::new();
                // Reserve ids up front: `build` needs a `FnMut` while `self` is borrowed.
                for _ in 0..(trees::VARIANTS * 2 + 1) {
                    ids.push(self.id());
                }
                let mut it = ids.into_iter();
                trees::build(&spt, &atlas, &mut || {
                    next += 1;
                    it.next().unwrap_or(1_000_000 + next)
                })?
            };
            let mk = |g: GeomData| Arc::new(g);
            let species_height = species.height;
            let variants = species
                .near
                .into_iter()
                .map(|(b, c)| TreeNearVariant {
                    bark: b.map(mk),
                    cards: c.map(mk),
                })
                .collect();
            let id = self.id();
            let card_mat = Arc::new(MatData {
                id,
                name: format!("tree:{key}"),
                kind: MatKind::Surface,
                blend_mode: Some(1),
                mask_reference: Some(110),
                layers: vec![LayerData {
                    tex: [Some(atlas), None],
                    scale: [[1.0; 2]; 2],
                    tint: [
                        TREE_LEAF_TINT * 1.25,
                        TREE_LEAF_TINT * 1.8,
                        TREE_LEAF_TINT * 0.9,
                    ],
                }],
                vertex_blend: false,
                normals: [None, None],
                layer_names: vec![atlas_stem],
                anim: Default::default(),
                wind_height: species_height,
            });
            let id = self.id();
            let bark_mat = Arc::new(MatData {
                id,
                name: format!("treebark:{key}"),
                kind: MatKind::Surface,
                blend_mode: None,
                mask_reference: None,
                layers: vec![LayerData {
                    tex: [bark_tex, None],
                    scale: [[1.0; 2]; 2],
                    tint: [TREE_BARK_TINT; 3],
                }],
                vertex_blend: false,
                normals: [None, None],
                layer_names: Vec::new(),
                anim: Default::default(),
                wind_height: species_height,
            });
            Some(Arc::new(TreeKind {
                variants,
                far_bark: species.far.0.map(Arc::new),
                far: Arc::new(species.far.1),
                bark_mat,
                card_mat,
                trunk: species.trunk,
            }))
        })();
        self.tree_kinds.insert(key, kind.clone());
        kind
    }

    /// `WeatherStates.g3ws` and the `eCWeatherZone_PS` entities of every world document.
    fn load_weather(&mut self) -> WeatherPayload {
        let weather_slot = self
            .vfs
            .iter()
            .find(|(p, _)| p.to_ascii_lowercase().ends_with("weatherstates.g3ws"))
            .map(|(_, s)| s);
        let data = weather_slot
            .and_then(|slot| self.vfs.read(slot).ok())
            .and_then(|b| gothic_formats::weather::parse(&b).ok());
        let slots: Vec<_> = self
            .vfs
            .iter()
            .filter(|(p, _)| {
                let l = p.to_ascii_lowercase();
                l.ends_with(".node") || l.ends_with(".lrentdat")
            })
            .map(|(_, s)| s)
            .collect();
        let mut zones = Vec::new();
        let mut lights = Vec::new();
        let (mut wind, mut precipitation, mut skydome) = (None, None, None);
        for slot in slots {
            let Ok(bytes) = self.vfs.read(slot) else {
                continue;
            };
            // Cheap reject: the class name is stored in the document's string table.
            if !bytes.windows(12).any(|w| {
                w == b"eCWeatherZon"
                    || w == b"eCStaticPoin"
                    || w == b"eCSpeedTreeW"
                    || w == b"eCPrecipitat"
                    || w == b"eCSkydome_PS"
            }) {
                continue;
            }
            let mut stats = gothic_formats::propstream::DecodeStats::default();
            let Ok((doc, sets)) = gothic_formats::world_node::WorldDocument::parse_with_properties(
                &bytes,
                &|c| {
                    matches!(
                        c,
                        "eCWeatherZone_PS"
                            | "eCStaticPointLight_PS"
                            | "eCSpeedTreeWind_PS"
                            | "eCPrecipitation_PS"
                            | "eCSkydome_PS"
                    )
                },
                &mut stats,
            ) else {
                continue;
            };
            for (e, s) in doc.entities.iter().zip(&sets) {
                if let Some(w) = s.iter().find(|p| p.class == "eCSpeedTreeWind_PS") {
                    wind = Some(gothic_environment::wind::SpeedTreeWind::from_properties(w));
                }
                if let Some(w) = s.iter().find(|p| p.class == "eCPrecipitation_PS") {
                    precipitation =
                        Some(gothic_environment::precip::PrecipParams::from_properties(w));
                }
                if let Some(w) = s.iter().find(|p| p.class == "eCSkydome_PS") {
                    skydome = Some(gothic_environment::sky::SkydomeParams::from_properties(w));
                }
                if let Some(l) = s.iter().find(|p| p.class == "eCStaticPointLight_PS") {
                    let m = Mat4::from_cols_array_2d(&e.world_matrix);
                    let off = match l.get("Offset") {
                        Some(gothic_formats::propstream::PropValue::Vec3(v)) => Vec3::from(*v),
                        _ => Vec3::ZERO,
                    };
                    let color = match l.get("Color") {
                        Some(gothic_formats::propstream::PropValue::ColorRgb(c)) => *c,
                        _ => [1.0; 3],
                    };
                    let range = l.float("Range").unwrap_or(0.0);
                    if m.is_finite() && range > 0.0 {
                        lights.push(StaticLightRaw {
                            position: m.transform_point3(off),
                            color,
                            range,
                        });
                    }
                }
                let Some(set) = s.iter().find(|p| p.class == "eCWeatherZone_PS") else {
                    continue;
                };
                let Some(zone) = gothic_environment::Zone::from_properties(set) else {
                    continue;
                };
                let m = Mat4::from_cols_array_2d(&e.world_matrix);
                if !m.is_finite() || m.determinant().abs() < 1e-12 {
                    continue;
                }
                // The serialized matrix is row-vector: `local = world * M^-1` in row form, which is
                // `M^-1` applied as a column matrix after transposition handled by from_cols.
                zones.push(ZoneInstance {
                    zone,
                    world_to_local: m.inverse(),
                });
            }
        }
        WeatherPayload {
            data,
            zones,
            lights,
            wind,
            precipitation,
            skydome,
        }
    }

    /// Authored ground cover of a tile: the instances stored in the `eCVegetation_PS` grids of the
    /// world documents (position, rotation, scale and colour per instance), instanced from the
    /// brush meshes of the same area.
    fn load_vegetation(&mut self, key: TileKey) -> VegPayload {
        if std::env::var_os("GR_VEG_SCATTER").is_some() {
            return self.load_vegetation_scatter(key);
        }
        let center = self.tile_center_gothic(key);
        let docs: Vec<String> = self
            .cstat_docs
            .iter()
            .filter(|(_, s)| {
                (s.0 - center.x).abs() <= 5000.0 + TILE_CM
                    && (s.1 - center.z).abs() <= 5000.0 + TILE_CM
            })
            .map(|(p, _)| p.clone())
            .collect();
        let origin = self.origin;
        let mut accum: HashMap<(String, usize), (Arc<MatData>, GeomData)> = HashMap::new();
        let mut instances = 0usize;
        for doc in docs {
            if !self.veg_docs.contains_key(&doc) {
                let list = self
                    .vfs
                    .resolve(&doc)
                    .and_then(|slot| self.vfs.read(slot).ok())
                    .and_then(|bytes| vegetation::extract(&bytes).ok())
                    .map(|v| {
                        let mut out = Vec::new();
                        for area in &v.areas {
                            let Some(grid) = &area.grid else { continue };
                            let names: HashMap<u16, &str> = area
                                .meshes
                                .iter()
                                .map(|m| (m.mesh_id, m.mesh.as_str()))
                                .collect();
                            let wind_of: HashMap<u16, f32> = area
                                .meshes
                                .iter()
                                .map(|m| (m.mesh_id, m.wind_strength))
                                .collect();
                            for e in &grid.entries {
                                let Some(name) = names.get(&e.mesh_id) else {
                                    continue;
                                };
                                out.push(VegInstance {
                                    mesh: (*name).to_string(),
                                    position: Vec3::from(e.position),
                                    rotation: Quat::from_xyzw(
                                        e.rotation[0],
                                        e.rotation[1],
                                        e.rotation[2],
                                        e.rotation[3],
                                    ),
                                    scale: e.scale,
                                    color: e.color,
                                    wind: wind_of.get(&e.mesh_id).copied().unwrap_or(0.0),
                                });
                            }
                        }
                        out
                    })
                    .unwrap_or_default();
                self.veg_docs.insert(doc.clone(), Arc::new(list));
            }
            let list = self.veg_docs[&doc].clone();
            for inst in list.iter() {
                if tile_of(origin.point(inst.position)) != key {
                    continue;
                }
                let Some(entry) = self.mesh(&inst.mesh, 0) else {
                    continue;
                };
                // Mirror the left-handed placement into our space: a reflection of x flips the
                // y and z components of the rotation quaternion.
                let q = Quat::from_xyzw(
                    inst.rotation.x,
                    -inst.rotation.y,
                    -inst.rotation.z,
                    inst.rotation.w,
                )
                .normalize();
                // The engine scales (x, y, x): `scale[1]` is the height.
                let scale = Vec3::new(inst.scale[0], inst.scale[1], inst.scale[0]);
                let xf =
                    Mat4::from_scale_rotation_translation(scale, q, origin.point(inst.position));
                let tint = [
                    ((inst.color >> 16) & 255) as f32 / 255.0,
                    ((inst.color >> 8) & 255) as f32 / 255.0,
                    (inst.color & 255) as f32 / 255.0,
                ];
                instances += 1;
                // Wind weight per vertex: `((y - y_min) / height)^2 * WindStrength` over the box of
                // the brush mesh (`eCVegetation_GridNode::CacheInGeometry`).
                let (y_min, y_max) = entry.parts.iter().fold((f32::MAX, f32::MIN), |a, (g, _)| {
                    g.positions
                        .iter()
                        .fold(a, |a, p| (a.0.min(p[1]), a.1.max(p[1])))
                });
                for (pi, (geom, material)) in entry.parts.iter().enumerate() {
                    let Some(material) = material else { continue };
                    let slot = accum.entry((material.name.clone(), pi)).or_insert_with(|| {
                        (
                            material.clone(),
                            GeomData {
                                id: 0,
                                positions: vec![],
                                normals: vec![],
                                uvs: vec![],
                                indices: vec![],
                                colors: vec![],
                                tangents: vec![],
                                light: vec![],
                            },
                        )
                    });
                    let g = &mut slot.1;
                    if g.positions.len() + geom.positions.len() > 3_000_000 {
                        continue;
                    }
                    let base = g.positions.len() as u32;
                    for k in 0..geom.positions.len() {
                        let wp = xf.transform_point3(Vec3::from(geom.positions[k]));
                        g.positions.push(wp.to_array());
                        // Ground cover shades like the ground it stands on: blend toward up.
                        let n = xf.transform_vector3(Vec3::from(geom.normals[k]));
                        let n = (Vec3::Y + n.normalize_or_zero() * 0.1).normalize_or_zero();
                        g.normals.push(n.to_array());
                        g.uvs.push(geom.uvs[k]);
                        let c = geom.colors[k];
                        let weight = gothic_environment::wind::vegetation_weight(
                            geom.positions[k][1],
                            y_min,
                            y_max - y_min,
                            inst.wind,
                        );
                        g.colors
                            .push([c[0] * tint[0], c[1] * tint[1], c[2] * tint[2], weight]);
                        g.tangents.push([1.0, 0.0, 0.0, 1.0]);
                    }
                    g.indices.extend(geom.indices.iter().map(|i| i + base));
                }
            }
        }
        let mut parts = Vec::new();
        for (_, (material, mut geom)) in accum {
            if geom.positions.is_empty() {
                continue;
            }
            geom.id = self.id();
            parts.push(Part {
                lod: None,
                source: None,
                ocean_plane_y_m: None,
                geom: Arc::new(geom),
                material: Some(material),
                transform: Mat4::IDENTITY,
                kind: PartKind::Foliage,
            });
        }
        VegPayload {
            key,
            parts,
            instances,
        }
    }

    /// Guessed ground cover (`GR_VEG_SCATTER=1`): the old terrain-material driven scatter, kept
    /// only to compare against the authored grid.
    fn load_vegetation_scatter(&mut self, key: TileKey) -> VegPayload {
        let center = self.tile_center_gothic(key);
        self.ensure_cells_near(center);
        let mut accum: HashMap<(String, usize), (Arc<MatData>, GeomData)> = HashMap::new();
        let mut instances = 0usize;
        let subs = self.terrain.get(&key).map_or(0, |v| v.len());
        let seed = (u64::from(key.0 as u32) << 32) | u64::from(key.1 as u32);
        let mut rng = Rng::new(seed ^ 0xA5A5_1234);
        for si in 0..subs {
            let (mat_name, tris) = {
                let s = &self.terrain[&key][si];
                let tris: Vec<([Vec3; 3], [f32; 3], [f32; 3])> = s
                    .indices
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|t| {
                        (
                            t.map(|i| Vec3::from(s.positions[i as usize])),
                            t.map(|i| s.colors[i as usize][3]),
                            t.map(|i| s.normals[i as usize][1]),
                        )
                    })
                    .collect();
                (s.material.clone(), tris)
            };
            let Some(mat) = self.material(&mat_name) else {
                continue;
            };
            let cover = ground_cover(&mat);
            if cover.is_empty() {
                continue;
            }
            for (tri, alpha, ny) in tris {
                let area = ((tri[1] - tri[0]).cross(tri[2] - tri[0]).length() * 0.5) * 1.0e-4; // m^2
                let slope = (ny[0] + ny[1] + ny[2]) / 3.0;
                if area < 1e-4 || slope < 0.72 {
                    continue;
                }
                let a = (alpha[0] + alpha[1] + alpha[2]) / 3.0;
                for (brush, layer, density) in &cover {
                    // layer 0 grows where the vertex alpha is low, layer 1 where it is high.
                    let weight = if mat.vertex_blend {
                        if *layer == 1 { a } else { 1.0 - a }
                    } else {
                        1.0
                    };
                    let expected = area * density * weight;
                    let mut n = expected.floor() as usize;
                    if rng.f32() < expected.fract() {
                        n += 1;
                    }
                    for _ in 0..n {
                        let (mut u, mut v) = (rng.f32(), rng.f32());
                        if u + v > 1.0 {
                            u = 1.0 - u;
                            v = 1.0 - v;
                        }
                        let p = tri[0] + (tri[1] - tri[0]) * u + (tri[2] - tri[0]) * v;
                        let yaw = rng.f32() * std::f32::consts::TAU;
                        let scale = 0.75 + rng.f32() * 0.55;
                        let shade = 0.82 + rng.f32() * 0.3;
                        let Some(entry) = self.mesh(brush, 0) else {
                            continue;
                        };
                        instances += 1;
                        let xf = Mat4::from_scale_rotation_translation(
                            Vec3::splat(scale),
                            Quat::from_rotation_y(yaw),
                            p - Vec3::Y * 2.0,
                        );
                        for (pi, (geom, material)) in entry.parts.iter().enumerate() {
                            let Some(material) = material else { continue };
                            let slot =
                                accum.entry((material.name.clone(), pi)).or_insert_with(|| {
                                    (
                                        material.clone(),
                                        GeomData {
                                            id: 0,
                                            positions: vec![],
                                            normals: vec![],
                                            uvs: vec![],
                                            indices: vec![],
                                            colors: vec![],
                                            tangents: vec![],
                                            light: vec![],
                                        },
                                    )
                                });
                            let g = &mut slot.1;
                            if g.positions.len() + geom.positions.len() > 3_000_000 {
                                continue;
                            }
                            let base = g.positions.len() as u32;
                            for k in 0..geom.positions.len() {
                                let wp = xf.transform_point3(Vec3::from(geom.positions[k]));
                                g.positions.push(wp.to_array());
                                let n = xf.transform_vector3(Vec3::from(geom.normals[k]));
                                // Blend toward up so clumps shade like the ground they stand on.
                                let n = Vec3::Y + n.normalize_or_zero() * 0.1;
                                let n = n.normalize_or_zero();
                                g.normals.push(n.to_array());
                                g.uvs.push(geom.uvs[k]);
                                let c = geom.colors[k];
                                let h = (geom.positions[k][1] / 60.0).clamp(0.0, 1.0);
                                g.colors
                                    .push([c[0] * shade, c[1] * shade, c[2] * shade, h * h]);
                                g.tangents.push([1.0, 0.0, 0.0, 1.0]);
                            }
                            g.indices.extend(geom.indices.iter().map(|i| i + base));
                        }
                    }
                }
            }
        }
        let mut parts = Vec::new();
        for (_, (material, mut geom)) in accum {
            if geom.positions.is_empty() {
                continue;
            }
            geom.id = self.id();
            parts.push(Part {
                lod: None,
                source: None,
                ocean_plane_y_m: None,
                geom: Arc::new(geom),
                material: Some(material),
                transform: Mat4::IDENTITY,
                kind: PartKind::Foliage,
            });
        }
        VegPayload {
            key,
            parts,
            instances,
        }
    }
}

/// Baked vertex light of a coarser LoD mesh: every vertex takes the colour of the nearest vertex of
/// the full-detail mesh (the lightmap of an instance is authored for LoD 0 only).
fn transfer_light(coarse: &GeomData, fine: &GeomData) -> GeomData {
    let n = coarse.positions.len();
    if fine.light.len() != fine.positions.len() || fine.positions.is_empty() {
        return GeomData {
            id: coarse.id,
            positions: coarse.positions.clone(),
            normals: coarse.normals.clone(),
            uvs: coarse.uvs.clone(),
            indices: coarse.indices.clone(),
            colors: coarse.colors.clone(),
            tangents: coarse.tangents.clone(),
            light: Vec::new(),
        };
    }
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in &fine.positions {
        lo = lo.min(Vec3::from(*p));
        hi = hi.max(Vec3::from(*p));
    }
    let cell = ((hi - lo).max_element() / 24.0).max(1e-3);
    let key = |p: Vec3| {
        let q = (p - lo) / cell;
        (q.x as i32, q.y as i32, q.z as i32)
    };
    let mut grid: HashMap<(i32, i32, i32), Vec<u32>> = HashMap::new();
    for (i, p) in fine.positions.iter().enumerate() {
        grid.entry(key(Vec3::from(*p))).or_default().push(i as u32);
    }
    let mut light = Vec::with_capacity(n);
    for p in &coarse.positions {
        let p = Vec3::from(*p);
        let (cx, cy, cz) = key(p);
        let mut best = (f32::MAX, 0usize);
        for r in 1..=3 {
            for dx in -r..=r {
                for dy in -r..=r {
                    for dz in -r..=r {
                        if let Some(list) = grid.get(&(cx + dx, cy + dy, cz + dz)) {
                            for &i in list {
                                let d = Vec3::from(fine.positions[i as usize]).distance_squared(p);
                                if d < best.0 {
                                    best = (d, i as usize);
                                }
                            }
                        }
                    }
                }
            }
            if best.0 < f32::MAX {
                break;
            }
        }
        light.push(fine.light[best.1]);
    }
    GeomData {
        id: coarse.id,
        positions: coarse.positions.clone(),
        normals: coarse.normals.clone(),
        uvs: coarse.uvs.clone(),
        indices: coarse.indices.clone(),
        colors: coarse.colors.clone(),
        tangents: coarse.tangents.clone(),
        light,
    }
}

/// Whether an unauthored-collision prop is big architecture that must stay solid.
fn architectural(resource: &str) -> bool {
    let l = resource.to_ascii_lowercase();
    [
        "house", "wall", "palisad", "gate", "bridge", "stairs", "tower", "fence", "ruin", "ship",
        "pier", "dock", "hut", "barn", "mill", "cliff", "rock", "stone",
    ]
    .iter()
    .any(|n| l.contains(n))
}

/// Ground-cover brushes for a terrain material: (brush mesh, layer index, instances per m^2).
///
/// Fallback only (`GR_VEG_SCATTER=1`): the authored instances are stored in the vegetation grid of
/// `eCVegetation_PS` (see `load_vegetation`); this guess is driven by what the terrain material
/// paints: grass layers grow grass, herbs and the odd stone; forest
/// floor grows ferns, underbrush, branches and mushrooms.
fn ground_cover(mat: &MatData) -> Vec<(&'static str, usize, f32)> {
    let mut out = Vec::new();
    for (i, names) in mat.layer_names.iter().enumerate() {
        if names.contains("grass") {
            out.extend([
                ("G3_Brush_Plant_Grass_05.xcmsh", i, 0.06),
                ("G3_Brush_Plant_Grass_06.xcmsh", i, 0.06),
                ("G3_Brush_Plant_Grass_07.xcmsh", i, 0.06),
                ("G3_Brush_Plant_Herb_01.xcmsh", i, 0.03),
                ("G3_Brush_Plant_Herb_02.xcmsh", i, 0.03),
                ("G3_Brush_Plant_Herb_03.xcmsh", i, 0.03),
                ("G3_Brush_Plant_Herb_05.xcmsh", i, 0.03),
                ("G3_Brush_Plant_Herb_07.xcmsh", i, 0.03),
                ("G3_Brush_Plant_Herb_08.xcmsh", i, 0.03),
                ("G3_Brush_Object_Stone_Small_01.xcmsh", i, 0.012),
                ("G3_Brush_Object_Stone_Small_02.xcmsh", i, 0.012),
                ("G3_Brush_Plant_Fern_01.xcmsh", i, 0.015),
            ]);
        } else if ["forest", "leaves", "moss", "pine", "needle", "underbrush"]
            .iter()
            .any(|n| names.contains(n))
        {
            out.extend([
                ("G3_Brush_Plant_Fern_01.xcmsh", i, 0.08),
                ("G3_Brush_Plant_Fern_02.xcmsh", i, 0.08),
                ("G3_Brush_Plant_Forest_Underbrush_01.xcmsh", i, 0.05),
                ("G3_Brush_Plant_Forest_Underbrush_02.xcmsh", i, 0.05),
                ("G3_Object_Branch_Small_01.xcmsh", i, 0.02),
                ("G3_Object_Branch_Small_02.xcmsh", i, 0.02),
                ("G3_Object_Mushroom_01.xcmsh", i, 0.01),
            ]);
        }
    }
    out
}

/// UV scale of a material texture, times the remaster profile's factor for a replaced texture
/// that covers a different real-world size.
fn remaster_scale(path: &str, scale: [f32; 2]) -> [f32; 2] {
    let f = gothic_formats::remaster::Profile::global()
        .map_or(1.0, |p| p.texture_scale(&stem_of(path)));
    [scale[0] * f, scale[1] * f]
}

/// Vertex colour of a mesh vertex: rgb = baked AO (stream 5 alpha), a = layer blend weight
/// (stream 4 alpha).
fn vertex_color(sub: &gothic_formats::xcmsh::Submesh, k: usize) -> [f32; 4] {
    let ao = sub.colors5.get(k).map_or(1.0, |c| f32::from(c[3]) / 255.0);
    let w = sub.colors4.get(k).map_or(1.0, |c| f32::from(c[3]) / 255.0);
    [ao, ao, ao, w]
}

/// Mirrored tangent; the handedness sign flips together with the X mirror.
fn vertex_tangent(sub: &gothic_formats::xcmsh::Submesh, k: usize) -> [f32; 4] {
    sub.tangents
        .get(k)
        .map_or([1.0, 0.0, 0.0, -1.0], |t| [-t[0], t[1], t[2], -1.0])
}

/// Merge opaque / alpha-tested static props of a tile into one mesh per (size class, material),
/// baking the instance transform into the vertices. Returns the number of ids consumed.
fn merge_statics(parts: &mut Vec<Part>, first_id: u64) -> u64 {
    if std::env::var_os("GR_MERGE").is_none() {
        return 0;
    }
    let mut groups: HashMap<(u8, u64), (Arc<MatData>, GeomData)> = HashMap::new();
    let mut keep = Vec::with_capacity(parts.len());
    let mut next = first_id;
    for part in parts.drain(..) {
        let mergeable = part.kind == PartKind::Static
            && part.lod.is_none()
            && part.material.as_ref().is_some_and(|m| {
                m.kind == MatKind::Surface && !matches!(m.blend_mode, Some(2 | 7 | 8))
            })
            && !part.geom.positions.is_empty()
            && part.geom.normals.len() == part.geom.positions.len()
            && part.geom.uvs.len() == part.geom.positions.len()
            && part.geom.colors.len() == part.geom.positions.len()
            && part.geom.tangents.len() == part.geom.positions.len();
        if !mergeable {
            keep.push(part);
            continue;
        }
        let xf = part.transform;
        let g = &part.geom;
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for p in &g.positions {
            let w = xf.transform_point3(Vec3::from(*p));
            lo = lo.min(w);
            hi = hi.max(w);
        }
        let radius_m = (hi - lo).length() * 0.5 * 0.01;
        let class = if radius_m < 2.5 {
            0u8
        } else if radius_m < 8.0 {
            1
        } else {
            2
        };
        let material = part.material.clone().expect("checked");
        let slot = groups.entry((class, material.id)).or_insert_with(|| {
            (
                material,
                GeomData {
                    id: 0,
                    positions: vec![],
                    normals: vec![],
                    uvs: vec![],
                    indices: vec![],
                    colors: vec![],
                    tangents: vec![],
                    light: vec![],
                },
            )
        });
        let out = &mut slot.1;
        if out.positions.len() + g.positions.len() > 4_000_000 {
            keep.push(part);
            continue;
        }
        let normal_m = xf.inverse().transpose();
        let det_neg = xf.determinant() < 0.0;
        let base = out.positions.len() as u32;
        for k in 0..g.positions.len() {
            out.positions
                .push(xf.transform_point3(Vec3::from(g.positions[k])).to_array());
            out.normals.push(
                normal_m
                    .transform_vector3(Vec3::from(g.normals[k]))
                    .normalize_or_zero()
                    .to_array(),
            );
            out.uvs.push(g.uvs[k]);
            out.colors.push(g.colors[k]);
            out.light.push(g.light.get(k).copied().unwrap_or([0.0; 2]));
            let t = g.tangents[k];
            let tv = xf
                .transform_vector3(Vec3::new(t[0], t[1], t[2]))
                .normalize_or_zero();
            out.tangents.push([tv.x, tv.y, tv.z, t[3]]);
        }
        for tri in g.indices.as_chunks::<3>().0 {
            if det_neg {
                out.indices
                    .extend([tri[0] + base, tri[2] + base, tri[1] + base]);
            } else {
                out.indices.extend(tri.iter().map(|i| i + base));
            }
        }
    }
    let mut consumed = 0;
    for ((class, _), (material, mut geom)) in groups {
        geom.id = next + consumed;
        consumed += 1;
        keep.push(Part {
            lod: None,
            source: None,
            ocean_plane_y_m: None,
            geom: Arc::new(geom),
            material: Some(material),
            transform: Mat4::IDENTITY,
            kind: match class {
                0 => PartKind::MergedSmall,
                1 => PartKind::MergedMid,
                _ => PartKind::MergedBig,
            },
        });
    }
    *parts = keep;
    consumed
}
