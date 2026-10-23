//! Text-mode dialog player using the real quest/dialog data.
//!
//! ```text
//! dialog-cli talk NPC [options]      interactive conversation (stdin) or --script
//! dialog-cli npcs [--min N]          list dialog partners by number of infos
//! dialog-cli inspect NPC [INFO]      inspect root availability or one canonical info
//! options:
//!   --data DIR      game Data directory (default D:/SteamLibrary/steamapps/common/Gothic 3/Data)
//!   --lang L        German (default) or English or any language of the string table
//!   --load FILE     start from a saved GameState (JSON)
//!   --save FILE     write the GameState after the conversation
//!   --script A,B,C  non-interactive: menu picks (1-based number or label substring)
//!   --event E       set a game event before talking (repeatable)
//!   --give ITEM=N   put items into the player inventory (repeatable)
//!   --quest Q=STATE force a quest status: running|success|failed|... (repeatable)
//!   --strict        do not assume world conditions (near-entity, NPC inventories)
//!   --verbose       also print forwarded world effects
//! ```
//! In interactive mode, besides menu numbers: `log` (quest log), `inv`, `state`, `q` (leave).
use anyhow::{Context, Result, bail};
use gothic_gameplay::conditions::Assumptions;
use gothic_gameplay::model::{InfoCond, InfoType, QuestStatus};
use gothic_gameplay::state::GameState;
use gothic_gameplay::transcript::{render_menu, render_result, resolve_pick, run_script};
use gothic_gameplay::{Engine, GameDatabase};
use std::io::{BufRead, Write};
use std::path::PathBuf;

const DEFAULT_DATA: &str = "D:/SteamLibrary/steamapps/common/Gothic 3/Data";

#[derive(Default)]
struct Opts {
    positional: Vec<String>,
    data: Option<String>,
    lang: Option<String>,
    load: Option<String>,
    save: Option<String>,
    script: Option<String>,
    events: Vec<String>,
    gives: Vec<String>,
    quests: Vec<String>,
    min: usize,
    strict: bool,
    verbose: bool,
}

fn parse() -> Result<Opts> {
    let mut o = Opts::default();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut val = |name: &str| it.next().with_context(|| format!("{name} needs a value"));
        match a.as_str() {
            "--data" => o.data = Some(val("--data")?),
            "--lang" => o.lang = Some(val("--lang")?),
            "--load" => o.load = Some(val("--load")?),
            "--save" => o.save = Some(val("--save")?),
            "--script" => o.script = Some(val("--script")?),
            "--event" => o.events.push(val("--event")?),
            "--give" => o.gives.push(val("--give")?),
            "--quest" => o.quests.push(val("--quest")?),
            "--min" => o.min = val("--min")?.parse()?,
            "--strict" => o.strict = true,
            "--verbose" => o.verbose = true,
            s if s.starts_with("--") => bail!("unknown option {s}"),
            _ => o.positional.push(a),
        }
    }
    Ok(o)
}

fn status_from(s: &str) -> Result<QuestStatus> {
    Ok(match s.to_ascii_lowercase().as_str() {
        "open" => QuestStatus::Open,
        "running" => QuestStatus::Running,
        "success" => QuestStatus::Success,
        "failed" => QuestStatus::Failed,
        "obsolete" => QuestStatus::Obsolete,
        "cancelled" => QuestStatus::Cancelled,
        "lost" => QuestStatus::Lost,
        "won" => QuestStatus::Won,
        other => bail!("unknown quest status {other}"),
    })
}

fn print_log(db: &GameDatabase, st: &GameState) {
    println!("--- Tagebuch ---");
    for (name, q) in &st.quests {
        if let Some(s) = q.status {
            println!(
                "  {} [{}]",
                gothic_gameplay::transcript::quest_title(db, name),
                s.name()
            );
        }
    }
    println!(
        "  EP {}  Stufe {}  Lernpunkte {}  Gold {}",
        st.xp,
        st.level,
        st.learning_points,
        st.gold()
    );
}

fn state_from_options(db: &GameDatabase, o: &Opts) -> Result<GameState> {
    let mut st = match &o.load {
        Some(f) => GameState::from_json(&std::fs::read_to_string(f)?)?,
        None => GameState::new(),
    };
    for e in &o.events {
        st.set_event(e);
    }
    for g in &o.gives {
        let (item, n) = g.split_once('=').context("--give ITEM=N")?;
        st.add_item(item, n.parse()?);
    }
    for q in &o.quests {
        let (name, s) = q.split_once('=').context("--quest Q=STATE")?;
        if db.quest_by_name(name).is_none() {
            bail!("unknown quest {name}");
        }
        st.set_quest_status(name, status_from(s)?);
    }
    Ok(st)
}

fn inspect_infos(
    db: &GameDatabase,
    st: &GameState,
    npc: &str,
    selected: Option<&str>,
    assume: Assumptions,
) -> Result<String> {
    let engine = Engine::with(db, assume);
    let ids = if let Some(name) = selected {
        vec![
            db.info_id(name)
                .with_context(|| format!("unknown info {name:?}"))?,
        ]
    } else {
        engine.root_list(st, npc)
    };

    let mut out = String::new();
    if selected.is_none() {
        out.push_str(&format!(
            "Currently available root infos for {npc:?} (before start auto-run):\n"
        ));
    }
    if ids.is_empty() {
        out.push_str("  (none)\n");
    }
    for id in ids {
        let info = db.info(id);
        let owner_matches =
            info.owner.eq_ignore_ascii_case(npc) || info.owner.eq_ignore_ascii_case("PC_Hero");
        let condition = engine.check(st, id, npc);
        out.push_str(&format!(
            "Info: {}\n  owner: {}\n  owner filter for {npc}: {}\n  parent: {}\n  quest: {}\n  InfoType: {} ({:?})\n  ConditionType: {} ({:?})\n",
            info.name,
            info.owner,
            if owner_matches { "eligible" } else { "filtered" },
            if info.parent.is_empty() { "(root)" } else { &info.parent },
            if info.quest.is_empty() { "(none)" } else { &info.quest },
            info.info_type,
            InfoType::from_raw(info.info_type),
            info.condition_type,
            InfoCond::from_raw(info.condition_type),
        ));
        match condition {
            Ok(()) => out.push_str("  condition result: available\n"),
            Err(reason) => out.push_str(&format!("  condition result: unavailable: {reason:?}\n")),
        }
        if selected.is_some() && !owner_matches {
            out.push_str(
                "  dialog result: owner-filtered (condition result is shown separately)\n",
            );
        }
    }
    Ok(out)
}

fn main() -> Result<()> {
    let o = parse()?;
    let cmd = o.positional.first().map(String::as_str).unwrap_or("");
    let data = PathBuf::from(o.data.clone().unwrap_or_else(|| DEFAULT_DATA.into()));
    let lang = o.lang.clone().unwrap_or_else(|| "German".into());
    let db = GameDatabase::load_install(&data, &lang)?;
    match cmd {
        "npcs" => {
            let mut v: Vec<(usize, &str)> =
                db.owners().map(|n| (db.infos_of(n).len(), n)).collect();
            v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
            for (n, name) in v.into_iter().filter(|x| x.0 >= o.min) {
                println!("{n:4} {name}");
            }
            Ok(())
        }
        "inspect" => {
            let npc = o.positional.get(1).context("inspect NPC [INFO]")?;
            let selected = o.positional.get(2).map(String::as_str);
            if o.positional.len() > 3 {
                bail!("inspect accepts NPC and at most one INFO name");
            }
            if o.save.is_some() || o.script.is_some() {
                bail!("inspect is read-only and does not accept --save or --script");
            }
            let st = state_from_options(&db, &o)?;
            let assume = if o.strict {
                Assumptions::STRICT
            } else {
                Assumptions::PERMISSIVE
            };
            print!("{}", inspect_infos(&db, &st, npc, selected, assume)?);
            Ok(())
        }
        "talk" => {
            let npc = o.positional.get(1).context("talk NPC")?.clone();
            let mut st = state_from_options(&db, &o)?;
            let eng = Engine::with(
                &db,
                if o.strict {
                    Assumptions::STRICT
                } else {
                    Assumptions::PERMISSIVE
                },
            );
            if db.infos_of(&npc).is_empty() {
                eprintln!(
                    "note: no infos are owned by {npc:?}; only the player's own infos can appear"
                );
            }
            if let Some(script) = &o.script {
                let picks: Vec<&str> = script.split(',').collect();
                print!("{}", run_script(&eng, &mut st, &npc, &picks));
            } else {
                interactive(&eng, &mut st, &npc, o.verbose)?;
            }
            if let Some(f) = &o.save {
                std::fs::write(f, st.to_json()?)?;
                eprintln!("state saved to {f}");
            }
            Ok(())
        }
        _ => {
            eprintln!(
                "usage: dialog-cli talk NPC [options] | inspect NPC [INFO] [options] | npcs [--min N]"
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gothic_formats::{info::Info, quest::Quest, stringtable::StringTable};

    fn fixture_db() -> GameDatabase {
        let teach_info = Info::parse(
            b"[Info]\r\nName=TEACH_FIXTURE\r\nSortID=1\r\nOwner=Trainer\r\nInfoGiven=false\r\nParent=\r\nQuest=\r\nConditionType=16\r\nInfoType=3\r\nTeachSkill=Skill_Fixture\r\n",
        )
        .unwrap();
        let quest_info = Info::parse(
            b"[Info]\r\nName=OPEN_QUEST_FIXTURE\r\nSortID=2\r\nOwner=Trainer\r\nInfoGiven=false\r\nParent=\r\nQuest=QUEST_FIXTURE\r\nConditionType=5\r\nInfoType=3\r\n",
        )
        .unwrap();
        let other_owner_info = Info::parse(
            b"[Info]\r\nName=OTHER_OWNER_FIXTURE\r\nSortID=3\r\nOwner=Other\r\nInfoGiven=false\r\nParent=\r\nQuest=\r\nConditionType=3\r\nInfoType=3\r\n",
        )
        .unwrap();
        let quest =
            Quest::parse(b"[Quest]\r\nName=QUEST_FIXTURE\r\nType=0\r\nLogTopic=QUEST_FIXTURE\r\n")
                .unwrap();
        let table_text = "[LocAdmin_Languages]\r\nLanguages=English;\r\nCurrentLanguage=English\r\nCurrentRevision=0\r\n[LocAdmin_Strings]\r\n[LocAdmin_Revisions]\r\n";
        let mut table = vec![0xFF, 0xFE];
        for unit in table_text.encode_utf16() {
            table.extend(unit.to_le_bytes());
        }
        GameDatabase::from_parts(
            vec![quest],
            vec![teach_info, quest_info, other_owner_info],
            StringTable::parse(&table).unwrap(),
            "English",
        )
        .unwrap()
    }

    #[test]
    fn inspect_reports_state_dependent_condition_rejection_and_acceptance() {
        let db = fixture_db();
        let mut st = GameState::new();
        let unavailable = inspect_infos(
            &db,
            &st,
            "Trainer",
            Some("TEACH_FIXTURE"),
            Assumptions::STRICT,
        )
        .unwrap();
        assert!(
            unavailable.contains("ConditionType: 16 (Teach)"),
            "{unavailable}"
        );
        assert!(
            unavailable.contains("condition result: unavailable: Flag(\"TeachEnabled\")"),
            "{unavailable}"
        );

        let npc = st.npc_mut("Trainer");
        npc.teach_enabled = Some(true);
        npc.teach_skills.insert("skill_fixture".into());
        let available = inspect_infos(
            &db,
            &st,
            "Trainer",
            Some("TEACH_FIXTURE"),
            Assumptions::STRICT,
        )
        .unwrap();
        assert!(
            available.contains("condition result: available"),
            "{available}"
        );
    }

    #[test]
    fn inspect_rejects_unknown_info_name() {
        let db = fixture_db();
        let error = inspect_infos(
            &db,
            &GameState::new(),
            "Trainer",
            Some("MISSING_INFO"),
            Assumptions::STRICT,
        )
        .unwrap_err();
        assert!(error.to_string().contains("unknown info"));
    }

    #[test]
    fn inspect_explains_quest_status_rejection() {
        let db = fixture_db();
        let mut st = GameState::new();
        let open = inspect_infos(
            &db,
            &st,
            "Trainer",
            Some("OPEN_QUEST_FIXTURE"),
            Assumptions::STRICT,
        )
        .unwrap();
        assert!(open.contains("condition result: available"), "{open}");

        st.set_quest_status("QUEST_FIXTURE", QuestStatus::Running);
        let running = inspect_infos(
            &db,
            &st,
            "Trainer",
            Some("OPEN_QUEST_FIXTURE"),
            Assumptions::STRICT,
        )
        .unwrap();
        assert!(running.contains("ConditionType: 5 (Open)"), "{running}");
        assert!(
            running.contains(
                "condition result: unavailable: QuestStatus { quest: \"QUEST_FIXTURE\", have: Running, need: \"Open\" }"
            ),
            "{running}"
        );
    }

    #[test]
    fn inspect_separates_owner_filter_from_condition_result() {
        let db = fixture_db();
        let output = inspect_infos(
            &db,
            &GameState::new(),
            "Trainer",
            Some("OTHER_OWNER_FIXTURE"),
            Assumptions::STRICT,
        )
        .unwrap();
        assert!(
            output.contains("owner filter for Trainer: filtered"),
            "{output}"
        );
        assert!(output.contains("condition result: available"), "{output}");
        assert!(output.contains("dialog result: owner-filtered"), "{output}");
    }
}

fn interactive(eng: &Engine<'_>, st: &mut GameState, npc: &str, verbose: bool) -> Result<()> {
    let db = eng.db;
    let (mut sess, open) = eng.start(st, npc);
    println!("=== Gespraech mit {npc} ===");
    for l in render_result(db, npc, &open, verbose) {
        println!("{l}");
    }
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    loop {
        let menu = eng.menu(st, &sess);
        if sess.ended || menu.is_empty() {
            break;
        }
        print!("{}> ", render_menu(&menu));
        std::io::stdout().flush()?;
        let Some(line) = lines.next() else { break };
        let line = line?;
        match line.trim() {
            "q" | "quit" => break,
            "log" => {
                print_log(db, st);
                continue;
            }
            "inv" => {
                for (k, v) in &st.inventory {
                    println!("  {v} x {k}");
                }
                continue;
            }
            "state" => {
                println!("{}", st.to_json()?);
                continue;
            }
            p => {
                let Some(c) = resolve_pick(&menu, p) else {
                    println!("(keine Auswahl)");
                    continue;
                };
                let c = c.clone();
                let r = eng.choose(st, &mut sess, &c);
                if let Some(why) = &r.refused {
                    println!("  [abgelehnt: {why}]");
                }
                for l in render_result(db, npc, &r, verbose) {
                    println!("{l}");
                }
            }
        }
    }
    println!("=== Ende ===");
    Ok(())
}
