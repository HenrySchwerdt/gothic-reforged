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
                && Some(*a) != value("--dump").as_ref()
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
    // `--dump NAME`: print the info(s) with that name or the quest and its infos, then exit.
    if let Some(name) = value("--dump") {
        dump(&db, &name);
        return Ok(ExitCode::SUCCESS);
    }
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

/// Debug dump: one compact line per info (`--dump NAME`: the info, or the quest and its infos).
fn dump(db: &GameDatabase, name: &str) {
    let line = |id| {
        let i = db.info(id);
        let cmds: Vec<String> = i
            .script
            .iter()
            .filter(|s| !matches!(s.command.as_str(), "Say" | "SaySvm" | "SaySound" | "Description"))
            .map(|s| format!("{}({},{},{})", s.command, s.entity1, s.id1, s.id2))
            .collect();
        println!(
            "{} @{} parent={:?} quest={:?} cond={:?} type={:?} perm={} knows={:?} knows_not={:?} items={:?}x{:?}@{:?} npcs={:?}{:?} near={:?} script=[{}]",
            i.name,
            i.owner,
            i.parent,
            i.quest,
            gothic_gameplay::model::InfoCond::from_raw(i.condition_type),
            gothic_gameplay::model::InfoType::from_raw(i.info_type),
            i.permanent,
            i.cond_player_knows,
            i.cond_player_knows_not,
            i.cond_items,
            i.cond_item_amounts,
            i.cond_item_container,
            i.cond_secondary_npc,
            i.cond_secondary_npc_states,
            i.cond_owner_near_entity,
            cmds.join(" ")
        );
    };
    if name == "--owners" {
        println!("owners: {}, infos: {}", db.owners().count(), db.infos.len());
        println!("PC_Hero infos: {}", db.infos_of("PC_Hero").len());
        return;
    }
    if let Some(&id) = db.infos_of(name).first() {
        if db.info_id(name).is_none() {
            println!("{} infos owned by {name}; first:", db.infos_of(name).len());
            line(id);
        }
    }
    if let Some(id) = db.info_id(name) {
        line(id);
    }
    if let Some(q) = db.quest_by_name(name) {
        println!(
            "QUEST {} type={:?} prereq={:?} deliver={:?}x{:?} dest={:?}",
            q.name,
            gothic_gameplay::model::QuestType::from_raw(q.quest_type),
            q.finished_quests,
            q.delivery_entities,
            q.delivery_amounts,
            q.destination_entity
        );
        for &id in db.infos_of_quest(name) {
            line(id);
        }
    }
}
