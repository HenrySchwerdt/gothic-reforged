//! gothic-assets: turns generated models (GLB, e.g. from Meshy) into replacements for original
//! Gothic 3 assets in a local remaster profile.
//!
//! ```text
//! gothic-assets info   <NAME>                          original mesh / actor statistics
//! gothic-assets static --glb F --target NAME --profile DIR [options]
//! gothic-assets actor  --glb F --target ACTOR --profile DIR [options]
//! gothic-assets obj    <NAME> <OUT.obj>                original geometry for inspection
//! gothic-assets views  <NAME> --out DIR [--glb F]    4 reference views (+ fitted GLB views)
//! gothic-assets glb    <NAME> <OUT.glb>                original geometry + UVs as binary glTF
//! gothic-assets texture --image F --target STEM --profile DIR [--normal F --normal-target STEM]
//!                       [--ao F] [--real-size M --tile M | --scale F]   replace one texture
//! common: --data DIR (default: the Steam installation's Data folder)
//! static: --yaw DEG --scale volume|height|footprint --tex N --gain F --lods 0.5,0.2
//!         --collision-tris N | --collision auto|generated|original|none  --lowpoly NAME  --name STEM
//! actor:  --yaw DEG --scale ... --bake MATERIAL_INDEX[=STEM] (repeatable) --bake-size N
//! ```
//!
//! Space conventions: glTF is right-handed, Y up, metres, counter-clockwise front faces;
//! Gothic is left-handed, Y up, centimetres, clockwise front faces. A point maps with
//! `(-x, y, z)` (the mirror flips handedness and winding together, so index order is kept),
//! then the similarity fit scales it onto the original's bounds.
mod actor;
mod geom;
mod glb;
mod original;
mod profile;
mod render;
mod statics;
mod tex;

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

const DEFAULT_DATA: &str = r"D:\SteamLibrary\steamapps\common\Gothic 3\Data";

pub struct Args {
    list: Vec<String>,
}

impl Args {
    pub fn flag(&self, name: &str) -> Option<&str> {
        self.list
            .iter()
            .position(|a| a == name)
            .and_then(|i| self.list.get(i + 1))
            .map(String::as_str)
    }
    pub fn all(&self, name: &str) -> Vec<&str> {
        self.list
            .windows(2)
            .filter(|w| w[0] == name)
            .map(|w| w[1].as_str())
            .collect()
    }
    pub fn has(&self, name: &str) -> bool {
        self.list.iter().any(|a| a == name)
    }
    pub fn need(&self, name: &str) -> Result<&str> {
        self.flag(name).with_context(|| format!("missing {name}"))
    }
    pub fn parse<T: std::str::FromStr>(&self, name: &str, default: T) -> Result<T> {
        match self.flag(name) {
            Some(v) => v.parse().map_err(|_| anyhow::anyhow!("bad value for {name}: {v}")),
            None => Ok(default),
        }
    }
    pub fn positional(&self, i: usize) -> Option<&str> {
        let mut skip = false;
        let mut n = 0;
        for a in &self.list[1..] {
            if skip {
                skip = false;
                continue;
            }
            if a.starts_with("--") {
                skip = !matches!(a.as_str(), "--verbose");
                continue;
            }
            if n == i {
                return Some(a);
            }
            n += 1;
        }
        None
    }
    pub fn data(&self) -> PathBuf {
        PathBuf::from(self.flag("--data").unwrap_or(DEFAULT_DATA))
    }
    pub fn scale_mode(&self) -> Result<geom::ScaleMode> {
        Ok(match self.flag("--scale").unwrap_or("volume") {
            "volume" => geom::ScaleMode::Volume,
            "height" => geom::ScaleMode::Height,
            "footprint" => geom::ScaleMode::Footprint,
            other => bail!("unknown --scale {other}"),
        })
    }
    pub fn yaw(&self) -> Result<Option<f32>> {
        self.flag("--yaw")
            .map(|v| v.parse().map_err(|_| anyhow::anyhow!("bad --yaw")))
            .transpose()
    }
}

fn main() -> Result<()> {
    let list: Vec<String> = std::env::args().skip(1).collect();
    let args = Args { list };
    match args.list.first().map(String::as_str).unwrap_or("help") {
        "info" => info(&args),
        "obj" => obj(&args),
        "views" => views(&args),
        "glb" => export_glb(&args),
        "terrain-survey" => terrain_survey(&args),
        "texture" => texture_override(&args),
        "mat" => {
            let name = args.positional(0).context("mat needs a material name")?;
            let mut o = original::Original::open(&args.data())?;
            let bytes = o.read(name)?;
            let x = gothic_formats::xshmat::Xshmat::parse(&bytes)?;
            println!("blend {:?} mask {:?}", x.blend_mode, x.mask_reference);
            for t in &x.textures {
                println!("  {:?} {}", t.role, t.path);
            }
            if let Ok(g) = gothic_formats::xshmat::graph::MaterialGraph::parse(&bytes)
                && let Some(r) = g.surface()
            {
                for l in &r.layers {
                    println!("  layer {}", l.textures.iter().map(|t| format!("{} x{:?}", t.path, t.scale)).collect::<Vec<_>>().join(" * "));
                }
            }
            Ok(())
        }
        "sprite" => {
            let g = args.need("--gltf")?;
            let nodes: Vec<String> = args.flag("--nodes").unwrap_or("").split(',').filter(|s| !s.is_empty()).map(str::to_string).collect();
            let mut model = glb::load_filtered(std::path::Path::new(g), |n| nodes.is_empty() || nodes.iter().any(|k| n == k))?;
            // The glTF references JPEG albedo; the PNG release carries the alpha cut-out.
            if let Some(a) = args.flag("--albedo") {
                let img = image::open(a)?.to_rgba8();
                for m in &mut model.materials {
                    m.base_color = Some(img.clone());
                }
            }
            let img = render::sprite(&model, args.parse("--w", 512)?, args.parse("--h", 512)?, args.parse("--az", 0.0)?, args.parse("--elev", 0.0)?);
            img.save(args.need("--out")?)?;
            Ok(())
        }
        "png" => {
            let stem = args.positional(0).context("png needs a texture stem")?;
            let out = args.positional(1).context("png needs an output path")?;
            let mut o = original::Original::open(&args.data())?;
            o.texture(stem)?.save(out)?;
            Ok(())
        }
        "list" => {
            let pat = args.positional(0).context("list needs a substring")?.to_ascii_lowercase();
            let o = original::Original::open(&args.data())?;
            let mut v: Vec<String> = o.vfs.iter().map(|(p, _)| p.to_string()).filter(|p| p.contains(&pat)).collect();
            v.sort();
            for p in v {
                println!("{p}");
            }
            Ok(())
        }
        "static" => statics::run(&args),
        "actor" => actor::run(&args),
        _ => {
            println!("{}", include_str!("main.rs").lines().take(14).collect::<Vec<_>>().join("\n"));
            Ok(())
        }
    }
}

fn info(args: &Args) -> Result<()> {
    let name = args.positional(0).context("info needs a name")?;
    let mut o = original::Original::open(&args.data())?;
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".xcmsh") || lower.ends_with(".xlmsh") {
        let (resolved, subs) = o.static_mesh(name)?;
        let all: Vec<geom::V3> = subs.iter().flat_map(|s| s.positions.clone()).collect();
        let b = geom::Aabb::of(all.iter());
        let col = o.collision(&resolved).map_or(0, |c| c.len());
        let out = serde_json::json!({
            "name": resolved,
            "bounds": b,
            "size_cm": b.size(),
            "collision_triangles": col,
            "submeshes": subs.iter().map(|s| serde_json::json!({
                "material": s.material,
                "vertices": s.positions.len(),
                "triangles": s.indices.len() / 3,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        let a = o.actor(name)?;
        let mut subs = Vec::new();
        let mut all = Vec::new();
        for m in &a.meshes {
            for s in &m.submeshes {
                all.extend(s.positions.iter().copied());
                let mat = a.materials.get(s.material as usize);
                subs.push(serde_json::json!({
                    "material_index": s.material,
                    "material": mat.map(|m| m.name.clone()),
                    "diffuse": mat.and_then(|m| m.diffuse_texture().map(str::to_string)),
                    "vertices": s.positions.len(),
                    "triangles": s.indices.len() / 3,
                }));
            }
        }
        let b = geom::Aabb::of(all.iter());
        let out = serde_json::json!({
            "name": name,
            "nodes": a.nodes.len(),
            "slots": a.nodes.iter().filter(|n| n.name.starts_with("Slot_")).map(|n| n.name.clone()).collect::<Vec<_>>(),
            "bounds": b,
            "size_cm": b.size(),
            "materials": a.materials.iter().map(|m| m.name.clone()).collect::<Vec<_>>(),
            "material_files": a.material_files,
            "submeshes": subs,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    }
    Ok(())
}

fn obj(args: &Args) -> Result<()> {
    let name = args.positional(0).context("obj needs a name")?;
    let out = args.positional(1).context("obj needs an output path")?;
    let mut o = original::Original::open(&args.data())?;
    let lower = name.to_ascii_lowercase();
    let mut groups: Vec<(String, Vec<geom::V3>, Vec<[f32; 2]>, Vec<u32>)> = Vec::new();
    if lower.ends_with(".xcmsh") || lower.ends_with(".xlmsh") {
        let (_, subs) = o.static_mesh(name)?;
        for s in subs {
            groups.push((s.material, s.positions, s.uvs, s.indices));
        }
    } else {
        let a = o.actor(name)?;
        for m in &a.meshes {
            for s in &m.submeshes {
                groups.push((format!("mat{}", s.material), s.positions.clone(), s.uvs.clone(), s.indices.clone()));
            }
        }
    }
    // OBJ is right-handed: un-mirror X and reverse the winding.
    let mut text = String::new();
    let mut base = 1u32;
    for (g, p, uv, idx) in &groups {
        text.push_str(&format!("g {g}\n"));
        for v in p {
            text.push_str(&format!("v {} {} {}\n", -v[0] / 100.0, v[1] / 100.0, v[2] / 100.0));
        }
        for t in uv {
            text.push_str(&format!("vt {} {}\n", t[0], 1.0 - t[1]));
        }
        for t in idx.chunks_exact(3) {
            let (a, b, c) = (t[0] + base, t[2] + base, t[1] + base);
            text.push_str(&format!("f {a}/{a} {b}/{b} {c}/{c}\n"));
        }
        base += p.len() as u32;
    }
    std::fs::write(out, text)?;
    println!("wrote {out}");
    Ok(())
}

/// Textured parts of an original asset (static mesh or actor) in Gothic space.
struct Owned {
    positions: Vec<geom::V3>,
    normals: Vec<geom::V3>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
    texture: Option<image::RgbaImage>,
    alpha_test: bool,
}

fn original_parts(o: &mut original::Original, name: &str) -> Result<Vec<Owned>> {
    let lower = name.to_ascii_lowercase();
    let mut out = Vec::new();
    if lower.ends_with(".xcmsh") || lower.ends_with(".xlmsh") {
        let (_, subs) = o.static_mesh(name)?;
        for s in subs {
            let mut texture = None;
            let mut alpha_test = false;
            if let Ok(bytes) = o.read(&s.material)
                && let Ok(x) = gothic_formats::xshmat::Xshmat::parse(&bytes)
            {
                alpha_test = x.blend_mode == Some(1);
                if let Some(d) = x.diffuse() {
                    texture = o.texture(d.stem()).ok();
                }
            }
            out.push(Owned { positions: s.positions, normals: s.normals, uvs: s.uvs, indices: s.indices, texture, alpha_test });
        }
    } else {
        let (stem, switch) = match name.rsplit_once("@M") {
            Some((n, s)) => (n, s.parse::<i32>().ok()),
            None => (name, None),
        };
        let a = o.actor(stem)?;
        for m in &a.meshes {
            for s in &m.submeshes {
                let mut texture = None;
                if let Some(mat) = a.materials.get(s.material as usize)
                    && let Some(d) = mat.diffuse_texture()
                {
                    let d = match (switch, d.rsplit_once("_S")) {
                        (Some(n), Some((base, _))) => format!("{base}_S{}", n + 1),
                        _ => d.to_string(),
                    };
                    texture = o.texture(&d).ok();
                }
                let n = s.positions.len();
                out.push(Owned {
                    positions: s.positions.clone(),
                    normals: if s.normals.len() == n { s.normals.clone() } else { vec![[0.0, 1.0, 0.0]; n] },
                    uvs: if s.uvs.len() == n { s.uvs.clone() } else { vec![[0.0; 2]; n] },
                    indices: s.indices.clone(),
                    texture,
                    alpha_test: true,
                });
            }
        }
    }
    Ok(out)
}

fn as_parts(v: &[Owned]) -> Vec<render::Part<'_>> {
    v.iter()
        .map(|p| render::Part {
            positions: &p.positions,
            normals: &p.normals,
            uvs: &p.uvs,
            indices: &p.indices,
            texture: p.texture.as_ref(),
            alpha_test: p.alpha_test,
        })
        .collect()
}

fn sheet(rows: &[Vec<image::RgbaImage>]) -> image::RgbaImage {
    let s = rows[0][0].width();
    let cols = rows.iter().map(Vec::len).max().unwrap_or(1) as u32;
    let mut out = image::RgbaImage::new(s * cols, s * rows.len() as u32);
    for (r, row) in rows.iter().enumerate() {
        for (c, im) in row.iter().enumerate() {
            image::imageops::overlay(&mut out, im, i64::from(c as u32 * s), i64::from(r as u32 * s));
        }
    }
    out
}

fn views(args: &Args) -> Result<()> {
    let name = args.positional(0).context("views needs a name")?;
    let out = PathBuf::from(args.need("--out")?);
    std::fs::create_dir_all(&out)?;
    let size: u32 = args.parse("--size", 768)?;
    let elev: f32 = args.parse("--elev", 15.0)?;
    let az: f32 = args.parse("--az", 0.0)?;
    let bg = if args.has("--dark") { [40, 42, 46] } else { [255, 255, 255] };
    let mut o = original::Original::open(&args.data())?;
    let orig = original_parts(&mut o, name)?;
    let stem = gothic_formats::remaster::key_of(name);
    let ov = render::turntable(&as_parts(&orig), size, elev, az, bg);
    for (i, im) in ov.iter().enumerate() {
        im.save(out.join(format!("{stem}-v{i}.png")))?;
    }
    let mut rows = vec![ov];
    if let Some(g) = args.flag("--glb") {
        let model = glb::load(std::path::Path::new(g))?;
        let mut subs: Vec<_> = model.primitives.iter().map(statics::to_gothic).collect();
        let otris: Vec<_> = orig.iter().flat_map(|p| original::soup(&p.positions, &p.indices)).collect();
        let mtris: Vec<_> = subs.iter().flat_map(|c| original::soup(&c.positions, &c.indices)).collect();
        let (fit, cands) = geom::fit(&otris, &mtris, args.scale_mode()?, args.yaw()?, !args.has("--no-refine"));
        eprintln!("fit {fit:?}\n{}", serde_json::to_string(&cands)?);
        for c in &mut subs {
            statics::apply_fit(c, &fit);
        }
        let owned: Vec<Owned> = subs
            .iter()
            .map(|c| Owned {
                positions: c.positions.clone(),
                normals: c.normals.clone(),
                uvs: c.uvs.clone(),
                indices: c.indices.clone(),
                texture: model.materials[c.material].base_color.clone(),
                alpha_test: model.materials[c.material].alpha_mask,
            })
            .collect();
        // Same framing as the original: render both together, then each alone with that box.
        let nv = render::turntable(&as_parts(&owned), size, elev, az, bg);
        rows.push(nv);
    }
    let sh = sheet(&rows);
    sh.save(out.join(format!("{stem}-sheet.png")))?;
    println!("{}", out.join(format!("{stem}-sheet.png")).display());
    Ok(())
}

/// Original geometry (one primitive per submesh, positions/normals/UV0, no textures) as a
/// binary glTF in glTF space: metres, right-handed (X un-mirrored, winding reversed).
fn export_glb(args: &Args) -> Result<()> {
    let name = args.positional(0).context("glb needs a name")?;
    let out = args.positional(1).context("glb needs an output path")?;
    let mut o = original::Original::open(&args.data())?;
    let parts = original_parts(&mut o, name)?;
    let mut bin: Vec<u8> = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut prims = Vec::new();
    let mut push = |bin: &mut Vec<u8>, bytes: Vec<u8>, target: u32| -> usize {
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        views.push(serde_json::json!({"buffer": 0, "byteOffset": bin.len(), "byteLength": bytes.len(), "target": target}));
        bin.extend(bytes);
        views.len() - 1
    };
    for p in &parts {
        let pos: Vec<[f32; 3]> = p.positions.iter().map(|v| [-v[0] / 100.0, v[1] / 100.0, v[2] / 100.0]).collect();
        let nrm: Vec<[f32; 3]> = p.normals.iter().map(|v| geom::norm([-v[0], v[1], v[2]])).collect();
        let idx: Vec<u32> = p
            .indices
            .chunks_exact(3)
            .filter(|t| t.iter().all(|&i| (i as usize) < pos.len()))
            .flat_map(|t| [t[0], t[2], t[1]])
            .collect();
        let b = geom::Aabb::of(pos.iter());
        let f32b = |v: &[f32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        let vp = push(&mut bin, f32b(pos.as_flattened()), 34962);
        let vn = push(&mut bin, f32b(nrm.as_flattened()), 34962);
        let vt = push(&mut bin, f32b(p.uvs.as_flattened()), 34962);
        let vi = push(&mut bin, idx.iter().flat_map(|x| x.to_le_bytes()).collect(), 34963);
        let a = accessors.len();
        accessors.push(serde_json::json!({"bufferView": vp, "componentType": 5126, "count": pos.len(), "type": "VEC3", "min": b.min, "max": b.max}));
        accessors.push(serde_json::json!({"bufferView": vn, "componentType": 5126, "count": pos.len(), "type": "VEC3"}));
        accessors.push(serde_json::json!({"bufferView": vt, "componentType": 5126, "count": pos.len(), "type": "VEC2"}));
        accessors.push(serde_json::json!({"bufferView": vi, "componentType": 5125, "count": idx.len(), "type": "SCALAR"}));
        prims.push(serde_json::json!({"attributes": {"POSITION": a, "NORMAL": a + 1, "TEXCOORD_0": a + 2}, "indices": a + 3, "material": 0}));
    }
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let json = serde_json::json!({
        "asset": {"version": "2.0", "generator": "gothic-assets"},
        "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
        "meshes": [{"primitives": prims}],
        "materials": [{"pbrMetallicRoughness": {"baseColorFactor": [0.8, 0.8, 0.8, 1.0], "metallicFactor": 0.0, "roughnessFactor": 0.9}}],
        "buffers": [{"byteLength": bin.len()}],
        "bufferViews": views, "accessors": accessors,
    });
    let mut js = serde_json::to_vec(&json)?;
    while js.len() % 4 != 0 {
        js.push(b' ');
    }
    let total = 12 + 8 + js.len() + 8 + bin.len();
    let mut file = Vec::with_capacity(total);
    file.extend(b"glTF");
    file.extend(2u32.to_le_bytes());
    file.extend((total as u32).to_le_bytes());
    file.extend((js.len() as u32).to_le_bytes());
    file.extend(b"JSON");
    file.extend(js);
    file.extend((bin.len() as u32).to_le_bytes());
    file.extend(b"BIN\0");
    file.extend(bin);
    std::fs::write(out, &file)?;
    println!("wrote {out} ({} bytes)", file.len());
    Ok(())
}

/// Terrain cells overlapping a Gothic-space XZ rectangle: per material the triangle count and
/// the layer textures of its material graph.
fn terrain_survey(args: &Args) -> Result<()> {
    let x: f32 = args.parse("--x", 87213.0)?;
    let z: f32 = args.parse("--z", -9699.0)?;
    let r: f32 = args.parse("--r", 12000.0)?;
    let mut o = original::Original::open(&args.data())?;
    let names: Vec<String> = o
        .vfs
        .iter()
        .map(|(p, _)| p.to_string())
        .filter(|p| p.contains("_landscape_cell_") && p.ends_with(".xcmsh"))
        .collect();
    let mut mats: std::collections::BTreeMap<String, usize> = Default::default();
    let mut uv_per_cm = (0f64, 0usize);
    for n in names {
        let Ok(bytes) = o.read(&n) else { continue };
        let Ok(mesh) = gothic_formats::xcmsh::Xcmsh::parse(&bytes) else { continue };
        for s in &mesh.submeshes {
            for t in s.indices.chunks_exact(3) {
                let p = s.positions[t[0] as usize];
                if (p[0] - x).abs() < r && (p[2] - z).abs() < r {
                    *mats.entry(s.material.clone()).or_default() += 1;
                    if s.uvs.len() == s.positions.len() {
                        let (a, b) = (t[0] as usize, t[1] as usize);
                        let dp = geom::len(geom::sub(s.positions[a], s.positions[b]));
                        let du = ((s.uvs[a][0] - s.uvs[b][0]).powi(2) + (s.uvs[a][1] - s.uvs[b][1]).powi(2)).sqrt();
                        if dp > 1.0 {
                            uv_per_cm.0 += f64::from(du / dp);
                            uv_per_cm.1 += 1;
                        }
                    }
                }
            }
        }
    }
    println!("terrain uv units per cm: {:.6}", uv_per_cm.0 / uv_per_cm.1.max(1) as f64);
    for (m, n) in &mats {
        let mut layers = Vec::new();
        if let Ok(bytes) = o.read(m)
            && let Ok(g) = gothic_formats::xshmat::graph::MaterialGraph::parse(&bytes)
            && let Some(rcp) = g.surface()
        {
            for l in &rcp.layers {
                layers.push(l.textures.iter().map(|t| format!("{} x{:?}", t.path, t.scale)).collect::<Vec<_>>().join(" * "));
            }
            for nm in rcp.normals.iter().flatten() {
                layers.push(format!("normal {} x{:?}", nm.path, nm.scale));
            }
            layers.push(format!("vertex_blend {}", rcp.vertex_blend));
        }
        println!("{n:7} {m}\n        {}", layers.join("\n        "));
    }
    Ok(())
}

/// Replaces one original texture (and optionally its normal map) with a photographed /
/// scanned material: colour matched to the original's mean, ambient occlusion folded into the
/// albedo (the Gothic shader has no AO map), UV scale corrected for the real-world size.
fn texture_override(args: &Args) -> Result<()> {
    let target = args.need("--target")?;
    let mut prof = profile::ProfileWriter::open(std::path::Path::new(args.need("--profile")?))?;
    let mut orig = original::Original::open(&args.data())?;
    let tex_max: u32 = args.parse("--tex", 2048)?;
    let mut img = image::open(args.need("--image")?)?.to_rgba8();
    if let Some(ao) = args.flag("--ao") {
        let ao = image::imageops::resize(
            &image::open(ao)?.to_luma8(),
            img.width(),
            img.height(),
            image::imageops::FilterType::Triangle,
        );
        let k: f32 = args.parse("--ao-strength", 0.6)?;
        for (p, a) in img.pixels_mut().zip(ao.pixels()) {
            let f = 1.0 - k * (1.0 - f32::from(a.0[0]) / 255.0);
            for c in 0..3 {
                p.0[c] = (f32::from(p.0[c]) * f) as u8;
            }
        }
    }
    // Global contrast around the mean (tile-safe, unlike a local blur) and a brightness trim:
    // photographed ground is busier than Gothic's painted textures and shimmers in the distance.
    let contrast: f32 = args.parse("--contrast", 1.0)?;
    if (contrast - 1.0).abs() > 1e-3 {
        let m = tex::mean_rgb(&img, None);
        for p in img.pixels_mut() {
            for c in 0..3 {
                p.0[c] = (m[c] + (f32::from(p.0[c]) - m[c]) * contrast).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    let flat: f32 = args.parse("--flatten", 0.0)?;
    if flat > 0.0 {
        tex::flatten(&mut img, args.parse("--flatten-grid", 8)?, flat);
    }
    let original = orig.texture(target).ok();
    let hist: f32 = args.parse("--match-histogram", 0.0)?;
    if hist > 0.0
        && let Some(o) = &original
    {
        tex::match_histogram(&mut img, o, hist);
    }
    let gain = match (&original, args.flag("--gain").unwrap_or(if hist > 0.0 { "1" } else { "auto" })) {
        (Some(o), "auto") => tex::colour_gain(tex::mean_rgb(&img, None), tex::mean_rgb(o, None)),
        (_, "auto") => [1.0; 3],
        (_, g) => [g.parse().context("bad --gain")?; 3],
    };
    let brightness: f32 = args.parse("--brightness", 1.0)?;
    let gain = gain.map(|g| g * brightness);
    let scale: f32 = match args.flag("--scale") {
        Some(s) => s.parse().context("bad --scale")?,
        None => {
            let real: f32 = args.parse("--real-size", 0.0)?;
            let tile: f32 = args.parse("--tile", 7.0)?;
            if real > 0.0 { tile / real } else { 1.0 }
        }
    };
    let opt = ((scale - 1.0).abs() > 1e-3).then(|| format!("scale={scale:.4}"));
    let stem = target.to_ascii_lowercase();
    let d = if args.has("--cutout") {
        // Colour is matched by the caller per atlas region; alpha drives the cut-out.
        tex::diffuse_cutout(&img, tex_max, args.parse("--cutoff", 0.4)?)?
    } else {
        tex::diffuse(&img, tex_max, false, gain)?
    };
    prof.write_file(&format!("tex/{stem}.dds"), &d.to_bytes())?;
    prof.set_with("texture", target, &format!("tex/{stem}.dds"), opt.clone());
    let mut report = serde_json::json!({
        "target": target, "gain": gain, "uv_scale": scale, "contrast": contrast, "size": [d.width, d.height],
        "original_size": original.as_ref().map(|o| [o.width(), o.height()]),
    });
    if let (Some(n), Some(nt)) = (args.flag("--normal"), args.flag("--normal-target")) {
        let n = tex::normal_map(&image::open(n)?.to_rgba8(), tex_max)?;
        let ns = nt.to_ascii_lowercase();
        prof.write_file(&format!("tex/{ns}.dds"), &n.to_bytes())?;
        prof.set_with("texture", nt, &format!("tex/{ns}.dds"), opt);
        report["normal"] = serde_json::json!(nt);
    }
    prof.save()?;
    println!("{report}");
    Ok(())
}
