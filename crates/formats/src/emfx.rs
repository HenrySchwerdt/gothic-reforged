//! Shared bounded reader for the chunked "FXA " (actor) and "LMA " (motion) containers
//! embedded in Gothic 3 `.xact` / `.xmot` GENOMFLE resources. Independent implementation
//! from byte observations; see `docs/research/ACTOR_ANIMATION_FORMAT.md`.
use anyhow::{Context, Result, bail, ensure};

pub(crate) const MAX_NAME: usize = 1024;

/// One `id, size, version` chunk. `data` excludes the 12-byte chunk header.
#[derive(Debug, Clone, Copy)]
pub struct Chunk<'a> {
    pub id: u32,
    pub version: u32,
    pub data: &'a [u8],
}

/// Located container: payload after the format magic and its version bytes.
pub struct Container<'a> {
    pub hi: u8,
    pub lo: u8,
    pub chunks: Vec<Chunk<'a>>,
    /// Bytes inside the declared container that did not form a whole chunk.
    pub leftover: usize,
}

/// Search window for the container magic inside the Genome body (the preceding
/// resource header is variable, 36..78 bytes observed).
const SEARCH_WINDOW: usize = 512;

/// Locate `magic` (4 bytes) in the first bytes of `body` and walk its chunks.
/// `header_len` is the number of version bytes after the magic (FXA 2, LMA 3).
/// `sized` selects the `gena <u16> <u32 size>` prefix used by actors; motions run to EOF.
pub fn read_container<'a>(
    body: &'a [u8],
    magic: &[u8; 4],
    header_len: usize,
    sized: bool,
) -> Result<Container<'a>> {
    let window = &body[..body.len().min(SEARCH_WINDOW)];
    let at = window
        .windows(4)
        .position(|w| w == magic)
        .with_context(|| {
            format!(
                "container magic {:?} not found",
                String::from_utf8_lossy(magic)
            )
        })?;
    let mut end = body.len();
    if sized {
        ensure!(
            at >= 10 && &body[at - 10..at - 6] == b"gena",
            "missing gena container prefix"
        );
        let size = u32::from_le_bytes(body[at - 4..at].try_into()?) as usize;
        end = end.min(at.saturating_add(4).saturating_add(size));
    }
    ensure!(end >= at + 4 + header_len, "container truncated");
    let hi = body[at + 4];
    let lo = body[at + 5];
    ensure!(hi == 1, "unsupported container version {hi}.{lo}");
    let mut p = at + 4 + header_len;
    let mut chunks = Vec::new();
    while p + 12 <= end {
        let id = u32::from_le_bytes(body[p..p + 4].try_into()?);
        let size = u32::from_le_bytes(body[p + 4..p + 8].try_into()?) as usize;
        let version = u32::from_le_bytes(body[p + 8..p + 12].try_into()?);
        let mut size = size;
        // Exporter info chunk: three strings after two words. A few resources declare a stale
        // size (observed 128 for 152 real bytes), so derive the length from its structure.
        if magic == b"FXA "
            && id == 16
            && version == 1
            && !plausible_chunk(body, p + 12 + size.min(end - p - 12), end)
            && let Some(real) = info_chunk_len(&body[p + 12..end])
            && plausible_chunk(body, p + 12 + real, end)
        {
            size = real;
        }
        if size > end - p - 12 {
            break;
        }
        ensure!(chunks.len() < 1_000_000, "chunk count exceeds ceiling");
        chunks.push(Chunk {
            id,
            version,
            data: &body[p + 12..p + 12 + size],
        });
        p += 12 + size;
    }
    Ok(Container {
        hi,
        lo,
        chunks,
        leftover: end - p,
    })
}

/// Bounds-checked little-endian cursor.
pub(crate) struct Reader<'a> {
    pub b: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Self { b }
    }
    pub fn len(&self) -> usize {
        self.b.len()
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        ensure!(self.b.len() >= n, "truncated chunk");
        let (h, t) = self.b.split_at(n);
        self.b = t;
        Ok(h)
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    pub fn f32(&mut self) -> Result<f32> {
        let v = f32::from_le_bytes(self.take(4)?.try_into()?);
        ensure!(v.is_finite(), "non-finite float");
        Ok(v)
    }
    /// Float that may legitimately hold uninitialised exporter bytes; non-finite becomes 0.
    pub fn f32_lenient(&mut self) -> Result<f32> {
        let v = f32::from_le_bytes(self.take(4)?.try_into()?);
        Ok(if v.is_finite() { v } else { 0.0 })
    }
    /// Reads a float, counting non-finite values in `bad` and returning 0 for them.
    pub fn f32_lenient_flag(&mut self, bad: &mut usize) -> Result<f32> {
        let v = f32::from_le_bytes(self.take(4)?.try_into()?);
        if v.is_finite() {
            Ok(v)
        } else {
            *bad += 1;
            Ok(0.0)
        }
    }
    pub fn vec3(&mut self) -> Result<[f32; 3]> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }
    pub fn quat(&mut self) -> Result<[f32; 4]> {
        Ok([self.f32()?, self.f32()?, self.f32()?, self.f32()?])
    }
    /// `u32 length + bytes` (no terminator), Latin-1.
    pub fn string(&mut self) -> Result<String> {
        let n = self.u32()? as usize;
        if n > MAX_NAME {
            bail!("name length {n} exceeds ceiling");
        }
        Ok(self.take(n)?.iter().map(|b| char::from(*b)).collect())
    }
}

/// Structural length of an info chunk: `u32, u32` then three `u32 length + bytes` strings.
fn info_chunk_len(data: &[u8]) -> Option<usize> {
    let mut at = 8usize;
    for _ in 0..3 {
        let n = u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?) as usize;
        if n > MAX_NAME {
            return None;
        }
        at = at.checked_add(4)?.checked_add(n)?;
    }
    (at <= data.len()).then_some(at)
}

/// Whether a chunk header at `at` looks real (small id/version, size within the container).
fn plausible_chunk(body: &[u8], at: usize, end: usize) -> bool {
    if at + 12 > end {
        // Exactly at the end (or inside the 4-byte tail) is acceptable.
        return at >= end.saturating_sub(12);
    }
    let word = |o: usize| u32::from_le_bytes(body[at + o..at + o + 4].try_into().unwrap_or([0; 4]));
    word(0) <= 16 && word(8) <= 16 && (word(4) as usize) <= end - at - 12
}
