//! `questbot` CLI.
//!
//! ```text
//! questbot --fixture [--broken] [--trace] [--json OUT]
//! questbot <Gothic3/Data> [--language German] [--quest NAME]... [--strict] [--trace] [--json OUT]
//! ```
//! Exit code: 0 when every target quest completed, 2 when some are stuck or unsuccessful.
use anyhow::{Context, Result, bail};
use gothic_gameplay::{Assumptions, GameDatabase};
use gothic_questbot::{BotOptions, fixture, play};
use std::path::Path;
use std::process::ExitCode;

fn main() -> Result<ExitCode> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    let value = |f: &str| {
        args.iter()
            .position(|a| a == f)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let db = if has("--fixture") {
        if has("--broken") {
            fixture::broken()
        } else {
            fixture::database()
        }
    } else {
        let Some(dir) = args.iter().find(|a| {
            !a.starts_with("--")
                && Some(*a) != value("--json").as_ref()
                && Some(*a) != value("--language").as_ref()
        }) else {
            bail!(
                "usage: questbot --fixture | questbot <Gothic3/Data> [--language German] [--quest NAME]... [--strict] [--trace] [--json OUT]"
            );
        };
        let language = value("--language").unwrap_or_else(|| "German".into());
        let mut db = GameDatabase::load_install(Path::new(dir), &language)
            .with_context(|| format!("loading quests from {dir}"))?;
        // Item names make the item-quest cheats exact; optional.
        let _ = db.load_item_names(Path::new(dir));
        db
    };
    let quests: Vec<String> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| *a == "--quest")
        .filter_map(|(i, _)| args.get(i + 1).cloned())
        .collect();
    let options = BotOptions {
        targets: (!quests.is_empty()).then_some(quests),
        assumptions: if has("--strict") {
            Assumptions::STRICT
        } else {
            Assumptions::PERMISSIVE
        },
        ..BotOptions::default()
    };
    let report = play(&db, options);
    print!("{}", report.to_text(has("--trace")));
    if let Some(out) = value("--json") {
        std::fs::write(&out, report.to_json()).with_context(|| format!("writing {out}"))?;
    }
    Ok(if report.all_completed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    })
}
