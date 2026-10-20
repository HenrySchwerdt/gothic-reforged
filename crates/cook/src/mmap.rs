//! Read-only memory-mapped (or owned, 16-byte aligned) byte blobs. This is the only place
//! in the crate that uses `unsafe`: mapping a file is inherently unsafe because another
//! process could truncate it. Cache files are written once and replaced by rename.
#![allow(unsafe_code)]

use anyhow::{Context, Result};
use std::path::Path;

/// Immutable bytes whose base address is at least 16-byte aligned.
pub enum Blob {
    Mapped(memmap2::Mmap),
    /// Backing store is `u128`s so the byte view is 16-byte aligned.
    Owned {
        words: Vec<u128>,
        len: usize,
    },
}

impl Blob {
    pub fn map(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
        if file.metadata()?.len() == 0 {
            return Ok(Self::from_bytes(&[]));
        }
        // SAFETY: the cache is only ever replaced by renaming a fully written temp file.
        let map = unsafe { memmap2::Mmap::map(&file) }
            .with_context(|| format!("mmap {}", path.display()))?;
        Ok(Self::Mapped(map))
    }

    /// Copy bytes into an aligned owned blob (tests, small files).
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut words = vec![0u128; bytes.len().div_ceil(16)];
        bytemuck::cast_slice_mut::<u128, u8>(&mut words)[..bytes.len()].copy_from_slice(bytes);
        Self::Owned {
            words,
            len: bytes.len(),
        }
    }

    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Mapped(m) => m,
            Self::Owned { words, len } => &bytemuck::cast_slice::<u128, u8>(words)[..*len],
        }
    }
}
