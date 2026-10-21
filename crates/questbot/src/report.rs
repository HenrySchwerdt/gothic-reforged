//! Trace and report types (format documented in `docs/QUESTBOT.md`).
use gothic_gameplay::conditions::Unmet;
use gothic_gameplay::dialog::PLAYER_OWNER;
use gothic_gameplay::model::{Cmd, InfoCond, QuestStatus};
use gothic_gameplay::state::norm;
use gothic_gameplay::{Engine, GameDatabase, GameState};
use serde::Serialize;
use std::fmt::Write as _;

/// Report schema identifier (bump when the JSON layout changes).
pub const SCHEMA: &str = "gothic-questbot/1";

/// A world objective the bot fakes instead of playing it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "cheat", rename_all = "snake_case")]
pub enum Cheat {
    GiveItems {
        item: String,
        amount: u32,
        quest: String,
    },
    Kill {
        npc: String,
        quest: String,
    },
    Defeat {
        npc: String,
        quest: String,
    },
    Enter {
        who: String,
        area: String,
        quest: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// A dialog choice (`info` = `None` for the automatic part of starting a dialog).
    /// `ran` lists every info that executed, including ones the dialog rules auto-ran after
    /// the choice (single remaining entries, Important infos).
    Talk {
        npc: String,
        info: Option<String>,
        label: String,
        ran: Vec<String>,
    },
    Cheat(Cheat),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QuestChange {
    pub quest: String,
    pub from: QuestStatus,
    pub to: QuestStatus,
}

/// One recorded action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TraceStep {
    pub step: usize,
    /// Game clock (hours) when it happened.
    pub hours: u64,
    pub action: Action,
    pub quest_changes: Vec<QuestChange>,
}

/// Why one info that would advance a stuck quest is not available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Blocker {
    pub info: String,
    pub owner: String,
    pub condition: String,
    /// The unmet condition (`Unmet` from the dialog rules), e.g. `Event("X")`.
    pub unmet: String,
    /// What would fix it, as far as the data tells.
    pub hint: Option<String>,
}

/// Stable reason codes of a stuck quest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StuckReason {
    /// A `FinishedQuests` prerequisite is not finished.
    PrerequisiteUnfinished,
    /// Still `Open`: no activating info is available.
    NotActivated,
    /// `Running`, but no info that completes it is available and no cheat applies.
    ObjectiveUnmet,
    /// Nothing in the data can start the quest (no activator info, no `RunQuest`).
    NoActivator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StuckQuest {
    pub quest: String,
    pub status: QuestStatus,
    pub reason: StuckReason,
    /// Unfinished prerequisites (`FinishedQuests`), if any.
    pub prerequisites: Vec<String>,
    pub blockers: Vec<Blocker>,
}

/// A target quest that finished, but not successfully.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unsuccessful {
    pub quest: String,
    pub status: QuestStatus,
}

/// The bot's result.
#[derive(Debug, Clone, Serialize)]
pub struct BotReport {
    pub schema: &'static str,
    pub targets: usize,
    pub completed: Vec<String>,
    pub unsuccessful: Vec<Unsuccessful>,
    pub stuck: Vec<StuckQuest>,
    pub rounds: usize,
    pub trace: Vec<TraceStep>,
}

impl BotReport {
    pub fn new(
        db: &GameDatabase,
        st: &GameState,
        targets: &[String],
        rounds: usize,
        trace: Vec<TraceStep>,
        stuck: Vec<StuckQuest>,
    ) -> Self {
        let mut completed = Vec::new();
        let mut unsuccessful = Vec::new();
        for q in targets {
            let name = db.quest_by_name(q).map_or(q.clone(), |x| x.name.clone());
            match st.quest_status(q) {
                QuestStatus::Success | QuestStatus::Won => completed.push(name),
                s if s.is_finished() => unsuccessful.push(Unsuccessful {
                    quest: name,
                    status: s,
                }),
                _ => {}
            }
        }
        Self {
            schema: SCHEMA,
            targets: targets.len(),
            completed,
            unsuccessful,
            stuck,
            rounds,
            trace,
        }
    }

    /// All targets completed successfully.
    pub fn all_completed(&self) -> bool {
        self.completed.len() == self.targets
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("report serialises")
    }

    /// Human-readable summary (trace, then stuck quests).
    pub fn to_text(&self, with_trace: bool) -> String {
        let mut o = String::new();
        let _ = writeln!(
            o,
            "questbot: {}/{} quests completed, {} unsuccessful, {} stuck, {} rounds, {} steps",
            self.completed.len(),
            self.targets,
            self.unsuccessful.len(),
            self.stuck.len(),
            self.rounds,
            self.trace.len()
        );
        if with_trace {
            for s in &self.trace {
                let what = match &s.action {
                    Action::Talk {
                        npc,
                        info,
                        label,
                        ran,
                    } => format!(
                        "talk {npc}: {} \"{label}\" ran [{}]",
                        info.as_deref().unwrap_or("(start)"),
                        ran.join(", ")
                    ),
                    Action::Cheat(c) => format!("cheat {c:?}"),
                };
                let _ = write!(o, "  {:>4} [{:>4}h] {what}", s.step, s.hours);
                for c in &s.quest_changes {
                    let _ = write!(o, "  => {} {}->{}", c.quest, c.from.name(), c.to.name());
                }
                o.push('\n');
            }
        }
        for u in &self.unsuccessful {
            let _ = writeln!(o, "UNSUCCESSFUL {} ({})", u.quest, u.status.name());
        }
        for s in &self.stuck {
            let _ = writeln!(
                o,
                "STUCK {} [{}] {:?}{}",
                s.quest,
                s.status.name(),
                s.reason,
                if s.prerequisites.is_empty() {
                    String::new()
                } else {
                    format!(" needs {}", s.prerequisites.join(", "))
                }
            );
            for b in &s.blockers {
                let _ = writeln!(
                    o,
                    "    {} ({}, {}): {}{}",
                    b.info,
                    b.owner,
                    b.condition,
                    b.unmet,
                    b.hint
                        .as_ref()
                        .map_or(String::new(), |h| format!(" -- {h}"))
                );
            }
        }
        o
    }
}

/// Infos whose script sets `event`.
fn setters(db: &GameDatabase, event: &str) -> Vec<String> {
    db.infos
        .iter()
        .filter(|i| {
            i.script.iter().any(|s| {
                Cmd::parse(&s.command) == Some(Cmd::SetGameEvent)
                    && s.id1.eq_ignore_ascii_case(event)
            })
        })
        .map(|i| i.name.clone())
        .collect()
}

/// Why a setter info has not run: already told (it ran, so the event was set and cleared
/// again, or the script line is conditional), available now, or its own unmet condition.
fn setter_state(db: &GameDatabase, engine: &Engine<'_>, st: &GameState, name: &str) -> String {
    let Some(id) = db.info_id(name) else {
        return format!("{name} (missing)");
    };
    let info = db.info(id);
    if st.told.contains(&norm(&info.name)) {
        return format!("{name} (already told)");
    }
    let partner = if info.owner.eq_ignore_ascii_case(PLAYER_OWNER) {
        db.owners()
            .find(|o| !o.eq_ignore_ascii_case(PLAYER_OWNER))
            .unwrap_or("")
            .to_string()
    } else {
        info.owner.clone()
    };
    match engine.check(st, id, &partner) {
        Ok(()) => format!("{name} @{} (available, never chosen)", info.owner),
        Err(u) => format!("{name} @{} (unmet: {u:?})", info.owner),
    }
}

fn hint(db: &GameDatabase, engine: &Engine<'_>, st: &GameState, unmet: &Unmet) -> Option<String> {
    Some(match unmet {
        Unmet::Event(e) => {
            let s = setters(db, e);
            if s.is_empty() {
                format!("no info sets game event {e}")
            } else {
                let states: Vec<String> =
                    s.iter().map(|n| setter_state(db, engine, st, n)).collect();
                format!("event {e} is set by {}", states.join(", "))
            }
        }
        Unmet::PrerequisiteQuest(p) => format!("quest {p} is {}", st.quest_status(p).name()),
        Unmet::QuestStatus { quest, have, need } => {
            format!("quest {quest} is {} (needs {need})", have.name())
        }
        Unmet::NotTriggered(what) => format!("needs a world/engine trigger: {what}"),
        Unmet::Flag(f) => format!("NPC flag {f} is not set by any script run so far"),
        Unmet::Items(i) => format!("item {i} is missing"),
        Unmet::MissingQuest(q) => format!("quest {q:?} does not exist"),
        _ => return None,
    })
}

/// Explain why each unfinished target cannot advance.
pub fn analyse_stuck(
    db: &GameDatabase,
    engine: &Engine<'_>,
    st: &GameState,
    targets: &[String],
) -> Vec<StuckQuest> {
    let mut out = Vec::new();
    for q in targets {
        let status = st.quest_status(q);
        if status.is_finished() {
            continue;
        }
        let quest = db.quest_by_name(q);
        let prerequisites: Vec<String> = quest
            .map(|x| {
                x.finished_quests
                    .iter()
                    .filter(|p| db.quest_by_name(p).is_some() && !st.quest_status(p).is_finished())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let wanted = |cond: InfoCond| match status {
            QuestStatus::Open => {
                matches!(cond, InfoCond::Activator | InfoCond::Open | InfoCond::Ready)
            }
            _ => matches!(
                cond,
                InfoCond::Delivery | InfoCond::PartDelivery | InfoCond::Running | InfoCond::Ready
            ),
        };
        let mut blockers = Vec::new();
        for &id in db.infos_of_quest(q) {
            let info = db.info(id);
            let cond = InfoCond::from_raw(info.condition_type);
            if !wanted(cond) {
                continue;
            }
            let partner = if info.owner.eq_ignore_ascii_case(PLAYER_OWNER) {
                db.owners()
                    .find(|o| !o.eq_ignore_ascii_case(PLAYER_OWNER))
                    .unwrap_or("")
                    .to_string()
            } else {
                info.owner.clone()
            };
            if let Err(u) = engine.check(st, id, &partner) {
                blockers.push(Blocker {
                    info: info.name.clone(),
                    owner: info.owner.clone(),
                    condition: format!("{cond:?}"),
                    unmet: format!("{u:?}"),
                    hint: hint(db, engine, st, &u),
                });
            }
        }
        let scripted_start = db.infos.iter().any(|i| {
            i.script
                .iter()
                .any(|s| Cmd::parse(&s.command) == Some(Cmd::RunQuest) && norm(&s.id1) == norm(q))
        });
        let has_activator = db.infos_of_quest(q).iter().any(|&id| {
            matches!(
                InfoCond::from_raw(db.info(id).condition_type),
                InfoCond::Activator | InfoCond::Reactivator
            )
        });
        let reason = if !prerequisites.is_empty() {
            StuckReason::PrerequisiteUnfinished
        } else if status == QuestStatus::Open && !has_activator && !scripted_start {
            StuckReason::NoActivator
        } else if status == QuestStatus::Open {
            StuckReason::NotActivated
        } else {
            StuckReason::ObjectiveUnmet
        };
        out.push(StuckQuest {
            quest: quest.map_or(q.clone(), |x| x.name.clone()),
            status,
            reason,
            prerequisites,
            blockers,
        });
    }
    out
}
