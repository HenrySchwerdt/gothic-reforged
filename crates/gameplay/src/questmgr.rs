//! Quest manager: the status state machine, rewards and the per-quest-type completion
//! hooks. Rules are from `docs/research/GAME_RULES.md` section 2 (references `[R 2.x]`);
//! rules not covered by that document are tagged `[I]`.
use crate::db::GameDatabase;
use crate::model::{QuestStatus, QuestType};
use crate::script::{DiagKind, Diagnostic, ExecResult, Output};
use crate::state::{GameState, norm};

/// `gEPoliticalAlignment` tag for the numeric `PoliticalSuccess` field (GAME_LOGIC_MAP §6).
pub fn alignment_tag(raw: u32) -> Option<&'static str> {
    Some(match raw {
        1 => "orc",
        2 => "nrd",
        3 => "reb",
        4 => "mid",
        5 => "ass",
        6 => "nom",
        7 => "out",
        8 => "slave",
        9 | 10 => "pir",
        _ => return None,
    })
}

/// World-sourced XP: `GiveXP(world, player, n)` multiplies by 5 [R 4.1].
pub fn give_world_xp(st: &mut GameState, amount: u32, res: &mut ExecResult) {
    if amount == 0 {
        return;
    }
    let total = u64::from(amount) * 5;
    let (before, after) = st.add_xp(total);
    res.outputs.push(Output::Xp {
        amount: total as u32,
        level_before: before,
        level_after: after,
    });
}

fn diag(res: &mut ExecResult, kind: DiagKind, detail: String) {
    res.diagnostics.push(Diagnostic {
        info: "<quest manager>".into(),
        command: "quest".into(),
        kind,
        detail,
    });
}

/// `gCQuest_PS::SetStatus` [R 2.2]. Returns whether the status changed.
pub fn set_status(
    db: &GameDatabase,
    st: &mut GameState,
    quest: &str,
    to: QuestStatus,
    res: &mut ExecResult,
) -> bool {
    use QuestStatus::*;
    let Some(qid) = db.quest_id(quest) else {
        diag(res, DiagKind::UnknownQuest, quest.to_string());
        return false;
    };
    let from = st.quest_status(quest);
    let allowed = match to {
        Open => false,
        Running => matches!(from, Open | Lost),
        Success | Won | Failed => matches!(from, Open | Running),
        Obsolete => from == Open,
        Cancelled | Lost => from == Running,
    };
    if !allowed {
        return false;
    }
    if to == Running {
        let now = st.hours;
        st.quests.entry(norm(quest)).or_default().activation_hours = Some(now);
    }
    st.set_quest_status(quest, to);
    res.outputs.push(Output::QuestChanged {
        quest: quest.to_string(),
        from,
        to,
    });
    if matches!(to, Success | Won) {
        rewards(db, st, qid, res);
    }
    true
}

/// Success/Won rewards in the original order [R 2.3].
fn rewards(db: &GameDatabase, st: &mut GameState, qid: crate::db::QuestId, res: &mut ExecResult) {
    let q = db.quest(qid);
    if q.political_success_amount > 0
        && let Some(tag) = alignment_tag(q.political_success)
    {
        st.add_fame(&format!("pol:{tag}"), q.political_success_amount as i32);
    }
    if !q.attrib_success.is_empty() && q.attrib_success_amount != 0 {
        *st.attributes.entry(norm(&q.attrib_success)).or_insert(0) +=
            q.attrib_success_amount as i32;
    }
    if !q.enclave_success.is_empty() && q.enclave_success_amount > 0 {
        st.add_fame(
            &format!("enc:{}", norm(&q.enclave_success)),
            q.enclave_success_amount as i32,
        );
    }
    give_world_xp(st, q.experience_points, res);
}

// ---- manager entry points [R 2.1] -----------------------------------------------------

pub fn run_quest(db: &GameDatabase, st: &mut GameState, quest: &str, res: &mut ExecResult) {
    if st.quest_status(quest) == QuestStatus::Open {
        set_status(db, st, quest, QuestStatus::Running, res);
    }
}

pub fn succeed_quest(db: &GameDatabase, st: &mut GameState, quest: &str, res: &mut ExecResult) {
    if matches!(
        st.quest_status(quest),
        QuestStatus::Open | QuestStatus::Running
    ) {
        set_status(db, st, quest, QuestStatus::Success, res);
    }
}

pub fn fail_quest(db: &GameDatabase, st: &mut GameState, quest: &str, res: &mut ExecResult) {
    set_status(db, st, quest, QuestStatus::Failed, res);
}

/// `SetCancelled`: Open -> Obsolete, otherwise Cancelled [R 2.1].
pub fn set_cancelled(db: &GameDatabase, st: &mut GameState, quest: &str, res: &mut ExecResult) {
    let to = if st.quest_status(quest) == QuestStatus::Open {
        QuestStatus::Obsolete
    } else {
        QuestStatus::Cancelled
    };
    set_status(db, st, quest, to, res);
}

/// `SetFailed`: Open -> Obsolete, otherwise Failed [R 2.1].
pub fn set_failed(db: &GameDatabase, st: &mut GameState, quest: &str, res: &mut ExecResult) {
    let to = if st.quest_status(quest) == QuestStatus::Open {
        QuestStatus::Obsolete
    } else {
        QuestStatus::Failed
    };
    set_status(db, st, quest, to, res);
}

/// `CloseQuest`: Open -> Obsolete; anything else -> Cancelled (only works from Running) [R 2.1].
pub fn close_quest(db: &GameDatabase, st: &mut GameState, quest: &str, res: &mut ExecResult) {
    if st.quest_status(quest) == QuestStatus::Open {
        set_status(db, st, quest, QuestStatus::Obsolete, res);
    } else {
        set_status(db, st, quest, QuestStatus::Cancelled, res);
    }
}

// ---- delivery counters [R 1.4, 2.4] ----------------------------------------------------

pub fn counter(st: &GameState, quest: &str, entity: &str) -> u32 {
    st.quest_progress(quest, entity)
}

/// `CheckDeliveryEntitiesStatus`: runs for Open/Running/Lost; when every counter reached
/// its amount (vacuously true without entities) the quest succeeds (Won for Sparring).
pub fn check_delivery_status(
    db: &GameDatabase,
    st: &mut GameState,
    quest: &str,
    res: &mut ExecResult,
) {
    let Some(q) = db.quest_by_name(quest) else {
        return;
    };
    if !matches!(
        st.quest_status(quest),
        QuestStatus::Open | QuestStatus::Running | QuestStatus::Lost
    ) {
        return;
    }
    let done = q
        .delivery_entities
        .iter()
        .enumerate()
        .all(|(i, e)| counter(st, quest, e) >= q.delivery_amounts.get(i).copied().unwrap_or(1));
    if done {
        let to = if QuestType::from_raw(q.quest_type) == QuestType::Sparring {
            QuestStatus::Won
        } else {
            QuestStatus::Success
        };
        set_status(db, st, quest, to, res);
    }
}

fn bump(st: &mut GameState, quest: &str, entity: &str, delta: i64) {
    let e = st
        .quests
        .entry(norm(quest))
        .or_default()
        .progress
        .entry(norm(entity))
        .or_insert(0);
    *e = (i64::from(*e) + delta).max(0) as u32;
}

fn active(st: &GameState, quest: &str) -> bool {
    matches!(
        st.quest_status(quest),
        QuestStatus::Open | QuestStatus::Running | QuestStatus::Lost
    )
}

fn has_entity(q: &gothic_formats::quest::Quest, e: &str) -> bool {
    q.delivery_entities
        .iter()
        .any(|x| x.eq_ignore_ascii_case(e))
}

/// Quests of the given types that are still active and list `entity` as a delivery entity.
fn matching<'a>(
    db: &'a GameDatabase,
    st: &GameState,
    entity: &str,
    types: &[QuestType],
) -> Vec<&'a gothic_formats::quest::Quest> {
    db.quests
        .iter()
        .filter(|q| types.contains(&QuestType::from_raw(q.quest_type)))
        .filter(|q| active(st, &q.name) && has_entity(q, entity))
        .collect()
}

/// `OnNPCKilled` [R 2.4]. Counts kills and fails quests whose protected NPC died.
pub fn on_npc_killed(db: &GameDatabase, st: &mut GameState, npc: &str, res: &mut ExecResult) {
    if npc.eq_ignore_ascii_case("pc_hero") {
        return;
    }
    st.npc_mut(npc).dead = true;
    count_npc_killed(db, st, npc, res);
    fail_on_npc_death(db, st, npc, res);
}

/// Counting half of `OnNPCKilled` (Kill/Defeat/DriveAway objectives).
pub fn count_npc_killed(db: &GameDatabase, st: &mut GameState, npc: &str, res: &mut ExecResult) {
    for q in matching(
        db,
        st,
        npc,
        &[QuestType::Kill, QuestType::Defeat, QuestType::DriveAway],
    ) {
        bump(st, &q.name, npc, 1);
        check_delivery_status(db, st, &q.name, res);
    }
}

/// Failure half of `OnNPCKilled`: a protected NPC or destination died.
pub fn fail_on_npc_death(db: &GameDatabase, st: &mut GameState, npc: &str, res: &mut ExecResult) {
    // Failure hooks (delivery entity or destination died).
    let fails: Vec<String> = db
        .quests
        .iter()
        .filter(|q| active(st, &q.name))
        .filter(|q| {
            let ty = QuestType::from_raw(q.quest_type);
            let in_list = has_entity(q, npc);
            let dest = q.destination_entity.eq_ignore_ascii_case(npc);
            let unmet = || {
                q.delivery_entities.iter().enumerate().any(|(i, e)| {
                    e.eq_ignore_ascii_case(npc)
                        && counter(st, &q.name, e) < q.delivery_amounts.get(i).copied().unwrap_or(1)
                })
            };
            match ty {
                QuestType::Report | QuestType::SpellCast => in_list && unmet(),
                QuestType::Arena | QuestType::Sparring => in_list,
                QuestType::BringNpc => (in_list && unmet()) || dest,
                QuestType::FollowNpc => {
                    q.delivery_entities
                        .first()
                        .is_some_and(|e| e.eq_ignore_ascii_case(npc))
                        && unmet()
                }
                QuestType::HasItems => dest,
                _ => false,
            }
        })
        .map(|q| q.name.clone())
        .collect();
    for name in fails {
        set_failed(db, st, &name, res);
    }
}

/// `OnNPCDefeated` [R 2.4] (Defeat; Arena/Sparring while running).
pub fn on_npc_defeated(db: &GameDatabase, st: &mut GameState, npc: &str, res: &mut ExecResult) {
    st.npc_mut(npc).defeated = true;
    for q in matching(db, st, npc, &[QuestType::Defeat]) {
        bump(st, &q.name, npc, 1);
        check_delivery_status(db, st, &q.name, res);
    }
    for q in matching(db, st, npc, &[QuestType::Arena, QuestType::Sparring]) {
        if st.quest_status(&q.name) == QuestStatus::Running {
            bump(st, &q.name, npc, 1);
            check_delivery_status(db, st, &q.name, res);
        }
    }
}

/// `OnEnter(who, area)` for EnterArea quests [R 2.4].
pub fn on_enter(
    db: &GameDatabase,
    st: &mut GameState,
    who: &str,
    area: &str,
    res: &mut ExecResult,
) {
    let hits: Vec<(String, String)> = db
        .quests
        .iter()
        .filter(|q| QuestType::from_raw(q.quest_type) == QuestType::EnterArea)
        .filter(|q| active(st, &q.name) && q.destination_entity.eq_ignore_ascii_case(area))
        .filter(|q| {
            q.delivery_entities
                .first()
                .is_some_and(|e| e.eq_ignore_ascii_case(who))
        })
        .map(|q| (q.name.clone(), q.delivery_entities[0].clone()))
        .collect();
    for (name, ent) in hits {
        bump(st, &name, &ent, 1);
        check_delivery_status(db, st, &name, res);
    }
}

/// `OnEnclaveStateChanged` for FreeEnclave quests [R 2.4].
pub fn on_enclave_state_changed(
    db: &GameDatabase,
    st: &mut GameState,
    enclave: &str,
    res: &mut ExecResult,
) {
    for q in matching(db, st, enclave, &[QuestType::FreeEnclave]) {
        bump(st, &q.name, enclave, 1);
        check_delivery_status(db, st, &q.name, res);
    }
}

/// `OnSpellCast(caster, target, spell)` for SpellCast quests [R 2.4].
pub fn on_spell_cast(
    db: &GameDatabase,
    st: &mut GameState,
    target: &str,
    spell: &str,
    res: &mut ExecResult,
) {
    let hits: Vec<String> = matching(db, st, target, &[QuestType::SpellCast])
        .into_iter()
        .filter(|q| q.spell_name.eq_ignore_ascii_case(spell))
        .map(|q| q.name.clone())
        .collect();
    for name in hits {
        bump(st, &name, target, 1);
        check_delivery_status(db, st, &name, res);
    }
}

/// `OnReceiveItem(from, to, item, n)` for HasItems / Plunder quests [R 2.4]. Entity names
/// are `player` or NPC/container names; the quest destination is compared by name.
pub fn on_receive_item(
    db: &GameDatabase,
    st: &mut GameState,
    from: &str,
    to: &str,
    item: &str,
    n: u32,
    res: &mut ExecResult,
) {
    let hits: Vec<(String, i64)> = db
        .quests
        .iter()
        .filter(|q| {
            matches!(
                QuestType::from_raw(q.quest_type),
                QuestType::HasItems | QuestType::Plunder
            )
        })
        .filter(|q| active(st, &q.name) && has_entity(q, item))
        .filter_map(|q| {
            let dest = &q.destination_entity;
            let plunder = QuestType::from_raw(q.quest_type) == QuestType::Plunder;
            let into = to.eq_ignore_ascii_case(dest);
            let out = from.eq_ignore_ascii_case(dest);
            let d = match (into, out, plunder) {
                (true, _, false) | (_, true, true) => i64::from(n),
                (_, true, false) | (true, _, true) => -i64::from(n),
                _ => return None,
            };
            Some((q.name.clone(), d))
        })
        .collect();
    for (name, d) in hits {
        bump(st, &name, item, d);
        if d > 0 {
            check_delivery_status(db, st, &name, res);
        }
    }
}
