//! The cooker: compiles an installation into the cache described in [`crate::cache`].
use crate::actor::{cook_actor, cook_motion};
use crate::cache::*;
use crate::container::{Kind, Writer};
use crate::ids::{AssetId, COOKER_VERSION, Profile, hash64};
use crate::material::{self, MaterialRec};
use crate::mesh::cook_mesh;
use crate::texture::cook_texture;
use crate::world::{CellBuilder, CollisionBuilder, Instance, TerrainRef, bevy_transform, cell_of};
use anyhow::{Context, Result, bail};
use gothic_formats::{
    terrain::{TerrainCell, collect_cells, parse_cell_name},
    vfs::{Slot, VirtualFileSystem},
    world_node::WorldDocument,
    xact::Actor,
    xcmsh::Xcmsh,
    xlmsh::Xlmsh,
    xmot::Motion,
    xnvmsh::Xnvmsh,
};
use gothic_gameplay::db::GameDatabase;
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CookKind {
    Db,
    Mesh,
    Collision,
    Material,
    Texture,
    World,
    Actor,
    Motion,
}

impl CookKind {
    pub const ALL: [CookKind; 8] = [
        Self::Db,
        Self::Mesh,
        Self::Collision,
        Self::Material,
        Self::Texture,
        Self::World,
        Self::Actor,
        Self::Motion,
    ];
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().trim_end_matches('s') {
            "db" | "database" => Self::Db,
            "mesh" => Self::Mesh,
            "collision" => Self::Collision,
            "material" => Self::Material,
            "texture" | "tex" => Self::Texture,
            "world" | "cell" => Self::World,
            "actor" => Self::Actor,
            "motion" => Self::Motion,
            _ => return None,
        })
    }
}

/// Gothic-space (cm) disc used by `--region` quick cooks.
#[derive(Debug, Clone)]
pub struct Region {
    pub name: String,
    pub center: [f32; 2],
    pub radius: f32,
}

impl Region {
    pub fn by_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            // Ardea city centre, see apps/game walk ORIGIN; ~2.5 world cells of margin.
            "ardea" => Some(Self {
                name: "ardea".into(),
                center: [92_400.0, -11_000.0],
                radius: 25_000.0,
            }),
            _ => None,
        }
    }
    pub fn contains(&self, x: f32, z: f32) -> bool {
        let (dx, dz) = (x - self.center[0], z - self.center[1]);
        dx * dx + dz * dz <= self.radius * self.radius
    }
    /// Does the Gothic-space XZ box intersect the region's bounding square?
    pub fn overlaps_box(&self, min: [f32; 2], max: [f32; 2]) -> bool {
        max[0] >= self.center[0] - self.radius
            && min[0] <= self.center[0] + self.radius
            && max[1] >= self.center[1] - self.radius
            && min[1] <= self.center[1] + self.radius
    }
}

#[derive(Debug, Clone)]
pub struct Options {
    pub data_dir: PathBuf,
    /// Cache root; the profile directory is `<out_root>/<profile name>`.
    pub out_root: PathBuf,
    pub profile: Profile,
    /// Empty = everything.
    pub only: Vec<CookKind>,
    pub region: Option<Region>,
    pub language: String,
    /// Ignore the previous manifest and re-cook everything.
    pub force: bool,
    pub quiet: bool,
}

#[derive(Debug, Default, Clone)]
pub struct Stat {
    pub cooked: usize,
    pub skipped: usize,
    pub failed: usize,
    /// Referenced sources absent from the installation (not an error).
    pub missing: usize,
    pub bytes: u64,
    pub seconds: f64,
}

#[derive(Debug, Default)]
pub struct Report {
    pub out_dir: PathBuf,
    pub stats: BTreeMap<&'static str, Stat>,
    pub total_bytes: u64,
    pub seconds: f64,
    pub failures: BTreeMap<String, String>,
}

struct Source {
    vfs: Mutex<VirtualFileSystem>,
    archive_stamps: HashMap<String, u64>,
    fingerprint: u64,
}

impl Source {
    fn open(data_dir: &Path) -> Result<Self> {
        let vfs = VirtualFileSystem::open_data_dir(data_dir)?;
        let mut archive_stamps = HashMap::new();
        let mut fp = Vec::new();
        for name in vfs.archive_names() {
            let meta =
                std::fs::metadata(data_dir.join(name)).with_context(|| format!("stat {name}"))?;
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            let stamp = hash64(format!("{name}|{}|{mtime}", meta.len()).as_bytes());
            archive_stamps.insert(name.clone(), stamp);
            fp.extend_from_slice(&stamp.to_le_bytes());
        }
        Ok(Self {
            vfs: Mutex::new(vfs),
            archive_stamps,
            fingerprint: hash64(&fp),
        })
    }
    fn resolve(&self, name: &str) -> Option<Slot> {
        self.vfs.lock().ok()?.resolve_name(name)
    }
    fn read(&self, slot: Slot) -> Result<Vec<u8>> {
        self.vfs
            .lock()
            .map_err(|_| anyhow::anyhow!("vfs poisoned"))?
            .read(slot)
    }
    /// Identity of the winning source member: archive (size+mtime) and entry path.
    fn stamp(&self, slot: Slot) -> (u64, String) {
        let v = self.vfs.lock().expect("vfs");
        let archive = v.archive_name(slot).to_owned();
        let st = self.archive_stamps.get(&archive).copied().unwrap_or(0);
        (
            hash64(format!("{st}|{}", v.entry_path(slot)).as_bytes()),
            archive,
        )
    }
    fn list(&self, ext: &str) -> Vec<(String, Slot)> {
        let v = self.vfs.lock().expect("vfs");
        let mut out: Vec<_> = v
            .iter()
            .filter(|(p, _)| p.ends_with(ext))
            .map(|(p, s)| (p.to_owned(), s))
            .collect();
        out.sort();
        out
    }
}

struct Progress {
    label: &'static str,
    total: usize,
    done: AtomicUsize,
    quiet: bool,
}

impl Progress {
    fn new(label: &'static str, total: usize, quiet: bool) -> Self {
        if !quiet {
            eprintln!("[{label}] {total} items");
        }
        Self {
            label,
            total,
            done: AtomicUsize::new(0),
            quiet,
        }
    }
    fn tick(&self) {
        let d = self.done.fetch_add(1, Ordering::Relaxed) + 1;
        let step = (self.total / 10).max(500);
        if !self.quiet && d.is_multiple_of(step) && d < self.total {
            eprintln!("[{}] {d}/{}", self.label, self.total);
        }
    }
}

struct Run<'a> {
    o: &'a Options,
    src: Source,
    out: PathBuf,
    prev: Manifest,
    entries: Mutex<BTreeMap<String, Entry>>,
    failures: Mutex<BTreeMap<String, String>>,
    stats: Mutex<BTreeMap<&'static str, Stat>>,
    bounds: Mutex<BTreeMap<String, TerrainBounds>>,
}

enum Outcome {
    Skipped,
    Cooked,
    Failed,
}

impl Run<'_> {
    fn want(&self, k: CookKind) -> bool {
        self.o.only.is_empty() || self.o.only.contains(&k)
    }

    fn stat(&self, kind: &'static str, f: impl FnOnce(&mut Stat)) {
        f(self.stats.lock().expect("stats").entry(kind).or_default());
    }

    fn fail(&self, kind: &'static str, what: &str, err: impl std::fmt::Display) {
        self.failures
            .lock()
            .expect("failures")
            .insert(format!("{kind}:{what}"), err.to_string());
        self.stat(kind, |s| s.failed += 1);
    }

    /// Cook-or-skip one artifact. `make` returns the bytes and the artifact's refs.
    fn emit(
        &self,
        kind: &'static str,
        rel: &str,
        key: u64,
        name: &str,
        source: &str,
        make: impl FnOnce() -> Result<(Vec<u8>, Vec<String>)>,
    ) -> Outcome {
        let path = self.out.join(rel);
        if !self.o.force
            && let Some(prev) = self.prev.entries.get(rel)
            && prev.key == key
            && path.is_file()
        {
            self.entries
                .lock()
                .expect("entries")
                .insert(rel.to_owned(), prev.clone());
            self.stat(kind, |s| s.skipped += 1);
            return Outcome::Skipped;
        }
        match make().and_then(|(bytes, refs)| {
            write_atomic(&path, &bytes)?;
            Ok((bytes, refs))
        }) {
            Ok((bytes, refs)) => {
                let e = Entry {
                    name: name.to_owned(),
                    source: source.to_owned(),
                    key,
                    content_hash: hash64(&bytes),
                    bytes: bytes.len() as u64,
                    refs,
                };
                self.stat(kind, |s| {
                    s.cooked += 1;
                    s.bytes += e.bytes;
                });
                self.entries
                    .lock()
                    .expect("entries")
                    .insert(rel.to_owned(), e);
                Outcome::Cooked
            }
            Err(err) => {
                self.fail(kind, name, format!("{err:#}"));
                Outcome::Failed
            }
        }
    }

    fn refs_of(&self, rel: &str) -> Vec<String> {
        if let Some(e) = self.entries.lock().expect("entries").get(rel) {
            return e.refs.clone();
        }
        self.prev
            .entries
            .get(rel)
            .map(|e| e.refs.clone())
            .unwrap_or_default()
    }

    fn salted(&self, stamp: u64, salt: &str) -> u64 {
        hash64(format!("{stamp}|{salt}|{}", self.o.profile.settings_hash()).as_bytes())
    }

    fn timed<T>(&self, kind: &'static str, f: impl FnOnce() -> T) -> T {
        let t = Instant::now();
        let r = f();
        self.stat(kind, |s| s.seconds += t.elapsed().as_secs_f64());
        r
    }
}

struct Inst {
    res: u32,
    matrix: [[f32; 4]; 4],
}

struct Scan {
    res_names: Vec<String>,
    instances: Vec<Inst>,
    terrain: Vec<TerrainCell>,
}

fn base_lower(name: &str) -> String {
    name.rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase()
}

fn stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

fn scan_world(run: &Run) -> Result<Scan> {
    let docs: Vec<_> = run
        .src
        .list(".node")
        .into_iter()
        .filter(|(p, _)| gothic_importer::survey::is_world_document(p))
        .collect();
    let prog = Progress::new("world docs", docs.len(), run.o.quiet);
    let region = run.o.region.as_ref();
    type DocOut = (Vec<(String, [[f32; 4]; 4])>, Vec<TerrainCell>);
    let parsed: Vec<Option<DocOut>> = docs
        .par_iter()
        .map(|(_, slot)| {
            let out = (|| -> Result<DocOut> {
                let doc = WorldDocument::parse(&run.src.read(*slot)?)?;
                let mut refs = Vec::new();
                for e in &doc.entities {
                    if !e.world_matrix.iter().flatten().all(|v| v.is_finite()) {
                        continue;
                    }
                    if let Some(r) = region
                        && !r.contains(e.world_matrix[3][0], e.world_matrix[3][2])
                    {
                        continue;
                    }
                    for a in &e.asset_references {
                        let l = a.to_ascii_lowercase();
                        if (l.ends_with(".xcmsh") || l.ends_with(".xlmsh"))
                            && parse_cell_name(a).is_none()
                        {
                            refs.push((base_lower(a), e.world_matrix));
                        }
                    }
                }
                Ok((refs, collect_cells(&doc)))
            })();
            prog.tick();
            out.ok()
        })
        .collect();
    let mut names: HashMap<String, u32> = HashMap::new();
    let mut res_names = Vec::new();
    let mut instances = Vec::new();
    let mut terrain: BTreeMap<String, TerrainCell> = BTreeMap::new();
    for (refs, cells) in parsed.into_iter().flatten() {
        for (n, m) in refs {
            let id = *names.entry(n.clone()).or_insert_with(|| {
                res_names.push(n);
                res_names.len() as u32 - 1
            });
            instances.push(Inst { res: id, matrix: m });
        }
        for c in cells {
            terrain.entry(c.mesh.to_ascii_lowercase()).or_insert(c);
        }
    }
    Ok(Scan {
        res_names,
        instances,
        terrain: terrain.into_values().collect(),
    })
}

#[derive(Default, Clone)]
struct Resolved {
    mesh: Option<String>,
    col: Option<String>,
}

fn resolve_resource(src: &Source, res: &str) -> Resolved {
    let mut out = Resolved::default();
    let mut col_stems = vec![stem(res).to_owned()];
    if res.ends_with(".xlmsh") {
        let level0 = src
            .resolve(res)
            .and_then(|s| src.read(s).ok())
            .and_then(|b| Xlmsh::parse(&b).ok())
            .and_then(|l| {
                l.levels
                    .iter()
                    .find(|v| v.source_index == 0)
                    .map(|v| base_lower(&v.mesh))
            });
        if let Some(m) = level0.filter(|m| m.ends_with(".xcmsh")) {
            col_stems.push(stem(&m).to_owned());
            out.mesh = Some(m);
        }
    } else {
        out.mesh = Some(res.to_owned());
    }
    out.mesh = out.mesh.filter(|m| src.resolve(m).is_some());
    let extra: Vec<String> = col_stems
        .iter()
        .filter_map(|s| s.strip_suffix("_lowpoly").map(str::to_owned))
        .collect();
    out.col = col_stems
        .into_iter()
        .chain(extra)
        .map(|s| format!("{s}_col.xnvmsh"))
        .find(|c| src.resolve(c).is_some());
    out
}

/// Terrain bounds in Bevy convention for one landscape cell mesh.
fn terrain_bounds(run: &Run, name: &str) -> Option<TerrainBounds> {
    let slot = run.src.resolve(name)?;
    let (stamp, _) = run.src.stamp(slot);
    let key = run.salted(stamp, "terrain-bounds");
    if let Some(b) = run.bounds.lock().expect("bounds").get(name)
        && b.key == key
    {
        return Some(b.clone());
    }
    let mesh = Xcmsh::parse(&run.src.read(slot).ok()?).ok()?;
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in mesh.submeshes.iter().flat_map(|s| s.positions.iter()) {
        let p = [-p[0], p[1], p[2]];
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    (lo[0] <= hi[0]).then(|| {
        let b = TerrainBounds {
            key,
            min: lo,
            max: hi,
        };
        run.bounds
            .lock()
            .expect("bounds")
            .insert(name.to_owned(), b.clone());
        b
    })
}

fn bounds_in_region(r: &Region, b: &TerrainBounds) -> bool {
    // Bevy x = -gothic x.
    r.overlaps_box([-b.max[0], b.min[2]], [-b.min[0], b.max[2]])
}

pub fn cook(o: &Options) -> Result<Report> {
    let t0 = Instant::now();
    let out = o.out_root.join(&o.profile.name);
    std::fs::create_dir_all(&out)?;
    let src = Source::open(&o.data_dir)?;
    let prev = Manifest::load(&out)
        .filter(|m| {
            m.profile == o.profile.name && m.profile_settings_hash == o.profile.settings_hash()
        })
        .unwrap_or_default();
    let run = Run {
        o,
        out: out.clone(),
        bounds: Mutex::new(prev.terrain_bounds.clone()),
        prev,
        src,
        entries: Mutex::default(),
        failures: Mutex::default(),
        stats: Mutex::default(),
    };
    if !o.quiet {
        eprintln!(
            "cooking {} -> {} (profile {}, {} threads){}",
            o.data_dir.display(),
            out.display(),
            o.profile.name,
            rayon::current_num_threads(),
            o.region
                .as_ref()
                .map_or(String::new(), |r| format!(", region {}", r.name))
        );
    }

    // ---- database
    if run.want(CookKind::Db) {
        run.timed("db", || cook_db(&run));
    }

    // ---- world scan + resource resolution (defines the scope of everything else)
    let needs_world = [
        CookKind::Mesh,
        CookKind::Collision,
        CookKind::Material,
        CookKind::Texture,
        CookKind::World,
    ]
    .iter()
    .any(|k| run.want(*k));
    let mut scan = None;
    let mut resolved: HashMap<String, Resolved> = HashMap::new();
    let mut mesh_names: BTreeSet<String> = BTreeSet::new();
    let mut col_names: BTreeSet<String> = BTreeSet::new();
    if needs_world {
        let t = Instant::now();
        let s = scan_world(&run)?;
        let prog = Progress::new("resolve resources", s.res_names.len(), o.quiet);
        resolved = s
            .res_names
            .par_iter()
            .map(|n| {
                let r = resolve_resource(&run.src, n);
                prog.tick();
                (n.clone(), r)
            })
            .collect();
        for r in resolved.values() {
            mesh_names.extend(r.mesh.iter().cloned());
            col_names.extend(r.col.iter().cloned());
        }
        if o.region.is_none() && run.want(CookKind::Mesh) {
            // Full cook: every standalone render mesh (props, items, creatures' attachments).
            mesh_names.extend(
                run.src
                    .list(".xcmsh")
                    .into_iter()
                    .map(|(p, _)| base_lower(&p))
                    .filter(|n| !n.contains("_landscape_cell_")),
            );
        }
        run.stat("world", |st| st.seconds += t.elapsed().as_secs_f64());
        scan = Some(s);
    }

    // ---- terrain selection
    let mut terrain_sel: Vec<(TerrainCell, Option<TerrainBounds>)> = Vec::new();
    if let Some(s) = &scan {
        let prog = Progress::new("terrain bounds", s.terrain.len(), o.quiet);
        let needs_bounds = o.region.is_some() || !run.want(CookKind::Mesh);
        let sel: Vec<_> = s
            .terrain
            .par_iter()
            .map(|c| {
                let b = if needs_bounds {
                    terrain_bounds(&run, &c.mesh.to_ascii_lowercase())
                } else {
                    None
                };
                prog.tick();
                (c.clone(), b)
            })
            .collect();
        for (c, b) in sel {
            if let (Some(r), Some(b)) = (&o.region, &b)
                && !bounds_in_region(r, b)
            {
                continue;
            }
            if o.region.is_some() && b.is_none() {
                continue;
            }
            terrain_sel.push((c, b));
        }
    }

    // ---- meshes
    if run.want(CookKind::Mesh) {
        run.timed("mesh", || cook_meshes(&run, &mesh_names, &terrain_sel));
    }
    // Terrain bounds known after cooking (incremental runs reuse them from the manifest).
    if scan.is_some() {
        for (c, b) in &mut terrain_sel {
            if b.is_none() {
                *b = terrain_bounds(&run, &c.mesh.to_ascii_lowercase());
            }
        }
    }

    // ---- collision pack
    if run.want(CookKind::Collision) {
        run.timed("collision", || cook_collision(&run, &col_names));
    }

    // ---- actors / motions (their refs feed materials and textures)
    if run.want(CookKind::Actor) {
        run.timed("actor", || cook_actors(&run));
    }
    if run.want(CookKind::Motion) {
        run.timed("motion", || cook_motions(&run));
    }
    carry_forward(&run, "actors/", !run.want(CookKind::Actor));
    carry_forward(&run, "motions/", !run.want(CookKind::Motion));

    // ---- materials + textures
    if run.want(CookKind::Material) || run.want(CookKind::Texture) {
        let mut mats: BTreeSet<String> = BTreeSet::new();
        let terrain_names = terrain_sel.iter().map(|(c, _)| c.mesh.to_ascii_lowercase());
        for n in mesh_names.iter().cloned().chain(terrain_names) {
            let rel = mesh_path(AssetId::of(&n));
            mats.extend(
                run.refs_of(&rel)
                    .into_iter()
                    .map(|m| m.to_ascii_lowercase()),
            );
        }
        for (rel, e) in run.entries.lock().expect("entries").iter() {
            if rel.starts_with("actors/") {
                mats.extend(
                    e.refs
                        .iter()
                        .filter_map(|r| r.strip_prefix("mat:"))
                        .map(str::to_ascii_lowercase),
                );
            }
        }
        mats.remove("");
        let tex_refs = run.timed("material", || cook_materials(&run, &mats));
        if run.want(CookKind::Texture) {
            run.timed("texture", || cook_textures(&run, tex_refs));
        }
    }
    carry_forward(&run, "textures/", !run.want(CookKind::Texture));
    carry_forward(&run, "meshes/", !run.want(CookKind::Mesh));

    // ---- world cells
    if run.want(CookKind::World)
        && let Some(s) = &scan
    {
        run.timed("world", || cook_cells(&run, s, &resolved, &terrain_sel));
    }
    carry_forward(&run, "cells/", !run.want(CookKind::World));
    if !run.want(CookKind::Db)
        && let Some(e) = run.prev.entries.get(DB_PATH)
    {
        run.entries
            .lock()
            .expect("entries")
            .insert(DB_PATH.into(), e.clone());
    }
    if !run.want(CookKind::Collision)
        && let Some(e) = run.prev.entries.get(COLLISION_PATH)
    {
        run.entries
            .lock()
            .expect("entries")
            .insert(COLLISION_PATH.into(), e.clone());
    }

    // ---- manifest
    let failures = run.failures.lock().expect("failures").clone();
    let manifest = Manifest {
        cooker_version: COOKER_VERSION,
        profile: o.profile.name.clone(),
        profile_settings_hash: o.profile.settings_hash(),
        source_root: o.data_dir.display().to_string(),
        source_fingerprint: run.src.fingerprint,
        cooked_at_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
        region: o.region.as_ref().map(|r| r.name.clone()),
        entries: run.entries.lock().expect("entries").clone(),
        terrain_bounds: run.bounds.lock().expect("bounds").clone(),
        failures: failures.clone(),
    };
    manifest.save(&out)?;
    Ok(Report {
        out_dir: out,
        stats: run.stats.lock().expect("stats").clone(),
        total_bytes: manifest.total_bytes(),
        seconds: t0.elapsed().as_secs_f64(),
        failures,
    })
}

/// Keep previous entries of an unselected kind so a partial cook does not drop them.
fn carry_forward(run: &Run, prefix: &str, active: bool) {
    if !active {
        return;
    }
    let mut e = run.entries.lock().expect("entries");
    for (k, v) in &run.prev.entries {
        if k.starts_with(prefix) {
            e.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
}

fn cook_db(run: &Run) {
    let dir = &run.o.data_dir;
    let stamps: Vec<u64> = ["quests.pak", "infos.pak", "strings.pak", "templates.pak"]
        .iter()
        .map(|n| run.src.archive_stamps.get(*n).copied().unwrap_or(0))
        .collect();
    let key = run.salted(
        hash64(format!("{stamps:?}|{}", run.o.language).as_bytes()),
        "db",
    );
    run.emit(
        "db",
        DB_PATH,
        key,
        "GameDatabase",
        "quests/infos/strings/templates.pak",
        || {
            let mut db = GameDatabase::load_install(dir, &run.o.language)?;
            let _ = db.load_item_names(dir);
            let payload = crate::db::encode(&db)?;
            let mut w = Writer::new();
            w.raw(payload);
            Ok((w.finish(Kind::Database, key), vec![]))
        },
    );
}

fn cook_meshes(
    run: &Run,
    names: &BTreeSet<String>,
    terrain: &[(TerrainCell, Option<TerrainBounds>)],
) {
    let mut jobs: Vec<(String, bool)> = names.iter().map(|n| (n.clone(), false)).collect();
    jobs.extend(
        terrain
            .iter()
            .map(|(c, _)| (c.mesh.to_ascii_lowercase(), true)),
    );
    let prog = Progress::new("meshes", jobs.len(), run.o.quiet);
    jobs.par_iter().for_each(|(name, is_terrain)| {
        let Some(slot) = run.src.resolve(name) else {
            run.fail("mesh", name, "source not found");
            return;
        };
        let (stamp, archive) = run.src.stamp(slot);
        let key = run.salted(stamp, if *is_terrain { "mesh-terrain" } else { "mesh" });
        let rel = mesh_path(AssetId::of(name));
        let o = run.emit("mesh", &rel, key, name, &archive, || {
            let src = Xcmsh::parse(&run.src.read(slot)?)?;
            let cooked = cook_mesh(&src, &run.o.profile, *is_terrain, key)?;
            if *is_terrain {
                run.bounds.lock().expect("bounds").insert(
                    name.clone(),
                    TerrainBounds {
                        key: run.salted(stamp, "terrain-bounds"),
                        min: cooked.bounds.0,
                        max: cooked.bounds.1,
                    },
                );
            }
            let mut refs: Vec<String> = cooked.material_names.clone();
            refs.sort();
            refs.dedup();
            Ok((cooked.bytes, refs))
        });
        let _ = matches!(o, Outcome::Failed);
        prog.tick();
    });
}

fn cook_collision(run: &Run, names: &BTreeSet<String>) {
    let items: Vec<(String, Slot)> = names
        .iter()
        .filter_map(|n| run.src.resolve(n).map(|s| (n.clone(), s)))
        .collect();
    let mut sig = Vec::new();
    for (n, s) in &items {
        sig.push(format!("{n}:{}", run.src.stamp(*s).0));
    }
    let key = run.salted(hash64(sig.join("|").as_bytes()), "collision");
    let prog = Progress::new("collision", items.len(), run.o.quiet);
    run.emit(
        "collision",
        COLLISION_PATH,
        key,
        "collision pack",
        "",
        || {
            let parsed: Vec<(u64, Vec<[f32; 3]>, Vec<u32>)> = items
                .par_iter()
                .filter_map(|(n, s)| {
                    prog.tick();
                    let nav = Xnvmsh::parse(&run.src.read(*s).ok()?).ok()?;
                    let (mut v, mut idx) = (Vec::new(), Vec::new());
                    for m in &nav.meshes {
                        let base = v.len() as u32;
                        v.extend(m.positions.iter().map(|p| {
                            let c = p.map(|x| x * gothic_formats::xnvmsh::UNITS_PER_METRE);
                            [-c[0], c[1], c[2]]
                        }));
                        idx.extend(m.indices.iter().map(|i| base + i));
                    }
                    (!idx.is_empty()).then(|| (AssetId::of(n).0, v, idx))
                })
                .collect();
            let mut sorted = parsed;
            sorted.sort_by_key(|p| p.0);
            let mut b = CollisionBuilder::default();
            for (id, v, i) in &sorted {
                b.add(*id, v, i);
            }
            Ok((b.finish(key), vec![]))
        },
    );
}

fn cook_actors(run: &Run) {
    let list = run.src.list(".xact");
    let prog = Progress::new("actors", list.len(), run.o.quiet);
    list.par_iter().for_each(|(path, slot)| {
        let name = base_lower(path);
        let (stamp, archive) = run.src.stamp(*slot);
        let key = run.salted(stamp, "actor");
        run.emit(
            "actor",
            &actor_path(AssetId::of(&name)),
            key,
            &name,
            &archive,
            || {
                let a = Actor::parse(&run.src.read(*slot)?)?;
                let c = cook_actor(&a, key, &|n| run.src.resolve(n).is_some())?;
                let mut refs: Vec<String> = c
                    .textures
                    .iter()
                    .map(|(t, srgb)| format!("tex:{t}|{}", u8::from(*srgb)))
                    .chain(c.materials.iter().map(|m| format!("mat:{m}")))
                    .collect();
                refs.sort();
                refs.dedup();
                Ok((c.bytes, refs))
            },
        );
        prog.tick();
    });
}

fn cook_motions(run: &Run) {
    let list = run.src.list(".xmot");
    let prog = Progress::new("motions", list.len(), run.o.quiet);
    list.par_iter().for_each(|(path, slot)| {
        let name = base_lower(path);
        let (stamp, archive) = run.src.stamp(*slot);
        let key = run.salted(stamp, "motion");
        run.emit(
            "motion",
            &motion_path(AssetId::of(&name)),
            key,
            &name,
            &archive,
            || {
                let m = Motion::parse(&run.src.read(*slot)?)?;
                Ok((cook_motion(&m, &run.o.profile, key)?, vec![]))
            },
        );
        prog.tick();
    });
}

/// Resolve materials into the table. Returns the texture references `(name, srgb)`.
fn cook_materials(run: &Run, names: &BTreeSet<String>) -> BTreeMap<String, bool> {
    let items: Vec<(String, Slot)> = names
        .iter()
        .filter_map(|n| run.src.resolve(n).map(|s| (n.clone(), s)))
        .collect();
    let sig: Vec<String> = items
        .iter()
        .map(|(n, s)| format!("{n}:{}", run.src.stamp(*s).0))
        .collect();
    let key = run.salted(hash64(sig.join("|").as_bytes()), "materials");
    let mut tex: BTreeMap<String, bool> = BTreeMap::new();
    let mut add = |name: &str, srgb: bool| {
        *tex.entry(name.to_owned()).or_insert(false) |= srgb;
    };
    // Actor textures.
    for (rel, e) in run.entries.lock().expect("entries").iter() {
        if rel.starts_with("actors/") {
            for r in &e.refs {
                if let Some(t) = r.strip_prefix("tex:")
                    && let Some((n, s)) = t.rsplit_once('|')
                {
                    add(n, s == "1");
                }
            }
        }
    }
    if !run.want(CookKind::Material) {
        // Reuse the previous table's texture refs.
        if let Some(e) = run.prev.entries.get(MATERIALS_PATH) {
            run.entries
                .lock()
                .expect("entries")
                .insert(MATERIALS_PATH.into(), e.clone());
            for r in &e.refs {
                if let Some((n, s)) = r.rsplit_once('|') {
                    add(n, s == "1");
                }
            }
        }
        return tex;
    }
    let prog = Progress::new("materials", items.len(), run.o.quiet);
    let table_refs: std::cell::RefCell<Vec<String>> = Default::default();
    let skipped_refs = (!run.o.force)
        .then(|| run.prev.entries.get(MATERIALS_PATH))
        .flatten()
        .filter(|e| e.key == key && run.out.join(MATERIALS_PATH).is_file())
        .map(|e| e.refs.clone());
    let outcome = run.emit(
        "material",
        MATERIALS_PATH,
        key,
        "material table",
        "",
        || {
            let resolved: Vec<Option<material::Resolved>> = items
                .par_iter()
                .map(|(n, s)| {
                    prog.tick();
                    run.src.read(*s).ok().and_then(|b| material::resolve(n, &b))
                })
                .collect();
            let mut recs: Vec<MaterialRec> = Vec::new();
            let mut refs: BTreeMap<String, bool> = BTreeMap::new();
            for r in resolved.into_iter().flatten() {
                recs.push(r.rec);
                for (t, srgb) in r.textures {
                    *refs.entry(t).or_insert(false) |= srgb;
                }
            }
            let list: Vec<String> = refs
                .iter()
                .map(|(n, s)| format!("{n}|{}", u8::from(*s)))
                .collect();
            *table_refs.borrow_mut() = list.clone();
            Ok((material::write_table(recs, key), list))
        },
    );
    let refs = match (outcome, skipped_refs) {
        (Outcome::Skipped, Some(r)) => r,
        _ => table_refs.into_inner(),
    };
    for r in &refs {
        if let Some((n, s)) = r.rsplit_once('|') {
            add(n, s == "1");
        }
    }
    tex
}

fn cook_textures(run: &Run, refs: BTreeMap<String, bool>) {
    let jobs: Vec<(String, bool)> = refs.into_iter().collect();
    let prog = Progress::new("textures", jobs.len(), run.o.quiet);
    jobs.par_iter().for_each(|(name, srgb)| {
        prog.tick();
        let Some(slot) = run.src.resolve(name) else {
            run.stat("texture", |s| s.missing += 1);
            return;
        };
        let (stamp, archive) = run.src.stamp(slot);
        let key = run.salted(stamp, &format!("tex|{srgb}"));
        run.emit(
            "texture",
            &texture_path(AssetId::of(name)),
            key,
            name,
            &archive,
            || {
                let bytes = run.src.read(slot)?;
                let c = cook_texture(&bytes, &run.o.profile, *srgb, key)?;
                Ok((c.bytes, vec![]))
            },
        );
    });
}

fn cook_cells(
    run: &Run,
    scan: &Scan,
    resolved: &HashMap<String, Resolved>,
    terrain: &[(TerrainCell, Option<TerrainBounds>)],
) {
    let region = run.o.region.as_ref();
    let mut cells: HashMap<(i32, i32), CellBuilder> = HashMap::new();
    // Only reference collision meshes the pack really holds (some xnvmsh files are empty).
    let packed: std::collections::HashSet<u64> =
        crate::mmap::Blob::map(&run.out.join(COLLISION_PATH))
            .ok()
            .and_then(|b| crate::world::CollisionPack::parse(std::sync::Arc::new(b)).ok())
            .map(|p| p.records().iter().map(|r| r.id).collect())
            .unwrap_or_default();
    for inst in &scan.instances {
        let Some(r) = resolved.get(&scan.res_names[inst.res as usize]) else {
            continue;
        };
        let Some(mesh) = &r.mesh else { continue };
        let (cx, cz) = cell_of(inst.matrix[3][0], inst.matrix[3][2]);
        cell_mut(&mut cells, cx, cz).instances.push(Instance {
            mesh: AssetId::of(mesh).0,
            collision: r
                .col
                .as_ref()
                .map(|c| AssetId::of(c).0)
                .filter(|id| packed.contains(id))
                .unwrap_or(0),
            transform: bevy_transform(&inst.matrix),
        });
    }
    // Terrain: references in every overlapped cell, collision triangles by centroid.
    let prog = Progress::new("terrain collision", terrain.len(), run.o.quiet);
    let tris: Vec<Vec<CellTri>> = terrain
        .par_iter()
        .map(|(c, _)| {
            prog.tick();
            let mut out = Vec::new();
            let nav = c.mesh.to_ascii_lowercase().replace(".xcmsh", ".xnvmsh");
            let Some(nav) = run
                .src
                .resolve(&nav)
                .and_then(|s| run.src.read(s).ok())
                .and_then(|b| Xnvmsh::parse(&b).ok())
            else {
                return out;
            };
            for t in nav.triangles_cm() {
                let t = t.map(|p| [-p[0], p[1], p[2]]);
                let gx = -(t[0][0] + t[1][0] + t[2][0]) / 3.0;
                let gz = (t[0][2] + t[1][2] + t[2][2]) / 3.0;
                if region.is_some_and(|r| !r.overlaps_box([gx, gz], [gx, gz])) {
                    continue;
                }
                out.push((cell_of(gx, gz), t));
            }
            out
        })
        .collect();
    for ((cx, cz), t) in tris.into_iter().flatten() {
        cell_mut(&mut cells, cx, cz).collision.extend_from_slice(&t);
    }
    for (c, b) in terrain {
        let Some(b) = b else { continue };
        let (x0, z0) = cell_of(-b.max[0], b.min[2]);
        let (x1, z1) = cell_of(-b.min[0], b.max[2]);
        let r = TerrainRef {
            mesh: AssetId::of(&c.mesh).0,
            bounds_min: b.min,
            bounds_max: b.max,
        };
        for cx in x0..=x1 {
            for cz in z0..=z1 {
                if region.is_some_and(|rg| {
                    !rg.overlaps_box(
                        [
                            cx as f32 * crate::world::CELL_SIZE,
                            cz as f32 * crate::world::CELL_SIZE,
                        ],
                        [
                            (cx + 1) as f32 * crate::world::CELL_SIZE,
                            (cz + 1) as f32 * crate::world::CELL_SIZE,
                        ],
                    )
                }) {
                    continue;
                }
                cell_mut(&mut cells, cx, cz).terrain.push(r);
            }
        }
    }
    let list: Vec<CellBuilder> = cells.into_values().collect();
    let prog = Progress::new("cells", list.len(), run.o.quiet);
    list.into_par_iter().for_each(|c| {
        let (cx, cz) = (c.cx, c.cz);
        let rel = cell_path(cx, cz);
        // Content-addressed: hash the cell body, then stamp the key into the header (offset 16).
        let n = c.instances.len();
        let mut bytes = c.write(0);
        let key = run.salted(hash64(&bytes), "cell");
        bytes[16..24].copy_from_slice(&key.to_le_bytes());
        run.emit("world", &rel, key, &rel, "", || {
            Ok((bytes, vec![format!("instances:{n}")]))
        });
        prog.tick();
    });
}

/// Convenience for callers that only know a profile name.
pub fn profile_or_err(name: &str) -> Result<Profile> {
    match Profile::by_name(name) {
        Some(p) => Ok(p),
        None => bail!(
            "unknown profile {name:?}; expected one of {:?}",
            Profile::all_names()
        ),
    }
}

type CellTri = ((i32, i32), [[f32; 3]; 3]);

fn cell_mut(cells: &mut HashMap<(i32, i32), CellBuilder>, cx: i32, cz: i32) -> &mut CellBuilder {
    cells
        .entry((cx, cz))
        .or_insert_with(|| CellBuilder::new(cx, cz))
}
