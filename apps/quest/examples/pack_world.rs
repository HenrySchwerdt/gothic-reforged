//! Host tool (reads the user's own Gothic 3 install and cooked `quest` cache read-only, writes under the gitignored
//! `.local/`): cooks the WHOLE world into a streamable Quest pack.
//!
//! ```text
//! pack_world <out_dir> [--bounds cx0,cz0,cx1,cz1] [--survey] [--no-npc] [--npc-in FILE] [--data DIR]
//! ```
//!
//! Pack layout (the `gothic_cook::Cache` layout, so the Ardea loader code reads it unchanged, plus):
//! * `cells/<cx>_<cz>.gcell`   one file per 100 m cell (instances, terrain refs, terrain collision)
//! * `meshes/*.gmsh`           shared by id; every submesh carries the quest profile's LOD0..2 index ranges
//! * `textures/*.gtex`         ASTC 6x6, shared by id (content-addressed by name, so deduplicated across cells)
//! * `materials.gmat`, `collision.gcol`, `actors/*.gact`
//! * `world.idx`               cell table with the mesh/texture ids of each cell and the size tables
//!   (lets the streamer pin resources, estimate memory and prefetch without touching the disk)
//! * `world_npcs.gnpc`         NPC manifest (looks, placements, clip names), written when `.local/quest/world_npcs_in.tsv`
//!   (`scripts/quest/pack_npcs.py`) exists
//! * `clips_npc.grcp`          original idle / routine clips of every skeleton family used by the manifest
//!
//! Submeshes whose material name contains `world_lowpoly` are the distant-proxy atlas that overlaps the real houses
//! (the runtime skips them, the desktop loader does too), so their textures are not packed.
use anyhow::{Context, Result, bail};
use bevy::prelude::AlphaMode;
use gothic_animation::ActorLibrary;
use gothic_animation::clip_pack::{ClipPack, PackClip};
use gothic_animation::names::MotionQuery;
use gothic_animation::stream::{AttachRequest, BuildRequest};
use gothic_cook::material::NONE_U32;
use gothic_cook::{AssetId, Cache, Profile};
use gothic_formats::vfs::VirtualFileSystem;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

const DEFAULT_DATA: &str = r"D:\SteamLibrary\steamapps\common\Gothic 3\Data";

fn hex(id: u64) -> String {
    AssetId(id).hex()
}

struct W(Vec<u8>);
impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
}

/// Copies `src` to `dst` unless an identical-size file is already there. Returns the byte size.
fn copy_if_needed(src: &Path, dst: &Path) -> Result<u64> {
    let len = std::fs::metadata(src)
        .with_context(|| format!("stat {}", src.display()))?
        .len();
    if std::fs::metadata(dst).map(|m| m.len()).ok() != Some(len) {
        std::fs::copy(src, dst).with_context(|| format!("copy {}", src.display()))?;
    }
    Ok(len)
}

struct MeshInfo {
    bytes: u64,
    tris: u32,
    textures: Vec<u64>,
}

struct CellRec {
    cx: i32,
    cz: i32,
    instances: u32,
    terrain: u32,
    tris: u32,
    meshes: Vec<u64>,
    textures: BTreeSet<u64>,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let out = PathBuf::from(args.get(1).context("usage: pack_world <out_dir> [flags]")?);
    let mut bounds: Option<[i32; 4]> = None;
    let mut survey = false;
    let mut want_npc = true;
    let mut npc_in = PathBuf::from(".local/quest/world_npcs_in.tsv");
    let mut data = PathBuf::from(DEFAULT_DATA);
    let mut extras = Some(PathBuf::from(".local/quest/ardea-pack")).filter(|p| p.is_dir());
    let mut it = args.iter().skip(2);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--survey" => survey = true,
            "--no-npc" => want_npc = false,
            "--bounds" => {
                let v: Vec<i32> = it
                    .next()
                    .context("--bounds needs cx0,cz0,cx1,cz1")?
                    .split(',')
                    .map(|s| s.parse())
                    .collect::<Result<_, _>>()?;
                bounds = Some(v.try_into().map_err(|_| anyhow::anyhow!("4 numbers"))?);
            }
            "--npc-in" => npc_in = it.next().context("--npc-in FILE")?.into(),
            "--extras" => extras = Some(it.next().context("--extras DIR")?.into()),
            "--data" => data = it.next().context("--data DIR")?.into(),
            o => bail!("unknown option {o}"),
        }
    }
    let cache = Cache::open_default("quest")?;
    let root = cache.root().to_path_buf();

    // ---- cells -----------------------------------------------------------------------------------------------------
    let mut coords: Vec<(i32, i32)> = std::fs::read_dir(root.join("cells"))?
        .filter_map(|e| {
            let n = e.ok()?.file_name().into_string().ok()?;
            let (a, b) = n.strip_suffix(".gcell")?.split_once('_')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        })
        .filter(|&(x, z)| {
            bounds.is_none_or(|b| x >= b[0] && x <= b[2] && z >= b[1] && z <= b[3])
        })
        .collect();
    coords.sort();
    let mut mesh_info: HashMap<u64, Option<MeshInfo>> = HashMap::new();
    let mut cells: Vec<CellRec> = Vec::new();
    for &(cx, cz) in &coords {
        let Some(cell) = cache.load_cell(cx, cz)? else {
            continue;
        };
        let mut rec = CellRec {
            cx,
            cz,
            instances: cell.instances().len() as u32,
            terrain: cell.terrain().len() as u32,
            tris: 0,
            meshes: Vec::new(),
            textures: BTreeSet::new(),
        };
        let mut set = BTreeSet::new();
        let ids = cell
            .instances()
            .iter()
            .map(|i| i.mesh)
            .chain(cell.terrain().iter().map(|t| t.mesh))
            .collect::<Vec<_>>();
        for &m in &ids {
            let info = mesh_info.entry(m).or_insert_with(|| mesh_meta(&cache, &root, m));
            if let Some(info) = info {
                rec.tris += info.tris;
                if set.insert(m) {
                    rec.textures.extend(info.textures.iter().copied());
                }
            }
        }
        rec.meshes = set.into_iter().filter(|m| mesh_info[m].is_some()).collect();
        cells.push(rec);
    }
    let all_meshes: BTreeSet<u64> = cells.iter().flat_map(|c| c.meshes.iter().copied()).collect();
    let mut all_tex: BTreeSet<u64> = cells.iter().flat_map(|c| c.textures.iter().copied()).collect();
    let mesh_bytes: u64 = all_meshes.iter().map(|m| mesh_info[m].as_ref().unwrap().bytes).sum();
    let tex_bytes: u64 = all_tex
        .iter()
        .filter_map(|t| std::fs::metadata(root.join(format!("textures/{}.gtex", hex(*t)))).ok())
        .map(|m| m.len())
        .sum();
    let cell_bytes: u64 = cells
        .iter()
        .filter_map(|c| std::fs::metadata(root.join(format!("cells/{}_{}.gcell", c.cx, c.cz))).ok())
        .map(|m| m.len())
        .sum();
    let col_bytes = std::fs::metadata(root.join("collision.gcol"))?.len();
    println!(
        "cells {} instances {} terrain refs {} | unique meshes {} ({:.0} MiB) textures {} ({:.0} MiB) cells {:.0} MiB collision {:.0} MiB | total {:.0} MiB",
        cells.len(),
        cells.iter().map(|c| c.instances as u64).sum::<u64>(),
        cells.iter().map(|c| c.terrain as u64).sum::<u64>(),
        all_meshes.len(),
        mesh_bytes as f64 / 1048576.0,
        all_tex.len(),
        tex_bytes as f64 / 1048576.0,
        cell_bytes as f64 / 1048576.0,
        col_bytes as f64 / 1048576.0,
        (mesh_bytes + tex_bytes + cell_bytes + col_bytes) as f64 / 1048576.0
    );
    if survey {
        return Ok(());
    }
    for d in ["cells", "meshes", "textures", "actors"] {
        std::fs::create_dir_all(out.join(d))?;
    }

    // ---- NPC manifest (also adds actors, textures, attachment meshes and clips) --------------------------------------
    let mut extra_meshes: BTreeSet<u64> = BTreeSet::new();
    let mut npc_per_cell: HashMap<(i32, i32), u32> = HashMap::new();
    if want_npc && npc_in.is_file() {
        let n = pack_npcs(
            &cache,
            &root,
            &out,
            &data,
            &npc_in,
            &mut all_tex,
            &mut extra_meshes,
            &mut npc_per_cell,
        )?;
        println!("npc manifest: {n} placements");
    } else if want_npc {
        println!(
            "no {} (run scripts/quest/pack_npcs.py): NPC manifest skipped",
            npc_in.display()
        );
    }

    // ---- copy world assets ---------------------------------------------------------------------------------------
    let mut copied = 0u64;
    for c in &cells {
        let n = format!("cells/{}_{}.gcell", c.cx, c.cz);
        copied += copy_if_needed(&root.join(&n), &out.join(&n))?;
    }
    let mesh_set: BTreeSet<u64> = all_meshes.iter().chain(extra_meshes.iter()).copied().collect();
    for m in &mesh_set {
        let n = format!("meshes/{}.gmsh", hex(*m));
        copied += copy_if_needed(&root.join(&n), &out.join(&n))?;
    }
    let (mut ntex, mut missing_tex) = (0, 0);
    for t in &all_tex {
        let n = format!("textures/{}.gtex", hex(*t));
        let dst = out.join(&n);
        if root.join(&n).is_file() {
            copied += copy_if_needed(&root.join(&n), &dst)?;
            ntex += 1;
        } else if dst.is_file() {
            ntex += 1; // cooked from the install by the NPC phase
        } else {
            missing_tex += 1;
        }
    }
    for f in ["materials.gmat", "collision.gcol"] {
        copied += copy_if_needed(&root.join(f), &out.join(f))?;
    }
    std::fs::write(out.join("manifest.json"), b"{}")?;
    // The Ardea pack's extras (hero/orc actors, weapons, clips, vegetation, game data) make this pack a superset of it,
    // so every system that reads `data_dir()` keeps working from the world pack.
    if let Some(ex) = &extras {
        let mut n = 0;
        for d in ["actors", "meshes", "textures", "veg"] {
            n += copy_dir(&ex.join(d), &out.join(d))?;
        }
        for f in ["clips_hero.grcp", "clips_orc.grcp", "items.json", "orcs.json"] {
            if ex.join(f).is_file() {
                copy_if_needed(&ex.join(f), &out.join(f))?;
            }
        }
        println!("extras from {}: {} files merged", ex.display(), n);
    }

    // ---- world.idx -------------------------------------------------------------------------------------------------
    let mesh_list: Vec<u64> = mesh_set.iter().copied().collect();
    let mesh_ix: HashMap<u64, u32> = mesh_list.iter().enumerate().map(|(i, m)| (*m, i as u32)).collect();
    let tex_list: Vec<u64> = all_tex.iter().copied().collect();
    let tex_ix: HashMap<u64, u32> = tex_list.iter().enumerate().map(|(i, m)| (*m, i as u32)).collect();
    let mut w = W(Vec::new());
    w.0.extend_from_slice(b"GWIDX001");
    w.u32(mesh_list.len() as u32);
    for m in &mesh_list {
        let (bytes, tris) = mesh_info
            .get(m)
            .and_then(|i| i.as_ref())
            .map_or((0, 0), |i| (i.bytes, i.tris_of_mesh()));
        w.u64(*m);
        w.u32(bytes as u32);
        w.u32(tris);
    }
    w.u32(tex_list.len() as u32);
    for t in &tex_list {
        let b = std::fs::metadata(out.join(format!("textures/{}.gtex", hex(*t))))
            .map_or(0, |m| m.len());
        w.u64(*t);
        w.u32(b as u32);
    }
    w.u32(cells.len() as u32);
    for c in &cells {
        w.i32(c.cx);
        w.i32(c.cz);
        w.u32(c.instances);
        w.u32(c.terrain);
        w.u32(c.tris);
        w.u32(npc_per_cell.get(&(c.cx, c.cz)).copied().unwrap_or(0));
        w.u32(c.meshes.len() as u32);
        let tx: Vec<u32> = c.textures.iter().filter_map(|t| tex_ix.get(t).copied()).collect();
        w.u32(tx.len() as u32);
        for m in &c.meshes {
            w.u32(mesh_ix[m]);
        }
        for t in tx {
            w.u32(t);
        }
    }
    std::fs::write(out.join("world.idx"), &w.0)?;
    let total: u64 = walk_size(&out);
    println!(
        "pack {}: {} cells, {} meshes, {} textures ({} missing in the cache), copied {:.0} MiB this run, pack size {:.1} MiB ({:.2} GiB)",
        out.display(),
        cells.len(),
        mesh_set.len(),
        ntex,
        missing_tex,
        copied as f64 / 1048576.0,
        total as f64 / 1048576.0,
        total as f64 / 1073741824.0
    );
    Ok(())
}

impl MeshInfo {
    fn tris_of_mesh(&self) -> u32 {
        self.tris
    }
}

fn copy_dir(src: &Path, dst: &Path) -> Result<usize> {
    if !src.is_dir() {
        return Ok(0);
    }
    std::fs::create_dir_all(dst)?;
    let mut n = 0;
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let to = dst.join(e.file_name());
        if e.file_type()?.is_dir() {
            n += copy_dir(&e.path(), &to)?;
        } else {
            copy_if_needed(&e.path(), &to)?;
            n += 1;
        }
    }
    Ok(n)
}

fn walk_size(p: &Path) -> u64 {
    let mut t = 0;
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let m = e.metadata();
            if let Ok(m) = m {
                if m.is_dir() {
                    t += walk_size(&e.path());
                } else {
                    t += m.len();
                }
            }
        }
    }
    t
}

/// Size, LOD0 triangle count and texture ids (lowpoly proxy submeshes excluded) of a cooked mesh.
fn mesh_meta(cache: &Cache, root: &Path, id: u64) -> Option<MeshInfo> {
    let f = root.join(format!("meshes/{}.gmsh", hex(id)));
    let bytes = std::fs::metadata(&f).ok()?.len();
    let md = cache.load_mesh(AssetId(id)).ok()?;
    let names = md.material_names();
    let mut tris = 0;
    let mut textures = Vec::new();
    for (si, s) in md.submeshes().iter().enumerate() {
        if names
            .get(si)
            .is_some_and(|n| n.to_ascii_lowercase().contains("world_lowpoly"))
        {
            continue;
        }
        tris += md.indices(s, 0).len() as u32 / 3;
        if let Some(mat) = cache.material(s.material) {
            for l in mat.layers.iter().take(mat.layer_count as usize) {
                textures.extend(l.tex.iter().copied().filter(|&t| t != 0));
            }
        }
    }
    textures.sort_unstable();
    textures.dedup();
    Some(MeshInfo {
        bytes,
        tris,
        textures,
    })
}

// ======================================================================================================================
// NPC manifest
// ======================================================================================================================

struct LookIn {
    body: (String, i32),
    extra: Vec<(String, i32)>,
    att: Vec<(String, i32, String)>,
}
struct NpcIn {
    pos: [f32; 3],
    yaw: f32,
    look: u32,
    action: String,
    species: String,
    tier: u8,
}

fn parse_sw(s: &str) -> (String, i32) {
    let (n, sw) = s.rsplit_once('@').unwrap_or((s, "-1"));
    (n.to_string(), sw.parse().unwrap_or(-1))
}

fn part_name(stem: &str, sw: i32) -> String {
    if sw >= 0 {
        format!("{stem}@M{sw}")
    } else {
        stem.to_string()
    }
}

/// Per-submesh texture/blend override resolved by the animation library (material switch applied).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct SubOv {
    tex: u64,
    /// 0 opaque, 1 alpha test (mask reference below).
    kind: u8,
    mask: u8,
}

/// Quest renderer has no sorted transparency: blended materials of the original (hair strands, lashes) are drawn
/// alpha-tested at this reference (`MASK_FOR_BLEND` of 0..255).
const MASK_FOR_BLEND: u8 = 128;

fn sub_ov(m: &gothic_animation::stream::CpuMesh) -> SubOv {
    let tex = m.material.texture.as_ref().map_or(0, |(k, _)| {
        let id = AssetId::of(k).0;
        TEX_NAMES.with(|t| t.borrow_mut().insert(id, k.clone()));
        id
    });
    let (kind, mask) = match m.material.material.alpha_mode {
        AlphaMode::Mask(v) => (1, (v * 255.0).round().clamp(1.0, 255.0) as u8),
        AlphaMode::Blend | AlphaMode::Add => (1, MASK_FOR_BLEND),
        _ => (0, 0),
    };
    SubOv { tex, kind, mask }
}

#[derive(Clone)]
struct Variant {
    actor: String,
    subs: Vec<SubOv>,
    lod: [u32; 2],
}

struct AttachOut {
    mesh: String,
    joint: String,
    offset: [f32; 12],
    ov: SubOv,
}

struct LookOut {
    family: String,
    fix: [f32; 4],
    parts: Vec<u32>,
    att: Vec<AttachOut>,
}

struct Strings {
    list: Vec<String>,
    ix: HashMap<String, u32>,
}
impl Strings {
    fn id(&mut self, s: &str) -> u32 {
        if let Some(&i) = self.ix.get(s) {
            return i;
        }
        let i = self.list.len() as u32;
        self.list.push(s.to_string());
        self.ix.insert(s.to_string(), i);
        i
    }
}

fn actor_file(stem: &str) -> String {
    format!("{}.xact", stem.to_ascii_lowercase())
}

#[allow(clippy::too_many_arguments)]
fn pack_npcs(
    cache: &Cache,
    root: &Path,
    out: &Path,
    data: &Path,
    npc_in: &Path,
    all_tex: &mut BTreeSet<u64>,
    extra_meshes: &mut BTreeSet<u64>,
    npc_per_cell: &mut HashMap<(i32, i32), u32>,
) -> Result<usize> {
    let text = std::fs::read_to_string(npc_in)?;
    let mut looks_in: BTreeMap<u32, LookIn> = BTreeMap::new();
    let mut npcs_in: Vec<NpcIn> = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.first().copied() {
            Some("L") if f.len() >= 5 => {
                let id: u32 = f[1].parse()?;
                looks_in.insert(
                    id,
                    LookIn {
                        body: parse_sw(f[2]),
                        extra: f[3].split(',').filter(|s| !s.is_empty()).map(parse_sw).collect(),
                        att: f[4]
                            .split(',')
                            .filter(|s| !s.is_empty())
                            .map(|s| {
                                let mut p = s.splitn(3, '@');
                                (
                                    p.next().unwrap_or("").to_string(),
                                    p.next().and_then(|v| v.parse().ok()).unwrap_or(0),
                                    p.next().unwrap_or("").to_string(),
                                )
                            })
                            .collect(),
                    },
                );
            }
            Some("N") if f.len() >= 10 => npcs_in.push(NpcIn {
                pos: [f[1].parse()?, f[2].parse()?, f[3].parse()?],
                yaw: f[4].parse()?,
                look: f[5].parse()?,
                action: f[6].to_string(),
                species: f[7].to_string(),
                tier: f[9].parse().unwrap_or(0),
            }),
            _ => {}
        }
    }
    println!("npc input: {} looks, {} placements", looks_in.len(), npcs_in.len());
    let mut lib = ActorLibrary::open(data)?;
    let mut vfs = VirtualFileSystem::open_data_dir(data)?;
    let profile = Profile::by_name("quest").context("quest profile")?;

    let mut variants: Vec<Variant> = Vec::new();
    let mut variant_ix: HashMap<String, u32> = HashMap::new();
    let mut looks: BTreeMap<u32, LookOut> = BTreeMap::new();
    let mut failed = 0;
    let mut ov_mismatch = 0;
    let t0 = std::time::Instant::now();
    for (&lid, l) in &looks_in {
        let mut names = vec![part_name(&l.body.0, l.body.1)];
        names.extend(l.extra.iter().map(|(s, w)| part_name(s, *w)));
        let mut req = BuildRequest::new(names.clone());
        // The installation does not ship every item mesh (the engine shows nothing for those either).
        let atts: Vec<&(String, i32, String)> =
            l.att.iter().filter(|(m, _, _)| lib.has_mesh(m)).collect();
        for (mesh, sw, slot) in &atts {
            req.attachments.push(AttachRequest {
                mesh: format!("{mesh}@M{sw}"),
                slot: slot.clone(),
                holster_slot: None,
            });
        }
        let bundle = match lib.build_bundle(&req) {
            Ok(b) => b,
            Err(e) => {
                if failed < 8 {
                    println!("look {lid} {:?}: {e:#}", names);
                }
                failed += 1;
                continue;
            }
        };
        let mut part_ix = Vec::new();
        let mut ok = true;
        for (k, name) in names.iter().enumerate() {
            let key = name.to_ascii_lowercase();
            if let Some(&i) = variant_ix.get(&key) {
                part_ix.push(i);
                continue;
            }
            let stem = if k == 0 { &l.body.0 } else { &l.extra[k - 1].0 };
            let cooked = cache.load_actor(AssetId::of(&actor_file(stem))).ok();
            let Some(cooked) = cooked else {
                ok = false;
                break;
            };
            let subs = overrides_for(&bundle.parts[k].meshes, cooked.submeshes().len());
            if subs.is_empty() && !bundle.parts[k].meshes.is_empty() {
                ov_mismatch += 1;
            }
            let id = variants.len() as u32;
            variants.push(Variant {
                actor: actor_file(stem),
                subs,
                lod: [0, 0],
            });
            variant_ix.insert(key, id);
            part_ix.push(id);
        }
        if !ok {
            failed += 1;
            continue;
        }
        let mut att = Vec::new();
        for (a, ca) in atts.iter().zip(&bundle.attachments) {
            let joint = bundle
                .joint_by_name
                .iter()
                .find(|&(_, &j)| j == ca.joint)
                .map(|(n, _)| n.clone())
                .unwrap_or_default();
            let ov = ca.meshes.first().map(sub_ov).unwrap_or(SubOv {
                tex: 0,
                kind: 0,
                mask: 0,
            });
            let m = ca.offset;
            let c = [
                m.matrix3.x_axis,
                m.matrix3.y_axis,
                m.matrix3.z_axis,
                m.translation,
            ];
            let mut offset = [0.0; 12];
            for (i, col) in c.iter().enumerate() {
                offset[i * 3] = col.x;
                offset[i * 3 + 1] = col.y;
                offset[i * 3 + 2] = col.z;
            }
            if !joint.is_empty() && cache.has_mesh(AssetId::of(&a.0.to_ascii_lowercase())) {
                att.push(AttachOut {
                    mesh: a.0.to_ascii_lowercase(),
                    joint,
                    offset,
                    ov,
                });
            }
        }
        looks.insert(
            lid,
            LookOut {
                family: bundle.rig.family.clone(),
                fix: bundle.facing_fix.to_array(),
                parts: part_ix,
                att,
            },
        );
    }
    println!(
        "looks resolved {} / {} ({} failed, {} part override mismatches) in {:.0}s, {} part variants",
        looks.len(),
        looks_in.len(),
        failed,
        ov_mismatch,
        t0.elapsed().as_secs_f32(),
        variants.len()
    );

    // LOD actors: `<stem>_lod1/_lod2` when the cooked file is clearly lighter than LOD0 (original actor LODs).
    let base: Vec<Variant> = variants.clone();
    for (i, v) in base.iter().enumerate() {
        let Ok(a0) = cache.load_actor(AssetId::of(&v.actor)) else {
            continue;
        };
        let t0 = a0.indices().len();
        let stem = v.actor.trim_end_matches(".xact");
        let name = variant_ix
            .iter()
            .find(|&(_, &x)| x as usize == i)
            .map(|(k, _)| k.clone())
            .unwrap_or_default();
        let sw = name.rsplit_once("@m").map(|(_, s)| s.to_string());
        let mut prev = t0;
        for (li, suffix) in ["_lod1", "_lod2"].iter().enumerate() {
            let lod_actor = format!("{stem}{suffix}.xact");
            let Ok(al) = cache.load_actor(AssetId::of(&lod_actor)) else {
                continue;
            };
            let n = al.indices().len();
            if n * 10 > prev * 7 || n == 0 {
                continue; // not at least 30 % lighter than the previous stage
            }
            let part = match &sw {
                Some(s) => format!("{}{suffix}@M{s}", stem.trim_end_matches(".xact")),
                None => format!("{stem}{suffix}"),
            };
            let Ok(b) = lib.build_bundle(&BuildRequest::new([part.clone()])) else {
                continue;
            };
            let subs = overrides_for(&b.parts[0].meshes, al.submeshes().len());
            let id = variants.len() as u32;
            variants.push(Variant {
                actor: lod_actor,
                subs,
                lod: [0, 0],
            });
            variants[i].lod[li] = id + 1;
            prev = n;
        }
    }
    let lod_variants = variants.iter().filter(|v| v.lod != [0, 0]).count();
    println!(
        "{} part variants have lighter actor LODs (total variants {})",
        lod_variants,
        variants.len()
    );

    // ---- clips ------------------------------------------------------------------------------------------------------
    let families: BTreeSet<String> = looks.values().map(|l| l.family.clone()).collect();
    let mut clip_names: BTreeSet<String> = BTreeSet::new();
    let mut idle_of: HashMap<String, String> = HashMap::new();
    let mut action_of: HashMap<(String, String), String> = HashMap::new();
    let actions: BTreeSet<String> = npcs_in.iter().map(|n| n.action.clone()).collect();
    for fam in &families {
        let find = |q: &MotionQuery| lib.catalog.find(fam, q).map(str::to_string);
        let idle = find(&MotionQuery {
            pose: Some("Stand"),
            left: Some("None"),
            right: Some("None"),
            pn: Some("P0"),
            action: Some("Ambient"),
            phase: Some("Loop"),
            mode: Some("N"),
            dir: Some("Fwd"),
        })
        .or_else(|| {
            find(&MotionQuery {
                pose: Some("Stand"),
                action: Some("Ambient"),
                phase: Some("Loop"),
                mode: Some("N"),
                ..Default::default()
            })
        })
        .or_else(|| {
            find(&MotionQuery {
                action: Some("Ambient"),
                phase: Some("Loop"),
                mode: Some("N"),
                ..Default::default()
            })
        });
        let Some(idle) = idle else {
            println!("family {fam}: no Ambient loop clip");
            continue;
        };
        clip_names.insert(idle.clone());
        idle_of.insert(fam.clone(), idle);
        for act in &actions {
            if act == "Ambient" {
                continue;
            }
            let c = find(&MotionQuery {
                pose: Some("Stand"),
                action: Some(act),
                phase: Some("Loop"),
                mode: Some("N"),
                ..Default::default()
            });
            if let Some(c) = c {
                clip_names.insert(c.clone());
                action_of.insert((fam.clone(), act.clone()), c);
            }
        }
    }
    let mut pack = ClipPack::default();
    for n in &clip_names {
        let clip = lib.clip(n)?;
        pack.clips.push(PackClip {
            clip: (*clip).clone(),
            fps: 30.0,
            timing: None,
        });
    }
    let bytes = pack.to_bytes();
    std::fs::write(out.join("clips_npc.grcp"), &bytes)?;
    println!(
        "clips_npc.grcp: {} clips, {} families {:?}, {} KiB",
        pack.clips.len(),
        families.len(),
        families,
        bytes.len() / 1024
    );

    // ---- assets: actors, their textures, attachment meshes ------------------------------------------------------------
    let mut cooked_tex = 0;
    let mut need_tex: BTreeSet<u64> = BTreeSet::new();
    for v in &variants {
        let id = AssetId::of(&v.actor);
        let src = root.join(format!("actors/{}.gact", id.hex()));
        if src.is_file() {
            copy_if_needed(&src, &out.join(format!("actors/{}.gact", id.hex())))?;
        }
        if let Ok(a) = cache.load_actor(id) {
            for m in a.materials() {
                if m.diffuse_tex != 0 {
                    need_tex.insert(m.diffuse_tex);
                }
                if let Some(rec) = cache.material(m.shader_material) {
                    for l in rec.layers.iter().take(rec.layer_count.max(1) as usize) {
                        need_tex.extend(l.tex.iter().copied().filter(|&t| t != 0));
                    }
                }
            }
        }
        need_tex.extend(v.subs.iter().map(|s| s.tex).filter(|&t| t != 0));
    }
    for l in looks.values() {
        for a in &l.att {
            let id = AssetId::of(&a.mesh);
            extra_meshes.insert(id.0);
            if let Some(info) = mesh_meta(cache, root, id.0) {
                need_tex.extend(info.textures);
            }
            if a.ov.tex != 0 {
                need_tex.insert(a.ov.tex);
            }
        }
    }
    // Textures the quest cache did not cook (material-switch variants): cook them from the install now.
    let tex_names = variant_texture_names(&looks, &variants, &mut lib);
    for t in &need_tex {
        let in_cache = root.join(format!("textures/{}.gtex", hex(*t))).is_file();
        let dst = out.join(format!("textures/{}.gtex", hex(*t)));
        if in_cache || dst.is_file() {
            continue;
        }
        let Some(name) = tex_names.get(t) else {
            continue;
        };
        let Some(slot) = vfs.resolve_name(name) else {
            continue;
        };
        let bytes = vfs.read(slot)?;
        match gothic_cook::texture::cook_texture(&bytes, &profile, true, *t) {
            Ok(c) => {
                std::fs::write(&dst, c.bytes)?;
                cooked_tex += 1;
            }
            Err(e) => println!("cook {name}: {e:#}"),
        }
    }
    println!(
        "npc textures: {} needed, {} cooked from the install (not in the quest cache)",
        need_tex.len(),
        cooked_tex
    );
    all_tex.extend(need_tex);

    // ---- write world_npcs.gnpc -------------------------------------------------------------------------------------
    let mut st = Strings {
        list: Vec::new(),
        ix: HashMap::new(),
    };
    let mut body = W(Vec::new());
    body.u32(variants.len() as u32);
    for v in &variants {
        body.u32(st.id(&v.actor));
        body.u32(v.lod[0]);
        body.u32(v.lod[1]);
        body.u32(v.subs.len() as u32);
        for s in &v.subs {
            body.u64(s.tex);
            body.u8(s.kind);
            body.u8(s.mask);
            body.u8(0);
            body.u8(0);
        }
    }
    let look_ids: Vec<u32> = looks.keys().copied().collect();
    let look_ix: HashMap<u32, u32> = look_ids.iter().enumerate().map(|(i, l)| (*l, i as u32)).collect();
    body.u32(look_ids.len() as u32);
    for lid in &look_ids {
        let l = &looks[lid];
        body.u32(st.id(&l.family));
        for q in l.fix {
            body.f32(q);
        }
        body.u32(l.parts.len() as u32);
        for p in &l.parts {
            body.u32(*p);
        }
        body.u32(l.att.len() as u32);
        for a in &l.att {
            body.u32(st.id(&a.mesh));
            body.u32(st.id(&a.joint));
            for f in a.offset {
                body.f32(f);
            }
            body.u64(a.ov.tex);
            body.u8(a.ov.kind);
            body.u8(a.ov.mask);
            body.u8(0);
            body.u8(0);
        }
    }
    let mut placed: Vec<(&NpcIn, u32, u32, u32)> = Vec::new();
    for n in &npcs_in {
        let Some(&li) = look_ix.get(&n.look) else {
            continue;
        };
        let fam = &looks[&n.look].family;
        let Some(idle) = idle_of.get(fam) else {
            continue;
        };
        let routine = action_of
            .get(&(fam.clone(), n.action.clone()))
            .map_or(u32::MAX, |c| st.id(c));
        let idle = st.id(idle);
        placed.push((n, li, idle, routine));
    }
    // Sorted by cell (Gothic x = -bevy x).
    let cell_of = |n: &NpcIn| ((-n.pos[0] / 10_000.0).floor() as i32, (n.pos[2] / 10_000.0).floor() as i32);
    placed.sort_by_key(|(n, ..)| cell_of(n));
    body.u32(placed.len() as u32);
    for (n, li, idle, routine) in &placed {
        *npc_per_cell.entry(cell_of(n)).or_default() += 1;
        for v in n.pos {
            body.f32(v);
        }
        body.f32(n.yaw);
        body.u32(*li);
        body.u32(*idle);
        body.u32(*routine);
        body.u32(st.id(&n.species));
        body.u32(u32::from(n.tier));
    }
    let mut file = W(Vec::new());
    file.0.extend_from_slice(b"GNPC0001");
    file.u32(st.list.len() as u32);
    for s in &st.list {
        file.0.extend_from_slice(&(s.len() as u16).to_le_bytes());
        file.0.extend_from_slice(s.as_bytes());
    }
    file.0.extend_from_slice(&body.0);
    std::fs::write(out.join("world_npcs.gnpc"), &file.0)?;
    // Summary by species / family.
    let mut by_family: BTreeMap<&str, usize> = BTreeMap::new();
    for (n, ..) in &placed {
        *by_family.entry(looks_in.get(&n.look).map_or("?", |_| looks[&n.look].family.as_str())).or_default() += 1;
    }
    println!("placements by clip family: {by_family:?}");
    let _ = NONE_U32;
    Ok(placed.len())
}

/// Texture overrides of a part in cooked-submesh order (bundle meshes without geometry are dropped the same way the
/// cooker drops them); empty when the counts disagree (then the cooked material is used unchanged).
fn overrides_for(meshes: &[gothic_animation::stream::CpuMesh], cooked_subs: usize) -> Vec<SubOv> {
    let v: Vec<SubOv> = meshes
        .iter()
        .filter(|m| {
            m.mesh.count_vertices() > 0 && m.mesh.indices().is_some_and(|i| i.len() >= 3)
        })
        .map(sub_ov)
        .collect();
    if v.len() == cooked_subs { v } else { Vec::new() }
}

/// Source texture name (`<stem>.ximg`) of every override texture id.
fn variant_texture_names(
    _looks: &BTreeMap<u32, LookOut>,
    variants: &[Variant],
    lib: &mut ActorLibrary,
) -> HashMap<u64, String> {
    // The ids are `AssetId::of("<lower stem>.ximg")`; the stems come from the library's image index through the
    // bundle keys, which `sub_ov` hashed. Rebuild the table from every candidate switch stem we can name.
    let _ = (variants, lib);
    TEX_NAMES.with(|t| t.borrow().clone())
}

thread_local! {
    static TEX_NAMES: std::cell::RefCell<HashMap<u64, String>> = std::cell::RefCell::new(HashMap::new());
}
