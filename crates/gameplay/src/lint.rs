//! Static analysis of the whole quest/dialog graph ("questlint").
//!
//! An optimistic abstract interpretation: starting with every quest `Open` and no events,
//! repeatedly mark each info *reached* if its conditions can hold in the abstract state
//! (events known so far, statuses reachable so far) and apply its effects. Anything that
//! stays unreached is **provably dead under the modelled rules**. Items and world actions
//! (kills, area entry, loot, NPC deaths) are assumed possible, `ClearGameEvent` and
//! `CondPlayerKnowsNot` are ignored (over-approximation), so *reached* does not prove
//! *playable* - [`simulate`] cross-checks with the concrete engine.
//!
//! The rules mirror `conditions.rs`, `script.rs` and `questmgr.rs` (GAME_RULES.md).
use crate::conditions::{self, Assumptions, required_statuses};
use crate::db::{GameDatabase, InfoId, QuestId};
use crate::dialog::Engine;
use crate::model::{Cmd, InfoCond, InfoType, QuestStatus, QuestType};
use crate::script::{self, Handling};
use crate::state::{GameState, norm};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const ALL: [QuestStatus; 8] = [
    QuestStatus::Open,
    QuestStatus::Running,
    QuestStatus::Success,
    QuestStatus::Failed,
    QuestStatus::Obsolete,
    QuestStatus::Cancelled,
    QuestStatus::Lost,
    QuestStatus::Won,
];

/// Does any info start this quest (Activator/Reactivator, RunQuest, SucceedQuest)?
pub fn has_start_path(db: &GameDatabase, quest: &str) -> bool {
    db.infos_of_quest(quest).iter().any(|&i| {
        matches!(
            InfoCond::from_raw(db.info(i).condition_type),
            InfoCond::Activator | InfoCond::Reactivator
        )
    }) || db.infos.iter().any(|i| {
        i.script.iter().any(|s| {
            matches!(
                Cmd::parse(&s.command),
                Some(Cmd::RunQuest | Cmd::SucceedQuest)
            ) && s.id1.eq_ignore_ascii_case(quest)
        })
    })
}

/// Is there a `Delivery` info that can hand the quest in?
pub fn has_delivery_info(db: &GameDatabase, quest: &str) -> bool {
    db.infos_of_quest(quest)
        .iter()
        .any(|&i| InfoCond::from_raw(db.info(i).condition_type) == InfoCond::Delivery)
}

fn bit(s: QuestStatus) -> u8 {
    1 << (s as u8)
}

fn mask_names(m: u8) -> Vec<&'static str> {
    ALL.iter()
        .filter(|s| m & bit(**s) != 0)
        .map(|s| s.name())
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct QuestVerdict {
    pub name: String,
    pub quest_type: String,
    /// `completable` (Success or Won reachable), `starts_but_never_finishes`, `never_starts`.
    pub verdict: &'static str,
    /// No info starts this quest; the lint assumes a script/world trigger starts it.
    pub external_start: bool,
    pub reachable_statuses: Vec<&'static str>,
    pub start_witness: Option<String>,
    pub finish_witness: Option<String>,
    /// Why not (best-effort, from the quest's own infos).
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnreachableInfo {
    pub info: String,
    pub owner: String,
    pub quest: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeadEvent {
    pub event: String,
    pub required_by: Vec<String>,
    /// `never_set` (no info sets it) or `only_set_by_unreachable_infos`.
    pub kind: &'static str,
}

/// A suspected original-data bug found by a structural check.
#[derive(Debug, Clone, Serialize)]
pub struct DataIssue {
    pub kind: &'static str,
    pub info: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Coverage {
    /// Condition type -> (name, number of infos, support level).
    pub condition_types: BTreeMap<u32, (String, usize, &'static str)>,
    /// Command (as written in data) -> (count, handling).
    pub commands: BTreeMap<String, (usize, &'static str)>,
    pub unknown_condition_infos: usize,
    pub unknown_command_uses: usize,
    pub distinct_conditions_known: usize,
    pub distinct_conditions_unknown: usize,
    pub distinct_commands_known: usize,
    pub distinct_commands_unknown: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LintReport {
    pub quests_total: usize,
    pub quests_completable: usize,
    /// Of those, how many only complete if started by a script/world trigger.
    pub quests_completable_external_start: usize,
    pub quests_start_only: usize,
    pub quests_never_start: usize,
    pub infos_total: usize,
    pub infos_reached: usize,
    pub quests: Vec<QuestVerdict>,
    pub unreachable_infos: Vec<UnreachableInfo>,
    /// Root-cause histogram for unreachable infos.
    pub unreachable_by_cause: BTreeMap<String, usize>,
    pub dead_events: Vec<DeadEvent>,
    pub data_issues: Vec<DataIssue>,
    /// Items referenced by the data but without a template (needs `known_items`).
    pub missing_items: Vec<String>,
    /// Items required by conditions/deliveries that no info ever gives (assumed world loot).
    pub items_not_given_by_any_info: Vec<String>,
    pub missing_strings: Vec<(String, String)>,
    pub undefined_quests: Vec<(String, String)>,
    pub undefined_parents: Vec<(String, String)>,
    pub coverage: Coverage,
    pub notes: Vec<String>,
}

struct Abstract<'a> {
    db: &'a GameDatabase,
    events: HashSet<String>,
    status: Vec<u8>,
    /// First cause that made `(quest, status)` reachable.
    witness: HashMap<(QuestId, QuestStatus), String>,
    reached: Vec<bool>,
    any_setter: HashSet<String>,
}

fn finished_mask() -> u8 {
    ALL.iter()
        .filter(|s| s.is_finished())
        .fold(0, |m, s| m | bit(*s))
}

impl Abstract<'_> {
    fn mask(&self, q: &str) -> Option<(QuestId, u8)> {
        self.db
            .quest_id(q)
            .map(|id| (id, self.status[id.0 as usize]))
    }

    fn add_status(&mut self, q: &str, s: QuestStatus, by: &str) -> bool {
        let Some(id) = self.db.quest_id(q) else {
            return false;
        };
        let m = &mut self.status[id.0 as usize];
        if *m & bit(s) == 0 {
            *m |= bit(s);
            self.witness
                .entry((id, s))
                .or_insert_with(|| by.to_string());
            true
        } else {
            false
        }
    }

    /// Conditions only (no tree position). `Err` carries the first blocking reason.
    fn cond_ok(&self, id: InfoId, depth: u8) -> Result<(), String> {
        let db = self.db;
        let info = db.info(id);
        let cond = InfoCond::from_raw(info.condition_type);
        if !info.quest.is_empty() && db.quest_by_name(&info.quest).is_none() {
            return Err(format!("quest {:?} undefined", info.quest));
        }
        match cond {
            InfoCond::Unknown(v) => return Err(format!("unknown condition type {v}")),
            InfoCond::Fight | InfoCond::LongTimeNoSee => {
                return Err(format!(
                    "condition {cond:?} is never fired by data (script only)"
                ));
            }
            _ => {}
        }
        if let Some(allowed) = required_statuses(cond) {
            let Some((_, m)) = self.mask(&info.quest) else {
                return Err("quest-bound condition without quest".into());
            };
            if allowed.iter().all(|s| m & bit(*s) == 0) {
                let need: Vec<_> = allowed.iter().map(|s| s.name()).collect();
                return Err(format!(
                    "quest {} never reaches {} (reachable: {})",
                    info.quest,
                    need.join("/"),
                    mask_names(m).join(",")
                ));
            }
            let q = db.quest_by_name(&info.quest);
            if matches!(
                cond,
                InfoCond::Open | InfoCond::Activator | InfoCond::Reactivator
            ) && let Some(q) = q
            {
                for pre in &q.finished_quests {
                    if let Some((_, pm)) = self.mask(pre)
                        && pm & finished_mask() == 0
                    {
                        return Err(format!("prerequisite quest {pre} never finishes"));
                    }
                }
            }
            if cond == InfoCond::Overtime
                && let Some(q) = q
                && q.running_time_years + q.running_time_days + q.running_time_hours == 0
            {
                return Err(format!(
                    "quest {} has no time limit, IsOvertime is never true",
                    q.name
                ));
            }
            if matches!(cond, InfoCond::Delivery | InfoCond::PartDelivery)
                && let Some(q) = q
            {
                let ty = QuestType::from_raw(q.quest_type);
                if ty == QuestType::HasItems
                    && !q.destination_entity.eq_ignore_ascii_case(&info.owner)
                {
                    return Err(format!(
                        "HasItems delivery needs Destination == Owner ({} != {})",
                        q.destination_entity, info.owner
                    ));
                }
                if cond == InfoCond::PartDelivery && ty != QuestType::HasItems {
                    return Err("PartDelivery only works for HasItems quests".into());
                }
            }
        }
        if InfoType::from_raw(info.info_type) == InfoType::QuestRunning
            && self
                .mask(&info.quest)
                .is_none_or(|m| m.1 & bit(QuestStatus::Running) == 0)
        {
            return Err(format!(
                "QuestRunning info needs quest {} Running",
                info.quest
            ));
        }
        for ev in &info.cond_player_knows {
            let e = norm(ev);
            let informs = db.info_id(ev).is_some_and(|i| self.reached[i.0 as usize]);
            if !self.events.contains(&e) && !informs {
                return Err(if self.any_setter.contains(&e) {
                    format!("event {ev} only set by unreachable infos")
                } else {
                    format!("event {ev} is never set by any info")
                });
            }
        }
        // A Parent-type info needs an available child [R 1.2.j].
        if InfoType::from_raw(info.info_type) == InfoType::Parent {
            let any = depth < 6
                && db
                    .children_of(&info.name)
                    .iter()
                    .filter(|&&c| {
                        matches!(
                            InfoType::from_raw(db.info(c).info_type),
                            InfoType::Info | InfoType::Parent
                        )
                    })
                    .any(|&c| self.cond_ok(c, depth + 1).is_ok());
            if !any {
                return Err("Parent info without any available child".into());
            }
        }
        Ok(())
    }

    fn avail(&self, id: InfoId) -> Result<(), String> {
        let info = self.db.info(id);
        if !info.parent.is_empty() {
            match self.db.info_id(&info.parent) {
                None => return Err(format!("parent info {} does not exist", info.parent)),
                Some(p) if !self.reached[p.0 as usize] => {
                    return Err(format!("parent {} unreachable", info.parent));
                }
                _ => {}
            }
        }
        self.cond_ok(id, 0)
    }

    fn apply(&mut self, id: InfoId) -> bool {
        let db = self.db;
        let info = db.info(id);
        let by = info.name.clone();
        let mut changed = !self.reached[id.0 as usize];
        self.reached[id.0 as usize] = true;
        for s in &info.script {
            match Cmd::parse(&s.command) {
                Some(Cmd::SetGameEvent) if !s.id1.is_empty() => {
                    changed |= self.events.insert(norm(&s.id1));
                }
                Some(Cmd::RunQuest) => {
                    if self
                        .mask(&s.id1)
                        .is_some_and(|m| m.1 & bit(QuestStatus::Open) != 0)
                    {
                        changed |= self.add_status(&s.id1, QuestStatus::Running, &by);
                    }
                }
                Some(Cmd::SucceedQuest) => {
                    changed |= self.add_status(&s.id1, QuestStatus::Success, &by);
                }
                Some(Cmd::FailQuest) => {
                    changed |= self.add_status(&s.id1, QuestStatus::Failed, &by);
                }
                Some(Cmd::CloseQuest) => {
                    if let Some((_, m)) = self.mask(&s.id1) {
                        if m & bit(QuestStatus::Open) != 0 {
                            changed |= self.add_status(&s.id1, QuestStatus::Obsolete, &by);
                        }
                        if m & bit(QuestStatus::Running) != 0 {
                            changed |= self.add_status(&s.id1, QuestStatus::Cancelled, &by);
                        }
                    }
                }
                _ => {}
            }
        }
        let cond = InfoCond::from_raw(info.condition_type);
        let Some(q) = db.quest_by_name(&info.quest) else {
            return changed;
        };
        let m = self.mask(&info.quest).map_or(0, |x| x.1);
        let ty = QuestType::from_raw(q.quest_type);
        match cond {
            InfoCond::Activator | InfoCond::Reactivator => {
                if m & (bit(QuestStatus::Open) | bit(QuestStatus::Lost)) != 0 {
                    changed |= self.add_status(&info.quest, QuestStatus::Running, &by);
                }
                if ty == QuestType::FollowNpc {
                    let others: Vec<String> = db
                        .quests
                        .iter()
                        .filter(|o| QuestType::from_raw(o.quest_type) == QuestType::FollowNpc)
                        .filter(|o| !o.name.eq_ignore_ascii_case(&q.name))
                        .filter(|o| {
                            self.mask(&o.name)
                                .is_some_and(|x| x.1 & bit(QuestStatus::Running) != 0)
                        })
                        .map(|o| o.name.clone())
                        .collect();
                    for o in others {
                        changed |= self.add_status(&o, QuestStatus::Lost, &by);
                    }
                }
            }
            InfoCond::Delivery if m & bit(QuestStatus::Running) != 0 => {
                let ok = match ty {
                    QuestType::HasItems
                    | QuestType::Defeat
                    | QuestType::BringNpc
                    | QuestType::SpellCast
                    | QuestType::FollowNpc => true,
                    QuestType::Report | QuestType::DriveAway => {
                        q.delivery_entities.is_empty()
                            || q.delivery_entities
                                .iter()
                                .any(|e| e.eq_ignore_ascii_case(&info.owner))
                    }
                    _ => false,
                };
                if ok {
                    changed |= self.add_status(&info.quest, QuestStatus::Success, &by);
                }
            }
            InfoCond::DoCancel if m & bit(QuestStatus::Running) != 0 => {
                changed |= self.add_status(&info.quest, QuestStatus::Cancelled, &by);
            }
            _ => {}
        }
        changed
    }

    /// Quest-manager completion/failure hooks fed by the world (kills, area entry, ...).
    fn world_rules(&mut self) -> bool {
        let db = self.db;
        let mut changed = false;
        for (i, q) in db.quests.iter().enumerate() {
            let m = self.status[i];
            if m & bit(QuestStatus::Running) == 0 {
                continue;
            }
            let ty = QuestType::from_raw(q.quest_type);
            let by = "<world objective>";
            match ty {
                QuestType::Kill
                | QuestType::Defeat
                | QuestType::DriveAway
                | QuestType::Arena
                | QuestType::EnterArea
                | QuestType::FreeEnclave
                | QuestType::Plunder
                | QuestType::SpellCast => {
                    changed |= self.add_status(&q.name, QuestStatus::Success, by);
                }
                QuestType::Sparring => {
                    changed |= self.add_status(&q.name, QuestStatus::Won, by);
                }
                // Without a Delivery info the items must reach the destination inventory
                // by pickup/drop in the world.
                QuestType::HasItems if !has_delivery_info(db, &q.name) => {
                    changed |= self.add_status(
                        &q.name,
                        QuestStatus::Success,
                        "<world: items reach destination>",
                    );
                }
                _ => {}
            }
            // Protected NPC dies / player loses an arena fight.
            if matches!(
                ty,
                QuestType::Report
                    | QuestType::SpellCast
                    | QuestType::Arena
                    | QuestType::Sparring
                    | QuestType::BringNpc
                    | QuestType::FollowNpc
                    | QuestType::HasItems
            ) {
                changed |= self.add_status(&q.name, QuestStatus::Failed, "<world: NPC died>");
            }
            if matches!(ty, QuestType::Arena | QuestType::Sparring) {
                changed |= self.add_status(&q.name, QuestStatus::Lost, "<world: arena lost>");
            }
        }
        changed
    }
}

pub fn lint(db: &GameDatabase) -> LintReport {
    let mut any_setter: HashSet<String> = HashSet::new();
    for i in &db.infos {
        for s in &i.script {
            if Cmd::parse(&s.command) == Some(Cmd::SetGameEvent) {
                any_setter.insert(norm(&s.id1));
            }
        }
    }
    let mut ab = Abstract {
        db,
        events: HashSet::new(),
        status: vec![bit(QuestStatus::Open); db.quests.len()],
        witness: HashMap::new(),
        reached: vec![false; db.infos.len()],
        any_setter,
    };
    let mut external = vec![false; db.quests.len()];
    for (i, q) in db.quests.iter().enumerate() {
        if !has_start_path(db, &q.name) {
            external[i] = true;
            ab.status[i] |= bit(QuestStatus::Running);
            ab.witness.insert(
                (QuestId(i as u32), QuestStatus::Running),
                "<external start: script/world trigger>".into(),
            );
        }
    }
    loop {
        let mut changed = false;
        for n in 0..db.infos.len() {
            let id = InfoId(n as u32);
            if !ab.reached[n] && ab.avail(id).is_ok() {
                changed |= ab.apply(id);
            }
        }
        changed |= ab.world_rules();
        if !changed {
            break;
        }
    }

    let mut unreachable_infos = Vec::new();
    let mut by_cause: BTreeMap<String, usize> = BTreeMap::new();
    for n in 0..db.infos.len() {
        if ab.reached[n] {
            continue;
        }
        let id = InfoId(n as u32);
        let info = db.info(id);
        let reason = ab.avail(id).err().unwrap_or_else(|| "unknown".into());
        *by_cause.entry(cause_class(&reason)).or_default() += 1;
        unreachable_infos.push(UnreachableInfo {
            info: info.name.clone(),
            owner: info.owner.clone(),
            quest: info.quest.clone(),
            reason,
        });
    }

    let mut required: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for i in &db.infos {
        for e in &i.cond_player_knows {
            required.entry(norm(e)).or_default().push(i.name.clone());
        }
    }
    let mut dead_events = Vec::new();
    for (e, by) in required {
        if ab.events.contains(&e) || db.info_id(&e).is_some_and(|i| ab.reached[i.0 as usize]) {
            continue;
        }
        dead_events.push(DeadEvent {
            kind: if ab.any_setter.contains(&e) {
                "only_set_by_unreachable_infos"
            } else {
                "never_set"
            },
            event: e,
            required_by: by,
        });
    }

    let mut quests = Vec::new();
    let (mut ok, mut start_only, mut never, mut ok_ext) = (0, 0, 0, 0);
    for (i, q) in db.quests.iter().enumerate() {
        let m = ab.status[i];
        let qid = QuestId(i as u32);
        let start_witness = ab.witness.get(&(qid, QuestStatus::Running)).cloned();
        let finish_witness = ab
            .witness
            .get(&(qid, QuestStatus::Success))
            .or_else(|| ab.witness.get(&(qid, QuestStatus::Won)))
            .cloned();
        let verdict = if m & (bit(QuestStatus::Success) | bit(QuestStatus::Won)) != 0 {
            ok += 1;
            ok_ext += usize::from(external[i]);
            "completable"
        } else if m & bit(QuestStatus::Running) != 0 {
            start_only += 1;
            "starts_but_never_finishes"
        } else {
            never += 1;
            "never_starts"
        };
        let mut blockers = Vec::new();
        if verdict != "completable" {
            let want: [InfoCond; 2] = if verdict == "never_starts" {
                [InfoCond::Activator, InfoCond::Open]
            } else {
                [InfoCond::Delivery, InfoCond::Success]
            };
            let mut seen = BTreeSet::new();
            for id in db.infos_of_quest(&q.name) {
                let cond = InfoCond::from_raw(db.info(*id).condition_type);
                if want.contains(&cond)
                    && !ab.reached[id.0 as usize]
                    && let Err(r) = ab.avail(*id)
                    && seen.insert(r.clone())
                    && blockers.len() < 4
                {
                    blockers.push(format!("{}: {r}", db.info(*id).name));
                }
            }
            if blockers.is_empty() {
                blockers.push(match verdict {
                    "never_starts" => {
                        "no Activator/Open info or RunQuest targets this quest".into()
                    }
                    _ => {
                        "no Delivery/Success info, SucceedQuest or world hook completes this quest"
                            .into()
                    }
                });
            }
        }
        quests.push(QuestVerdict {
            name: q.name.clone(),
            quest_type: format!("{:?}", QuestType::from_raw(q.quest_type)),
            verdict,
            external_start: external[i],
            reachable_statuses: mask_names(m),
            start_witness,
            finish_witness,
            blockers,
        });
    }

    let mut missing_strings = Vec::new();
    let mut undefined_quests = Vec::new();
    let mut undefined_parents = Vec::new();
    let mut data_issues = Vec::new();
    let mut given: HashSet<String> = HashSet::new();
    let mut needed: BTreeSet<String> = BTreeSet::new();
    for i in &db.infos {
        if !i.quest.is_empty() && db.quest_by_name(&i.quest).is_none() {
            undefined_quests.push((i.name.clone(), i.quest.clone()));
        }
        if !i.parent.is_empty() && db.info_id(&i.parent).is_none() {
            undefined_parents.push((i.name.clone(), i.parent.clone()));
        }
        let cond = InfoCond::from_raw(i.condition_type);
        if conditions::needs_quest(cond) && i.quest.is_empty() {
            data_issues.push(DataIssue {
                kind: "quest_condition_without_quest",
                info: i.name.clone(),
                detail: format!("{cond:?} info has no Quest; the game rejects it"),
            });
        }
        for e in &i.cond_player_knows_not {
            if !ab.any_setter.contains(&norm(e)) {
                data_issues.push(DataIssue {
                    kind: "knows_not_event_never_set",
                    info: i.name.clone(),
                    detail: e.clone(),
                });
            }
        }
        needed.extend(i.cond_items.iter().cloned());
        for s in &i.script {
            let cmd = Cmd::parse(&s.command);
            if cmd.is_none() {
                data_issues.push(DataIssue {
                    kind: "unknown_command",
                    info: i.name.clone(),
                    detail: s.command.clone(),
                });
            }
            let text_is_key = !matches!(cmd, Some(Cmd::ModReputEnc | Cmd::ModReputPol) | None);
            if text_is_key && !s.text.is_empty() && db.text(&s.text).is_none() {
                missing_strings.push((i.name.clone(), s.text.clone()));
            }
            if matches!(cmd, Some(Cmd::Give | Cmd::Use | Cmd::Wear)) && !s.id1.is_empty() {
                needed.insert(s.id1.clone());
                if cmd == Some(Cmd::Give) {
                    given.insert(norm(&s.id1));
                }
            }
            if matches!(
                cmd,
                Some(Cmd::RunQuest | Cmd::CloseQuest | Cmd::SucceedQuest | Cmd::FailQuest)
            ) && db.quest_by_name(&s.id1).is_none()
            {
                data_issues.push(DataIssue {
                    kind: "script_targets_undefined_quest",
                    info: i.name.clone(),
                    detail: format!("{} {}", s.command, s.id1),
                });
            }
        }
    }
    for q in &db.quests {
        if db.text(&q.log_topic).is_none() {
            missing_strings.push((format!("quest:{}", q.name), q.log_topic.clone()));
        }
        if QuestType::from_raw(q.quest_type) == QuestType::HasItems {
            needed.extend(q.delivery_entities.iter().cloned());
        }
        if q.delivery_entities.len() != q.delivery_amounts.len() {
            data_issues.push(DataIssue {
                kind: "delivery_lists_differ_in_length",
                info: format!("quest:{}", q.name),
                detail: format!(
                    "{} entities vs {} amounts",
                    q.delivery_entities.len(),
                    q.delivery_amounts.len()
                ),
            });
        }
    }
    let plausible = |n: &str| n.len() > 3 && n[..3].eq_ignore_ascii_case("it_");
    let missing_items = match &db.known_items {
        Some(k) => needed
            .iter()
            .filter(|n| !k.contains(&norm(n)) && plausible(n))
            .cloned()
            .collect(),
        None => Vec::new(),
    };
    let items_not_given_by_any_info = needed
        .iter()
        .filter(|n| plausible(n) && !given.contains(&norm(n)))
        .cloned()
        .collect();

    let mut notes = vec![
        "optimistic: items, kills, area entry and NPC deaths are assumed possible; ClearGameEvent \
         and CondPlayerKnowsNot are ignored; NPC placement/routines are not modelled"
            .to_string(),
    ];
    if db.known_items.is_none() {
        notes.push("item templates not loaded: missing_items is empty".into());
    }
    LintReport {
        quests_total: db.quests.len(),
        quests_completable: ok,
        quests_completable_external_start: ok_ext,
        quests_start_only: start_only,
        quests_never_start: never,
        infos_total: db.infos.len(),
        infos_reached: ab.reached.iter().filter(|r| **r).count(),
        quests,
        unreachable_infos,
        unreachable_by_cause: by_cause,
        dead_events,
        data_issues,
        missing_items,
        items_not_given_by_any_info,
        missing_strings,
        undefined_quests,
        undefined_parents,
        coverage: coverage(db),
        notes,
    }
}

fn cause_class(reason: &str) -> String {
    for (needle, class) in [
        ("unknown condition", "unknown condition type"),
        ("never fired", "condition never fired by data"),
        ("parent info", "parent info missing"),
        ("parent ", "parent unreachable"),
        ("undefined", "quest undefined"),
        ("never reaches", "quest status unreachable"),
        ("prerequisite", "prerequisite quest never finishes"),
        ("never set", "event never set by any info"),
        (
            "only set by unreachable",
            "event only set by unreachable infos",
        ),
        (
            "Destination == Owner",
            "HasItems delivery owner != destination",
        ),
        ("no time limit", "Overtime info on quest without time limit"),
        ("Parent info without", "Parent info without available child"),
    ] {
        if reason.contains(needle) {
            return class.into();
        }
    }
    "other".into()
}

fn coverage(db: &GameDatabase) -> Coverage {
    let mut c = Coverage::default();
    for i in &db.infos {
        let cond = InfoCond::from_raw(i.condition_type);
        let support = match cond {
            InfoCond::Unknown(_) => {
                c.unknown_condition_infos += 1;
                "UNKNOWN"
            }
            InfoCond::Fight | InfoCond::LongTimeNoSee => {
                "evaluated (always false, script-fired only)"
            }
            x if required_statuses(x).is_some() => "evaluated (quest status)",
            InfoCond::Crime
            | InfoCond::PickPocket
            | InfoCond::MasterThief
            | InfoCond::Heal
            | InfoCond::EnclaveCrime
            | InfoCond::PoliticalCrime
            | InfoCond::FirstWarn
            | InfoCond::SecondWarn
            | InfoCond::Animal
            | InfoCond::NotFriend
            | InfoCond::NotFriendOrThief => "evaluated; trigger needs world simulation",
            _ => "evaluated",
        };
        let e = c
            .condition_types
            .entry(i.condition_type)
            .or_insert_with(|| (format!("{cond:?}"), 0, support));
        e.1 += 1;
        for s in &i.script {
            let h = match Cmd::parse(&s.command) {
                Some(cmd) => match script::handling(cmd) {
                    Handling::Applied => "applied to state",
                    Handling::Presented => "presented",
                    Handling::Forwarded => "forwarded to host as effect",
                    Handling::Flow => "dialog flow",
                    Handling::IgnoredByOriginal => "no-op in original executor",
                },
                None => {
                    c.unknown_command_uses += 1;
                    "UNKNOWN"
                }
            };
            c.commands.entry(s.command.clone()).or_insert((0, h)).0 += 1;
        }
    }
    c.distinct_conditions_known = c
        .condition_types
        .values()
        .filter(|v| v.2 != "UNKNOWN")
        .count();
    c.distinct_conditions_unknown = c.condition_types.len() - c.distinct_conditions_known;
    c.distinct_commands_known = c.commands.values().filter(|v| v.1 != "UNKNOWN").count();
    c.distinct_commands_unknown = c.commands.len() - c.distinct_commands_known;
    c
}

/// Result of a concrete greedy playthrough with the real engine.
#[derive(Debug, Clone, Serialize)]
pub struct SimReport {
    pub rounds: usize,
    pub infos_executed: usize,
    pub quests_success: Vec<String>,
    pub quests_running_only: Vec<String>,
    pub quests_untouched: usize,
    pub diagnostics: BTreeMap<String, usize>,
}

fn world_objectives(db: &GameDatabase, st: &mut GameState, q: &gothic_formats::quest::Quest) {
    use crate::questmgr as m;
    let mut sink = script::ExecResult::default();
    let ty = QuestType::from_raw(q.quest_type);
    for (k, ent) in q.delivery_entities.iter().enumerate() {
        let n = q.delivery_amounts.get(k).copied().unwrap_or(1).max(1);
        for _ in 0..n {
            match ty {
                QuestType::Kill | QuestType::DriveAway => {
                    m::count_npc_killed(db, st, ent, &mut sink)
                }
                QuestType::Defeat | QuestType::Arena | QuestType::Sparring => {
                    m::on_npc_defeated(db, st, ent, &mut sink);
                }
                QuestType::EnterArea => m::on_enter(db, st, ent, &q.destination_entity, &mut sink),
                QuestType::FreeEnclave => m::on_enclave_state_changed(db, st, ent, &mut sink),
                QuestType::SpellCast => m::on_spell_cast(db, st, ent, &q.spell_name, &mut sink),
                QuestType::Plunder => {
                    m::on_receive_item(db, st, &q.destination_entity, "PC_Hero", ent, 1, &mut sink);
                }
                QuestType::HasItems => {
                    // Loot: the player finds the items; a Delivery info hands them in,
                    // otherwise they reach the destination inventory directly.
                    if has_delivery_info(db, &q.name) {
                        if st.item_count(ent) < n {
                            st.add_item(ent, n - st.item_count(ent));
                        }
                    } else {
                        m::on_receive_item(db, st, "", &q.destination_entity, ent, 1, &mut sink);
                    }
                }
                _ => {}
            }
        }
    }
}

#[derive(Clone)]
struct Sim {
    st: GameState,
    executed: HashSet<InfoId>,
    fed: HashSet<String>,
    rounds: usize,
    reruns: HashMap<InfoId, u8>,
}

fn is_spoiler(info: &gothic_formats::info::Info) -> bool {
    info.script.iter().any(|s| {
        matches!(
            Cmd::parse(&s.command),
            Some(Cmd::CloseQuest | Cmd::FailQuest)
        )
    }) || InfoCond::from_raw(info.condition_type) == InfoCond::DoCancel
}

fn sim_owner(info: &gothic_formats::info::Info) -> String {
    if info.owner.eq_ignore_ascii_case("pc_hero") {
        "Kerth".to_string()
    } else {
        info.owner.clone()
    }
}

impl Sim {
    fn successes(&self, db: &GameDatabase) -> usize {
        db.quests
            .iter()
            .filter(|q| {
                matches!(
                    self.st.quest_status(&q.name),
                    QuestStatus::Success | QuestStatus::Won
                )
            })
            .count()
    }

    /// Run all non-spoiler infos and world hooks until nothing changes.
    fn quiesce(
        &mut self,
        db: &GameDatabase,
        eng: &Engine<'_>,
        order: &[InfoId],
        diagnostics: &mut BTreeMap<String, usize>,
    ) {
        loop {
            self.rounds += 1;
            let mut progress = false;
            self.st.hours += 3;
            for q in &db.quests {
                if self.st.quest_status(&q.name) == QuestStatus::Running
                    && self.fed.insert(norm(&q.name))
                {
                    world_objectives(db, &mut self.st, q);
                    progress = true;
                }
            }
            for &id in order {
                let info = db.info(id);
                let reactivator = InfoCond::from_raw(info.condition_type) == InfoCond::Reactivator;
                if is_spoiler(info) {
                    continue;
                }
                if self.executed.contains(&id) {
                    // Reactivators legitimately run again after the quest was lost (bounded).
                    let n = self.reruns.entry(id).or_insert(0);
                    if !reactivator || *n >= 3 {
                        continue;
                    }
                }
                if !info.parent.is_empty() {
                    match db.info_id(&info.parent) {
                        Some(p) if self.executed.contains(&p) => {}
                        _ => continue,
                    }
                }
                let owner = sim_owner(info);
                if !eng.is_available(&self.st, id, &owner) {
                    continue;
                }
                if !self.executed.insert(id) {
                    *self.reruns.entry(id).or_insert(0) += 1;
                }
                let r = eng.execute(&mut self.st, id, &owner);
                for d in r.diagnostics {
                    *diagnostics
                        .entry(format!("{:?}:{}", d.kind, d.command))
                        .or_default() += 1;
                }
                progress = true;
            }
            if !progress || self.rounds > 2000 {
                break;
            }
        }
    }
}

/// Greedy concrete playthrough: repeatedly execute every available non-spoiler info in
/// `SortID` order (parents before children) and feed the world hooks for running quests
/// until nothing changes; then try each info that closes/fails/cancels quests on a copy and
/// keep it only if the number of succeeded quests does not drop. Order dependent
/// (`ClearGameEvent`), so it under-approximates what a player can do - complementary to
/// [`lint`].
pub fn simulate(db: &GameDatabase) -> (SimReport, GameState) {
    let eng = Engine::with(db, Assumptions::OPTIMISTIC);
    let mut sim = Sim {
        st: GameState::new(),
        executed: HashSet::new(),
        fed: HashSet::new(),
        rounds: 0,
        reruns: HashMap::new(),
    };
    sim.st.add_item("it_gold", 1_000_000);
    let mut order: Vec<InfoId> = (0..db.infos.len() as u32).map(InfoId).collect();
    order.sort_by_key(|&i| {
        let x = db.info(i);
        (x.sort_id, x.name.to_ascii_lowercase())
    });
    let mut diagnostics: BTreeMap<String, usize> = BTreeMap::new();
    let mut sink = script::ExecResult::default();
    for q in &db.quests {
        if !has_start_path(db, &q.name) {
            crate::questmgr::run_quest(db, &mut sim.st, &q.name, &mut sink);
        }
    }
    sim.quiesce(db, &eng, &order, &mut diagnostics);
    // Phase 2: spoilers, each tried on a copy.
    let mut tried: HashSet<InfoId> = HashSet::new();
    loop {
        let mut adopted = false;
        for &id in &order {
            let info = db.info(id);
            if !is_spoiler(info) || sim.executed.contains(&id) || tried.contains(&id) {
                continue;
            }
            if !info.parent.is_empty() {
                match db.info_id(&info.parent) {
                    Some(p) if sim.executed.contains(&p) => {}
                    _ => continue,
                }
            }
            let owner = sim_owner(info);
            if !eng.is_available(&sim.st, id, &owner) {
                continue;
            }
            tried.insert(id);
            let before = sim.successes(db);
            let mut trial = sim.clone();
            trial.executed.insert(id);
            let r = eng.execute(&mut trial.st, id, &owner);
            let mut d2 = BTreeMap::new();
            trial.quiesce(db, &eng, &order, &mut d2);
            let spoils = db.quests.iter().any(|q| {
                matches!(
                    sim.st.quest_status(&q.name),
                    QuestStatus::Open | QuestStatus::Running
                ) && matches!(
                    trial.st.quest_status(&q.name),
                    QuestStatus::Obsolete
                        | QuestStatus::Cancelled
                        | QuestStatus::Failed
                        | QuestStatus::Lost
                )
            });
            if !spoils && trial.successes(db) >= before {
                for d in r.diagnostics {
                    *diagnostics
                        .entry(format!("{:?}:{}", d.kind, d.command))
                        .or_default() += 1;
                }
                for (k, v) in d2 {
                    *diagnostics.entry(k).or_default() += v;
                }
                sim = trial;
                adopted = true;
            }
        }
        if !adopted {
            break;
        }
    }
    let mut success = Vec::new();
    let mut running = Vec::new();
    let mut untouched = 0;
    for q in &db.quests {
        match sim.st.quest_status(&q.name) {
            QuestStatus::Success | QuestStatus::Won => success.push(q.name.clone()),
            QuestStatus::Running => running.push(q.name.clone()),
            QuestStatus::Open => untouched += 1,
            _ => {}
        }
    }
    (
        SimReport {
            rounds: sim.rounds,
            infos_executed: sim.executed.len(),
            quests_success: success,
            quests_running_only: running,
            quests_untouched: untouched,
            diagnostics,
        },
        sim.st,
    )
}
