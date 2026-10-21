//! Validation of a mod folder: manifests, every patch file (parsed and applied onto an empty
//! base, so typos and type errors show up), script compilation, file types.
use crate::ALLOWED_EXTENSIONS;
use gothic_mods::{LoadOrder, LoadedMod, ModData, apply};
use gothic_mods_script::{ScriptConfig, ScriptHost, ScriptManifest};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModKind {
    Data,
    Script,
    Both,
    /// Only a manifest.
    Empty,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ValidationReport {
    pub id: String,
    pub version: String,
    pub kind: Option<ModKind>,
    pub patch_files: usize,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ValidationReport {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn to_text(&self) -> String {
        let mut s = format!(
            "{} {} ({:?}, {} patch files): {}\n",
            self.id,
            self.version,
            self.kind.unwrap_or(ModKind::Empty),
            self.patch_files,
            if self.ok() { "OK" } else { "INVALID" }
        );
        for e in &self.errors {
            s.push_str(&format!("  ERROR {e}\n"));
        }
        for w in &self.warnings {
            s.push_str(&format!("  WARN  {w}\n"));
        }
        s
    }
}

/// Every file under `dir`, relative, sorted, `/`-separated.
pub fn files(dir: &Path) -> std::io::Result<Vec<String>> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
        for e in std::fs::read_dir(dir)? {
            let e = e?;
            let p = e.path();
            let ty = e.file_type()?;
            if ty.is_symlink() {
                continue;
            }
            if ty.is_dir() {
                walk(root, &p, out)?;
            } else {
                let rel: PathBuf = p.strip_prefix(root).unwrap_or(&p).to_path_buf();
                out.push(
                    rel.components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/"),
                );
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out)?;
    out.sort();
    Ok(out)
}

pub fn extension_allowed(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| ALLOWED_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// Languages of the game's string table (a string in another language is an error).
pub const LANGUAGES: &[&str] = &[
    "English",
    "German",
    "French",
    "Italian",
    "Polish",
    "Russian",
    "Czech",
    "Hungarian",
    "Spanish",
];

/// No records, but the string table's languages, so string operations can be checked.
pub fn empty_base() -> ModData {
    let text = format!(
        "[LocAdmin_Languages]\r\nLanguages={}\r\nCurrentLanguage=English\r\n[LocAdmin_Strings]\r\n",
        LANGUAGES.join(";")
    );
    let mut bytes = vec![0xFF, 0xFE];
    for u in text.encode_utf16() {
        bytes.extend(u.to_le_bytes());
    }
    let strings = gothic_formats::stringtable::StringTable::parse(&bytes).unwrap_or_default();
    ModData::new(&[], &[], strings, &[], Vec::new())
}

pub fn validate_dir(dir: &Path) -> ValidationReport {
    let mut r = ValidationReport::default();
    let text = match std::fs::read_to_string(dir.join("mod.toml")) {
        Ok(t) => t,
        Err(e) => {
            r.errors.push(format!("mod.toml: {e}"));
            return r;
        }
    };
    let script_manifest = match ScriptManifest::parse(&text) {
        Ok(m) => Some(m),
        Err(e) => {
            r.errors.push(format!("{e:#}"));
            None
        }
    };
    // Data side: manifest (semver, dependencies) and patches.
    let loaded = match LoadedMod::load(dir) {
        Ok(m) => Some(m),
        Err(e) => {
            r.errors.push(format!("{e:#}"));
            None
        }
    };
    if let Some(m) = &loaded {
        r.id = m.id().to_string();
        r.version = m.manifest.info.version.clone();
        r.patch_files = m.patches.len();
        // Apply onto an empty base: patches of base-game records cannot be checked without
        // the game data ("no such record"), everything else (unknown fields, wrong types,
        // duplicate adds, bad strings) is a real error.
        let (_, report) = apply(
            empty_base(),
            &LoadOrder {
                order: vec![m.clone()],
                problems: Vec::new(),
            },
        );
        for e in report.errors {
            if e.message == "no such record" {
                continue;
            }
            r.errors.push(format!(
                "{}: {} {}: {}",
                e.file,
                e.kind.name(),
                e.record,
                e.message
            ));
        }
        if !m.manifest.dependencies.is_empty() {
            r.warnings.push(format!(
                "depends on {}",
                m.manifest
                    .dependencies
                    .iter()
                    .map(|(k, v)| format!("{k} {v}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    // Script side.
    let has_script = script_manifest.as_ref().is_some_and(|m| m.script.is_some());
    if has_script {
        let mut host = ScriptHost::new(ScriptConfig::default());
        if let Err(e) = host.load_dir(dir) {
            r.errors.push(format!("script: {e:#}"));
        }
        for e in host.take_errors() {
            r.errors.push(format!("script {}: {}", e.during, e.message));
        }
    }
    let has_data = r.patch_files > 0;
    r.kind = Some(match (has_data, has_script) {
        (true, true) => ModKind::Both,
        (true, false) => ModKind::Data,
        (false, true) => ModKind::Script,
        (false, false) => ModKind::Empty,
    });
    if r.kind == Some(ModKind::Empty) {
        r.warnings
            .push("no patches/ and no [script]: the mod does nothing".into());
    }
    match files(dir) {
        Ok(fs) => {
            for f in fs {
                if !extension_allowed(&f) {
                    r.errors.push(format!(
                        "{f}: file type not allowed in mods (allowed: {})",
                        crate::ALLOWED_EXTENSIONS.join(", ")
                    ));
                }
            }
        }
        Err(e) => r.errors.push(format!("listing files: {e}")),
    }
    r
}
