//! `questbot` CLI.
//!
//! ```text
//! questbot --fixture [--broken] [--trace] [--json OUT]
//! questbot <Gothic3/Data> [--language German] [--quest NAME]... [--strict] [--trace] [--isolated [--shard I/N]] [--max-rounds N] [--json OUT]
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
                && Some(*a) != value("--max-rounds").as_ref()
                && Some(*a) != value("--shard").as_ref()
        }) else {
            bail!(
                "usage: questbot --fixture | questbot <Gothic3/Data> [--language German] [--quest NAME]... [--strict] [--trace] [--isolated [--shard I/N]] [--max-rounds N] [--json OUT]"
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
        max_rounds: value("--max-rounds")
            .and_then(|v| v.parse().ok())
            .unwrap_or(BotOptions::default().max_rounds),
        ..BotOptions::default()
    };
    if has("--isolated") {
        return isolated(&db, &options, value("--json"), value("--shard"));
    }
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

/// `--isolated`: every quest alone (own fresh game state, only its prerequisites played).
/// Prints the totals, one line per failure and optionally a JSON summary.
fn isolated(
    db: &GameDatabase,
    options: &BotOptions,
    json: Option<String>,
    shard: Option<String>,
) -> Result<ExitCode> {
    // `--shard I/N`: only every N-th quest starting at I, to spread a run over processes.
    let (si, sn) = shard
        .and_then(|s| {
            let (i, n) = s.split_once('/')?;
            Some((i.parse::<usize>().ok()?, n.parse::<usize>().ok()?.max(1)))
        })
        .unwrap_or((0, 1));
    let mut rows = Vec::new();
    let mut done = 0usize;
    for q in db.quests.iter().skip(si).step_by(sn) {
        let report = play(
            db,
            BotOptions {
                targets: Some(vec![q.name.clone()]),
                ..options.clone()
            },
        );
        let result = if !report.completed.is_empty() {
            done += 1;
            "completed".to_string()
        } else if let Some(s) = report.stuck.first() {
            format!("stuck:{:?}", s.reason)
        } else {
            "unsuccessful".to_string()
        };
        println!("{:<44} {result}", q.name);
        rows.push((q.name.clone(), result));
    }
    println!("isolated: {done}/{} completed", rows.len());
    if let Some(out) = json {
        let v: Vec<serde_json::Value> = rows
            .iter()
            .map(|(q, r)| serde_json::json!({ "quest": q, "result": r }))
            .collect();
        let doc = serde_json::json!({ "mode": "isolated", "total": rows.len(), "completed": done, "quests": v });
        std::fs::write(&out, serde_json::to_string_pretty(&doc)?)
            .with_context(|| format!("writing {out}"))?;
    }
    Ok(if done == rows.len() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    })
}
