//! Cooked actors (`.gact`: resolved skeleton + skinned vertex buffers) and motions
//! (`.gmot`: compact per-bone tracks, optional 16-bit quantized rotations).
//!
//! Both stay in the **source (Gothic) convention** (unmirrored): the animation crate
//! samples them in that space and mirrors at the end, exactly as it does for parsed
//! `Actor` / `Motion` today.
use crate::container::{Container, Kind, Writer};
use crate::ids::{AssetId, Profile};
use crate::mmap::Blob;
use anyhow::{Result, ensure};
use bytemuck::{Pod, Zeroable};
use gothic_formats::{xact::Actor, xmot::Motion};
use std::sync::Arc;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct ActorHeader {
    pub node_count: u32,
    pub material_count: u32,
    pub sub_count: u32,
    pub vertex_count: u32,
    pub index_count: u32,
    pub _pad: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct ActorNodeRec {
    pub name_hash: u64,
    /// Parent node index or -1.
    pub parent: i32,
    pub _pad: u32,
    pub position: [f32; 3],
    /// Quaternion x, y, z, w.
    pub rotation: [f32; 4],
    pub scale_rotation: [f32; 4],
    pub scale: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct ActorMatRec {
    /// Texture ids (0 = none), `<name>.ximg` hashed like all texture ids.
    pub diffuse_tex: u64,
    pub normal_tex: u64,
    /// Id of the referenced `.xshmat` material, or 0.
    pub shader_material: u64,
    pub diffuse: [f32; 3],
    pub opacity: f32,
    pub flags: u32,
    pub transparency: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct ActorSubRec {
    pub node: u32,
    pub material: u32,
    pub first_vertex: u32,
    pub vertex_count: u32,
    pub first_index: u32,
    pub index_count: u32,
    pub skinned: u32,
    pub _pad: u32,
}

/// 44 bytes. `weights` are UNORM8 summing to 255.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default, Debug, PartialEq)]
pub struct SkinVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub joints: [u16; 4],
    pub weights: [u8; 4],
}

pub struct CookedActor {
    pub bytes: Vec<u8>,
    /// `ximg` names and `xshmat` names the actor needs.
    pub textures: Vec<(String, bool)>,
    pub materials: Vec<String>,
}

fn quant_weights(w: [f32; 4]) -> [u8; 4] {
    let sum: f32 = w.iter().map(|v| v.max(0.0)).sum();
    if sum <= 0.0 {
        return [255, 0, 0, 0];
    }
    let mut q = [0u8; 4];
    let mut total = 0i32;
    for i in 0..4 {
        q[i] = (w[i].max(0.0) / sum * 255.0).round() as u8;
        total += i32::from(q[i]);
    }
    let big = (0..4).max_by_key(|&i| q[i]).unwrap_or(0);
    q[big] = (i32::from(q[big]) + 255 - total).clamp(0, 255) as u8;
    q
}

pub fn cook_actor(a: &Actor, key: u64, exists: &dyn Fn(&str) -> bool) -> Result<CookedActor> {
    let tex = |name: &str| format!("{}.ximg", name.to_ascii_lowercase());
    let mut textures = Vec::new();
    let nodes: Vec<ActorNodeRec> = a
        .nodes
        .iter()
        .map(|n| ActorNodeRec {
            name_hash: AssetId::of(&n.name).0,
            parent: n.parent.map_or(-1, |p| p as i32),
            _pad: 0,
            position: n.position,
            rotation: n.rotation,
            scale_rotation: n.scale_rotation,
            scale: n.scale,
        })
        .collect();
    let mut mats = Vec::new();
    let mut materials: Vec<String> = Vec::new();
    for (i, m) in a.materials.iter().enumerate() {
        let layer = |ty: u8| {
            m.layers
                .iter()
                .find(|l| l.map_type == ty)
                .map(|l| tex(&l.texture))
                .filter(|t| exists(t))
        };
        // The listed material can be a generic exporter name belonging to another model, so the
        // diffuse layer's own `_A.xshmat` / `.xshmat` are tried first (as the animation crate does).
        let diffuse_raw = m.diffuse_texture().map(str::to_owned);
        let mut candidates: Vec<String> = Vec::new();
        if let Some(t) = &diffuse_raw {
            candidates.push(format!("{t}_A.xshmat"));
            candidates.push(format!("{t}.xshmat"));
        }
        candidates.extend(a.material_file(i).map(str::to_owned));
        let shader = candidates.into_iter().find(|c| exists(c));
        if let Some(s) = &shader {
            materials.push(s.clone());
        }
        let diffuse = layer(1);
        let normal = layer(3);
        if let Some(t) = &diffuse {
            textures.push((t.clone(), true));
        }
        if let Some(t) = &normal {
            textures.push((t.clone(), false));
        }
        mats.push(ActorMatRec {
            diffuse_tex: diffuse.as_deref().map_or(0, |t| AssetId::of(t).0),
            normal_tex: normal.as_deref().map_or(0, |t| AssetId::of(t).0),
            shader_material: shader.as_deref().map_or(0, |f| AssetId::of(f).0),
            diffuse: m.diffuse,
            opacity: m.opacity,
            flags: u32::from(m.double_sided),
            transparency: u32::from(m.transparency),
        });
    }
    let mut verts: Vec<SkinVertex> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let mut subs = Vec::new();
    for mesh in &a.meshes {
        for s in &mesh.submeshes {
            let n = s.positions.len();
            if n == 0 || s.indices.len() < 3 {
                continue;
            }
            let first_vertex = verts.len() as u32;
            for k in 0..n {
                verts.push(SkinVertex {
                    pos: s.positions[k],
                    normal: s.normals.get(k).copied().unwrap_or([0.0, 1.0, 0.0]),
                    uv: s.uvs.get(k).copied().unwrap_or([0.0, 0.0]),
                    joints: s.joints.get(k).copied().unwrap_or([0; 4]),
                    weights: s
                        .weights
                        .get(k)
                        .map_or([255, 0, 0, 0], |w| quant_weights(*w)),
                });
            }
            let first_index = idx.len() as u32;
            idx.extend(s.indices.iter().copied().filter(|&i| (i as usize) < n));
            subs.push(ActorSubRec {
                node: mesh.node,
                material: u32::from(s.material),
                first_vertex,
                vertex_count: n as u32,
                first_index,
                index_count: idx.len() as u32 - first_index,
                skinned: u32::from(mesh.skinned),
                _pad: 0,
            });
        }
    }
    let header = ActorHeader {
        node_count: nodes.len() as u32,
        material_count: mats.len() as u32,
        sub_count: subs.len() as u32,
        vertex_count: verts.len() as u32,
        index_count: idx.len() as u32,
        _pad: [0; 3],
    };
    let names: Vec<&str> = a.nodes.iter().map(|n| n.name.as_str()).collect();
    let mut w = Writer::new();
    w.one(&header)
        .slice(&nodes)
        .slice(&mats)
        .slice(&subs)
        .slice(&verts)
        .slice(&idx)
        .raw(names.join("\0").into_bytes());
    Ok(CookedActor {
        bytes: w.finish(Kind::Actor, key),
        textures,
        materials,
    })
}

#[derive(Clone)]
pub struct ActorData {
    c: Container,
}

impl ActorData {
    pub fn parse(blob: Arc<Blob>) -> Result<Self> {
        let c = Container::parse(blob, Kind::Actor)?;
        ensure!(c.section_count() == 7, "actor sections");
        let a = Self { c };
        let h = a.header();
        ensure!(
            a.nodes().len() == h.node_count as usize
                && a.vertices().len() == h.vertex_count as usize
                && a.indices().len() == h.index_count as usize,
            "actor sizes disagree with header"
        );
        Ok(a)
    }
    pub fn header(&self) -> ActorHeader {
        self.c.one(0).unwrap_or_else(Zeroable::zeroed)
    }
    pub fn nodes(&self) -> &[ActorNodeRec] {
        self.c.slice(1)
    }
    pub fn materials(&self) -> &[ActorMatRec] {
        self.c.slice(2)
    }
    pub fn submeshes(&self) -> &[ActorSubRec] {
        self.c.slice(3)
    }
    pub fn vertices(&self) -> &[SkinVertex] {
        self.c.slice(4)
    }
    pub fn indices(&self) -> &[u32] {
        self.c.slice(5)
    }
    pub fn node_names(&self) -> Vec<String> {
        String::from_utf8_lossy(self.c.raw(6))
            .split('\0')
            .map(str::to_owned)
            .collect()
    }
}

// ---------------------------------------------------------------- motions

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct MotionHeader {
    pub node_count: u32,
    pub duration: f32,
    /// 1 when rotation keys are [`QuatKeyQ`], 0 when [`QuatKey`].
    pub quantized: u32,
    pub _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct MotionNodeRec {
    pub name_hash: u64,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    /// (first, count) into the key arrays.
    pub pos: [u32; 2],
    pub rot: [u32; 2],
    pub scl: [u32; 2],
    pub _pad: [u32; 2],
}

/// Time + xyz.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct Vec3Key {
    pub t: f32,
    pub v: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct QuatKey {
    pub t: f32,
    pub q: [f32; 4],
}

/// Time + snorm16 quaternion (x, y, z, w), 12 bytes.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct QuatKeyQ {
    pub t: f32,
    pub q: [i16; 4],
}

impl QuatKeyQ {
    pub fn quat(&self) -> [f32; 4] {
        let mut q = self.q.map(|v| f32::from(v) / 32767.0);
        let len = q.iter().map(|v| v * v).sum::<f32>().sqrt();
        if len > 0.0 {
            q = q.map(|v| v / len);
        }
        q
    }
}

pub fn cook_motion(m: &Motion, profile: &Profile, key: u64) -> Result<Vec<u8>> {
    let mut recs = Vec::new();
    let mut pos: Vec<Vec3Key> = Vec::new();
    let mut scl: Vec<Vec3Key> = Vec::new();
    let mut rot: Vec<QuatKey> = Vec::new();
    for n in &m.nodes {
        let p0 = pos.len() as u32;
        pos.extend(
            n.positions
                .keys
                .iter()
                .map(|(t, v)| Vec3Key { t: *t, v: *v }),
        );
        let r0 = rot.len() as u32;
        rot.extend(
            n.rotations
                .keys
                .iter()
                .map(|(t, q)| QuatKey { t: *t, q: *q }),
        );
        let s0 = scl.len() as u32;
        scl.extend(n.scales.keys.iter().map(|(t, v)| Vec3Key { t: *t, v: *v }));
        recs.push(MotionNodeRec {
            name_hash: AssetId::of(&n.name).0,
            position: n.position,
            rotation: n.rotation,
            scale: n.scale,
            pos: [p0, pos.len() as u32 - p0],
            rot: [r0, rot.len() as u32 - r0],
            scl: [s0, scl.len() as u32 - s0],
            _pad: [0; 2],
        });
    }
    let header = MotionHeader {
        node_count: recs.len() as u32,
        duration: m.duration,
        quantized: u32::from(profile.quantize_motion),
        _pad: 0,
    };
    let mut w = Writer::new();
    w.one(&header).slice(&recs).slice(&pos).slice(&scl);
    if profile.quantize_motion {
        let q: Vec<QuatKeyQ> = rot
            .iter()
            .map(|k| {
                let len = k.q.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
                QuatKeyQ {
                    t: k.t,
                    q: k.q.map(|v| (v / len * 32767.0).round() as i16),
                }
            })
            .collect();
        w.slice(&q);
    } else {
        w.slice(&rot);
    }
    Ok(w.finish(Kind::Motion, key))
}

#[derive(Clone)]
pub struct MotionData {
    c: Container,
}

impl MotionData {
    pub fn parse(blob: Arc<Blob>) -> Result<Self> {
        let c = Container::parse(blob, Kind::Motion)?;
        ensure!(c.section_count() == 5, "motion sections");
        let m = Self { c };
        ensure!(
            m.nodes().len() == m.header().node_count as usize,
            "motion header"
        );
        Ok(m)
    }
    pub fn header(&self) -> MotionHeader {
        self.c.one(0).unwrap_or_else(Zeroable::zeroed)
    }
    pub fn nodes(&self) -> &[MotionNodeRec] {
        self.c.slice(1)
    }
    pub fn position_keys(&self, n: &MotionNodeRec) -> &[Vec3Key] {
        let r = n.pos;
        self.c
            .slice::<Vec3Key>(2)
            .get(r[0] as usize..(r[0] + r[1]) as usize)
            .unwrap_or(&[])
    }
    pub fn scale_keys(&self, n: &MotionNodeRec) -> &[Vec3Key] {
        let r = n.scl;
        self.c
            .slice::<Vec3Key>(3)
            .get(r[0] as usize..(r[0] + r[1]) as usize)
            .unwrap_or(&[])
    }
    /// Rotation keys as (time, quaternion), decoding the quantized form if needed.
    pub fn rotation_keys(&self, n: &MotionNodeRec) -> Vec<(f32, [f32; 4])> {
        let (a, b) = (n.rot[0] as usize, (n.rot[0] + n.rot[1]) as usize);
        if self.header().quantized == 1 {
            self.c
                .slice::<QuatKeyQ>(4)
                .get(a..b)
                .unwrap_or(&[])
                .iter()
                .map(|k| (k.t, k.quat()))
                .collect()
        } else {
            self.c
                .slice::<QuatKey>(4)
                .get(a..b)
                .unwrap_or(&[])
                .iter()
                .map(|k| (k.t, k.q))
                .collect()
        }
    }
}
