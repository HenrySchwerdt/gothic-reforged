//! Extracts the authored ground cover (`eCVegetation_PS` grids of the `_cstat.node` documents) around Ardea from the
//! user's own game installation (read-only) into the Quest pack: `veg/<cx>_<cz>.gveg` per Ardea cell plus the brush
//! meshes / materials / textures they need (copied from the cooked `quest` cache).
//! Usage: pack_veg <game Data dir> <pack dir> [radius_cm=25000]
//!
//! `.gveg`: `b"GVG1"`, `u32 count`, then `count` records of 52 little-endian bytes in Bevy convention (cm, X mirrored):
//! `u64 mesh AssetId, f32 wind strength, u32 colour, f32 pos[3], f32 quat[4], f32 scale[2]`.
use anyhow::{Context, Result};
use gothic_cook::{AssetId, Cache};
use gothic_formats::terrain::vegetation;
use gothic_formats::vfs::VirtualFileSystem;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

fn sector_of(document: &str) -> Option<(f32, f32)> {
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

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let data = PathBuf::from(a.get(1).context("game Data dir")?);
    let pack = PathBuf::from(a.get(2).context("pack dir")?);
    let radius: f32 = a.get(3).and_then(|v| v.parse().ok()).unwrap_or(25_000.0);
    let center = [92_400.0f32, -11_000.0f32];
    let cache = Cache::open_default("quest")?;
    let mut vfs = VirtualFileSystem::open_data_dir(&data)?;
    let docs: Vec<(String, gothic_formats::vfs::Slot)> = vfs
        .iter()
        .filter(|(p, _)| p.ends_with("_cstat.node"))
        .filter_map(|(p, s)| {
            let (x, z) = sector_of(p)?;
            ((x - center[0]).abs() <= radius + 10_000.0
                && (z - center[1]).abs() <= radius + 10_000.0)
                .then(|| (p.to_string(), s))
        })
        .collect();
    println!("{} cstat documents near Ardea", docs.len());
    let mut cells: BTreeMap<(i32, i32), Vec<[u8; 52]>> = BTreeMap::new();
    let mut brush: BTreeSet<String> = BTreeSet::new();
    let mut per_mesh: HashMap<String, usize> = HashMap::new();
    for (path, slot) in docs {
        let Ok(bytes) = vfs.read(slot) else { continue };
        let Ok(v) = vegetation::extract(&bytes) else {
            continue;
        };
        for area in &v.areas {
            let Some(grid) = &area.grid else { continue };
            let by_id: HashMap<u16, &vegetation::BrushMesh> =
                area.meshes.iter().map(|m| (m.mesh_id, m)).collect();
            for e in &grid.entries {
                let Some(m) = by_id.get(&e.mesh_id) else {
                    continue;
                };
                let (gx, gz) = (e.position[0], e.position[2]);
                if ((gx - center[0]).powi(2) + (gz - center[1]).powi(2)).sqrt() > radius + 7_100.0 {
                    continue;
                }
                let cell = (
                    (gx / 10_000.0).floor() as i32,
                    (gz / 10_000.0).floor() as i32,
                );
                // Left-handed Gothic space -> Bevy: mirror X (a reflection flips the y/z quaternion parts).
                let pos = [-e.position[0], e.position[1], e.position[2]];
                let q = [e.rotation[0], -e.rotation[1], -e.rotation[2], e.rotation[3]];
                let mut r = [0u8; 52];
                let mut o = 0;
                let mut put = |b: &[u8]| {
                    r[o..o + b.len()].copy_from_slice(b);
                    o += b.len();
                };
                put(&AssetId::of(&m.mesh).0.to_le_bytes());
                put(&m.wind_strength.to_le_bytes());
                put(&e.color.to_le_bytes());
                for f in pos.iter().chain(q.iter()).chain(e.scale.iter()) {
                    put(&f.to_le_bytes());
                }
                debug_assert_eq!(o, 52);
                cells.entry(cell).or_default().push(r);
                brush.insert(m.mesh.clone());
                *per_mesh.entry(m.mesh.clone()).or_default() += 1;
            }
        }
        let _ = path;
    }
    std::fs::create_dir_all(pack.join("veg"))?;
    let mut total = 0;
    for ((cx, cz), recs) in &cells {
        if !pack.join(format!("cells/{cx}_{cz}.gcell")).is_file() {
            continue;
        }
        let mut out = Vec::with_capacity(8 + recs.len() * 52);
        out.extend_from_slice(b"GVG1");
        out.extend_from_slice(&(recs.len() as u32).to_le_bytes());
        for r in recs {
            out.extend_from_slice(r);
        }
        std::fs::write(pack.join(format!("veg/{cx}_{cz}.gveg")), out)?;
        total += recs.len();
    }
    println!(
        "{} cells with ground cover, {total} instances, {} brush meshes",
        cells.len(),
        brush.len()
    );
    // Brush meshes + textures from the cooked cache.
    std::fs::create_dir_all(pack.join("meshes"))?;
    std::fs::create_dir_all(pack.join("textures"))?;
    let root = cache.root().to_path_buf();
    let (mut missing, mut ntex) = (0, 0);
    let mut texs = BTreeSet::new();
    for name in &brush {
        let id = AssetId::of(name);
        let src = root.join(format!("meshes/{}.gmsh", id.hex()));
        if !src.is_file() {
            println!(
                "MISSING brush mesh in cache: {name} ({} instances)",
                per_mesh[name]
            );
            missing += 1;
            continue;
        }
        std::fs::copy(&src, pack.join(format!("meshes/{}.gmsh", id.hex())))?;
        let md = cache.load_mesh(id)?;
        for s in md.submeshes() {
            match cache.material(s.material) {
                Some(mat) => {
                    for l in mat.layers.iter().take(mat.layer_count as usize) {
                        texs.extend(l.tex.iter().copied().filter(|t| *t != 0));
                    }
                }
                None => println!("MISSING material for {name}"),
            }
        }
    }
    for t in &texs {
        let src = root.join(format!("textures/{}.gtex", AssetId(*t).hex()));
        if src.is_file() {
            std::fs::copy(
                &src,
                pack.join(format!("textures/{}.gtex", AssetId(*t).hex())),
            )?;
            ntex += 1;
        }
    }
    std::fs::copy(root.join("materials.gmat"), pack.join("materials.gmat"))?;
    let mut counts: Vec<_> = per_mesh.into_iter().collect();
    counts.sort_by_key(|c| std::cmp::Reverse(c.1));
    for (n, c) in counts.iter().take(40) {
        println!("  {c:7} {n}");
    }
    println!(
        "textures {ntex}/{} copied, {missing} brush meshes missing",
        texs.len()
    );
    let _ = Path::new("");
    Ok(())
}
