//! Text-mode dialog player using the real quest/dialog data.
//!
//! ```text
//! dialog-cli talk NPC [options]      interactive conversation (stdin) or --script
//! dialog-cli npcs [--min N]          list dialog partners by number of infos
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
use gothic_gameplay::model::QuestStatus;
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
        "talk" => {
            let npc = o.positional.get(1).context("talk NPC")?.clone();
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
            eprintln!("usage: dialog-cli talk NPC [options] | npcs [--min N]  (see source header)");
            Ok(())
        }
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
