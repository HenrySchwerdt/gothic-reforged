//! Integrity check of a cooked profile: every manifest artifact must parse, mesh indices
//! must be in range, and every cell instance must reference a cooked mesh and material.
//!
//! cargo run --release -p gothic-cook --example verify -- [PROFILE_DIR]
use anyhow::Result;
use gothic_cook::{AssetId, Cache};
use rayon::prelude::*;
use std::collections::BTreeSet;

fn main() -> Result<()> {
    let dir = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| gothic_cook::default_cache_root().join("desktop-high"));
    let cache = Cache::open(&dir)?;
    let m = cache.manifest().expect("manifest");
    let (mut bad, mut checked) = (0usize, 0usize);
    let mut report = |what: String| {
        bad += 1;
        if bad <= 20 {
            println!("  BAD {what}");
        }
    };
    let rels: Vec<&String> = m.entries.keys().collect();
    let results: Vec<Option<String>> = rels
        .par_iter()
        .map(|rel| {
            let id = |p: &str, ext: &str| {
                let hex = rel.strip_prefix(p)?.strip_suffix(ext)?;
                u64::from_str_radix(hex, 16).ok().map(AssetId)
            };
            if let Some(i) = id("meshes/", ".gmsh") {
                let mesh = match cache.load_mesh(i) {
                    Ok(m) => m,
                    Err(e) => return Some(format!("{rel}: {e:#}")),
                };
                for s in mesh.submeshes() {
                    for lod in 0..s.lod_count as usize {
                        let idx = mesh.indices(s, lod);
                        if idx.is_empty()
                            || idx.len() % 3 != 0
                            || (0..idx.len()).any(|k| idx.get(k) >= s.vertex_count)
                        {
                            return Some(format!("{rel}: bad indices lod {lod}"));
                        }
                    }
                }
                if mesh
                    .vertices()
                    .iter()
                    .any(|v| !v.pos.iter().all(|c| c.is_finite()))
                {
                    return Some(format!("{rel}: non-finite vertex"));
                }
            } else if let Some(i) = id("textures/", ".gtex")
                && let Err(e) = cache.load_texture(i)
            {
                return Some(format!("{rel}: {e:#}"));
            } else if let Some(i) = id("actors/", ".gact")
                && let Err(e) = cache.load_actor(i)
            {
                return Some(format!("{rel}: {e:#}"));
            } else if let Some(i) = id("motions/", ".gmot")
                && let Err(e) = cache.load_motion(i)
            {
                return Some(format!("{rel}: {e:#}"));
            }
            None
        })
        .collect();
    for r in results {
        checked += 1;
        if let Some(r) = r {
            report(r);
        }
    }
    // Cells: references resolve.
    let (mut cells, mut instances, mut missing_mesh, mut missing_col, mut tris) = (0, 0, 0, 0, 0);
    let mut missing_mat: BTreeSet<u64> = BTreeSet::new();
    for rel in m.entries.keys().filter(|r| r.starts_with("cells/")) {
        let name = rel.trim_start_matches("cells/").trim_end_matches(".gcell");
        let (cx, cz) = name.split_once('_').expect("cell name");
        let Some(c) = cache.load_cell(cx.parse()?, cz.parse()?)? else {
            continue;
        };
        cells += 1;
        tris += c.collision_triangles().count();
        for i in c.instances() {
            instances += 1;
            if !cache.has_mesh(AssetId(i.mesh)) {
                missing_mesh += 1;
            }
            if i.collision != 0 && cache.collision(i.collision).is_none() {
                missing_col += 1;
            }
        }
        for t in c.terrain() {
            if let Ok(mesh) = cache.load_mesh(AssetId(t.mesh)) {
                for s in mesh.submeshes() {
                    if cache.material(s.material).is_none() {
                        missing_mat.insert(s.material);
                    }
                }
            } else {
                missing_mesh += 1;
            }
        }
    }
    println!(
        "{checked} artifacts checked, {bad} bad; {cells} cells, {instances} instances, {tris} terrain collision tris; missing: {missing_mesh} meshes, {missing_col} collision, {} terrain materials; {} materials in table",
        missing_mat.len(),
        cache.material_count()
    );
    std::process::exit(i32::from(bad > 0));
}
