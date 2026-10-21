//! Development hot reload: re-read the mods directory when any mod file changed and
//! re-apply everything onto the unmodified base data.
//!
//! Polling based (modification times and sizes of every file below the mods directory), so
//! it needs no file-watcher dependency and behaves the same on every platform. A host calls
//! [`HotReload::poll`] once in a while (e.g. once per second in a dev build).
use crate::apply::{ModReport, apply};
use crate::data::ModData;
use crate::loader::{discover, resolve};
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Base data plus the mods directory being watched.
pub struct HotReload {
    base: ModData,
    dir: PathBuf,
    stamp: Vec<(PathBuf, Option<SystemTime>, u64)>,
}

impl HotReload {
    pub fn new(base: ModData, mods_dir: impl Into<PathBuf>) -> Self {
        Self {
            base,
            dir: mods_dir.into(),
            stamp: Vec::new(),
        }
    }

    /// Load and apply now (and remember the file state).
    pub fn load(&mut self) -> Result<(ModData, ModReport)> {
        self.stamp = stamp(&self.dir);
        let order = resolve(discover(&self.dir)?);
        Ok(apply(self.base.clone(), &order))
    }

    /// `Some` with the re-applied data when a file was added, removed or changed since the
    /// last load.
    pub fn poll(&mut self) -> Option<Result<(ModData, ModReport)>> {
        if stamp(&self.dir) == self.stamp {
            return None;
        }
        Some(self.load())
    }
}

fn stamp(dir: &Path) -> Vec<(PathBuf, Option<SystemTime>, u64)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            match e.metadata() {
                Ok(m) if m.is_dir() => stack.push(p),
                Ok(m) => out.push((p, m.modified().ok(), m.len())),
                Err(_) => {}
            }
        }
    }
    out.sort();
    out
}
