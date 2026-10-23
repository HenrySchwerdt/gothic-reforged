//! Whole-world streaming for the Quest build (pack written by `examples/pack_world.rs`).
//!
//! * `world.idx` lists every 100 m cell with the ids of the meshes and textures it uses plus size tables, so the main
//!   thread knows what a cell needs without touching the disk.
//! * Cells, meshes and textures are loaded by background workers (file read, vertex/index conversion, image
//!   construction, collision triangles). The main thread only moves finished data into `Assets`, inserts collision
//!   chunks and spawns entities, each under a per-frame time budget, so streaming never stalls a frame.
//! * Meshes/textures/materials are shared between cells by id and reference counted (pinned when a cell is requested,
//!   released when it is unloaded), so a prop that appears in many cells exists once.
//! * Cells load inside `LOAD_RADIUS` of the player and unload beyond `UNLOAD_RADIUS` (hysteresis); a memory cap
//!   shrinks the radius when the resident asset bytes exceed it. Mesh LODs come from the cooked LOD index ranges
//!   (stage distances from `gothic-render-rules`, applied by `ardea::update_lod` with 5 % hysteresis); LOD1/2 are
//!   built with compacted vertex buffers so far stages cost a fraction of LOD0 memory.
use std::any::Any;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetBytesPerFrame;
use gothic_cook::mesh::{Indices as CIdx, MeshData, Vertex};
use gothic_cook::{AssetId, Cache};
use gothic_vr::{VrPlayer, VrWorld};
use gothic_xr_quest::knob_f32;

use crate::ardea::{Ardea, Lod, StaticProp, TriCount, build_image};
use crate::locomotion::Physics;
use crate::qmat::QuestMaterial;

// ---------------------------------------------------------------------------------------------------------------------
// Worker pool
// ---------------------------------------------------------------------------------------------------------------------

/// A unit of background work; results come back as [`Done`] in the order they finish.
pub struct Job {
    prio: f32,
    cancel: Arc<AtomicBool>,
    key: u64,
    run: Box<dyn FnOnce() -> Box<dyn Any + Send> + Send>,
}

pub struct Done {
    pub key: u64,
    pub payload: Box<dyn Any + Send>,
}

type Queue = Arc<(Mutex<Vec<Job>>, Condvar)>;

/// Small priority thread pool (lowest `prio` first; jobs whose `cancel` flag is set are dropped unrun).
pub struct Workers {
    q: Queue,
    rx: Mutex<Receiver<Done>>,
    running: Arc<AtomicU32>,
}

impl Workers {
    pub fn new(threads: usize, name: &str) -> Self {
        let q: Queue = Arc::new((Mutex::new(Vec::new()), Condvar::new()));
        let (tx, rx) = channel::<Done>();
        let running = Arc::new(AtomicU32::new(0));
        for i in 0..threads.max(1) {
            let (q, tx, running) = (q.clone(), tx.clone(), running.clone());
            let _ = std::thread::Builder::new()
                .name(format!("{name}-{i}"))
                .spawn(move || worker_loop(q, tx, running));
        }
        Self {
            q,
            rx: Mutex::new(rx),
            running,
        }
    }

    pub fn submit(
        &self,
        prio: f32,
        cancel: Arc<AtomicBool>,
        key: u64,
        run: impl FnOnce() -> Box<dyn Any + Send> + Send + 'static,
    ) {
        let (m, cv) = &*self.q;
        if let Ok(mut g) = m.lock() {
            g.push(Job {
                prio,
                cancel,
                key,
                run: Box::new(run),
            });
            cv.notify_one();
        }
    }

    /// Finished jobs (at most `max`).
    pub fn drain(&self, max: usize) -> Vec<Done> {
        let mut out = Vec::new();
        if let Ok(rx) = self.rx.lock() {
            while out.len() < max {
                match rx.try_recv() {
                    Ok(d) => out.push(d),
                    Err(_) => break,
                }
            }
        }
        out
    }

    /// Queued plus running jobs.
    pub fn pending(&self) -> usize {
        self.q.0.lock().map_or(0, |g| g.len()) + self.running.load(Ordering::Relaxed) as usize
    }
}

fn worker_loop(q: Queue, tx: Sender<Done>, running: Arc<AtomicU32>) {
    loop {
        let job = {
            let (m, cv) = &*q;
            let Ok(mut g) = m.lock() else { return };
            loop {
                if let Some(i) = g
                    .iter()
                    .enumerate()
                    .min_by(|a, b| a.1.prio.total_cmp(&b.1.prio))
                    .map(|(i, _)| i)
                {
                    break g.swap_remove(i);
                }
                g = match cv.wait(g) {
                    Ok(g) => g,
                    Err(_) => return,
                };
            }
        };
        if job.cancel.load(Ordering::Relaxed) {
            continue;
        }
        running.fetch_add(1, Ordering::Relaxed);
        let key = job.key;
        let run = job.run;
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run));
        running.fetch_sub(1, Ordering::Relaxed);
        if let Ok(payload) = res {
            if tx.send(Done { key, payload }).is_err() {
                return;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Index
// ---------------------------------------------------------------------------------------------------------------------

pub struct CellIdx {
    pub cx: i32,
    pub cz: i32,
    pub instances: u32,
    pub tris: u32,
    pub npcs: u32,
    pub meshes: Vec<u32>,
    pub tex: Vec<u32>,
}

pub struct WorldIndex {
    /// `(id, file bytes, LOD0 triangles)`.
    pub meshes: Vec<(u64, u32, u32)>,
    pub mesh_ix: HashMap<u64, u32>,
    /// `(id, file bytes)`.
    pub textures: Vec<(u64, u32)>,
    pub cells: Vec<CellIdx>,
    pub by_coord: HashMap<(i32, i32), u32>,
}

struct Rd<'a>(&'a [u8], usize);
impl Rd<'_> {
    fn u32(&mut self) -> Option<u32> {
        let b = self.0.get(self.1..self.1 + 4)?;
        self.1 += 4;
        Some(u32::from_le_bytes(b.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        let b = self.0.get(self.1..self.1 + 8)?;
        self.1 += 8;
        Some(u64::from_le_bytes(b.try_into().ok()?))
    }
}

impl WorldIndex {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let data = std::fs::read(path)?;
        anyhow::ensure!(data.starts_with(b"GWIDX001"), "bad world.idx");
        let mut r = Rd(&data, 8);
        let bad = || anyhow::anyhow!("truncated world.idx");
        let nm = r.u32().ok_or_else(bad)? as usize;
        let mut meshes = Vec::with_capacity(nm);
        for _ in 0..nm {
            meshes.push((
                r.u64().ok_or_else(bad)?,
                r.u32().ok_or_else(bad)?,
                r.u32().ok_or_else(bad)?,
            ));
        }
        let nt = r.u32().ok_or_else(bad)? as usize;
        let mut textures = Vec::with_capacity(nt);
        for _ in 0..nt {
            textures.push((r.u64().ok_or_else(bad)?, r.u32().ok_or_else(bad)?));
        }
        let nc = r.u32().ok_or_else(bad)? as usize;
        let mut cells = Vec::with_capacity(nc);
        for _ in 0..nc {
            let cx = r.u32().ok_or_else(bad)? as i32;
            let cz = r.u32().ok_or_else(bad)? as i32;
            let instances = r.u32().ok_or_else(bad)?;
            let _terrain = r.u32().ok_or_else(bad)?;
            let tris = r.u32().ok_or_else(bad)?;
            let npcs = r.u32().ok_or_else(bad)?;
            let (a, b) = (
                r.u32().ok_or_else(bad)? as usize,
                r.u32().ok_or_else(bad)? as usize,
            );
            let mut ms = Vec::with_capacity(a);
            for _ in 0..a {
                ms.push(r.u32().ok_or_else(bad)?);
            }
            let mut ts = Vec::with_capacity(b);
            for _ in 0..b {
                ts.push(r.u32().ok_or_else(bad)?);
            }
            cells.push(CellIdx {
                cx,
                cz,
                instances,
                tris,
                npcs,
                meshes: ms,
                tex: ts,
            });
        }
        let mesh_ix = meshes
            .iter()
            .enumerate()
            .map(|(i, m)| (m.0, i as u32))
            .collect();
        let by_coord = cells
            .iter()
            .enumerate()
            .map(|(i, c)| ((c.cx, c.cz), i as u32))
            .collect();
        Ok(Self {
            meshes,
            mesh_ix,
            textures,
            cells,
            by_coord,
        })
    }
}

/// Centre of a cell in the world root's centimetre frame (Bevy x = -Gothic x).
pub fn cell_center(cx: i32, cz: i32) -> Vec3 {
    Vec3::new(
        -(cx as f32 + 0.5) * 10_000.0,
        0.0,
        (cz as f32 + 0.5) * 10_000.0,
    )
}

/// `(cx, cz)` of a world-root position (cm).
pub fn cell_at(p: Vec3) -> (i32, i32) {
    ((-p.x / 10_000.0).floor() as i32, (p.z / 10_000.0).floor() as i32)
}

/// Id of piece `k` of a cell's collision (a cell's triangles are inserted in pieces so no frame pays for a whole cell).
pub fn chunk_id(cx: i32, cz: i32, k: u32) -> u64 {
    (((cx + 4096) as u64) << 44) | (((cz + 4096) as u64) << 24) | u64::from(k)
}

/// Triangles per collision piece and the main-thread time spent on pieces per frame.
const COLL_PIECE: usize = 2_500;
const COLL_BUDGET_US: u128 = 600;

struct CollJob {
    key: (i32, i32),
    tris: Vec<[Vec3; 3]>,
    next: usize,
    pieces: u32,
}

// ---------------------------------------------------------------------------------------------------------------------
// Worker-side builders
// ---------------------------------------------------------------------------------------------------------------------

pub struct SubMeta {
    pub mat: u64,
    pub aabb: Aabb,
    pub levels: u8,
    pub tris: [u32; 3],
    pub skip: bool,
}

pub struct MeshMeta {
    pub bmin: Vec3,
    pub bmax: Vec3,
    pub terrain: bool,
    pub subs: Vec<SubMeta>,
}

struct MeshPayload {
    idx: u32,
    meta: MeshMeta,
    subs: Vec<[Option<Mesh>; 3]>,
    bytes: u64,
}

struct TexPayload {
    idx: u32,
    img: Option<(Image, Vec2)>,
    bytes: u64,
}

struct InstData {
    mesh: u32,
    m: Mat4,
    terrain: bool,
}

struct CellPayload {
    cx: i32,
    cz: i32,
    insts: Vec<InstData>,
    collision: Vec<[Vec3; 3]>,
}

enum Payload {
    Cell((i32, i32), Option<CellPayload>),
    Mesh(Option<MeshPayload>),
    Tex(TexPayload),
}

fn mesh_from(vs: &[Vertex], pick: Option<&[u32]>, idx: Indices) -> Mesh {
    let get = |i: usize| match pick {
        Some(p) => &vs[p[i] as usize],
        None => &vs[i],
    };
    let n = pick.map_or(vs.len(), <[u32]>::len);
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    m.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        (0..n).map(|i| get(i).pos).collect::<Vec<_>>(),
    );
    m.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        (0..n).map(|i| get(i).normal).collect::<Vec<_>>(),
    );
    m.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        (0..n).map(|i| get(i).uv).collect::<Vec<_>>(),
    );
    m.insert_attribute(
        Mesh::ATTRIBUTE_COLOR,
        (0..n)
            .map(|i| {
                let c = get(i).color;
                [
                    c[0] as f32 / 255.0,
                    c[1] as f32 / 255.0,
                    c[2] as f32 / 255.0,
                    c[3] as f32 / 255.0,
                ]
            })
            .collect::<Vec<[f32; 4]>>(),
    );
    m.insert_indices(idx);
    m
}

/// One LOD stage of a submesh. LOD0 keeps the cooked vertex buffer; later stages keep only the vertices they use.
fn lod_mesh(md: &MeshData, s: &gothic_cook::mesh::SubmeshRec, lod: usize) -> (Mesh, u32, u64) {
    let vs = md.submesh_vertices(s);
    let idx = md.indices(s, lod);
    let tris = idx.len() as u32 / 3;
    if lod == 0 {
        let ind = match idx {
            CIdx::U16(i) => Indices::U16(i.to_vec()),
            CIdx::U32(i) => Indices::U32(i.to_vec()),
        };
        let bytes = vs.len() as u64 * 48 + tris as u64 * 12;
        return (mesh_from(vs, None, ind), tris, bytes);
    }
    let raw: Vec<u32> = match idx {
        CIdx::U16(i) => i.iter().map(|&x| u32::from(x)).collect(),
        CIdx::U32(i) => i.to_vec(),
    };
    let mut map = vec![u32::MAX; vs.len()];
    let mut used: Vec<u32> = Vec::new();
    for &i in &raw {
        let slot = &mut map[i as usize];
        if *slot == u32::MAX {
            *slot = used.len() as u32;
            used.push(i);
        }
    }
    let remapped: Vec<u32> = raw.iter().map(|&i| map[i as usize]).collect();
    let ind = if used.len() <= u16::MAX as usize {
        Indices::U16(remapped.iter().map(|&i| i as u16).collect())
    } else {
        Indices::U32(remapped)
    };
    let bytes = used.len() as u64 * 48 + tris as u64 * 12;
    (mesh_from(vs, Some(&used), ind), tris, bytes)
}

fn build_mesh_payload(cache: &Cache, id: u64, idx: u32) -> Option<MeshPayload> {
    let md = cache.load_mesh(AssetId(id)).ok()?;
    let hdr = md.header();
    let names = md.material_names();
    let terrain = md.is_terrain();
    let mut subs = Vec::new();
    let mut metas = Vec::new();
    let mut bytes = 0u64;
    for (si, s) in md.submeshes().iter().enumerate() {
        let skip = names
            .get(si)
            .is_some_and(|n| n.to_ascii_lowercase().contains("world_lowpoly"));
        let levels = if terrain {
            1
        } else {
            (s.lod_count as usize).clamp(1, 3)
        };
        let mut built: [Option<Mesh>; 3] = [None, None, None];
        let mut tris = [0u32; 3];
        if !skip {
            for lod in 0..levels {
                let (m, t, b) = lod_mesh(&md, s, lod);
                built[lod] = Some(m);
                tris[lod] = t;
                bytes += b;
            }
        }
        metas.push(SubMeta {
            mat: s.material,
            aabb: Aabb::from_min_max(Vec3::from(s.bounds_min), Vec3::from(s.bounds_max)),
            levels: levels as u8,
            tris,
            skip,
        });
        subs.push(built);
    }
    Some(MeshPayload {
        idx,
        meta: MeshMeta {
            bmin: Vec3::from(hdr.bounds_min),
            bmax: Vec3::from(hdr.bounds_max),
            terrain,
            subs: metas,
        },
        subs,
        bytes,
    })
}

fn build_cell_payload(cache: &Cache, ix: &WorldIndex, cx: i32, cz: i32) -> Option<CellPayload> {
    let cell = cache.load_cell(cx, cz).ok()??;
    let mut collision: Vec<[Vec3; 3]> = cell
        .collision_triangles()
        .map(|t| [Vec3::from(t[0]), Vec3::from(t[1]), Vec3::from(t[2])])
        .collect();
    let mut insts = Vec::with_capacity(cell.instances().len() + cell.terrain().len());
    for t in cell.terrain() {
        if let Some(&mesh) = ix.mesh_ix.get(&t.mesh) {
            insts.push(InstData {
                mesh,
                m: Mat4::IDENTITY,
                terrain: true,
            });
        }
    }
    for i in cell.instances() {
        let c = i.transform;
        let m = Mat4::from_cols(
            Vec3::from(c[0]).extend(0.0),
            Vec3::from(c[1]).extend(0.0),
            Vec3::from(c[2]).extend(0.0),
            Vec3::from(c[3]).extend(1.0),
        );
        if i.collision != 0 {
            if let Some(cm) = cache.collision(i.collision) {
                for t in cm.indices.chunks_exact(3) {
                    let v = |k: u32| m.transform_point3(Vec3::from(cm.vertices[k as usize]));
                    collision.push([v(t[0]), v(t[1]), v(t[2])]);
                }
            }
        }
        if let Some(&mesh) = ix.mesh_ix.get(&i.mesh) {
            insts.push(InstData {
                mesh,
                m,
                terrain: false,
            });
        }
    }
    Some(CellPayload {
        cx,
        cz,
        insts,
        collision,
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// Main-thread state
// ---------------------------------------------------------------------------------------------------------------------

struct MeshRes {
    refs: u32,
    ready: bool,
    cancel: Arc<AtomicBool>,
    meta: Option<Arc<MeshMeta>>,
    handles: Vec<[Option<Handle<Mesh>>; 3]>,
    bytes: u64,
}

struct TexRes {
    refs: u32,
    ready: bool,
    cancel: Arc<AtomicBool>,
    bytes: u64,
}

enum CellState {
    /// Cell job in flight.
    Loading,
    /// Payload arrived; waiting for shared meshes / textures that other cells requested earlier.
    Waiting(Vec<InstData>),
    Spawning { insts: Vec<InstData>, cursor: usize },
    Resident,
}

struct Cell {
    state: CellState,
    root: Entity,
    ix: u32,
    cancel: Arc<AtomicBool>,
    mats: HashSet<u64>,
    entities: u32,
    tris: u64,
    coll_pieces: u32,
}

/// Counters for logs and benchmarks.
#[derive(Resource, Default, Clone, Debug)]
pub struct WorldStats {
    pub cells: usize,
    pub loading: usize,
    pub entities: usize,
    pub tris: u64,
    pub mem_mb: f32,
    pub jobs: usize,
    pub radius: f32,
    /// Main-thread microseconds of the last / worst streaming frame.
    pub us_last: u32,
    pub us_peak: u32,
    pub collision_us_peak: u32,
    pub spawned_total: u64,
    pub loaded_total: u64,
    pub unloaded_total: u64,
}

#[derive(Resource)]
pub struct WorldStream {
    pub idx: Arc<WorldIndex>,
    cache: Arc<Cache>,
    workers: Workers,
    cells: HashMap<(i32, i32), Cell>,
    meshes: HashMap<u32, MeshRes>,
    textures: HashMap<u32, TexRes>,
    tex_ix: HashMap<u64, u32>,
    mat_refs: HashMap<u64, u32>,
    coll_queue: VecDeque<CollJob>,
    mem: u64,
    pub load_radius: f32,
    unload_radius: f32,
    eff_radius: f32,
    mem_cap: u64,
    max_loading: usize,
    spawn_budget_us: u32,
    pub stats: WorldStats,
    /// Position (cm, world-root frame) the streaming follows; defaults to the player.
    pub focus_override: Option<Vec3>,
    pub collision_cells: HashSet<(i32, i32)>,
}

impl WorldStream {
    /// Is the cell fully spawned?
    pub fn resident(&self, cx: i32, cz: i32) -> bool {
        self.cells
            .get(&(cx, cz))
            .is_some_and(|c| matches!(c.state, CellState::Resident))
    }
    /// Is the cell's collision chunk in the physics world?
    pub fn has_collision(&self, cx: i32, cz: i32) -> bool {
        self.collision_cells.contains(&(cx, cz))
    }
    pub fn cell_entity(&self, cx: i32, cz: i32) -> Option<Entity> {
        self.cells.get(&(cx, cz)).map(|c| c.root)
    }
    pub fn all_ready(&self) -> bool {
        self.cells
            .values()
            .all(|c| matches!(c.state, CellState::Resident))
            && self.coll_queue.is_empty()
    }
    pub fn resident_cells(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.cells
            .iter()
            .filter(|(_, c)| matches!(c.state, CellState::Resident))
            .map(|(k, _)| *k)
    }
}

/// Registers the streamer (called by `ardea::build` when the pack has a `world.idx`).
pub fn install(app: &mut App, dir: &Path) -> anyhow::Result<()> {
    let idx = Arc::new(WorldIndex::load(&dir.join("world.idx"))?);
    let cache = app.world().resource::<Ardea>().cache.clone();
    let radius = knob_f32("world_radius", 28_000.0).max(8_000.0);
    let tex_ix = idx
        .textures
        .iter()
        .enumerate()
        .map(|(i, t)| (t.0, i as u32))
        .collect();
    let threads = knob_f32("world_threads", 2.0) as usize;
    log::info!(
        "[world] index: {} cells, {} meshes, {} textures; radius {:.0} m, {} workers",
        idx.cells.len(),
        idx.meshes.len(),
        idx.textures.len(),
        radius / 100.0,
        threads
    );
    app.insert_resource(WorldStream {
        idx,
        cache,
        workers: Workers::new(threads, "world-io"),
        cells: HashMap::new(),
        meshes: HashMap::new(),
        textures: HashMap::new(),
        tex_ix,
        mat_refs: HashMap::new(),
        coll_queue: VecDeque::new(),
        mem: 0,
        load_radius: radius,
        unload_radius: radius + knob_f32("world_hyst", 4_000.0),
        eff_radius: radius,
        mem_cap: (knob_f32("world_mem_mb", 1_000.0) * 1_048_576.0) as u64,
        max_loading: knob_f32("world_loading", 6.0) as usize,
        spawn_budget_us: knob_f32("world_spawn_us", 1_000.0) as u32,
        stats: WorldStats::default(),
        focus_override: None,
        collision_cells: HashSet::new(),
    });
    app.init_resource::<WorldStats>();
    // Uploads (meshes, images) are throttled so a streamed cell never spends a whole frame in the render world.
    app.insert_resource(RenderAssetBytesPerFrame::new(
        (knob_f32("world_upload_mb", 3.0) * 1_048_576.0) as usize,
    ));
    app.add_systems(
        Update,
        (
            stream_system,
            crate::ardea::update_lod.after(stream_system),
            report.after(stream_system),
        ),
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn stream_system(
    mut commands: Commands,
    mut ws: ResMut<WorldStream>,
    mut a: ResMut<Ardea>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut qmats: ResMut<Assets<QuestMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut vrworld: ResMut<VrWorld>,
    mut phys: ResMut<Physics>,
    mut player: ResMut<VrPlayer>,
) {
    let t_start = Instant::now();
    let ws = &mut *ws;
    let pp = ws.focus_override.unwrap_or(player.ctrl.position);

    // 1. Finished jobs.
    for d in ws.workers.drain(96) {
        let Ok(p) = d.payload.downcast::<Payload>() else {
            continue;
        };
        match *p {
            Payload::Mesh(Some(mp)) => {
                if let Some(r) = ws.meshes.get_mut(&mp.idx) {
                    if r.ready {
                        continue;
                    }
                    r.handles = mp
                        .subs
                        .into_iter()
                        .map(|lods| lods.map(|m| m.map(|m| meshes.add(m))))
                        .collect();
                    r.meta = Some(Arc::new(mp.meta));
                    r.bytes = mp.bytes;
                    r.ready = true;
                    ws.mem += mp.bytes;
                }
            }
            Payload::Mesh(None) => {}
            Payload::Tex(tp) => {
                if let Some(r) = ws.textures.get_mut(&tp.idx) {
                    if r.ready {
                        continue;
                    }
                    let id = ws.idx.textures[tp.idx as usize].0;
                    a.textures
                        .insert(id, tp.img.map(|(img, uv)| (images.add(img), uv)));
                    r.bytes = tp.bytes;
                    r.ready = true;
                    ws.mem += tp.bytes;
                }
            }
            Payload::Cell(k, cp) => {
                let Some(cp) = cp else {
                    // Unreadable cell: treat as empty so it does not block the loading slots.
                    if let Some(c) = ws.cells.get_mut(&k) {
                        c.state = CellState::Resident;
                    }
                    continue;
                };
                let key = (cp.cx, cp.cz);
                let Some(cell) = ws.cells.get_mut(&key) else {
                    continue;
                };
                cell.state = CellState::Waiting(cp.insts);
                ws.coll_queue.push_back(CollJob {
                    key,
                    tris: cp.collision,
                    next: 0,
                    pieces: 0,
                });
            }
        }
    }

    // 2. Collision pieces (time-boxed): the player and NPC placement need the ground early.
    let t_coll = Instant::now();
    while let Some(job) = ws.coll_queue.front_mut() {
        if !ws.cells.contains_key(&job.key) {
            ws.coll_queue.pop_front();
            continue;
        }
        let t = Instant::now();
        let end = (job.next + COLL_PIECE).min(job.tris.len());
        let piece: Vec<[Vec3; 3]> = job.tris[job.next..end].to_vec();
        vrworld
            .0
            .insert_chunk(chunk_id(job.key.0, job.key.1, job.pieces), piece);
        job.pieces += 1;
        job.next = end;
        ws.stats.collision_us_peak = ws.stats.collision_us_peak.max(t.elapsed().as_micros() as u32);
        let done = job.next >= job.tris.len();
        let key = job.key;
        let pieces = job.pieces;
        if let Some(c) = ws.cells.get_mut(&key) {
            c.coll_pieces = pieces;
        }
        if done {
            ws.coll_queue.pop_front();
            ws.collision_cells.insert(key);
        }
        if t_coll.elapsed().as_micros() > COLL_BUDGET_US {
            break;
        }
    }

    // 3. Which cells are wanted.
    let (pcx, pcz) = cell_at(pp);
    let reach = (ws.eff_radius / 10_000.0).ceil() as i32 + 1;
    let mut want: Vec<(f32, i32, i32)> = Vec::new();
    for dx in -reach..=reach {
        for dz in -reach..=reach {
            let (cx, cz) = (pcx + dx, pcz + dz);
            if !ws.idx.by_coord.contains_key(&(cx, cz)) {
                continue;
            }
            let c = cell_center(cx, cz);
            let d = Vec2::new(c.x - pp.x, c.z - pp.z).length();
            if d < ws.eff_radius + 7_071.0 * 0.0 {
                want.push((d, cx, cz));
            }
        }
    }
    want.sort_by(|x, y| x.0.total_cmp(&y.0));
    let loading = ws
        .cells
        .values()
        .filter(|c| !matches!(c.state, CellState::Resident))
        .count();
    let mut slots = ws.max_loading.saturating_sub(loading);
    for (d, cx, cz) in want {
        if slots == 0 {
            break;
        }
        if ws.cells.contains_key(&(cx, cz)) {
            continue;
        }
        slots -= 1;
        request_cell(ws, &mut commands, &a, cx, cz, d * d);
    }

    // 4. Waiting -> Spawning when every shared resource is ready.
    let keys: Vec<(i32, i32)> = ws.cells.keys().copied().collect();
    for k in &keys {
        let ready = {
            let c = &ws.cells[k];
            matches!(c.state, CellState::Waiting(_))
                && ws.idx.cells[c.ix as usize]
                    .meshes
                    .iter()
                    .all(|m| ws.meshes.get(m).is_some_and(|r| r.ready))
                && ws.idx.cells[c.ix as usize]
                    .tex
                    .iter()
                    .all(|t| ws.textures.get(t).is_some_and(|r| r.ready))
        };
        if ready {
            let c = ws.cells.get_mut(k).unwrap();
            if let CellState::Waiting(insts) = std::mem::replace(&mut c.state, CellState::Resident)
            {
                c.state = CellState::Spawning { insts, cursor: 0 };
            }
        }
    }

    // 5. Spawn (nearest cells first) within the time budget.
    let mut spawning: Vec<((i32, i32), f32)> = ws
        .cells
        .iter()
        .filter(|(_, c)| matches!(c.state, CellState::Spawning { .. }))
        .map(|(k, _)| {
            let c = cell_center(k.0, k.1);
            (*k, Vec2::new(c.x - pp.x, c.z - pp.z).length())
        })
        .collect();
    spawning.sort_by(|x, y| x.1.total_cmp(&y.1));
    let budget = ws.spawn_budget_us as u128;
    let t_spawn = Instant::now();
    for (k, _) in spawning {
        if t_spawn.elapsed().as_micros() > budget {
            break;
        }
        spawn_cell(
            ws,
            &mut commands,
            &mut a,
            &mut qmats,
            &mut images,
            k,
            pp,
            t_spawn,
            budget,
        );
    }

    // 6. Unload far cells; memory cap shrinks the radius.
    let over = ws.mem > ws.mem_cap;
    if over {
        ws.eff_radius = (ws.eff_radius * 0.95).max(10_000.0);
    } else if ws.mem < ws.mem_cap / 10 * 8 && ws.eff_radius < ws.load_radius {
        ws.eff_radius = (ws.eff_radius * 1.02).min(ws.load_radius);
    }
    let unload_r = ws.unload_radius.min(ws.eff_radius + 4_000.0);
    let far: Vec<(i32, i32)> = ws
        .cells
        .keys()
        .filter(|k| {
            let c = cell_center(k.0, k.1);
            Vec2::new(c.x - pp.x, c.z - pp.z).length() > unload_r
        })
        .copied()
        .collect();
    for k in far {
        unload_cell(ws, &mut commands, &mut a, &mut vrworld, k);
    }

    // 7. Release the player once the ground below exists.
    if !phys.ready && !ws.collision_cells.is_empty() {
        let p = player.ctrl.position;
        if let Some(y) = vrworld
            .0
            .ground_below(Vec3::new(p.x, 50_000.0, p.z), 100_000.0)
        {
            player.ctrl.teleport(Vec3::new(p.x, y + 20.0, p.z));
            phys.ready = true;
            info!(
                "[world] ground found at y={y:.0} cm after {} cells",
                ws.collision_cells.len()
            );
        }
    }

    // Stats.
    let resident = ws
        .cells
        .values()
        .filter(|c| matches!(c.state, CellState::Resident))
        .count();
    ws.stats.cells = resident;
    ws.stats.loading = ws.cells.len() - resident;
    ws.stats.entities = ws.cells.values().map(|c| c.entities as usize).sum();
    ws.stats.tris = ws.cells.values().map(|c| c.tris).sum();
    ws.stats.mem_mb = ws.mem as f32 / 1_048_576.0;
    ws.stats.jobs = ws.workers.pending();
    ws.stats.radius = ws.eff_radius;
    let us = t_start.elapsed().as_micros() as u32;
    ws.stats.us_last = us;
    ws.stats.us_peak = ws.stats.us_peak.max(us);
    a.cells_done = resident;
}

fn request_cell(ws: &mut WorldStream, commands: &mut Commands, a: &Ardea, cx: i32, cz: i32, prio: f32) {
    let ix = ws.idx.by_coord[&(cx, cz)];
    let root = commands
        .spawn((
            Transform::IDENTITY,
            Visibility::default(),
            ChildOf(a.root),
            Name::new(format!("cell {cx},{cz}")),
        ))
        .id();
    let cancel = Arc::new(AtomicBool::new(false));
    let idx = ws.idx.clone();
    let cache = ws.cache.clone();
    {
        let (idx2, cache2) = (idx.clone(), cache.clone());
        ws.workers.submit(prio, cancel.clone(), 0, move || {
            Box::new(Payload::Cell(
                (cx, cz),
                build_cell_payload(&cache2, &idx2, cx, cz),
            ))
        });
    }
    for &m in &idx.cells[ix as usize].meshes {
        let r = ws.meshes.entry(m).or_insert_with(|| MeshRes {
            refs: 0,
            ready: false,
            cancel: Arc::new(AtomicBool::new(false)),
            meta: None,
            handles: Vec::new(),
            bytes: 0,
        });
        r.refs += 1;
        if r.refs == 1 {
            let (cache, id, c) = (cache.clone(), idx.meshes[m as usize].0, r.cancel.clone());
            ws.workers.submit(prio + 1.0, c, 1, move || {
                Box::new(Payload::Mesh(build_mesh_payload(&cache, id, m)))
            });
        }
    }
    for &t in &idx.cells[ix as usize].tex {
        let r = ws.textures.entry(t).or_insert_with(|| TexRes {
            refs: 0,
            ready: false,
            cancel: Arc::new(AtomicBool::new(false)),
            bytes: 0,
        });
        r.refs += 1;
        if r.refs == 1 {
            let (cache, id, c) = (cache.clone(), idx.textures[t as usize].0, r.cancel.clone());
            ws.workers.submit(prio + 1.0, c, 2, move || {
                let (img, bytes) = match cache.load_texture(AssetId(id)) {
                    Ok(td) => match build_image(&td) {
                        Some((i, uv)) => {
                            let b = i.data.as_ref().map_or(0, |d| d.len() as u64);
                            (Some((i, uv)), b)
                        }
                        None => (None, 0),
                    },
                    Err(_) => (None, 0),
                };
                Box::new(Payload::Tex(TexPayload { idx: t, img, bytes }))
            });
        }
    }
    ws.cells.insert(
        (cx, cz),
        Cell {
            state: CellState::Loading,
            root,
            ix,
            cancel,
            mats: HashSet::new(),
            entities: 0,
            tris: 0,
            coll_pieces: 0,
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn spawn_cell(
    ws: &mut WorldStream,
    commands: &mut Commands,
    a: &mut Ardea,
    qmats: &mut Assets<QuestMaterial>,
    images: &mut Assets<Image>,
    key: (i32, i32),
    focus: Vec3,
    t_spawn: Instant,
    budget_us: u128,
) {
    let Some(mut cell) = ws.cells.remove(&key) else {
        return;
    };
    let CellState::Spawning { insts, mut cursor } =
        std::mem::replace(&mut cell.state, CellState::Resident)
    else {
        ws.cells.insert(key, cell);
        return;
    };
    let (lod_scale, cull_scale) = (a.lod_scale, a.cull_scale);
    while cursor < insts.len() {
        if t_spawn.elapsed().as_micros() > budget_us {
            break;
        }
        let inst = &insts[cursor];
        cursor += 1;
        let Some(mr) = ws.meshes.get(&inst.mesh) else {
            continue;
        };
        let Some(meta) = mr.meta.clone() else {
            continue;
        };
        let m = inst.m;
        let scale = m.x_axis.truncate().length().max(1e-3);
        let radius = (meta.bmax - meta.bmin).length() * 0.5 * scale;
        let center = m.transform_point3((meta.bmin + meta.bmax) * 0.5);
        let dist0 = (center - focus).length();
        let end = if inst.terrain || meta.terrain {
            f32::MAX
        } else {
            gothic_render_rules::cull::distance_band(
                radius,
                gothic_render_rules::cull::screen_cull_factor(0) / cull_scale.max(0.01),
            )
            .1
            .clamp(3_000.0, 25_000.0)
        };
        let hidden = dist0 > end;
        for (si, sm) in meta.subs.iter().enumerate() {
            if sm.skip {
                continue;
            }
            let Some(q) = a.qmaterial(sm.mat, qmats, images) else {
                continue;
            };
            if cell.mats.insert(sm.mat) {
                *ws.mat_refs.entry(sm.mat).or_default() += 1;
            }
            let levels = sm.levels as usize;
            let stage_d = gothic_render_rules::lod::stage_distances(radius, levels, lod_scale, 1.0);
            let stage = stage_d
                .iter()
                .filter(|d| dist0 > **d)
                .count()
                .min(levels - 1);
            let hs = &mr.handles[si];
            let Some(h) = hs[stage].clone() else {
                continue;
            };
            let tris = sm.tris;
            commands.spawn((
                Mesh3d(h),
                MeshMaterial3d(q),
                Transform::from_matrix(m),
                sm.aabb,
                TriCount(tris[stage]),
                StaticProp,
                Lod::new(
                    hs.clone(),
                    tris,
                    [
                        stage_d.first().copied().unwrap_or(f32::MAX),
                        stage_d.get(1).copied().unwrap_or(f32::MAX),
                    ],
                    levels as u8,
                    stage as u8,
                    center,
                    end,
                    hidden,
                ),
                if hidden {
                    Visibility::Hidden
                } else {
                    Visibility::Inherited
                },
                ChildOf(cell.root),
            ));
            cell.entities += 1;
            cell.tris += u64::from(tris[stage]);
            ws.stats.spawned_total += 1;
        }
    }
    if cursor >= insts.len() {
        cell.state = CellState::Resident;
        ws.stats.loaded_total += 1;
    } else {
        cell.state = CellState::Spawning { insts, cursor };
    }
    ws.cells.insert(key, cell);
}

fn unload_cell(
    ws: &mut WorldStream,
    commands: &mut Commands,
    a: &mut Ardea,
    vrworld: &mut VrWorld,
    key: (i32, i32),
) {
    let Some(cell) = ws.cells.remove(&key) else {
        return;
    };
    cell.cancel.store(true, Ordering::Relaxed);
    commands.entity(cell.root).despawn();
    for k in 0..cell.coll_pieces {
        vrworld.0.remove_chunk(chunk_id(key.0, key.1, k));
    }
    ws.collision_cells.remove(&key);
    ws.coll_queue.retain(|c| c.key != key);
    let ix = ws.idx.clone();
    let ci = &ix.cells[cell.ix as usize];
    for m in &ci.meshes {
        if let Some(r) = ws.meshes.get_mut(m) {
            r.refs -= 1;
            if r.refs == 0 {
                r.cancel.store(true, Ordering::Relaxed);
                ws.mem = ws.mem.saturating_sub(r.bytes);
                ws.meshes.remove(m);
            }
        }
    }
    for t in &ci.tex {
        if let Some(r) = ws.textures.get_mut(t) {
            r.refs -= 1;
            if r.refs == 0 {
                r.cancel.store(true, Ordering::Relaxed);
                ws.mem = ws.mem.saturating_sub(r.bytes);
                ws.textures.remove(t);
                a.textures.remove(&ix.textures[*t as usize].0);
            }
        }
    }
    for mat in &cell.mats {
        if let Some(n) = ws.mat_refs.get_mut(mat) {
            *n -= 1;
            if *n == 0 {
                ws.mat_refs.remove(mat);
                a.qmaterials.remove(mat);
            }
        }
    }
    ws.stats.unloaded_total += 1;
}

fn report(
    time: Res<Time>,
    mut acc: Local<f32>,
    ws: Res<WorldStream>,
    player: Res<VrPlayer>,
    vis: Query<(&ViewVisibility, &TriCount)>,
) {
    *acc += time.delta_secs();
    if *acc < 2.0 {
        return;
    }
    *acc = 0.0;
    let (mut n, mut tris) = (0u32, 0u64);
    for (v, t) in &vis {
        if v.get() {
            n += 1;
            tris += u64::from(t.0);
        }
    }
    let s = &ws.stats;
    info!(
        "[world] cells={} loading={} entities={} tris={} mem={:.0}MB jobs={} radius={:.0}m stream_us={} peak_us={} coll_peak_us={} visible_meshes={} visible_tris={} pos_cm=({:.0},{:.0},{:.0})",
        s.cells,
        s.loading,
        s.entities,
        s.tris,
        s.mem_mb,
        s.jobs,
        s.radius / 100.0,
        s.us_last,
        s.us_peak,
        s.collision_us_peak,
        n,
        tris,
        player.ctrl.position.x,
        player.ctrl.position.y,
        player.ctrl.position.z
    );
}
