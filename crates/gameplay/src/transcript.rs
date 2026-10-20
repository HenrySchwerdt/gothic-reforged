//! Text rendering of dialog results and a scripted (non-interactive) conversation runner,
//! shared by the `dialog-cli` binary and the tests.
use crate::db::GameDatabase;
use crate::dialog::{Choice, DialogSession, Engine};
use crate::model::QuestStatus;
use crate::script::{ExecResult, Output};
use crate::state::GameState;
use std::fmt::Write as _;

fn who(npc: &str, speaker: &str) -> String {
    if speaker == "player" {
        "Du".into()
    } else if speaker.is_empty() || speaker.eq_ignore_ascii_case(npc) {
        npc.to_string()
    } else {
        speaker.to_string()
    }
}

pub fn quest_title(db: &GameDatabase, quest: &str) -> String {
    db.quest_by_name(quest)
        .and_then(|q| db.text(&q.log_topic))
        .unwrap_or_else(|| quest.to_string())
}

/// One printable line for an output (None = nothing to show, e.g. forwarded effects).
pub fn render_output(db: &GameDatabase, npc: &str, o: &Output, verbose: bool) -> Option<String> {
    Some(match o {
        Output::Say {
            speaker,
            key,
            text,
            direction,
        } => {
            let t = text.clone().unwrap_or_else(|| format!("[{key}]"));
            let d = if direction.is_empty() || !verbose {
                String::new()
            } else {
                format!(" <{direction}>")
            };
            format!("{}: {t}{d}", who(npc, speaker))
        }
        Output::Svm { speaker, id } => format!("{}: ({id})", who(npc, speaker)),
        Output::Presentation { kind, arg, text } => match text {
            Some(t) => format!("  [{kind}] {t}"),
            None if verbose => format!("  [{kind}] {arg}"),
            None => return None,
        },
        Output::QuestChanged { quest, from, to } => {
            let verb = match to {
                QuestStatus::Running => "Neue Aufgabe",
                QuestStatus::Success => "Aufgabe erfuellt",
                QuestStatus::Failed => "Aufgabe gescheitert",
                _ => "Aufgabe",
            };
            format!(
                "  [Tagebuch] {verb}: {} ({} -> {})",
                quest_title(db, quest),
                from.name(),
                to.name()
            )
        }
        Output::QuestLog { quest, text } => format!(
            "  [Tagebuch] {}: {}",
            quest_title(db, quest),
            text.clone().unwrap_or_default()
        ),
        Output::Xp {
            amount,
            level_before,
            level_after,
        } => {
            if level_after > level_before {
                format!("  [+{amount} EP] Stufenaufstieg: {level_after}")
            } else {
                format!("  [+{amount} EP]")
            }
        }
        Output::Item {
            from,
            to,
            item,
            amount,
        } => {
            let name = db
                .text(&format!("FO_{item}"))
                .or_else(|| db.text(item))
                .unwrap_or_else(|| item.clone());
            if to == "player" {
                format!("  [Inventar] +{amount} {name}")
            } else if from == "player" {
                format!("  [Inventar] -{amount} {name}")
            } else if verbose {
                format!("  [Inventar] {from} -> {to}: {amount} {name}")
            } else {
                return None;
            }
        }
        Output::Message { text } => format!("  [Hinweis] {text}"),
        Output::Effect {
            command,
            entity1,
            arg1,
            ..
        } => {
            if verbose {
                format!("  (Effekt: {command} {entity1} {arg1})")
            } else {
                return None;
            }
        }
        Output::End => return None,
    })
}

pub fn render_result(db: &GameDatabase, npc: &str, r: &ExecResult, verbose: bool) -> Vec<String> {
    r.outputs
        .iter()
        .filter_map(|o| render_output(db, npc, o, verbose))
        .collect()
}

pub fn render_menu(menu: &[Choice]) -> String {
    let mut s = String::new();
    for (i, c) in menu.iter().enumerate() {
        let _ = writeln!(s, "  {}) {}", i + 1, c.label.replace('\n', " "));
    }
    s
}

/// Resolve a scripted pick: 1-based number or case-insensitive label substring.
pub fn resolve_pick<'a>(menu: &'a [Choice], pick: &str) -> Option<&'a Choice> {
    let p = pick.trim();
    if let Ok(n) = p.parse::<usize>() {
        return menu.get(n.checked_sub(1)?);
    }
    let needle = p.to_lowercase();
    menu.iter()
        .find(|c| c.label.to_lowercase().contains(&needle))
}

/// Run a whole conversation non-interactively and return the transcript. Each element
/// of `picks` selects a menu entry; the conversation also ends when the menu is empty.
pub fn run_script(eng: &Engine<'_>, st: &mut GameState, npc: &str, picks: &[&str]) -> String {
    let db = eng.db;
    let mut out = String::new();
    let (mut sess, open) = eng.start(st, npc);
    let _ = writeln!(out, "=== Gespraech mit {npc} ===");
    for l in render_result(db, npc, &open, false) {
        let _ = writeln!(out, "{l}");
    }
    let mut picks = picks.iter();
    loop {
        let menu = eng.menu(st, &sess);
        if sess.ended || menu.is_empty() {
            break;
        }
        let _ = write!(out, "{}", render_menu(&menu));
        let Some(p) = picks.next() else { break };
        let Some(c) = resolve_pick(&menu, p) else {
            let _ = writeln!(out, "> ?? keine Auswahl fuer {p:?}");
            break;
        };
        let _ = writeln!(out, "> {}", c.label.replace('\n', " "));
        let c = c.clone();
        let r = eng.choose(st, &mut sess, &c);
        if let Some(why) = &r.refused {
            let _ = writeln!(out, "  [abgelehnt: {why}]");
        }
        for l in render_result(db, npc, &r, false) {
            let _ = writeln!(out, "{l}");
        }
    }
    let _ = writeln!(out, "=== Ende ===");
    out
}

/// Convenience for tests and tools.
pub fn session_ended(s: &DialogSession) -> bool {
    s.ended
}
