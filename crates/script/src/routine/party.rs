//! B03: player party join/leave/wait and guide start/stop (`PSParty` bookkeeping).
//!
//! Member types: Party 1, Mob 2, Slave 3 (a "Controlled" NPC follows another NPC, a "Summoned"
//! creature is bound to the player's magic). Joining sets the leader to the player, clears
//! `Waiting`, stores the member type and starts `ZS_FollowPlayer`; leaving clears the leader and
//! hands the NPC back to its routine.

use crate::enums::{AiMode, PartyMemberType};
use crate::host::{EntityId, ScriptHost};
use crate::routine::select::{continue_routine, set_current_destination_point};
use crate::routine::state::State;

/// Game-message kind of join messages.
const MSG_JOIN: u32 = 8;
/// Game-message kind of dismiss/wait messages.
const MSG_LEAVE: u32 = 7;

/// How a join call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Joined {
    /// A summoned creature simply follows the player (no message, type unchanged).
    Summoned,
    /// A regular member of the given kind was added.
    Member(PartyMemberType),
}

fn join(host: &mut dyn ScriptHost, npc: EntityId, message: &str, kind: PartyMemberType) -> Joined {
    let player = host.player();
    if host.party_member_type(npc) == PartyMemberType::Summoned {
        // Helper S:a82a0.
        host.set_party_leader(npc, player);
        host.set_party_waiting(npc, false);
        host.full_stop(npc);
        host.set_task(npc, State::FollowPlayer);
        return Joined::Summoned;
    }
    let name = host.name(npc);
    host.print_game_message(message, &name, MSG_JOIN);
    // Free the reserved work point (SetCurrentDestinationPoint to None).
    set_current_destination_point(host, npc, EntityId::NONE);
    host.set_party_leader(npc, player);
    host.set_party_waiting(npc, false);
    host.set_party_member_type(npc, kind);
    host.full_stop(npc);
    host.set_task(npc, State::FollowPlayer);
    Joined::Member(kind)
}

/// Port of `JoinPlayerParty` (S:a8480): message `GO_PartyJoin`, member type Party.
pub fn join_player_party(host: &mut dyn ScriptHost, npc: EntityId) -> Joined {
    gothic_portcov::hit!("join_player_party");
    join(host, npc, "GO_PartyJoin", PartyMemberType::Party)
}

/// Port of `JoinPlayerMob` (S:a8900): message `GO_MobJoin`, member type Mob.
pub fn join_player_mob(host: &mut dyn ScriptHost, npc: EntityId) -> Joined {
    gothic_portcov::hit!("join_player_mob");
    join(host, npc, "GO_MobJoin", PartyMemberType::Mob)
}

/// Port of `JoinPlayerSlavery` (S:a8d30): message `GO_SlaveJoin`, member type Slave.
pub fn join_player_slavery(host: &mut dyn ScriptHost, npc: EntityId) -> Joined {
    gothic_portcov::hit!("join_player_slavery");
    join(host, npc, "GO_SlaveJoin", PartyMemberType::Slave)
}

fn dismiss(host: &mut dyn ScriptHost, npc: EntityId, message: &str, resume_controlled: bool) {
    if host.party_member_type(npc) == PartyMemberType::Summoned {
        // Helper S:a83e0: a dismissed summon dies.
        host.full_stop(npc);
        host.set_task(npc, State::RagDollDead);
        if !host.in_processing_range(npc) {
            host.set_ai_mode(npc, AiMode::Dead);
            host.set_party_leader(npc, EntityId::NONE);
            host.set_party_waiting(npc, false);
        }
        return;
    }
    let name = host.name(npc);
    host.print_game_message(message, &name, MSG_LEAVE);
    host.set_party_leader(npc, EntityId::NONE);
    let now = host.playing_time().round() as u32;
    host.set_time_stamp_transformed(npc, now);
    host.set_party_waiting(npc, false);
    if resume_controlled && host.party_member_type(npc) == PartyMemberType::Controlled {
        host.full_stop(npc);
        continue_routine(host, npc);
    }
    host.set_party_member_type(npc, PartyMemberType::None);
}

/// Port of `DismissPlayerParty` (S:a8690): message `GO_PartyDismiss`; a Controlled member also
/// restarts its routine. A summoned creature is killed instead.
pub fn dismiss_player_party(host: &mut dyn ScriptHost, npc: EntityId) {
    gothic_portcov::hit!("dismiss_player_party");
    dismiss(host, npc, "GO_PartyDismiss", true);
}

/// Port of `DismissPlayerMob` (S:a8b10): message `GO_MobDismiss`.
pub fn dismiss_player_mob(host: &mut dyn ScriptHost, npc: EntityId) {
    gothic_portcov::hit!("dismiss_player_mob");
    dismiss(host, npc, "GO_MobDismiss", false);
}

/// Port of `WaitPlayerParty` (S:a8f40): the member waits where it stands (`GO_SlaveDismiss`
/// message, `Waiting` set, `ZS_WaitForPlayer`). Summoned creatures ignore the call.
pub fn wait_player_party(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("wait_player_party");
    if host.party_member_type(npc) == PartyMemberType::Summoned {
        return true;
    }
    let name = host.name(npc);
    host.print_game_message("GO_SlaveDismiss", &name, MSG_LEAVE);
    host.set_party_waiting(npc, true);
    host.full_stop(npc);
    host.set_task(npc, State::WaitForPlayer);
    true
}

/// Port of `CanJoinPlayerParty` (S:a80a0): summoned creatures always may; otherwise a waiting or
/// out-of-range companion is dismissed to make room and an active, in-range one blocks, so at
/// most one active companion exists.
pub fn can_join_player_party(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("can_join_player_party");
    if host.party_member_type(npc) == PartyMemberType::Summoned {
        return true;
    }
    let player = host.player();
    for member in host.party_members(player) {
        if host.party_member_type(member) != PartyMemberType::Party {
            continue;
        }
        if !host.party_waiting(member) && host.in_processing_range(member) {
            return false;
        }
        dismiss_player_party(host, member);
    }
    true
}

/// Port of `StartGuidePlayer` (S:a9110): the NPC becomes the player's guide (the player's
/// `PSParty` leader is set to the NPC).
pub fn start_guide_player(host: &mut dyn ScriptHost, npc: EntityId) {
    gothic_portcov::hit!("start_guide_player");
    let player = host.player();
    host.set_party_leader(player, npc);
}

/// Port of `StopGuidePlayer` (S:a9200): only the current guide can stop guiding; the guide link
/// is cleared and the stop time recorded. Returns false when `npc` is not the guide.
pub fn stop_guide_player(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("stop_guide_player");
    let player = host.player();
    if host.party_leader(player) != npc {
        return false;
    }
    host.set_party_leader(player, EntityId::NONE);
    let now = host.playing_time().round() as u32;
    host.set_time_stamp_transformed(npc, now);
    true
}

/// Port of `StopBringNPC` (S:a9370): clears the dialog's `PartyEnabled` flag so the NPC can no
/// longer be asked to join.
pub fn stop_bring_npc(host: &mut dyn ScriptHost, npc: EntityId) {
    gothic_portcov::hit!("stop_bring_npc");
    host.set_dialog_party_enabled(npc, false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{AnchorType, Species, UseType};
    #[allow(unused_imports)]
    use crate::host::{EntityApi, InventoryApi, NpcApi, ScriptCtx, StatsApi};
    use crate::testhost::TestWorld;

    #[test]
    fn join_then_dismiss_party() {
        let mut w = TestWorld::new();
        let p = w.player();
        let npc = w.spawn_npc("Diego", Species::Human, 10, 10);
        // Reserve a work point so the join frees it.
        let anchor = w.add_anchor("A", AnchorType::Local, 2);
        let point = w.add_point(UseType::Anvil, anchor);
        set_current_destination_point(&mut w, npc, point);
        assert_eq!(w.anchor_user_count(anchor), 1);

        assert_eq!(
            join_player_party(&mut w, npc),
            Joined::Member(PartyMemberType::Party)
        );
        assert_eq!(w.party_leader(npc), p);
        assert!(!w.party_waiting(npc));
        assert_eq!(w.party_member_type(npc), PartyMemberType::Party);
        assert_eq!(w.current_task(npc), Some(State::FollowPlayer));
        assert_eq!(w.anchor_user_count(anchor), 0, "work point released");
        assert!(w.destination_point(npc).is_none());
        assert_eq!(w.messages.last().unwrap().0, "GO_PartyJoin");
        assert_eq!(w.messages.last().unwrap().1, "Diego");

        wait_player_party(&mut w, npc);
        assert!(w.party_waiting(npc));
        assert_eq!(w.current_task(npc), Some(State::WaitForPlayer));

        dismiss_player_party(&mut w, npc);
        assert!(w.party_leader(npc).is_none());
        assert_eq!(w.party_member_type(npc), PartyMemberType::None);
        assert_eq!(w.messages.last().unwrap().0, "GO_PartyDismiss");
    }

    #[test]
    fn mob_and_slavery_kinds() {
        let mut w = TestWorld::new();
        let a = w.spawn_npc("A", Species::Human, 5, 5);
        let b = w.spawn_npc("B", Species::Human, 5, 5);
        assert_eq!(
            join_player_mob(&mut w, a),
            Joined::Member(PartyMemberType::Mob)
        );
        assert_eq!(w.messages.last().unwrap().0, "GO_MobJoin");
        assert_eq!(
            join_player_slavery(&mut w, b),
            Joined::Member(PartyMemberType::Slave)
        );
        assert_eq!(w.messages.last().unwrap().0, "GO_SlaveJoin");
        dismiss_player_mob(&mut w, a);
        assert_eq!(w.messages.last().unwrap().0, "GO_MobDismiss");
    }

    #[test]
    fn summoned_follow_and_die() {
        let mut w = TestWorld::new();
        let p = w.player();
        let sum = w.spawn_npc("Skeleton", Species::Skeleton, 5, 5);
        w.set_leader(sum, EntityId::NONE, PartyMemberType::Summoned, true);
        assert_eq!(join_player_party(&mut w, sum), Joined::Summoned);
        assert_eq!(w.party_leader(sum), p);
        assert!(!w.party_waiting(sum));
        assert_eq!(w.party_member_type(sum), PartyMemberType::Summoned);
        assert!(w.messages.is_empty(), "no join message for summons");
        w.in_range = false;
        dismiss_player_party(&mut w, sum);
        assert_eq!(w.current_task(sum), Some(State::RagDollDead));
        assert_eq!(w.ai_mode(sum), AiMode::Dead);
        assert!(w.party_leader(sum).is_none());
    }

    #[test]
    fn only_one_active_companion() {
        let mut w = TestWorld::new();
        let a = w.spawn_npc("A", Species::Human, 5, 5);
        let b = w.spawn_npc("B", Species::Human, 5, 5);
        join_player_party(&mut w, a);
        assert!(!can_join_player_party(&mut w, b), "active companion blocks");
        wait_player_party(&mut w, a);
        assert!(
            can_join_player_party(&mut w, b),
            "a waiting one is dismissed"
        );
        assert!(w.party_leader(a).is_none());
        // An out-of-range companion is dismissed too.
        join_player_party(&mut w, a);
        w.in_range = false;
        assert!(can_join_player_party(&mut w, b));
        assert!(w.party_leader(a).is_none());
        // Summons never count.
        let s = w.spawn_npc("S", Species::Skeleton, 5, 5);
        w.set_leader(s, EntityId::NONE, PartyMemberType::Summoned, false);
        join_player_party(&mut w, a);
        w.in_range = true;
        assert!(can_join_player_party(&mut w, s));
    }

    #[test]
    fn guide_start_stop() {
        let mut w = TestWorld::new();
        let p = w.player();
        let guide = w.spawn_npc("Guide", Species::Human, 5, 5);
        let other = w.spawn_npc("Other", Species::Human, 5, 5);
        start_guide_player(&mut w, guide);
        assert_eq!(w.party_leader(p), guide);
        assert!(!stop_guide_player(&mut w, other));
        assert_eq!(w.party_leader(p), guide);
        assert!(stop_guide_player(&mut w, guide));
        assert!(w.party_leader(p).is_none());
        stop_bring_npc(&mut w, guide);
        assert!(!w.dialog_party_enabled(guide));
    }
}
