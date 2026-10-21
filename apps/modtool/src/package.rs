//! `.grmod` packages: a zip of the mod folder with `grmod.toml` (file list with SHA-256 and
//! sizes) and an optional Ed25519 signature `grmod.sig` over `grmod.toml`. Packing is
//! deterministic (sorted entries, fixed timestamps), so the same folder gives the same bytes.
use crate::validate::{extension_allowed, files};
use anyhow::{Context, Result, bail, ensure};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::Path;

pub const MANIFEST: &str = "grmod.toml";
pub const SIGNATURE: &str = "grmod.sig";
pub const FORMAT: u32 = 1;
/// Limits on what a package may contain (checked before extraction).
pub const MAX_FILES: usize = 10_000;
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackagedFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageManifest {
    pub format: u32,
    pub id: String,
    pub version: String,
    /// Hex Ed25519 public key of the signer (present when signed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer: Option<String>,
    pub files: Vec<PackagedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SignatureStatus {
    Unsigned,
    /// Signed and the signature matches the embedded signer key.
    Valid {
        signer: String,
    },
    Invalid {
        reason: String,
    },
}

/// An opened, hash-checked package.
#[derive(Debug, Clone)]
pub struct Package {
    pub manifest: PackageManifest,
    manifest_bytes: Vec<u8>,
    signature: Option<Vec<u8>>,
    /// Mod files by relative path.
    pub files: BTreeMap<String, Vec<u8>>,
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(s: &str) -> Result<Vec<u8>> {
    let s = s.trim();
    ensure!(s.len().is_multiple_of(2), "odd-length hex");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).context("bad hex"))
        .collect()
}

fn sha256(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

/// A new signing key: `(secret hex, public hex)`.
pub fn keygen() -> Result<(String, String)> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
    let key = SigningKey::from_bytes(&seed);
    Ok((hex(&seed), hex(key.verifying_key().as_bytes())))
}

pub fn signing_key(secret_hex: &str) -> Result<SigningKey> {
    let b: [u8; 32] = unhex(secret_hex)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("a secret key is 32 bytes (64 hex digits)"))?;
    Ok(SigningKey::from_bytes(&b))
}

pub fn verifying_key(public_hex: &str) -> Result<VerifyingKey> {
    let b: [u8; 32] = unhex(public_hex)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("a public key is 32 bytes (64 hex digits)"))?;
    VerifyingKey::from_bytes(&b).context("invalid public key")
}

/// A path that stays inside the mod folder (no absolute paths, `..`, backslashes or drive
/// letters).
pub fn safe_path(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && !p.contains('\\')
        && !p.contains(':')
        && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
}

/// Pack a mod folder. The folder must validate first (see [`crate::validate_dir`]).
pub fn pack(dir: &Path, sign: Option<&SigningKey>) -> Result<(PackageManifest, Vec<u8>)> {
    let report = crate::validate_dir(dir);
    if !report.ok() {
        bail!("mod does not validate:\n{}", report.to_text());
    }
    let mut list = Vec::new();
    let mut contents = Vec::new();
    for f in files(dir)? {
        ensure!(extension_allowed(&f), "{f}: file type not allowed");
        ensure!(f != MANIFEST && f != SIGNATURE, "{f} is reserved");
        let bytes = std::fs::read(dir.join(&f)).with_context(|| format!("reading {f}"))?;
        ensure!(bytes.len() as u64 <= MAX_FILE_BYTES, "{f} is too large");
        list.push(PackagedFile {
            path: f.clone(),
            size: bytes.len() as u64,
            sha256: sha256(&bytes),
        });
        contents.push((f, bytes));
    }
    let manifest = PackageManifest {
        format: FORMAT,
        id: report.id.clone(),
        version: report.version.clone(),
        signer: sign.map(|k| hex(k.verifying_key().as_bytes())),
        files: list,
    };
    let manifest_bytes = toml::to_string_pretty(&manifest)?.into_bytes();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    zip.start_file(MANIFEST, opts)?;
    zip.write_all(&manifest_bytes)?;
    if let Some(k) = sign {
        zip.start_file(SIGNATURE, opts)?;
        zip.write_all(hex(&k.sign(&manifest_bytes).to_bytes()).as_bytes())?;
    }
    for (path, bytes) in contents {
        zip.start_file(path, opts)?;
        zip.write_all(&bytes)?;
    }
    Ok((manifest, zip.finish()?.into_inner()))
}

impl Package {
    /// Open a package and check it: manifest, safe paths, sizes, every file present with the
    /// listed hash, nothing unlisted.
    pub fn open(bytes: &[u8]) -> Result<Self> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).context("not a .grmod (zip)")?;
        ensure!(zip.len() <= MAX_FILES + 2, "too many files");
        let mut raw: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let mut total = 0u64;
        for i in 0..zip.len() {
            let mut f = zip.by_index(i)?;
            if f.is_dir() {
                continue;
            }
            let name = f.name().to_string();
            ensure!(safe_path(&name), "unsafe path in package: {name:?}");
            ensure!(f.size() <= MAX_FILE_BYTES, "{name} is too large");
            total += f.size();
            ensure!(total <= MAX_TOTAL_BYTES, "package is too large");
            let mut b = Vec::with_capacity(f.size() as usize);
            (&mut f).take(MAX_FILE_BYTES + 1).read_to_end(&mut b)?;
            ensure!(b.len() as u64 == f.size(), "{name}: size mismatch");
            ensure!(raw.insert(name.clone(), b).is_none(), "duplicate {name}");
        }
        let manifest_bytes = raw.remove(MANIFEST).context("package has no grmod.toml")?;
        let signature = raw.remove(SIGNATURE);
        let manifest: PackageManifest =
            toml::from_str(std::str::from_utf8(&manifest_bytes)?).context("grmod.toml")?;
        ensure!(
            manifest.format == FORMAT,
            "unsupported package format {}",
            manifest.format
        );
        semver::Version::parse(&manifest.version).context("package version")?;
        let mut files = BTreeMap::new();
        for pf in &manifest.files {
            ensure!(safe_path(&pf.path), "unsafe path {:?}", pf.path);
            ensure!(
                extension_allowed(&pf.path),
                "{}: file type not allowed",
                pf.path
            );
            let b = raw
                .remove(&pf.path)
                .with_context(|| format!("{} is listed but missing", pf.path))?;
            ensure!(b.len() as u64 == pf.size, "{}: size mismatch", pf.path);
            ensure!(
                sha256(&b) == pf.sha256,
                "{}: hash mismatch (corrupted or modified)",
                pf.path
            );
            files.insert(pf.path.clone(), b);
        }
        if let Some(extra) = raw.keys().next() {
            bail!("{extra} is in the package but not listed in grmod.toml");
        }
        ensure!(files.contains_key("mod.toml"), "package has no mod.toml");
        Ok(Self {
            manifest,
            manifest_bytes,
            signature,
            files,
        })
    }

    pub fn signature_status(&self) -> SignatureStatus {
        let (Some(sig), Some(signer)) = (&self.signature, &self.manifest.signer) else {
            return if self.signature.is_some() || self.manifest.signer.is_some() {
                SignatureStatus::Invalid {
                    reason: "signature and signer key must both be present".into(),
                }
            } else {
                SignatureStatus::Unsigned
            };
        };
        let check = || -> Result<()> {
            let key = verifying_key(signer)?;
            let bytes: [u8; 64] = unhex(std::str::from_utf8(sig)?)?
                .try_into()
                .map_err(|_| anyhow::anyhow!("signature is not 64 bytes"))?;
            key.verify(&self.manifest_bytes, &Signature::from_bytes(&bytes))?;
            Ok(())
        };
        match check() {
            Ok(()) => SignatureStatus::Valid {
                signer: signer.clone(),
            },
            Err(e) => SignatureStatus::Invalid {
                reason: format!("{e:#}"),
            },
        }
    }

    /// Write the mod files into `dir` (which must not exist yet).
    pub fn extract(&self, dir: &Path) -> Result<()> {
        ensure!(!dir.exists(), "{} already exists", dir.display());
        for (path, bytes) in &self.files {
            let target = dir.join(path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, bytes)?;
        }
        Ok(())
    }
}
