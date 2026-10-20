//! Load-time comparison: original parse path vs cooked cache path.
//!
//! cargo run --release -p gothic-cook --example bench -- [--data DIR] [--cache PROFILE_DIR]
//!                                                      [--language English]
//!
//! (a) the Ardea region: every cell, mesh, material and texture within the cooked region;
//! (b) the GameDatabase.
//! Both paths are single-threaded and touch every byte they would upload/consume.
use anyhow::{Context, Result};
use gothic_cook::cache::Cache;
use gothic_cook::ids::AssetId;
use gothic_cook::material::tex_id;
use gothic_cook::mesh::Vertex;
use gothic_formats::{
    vfs::VirtualFileSystem,
    xcmsh::Xcmsh,
    ximg::Ximg,
    xshmat::{Xshmat, graph::MaterialGraph},
};
use gothic_gameplay::db::GameDatabase;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Instant;

fn touch(bytes: &[u8]) -> u64 {
    bytes.iter().step_by(64).map(|&b| u64::from(b)).sum()
}

fn arg(name: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter()
        .position(|x| x == name)
        .and_then(|i| a.get(i + 1).cloned())
}

fn main() -> Result<()> {
    let data = PathBuf::from(
        arg("--data").unwrap_or_else(|| r"D:\SteamLibrary\steamapps\common\Gothic 3\Data".into()),
    );
    let language = arg("--language").unwrap_or_else(|| "English".into());
    let cache = Cache::open(
        arg("--cache")
            .map(PathBuf::from)
            .unwrap_or_else(|| gothic_cook::default_cache_root().join("desktop-high")),
    )?;
    let manifest = cache.manifest().context("manifest")?;
    println!(
        "cache {} ({:.1} MiB)",
        cache.root().display(),
        manifest.total_bytes() as f64 / 1_048_576.0
    );

    // ---------------- cooked path: gather the working set
    let t = Instant::now();
    let mut sink = 0u64;
    let mut meshes: BTreeSet<u64> = BTreeSet::new();
    let (mut cells, mut instances) = (0, 0);
    // Cells overlapping the Ardea disc (centre 92400,-11000, radius 25000).
    for cx in 6..=11 {
        for cz in -4..=1 {
            if let Some(c) = cache.load_cell(cx, cz)? {
                cells += 1;
                instances += c.instances().len();
                sink += touch(bytemuck::cast_slice(c.instances()));
                sink += touch(bytemuck::cast_slice(c.collision_corners()));
                meshes.extend(c.instances().iter().map(|i| i.mesh));
                meshes.extend(c.terrain().iter().map(|t| t.mesh));
            }
        }
    }
    let cooked_cells = t.elapsed();

    let t = Instant::now();
    let mut mats: BTreeSet<u64> = BTreeSet::new();
    let (mut n_mesh, mut verts) = (0usize, 0usize);
    for id in &meshes {
        if let Ok(m) = cache.load_mesh(AssetId(*id)) {
            n_mesh += 1;
            verts += m.vertices().len();
            sink += touch(bytemuck::cast_slice::<Vertex, u8>(m.vertices()));
            mats.extend(m.submeshes().iter().map(|s| s.material));
        }
    }
    let cooked_meshes = t.elapsed();

    let t = Instant::now();
    let mut texs: BTreeSet<u64> = BTreeSet::new();
    for m in &mats {
        if let Some(r) = cache.material(*m) {
            for l in &r.layers[..r.layer_count as usize] {
                texs.extend(l.tex.iter().filter(|t| **t != 0));
            }
            texs.extend(r.normal_tex.iter().filter(|t| **t != 0));
        }
    }
    let cooked_materials = t.elapsed();

    let t = Instant::now();
    let (mut n_tex, mut tex_bytes) = (0usize, 0usize);
    for id in &texs {
        if let Ok(tx) = cache.load_texture(AssetId(*id)) {
            n_tex += 1;
            tex_bytes += tx.data().len();
            sink += touch(tx.data());
        }
    }
    let cooked_textures = t.elapsed();
    let cooked_total = cooked_cells + cooked_meshes + cooked_materials + cooked_textures;

    // ---------------- original path over the same working set
    let names: BTreeMap<u64, String> = manifest
        .entries
        .iter()
        .filter(|(k, _)| k.starts_with("meshes/"))
        .map(|(_, e)| (AssetId::of(&e.name).0, e.name.clone()))
        .collect();
    let t = Instant::now();
    let mut vfs = VirtualFileSystem::open_data_dir(&data)?;
    let orig_open = t.elapsed();

    let t = Instant::now();
    let index = gothic_importer::world_index::WorldIndex::build(&mut vfs)?;
    let hits = index.query_xz([92_400.0, 0.0, -11_000.0], 25_000.0).len();
    let orig_index = t.elapsed();

    let t = Instant::now();
    let mut o_mats: BTreeSet<String> = BTreeSet::new();
    let (mut o_mesh, mut o_verts) = (0usize, 0usize);
    for id in &meshes {
        let Some(name) = names.get(id) else { continue };
        let Some(slot) = vfs.resolve_name(name) else {
            continue;
        };
        let Ok(bytes) = vfs.read(slot) else { continue };
        let Ok(x) = Xcmsh::parse(&bytes) else {
            continue;
        };
        o_mesh += 1;
        for s in &x.submeshes {
            // What a loader must do to feed the GPU: build interleaved vertices.
            let v: Vec<Vertex> = (0..s.positions.len())
                .map(|k| Vertex {
                    pos: s.positions[k],
                    normal: s.normals.get(k).copied().unwrap_or_default(),
                    uv: s.uvs.get(k).copied().unwrap_or_default(),
                    ..Default::default()
                })
                .collect();
            o_verts += v.len();
            sink += touch(bytemuck::cast_slice(&v));
            o_mats.insert(s.material.to_ascii_lowercase());
        }
    }
    let orig_meshes = t.elapsed();

    let t = Instant::now();
    let mut o_texs: BTreeSet<String> = BTreeSet::new();
    for m in &o_mats {
        let Some(slot) = vfs.resolve_name(m) else {
            continue;
        };
        let Ok(bytes) = vfs.read(slot) else { continue };
        let Ok(x) = Xshmat::parse(&bytes) else {
            continue;
        };
        let g = MaterialGraph::parse(&bytes).ok().and_then(|g| g.surface());
        // Same texture set the cooked material table keeps: colour layers + normal maps.
        let mut paths: Vec<String> = Vec::new();
        if let Some(r) = g {
            paths.extend(
                r.layers
                    .iter()
                    .flat_map(|l| l.textures.iter().take(2).map(|t| t.path.clone())),
            );
            paths.extend(r.normals.iter().take(2).flatten().map(|t| t.path.clone()));
        } else {
            use gothic_formats::xshmat::TextureRole;
            paths.extend(x.texture(TextureRole::Diffuse).map(|t| t.path.clone()));
            paths.extend(x.texture(TextureRole::Normal).map(|t| t.path.clone()));
        }
        o_texs.extend(paths.iter().map(|p| {
            let s = p.rsplit(['/', '\\']).next().unwrap_or(p);
            format!(
                "{}.ximg",
                s.rsplit_once('.').map_or(s, |x| x.0).to_ascii_lowercase()
            )
        }));
    }
    let orig_materials = t.elapsed();

    let t = Instant::now();
    let (mut o_tex, mut o_tex_bytes) = (0usize, 0usize);
    for name in &o_texs {
        let Some(slot) = vfs.resolve_name(name) else {
            continue;
        };
        let Ok(bytes) = vfs.read(slot) else { continue };
        let Ok(img) = Ximg::parse(&bytes) else {
            continue;
        };
        o_tex += 1;
        for m in img.mips() {
            o_tex_bytes += m.len();
            sink += touch(m);
        }
    }
    let orig_textures = t.elapsed();
    let orig_total = orig_open + orig_index + orig_meshes + orig_materials + orig_textures;
    let _ = tex_id;

    println!("\n(a) Ardea region working set");
    println!(
        "    cooked: {cells} cells, {instances} instances, {n_mesh} meshes ({verts} verts), {} materials, {n_tex} textures ({:.1} MiB)",
        mats.len(),
        tex_bytes as f64 / 1_048_576.0
    );
    println!(
        "    original: {hits} world records queried, {o_mesh} meshes ({o_verts} verts), {} materials, {o_tex} textures ({:.1} MiB)",
        o_mats.len(),
        o_tex_bytes as f64 / 1_048_576.0
    );
    let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
    println!(
        "    {:<28}{:>12}{:>12}",
        "stage", "original ms", "cooked ms"
    );
    println!(
        "    {:<28}{:>12.1}{:>12}",
        "open VFS",
        ms(orig_open),
        "0 (mmap)"
    );
    println!(
        "    {:<28}{:>12.1}{:>12.1}",
        "cells / world index",
        ms(orig_index),
        ms(cooked_cells)
    );
    println!(
        "    {:<28}{:>12.1}{:>12.1}",
        "meshes",
        ms(orig_meshes),
        ms(cooked_meshes)
    );
    println!(
        "    {:<28}{:>12.1}{:>12.1}",
        "materials",
        ms(orig_materials),
        ms(cooked_materials)
    );
    println!(
        "    {:<28}{:>12.1}{:>12.1}",
        "textures",
        ms(orig_textures),
        ms(cooked_textures)
    );
    println!(
        "    {:<28}{:>12.1}{:>12.1}  ({:.0}x)",
        "TOTAL",
        ms(orig_total),
        ms(cooked_total),
        ms(orig_total) / ms(cooked_total).max(0.001)
    );

    // ---------------- (b) GameDatabase
    let t = Instant::now();
    let mut db = GameDatabase::load_install(&data, &language)?;
    let _ = db.load_item_names(&data);
    let orig_db = t.elapsed();
    let t = Instant::now();
    let cdb = cache.load_db()?;
    let cooked_db = t.elapsed();
    println!("\n(b) GameDatabase");
    println!(
        "    original parse {:.1} ms ({} quests, {} infos) | cooked {:.1} ms ({} quests, {} infos) | {:.0}x",
        ms(orig_db),
        db.quests.len(),
        db.infos.len(),
        ms(cooked_db),
        cdb.quests.len(),
        cdb.infos.len(),
        ms(orig_db) / ms(cooked_db).max(0.001)
    );
    println!("(checksum {sink})");
    Ok(())
}
