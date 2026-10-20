//! Gothic 3 landscape ("terrain") model.
//!
//! Finding (see `docs/research/TERRAIN_FORMAT.md`): Gothic 3 has **no heightfield
//! file**. The landscape is a set of ordinary static meshes, "landscape cells"
//! `G3_<Region>_Landscape_Cell_<N>.xcmsh` (render, with LOD sub-meshes) plus a
//! matching `.xnvmsh` (collision/navigation) stored in `_compiledMesh.pak` /
//! `_compiledPhysic`-style archives. Each cell is placed by an
//! `eCVisualMeshStatic_PS` entity named `G3_<Region>_Landscape_<layer>_<N>` inside the
//! spatial-grid world documents `SysDyn_{...}/G3_World_01_x<X>y0z<Z>_CStat.node`
//! (10 000-unit grid) in `Projects_compiled.pak`.
//!
//! This module only extracts the *placement index* from already-parsed world
//! documents and bakes a cell mesh into world space. Mesh decoding is
//! [`crate::xcmsh`], entity decoding is [`crate::world_node`].
use crate::{world_node::WorldDocument, xcmsh::Xcmsh};
use anyhow::{Result, ensure};
use serde::Serialize;

/// Maximum triangle/vertex counts accepted when merging cells into a patch.
const MAX_PATCH_VERTICES: usize = 8_000_000;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TerrainCell {
    /// `Myrtana`, `Varant`, `Nordmar`, ...
    pub region: String,
    /// Cell number from the resource name (`..._Cell_243.xcmsh` -> 243).
    pub cell: u32,
    /// Render mesh basename, e.g. `G3_Myrtana_Landscape_Cell_243.xcmsh`.
    pub mesh: String,
    /// Collision/navigation mesh basename if the entity also references one.
    pub nav_mesh: Option<String>,
    pub entity_name: String,
    /// X/Y/Z basis rows then translation row, as serialized (row-vector convention).
    pub world_matrix: [[f32; 4]; 4],
}

/// Parse `G3_<Region>_Landscape_Cell_<N>.<ext>` (ASCII case-insensitive, any directory prefix).
pub fn parse_cell_name(reference: &str) -> Option<(String, u32, String)> {
    let base = reference.rsplit(['/', '\\']).next()?;
    let (stem, ext) = base.rsplit_once('.')?;
    let ext = ext.to_ascii_lowercase();
    if ext != "xcmsh" && ext != "xnvmsh" {
        return None;
    }
    let rest = stem
        .strip_prefix("G3_")
        .or_else(|| stem.strip_prefix("g3_"))?;
    let (region, number) = rest.split_once("_Landscape_Cell_")?;
    if region.is_empty() || region.contains('_') {
        return None;
    }
    Some((region.to_string(), number.parse().ok()?, ext))
}

/// Collect landscape cells placed by a world document.
pub fn collect_cells(doc: &WorldDocument) -> Vec<TerrainCell> {
    let mut out = Vec::new();
    for e in &doc.entities {
        let mut render: Option<(String, u32, String)> = None;
        let mut nav: Option<String> = None;
        for r in &e.asset_references {
            if let Some((region, cell, ext)) = parse_cell_name(r) {
                let base = r.rsplit(['/', '\\']).next().unwrap_or(r).to_string();
                if ext == "xcmsh" {
                    render = Some((region, cell, base));
                } else {
                    nav = Some(base);
                }
            }
        }
        if let Some((region, cell, mesh)) = render {
            out.push(TerrainCell {
                region,
                cell,
                mesh,
                nav_mesh: nav,
                entity_name: e.name.clone(),
                world_matrix: e.world_matrix,
            });
        }
    }
    out
}

/// Transform a point by a serialized matrix (rows = X/Y/Z basis, translation).
pub fn transform_point(m: &[[f32; 4]; 4], p: [f32; 3]) -> [f32; 3] {
    let mut out = [0.0; 3];
    for (i, o) in out.iter_mut().enumerate() {
        *o = p[0] * m[0][i] + p[1] * m[1][i] + p[2] * m[2][i] + m[3][i];
    }
    out
}

fn transform_dir(m: &[[f32; 4]; 4], n: [f32; 3]) -> [f32; 3] {
    let mut out = [0.0; 3];
    for (i, o) in out.iter_mut().enumerate() {
        *o = n[0] * m[0][i] + n[1] * m[1][i] + n[2] * m[2][i];
    }
    let len = (out[0] * out[0] + out[1] * out[1] + out[2] * out[2]).sqrt();
    if len > 0.0 { out.map(|v| v / len) } else { out }
}

/// Merged world-space geometry (source axes/units/winding; nothing flipped).
#[derive(Debug, Default, Clone, Serialize)]
pub struct TerrainPatch {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub cells: Vec<u32>,
}

impl TerrainPatch {
    /// Append one cell's mesh, baked with its world matrix. Normals/UVs are only
    /// kept if every submesh provides them, so the arrays stay parallel to `positions`.
    pub fn append(&mut self, cell: &TerrainCell, mesh: &Xcmsh) -> Result<()> {
        for sub in &mesh.submeshes {
            ensure!(
                self.positions.len() + sub.positions.len() <= MAX_PATCH_VERTICES,
                "terrain patch exceeds vertex ceiling"
            );
            let base = u32::try_from(self.positions.len())?;
            let n = sub.positions.len();
            let has_n = sub.normals.len() == n;
            let has_uv = sub.uvs.len() == n;
            for (i, p) in sub.positions.iter().enumerate() {
                self.positions.push(transform_point(&cell.world_matrix, *p));
                self.normals.push(if has_n {
                    transform_dir(&cell.world_matrix, sub.normals[i])
                } else {
                    [0.0, 1.0, 0.0]
                });
                self.uvs.push(if has_uv { sub.uvs[i] } else { [0.0, 0.0] });
            }
            for &i in &sub.indices {
                ensure!((i as usize) < n, "submesh index out of range");
                self.indices.push(base + i);
            }
        }
        self.cells.push(cell.cell);
        Ok(())
    }

    /// Like [`append`](Self::append) but keeps only triangles whose world-space XZ
    /// bounding box overlaps `xz = [x_min, x_max, z_min, z_max]`. Vertices are
    /// re-indexed so only referenced ones are stored.
    pub fn append_in_box(&mut self, cell: &TerrainCell, mesh: &Xcmsh, xz: [f32; 4]) -> Result<()> {
        for sub in &mesh.submeshes {
            let n = sub.positions.len();
            ensure!(
                sub.indices.iter().all(|&i| (i as usize) < n),
                "submesh index out of range"
            );
            let world: Vec<[f32; 3]> = sub
                .positions
                .iter()
                .map(|p| transform_point(&cell.world_matrix, *p))
                .collect();
            let mut remap: Vec<Option<u32>> = vec![None; n];
            for t in sub.indices.as_chunks::<3>().0.iter() {
                let v = [
                    world[t[0] as usize],
                    world[t[1] as usize],
                    world[t[2] as usize],
                ];
                let (min_x, max_x) = (
                    v[0][0].min(v[1][0]).min(v[2][0]),
                    v[0][0].max(v[1][0]).max(v[2][0]),
                );
                let (min_z, max_z) = (
                    v[0][2].min(v[1][2]).min(v[2][2]),
                    v[0][2].max(v[1][2]).max(v[2][2]),
                );
                if max_x < xz[0] || min_x > xz[1] || max_z < xz[2] || min_z > xz[3] {
                    continue;
                }
                ensure!(
                    self.positions.len() + 3 <= MAX_PATCH_VERTICES,
                    "terrain patch exceeds vertex ceiling"
                );
                for &i in t {
                    let i = i as usize;
                    let out = match remap[i] {
                        Some(o) => o,
                        None => {
                            let o = u32::try_from(self.positions.len())?;
                            self.positions.push(world[i]);
                            self.normals.push(if sub.normals.len() == n {
                                transform_dir(&cell.world_matrix, sub.normals[i])
                            } else {
                                [0.0, 1.0, 0.0]
                            });
                            self.uvs.push(if sub.uvs.len() == n {
                                sub.uvs[i]
                            } else {
                                [0.0, 0.0]
                            });
                            remap[i] = Some(o);
                            o
                        }
                    };
                    self.indices.push(out);
                }
            }
        }
        self.cells.push(cell.cell);
        Ok(())
    }

    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let first = *self.positions.first()?;
        let mut lo = first;
        let mut hi = first;
        for p in &self.positions {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        Some((lo, hi))
    }

    /// Wavefront OBJ text (positions, UVs, normals, triangles) for external inspection.
    pub fn to_obj(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        for p in &self.positions {
            let _ = writeln!(s, "v {} {} {}", p[0], p[1], p[2]);
        }
        for t in &self.uvs {
            let _ = writeln!(s, "vt {} {}", t[0], t[1]);
        }
        for n in &self.normals {
            let _ = writeln!(s, "vn {} {} {}", n[0], n[1], n[2]);
        }
        for t in self.indices.as_chunks::<3>().0.iter() {
            let (a, b, c) = (t[0] + 1, t[1] + 1, t[2] + 1);
            let _ = writeln!(s, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}");
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xcmsh::Submesh;

    #[test]
    fn parses_cell_names() {
        assert_eq!(
            parse_cell_name("G3_Myrtana_Landscape_01/LOD/G3_Myrtana_Landscape_Cell_243.xcmsh"),
            Some(("Myrtana".into(), 243, "xcmsh".into()))
        );
        assert_eq!(
            parse_cell_name("G3_Varant_Landscape_Cell_0.XNVMSH"),
            Some(("Varant".into(), 0, "xnvmsh".into()))
        );
        for bad in [
            "G3_Myrtana_Landscape_Cell_x.xcmsh",
            "G3_Myrtana_Landscape_Cell_1.ximg",
            "G3_Landscape_Cell_1.xcmsh",
            "G3__Landscape_Cell_1.xcmsh",
            "Landscape_Cell_1.xcmsh",
            "",
        ] {
            assert!(parse_cell_name(bad).is_none(), "{bad}");
        }
    }

    fn cell(matrix: [[f32; 4]; 4]) -> TerrainCell {
        TerrainCell {
            region: "Myrtana".into(),
            cell: 7,
            mesh: "G3_Myrtana_Landscape_Cell_7.xcmsh".into(),
            nav_mesh: None,
            entity_name: "e".into(),
            world_matrix: matrix,
        }
    }

    #[test]
    fn bakes_cell_with_translation_and_checks_indices() {
        let m = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [100.0, 5.0, -50.0, 1.0],
        ];
        let sub = Submesh {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            normals: vec![[0.0, 1.0, 0.0]; 3],
            uvs: vec![[0.0, 0.0]; 3],
            indices: vec![0, 1, 2],
            material: String::new(),
            bounds_min: [0.0; 3],
            bounds_max: [1.0; 3],
        };
        let mut mesh = Xcmsh {
            submeshes: vec![sub],
        };
        let mut patch = TerrainPatch::default();
        patch.append(&cell(m), &mesh).unwrap();
        assert_eq!(patch.positions[1], [101.0, 5.0, -50.0]);
        assert_eq!(patch.normals[0], [0.0, 1.0, 0.0]);
        assert_eq!(patch.bounds().unwrap().0, [100.0, 5.0, -50.0]);
        assert!(patch.to_obj().contains("f 1/1/1 2/2/2 3/3/3"));
        mesh.submeshes[0].indices[2] = 9;
        assert!(patch.append(&cell(m), &mesh).is_err());
    }
}
