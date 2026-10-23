//! Info availability predicates, aligned to `docs/research/GAME_RULES.md` section 1.
//!
//! This module is the single place that encodes `gCInfo_PS::IsAvailable` /
//! `AreConditionsFulfilled`. Rule references use `[R x.y]` (GAME_RULES section); `[I]`
//! marks parts the document leaves inferred or that depend on a world simulation which
//! does not exist yet (see [`Assumptions`]).
use crate::db::{GameDatabase, InfoId};
use crate::model::{InfoCond, InfoType, NpcStatus, QuestStatus, QuestType};
use crate::state::{GameState, norm};

/// How to treat conditions that depend on world state we do not simulate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Assumptions {
    /// `CondOwnerNearEntity` [R 1.2.3]: pretend the NPC is within 500 of the entity.
    pub owner_near_entity: bool,
    /// Inventories of NPCs not present in the state (template inventories are undecoded).
    pub unknown_npc_inventory: bool,
    /// Dialog flags (`TeachEnabled`, `TradeEnabled`, `PartyEnabled`, ...) that no script
    /// set yet default to enabled (the template default is not decoded).
    pub default_npc_flags: bool,
    /// Engine/script-triggered reactions whose trigger source does not exist yet (crime,
    /// guard warnings, pickpocket, heal, animal, friend checks, delivery proximity).
    pub triggered_reactions: bool,
    /// `EnclaveFriend` / `PoliticalFriend` lines need fame >= 75 (`Execute` refuses with
    /// `IS_Running_NoEnclaveReputation` / `NoPoliticalReputation`, GAME_RULES 1.6). The NPC's
    /// enclave / alignment come from its template, which is not decoded, so any enclave
    /// (alignment) with fame >= 75 counts [I].
    pub friend_reputation_gate: bool,
}

impl Assumptions {
    pub const STRICT: Self = Self {
        owner_near_entity: false,
        unknown_npc_inventory: false,
        default_npc_flags: false,
        triggered_reactions: false,
        friend_reputation_gate: true,
    };
    /// Interactive play without a world: dialog conditions pass, reactions never fire, and
    /// teach/trade/party flags are off until an info script enables them.
    pub const PERMISSIVE: Self = Self {
        owner_near_entity: true,
        unknown_npc_inventory: true,
        default_npc_flags: false,
        triggered_reactions: false,
        friend_reputation_gate: true,
    };
    /// Everything the world might provide is possible (linter, concrete playthrough).
    pub const OPTIMISTIC: Self = Self {
        owner_near_entity: true,
        unknown_npc_inventory: true,
        default_npc_flags: true,
        triggered_reactions: true,
        friend_reputation_gate: false,
    };
}

/// Why an info is not available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unmet {
    /// `InfoGiven` and not an `End` info [R 1.1].
    AlreadyGiven,
    UnknownConditionType(u32),
    /// The info names a quest that does not exist, or a quest-bound condition has none [R 1.2.2].
    MissingQuest(String),
    QuestStatus {
        quest: String,
        have: QuestStatus,
        need: &'static str,
    },
    PrerequisiteQuest(String),
    Delivery(String),
    Event(String),
    EventPresent(String),
    Items(String),
    NpcStatus {
        npc: String,
        status: u32,
    },
    NotNear(String),
    NotInParty(String),
    /// The NPC does not offer this exact Teach subject.
    TeachNotOffered(String),
    /// The player already knows the skill offered by this info.
    SkillAlreadyLearned(String),
    Overtime(String),
    TooSoon(String),
    NoChildren,
    /// Engine/script-fired condition without a trigger (Fight, LongTimeNoSee, ...).
    NotTriggered(&'static str),
    Flag(&'static str),
}

/// Statuses in which a quest-bound condition type holds [R 1.3]. `None` = no status gate.
pub fn required_statuses(cond: InfoCond) -> Option<&'static [QuestStatus]> {
    use InfoCond::*;
    use QuestStatus as S;
    Some(match cond {
        Open | Activator => &[S::Open],
        Ready => &[S::Open, S::Running],
        Overtime | Running | Delivery | PartDelivery | DoCancel => &[S::Running],
        Reactivator | Lost => &[S::Lost],
        Success => &[S::Success],
        Won => &[S::Won],
        Failed => &[S::Failed],
        Cancelled => &[S::Cancelled],
        _ => return None,
    })
}

/// Condition types that read the bound quest (error + false without one) [R 1.3].
pub fn needs_quest(cond: InfoCond) -> bool {
    required_statuses(cond).is_some()
}

/// Is this delivery entity an inventory item (rather than an NPC/area)? Uses the template
/// list when loaded, else the `It_` naming convention [I].
pub fn is_item_name(db: &GameDatabase, name: &str) -> bool {
    if let Some(k) = &db.known_items {
        return k.contains(&norm(name));
    }
    name.len() > 3 && name[..3].eq_ignore_ascii_case("it_")
}

/// (player holds all required items, holds some but not all) for a HasItems quest
/// (`HaveNPCsAllRequiredItems` / `HasNPCpartOfRequiredItems`) [R 1.3].
pub fn held_items(db: &GameDatabase, st: &GameState, quest: &str) -> (bool, bool) {
    let Some(q) = db.quest_by_name(quest) else {
        return (false, false);
    };
    let (mut all, mut some) = (true, false);
    for (i, ent) in q.delivery_entities.iter().enumerate() {
        let need = q.delivery_amounts.get(i).copied().unwrap_or(1).max(1);
        let have = st
            .item_count(ent)
            .saturating_add(st.quest_progress(quest, ent));
        some |= have > 0;
        all &= have >= need;
    }
    (all && !q.delivery_entities.is_empty(), some)
}

fn container_has(st: &GameState, assume: Assumptions, container: &str, item: &str, n: u32) -> bool {
    let have =
        if container.eq_ignore_ascii_case("pc_hero") || container.eq_ignore_ascii_case("player") {
            st.item_count(item)
        } else {
            match st.npc(container) {
                Some(npc) if !npc.inventory.is_empty() => npc.inventory.count(item),
                // Unknown inventory: pretend it holds what the info asks for.
                _ if assume.unknown_npc_inventory => return true,
                _ => 0,
            }
        };
    if n == 0 { have == 0 } else { have >= n }
}

/// `IsAvailable` [R 1.1] for the dialog with `partner`.
pub fn evaluate(
    db: &GameDatabase,
    st: &GameState,
    assume: Assumptions,
    id: InfoId,
    partner: &str,
) -> Result<(), Unmet> {
    let info = db.info(id);
    let cond = InfoCond::from_raw(info.condition_type);
    if st.told.contains(&norm(&info.name)) && cond != InfoCond::End {
        return Err(Unmet::AlreadyGiven);
    }
    fulfilled(db, st, assume, id, partner, 0)
}

/// `AreConditionsFulfilled` [R 1.2].
fn fulfilled(
    db: &GameDatabase,
    st: &GameState,
    assume: Assumptions,
    id: InfoId,
    partner: &str,
    depth: u8,
) -> Result<(), Unmet> {
    let info = db.info(id);
    let cond = InfoCond::from_raw(info.condition_type);
    // [R 1.2.2] A named quest must exist, for every condition type.
    if !info.quest.is_empty() && db.quest_by_name(&info.quest).is_none() {
        return Err(Unmet::MissingQuest(info.quest.clone()));
    }
    // [R 1.2.3]
    if !info.cond_owner_near_entity.is_empty()
        && !assume.owner_near_entity
        && !st.near.contains(&norm(&info.cond_owner_near_entity))
    {
        return Err(Unmet::NotNear(info.cond_owner_near_entity.clone()));
    }
    // [R 1.2.4] Types above 53 are false; 40..49 and 30 are "script/engine fired only" [R 1.3].
    if let InfoCond::Unknown(v) = cond {
        return Err(Unmet::UnknownConditionType(v));
    }
    predicate(db, st, assume, info, cond, partner)?;
    common(db, st, assume, id, partner, depth)
}

fn flag(opt: Option<bool>, assume: Assumptions, what: &'static str) -> Result<(), Unmet> {
    if opt.unwrap_or(assume.default_npc_flags) {
        Ok(())
    } else {
        Err(Unmet::Flag(what))
    }
}

/// `IsFriend` threshold of the enclave / political fame (GAME_RULES constants).
const FRIEND_FAME: i32 = 75;

fn friend(
    st: &GameState,
    assume: Assumptions,
    prefix: &str,
    what: &'static str,
) -> Result<(), Unmet> {
    if !assume.friend_reputation_gate
        || st
            .fame
            .iter()
            .any(|(k, v)| k.starts_with(prefix) && *v >= FRIEND_FAME)
    {
        Ok(())
    } else {
        Err(Unmet::Flag(what))
    }
}

fn reaction(assume: Assumptions, what: &'static str) -> Result<(), Unmet> {
    if assume.triggered_reactions {
        Ok(())
    } else {
        Err(Unmet::NotTriggered(what))
    }
}

/// Per-condition-type predicate [R 1.3].
fn predicate(
    db: &GameDatabase,
    st: &GameState,
    assume: Assumptions,
    info: &gothic_formats::info::Info,
    cond: InfoCond,
    partner: &str,
) -> Result<(), Unmet> {
    use InfoCond::*;
    let npc = st.npc(partner).cloned().unwrap_or_default();
    let in_party = st.party.contains(&norm(partner));
    if let Some(allowed) = required_statuses(cond) {
        if info.quest.is_empty() {
            return Err(Unmet::MissingQuest(String::new()));
        }
        let have = st.quest_status(&info.quest);
        if !allowed.contains(&have) {
            return Err(Unmet::QuestStatus {
                quest: info.quest.clone(),
                have,
                need: match cond {
                    Open | Activator => "Open",
                    Ready => "Open or Running",
                    Reactivator | Lost => "Lost",
                    Success => "Success",
                    Won => "Won",
                    Failed => "Failed",
                    Cancelled => "Cancelled",
                    _ => "Running",
                },
            });
        }
    }
    let quest = db.quest_by_name(&info.quest);
    match cond {
        Fight => return Err(Unmet::NotTriggered("Fight (only started by script)")),
        LongTimeNoSee => return Err(Unmet::NotTriggered("LongTimeNoSee")),
        Hello => {
            if npc.talked {
                return Err(Unmet::Flag("already talked"));
            }
        }
        Overtime => {
            let q = quest.ok_or_else(|| Unmet::MissingQuest(info.quest.clone()))?;
            let limit = (u64::from(q.running_time_years) * 365 + u64::from(q.running_time_days))
                * 24
                + u64::from(q.running_time_hours);
            if !st.quest_overtime(&info.quest, limit) {
                return Err(Unmet::Overtime(info.quest.clone()));
            }
        }
        Open | Activator | Reactivator => {
            // [R 1.3] AreFinishedQuestsFinished; unknown prerequisite quests count as finished [R 1.4].
            if let Some(q) = quest {
                for pre in &q.finished_quests {
                    if db.quest_by_name(pre).is_some() && !st.quest_status(pre).is_finished() {
                        return Err(Unmet::PrerequisiteQuest(pre.clone()));
                    }
                }
                // Arena destination must not be running: arenas are not simulated [I].
            }
        }
        Delivery | PartDelivery => {
            let q = quest.ok_or_else(|| Unmet::MissingQuest(info.quest.clone()))?;
            let ty = QuestType::from_raw(q.quest_type);
            let (all, some) = held_items(db, st, &info.quest);
            if cond == PartDelivery {
                // Type HasItems, partial items, destination exists.
                if !(ty == QuestType::HasItems && some && !all) {
                    return Err(Unmet::Delivery(info.quest.clone()));
                }
            } else {
                let ok = match ty {
                    QuestType::HasItems => {
                        q.destination_entity.eq_ignore_ascii_case(&info.owner) && all
                    }
                    // Destination NPC must be defeated (missing destination passes).
                    QuestType::Defeat => {
                        q.destination_entity.is_empty()
                            || st.npc(&q.destination_entity).is_some_and(|n| n.defeated)
                    }
                    // Proximity / follower checks need a world simulation [I].
                    QuestType::BringNpc | QuestType::FollowNpc => {
                        assume.triggered_reactions || assume.owner_near_entity
                    }
                    _ => true,
                };
                if !ok {
                    return Err(Unmet::Delivery(info.quest.clone()));
                }
            }
        }
        Join => {
            flag(npc.party_enabled, assume, "PartyEnabled")?;
            if in_party {
                return Err(Unmet::Flag("already in party"));
            }
        }
        MobJoin => {
            flag(npc.mob_enabled, assume, "MobEnabled")?;
            if in_party {
                return Err(Unmet::Flag("already in party"));
            }
        }
        SlaveJoin => {
            flag(npc.slavery_enabled, assume, "SlaveryEnabled")?;
            if in_party {
                return Err(Unmet::Flag("already in party"));
            }
        }
        Dismiss | MobDismiss | Wait | Companion => {
            if !in_party {
                return Err(Unmet::NotInParty(partner.to_string()));
            }
        }
        Teach => {
            flag(npc.teach_enabled, assume, "TeachEnabled")?;
            if !info.teach_attrib.is_empty() {
                let subject = norm(&info.teach_attrib);
                if !npc.teach_attribs.contains(&subject) {
                    return Err(Unmet::TeachNotOffered(info.teach_attrib.clone()));
                }
            } else {
                let subject = norm(&info.teach_skill);
                if !npc.teach_skills.contains(&subject) {
                    return Err(Unmet::TeachNotOffered(info.teach_skill.clone()));
                }
                if !subject.is_empty() && st.skills.contains(&subject) {
                    return Err(Unmet::SkillAlreadyLearned(info.teach_skill.clone()));
                }
            }
        }
        Trade => flag(npc.trade_enabled, assume, "TradeEnabled")?,
        PickPocket => reaction(assume, "PickPocket")?,
        MasterThief => reaction(assume, "MasterThief")?,
        Heal => reaction(assume, "Heal")?,
        EnclaveCrime => reaction(assume, "EnclaveCrime")?,
        PoliticalCrime => reaction(assume, "PoliticalCrime")?,
        FirstWarn => reaction(assume, "FirstWarn")?,
        SecondWarn => reaction(assume, "SecondWarn")?,
        Animal => reaction(assume, "Animal")?,
        NotFriend | NotFriendOrThief => reaction(assume, "NotFriend")?,
        // Crime(0): guard/crime flag [I] - a reaction without trigger.
        Crime => reaction(assume, "Crime")?,
        // No predicate beyond the common checks.
        EnclaveFriend => friend(st, assume, "enc:", "EnclaveFriend (fame >= 75)")?,
        PoliticalFriend => friend(st, assume, "pol:", "PoliticalFriend (fame >= 75)")?,
        General | NothingToSay | End | Back | StepBack => {}
        // Status-only types were handled by the quest gate above.
        Running | DoCancel | Success | Won | Failed | Cancelled | Lost | Ready => {}
        Unknown(_) => {}
    }
    Ok(())
}

/// Common checks [R 1.2.6].
fn common(
    db: &GameDatabase,
    st: &GameState,
    assume: Assumptions,
    id: InfoId,
    partner: &str,
    depth: u8,
) -> Result<(), Unmet> {
    let info = db.info(id);
    // a. item container: skipped when no container is named.
    for (i, item) in info.cond_items.iter().enumerate() {
        let Some(container) = info
            .cond_item_container
            .get(i)
            .or_else(|| info.cond_item_container.first())
        else {
            break;
        };
        let n = info.cond_item_amounts.get(i).copied().unwrap_or(1);
        if !container_has(st, assume, container, item, n) {
            return Err(Unmet::Items(item.clone()));
        }
    }
    // c. player knows / knows not. Game events live in the player-memory `PlayerKnows`
    // list [R 3 id 35, I link]; given infos are accepted as knowledge too [I].
    let knows = |e: &str| st.knows(e) || st.told.contains(&norm(e));
    for ev in &info.cond_player_knows {
        if !knows(ev) {
            return Err(Unmet::Event(ev.clone()));
        }
    }
    for ev in &info.cond_player_knows_not {
        if knows(ev) {
            return Err(Unmet::EventPresent(ev.clone()));
        }
    }
    // e. secondary NPCs.
    for (i, npc) in info.cond_secondary_npc.iter().enumerate() {
        let raw = info.cond_secondary_npc_states.get(i).copied().unwrap_or(0);
        let ok = NpcStatus::from_raw(raw).is_some_and(|s| st.npc_has_status(npc, s));
        if !ok {
            return Err(Unmet::NpcStatus {
                npc: npc.clone(),
                status: raw,
            });
        }
    }
    // h. party members: at least as many listed members in the party as listed.
    let wanted: Vec<&str> = info
        .cond_player_party_member
        .split(';')
        .filter(|s| !s.is_empty())
        .collect();
    if !wanted.is_empty() {
        let n = wanted
            .iter()
            .filter(|w| st.party.contains(&norm(w)))
            .count();
        if n < wanted.len() {
            return Err(Unmet::NotInParty(info.cond_player_party_member.clone()));
        }
    }
    // i. QuestRunning: running and at least 2 hours since activation / last run.
    let ty = InfoType::from_raw(info.info_type);
    if ty == InfoType::QuestRunning {
        if st.quest_status(&info.quest) != QuestStatus::Running {
            return Err(Unmet::QuestStatus {
                quest: info.quest.clone(),
                have: st.quest_status(&info.quest),
                need: "Running",
            });
        }
        let base = st
            .info_last_run
            .get(&norm(&info.name))
            .copied()
            .filter(|t| *t > 0)
            .or_else(|| {
                st.quests
                    .get(&norm(&info.quest))
                    .and_then(|q| q.activation_hours)
            })
            .unwrap_or(0);
        if st.hours.saturating_sub(base) < 2 {
            return Err(Unmet::TooSoon(info.name.clone()));
        }
    }
    // j. Parent infos are available only if a child is.
    if ty == InfoType::Parent {
        let any = db
            .children_of(&info.name)
            .iter()
            .filter(|&&c| owner_ok(db, c, partner))
            .filter(|&&c| {
                matches!(
                    InfoType::from_raw(db.info(c).info_type),
                    InfoType::Info | InfoType::Parent
                )
            })
            .any(|&c| {
                depth < 8
                    && !is_given(db, st, c)
                    && fulfilled(db, st, assume, c, partner, depth + 1).is_ok()
            });
        if !any {
            return Err(Unmet::NoChildren);
        }
    }
    Ok(())
}

fn is_given(db: &GameDatabase, st: &GameState, id: InfoId) -> bool {
    let i = db.info(id);
    st.told.contains(&norm(&i.name)) && InfoCond::from_raw(i.condition_type) != InfoCond::End
}

/// Child infos must belong to the dialog partner or to the player (`PC_Hero`) [R 1.5].
pub fn owner_ok(db: &GameDatabase, id: InfoId, partner: &str) -> bool {
    let o = &db.info(id).owner;
    o.eq_ignore_ascii_case(partner) || o.eq_ignore_ascii_case("pc_hero")
}

/// `IsPermanent` [R 1.5]: the info is not consumed after it ran.
pub fn is_permanent(db: &GameDatabase, id: InfoId) -> bool {
    let i = db.info(id);
    let cond = InfoCond::from_raw(i.condition_type);
    matches!(
        InfoType::from_raw(i.info_type),
        InfoType::Refuse | InfoType::Parent
    ) || matches!(
        cond,
        InfoCond::PartDelivery | InfoCond::Back | InfoCond::End | InfoCond::StepBack
    ) || i.permanent
        || i.owner.eq_ignore_ascii_case("pc_hero")
}
