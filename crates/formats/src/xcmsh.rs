//! Independent reader for validated GENOMFLE class83 and raw class82 XCMSH meshes.
//! Positions, normals, UVs and triangle winding retain their source representation.
use crate::genome::GenomeFile;
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;

const MAX_VERTICES: usize = 1_000_000;
const MAX_INDICES: usize = 3_000_000;
const MAX_SUBMESHES: usize = 4096;
const MAX_DECODED_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct Xcmsh {
    pub submeshes: Vec<Submesh>,
}

#[derive(Debug, Serialize)]
pub struct Submesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub material: String,
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
}

impl Xcmsh {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= crate::MAX_PAYLOAD as usize,
            "XCMSH exceeds resource ceiling"
        );
        let file = if bytes.starts_with(b"GENOMFLE") {
            Some(GenomeFile::parse(bytes).context("invalid XCMSH GENOMFLE envelope")?)
        } else {
            None
        };
        let mut c = Cursor(file.as_ref().map_or(bytes, |f| f.body));
        ensure!(
            c.u16()? == 1 && c.u8()? == 1 && c.u16()? == 1 && c.u8()? == 1,
            "unsupported XCMSH accessor header"
        );
        ensure!(
            string(&mut c, file.as_ref())? == b"eCResourceMeshComplex_PS",
            "unexpected XCMSH resource class"
        );
        ensure!(
            c.u8()? == 1 && c.u16()? == 0,
            "unsupported XCMSH class header"
        );
        let class_version = c.u16()?;
        ensure!(
            (1..=83).contains(&class_version),
            "unsupported XCMSH class version"
        );
        c.take(6)?;
        // Historical layouts (observed: class version 1) carry extra fields.
        if class_version < 81 {
            string(&mut c, file.as_ref())?;
        }
        if class_version < 82 {
            c.take(20)?;
        }
        ensure!(c.u16()? == 30, "unsupported XCMSH property header version");
        let properties = c.count(4096, 10)?;
        for _ in 0..properties {
            string(&mut c, file.as_ref())?;
            string(&mut c, file.as_ref())?;
            ensure!(c.u16()? == 30, "unsupported XCMSH property version");
            let n = c.u32()? as usize;
            c.take(n)?;
        }
        let mesh_version = c.u16()?;
        let resource_version = c.u16()?;
        ensure!(
            mesh_version >= 34 && resource_version >= 22,
            "unsupported XCMSH mesh/resource version"
        );
        if resource_version > 22 {
            c.u32()?;
        }
        if resource_version < 30 && c.u16()? > 1 {
            c.u8()?;
        }
        c.float().context("XCMSH header float")?;
        let count = c.count(MAX_SUBMESHES, 40)?;
        ensure!(count > 0, "XCMSH has no submeshes");
        let mut submeshes = Vec::with_capacity(count);
        let mut decoded_bytes = 0usize;
        for _ in 0..count {
            let version = c.u16()?;
            ensure!(
                (1..=5).contains(&version),
                "unsupported XCMSH submesh version"
            );
            c.u32()?;
            let bounds_min = c.vector3().context("XCMSH submesh bounds")?;
            let bounds_max = c.vector3().context("XCMSH submesh bounds")?;
            ensure!(
                (0..3).all(|a| bounds_min[a] <= bounds_max[a]),
                "inverted XCMSH bounds"
            );
            c.u32()?;
            let material = string(&mut c, file.as_ref())?
                .iter()
                .map(|b| char::from(*b))
                .collect();
            let streams = c.count(32, 11)?;
            let (mut positions, mut normals, mut uvs, mut indices) = (None, None, None, None);
            let mut tags = std::collections::HashSet::new();
            for _ in 0..streams {
                let tag = c.u32()?;
                ensure!(tags.insert(tag), "duplicate XCMSH stream tag {tag}");
                ensure!(
                    c.u16()? == 1 && c.u8()? == 1,
                    "unsupported XCMSH stream header"
                );
                let stride = match tag {
                    0 | 4 | 5 | 6 => 4,
                    1 | 3 | 64 | 72 => 12,
                    12 | 15 | 18 | 21 | 73 => 8,
                    2 => 16,
                    _ => bail!("unsupported XCMSH stream tag {tag}"),
                };
                let limit = if tag == 0 { MAX_INDICES } else { MAX_VERTICES };
                let n = c.count(limit, stride)?;
                let size = n
                    .checked_mul(stride)
                    .context("XCMSH stream size overflow")?;
                decoded_bytes = decoded_bytes
                    .checked_add(size)
                    .context("XCMSH decoded size overflow")?;
                ensure!(
                    decoded_bytes <= MAX_DECODED_BYTES,
                    "XCMSH decoded streams exceed ceiling"
                );
                let mut payload = Cursor(c.take(size)?);
                match tag {
                    0 => {
                        ensure!(n > 0 && n % 3 == 0, "invalid XCMSH triangle index count");
                        let mut values = Vec::with_capacity(n);
                        for _ in 0..n {
                            values.push(payload.u32()?);
                        }
                        indices = Some(values);
                    }
                    1 | 3 => {
                        let mut values = Vec::with_capacity(n);
                        for _ in 0..n {
                            values.push(
                                payload
                                    .vector3()
                                    .with_context(|| format!("XCMSH stream tag {tag}"))?,
                            );
                        }
                        if tag == 1 {
                            positions = Some(values);
                        } else {
                            normals = Some(values);
                        }
                    }
                    12 => {
                        let mut values = Vec::with_capacity(n);
                        for _ in 0..n {
                            // Observed in real data (river meshes): NaN/inf texture
                            // coordinates. They are sanitised to 0 instead of
                            // rejecting the whole mesh; positions stay strict.
                            let mut uv = [0f32; 2];
                            for value in &mut uv {
                                let raw = f32::from_bits(payload.u32()?);
                                *value = if raw.is_finite() { raw } else { 0.0 };
                            }
                            values.push(uv);
                        }
                        uvs = Some(values);
                    }
                    // Tangent / auxiliary coordinate streams are length-checked
                    // by `take` above; their float values are not interpreted
                    // (tag 2 is the transformed-position layout, 16 bytes; tag 73 two floats).
                    2 | 15 | 18 | 21 | 64 | 72 | 73 => {}
                    4..=6 => { /* Opaque packed colors/point size; mapping unresolved. */ }
                    _ => bail!("unsupported XCMSH stream tag {tag}"),
                }
            }
            // Validated version-5 auxiliary arrays; semantics are deliberately opaque.
            if version > 2 {
                c.array(4, true)?;
                c.array(4, true)?;
            }
            if version > 1 {
                ensure!(c.u8()? == 1, "unsupported nested XCMSH array marker");
                let nested = c.count(MAX_VERTICES, 94)?;
                for _ in 0..nested {
                    c.array(4, true)?;
                    c.array(4, true)?;
                    c.take(84)?;
                }
            }
            if version > 3 {
                c.array(24, false)?;
                c.array(4, true)?;
            }
            let positions = positions.context("XCMSH positions stream missing")?;
            let indices = indices.context("XCMSH indices stream missing")?;
            ensure!(!positions.is_empty(), "XCMSH positions stream empty");
            ensure!(
                indices.iter().all(|&i| (i as usize) < positions.len()),
                "XCMSH index outside vertex stream"
            );
            let normals = normals.unwrap_or_default();
            let uvs = uvs.unwrap_or_default();
            ensure!(
                normals.is_empty() || normals.len() == positions.len(),
                "XCMSH normal count mismatch"
            );
            ensure!(
                uvs.is_empty() || uvs.len() == positions.len(),
                "XCMSH UV count mismatch"
            );
            submeshes.push(Submesh {
                positions,
                normals,
                uvs,
                indices,
                material,
                bounds_min,
                bounds_max,
            });
        }
        ensure!(c.0.is_empty(), "unexpected trailing XCMSH body bytes");
        Ok(Self { submeshes })
    }
}

fn string<'a>(cursor: &mut Cursor<'a>, file: Option<&GenomeFile<'a>>) -> Result<&'a [u8]> {
    let value = cursor.u16()? as usize;
    if let Some(file) = file {
        file.strings
            .get(value)
            .copied()
            .context("XCMSH string index outside table")
    } else {
        ensure!(value <= 16_384, "XCMSH inline string exceeds ceiling");
        cursor.take(value)
    }
}

struct Cursor<'a>(&'a [u8]);
impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        ensure!(n <= self.0.len(), "truncated XCMSH field");
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn float(&mut self) -> Result<f32> {
        let value = f32::from_bits(self.u32()?);
        ensure!(value.is_finite(), "non-finite XCMSH float");
        Ok(value)
    }
    fn vector3(&mut self) -> Result<[f32; 3]> {
        Ok([self.float()?, self.float()?, self.float()?])
    }
    fn count(&mut self, max: usize, stride: usize) -> Result<usize> {
        let count = self.u32()? as usize;
        ensure!(
            count <= max && count <= self.0.len() / stride,
            "XCMSH count exceeds bounds"
        );
        Ok(count)
    }
    fn array(&mut self, stride: usize, marker: bool) -> Result<()> {
        if marker {
            ensure!(self.u8()? == 1, "unsupported XCMSH array marker");
        }
        let n = self.count(MAX_INDICES, stride)?;
        self.take(n.checked_mul(stride).context("XCMSH array overflow")?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        bytes: Vec<u8>,
        position: usize,
        index: usize,
        count: usize,
        tag: usize,
    }
    fn u16(b: &mut Vec<u8>, n: u16) {
        b.extend(n.to_le_bytes());
    }
    fn u32(b: &mut Vec<u8>, n: u32) {
        b.extend(n.to_le_bytes());
    }
    fn floats(b: &mut Vec<u8>, values: &[f32]) {
        for value in values {
            b.extend(value.to_le_bytes());
        }
    }
    fn fixture() -> Fixture {
        fixture_kind(false)
    }
    fn fixture_kind(raw: bool) -> Fixture {
        let prefix = if raw { 0 } else { 14 };
        let mut b = Vec::new();
        u16(&mut b, 1);
        b.push(1);
        u16(&mut b, 1);
        b.push(1);
        if raw {
            u16(&mut b, 24);
            b.extend(b"eCResourceMeshComplex_PS");
        } else {
            u16(&mut b, 0);
        }
        b.push(1);
        u16(&mut b, 0);
        u16(&mut b, if raw { 82 } else { 83 });
        b.extend([0; 6]);
        u16(&mut b, 30);
        u32(&mut b, 0);
        u16(&mut b, 35);
        u16(&mut b, 30);
        u32(&mut b, 0);
        floats(&mut b, &[0.]);
        u32(&mut b, 1);
        u16(&mut b, 5);
        u32(&mut b, 0);
        floats(&mut b, &[0., 0., 0., 1., 1., 0.]);
        u32(&mut b, 0);
        if raw {
            u16(&mut b, 16);
            b.extend(b"synthetic.xshmat");
        } else {
            u16(&mut b, 1);
        }
        u32(&mut b, 3);
        let tag = b.len() + prefix;
        u32(&mut b, 1);
        u16(&mut b, 1);
        b.push(1);
        let count = b.len() + prefix;
        u32(&mut b, 3);
        let position = b.len() + prefix;
        floats(&mut b, &[0., 0., 0., 1., 0., 0., 0., 1., 0.]);
        u32(&mut b, 3);
        u16(&mut b, 1);
        b.push(1);
        u32(&mut b, 3);
        floats(&mut b, &[0., 0., -1., 0., 0., -1., 0., 0., -1.]);
        u32(&mut b, 0);
        u16(&mut b, 1);
        b.push(1);
        u32(&mut b, 3);
        let index = b.len() + prefix;
        u32(&mut b, 0);
        u32(&mut b, 1);
        u32(&mut b, 2);
        for _ in 0..3 {
            b.push(1);
            u32(&mut b, 0);
        }
        u32(&mut b, 0);
        b.push(1);
        u32(&mut b, 0);
        if raw {
            return Fixture {
                bytes: b,
                position,
                index,
                count,
                tag,
            };
        }
        let mut bytes = b"GENOMFLE".to_vec();
        u16(&mut bytes, 1);
        u32(&mut bytes, (14 + b.len()) as u32);
        bytes.extend(b);
        u32(&mut bytes, 0xdeadbeef);
        bytes.push(1);
        u32(&mut bytes, 2);
        for s in ["eCResourceMeshComplex_PS", "synthetic.xshmat"] {
            u16(&mut bytes, s.len() as u16);
            bytes.extend(s.bytes());
        }
        Fixture {
            bytes,
            position,
            index,
            count,
            tag,
        }
    }

    #[test]
    fn preserves_source_vertices_normals_and_winding() -> Result<()> {
        let f = fixture();
        let mesh = Xcmsh::parse(&f.bytes)?;
        assert_eq!(mesh.submeshes.len(), 1);
        let s = &mesh.submeshes[0];
        assert_eq!(s.material, "synthetic.xshmat");
        assert_eq!(s.positions, [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]]);
        assert_eq!(s.indices, [0, 1, 2]);
        assert_eq!(s.normals, [[0., 0., -1.]; 3]);
        assert!(s.uvs.is_empty());
        Ok(())
    }

    #[test]
    fn raw_class82_inline_strings_and_boundaries() -> Result<()> {
        let f = fixture_kind(true);
        let m = Xcmsh::parse(&f.bytes)?;
        assert_eq!(m.submeshes[0].material, "synthetic.xshmat");
        assert_eq!(m.submeshes[0].indices, [0, 1, 2]);
        for end in 0..f.bytes.len() {
            assert!(Xcmsh::parse(&f.bytes[..end]).is_err());
        }
        let mut trailing = f.bytes.clone();
        trailing.push(0);
        assert!(Xcmsh::parse(&trailing).is_err());
        let mut long = f.bytes.clone();
        long[6..8].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(Xcmsh::parse(&long).is_err());
        Ok(())
    }

    #[test]
    fn raw_submeshes_keep_local_indices() -> Result<()> {
        let f = fixture_kind(true);
        // This synthetic fixture's fixed prefix ends after the count at byte65.
        let mut doubled = f.bytes.clone();
        doubled[61..65].copy_from_slice(&2u32.to_le_bytes());
        doubled.extend_from_slice(&f.bytes[65..]);
        let m = Xcmsh::parse(&doubled)?;
        assert_eq!(m.submeshes.len(), 2);
        for s in &m.submeshes {
            assert_eq!(s.indices, [0, 1, 2]);
        }
        let mut old = f.bytes.clone();
        old[35..37].copy_from_slice(&1u16.to_le_bytes());
        assert!(Xcmsh::parse(&old).is_err());
        Ok(())
    }

    /// Raw (no GENOMFLE) mesh with configurable historical layout.
    fn raw_mesh(
        class_version: u16,
        submesh_version: u16,
        extra: &[(u32, usize)],
        uv: Option<[f32; 2]>,
    ) -> Vec<u8> {
        let mut b = Vec::new();
        u16(&mut b, 1);
        b.push(1);
        u16(&mut b, 1);
        b.push(1);
        u16(&mut b, 24);
        b.extend(b"eCResourceMeshComplex_PS");
        b.push(1);
        u16(&mut b, 0);
        u16(&mut b, class_version);
        b.extend([0; 6]);
        if class_version < 81 {
            u16(&mut b, 3);
            b.extend(b"abc");
        }
        if class_version < 82 {
            b.extend([0; 20]);
        }
        u16(&mut b, 30);
        u32(&mut b, 0);
        u16(&mut b, 35);
        u16(&mut b, 30);
        u32(&mut b, 0);
        floats(&mut b, &[0.]);
        u32(&mut b, 1);
        u16(&mut b, submesh_version);
        u32(&mut b, 0);
        floats(&mut b, &[0., 0., 0., 1., 1., 0.]);
        u32(&mut b, 0);
        u16(&mut b, 1);
        b.push(b'm');
        let streams = 2 + extra.len() as u32 + u32::from(uv.is_some());
        u32(&mut b, streams);
        let stream = |b: &mut Vec<u8>, tag: u32, count: u32, bytes: &[u8]| {
            u32(b, tag);
            u16(b, 1);
            b.push(1);
            u32(b, count);
            b.extend(bytes);
        };
        let mut positions = Vec::new();
        for v in [0f32, 0., 0., 1., 0., 0., 0., 1., 0.] {
            positions.extend(v.to_le_bytes());
        }
        stream(&mut b, 1, 3, &positions);
        let indices: Vec<u8> = [0u32, 1, 2].iter().flat_map(|i| i.to_le_bytes()).collect();
        stream(&mut b, 0, 3, &indices);
        for &(tag, stride) in extra {
            stream(&mut b, tag, 3, &vec![0u8; 3 * stride]);
        }
        if let Some(uv) = uv {
            let bytes: Vec<u8> = (0..3).flat_map(|_| uv).flat_map(f32::to_le_bytes).collect();
            stream(&mut b, 12, 3, &bytes);
        }
        let array = |b: &mut Vec<u8>, marker: bool| {
            if marker {
                b.push(1);
            }
            u32(b, 0);
        };
        if submesh_version > 2 {
            array(&mut b, true);
            array(&mut b, true);
        }
        if submesh_version > 1 {
            b.push(1);
            u32(&mut b, 0);
        }
        if submesh_version > 3 {
            array(&mut b, false);
            array(&mut b, true);
        }
        b
    }

    #[test]
    fn historical_class_and_submesh_versions_parse() -> Result<()> {
        for (class_version, submesh_version) in [
            (1, 5),
            (1, 1),
            (1, 2),
            (1, 3),
            (1, 4),
            (81, 5),
            (82, 5),
            (80, 3),
        ] {
            let bytes = raw_mesh(class_version, submesh_version, &[], None);
            let mesh = Xcmsh::parse(&bytes)
                .map_err(|e| e.context(format!("{class_version}/{submesh_version}")))?;
            assert_eq!(mesh.submeshes[0].indices, [0, 1, 2]);
            assert_eq!(mesh.submeshes[0].material, "m");
            for end in 0..bytes.len() {
                assert!(
                    Xcmsh::parse(&bytes[..end]).is_err(),
                    "{class_version}/{submesh_version} prefix {end}"
                );
            }
        }
        assert!(Xcmsh::parse(&raw_mesh(84, 5, &[], None)).is_err());
        assert!(Xcmsh::parse(&raw_mesh(0, 5, &[], None)).is_err());
        assert!(Xcmsh::parse(&raw_mesh(82, 6, &[], None)).is_err());
        assert!(Xcmsh::parse(&raw_mesh(82, 0, &[], None)).is_err());
        Ok(())
    }

    #[test]
    fn auxiliary_stream_tags_are_length_checked_and_skipped() -> Result<()> {
        let extra = [
            (2, 16),
            (6, 4),
            (15, 8),
            (18, 8),
            (21, 8),
            (64, 12),
            (72, 12),
            (73, 8),
            (4, 4),
            (5, 4),
        ];
        let mesh = Xcmsh::parse(&raw_mesh(82, 5, &extra, None))?;
        assert_eq!(mesh.submeshes[0].positions.len(), 3);
        // A wrong stride for any known tag desynchronises the stream and must fail.
        for (tag, stride) in extra {
            assert!(
                Xcmsh::parse(&raw_mesh(82, 5, &[(tag, stride + 4)], None)).is_err(),
                "tag {tag}"
            );
        }
        assert!(Xcmsh::parse(&raw_mesh(82, 5, &[(7, 4)], None)).is_err());
        Ok(())
    }

    #[test]
    fn non_finite_uvs_are_sanitised_but_positions_stay_strict() -> Result<()> {
        let mesh = Xcmsh::parse(&raw_mesh(82, 5, &[], Some([f32::NAN, 0.5])))?;
        assert_eq!(mesh.submeshes[0].uvs, [[0.0, 0.5]; 3]);
        let mesh = Xcmsh::parse(&raw_mesh(82, 5, &[], Some([f32::INFINITY, -1.0])))?;
        assert_eq!(mesh.submeshes[0].uvs, [[0.0, -1.0]; 3]);
        Ok(())
    }

    #[test]
    fn rejects_truncation_invalid_counts_indices_floats_and_tags() {
        let f = fixture();
        for end in 0..f.bytes.len() {
            assert!(Xcmsh::parse(&f.bytes[..end]).is_err(), "prefix {end}");
        }
        for (offset, value) in [
            (f.index, 3u32),
            (f.position, f32::NAN.to_bits()),
            (f.count, u32::MAX),
            (f.tag, 999),
        ] {
            let mut bytes = f.bytes.clone();
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(Xcmsh::parse(&bytes).is_err(), "mutated offset {offset}");
        }
        let mut bytes = f.bytes.clone();
        bytes[25..27].copy_from_slice(&84u16.to_le_bytes());
        assert!(Xcmsh::parse(&bytes).is_err());
    }
}
