//! Cooked static meshes (`.gmsh`): one interleaved GPU-ready vertex buffer for all
//! submeshes, per-submesh index ranges for up to three LODs, already in Bevy convention
//! (X mirrored; the mirror turns the source clockwise front faces into Bevy's
//! counter-clockwise ones, so index order is unchanged).
use crate::container::{Container, Kind, Writer};
use crate::ids::{AssetId, Profile};
use crate::mmap::Blob;
use anyhow::{Result, ensure};
use bytemuck::{Pod, Zeroable};
use gothic_formats::xcmsh::{Submesh, Xcmsh};
use std::sync::Arc;

pub const MAX_LODS: usize = 3;

/// Interleaved vertex, 52 bytes. `tangent.w` is the bitangent sign.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default, PartialEq, Debug)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub tangent: [f32; 4],
    /// RGB = baked ambient occlusion, A = vertex blend weight (UNORM8).
    pub color: [u8; 4],
}

pub const FLAG_TERRAIN: u32 = 1;
pub const FLAG_WIDE_INDICES: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct MeshHeader {
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
    pub vertex_count: u32,
    pub index_count: u32,
    pub submesh_count: u32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default, Debug)]
pub struct LodRange {
    pub first_index: u32,
    pub index_count: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct SubmeshRec {
    /// [`AssetId`] of the material name; look it up in the material table.
    pub material: u64,
    pub first_vertex: u32,
    pub vertex_count: u32,
    /// Index ranges for LOD0..LOD2. Indices are relative to `first_vertex`.
    pub lods: [LodRange; MAX_LODS],
    /// Relative simplification error per LOD (0 for LOD0).
    pub lod_error: [f32; MAX_LODS],
    pub lod_count: u32,
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
}

/// Result of cooking one mesh (bytes plus what later phases need).
pub struct CookedMesh {
    pub bytes: Vec<u8>,
    pub material_names: Vec<String>,
    pub bounds: ([f32; 3], [f32; 3]),
    pub triangles: usize,
}

fn convert_vertex(sub: &Submesh, k: usize) -> Vertex {
    let p = sub.positions[k];
    let n = sub.normals.get(k).copied().unwrap_or([0.0, 1.0, 0.0]);
    let ao = sub.colors5.get(k).map_or(255, |c| c[3]);
    let w = sub.colors4.get(k).map_or(255, |c| c[3]);
    Vertex {
        pos: [-p[0], p[1], p[2]],
        normal: [-n[0], n[1], n[2]],
        uv: sub.uvs.get(k).copied().unwrap_or([0.0, 0.0]),
        tangent: sub
            .tangents
            .get(k)
            .map_or([1.0, 0.0, 0.0, -1.0], |t| [-t[0], t[1], t[2], -1.0]),
        color: [ao, ao, ao, w],
    }
}

fn aabb(vs: &[Vertex]) -> ([f32; 3], [f32; 3]) {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for v in vs {
        for a in 0..3 {
            lo[a] = lo[a].min(v.pos[a]);
            hi[a] = hi[a].max(v.pos[a]);
        }
    }
    (lo, hi)
}

/// Cook a parsed source mesh. `terrain` meshes get no LODs (they are tiled by the world).
pub fn cook_mesh(src: &Xcmsh, profile: &Profile, terrain: bool, key: u64) -> Result<CookedMesh> {
    struct Sub {
        material: String,
        verts: Vec<Vertex>,
        lods: Vec<Vec<u32>>,
        errors: Vec<f32>,
    }
    let mut subs: Vec<Sub> = Vec::new();
    for s in &src.submeshes {
        let n = s.positions.len();
        if n == 0 || s.indices.len() < 3 {
            continue;
        }
        let mut idx: Vec<u32> = s
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .filter(|t| t.iter().all(|&i| (i as usize) < n))
            .flatten()
            .copied()
            .collect();
        if idx.is_empty() {
            continue;
        }
        let verts: Vec<Vertex> = (0..n).map(|k| convert_vertex(s, k)).collect();
        // Weld identical vertices, then optimise for the vertex cache.
        let (count, remap) = meshopt::generate_vertex_remap(&verts, Some(&idx));
        let verts = meshopt::remap_vertex_buffer(&verts, count, &remap);
        idx = meshopt::remap_index_buffer(Some(&idx), n, &remap);
        idx = meshopt::optimize_vertex_cache(&idx, verts.len());

        let mut lods = vec![idx.clone()];
        let mut errors = vec![0.0f32];
        let tris = idx.len() / 3;
        if !terrain && tris >= profile.lod_min_triangles {
            let adapter = meshopt::VertexDataAdapter::new(
                bytemuck::cast_slice(&verts),
                std::mem::size_of::<Vertex>(),
                0,
            )?;
            for ratio in profile.lod_ratios {
                if ratio <= 0.0 {
                    continue;
                }
                let target = ((tris as f32 * ratio) as usize * 3).max(12);
                let mut err = 0.0f32;
                let lod = meshopt::simplify(
                    &idx,
                    &adapter,
                    target,
                    0.05,
                    meshopt::SimplifyOptions::None,
                    Some(&mut err),
                );
                let last = lods.last().map_or(0, Vec::len);
                // Keep a LOD only when it is meaningfully smaller than the previous one.
                if lod.len() >= 12 && lod.len() * 10 <= last * 8 {
                    lods.push(meshopt::optimize_vertex_cache(&lod, verts.len()));
                    errors.push(err);
                }
            }
        }
        // Fetch-order the vertices by LOD0 and remap every LOD to the new order.
        let remap = meshopt::optimize_vertex_fetch_remap(&lods[0], verts.len());
        let used = remap.iter().filter(|&&r| r != u32::MAX).count();
        let verts = meshopt::remap_vertex_buffer(&verts, used, &remap);
        let lods = lods
            .iter()
            .map(|l| meshopt::remap_index_buffer(Some(l), remap.len(), &remap))
            .collect();
        subs.push(Sub {
            material: s.material.clone(),
            verts,
            lods,
            errors,
        });
    }
    ensure!(!subs.is_empty(), "mesh has no usable geometry");

    let wide = subs.iter().any(|s| s.verts.len() > 65_535);
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut idx16: Vec<u16> = Vec::new();
    let mut idx32: Vec<u32> = Vec::new();
    let mut recs = Vec::new();
    let mut names = Vec::new();
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    let mut triangles = 0;
    for s in &subs {
        let (bmin, bmax) = aabb(&s.verts);
        for a in 0..3 {
            lo[a] = lo[a].min(bmin[a]);
            hi[a] = hi[a].max(bmax[a]);
        }
        let mut rec = SubmeshRec {
            material: AssetId::of(&s.material).0,
            first_vertex: vertices.len() as u32,
            vertex_count: s.verts.len() as u32,
            lods: [LodRange::default(); MAX_LODS],
            lod_error: [0.0; MAX_LODS],
            lod_count: s.lods.len() as u32,
            bounds_min: bmin,
            bounds_max: bmax,
        };
        for (l, lod) in s.lods.iter().enumerate().take(MAX_LODS) {
            let first = if wide { idx32.len() } else { idx16.len() };
            rec.lods[l] = LodRange {
                first_index: first as u32,
                index_count: lod.len() as u32,
            };
            rec.lod_error[l] = s.errors[l];
            if wide {
                idx32.extend_from_slice(lod);
            } else {
                idx16.extend(lod.iter().map(|&i| i as u16));
            }
        }
        triangles += s.lods[0].len() / 3;
        vertices.extend_from_slice(&s.verts);
        recs.push(rec);
        names.push(s.material.clone());
    }
    let header = MeshHeader {
        bounds_min: lo,
        bounds_max: hi,
        vertex_count: vertices.len() as u32,
        index_count: if wide { idx32.len() } else { idx16.len() } as u32,
        submesh_count: recs.len() as u32,
        flags: if terrain { FLAG_TERRAIN } else { 0 } | if wide { FLAG_WIDE_INDICES } else { 0 },
    };
    let mut w = Writer::new();
    w.one(&header).slice(&vertices);
    if wide {
        w.slice(&idx32);
    } else {
        w.slice(&idx16);
    }
    w.slice(&recs).raw(names.join("\0").into_bytes());
    Ok(CookedMesh {
        bytes: w.finish(Kind::Mesh, key),
        material_names: names,
        bounds: (lo, hi),
        triangles,
    })
}

/// Indices of one submesh LOD.
#[derive(Clone, Copy)]
pub enum Indices<'a> {
    U16(&'a [u16]),
    U32(&'a [u32]),
}

impl Indices<'_> {
    pub fn len(&self) -> usize {
        match self {
            Indices::U16(s) => s.len(),
            Indices::U32(s) => s.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn get(&self, i: usize) -> u32 {
        match self {
            Indices::U16(s) => u32::from(s[i]),
            Indices::U32(s) => s[i],
        }
    }
}

/// Zero-copy view over a cooked mesh file.
#[derive(Clone)]
pub struct MeshData {
    c: Container,
}

impl MeshData {
    pub fn parse(blob: Arc<Blob>) -> Result<Self> {
        let c = Container::parse(blob, Kind::Mesh)?;
        ensure!(c.section_count() == 5, "mesh section count");
        let m = Self { c };
        let h = m.header();
        ensure!(
            m.vertices().len() == h.vertex_count as usize
                && m.submeshes().len() == h.submesh_count as usize,
            "mesh section sizes disagree with header"
        );
        Ok(m)
    }
    pub fn header(&self) -> MeshHeader {
        self.c.one(0).unwrap_or_else(Zeroable::zeroed)
    }
    pub fn vertices(&self) -> &[Vertex] {
        self.c.slice(1)
    }
    pub fn submeshes(&self) -> &[SubmeshRec] {
        self.c.slice(3)
    }
    pub fn is_terrain(&self) -> bool {
        self.header().flags & FLAG_TERRAIN != 0
    }
    pub fn indices(&self, sub: &SubmeshRec, lod: usize) -> Indices<'_> {
        let lod = lod.min((sub.lod_count as usize).max(1) - 1);
        let r = sub.lods[lod];
        let (a, b) = (
            r.first_index as usize,
            (r.first_index + r.index_count) as usize,
        );
        if self.header().flags & FLAG_WIDE_INDICES != 0 {
            Indices::U32(self.c.slice::<u32>(2).get(a..b).unwrap_or(&[]))
        } else {
            Indices::U16(self.c.slice::<u16>(2).get(a..b).unwrap_or(&[]))
        }
    }
    /// Vertices of one submesh (index values are relative to this slice).
    pub fn submesh_vertices(&self, sub: &SubmeshRec) -> &[Vertex] {
        let a = sub.first_vertex as usize;
        self.vertices()
            .get(a..a + sub.vertex_count as usize)
            .unwrap_or(&[])
    }
    pub fn material_names(&self) -> Vec<String> {
        String::from_utf8_lossy(self.c.raw(4))
            .split('\0')
            .map(str::to_owned)
            .collect()
    }
}
