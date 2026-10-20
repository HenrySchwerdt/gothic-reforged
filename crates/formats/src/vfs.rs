//! Read-only virtual file system over `Data/*.pak` plus `*.pNN` patch overlays.
//!
//! Layer order: every `.pak` (sorted by name), then every `.p00..p99` patch
//! (sorted by number, then name). Later layers replace earlier entries of the
//! same case-insensitive path. A payloadless patch record (offset, stored and
//! decoded size all zero) is treated as *not an override*: the lower layer's
//! entry stays visible, and the record is reported in [`VirtualFileSystem::ignored_payloadless`]
//! (see docs/research/WORLD_SURVEY.md for why).
use crate::PakArchive;
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::Path,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Slot {
    pub archive: usize,
    pub entry: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct IgnoredRecord {
    pub archive: String,
    pub path: String,
    /// True when a lower layer holds the same path (record shadowed nothing).
    pub base_exists: bool,
}

#[derive(Default)]
pub struct VirtualFileSystem {
    archives: Vec<PakArchive>,
    names: Vec<String>,
    by_path: HashMap<String, Slot>,
    by_basename: HashMap<String, Vec<String>>,
    pub overridden: usize,
    pub ignored_payloadless: Vec<IgnoredRecord>,
    /// Archives that could not be opened (e.g. CPT), with the reason.
    pub skipped: Vec<(String, String)>,
}

impl VirtualFileSystem {
    /// Mount every `*.pak` / `*.pNN` directly inside `data_dir` (no recursion).
    pub fn open_data_dir(data_dir: &Path) -> Result<Self> {
        ensure!(data_dir.is_dir(), "not a directory: {}", data_dir.display());
        let mut paks = Vec::new();
        let mut patches = BTreeMap::new();
        for entry in fs::read_dir(data_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            let ext = path
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            if ext == "pak" {
                paks.push((name, path));
            } else if ext.len() == 3
                && ext.starts_with('p')
                && ext[1..].bytes().all(|b| b.is_ascii_digit())
            {
                patches.insert((ext, name), path);
            }
        }
        paks.sort();
        let mut vfs = Self::default();
        for (name, path) in paks {
            vfs.mount_file(name, &path);
        }
        for ((_, name), path) in patches {
            vfs.mount_file(name, &path);
        }
        Ok(vfs)
    }

    fn mount_file(&mut self, name: String, path: &Path) {
        match PakArchive::open(path) {
            Ok(archive) => self.mount(name, archive),
            Err(error) => self.skipped.push((name, format!("{error:#}"))),
        }
    }

    /// Mount an already opened archive on top of the current layers.
    pub fn mount(&mut self, name: String, archive: PakArchive) {
        let layer = self.archives.len();
        for (index, entry) in archive.entries.iter().enumerate() {
            let key = entry.path.to_ascii_lowercase();
            if entry.payloadless() {
                self.ignored_payloadless.push(IgnoredRecord {
                    archive: name.clone(),
                    path: entry.path.clone(),
                    base_exists: self.by_path.contains_key(&key),
                });
                continue;
            }
            let slot = Slot {
                archive: layer,
                entry: index,
            };
            if self.by_path.insert(key.clone(), slot).is_some() {
                self.overridden += 1;
            } else {
                let base = key.rsplit('/').next().unwrap_or(&key).to_owned();
                self.by_basename.entry(base).or_default().push(key);
            }
        }
        self.archives.push(archive);
        self.names.push(name);
    }

    pub fn archive_names(&self) -> &[String] {
        &self.names
    }
    pub fn len(&self) -> usize {
        self.by_path.len()
    }
    pub fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }

    /// Resolve a full member path (any slash style, case-insensitive).
    pub fn resolve(&self, path: &str) -> Option<Slot> {
        self.by_path
            .get(&path.replace('\\', "/").to_ascii_lowercase())
            .copied()
    }

    /// All visible paths (lowercase) sharing a basename (case-insensitive).
    pub fn paths_for_basename(&self, basename: &str) -> &[String] {
        self.by_basename
            .get(&basename.to_ascii_lowercase())
            .map_or(&[], Vec::as_slice)
    }

    /// Resolve by full path, else by unique-or-first basename. Ambiguous
    /// basenames pick the lexicographically first path for determinism.
    pub fn resolve_name(&self, name: &str) -> Option<Slot> {
        let name = name.replace('\\', "/");
        if let Some(slot) = self.resolve(&name) {
            return Some(slot);
        }
        let base = name.rsplit('/').next().unwrap_or(&name);
        self.paths_for_basename(base)
            .iter()
            .min()
            .and_then(|p| self.by_path.get(p).copied())
    }

    pub fn entry_path(&self, slot: Slot) -> &str {
        &self.archives[slot.archive].entries[slot.entry].path
    }
    pub fn archive_name(&self, slot: Slot) -> &str {
        &self.names[slot.archive]
    }

    pub fn read(&mut self, slot: Slot) -> Result<Vec<u8>> {
        self.archives
            .get_mut(slot.archive)
            .context("slot archive out of range")?
            .read_payload(slot.entry)
    }

    /// Iterate visible `(lowercase path, slot)` pairs in unspecified order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, Slot)> {
        self.by_path.iter().map(|(k, v)| (k.as_str(), *v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Minimal G3V0 archive: plain members, plus zero-size payloadless records.
    fn pak(files: &[(&str, &[u8])], payloadless: &[&str]) -> Vec<u8> {
        let mut payload = Vec::new();
        let mut records = Vec::new();
        for (path, data) in files {
            records.push((*path, 48 + payload.len() as u64, data.len() as u64));
            payload.extend(*data);
        }
        for path in payloadless {
            records.push((*path, 0, 0));
        }
        let table = 48 + payload.len() as u64;
        let mut b = Vec::new();
        for n in [0u32, 0x3056_3347, 0, 0, 0, 0] {
            b.extend(n.to_le_bytes());
        }
        for n in [table; 3] {
            b.extend(n.to_le_bytes());
        }
        b.extend(&payload);
        b.extend([0; 32]);
        b.extend(0x10u32.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend((records.len() as u32).to_le_bytes());
        for (path, offset, size) in records {
            b.extend([0; 32]);
            b.extend(0x20000u32.to_le_bytes());
            for n in [offset, size, size] {
                b.extend(n.to_le_bytes());
            }
            b.extend(0u32.to_le_bytes());
            b.extend(0u32.to_le_bytes());
            b.extend((path.len() as u32).to_le_bytes());
            b.extend(path.as_bytes());
            b.push(0);
            b.extend(0u32.to_le_bytes());
        }
        b
    }

    struct Dir(std::path::PathBuf);
    impl Dir {
        fn new() -> Self {
            static N: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "gothic-vfs-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn patches_override_paks_in_number_order_and_payloadless_is_not_a_deletion() -> Result<()> {
        let dir = Dir::new();
        let write = |name: &str, bytes: Vec<u8>| fs::write(dir.0.join(name), bytes).unwrap();
        write(
            "B.pak",
            pak(&[("dir/Same.txt", b"base"), ("dir/only.txt", b"only")], &[]),
        );
        write("A.pak", pak(&[("other/a.txt", b"a")], &[]));
        // Patch files are named after their base but must sort by patch number.
        write(
            "A.p01",
            pak(
                &[("dir/same.TXT", b"p01")],
                &["dir/only.txt", "new/empty.txt"],
            ),
        );
        write("B.p00", pak(&[("dir/Same.txt", b"p00")], &[]));
        write("junk.cpt", b"opaque".to_vec());
        write("bad.pak", b"not an archive".to_vec());
        let mut vfs = VirtualFileSystem::open_data_dir(&dir.0)?;
        assert_eq!(vfs.archive_names(), ["a.pak", "b.pak", "b.p00", "a.p01"]);
        assert_eq!(vfs.skipped.len(), 1);
        assert_eq!(vfs.skipped[0].0, "bad.pak");
        assert_eq!(vfs.len(), 3);
        assert_eq!(vfs.overridden, 2);
        // Case-insensitive, both slash styles; highest layer wins.
        let slot = vfs.resolve(r"DIR\SAME.txt").unwrap();
        assert_eq!(vfs.archive_name(slot), "a.p01");
        assert_eq!(vfs.read(slot)?, b"p01");
        // The payloadless record neither deletes nor shadows the base member.
        let only = vfs.resolve("dir/only.txt").unwrap();
        assert_eq!(vfs.read(only)?, b"only");
        assert!(vfs.resolve("new/empty.txt").is_none());
        let flags: Vec<_> = vfs
            .ignored_payloadless
            .iter()
            .map(|r| (r.path.as_str(), r.base_exists))
            .collect();
        assert_eq!(flags, [("dir/only.txt", true), ("new/empty.txt", false)]);
        // Basename lookup is case-insensitive and deterministic.
        assert_eq!(vfs.paths_for_basename("A.TXT"), ["other/a.txt"]);
        let by_name = vfs.resolve_name("whatever/path/ONLY.txt").unwrap();
        assert_eq!(vfs.entry_path(by_name), "dir/only.txt");
        assert!(vfs.resolve_name("missing.bin").is_none());
        Ok(())
    }

    #[test]
    fn missing_directory_is_an_error() {
        assert!(VirtualFileSystem::open_data_dir(Path::new("definitely/not/here")).is_err());
    }
}
