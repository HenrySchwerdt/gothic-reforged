//! Loads a generated model (binary glTF) into flat primitives in glTF space
//! (right-handed, Y up, metres, counter-clockwise front faces). Node transforms are applied.
use anyhow::{Context, Result, bail, ensure};
use image::RgbaImage;
use std::path::Path;

pub struct Material {
    pub name: String,
    pub base_color: Option<RgbaImage>,
    pub base_factor: [f32; 4],
    pub normal: Option<RgbaImage>,
    /// glTF metallic-roughness texture (G roughness, B metallic).
    pub metal_rough: Option<RgbaImage>,
    pub roughness_factor: f32,
    pub alpha_mask: bool,
}

pub struct Primitive {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub material: usize,
}

pub struct Model {
    pub primitives: Vec<Primitive>,
    pub materials: Vec<Material>,
}

const MAX_VERTICES: usize = 4_000_000;

fn to_rgba(img: &gltf::image::Data) -> Result<RgbaImage> {
    use gltf::image::Format;
    let (w, h) = (img.width, img.height);
    let px = &img.pixels;
    let rgba: Vec<u8> = match img.format {
        Format::R8G8B8A8 => px.clone(),
        Format::R8G8B8 => px.chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        Format::R8G8 => px.chunks_exact(2).flat_map(|c| [c[0], c[1], 0, 255]).collect(),
        Format::R8 => px.iter().flat_map(|&c| [c, c, c, 255]).collect(),
        other => bail!("unsupported embedded image format {other:?}"),
    };
    RgbaImage::from_raw(w, h, rgba).context("image size mismatch")
}

pub fn load(path: &Path) -> Result<Model> {
    load_filtered(path, |_| true)
}

/// Loads only the scene nodes (and their children) whose name passes `keep`.
pub fn load_filtered(path: &Path, keep: impl Fn(&str) -> bool) -> Result<Model> {
    let (doc, buffers, images) =
        gltf::import(path).with_context(|| format!("importing {}", path.display()))?;
    let image = |tex: Option<gltf::texture::Texture>| -> Result<Option<RgbaImage>> {
        match tex {
            Some(t) => {
                let i = t.source().index();
                Ok(Some(to_rgba(images.get(i).context("missing image")?)?))
            }
            None => Ok(None),
        }
    };
    let mut materials = Vec::new();
    for m in doc.materials() {
        let pbr = m.pbr_metallic_roughness();
        materials.push(Material {
            name: m.name().unwrap_or("material").to_string(),
            base_color: image(pbr.base_color_texture().map(|t| t.texture()))?,
            base_factor: pbr.base_color_factor(),
            normal: image(m.normal_texture().map(|t| t.texture()))?,
            metal_rough: image(pbr.metallic_roughness_texture().map(|t| t.texture()))?,
            roughness_factor: pbr.roughness_factor(),
            alpha_mask: !matches!(m.alpha_mode(), gltf::material::AlphaMode::Opaque),
        });
    }
    if materials.is_empty() {
        materials.push(Material {
            name: "default".into(),
            base_color: None,
            base_factor: [0.6, 0.6, 0.6, 1.0],
            normal: None,
            metal_rough: None,
            roughness_factor: 0.9,
            alpha_mask: false,
        });
    }
    let mut primitives = Vec::new();
    let mut total = 0usize;
    let scene = doc
        .default_scene()
        .or_else(|| doc.scenes().next())
        .context("glTF has no scene")?;
    let mut stack: Vec<(gltf::Node, [[f32; 4]; 4])> = scene
        .nodes()
        .filter(|n| keep(n.name().unwrap_or("")))
        .map(|n| (n, identity()))
        .collect();
    while let Some((node, parent)) = stack.pop() {
        let world = mul(parent, node.transform().matrix());
        for child in node.children() {
            stack.push((child, world));
        }
        let Some(mesh) = node.mesh() else { continue };
        let normal_m = normal_matrix(world);
        for prim in mesh.primitives() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = prim.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
            let positions: Vec<[f32; 3]> = reader
                .read_positions()
                .context("primitive without positions")?
                .map(|p| xform(world, p))
                .collect();
            let n = positions.len();
            total += n;
            ensure!(total <= MAX_VERTICES, "model exceeds vertex budget");
            let normals: Vec<[f32; 3]> = match reader.read_normals() {
                Some(it) => it.map(|v| normalize(xform3(normal_m, v))).collect(),
                None => vec![[0.0, 1.0, 0.0]; n],
            };
            let uvs: Vec<[f32; 2]> = match reader.read_tex_coords(0) {
                Some(t) => t.into_f32().collect(),
                None => vec![[0.0, 0.0]; n],
            };
            let indices: Vec<u32> = match reader.read_indices() {
                Some(i) => i.into_u32().collect(),
                None => (0..n as u32).collect(),
            };
            ensure!(
                indices.iter().all(|&i| (i as usize) < n) && indices.len() % 3 == 0,
                "invalid index buffer"
            );
            ensure!(normals.len() == n && uvs.len() == n, "vertex streams differ");
            // A mirroring node transform flips the winding.
            let mut indices = indices;
            if det3(world) < 0.0 {
                for t in indices.chunks_exact_mut(3) {
                    t.swap(1, 2);
                }
            }
            primitives.push(Primitive {
                positions,
                normals,
                uvs,
                indices,
                material: prim.material().index().unwrap_or(0).min(materials.len() - 1),
            });
        }
    }
    ensure!(!primitives.is_empty(), "model has no triangle primitives");
    Ok(Model {
        primitives,
        materials,
    })
}

fn identity() -> [[f32; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// Column-major 4x4 product `a * b` (glTF matrices are column-major `[col][row]`).
fn mul(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut o = [[0.0; 4]; 4];
    for (c, oc) in o.iter_mut().enumerate() {
        for (r, v) in oc.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    o
}

fn xform(m: [[f32; 4]; 4], p: [f32; 3]) -> [f32; 3] {
    let mut o = [0.0; 3];
    for (r, v) in o.iter_mut().enumerate() {
        *v = m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r];
    }
    o
}

fn xform3(m: [[f32; 3]; 3], p: [f32; 3]) -> [f32; 3] {
    let mut o = [0.0; 3];
    for (r, v) in o.iter_mut().enumerate() {
        *v = m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2];
    }
    o
}

fn det3(m: [[f32; 4]; 4]) -> f32 {
    m[0][0] * (m[1][1] * m[2][2] - m[2][1] * m[1][2]) - m[1][0] * (m[0][1] * m[2][2] - m[2][1] * m[0][2])
        + m[2][0] * (m[0][1] * m[1][2] - m[1][1] * m[0][2])
}

/// Inverse transpose of the upper 3x3 (column-major).
fn normal_matrix(m: [[f32; 4]; 4]) -> [[f32; 3]; 3] {
    let a = |c: usize, r: usize| m[c][r];
    let det = det3(m);
    let inv = if det.abs() < 1e-20 { 1.0 } else { 1.0 / det };
    // cofactor matrix == transpose of the inverse times det
    let mut o = [[0.0; 3]; 3];
    for c in 0..3 {
        for r in 0..3 {
            let (c1, c2) = ((c + 1) % 3, (c + 2) % 3);
            let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
            o[c][r] = (a(c1, r1) * a(c2, r2) - a(c2, r1) * a(c1, r2)) * inv;
        }
    }
    o
}

pub fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-12 {
        [v[0] / l, v[1] / l, v[2] / l]
    } else {
        [0.0, 1.0, 0.0]
    }
}
