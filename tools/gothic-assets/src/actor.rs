//! Character part replacement (`.xact` head / body). The generated mesh is fitted onto the
//! original bind-pose geometry; every new vertex takes its skin weights from the closest point
//! of the original surface (so the original skeleton, joint positions and every animation keep
//! working), and every new triangle takes the material and UV chart of the closest original
//! triangle (so the material-switch texture variants `_S<n>` keep working). Optionally the
//! generated colour is baked into the original UV layout of chosen materials.
use crate::{
    Args,
    geom::{self, Aabb, TriGrid, V3},
    glb, original,
    profile::ProfileWriter,
    statics::{apply_fit, to_gothic},
    tex,
};
use anyhow::{Context, Result, ensure};
use gothic_formats::remaster::{OverrideMesh, OverrideSubmesh};
use image::RgbaImage;
use serde_json::json;
use std::{collections::HashMap, path::Path};

struct OrigTri {
    sub: usize,
    verts: [usize; 3],
}

struct OrigSub {
    material: u8,
    uvs: Vec<[f32; 2]>,
    joints: Vec<[u16; 4]>,
    weights: Vec<[f32; 4]>,
}

/// Blend up to 3 x 4 influences, keep the strongest 4, normalise.
fn blend_weights(items: &[([u16; 4], [f32; 4], f32)]) -> ([u16; 4], [f32; 4]) {
    let mut acc: HashMap<u16, f32> = HashMap::new();
    for (j, w, b) in items {
        for k in 0..4 {
            if w[k] > 0.0 && *b > 0.0 {
                *acc.entry(j[k]).or_default() += w[k] * b;
            }
        }
    }
    let mut v: Vec<(u16, f32)> = acc.into_iter().collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    v.truncate(4);
    let sum: f32 = v.iter().map(|x| x.1).sum();
    let mut j = [0u16; 4];
    let mut w = [0f32; 4];
    for (k, (jj, ww)) in v.into_iter().enumerate() {
        j[k] = jj;
        w[k] = if sum > 0.0 { ww / sum } else { 0.0 };
    }
    if sum <= 0.0 {
        w[0] = 1.0;
    }
    (j, w)
}

fn sample_bilinear(img: &RgbaImage, uv: [f32; 2]) -> [f32; 4] {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let x = (uv[0].rem_euclid(1.0) * w - 0.5).max(0.0);
    let y = (uv[1].rem_euclid(1.0) * h - 0.5).max(0.0);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(img.width() - 1), (y0 + 1).min(img.height() - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let p = |x, y| img.get_pixel(x, y).0.map(f32::from);
    let (a, b, c, d) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
    let mut o = [0.0; 4];
    for k in 0..4 {
        o[k] = (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy;
    }
    o
}

pub fn run(args: &Args) -> Result<()> {
    let glb_path = Path::new(args.need("--glb")?);
    let target = args.need("--target")?;
    let mut prof = ProfileWriter::open(Path::new(args.need("--profile")?))?;
    let mut orig = original::Original::open(&args.data())?;
    let actor = orig.actor(target)?;
    let stem = args
        .flag("--name")
        .map(str::to_string)
        .unwrap_or_else(|| gothic_formats::remaster::key_of(target));

    // Original surface in bind pose.
    let mut osubs = Vec::new();
    let mut otris: Vec<[V3; 3]> = Vec::new();
    let mut tri_info = Vec::new();
    for m in &actor.meshes {
        for s in &m.submeshes {
            let si = osubs.len();
            for t in s.indices.chunks_exact(3) {
                let v = [t[0] as usize, t[1] as usize, t[2] as usize];
                if v.iter().any(|&i| i >= s.positions.len()) {
                    continue;
                }
                otris.push(v.map(|i| s.positions[i]));
                tri_info.push(OrigTri { sub: si, verts: v });
            }
            ensure!(
                s.joints.len() == s.positions.len() && s.weights.len() == s.positions.len(),
                "original submesh without per-vertex skin"
            );
            osubs.push(OrigSub {
                material: s.material,
                uvs: s.uvs.clone(),
                joints: s.joints.clone(),
                weights: s.weights.clone(),
            });
        }
    }
    ensure!(!otris.is_empty(), "{target} has no geometry");
    let ob = Aabb::of(otris.iter().flatten());

    let model = glb::load(glb_path)?;
    let mut subs: Vec<_> = model.primitives.iter().map(to_gothic).collect();
    let mtris: Vec<[V3; 3]> = subs
        .iter()
        .flat_map(|c| original::soup(&c.positions, &c.indices))
        .collect();
    let (fit, cands) = geom::fit(&otris, &mtris, args.scale_mode()?, args.yaw()?, !args.has("--no-refine"));
    for c in &mut subs {
        apply_fit(c, &fit);
    }
    let orientation = crate::statics::orient(&otris, &mut subs);
    let nb = Aabb::of(subs.iter().flat_map(|c| c.positions.iter()));

    // `--own-uv`: keep the generated UVs and texture; the original material stays, its texture
    // stem is overridden with the generated colour (variants of this part are given up).
    let own_uv = args.has("--own-uv");
    let cell = (ob.diagonal() / 60.0).max(0.5);
    let ogrid = TriGrid::new(&otris, cell);
    let otri_normal = |i: usize| {
        let t = &otris[i];
        geom::norm(geom::cross(geom::sub(t[1], t[0]), geom::sub(t[2], t[0])))
    };

    // Per original material: (vertex key -> new index), streams.
    #[derive(Default)]
    struct Out {
        map: HashMap<(usize, usize, u32, u32), u32>,
        s: OverrideSubmesh,
    }
    let mut outs: HashMap<u8, Out> = HashMap::new();
    let mut dist_sum = 0.0f64;
    let mut dist_max = 0.0f32;
    let mut dist_n = 0usize;
    for (pi, c) in subs.iter().enumerate() {
        // Skin weights per new vertex from the closest original point.
        let skin: Vec<([u16; 4], [f32; 4])> = c
            .positions
            .iter()
            .map(|&p| {
                let (ti, _, bary, d) = ogrid.closest(p).expect("non-empty grid");
                dist_sum += f64::from(d);
                dist_max = dist_max.max(d);
                dist_n += 1;
                let info = &tri_info[ti];
                let os = &osubs[info.sub];
                let items: Vec<_> = (0..3)
                    .map(|k| (os.joints[info.verts[k]], os.weights[info.verts[k]], bary[k]))
                    .collect();
                blend_weights(&items)
            })
            .collect();
        for t in c.indices.chunks_exact(3) {
            let corners = [t[0] as usize, t[1] as usize, t[2] as usize];
            let centroid = geom::scale(
                geom::add(geom::add(c.positions[corners[0]], c.positions[corners[1]]), c.positions[corners[2]]),
                1.0 / 3.0,
            );
            let n_new = geom::norm(geom::cross(
                geom::sub(c.positions[corners[1]], c.positions[corners[0]]),
                geom::sub(c.positions[corners[2]], c.positions[corners[0]]),
            ));
            // Facing-consistent closest original triangle decides material and UV chart.
            let hit = ogrid
                .closest_within(centroid, |i| geom::dot(otri_normal(i), n_new) > 0.2, 6)
                .or_else(|| ogrid.closest(centroid));
            let Some((ti, _, _, _)) = hit else { continue };
            let info = &tri_info[ti];
            let os = &osubs[info.sub];
            let out = outs.entry(os.material).or_default();
            for &v in &corners {
                let b = geom::plane_barycentric(c.positions[v], &otris[ti]);
                // Keep extrapolation inside a sane band around the chart triangle.
                let b = b.map(|x| x.clamp(-0.5, 1.5));
                let s = b[0] + b[1] + b[2];
                let b = b.map(|x| x / s);
                let uv0 = os.uvs.get(info.verts[0]).copied().unwrap_or_default();
                let uv1 = os.uvs.get(info.verts[1]).copied().unwrap_or_default();
                let uv2 = os.uvs.get(info.verts[2]).copied().unwrap_or_default();
                let uv = [
                    uv0[0] * b[0] + uv1[0] * b[1] + uv2[0] * b[2],
                    uv0[1] * b[0] + uv1[1] * b[1] + uv2[1] * b[2],
                ];
                let uv = if own_uv { c.uvs[v] } else { uv };
                let key = (pi, v, uv[0].to_bits(), uv[1].to_bits());
                let next = out.s.positions.len() as u32;
                let idx = *out.map.entry(key).or_insert_with(|| {
                    out.s.positions.push(c.positions[v]);
                    out.s.normals.push(c.normals[v]);
                    out.s.uvs.push(uv);
                    out.s.tangents.push(c.tangents[v]);
                    out.s.joints.push(skin[v].0);
                    out.s.weights.push(skin[v].1);
                    next
                });
                if out.s.lods.is_empty() {
                    out.s.lods.push(Vec::new());
                }
                out.s.lods[0].push(idx);
            }
        }
    }
    let mut materials: Vec<u8> = outs.keys().copied().collect();
    materials.sort();
    let mut out_subs = Vec::new();
    let mut sub_report = Vec::new();
    for m in &materials {
        let mut o = outs.remove(m).expect("key");
        o.s.material = format!(
            "#{m}:{}",
            actor.materials.get(*m as usize).map_or("", |x| x.name.as_str())
        );
        o.s.roughness = 0.85;
        let l0 = meshopt::optimize_vertex_cache(&o.s.lods[0], o.s.positions.len());
        o.s.lods = vec![l0];
        sub_report.push(json!({
            "material": o.s.material,
            "vertices": o.s.positions.len(),
            "triangles": o.s.lods[0].len() / 3,
        }));
        out_subs.push(o.s);
    }

    // Optional colour bake into the original UV layout.
    let mut bakes = Vec::new();
    let bake_size: u32 = args.parse("--bake-size", 2048)?;
    for spec in args.all("--bake") {
        // `1+2=STEM`: materials sharing one texture are baked into the same image.
        let (list, stem_arg) = match spec.split_once('=') {
            Some((a, b)) => (a, Some(b.to_string())),
            None => (spec, None),
        };
        let mats: Vec<u8> = list
            .split('+')
            .map(|m| m.parse::<u8>().context("bad --bake index"))
            .collect::<Result<_>>()?;
        let mi = mats[0];
        let tex_stem = stem_arg
            .or_else(|| {
                actor
                    .materials
                    .get(mi as usize)
                    .and_then(|m| m.diffuse_texture().map(str::to_string))
            })
            .context("material has no diffuse texture; pass --bake N=STEM")?;
        let r = bake(&mut orig, &model, &subs, &otris, &tri_info, &osubs, &mats, &tex_stem, bake_size)?;
        let gain = match args.flag("--gain").unwrap_or("auto") {
            "auto" => {
                let base = image::imageops::resize(
                    &orig.texture(&tex_stem)?,
                    bake_size,
                    bake_size,
                    image::imageops::FilterType::Triangle,
                );
                tex::colour_gain(tex::mean_rgb(&r.0, Some(&r.2)), tex::mean_rgb(&base, Some(&r.2)))
            }
            g => [g.parse().context("bad --gain")?; 3],
        };
        let dds = tex::diffuse(&r.0, bake_size, false, gain)?;
        let file = format!("{stem}_bake_{}.dds", tex_stem.to_ascii_lowercase());
        prof.write_file(&file, &dds.to_bytes())?;
        r.0.save(prof.root.join(format!("reports/{stem}_bake_{}.png", tex_stem.to_ascii_lowercase())))
            .ok();
        prof.set("texture", &tex_stem, &file);
        bakes.push(json!({"gain": gain, "materials": mats, "stem": tex_stem, "covered_texels": r.1, "size": bake_size}));
    }

    if own_uv {
        let img = model
            .materials
            .iter()
            .find_map(|m| m.base_color.as_ref())
            .context("generated model has no base colour texture")?;
        let mut done = std::collections::HashSet::new();
        for m in &materials {
            let Some(stem) = actor
                .materials
                .get(*m as usize)
                .and_then(|x| x.diffuse_texture().map(str::to_string))
            else {
                continue;
            };
            if !done.insert(stem.to_ascii_lowercase()) {
                continue;
            }
            let gain = match orig.texture(&stem) {
                Ok(o) if args.flag("--gain").unwrap_or("auto") == "auto" => {
                    tex::colour_gain(tex::mean_rgb(img, None), tex::mean_rgb(&o, None))
                }
                _ => [args.parse("--gain", 1.0f32)?; 3],
            };
            let dds = tex::diffuse(img, args.parse("--tex", 2048)?, false, gain)?;
            let file = format!("{stem}_own_{}.dds", stem.to_ascii_lowercase());
            let file = file.to_ascii_lowercase();
            prof.write_file(&file, &dds.to_bytes())?;
            prof.set("texture", &stem, &file);
            bakes.push(json!({"own_uv_texture": stem, "gain": gain}));
        }
    }
    let mesh = OverrideMesh {
        skinned: true,
        nodes: actor.nodes.iter().map(|n| n.name.clone()).collect(),
        submeshes: out_subs,
        collision: vec![],
    };
    let bytes = mesh.to_bytes()?;
    ensure!(OverrideMesh::parse(&bytes)? == mesh, "GRMO round trip failed");
    prof.write_file(&format!("{stem}.grmo"), &bytes)?;
    prof.set("actor", target, &format!("{stem}.grmo"));
    prof.save()?;
    let report = json!({
        "target": target,
        "source": glb_path.display().to_string(),
        "original": {"bounds": ob, "size_cm": ob.size(), "triangles": otris.len(), "nodes": actor.nodes.len()},
        "fit": fit,
        "fit_candidates": cands,
        "result": {
            "bounds": nb, "size_cm": nb.size(),
            "submeshes": sub_report,
            "orientation_agreement": orientation,
            "skin_transfer_distance_cm": {"mean": dist_sum / dist_n.max(1) as f64, "max": dist_max},
            "bakes": bakes,
            "grmo_bytes": bytes.len(),
        },
    });
    let text = serde_json::to_string_pretty(&report)?;
    prof.write_file(&format!("reports/{stem}.json"), text.as_bytes())?;
    println!("{text}");
    Ok(())
}

/// Rasterises the original triangles of the materials `mats` in UV space; every texel looks up the
/// closest facing point of the fitted generated mesh and copies its base colour. Texels the
/// material does not cover keep the original texture.
#[allow(clippy::too_many_arguments)]
fn bake(
    orig: &mut original::Original,
    model: &glb::Model,
    subs: &[crate::statics::Converted],
    otris: &[[V3; 3]],
    tri_info: &[OrigTri],
    osubs: &[OrigSub],
    mats: &[u8],
    tex_stem: &str,
    size: u32,
) -> Result<(RgbaImage, usize, Vec<bool>)> {
    let base = orig
        .texture(tex_stem)
        .with_context(|| format!("original texture {tex_stem}"))?;
    let mut canvas = image::imageops::resize(&base, size, size, image::imageops::FilterType::Lanczos3);
    // New surface with per-triangle UV + material lookup.
    let mut ntris = Vec::new();
    let mut nref = Vec::new();
    for (pi, c) in subs.iter().enumerate() {
        for t in c.indices.chunks_exact(3) {
            let v = [t[0] as usize, t[1] as usize, t[2] as usize];
            ntris.push(v.map(|i| c.positions[i]));
            nref.push((pi, v));
        }
    }
    let ob = Aabb::of(otris.iter().flatten());
    let grid = TriGrid::new(&ntris, (ob.diagonal() / 60.0).max(0.5));
    let max_dist = ob.diagonal() * 0.02;
    let mut covered = 0usize;
    let mut written = vec![false; (size * size) as usize];
    for (ti, info) in tri_info.iter().enumerate() {
        let os = &osubs[info.sub];
        if !mats.contains(&os.material) {
            continue;
        }
        let uv = info.verts.map(|i| os.uvs.get(i).copied().unwrap_or_default());
        let n = geom::norm(geom::cross(
            geom::sub(otris[ti][1], otris[ti][0]),
            geom::sub(otris[ti][2], otris[ti][0]),
        ));
        let px = uv.map(|u| [u[0] * size as f32, u[1] * size as f32]);
        let minx = px.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min).floor().max(-1e6) as i64;
        let maxx = px.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max).ceil().min(1e6) as i64;
        let miny = px.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min).floor().max(-1e6) as i64;
        let maxy = px.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max).ceil().min(1e6) as i64;
        let area = (px[1][0] - px[0][0]) * (px[2][1] - px[0][1]) - (px[2][0] - px[0][0]) * (px[1][1] - px[0][1]);
        if area.abs() < 1e-8 || (maxx - minx) * (maxy - miny) > 16_000_000 {
            continue;
        }
        // One texel of padding so the bake bleeds over the chart border.
        for y in (miny - 1)..=(maxy + 1) {
            for x in (minx - 1)..=(maxx + 1) {
                let p = [x as f32 + 0.5, y as f32 + 0.5];
                let e = |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
                let w0 = e(px[1], px[2]) / area;
                let w1 = e(px[2], px[0]) / area;
                let w2 = e(px[0], px[1]) / area;
                let pad = 1.5 / (area.abs().sqrt().max(1.0));
                if w0 < -pad || w1 < -pad || w2 < -pad {
                    continue;
                }
                let inside = w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0;
                let (tx, ty) = (x.rem_euclid(size as i64) as u32, y.rem_euclid(size as i64) as u32);
                let slot = (ty * size + tx) as usize;
                if !inside && written[slot] {
                    continue;
                }
                let pos = geom::add(
                    geom::add(geom::scale(otris[ti][0], w0), geom::scale(otris[ti][1], w1)),
                    geom::scale(otris[ti][2], w2),
                );
                let ntri_normal = |i: usize| {
                    let t = &ntris[i];
                    geom::norm(geom::cross(geom::sub(t[1], t[0]), geom::sub(t[2], t[0])))
                };
                let Some((ni, _, b, dist)) = grid
                    .closest_within(pos, |i| geom::dot(ntri_normal(i), n) > 0.0, 6)
                    .or_else(|| grid.closest(pos))
                else {
                    continue;
                };
                // Parts the generated mesh does not model (mouth interior, eyelid insides)
                // keep the original texels.
                if dist > max_dist {
                    continue;
                }
                let (pi, v) = nref[ni];
                let c = &subs[pi];
                let nuv = [
                    c.uvs[v[0]][0] * b[0] + c.uvs[v[1]][0] * b[1] + c.uvs[v[2]][0] * b[2],
                    c.uvs[v[0]][1] * b[0] + c.uvs[v[1]][1] * b[1] + c.uvs[v[2]][1] * b[2],
                ];
                let mat = &model.materials[c.material];
                let col = match &mat.base_color {
                    Some(img) => sample_bilinear(img, nuv),
                    None => mat.base_factor.map(|f| f * 255.0),
                };
                canvas.put_pixel(
                    tx,
                    ty,
                    image::Rgba([col[0] as u8, col[1] as u8, col[2] as u8, 255]),
                );
                if inside && !written[slot] {
                    covered += 1;
                }
                written[slot] |= inside;
            }
        }
    }
    Ok((canvas, covered, written))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_blend_keep_top_four_and_normalise() {
        let (j, w) = blend_weights(&[
            ([1, 2, 0, 0], [0.5, 0.5, 0.0, 0.0], 0.5),
            ([3, 4, 5, 6], [0.4, 0.3, 0.2, 0.1], 0.5),
        ]);
        let s: f32 = w.iter().sum();
        assert!((s - 1.0).abs() < 1e-5);
        assert_eq!(&j[..2], &[1, 2]);
        assert!(w[0] >= w[1] && w[1] >= w[2] && w[2] >= w[3]);
    }
}
