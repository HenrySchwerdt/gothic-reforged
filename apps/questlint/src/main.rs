//! Static quest-graph checker.
//!
//! ```text
//! questlint [--data DIR] [--lang L] [--items] [--simulate] [--out report.json]
//!           [--list unreachable|quests|events|issues|strings] [--quest NAME]
//! ```
//! `--items` also loads `Templates.pak` entity names to find missing item templates (slow).
//! `--simulate` additionally plays the whole game greedily with the concrete engine.
//! Exit code 0 always; the report is the product.
use anyhow::{Context, Result, bail};
use gothic_gameplay::GameDatabase;
use gothic_gameplay::lint::{lint, simulate};
use std::path::PathBuf;

const DEFAULT_DATA: &str = "D:/SteamLibrary/steamapps/common/Gothic 3/Data";

fn main() -> Result<()> {
    let mut data = DEFAULT_DATA.to_string();
    let mut lang = "German".to_string();
    let (mut items, mut sim) = (false, false);
    let (mut out, mut list, mut quest) = (None::<String>, None::<String>, None::<String>);
    let mut why = None::<String>;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut val = |n: &str| it.next().with_context(|| format!("{n} needs a value"));
        match a.as_str() {
            "--data" => data = val("--data")?,
            "--lang" => lang = val("--lang")?,
            "--items" => items = true,
            "--simulate" => sim = true,
            "--out" => out = Some(val("--out")?),
            "--list" => list = Some(val("--list")?),
            "--quest" => quest = Some(val("--quest")?),
            "--why" => why = Some(val("--why")?),
            other => bail!("unknown argument {other}"),
        }
    }
    let data = PathBuf::from(data);
    let mut db = GameDatabase::load_install(&data, &lang)?;
    if items {
        let n = db.load_item_names(&data)?;
        eprintln!("loaded {n} item/entity template names");
    }
    let r = lint(&db);
    println!(
        "== questlint: {} quests, {} infos ==",
        r.quests_total, r.infos_total
    );
    println!(
        "quests statically completable: {} / {}  ({} of them only if a script/world trigger starts them; start-only: {}, never start: {})",
        r.quests_completable,
        r.quests_total,
        r.quests_completable_external_start,
        r.quests_start_only,
        r.quests_never_start
    );
    println!(
        "infos reachable: {} / {}  ({} unreachable)",
        r.infos_reached,
        r.infos_total,
        r.unreachable_infos.len()
    );
    println!("unreachable by root cause:");
    let mut causes: Vec<_> = r.unreachable_by_cause.iter().collect();
    causes.sort_by_key(|c| std::cmp::Reverse(*c.1));
    for (c, n) in causes {
        println!("  {n:5}  {c}");
    }
    let never = r
        .dead_events
        .iter()
        .filter(|e| e.kind == "never_set")
        .count();
    println!(
        "dead events: {} never set by any info, {} only set by unreachable infos",
        never,
        r.dead_events.len() - never
    );
    println!(
        "data: {} issues, {} missing strings, {} undefined quest refs, {} undefined parents, {} missing items, {} items never given by an info",
        r.data_issues.len(),
        r.missing_strings.len(),
        r.undefined_quests.len(),
        r.undefined_parents.len(),
        r.missing_items.len(),
        r.items_not_given_by_any_info.len()
    );
    let c = &r.coverage;
    println!(
        "coverage: condition types {} known / {} unknown ({} infos unknown); commands {} known / {} unknown ({} uses unknown)",
        c.distinct_conditions_known,
        c.distinct_conditions_unknown,
        c.unknown_condition_infos,
        c.distinct_commands_known,
        c.distinct_commands_unknown,
        c.unknown_command_uses
    );
    for (cmd, (n, h)) in &c.commands {
        if *h == "UNKNOWN" {
            println!("  UNKNOWN command {cmd} x{n}");
        }
    }
    if let Some(l) = &list {
        match l.as_str() {
            "unreachable" => {
                for u in &r.unreachable_infos {
                    println!(
                        "  {} (owner {}, quest {:?}): {}",
                        u.info, u.owner, u.quest, u.reason
                    );
                }
            }
            "quests" => {
                for q in &r.quests {
                    if quest
                        .as_ref()
                        .is_none_or(|n| n.eq_ignore_ascii_case(&q.name))
                    {
                        println!(
                            "  {:<45} {:<12} {:<26} start={:?} finish={:?} {}",
                            q.name,
                            q.quest_type,
                            q.verdict,
                            q.start_witness.as_deref().unwrap_or("-"),
                            q.finish_witness.as_deref().unwrap_or("-"),
                            q.blockers.join(" | ")
                        );
                    }
                }
            }
            "events" => {
                for e in &r.dead_events {
                    println!(
                        "  {} [{}] required by {}",
                        e.event,
                        e.kind,
                        e.required_by.join(",")
                    );
                }
            }
            "issues" => {
                for d in &r.data_issues {
                    println!("  {} {}: {}", d.kind, d.info, d.detail);
                }
            }
            "strings" => {
                for (i, k) in &r.missing_strings {
                    println!("  {i}: {k}");
                }
            }
            "simgap" => {}
            other => bail!("unknown --list {other}"),
        }
    }
    let mut sim_json = None;
    if sim {
        let (s, st) = simulate(&db);
        println!(
            "simulation: {} rounds, {} infos executed, {} quests Success/Won, {} still running, {} never touched; XP {}, level {}",
            s.rounds,
            s.infos_executed,
            s.quests_success.len(),
            s.quests_running_only.len(),
            s.quests_untouched,
            st.xp,
            st.level
        );
        let statically: std::collections::HashSet<_> = r
            .quests
            .iter()
            .filter(|q| q.verdict == "completable")
            .map(|q| q.name.to_ascii_lowercase())
            .collect();
        let sim_set: std::collections::HashSet<_> = s
            .quests_success
            .iter()
            .map(|n| n.to_ascii_lowercase())
            .collect();
        println!(
            "  completable statically but not in greedy playthrough: {} (order effects or ClearGameEvent)",
            statically.difference(&sim_set).count()
        );
        if list.as_deref() == Some("simgap") {
            let mut gap: Vec<_> = statically.difference(&sim_set).collect();
            gap.sort();
            for g in gap {
                let q = r.quests.iter().find(|q| q.name.eq_ignore_ascii_case(g));
                println!("  {} ({})", g, q.map_or("?", |q| q.quest_type.as_str()));
            }
        }
        if let Some(w) = &why {
            let eng = gothic_gameplay::Engine::with(&db, gothic_gameplay::Assumptions::OPTIMISTIC);
            if let Some(q) = quest.as_ref() {
                for e in st.log.iter().filter(|e| e.quest.eq_ignore_ascii_case(q)) {
                    println!("  log {:?}", e.kind);
                }
            }
            for name in w.split(',') {
                match db.info_id(name) {
                    Some(id) => {
                        let owner = db.info(id).owner.clone();
                        println!(
                            "  why {name}: {:?} (parent {:?})",
                            eng.check(&st, id, &owner),
                            db.info(id).parent
                        );
                    }
                    None => println!("  why {name}: no such info"),
                }
            }
        }
        sim_json = Some(serde_json::to_value(&s)?);
    }
    if let Some(f) = out {
        let mut v = serde_json::to_value(&r)?;
        if let (Some(s), Some(o)) = (sim_json, v.as_object_mut()) {
            o.insert("simulation".into(), s);
        }
        std::fs::write(&f, serde_json::to_string_pretty(&v)?)?;
        eprintln!("wrote {f}");
    }
    Ok(())
}
