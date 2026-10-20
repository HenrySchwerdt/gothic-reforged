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
    /// Origin-relative cm transform (mirrored space).
    pub transform: Mat4,
    pub kind: PartKind,
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
}
pub enum Response {
    Ready { records: usize, millis: u128 },
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
        let loaded = (|| {
            let slot = self.vfs.resolve_name(&key)?;
            let bytes = self.vfs.read(slot).ok()?;
            let img = Ximg::parse(&bytes).ok()?;
            Some((
                img.width,
                img.height,
                img.fourcc,
                img.mips().iter().map(|m| m.to_vec()).collect::<Vec<_>>(),
            ))
        })();
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
            if let Some(r) = &recipe {
                vertex_blend = r.vertex_blend;
                for l in &r.layers {
                    let mut tex: [Option<Arc<TexData>>; 2] = [None, None];
                    let mut scale = [[1.0f32; 2]; 2];
                    for (i, t) in l.textures.iter().take(2).enumerate() {
                        tex[i] = self.texture_path(&t.path);
                        scale[i] = t.scale;
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
                        normals[i] = Some((tex, t.scale));
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
            let water = lname.contains("water") && layers.is_empty();
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
                kind: if water {
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
        let slot = self.vfs.resolve_name(key)?;
        let bytes = self.vfs.read(slot).ok()?;
        if key.ends_with(".xlmsh") {
            if depth > 0 {
                return None;
            }
            let lod = Xlmsh::parse(&bytes).ok()?;
            let level = lod.levels.iter().find(|l| l.source_index == 0)?;
            return self.mesh(&level.mesh.clone(), depth + 1);
        }
        let mesh = Xcmsh::parse(&bytes).ok()?;
        let mut parts = Vec::new();
        for sub in &mesh.submeshes {
            if sub.positions.is_empty() || sub.indices.is_empty() {
                continue;
            }
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
            });
            let material = self.material(&sub.material);
            parts.push((geom, material));
        }
        Some(Arc::new(MeshEntry { parts }))
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
                geom: Arc::new(GeomData {
                    id: self.id(),
                    positions: geom.0,
                    normals: geom.1,
                    uvs: geom.2,
                    colors: geom.3,
                    tangents: geom.4,
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
        let hits: Vec<(String, [[f32; 4]; 4])> = self
            .index
            .query_xz([center.x, 0.0, center.z], radius)
            .into_iter()
            .filter(|r| parse_cell_name(&r.resource).is_none())
            .map(|r| (r.resource.to_string(), r.world_matrix))
            .collect();
        let f = flip();
        let origin = self.origin;
        for (resource, matrix) in hits {
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
            let transform =
                Mat4::from_translation(Vec3::new(origin.0.x, -origin.0.y, -origin.0.z)) * f * m * f;
            // Translation of F*M*F is (-tx, ty, tz); the origin shift above makes it
            // origin-relative: (ox - tx, ty - oy, tz - oz).
            let mut any_solid = false;
            let mut water_only = true;
            for (geom, material) in &mesh.parts {
                // Distant-LOD proxy submeshes (low-poly world atlas) overlap the real geometry.
                if material.as_ref().is_some_and(|m| {
                    m.name.contains("world_lowpoly")
                        || m.layer_names.iter().any(|n| n.contains("world_lowpoly"))
                }) {
                    continue;
                }
                let is_water = material.as_ref().is_some_and(|m| m.kind == MatKind::Water);
                let solid = !is_water
                    && material
                        .as_ref()
                        .is_none_or(|m| !matches!(m.blend_mode, Some(2 | 7 | 8)));
                any_solid |= solid;
                water_only &= is_water;
                parts.push(Part {
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
            if water_only {
                continue;
            }
            stats.statics += 1;
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
                        geom: g.clone(),
                        material: Some(kind.bark_mat.clone()),
                        transform,
                        kind: PartKind::TreeBark,
                    });
                }
                if let Some(g) = &variant.cards {
                    parts.push(Part {
                        geom: g.clone(),
                        material: Some(kind.card_mat.clone()),
                        transform,
                        kind: PartKind::Tree,
                    });
                }
                if let Some(g) = &kind.far_bark {
                    parts.push(Part {
                        geom: g.clone(),
                        material: Some(kind.bark_mat.clone()),
                        transform,
                        kind: PartKind::TreeFarBark,
                    });
                }
                parts.push(Part {
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

    /// Procedurally scattered ground cover for a tile (see `veg_density`).
    fn load_vegetation(&mut self, key: TileKey) -> VegPayload {
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
                                g.colors
                                    .push([c[0] * shade, c[1] * shade, c[2] * shade, 1.0]);
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
/// The per-instance data of the engine's vegetation system is not recoverable from the data
/// (every `eCVegetation_PS` entity stores the same template surface), so cover is driven by
/// what the terrain material paints: grass layers grow grass, herbs and the odd stone; forest
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
