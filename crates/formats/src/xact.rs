//! Independent reader for Gothic 3 `.xact` actor resources: GENOMFLE envelope around a
//! chunked "FXA " container holding the skeleton, materials, skinned meshes and skin weights.
//! Only observed chunk layouts are decoded; see `docs/research/ACTOR_ANIMATION_FORMAT.md`.
use crate::{
    emfx::{Reader, read_container},
    genome::GenomeFile,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

const MAX_NODES: usize = 8192;
const MAX_VERTICES: usize = 1_000_000;
const MAX_INDICES: usize = 3_000_000;
const MAX_SUBMESHES: usize = 1024;
const MAX_INFLUENCES: usize = 255;

const CHUNK_NODE: u32 = 0;
const CHUNK_MESH: u32 = 3;
const CHUNK_SKIN: u32 = 4;
const CHUNK_MATERIAL: u32 = 6;
const CHUNK_LAYER: u32 = 7;

#[derive(Debug, Clone, Serialize)]
pub struct ActorNode {
    pub name: String,
    pub parent_name: String,
    /// Index into `Actor::nodes`; `None` for roots.
    pub parent: Option<usize>,
    pub position: [f32; 3],
    /// Quaternion x, y, z, w.
    pub rotation: [f32; 4],
    pub scale_rotation: [f32; 4],
    pub scale: [f32; 3],
}

#[derive(Debug, Clone, Serialize)]
pub struct MaterialLayer {
    /// Observed values: 1 diffuse, 2/9 specular, 3 normal/bump (inferred from file names).
    pub map_type: u8,
    pub amount: f32,
    pub uv_offset: [f32; 2],
    pub uv_tiling: [f32; 2],
    pub rotation: f32,
    /// Texture base name without extension, e.g. `G3_Human_Hero_Body_Diffuse_S1`.
    pub texture: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActorMaterial {
    pub name: String,
    pub ambient: [f32; 3],
    pub diffuse: [f32; 3],
    pub specular: [f32; 3],
    pub emissive: [f32; 3],
    pub shine: f32,
    pub opacity: f32,
    pub double_sided: bool,
    /// ASCII transparency type byte, `F` (filter) in every observed material.
    pub transparency: u8,
    pub layers: Vec<MaterialLayer>,
}

impl ActorMaterial {
    pub fn diffuse_texture(&self) -> Option<&str> {
        self.layers
            .iter()
            .find(|l| l.map_type == 1)
            .map(|l| l.texture.as_str())
    }
}

#[derive(Debug, Serialize)]
pub struct ActorSubmesh {
    /// Index into `Actor::materials` (0 is the exporter's "EMFX Default").
    pub material: u8,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Second UV set when the mesh carries two (lightmap-like, semantics unverified).
    pub uvs2: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// Per vertex: the original (pre-split) vertex number that indexes the skin table.
    pub original_vertex: Vec<u32>,
    /// Per vertex: up to four strongest bone influences (node indices) and normalised weights.
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
}

#[derive(Debug, Serialize)]
pub struct ActorMesh {
    /// Node the mesh is attached to.
    pub node: u32,
    pub original_vertex_count: u32,
    pub skinned: bool,
    /// Largest influence count observed on any original vertex.
    pub max_influences: usize,
    pub submeshes: Vec<ActorSubmesh>,
}

#[derive(Debug, Serialize)]
pub struct Actor {
    pub nodes: Vec<ActorNode>,
    pub materials: Vec<ActorMaterial>,
    pub meshes: Vec<ActorMesh>,
    /// `.xshmat` names from the Genome string table, in file order.
    pub material_files: Vec<String>,
    pub chunk_counts: BTreeMap<u32, usize>,
    /// Bytes of the declared container not consumed as whole chunks (4 is the norm).
    pub leftover: usize,
}

impl Actor {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let genome = GenomeFile::parse(bytes).context("invalid XACT GENOMFLE envelope")?;
        let container = read_container(genome.body, b"FXA ", 2, true)?;
        let mut chunk_counts = BTreeMap::new();
        let mut nodes = Vec::new();
        let mut materials: Vec<ActorMaterial> = Vec::new();
        let mut raw_meshes = Vec::new();
        let mut skins: HashMap<u32, SkinTable> = HashMap::new();
        for chunk in &container.chunks {
            *chunk_counts.entry(chunk.id).or_insert(0) += 1;
            match chunk.id {
                CHUNK_NODE => {
                    ensure!(chunk.version == 3, "unsupported node chunk version");
                    ensure!(nodes.len() < MAX_NODES, "too many nodes");
                    nodes.push(parse_node(chunk.data)?);
                }
                CHUNK_MATERIAL => {
                    ensure!(chunk.version == 5, "unsupported material chunk version");
                    ensure!(materials.len() < 4096, "too many materials");
                    materials.push(parse_material(chunk.data)?);
                }
                CHUNK_LAYER => {
                    ensure!(chunk.version == 4, "unsupported layer chunk version");
                    let (number, layer) = parse_layer(chunk.data)?;
                    let material = materials
                        .get_mut(number as usize)
                        .context("layer references unknown material")?;
                    material.layers.push(layer);
                }
                CHUNK_MESH => {
                    ensure!(chunk.version == 3, "unsupported mesh chunk version");
                    raw_meshes.push(parse_mesh(chunk.data, materials.len())?);
                }
                CHUNK_SKIN => {
                    ensure!(chunk.version == 1, "unsupported skin chunk version");
                    let (node, table) = parse_skin(chunk.data)?;
                    ensure!(
                        skins.insert(node, table).is_none(),
                        "duplicate skin chunk for node"
                    );
                }
                // Chunk 16 (exporter info) and chunks 8/9 (88/72 byte per-node records, absent
                // from most actors) are counted but not decoded.
                _ => {}
            }
        }
        ensure!(!nodes.is_empty(), "actor has no nodes");
        resolve_parents(&mut nodes)?;
        let mut meshes = Vec::new();
        for raw in raw_meshes {
            ensure!(
                (raw.node as usize) < nodes.len(),
                "mesh node outside skeleton"
            );
            let skin = skins.remove(&raw.node);
            meshes.push(raw.finish(skin, nodes.len())?);
        }
        let material_files = genome
            .strings
            .iter()
            .map(|s| s.iter().map(|b| char::from(*b)).collect::<String>())
            .filter(|s| s.to_ascii_lowercase().ends_with(".xshmat"))
            .collect();
        Ok(Self {
            nodes,
            materials,
            meshes,
            material_files,
            chunk_counts,
            leftover: container.leftover,
        })
    }

    pub fn node_index(&self, name: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.name == name)
    }

    /// `.xshmat` file name for material index `i` (>= 1); the string table lists the
    /// real materials in order, skipping the default material.
    pub fn material_file(&self, material: usize) -> Option<&str> {
        material
            .checked_sub(1)
            .and_then(|i| self.material_files.get(i))
            .map(String::as_str)
    }
}

fn parse_node(data: &[u8]) -> Result<ActorNode> {
    let mut r = Reader::new(data);
    let position = r.vec3()?;
    let rotation = r.quat()?;
    let scale_rotation = r.quat()?;
    let scale = r.vec3()?;
    // Three floats that are zero in every observed node.
    for _ in 0..3 {
        r.f32_lenient()?;
    }
    let name = r.string()?;
    let parent_name = r.string()?;
    ensure!(r.len() == 0, "trailing node bytes");
    ensure!(!name.is_empty(), "unnamed node");
    Ok(ActorNode {
        name,
        parent_name,
        parent: None,
        position,
        rotation,
        scale_rotation,
        scale,
    })
}

fn resolve_parents(nodes: &mut [ActorNode]) -> Result<()> {
    let mut index = HashMap::new();
    for (i, n) in nodes.iter().enumerate() {
        // First occurrence wins for duplicate names.
        index.entry(n.name.clone()).or_insert(i);
    }
    for (i, node) in nodes.iter_mut().enumerate() {
        if node.parent_name.is_empty() {
            continue;
        }
        let parent = *index
            .get(&node.parent_name)
            .with_context(|| format!("unknown parent node {:?}", node.parent_name))?;
        ensure!(parent != i, "node is its own parent");
        node.parent = Some(parent);
    }
    // Reject cycles.
    for i in 0..nodes.len() {
        let mut at = i;
        for _ in 0..=nodes.len() {
            match nodes[at].parent {
                Some(p) => at = p,
                None => break,
            }
        }
        ensure!(nodes[at].parent.is_none(), "cyclic node hierarchy");
    }
    Ok(())
}

fn parse_material(data: &[u8]) -> Result<ActorMaterial> {
    let mut r = Reader::new(data);
    let ambient = r.vec3()?;
    let diffuse = r.vec3()?;
    let specular = r.vec3()?;
    let emissive = r.vec3()?;
    let shine = r.f32()?;
    r.f32()?; // observed 0
    let opacity = r.f32()?;
    r.f32()?; // observed 1.5 (index of refraction)
    let flags = r.take(4)?;
    let name = r.string()?;
    r.u32()?; // observed 0
    ensure!(r.len() == 0, "trailing material bytes");
    Ok(ActorMaterial {
        name,
        ambient,
        diffuse,
        specular,
        emissive,
        shine,
        opacity,
        double_sided: flags[0] != 0,
        transparency: flags[2],
        layers: Vec::new(),
    })
}

fn parse_layer(data: &[u8]) -> Result<(u16, MaterialLayer)> {
    let mut r = Reader::new(data);
    let map_type = r.u8()?;
    r.u8()?; // exporter garbage
    let material = r.u16()?;
    let amount = r.f32()?;
    let uv_offset = [r.f32()?, r.f32()?];
    let uv_tiling = [r.f32()?, r.f32()?];
    let rotation = r.f32()?;
    let texture = r.string()?;
    ensure!(r.len() == 0, "trailing layer bytes");
    Ok((
        material,
        MaterialLayer {
            map_type,
            amount,
            uv_offset,
            uv_tiling,
            rotation,
            texture,
        },
    ))
}

struct RawSubmesh {
    material: u8,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    uvs2: Vec<[f32; 2]>,
    indices: Vec<u32>,
    original_vertex: Vec<u32>,
}

struct RawMesh {
    node: u32,
    original_vertex_count: u32,
    submeshes: Vec<RawSubmesh>,
}

fn parse_mesh(data: &[u8], material_count: usize) -> Result<RawMesh> {
    let mut r = Reader::new(data);
    let node = r.u32()?;
    let original_vertex_count = r.u32()?;
    let vertex_total = r.u32()? as usize;
    let index_total = r.u32()? as usize;
    let submesh_count = r.u32()? as usize;
    let uv_sets = r.u32()? as usize; // observed 1 or 2
    ensure!((1..=2).contains(&uv_sets), "unsupported UV set count");
    r.u32()?; // opaque tag word (00 c9 + exporter garbage)
    ensure!(
        vertex_total <= MAX_VERTICES && index_total <= MAX_INDICES,
        "mesh exceeds ceiling"
    );
    ensure!(
        (1..=MAX_SUBMESHES).contains(&submesh_count),
        "invalid submesh count"
    );
    let mut submeshes = Vec::with_capacity(submesh_count);
    let (mut seen_vertices, mut seen_indices) = (0usize, 0usize);
    for _ in 0..submesh_count {
        let tag = r.take(4)?;
        let material = tag[0];
        ensure!(
            (material as usize) < material_count && tag[1] as usize == uv_sets,
            "invalid submesh tag"
        );
        let index_count = r.u32()? as usize;
        let vertex_count = r.u32()? as usize;
        seen_vertices = seen_vertices
            .checked_add(vertex_count)
            .context("vertex count overflow")?;
        seen_indices = seen_indices
            .checked_add(index_count)
            .context("index count overflow")?;
        ensure!(
            seen_vertices <= vertex_total && seen_indices <= index_total,
            "submesh counts exceed mesh totals"
        );
        ensure!(
            index_count.is_multiple_of(3) && vertex_count > 0,
            "invalid submesh counts"
        );
        let mut sub = RawSubmesh {
            material,
            positions: Vec::with_capacity(vertex_count),
            normals: Vec::with_capacity(vertex_count),
            uvs: Vec::with_capacity(vertex_count),
            uvs2: Vec::new(),
            indices: Vec::with_capacity(index_count),
            original_vertex: Vec::with_capacity(vertex_count),
        };
        for _ in 0..vertex_count {
            let original = r.u32()?;
            ensure!(
                original < original_vertex_count,
                "original vertex outside table"
            );
            sub.original_vertex.push(original);
            sub.positions.push(r.vec3()?);
            sub.normals.push(r.vec3()?);
            sub.uvs.push([r.f32_lenient()?, r.f32_lenient()?]);
            if uv_sets == 2 {
                sub.uvs2.push([r.f32_lenient()?, r.f32_lenient()?]);
            }
        }
        for _ in 0..index_count {
            let i = r.u32()?;
            ensure!((i as usize) < vertex_count, "index outside submesh");
            sub.indices.push(i);
        }
        submeshes.push(sub);
    }
    ensure!(
        seen_vertices == vertex_total && seen_indices == index_total,
        "submesh counts differ from mesh totals"
    );
    ensure!(r.len() == 0, "trailing mesh bytes");
    Ok(RawMesh {
        node,
        original_vertex_count,
        submeshes,
    })
}

type SkinTable = Vec<Vec<(u16, f32)>>;

fn parse_skin(data: &[u8]) -> Result<(u32, SkinTable)> {
    let mut r = Reader::new(data);
    let node = r.u32()?;
    let mut table = Vec::new();
    while r.len() > 0 {
        let count = r.u8()? as usize;
        ensure!(count <= MAX_INFLUENCES, "influence count");
        ensure!(table.len() < MAX_VERTICES, "skin table too large");
        let mut list = Vec::with_capacity(count);
        for _ in 0..count {
            let bone = r.u16()?;
            r.u16()?; // exporter garbage
            let weight = r.f32()?;
            // Negative weights occur (e.g. 1.029 / -0.129); they are legitimate data.
            ensure!(weight.abs() < 16.0, "implausible skin weight");
            list.push((bone, weight));
        }
        table.push(list);
    }
    Ok((node, table))
}

impl RawMesh {
    fn finish(self, skin: Option<SkinTable>, node_count: usize) -> Result<ActorMesh> {
        let max_influences = skin
            .as_ref()
            .map_or(1, |t| t.iter().map(Vec::len).max().unwrap_or(0));
        if let Some(table) = &skin {
            ensure!(
                table.len() == self.original_vertex_count as usize,
                "skin table size differs from original vertex count"
            );
            for list in table {
                for (bone, _) in list {
                    ensure!((*bone as usize) < node_count, "skin bone outside skeleton");
                }
            }
        }
        let skinned = skin.is_some();
        let mut submeshes = Vec::with_capacity(self.submeshes.len());
        for sub in self.submeshes {
            let mut joints = Vec::with_capacity(sub.positions.len());
            let mut weights = Vec::with_capacity(sub.positions.len());
            for &original in &sub.original_vertex {
                let (j, w) = match &skin {
                    Some(table) => top_four(&table[original as usize]),
                    // Rigid mesh: bound entirely to its node.
                    None => ([self.node as u16, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
                };
                joints.push(j);
                weights.push(w);
            }
            submeshes.push(ActorSubmesh {
                material: sub.material,
                positions: sub.positions,
                normals: sub.normals,
                uvs: sub.uvs,
                uvs2: sub.uvs2,
                indices: sub.indices,
                original_vertex: sub.original_vertex,
                joints,
                weights,
            });
        }
        Ok(ActorMesh {
            node: self.node,
            original_vertex_count: self.original_vertex_count,
            skinned,
            max_influences,
            submeshes,
        })
    }
}

/// Strongest four influences, renormalised. A vertex without usable weight binds to bone 0.
fn top_four(list: &[(u16, f32)]) -> ([u16; 4], [f32; 4]) {
    let mut sorted: Vec<(u16, f32)> = list.iter().copied().filter(|(_, w)| *w > 0.0).collect();
    sorted.sort_by(|a, b| b.1.total_cmp(&a.1));
    sorted.truncate(4);
    let sum: f32 = sorted.iter().map(|(_, w)| w).sum();
    let mut joints = [0u16; 4];
    let mut weights = [0f32; 4];
    if sum <= 1e-6 {
        weights[0] = 1.0;
        return (joints, weights);
    }
    for (i, (bone, weight)) in sorted.into_iter().enumerate() {
        joints[i] = bone;
        weights[i] = weight / sum;
    }
    (joints, weights)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32b(b: &mut Vec<u8>, n: u32) {
        b.extend(n.to_le_bytes());
    }
    fn f32s(b: &mut Vec<u8>, v: &[f32]) {
        for x in v {
            b.extend(x.to_le_bytes());
        }
    }
    fn string(b: &mut Vec<u8>, s: &str) {
        u32b(b, s.len() as u32);
        b.extend(s.as_bytes());
    }
    fn chunk(out: &mut Vec<u8>, id: u32, version: u32, data: &[u8]) {
        u32b(out, id);
        u32b(out, data.len() as u32);
        u32b(out, version);
        out.extend(data);
    }
    fn node(name: &str, parent: &str, pos: [f32; 3]) -> Vec<u8> {
        let mut d = Vec::new();
        f32s(&mut d, &pos);
        f32s(&mut d, &[0., 0., 0., 1.]);
        f32s(&mut d, &[0., 0., 0., 1.]);
        f32s(&mut d, &[1., 1., 1.]);
        f32s(&mut d, &[0., 0., 0.]);
        string(&mut d, name);
        string(&mut d, parent);
        d
    }
    fn material(name: &str) -> Vec<u8> {
        let mut d = Vec::new();
        f32s(&mut d, &[0.; 3]);
        f32s(&mut d, &[0.75; 3]);
        f32s(&mut d, &[1.; 3]);
        f32s(&mut d, &[0.; 3]);
        f32s(&mut d, &[25., 0., 1., 1.5]);
        d.extend([0, 0, 0x46, 0]);
        string(&mut d, name);
        u32b(&mut d, 0);
        d
    }
    fn layer(material: u16, texture: &str) -> Vec<u8> {
        let mut d = vec![1, 0xf7];
        d.extend(material.to_le_bytes());
        f32s(&mut d, &[1., 0., 0., 1., 1., 0.]);
        string(&mut d, texture);
        d
    }
    fn mesh() -> Vec<u8> {
        let mut d = Vec::new();
        for n in [1u32, 3, 3, 3, 1, 1, 0xc9_00] {
            u32b(&mut d, n);
        }
        d.extend([1, 1, 0, 0]);
        u32b(&mut d, 3);
        u32b(&mut d, 3);
        for (i, p) in [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]]
            .iter()
            .enumerate()
        {
            u32b(&mut d, i as u32);
            f32s(&mut d, p);
            f32s(&mut d, &[0., 0., 1.]);
            f32s(&mut d, &[i as f32, 0.]);
        }
        for i in 0..3 {
            u32b(&mut d, i);
        }
        d
    }
    fn skin() -> Vec<u8> {
        let mut d = Vec::new();
        u32b(&mut d, 1);
        for bones in [&[(0u16, 1.0f32)][..], &[(0, 0.25), (1, 0.75)], &[(1, 1.0)]] {
            d.push(bones.len() as u8);
            for (b, w) in bones {
                d.extend(b.to_le_bytes());
                d.extend([0xd3, 0x13]);
                d.extend(w.to_le_bytes());
            }
        }
        d
    }
    fn actor_bytes(with_skin: bool, extra: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
        actor_bytes_info(with_skin, 0, extra)
    }
    /// `info_shortfall` makes the info chunk declare that many fewer bytes than it holds.
    fn actor_bytes_info(
        with_skin: bool,
        info_shortfall: usize,
        extra: impl FnOnce(&mut Vec<u8>),
    ) -> Vec<u8> {
        let mut chunks = Vec::new();
        let mut info = vec![0u8; 8];
        for text in ["3D Studio Max", "W:/some/path.max", "Mar  1 2005"] {
            string(&mut info, text);
        }
        u32b(&mut chunks, 16);
        u32b(&mut chunks, (info.len() - info_shortfall) as u32);
        u32b(&mut chunks, 1);
        chunks.extend(&info);
        chunk(&mut chunks, 0, 3, &node("Root", "", [0., 0., 0.]));
        chunk(&mut chunks, 0, 3, &node("Mesh", "Root", [0., 2., 0.]));
        chunk(&mut chunks, 6, 5, &material("EMFX Default"));
        chunk(&mut chunks, 6, 5, &material("Skin"));
        chunk(&mut chunks, 7, 4, &layer(1, "Tex_Diffuse"));
        chunk(&mut chunks, 3, 3, &mesh());
        if with_skin {
            chunk(&mut chunks, 4, 1, &skin());
        }
        extra(&mut chunks);
        let mut body = vec![0x36, 0, 0, 0, 0, 0];
        body.extend(b"gena");
        body.extend(4u16.to_le_bytes());
        let container = 2 + chunks.len() + 4;
        u32b(&mut body, container as u32);
        body.extend(b"FXA ");
        body.extend([1, 1]);
        body.extend(chunks);
        body.extend([0; 4]);
        let mut out = b"GENOMFLE".to_vec();
        out.extend(1u16.to_le_bytes());
        out.extend(((14 + body.len()) as u32).to_le_bytes());
        out.extend(&body);
        out.extend([0xef, 0xbe, 0xad, 0xde, 1]);
        u32b(&mut out, 1);
        let name = b"Skin.xshmat";
        out.extend((name.len() as u16).to_le_bytes());
        out.extend(name);
        out
    }

    #[test]
    fn parses_synthetic_actor() -> Result<()> {
        let a = Actor::parse(&actor_bytes(true, |_| {}))?;
        assert_eq!(a.nodes.len(), 2);
        assert_eq!(a.nodes[1].parent, Some(0));
        assert_eq!(a.nodes[1].position, [0., 2., 0.]);
        assert_eq!(a.materials.len(), 2);
        assert_eq!(a.materials[1].diffuse_texture(), Some("Tex_Diffuse"));
        assert_eq!(a.material_file(1), Some("Skin.xshmat"));
        assert_eq!(a.leftover, 4);
        let m = &a.meshes[0];
        assert!(m.skinned);
        assert_eq!(m.max_influences, 2);
        let s = &m.submeshes[0];
        assert_eq!(s.material, 1);
        assert_eq!(s.indices, vec![0, 1, 2]);
        assert_eq!(s.joints[1][..2], [1, 0]);
        assert!((s.weights[1][0] - 0.75).abs() < 1e-6);
        Ok(())
    }

    #[test]
    fn recovers_from_stale_info_chunk_size() -> Result<()> {
        let a = Actor::parse(&actor_bytes_info(true, 12, |_| {}))?;
        assert_eq!(a.nodes.len(), 2);
        assert_eq!(a.leftover, 4);
        Ok(())
    }

    #[test]
    fn rigid_mesh_binds_to_its_node() -> Result<()> {
        let a = Actor::parse(&actor_bytes(false, |_| {}))?;
        assert!(!a.meshes[0].skinned);
        assert_eq!(a.meshes[0].submeshes[0].joints[0][0], 1);
        Ok(())
    }

    #[test]
    fn rejects_cycles_unknown_parents_and_bad_refs() {
        let cyc = actor_bytes(true, |c| chunk(c, 0, 3, &node("A", "B", [0.; 3])));
        assert!(Actor::parse(&cyc).is_err());
        let unknown = actor_bytes(true, |c| chunk(c, 0, 3, &node("A", "Nope", [0.; 3])));
        assert!(Actor::parse(&unknown).is_err());
        let layer_bad = actor_bytes(true, |c| chunk(c, 7, 4, &layer(9, "x")));
        assert!(Actor::parse(&layer_bad).is_err());
        let nan_pos = actor_bytes(true, |c| {
            chunk(c, 0, 3, &node("A", "Root", [f32::NAN, 0., 0.]))
        });
        assert!(Actor::parse(&nan_pos).is_err());
    }

    #[test]
    fn truncation_never_panics() {
        let bytes = actor_bytes(true, |_| {});
        for n in 0..bytes.len() {
            let _ = Actor::parse(&bytes[..n]);
        }
    }

    #[test]
    fn normalises_top_four_weights() {
        let (j, w) = top_four(&[(1, 0.1), (2, 0.4), (3, 0.3), (4, 0.2), (5, 0.1)]);
        assert_eq!(j[0], 2);
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        assert_eq!(top_four(&[]).0, [0; 4]);
    }
}
