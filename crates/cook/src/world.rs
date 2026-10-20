//! Packed world cells (`cells/<cx>_<cz>.gcell`) and the shared collision pack.
//!
//! A cell covers `CELL_SIZE` source units (cm) square in *Gothic* XZ:
//! `cx = floor(x / 10000)`. Everything the streamer needs for it sits in one file:
//! prop instances (mesh id, collision id, Bevy-convention transform), terrain mesh
//! references overlapping the cell, and the terrain collision triangles whose centroid
//! lies inside it (already mirrored to Bevy convention, centimetres).
use crate::container::{Container, Kind, Writer};
use crate::mmap::Blob;
use anyhow::{Result, ensure};
use bytemuck::{Pod, Zeroable};
use std::sync::Arc;

pub const CELL_SIZE: f32 = 10_000.0;

pub fn cell_of(gothic_x: f32, gothic_z: f32) -> (i32, i32) {
    (
        (gothic_x / CELL_SIZE).floor() as i32,
        (gothic_z / CELL_SIZE).floor() as i32,
    )
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct CellHeader {
    pub cx: i32,
    pub cz: i32,
    pub instance_count: u32,
    pub terrain_count: u32,
    pub triangle_count: u32,
    pub _pad: u32,
}

/// A placed prop. `transform` is four columns (X, Y, Z axis, translation) of the affine
/// matrix in Bevy convention (`F * M * F` with `F = scale(-1, 1, 1)`), centimetres.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
pub struct Instance {
    pub mesh: u64,
    /// Collision mesh id in the collision pack, or 0.
    pub collision: u64,
    pub transform: [[f32; 3]; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
pub struct TerrainRef {
    pub mesh: u64,
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
}

/// Serialized row-major basis + translation -> Bevy-convention affine columns.
pub fn bevy_transform(m: &[[f32; 4]; 4]) -> [[f32; 3]; 4] {
    const S: [f32; 3] = [-1.0, 1.0, 1.0];
    let mut out = [[0.0; 3]; 4];
    for j in 0..3 {
        for i in 0..3 {
            out[j][i] = S[i] * S[j] * m[j][i];
        }
    }
    for i in 0..3 {
        out[3][i] = S[i] * m[3][i];
    }
    out
}

pub struct CellBuilder {
    pub cx: i32,
    pub cz: i32,
    pub instances: Vec<Instance>,
    pub terrain: Vec<TerrainRef>,
    /// Flat list of triangle corners (3 entries per triangle).
    pub collision: Vec<[f32; 3]>,
}

impl CellBuilder {
    pub fn new(cx: i32, cz: i32) -> Self {
        Self {
            cx,
            cz,
            instances: Vec::new(),
            terrain: Vec::new(),
            collision: Vec::new(),
        }
    }
    pub fn write(mut self, key: u64) -> Vec<u8> {
        // Deterministic content regardless of thread scheduling.
        self.instances.sort_by(|a, b| {
            (a.mesh, a.collision, bytemuck::bytes_of(&a.transform)).cmp(&(
                b.mesh,
                b.collision,
                bytemuck::bytes_of(&b.transform),
            ))
        });
        self.terrain.sort_by_key(|t| t.mesh);
        let header = CellHeader {
            cx: self.cx,
            cz: self.cz,
            instance_count: self.instances.len() as u32,
            terrain_count: self.terrain.len() as u32,
            triangle_count: (self.collision.len() / 3) as u32,
            _pad: 0,
        };
        let mut w = Writer::new();
        w.one(&header)
            .slice(&self.instances)
            .slice(&self.terrain)
            .slice(&self.collision);
        w.finish(Kind::Cell, key)
    }
}

#[derive(Clone)]
pub struct CellData {
    c: Container,
}

impl CellData {
    pub fn parse(blob: Arc<Blob>) -> Result<Self> {
        let c = Container::parse(blob, Kind::Cell)?;
        ensure!(c.section_count() == 4, "cell sections");
        let s = Self { c };
        let h = s.header();
        ensure!(
            s.instances().len() == h.instance_count as usize
                && s.terrain().len() == h.terrain_count as usize
                && s.collision_corners().len() == 3 * h.triangle_count as usize,
            "cell sizes disagree with header"
        );
        Ok(s)
    }
    pub fn header(&self) -> CellHeader {
        self.c.one(0).unwrap_or_else(Zeroable::zeroed)
    }
    pub fn instances(&self) -> &[Instance] {
        self.c.slice(1)
    }
    pub fn terrain(&self) -> &[TerrainRef] {
        self.c.slice(2)
    }
    pub fn collision_corners(&self) -> &[[f32; 3]] {
        self.c.slice(3)
    }
    pub fn collision_triangles(&self) -> impl Iterator<Item = &[[f32; 3]; 3]> {
        self.collision_corners().as_chunks::<3>().0.iter()
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct ColRec {
    pub id: u64,
    pub first_vertex: u32,
    pub vertex_count: u32,
    pub first_index: u32,
    pub index_count: u32,
}

/// Builder of the collision pack: meshes in centimetres, Bevy convention.
#[derive(Default)]
pub struct CollisionBuilder {
    recs: Vec<ColRec>,
    verts: Vec<[f32; 3]>,
    indices: Vec<u32>,
}

impl CollisionBuilder {
    pub fn add(&mut self, id: u64, verts: &[[f32; 3]], indices: &[u32]) {
        self.recs.push(ColRec {
            id,
            first_vertex: self.verts.len() as u32,
            vertex_count: verts.len() as u32,
            first_index: self.indices.len() as u32,
            index_count: indices.len() as u32,
        });
        self.verts.extend_from_slice(verts);
        self.indices.extend_from_slice(indices);
    }
    pub fn finish(mut self, key: u64) -> Vec<u8> {
        // Sort records by id; data order stays as inserted (records point into it).
        self.recs.sort_by_key(|r| r.id);
        let mut w = Writer::new();
        w.slice(&self.recs).slice(&self.verts).slice(&self.indices);
        w.finish(Kind::Collision, key)
    }
}

#[derive(Clone)]
pub struct CollisionPack {
    c: Container,
}

pub struct CollisionMesh<'a> {
    pub vertices: &'a [[f32; 3]],
    /// Triangle list, relative to `vertices`.
    pub indices: &'a [u32],
}

impl CollisionPack {
    pub fn parse(blob: Arc<Blob>) -> Result<Self> {
        let c = Container::parse(blob, Kind::Collision)?;
        ensure!(c.section_count() == 3, "collision sections");
        Ok(Self { c })
    }
    pub fn records(&self) -> &[ColRec] {
        self.c.slice(0)
    }
    pub fn get(&self, id: u64) -> Option<CollisionMesh<'_>> {
        let recs = self.records();
        let r = recs
            .binary_search_by_key(&id, |r| r.id)
            .ok()
            .map(|i| &recs[i])?;
        let v = self.c.slice::<[f32; 3]>(1);
        let i = self.c.slice::<u32>(2);
        Some(CollisionMesh {
            vertices: v.get(r.first_vertex as usize..(r.first_vertex + r.vertex_count) as usize)?,
            indices: i.get(r.first_index as usize..(r.first_index + r.index_count) as usize)?,
        })
    }
}
