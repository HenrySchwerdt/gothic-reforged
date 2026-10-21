//! Deterministic, read-only installation inventory. Never executes game binaries.
pub mod preview_scene;
pub mod survey;
pub mod world_index;
use anyhow::{Context, Result, ensure};
use gothic_formats::{PakArchive, PakHeader};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufReader, Read},
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize)]
pub struct InstallationReport {
    pub root: String,
    pub archives: Vec<ArchiveSummary>,
    pub binaries: Vec<BinarySummary>,
    pub ini_evidence: Vec<IniEvidence>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ArchiveSummary {
    pub path: String,
    pub size_bytes: u64,
    /// `indexed`, `unsupported_cpt`, or `error`. Indexing does not decode payloads.
    pub status: String,
    pub header: Option<PakHeader>,
    pub entry_count: usize,
    pub deleted_entry_count: usize,
    pub stored_bytes: u64,
    pub uncompressed_bytes: u64,
    pub extensions: BTreeMap<String, usize>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BinarySummary {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
pub struct IniEvidence {
    pub path: String,
    pub line: usize,
    pub section: String,
    pub key: String,
    pub value: String,
}

/// Inventory Data archives, root/bin binaries, and Ini configuration evidence.
/// Missing optional directories produce warnings; inaccessible files are reported
/// independently so one unsupported archive cannot suppress the rest of the scan.
/// Symlinks/reparse points are not followed. Root must be an existing directory.
pub fn scan(root: &Path) -> Result<InstallationReport> {
    ensure!(
        root.is_dir(),
        "installation root is not a directory: {}",
        root.display()
    );
    let root = root.canonicalize().context("resolve installation root")?;
    let mut report = InstallationReport {
        root: root.display().to_string(),
        archives: Vec::new(),
        binaries: Vec::new(),
        ini_evidence: Vec::new(),
        warnings: Vec::new(),
    };
    if let Some(data) = child_dir(&root, "data")? {
        let mut files = Vec::new();
        walk(&data, &mut files, &mut report.warnings, 0)?;
        files.sort();
        for path in files {
            let ext = extension(&path);
            if ext == "pak" || is_patch_extension(&ext) || ext == "cpt" {
                report.archives.push(inspect_archive(&root, &path, &ext)?);
            }
        }
    } else {
        report
            .warnings
            .push("Data directory missing; this may not be a Gothic 3 installation".into());
    }
    let mut binaries = regular_files(&root)?;
    if let Some(bin) = child_dir(&root, "bin")? {
        binaries.extend(regular_files(&bin)?);
    }
    binaries.sort();
    for path in binaries {
        if matches!(extension(&path).as_str(), "exe" | "dll") {
            match hash_binary(&root, &path) {
                Ok(binary) => report.binaries.push(binary),
                Err(error) => report
                    .warnings
                    .push(format!("{}: {error:#}", relative(&root, &path))),
            }
        }
    }
    if let Some(ini) = child_dir(&root, "ini")? {
        let mut files = regular_files(&ini)?;
        files.sort();
        for path in files.into_iter().filter(|path| extension(path) == "ini") {
            match ini_evidence(&root, &path) {
                Ok(evidence) => report.ini_evidence.extend(evidence),
                Err(error) => report
                    .warnings
                    .push(format!("{}: {error:#}", relative(&root, &path))),
            }
        }
    }
    report.archives.sort_by(|a, b| a.path.cmp(&b.path));
    report.binaries.sort_by(|a, b| a.path.cmp(&b.path));
    report.warnings.sort();
    Ok(report)
}

fn inspect_archive(root: &Path, path: &Path, ext: &str) -> Result<ArchiveSummary> {
    let mut summary = ArchiveSummary {
        path: relative(root, path),
        size_bytes: fs::metadata(path)?.len(),
        status: "error".into(),
        header: None,
        entry_count: 0,
        deleted_entry_count: 0,
        stored_bytes: 0,
        uncompressed_bytes: 0,
        extensions: BTreeMap::new(),
        error: None,
    };
    if ext == "cpt" {
        summary.status = "unsupported_cpt".into();
        summary.error = Some("CPT format is not implemented; not treated as G3V0 PAK".into());
        return Ok(summary);
    }
    match PakArchive::open(path) {
        Ok(archive) => {
            summary.status = "indexed".into();
            summary.entry_count = archive.entries.len();
            summary.header = Some(archive.header);
            for entry in &archive.entries {
                summary.deleted_entry_count += usize::from(entry.deleted());
                let totals = summary
                    .stored_bytes
                    .checked_add(entry.stored_size)
                    .zip(summary.uncompressed_bytes.checked_add(entry.size));
                let Some((stored_bytes, uncompressed_bytes)) = totals else {
                    // Hostile declared sizes must not suppress the remaining archive inventory.
                    summary.status = "error".into();
                    summary.error = Some("archive member size totals overflow u64".into());
                    summary.stored_bytes = 0;
                    summary.uncompressed_bytes = 0;
                    summary.extensions.clear();
                    return Ok(summary);
                };
                summary.stored_bytes = stored_bytes;
                summary.uncompressed_bytes = uncompressed_bytes;
                *summary
                    .extensions
                    .entry(extension(Path::new(&entry.path)))
                    .or_default() += 1;
            }
        }
        Err(error) => summary.error = Some(format!("{error:#}")),
    }
    Ok(summary)
}

fn hash_binary(root: &Path, path: &Path) -> Result<BinarySummary> {
    let mut reader = BufReader::new(File::open(path)?);
    let size_bytes = reader.get_ref().metadata()?.len();
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(BinarySummary {
        path: relative(root, path),
        size_bytes,
        sha256: format!("{:x}", digest.finalize()),
    })
}

fn ini_evidence(root: &Path, path: &Path) -> Result<Vec<IniEvidence>> {
    const LIMIT: u64 = 4 * 1024 * 1024;
    let file = File::open(path)?;
    ensure!(
        file.metadata()?.len() <= LIMIT,
        "INI exceeds 4 MiB evidence limit"
    );
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= LIMIT,
        "INI exceeds 4 MiB evidence limit"
    );
    let text = if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let little = bytes[0] == 0xff;
        let words: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| {
                if little {
                    u16::from_le_bytes([chunk[0], chunk[1]])
                } else {
                    u16::from_be_bytes([chunk[0], chunk[1]])
                }
            })
            .collect();
        String::from_utf16_lossy(&words)
    } else {
        String::from_utf8_lossy(&bytes)
            .trim_start_matches('\u{feff}')
            .to_owned()
    };
    let mut section = String::new();
    let mut evidence = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_owned();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let lower = key.to_ascii_lowercase();
        let section_lower = section.to_ascii_lowercase();
        if ["version", "language", "subtitle"]
            .iter()
            .any(|needle| lower.contains(needle))
            || ["version", "language"]
                .iter()
                .any(|needle| section_lower.contains(needle))
        {
            evidence.push(IniEvidence {
                path: relative(root, path),
                line: index + 1,
                section: section.clone(),
                key: key.into(),
                value: value.trim().into(),
            });
        }
    }
    Ok(evidence)
}

fn extension(path: &Path) -> String {
    path.extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase()
}
fn is_patch_extension(ext: &str) -> bool {
    ext.len() == 3 && ext.starts_with('p') && ext.as_bytes()[1..].iter().all(u8::is_ascii_digit)
}
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
fn regular_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() && !is_link(&entry.path())? {
            files.push(entry.path());
        }
    }
    Ok(files)
}
fn child_dir(root: &Path, name: &str) -> Result<Option<PathBuf>> {
    let mut children = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(name)
            && entry.file_type()?.is_dir()
            && !is_link(&entry.path())?
        {
            children.push(entry.path());
        }
    }
    children.sort();
    Ok(children.into_iter().next())
}
fn walk(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    warnings: &mut Vec<String>,
    depth: usize,
) -> Result<()> {
    if depth >= 32 {
        warnings.push(format!("directory depth limit reached: {}", dir.display()));
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if is_link(&path)? {
            warnings.push(format!("skipped link/reparse point: {}", path.display()));
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            walk(&path, files, warnings, depth + 1)?;
        } else if kind.is_file() {
            files.push(path);
        }
    }
    Ok(())
}
fn is_link(path: &Path) -> Result<bool> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        Ok(metadata.file_attributes() & 0x400 != 0)
    }
    #[cfg(not(windows))]
    {
        Ok(metadata.file_type().is_symlink())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "gothic-importer-test-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn scan_is_deterministic_and_keeps_unsupported_archives_visible() {
        let fixture = Fixture::new();
        fs::create_dir(fixture.0.join("Data")).unwrap();
        fs::create_dir(fixture.0.join("Ini")).unwrap();
        fs::write(fixture.0.join("Data/Z.p00"), b"invalid").unwrap();
        fs::write(fixture.0.join("Data/A.cpt"), b"opaque").unwrap();
        fs::write(fixture.0.join("Data/ignore.txt"), b"ignore").unwrap();
        fs::write(fixture.0.join("Gothic3.exe"), b"abc").unwrap();
        fs::write(
            fixture.0.join("Ini/ge3.INI"),
            b"[Game]\n;Language=Fake\nTextLanguage=German\n[Version]\nPatch=1.75\n",
        )
        .unwrap();
        let a = scan(&fixture.0).unwrap();
        let b = scan(&fixture.0).unwrap();
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
        assert_eq!(a.archives.len(), 2);
        assert_eq!(a.archives[0].status, "unsupported_cpt");
        assert_eq!(a.archives[1].status, "error");
        assert_eq!(
            a.binaries[0].sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(a.ini_evidence.len(), 2);
        assert_eq!(a.ini_evidence[0].line, 3);
        assert_eq!(fs::read(fixture.0.join("Gothic3.exe")).unwrap(), b"abc");
    }
    #[test]
    fn missing_data_is_a_report_warning_and_missing_root_is_an_error() {
        let fixture = Fixture::new();
        assert_eq!(scan(&fixture.0).unwrap().warnings.len(), 1);
        assert!(scan(&fixture.0.join("missing")).is_err());
    }
    #[test]
    fn patch_extension_is_exact_and_utf16_configuration_is_readable() {
        assert!(is_patch_extension("p00"));
        assert!(is_patch_extension("p99"));
        assert!(!is_patch_extension("pak"));
        assert!(!is_patch_extension("p000"));
        let fixture = Fixture::new();
        let path = fixture.0.join("config.ini");
        let mut bytes = vec![0xff, 0xfe];
        for word in "[Game]\nVoiceLanguage=German".encode_utf16() {
            bytes.extend(word.to_le_bytes());
        }
        fs::write(&path, bytes).unwrap();
        assert_eq!(ini_evidence(&fixture.0, &path).unwrap()[0].value, "German");
    }

    #[test]
    fn hostile_declared_sizes_do_not_suppress_other_archives() {
        let fixture = Fixture::new();
        fs::create_dir(fixture.0.join("Data")).unwrap();
        let mut bytes = Vec::new();
        for word in [0u32, 0x30563347, 0, 0, 1, 0] {
            bytes.extend(word.to_le_bytes());
        }
        for offset in [48u64; 3] {
            bytes.extend(offset.to_le_bytes());
        }
        bytes.extend([0; 32]);
        for word in [0x10u32, 0, 0, 2] {
            bytes.extend(word.to_le_bytes());
        }
        for name in *b"ab" {
            bytes.extend([0; 32]);
            bytes.extend(0x20000u32.to_le_bytes());
            for value in [48u64, 0, u64::MAX] {
                bytes.extend(value.to_le_bytes());
            }
            for word in [0u32, 0, 1] {
                bytes.extend(word.to_le_bytes());
            }
            bytes.extend([name, 0]);
            bytes.extend(0u32.to_le_bytes());
        }
        fs::write(fixture.0.join("Data/A.pak"), bytes).unwrap();
        fs::write(fixture.0.join("Data/B.cpt"), b"opaque").unwrap();
        let report = scan(&fixture.0).unwrap();
        assert_eq!(report.archives.len(), 2);
        assert_eq!(report.archives[0].status, "error");
        assert_eq!(report.archives[1].status, "unsupported_cpt");
    }
}
