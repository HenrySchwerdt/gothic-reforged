//! Resolved material table (`materials.gmat`): every material reduced to texture ids, blend
//! and alpha-test state, terrain layer recipe and normal maps, sorted by id for binary search.
use crate::container::{Container, Kind, Writer};
use crate::ids::AssetId;
use crate::mmap::Blob;
use anyhow::{Result, ensure};
use bytemuck::{Pod, Zeroable};
use gothic_formats::xshmat::{TextureRole, Xshmat, graph::MaterialGraph};
use std::sync::Arc;

pub const NONE_U32: u32 = u32::MAX;
pub const FLAG_VERTEX_BLEND: u32 = 1;
pub const FLAG_SPECULAR: u32 = 2;
pub const FLAG_NO_LIGHTING: u32 = 4;
pub const KIND_SURFACE: u32 = 0;
pub const KIND_WATER: u32 = 1;

/// One colour layer: texture ids (0 = none), UV scales, tint.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default, Debug)]
pub struct LayerRec {
    pub tex: [u64; 2],
    pub scale: [[f32; 2]; 2],
    pub tint: [f32; 3],
    pub _pad: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default, Debug)]
pub struct MaterialRec {
    pub id: u64,
    pub kind: u32,
    /// Raw `BlendMode` (see XSHMAT notes) or [`NONE_U32`].
    pub blend_mode: u32,
    /// Alpha-test reference (0..255) or [`NONE_U32`].
    pub mask_reference: u32,
    pub flags: u32,
    pub layer_count: u32,
    pub _pad: u32,
    /// Colour layers; layer 1 is blended by vertex alpha when `FLAG_VERTEX_BLEND`.
    pub layers: [LayerRec; 2],
    /// Normal-map texture ids per layer (0 = none) and UV scales.
    pub normal_tex: [u64; 2],
    pub normal_scale: [[f32; 2]; 2],
}

/// A resolved material plus the texture names it uses (`stem`, `srgb`).
pub struct Resolved {
    pub rec: MaterialRec,
    pub textures: Vec<(String, bool)>,
}

pub fn tex_id(path: &str) -> u64 {
    let stem = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = stem.rsplit_once('.').map_or(stem, |(s, _)| s);
    AssetId::of(&format!("{stem}.ximg")).0
}

fn stem_name(path: &str) -> String {
    let stem = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = stem.rsplit_once('.').map_or(stem, |(s, _)| s);
    format!("{}.ximg", stem.to_ascii_lowercase())
}

/// Same resolution rules the walkable prototype uses for its `MatData`.
pub fn resolve(name: &str, bytes: &[u8]) -> Option<Resolved> {
    let x = Xshmat::parse(bytes).ok()?;
    let recipe = MaterialGraph::parse(bytes).ok().and_then(|g| g.surface());
    let mut rec = MaterialRec {
        id: AssetId::of(name).0,
        blend_mode: x.blend_mode.unwrap_or(NONE_U32),
        mask_reference: x.mask_reference.map_or(NONE_U32, u32::from),
        flags: if x.enable_specular == Some(true) {
            FLAG_SPECULAR
        } else {
            0
        } | if x.disable_lighting == Some(true) {
            FLAG_NO_LIGHTING
        } else {
            0
        },
        ..Default::default()
    };
    let mut textures: Vec<(String, bool)> = Vec::new();
    let mut add = |path: &str, srgb: bool| {
        textures.push((stem_name(path), srgb));
        tex_id(path)
    };
    let mut vertex_blend = false;
    if let Some(r) = &recipe {
        vertex_blend = r.vertex_blend;
        for l in r.layers.iter().take(2) {
            let mut layer = LayerRec {
                tint: l.tint,
                ..Default::default()
            };
            for (i, t) in l.textures.iter().take(2).enumerate() {
                layer.tex[i] = add(&t.path, true);
                layer.scale[i] = t.scale;
            }
            rec.layers[rec.layer_count as usize] = layer;
            rec.layer_count += 1;
        }
        for (i, n) in r.normals.iter().take(2).enumerate() {
            if let Some(t) = n {
                rec.normal_tex[i] = add(&t.path, false);
                rec.normal_scale[i] = t.scale;
            }
        }
    } else if let Some(d) = x.texture(TextureRole::Diffuse) {
        rec.layers[0] = LayerRec {
            tex: [add(&d.path, true), 0],
            scale: [[1.0; 2]; 2],
            tint: [1.0; 3],
            _pad: 0.0,
        };
        rec.layer_count = 1;
        if let Some(n) = x.texture(TextureRole::Normal) {
            rec.normal_tex[0] = add(&n.path, false);
            rec.normal_scale[0] = [1.0, 1.0];
        }
    }
    let water = name.to_ascii_lowercase().contains("water") && rec.layer_count == 0;
    if water {
        rec.kind = KIND_WATER;
        let mut found: Vec<&str> = x
            .textures
            .iter()
            .filter(|t| t.role == TextureRole::Normal)
            .map(|t| t.path.as_str())
            .collect();
        found.dedup();
        for (i, p) in found.iter().take(2).enumerate() {
            rec.normal_tex[i] = add(p, false);
            rec.normal_scale[i] = [1.0, 1.0];
        }
    }
    if rec.layer_count == 0 && !water {
        return None;
    }
    if rec.layer_count >= 2 && vertex_blend {
        rec.flags |= FLAG_VERTEX_BLEND;
    }
    Some(Resolved { rec, textures })
}

pub fn write_table(mut recs: Vec<MaterialRec>, key: u64) -> Vec<u8> {
    recs.sort_by_key(|r| r.id);
    recs.dedup_by_key(|r| r.id);
    let mut w = Writer::new();
    w.slice(&recs);
    w.finish(Kind::Materials, key)
}

/// Memory-mapped material table.
#[derive(Clone)]
pub struct MaterialTable {
    c: Container,
}

impl MaterialTable {
    pub fn parse(blob: Arc<Blob>) -> Result<Self> {
        let c = Container::parse(blob, Kind::Materials)?;
        ensure!(c.section_count() == 1, "material table sections");
        Ok(Self { c })
    }
    pub fn records(&self) -> &[MaterialRec] {
        self.c.slice(0)
    }
    pub fn get(&self, id: u64) -> Option<&MaterialRec> {
        let r = self.records();
        r.binary_search_by_key(&id, |m| m.id).ok().map(|i| &r[i])
    }
}
