//! Template (`Templates.pak`, `*.tple`) envelope summary: **not** a full property decoder.
//!
//! Observed variants (all 6295 installed `.tple`; 6294 parse, 1 is corrupt in the shipped data:
//! `sfx_env_water_waterfall_03.tple` has a wrong first string length and a trailer overrunning the file):
//! * `GENOMFLE` v1 envelope, string table in the trailer, body = property stream
//!   (6129 files; every body starts with `GENOMETP`, i.e. the same container wrapped in an envelope). `strings[0]` is the template file name, `strings[1]` the entity /
//!   template name, later strings are class names (`gCInventory_PS`, `eCEntityProxy`),
//!   property names and type names, in first-use order.
//! * Bare `GENOMETP` files (115): `"GENOMETP"`, `u16` version (58..=62), 3 bytes `01 00 01`,
//!   `u32` string count, then `u16`-length-prefixed strings (same order as above), then the
//!   property stream, ending in `ff*8`.
//! * Raw legacy variant (50 files): `u32 = 2`, `u16`, 16-byte GUID, ...; the entity name is a
//!   `u16`-length-prefixed string at fixed offset 42 (validated by the survey on every file).
//!
//! [`TemplateSummary`] decodes variant, entity name and vocabulary only. The full property
//! streams (stats, attributes, inventories) are decoded by [`TemplateDocument`] on top of
//! [`crate::propstream`]; see `docs/research/PROPERTY_STREAMS.md` for coverage.
use anyhow::{Result, bail, ensure};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum TemplateVariant {
    /// `GENOMFLE` envelope with trailing string table.
    Genomfle,
    /// `GENOMFLE` envelope whose body starts with `GENOMETP` (the normal case).
    GenomfleGenometp,
    /// Bare `GENOMETP` file with a leading string table.
    Genometp,
    /// Legacy raw variant (`02 00 00 00` header).
    Raw,
}

#[derive(Debug, Clone, Serialize)]
pub struct TemplateSummary {
    pub variant: TemplateVariant,
    /// File name recorded in the Genome string table (`*.tple`), if present.
    pub recorded_file_name: Option<String>,
    /// Entity/template name used by game scripts (e.g. `It_FiremageCup`).
    pub entity_name: String,
    /// Persistent class names, e.g. `gCInventory_PS`.
    pub classes: Vec<String>,
    /// Every other string-table entry (property and type names), in order.
    pub other_strings: Vec<String>,
    pub payload_bytes: usize,
}

/// `gC...`/`eC...`/`bC...` class-style names that denote persistent classes (not `bC*` value types).
fn is_class_name(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() > 3
        && matches!(&b[..2], b"gC" | b"eC")
        && b[2].is_ascii_uppercase()
        && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

const MAX_STRINGS: usize = 65_536;
const MAX_STRING: usize = 1024 * 1024;

fn take<'a>(bytes: &mut &'a [u8], n: usize) -> Result<&'a [u8]> {
    ensure!(bytes.len() >= n, "truncated template string table");
    let (head, tail) = bytes.split_at(n);
    *bytes = tail;
    Ok(head)
}

/// Read `count` u16-length-prefixed strings; returns the remaining bytes.
fn read_strings(mut tail: &[u8], count: usize) -> Result<(Vec<&[u8]>, &[u8])> {
    ensure!(
        count <= MAX_STRINGS && count <= tail.len() / 2,
        "template string count out of bounds"
    );
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let len = usize::from(u16::from_le_bytes(take(&mut tail, 2)?.try_into()?));
        ensure!(len <= MAX_STRING, "template string exceeds ceiling");
        out.push(take(&mut tail, len)?);
    }
    Ok((out, tail))
}

impl TemplateSummary {
    fn from_strings(
        variant: TemplateVariant,
        strings: Vec<String>,
        payload_bytes: usize,
    ) -> Result<Self> {
        let recorded_file_name = strings
            .first()
            .filter(|s| s.to_ascii_lowercase().ends_with(".tple"))
            .cloned();
        let entity_name = match (&recorded_file_name, strings.get(1)) {
            (Some(_), Some(n)) => n.clone(),
            _ => bail!("template string table has no file/entity name pair"),
        };
        let (classes, other_strings) = strings.into_iter().skip(2).partition(|s| is_class_name(s));
        Ok(Self {
            variant,
            recorded_file_name,
            entity_name,
            classes,
            other_strings,
            payload_bytes,
        })
    }

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= crate::MAX_PAYLOAD as usize,
            "template exceeds payload ceiling"
        );
        if bytes.starts_with(b"GENOMFLE") {
            ensure!(
                bytes.get(8..10) == Some(&[1, 0]),
                "unsupported GENOMFLE version"
            );
            let table_offset = u32::from_le_bytes(
                bytes
                    .get(10..14)
                    .ok_or_else(|| anyhow::anyhow!("truncated header"))?
                    .try_into()?,
            ) as usize;
            ensure!(
                (14..bytes.len()).contains(&table_offset),
                "table offset out of bounds"
            );
            let mut tail = &bytes[table_offset..];
            ensure!(
                take(&mut tail, 4)? == [0xef, 0xbe, 0xad, 0xde],
                "missing table marker"
            );
            ensure!(take(&mut tail, 1)? == [1], "unsupported table version");
            let count = u32::from_le_bytes(take(&mut tail, 4)?.try_into()?) as usize;
            let (strings, rest) = read_strings(tail, count)?;
            ensure!(rest.is_empty(), "unexpected trailing metadata");
            let body = &bytes[14..table_offset];
            let variant = if body.starts_with(b"GENOMETP") {
                TemplateVariant::GenomfleGenometp
            } else {
                TemplateVariant::Genomfle
            };
            return Self::from_strings(
                variant,
                strings.iter().map(|s| latin1(s)).collect(),
                body.len(),
            );
        }
        if bytes.starts_with(b"GENOMETP") {
            let mut tail = &bytes[8..];
            let version = u16::from_le_bytes(take(&mut tail, 2)?.try_into()?);
            ensure!(
                (58..=62).contains(&version),
                "unsupported GENOMETP version {version}"
            );
            ensure!(
                take(&mut tail, 3)? == [1, 0, 1],
                "unexpected GENOMETP preamble"
            );
            let count = u32::from_le_bytes(take(&mut tail, 4)?.try_into()?) as usize;
            let (strings, rest) = read_strings(tail, count)?;
            return Self::from_strings(
                TemplateVariant::Genometp,
                strings.iter().map(|s| latin1(s)).collect(),
                rest.len(),
            );
        }
        ensure!(
            bytes.len() > 44 && bytes[..4] == [2, 0, 0, 0],
            "unrecognized template header"
        );
        let len = usize::from(u16::from_le_bytes([bytes[42], bytes[43]]));
        ensure!(
            (1..=128).contains(&len),
            "implausible raw template name length"
        );
        let name = bytes
            .get(44..44 + len)
            .ok_or_else(|| anyhow::anyhow!("raw template name truncated"))?;
        ensure!(
            name.iter().all(|&c| c.is_ascii_graphic()),
            "raw template name is not printable ASCII"
        );
        Ok(Self {
            variant: TemplateVariant::Raw,
            recorded_file_name: None,
            entity_name: latin1(name),
            classes: Vec::new(),
            other_strings: Vec::new(),
            payload_bytes: bytes.len(),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Full template decoding
// ---------------------------------------------------------------------------------------------

use crate::propstream::{Cursor, DecodeStats, PropertySet, Strings, hex, read_sub_class};

const DEADCODE: u32 = 0xdead_c0de;

/// One entity header of a template (`*.tple` holds a small tree of these; the first is the
/// item/management header, the second the reference header that carries the property sets).
#[derive(Debug, Clone, Serialize)]
pub struct TemplateEntity {
    pub header_version: u16,
    pub file_name: String,
    /// Template GUID (20 raw bytes as lowercase hex): the id other templates and inventories use.
    pub guid: String,
    pub flags: [bool; 7],
    pub helper_parent: bool,
    /// Set when this header only references another template.
    pub ref_template: Option<String>,
    pub name: String,
    pub world_matrix: [[f32; 4]; 4],
    pub local_matrix: [[f32; 4]; 4],
    pub deleted: bool,
    pub property_sets: Vec<PropertySet>,
}

impl TemplateEntity {
    pub fn set(&self, class: &str) -> Option<&PropertySet> {
        self.property_sets.iter().find(|s| s.class == class)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TemplateDocument {
    pub variant: TemplateVariant,
    pub version: u16,
    pub entities: Vec<TemplateEntity>,
    pub edges: Vec<(u32, u32)>,
}

fn matrix(cur: &mut Cursor) -> Result<[[f32; 4]; 4]> {
    let mut m = [[0.0; 4]; 4];
    for row in &mut m {
        *row = cur.floats()?;
    }
    Ok(m)
}

impl TemplateDocument {
    /// Parse a `GENOMFLE` or bare `GENOMETP` template (the 50 legacy raw files are rejected).
    pub fn parse(bytes: &[u8], stats: &mut DecodeStats) -> Result<Self> {
        ensure!(
            bytes.len() <= crate::MAX_PAYLOAD as usize,
            "template exceeds payload ceiling"
        );
        if bytes.starts_with(b"GENOMFLE") {
            ensure!(
                bytes.get(8..10) == Some(&[1, 0]),
                "unsupported GENOMFLE version"
            );
            let table_offset = u32::from_le_bytes(
                bytes
                    .get(10..14)
                    .ok_or_else(|| anyhow::anyhow!("truncated header"))?
                    .try_into()?,
            ) as usize;
            ensure!(
                (14..bytes.len()).contains(&table_offset),
                "table offset out of bounds"
            );
            let mut tail = &bytes[table_offset..];
            ensure!(
                take(&mut tail, 4)? == [0xef, 0xbe, 0xad, 0xde],
                "missing table marker"
            );
            ensure!(take(&mut tail, 1)? == [1], "unsupported table version");
            let count = u32::from_le_bytes(take(&mut tail, 4)?.try_into()?) as usize;
            let (strings, rest) = read_strings(tail, count)?;
            ensure!(rest.is_empty(), "unexpected trailing metadata");
            let mut cur = Cursor::with_range(bytes, 14, table_offset)?;
            ensure!(cur.take(8)? == b"GENOMETP", "template body is not GENOMETP");
            let version = cur.u16()?;
            ensure!(
                (58..=62).contains(&version),
                "unsupported GENOMETP version {version}"
            );
            cur.skip(2)?;
            let indexed = cur.flag()?;
            let effective: Vec<&[u8]> = if indexed {
                let n = cur.u32()? as usize;
                ensure!(
                    n <= MAX_STRINGS && n <= cur.remaining() / 2,
                    "index table bounds"
                );
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    let i = usize::from(cur.u16()?);
                    v.push(
                        *strings
                            .get(i)
                            .ok_or_else(|| anyhow::anyhow!("string index"))?,
                    );
                }
                v
            } else {
                strings
            };
            Self::body(
                TemplateVariant::GenomfleGenometp,
                version,
                &mut cur,
                &effective,
                stats,
            )
        } else if bytes.starts_with(b"GENOMETP") {
            let mut cur = Cursor::new(bytes);
            cur.skip(8)?;
            let version = cur.u16()?;
            ensure!(
                (58..=62).contains(&version),
                "unsupported GENOMETP version {version}"
            );
            ensure!(cur.take(3)? == [1, 0, 1], "unexpected GENOMETP preamble");
            let count = cur.u32()? as usize;
            let (strings, rest) = read_strings(&bytes[cur.pos..], count)?;
            cur.pos = bytes.len() - rest.len();
            Self::body(
                TemplateVariant::Genometp,
                version,
                &mut cur,
                &strings,
                stats,
            )
        } else {
            bail!("unsupported template variant (legacy raw)")
        }
    }

    fn body(
        variant: TemplateVariant,
        version: u16,
        cur: &mut Cursor,
        table: &[&[u8]],
        stats: &mut DecodeStats,
    ) -> Result<Self> {
        let strings = Strings::new(table);
        cur.skip(4)?; // size of code section
        let count = cur.u32()? as usize;
        ensure!(count > 0 && count <= 1024, "header count {count}");
        let mut entities = Vec::with_capacity(count);
        for _ in 0..count {
            entities.push(Self::entity(cur, &strings, stats)?);
        }
        let mut edges = Vec::new();
        loop {
            let parent = cur.u32()?;
            let child = cur.u32()?;
            if parent == u32::MAX {
                ensure!(child == u32::MAX, "invalid graph terminator");
                break;
            }
            ensure!(
                (parent as usize) < count && (child as usize) < count,
                "invalid entity index"
            );
            edges.push((parent, child));
            ensure!(edges.len() <= count, "too many edges");
        }
        ensure!(cur.remaining() == 0, "trailing bytes after template graph");
        Ok(Self {
            variant,
            version,
            entities,
            edges,
        })
    }

    fn entity(
        cur: &mut Cursor,
        strings: &Strings,
        stats: &mut DecodeStats,
    ) -> Result<TemplateEntity> {
        let header_version = cur.u16()?;
        ensure!(
            (40..=62).contains(&header_version),
            "unsupported template header version {header_version}"
        );
        let file_name = strings.entry(cur)?;
        let guid = hex(&cur.guid()?);
        let mut flags = [false; 7];
        for f in &mut flags {
            *f = cur.flag()?;
        }
        let helper_parent = cur.flag()?;
        let ref_template = if cur.flag()? {
            Some(hex(&cur.guid()?))
        } else {
            None
        };
        cur.skip(4 + 2 + 1)?; // alpha, insert type, render priority
        let name = strings.entry(cur)?;
        let world_matrix = matrix(cur)?;
        let local_matrix = matrix(cur)?;
        cur.skip(3 * 24)?; // three boxes
        cur.skip(4)?; // visual LoD factor
        cur.flag()?;
        let deleted = cur.flag()?;
        cur.skip(4)?; // object cull factor (>= 40)
        if header_version >= 56 {
            cur.skip(4)?;
        }
        if header_version >= 58 {
            cur.skip(1)?;
        }
        if header_version >= 61 {
            cur.skip(4)?;
        }
        if header_version >= 62 {
            cur.skip(2)?;
        }
        let mut property_sets = Vec::new();
        if ref_template.is_none() {
            let n = cur.u32()? as usize;
            ensure!(n <= 4096 && n <= cur.remaining() / 20, "class entry count");
            for _ in 0..n {
                let wrapper = cur.u16()?;
                cur.skip(4)?; // pre-size
                let _pre_type = strings.entry(cur)?;
                let mut set = read_sub_class(cur, strings, 0, stats)?;
                set.wrapper_version = Some(wrapper);
                ensure!(cur.u32()? == DEADCODE, "class terminator missing");
                property_sets.push(set);
            }
        }
        Ok(TemplateEntity {
            header_version,
            file_name,
            guid,
            flags,
            helper_parent,
            ref_template,
            name,
            world_matrix,
            local_matrix,
            deleted,
            property_sets,
        })
    }

    /// The header that carries the property sets of an ordinary (non object-group) template.
    pub fn reference_header(&self) -> Option<&TemplateEntity> {
        self.entities
            .iter()
            .find(|e| !e.helper_parent && e.ref_template.is_none())
            .or_else(|| self.entities.first())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn genome(body: &[u8], strings: &[&str]) -> Vec<u8> {
        let mut b = b"GENOMFLE".to_vec();
        b.extend(1u16.to_le_bytes());
        b.extend((14 + body.len() as u32).to_le_bytes());
        b.extend(body);
        b.extend([0xef, 0xbe, 0xad, 0xde, 1]);
        b.extend((strings.len() as u32).to_le_bytes());
        for s in strings {
            b.extend((s.len() as u16).to_le_bytes());
            b.extend(s.as_bytes());
        }
        b
    }

    #[test]
    fn classifies_genome_variants_and_vocabulary() {
        let b = genome(
            b"\x01\x00",
            &[
                "X.tple",
                "It_X",
                "gCInventory_PS",
                "Amount",
                "int",
                "eCEntityProxy",
                "bCString",
            ],
        );
        let t = TemplateSummary::parse(&b).unwrap();
        assert_eq!(t.variant, TemplateVariant::Genomfle);
        assert_eq!(t.entity_name, "It_X");
        assert_eq!(t.classes, ["gCInventory_PS", "eCEntityProxy"]);
        assert_eq!(t.other_strings, ["Amount", "int", "bCString"]);
        let b = genome(b"GENOMETP>\x00", &["X.tple", "N"]);
        assert_eq!(
            TemplateSummary::parse(&b).unwrap().variant,
            TemplateVariant::GenomfleGenometp
        );
        assert!(TemplateSummary::parse(&genome(b"", &["only"])).is_err());
        let mut g = b"GENOMETP".to_vec();
        g.extend([58, 0, 1, 0, 1, 2, 0, 0, 0]);
        for s in ["A.tple", "Ent"] {
            g.extend((s.len() as u16).to_le_bytes());
            g.extend(s.as_bytes());
        }
        g.extend([0xff; 8]);
        let t = TemplateSummary::parse(&g).unwrap();
        assert_eq!(
            (t.variant, t.entity_name.as_str(), t.payload_bytes),
            (TemplateVariant::Genometp, "Ent", 8)
        );
        for n in 0..g.len() - 8 {
            assert!(TemplateSummary::parse(&g[..n]).is_err(), "{n}");
        }
    }

    #[test]
    fn parses_raw_variant_name_and_rejects_garbage() {
        let mut b = vec![2, 0, 0, 0, 42, 0];
        b.extend([0u8; 36]);
        b.extend(4u16.to_le_bytes());
        b.extend(b"Name");
        b.extend([0; 8]);
        let t = TemplateSummary::parse(&b).unwrap();
        assert_eq!(
            (t.variant, t.entity_name.as_str()),
            (TemplateVariant::Raw, "Name")
        );
        for n in 0..b.len() - 8 {
            assert!(TemplateSummary::parse(&b[..n]).is_err(), "{n}");
        }
        assert!(
            TemplateSummary::parse(b"garbage garbage garbage garbage garbage garbage!!").is_err()
        );
    }
}
