//! `.xnvmsh` collision mesh reader (bounded, partial).
//!
//! Finding: the file is a GENOMFLE-wrapped list of **PhysX 2.x cooked triangle meshes**
//! (`NXS\x01MESH` streams). Each stream observed in the installed Gothic 3 1.75 landscape cells is:
//!
//! ```text
//! "NXS\x01MESH"  u32 0  u32 cook_version(9|11|19)  f32 0.001  u32 0xff  u32 0
//! u32 vertex_count  u32 triangle_count
//! vertex_count * 3 * f32      (METRES: x100 gives the render-mesh centimetre space)
//! triangle_count * 3 * index  (u8 when vertex_count <= 256, else u16)
//! ...opaque: material indices, adjacency, "OPC" opcode tree, "HBM" hull data
//! ```
//!
//! Only vertices and triangle indices are decoded; the remaining cooked data is ignored.
//! Streams are located by signature scan and each is validated (counts, bounds, finite
//! values, indices in range); a stream that fails validation is skipped, never guessed.
//! Evidence: Myrtana cell 131 holds 7 streams whose vertex bounds (x100) fall inside the
//! matching `.xcmsh` cell bounds. Synthetic tests only; no original data is committed.
use anyhow::{Result, ensure};
use serde::Serialize;

const SIGNATURE: &[u8; 8] = b"NXS\x01MESH";
const MAX_VERTICES: usize = 1_000_000;
const MAX_TRIANGLES: usize = 2_000_000;
const MAX_INPUT: usize = 64 * 1024 * 1024;
/// Source units per metre: positions are stored in metres, the world uses centimetres.
pub const UNITS_PER_METRE: f32 = 100.0;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CollisionMesh {
    /// Positions in metres, exactly as stored.
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub cook_version: u32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Xnvmsh {
    pub meshes: Vec<CollisionMesh>,
    /// Signatures that failed validation and were skipped.
    pub rejected: usize,
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(o..o.checked_add(4)?)?.try_into().ok()?,
    ))
}

impl Xnvmsh {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= MAX_INPUT, "xnvmsh exceeds 64 MiB");
        ensure!(bytes.starts_with(b"GENOMFLE"), "not a GENOMFLE xnvmsh");
        let mut out = Xnvmsh::default();
        let mut i = 0;
        while i + SIGNATURE.len() <= bytes.len() {
            if &bytes[i..i + 8] != SIGNATURE {
                i += 1;
                continue;
            }
            match parse_stream(bytes, i + 8) {
                Some((mesh, end)) => {
                    out.meshes.push(mesh);
                    i = end;
                }
                None => {
                    out.rejected += 1;
                    i += 8;
                }
            }
        }
        ensure!(
            !out.meshes.is_empty(),
            "no valid cooked triangle mesh in xnvmsh"
        );
        Ok(out)
    }

    /// All triangles of all streams in centimetres (x100), source axes and index order.
    pub fn triangles_cm(&self) -> Vec<[[f32; 3]; 3]> {
        let mut out = Vec::new();
        for m in &self.meshes {
            let p = |i: u32| m.positions[i as usize].map(|v| v * UNITS_PER_METRE);
            for t in m.indices.as_chunks::<3>().0 {
                out.push([p(t[0]), p(t[1]), p(t[2])]);
            }
        }
        out
    }
}

fn parse_stream(b: &[u8], q: usize) -> Option<(CollisionMesh, usize)> {
    let version = u32_at(b, q + 4)?;
    let nv = u32_at(b, q + 20)? as usize;
    let nt = u32_at(b, q + 24)? as usize;
    if nv == 0 || nt == 0 || nv > MAX_VERTICES || nt > MAX_TRIANGLES {
        return None;
    }
    let vs = q + 28;
    let ve = vs.checked_add(nv.checked_mul(12)?)?;
    let width = if nv <= 256 { 1 } else { 2 };
    let ie = ve.checked_add(nt.checked_mul(3 * width)?)?;
    let raw = b.get(vs..ie)?;
    let mut positions = Vec::with_capacity(nv);
    for c in raw[..nv * 12].as_chunks::<12>().0.iter() {
        let p = [0, 4, 8].map(|o| f32::from_le_bytes(c[o..o + 4].try_into().unwrap()));
        if !p.iter().all(|v| v.is_finite() && v.abs() < 1.0e6) {
            return None;
        }
        positions.push(p);
    }
    let idx = &raw[nv * 12..];
    let indices: Vec<u32> = if width == 1 {
        idx.iter().map(|&v| u32::from(v)).collect()
    } else {
        idx.as_chunks::<2>()
            .0
            .iter()
            .map(|c| u32::from(u16::from_le_bytes([c[0], c[1]])))
            .collect()
    };
    if indices.iter().any(|&i| i as usize >= nv) {
        return None;
    }
    Some((
        CollisionMesh {
            positions,
            indices,
            cook_version: version,
        },
        ie,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(nv: u32, verts: &[[f32; 3]], idx: &[u8]) -> Vec<u8> {
        let mut b = b"NXS\x01MESH".to_vec();
        for v in [0u32, 9] {
            b.extend(v.to_le_bytes());
        }
        b.extend(0.001f32.to_le_bytes());
        for v in [0xffu32, 0, nv, (idx.len() / 3) as u32] {
            b.extend(v.to_le_bytes());
        }
        for v in verts.iter().flatten() {
            b.extend(v.to_le_bytes());
        }
        b.extend(idx);
        b
    }
    fn file(parts: &[Vec<u8>]) -> Vec<u8> {
        let mut b = b"GENOMFLE\x01\x00junk".to_vec();
        for p in parts {
            b.extend(p);
            b.extend([1, 0, 1, 0]);
        }
        b
    }

    #[test]
    fn decodes_streams_and_scales_to_centimetres() {
        let s = stream(
            3,
            &[[1.0, 2.0, 3.0], [2.0, 2.0, 3.0], [1.0, 2.0, 4.0]],
            &[0, 1, 2],
        );
        let x = Xnvmsh::parse(&file(&[s.clone(), s])).unwrap();
        assert_eq!(x.meshes.len(), 2);
        let t = x.triangles_cm();
        assert_eq!(t[0][1], [200.0, 200.0, 300.0]);
    }
    #[test]
    fn rejects_bad_index_truncation_and_nonfinite() {
        let v = [[0.0; 3], [1.0; 3], [2.0; 3]];
        assert!(Xnvmsh::parse(&file(&[stream(3, &v, &[0, 1, 3])])).is_err());
        let mut s = stream(3, &v, &[0, 1, 2]);
        s.truncate(s.len() - 1);
        let mut raw = b"GENOMFLE".to_vec();
        raw.extend(s);
        assert!(Xnvmsh::parse(&raw).is_err());
        let nan = [[f32::NAN, 0.0, 0.0], [1.0; 3], [2.0; 3]];
        assert!(Xnvmsh::parse(&file(&[stream(3, &nan, &[0, 1, 2])])).is_err());
        assert!(Xnvmsh::parse(b"nope").is_err());
    }
    #[test]
    fn skips_rejected_stream_but_keeps_valid_one() {
        let v = [[0.0; 3], [1.0; 3], [2.0; 3]];
        let bad = stream(3, &v, &[0, 1, 9]);
        let good = stream(3, &v, &[0, 1, 2]);
        let x = Xnvmsh::parse(&file(&[bad, good])).unwrap();
        assert_eq!((x.meshes.len(), x.rejected), (1, 1));
    }
}
