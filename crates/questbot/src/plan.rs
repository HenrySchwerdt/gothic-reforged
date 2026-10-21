//! Route planning through the info/quest graph.
//!
//! From the target quests the planner derives what matters:
//! * **relevant quests**: the targets plus, transitively, their `FinishedQuests`
//!   prerequisites;
//! * **useful events**: game events gating infos of relevant quests (`CondPlayerKnows`),
//!   closed over the infos that set them (an info that sets a useful event may itself need
//!   another event, which is then useful too).
//!
//! [`Planner::score`] ranks an available info by how directly it advances that route; the bot
//! always takes the highest-scoring option.
use gothic_gameplay::conditions::is_permanent;
use gothic_gameplay::model::{Cmd, InfoCond};
use gothic_gameplay::state::norm;
use gothic_gameplay::{GameDatabase, GameState, InfoId};
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct Planner {
    targets: Vec<String>,
    relevant: BTreeSet<String>,
    useful_events: BTreeSet<String>,
}

/// Score of an info that belongs to an unfinished relevant quest.
pub const QUEST_STEP: i32 = 100;
/// Score of an info that sets a useful event not known yet.
pub const UNLOCKS: i32 = 80;
/// Score of an info that starts or finishes a relevant quest by script command.
pub const SCRIPTED: i32 = 70;
/// Score of any other unexplored, non-permanent info.
pub const EXPLORE: i32 = 1;

impl Planner {
    pub fn new(db: &GameDatabase, targets: &[String]) -> Self {
        let mut relevant: BTreeSet<String> = BTreeSet::new();
        let mut todo: Vec<String> = targets.to_vec();
        while let Some(q) = todo.pop() {
            if !relevant.insert(norm(&q)) {
                continue;
            }
            if let Some(quest) = db.quest_by_name(&q) {
                todo.extend(quest.finished_quests.iter().cloned());
            }
        }
        // Events needed by infos of relevant quests, closed over their setters' needs.
        let mut useful: BTreeSet<String> = BTreeSet::new();
        let mut frontier: Vec<String> = db
            .infos
            .iter()
            .filter(|i| relevant.contains(&norm(&i.quest)))
            .flat_map(|i| i.cond_player_knows.iter().map(|e| norm(e)))
            .collect();
        while let Some(e) = frontier.pop() {
            if !useful.insert(e.clone()) {
                continue;
            }
            for info in &db.infos {
                let sets = info.script.iter().any(|s| {
                    Cmd::parse(&s.command) == Some(Cmd::SetGameEvent) && norm(&s.id1) == e
                });
                if sets {
                    frontier.extend(info.cond_player_knows.iter().map(|x| norm(x)));
                    // A child info also needs its parent chain to be reachable.
                    let mut parent = info.parent.clone();
                    while let Some(p) = db.info_id(&parent).map(|id| db.info(id)) {
                        frontier.extend(p.cond_player_knows.iter().map(|x| norm(x)));
                        parent = p.parent.clone();
                    }
                }
            }
        }
        Self {
            targets: targets.to_vec(),
            relevant,
            useful_events: useful,
        }
    }

    pub fn targets(&self) -> &[String] {
        &self.targets
    }

    pub fn is_relevant(&self, quest: &str) -> bool {
        self.relevant.contains(&norm(quest))
    }

    pub fn is_useful_event(&self, event: &str) -> bool {
        self.useful_events.contains(&norm(event))
    }

    /// How much choosing `id` now advances the route (0 = not worth it).
    pub fn score(&self, db: &GameDatabase, st: &GameState, id: InfoId) -> i32 {
        self.score_depth(db, st, id, 0)
    }

    fn score_depth(&self, db: &GameDatabase, st: &GameState, id: InfoId, depth: u8) -> i32 {
        let info = db.info(id);
        let cond = InfoCond::from_raw(info.condition_type);
        if matches!(
            cond,
            InfoCond::End | InfoCond::Back | InfoCond::StepBack | InfoCond::NothingToSay
        ) {
            return 0;
        }
        let mut best = 0;
        if !info.quest.is_empty()
            && self.is_relevant(&info.quest)
            && !st.quest_status(&info.quest).is_finished()
        {
            best = QUEST_STEP
                + match cond {
                    InfoCond::Delivery | InfoCond::PartDelivery => 40,
                    InfoCond::Success => 30,
                    InfoCond::Activator | InfoCond::Reactivator => 20,
                    _ => 10,
                };
        } else if !info.quest.is_empty()
            && self.is_relevant(&info.quest)
            && matches!(cond, InfoCond::Success | InfoCond::Won)
        {
            // Success infos run after the quest finished; they often set the next gate.
            best = SCRIPTED;
        }
        for s in &info.script {
            match Cmd::parse(&s.command) {
                Some(Cmd::SetGameEvent) if self.is_useful_event(&s.id1) && !st.knows(&s.id1) => {
                    best = best.max(UNLOCKS);
                }
                Some(Cmd::RunQuest | Cmd::SucceedQuest)
                    if self.is_relevant(&s.id1) && !st.quest_status(&s.id1).is_finished() =>
                {
                    best = best.max(SCRIPTED);
                }
                _ => {}
            }
        }
        // Any info that has children opens them after it ran (`OnInfoScriptFinished`), not
        // only `Parent` infos: `FILLER140` is a plain `Info` whose children start Braga_Tribut.
        if depth < 6 {
            for &c in db.children_of(&info.name) {
                if !st.told.contains(&norm(&db.info(c).name)) {
                    best = best.max(self.score_depth(db, st, c, depth + 1));
                }
            }
        }
        if best == 0 && !is_permanent(db, id) && !st.told.contains(&norm(&info.name)) {
            best = EXPLORE;
        }
        best
    }
}
