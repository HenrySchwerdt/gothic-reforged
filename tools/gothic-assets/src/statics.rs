//! Static prop replacement: fit, LODs, collision, textures, profile entry and report.
use crate::{
    Args,
    geom::{self, Aabb, V3},
    glb, original,
    profile::ProfileWriter,
    tex,
};
use anyhow::{Context, Result, bail, ensure};
use gothic_formats::remaster::{OverrideMesh, OverrideSubmesh};
use serde_json::json;
use std::path::Path;

pub struct Converted {
    pub positions: Vec<V3>,
    pub normals: Vec<V3>,
    pub uvs: Vec<[f32; 2]>,
    pub tangents: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    pub material: usize,
}

/// glTF primitive -> Gothic space (mirror X, keep index order), tangents computed in glTF
/// space first and mirrored with the geometry (handedness flips with the mirror).
pub fn to_gothic(p: &glb::Primitive) -> Converted {
    let t = geom::tangents(&p.positions, &p.normals, &p.uvs, &p.indices);
    Converted {
        positions: p.positions.iter().map(|v| [-v[0], v[1], v[2]]).collect(),
        normals: p.normals.iter().map(|v| [-v[0], v[1], v[2]]).collect(),
        uvs: p.uvs.clone(),
        tangents: t.iter().map(|t| [-t[0], t[1], t[2], -t[3]]).collect(),
        indices: p.indices.clone(),
        material: p.material,
    }
}

pub fn apply_fit(c: &mut Converted, f: &geom::Fit) {
    for p in &mut c.positions {
        *p = f.point(*p);
    }
    for n in &mut c.normals {
        *n = f.vector(*n);
    }
    for t in &mut c.tangents {
        let r = f.vector([t[0], t[1], t[2]]);
        *t = [r[0], r[1], r[2], t[3]];
    }
}

fn pos_bytes(p: &[V3]) -> Vec<u8> {
    p.iter().flat_map(|v| v.iter().flat_map(|x| x.to_le_bytes())).collect()
}

/// Weld, cache-optimise and build simplified LODs (meshopt).
pub fn lods(c: &mut Converted, ratios: &[f32]) -> Result<(Vec<Vec<u32>>, Vec<f32>)> {
    let bytes = pos_bytes(&c.positions);
    let adapter = meshopt::VertexDataAdapter::new(&bytes, 12, 0)?;
    let base = meshopt::optimize_vertex_cache(&c.indices, c.positions.len());
    let tris = base.len() / 3;
    let mut out = vec![base];
    let mut errors = vec![0.0];
    for &r in ratios {
        if r <= 0.0 {
            continue;
        }
        let target = ((tris as f32 * r) as usize * 3).max(36);
        let mut err = 0.0f32;
        let lod = meshopt::simplify(
            &out[0],
            &adapter,
            target,
            0.08,
            meshopt::SimplifyOptions::LockBorder,
            Some(&mut err),
        );
        let last = out.last().map_or(0, Vec::len);
        if lod.len() >= 12 && lod.len() * 10 <= last * 9 {
            out.push(meshopt::optimize_vertex_cache(&lod, c.positions.len()));
            errors.push(err);
        }
    }
    Ok((out, errors))
}

/// Coarse collision from the visual mesh: welded positions simplified to about `target`
/// triangles.
pub fn collision_from(subs: &[Converted], target: usize) -> Result<Vec<[V3; 3]>> {
    let mut pos: Vec<V3> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    for c in subs {
        let base = pos.len() as u32;
        pos.extend(&c.positions);
        idx.extend(c.indices.iter().map(|i| i + base));
    }
    // Weld by position so UV seams do not block simplification.
    let (count, remap) = meshopt::generate_vertex_remap(
        &pos.iter().map(|p| p.map(f32::to_bits)).collect::<Vec<_>>(),
        Some(&idx),
    );
    let pos = meshopt::remap_vertex_buffer(&pos, count, &remap);
    let idx = meshopt::remap_index_buffer(Some(&idx), remap.len(), &remap);
    let bytes = pos_bytes(&pos);
    let adapter = meshopt::VertexDataAdapter::new(&bytes, 12, 0)?;
    let simplified = if idx.len() / 3 > target {
        meshopt::simplify_sloppy(&idx, &adapter, target * 3, 1.0, None)
    } else {
        idx.clone()
    };
    let simplified = if simplified.len() < 12 { idx } else { simplified };
    Ok(original::soup(&pos, &simplified))
}

pub fn run(args: &Args) -> Result<()> {
    let glb_path = Path::new(args.need("--glb")?);
    let target = args.need("--target")?;
    let mut prof = ProfileWriter::open(Path::new(args.need("--profile")?))?;
    let tex_max: u32 = args.parse("--tex", 2048)?;
    let gain_arg = args.flag("--gain").unwrap_or("auto").to_string();
    let ratios: Vec<f32> = args
        .flag("--lods")
        .unwrap_or("0.5,0.2")
        .split(',')
        .map(|s| s.trim().parse().context("bad --lods"))
        .collect::<Result<_>>()?;
    let mut orig = original::Original::open(&args.data())?;
    let (resolved, osubs) = orig.static_mesh(target)?;
    let otris: Vec<[V3; 3]> = osubs
        .iter()
        .flat_map(|s| original::soup(&s.positions, &s.indices))
        .collect();
    let ob = Aabb::of(otris.iter().flatten());
    let model = glb::load(glb_path)?;
    let mut subs: Vec<Converted> = model.primitives.iter().map(to_gothic).collect();
    let mtris: Vec<[V3; 3]> = subs
        .iter()
        .flat_map(|c| original::soup(&c.positions, &c.indices))
        .collect();
    let (fit, cands) = geom::fit(&otris, &mtris, args.scale_mode()?, args.yaw()?, !args.has("--no-refine"));
    for c in &mut subs {
        apply_fit(c, &fit);
    }
    let orientation = orient(&otris, &mut subs);
    let nb = Aabb::of(subs.iter().flat_map(|c| c.positions.iter()));
    let ratio = [
        nb.size()[0] / ob.size()[0].max(1e-6),
        nb.size()[1] / ob.size()[1].max(1e-6),
        nb.size()[2] / ob.size()[2].max(1e-6),
    ];
    let mut warnings = Vec::new();
    for (k, axis) in ["X", "Y", "Z"].iter().enumerate() {
        if !(0.75..=1.33).contains(&ratio[k]) {
            warnings.push(format!(
                "{axis} extent differs from the original by factor {:.2} (proportions or axes do not match)",
                ratio[k]
            ));
        }
    }
    let best = cands
        .iter()
        .rev()
        .find(|c| c.yaw_degrees == fit.yaw_degrees)
        .map_or(0.0, |c| c.chamfer_rel);
    if best > 0.05 {
        warnings.push(format!(
            "shape deviates from the original (chamfer {:.1}% of the diagonal)",
            best * 100.0
        ));
    }

    // Brightness: match the mean albedo of the original textures (the generator inherits the
    // dark reference renders and adds its own lighting removal).
    let gain: [f32; 3] = if gain_arg == "auto" {
        let mut acc = ([0f32; 3], 0f32);
        for s in &osubs {
            if let Ok(bytes) = orig.read(&s.material)
                && let Ok(x) = gothic_formats::xshmat::Xshmat::parse(&bytes)
                && let Some(d) = x.diffuse()
                && let Ok(img) = orig.texture(d.stem())
            {
                let w = (s.indices.len() / 3) as f32;
                let m = tex::mean_rgb(&img, None);
                for c in 0..3 {
                    acc.0[c] += m[c] * w;
                }
                acc.1 += w;
            }
        }
        let mut new = ([0f32; 3], 0f32);
        for m in model.materials.iter().filter_map(|m| m.base_color.as_ref()) {
            let c = tex::mean_rgb(m, None);
            for k in 0..3 {
                new.0[k] += c[k];
            }
            new.1 += 1.0;
        }
        if acc.1 > 0.0 && new.1 > 0.0 {
            tex::colour_gain(new.0.map(|v| v / new.1), acc.0.map(|v| v / acc.1))
        } else {
            [1.0; 3]
        }
    } else {
        [gain_arg.parse().context("bad --gain")?; 3]
    };
    let stem = args
        .flag("--name")
        .map(str::to_string)
        .unwrap_or_else(|| gothic_formats::remaster::key_of(target));
    // Textures per glTF material.
    let mut mat_tex: Vec<(String, String, f32, u32)> = Vec::new();
    let mut tex_report = Vec::new();
    for (i, m) in model.materials.iter().enumerate() {
        let d_stem = format!("{stem}_rm{i}_d");
        let n_stem = format!("{stem}_rm{i}_n");
        let mut d_name = String::new();
        let mut n_name = String::new();
        if let Some(img) = &m.base_color {
            let alpha = m.alpha_mask;
            let t = tex::diffuse(img, tex_max, alpha, gain)?;
            prof.write_file(&format!("{d_stem}.dds"), &t.to_bytes())?;
            prof.set("texture", &d_stem, &format!("{d_stem}.dds"));
            tex_report.push(json!({"stem": d_stem, "size": [t.width, t.height], "format": String::from_utf8_lossy(&t.fourcc), "mips": t.mips.len()}));
            d_name = d_stem;
        } else {
            warnings.push(format!("material {} has no base colour texture", m.name));
        }
        if let Some(img) = &m.normal {
            let t = tex::normal_map(img, tex_max)?;
            prof.write_file(&format!("{n_stem}.dds"), &t.to_bytes())?;
            prof.set("texture", &n_stem, &format!("{n_stem}.dds"));
            tex_report.push(json!({"stem": n_stem, "size": [t.width, t.height], "format": "DXT5nm", "mips": t.mips.len()}));
            n_name = n_stem;
        }
        let rough = match &m.metal_rough {
            Some(img) => {
                let sum: f64 = img.pixels().map(|p| f64::from(p.0[1])).sum();
                (sum / f64::from(img.width() * img.height()) / 255.0) as f32 * m.roughness_factor
            }
            None => m.roughness_factor,
        };
        mat_tex.push((d_name, n_name, rough, u32::from(m.alpha_mask)));
    }

    let mut out_subs = Vec::new();
    let mut lod_report = Vec::new();
    for c in &mut subs {
        let (l, errors) = lods(c, &ratios)?;
        let (d, n, rough, blend) = mat_tex[c.material].clone();
        lod_report.push(json!({
            "material": model.materials[c.material].name,
            "vertices": c.positions.len(),
            "triangles": l.iter().map(|x| x.len() / 3).collect::<Vec<_>>(),
            "simplify_error": errors,
        }));
        out_subs.push(OverrideSubmesh {
            material: String::new(),
            diffuse: d,
            normal: n,
            roughness: rough,
            blend,
            positions: c.positions.clone(),
            normals: c.normals.clone(),
            uvs: c.uvs.clone(),
            tangents: c.tangents.clone(),
            joints: vec![],
            weights: vec![],
            lods: l,
        });
    }
    let original_collision = orig.collision(&resolved).map_or(0, |c| c.len());
    // Keep gameplay parity: props without original collision stay walk-through.
    let collision_mode = match args.flag("--collision").unwrap_or("auto") {
        "auto" if original_collision == 0 => "none",
        "auto" => "generated",
        other => other,
    };
    let collision = match collision_mode {
        "original" => orig
            .collision(&resolved)
            .map(|t| t.into_iter().map(|t| t.map(|p| [p[0], p[1], p[2]])).collect())
            .unwrap_or_default(),
        "none" => vec![],
        "generated" => collision_from(&subs, args.parse("--collision-tris", 400)?)?,
        other => bail!("unknown --collision {other}"),
    };
    let mesh = OverrideMesh {
        skinned: false,
        nodes: vec![],
        submeshes: out_subs,
        collision,
    };
    let bytes = mesh.to_bytes()?;
    ensure!(OverrideMesh::parse(&bytes)? == mesh, "GRMO round trip failed");
    prof.write_file(&format!("{stem}.grmo"), &bytes)?;
    prof.set("mesh", target, &format!("{stem}.grmo"));
    if !resolved.eq_ignore_ascii_case(target) {
        prof.set("mesh", &resolved, &format!("{stem}.grmo"));
    }
    // Distance stand-in placed as its own entity: the coarsest LOD replaces it.
    for lp in args.all("--lowpoly") {
        let mut low = mesh.clone();
        for s in &mut low.submeshes {
            let coarse = s.lods.last().cloned().unwrap_or_default();
            s.lods = vec![coarse];
        }
        let lp_stem = gothic_formats::remaster::key_of(lp);
        prof.write_file(&format!("{lp_stem}.grmo"), &low.to_bytes()?)?;
        prof.set("mesh", lp, &format!("{lp_stem}.grmo"));
    }
    prof.save()?;
    let report = json!({
        "target": target,
        "resolved_mesh": resolved,
        "source": glb_path.display().to_string(),
        "original": {
            "bounds": ob, "size_cm": ob.size(),
            "triangles": otris.len(),
            "submeshes": osubs.iter().map(|s| &s.material).collect::<Vec<_>>(),
            "collision_triangles": original_collision,
        },
        "fit": fit,
        "fit_candidates": cands,
        "orientation_agreement": orientation,
        "albedo_gain": gain,
        "result": {
            "bounds": nb, "size_cm": nb.size(), "extent_ratio": ratio,
            "submeshes": lod_report,
            "collision": {"mode": collision_mode, "triangles": mesh.collision.len()},
            "textures": tex_report,
            "grmo_bytes": bytes.len(),
        },
        "warnings": warnings,
    });
    let text = serde_json::to_string_pretty(&report)?;
    prof.write_file(&format!("reports/{stem}.json"), text.as_bytes())?;
    println!("{text}");
    Ok(())
}

/// Makes the model's triangle winding agree with the original's (the actor format winds the
/// other way than static meshes). Returns (agreement before, after).
pub fn orient(original: &[[V3; 3]], subs: &mut [Converted]) -> [f32; 2] {
    let tris = |subs: &[Converted]| -> Vec<[V3; 3]> {
        subs.iter().flat_map(|c| original::soup(&c.positions, &c.indices)).collect()
    };
    let before = geom::orientation_agreement(original, &tris(subs));
    if before >= 0.5 {
        return [before, before];
    }
    for c in subs.iter_mut() {
        for t in c.indices.chunks_exact_mut(3) {
            t.swap(1, 2);
        }
    }
    [before, geom::orientation_agreement(original, &tris(subs))]
}
