//! B03: the routine engine proper: task selection, destination selection, `ContinueRoutine`,
//! heal-self gating, ambient action choice and destination bookkeeping.
//!
//! The work-station state bodies live in [`crate::routine::work`]; the player-party calls in
//! [`crate::routine::party`].

use crate::enums::{Action, AiMode, EnclaveStatus, NpcClass, PartyMemberType, Species, UseType};
use crate::host::{EntityId, Pool, ScriptHost};
use crate::routine::state::State;
use crate::routine::time::routine_time;
use crate::stats;

/// One row of the use-type to task table (`SelectRoutineTask`, S:a9440).
#[derive(Debug, Clone, Copy)]
struct TaskRow {
    use_type: UseType,
    task: State,
    /// Orcs run the same task (rows the DLL's Orc switch shares with the Human switch).
    orc: bool,
}

const fn row(use_type: UseType, task: State, orc: bool) -> TaskRow {
    TaskRow {
        use_type,
        task,
        orc,
    }
}

/// Human table. Unlisted use types give `ZS_HangAround`.
const HUMAN_TASKS: &[TaskRow] = &[
    row(UseType::Anvil, State::Smith, true),
    row(UseType::Forge, State::Smith, true),
    row(UseType::GrindStone, State::Grind, true),
    row(UseType::Cauldron, State::Cook, true),
    row(UseType::Barbecue, State::Barbecue, true),
    row(UseType::Alchemy, State::Alchemy, true),
    row(UseType::Bookshelf, State::Study, true),
    row(UseType::Bookstand, State::Study, true),
    row(UseType::TakeStone, State::CarryStones, false),
    row(UseType::DropStone, State::CarryStones, false),
    row(UseType::PickOre, State::Digger, false),
    row(UseType::PickGround, State::Digger, false),
    row(UseType::DigGround, State::Digger, false),
    row(UseType::Field, State::Farmer, false),
    row(UseType::Repair, State::Woodwork, false),
    row(UseType::SawLog, State::Woodwork, false),
    row(UseType::Lumberjack, State::Lumberjack, false),
    row(UseType::Bed, State::Sleep, true),
    row(UseType::SleepGround, State::Sleep, true),
    row(UseType::CleanFloor, State::SlaveBabe, false),
    row(UseType::Dance, State::SlaveBabe, false),
    row(UseType::FanBoss, State::SlaveBabe, false),
    row(UseType::Boss, State::Boss, true),
    row(UseType::Throne, State::Boss, true),
    row(UseType::Pace, State::Boss, true),
    row(UseType::Bard, State::Bard, false),
    row(UseType::Preach, State::Preach, true),
    row(UseType::Guard, State::Guard, true),
    row(UseType::Trader, State::Trader, true),
    row(UseType::Shrine, State::Pray, true),
    row(UseType::AttackPoint, State::RunToAttackPoint, false),
];

/// Use types that start the Orc party task (Orcs only; Humans fall through to `HangAround`).
const ORC_PARTY_USES: &[UseType] = &[UseType::OrcDance, UseType::OrcDrum, UseType::OrcBoulder];

/// Port of the use-type switch of `SelectRoutineTask` (S:a9440): the task for a destination
/// point's use type.
///
/// Humans use the full table, Orcs the rows marked shared plus the three Orc party points, every
/// other species sleeps on the ground (`SleepGround` gives `ZS_AnimalSleep`), roams (`EatGround`
/// and `Roam` give `ZS_Monster`) or ambles (`ZS_Ambient`). `use_type == None` means the NPC has
/// no destination point.
pub fn task_for_use_type(species: Species, use_type: UseType) -> State {
    gothic_portcov::hit!("task_for_use_type");
    match species {
        Species::Human => HUMAN_TASKS
            .iter()
            .find(|r| r.use_type == use_type)
            .map_or(State::HangAround, |r| r.task),
        Species::Orc => {
            if ORC_PARTY_USES.contains(&use_type) {
                return State::OrcParty;
            }
            HUMAN_TASKS
                .iter()
                .find(|r| r.use_type == use_type && r.orc)
                .map_or(State::HangAround, |r| r.task)
        }
        _ => match use_type {
            UseType::SleepGround => State::AnimalSleep,
            UseType::EatGround | UseType::Roam => State::Monster,
            _ => State::Ambient,
        },
    }
}

/// Port of `SelectRoutineTask` (S:a9440): choose and set the NPC's task.
///
/// First clears the current target, then by priority: a running arena fight `ZS_Arena`; the NPC
/// leads the player's party `ZS_GuidePlayer`; the player leads the NPC `ZS_WaitForPlayer` while
/// waiting, else `ZS_FollowPlayer`; a controlled follower of another NPC `ZS_FollowNPC`;
/// otherwise the destination point's use type via [`task_for_use_type`].
pub fn select_routine_task(host: &mut dyn ScriptHost, npc: EntityId) -> State {
    gothic_portcov::hit!("select_routine_task");
    host.set_current_target(npc, EntityId::NONE);
    let player = host.player();
    let task = if host.arena_active(npc) {
        State::Arena
    } else if host.party_leader(player) == npc {
        State::GuidePlayer
    } else if host.party_leader(npc) == player {
        if host.party_waiting(npc) {
            State::WaitForPlayer
        } else {
            State::FollowPlayer
        }
    } else if host.party_member_type(npc) == PartyMemberType::Controlled
        && host.party_leader(npc).is_some()
    {
        State::FollowNpc
    } else {
        let point = host.destination_point(npc);
        let use_type = if point.is_some() {
            host.point_use_type(point)
        } else {
            UseType::None
        };
        task_for_use_type(host.species(npc), use_type)
    };
    host.set_task(npc, task);
    task
}

/// Port of `IsFreeDestination` (S:a9d00): can `npc` use interact point `point`?
///
/// A point without an interaction property is never free; an owned point is free only for its
/// owner; an anchor-backed point needs spare capacity (`MaxUsers - UserCount > 0`, where
/// `MaxUsers` defaults to the number of interact points and the NPC's own current slot does not
/// count against it); a point with a user other than the NPC is taken.
pub fn is_free_destination(host: &dyn ScriptHost, npc: EntityId, point: EntityId) -> bool {
    gothic_portcov::hit!("is_free_destination");
    if point.is_none() || host.point_use_type(point) == UseType::None {
        return false;
    }
    let owner = host.point_owner(point);
    if owner.is_some() && owner != npc {
        return false;
    }
    let anchor = host.point_anchor(point);
    if anchor.is_some() {
        let mut max = host.anchor_max_users(anchor);
        if max == 0 {
            max = host.anchor_points(anchor).len() as u32;
        }
        let mut used = host.anchor_user_count(anchor);
        let mine = host.destination_point(npc);
        if mine.is_some() && host.point_anchor(mine) == anchor {
            used = used.saturating_sub(1);
        }
        if max <= used {
            return false;
        }
    }
    let user = host.point_user(point);
    user.is_none() || user == npc
}

/// Port of `SetCurrentDestinationPoint` (S:a7ca0): move the NPC's reservation to `new_point`.
///
/// Returns `false` (nothing changed) when `new_point` is already the destination and either
/// empty or reserved by the NPC. Otherwise releases the old point (clears its user and decrements
/// its anchor's `UserCount`), claims the new one when it is unclaimed (sets the user and
/// increments the count), clears `IsOnDestination` and stores the point.
pub fn set_current_destination_point(
    host: &mut dyn ScriptHost,
    npc: EntityId,
    new_point: EntityId,
) -> bool {
    gothic_portcov::hit!("set_current_destination_point");
    let old = host.destination_point(npc);
    if old == new_point && (new_point.is_none() || host.point_user(new_point) == npc) {
        return false;
    }
    if old.is_some() && host.point_use_type(old) != UseType::None {
        host.set_point_user(old, EntityId::NONE);
        let anchor = host.point_anchor(old);
        if anchor.is_some() {
            let n = host.anchor_user_count(anchor);
            host.set_anchor_user_count(anchor, n.saturating_sub(1));
        }
    }
    if new_point.is_some()
        && host.point_use_type(new_point) != UseType::None
        && host.point_user(new_point).is_none()
    {
        host.set_point_user(new_point, npc);
        let anchor = host.point_anchor(new_point);
        if anchor.is_some() {
            let n = host.anchor_user_count(anchor);
            host.set_anchor_user_count(anchor, n + 1);
        }
    }
    host.set_on_destination(npc, false);
    host.set_destination_point(npc, new_point);
    true
}

/// Outcome of [`select_routine_destination`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    /// Nothing to do: arena fight, player-party member, already travelling or no routine anchor.
    Skipped,
    /// Anchor exists but none of its points is free ("Can't find free destination for NPC").
    NotFound,
    /// A point was chosen (and not yet reserved; call [`set_current_destination_point`]).
    Picked(EntityId),
}

/// Use types whose point gets reserved for the NPC the moment its routine pass starts (the 19
/// "single user" work and relax points in the DLL's switch).
const RESERVE_ON_PASS: &[UseType] = &[
    UseType::Anvil,
    UseType::Cauldron,
    UseType::Barbecue,
    UseType::Alchemy,
    UseType::Bookstand,
    UseType::PickOre,
    UseType::SawLog,
    UseType::Bed,
    UseType::Throne,
    UseType::Stool,
    UseType::Bench,
    UseType::Waterpipe,
    UseType::WaterBarrel,
    UseType::OrcDance,
    UseType::OrcDrum,
    UseType::OrcBoulder,
    UseType::Chest,
    UseType::Shrine,
    UseType::GrindStone,
];

/// Port of `SelectRoutineDestination` (S:a7050): pick the point the NPC should walk to for the
/// current routine time.
///
/// Skips NPCs in a running arena fight, members of the player's party and NPCs that are still
/// travelling. A destination that is still unclaimed gets reserved when it is one of
/// [`RESERVE_ON_PASS`]. Otherwise the anchor of the current routine slot is resolved
/// (`GetRoutineTime`): a **Local** anchor offers its first free interact point, a **Roam**
/// anchor a random free roaming point of the same name, a **Patrol** anchor the next patrol
/// point. Capacity is `MaxUsers - UserCount >= party size` (party members of the NPC itself
/// count as users; the NPC leading the player counts as none).
pub fn select_routine_destination(host: &mut dyn ScriptHost, npc: EntityId) -> Destination {
    gothic_portcov::hit!("select_routine_destination");
    use crate::enums::AnchorType;
    if host.arena_active(npc) {
        return Destination::Skipped;
    }
    let player = host.player();
    if host.party_leader(npc) == player {
        return Destination::Skipped;
    }
    let current = host.destination_point(npc);
    if current.is_some() && host.point_user(current).is_none() {
        let use_type = host.point_use_type(current);
        if RESERVE_ON_PASS.contains(&use_type) {
            host.set_point_user(current, npc);
            let anchor = host.point_anchor(current);
            if anchor.is_some() {
                let n = host.anchor_user_count(anchor);
                host.set_anchor_user_count(anchor, n + 1);
            }
        }
    }
    if host.is_travelling(npc) {
        return Destination::Skipped;
    }
    let time = routine_time(host.species(npc), host.world_hour());
    let anchor = host.routine_point(npc, time);
    if anchor.is_none() {
        return Destination::Skipped;
    }
    // Slots needed: the NPC plus its followers.
    let slots = host.party_members(npc).len().max(1) as i64;
    let mut max = host.anchor_max_users(anchor);
    if max == 0 {
        max = host.anchor_points(anchor).len() as u32;
        host.set_anchor_max_users(anchor, max);
    }
    let free = |host: &dyn ScriptHost, a: EntityId| {
        i64::from(host.anchor_max_users(a)) - i64::from(host.anchor_user_count(a)) >= slots
    };
    match host.anchor_type(anchor) {
        AnchorType::Roam => {
            let name = host.name(anchor);
            let candidates: Vec<EntityId> = host
                .roaming_points_by_name(&name)
                .into_iter()
                .filter(|&p| {
                    let a = host.point_anchor(p);
                    let target = if a.is_some() { a } else { p };
                    p == current || free(host, target)
                })
                .collect();
            if candidates.is_empty() {
                return Destination::NotFound;
            }
            let pick = host.random(candidates.len() as i32) as usize;
            Destination::Picked(candidates[pick.min(candidates.len() - 1)])
        }
        AnchorType::Patrol => {
            let next = host.next_patrol_point(current);
            if next.is_some() {
                Destination::Picked(next)
            } else {
                Destination::NotFound
            }
        }
        AnchorType::Local | AnchorType::Event => {
            if !free(host, anchor) {
                return Destination::NotFound;
            }
            host.anchor_points(anchor)
                .into_iter()
                .find(|&p| is_free_destination(host, npc, p))
                .map_or(Destination::NotFound, Destination::Picked)
        }
    }
}

/// What [`continue_routine`] decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resume {
    /// Dead: `ZS_Dead`.
    Dead,
    /// Was down: back on its feet via `ZS_StandUp`.
    StandUp,
    /// Enclave uprising: `ZS_RevolutionAttack`.
    RevolutionAttack,
    /// Enclave flight: `ZS_Flee`.
    Flee,
    /// Low on hit points: `ZS_HealSelf`.
    HealSelf,
    /// Back to the daily routine; the task was re-selected.
    Routine(State),
}

/// Hit-point percentage at or below which an NPC tries to heal (`< 61`).
pub const HEAL_SELF_BELOW_PERCENT: i32 = 61;

/// Port of `CanHealSelf` (S:a66b0): the NPC leading the player can always heal; Mages and
/// Paladins heal by spell; everyone else needs an `It_Potion_Health`.
pub fn can_heal_self(host: &dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("can_heal_self");
    if host.party_leader(host.player()) == npc {
        return true;
    }
    match host.npc_class(npc) {
        NpcClass::Mage | NpcClass::Paladin => true,
        _ => host.has_items(npc, "It_Potion_Health", 1),
    }
}

/// Port of `ContinueRoutine` (S:a5ea0): resume the NPC's daily routine after any interruption.
///
/// Order: dead NPCs go to `ZS_Dead`; a downed NPC gets `AIMode::Routine` back and `ZS_StandUp`;
/// then enclave states (enclave **Attack** with a Friendly or Hostile revolution attitude gives
/// `ZS_RevolutionAttack`, **Flee** with Friendly gives `ZS_Flee`); then hit points below 61
/// percent with [`can_heal_self`] give `ZS_HealSelf`; otherwise the task is re-selected. The
/// target is cleared on every branch that changes task. The off-screen travel part of the DLL
/// (`StartGoto` for NPCs outside processing range) is host work and not modelled [I].
pub fn continue_routine(host: &mut dyn ScriptHost, npc: EntityId) -> Resume {
    gothic_portcov::hit!("continue_routine");
    match host.ai_mode(npc) {
        AiMode::Dead => {
            host.set_current_target(npc, EntityId::NONE);
            host.set_task(npc, State::Dead);
            return Resume::Dead;
        }
        AiMode::Down => {
            host.set_ai_mode(npc, AiMode::Routine);
            host.set_current_target(npc, EntityId::NONE);
            host.set_task(npc, State::StandUp);
            return Resume::StandUp;
        }
        _ => {}
    }
    let enclave = host.enclave(npc);
    if enclave.is_some() {
        use crate::enums::Attitude;
        let attitude = host.revolution_attitude(npc, host.player());
        match host.enclave_status(enclave) {
            EnclaveStatus::Attack if matches!(attitude, Attitude::Friendly | Attitude::Hostile) => {
                host.set_current_target(npc, EntityId::NONE);
                host.set_task(npc, State::RevolutionAttack);
                return Resume::RevolutionAttack;
            }
            EnclaveStatus::Flee if attitude == Attitude::Friendly => {
                host.set_current_target(npc, EntityId::NONE);
                host.set_task(npc, State::Flee);
                return Resume::Flee;
            }
            _ => {}
        }
    }
    if stats::pool_percent(host, npc, Pool::Hit) < HEAL_SELF_BELOW_PERCENT
        && can_heal_self(host, npc)
    {
        host.set_current_target(npc, EntityId::NONE);
        host.set_task(npc, State::HealSelf);
        return Resume::HealSelf;
    }
    Resume::Routine(select_routine_task(host, npc))
}

/// Port of `HaveABreak` (S:8c390): used at the end of work states. An NPC whose current point
/// belongs to an anchor goes straight back to `ContinueRoutine`; an NPC working a free-standing
/// point takes a break (`ZS_Break`).
pub fn have_a_break(host: &mut dyn ScriptHost, npc: EntityId) -> Resume {
    gothic_portcov::hit!("have_a_break");
    let point = host.destination_point(npc);
    if point.is_some() && host.point_anchor(point).is_some() {
        continue_routine(host, npc)
    } else {
        host.set_task(npc, State::Break);
        Resume::Routine(State::Break)
    }
}

/// Port of `GetAmbientAction` (S:884c0): the idle action of a grazing animal.
///
/// Scavenger, Bison, Rhino and Pig eat the ground with 80 percent, Boar, Deer and Cow with 50
/// percent; every other species and the remaining rolls give the plain `Ambient` action.
pub fn ambient_action(host: &mut dyn ScriptHost, npc: EntityId) -> Action {
    gothic_portcov::hit!("ambient_action");
    let chance = match host.species(npc) {
        Species::Scavenger | Species::Bison | Species::Rhino | Species::Pig => 80,
        Species::Boar | Species::Deer | Species::Cow => 50,
        _ => return Action::Ambient,
    };
    if host.random(100) < chance {
        Action::EatGround
    } else {
        Action::Ambient
    }
}

/// Port of `SendToArena` (S:a7f80): stop what the NPC does and, if it is within processing
/// range, restart its routine (the arena task takes over through `SelectRoutineTask`).
///
/// The DLL refuses (returns false) when `AIMode == 6` (`Talk`); every other NPC gets a
/// `FullStop` and, in range, `ContinueRoutine`.
pub fn send_to_arena(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("send_to_arena");
    if host.ai_mode(npc) == AiMode::Talk {
        return false;
    }
    host.full_stop(npc);
    if host.in_processing_range(npc) {
        continue_routine(host, npc);
    }
    true
}

/// Port of `Rtn_Monster` (S:86a10): entry for monsters: just resume the routine.
pub fn rtn_monster(host: &mut dyn ScriptHost, npc: EntityId) -> Resume {
    gothic_portcov::hit!("rtn_monster");
    continue_routine(host, npc)
}

/// Port of `Rtn_Dead` (S:86a80): entry for dead NPCs: AI mode Dead and `ZS_Dead`.
pub fn rtn_dead(host: &mut dyn ScriptHost, npc: EntityId) {
    gothic_portcov::hit!("rtn_dead");
    host.set_ai_mode(npc, AiMode::Dead);
    host.set_task(npc, State::Dead);
}

/// Port of `Rtn_Player` (S:86ba0): entry for the player entity. Resets AI mode and `Ransacked`,
/// then picks `PS_Ghost` (status bit 0x80), else by weapon mode `PS_Melee`, `PS_Ranged`,
/// `PS_Magic` or `PS_Normal`. `weapon_mode` is the remembered `LastWeaponConfig` (0..=3).
pub fn rtn_player(host: &mut dyn ScriptHost, npc: EntityId, weapon_mode: u32) {
    gothic_portcov::hit!("rtn_player");
    host.set_ai_mode(npc, AiMode::Routine);
    host.set_ransacked(npc, false);
    let task = if host.has_status(npc, 0x80) {
        State::PsGhost
    } else {
        match weapon_mode {
            1 => State::PsMelee,
            2 => State::PsRanged,
            3 => State::PsMagic,
            _ => State::PsNormal,
        }
    };
    host.set_task(npc, task);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{AnchorType, EnclaveStatus};
    #[allow(unused_imports)]
    use crate::host::{EntityApi, InventoryApi, NpcApi, ScriptCtx, StatsApi};
    use crate::testhost::TestWorld;

    #[test]
    fn use_type_table_matches_the_rule_doc() {
        use State::*;
        let t = task_for_use_type;
        assert_eq!(t(Species::Human, UseType::Anvil), Smith);
        assert_eq!(t(Species::Human, UseType::Forge), Smith);
        assert_eq!(t(Species::Orc, UseType::Anvil), Smith);
        assert_eq!(t(Species::Human, UseType::Field), Farmer);
        assert_eq!(t(Species::Orc, UseType::Field), HangAround);
        assert_eq!(t(Species::Orc, UseType::OrcDrum), OrcParty);
        assert_eq!(t(Species::Human, UseType::OrcDrum), HangAround);
        assert_eq!(t(Species::Human, UseType::Bed), Sleep);
        assert_eq!(t(Species::Orc, UseType::SleepGround), Sleep);
        assert_eq!(t(Species::Human, UseType::Stool), HangAround);
        assert_eq!(t(Species::Human, UseType::None), HangAround);
        assert_eq!(t(Species::Human, UseType::Guard), Guard);
        assert_eq!(t(Species::Human, UseType::Shrine), Pray);
        assert_eq!(t(Species::Human, UseType::Lumberjack), Lumberjack);
        assert_eq!(t(Species::Wolf, UseType::SleepGround), AnimalSleep);
        assert_eq!(t(Species::Wolf, UseType::Roam), Monster);
        assert_eq!(t(Species::Wolf, UseType::EatGround), Monster);
        assert_eq!(t(Species::Meatbug, UseType::Anvil), Ambient);
        assert_eq!(t(Species::Goblin, UseType::None), Ambient);
    }

    #[test]
    fn orc_attack_point_discrepancy() {
        // The DLL's Orc switch has no AttackPoint row: orcs fall back to HangAround, humans run.
        assert_eq!(
            task_for_use_type(Species::Human, UseType::AttackPoint),
            State::RunToAttackPoint
        );
        assert_eq!(
            task_for_use_type(Species::Orc, UseType::AttackPoint),
            State::HangAround
        );
    }

    #[test]
    fn every_human_row_has_a_state_def() {
        for r in HUMAN_TASKS {
            assert!(
                crate::routine::work::definition(r.task).is_some(),
                "{:?} has no definition",
                r.task
            );
        }
    }

    #[test]
    fn task_priority() {
        let mut w = TestWorld::new();
        let p = w.player();
        let npc = w.spawn_npc("Smith", Species::Human, 5, 5);
        let point = w.add_point(UseType::Forge, EntityId::NONE);
        w.set_destination(npc, point);
        assert_eq!(select_routine_task(&mut w, npc), State::Smith);
        assert_eq!(w.current_task(npc), Some(State::Smith));
        // Led by the player.
        w.set_leader(npc, p, PartyMemberType::Party, false);
        assert_eq!(select_routine_task(&mut w, npc), State::FollowPlayer);
        w.set_leader(npc, p, PartyMemberType::Party, true);
        assert_eq!(select_routine_task(&mut w, npc), State::WaitForPlayer);
        // Arena wins over everything.
        w.arena = true;
        assert_eq!(select_routine_task(&mut w, npc), State::Arena);
        w.arena = false;
        // Guiding the player.
        let guide = w.spawn_npc("Guide", Species::Human, 5, 5);
        w.set_leader(guide, EntityId::NONE, PartyMemberType::None, false);
        w.set_leader(p, guide, PartyMemberType::None, false);
        assert_eq!(select_routine_task(&mut w, guide), State::GuidePlayer);
        // Controlled follower of another NPC.
        let slave = w.spawn_npc("Thrall", Species::Human, 5, 5);
        w.set_leader(slave, npc, PartyMemberType::Controlled, false);
        assert_eq!(select_routine_task(&mut w, slave), State::FollowNpc);
        // No destination at all.
        let idle = w.spawn_npc("Idle", Species::Human, 5, 5);
        assert_eq!(select_routine_task(&mut w, idle), State::HangAround);
    }

    #[test]
    fn selecting_a_task_clears_the_target() {
        let mut w = TestWorld::new();
        let a = w.spawn_npc("A", Species::Human, 5, 5);
        let b = w.spawn_npc("B", Species::Human, 5, 5);
        w.set_current_target(a, b);
        select_routine_task(&mut w, a);
        assert!(w.current_target(a).is_none());
    }

    #[test]
    fn free_destination_capacity() {
        let mut w = TestWorld::new();
        let a = w.spawn_npc("A", Species::Human, 5, 5);
        let b = w.spawn_npc("B", Species::Human, 5, 5);
        let anchor = w.add_anchor("Mine", AnchorType::Local, 1);
        let p1 = w.add_point(UseType::PickOre, anchor);
        let p2 = w.add_point(UseType::PickOre, anchor);
        assert!(is_free_destination(&w, a, p1));
        assert!(!is_free_destination(&w, a, EntityId::NONE));
        // One user fills the one-slot anchor.
        assert!(set_current_destination_point(&mut w, b, p1));
        assert_eq!(w.anchor_user_count(anchor), 1);
        assert!(!is_free_destination(&w, a, p2), "anchor is full");
        assert!(!is_free_destination(&w, a, p1));
        // b may still re-use its own slot.
        assert!(is_free_destination(&w, b, p1));
        // Moving b away frees the anchor again.
        assert!(set_current_destination_point(&mut w, b, EntityId::NONE));
        assert_eq!(w.anchor_user_count(anchor), 0);
        assert!(is_free_destination(&w, a, p2));
        // Setting the same (empty) destination again is a no-op.
        assert!(!set_current_destination_point(&mut w, b, EntityId::NONE));
    }

    #[test]
    fn owned_points_are_private() {
        let mut w = TestWorld::new();
        let a = w.spawn_npc("A", Species::Human, 5, 5);
        let b = w.spawn_npc("B", Species::Human, 5, 5);
        let bed = w.add_point(UseType::Bed, EntityId::NONE);
        w.set_point_owner(bed, a);
        assert!(is_free_destination(&w, a, bed));
        assert!(!is_free_destination(&w, b, bed));
    }

    #[test]
    fn destination_by_anchor_type() {
        let mut w = TestWorld::new();
        w.hour = 10.0;
        let npc = w.spawn_npc("Miner", Species::Human, 5, 5);
        let anchor = w.add_anchor("MINE", AnchorType::Local, 0);
        let p1 = w.add_point(UseType::PickOre, anchor);
        let p2 = w.add_point(UseType::PickOre, anchor);
        w.set_routine_anchor(npc, crate::routine::time::RoutineTime::Work, anchor);
        assert_eq!(
            select_routine_destination(&mut w, npc),
            Destination::Picked(p1)
        );
        // First point taken by someone else: the second is offered.
        let other = w.spawn_npc("Other", Species::Human, 5, 5);
        set_current_destination_point(&mut w, other, p1);
        assert_eq!(
            select_routine_destination(&mut w, npc),
            Destination::Picked(p2)
        );
        // Anchor full: nothing found.
        set_current_destination_point(&mut w, npc, p2);
        w.set_on_destination(npc, true);
        let third = w.spawn_npc("Third", Species::Human, 5, 5);
        assert_eq!(
            select_routine_destination(&mut w, third),
            Destination::Skipped,
            "no anchor assigned for the third NPC"
        );
        w.set_routine_anchor(third, crate::routine::time::RoutineTime::Work, anchor);
        assert_eq!(
            select_routine_destination(&mut w, third),
            Destination::NotFound
        );
    }

    #[test]
    fn roam_anchor_picks_a_free_roaming_point() {
        let mut w = TestWorld::new();
        w.hour = 12.0;
        let wolf = w.spawn_npc("Wolf", Species::Wolf, 5, 5);
        let anchor = w.add_anchor("WOLFDEN", AnchorType::Roam, 4);
        let a = w.add_point(UseType::Roam, anchor);
        let b = w.add_point(UseType::Roam, anchor);
        w.set_roaming_points("WOLFDEN", &[a, b]);
        w.set_routine_anchor(wolf, crate::routine::time::RoutineTime::Work, anchor);
        let got = select_routine_destination(&mut w, wolf);
        assert!(matches!(got, Destination::Picked(p) if p == a || p == b));
    }

    #[test]
    fn party_members_and_arena_skip_destination() {
        let mut w = TestWorld::new();
        let p = w.player();
        let npc = w.spawn_npc("Buddy", Species::Human, 5, 5);
        let anchor = w.add_anchor("A", AnchorType::Local, 0);
        w.add_point(UseType::Stool, anchor);
        w.set_routine_anchor(npc, crate::routine::time::RoutineTime::Work, anchor);
        w.set_leader(npc, p, PartyMemberType::Party, false);
        assert_eq!(
            select_routine_destination(&mut w, npc),
            Destination::Skipped
        );
        w.set_leader(npc, EntityId::NONE, PartyMemberType::None, false);
        w.arena = true;
        assert_eq!(
            select_routine_destination(&mut w, npc),
            Destination::Skipped
        );
    }

    #[test]
    fn continue_routine_order() {
        let mut w = TestWorld::new();
        let npc = w.spawn_npc("Guy", Species::Human, 10, 10);
        // Dead beats everything.
        w.set_ai_mode(npc, AiMode::Dead);
        assert_eq!(continue_routine(&mut w, npc), Resume::Dead);
        assert_eq!(w.current_task(npc), Some(State::Dead));
        // Down: gets up and returns to Routine mode.
        w.set_ai_mode(npc, AiMode::Down);
        assert_eq!(continue_routine(&mut w, npc), Resume::StandUp);
        assert_eq!(w.ai_mode(npc), AiMode::Routine);
        // Healthy: back to the routine.
        w.set_ai_mode(npc, AiMode::Routine);
        let max = w.dr_pool_max(npc, Pool::Hit);
        w.dr_set_pool(npc, Pool::Hit, max);
        assert!(matches!(continue_routine(&mut w, npc), Resume::Routine(_)));
        // Hurt below 61 percent and holding a potion: heal.
        w.dr_set_pool(npc, Pool::Hit, max * 60 / 100);
        assert!(
            matches!(continue_routine(&mut w, npc), Resume::Routine(_)),
            "no potion"
        );
        w.give_item(npc, "It_Potion_Health");
        assert_eq!(continue_routine(&mut w, npc), Resume::HealSelf);
        w.dr_set_pool(npc, Pool::Hit, max * 61 / 100);
        assert!(
            matches!(continue_routine(&mut w, npc), Resume::Routine(_)),
            "61 percent is fine"
        );
        // Enclave states.
        let enc = w.add_enclave(crate::enums::Alignment::Reb);
        w.set_enclave(npc, enc);
        w.set_enclave_status(enc, EnclaveStatus::Attack);
        w.set_revolution_attitude(npc, crate::enums::Attitude::Hostile);
        assert_eq!(continue_routine(&mut w, npc), Resume::RevolutionAttack);
        w.set_revolution_attitude(npc, crate::enums::Attitude::Neutral);
        assert!(matches!(continue_routine(&mut w, npc), Resume::Routine(_)));
        w.set_enclave_status(enc, EnclaveStatus::Flee);
        w.set_revolution_attitude(npc, crate::enums::Attitude::Friendly);
        assert_eq!(continue_routine(&mut w, npc), Resume::Flee);
    }

    #[test]
    fn mages_heal_without_potions() {
        let mut w = TestWorld::new();
        let npc = w.spawn_npc("Mage", Species::Human, 10, 10);
        w.set_class(npc, NpcClass::Mage);
        assert!(can_heal_self(&w, npc));
        w.set_class(npc, NpcClass::Warrior);
        assert!(!can_heal_self(&w, npc));
        // The NPC that guides the player can always heal.
        let p = w.player();
        w.set_leader(p, npc, PartyMemberType::None, false);
        assert!(can_heal_self(&w, npc));
    }

    #[test]
    fn break_depends_on_anchor() {
        let mut w = TestWorld::new();
        let npc = w.spawn_npc("Guy", Species::Human, 10, 10);
        let anchor = w.add_anchor("A", AnchorType::Local, 2);
        let with_anchor = w.add_point(UseType::Anvil, anchor);
        let loose = w.add_point(UseType::Anvil, EntityId::NONE);
        w.set_destination(npc, loose);
        assert_eq!(have_a_break(&mut w, npc), Resume::Routine(State::Break));
        assert_eq!(w.current_task(npc), Some(State::Break));
        w.set_destination(npc, with_anchor);
        assert!(matches!(have_a_break(&mut w, npc), Resume::Routine(_)));
        assert_ne!(w.current_task(npc), Some(State::Break));
    }

    #[test]
    fn ambient_actions() {
        let mut w = TestWorld::new();
        let pig = w.spawn_npc("Pig", Species::Pig, 1, 1);
        let wolf = w.spawn_npc("Wolf", Species::Wolf, 1, 1);
        let boar = w.spawn_npc("Boar", Species::Boar, 1, 1);
        w.rolls = vec![79, 80, 49, 50];
        assert_eq!(ambient_action(&mut w, pig), Action::EatGround);
        assert_eq!(ambient_action(&mut w, pig), Action::Ambient);
        assert_eq!(ambient_action(&mut w, boar), Action::EatGround);
        assert_eq!(ambient_action(&mut w, boar), Action::Ambient);
        assert_eq!(ambient_action(&mut w, wolf), Action::Ambient);
    }

    #[test]
    fn player_and_dead_entries() {
        let mut w = TestWorld::new();
        let p = w.player();
        rtn_player(&mut w, p, 0);
        assert_eq!(w.current_task(p), Some(State::PsNormal));
        rtn_player(&mut w, p, 2);
        assert_eq!(w.current_task(p), Some(State::PsRanged));
        w.set_status(p, 0x80);
        rtn_player(&mut w, p, 1);
        assert_eq!(w.current_task(p), Some(State::PsGhost));
        let npc = w.spawn_npc("Corpse", Species::Human, 1, 1);
        rtn_dead(&mut w, npc);
        assert_eq!(w.ai_mode(npc), AiMode::Dead);
        assert_eq!(w.current_task(npc), Some(State::Dead));
    }

    #[test]
    fn monster_entry_resumes_the_routine() {
        let mut w = TestWorld::new();
        let wolf = w.spawn_npc("Wolf", Species::Wolf, 5, 5);
        assert!(matches!(
            rtn_monster(&mut w, wolf),
            Resume::Routine(State::Ambient)
        ));
    }

    #[test]
    fn arena_send_refuses_talkers() {
        let mut w = TestWorld::new();
        let npc = w.spawn_npc("Fighter", Species::Human, 5, 5);
        w.set_ai_mode(npc, AiMode::Talk);
        assert!(!send_to_arena(&mut w, npc));
        w.set_ai_mode(npc, AiMode::Routine);
        assert!(send_to_arena(&mut w, npc));
        assert!(w.full_stops > 0);
    }
}
