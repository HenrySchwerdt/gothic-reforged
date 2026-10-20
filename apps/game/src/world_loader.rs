//! Background loader: turns world tiles into CPU-ready render + collision payloads.
//!
//! Coordinate conversion (see docs/research/TERRAIN_FORMAT.md, XCMSH_LAYOUT.md):
//! Gothic 3 is a left-handed, Y-up, clockwise-front D3D title; 1 unit = 1 cm. Bevy is
//! right-handed. We mirror X (`F = diag(-1,1,1)`), which flips handedness *and* the
//! winding at once, so source index order is kept and front faces stay front. Instance
//! matrices (row-vector convention) become `F * M * F`; a fixed origin offset keeps f32
//! coordinates small. Everything this module emits is in centimetres relative to the
//! origin; the renderer applies a uniform 0.01 scale.
use anyhow::{Context, Result};
use bevy::math::{Mat4, Vec3};
use gothic_formats::{
    terrain::parse_cell_name,
    vfs::VirtualFileSystem,
    xcmsh::Xcmsh,
    ximg::Ximg,
    xlmsh::Xlmsh,
    xnvmsh::Xnvmsh,
    xshmat::{TextureRole, Xshmat},
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
pub type TileKey = (i32, i32);

pub struct TexData {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub fourcc: [u8; 4],
    pub mips: Vec<Vec<u8>>,
}

pub struct MatData {
    pub id: u64,
    pub blend_mode: Option<u32>,
    pub mask_reference: Option<u8>,
    pub diffuse: Option<Arc<TexData>>,
}

/// GPU-ready geometry in local (mirrored) space; shared between instances.
pub struct GeomData {
    pub id: u64,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

pub struct Part {
    pub geom: Arc<GeomData>,
    pub material: Option<Arc<MatData>>,
    /// Origin-relative cm transform (mirrored space).
    pub transform: Mat4,
    pub terrain: bool,
}

pub struct TilePayload {
    pub key: TileKey,
    pub parts: Vec<Part>,
    /// Collision triangles, cm, origin-relative mirrored space.
    pub collision: Vec<[Vec3; 3]>,
    pub terrain_collision_from_nav: bool,
}

pub enum Request {
    Tile(TileKey),
}
pub enum Response {
    Ready { records: usize, millis: u128 },
    Tile(Box<TilePayload>),
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
                    while let Ok(Request::Tile(key)) = req_rx.recv() {
                        match loader.load_tile(key) {
                            Ok(p) => {
                                if resp_tx.send(Response::Tile(Box::new(p))).is_err() {
                                    break;
                                }
                            }
                            Err(e) => {
                                let _ =
                                    resp_tx.send(Response::Failed(format!("tile {key:?}: {e:#}")));
                                // An empty payload keeps the streaming bookkeeping moving.
                                let _ = resp_tx.send(Response::Tile(Box::new(TilePayload {
                                    key,
                                    parts: vec![],
                                    collision: vec![],
                                    terrain_collision_from_nav: false,
                                })));
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
                    indices: vec![],
                });
                for &i in t {
                    let out = *remap.entry((key, i)).or_insert_with(|| {
                        let o = entry.positions.len() as u32;
                        entry.positions.push(world[i as usize].to_array());
                        let nrm = sub
                            .normals
                            .get(i as usize)
                            .copied()
                            .unwrap_or([0.0, 1.0, 0.0]);
                        entry.normals.push([-nrm[0], nrm[1], nrm[2]]);
                        entry
                            .uvs
                            .push(sub.uvs.get(i as usize).copied().unwrap_or([0.0, 0.0]));
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

    fn material(&mut self, name: &str) -> Option<Arc<MatData>> {
        let key = name.to_ascii_lowercase();
        if let Some(m) = self.materials.get(&key) {
            return m.clone();
        }
        let parsed = (|| {
            let slot = self.vfs.resolve_name(&key)?;
            Xshmat::parse(&self.vfs.read(slot).ok()?).ok()
        })();
        let mat = parsed.map(|x| {
            let stem = x
                .texture(TextureRole::Diffuse)
                .map(|t| t.stem().to_string());
            let diffuse = stem.and_then(|s| self.texture(&s));
            Arc::new(MatData {
                id: self.id(),
                blend_mode: x.blend_mode,
                mask_reference: x.mask_reference,
                diffuse,
            })
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
            let positions: Vec<[f32; 3]> =
                sub.positions.iter().map(|p| [-p[0], p[1], p[2]]).collect();
            let normals: Vec<[f32; 3]> = if sub.normals.len() == positions.len() {
                sub.normals.iter().map(|n| [-n[0], n[1], n[2]]).collect()
            } else {
                vec![[0.0, 1.0, 0.0]; positions.len()]
            };
            let uvs = if sub.uvs.len() == positions.len() {
                sub.uvs.clone()
            } else {
                vec![[0.0, 0.0]; positions.len()]
            };
            let geom = Arc::new(GeomData {
                id: self.id(),
                positions,
                normals,
                uvs,
                indices: sub.indices.clone(),
            });
            let material = self.material(&sub.material);
            parts.push((geom, material));
        }
        Some(Arc::new(MeshEntry { parts }))
    }

    fn load_tile(&mut self, key: TileKey) -> Result<TilePayload> {
        let center = self.tile_center_gothic(key);
        self.ensure_cells_near(center);
        let mut parts = Vec::new();
        let mut collision: Vec<[Vec3; 3]> = Vec::new();

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
                    indices: geom.3,
                }),
                material,
                transform: Mat4::IDENTITY,
                terrain: true,
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
            for (geom, material) in &mesh.parts {
                let solid = material
                    .as_ref()
                    .is_none_or(|m| !matches!(m.blend_mode, Some(2 | 7 | 8)));
                if solid && geom.indices.len() <= 60_000 {
                    for tri in geom.indices.as_chunks::<3>().0 {
                        collision.push([0, 1, 2].map(|k| {
                            transform.transform_point3(Vec3::from(geom.positions[tri[k] as usize]))
                        }));
                    }
                }
                parts.push(Part {
                    geom: geom.clone(),
                    material: material.clone(),
                    transform,
                    terrain: false,
                });
            }
        }
        Ok(TilePayload {
            key,
            parts,
            collision,
            terrain_collision_from_nav: from_nav,
        })
    }
}
