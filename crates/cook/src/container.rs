//! Tiny sectioned binary container (`GCK1`). Sections are 16-byte aligned so a mapped file
//! can be viewed as typed slices with `bytemuck` without copying.
//!
//! ```text
//! Header (32 bytes): magic "GCK1", kind u32, version u32, section_count u32, key u64, reserved u64
//! Section table:     section_count * { offset u64, len u64 }
//! Section data:      each at a 16-byte aligned offset
//! ```
use crate::ids::COOKER_VERSION;
use crate::mmap::Blob;
use anyhow::{Result, bail, ensure};
use bytemuck::{Pod, Zeroable};
use std::sync::Arc;

pub const MAGIC: [u8; 4] = *b"GCK1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Kind {
    Mesh = 1,
    Texture = 2,
    Materials = 3,
    Cell = 4,
    Actor = 5,
    Motion = 6,
    Database = 7,
    Collision = 8,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Header {
    magic: [u8; 4],
    kind: u32,
    version: u32,
    sections: u32,
    key: u64,
    reserved: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SectionRec {
    offset: u64,
    len: u64,
}

#[derive(Default)]
pub struct Writer {
    sections: Vec<Vec<u8>>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn raw(&mut self, bytes: Vec<u8>) -> &mut Self {
        self.sections.push(bytes);
        self
    }
    pub fn slice<T: Pod>(&mut self, data: &[T]) -> &mut Self {
        self.raw(bytemuck::cast_slice(data).to_vec())
    }
    pub fn one<T: Pod>(&mut self, value: &T) -> &mut Self {
        self.slice(std::slice::from_ref(value))
    }
    pub fn finish(self, kind: Kind, key: u64) -> Vec<u8> {
        let n = self.sections.len();
        let table_end = 32 + 16 * n;
        let mut offset = table_end.next_multiple_of(16);
        let mut recs = Vec::with_capacity(n);
        for s in &self.sections {
            recs.push(SectionRec {
                offset: offset as u64,
                len: s.len() as u64,
            });
            offset = (offset + s.len()).next_multiple_of(16);
        }
        let mut out = Vec::with_capacity(offset);
        let header = Header {
            magic: MAGIC,
            kind: kind as u32,
            version: COOKER_VERSION,
            sections: n as u32,
            key,
            reserved: 0,
        };
        out.extend_from_slice(bytemuck::bytes_of(&header));
        out.extend_from_slice(bytemuck::cast_slice(&recs));
        for (s, r) in self.sections.iter().zip(&recs) {
            out.resize(r.offset as usize, 0);
            out.extend_from_slice(s);
        }
        out.resize(offset, 0);
        out
    }
}

/// A parsed, validated container over a shared blob.
#[derive(Clone)]
pub struct Container {
    blob: Arc<Blob>,
    recs: Vec<(usize, usize)>,
    pub key: u64,
}

impl Container {
    pub fn parse(blob: Arc<Blob>, expect: Kind) -> Result<Self> {
        let b = blob.bytes();
        ensure!(b.len() >= 32, "cooked file too small");
        let h: Header = bytemuck::pod_read_unaligned(&b[..32]);
        ensure!(h.magic == MAGIC, "bad cooked magic");
        ensure!(h.kind == expect as u32, "unexpected cooked kind {}", h.kind);
        if h.version != COOKER_VERSION {
            bail!("cooked version {} != {}", h.version, COOKER_VERSION);
        }
        let n = h.sections as usize;
        ensure!(n <= 64 && b.len() >= 32 + 16 * n, "bad section table");
        let mut recs = Vec::with_capacity(n);
        for i in 0..n {
            let r: SectionRec = bytemuck::pod_read_unaligned(&b[32 + 16 * i..48 + 16 * i]);
            let (o, l) = (r.offset as usize, r.len as usize);
            ensure!(
                o % 16 == 0 && o.checked_add(l).is_some_and(|e| e <= b.len()),
                "section out of range"
            );
            recs.push((o, l));
        }
        Ok(Self {
            blob,
            recs,
            key: h.key,
        })
    }

    pub fn section_count(&self) -> usize {
        self.recs.len()
    }
    pub fn raw(&self, i: usize) -> &[u8] {
        let (o, l) = self.recs[i];
        &self.blob.bytes()[o..o + l]
    }
    /// Typed zero-copy view; sections are 16-byte aligned and blobs at least 16-byte aligned.
    pub fn slice<T: Pod>(&self, i: usize) -> &[T] {
        bytemuck::try_cast_slice(self.raw(i)).unwrap_or(&[])
    }
    pub fn one<T: Pod>(&self, i: usize) -> Option<T> {
        self.raw(i)
            .get(..std::mem::size_of::<T>())
            .map(bytemuck::pod_read_unaligned)
    }
}
