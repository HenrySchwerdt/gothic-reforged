//! `cook` command line, shared by `gothic-inspector cook` and the `gothic-cook` binary.
//!
//! ```text
//! cook [--data DIR] [--profile desktop-high|desktop-low|quest|remaster] [--out ROOT]
//!      [--only db,mesh,collision,material,texture,world,actor,motion]
//!      [--region ardea] [--language English] [--force] [--quiet]
//! ```
use crate::cache::default_cache_root;
use crate::cook::{CookKind, Options, Region, cook, profile_or_err};
use anyhow::{Context, Result, bail};
use std::ffi::OsString;
use std::path::PathBuf;

const DEFAULT_DATA: &str = r"D:\SteamLibrary\steamapps\common\Gothic 3\Data";

/// Returns `Ok(true)` when `command` was `cook`.
pub fn dispatch(command: &str, args: &[OsString]) -> Result<bool> {
    if command != "cook" {
        return Ok(false);
    }
    let mut data = PathBuf::from(DEFAULT_DATA);
    let mut out = default_cache_root();
    let mut profile = "desktop-high".to_owned();
    let mut only = Vec::new();
    let mut region = None;
    let mut language = "English".to_owned();
    let (mut force, mut quiet) = (false, false);
    let mut it = args
        .iter()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned());
    while let Some(a) = it.next() {
        let mut val = |what: &str| it.next().with_context(|| format!("{a} needs {what}"));
        match a.as_str() {
            "--data" => data = val("a directory")?.into(),
            "--out" => out = val("a directory")?.into(),
            "--profile" => profile = val("a profile")?,
            "--language" => language = val("a language")?,
            "--only" => {
                for k in val("a kind list")?.split(',') {
                    only.push(CookKind::parse(k).with_context(|| format!("unknown kind {k:?}"))?);
                }
            }
            "--region" => {
                let r = val("a region")?;
                region = Some(
                    Region::by_name(&r)
                        .with_context(|| format!("unknown region {r:?} (known: ardea)"))?,
                );
            }
            "--force" => force = true,
            "--quiet" => quiet = true,
            other => bail!("unknown cook option {other}"),
        }
    }
    let opts = Options {
        data_dir: data,
        out_root: out,
        profile: profile_or_err(&profile)?,
        only,
        region,
        language,
        force,
        quiet,
    };
    let r = cook(&opts)?;
    println!("cache: {}", r.out_dir.display());
    for (kind, s) in &r.stats {
        println!(
            "{kind:<10} cooked {:>7}  skipped {:>7}  failed {:>5}  missing {:>5}  {:>10.1} MiB  {:>7.1} s",
            s.cooked,
            s.skipped,
            s.failed,
            s.missing,
            s.bytes as f64 / 1_048_576.0,
            s.seconds
        );
    }
    println!(
        "total cache size {:.1} MiB, {:.1} s",
        r.total_bytes as f64 / 1_048_576.0,
        r.seconds
    );
    for (k, v) in r.failures.iter().take(15) {
        println!("  failed {k}: {v}");
    }
    if r.failures.len() > 15 {
        println!("  ... {} more (see manifest.json)", r.failures.len() - 15);
    }
    Ok(true)
}

pub fn main() -> Result<()> {
    let mut args: Vec<OsString> = vec!["cook".into()];
    args.extend(std::env::args_os().skip(1));
    dispatch("cook", &args)?;
    Ok(())
}
