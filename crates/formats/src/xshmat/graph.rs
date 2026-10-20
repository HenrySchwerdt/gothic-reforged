//! Shader-graph decoder for `eCResourceShaderMaterial_PS` (XSHMAT).
//!
//! The `eCShaderDefault` root object carries every node of the colour graph as a flat list of
//! nested objects (`eCColorSrcSampler`, `...Combiner`, `...Blend`, `...VertexColor`,
//! `...Constant`, `eCTexCoordSrcScale`, `...BumpOffset`). Every node ends with its own 16-byte
//! GUID (`tail[len-36..len-20]`, followed by four editor-position words); inputs are stored as
//! the GUIDs of the nodes they read, in input order. The root references its output slots the
//! same way, in the order diffuse, specular colour, specular constant, normal.
//!
//! Evidence and limits: `docs/research/XSHMAT_FORMAT.md` (graph section). Only the node kinds
//! listed above were observed; unknown classes decode as [`Node::Other`]. Semantics such as
//! `CombinerType == 2` meaning "modulate x2" are inferred from data, not from the engine.
use super::Scanner;
use crate::genome::GenomeFile;
use anyhow::{Result, ensure};
use serde::Serialize;

const MAX_NODES: usize = 256;
const ACCESSOR: [u8; 6] = [1, 0, 1, 1, 0, 1];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Node {
    Sampler {
        path: String,
        /// Raw `TexRepeatU/V` enum values.
        repeat: [u32; 2],
        /// Texture-coordinate transform inputs (node indices).
        inputs: Vec<usize>,
    },
    Combiner {
        kind: u32,
        inputs: Vec<usize>,
    },
    Blend {
        inputs: Vec<usize>,
    },
    VertexColor,
    Constant {
        color: [f32; 3],
        alpha: f32,
    },
    Scale {
        scale: [f32; 2],
    },
    BumpOffset {
        inputs: Vec<usize>,
    },
    Other(String),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MaterialGraph {
    pub nodes: Vec<Node>,
    /// Output slots referenced by the root in order (diffuse, specular colour, specular
    /// constant, normal) with unresolved slots omitted.
    pub slots: Vec<usize>,
}

/// A sampled texture together with the uv scale applied to it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TexUse {
    pub path: String,
    pub scale: [f32; 2],
}

/// One blendable colour layer: `textures[0] * textures[1] * 2^(n-1) * tint`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Layer {
    pub textures: Vec<TexUse>,
    pub tint: [f32; 3],
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SurfaceRecipe {
    /// One layer, or two when `vertex_blend` (layer 1 weight is the vertex alpha).
    pub layers: Vec<Layer>,
    pub vertex_blend: bool,
    /// Normal-map sampler per layer (same blending), if any.
    pub normals: Vec<Option<TexUse>>,
}

impl MaterialGraph {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let genome = if bytes.starts_with(b"GENOMFLE") {
            Some(GenomeFile::parse(bytes)?)
        } else {
            None
        };
        let (body, table) = match &genome {
            Some(g) => (g.body, Some(g.strings.as_slice())),
            None => (bytes, None),
        };
        let scanner = Scanner {
            body,
            table,
            out: super::Xshmat::default(),
            seen_class: true,
        };
        let mut objects = Vec::new();
        let mut at = 0usize;
        while at + 8 < body.len() {
            if body[at..at + 6] == ACCESSOR
                && let Some((name, after)) = scanner.string_at(at + 6)
                && name.starts_with(b"eC")
                && let Some(object) = scanner.read_object(after + 5)
            {
                objects.push((String::from_utf8_lossy(name).into_owned(), object));
                at = after;
                continue;
            }
            at += 1;
        }
        ensure!(!objects.is_empty(), "no objects in XSHMAT");
        let root = objects
            .iter()
            .position(|(n, _)| n == "eCShaderDefault")
            .ok_or_else(|| anyhow::anyhow!("no eCShaderDefault root"))?;
        let nodes_objects: Vec<_> = objects
            .iter()
            .enumerate()
            .filter(|(i, (n, _))| {
                *i != root && (n.starts_with("eCColorSrc") || n.starts_with("eCTexCoordSrc"))
            })
            .map(|(_, o)| o)
            .collect();
        ensure!(nodes_objects.len() <= MAX_NODES, "too many shader nodes");
        let guids: Vec<Option<[u8; 16]>> = nodes_objects
            .iter()
            .map(|(_, o)| {
                let t = o.tail;
                (t.len() >= 36)
                    .then(|| t[t.len() - 36..t.len() - 20].try_into().ok())
                    .flatten()
            })
            .collect();
        let find = |window: &[u8]| -> Option<usize> {
            guids
                .iter()
                .position(|g| g.is_some_and(|g| g.as_slice() == window))
        };
        let links = |tail: &[u8], skip: Option<usize>| -> Vec<usize> {
            let mut out = Vec::new();
            for off in 0..tail.len().saturating_sub(15) {
                if let Some(n) = find(&tail[off..off + 16])
                    && Some(n) != skip
                {
                    out.push(n);
                }
            }
            out
        };
        let mut nodes = Vec::new();
        for (i, (name, o)) in nodes_objects.iter().enumerate() {
            let inputs = links(o.tail, Some(i));
            let prop = |n: &str| o.props.iter().find(|(k, _)| k == n).map(|(_, v)| *v);
            let enum_value = |n: &str| prop(n).and_then(|p| p.get(2..6)).map_or(0, u32_le);
            let node = match name.as_str() {
                "eCColorSrcSampler" => {
                    let path = prop("ImageFilePath")
                        .and_then(|p| scanner.string_at_slice(p))
                        .unwrap_or_default();
                    Node::Sampler {
                        path,
                        repeat: [enum_value("TexRepeatU"), enum_value("TexRepeatV")],
                        inputs,
                    }
                }
                "eCColorSrcCombiner" => Node::Combiner {
                    kind: enum_value("CombinerType"),
                    inputs,
                },
                "eCColorSrcBlend" => Node::Blend { inputs },
                "eCColorSrcVertexColor" => Node::VertexColor,
                "eCColorSrcConstant" => {
                    let c = prop("Color").filter(|p| p.len() >= 16);
                    let color = c.map_or([1.0; 3], |p| {
                        [f32_le(&p[4..8]), f32_le(&p[8..12]), f32_le(&p[12..16])]
                    });
                    let alpha = prop("Alpha").filter(|p| p.len() == 4).map_or(1.0, f32_le);
                    Node::Constant { color, alpha }
                }
                "eCTexCoordSrcScale" => {
                    let s = prop("Scale").filter(|p| p.len() == 8);
                    Node::Scale {
                        scale: s.map_or([1.0; 2], |p| [f32_le(&p[0..4]), f32_le(&p[4..8])]),
                    }
                }
                "eCTexCoordSrcBumpOffset" => Node::BumpOffset { inputs },
                other => Node::Other(other.to_owned()),
            };
            nodes.push(node);
        }
        // Root slots: GUIDs in the root tail before the first child object.
        let root_object = &objects[root].1;
        let first_child = objects.iter().skip(root + 1).map(|(_, o)| o.start).next();
        let prefix_len = first_child
            .map_or(root_object.tail.len(), |s| {
                s.saturating_sub(root_object.tail_start)
            })
            .min(root_object.tail.len());
        let mut slots = Vec::new();
        for n in links(&root_object.tail[..prefix_len], None) {
            if !slots.contains(&n) {
                slots.push(n);
            }
        }
        Ok(Self { nodes, slots })
    }

    fn inputs(&self, n: usize) -> &[usize] {
        match self.nodes.get(n) {
            Some(
                Node::Combiner { inputs, .. }
                | Node::Blend { inputs }
                | Node::Sampler { inputs, .. }
                | Node::BumpOffset { inputs },
            ) => inputs,
            _ => &[],
        }
    }

    /// All sampler textures under `n`, depth first.
    pub fn samplers(&self, n: usize) -> Vec<TexUse> {
        let mut out = Vec::new();
        self.collect_samplers(n, &mut out, 0);
        out
    }

    fn collect_samplers(&self, n: usize, out: &mut Vec<TexUse>, depth: u8) {
        if depth > 12 {
            return;
        }
        if let Some(Node::Sampler { path, inputs, .. }) = self.nodes.get(n) {
            let scale = self.uv_scale(inputs, 0).unwrap_or([1.0, 1.0]);
            out.push(TexUse {
                path: path.clone(),
                scale,
            });
            return;
        }
        for i in self.inputs(n) {
            self.collect_samplers(*i, out, depth + 1);
        }
    }

    /// Texture-coordinate scale reachable through `inputs` (directly or via a bump offset).
    fn uv_scale(&self, inputs: &[usize], depth: u8) -> Option<[f32; 2]> {
        if depth > 4 {
            return None;
        }
        inputs.iter().find_map(|i| match self.nodes.get(*i) {
            Some(Node::Scale { scale }) => Some(*scale),
            Some(Node::BumpOffset { inputs }) => self.uv_scale(inputs, depth + 1),
            _ => None,
        })
    }

    fn is_normal_expr(&self, n: usize) -> bool {
        let s = self.samplers(n);
        !s.is_empty()
            && s.iter()
                .all(|t| t.path.to_ascii_lowercase().contains("normal"))
    }

    /// Flatten an expression (samplers, constants, x2 combiners) into one layer.
    fn layer(&self, n: usize, depth: u8, out: &mut Layer) {
        if depth > 12 {
            return;
        }
        match self.nodes.get(n) {
            Some(Node::Sampler { .. }) => out.textures.extend(self.samplers(n)),
            Some(Node::Combiner { inputs, .. }) => {
                for i in inputs {
                    match self.nodes.get(*i) {
                        Some(Node::Constant { color, .. }) => {
                            for (t, c) in out.tint.iter_mut().zip(color) {
                                *t *= c * 2.0;
                            }
                        }
                        _ => self.layer(*i, depth + 1, out),
                    }
                }
            }
            _ => {}
        }
    }

    /// Diffuse/normal surface description for a vertex-blended or plain textured material.
    pub fn surface(&self) -> Option<SurfaceRecipe> {
        let diffuse = self.slots.iter().copied().find(|s| {
            !self.is_normal_expr(*s)
                && self.samplers(*s).iter().all(|t| {
                    let p = t.path.to_ascii_lowercase();
                    !p.contains("normal") && !p.contains("specular") && !p.contains("cubemap")
                })
                && !self.samplers(*s).is_empty()
        })?;
        let normal = self.slots.iter().copied().find(|s| self.is_normal_expr(*s));
        let mut recipe = SurfaceRecipe::default();
        let make_layer = |n: usize| {
            let mut l = Layer {
                textures: Vec::new(),
                tint: [1.0; 3],
            };
            self.layer(n, 0, &mut l);
            l
        };
        match self.nodes.get(diffuse) {
            Some(Node::Blend { inputs }) if inputs.len() >= 2 => {
                recipe.layers = vec![make_layer(inputs[0]), make_layer(inputs[1])];
                recipe.vertex_blend = inputs
                    .iter()
                    .any(|i| matches!(self.nodes.get(*i), Some(Node::VertexColor)));
            }
            _ => recipe.layers = vec![make_layer(diffuse)],
        }
        recipe.normals = match normal.and_then(|n| self.nodes.get(n).map(|node| (n, node))) {
            Some((_, Node::Blend { inputs })) if inputs.len() >= 2 => inputs[..2]
                .iter()
                .map(|i| self.samplers(*i).into_iter().next())
                .collect(),
            Some((n, _)) => vec![self.samplers(n).into_iter().next()],
            None => Vec::new(),
        };
        recipe.layers.retain(|l| !l.textures.is_empty());
        if recipe.layers.is_empty() {
            return None;
        }
        if recipe.layers.len() < 2 {
            recipe.vertex_blend = false;
        }
        Some(recipe)
    }
}

fn u32_le(b: &[u8]) -> u32 {
    b.get(..4)
        .map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap_or_default()))
}
fn f32_le(b: &[u8]) -> f32 {
    f32::from_bits(u32_le(b))
}

/// One parsed `0x53` object: framed properties plus the class tail.
pub(super) struct Object<'a> {
    pub props: Vec<(String, &'a [u8])>,
    pub tail: &'a [u8],
    /// Body offset of the object's first byte (its version word) and of the tail.
    pub start: usize,
    pub tail_start: usize,
}

impl<'a> Scanner<'a> {
    pub(super) fn string_at_slice(&self, payload: &[u8]) -> Option<String> {
        let sub = Scanner {
            body: payload,
            table: self.table,
            out: super::Xshmat::default(),
            seen_class: true,
        };
        let (text, end) = sub.string_at(0)?;
        (end == payload.len()).then(|| text.iter().map(|&b| char::from(b)).collect())
    }

    /// Read an object starting at its version word (`0x53`), after the accessor/name/filler.
    pub(super) fn read_object(&self, at: usize) -> Option<Object<'a>> {
        let version = self.u16_at(at)?;
        if !(version == 1 || (0x51..=0x53).contains(&version)) {
            return None;
        }
        let size = self.u32_at(at + 2)? as usize;
        let end = (at + 6).checked_add(size)?;
        if end > self.body.len() {
            return None;
        }
        let mut q = at + 6;
        if version == 1 {
            q = self.string_at(q)?.1; // legacy object name
        }
        if version <= 0x51 {
            q += 20; // legacy object id
        }
        if self.u16_at(q)? != 30 {
            return None;
        }
        let count = self.u32_at(q + 2)? as usize;
        if count > 256 {
            return None;
        }
        let mut p = q + 6;
        let mut props = Vec::new();
        for _ in 0..count {
            let (name, a) = self.string_at(p)?;
            let (_ty, b) = self.string_at(a)?;
            if self.u16_at(b)? != 30 {
                return None;
            }
            let len = self.u32_at(b + 2)? as usize;
            let payload = self.body.get(b + 6..(b + 6).checked_add(len)?)?;
            props.push((String::from_utf8_lossy(name).into_owned(), payload));
            p = b + 6 + len;
        }
        if p > end {
            return None;
        }
        Some(Object {
            props,
            tail: &self.body[p..end],
            start: at,
            tail_start: p,
        })
    }
}
