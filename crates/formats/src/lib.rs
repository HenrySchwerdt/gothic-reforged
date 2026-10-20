//! Read-only, bounded Genome G3V0 archive reader. No original game assets embedded.
pub mod emfx;
pub mod gdini;
pub mod genome;
pub mod igl;
pub mod info;
pub mod propstream;
pub mod props;
pub mod quest;
pub mod stringtable;
pub mod template;
pub mod terrain;
pub mod vfs;
pub mod world_node;
pub mod xact;
pub mod xcmsh;
pub mod ximg;
pub mod xlmp;
pub mod xlmsh;
pub mod xmot;
pub mod xnvmsh;
pub mod xshmat;
use anyhow::{Context, Result, bail, ensure};
use flate2::{Decompress, FlushDecompress, Status};
use serde::Serialize;
use std::{
    collections::HashSet,
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
};

const MAX_ENTRIES: usize = 1_000_000;
const MAX_STRING: usize = 16_384;
const MAX_DEPTH: usize = 64;
pub const MAX_PAYLOAD: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct PakHeader {
    pub version: u32,
    pub revision: u32,
    pub encryption: u32,
    pub compression: u32,
    pub reserved: u32,
    pub files_offset: u64,
    pub folders_offset: u64,
    pub volume_offset: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PakEntry {
    pub path: String,
    pub offset: u64,
    pub stored_size: u64,
    pub size: u64,
    pub encryption: u32,
    pub compression: u32,
    pub attributes: u32,
}

impl PakEntry {
    /// Known reference flag; additional build-specific flags remain opaque.
    pub fn deleted(&self) -> bool {
        self.attributes & 0x8000 != 0
    }

    /// Observed patch record with no backing payload. Deletion semantics are
    /// intentionally not inferred from this byte layout alone.
    pub fn payloadless(&self) -> bool {
        self.offset == 0 && self.stored_size == 0 && self.size == 0
    }
}

pub struct PakArchive<R = BufReader<File>> {
    reader: R,
    pub header: PakHeader,
    pub entries: Vec<PakEntry>,
}

impl PakArchive {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::read(BufReader::new(
            File::open(path.as_ref())
                .with_context(|| format!("open {}", path.as_ref().display()))?,
        ))
    }
}

impl<R: Read + Seek> PakArchive<R> {
    pub fn read(mut reader: R) -> Result<Self> {
        let len = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(0))?;
        ensure!(len >= 48, "truncated G3V0 header");
        let version = u32le(&mut reader)?;
        ensure!(u32le(&mut reader)? == 0x30563347, "not a G3V0 archive");
        let revision = u32le(&mut reader)?;
        let encryption = u32le(&mut reader)?;
        let compression = u32le(&mut reader)?;
        let reserved = u32le(&mut reader)?;
        ensure!(
            version == 0 && revision == 0,
            "unsupported G3V0 version/revision: {version}/{revision}"
        );
        ensure!(encryption == 0, "encrypted archive unsupported");
        let header = PakHeader {
            version,
            revision,
            encryption,
            compression,
            reserved,
            files_offset: u64le(&mut reader)?,
            folders_offset: u64le(&mut reader)?,
            volume_offset: u64le(&mut reader)?,
        };
        ensure!(
            (48..len).contains(&header.files_offset),
            "table offset out of bounds"
        );
        ensure!(
            len - header.files_offset <= 64 * 1024 * 1024,
            "archive table exceeds 64 MiB metadata ceiling"
        );
        ensure!(
            header.folders_offset < len && header.volume_offset < len,
            "header offset out of bounds"
        );
        reader.seek(SeekFrom::Start(header.files_offset))?;
        let mut entries = Vec::new();
        let mut count = 0;
        read_entry(
            &mut reader,
            len,
            header.files_offset,
            0,
            &mut count,
            &mut entries,
        )?;
        let mut paths = HashSet::new();
        for entry in &entries {
            ensure!(
                paths.insert(entry.path.to_lowercase()),
                "duplicate archive path: {}",
                entry.path
            );
        }
        Ok(Self {
            reader,
            header,
            entries,
        })
    }

    /// Fetch one member lazily; strict size ceiling applies before decompression.
    pub fn read_payload(&mut self, index: usize) -> Result<Vec<u8>> {
        let e = self
            .entries
            .get(index)
            .context("member index out of bounds")?;
        ensure!(!e.deleted(), "member is deleted");
        ensure!(
            !e.payloadless(),
            "member has no backing payload; patch semantics unresolved"
        );
        ensure!(e.encryption == 0, "encrypted member unsupported");
        ensure!(
            e.size <= MAX_PAYLOAD && e.stored_size <= MAX_PAYLOAD,
            "member exceeds 256 MiB payload ceiling"
        );
        self.reader.seek(SeekFrom::Start(e.offset))?;
        let stored_size = usize::try_from(e.stored_size)?;
        let mut stored = vec![0; stored_size];
        self.reader.read_exact(&mut stored)?;
        let result = match e.compression {
            0 => stored,
            2 => {
                let mut decoder = Decompress::new(true);
                let mut result = Vec::with_capacity(usize::try_from(e.size + 1)?);
                let status =
                    decoder.decompress_vec(&stored, &mut result, FlushDecompress::Finish)?;
                ensure!(
                    status == Status::StreamEnd,
                    "incomplete zlib stream or decoded size exceeded"
                );
                ensure!(
                    decoder.total_in() == e.stored_size,
                    "zlib stream has trailing or unread bytes"
                );
                result
            }
            other => bail!("unsupported compression {other}"),
        };
        ensure!(
            result.len() as u64 == e.size,
            "decoded member size mismatch"
        );
        Ok(result)
    }
}

fn read_entry<R: Read + Seek>(
    r: &mut R,
    len: u64,
    table_start: u64,
    depth: usize,
    count: &mut usize,
    entries: &mut Vec<PakEntry>,
) -> Result<()> {
    ensure!(depth <= MAX_DEPTH, "directory nesting exceeds limit");
    *count += 1;
    ensure!(*count <= MAX_ENTRIES, "archive entry count exceeds limit");
    let mut times = [0; 32];
    r.read_exact(&mut times)?;
    let attributes = u32le(r)?;
    if attributes & 0x10 != 0 {
        let _directory = string(r)?;
        for _ in 0..2 {
            let children = u32le(r)? as usize;
            ensure!(
                children <= MAX_ENTRIES - *count,
                "directory child count exceeds limit"
            );
            for _ in 0..children {
                read_entry(r, len, table_start, depth + 1, count, entries)?;
            }
        }
    } else {
        let offset = u64le(r)?;
        let stored_size = u64le(r)?;
        let size = u64le(r)?;
        let encryption = u32le(r)?;
        let compression = u32le(r)?;
        let path = normalize_path(&string(r)?)?;
        let _comment = string(r)?;
        let end = offset
            .checked_add(stored_size)
            .context("member range overflow")?;
        ensure!(
            (offset == 0 && stored_size == 0 && size == 0)
                || (end <= len && end <= table_start && offset >= 48),
            "payload outside data region: {path}"
        );
        entries.push(PakEntry {
            path,
            offset,
            stored_size,
            size,
            encryption,
            compression,
            attributes,
        });
    }
    Ok(())
}

/// Archive names are Latin-1 bytes; reject paths unsafe on Windows and Unix.
pub fn normalize_path(path: &str) -> Result<String> {
    let path = path.replace('\\', "/");
    ensure!(
        !path.is_empty() && !path.starts_with('/'),
        "empty or absolute member path"
    );
    for part in path.split('/') {
        ensure!(
            !part.is_empty() && part != "." && part != "..",
            "unsafe member path"
        );
        ensure!(
            !part
                .chars()
                .any(|c| c.is_control() || ":<>\"|?*".contains(c)),
            "invalid member path character"
        );
        ensure!(!part.ends_with([' ', '.']), "ambiguous member path");
        let base = part
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        ensure!(
            !matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                && !(base.len() == 4
                    && (base.starts_with("COM") || base.starts_with("LPT"))
                    && matches!(base.as_bytes()[3], b'1'..=b'9')),
            "reserved device member path"
        );
    }
    Ok(path)
}

fn u32le(r: &mut impl Read) -> Result<u32> {
    let mut b = [0; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}
fn u64le(r: &mut impl Read) -> Result<u64> {
    let mut b = [0; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}
fn string(r: &mut impl Read) -> Result<String> {
    let length = u32le(r)? as usize;
    ensure!(length <= MAX_STRING, "archive string exceeds limit");
    let mut bytes = vec![0; length];
    r.read_exact(&mut bytes)?;
    if length > 0 {
        let mut nul = [0];
        r.read_exact(&mut nul)?;
        ensure!(nul == [0], "missing string terminator");
    }
    Ok(bytes.into_iter().map(char::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn fixture(compression: u32, payload: &[u8], size: u64) -> Vec<u8> {
        let table = 48 + payload.len() as u64;
        let mut b = Vec::new();
        for n in [0, 0x30563347, 0, 0, 2, 0] {
            b.extend(u32::to_le_bytes(n));
        }
        for n in [table, table, table] {
            b.extend(n.to_le_bytes());
        }
        b.extend(payload);
        b.extend([0; 32]);
        b.extend(0x10u32.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        b.extend([0; 32]);
        b.extend(0x20000u32.to_le_bytes());
        for n in [48, payload.len() as u64, size] {
            b.extend(n.to_le_bytes());
        }
        b.extend(0u32.to_le_bytes());
        b.extend(compression.to_le_bytes());
        b.extend(8u32.to_le_bytes());
        b.extend(b"test.txt\0");
        b.extend(0u32.to_le_bytes());
        b
    }
    #[test]
    fn reads_plain_member() -> Result<()> {
        let mut a = PakArchive::read(Cursor::new(fixture(0, b"hello", 5)))?;
        assert_eq!(a.entries[0].path, "test.txt");
        assert_eq!(a.read_payload(0)?, b"hello");
        Ok(())
    }
    #[test]
    fn reads_zlib_and_rejects_wrong_size() -> Result<()> {
        use std::io::Write;
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(b"hello")?;
        let data = z.finish()?;
        assert_eq!(
            PakArchive::read(Cursor::new(fixture(2, &data, 5)))?.read_payload(0)?,
            b"hello"
        );
        assert!(
            PakArchive::read(Cursor::new(fixture(2, &data, 4)))?
                .read_payload(0)
                .is_err()
        );
        Ok(())
    }
    #[test]
    fn truncated_input_never_panics() {
        let bytes = fixture(0, b"hello", 5);
        for n in 0..bytes.len() {
            assert!(
                PakArchive::read(Cursor::new(&bytes[..n])).is_err(),
                "length {n}"
            );
        }
    }
    #[test]
    fn rejects_hostile_paths() {
        for path in [
            "../a",
            "/a",
            "C:\\x",
            "a/../../b",
            "a//b",
            "a/NUL.txt",
            "a/CON",
            "a/b.",
            "a\0b",
        ] {
            assert!(normalize_path(path).is_err(), "{path}");
        }
        assert_eq!(normalize_path("a\\ä.txt").unwrap(), "a/ä.txt");
    }
    #[test]
    fn rejects_bad_header_and_range() {
        let mut bytes = fixture(0, b"hello", 5);
        bytes[4] = 0;
        assert!(PakArchive::read(Cursor::new(bytes)).is_err());
        let mut bytes = fixture(0, b"hello", 5);
        bytes[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(PakArchive::read(Cursor::new(bytes)).is_err());
    }

    #[test]
    fn rejects_truncated_zlib_even_when_decoded_length_matches() -> Result<()> {
        use std::io::Write;
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(b"hello")?;
        let compressed = z.finish()?;
        for n in 0..compressed.len() {
            let mut pak = PakArchive::read(Cursor::new(fixture(2, &compressed[..n], 5)))?;
            assert!(pak.read_payload(0).is_err(), "truncated at {n}");
        }
        Ok(())
    }

    #[test]
    fn preserves_but_does_not_extract_payloadless_patch_records() -> Result<()> {
        let mut bytes = fixture(0, &[], 0);
        // Header + root metadata/name/counts + file metadata -> payload offset.
        bytes[132..140].copy_from_slice(&0u64.to_le_bytes());
        let mut pak = PakArchive::read(Cursor::new(bytes))?;
        assert!(pak.entries[0].payloadless());
        assert!(pak.read_payload(0).is_err());
        Ok(())
    }
}
