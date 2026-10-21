//! `modtool`: the mod manager CLI. Run `modtool help` for the commands.
use anyhow::{Context, Result, bail};
use gothic_modtool::package::{signing_key, verifying_key};
use gothic_modtool::{
    InstallOptions, ModList, Package, SignatureStatus, install, keygen, pack, scaffold,
    static_conflicts, uninstall, validate_dir,
};
use std::path::{Path, PathBuf};

const HELP: &str = "modtool - Gothic Reforged mod manager

  modtool new <dir> [--id ID] [--name NAME] [--data] [--script]
  modtool validate <mod-dir>... [--json]
  modtool pack <mod-dir> [-o FILE.grmod] [--sign SECRET-KEY-FILE]
  modtool keygen <SECRET-KEY-FILE>          (prints the public key)
  modtool verify <FILE.grmod> [--trust PUBLIC-KEY]
  modtool install <FILE.grmod> [--mods DIR] [--force] [--trust PUBLIC-KEY]... [--require-signature]
  modtool uninstall <id> [--mods DIR]
  modtool list [--mods DIR] [--json]
  modtool enable <id> | disable <id> [--mods DIR]
  modtool move <id> <position> [--mods DIR]
  modtool conflicts [--mods DIR] [--json]

The mods folder defaults to ./mods. See docs/MODDING.md.";

struct Args {
    positional: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

impl Args {
    fn parse(raw: impl Iterator<Item = String>) -> Self {
        const WITH_VALUE: &[&str] = &["--id", "--name", "-o", "--sign", "--trust", "--mods"];
        let mut positional = Vec::new();
        let mut flags = Vec::new();
        let mut it = raw.peekable();
        while let Some(a) = it.next() {
            if WITH_VALUE.contains(&a.as_str()) {
                flags.push((a, it.next()));
            } else if a.starts_with('-') && a.len() > 1 {
                flags.push((a, None));
            } else {
                positional.push(a);
            }
        }
        Self { positional, flags }
    }

    fn flag(&self, name: &str) -> bool {
        self.flags.iter().any(|(f, _)| f == name)
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .find(|(f, _)| f == name)
            .and_then(|(_, v)| v.as_deref())
    }

    fn values(&self, name: &str) -> Vec<String> {
        self.flags
            .iter()
            .filter(|(f, _)| f == name)
            .filter_map(|(_, v)| v.clone())
            .collect()
    }

    fn mods_dir(&self) -> PathBuf {
        PathBuf::from(self.value("--mods").unwrap_or("mods"))
    }

    fn arg(&self, i: usize, what: &str) -> Result<&str> {
        self.positional
            .get(i)
            .map(String::as_str)
            .with_context(|| format!("missing {what}; see `modtool help`"))
    }
}

fn main() {
    let mut raw = std::env::args().skip(1);
    let cmd = raw.next().unwrap_or_else(|| "help".into());
    let args = Args::parse(raw);
    match run(&cmd, &args) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("modtool {cmd}: {e:#}");
            std::process::exit(2);
        }
    }
}

fn run(cmd: &str, a: &Args) -> Result<i32> {
    let json = a.flag("--json");
    match cmd {
        "help" | "--help" | "-h" => println!("{HELP}"),
        "new" => {
            let dir = PathBuf::from(a.arg(0, "directory")?);
            let id = a.value("--id").map(str::to_string).unwrap_or_else(|| {
                dir.file_name()
                    .map(|n| n.to_string_lossy().to_ascii_lowercase().replace(' ', "_"))
                    .unwrap_or_else(|| "my_mod".into())
            });
            let name = a.value("--name").unwrap_or(&id).to_string();
            let (data, script) = match (a.flag("--data"), a.flag("--script")) {
                (false, false) => (true, false),
                other => other,
            };
            scaffold::scaffold(&dir, &id, &name, data, script)?;
            println!("created {} ({id})", dir.display());
        }
        "validate" => {
            if a.positional.is_empty() {
                bail!("missing mod directory");
            }
            let reports: Vec<_> = a
                .positional
                .iter()
                .map(|d| validate_dir(Path::new(d)))
                .collect();
            if json {
                println!("{}", serde_json::to_string_pretty(&reports)?);
            } else {
                for r in &reports {
                    print!("{}", r.to_text());
                }
            }
            return Ok(i32::from(reports.iter().any(|r| !r.ok())));
        }
        "pack" => {
            let dir = Path::new(a.arg(0, "mod directory")?);
            let key = match a.value("--sign") {
                Some(f) => Some(signing_key(&std::fs::read_to_string(f)?)?),
                None => None,
            };
            let (manifest, bytes) = pack(dir, key.as_ref())?;
            let out = a.value("-o").map(PathBuf::from).unwrap_or_else(|| {
                PathBuf::from(format!("{}-{}.grmod", manifest.id, manifest.version))
            });
            std::fs::write(&out, &bytes)?;
            println!(
                "packed {} {} ({} files, {} bytes{}) -> {}",
                manifest.id,
                manifest.version,
                manifest.files.len(),
                bytes.len(),
                if key.is_some() { ", signed" } else { "" },
                out.display()
            );
        }
        "keygen" => {
            let out = PathBuf::from(a.arg(0, "secret key file")?);
            if out.exists() {
                bail!("{} exists; not overwriting a key", out.display());
            }
            let (secret, public) = keygen()?;
            std::fs::write(&out, secret)?;
            println!("secret key written to {} (keep it private)", out.display());
            println!("public key: {public}");
        }
        "verify" => {
            let pkg = Package::open(&std::fs::read(a.arg(0, "package")?)?)?;
            let status = pkg.signature_status();
            println!(
                "{} {}: {} files, hashes OK, signature: {}",
                pkg.manifest.id,
                pkg.manifest.version,
                pkg.files.len(),
                match &status {
                    SignatureStatus::Unsigned => "unsigned".to_string(),
                    SignatureStatus::Valid { signer } => format!("valid ({signer})"),
                    SignatureStatus::Invalid { reason } => format!("INVALID ({reason})"),
                }
            );
            if let Some(trust) = a.value("--trust") {
                verifying_key(trust)?;
                let trusted = matches!(&status, SignatureStatus::Valid { signer } if signer.eq_ignore_ascii_case(trust));
                println!("trusted: {trusted}");
                return Ok(i32::from(!trusted));
            }
            return Ok(i32::from(matches!(status, SignatureStatus::Invalid { .. })));
        }
        "install" => {
            let bytes = std::fs::read(a.arg(0, "package")?)?;
            let opts = InstallOptions {
                force: a.flag("--force"),
                trusted_keys: a.values("--trust"),
                require_signature: a.flag("--require-signature"),
            };
            let r = install(&bytes, &a.mods_dir(), &opts)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                println!(
                    "installed {} {}{}",
                    r.id,
                    r.version,
                    r.replaced
                        .as_ref()
                        .map(|v| format!(" (replaced {v})"))
                        .unwrap_or_default()
                );
                for p in &r.missing_dependencies {
                    println!("  WARN {p}");
                }
            }
        }
        "uninstall" => {
            let id = a.arg(0, "mod id")?;
            uninstall(&a.mods_dir(), id)?;
            println!("uninstalled {id}");
        }
        "enable" | "disable" => {
            let id = a.arg(0, "mod id")?;
            let mut list = ModList::load(&a.mods_dir())?;
            if !list.set_enabled(id, cmd == "enable") {
                bail!("{id} is not installed");
            }
            list.save()?;
            println!("{cmd}d {id}");
        }
        "move" => {
            let id = a.arg(0, "mod id")?;
            let pos: usize = a.arg(1, "position")?.parse().context("position")?;
            let mut list = ModList::load(&a.mods_dir())?;
            if !list.move_to(id, pos) {
                bail!("{id} is not installed");
            }
            list.save()?;
            println!("moved {id} to {pos}");
        }
        "list" => {
            let list = ModList::load(&a.mods_dir())?;
            let entries = list.entries();
            if json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else {
                for e in &entries {
                    println!(
                        "{:>2}. [{}] {} {} {:?}{}{}",
                        e.position,
                        if e.enabled { "x" } else { " " },
                        e.id,
                        e.version,
                        e.status,
                        e.load_index
                            .map(|i| format!(" (loads #{i})"))
                            .unwrap_or_default(),
                        if e.problems.is_empty() {
                            String::new()
                        } else {
                            format!(": {}", e.problems.join("; "))
                        }
                    );
                }
            }
            return Ok(i32::from(
                entries
                    .iter()
                    .any(|e| e.status == gothic_modtool::ModStatus::Problem),
            ));
        }
        "conflicts" => {
            let list = ModList::load(&a.mods_dir())?;
            let c = static_conflicts(&list.active_mods());
            if json {
                println!("{}", serde_json::to_string_pretty(&c)?);
            } else if c.is_empty() {
                println!("no conflicts");
            } else {
                for x in &c {
                    println!("CONFLICT {x}");
                }
            }
        }
        other => bail!("unknown command {other:?}; see `modtool help`"),
    }
    Ok(0)
}
