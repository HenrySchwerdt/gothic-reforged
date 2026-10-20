//! Bounded GENOMFLE v1 envelope and raw string table. String encoding is not inferred.
use anyhow::{Context, Result, ensure};

#[derive(Debug)]
pub struct GenomeFile<'a> {
    pub body: &'a [u8],
    pub table_offset: usize,
    pub strings: Vec<&'a [u8]>,
}

impl<'a> GenomeFile<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= crate::MAX_PAYLOAD as usize,
            "Genome resource exceeds ceiling"
        );
        ensure!(
            bytes.get(..8) == Some(b"GENOMFLE"),
            "not a GENOMFLE resource"
        );
        ensure!(
            bytes.get(8..10) == Some(&[1, 0]),
            "unsupported GENOMFLE version"
        );
        let offset_bytes = bytes.get(10..14).context("truncated Genome header")?;
        let table_offset = u32::from_le_bytes(offset_bytes.try_into()?) as usize;
        ensure!(
            (14..bytes.len()).contains(&table_offset),
            "Genome table offset out of bounds"
        );
        let mut tail = &bytes[table_offset..];
        ensure!(
            take(&mut tail, 4)? == [0xef, 0xbe, 0xad, 0xde],
            "missing Genome table marker"
        );
        ensure!(
            take(&mut tail, 1)? == [1],
            "unsupported Genome table version"
        );
        let count = u32::from_le_bytes(take(&mut tail, 4)?.try_into()?) as usize;
        ensure!(
            count <= 65536 && count <= tail.len() / 2,
            "Genome string count exceeds bounds"
        );
        let mut strings = Vec::with_capacity(count);
        for _ in 0..count {
            let len = u16::from_le_bytes(take(&mut tail, 2)?.try_into()?) as usize;
            ensure!(len <= 16384, "Genome string exceeds ceiling");
            strings.push(take(&mut tail, len)?);
        }
        ensure!(tail.is_empty(), "unexpected trailing Genome metadata");
        Ok(Self {
            body: &bytes[14..table_offset],
            table_offset,
            strings,
        })
    }
}

fn take<'a>(bytes: &mut &'a [u8], count: usize) -> Result<&'a [u8]> {
    ensure!(bytes.len() >= count, "truncated Genome metadata");
    let (result, tail) = bytes.split_at(count);
    *bytes = tail;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_table_and_keeps_raw_strings() -> Result<()> {
        let mut b = b"GENOMFLE".to_vec();
        b.extend(1u16.to_le_bytes());
        b.extend(14u32.to_le_bytes());
        b.extend([0xef, 0xbe, 0xad, 0xde, 1]);
        b.extend(1u32.to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend([0xff, b'a']);
        assert_eq!(GenomeFile::parse(&b)?.strings, vec![&[0xff, b'a'][..]]);
        for n in 0..b.len() {
            assert!(GenomeFile::parse(&b[..n]).is_err(), "truncated {n}");
        }
        b.push(0);
        assert!(GenomeFile::parse(&b).is_err());
        Ok(())
    }
}
