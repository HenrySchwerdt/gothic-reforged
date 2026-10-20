//! Vegetation entities of the world documents: SpeedTree placements and painted
//! "vegetation brush" areas.
//!
//! * `eCSpeedTree_PS` entities carry only a `ResourceFilePath` (`*.spt`); the entity world
//!   matrix places (and scales) the tree.
//! * `eCVegetation_PS` entities carry a list of `eCVegetation_Mesh` objects. Each holds a brush
//!   mesh (`MeshFilePath`, an authoring-machine absolute path whose basename is the real
//!   resource), a `MinSpacing` (cm) and, in the class tail, the **painted ground surface**: a
//!   bounding box (metres, relative to the entity) and an indexed triangle mesh (positions,
//!   normals, per-vertex pairs, `u32` indices). The engine scatters brush instances over this
//!   surface; the per-instance transforms are *not* stored (scattering is runtime).
//!
//! Tail layout of `eCVegetation_Mesh` (verified: all tails of the installation are consumed
//! exactly): `u16 version (2)`, 14 opaque bytes, `6 x f32` AABB, then four arrays each written
//! as `u8 1, u32 n, n elements`: positions (3 x f32), normals (3 x f32), a 2 x f32 pair and
//! `u32` indices (`n % 3 == 0`).
use crate::{
    genome::GenomeFile,
    propstream::{Cursor, Strings},
    world_node::WorldDocument,
};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;

const ACCESSOR: [u8; 6] = [1, 0, 1, 1, 0, 1];
const MAX_MESH_VERTICES: usize = 200_000;

#[derive(Debug, Clone, Serialize)]
pub struct TreeEntity {
    pub entity: String,
    /// `.spt` resource name as serialized (basename).
    pub spt: String,
    /// Serialized world matrix (row-vector layout, scale included).
    pub matrix: [[f32; 4]; 4],
}

#[derive(Debug, Clone, Serialize)]
pub struct BrushMesh {
    /// Brush mesh resource basename (e.g. `G3_Brush_Plant_Grass_05.xcmsh`).
    pub mesh: String,
    /// Minimum instance spacing in cm.
    pub min_spacing: f32,
    pub wind_strength: f32,
    pub double_sided: bool,
    /// Raw `eEVegetationMeshShading` value.
    pub shading: u32,
    /// Painted surface, metres relative to the entity.
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// The 14 tail bytes between the version word and the bounds (meaning unknown).
    pub header: [u8; 14],
    /// The per-vertex 2 x f32 array (meaning unknown).
    pub pairs: Vec<[f32; 2]>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VegetationArea {
    pub entity: String,
    pub matrix: [[f32; 4]; 4],
    /// cm.
    pub view_range: f32,
    pub fade_out_start: f32,
    pub grid_node_size: f32,
    pub meshes: Vec<BrushMesh>,
}

#[derive(Debug, Default, Serialize)]
pub struct WorldVegetation {
    pub trees: Vec<TreeEntity>,
    pub areas: Vec<VegetationArea>,
    /// Brush meshes whose tail did not match the verified layout.
    pub rejected_meshes: usize,
}

struct ParsedObject<'a> {
    props: Vec<(String, &'a [u8])>,
    tail_start: usize,
    end: usize,
}

fn read_object<'a>(bytes: &'a [u8], strings: &Strings, start: usize, limit: usize) -> Result<ParsedObject<'a>> {
    let mut cur = Cursor::with_range(bytes, start, limit)?;
    let version = cur.u16()?;
    ensure!(version == 0x53 || version == 0x52, "unsupported object version");
    let size = cur.u32()? as usize;
    ensure!(size <= cur.remaining(), "object exceeds container");
    let end = cur.pos + size;
    let mut cur = Cursor::with_range(bytes, cur.pos, end)?;
    ensure!(cur.u16()? == 30, "unsupported property list version");
    let count = cur.u32()? as usize;
    ensure!(count <= 4096 && count <= cur.remaining() / 10, "property count");
    let mut props = Vec::with_capacity(count);
    for _ in 0..count {
        let name = strings.entry(&mut cur)?;
        let _ty = cur.u16()?;
        ensure!(cur.u16()? == 30, "unsupported property version");
        let len = cur.u32()? as usize;
        props.push((name, cur.take(len)?));
    }
    Ok(ParsedObject {
        props,
        tail_start: cur.pos,
        end,
    })
}

fn prop<'a>(o: &ParsedObject<'a>, name: &str) -> Option<&'a [u8]> {
    o.props.iter().find(|(n, _)| n == name).map(|(_, p)| *p)
}
fn prop_f32(o: &ParsedObject, name: &str) -> Option<f32> {
    let p = prop(o, name)?;
    (p.len() == 4).then(|| f32::from_le_bytes([p[0], p[1], p[2], p[3]]))
}
fn prop_bool(o: &ParsedObject, name: &str) -> Option<bool> {
    match prop(o, name)? {
        [v] => Some(*v != 0),
        _ => None,
    }
}
fn prop_str(o: &ParsedObject, strings: &Strings, name: &str) -> Option<String> {
    let p = prop(o, name)?;
    (p.len() == 2)
        .then(|| strings.get(u16::from_le_bytes([p[0], p[1]])).ok())
        .flatten()
}

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn read_array<'a>(cur: &mut Cursor<'a>, stride: usize) -> Result<(usize, &'a [u8])> {
    ensure!(cur.u8()? == 1, "array marker");
    let n = cur.u32()? as usize;
    ensure!(n <= MAX_MESH_VERTICES * 3, "array too large");
    let bytes = cur.take(n.checked_mul(stride).context("array size")?)?;
    Ok((n, bytes))
}

fn f32s<const N: usize>(bytes: &[u8]) -> Vec<[f32; N]> {
    bytes
        .chunks_exact(4 * N)
        .map(|c| {
            let mut out = [0f32; N];
            for (v, b) in out.iter_mut().zip(c.chunks_exact(4)) {
                *v = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            }
            out
        })
        .collect()
}

fn brush_mesh(bytes: &[u8], strings: &Strings, o: &ParsedObject) -> Result<BrushMesh> {
    let path = prop_str(o, strings, "MeshFilePath").context("MeshFilePath")?;
    let mut cur = Cursor::with_range(bytes, o.tail_start, o.end)?;
    ensure!(cur.u16()? == 2, "unsupported brush mesh version");
    let header: [u8; 14] = cur.take(14)?.try_into()?;
    let b: [f32; 6] = cur.floats()?;
    ensure!(b.iter().all(|v| v.is_finite()), "bounds");
    let (n, pos) = read_array(&mut cur, 12)?;
    ensure!(n > 0 && n <= MAX_MESH_VERTICES, "vertex count");
    let (nn, nrm) = read_array(&mut cur, 12)?;
    ensure!(nn == n, "normal count");
    let (pn, pairs) = read_array(&mut cur, 8)?;
    ensure!(pn == n, "pair count");
    let (ni, idx) = read_array(&mut cur, 4)?;
    ensure!(ni % 3 == 0, "index count");
    ensure!(cur.remaining() == 0, "unexpected brush mesh tail bytes");
    let indices: Vec<u32> = idx
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    ensure!(
        indices.iter().all(|&i| (i as usize) < n),
        "index out of range"
    );
    let positions = f32s::<3>(pos);
    ensure!(
        positions.iter().flatten().all(|v| v.is_finite()),
        "non-finite positions"
    );
    Ok(BrushMesh {
        mesh: basename(&path).to_string(),
        min_spacing: prop_f32(o, "MinSpacing").unwrap_or(100.0),
        wind_strength: prop_f32(o, "WindStrength").unwrap_or(0.0),
        double_sided: prop_bool(o, "DoubleSided").unwrap_or(false),
        shading: prop(o, "MeshShading")
            .filter(|p| p.len() == 6)
            .map_or(0, |p| u32::from_le_bytes([p[2], p[3], p[4], p[5]])),
        bounds_min: [b[0], b[1], b[2]],
        bounds_max: [b[3], b[4], b[5]],
        positions,
        normals: f32s::<3>(nrm),
        indices,
        header,
        pairs: f32s::<2>(pairs),
    })
}

/// Extract SpeedTree placements and vegetation areas from a world document.
pub fn extract(bytes: &[u8]) -> Result<WorldVegetation> {
    let (doc, spans) = WorldDocument::parse_with_spans(bytes)?;
    let mut out = WorldVegetation::default();
    if !spans
        .iter()
        .flatten()
        .any(|s| s.class == "eCSpeedTree_PS" || s.class == "eCVegetation_PS")
    {
        return Ok(out);
    }
    let genome = GenomeFile::parse(bytes)?;
    let strings = Strings::new(&genome.strings);
    let mesh_class = genome
        .strings
        .iter()
        .position(|s| *s == b"eCVegetation_Mesh")
        .map(|i| i as u16);
    for (entity, sets) in doc.entities.iter().zip(&spans) {
        for span in sets {
            match span.class.as_str() {
                "eCSpeedTree_PS" => {
                    let Ok(o) = read_object(bytes, &strings, span.start, span.end) else {
                        continue;
                    };
                    if let Some(spt) = prop_str(&o, &strings, "ResourceFilePath") {
                        out.trees.push(TreeEntity {
                            entity: entity.name.clone(),
                            spt: basename(&spt).to_string(),
                            matrix: entity.world_matrix,
                        });
                    }
                }
                "eCVegetation_PS" => {
                    let Ok(o) = read_object(bytes, &strings, span.start, span.end) else {
                        continue;
                    };
                    let mut area = VegetationArea {
                        entity: entity.name.clone(),
                        matrix: entity.world_matrix,
                        view_range: prop_f32(&o, "ViewRange").unwrap_or(5000.0),
                        fade_out_start: prop_f32(&o, "FadeOutStart").unwrap_or(2500.0),
                        grid_node_size: prop_f32(&o, "GridNodeSize").unwrap_or(1000.0),
                        meshes: Vec::new(),
                    };
                    let Some(class_idx) = mesh_class else {
                        bail!("vegetation without mesh class string");
                    };
                    let tail = &bytes[o.tail_start..o.end];
                    let mut at = 0;
                    while at + 13 < tail.len() {
                        if tail[at..at + 6] == ACCESSOR
                            && u16::from_le_bytes([tail[at + 6], tail[at + 7]]) == class_idx
                        {
                            let start = o.tail_start + at + 13;
                            match read_object(bytes, &strings, start, o.end)
                                .and_then(|m| brush_mesh(bytes, &strings, &m).map(|b| (m.end, b)))
                            {
                                Ok((end, mesh)) => {
                                    area.meshes.push(mesh);
                                    at = end - o.tail_start;
                                    continue;
                                }
                                Err(_) => out.rejected_meshes += 1,
                            }
                        }
                        at += 1;
                    }
                    out.areas.push(area);
                }
                _ => {}
            }
        }
    }
    Ok(out)
}
