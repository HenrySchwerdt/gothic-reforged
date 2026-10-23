//! Builds a self-contained subset of the cooked `quest` cache for Ardea (cells in a radius around the plaza)
//! that can be pushed to the headset. Usage: pack_ardea <out_dir> [radius_cm=25000] [profile=quest]
use anyhow::Result;
use gothic_cook::{AssetId, Cache, cook::Region};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let out = PathBuf::from(a.get(1).expect("out dir"));
    let radius: f32 = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(25_000.0);
    let profile = a.get(3).map_or("quest", String::as_str);
    let cache = Cache::open_default(profile)?;
    let mut region = Region::by_name("ardea").unwrap();
    region.radius = radius;
    let (c0, r) = (region.center, region.radius);
    let (cx0, cx1) = (
        ((c0[0] - r) / 10000.0).floor() as i32,
        ((c0[0] + r) / 10000.0).floor() as i32,
    );
    let (cz0, cz1) = (
        ((c0[1] - r) / 10000.0).floor() as i32,
        ((c0[1] + r) / 10000.0).floor() as i32,
    );
    std::fs::create_dir_all(out.join("cells"))?;
    std::fs::create_dir_all(out.join("meshes"))?;
    std::fs::create_dir_all(out.join("textures"))?;
    let root = cache.root().to_path_buf();
    let mut meshes = BTreeSet::new();
    let mut ncell = 0;
    let (mut inst, mut terr) = (0, 0);
    for cx in cx0..=cx1 {
        for cz in cz0..=cz1 {
            if let Some(cell) = cache.load_cell(cx, cz)? {
                std::fs::copy(
                    root.join(format!("cells/{cx}_{cz}.gcell")),
                    out.join(format!("cells/{cx}_{cz}.gcell")),
                )?;
                ncell += 1;
                for i in cell.instances() {
                    meshes.insert(i.mesh);
                }
                for t in cell.terrain() {
                    meshes.insert(t.mesh);
                }
                inst += cell.instances().len();
                terr += cell.terrain().len();
            }
        }
    }
    let mut texs = BTreeSet::new();
    let mut nmesh = 0;
    let mut missing = 0;
    for m in &meshes {
        let src = root.join(format!("meshes/{}.gmsh", AssetId(*m).hex()));
        if !src.is_file() {
            missing += 1;
            continue;
        }
        std::fs::copy(&src, out.join(format!("meshes/{}.gmsh", AssetId(*m).hex())))?;
        nmesh += 1;
        let md = cache.load_mesh(AssetId(*m))?;
        for s in md.submeshes() {
            if let Some(mat) = cache.material(s.material) {
                for l in mat.layers.iter().take(mat.layer_count as usize) {
                    for t in l.tex {
                        if t != 0 {
                            texs.insert(t);
                        }
                    }
                }
            }
        }
    }
    let mut ntex = 0;
    for t in &texs {
        let src = root.join(format!("textures/{}.gtex", AssetId(*t).hex()));
        if src.is_file() {
            std::fs::copy(
                &src,
                out.join(format!("textures/{}.gtex", AssetId(*t).hex())),
            )?;
            ntex += 1;
        }
    }
    std::fs::copy(root.join("materials.gmat"), out.join("materials.gmat"))?;
    std::fs::write(out.join("manifest.json"), b"{}")?;
    println!(
        "cells {ncell} (x {cx0}..{cx1}, z {cz0}..{cz1}) instances {inst} terrain refs {terr} meshes {nmesh} (missing {missing}) textures {ntex}"
    );
    Ok(())
}
