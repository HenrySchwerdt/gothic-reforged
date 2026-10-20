//! B12: the companion states: following the player or another NPC, waiting for the player and
//! guiding the player to a place.
//!
//! All four tasks run in AI mode `PlayerParty` (except `ZS_FollowNPC`, which uses `Routine`) and
//! poll once per second of state time; the callbacks (`OnFollowPlayer` and friends) run every
//! tick and decide whether the companion has arrived, must wait or must be dismissed.

use crate::enums::{AiMode, PartyMemberType};
use crate::host::{AiCommand, EntityId, ScriptHost};
use crate::routine::select::continue_routine;
use crate::routine::state::{Callback, State};
use crate::routine::work::{End, Out, Step};
use Step::*;

/// Seconds of state time between two follow checks.
pub const FOLLOW_POLL_SECS: f32 = 1.0;
/// Distance each rank adds to the follow distance (cm).
pub const FOLLOW_STEP: f32 = 150.0;
/// Follow distance of a companion without a rank (cm).
pub const DEFAULT_FOLLOW_DISTANCE: f32 = 500.0;
/// A guide waits when the player is farther than this (cm).
pub const GUIDE_WAIT_DISTANCE: f32 = 1000.0;
/// A waiting guide resumes when the player is closer than this (cm).
pub const GUIDE_RESUME_DISTANCE: f32 = 800.0;
/// A mob member farther than this from the "Fireclan" enclave is dismissed (cm).
pub const MOB_LEASH: f32 = 62000.0;

/// Port of `FUN_10012d40` (S:12d40): the follow distance of `npc` in the player's party.
///
/// Companions that are waiting or already in `ZS_WaitForPlayer` do not count; the remaining
/// members are ranked in party order and the distance is `(rank + 3) * 150`. An NPC that is not
/// ranked (not in the party) keeps 500.
pub fn follow_distance(host: &dyn ScriptHost, npc: EntityId) -> f32 {
    let player = host.player();
    let rank = host
        .party_members(player)
        .into_iter()
        .filter(|&m| !host.party_waiting(m) && host.current_task(m) != Some(State::WaitForPlayer))
        .position(|m| m == npc);
    rank.map_or(DEFAULT_FOLLOW_DISTANCE, |r| (r as f32 + 3.0) * FOLLOW_STEP)
}

fn queue(host: &mut dyn ScriptHost, npc: EntityId, cmd: AiCommand) -> Out {
    host.queue_ai(npc, cmd);
    Out::Yield
}

fn party_mode(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_ai_mode(npc, AiMode::PlayerParty);
    Out::Next
}

fn follow_callback(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_task_callback(npc, Callback::OnFollowPlayer);
    Out::Next
}

fn wait_callback(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_ai_mode(npc, AiMode::PlayerParty);
    host.set_task_callback(npc, Callback::OnWaitForPlayer);
    Out::Next
}

fn guide_callback(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_task_callback(npc, Callback::OnGuidePlayer);
    Out::Next
}

fn follow_npc_start(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_ai_mode(npc, AiMode::Routine);
    queue(host, npc, AiCommand::StandUp)
}

fn follow_npc_callback(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    host.set_task_callback(npc, Callback::OnFollowNpc);
    Out::Next
}

/// Follow `leader` when farther than `within` (shared by `ZS_FollowPlayer_Loop` and
/// `ZS_FollowNPC_Loop`).
fn follow_leader(host: &mut dyn ScriptHost, npc: EntityId, leader: EntityId, within: f32) -> Out {
    if host.state_time(npc) <= FOLLOW_POLL_SECS {
        return Out::Abort;
    }
    host.set_state_time(npc, 0.0);
    if leader.is_some() && host.distance(npc, leader) > within {
        return queue(
            host,
            npc,
            AiCommand::GotoFollow {
                target: leader,
                within,
            },
        );
    }
    Out::Abort
}

fn follow_player_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let within = follow_distance(host, npc);
    let player = host.player();
    follow_leader(host, npc, player, within)
}

fn follow_npc_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let leader = host.party_leader(npc);
    follow_leader(host, npc, leader, DEFAULT_FOLLOW_DISTANCE)
}

fn wait_loop(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.state_time(npc) <= FOLLOW_POLL_SECS {
        return Out::Abort;
    }
    if host.party_leader(npc) != host.player() {
        continue_routine(host, npc);
    }
    host.set_state_time(npc, 0.0);
    Out::Abort
}

fn guide_go(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    let target = host.destination_point(npc);
    queue(host, npc, AiCommand::GotoExt { target })
}

fn guide_arrived(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.is_on_destination(npc) {
        host.set_state(npc, State::GuidePlayerWait);
        Out::Leave
    } else {
        Out::Abort
    }
}

fn guide_wait(host: &mut dyn ScriptHost, npc: EntityId) -> Out {
    if host.distance(npc, host.player()) < GUIDE_RESUME_DISTANCE {
        host.set_state(npc, State::GuidePlayerLoop);
        return Out::Leave;
    }
    Out::Abort
}

/// `ZS_FollowPlayer` (S:12990): join the player's party mode, stand up, put everything away,
/// register `OnFollowPlayer` and enter the loop.
pub const FOLLOW_PLAYER: &[Step] = &[
    Custom(party_mode),
    StandUp,
    HoldNone,
    Custom(follow_callback),
    Then(End::State(State::FollowPlayerLoop)),
];
/// `ZS_FollowPlayer_Loop` (S:131f0): once per second walk toward the player when farther than the
/// follow distance.
pub const FOLLOW_PLAYER_LOOP: &[Step] = &[Custom(follow_player_loop)];
/// `ZS_WaitForPlayer` (S:14200): hold position in party mode with `OnWaitForPlayer`.
pub const WAIT_FOR_PLAYER: &[Step] = &[
    Custom(wait_callback),
    Then(End::State(State::WaitForPlayerLoop)),
];
/// `ZS_WaitForPlayer_Loop` (S:14330): once per second leave the wait if the player is no longer
/// the leader.
pub const WAIT_FOR_PLAYER_LOOP: &[Step] = &[Custom(wait_loop)];
/// `ZS_GuidePlayer` (S:13b30 family): party mode, hands free, `OnGuidePlayer`, loop.
pub const GUIDE_PLAYER: &[Step] = &[
    Custom(party_mode),
    HoldNone,
    Custom(guide_callback),
    Then(End::State(State::GuidePlayerLoop)),
];
/// `ZS_GuidePlayer_Loop`: walk to the destination, then wait for the player.
pub const GUIDE_PLAYER_LOOP: &[Step] = &[Custom(guide_go), Custom(guide_arrived)];
/// `ZS_GuidePlayer_Wait`: resume once the player has caught up.
pub const GUIDE_PLAYER_WAIT: &[Step] = &[Custom(guide_wait)];
/// `ZS_FollowNPC`: routine AI mode, stand up, hands free, `OnFollowNPC`, loop.
pub const FOLLOW_NPC: &[Step] = &[
    Custom(follow_npc_start),
    HoldNone,
    Custom(follow_npc_callback),
    Then(End::State(State::FollowNpcLoop)),
];
/// `ZS_FollowNPC_Loop`: once per second walk toward the leader NPC when farther than 5 m.
pub const FOLLOW_NPC_LOOP: &[Step] = &[Custom(follow_npc_loop)];

/// Port of `OnFollowPlayer` (S:12f50), the per-tick callback of `ZS_FollowPlayer`.
///
/// The companion stops walking once it is within its follow distance. If the player swims or dives it
/// switches to `ZS_WaitForPlayer`. A **mob** member (dialog `MobEnabled`) farther than 62000 from
/// the enclave "Fireclan" (and not within an unusual state of the engine) is dismissed: it
/// forgets the player, says "TooFarFromMeetingPoint" and resumes its routine. Returns true if it
/// changed the NPC's task.
pub fn on_follow_player(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    let player = host.player();
    let within = follow_distance(host, npc);
    if host.distance(npc, player) <= within {
        host.stop_ai_goto(npc);
    }
    if host.is_swimming(player) {
        host.full_stop(npc);
        host.set_task(npc, State::WaitForPlayer);
        return true;
    }
    if host.dialog_mob_enabled(npc) {
        let camp = host.find_enclave("Fireclan");
        let too_far = camp.is_some() && host.distance(npc, camp) > MOB_LEASH;
        if too_far {
            host.full_stop(npc);
            host.set_party_leader(npc, EntityId::NONE);
            let now = host.playing_time().round() as u32;
            host.set_time_stamp_transformed(npc, now);
            host.set_party_waiting(npc, false);
            host.set_party_member_type(npc, PartyMemberType::None);
            host.queue_ai(
                npc,
                AiCommand::SaySvm {
                    family: "TooFarFromMeetingPoint",
                    index: 0,
                },
            );
            continue_routine(host, npc);
            return true;
        }
    }
    false
}

/// Port of `OnFollowNPC` (S:13790), the per-tick callback of `ZS_FollowNPC`: stop walking once
/// within the follow distance of the leader.
pub fn on_follow_npc(host: &mut dyn ScriptHost, npc: EntityId) {
    let leader = host.party_leader(npc);
    if leader.is_some() && host.distance(npc, leader) <= follow_distance(host, npc) {
        host.stop_ai_goto(npc);
    }
}

/// Port of `OnGuidePlayer` (S:13d60), the per-tick callback of `ZS_GuidePlayer`: when the player
/// lags more than 10 m behind (out of sight, or farther from the destination than the guide),
/// stop and wait.
pub fn on_guide_player(host: &mut dyn ScriptHost, guide: EntityId) -> bool {
    let player = host.player();
    if host.distance(guide, player) <= GUIDE_WAIT_DISTANCE {
        return false;
    }
    let dest = host.destination_point(guide);
    if host.is_in_fov(guide, player) && dest.is_some() {
        let guide_to_dest = host.distance(guide, dest);
        let player_to_dest = host.distance(player, dest);
        if player_to_dest <= guide_to_dest {
            return false;
        }
    }
    host.stop_ai_goto(guide);
    host.set_state(guide, State::GuidePlayerWait);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::Species;
    #[allow(unused_imports)]
    use crate::host::{EntityApi, InventoryApi, NpcApi, ScriptCtx, StatsApi, WorldApi};
    use crate::routine::party::join_player_party;
    use crate::routine::work::{Flow, run_state};
    use crate::testhost::TestWorld;

    fn companions(n: usize) -> (TestWorld, Vec<EntityId>) {
        let mut w = TestWorld::new();
        let mut v = Vec::new();
        for i in 0..n {
            let c = w.spawn_npc(&format!("C{i}"), Species::Human, 5, 5);
            join_player_party(&mut w, c);
            v.push(c);
        }
        (w, v)
    }

    #[test]
    fn follow_distance_by_rank() {
        let (mut w, c) = companions(3);
        assert_eq!(follow_distance(&w, c[0]), 450.0);
        assert_eq!(follow_distance(&w, c[1]), 600.0);
        assert_eq!(follow_distance(&w, c[2]), 750.0);
        // Waiters free their rank.
        w.set_party_waiting(c[0], true);
        assert_eq!(follow_distance(&w, c[1]), 450.0);
        assert_eq!(follow_distance(&w, c[0]), 500.0, "unranked");
        w.set_party_waiting(c[0], false);
        w.set_task_directly(c[1], State::WaitForPlayer);
        assert_eq!(follow_distance(&w, c[2]), 600.0);
        let stranger = w.spawn_npc("S", Species::Human, 5, 5);
        assert_eq!(follow_distance(&w, stranger), 500.0);
    }

    #[test]
    fn follow_start_sequence() {
        let (mut w, c) = companions(1);
        w.set_task_directly(c[0], State::FollowPlayer);
        let mut cmds = Vec::new();
        for _ in 0..4 {
            run_state(&mut w, c[0]);
            cmds.extend(w.finish_commands(c[0]));
            if w.current_state(c[0]) == Some(State::FollowPlayerLoop) {
                break;
            }
        }
        assert_eq!(w.ai_mode(c[0]), AiMode::PlayerParty);
        assert_eq!(w.callback(c[0]), Callback::OnFollowPlayer);
        assert_eq!(cmds[0], AiCommand::StandUp);
        assert_eq!(w.current_state(c[0]), Some(State::FollowPlayerLoop));
    }

    #[test]
    fn follow_loop_walks_when_far() {
        let (mut w, c) = companions(2);
        let p = w.player();
        w.set_task_directly(c[1], State::FollowPlayerLoop);
        w.set_distance(c[1], p, 400.0);
        w.set_state_time(c[1], 1.5);
        assert_eq!(run_state(&mut w, c[1]), Flow::Done);
        assert!(w.finish_commands(c[1]).is_empty(), "within 600: no walk");
        assert_eq!(w.state_time(c[1]), 0.0, "poll timer restarted");
        w.set_distance(c[1], p, 900.0);
        w.set_state_time(c[1], 1.5);
        assert_eq!(run_state(&mut w, c[1]), Flow::Yield);
        assert_eq!(
            w.finish_commands(c[1]),
            vec![AiCommand::GotoFollow {
                target: p,
                within: 600.0
            }]
        );
        // Not due yet.
        w.set_state_time(c[1], 0.5);
        run_state(&mut w, c[1]);
        assert!(w.finish_commands(c[1]).is_empty());
    }

    #[test]
    fn swimming_player_makes_companions_wait() {
        let (mut w, c) = companions(1);
        let p = w.player();
        w.set_distance(c[0], p, 100.0);
        assert!(!on_follow_player(&mut w, c[0]));
        w.swimming = true;
        assert!(on_follow_player(&mut w, c[0]));
        assert_eq!(w.current_task(c[0]), Some(State::WaitForPlayer));
    }

    #[test]
    fn mobs_are_dismissed_far_from_the_camp() {
        let (mut w, c) = companions(1);
        let camp = w.add_enclave(crate::enums::Alignment::Reb);
        w.set_enclave_name(camp, "Fireclan");
        w.set_dialog_mob(c[0], true);
        w.set_distance(c[0], camp, 61_000.0);
        assert!(!on_follow_player(&mut w, c[0]));
        w.set_distance(c[0], camp, 63_000.0);
        assert!(on_follow_player(&mut w, c[0]));
        assert!(w.party_leader(c[0]).is_none());
        assert_eq!(w.party_member_type(c[0]), PartyMemberType::None);
        assert!(w.log.iter().any(|x| matches!(
            x,
            AiCommand::SaySvm {
                family: "TooFarFromMeetingPoint",
                ..
            }
        )));
        // Ordinary companions are not leashed.
        let (mut w, c) = companions(1);
        let camp = w.add_enclave(crate::enums::Alignment::Reb);
        w.set_enclave_name(camp, "Fireclan");
        w.set_distance(c[0], camp, 90_000.0);
        assert!(!on_follow_player(&mut w, c[0]));
    }

    #[test]
    fn waiting_ends_when_the_leader_changes() {
        let (mut w, c) = companions(1);
        w.set_task_directly(c[0], State::WaitForPlayerLoop);
        w.set_state_time(c[0], 2.0);
        run_state(&mut w, c[0]);
        assert_eq!(
            w.current_state(c[0]),
            Some(State::WaitForPlayerLoop),
            "still the leader"
        );
        w.set_leader(c[0], EntityId::NONE, PartyMemberType::None, false);
        w.set_state_time(c[0], 2.0);
        run_state(&mut w, c[0]);
        assert_ne!(w.current_state(c[0]), Some(State::WaitForPlayerLoop));
    }

    #[test]
    fn wait_task_sets_up_the_loop() {
        let (mut w, c) = companions(1);
        w.set_task_directly(c[0], State::WaitForPlayer);
        run_state(&mut w, c[0]);
        assert_eq!(w.callback(c[0]), Callback::OnWaitForPlayer);
        assert_eq!(w.current_state(c[0]), Some(State::WaitForPlayerLoop));
    }

    #[test]
    fn guide_waits_for_a_lagging_player() {
        let mut w = TestWorld::new();
        let p = w.player();
        let guide = w.spawn_npc("Guide", Species::Human, 5, 5);
        let dest = w.add_point(crate::enums::UseType::Stand, EntityId::NONE);
        w.set_destination(guide, dest);
        w.set_distance(guide, p, 500.0);
        assert!(!on_guide_player(&mut w, guide));
        // Far, but the player is ahead of the guide: keep walking.
        w.set_distance(guide, p, 1500.0);
        w.set_distance(guide, dest, 3000.0);
        w.set_distance(p, dest, 2000.0);
        assert!(!on_guide_player(&mut w, guide));
        // Far and lagging: wait.
        w.set_distance(p, dest, 4000.0);
        assert!(on_guide_player(&mut w, guide));
        assert_eq!(w.current_state(guide), Some(State::GuidePlayerWait));
        // Out of sight: wait regardless.
        w.set_task_directly(guide, State::GuidePlayerLoop);
        w.set_distance(p, dest, 2000.0);
        w.fov = false;
        assert!(on_guide_player(&mut w, guide));
    }

    #[test]
    fn guide_loop_and_wait_states() {
        let mut w = TestWorld::new();
        let p = w.player();
        let guide = w.spawn_npc("Guide", Species::Human, 5, 5);
        let dest = w.add_point(crate::enums::UseType::Stand, EntityId::NONE);
        w.set_destination(guide, dest);
        w.set_task_directly(guide, State::GuidePlayer);
        for _ in 0..2 {
            run_state(&mut w, guide);
            w.finish_commands(guide);
        }
        assert_eq!(w.callback(guide), Callback::OnGuidePlayer);
        assert_eq!(w.ai_mode(guide), AiMode::PlayerParty);
        assert_eq!(w.current_state(guide), Some(State::GuidePlayerLoop));
        // Walk, then wait on arrival.
        run_state(&mut w, guide);
        assert_eq!(
            w.finish_commands(guide),
            vec![AiCommand::GotoExt { target: dest }]
        );
        w.set_on_destination(guide, true);
        run_state(&mut w, guide);
        assert_eq!(w.current_state(guide), Some(State::GuidePlayerWait));
        w.set_distance(guide, p, 900.0);
        run_state(&mut w, guide);
        assert_eq!(w.current_state(guide), Some(State::GuidePlayerWait));
        w.set_distance(guide, p, 700.0);
        run_state(&mut w, guide);
        assert_eq!(w.current_state(guide), Some(State::GuidePlayerLoop));
    }

    #[test]
    fn npc_followers_keep_five_metres() {
        let mut w = TestWorld::new();
        let boss = w.spawn_npc("Boss", Species::Human, 5, 5);
        let thrall = w.spawn_npc("Thrall", Species::Human, 5, 5);
        w.set_leader(thrall, boss, PartyMemberType::Controlled, false);
        w.set_task_directly(thrall, State::FollowNpc);
        for _ in 0..4 {
            run_state(&mut w, thrall);
            w.finish_commands(thrall);
        }
        assert_eq!(w.current_state(thrall), Some(State::FollowNpcLoop));
        assert_eq!(w.ai_mode(thrall), AiMode::Routine);
        w.set_distance(thrall, boss, 900.0);
        w.set_state_time(thrall, 2.0);
        run_state(&mut w, thrall);
        assert_eq!(
            w.finish_commands(thrall),
            vec![AiCommand::GotoFollow {
                target: boss,
                within: 500.0
            }]
        );
        w.set_distance(thrall, boss, 300.0);
        on_follow_npc(&mut w, thrall);
        assert_eq!(w.stopped_goto, 1);
    }
}
