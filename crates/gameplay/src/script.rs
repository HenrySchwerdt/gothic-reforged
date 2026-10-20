//! Info-script execution: applies an info's commands and the side effects the engine
//! attaches to its condition type to a [`GameState`], reporting everything as
//! [`Output`]s plus [`Diagnostic`]s. Nothing is ignored silently:
//!
//! * every command of the game's table is *applied* to the state, *presented* (speech,
//!   subtitles), *forwarded* to the host as [`Output::Effect`] (world/AI actions), is
//!   dialog flow, or is documented as a no-op in the original executor
//!   ([`Handling::IgnoredByOriginal`]);
//! * a command name outside the table yields [`DiagKind::UnknownCommand`];
//! * argument problems yield diagnostics.
//!
//! Rule references `[R x.y]` point to `docs/research/GAME_RULES.md`; `[I]` marks guesses.
use crate::conditions::is_permanent;
use crate::db::{GameDatabase, InfoId};
use crate::model::{Cmd, InfoCond, InfoType, QuestStatus, QuestType};
use crate::questmgr;
use crate::state::{GOLD_ITEM, GameState, LogEntry, LogKind, norm};
use gothic_formats::info::{Info, ScriptStep};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Output {
    Say {
        speaker: String,
        key: String,
        text: Option<String>,
        direction: String,
    },
    Svm {
        speaker: String,
        id: String,
    },
    Presentation {
        kind: String,
        arg: String,
        text: Option<String>,
    },
    QuestChanged {
        quest: String,
        from: QuestStatus,
        to: QuestStatus,
    },
    QuestLog {
        quest: String,
        text: Option<String>,
    },
    Xp {
        amount: u32,
        level_before: u32,
        level_after: u32,
    },
    Item {
        from: String,
        to: String,
        item: String,
        amount: u32,
    },
    Message {
        text: String,
    },
    /// World/AI action the host has to carry out (the state may also record it).
    Effect {
        command: String,
        entity1: String,
        entity2: String,
        arg1: String,
        arg2: String,
    },
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum DiagKind {
    /// Command name is not in the game's command table.
    UnknownCommand,
    BadArgument,
    MissingItems,
    UnknownQuest,
    UnknownText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub info: String,
    pub command: String,
    pub kind: DiagKind,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ExecResult {
    pub outputs: Vec<Output>,
    pub diagnostics: Vec<Diagnostic>,
    pub ended: bool,
    /// The engine refused to run the info (`IS_Running_NotEnoughGold`, `NotEnoughLP`, ...)
    /// [R 1.6]; the info stays available.
    pub refused: Option<String>,
}

/// How a command is handled; used for the coverage report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handling {
    /// Changes the [`GameState`].
    Applied,
    /// Dialog line or presentation output.
    Presented,
    /// Forwarded to the host as an [`Output::Effect`] (no gameplay state of its own yet).
    Forwarded,
    /// Dialog-flow marker.
    Flow,
    /// The original executor has no implementation either [R 3].
    IgnoredByOriginal,
}

pub fn handling(cmd: Cmd) -> Handling {
    use Cmd::*;
    match cmd {
        Say | SaySvm | SaySound | SayKeepItUp | SayBossNews | SayGuardNews | SayConfirm
        | SayHoldingPosition | ShowSubtitle | ShowPicture | InfoWait | StartEffect
        | ShowGameMessage | Description => Handling::Presented,
        Give | GiveAll | GiveXp | Use | Erase | Grant | BoostAttribs | SetGameEvent
        | ClearGameEvent | RunQuest | CloseQuest | SucceedQuest | FailQuest | AddQuestLog
        | ModReputPol | ModReputEnc | SetTeachEnabled | SetTradeEnabled | SetPartyEnabled
        | SetMobEnabled | SetSlaveryEnabled | SetPal | SetRoutine | SetSectorStatus
        | SetChapter | EndGame | JoinPlayer | JoinNpc | SetGuardStatus | SetClass
        | SetRevolutionEnabled | SetRaidEnabled | SetSleepingPoint | SetWorkingPoint
        | SetRelaxingPoint | Sleep | Humanize => Handling::Applied,
        End => Handling::Flow,
        SetEnclave | PlayMusic | Back => Handling::IgnoredByOriginal,
        Excuse | Bribe | PickPocket | Attack | Wear | Heal | OpenDoor | FullStop | MoveTo
        | DoTrade | DoLoot | StartRevolution | Spawn | ResetCrime | DecreaseRevolCount => {
            Handling::Forwarded
        }
    }
}

struct Ctx<'a> {
    db: &'a GameDatabase,
    st: &'a mut GameState,
    info: &'a str,
    partner: &'a str,
    res: ExecResult,
}

impl Ctx<'_> {
    fn diag(&mut self, cmd: &str, kind: DiagKind, detail: impl Into<String>) {
        self.res.diagnostics.push(Diagnostic {
            info: self.info.to_string(),
            command: cmd.to_string(),
            kind,
            detail: detail.into(),
        });
    }

    /// Entity alias resolution [R 3]: `player`/`PC_Hero` -> "player"; `npc`/`self`/`owner`
    /// and the empty string -> the dialog partner; `other` -> the player [I]; else the name.
    fn who(&self, e: &str) -> String {
        match norm(e).as_str() {
            "player" | "pc_hero" | "hero" | "other" => "player".into(),
            "" | "npc" | "owner" | "self" => norm(self.partner),
            other => other.into(),
        }
    }

    /// Entity name as the quest hooks see it (`PC_Hero` for the player).
    fn hook_name(who: &str) -> String {
        if who == "player" {
            "PC_Hero".into()
        } else {
            who.to_string()
        }
    }

    fn amount(&mut self, cmd: &str, s: &str, default: u32) -> u32 {
        if s.trim().is_empty() {
            return default;
        }
        match s.trim().parse::<u32>() {
            Ok(v) => v,
            Err(_) => {
                self.diag(cmd, DiagKind::BadArgument, format!("amount {s:?}"));
                default
            }
        }
    }

    /// Booleans parse as integer or contain "true" [R 3].
    fn boolean(&self, s: &str) -> bool {
        let t = norm(s);
        t.contains("true") || t.parse::<i64>().is_ok_and(|v| v != 0)
    }

    fn quest_exists(&mut self, cmd: &str, q: &str) -> bool {
        let ok = self.db.quest_id(q).is_some();
        if !ok {
            self.diag(cmd, DiagKind::UnknownQuest, q);
        }
        ok
    }

    /// `Give` [R 3 id 13]: a missing stack is created in the giver's inventory first.
    fn transfer(&mut self, from: &str, to: &str, item: &str, n: u32) {
        if from == to || n == 0 {
            return;
        }
        if from == "player" {
            let have = self.st.item_count(item);
            if have < n {
                self.st.add_item(item, n - have);
            }
            self.st.take_item(item, n);
        } else {
            let inv = &mut self.st.npc_mut(from).inventory;
            let n = n.min(inv.count(item));
            inv.take(item, n);
        }
        if to == "player" {
            self.st.add_item(item, n);
        } else {
            self.st.npc_mut(to).inventory.add(item, n);
        }
        self.res.outputs.push(Output::Item {
            from: from.into(),
            to: to.into(),
            item: item.into(),
            amount: n,
        });
        let (f, t) = (Self::hook_name(from), Self::hook_name(to));
        questmgr::on_receive_item(self.db, self.st, &f, &t, item, n, &mut self.res);
    }

    fn effect(&mut self, s: &ScriptStep) {
        self.res.outputs.push(Output::Effect {
            command: s.command.clone(),
            entity1: s.entity1.clone(),
            entity2: s.entity2.clone(),
            arg1: s.id1.clone(),
            arg2: s.id2.clone(),
        });
    }

    fn say_text(&mut self, cmd: &str, key: &str) -> Option<String> {
        if key.is_empty() {
            return None;
        }
        let t = self.db.text(key);
        if t.is_none() {
            self.diag(cmd, DiagKind::UnknownText, key);
        }
        t
    }

    fn step(&mut self, s: &ScriptStep) {
        let Some(cmd) = Cmd::parse(&s.command) else {
            self.diag(&s.command, DiagKind::UnknownCommand, "not in command table");
            return;
        };
        let name = s.command.as_str();
        match cmd {
            Cmd::Say
            | Cmd::SayKeepItUp
            | Cmd::SayConfirm
            | Cmd::SayHoldingPosition
            | Cmd::SayBossNews
            | Cmd::SayGuardNews => {
                let text = self.say_text(name, &s.text);
                let speaker = self.who(if s.entity1.is_empty() {
                    "npc"
                } else {
                    &s.entity1
                });
                self.res.outputs.push(Output::Say {
                    speaker,
                    key: s.text.clone(),
                    text,
                    direction: s.id2.clone(),
                });
            }
            Cmd::SaySvm | Cmd::SaySound => {
                let speaker = self.who(&s.entity1);
                self.res.outputs.push(Output::Svm {
                    speaker,
                    id: s.id1.clone(),
                });
            }
            Cmd::Description | Cmd::AddQuestLog => {} // label / applied in the log pass
            Cmd::ShowSubtitle
            | Cmd::ShowGameMessage
            | Cmd::ShowPicture
            | Cmd::InfoWait
            | Cmd::StartEffect => {
                let text = self.say_text(name, &s.text);
                self.res.outputs.push(Output::Presentation {
                    kind: s.command.clone(),
                    arg: s.id1.clone(),
                    text,
                });
            }
            Cmd::Give => {
                let (from, to) = (self.who(&s.entity1), self.who(&s.entity2));
                let n = self.amount(name, &s.id2, 1);
                if s.id1.is_empty() || n == 0 {
                    self.diag(name, DiagKind::BadArgument, "item/amount missing");
                } else {
                    self.transfer(&from, &to, &s.id1, n);
                }
            }
            Cmd::GiveAll => {
                // [R 3 id 64] transfers E1's whole inventory to the other party; the
                // receiver is E2, or the entity named by ID1 in the shipped data [I].
                let from = self.who(&s.entity1);
                let to = if s.entity2.is_empty() {
                    self.who(&s.id1)
                } else {
                    self.who(&s.entity2)
                };
                let all: Vec<(String, u32)> = if from == "player" {
                    self.st
                        .inventory
                        .iter()
                        .map(|(k, v)| (k.clone(), *v))
                        .collect()
                } else {
                    self.st
                        .npc(&from)
                        .map(|n| n.inventory.iter().map(|(k, v)| (k.clone(), *v)).collect())
                        .unwrap_or_default()
                };
                for (item, n) in all {
                    self.transfer(&from, &to, &item, n);
                }
            }
            Cmd::Grant => {
                // [R 3 id 61] create items in the entity's inventory.
                let who = self.who(&s.entity1);
                let n = self.amount(name, &s.id2, 1);
                if who == "player" {
                    self.st.add_item(&s.id1, n);
                } else {
                    self.st.npc_mut(&who).inventory.add(&s.id1, n);
                }
                self.effect(s);
            }
            Cmd::GiveXp => {
                // [R 3 id 12] GiveXP(world, player, n) pays 5 x n.
                let n = self.amount(name, &s.id1, 0);
                if n == 0 {
                    self.diag(name, DiagKind::BadArgument, "xp amount must be > 0");
                } else {
                    questmgr::give_world_xp(self.st, n, &mut self.res);
                }
            }
            Cmd::Use | Cmd::Erase => {
                // [R 3 ids 15, 54] the item is ensured/created first, then used/purged.
                let who = self.who(&s.entity1);
                let n = self.amount(name, &s.id2, 1);
                if who == "player" {
                    self.st.take_item(&s.id1, n);
                } else {
                    let inv = &mut self.st.npc_mut(&who).inventory;
                    let n = n.min(inv.count(&s.id1));
                    inv.take(&s.id1, n);
                }
                self.effect(s);
            }
            Cmd::BoostAttribs => {
                // [R 3 id 52] ID1 in {STR, DEX, INT, XAR (= all three)}; the boost size is
                // script-defined (not recovered): +1 [I].
                let tags: Vec<String> = match norm(&s.id1).as_str() {
                    "xar" => vec!["str".into(), "dex".into(), "int".into()],
                    t => vec![t.into()],
                };
                for t in tags {
                    *self.st.attributes.entry(t).or_insert(0) += 1;
                }
            }
            Cmd::SetGameEvent => {
                if s.id1.is_empty() {
                    self.diag(name, DiagKind::BadArgument, "event name missing");
                } else {
                    self.st.set_event(&s.id1);
                }
            }
            Cmd::ClearGameEvent => {
                if s.id1.is_empty() {
                    self.diag(name, DiagKind::BadArgument, "event name missing");
                } else {
                    self.st.clear_event(&s.id1);
                }
            }
            Cmd::RunQuest => {
                if self.quest_exists(name, &s.id1) {
                    questmgr::run_quest(self.db, self.st, &s.id1, &mut self.res);
                }
            }
            Cmd::SucceedQuest => {
                if self.quest_exists(name, &s.id1) {
                    questmgr::succeed_quest(self.db, self.st, &s.id1, &mut self.res);
                }
            }
            Cmd::FailQuest => {
                if self.quest_exists(name, &s.id1) {
                    questmgr::fail_quest(self.db, self.st, &s.id1, &mut self.res);
                }
            }
            Cmd::CloseQuest => {
                if self.quest_exists(name, &s.id1) {
                    questmgr::close_quest(self.db, self.st, &s.id1, &mut self.res);
                }
            }
            Cmd::ModReputPol | Cmd::ModReputEnc => {
                // [R 3 ids 59, 60] amount > 0, Text "DEC" decreases; fame clamped 0..=100.
                let (prefix, key) = if cmd == Cmd::ModReputPol {
                    ("pol", norm(&s.id1))
                } else {
                    (
                        "enc",
                        norm(if s.entity2.is_empty() {
                            &s.id1
                        } else {
                            &s.entity2
                        }),
                    )
                };
                let n = self.amount(name, &s.id2, 0) as i32;
                if n <= 0 {
                    self.diag(name, DiagKind::BadArgument, "amount must be > 0");
                } else {
                    let d = if s.text.eq_ignore_ascii_case("DEC") {
                        -n
                    } else {
                        n
                    };
                    self.st.add_fame(&format!("{prefix}:{key}"), d);
                }
                self.effect(s);
            }
            Cmd::SetTeachEnabled
            | Cmd::SetTradeEnabled
            | Cmd::SetPartyEnabled
            | Cmd::SetMobEnabled
            | Cmd::SetSlaveryEnabled => {
                let who = self.who(&s.entity1);
                let v = self.boolean(&s.id1);
                let npc = self.st.npc_mut(&who);
                match cmd {
                    Cmd::SetTeachEnabled => npc.teach_enabled = Some(v),
                    Cmd::SetTradeEnabled => npc.trade_enabled = Some(v),
                    Cmd::SetPartyEnabled => npc.party_enabled = Some(v),
                    Cmd::SetMobEnabled => npc.mob_enabled = Some(v),
                    _ => npc.slavery_enabled = Some(v),
                }
            }
            Cmd::SetRoutine => {
                let who = self.who(&s.entity1);
                if s.id1.is_empty() {
                    self.diag(name, DiagKind::BadArgument, "routine name missing");
                }
                self.st.npc_mut(&who).routine = s.id1.clone();
                self.effect(s);
            }
            Cmd::SetPal => {
                let who = self.who(&s.entity1);
                if who == "player" {
                    self.st.guild = s.id1.clone();
                } else {
                    self.st.npc_mut(&who).pal = s.id1.clone();
                }
                self.effect(s);
            }
            Cmd::SetGuardStatus
            | Cmd::SetClass
            | Cmd::SetRevolutionEnabled
            | Cmd::SetRaidEnabled
            | Cmd::SetSleepingPoint
            | Cmd::SetWorkingPoint
            | Cmd::SetRelaxingPoint => {
                let who = self.who(&s.entity1);
                let value = if s.id2.is_empty() {
                    s.id1.clone()
                } else {
                    format!("{};{}", s.id1, s.id2)
                };
                self.st
                    .npc_mut(&who)
                    .settings
                    .insert(norm(&s.command), value);
                self.effect(s);
            }
            Cmd::SetSectorStatus => {
                let v = self.boolean(&s.id2);
                self.st.sectors.insert(s.id1.clone(), v);
                self.effect(s);
            }
            Cmd::SetChapter => {
                self.st.chapter = self.amount(name, &s.id1, 0);
            }
            Cmd::Sleep => {
                // [R 3 id 21] ID1 hours, advances time.
                let h = self.amount(name, &s.id1, 0);
                self.st.hours += u64::from(h);
                self.effect(s);
            }
            Cmd::EndGame => {
                self.st.game_over = true;
                self.effect(s);
            }
            Cmd::JoinPlayer => {
                let who = self.who(&s.entity1);
                if self.boolean(&s.id1) {
                    self.st.party.insert(who);
                } else {
                    self.st.party.remove(&who);
                }
                self.effect(s);
            }
            Cmd::JoinNpc => {
                let who = self.who(&s.entity2);
                self.st.party.insert(who);
                self.effect(s);
            }
            Cmd::Humanize => {
                self.effect(s);
            }
            Cmd::End => {
                self.res.ended = true;
                self.res.outputs.push(Output::End);
            }
            Cmd::SetEnclave | Cmd::PlayMusic | Cmd::Back => {} // no-ops in the original [R 3]
            Cmd::Excuse
            | Cmd::Bribe
            | Cmd::PickPocket
            | Cmd::Attack
            | Cmd::Wear
            | Cmd::Heal
            | Cmd::OpenDoor
            | Cmd::FullStop
            | Cmd::MoveTo
            | Cmd::DoTrade
            | Cmd::DoLoot
            | Cmd::StartRevolution
            | Cmd::Spawn
            | Cmd::ResetCrime
            | Cmd::DecreaseRevolCount => {
                self.effect(s);
            }
        }
    }
}

/// Check `Execute`'s pre-conditions [R 1.6]; `Some(reason)` = the engine refuses.
fn refusal(st: &GameState, info: &Info, cond: InfoCond) -> Option<&'static str> {
    if cond == InfoCond::Teach
        && info.teach_attrib_value > 0
        && !info.teach_attrib.is_empty()
        && st.learning_points < info.teach_attrib_value
    {
        return Some("IS_Running_NotEnoughLP");
    }
    if info.gold_cost > 0 && st.gold() < info.gold_cost {
        return Some("IS_Running_NotEnoughGold");
    }
    None
}

/// Execute `id` in a dialog with `partner`: pre-checks, gold, `OnDelivery`, the script,
/// `OnEndInfo` side effects, consumption. The caller decided the info was selectable.
pub fn execute_info(
    db: &GameDatabase,
    st: &mut GameState,
    id: InfoId,
    partner: &str,
) -> ExecResult {
    let info = db.info(id);
    let cond = InfoCond::from_raw(info.condition_type);
    if let Some(why) = refusal(st, info, cond) {
        return ExecResult {
            refused: Some(why.into()),
            ..ExecResult::default()
        };
    }
    let mut cx = Ctx {
        db,
        st,
        info: &info.name,
        partner,
        res: ExecResult::default(),
    };
    cx.st.npc_mut(partner).talked = true;
    // Consumption [R 1.5]: InfoGiven is set unless the info is permanent.
    if !is_permanent(db, id) {
        cx.st.told.insert(norm(&info.name));
    }
    if InfoType::from_raw(info.info_type) == InfoType::QuestRunning {
        let now = cx.st.hours.max(1);
        cx.st.info_last_run.insert(norm(&info.name), now);
    }
    if info.gold_cost > 0 {
        let p = norm(partner);
        cx.transfer("player", &p, GOLD_ITEM, info.gold_cost);
    }
    if matches!(cond, InfoCond::Delivery | InfoCond::PartDelivery) {
        on_delivery(&mut cx, info, cond);
    }
    for s in &info.script {
        cx.step(s);
    }
    on_end_info(&mut cx, info, cond);
    cx.res
}

/// `OnDelivery` [R 1.6]: runs when the script first advances (before the speech).
fn on_delivery(cx: &mut Ctx<'_>, info: &Info, cond: InfoCond) {
    let Some(q) = cx.db.quest_by_name(&info.quest) else {
        return;
    };
    let ty = QuestType::from_raw(q.quest_type);
    if ty == QuestType::HasItems {
        let list: Vec<(String, u32)> = q
            .delivery_entities
            .iter()
            .enumerate()
            .map(|(i, e)| (e.clone(), q.delivery_amounts.get(i).copied().unwrap_or(1)))
            .collect();
        let partner = norm(cx.partner);
        for (item, need) in list {
            let have_counter = questmgr::counter(cx.st, &q.name, &item);
            let missing = need.saturating_sub(have_counter);
            let give = missing.min(cx.st.item_count(&item));
            if give < missing {
                cx.diag(
                    "Delivery",
                    DiagKind::MissingItems,
                    format!("{} x {item}", missing - give),
                );
            }
            if give > 0 {
                // Items go from the player to the dialog partner; the hook counts them when
                // the partner is the quest destination.
                let owner = norm(&info.owner);
                let to = if owner == "pc_hero" {
                    partner.clone()
                } else {
                    owner
                };
                cx.transfer("player", &to, &item, give);
            }
        }
    }
    if cond != InfoCond::Delivery {
        return;
    }
    let name = q.name.clone();
    match ty {
        QuestType::Report | QuestType::DriveAway => {
            let owner = info.owner.clone();
            if q.delivery_entities
                .iter()
                .any(|e| e.eq_ignore_ascii_case(&owner))
            {
                let e = cx.st.quests.entry(norm(&name)).or_default();
                *e.progress.entry(norm(&owner)).or_insert(0) += 1;
            }
            questmgr::check_delivery_status(cx.db, cx.st, &name, &mut cx.res);
        }
        QuestType::Defeat | QuestType::BringNpc | QuestType::SpellCast | QuestType::FollowNpc => {
            questmgr::set_status(cx.db, cx.st, &name, QuestStatus::Success, &mut cx.res);
        }
        _ => {}
    }
}

/// `OnEndInfo` side effects by condition type [R 1.6].
fn on_end_info(cx: &mut Ctx<'_>, info: &Info, cond: InfoCond) {
    // Quest log pass: speech lines of the listed condition types are appended to the quest log.
    let logged = matches!(
        cond,
        InfoCond::General
            | InfoCond::Overtime
            | InfoCond::Open
            | InfoCond::Activator
            | InfoCond::Running
            | InfoCond::Delivery
            | InfoCond::Success
            | InfoCond::DoCancel
            | InfoCond::Ready
    );
    for s in &info.script {
        if Cmd::parse(&s.command) == Some(Cmd::AddQuestLog) {
            let q = s.id1.split('#').next().unwrap_or("").to_string();
            if !cx.quest_exists("AddQuestLog", &q) {
                continue;
            }
            let text = cx.say_text("AddQuestLog", &s.text);
            cx.st.log.push(LogEntry {
                quest: norm(&q),
                kind: LogKind::Text(s.text.clone()),
            });
            cx.res.outputs.push(Output::QuestLog { quest: q, text });
        }
    }
    if logged
        && !info.quest.is_empty()
        && !info.suppress_log
        && cx.db.quest_id(&info.quest).is_some()
    {
        cx.st.log.push(LogEntry {
            quest: norm(&info.quest),
            kind: LogKind::Dialog(info.name.clone()),
        });
    }
    if info.quest.is_empty() || cx.db.quest_id(&info.quest).is_none() {
        // Teaching is independent of quests.
        teach(cx, info, cond);
        return;
    }
    let (db, quest) = (cx.db, info.quest.clone());
    match cond {
        InfoCond::Activator | InfoCond::Reactivator => {
            questmgr::set_status(db, cx.st, &quest, QuestStatus::Running, &mut cx.res);
            if QuestType::from_raw(db.quest_by_name(&quest).map_or(99, |q| q.quest_type))
                == QuestType::FollowNpc
            {
                // Any other running FollowNpc quest is lost.
                let others: Vec<String> = db
                    .quests
                    .iter()
                    .filter(|q| QuestType::from_raw(q.quest_type) == QuestType::FollowNpc)
                    .filter(|q| !q.name.eq_ignore_ascii_case(&quest))
                    .filter(|q| cx.st.quest_status(&q.name) == QuestStatus::Running)
                    .map(|q| q.name.clone())
                    .collect();
                for o in others {
                    questmgr::set_status(db, cx.st, &o, QuestStatus::Lost, &mut cx.res);
                }
            }
        }
        InfoCond::DoCancel => questmgr::set_cancelled(db, cx.st, &quest, &mut cx.res),
        _ => {}
    }
    teach(cx, info, cond);
}

/// Teach: raise the attribute / learn the skill and pay learning points [R 1.6].
fn teach(cx: &mut Ctx<'_>, info: &Info, cond: InfoCond) {
    if cond != InfoCond::Teach {
        return;
    }
    if !info.teach_attrib.is_empty() && info.teach_attrib_value > 0 {
        let tag = norm(&info.teach_attrib);
        // HP/MP/SP grow by 4 units per point, everything else by 1.
        let m = if matches!(tag.as_str(), "hp" | "mp" | "sp") {
            4
        } else {
            1
        };
        *cx.st.attributes.entry(tag).or_insert(0) += (info.teach_attrib_value * m) as i32;
        cx.st.learning_points -= info.teach_attrib_value.min(cx.st.learning_points);
    } else if !info.teach_skill.is_empty() {
        // Skill LP costs live in the skill property sets (not decoded): free for now [U].
        cx.st.skills.insert(norm(&info.teach_skill));
    }
}

// ---- world notifications (thin wrappers over the quest manager hooks) -----------------

pub fn npc_killed(db: &GameDatabase, st: &mut GameState, npc: &str) -> ExecResult {
    let mut r = ExecResult::default();
    questmgr::on_npc_killed(db, st, npc, &mut r);
    r
}

pub fn npc_defeated(db: &GameDatabase, st: &mut GameState, npc: &str) -> ExecResult {
    let mut r = ExecResult::default();
    questmgr::on_npc_defeated(db, st, npc, &mut r);
    r
}

pub fn area_entered(db: &GameDatabase, st: &mut GameState, who: &str, area: &str) -> ExecResult {
    let mut r = ExecResult::default();
    questmgr::on_enter(db, st, who, area, &mut r);
    r
}
