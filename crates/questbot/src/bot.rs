//! The bot: plays the Bevy-free quest/dialog core until every target quest is finished or no
//! action makes progress any more.
//!
//! Each round:
//! 1. **World cheats** for running quests whose objective lies outside dialogs: give the
//!    delivery items of item quests, kill/defeat the listed NPCs, enter the destination area
//!    (each at most once per quest, recorded in the trace as `Cheat`);
//! 2. **Plan**: rank every NPC by the most useful info it can offer now
//!    ([`crate::plan::Planner`]: infos of unfinished target quests first, then infos that set
//!    events, start or finish quests that unlock targets, then unexplored talk);
//! 3. **Talk** to the best NPC: open the dialog and keep choosing the most useful unchosen
//!    entry (descending into dialog trees) until nothing useful is left, then leave.
//!
//! A round in which the serialised game state does not change means the bot is stuck; the
//! unfinished targets are then analysed for their blockers ([`crate::report`]).
use crate::plan::Planner;
use crate::report::{Action, BotReport, Cheat, QuestChange, TraceStep, analyse_stuck};
use gothic_gameplay::conditions::is_item_name;
use gothic_gameplay::dialog::PLAYER_OWNER;
use gothic_gameplay::model::{QuestStatus, QuestType};
use gothic_gameplay::script::{self, ExecResult, Output};
use gothic_gameplay::state::norm;
use gothic_gameplay::{Assumptions, Engine, GameDatabase, GameState};
use std::collections::BTreeSet;

/// Bot options.
#[derive(Debug, Clone)]
pub struct BotOptions {
    /// Quests to finish (`None` = every quest in the database).
    pub targets: Option<Vec<String>>,
    /// Dialog assumptions about the (unsimulated) world.
    pub assumptions: Assumptions,
    /// Hard cap on rounds.
    pub max_rounds: usize,
    /// Hard cap on menu choices per conversation.
    pub max_choices: usize,
    /// Allow the world cheats (items, kills, defeats, areas).
    pub cheats: bool,
}

impl Default for BotOptions {
    fn default() -> Self {
        Self {
            targets: None,
            assumptions: Assumptions::PERMISSIVE,
            max_rounds: 10_000,
            max_choices: 64,
            cheats: true,
        }
    }
}

/// The quest bot.
pub struct QuestBot<'a> {
    pub db: &'a GameDatabase,
    pub state: GameState,
    pub options: BotOptions,
    engine: Engine<'a>,
    planner: Planner,
    trace: Vec<TraceStep>,
    cheated: BTreeSet<(String, &'static str)>,
}

impl<'a> QuestBot<'a> {
    pub fn new(db: &'a GameDatabase, options: BotOptions) -> Self {
        let targets: Vec<String> = options
            .targets
            .clone()
            .unwrap_or_else(|| db.quests.iter().map(|q| q.name.clone()).collect());
        Self {
            db,
            state: GameState::new(),
            engine: Engine::with(db, options.assumptions),
            planner: Planner::new(db, &targets),
            options,
            trace: Vec::new(),
            cheated: BTreeSet::new(),
        }
    }

    fn targets(&self) -> &[String] {
        self.planner.targets()
    }

    fn all_finished(&self) -> bool {
        self.targets()
            .iter()
            .all(|q| self.state.quest_status(q).is_finished())
    }

    /// Play until done or stuck.
    pub fn run(mut self) -> BotReport {
        let mut rounds = 0;
        let mut stuck = false;
        let mut idle = 0;
        while !self.all_finished() {
            if rounds >= self.options.max_rounds {
                stuck = true;
                break;
            }
            rounds += 1;
            let before = fingerprint(&self.state);
            if self.options.cheats {
                self.world_cheats();
            }
            if !self.all_finished() {
                self.talk_round();
            }
            // Time passes between actions; an idle round waits a few hours more, since
            // `QuestRunning` infos need two hours since their quest started.
            if fingerprint(&self.state) == before {
                idle += 1;
                if idle >= IDLE_ROUNDS {
                    stuck = true;
                    break;
                }
                self.state.hours += 3;
            } else {
                idle = 0;
                self.state.hours += 1;
            }
        }
        let stuck_quests = if stuck {
            analyse_stuck(self.db, &self.engine, &self.state, self.targets())
        } else {
            Vec::new()
        };
        let targets = self.targets().to_vec();
        BotReport::new(
            self.db,
            &self.state,
            &targets,
            rounds,
            self.trace,
            stuck_quests,
        )
    }

    fn record(&mut self, action: Action, res: &ExecResult) {
        let changes: Vec<QuestChange> = res
            .outputs
            .iter()
            .filter_map(|o| match o {
                Output::QuestChanged { quest, from, to } => Some(QuestChange {
                    quest: quest.clone(),
                    from: *from,
                    to: *to,
                }),
                _ => None,
            })
            .collect();
        self.trace.push(TraceStep {
            step: self.trace.len() + 1,
            hours: self.state.hours,
            action,
            quest_changes: changes,
        });
    }

    /// Objectives outside dialogs, for running quests.
    fn world_cheats(&mut self) {
        let db = self.db;
        let running: Vec<&gothic_formats::quest::Quest> = db
            .quests
            .iter()
            .filter(|q| self.state.quest_status(&q.name) == QuestStatus::Running)
            .collect();
        for q in running {
            let ty = QuestType::from_raw(q.quest_type);
            let kind: &'static str = match ty {
                QuestType::HasItems => "items",
                QuestType::Kill | QuestType::DriveAway => "kill",
                QuestType::Defeat | QuestType::Arena | QuestType::Sparring => "defeat",
                QuestType::EnterArea => "enter",
                _ => continue,
            };
            if !self.cheated.insert((norm(&q.name), kind)) {
                continue;
            }
            for (i, entity) in q.delivery_entities.iter().enumerate() {
                let need = q.delivery_amounts.get(i).copied().unwrap_or(1).max(1);
                match ty {
                    QuestType::HasItems if is_item_name(db, entity) => {
                        let have = self.state.item_count(entity);
                        if have < need {
                            self.state.add_item(entity, need - have);
                            self.record(
                                Action::Cheat(Cheat::GiveItems {
                                    item: entity.clone(),
                                    amount: need - have,
                                    quest: q.name.clone(),
                                }),
                                &ExecResult::default(),
                            );
                        }
                    }
                    QuestType::Kill | QuestType::DriveAway => {
                        for _ in 0..need {
                            let r = script::npc_killed(db, &mut self.state, entity);
                            self.record(
                                Action::Cheat(Cheat::Kill {
                                    npc: entity.clone(),
                                    quest: q.name.clone(),
                                }),
                                &r,
                            );
                        }
                    }
                    QuestType::Defeat | QuestType::Arena | QuestType::Sparring => {
                        for _ in 0..need {
                            let r = script::npc_defeated(db, &mut self.state, entity);
                            self.record(
                                Action::Cheat(Cheat::Defeat {
                                    npc: entity.clone(),
                                    quest: q.name.clone(),
                                }),
                                &r,
                            );
                        }
                    }
                    QuestType::EnterArea if i == 0 => {
                        let r = script::area_entered(
                            db,
                            &mut self.state,
                            entity,
                            &q.destination_entity,
                        );
                        self.record(
                            Action::Cheat(Cheat::Enter {
                                who: entity.clone(),
                                area: q.destination_entity.clone(),
                                quest: q.name.clone(),
                            }),
                            &r,
                        );
                    }
                    _ => {}
                }
            }
        }
    }

    /// Talk to the NPC with the most useful offer, if any.
    fn talk_round(&mut self) {
        let mut npcs: Vec<String> = self
            .db
            .owners()
            .filter(|o| !o.eq_ignore_ascii_case(PLAYER_OWNER))
            .map(str::to_string)
            .collect();
        npcs.sort();
        let best = npcs
            .iter()
            .filter_map(|npc| {
                let score = self
                    .engine
                    .root_list(&self.state, npc)
                    .into_iter()
                    .map(|id| self.planner.score(self.db, &self.state, id))
                    .max()?;
                (score > 0).then_some((score, npc.clone()))
            })
            // Highest score, ties by name (deterministic).
            .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)));
        if let Some((_, npc)) = best {
            self.converse(&npc);
        }
    }

    /// Infos told since `before` (what actually ran), in database order.
    fn ran_since(&self, before: &BTreeSet<String>) -> Vec<String> {
        self.db
            .infos
            .iter()
            .filter(|i| {
                let n = norm(&i.name);
                self.state.told.contains(&n) && !before.contains(&n)
            })
            .map(|i| i.name.clone())
            .collect()
    }

    fn converse(&mut self, npc: &str) {
        let told = self.state.told.clone();
        let (mut session, res) = self.engine.start(&mut self.state, npc);
        if !res.outputs.is_empty() {
            let ran = self.ran_since(&told);
            self.record(
                Action::Talk {
                    npc: npc.to_string(),
                    info: None,
                    label: "(dialog start)".into(),
                    ran,
                },
                &res,
            );
        }
        let mut chosen: BTreeSet<String> = BTreeSet::new();
        for _ in 0..self.options.max_choices {
            if session.ended {
                break;
            }
            let menu = self.engine.menu(&self.state, &session);
            let pick = menu
                .iter()
                .filter_map(|c| c.info.map(|id| (c, id)))
                .filter(|(_, id)| !chosen.contains(&self.db.info(*id).name))
                .map(|(c, id)| (self.planner.score(self.db, &self.state, id), c))
                .filter(|(s, _)| *s > 0)
                .max_by_key(|(s, _)| *s)
                .map(|(_, c)| c.clone());
            let Some(choice) = pick else {
                // Nothing useful: leave (or step out of a child menu with its first entry).
                if let Some(leave) = menu.iter().find(|c| c.info.is_none()) {
                    self.engine.choose(&mut self.state, &mut session, leave);
                } else if let Some(first) = menu.first() {
                    let told = self.state.told.clone();
                    let res = self.engine.choose(&mut self.state, &mut session, first);
                    let ran = self.ran_since(&told);
                    self.record(
                        Action::Talk {
                            npc: npc.to_string(),
                            info: first.info.map(|i| self.db.info(i).name.clone()),
                            label: first.label.clone(),
                            ran,
                        },
                        &res,
                    );
                    continue;
                }
                break;
            };
            let id = choice.info.expect("info choice");
            chosen.insert(self.db.info(id).name.clone());
            let told = self.state.told.clone();
            let res = self.engine.choose(&mut self.state, &mut session, &choice);
            let ran = self.ran_since(&told);
            self.record(
                Action::Talk {
                    npc: npc.to_string(),
                    info: Some(self.db.info(id).name.clone()),
                    label: choice.label.clone(),
                    ran,
                },
                &res,
            );
        }
    }
}

/// Rounds without any state change before the bot gives up.
const IDLE_ROUNDS: usize = 2;

/// Serialised state without the clock (waiting is not progress).
fn fingerprint(st: &GameState) -> String {
    let mut s = st.clone();
    s.hours = 0;
    serde_json::to_string(&s).unwrap_or_default()
}

/// Run the bot with options and return its report.
pub fn play(db: &GameDatabase, options: BotOptions) -> BotReport {
    QuestBot::new(db, options).run()
}
