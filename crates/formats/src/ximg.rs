//! GENOMFLE v1 / observed G3IMG DXT texture subset. No game data embedded.
use anyhow::{Context, Result, bail, ensure};
use std::ops::Range;

#[derive(Debug)]
pub struct Ximg<'a> {
    pub width: u32,
    pub height: u32,
    pub fourcc: [u8; 4],
    /// Largest to smallest; source XIMG stores the chain in the opposite order.
    mips: Vec<&'a [u8]>,
}

impl<'a> Ximg<'a> {
    pub fn mips(&self) -> &[&'a [u8]] {
        &self.mips
    }
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let genome = crate::genome::GenomeFile::parse(bytes)?;
        ensure!(
            bytes.len() <= crate::MAX_PAYLOAD as usize,
            "XIMG exceeds size ceiling"
        );
        ensure!(
            bytes.get(..8) == Some(b"GENOMFLE"),
            "not a GENOMFLE resource"
        );
        ensure!(
            bytes.get(8..10) == Some(&[1, 0]),
            "unsupported GENOMFLE version"
        );
        ensure!(
            bytes.get(38..43) == Some(b"G3IMG"),
            "unsupported resource header; expected G3IMG at 38"
        );
        let table = genome.table_offset;
        ensure!(
            (87..bytes.len()).contains(&table),
            "XIMG data/table range out of bounds"
        );
        ensure!(
            bytes.get(table..table + 4) == Some(&[0xef, 0xbe, 0xad, 0xde]),
            "missing GENOMFLE string-table marker"
        );
        let width = word(bytes, 47)?;
        let height = word(bytes, 51)?;
        ensure!(
            (1..=16384).contains(&width) && (1..=16384).contains(&height),
            "unsupported texture dimensions"
        );
        let count = word(bytes, 63)?;
        let max_mips = 32 - width.max(height).leading_zeros();
        ensure!((1..=max_mips).contains(&count), "invalid mip count");
        let fourcc: [u8; 4] = bytes.get(71..75).context("missing FourCC")?.try_into()?;
        let block_bytes = match &fourcc {
            b"DXT1" => 8,
            b"DXT3" | b"DXT5" => 16,
            _ => bail!("unsupported XIMG texture format {:?}", fourcc),
        };
        let mut cursor = 87usize;
        let mut ranges: Vec<Range<usize>> = Vec::new();
        for level in (0..count).rev() {
            let size = (width >> level).max(1).div_ceil(4) as usize
                * (height >> level).max(1).div_ceil(4) as usize
                * block_bytes;
            let end = cursor.checked_add(size).context("mip range overflow")?;
            ensure!(end <= table, "truncated XIMG mip payload");
            ranges.push(cursor..end);
            cursor = end;
        }
        ensure!(
            cursor == table,
            "XIMG data size differs from mip chain; unsupported layout"
        );
        let mips = ranges
            .into_iter()
            .rev()
            .map(|range| &bytes[range])
            .collect();
        Ok(Self {
            width,
            height,
            fourcc,
            mips,
        })
    }

    /// Rewrap compressed blocks without lossy re-encoding. Legacy DDS 2D only.
    pub fn to_dds(&self) -> Vec<u8> {
        let mut header = [0u32; 31];
        header[0] = 124;
        header[1] = 0x81007 | if self.mips.len() > 1 { 0x20000 } else { 0 };
        header[2] = self.height;
        header[3] = self.width;
        header[4] = self.mips[0].len() as u32;
        header[6] = self.mips.len() as u32;
        header[18] = 32;
        header[19] = 4;
        header[20] = u32::from_le_bytes(self.fourcc);
        header[26] = 0x1000 | if self.mips.len() > 1 { 0x400008 } else { 0 };
        let mut result = Vec::with_capacity(128 + self.mips.iter().map(|m| m.len()).sum::<usize>());
        result.extend(b"DDS ");
        for value in header {
            result.extend(value.to_le_bytes());
        }
        for mip in &self.mips {
            result.extend(*mip);
        }
        result
    }
}

fn word(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .context("truncated XIMG field")?
            .try_into()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 87];
        b[..8].copy_from_slice(b"GENOMFLE");
        b[8] = 1;
        b[38..43].copy_from_slice(b"G3IMG");
        b[47..51].copy_from_slice(&8u32.to_le_bytes());
        b[51..55].copy_from_slice(&4u32.to_le_bytes());
        b[63..67].copy_from_slice(&4u32.to_le_bytes());
        b[71..75].copy_from_slice(b"DXT1");
        for (value, size) in [(3, 8), (2, 8), (1, 8), (0, 16)] {
            b.extend(vec![value; size]);
        }
        let table = b.len() as u32;
        b[10..14].copy_from_slice(&table.to_le_bytes());
        b.extend([0xef, 0xbe, 0xad, 0xde, 1, 0, 0, 0, 0]);
        b
    }
    #[test]
    fn preserves_blocks_and_reverses_mips() -> Result<()> {
        let b = fixture();
        let image = Ximg::parse(&b)?;
        assert_eq!(
            image.mips.iter().map(|m| m.len()).collect::<Vec<_>>(),
            vec![16, 8, 8, 8]
        );
        let dds = image.to_dds();
        assert_eq!(&dds[..4], b"DDS ");
        assert_eq!(&dds[84..88], b"DXT1");
        assert_eq!(&dds[128..144], &[0; 16]);
        assert_eq!(&dds[160..168], &[3; 8]);
        Ok(())
    }
    #[test]
    fn rejects_truncated_and_unsupported_resources() {
        let b = fixture();
        for n in 0..b.len() - 5 {
            assert!(Ximg::parse(&b[..n]).is_err(), "{n}");
        }
        let mut b = fixture();
        b[71..75].copy_from_slice(b"XXXX");
        assert!(Ximg::parse(&b).is_err());
        let mut b = fixture();
        b[63..67].copy_from_slice(&32u32.to_le_bytes());
        assert!(Ximg::parse(&b).is_err());
    }
}
