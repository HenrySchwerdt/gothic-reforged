//! Installing / uninstalling packages into a mods folder.
use crate::modlist::ModList;
use crate::package::{Package, SignatureStatus};
use anyhow::{Context, Result, bail, ensure};
use semver::Version;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct InstallOptions {
    /// Replace an installed mod even with an older or equal version.
    pub force: bool,
    /// Require a valid signature by one of these public keys (hex).
    pub trusted_keys: Vec<String>,
    /// Refuse unsigned packages.
    pub require_signature: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallReport {
    pub id: String,
    pub version: String,
    pub replaced: Option<String>,
    pub signature: SignatureStatus,
    /// Dependencies not (or not suitably) installed.
    pub missing_dependencies: Vec<String>,
}

fn installed_version(dir: &Path) -> Option<Version> {
    let text = std::fs::read_to_string(dir.join("mod.toml")).ok()?;
    let m = gothic_mods::Manifest::parse(&text).ok()?;
    m.version().ok()
}

/// Install a package (bytes of a `.grmod`) into `mods_dir`: verify, validate the extracted
/// copy in a staging folder, then swap it in. The mod is added to the mod list (enabled).
pub fn install(package: &[u8], mods_dir: &Path, opts: &InstallOptions) -> Result<InstallReport> {
    let pkg = Package::open(package)?;
    let signature = pkg.signature_status();
    match &signature {
        SignatureStatus::Invalid { reason } => bail!("invalid signature: {reason}"),
        SignatureStatus::Unsigned if opts.require_signature || !opts.trusted_keys.is_empty() => {
            bail!("package is not signed")
        }
        SignatureStatus::Valid { signer }
            if !opts.trusted_keys.is_empty()
                && !opts
                    .trusted_keys
                    .iter()
                    .any(|k| k.eq_ignore_ascii_case(signer)) =>
        {
            bail!("package is signed by an untrusted key {signer}")
        }
        _ => {}
    }
    let id = pkg.manifest.id.clone();
    ensure!(
        !id.is_empty() && crate::package::safe_path(&id) && !id.contains('/'),
        "bad mod id {id:?}"
    );
    let new_version = Version::parse(&pkg.manifest.version)?;
    std::fs::create_dir_all(mods_dir)?;
    let target = mods_dir.join(&id);
    let old = installed_version(&target);
    if let Some(old) = &old
        && *old >= new_version
        && !opts.force
    {
        bail!("{id} {old} is installed; {new_version} is not newer (use --force)");
    }
    let staging = mods_dir.join(format!(".staging-{id}"));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    pkg.extract(&staging)?;
    let report = crate::validate_dir(&staging);
    if !report.ok() || report.id != id {
        let _ = std::fs::remove_dir_all(&staging);
        bail!(
            "package content does not validate{}:\n{}",
            if report.id != id {
                format!(" (manifest id {} vs package id {id})", report.id)
            } else {
                String::new()
            },
            report.to_text()
        );
    }
    if target.exists() {
        std::fs::remove_dir_all(&target).with_context(|| format!("removing old {id}"))?;
    }
    std::fs::rename(&staging, &target)?;
    let mut list = ModList::load(mods_dir)?;
    list.set_enabled(&id, true);
    list.save()?;
    let missing_dependencies = list
        .entries()
        .iter()
        .find(|e| e.id == id)
        .map(|e| e.problems.clone())
        .unwrap_or_default();
    Ok(InstallReport {
        id,
        version: new_version.to_string(),
        replaced: old.map(|v| v.to_string()),
        signature,
        missing_dependencies,
    })
}

pub fn uninstall(mods_dir: &Path, id: &str) -> Result<()> {
    ensure!(
        crate::package::safe_path(id) && !id.contains('/'),
        "bad mod id {id:?}"
    );
    let dir = mods_dir.join(id);
    ensure!(dir.join("mod.toml").is_file(), "{id} is not installed");
    std::fs::remove_dir_all(&dir)?;
    let mut list = ModList::load(mods_dir)?;
    list.forget(id);
    list.save()
}
