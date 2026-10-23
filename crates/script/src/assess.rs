//! B06 residual: the `Assess*` / `SendPerception*` family beyond the theft, weapon and
//! sneaking handlers of [`crate::perception`]: warnings, fight sounds, murders, defeats, orders
//! to companions, zone observation, preaching and the active perception poll.
//!
//! `SendPerception*` walk every NPC, filter with [`can_receive_perception_from`] and call the
//! receiver's `Assess*`, each of which applies its own distance, sight and attitude tests.
//! Where the decompilation hid the condition chain the doc of the function says `[I]`.

use crate::attitude::get_attitude;
use crate::enums::{AiMode, AttackReason, Attitude, EnclaveStatus, NpcType, SecurityLevel};
use crate::host::{EntityId, ScriptHost};
use crate::perception::{
    NOTICE_RANGE, THEFT_RANGE, assess_player_sneaking, assess_player_weapon, assess_target,
    can_receive_perception_from, can_see, is_bothered_by_player_theft_of,
};
use crate::predicates::{can_threaten, is_boss, is_humanoid, is_upset, zone_attitude_to_player};
use crate::routine::state::State;

/// Distance within which fights, murders and defeats are noticed (20 m).
pub const FIGHT_HEARING_RANGE: f32 = 2000.0;
/// Fight hearing range of an NPC whose enclave is in `Attack` status (40 m).
pub const ENCLAVE_ALERT_RANGE: f32 = 4000.0;

/// `SendPerception*`: offer an event of `sender` to every NPC that may perceive it.
fn broadcast(
    host: &mut dyn ScriptHost,
    sender: EntityId,
    mut assess: impl FnMut(&mut dyn ScriptHost, EntityId),
) {
    for npc in host.all_npcs() {
        if can_receive_perception_from(host, npc, sender, false) {
            assess(host, npc);
        }
    }
}

/// Port of `AssessWarn` (S:318c0): a warning shout makes calm bystanders pay attention.
///
/// Receivers that are not in `Routine` mode, friend-types, the player's companions, strangers
/// farther than 10 m and, unless humanoid, those that are not friendly to the warner ignore it;
/// the rest turn to the warner's target and watch it like the warner does (`ZS_ReactToWeapon`
/// while the warner is in it, else `ZS_ObserveSuspect`). [I on the zone clause]
pub fn assess_warn(host: &mut dyn ScriptHost, receiver: EntityId, warner: EntityId) -> bool {
    gothic_portcov::hit!("assess_warn");
    if host.ai_mode(receiver) != AiMode::Routine || host.npc_type(receiver) == NpcType::Friend {
        return false;
    }
    let player = host.player();
    let zone = host.zone(warner);
    if zone.is_some() && host.zone_is_outdoor(zone) && host.zone(receiver) != zone {
        return false;
    }
    if host.party_leader(receiver) == player {
        return false;
    }
    if !is_humanoid(host, receiver) && get_attitude(host, receiver, warner) != Attitude::Friendly {
        return false;
    }
    if host.distance(receiver, warner) > THEFT_RANGE {
        return false;
    }
    let culprit = host.current_target(warner);
    let reacting = host.current_task(warner) == Some(State::ReactToWeapon);
    host.full_stop(receiver);
    host.set_current_target(receiver, culprit);
    host.set_task(
        receiver,
        if reacting {
            State::ReactToWeapon
        } else {
            State::ObserveSuspect
        },
    );
    true
}

/// Port of `SendPerceptionWarn` (S:32bd0).
pub fn send_perception_warn(host: &mut dyn ScriptHost, warner: EntityId) {
    gothic_portcov::hit!("send_perception_warn");
    broadcast(host, warner, |h, n| {
        assess_warn(h, n, warner);
    });
}

/// Port of `AssessCmdAttack` (S:29e70): a companion attacks what the player focuses.
pub fn assess_cmd_attack(host: &mut dyn ScriptHost, companion: EntityId) -> bool {
    gothic_portcov::hit!("assess_cmd_attack");
    let player = host.player();
    let focus = host.focus_entity();
    if focus.is_none() || focus == player || focus == companion {
        return false;
    }
    assess_target(host, companion, focus, AttackReason::PlayerCommand)
}

/// Port of `SendPerceptionCmdAttack` (S:31ff0): every companion of the player hears the order.
pub fn send_perception_cmd_attack(host: &mut dyn ScriptHost, commander: EntityId) {
    gothic_portcov::hit!("send_perception_cmd_attack");
    let player = host.player();
    broadcast(host, commander, |h, n| {
        if h.party_leader(n) == player {
            assess_cmd_attack(h, n);
        }
    });
}

/// Port of `AssessCmdFollow` (S:56a80): a companion that waits stops waiting and follows again.
pub fn assess_cmd_follow(host: &mut dyn ScriptHost, companion: EntityId) -> bool {
    gothic_portcov::hit!("assess_cmd_follow");
    if host.party_waiting(companion) {
        host.set_party_waiting(companion, false);
        host.set_task(companion, State::FollowPlayer);
        return true;
    }
    false
}

/// Port of `SendPerceptionCmdFollow` (S:32170).
pub fn send_perception_cmd_follow(host: &mut dyn ScriptHost, commander: EntityId) {
    gothic_portcov::hit!("send_perception_cmd_follow");
    let player = host.player();
    broadcast(host, commander, |h, n| {
        if h.party_leader(n) == player {
            assess_cmd_follow(h, n);
        }
    });
}

/// What a bystander does about a fight.
enum Side {
    /// Step in against this fighter.
    Attack(EntityId, AttackReason),
    /// Run from this fighter.
    Flee(EntityId),
    /// Stop and watch.
    Watch,
    /// Not interested.
    Ignore,
}

fn pick_side(host: &dyn ScriptHost, npc: EntityId, a: EntityId, b: EntityId) -> Side {
    let toward_a = get_attitude(host, npc, a);
    let toward_b = get_attitude(host, npc, b);
    if toward_a == Attitude::Panic {
        return Side::Flee(a);
    }
    if toward_b == Attitude::Panic {
        return Side::Flee(b);
    }
    let friendly = |t: Attitude| t == Attitude::Friendly;
    let hostile = |t: Attitude| matches!(t, Attitude::Hostile | Attitude::Predator);
    if friendly(toward_a) && hostile(toward_b) {
        return Side::Attack(b, AttackReason::StopFight);
    }
    if friendly(toward_b) && hostile(toward_a) {
        return Side::Attack(a, AttackReason::StopFight);
    }
    if is_humanoid(host, npc) {
        Side::Watch
    } else {
        Side::Ignore
    }
}

/// Port of `AssessFightSound` (S:2a510): a fight between `attacker` and `victim` is heard.
///
/// Fighters, fleeing, talking and boss NPCs, waiting companions and arena fights are not
/// moved; others within 20 m of either fighter (40 m for a member of an enclave that is in
/// `Attack` status) take a side by their attitudes: they step in against the aggressor of a
/// friend (reason StopFight), flee from what they fear, or stop and watch (`ZS_WatchFight`).
/// [I: the DLL decides with a long attitude case table; this keeps its inputs and outcomes.]
pub fn assess_fight_sound(
    host: &mut dyn ScriptHost,
    receiver: EntityId,
    attacker: EntityId,
    victim: EntityId,
) -> bool {
    gothic_portcov::hit!("assess_fight_sound");
    if receiver == attacker || receiver == victim || victim.is_none() {
        return false;
    }
    if matches!(
        host.ai_mode(receiver),
        AiMode::Combat | AiMode::Flee | AiMode::Talk
    ) || is_boss(host, receiver)
    {
        return false;
    }
    if host.attack_reason(attacker) == AttackReason::Arena
        || host.attack_reason(victim) == AttackReason::Arena
    {
        return false;
    }
    let enclave = host.enclave(receiver);
    let alert = enclave.is_some() && host.enclave_status(enclave) == EnclaveStatus::Attack;
    let range = if alert {
        ENCLAVE_ALERT_RANGE
    } else {
        FIGHT_HEARING_RANGE
    };
    if host.distance(receiver, attacker) > range && host.distance(receiver, victim) > range {
        return false;
    }
    let player = host.player();
    if player.is_some() && host.party_leader(receiver) == player && host.party_waiting(receiver) {
        return false;
    }
    match pick_side(host, receiver, attacker, victim) {
        Side::Attack(target, reason) => {
            assess_target(host, receiver, target, reason);
            true
        }
        Side::Flee(from) => {
            host.full_stop(receiver);
            host.set_current_target(receiver, from);
            host.set_task(receiver, State::Flee);
            true
        }
        Side::Watch => {
            if host.ai_mode(receiver) == AiMode::Routine {
                host.full_stop(receiver);
                host.set_current_target(receiver, attacker);
                host.set_alternative_target(receiver, victim);
                host.set_task(receiver, State::WatchFight);
                return true;
            }
            false
        }
        Side::Ignore => false,
    }
}

/// Port of `SendPerceptionFightSound` (S:322f0).
pub fn send_perception_fight_sound(
    host: &mut dyn ScriptHost,
    attacker: EntityId,
    victim: EntityId,
) {
    gothic_portcov::hit!("send_perception_fight_sound");
    broadcast(host, attacker, |h, n| {
        assess_fight_sound(h, n, attacker, victim);
    });
}

/// Port of `AssessMurder` (S:2eee0): `murderer` killed `victim` where `witness` can tell.
///
/// Fleeing NPCs are out; within 20 m of either (and with a free line of sight to one of them)
/// the witness reacts: it flees from a murderer it panics at; a witness friendly to the victim
/// attacks the murderer (reason Murder, StopFight when both belong to the same enclave,
/// Livestock without a crime for animals of the farm); strangers shy away.
pub fn assess_murder(
    host: &mut dyn ScriptHost,
    witness: EntityId,
    murderer: EntityId,
    victim: EntityId,
) -> bool {
    gothic_portcov::hit!("assess_murder");
    if witness == murderer || host.ai_mode(witness) == AiMode::Flee {
        return false;
    }
    let near = host.distance(witness, murderer) <= FIGHT_HEARING_RANGE
        || host.distance(witness, victim) <= FIGHT_HEARING_RANGE;
    let sees = host.has_free_los(witness, murderer) || host.has_free_los(witness, victim);
    if !near || !sees {
        return false;
    }
    let player = host.player();
    if player.is_some() && host.party_leader(witness) == player && witness != victim {
        return false;
    }
    let toward_murderer = get_attitude(host, witness, murderer);
    let toward_victim = get_attitude(host, witness, victim);
    if toward_murderer == Attitude::Panic {
        host.full_stop(witness);
        host.set_current_target(witness, murderer);
        host.set_task(witness, State::Flee);
        return true;
    }
    if toward_victim != Attitude::Friendly || host.npc_type(witness) == NpcType::Friend {
        return false;
    }
    let reason = if matches!(
        host.species(victim),
        crate::enums::Species::Pig | crate::enums::Species::Cow | crate::enums::Species::Chicken
    ) {
        AttackReason::Livestock
    } else if host.enclave(witness).is_some() && host.enclave(witness) == host.enclave(victim) {
        AttackReason::StopFight
    } else {
        AttackReason::Murder
    };
    assess_target(host, witness, murderer, reason)
}

/// Port of `SendPerceptionMurder` (S:32720).
pub fn send_perception_murder(host: &mut dyn ScriptHost, murderer: EntityId, victim: EntityId) {
    gothic_portcov::hit!("send_perception_murder");
    broadcast(host, murderer, |h, n| {
        assess_murder(h, n, murderer, victim);
    });
}

/// Port of `AssessDefeat` (S:29fb0): a knock-out is seen within 20 m with a free line of sight;
/// humanoid bystanders friendly to the loser who were not fighting it comment on it
/// (`StartSayDefeatComment`).
pub fn assess_defeat(
    host: &mut dyn ScriptHost,
    witness: EntityId,
    victor: EntityId,
    loser: EntityId,
) -> bool {
    gothic_portcov::hit!("assess_defeat");
    if witness == victor || host.ai_mode(witness) == AiMode::Flee {
        return false;
    }
    let near = host.distance(witness, victor) <= FIGHT_HEARING_RANGE
        || host.distance(witness, loser) <= FIGHT_HEARING_RANGE;
    let sees = host.has_free_los(witness, victor) || host.has_free_los(witness, loser);
    if !near || !sees || host.current_target(witness) == loser {
        return false;
    }
    if is_humanoid(host, witness) && get_attitude(host, witness, loser) == Attitude::Friendly {
        return crate::speech::start_say_defeat_comment(host, witness, loser);
    }
    false
}

/// Port of `SendPerceptionDefeat` (S:32600).
pub fn send_perception_defeat(host: &mut dyn ScriptHost, victor: EntityId, loser: EntityId) {
    gothic_portcov::hit!("send_perception_defeat");
    broadcast(host, victor, |h, n| {
        assess_defeat(h, n, victor, loser);
    });
}

/// Port of `AssessUseInteractObject` (S:316f0): a witness within 10 m, in sight and bothered by
/// the use (chest, door, ...) of something that belongs to somebody else reacts like to a theft.
pub fn assess_use_interact_object(
    host: &mut dyn ScriptHost,
    witness: EntityId,
    user: EntityId,
    object: EntityId,
) -> bool {
    gothic_portcov::hit!("assess_use_interact_object");
    if host.ai_mode(witness) == AiMode::Flee || user != host.player() {
        return false;
    }
    if host.distance(witness, user) > THEFT_RANGE
        || !can_see(host, witness, user)
        || !is_bothered_by_player_theft_of(host, witness, object)
    {
        return false;
    }
    assess_target(host, witness, user, AttackReason::UseInteractObject);
    true
}

/// Port of `SendPerceptionUseInteractObject` (S:32a90).
pub fn send_perception_use_interact_object(
    host: &mut dyn ScriptHost,
    user: EntityId,
    object: EntityId,
) {
    gothic_portcov::hit!("send_perception_use_interact_object");
    broadcast(host, user, |h, n| {
        assess_use_interact_object(h, n, user, object);
    });
}

/// Port of `AssessPlayerEnterZone` (S:30000): the player stepped into a zone with an owner
/// within 7 m of the witness: a calm witness goes and observes (`ZS_ObserveZonePlayer`).
pub fn assess_player_enter_zone(host: &mut dyn ScriptHost, witness: EntityId) -> bool {
    gothic_portcov::hit!("assess_player_enter_zone");
    let player = host.player();
    if host.ai_mode(witness) != AiMode::Routine || host.npc_type(witness) == NpcType::Friend {
        return false;
    }
    let zone = host.zone(player);
    if zone.is_none() || host.zone_owner(zone).is_none() {
        return false;
    }
    if host.distance(witness, player) > NOTICE_RANGE || !can_see(host, witness, player) {
        return false;
    }
    host.full_stop(witness);
    host.set_current_target(witness, player);
    host.set_task(witness, State::ObserveZonePlayer);
    true
}

/// Port of `SendPerceptionEnterZone` (S:33170).
pub fn send_perception_enter_zone(host: &mut dyn ScriptHost, player: EntityId) {
    gothic_portcov::hit!("send_perception_enter_zone");
    broadcast(host, player, |h, n| {
        assess_player_enter_zone(h, n);
    });
}

/// Port of `AssessPlayerZone` (S:2fca0): a witness polices the zone the player stands in:
/// public zones are observed, secured zones need the zone owner to be angry at the player and
/// lead to `ZS_ReactToZone`. [I on the vertical-offset clause of the DLL]
pub fn assess_player_zone(host: &mut dyn ScriptHost, witness: EntityId) -> bool {
    gothic_portcov::hit!("assess_player_zone");
    let player = host.player();
    let zone = host.zone(player);
    if zone.is_none() || host.zone_owner(zone).is_none() {
        return false;
    }
    if host.distance(witness, player) > NOTICE_RANGE * 2.0 {
        return false;
    }
    let level = host.zone_security(zone);
    if level != SecurityLevel::Public && zone_attitude_to_player(host, zone) != Attitude::Angry {
        return false;
    }
    host.full_stop(witness);
    host.set_current_target(witness, player);
    host.set_task(
        witness,
        if level == SecurityLevel::Public {
            State::ObserveSuspect
        } else {
            State::ReactToZone
        },
    );
    true
}

/// Port of `SendPerceptionPreach` (S:32cf0): calm humanoids within 10 m of a preaching NPC stop
/// and listen (`ZS_Listen`).
pub fn send_perception_preach(host: &mut dyn ScriptHost, preacher: EntityId) {
    gothic_portcov::hit!("send_perception_preach");
    broadcast(host, preacher, |h, n| {
        if h.ai_mode(n) == AiMode::Routine
            && h.distance(n, preacher) <= THEFT_RANGE
            && h.current_task(n) != Some(State::Listen)
            && h.npc_type(n) != NpcType::Friend
            && is_humanoid(h, n)
        {
            h.set_current_target(n, preacher);
            h.set_task(n, State::Listen);
        }
    });
}

/// How far the active scan sees (centimetres), by what the seen one is.
fn scan_range(host: &dyn ScriptHost, target: EntityId, attitude: Attitude) -> f32 {
    if target == host.player() {
        return 2000.0;
    }
    match host.ai_mode(target) {
        AiMode::Combat | AiMode::Arena => return 4000.0,
        AiMode::Dead | AiMode::Down => return 1000.0,
        _ => {}
    }
    match attitude {
        Attitude::Friendly | Attitude::Neutral | Attitude::None => 700.0,
        Attitude::Angry => 1000.0,
        Attitude::Hostile | Attitude::Predator | Attitude::Panic => 2000.0,
    }
}

/// Port of `AssessActivePerception` (S:295f0): the poll an idle NPC runs to look around.
///
/// It scans the player and all NPCs and keeps the nearest visible one per attitude class within
/// its range, then reacts in a fixed order: a panic-inducing one means `ZS_Flee`; an upset enemy
/// of the player is fought again; a corpse draws a non-humanoid to `ZS_EatBody`; predators and
/// hostile ones are attacked (reasons Prey, Enemy); then the zone check, an angry foe (a
/// threatening creature) and the weapon and sneaking checks of the player run. Fighting NPCs
/// do not react. Returns true when a task was set. [I: the guard/talk/remark block at the end
/// of the DLL function is covered by the individual `Assess*` handlers.]
pub fn assess_active_perception(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("assess_active_perception");
    if matches!(
        host.ai_mode(npc),
        AiMode::Dead
            | AiMode::Down
            | AiMode::Talk
            | AiMode::Sleep
            | AiMode::Abandoned
            | AiMode::Combat
            | AiMode::Arena
            | AiMode::Flee
    ) {
        return false;
    }
    let player = host.player();
    let mut panic: Option<EntityId> = None;
    let mut predator: Option<EntityId> = None;
    let mut hostile: Option<EntityId> = None;
    let mut corpse: Option<EntityId> = None;
    let mut angry: Option<EntityId> = None;
    fn closer(host: &dyn ScriptHost, npc: EntityId, slot: &mut Option<EntityId>, e: EntityId) {
        let d = host.distance(npc, e);
        if slot.is_none_or(|s| host.distance(npc, s) > d) {
            *slot = Some(e);
        }
    }
    let mut candidates = host.all_npcs();
    if player.is_some() {
        candidates.push(player);
    }
    for e in candidates {
        if e == npc || !host.exists(e) {
            continue;
        }
        let attitude = get_attitude(host, npc, e);
        if host.distance(npc, e) > scan_range(host, e, attitude) || !can_see(host, npc, e) {
            continue;
        }
        if e == player && host.is_swimming(e) {
            continue;
        }
        if matches!(host.ai_mode(e), AiMode::Dead | AiMode::Down) {
            if !is_humanoid(host, npc) {
                closer(host, npc, &mut corpse, e);
            }
            continue;
        }
        match attitude {
            Attitude::Panic => closer(host, npc, &mut panic, e),
            Attitude::Predator => closer(host, npc, &mut predator, e),
            Attitude::Hostile => closer(host, npc, &mut hostile, e),
            Attitude::Angry => closer(host, npc, &mut angry, e),
            _ => {}
        }
    }
    if let Some(p) = panic {
        host.full_stop(npc);
        host.set_current_target(npc, p);
        host.set_task(npc, State::Flee);
        return true;
    }
    if player.is_some() && is_upset(host, npc, player) {
        let reason = match host.last_player_attack_reason(npc) {
            AttackReason::None => AttackReason::Enemy,
            r => r,
        };
        assess_target(host, npc, player, reason);
        return true;
    }
    if let Some(c) = corpse {
        host.full_stop(npc);
        host.set_current_target(npc, c);
        host.set_task(npc, State::EatBody);
        return true;
    }
    if let Some(p) = predator {
        assess_target(host, npc, p, AttackReason::Prey);
        return true;
    }
    if let Some(h) = hostile {
        assess_target(host, npc, h, AttackReason::Enemy);
        return true;
    }
    if assess_player_zone(host, npc) {
        return true;
    }
    if let Some(a) = angry
        && !is_humanoid(host, npc)
        && can_threaten(host, npc)
    {
        host.full_stop(npc);
        host.set_current_target(npc, a);
        host.set_task(npc, State::Threaten);
        return true;
    }
    assess_player_weapon(host, npc) || assess_player_sneaking(host, npc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{Alignment, Slot, Species, UseType};
    use crate::host::{EntityApi, NpcApi};
    use crate::testhost::TestWorld;

    /// A calm orc `observer` and an orc `victim` beaten by a human `attacker`, all within 10 m.
    fn brawl() -> (TestWorld, EntityId, EntityId, EntityId) {
        let mut w = TestWorld::new();
        let observer = w.spawn_npc("Watcher", Species::Orc, 5, 5);
        let victim = w.spawn_npc("Victim", Species::Orc, 5, 5);
        let attacker = w.spawn_npc("Attacker", Species::Human, 5, 5);
        for e in [observer, victim, attacker] {
            w.set_alignment(e, Alignment::Orc);
        }
        w.set_distance(observer, attacker, 500.0);
        w.set_distance(observer, victim, 600.0);
        w.set_distance(attacker, victim, 150.0);
        (w, observer, attacker, victim)
    }

    #[test]
    fn a_friend_of_the_victim_steps_in_against_the_aggressor() {
        let (mut w, observer, attacker, victim) = brawl();
        w.set_alignment(attacker, Alignment::Reb);
        assert!(assess_fight_sound(&mut w, observer, attacker, victim));
        assert_eq!(w.current_task(observer), Some(State::Attack));
        assert_eq!(w.current_target(observer), attacker);
    }

    #[test]
    fn a_neutral_bystander_watches_and_far_ones_do_not_hear() {
        let (mut w, observer, attacker, victim) = brawl();
        w.set_alignment(observer, Alignment::Mid);
        w.set_alignment(attacker, Alignment::Mid);
        w.set_alignment(victim, Alignment::Mid);
        assert!(assess_fight_sound(&mut w, observer, attacker, victim));
        assert_eq!(w.current_task(observer), Some(State::WatchFight));
        assert_eq!(w.alternative_target(observer), victim);
        let (mut w, observer, attacker, victim) = brawl();
        w.set_distance(observer, attacker, 2500.0);
        w.set_distance(observer, victim, 2600.0);
        assert!(!assess_fight_sound(&mut w, observer, attacker, victim));
    }

    #[test]
    fn warnings_pass_on_the_reaction_to_calm_neighbours() {
        let (mut w, observer, warner, _) = brawl();
        let culprit = w.player();
        w.set_target(warner, culprit);
        w.set_distance(observer, warner, 400.0);
        w.set_alignment(observer, Alignment::Reb);
        assert!(assess_warn(&mut w, observer, warner));
        assert_eq!(w.current_task(observer), Some(State::ObserveSuspect));
        assert_eq!(w.current_target(observer), culprit);
        // A companion of the player stays out.
        let (mut w, observer, warner, _) = brawl();
        w.set_leader(
            observer,
            w.player(),
            crate::enums::PartyMemberType::Party,
            false,
        );
        assert!(!assess_warn(&mut w, observer, warner));
    }

    #[test]
    fn a_murder_in_front_of_a_friend_of_the_victim_is_answered_with_force() {
        let (mut w, observer, murderer, victim) = brawl();
        w.set_alignment(murderer, Alignment::Reb);
        assert!(assess_murder(&mut w, observer, murderer, victim));
        assert_eq!(w.current_task(observer), Some(State::Attack));
        assert_eq!(w.attack_reason(observer), AttackReason::Murder);
    }

    #[test]
    fn the_active_poll_attacks_what_it_hates_and_ignores_the_calm() {
        let (mut w, npc, foe, _) = brawl();
        w.set_alignment(foe, Alignment::Reb);
        w.set_distance(npc, foe, 900.0);
        w.set_slot(npc, Slot::RightHand, UseType::OneH, 1);
        assert!(assess_active_perception(&mut w, npc));
        assert_eq!(w.current_target(npc), foe);
        let (mut w, npc, _, _) = brawl();
        assert!(
            !assess_active_perception(&mut w, npc),
            "friends and orcs are left alone"
        );
    }
}
