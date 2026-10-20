//! Bounded raw class82/resource23/PS30 LOD-reference reader.
//! Levels retain serialized array order; no runtime distance selection is inferred.
use anyhow::{Context, Result, ensure};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Xlmsh {
    pub levels: Vec<LodLevel>,
    pub lod_scale: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct LodLevel {
    pub source_index: usize,
    /// Exact source resource ID, usually a basename, not an archive-resolved path.
    pub mesh: String,
}

impl Xlmsh {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= 1024 * 1024,
            "XLM SH exceeds resource ceiling"
        );
        let mut c = Cursor(bytes);
        ensure!(
            c.u16()? == 1 && c.u8()? == 1 && c.u16()? == 1 && c.u8()? == 1,
            "unsupported XLM SH accessor header (only raw resources supported)"
        );
        ensure!(
            c.string()? == b"eCResourceMeshLoD_PS",
            "unexpected XLM SH resource class"
        );
        ensure!(
            c.u8()? == 1 && c.u16()? == 0 && c.u16()? == 82,
            "unsupported XLM SH class header/version"
        );
        c.take(6)?;
        ensure!(c.u16()? == 30, "unsupported XLM SH property header version");
        let properties = c.count(4096, 10)?;
        let mut lod_scale = None;
        for _ in 0..properties {
            let name = c.string()?;
            let kind = c.string()?;
            ensure!(c.u16()? == 30, "unsupported XLM SH property version");
            let length = c.u32()? as usize;
            let payload = c.take(length)?;
            if kind == b"float" {
                ensure!(payload.len() == 4, "invalid XLM SH float property length");
                let value = f32::from_le_bytes(payload.try_into()?);
                ensure!(value.is_finite(), "non-finite XLM SH float property");
                if name == b"LoDScale" {
                    ensure!(
                        lod_scale.replace(value).is_none(),
                        "duplicate XLM SH LoDScale"
                    );
                }
            } else if kind == b"bCBox" {
                ensure!(payload.len() == 24, "invalid XLM SH bounds property length");
                for chunk in payload.chunks_exact(4) {
                    ensure!(
                        f32::from_le_bytes(chunk.try_into()?).is_finite(),
                        "non-finite XLM SH bounds"
                    );
                }
            }
        }
        ensure!(
            c.u16()? == 23 && c.u16()? == 30,
            "unsupported XLM SH LOD/resource version"
        );
        c.u32()?; // Opaque source size/metadata; not a per-level distance.
        let count = c.count(128, 2)?;
        ensure!(count > 0, "XLM SH has no levels");
        let mut levels = Vec::with_capacity(count);
        for source_index in 0..count {
            let mesh: String = c.string()?.iter().map(|&b| char::from(b)).collect();
            crate::normalize_path(&mesh).context("unsafe XLM SH resource ID")?;
            ensure!(
                mesh.to_ascii_lowercase().ends_with(".xcmsh"),
                "unsupported XLM SH reference extension"
            );
            levels.push(LodLevel { source_index, mesh });
        }
        ensure!(c.0.is_empty(), "unexpected trailing XLM SH bytes");
        Ok(Self { levels, lod_scale })
    }
}

struct Cursor<'a>(&'a [u8]);
impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        ensure!(n <= self.0.len(), "truncated XLM SH field");
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
    fn string(&mut self) -> Result<&'a [u8]> {
        let n = self.u16()? as usize;
        ensure!(n <= 16_384, "XLM SH inline string exceeds ceiling");
        self.take(n)
    }
    fn count(&mut self, max: usize, stride: usize) -> Result<usize> {
        let n = self.u32()? as usize;
        ensure!(
            n <= max && n <= self.0.len() / stride,
            "XLM SH count exceeds bounds"
        );
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn u16(b: &mut Vec<u8>, n: u16) {
        b.extend(n.to_le_bytes());
    }
    fn u32(b: &mut Vec<u8>, n: u32) {
        b.extend(n.to_le_bytes());
    }
    fn string(b: &mut Vec<u8>, s: &str) {
        u16(b, s.len() as u16);
        b.extend(s.bytes());
    }
    fn fixture(first: &str) -> Vec<u8> {
        let mut b = Vec::new();
        u16(&mut b, 1);
        b.push(1);
        u16(&mut b, 1);
        b.push(1);
        string(&mut b, "eCResourceMeshLoD_PS");
        b.push(1);
        u16(&mut b, 0);
        u16(&mut b, 82);
        b.extend([0; 6]);
        u16(&mut b, 30);
        u32(&mut b, 1);
        string(&mut b, "LoDScale");
        string(&mut b, "float");
        u16(&mut b, 30);
        u32(&mut b, 4);
        b.extend(1f32.to_le_bytes());
        u16(&mut b, 23);
        u16(&mut b, 30);
        u32(&mut b, 0);
        u32(&mut b, 2);
        string(&mut b, first);
        string(&mut b, "test_LOD1.xcmsh");
        b
    }
    #[test]
    fn preserves_reference_order_and_scale() -> Result<()> {
        let x = Xlmsh::parse(&fixture("test.xcmsh"))?;
        assert_eq!(x.lod_scale, Some(1.));
        assert_eq!(x.levels.len(), 2);
        assert_eq!(x.levels[0].source_index, 0);
        assert_eq!(x.levels[0].mesh, "test.xcmsh");
        assert_eq!(x.levels[1].source_index, 1);
        assert_eq!(x.levels[1].mesh, "test_LOD1.xcmsh");
        Ok(())
    }
    #[test]
    fn rejects_truncation_trailing_and_unsafe_ids() {
        let b = fixture("test.xcmsh");
        for n in 0..b.len() {
            assert!(Xlmsh::parse(&b[..n]).is_err());
        }
        let mut trailing = b.clone();
        trailing.push(0);
        assert!(Xlmsh::parse(&trailing).is_err());
        for bad in ["../test.xcmsh", "C:/test.xcmsh", "test.xlmsh"] {
            assert!(Xlmsh::parse(&fixture(bad)).is_err());
        }
        let mut version = b;
        version[31..33].copy_from_slice(&1u16.to_le_bytes());
        assert!(Xlmsh::parse(&version).is_err());
    }
}
