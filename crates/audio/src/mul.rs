//! `Library.pak/g3.mul`: the music table (`gCMusicTrigger`, `gCMusicFragment`, `gCMusicLink`).
//!
//! Container: `GENOMFLE`, `u16 version`, `u32 size`, `u16 2`, `u16 1`, `u32 8`, a run of
//! serialized class instances (accessor + class name + object, see `PROPERTY_STREAMS.md`), then
//! `0xDEADBEEF`, `u8 1`, `u32 n` and `n` length-prefixed (`u16`) strings up to end of file.
//! Property payloads use the shared Genome property grammar, so decoding goes through
//! `gothic_formats::propstream`.
use anyhow::{Context, Result, bail, ensure};
use gothic_formats::propstream::{
    CLASS_ACCESSOR, Cursor, DecodeStats, PropertySet, Strings, read_sub_class,
};

const MAX_OBJECTS: usize = 100_000;
const MAX_STRINGS: usize = 1_000_000;
const MARKER: [u8; 4] = [0xEF, 0xBE, 0xAD, 0xDE];

/// All property-set instances found in a `.mul` file, in file order.
#[derive(Debug, Clone, Default)]
pub struct MulDocument {
    pub objects: Vec<PropertySet>,
    pub stats: DecodeStats,
}

pub fn parse(bytes: &[u8]) -> Result<MulDocument> {
    ensure!(
        bytes.len() > 24 && &bytes[..8] == b"GENOMFLE",
        "not a Genome file"
    );
    // The string table is the trailing section; find its marker from the back.
    let marker = (0..bytes.len().saturating_sub(9))
        .rev()
        .find(|&i| {
            bytes[i..i + 4] == MARKER && bytes[i + 4] == 1 && table_ok(bytes, i + 5).is_some()
        })
        .context("string table not found")?;
    let table = table_ok(bytes, marker + 5).context("string table")?;
    let refs: Vec<&[u8]> = table.iter().map(|&(a, b)| &bytes[a..b]).collect();
    let strings = Strings::new(&refs);
    // Body after the 8-byte magic, `u16 file version`, `u32 size`: `u16 2`, then groups of
    // `u16 1`, `u32 count` and `count` class instances.
    let mut cur = Cursor::with_range(bytes, 14, marker)?;
    let mut doc = MulDocument::default();
    let mut stats = DecodeStats::default();
    let _version = cur.u16()?;
    while cur.remaining() > 0 {
        let group_flag = cur.u16()?;
        ensure!(group_flag == 1, "unexpected group flag {group_flag}");
        let count = cur.u32()? as usize;
        ensure!(doc.objects.len() + count <= MAX_OBJECTS, "too many objects");
        for _ in 0..count {
            if !bytes[cur.pos..].starts_with(&CLASS_ACCESSOR) {
                bail!("unexpected byte at {:#x}, expected class accessor", cur.pos);
            }
            doc.objects
                .push(read_sub_class(&mut cur, &strings, 0, &mut stats)?);
        }
    }
    doc.stats = stats;
    Ok(doc)
}

/// Validate a string table starting at `at` (`u32 n`, then `n` x (`u16 len`, bytes)) that ends
/// exactly at end of file; return the byte ranges.
fn table_ok(bytes: &[u8], at: usize) -> Option<Vec<(usize, usize)>> {
    let n = u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize;
    if n > MAX_STRINGS || n > bytes.len() {
        return None;
    }
    let mut p = at + 4;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let len = u16::from_le_bytes(bytes.get(p..p + 2)?.try_into().ok()?) as usize;
        p += 2;
        bytes.get(p..p + len)?;
        out.push((p, p + len));
        p += len;
    }
    (p == bytes.len()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic `.mul`: one `gCMusicTrigger`-like class instance with a bool and a string
    /// property, framed exactly like the real file.
    fn synthetic() -> Vec<u8> {
        let strings = [
            "gCMusicTrigger",
            "ID",
            "bCString",
            "Loc_Test",
            "IsStopTrigger",
            "bool",
        ];
        let prop = |name: u16, ty: u16, payload: &[u8]| {
            let mut p = Vec::new();
            p.extend(name.to_le_bytes());
            p.extend(ty.to_le_bytes());
            p.extend(30u16.to_le_bytes());
            p.extend((payload.len() as u32).to_le_bytes());
            p.extend(payload);
            p
        };
        let mut body = Vec::new();
        body.extend(30u16.to_le_bytes()); // list version
        body.extend(2u32.to_le_bytes());
        body.extend(prop(1, 2, &3u16.to_le_bytes()));
        body.extend(prop(4, 5, &[1]));
        let mut obj = Vec::new();
        obj.extend(0x53u16.to_le_bytes());
        obj.extend((body.len() as u32).to_le_bytes());
        obj.extend(&body);
        let mut b = b"GENOMFLE".to_vec();
        b.extend(1u16.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(1u16.to_le_bytes()); // group flag
        b.extend(1u32.to_le_bytes()); // count
        b.extend(CLASS_ACCESSOR);
        b.extend(0u16.to_le_bytes()); // class name index
        b.extend([1, 0, 0, 0x53, 0]);
        b.extend(&obj);
        b.extend(MARKER);
        b.push(1);
        b.extend((strings.len() as u32).to_le_bytes());
        for s in strings {
            b.extend((s.len() as u16).to_le_bytes());
            b.extend(s.as_bytes());
        }
        b
    }

    #[test]
    fn parses_synthetic_document() {
        let doc = parse(&synthetic()).unwrap();
        assert_eq!(doc.objects.len(), 1);
        let o = &doc.objects[0];
        assert_eq!(o.class, "gCMusicTrigger");
        assert_eq!(o.string("ID"), Some("Loc_Test"));
        assert_eq!(o.boolean("IsStopTrigger"), Some(true));
        assert_eq!(doc.stats.typed, doc.stats.properties);
    }

    #[test]
    fn rejects_truncated_and_foreign_input() {
        let good = synthetic();
        assert!(parse(&good[..good.len() - 3]).is_err());
        assert!(parse(b"not a genome file at all, long enough....").is_err());
        let mut bad_group = good.clone();
        bad_group[16] = 9; // group flag
        assert!(parse(&bad_group).is_err());
        for n in 0..good.len() {
            let _ = parse(&good[..n]); // must never panic
        }
    }
}
