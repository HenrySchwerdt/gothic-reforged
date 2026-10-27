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
//! Progress is any change of the serialised game state (the clock aside). An info chosen
//! without any change (a permanent refusal, an already-delivered report) is *futile* until the
//! next progress and no longer counts for planning, so the bot moves on to other NPCs instead
//! of repeating it. Without progress, every NPC with something useful is visited once; when
//! none is left the clock jumps a day ahead (time-gated infos) and the sweep repeats once. Only
//! then is the bot stuck, and the unfinished targets are analysed for their blockers
//! ([`crate::report`]).
use crate::plan::{self as planner, Planner};
use crate::report::{Action, BotReport, Cheat, QuestChange, TraceStep, analyse_stuck};
use gothic_gameplay::conditions::is_item_name;
use gothic_gameplay::dialog::PLAYER_OWNER;
use gothic_gameplay::model::{Cmd, InfoCond, QuestStatus, QuestType};
use gothic_gameplay::questmgr;
use gothic_gameplay::script::{self, ExecResult, Output};
use gothic_gameplay::state::{GOLD_ITEM, norm};
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
    /// Quests nothing in the data can start (no activator info, no `RunQuest`): the quest
    /// manager hooks act on `Open` quests too (`GAME_RULES.md` 2.4, `CheckDeliveryEntitiesStatus`
    /// runs for Open/Running/Lost), so the world objective alone completes them.
    orphans: BTreeSet<String>,
    /// Infos chosen without effect since the last progress (normalised names).
    futile: BTreeSet<String>,
    /// NPCs talked to since the last progress.
    visited: BTreeSet<String>,
    /// Whether infos that close or fail route quests may be taken (only once nothing else is
    /// left, see [`Planner::score`]).
    allow_spoilers: bool,
    /// Per-NPC best score of the own (and partner-dependent hero) infos, valid for the state
    /// and clock it was computed in.
    cache: ScoreCache,
}

#[derive(Default)]
struct ScoreCache {
    key: (u64, u64, bool),
    scores: std::collections::HashMap<String, i32>,
}

impl<'a> QuestBot<'a> {
    pub fn new(db: &'a GameDatabase, options: BotOptions) -> Self {
        let targets: Vec<String> = options
            .targets
            .clone()
            .unwrap_or_else(|| db.quests.iter().map(|q| q.name.clone()).collect());
        let started: BTreeSet<String> = db
            .infos
            .iter()
            .flat_map(|i| i.script.iter())
            .filter(|s| Cmd::parse(&s.command) == Some(Cmd::RunQuest))
            .map(|s| norm(&s.id1))
            .collect();
        let orphans = db
            .quests
            .iter()
            .filter(|q| {
                !started.contains(&norm(&q.name))
                    && !db.infos_of_quest(&q.name).iter().any(|&id| {
                        matches!(
                            InfoCond::from_raw(db.info(id).condition_type),
                            InfoCond::Activator | InfoCond::Reactivator
                        )
                    })
            })
            .map(|q| norm(&q.name))
            .collect();
        Self {
            db,
            orphans,
            state: GameState::new(),
            engine: Engine::with(db, options.assumptions),
            planner: Planner::new(db, &targets),
            options,
            trace: Vec::new(),
            cheated: BTreeSet::new(),
            futile: BTreeSet::new(),
            visited: BTreeSet::new(),
            cache: ScoreCache::default(),
            allow_spoilers: false,
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
        // Whether the clock already jumped ahead since the last progress.
        let mut waited = false;
        // Every state seen at the end of a round: returning to one (two permanent lines
        // swapping a status back and forth, e.g. the companion quests that make each other
        // `Lost`) is not progress.
        let mut seen: BTreeSet<u64> = BTreeSet::new();
        seen.insert(fingerprint(&self.state));
        while !self.all_finished() {
            if rounds >= self.options.max_rounds {
                stuck = true;
                break;
            }
            rounds += 1;
            let mut pick = None;
            if !self.all_finished() {
                if self.options.cheats {
                    self.obtain_required_items();
                }
                pick = self.pick_npc();
                // The world objectives (kills, pickups, ...) come once the dialogs on the
                // route are exhausted: killing an NPC earlier would take away the lines that
                // need it alive (`Falk_HuntSnapper`), finishing a quest earlier those that need
                // it Open or Running (`Hassan_Murder`).
                if self.options.cheats && pick.as_ref().is_none_or(|(s, _)| *s <= planner::EXPLORE)
                {
                    self.world_cheats();
                    self.satisfy_npc_conditions();
                    pick = self.pick_npc();
                }
            }
            let talked = pick.is_some();
            if let Some((_, npc)) = pick {
                self.talk_to(&npc);
            }
            if seen.insert(fingerprint(&self.state)) {
                self.futile.clear();
                self.visited.clear();
                self.allow_spoilers = false;
                waited = false;
                self.state.hours += 1;
                continue;
            }
            // No progress. The clock stays put while the sweep goes on (scores are cached per
            // state); `QuestRunning` / `Overtime` infos get their time from the day jump below.
            if talked {
                continue;
            }
            // Nobody left with anything useful: take the lines that close route quests, then
            // wait a day once and sweep again.
            if !self.allow_spoilers {
                self.allow_spoilers = true;
                self.visited.clear();
                self.futile.clear();
                continue;
            }
            if waited {
                stuck = true;
                break;
            }
            waited = true;
            self.state.hours += WAIT_HOURS;
            self.futile.clear();
            self.visited.clear();
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

    /// Route infos that need another NPC in a state the player can bring about: defeated or
    /// dead (a fight), or talked to (a conversation).
    fn satisfy_npc_conditions(&mut self) {
        use gothic_gameplay::conditions::Unmet;
        let db = self.db;
        let route = self.planner.route_infos().to_vec();
        for id in route {
            let info = db.info(id);
            let friend = matches!(
                InfoCond::from_raw(info.condition_type),
                InfoCond::EnclaveFriend | InfoCond::PoliticalFriend
            );
            if (info.cond_secondary_npc.is_empty() && !friend)
                || (!info.quest.is_empty() && self.state.quest_status(&info.quest).is_finished())
            {
                continue;
            }
            for _ in 0..=info.cond_secondary_npc.len() {
                let partner = if info.owner.eq_ignore_ascii_case(PLAYER_OWNER) {
                    ""
                } else {
                    info.owner.as_str()
                };
                let (npc, status) = match self.engine.check(&self.state, id, partner) {
                    Err(Unmet::NpcStatus { npc, status }) => (npc, status),
                    // Reputation lines (`EnclaveFriend` / `PoliticalFriend`): the player earns
                    // the fame with the faction's quests first.
                    Err(Unmet::Flag(what)) if what.contains("fame >= 75") => {
                        let key = if what.starts_with("Enclave") {
                            "enc:friends"
                        } else {
                            "pol:friends"
                        };
                        self.state.add_fame(key, 75);
                        self.record(
                            Action::Cheat(Cheat::Earn {
                                what: format!("fame ({key})"),
                                amount: 75,
                                info: info.name.clone(),
                            }),
                            &ExecResult::default(),
                        );
                        continue;
                    }
                    _ => break,
                };
                match status {
                    2 => {
                        let r = script::npc_defeated(db, &mut self.state, &npc);
                        self.record(
                            Action::Cheat(Cheat::Defeat {
                                npc: npc.clone(),
                                quest: info.quest.clone(),
                            }),
                            &r,
                        );
                    }
                    3 => {
                        let r = script::npc_killed(db, &mut self.state, &npc);
                        self.record(
                            Action::Cheat(Cheat::Kill {
                                npc: npc.clone(),
                                quest: info.quest.clone(),
                            }),
                            &r,
                        );
                    }
                    4 => {
                        if db.infos_of(&npc).is_empty() {
                            self.state.npc_mut(&npc).talked = true;
                        } else {
                            self.converse(&npc);
                        }
                    }
                    // Alive / not defeated / not talked to: nothing to do (or too late).
                    _ => break,
                }
            }
        }
    }

    /// Items the player must carry for a route info to be available (a delivery asking for
    /// raw meat, a quest line requiring a key): the player hunts / buys / finds them.
    fn obtain_required_items(&mut self) {
        let db = self.db;
        let route = self.planner.route_infos().to_vec();
        for id in route {
            let info = db.info(id);
            if info.cond_items.is_empty() {
                continue;
            }
            let qs = self.state.quest_status(&info.quest);
            if !info.quest.is_empty() && qs.is_finished() {
                continue;
            }
            for _ in 0..info.cond_items.len() {
                let partner = if info.owner.eq_ignore_ascii_case(PLAYER_OWNER) {
                    ""
                } else {
                    info.owner.as_str()
                };
                let Err(gothic_gameplay::conditions::Unmet::Items(item)) =
                    self.engine.check(&self.state, id, partner)
                else {
                    break;
                };
                let idx = info.cond_items.iter().position(|i| *i == item).unwrap_or(0);
                let container = info
                    .cond_item_container
                    .get(idx)
                    .or_else(|| info.cond_item_container.first());
                if !container.is_some_and(|c| {
                    c.eq_ignore_ascii_case("pc_hero") || c.eq_ignore_ascii_case("player")
                }) {
                    break;
                }
                let need = info.cond_item_amounts.get(idx).copied().unwrap_or(1).max(1);
                let have = self.state.item_count(&item);
                if have >= need {
                    break;
                }
                self.state.add_item(&item, need - have);
                self.record(
                    Action::Cheat(Cheat::GiveItems {
                        item: item.clone(),
                        amount: need - have,
                        quest: info.quest.clone(),
                    }),
                    &ExecResult::default(),
                );
            }
        }
    }

    /// Objectives outside dialogs, for running quests (and quests without any starter).
    ///
    /// Each is the player action whose quest manager hook the original runs: kills
    /// (`OnNPCKilled`), defeats (`OnNPCDefeated`), entering an area (`OnEnter`), picking items
    /// up (`OnReceiveItem`), freeing an enclave (`OnEnclaveStateChanged`), casting a spell
    /// (`OnSpellCast`).
    fn world_cheats(&mut self) {
        let db = self.db;
        let running: Vec<&gothic_formats::quest::Quest> = db
            .quests
            .iter()
            .filter(|q| match self.state.quest_status(&q.name) {
                QuestStatus::Running => true,
                QuestStatus::Open => {
                    self.planner.is_relevant(&q.name) && self.orphans.contains(&norm(&q.name))
                }
                _ => false,
            })
            .collect();
        for q in running {
            let ty = QuestType::from_raw(q.quest_type);
            let running_now = self.state.quest_status(&q.name) == QuestStatus::Running;
            let kind: &'static str = match ty {
                QuestType::HasItems | QuestType::Plunder => "items",
                QuestType::Kill | QuestType::DriveAway => "kill",
                QuestType::Defeat | QuestType::Arena | QuestType::Sparring => "defeat",
                QuestType::EnterArea => "enter",
                QuestType::FreeEnclave => "enclave",
                QuestType::SpellCast => "cast",
                _ => continue,
            };
            if !self.cheated.insert((norm(&q.name), kind)) {
                continue;
            }
            let has_delivery = db.infos_of_quest(&q.name).iter().any(|&id| {
                matches!(
                    InfoCond::from_raw(db.info(id).condition_type),
                    InfoCond::Delivery | InfoCond::PartDelivery
                )
            });
            for (i, entity) in q.delivery_entities.iter().enumerate() {
                let need = q.delivery_amounts.get(i).copied().unwrap_or(1).max(1);
                match ty {
                    // Handed in by a Delivery info: the items sit in the inventory.
                    QuestType::HasItems
                        if running_now && has_delivery && is_item_name(db, entity) =>
                    {
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
                    // Items that reach the quest destination by themselves (the player's
                    // pickup, `OnReceiveItem`); `Plunder` takes them from the destination.
                    QuestType::HasItems | QuestType::Plunder => {
                        let (from, to) = if ty == QuestType::Plunder {
                            (q.destination_entity.as_str(), "PC_Hero")
                        } else {
                            ("", q.destination_entity.as_str())
                        };
                        let mut r = ExecResult::default();
                        for _ in 0..need {
                            questmgr::on_receive_item(
                                db,
                                &mut self.state,
                                from,
                                to,
                                entity,
                                1,
                                &mut r,
                            );
                        }
                        self.record(
                            Action::Cheat(Cheat::Pickup {
                                item: entity.clone(),
                                amount: need,
                                quest: q.name.clone(),
                            }),
                            &r,
                        );
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
                    QuestType::FreeEnclave => {
                        let mut r = ExecResult::default();
                        questmgr::on_enclave_state_changed(db, &mut self.state, entity, &mut r);
                        self.record(
                            Action::Cheat(Cheat::FreeEnclave {
                                enclave: entity.clone(),
                                quest: q.name.clone(),
                            }),
                            &r,
                        );
                    }
                    QuestType::SpellCast => {
                        let mut r = ExecResult::default();
                        questmgr::on_spell_cast(db, &mut self.state, entity, &q.spell_name, &mut r);
                        self.record(
                            Action::Cheat(Cheat::Cast {
                                target: entity.clone(),
                                spell: q.spell_name.clone(),
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

    /// Planner score of an info, or 0 if it was futile since the last progress.
    fn score(&self, id: gothic_gameplay::InfoId) -> i32 {
        if self.futile.contains(&norm(&self.db.info(id).name)) {
            return 0;
        }
        self.planner
            .score(self.db, &self.state, id, self.allow_spoilers)
    }

    /// The not yet visited NPC with the most useful offer, with its score.
    fn pick_npc(&mut self) -> Option<(i32, String)> {
        let mut npcs: Vec<String> = self
            .db
            .owners()
            .filter(|o| !o.eq_ignore_ascii_case(PLAYER_OWNER))
            .filter(|o| !DEBUG_OWNERS.iter().any(|d| o.eq_ignore_ascii_case(d)))
            .filter(|o| !self.visited.contains(&norm(o)))
            .map(str::to_string)
            .collect();
        npcs.sort();
        // `PC_Hero` infos are offered to every NPC; the ones that do not depend on the partner
        // are evaluated once per round instead of once per NPC.
        let mut hero_best = 0;
        let mut hero_dependent = Vec::new();
        for id in self.engine.hero_root_candidates() {
            if self.engine.hero_info_depends_on_partner(id) {
                hero_dependent.push(id);
            } else if self.engine.is_available(&self.state, id, "") {
                hero_best = hero_best.max(self.score(id));
            }
        }
        let key = (
            fingerprint(&self.state),
            self.state.hours,
            self.allow_spoilers,
        );
        if self.cache.key != key {
            self.cache.key = key;
            self.cache.scores.clear();
        }
        let mut best: Option<(i32, String)> = None;
        for npc in &npcs {
            let own = match self.cache.scores.get(npc) {
                Some(&v) => v,
                None => {
                    let own_infos = self.engine.own_root_available(&self.state, npc);
                    // Approaching an NPC runs its `Important` / `News` / `QuestRunning` /
                    // `Refuse` line by itself: one that closes a quest of the route keeps the
                    // bot away (until nothing else is left).
                    let autorun_spoils = !self.allow_spoilers
                        && own_infos.iter().any(|&i| {
                            let info = self.db.info(i);
                            matches!(
                                gothic_gameplay::model::InfoType::from_raw(info.info_type),
                                gothic_gameplay::model::InfoType::Important
                                    | gothic_gameplay::model::InfoType::News
                                    | gothic_gameplay::model::InfoType::QuestRunning
                                    | gothic_gameplay::model::InfoType::Refuse
                            ) && self.planner.spoils(&self.state, info)
                        });
                    let v = if autorun_spoils {
                        0
                    } else {
                        own_infos
                            .into_iter()
                            .chain(
                                hero_dependent
                                    .iter()
                                    .copied()
                                    .filter(|&i| self.engine.is_available(&self.state, i, npc)),
                            )
                            .map(|id| self.score(id))
                            .max()
                            .unwrap_or(0)
                    };
                    self.cache.scores.insert(npc.clone(), v);
                    v
                }
            };
            let score = own.max(hero_best);
            if score <= 0 {
                continue;
            }
            // Highest score, ties by name (deterministic; `npcs` is sorted).
            if best.as_ref().is_none_or(|(b, _)| score > *b) {
                best = Some((score, npc.clone()));
            }
        }
        best
    }

    /// Talk to `npc` (the pick of [`Self::pick_npc`]).
    fn talk_to(&mut self, npc: &str) {
        self.visited.insert(norm(npc));
        self.converse(npc);
    }

    /// Does choosing this info end the conversation (an `End` script line, an `End` /
    /// `NothingToSay` condition, a `Refuse` info)?
    fn ends_dialog(&self, id: gothic_gameplay::InfoId) -> bool {
        use gothic_gameplay::model::InfoType;
        let info = self.db.info(id);
        matches!(
            InfoCond::from_raw(info.condition_type),
            InfoCond::End | InfoCond::NothingToSay
        ) || InfoType::from_raw(info.info_type) == InfoType::Refuse
            || info
                .script
                .iter()
                .any(|s| Cmd::parse(&s.command) == Some(Cmd::End))
    }

    /// Give the player what a refused info lacks (`IS_Running_NotEnoughGold` /
    /// `IS_Running_NotEnoughLP`, `refusal` in the info executor). Returns whether it did.
    fn provide_for_refusal(&mut self, id: gothic_gameplay::InfoId, why: &str) -> bool {
        let info = self.db.info(id);
        let (kind, amount) = match why {
            "IS_Running_NotEnoughGold" => {
                let need = info.gold_cost.saturating_sub(self.state.gold());
                self.state.add_item(GOLD_ITEM, need);
                ("gold", need)
            }
            "IS_Running_NotEnoughLP" => {
                let need = info
                    .teach_attrib_value
                    .saturating_sub(self.state.learning_points);
                self.state.learning_points += need;
                ("learning points", need)
            }
            _ => return false,
        };
        self.record(
            Action::Cheat(Cheat::Earn {
                what: kind.into(),
                amount,
                info: info.name.clone(),
            }),
            &ExecResult::default(),
        );
        true
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
        let mut stepped_out: BTreeSet<Option<u32>> = BTreeSet::new();
        for _ in 0..self.options.max_choices {
            if session.ended {
                break;
            }
            let menu = self.engine.menu(&self.state, &session);
            let pick = menu
                .iter()
                .filter_map(|c| c.info.map(|id| (c, id)))
                .filter(|(_, id)| !chosen.contains(&self.db.info(*id).name))
                .map(|(c, id)| (self.score(id), c, self.ends_dialog(id)))
                .filter(|(s, _, _)| *s > 0)
                // Lines that end the conversation come last: whatever else is on offer first.
                .max_by_key(|(s, _, ends)| (!*ends, *s))
                .map(|(_, c, _)| c.clone());
            let Some(choice) = pick else {
                // Nothing useful: leave (or step out of a child menu with its first entry).
                if let Some(leave) = menu.iter().find(|c| c.info.is_none()) {
                    self.engine.choose(&mut self.state, &mut session, leave);
                } else if let Some(first) = menu.first() {
                    // Stepping out of a child menu with its first entry once is enough.
                    if !stepped_out.insert(first.info.map(|i| i.0)) {
                        break;
                    }
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
            let before = fingerprint(&self.state);
            let mut res = self.engine.choose(&mut self.state, &mut session, &choice);
            // The engine refused for lack of gold / learning points: the player earns them
            // (loot, trade, level-ups) and tries again.
            if self.options.cheats
                && let Some(why) = res.refused.clone()
                && self.provide_for_refusal(id, &why)
            {
                res = self.engine.choose(&mut self.state, &mut session, &choice);
            }
            if fingerprint(&self.state) == before {
                self.futile.insert(norm(&self.db.info(id).name));
                // Its owner's cached score is stale (every NPC's for a hero info).
                if self.db.info(id).owner.eq_ignore_ascii_case(PLAYER_OWNER) {
                    self.cache.scores.clear();
                } else {
                    self.cache.scores.remove(npc);
                }
            }
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

/// Info owners that are developer cheat menus, not NPCs a player can talk to: `sh` (365
/// infos: "Gib mir ...", "Befreie Myrtana!" which succeeds quests, gold and item gifts).
const DEBUG_OWNERS: &[&str] = &["sh"];

/// Hours the clock jumps ahead when no NPC has anything useful left.
const WAIT_HOURS: u64 = 24;

/// Hash of the serialised state without the clock (waiting is not progress), the quest log
/// (permanent lines append to it every time they run) and the `QuestRunning` run times.
fn fingerprint(st: &GameState) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut s = st.clone();
    s.hours = 0;
    s.log.clear();
    s.info_last_run.clear();
    // Having talked only ever removes `Hello` lines.
    for n in s.npcs.values_mut() {
        n.talked = false;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(&s).unwrap_or_default().hash(&mut h);
    h.finish()
}

/// Run the bot with options and return its report.
pub fn play(db: &GameDatabase, options: BotOptions) -> BotReport {
    QuestBot::new(db, options).run()
}
