//! B06: perception. Who may notice what, and the `Assess*` reactions that turn a noticed event
//! into a task (`ZS_Attack`, `ZS_Flee`, `ZS_ReactToWeapon`, ...).
//!
//! The DLL has no perception radius: `SendPerception*` walk every NPC, filter with
//! [`can_receive_perception_from`] and call the receiver's `Assess*`, each of which applies its
//! own distance, field-of-view and line-of-sight tests. Distances are centimetres.

use crate::attitude::get_attitude;
use crate::crime::{bury, memorize_player_crime, notify_enclave, set_attack_reason};
use crate::enums::{
    AiMode, Alignment, AniState, AttackReason, Attitude, NpcType, PlayerCrime, Species, UseType,
};
use crate::host::{EntityId, ScriptHost};
use crate::predicates::{
    has_dangerous_weapon, is_ambient_creature, is_gate_guard, is_guard, is_humanoid, is_slave,
    is_temporary_friendly_to_player,
};
use crate::routine::state::State;

/// Distance within which a witness notices sneaking, drawn weapons and thefts (7 m).
pub const NOTICE_RANGE: f32 = 700.0;
/// Distance within which a theft is noticed and help is called (10 m).
pub const THEFT_RANGE: f32 = 1000.0;
/// Seconds after leaving the party during which a former companion ignores drawn weapons.
pub const PARTY_GRACE_SECS: f32 = 1.0;
/// State time between the gate guard's perception polls.
pub const GATE_GUARD_POLL_SECS: f32 = 0.1;

/// Port of `FUN_10008f70` (S:8f70), "can see": `observer` has `target` in its field of view and
/// either both stand in the same outdoor zone (no line-of-sight test) or the line is free.
pub fn can_see(host: &dyn ScriptHost, observer: EntityId, target: EntityId) -> bool {
    gothic_portcov::hit!("can_see");
    if !host.is_in_fov(observer, target) {
        return false;
    }
    let zone = host.zone(observer);
    if zone == host.zone(target) && zone.is_some() && host.zone_is_outdoor(zone) {
        return true;
    }
    host.has_free_los(observer, target)
}

/// Port of `FUN_10009100` (S:9100), "is audible": the entity is neither sneaking (`Sneak` or
/// `Parade` pose) nor standing still.
pub fn is_audible(host: &dyn ScriptHost, e: EntityId) -> bool {
    gothic_portcov::hit!("is_audible");
    !matches!(host.ani_state(e), AniState::Parade | AniState::Sneak) && !host.is_standing(e)
}

/// Port of `CanReceivePerceptionFrom` (S:31c80): may `receiver` perceive an event caused by
/// `sender`?
///
/// False for the player, for the sender itself, for ghosts (status 0x80 on the sender) and
/// frozen receivers (0x40), for dead, down, talking and abandoned NPCs, for sleepers unless
/// `wake_sleepers`, and for receivers busy in `ZS_FetchWeapon` or `ZS_Unconscious`.
pub fn can_receive_perception_from(
    host: &dyn ScriptHost,
    receiver: EntityId,
    sender: EntityId,
    wake_sleepers: bool,
) -> bool {
    gothic_portcov::hit!("can_receive_perception_from");
    if !host.exists(receiver) || !host.exists(sender) || receiver == sender {
        return false;
    }
    if receiver == host.player() {
        return false;
    }
    match host.ai_mode(receiver) {
        AiMode::Dead | AiMode::Down | AiMode::Talk | AiMode::Abandoned => return false,
        AiMode::Sleep if !wake_sleepers => return false,
        _ => {}
    }
    if host.has_status(sender, 0x80) || host.has_status(receiver, 0x40) {
        return false;
    }
    !matches!(
        host.current_task(receiver),
        Some(State::FetchWeapon | State::Unconscious)
    )
}

/// Port of `CanAttack` (S:57ce0): species that may enter combat. Ambient creatures, Deer, Pig
/// and Cow never do (they flee instead).
pub fn can_attack(host: &dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("can_attack");
    host.exists(npc)
        && !is_ambient_creature(host, npc)
        && !matches!(
            host.species(npc),
            Species::Deer | Species::Pig | Species::Cow
        )
}

/// Port of `AssessTarget` (S:31470), the funnel for every attack.
///
/// A dead witness stays dead. Otherwise the witness' focus attitude is refreshed; if it panics
/// at the target or cannot attack (see [`can_attack`]) it flees. Else the attack reason is
/// stored ([`set_attack_reason`]), a crime is memorised (`Theft`/`UseInteractObject` give Theft,
/// `Murder` gives Murder), the enclave is notified (`ReactToDamage` mode 3, `Enemy` mode 2), the
/// combat group state is raised, the target is alerted ([`assess_attack`]) and the witness goes
/// to `ZS_Attack`. Returns true when a task was set.
pub fn assess_target(
    host: &mut dyn ScriptHost,
    witness: EntityId,
    target: EntityId,
    reason: AttackReason,
) -> bool {
    gothic_portcov::hit!("assess_target");
    if host.ai_mode(witness) == AiMode::Dead {
        bury(host, witness);
        return false;
    }
    let focus = get_attitude(host, witness, host.player());
    let _ = focus; // PSNpc.AttitudeToPlayer2 is a display property of the host.
    let attitude = get_attitude(host, witness, target);
    if attitude != Attitude::Panic && can_attack(host, witness) {
        set_attack_reason(host, witness, target, reason);
        let crime = match reason {
            AttackReason::Theft | AttackReason::UseInteractObject => PlayerCrime::Theft,
            AttackReason::Murder => PlayerCrime::Murder,
            _ => PlayerCrime::None,
        };
        memorize_player_crime(host, witness, target, crime);
        match reason {
            AttackReason::ReactToDamage => {
                notify_enclave(host, witness, 3);
            }
            AttackReason::Enemy => {
                notify_enclave(host, witness, 2);
            }
            _ => {}
        }
        if host.combat_state(witness) != 1 {
            host.set_combat_state(witness, 1);
        }
        assess_attack(host, target, witness);
        if host.ai_mode(witness) != AiMode::Combat {
            host.full_stop(witness);
        }
        host.set_current_target(witness, target);
        host.set_task(witness, State::Attack);
        return true;
    }
    host.full_stop(witness);
    host.set_current_target(witness, target);
    host.set_task(witness, State::Flee);
    true
}

/// Port of `AssessAttack` (S:31b70): `receiver` learns that `attacker` is fighting; a receiver
/// that is itself in combat (but not yet part of the engaged group) joins with reason
/// `ReactToDamage`.
pub fn assess_attack(host: &mut dyn ScriptHost, receiver: EntityId, attacker: EntityId) -> bool {
    gothic_portcov::hit!("assess_attack");
    if can_receive_perception_from(host, receiver, attacker, false)
        && host.combat_state(receiver) != 1
        && host.ai_mode(receiver) == AiMode::Combat
    {
        assess_target(host, receiver, attacker, AttackReason::ReactToDamage);
        return true;
    }
    false
}

/// Port of `IsBotheredByPlayerTheftOf` (S:30840): does taking `item` bother `witness`?
///
/// Not for plants, teleport runes, bolts, arrows, bacon and `It_Gancka_Wood`; weapon packs
/// bother only Reb-aligned witnesses. Friends and the player's companions never mind. If the
/// zone has an owner the witness must be politically friendly with it. The one room that allows
/// plundering after the `Tjalf_Room_Plunder` event is exempt. In a secured zone the theft always
/// bothers; otherwise only when the item lies within 10 m of the witness' work point.
pub fn is_bothered_by_player_theft_of(
    host: &dyn ScriptHost,
    witness: EntityId,
    item: EntityId,
) -> bool {
    gothic_portcov::hit!("is_bothered_by_player_theft_of");
    if item.is_none() || !host.exists(item) {
        return false;
    }
    let item_name = host.name(item);
    if host.point_use_type(item) == UseType::Plant {
        return false;
    }
    let lower = item_name.to_ascii_lowercase();
    if lower.contains("_teleport_")
        || lower.contains("bolt")
        || lower.contains("arrow")
        || item_name == "It_Bacon"
        || item_name == "It_Gancka_Wood"
    {
        return false;
    }
    if item_name == "It_WeaponPack" && host.alignment(witness) != Alignment::Reb {
        return false;
    }
    if host.npc_type(witness) == NpcType::Friend || host.party_leader(witness) == host.player() {
        return false;
    }
    let zone = host.zone(item);
    let owner = host.zone_owner(zone);
    if owner.is_some() {
        let friendly =
            crate::predicates::political_attitude(host, witness, owner) == Attitude::Friendly;
        if !friendly {
            return false;
        }
    }
    if host.name(zone).eq_ignore_ascii_case("Tjalf_Room")
        && host.game_event_set("Tjalf_Room_Plunder")
    {
        return false;
    }
    if host.zone_security(zone) != crate::enums::SecurityLevel::None {
        return true;
    }
    let work = host.destination_point(witness);
    work.is_some() && host.distance(item, work) < THEFT_RANGE
}

/// Port of `AssessTakeItem` (S:30620): `witness` saw `taker` pick up `item`.
///
/// A witness that was already after the taker stops and resumes its routine. If it is not
/// fleeing, the item was not dropped by someone, the taker is the player within 10 m and visible,
/// and the theft bothers the witness, the witness attacks with reason `Theft`.
pub fn assess_take_item(
    host: &mut dyn ScriptHost,
    witness: EntityId,
    taker: EntityId,
    item: EntityId,
) -> bool {
    gothic_portcov::hit!("assess_take_item");
    if host.current_target(witness) == taker {
        host.full_stop(witness);
        crate::routine::select::continue_routine(host, witness);
    }
    if host.ai_mode(witness) == AiMode::Flee || host.item_is_dropped(item) {
        return false;
    }
    if taker != host.player()
        || host.distance(witness, taker) > THEFT_RANGE
        || !can_see(host, witness, taker)
        || !is_bothered_by_player_theft_of(host, witness, item)
    {
        return false;
    }
    assess_target(host, witness, taker, AttackReason::Theft);
    true
}

/// Port of `SendPerceptionTakeItem` (S:32c00 family): offer the theft to every NPC that can
/// perceive the taker.
pub fn send_perception_take_item(host: &mut dyn ScriptHost, taker: EntityId, item: EntityId) {
    gothic_portcov::hit!("send_perception_take_item");
    for npc in host.all_npcs() {
        if can_receive_perception_from(host, npc, taker, false) {
            assess_take_item(host, npc, taker, item);
        }
    }
}

/// Port of `CallHelp` (S:332e0): `caller` was wronged by `culprit`; guards that are friendly
/// toward the caller, not friendly toward the culprit and within 10 m of the culprit attack it
/// with reason `StopThief`. Does nothing when the caller's own attack reason is StopThief, Theft
/// or UseInteractObject. Returns whether any guard answered.
pub fn call_help(host: &mut dyn ScriptHost, caller: EntityId, culprit: EntityId) -> bool {
    gothic_portcov::hit!("call_help");
    if matches!(
        host.attack_reason(caller),
        AttackReason::StopThief | AttackReason::Theft | AttackReason::UseInteractObject
    ) {
        return false;
    }
    let mut answered = false;
    for guard in host.all_npcs() {
        if can_receive_perception_from(host, guard, caller, false)
            && is_guard(host, guard)
            && get_attitude(host, guard, caller) == Attitude::Friendly
            && get_attitude(host, guard, culprit) != Attitude::Friendly
            && host.distance(guard, culprit) <= THEFT_RANGE
        {
            assess_target(host, guard, culprit, AttackReason::StopThief);
            answered = true;
        }
    }
    answered
}

/// Port of `AssessPlayerSneaking` (S:2f5b0): a sneaking (`Sneak`/`Parade`) player within 7 m
/// and in sight of a witness that is not friendly, not reacting and not fighting makes it enter
/// `ZS_ReactToSneak`.
pub fn assess_player_sneaking(host: &mut dyn ScriptHost, witness: EntityId) -> bool {
    gothic_portcov::hit!("assess_player_sneaking");
    let player = host.player();
    if !matches!(host.ani_state(player), AniState::Parade | AniState::Sneak) {
        return false;
    }
    if matches!(host.ai_mode(witness), AiMode::Reaction | AiMode::Combat) {
        return false;
    }
    if get_attitude(host, witness, player) == Attitude::Friendly
        || host.distance(witness, player) > NOTICE_RANGE
        || !can_see(host, witness, player)
    {
        return false;
    }
    host.full_stop(witness);
    host.set_current_target(witness, player);
    host.set_state(witness, State::ReactToSneak);
    true
}

/// Port of `AssessPlayerWeapon` (S:2f780): a humanoid witness reacts to the player's drawn
/// dangerous weapon.
///
/// Excluded: friend-types, gate guards, slaves, temporary friends, anyone friendly toward the
/// player, witnesses farther than 7 m or without sight, party members and companions dismissed
/// within the last second. The reaction is `ZS_ReactToWeapon`, or `ZS_ObserveSuspect` for a
/// Neutral slave-aligned witness. Drawing a weapon is never a crime.
pub fn assess_player_weapon(host: &mut dyn ScriptHost, witness: EntityId) -> bool {
    gothic_portcov::hit!("assess_player_weapon");
    let player = host.player();
    if !is_humanoid(host, witness) || host.npc_type(witness) == NpcType::Friend {
        return false;
    }
    let attitude = get_attitude(host, witness, player);
    if host.distance(witness, player) > NOTICE_RANGE
        || attitude == Attitude::Friendly
        || is_temporary_friendly_to_player(host, witness)
        || is_gate_guard(host, witness)
        || is_slave(host, witness)
        || !can_see(host, witness, player)
    {
        return false;
    }
    // The DLL compares unsigned values: before the first second of play nobody counts as recent.
    let cutoff = (host.playing_time() - PARTY_GRACE_SECS).round();
    let recent = cutoff >= 0.0 && (cutoff as u32) < host.time_stamp_transformed(witness);
    if host.party_leader(witness) == player || recent {
        return false;
    }
    if host.ai_mode(witness) == AiMode::Reaction || !has_dangerous_weapon(host, player) {
        return false;
    }
    host.full_stop(witness);
    host.set_current_target(witness, player);
    let task = if attitude == Attitude::Neutral && host.alignment(witness) == Alignment::Slave {
        State::ObserveSuspect
    } else {
        State::ReactToWeapon
    };
    host.set_task(witness, task);
    true
}

/// Port of `OnSleep` (S:86dc0), the callback of a sleeping NPC: wake into
/// `ZS_ReactToFootsteps` when the player, whom the sleeper does not like, is audible, in clear
/// view, within 10 m and within 5 m once the zone rule is applied (a sleeper in an outdoor zone
/// ignores a player standing in another zone).
pub fn on_sleep(host: &mut dyn ScriptHost, sleeper: EntityId) -> bool {
    gothic_portcov::hit!("on_sleep");
    let player = host.player();
    if get_attitude(host, sleeper, player) == Attitude::Friendly
        || !host.has_free_los(sleeper, player)
        || host.distance(sleeper, player) > THEFT_RANGE
        || !is_audible(host, player)
    {
        return false;
    }
    let zone = host.zone(sleeper);
    if zone.is_some() && host.zone_is_outdoor(zone) && zone != host.zone(player) {
        return false;
    }
    if host.distance(sleeper, player) > 500.0 {
        return false;
    }
    host.full_stop(sleeper);
    host.set_current_target(sleeper, player);
    host.set_task(sleeper, State::ReactToFootsteps);
    true
}

/// Port of `OnAnimalSleep` (S:89f20), the callback of a sleeping animal: the player wakes it by
/// coming close unless hidden. With `Perk_Sneak_2` a sneaking player (or one in `PS_Ranged`) is
/// never noticed and others are noticed within 10 m; without it a sneaking player is noticed
/// within 5 m and others within 10 m. A woken animal that is not Friendly or Neutral toward the
/// player attacks with reason `Angry`.
pub fn on_animal_sleep(host: &mut dyn ScriptHost, animal: EntityId) -> bool {
    gothic_portcov::hit!("on_animal_sleep");
    let player = host.player();
    let sneaking = matches!(host.ani_state(player), AniState::Parade | AniState::Sneak);
    let d = host.distance(animal, player);
    if host.is_skill_active(player, "Perk_Sneak_2") {
        let hidden = sneaking || host.current_state(player) == Some(State::PsRanged);
        if hidden || d > THEFT_RANGE {
            return false;
        }
    } else if sneaking {
        if d > 500.0 {
            return false;
        }
    } else if d > THEFT_RANGE {
        return false;
    }
    let attitude = get_attitude(host, animal, player);
    if matches!(attitude, Attitude::Friendly | Attitude::Neutral) {
        return false;
    }
    host.full_stop(animal);
    assess_target(host, animal, player, AttackReason::Angry);
    true
}

/// Port of `OnGateGuard` (S:86c80), the local callback of a gate guard: returns true once per
/// [`GATE_GUARD_POLL_SECS`] of state time, when the caller should run the active perception
/// scan.
pub fn on_gate_guard(host: &mut dyn ScriptHost, guard: EntityId) -> bool {
    gothic_portcov::hit!("on_gate_guard");
    if host.state_time(guard) > GATE_GUARD_POLL_SECS {
        host.set_state_time(guard, 0.0);
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{LastFight, PartyMemberType, Slot};
    #[allow(unused_imports)]
    use crate::host::{EntityApi, InventoryApi, NpcApi, ScriptCtx, StatsApi, WorldApi};
    use crate::testhost::TestWorld;

    fn pair() -> (TestWorld, EntityId, EntityId) {
        let mut w = TestWorld::new();
        let p = w.player();
        // Player Nrd, witness Orc: politically hostile, so no species-based friendship.
        w.set_alignment(p, Alignment::Nrd);
        let npc = w.spawn_npc("Witness", Species::Human, 5, 5);
        w.set_alignment(npc, Alignment::Orc);
        (w, p, npc)
    }

    #[test]
    fn receive_filter() {
        let (mut w, p, a) = pair();
        let b = w.spawn_npc("B", Species::Human, 5, 5);
        assert!(can_receive_perception_from(&w, a, b, false));
        assert!(
            !can_receive_perception_from(&w, a, a, false),
            "not the sender"
        );
        assert!(
            !can_receive_perception_from(&w, p, a, false),
            "not the player"
        );
        for mode in [AiMode::Dead, AiMode::Down, AiMode::Talk, AiMode::Abandoned] {
            w.set_ai_mode(a, mode);
            assert!(!can_receive_perception_from(&w, a, b, true), "{mode:?}");
        }
        w.set_ai_mode(a, AiMode::Sleep);
        assert!(!can_receive_perception_from(&w, a, b, false));
        assert!(
            can_receive_perception_from(&w, a, b, true),
            "sleepers can be woken"
        );
        w.set_ai_mode(a, AiMode::Routine);
        w.set_status(b, 0x80);
        assert!(
            !can_receive_perception_from(&w, a, b, false),
            "ghost sender"
        );
        w.set_status(b, 0);
        w.set_status(a, 0x40);
        assert!(
            !can_receive_perception_from(&w, a, b, false),
            "frozen receiver"
        );
        w.set_status(a, 0);
        w.set_task_directly(a, State::Unconscious);
        assert!(!can_receive_perception_from(&w, a, b, false));
        w.set_task_directly(a, State::FetchWeapon);
        assert!(!can_receive_perception_from(&w, a, b, false));
    }

    #[test]
    fn visibility_helpers() {
        let (mut w, p, a) = pair();
        w.fov = false;
        assert!(!can_see(&w, a, p));
        w.fov = true;
        w.los = false;
        assert!(!can_see(&w, a, p));
        let zone = EntityId(700);
        w.set_zone_of(a, zone);
        w.set_zone_of(p, zone);
        w.set_zone_outdoor(zone, true);
        assert!(
            can_see(&w, a, p),
            "same outdoor zone needs no line of sight"
        );
        w.set_zone_outdoor(zone, false);
        assert!(!can_see(&w, a, p));
        w.los = true;
        assert!(can_see(&w, a, p));
        // Audibility.
        w.standing = true;
        assert!(!is_audible(&w, p));
        w.standing = false;
        assert!(is_audible(&w, p));
        w.set_ani_state(p, AniState::Sneak);
        assert!(!is_audible(&w, p));
    }

    #[test]
    fn who_can_attack() {
        let mut w = TestWorld::new();
        for (s, ok) in [
            (Species::Human, true),
            (Species::Wolf, true),
            (Species::Deer, false),
            (Species::Pig, false),
            (Species::Cow, false),
            (Species::Meatbug, false),
        ] {
            let n = w.spawn_npc("x", s, 1, 1);
            assert_eq!(can_attack(&w, n), ok, "{s:?}");
        }
    }

    #[test]
    fn assess_target_attacks_and_remembers() {
        let (mut w, p, npc) = pair();
        assert!(assess_target(&mut w, npc, p, AttackReason::Theft));
        assert_eq!(w.current_task(npc), Some(State::Attack));
        assert_eq!(w.current_target(npc), p);
        assert_eq!(w.attack_reason(npc), AttackReason::Theft);
        assert_eq!(w.last_player_crime(npc), PlayerCrime::Theft);
        assert_eq!(w.combat_state(npc), 1);
        // Murder is a crime of its own; an enemy is not.
        let b = w.spawn_npc("B", Species::Human, 5, 5);
        assert_eq!(w.last_player_crime(b), PlayerCrime::None);
        assert!(assess_target(&mut w, b, p, AttackReason::Enemy));
        assert_eq!(w.last_player_crime(b), PlayerCrime::None);
        assert!(assess_target(&mut w, b, p, AttackReason::Murder));
        assert_eq!(w.last_player_crime(b), PlayerCrime::Murder);
    }

    #[test]
    fn panic_and_unfit_species_flee() {
        let (mut w, p, _) = pair();
        let deer = w.spawn_npc("Deer", Species::Deer, 1, 1);
        assert!(assess_target(&mut w, deer, p, AttackReason::Prey));
        assert_eq!(w.current_task(deer), Some(State::Flee));
        assert_eq!(w.attack_reason(deer), AttackReason::None);
        // Fleeing from the target already: Panic.
        let man = w.spawn_npc("Man", Species::Human, 5, 5);
        let foe = w.spawn_npc("Foe", Species::Human, 5, 5);
        w.set_ai_mode(man, AiMode::Flee);
        w.set_current_target(man, foe);
        assess_target(&mut w, man, foe, AttackReason::Enemy);
        assert_eq!(w.current_task(man), Some(State::Flee));
    }

    #[test]
    fn dead_witness_stays_dead() {
        let (mut w, p, npc) = pair();
        w.set_ai_mode(npc, AiMode::Dead);
        assert!(!assess_target(&mut w, npc, p, AttackReason::Enemy));
        assert_eq!(w.current_task(npc), Some(State::Dead));
    }

    #[test]
    fn the_victim_in_combat_joins_the_fight() {
        let (mut w, _, a) = pair();
        let b = w.spawn_npc("B", Species::Human, 5, 5);
        w.set_alignment(b, Alignment::Orc);
        w.set_ai_mode(b, AiMode::Combat);
        assert!(assess_attack(&mut w, b, a));
        assert_eq!(w.attack_reason(b), AttackReason::ReactToDamage);
        assert_eq!(w.current_task(b), Some(State::Attack));
        // Already part of the group: nothing.
        let c = w.spawn_npc("C", Species::Human, 5, 5);
        w.set_ai_mode(c, AiMode::Combat);
        w.set_combat_state(c, 1);
        assert!(!assess_attack(&mut w, c, a));
        // Not in combat: nothing.
        let d = w.spawn_npc("D", Species::Human, 5, 5);
        assert!(!assess_attack(&mut w, d, a));
    }

    #[test]
    fn theft_bother_rules() {
        let (mut w, _, npc) = pair();
        let mk = |w: &mut TestWorld, n: &str| w.add_item(n);
        let sword = mk(&mut w, "It_Sword");
        let arrow = mk(&mut w, "It_Arrow_Wood");
        let bacon = mk(&mut w, "It_Bacon");
        let pack = mk(&mut w, "It_WeaponPack");
        let rune = mk(&mut w, "It_Spell_Teleport_Ardea");
        let zone = EntityId(800);
        for i in [sword, arrow, bacon, pack, rune] {
            w.set_zone_of(i, zone);
        }
        w.set_zone(zone, EntityId::NONE, crate::enums::SecurityLevel::Public);
        assert!(is_bothered_by_player_theft_of(&w, npc, sword));
        assert!(!is_bothered_by_player_theft_of(&w, npc, arrow));
        assert!(!is_bothered_by_player_theft_of(&w, npc, bacon));
        assert!(!is_bothered_by_player_theft_of(&w, npc, rune));
        assert!(
            !is_bothered_by_player_theft_of(&w, npc, pack),
            "only Reb mind weapon packs"
        );
        w.set_alignment(npc, Alignment::Reb);
        assert!(is_bothered_by_player_theft_of(&w, npc, pack));
        // Friends and companions do not mind.
        w.set_npc_type(npc, NpcType::Friend);
        assert!(!is_bothered_by_player_theft_of(&w, npc, sword));
        w.set_npc_type(npc, NpcType::Personal);
        // Unsecured zone: only near the work point.
        w.set_zone(zone, EntityId::NONE, crate::enums::SecurityLevel::None);
        assert!(!is_bothered_by_player_theft_of(&w, npc, sword));
        let work = w.add_point(UseType::Anvil, EntityId::NONE);
        w.set_destination(npc, work);
        w.set_distance(sword, work, 500.0);
        assert!(is_bothered_by_player_theft_of(&w, npc, sword));
        w.set_distance(sword, work, 1500.0);
        assert!(!is_bothered_by_player_theft_of(&w, npc, sword));
        // A faction zone: the witness must be friendly with its owner.
        let enc = w.add_enclave(Alignment::Nrd);
        w.set_zone(zone, enc, crate::enums::SecurityLevel::Enclave);
        assert!(
            !is_bothered_by_player_theft_of(&w, npc, sword),
            "Reb witness vs Nrd zone"
        );
        w.set_alignment(npc, Alignment::Nrd);
        assert!(is_bothered_by_player_theft_of(&w, npc, sword));
    }

    #[test]
    fn the_plunder_room_is_open_after_the_event() {
        let (mut w, _, npc) = pair();
        let item = w.add_item("It_Sword");
        let zone = w.add_named_zone("Tjalf_Room");
        w.set_zone_of(item, zone);
        w.set_zone(zone, EntityId::NONE, crate::enums::SecurityLevel::Public);
        assert!(is_bothered_by_player_theft_of(&w, npc, item));
        w.set_game_event("Tjalf_Room_Plunder");
        assert!(!is_bothered_by_player_theft_of(&w, npc, item));
    }

    #[test]
    fn take_item_flow() {
        let (mut w, p, npc) = pair();
        let item = w.add_item("It_Sword");
        let zone = EntityId(810);
        w.set_zone_of(item, zone);
        w.set_zone(zone, EntityId::NONE, crate::enums::SecurityLevel::Public);
        w.set_distance(npc, p, 800.0);
        assert!(assess_take_item(&mut w, npc, p, item));
        assert_eq!(w.attack_reason(npc), AttackReason::Theft);
        assert_eq!(w.current_task(npc), Some(State::Attack));
        // Too far, dropped items, fleeing witnesses, other thieves: nothing.
        let far = w.spawn_npc("Far", Species::Human, 5, 5);
        w.set_distance(far, p, 1200.0);
        assert!(!assess_take_item(&mut w, far, p, item));
        let near = w.spawn_npc("Near", Species::Human, 5, 5);
        w.set_distance(near, p, 100.0);
        w.set_item_dropped(item, true);
        assert!(!assess_take_item(&mut w, near, p, item));
        w.set_item_dropped(item, false);
        w.set_ai_mode(near, AiMode::Flee);
        assert!(!assess_take_item(&mut w, near, p, item));
        w.set_ai_mode(near, AiMode::Routine);
        assert!(!assess_take_item(&mut w, near, far, item));
        // Broadcast: everyone who can perceive and is bothered reacts.
        w.set_distance(near, p, 100.0);
        send_perception_take_item(&mut w, p, item);
        assert_eq!(w.current_task(near), Some(State::Attack));
        assert_eq!(w.last_player_crime(near), PlayerCrime::Theft);
    }

    #[test]
    fn guards_answer_calls_for_help() {
        let (mut w, _p, victim) = pair();
        let enc = w.add_enclave(Alignment::Nrd);
        let guard = w.spawn_npc("Guard", Species::Human, 5, 5);
        w.set_alignment(guard, Alignment::Nrd);
        w.set_alignment(victim, Alignment::Nrd);
        w.set_npc_type(guard, NpcType::Enclave);
        w.set_enclave(guard, enc);
        let thief = w.spawn_npc("Thief", Species::Human, 5, 5);
        w.set_alignment(thief, Alignment::Out);
        w.set_distance(guard, thief, 600.0);
        assert!(call_help(&mut w, victim, thief));
        assert_eq!(w.current_task(guard), Some(State::Attack));
        assert_eq!(w.attack_reason(guard), AttackReason::StopThief);
        // Out of range, wrong faction relations, or a theft-type reason of the caller: nothing.
        w.set_task_directly(guard, State::Idiot);
        w.set_distance(guard, thief, 1500.0);
        assert!(!call_help(&mut w, victim, thief));
        w.set_distance(guard, thief, 600.0);
        w.set_attack_reason(victim, AttackReason::Theft);
        assert!(!call_help(&mut w, victim, thief));
    }

    #[test]
    fn sneaking_player_is_noticed_close_by() {
        let (mut w, p, npc) = pair();
        w.set_distance(npc, p, 600.0);
        assert!(!assess_player_sneaking(&mut w, npc), "not sneaking");
        w.set_ani_state(p, AniState::Sneak);
        assert!(assess_player_sneaking(&mut w, npc));
        assert_eq!(w.current_state(npc), Some(State::ReactToSneak));
        assert_eq!(w.current_target(npc), p);
        let far = w.spawn_npc("Far", Species::Human, 5, 5);
        w.set_distance(far, p, 800.0);
        assert!(!assess_player_sneaking(&mut w, far));
        let busy = w.spawn_npc("Busy", Species::Human, 5, 5);
        w.set_distance(busy, p, 100.0);
        w.set_ai_mode(busy, AiMode::Combat);
        assert!(!assess_player_sneaking(&mut w, busy));
        let pal = w.spawn_npc("Pal", Species::Human, 5, 5);
        w.set_distance(pal, p, 100.0);
        w.set_leader(pal, p, PartyMemberType::Party, false);
        assert!(!assess_player_sneaking(&mut w, pal), "friends do not mind");
    }

    #[test]
    fn drawn_weapons_provoke_a_warning_not_a_crime() {
        let (mut w, p, npc) = pair();
        w.set_distance(npc, p, 500.0);
        assert!(!assess_player_weapon(&mut w, npc), "nothing drawn");
        w.set_slot(p, Slot::RightHand, UseType::OneH, 1);
        assert!(assess_player_weapon(&mut w, npc));
        assert_eq!(w.current_task(npc), Some(State::ReactToWeapon));
        assert_eq!(w.last_player_crime(npc), PlayerCrime::None);
        // Fists are harmless.
        let b = w.spawn_npc("B", Species::Human, 5, 5);
        w.set_alignment(b, Alignment::Orc);
        w.set_distance(b, p, 500.0);
        w.set_slot(p, Slot::RightHand, UseType::Fist, 0);
        assert!(!assess_player_weapon(&mut w, b));
        w.set_slot(p, Slot::RightHand, UseType::OneH, 1);
        // Exclusions.
        w.set_distance(b, p, 800.0);
        assert!(!assess_player_weapon(&mut w, b), "too far");
        w.set_distance(b, p, 500.0);
        w.set_leader(b, p, PartyMemberType::Party, false);
        assert!(!assess_player_weapon(&mut w, b), "party members");
        w.set_leader(b, EntityId::NONE, PartyMemberType::None, false);
        w.set_alignment(b, Alignment::Slave);
        assert!(!assess_player_weapon(&mut w, b), "slaves do not care");
        w.set_alignment(b, Alignment::Orc);
        w.set_npc_type(b, NpcType::Friend);
        assert!(!assess_player_weapon(&mut w, b));
        w.set_npc_type(b, NpcType::Personal);
        w.set_last_fight(b, 9.0, LastFight::Friendly);
        w.playing_time = 9.5;
        assert!(!assess_player_weapon(&mut w, b), "temporary friend");
        w.playing_time = 20.0;
        w.set_time_stamp_transformed_value(b, 20);
        assert!(!assess_player_weapon(&mut w, b), "just left the party");
        w.playing_time = 25.0;
        assert!(assess_player_weapon(&mut w, b));
        // Ranged and magic weapons count; harmless spells do not.
        w.set_slot(p, Slot::RightHand, UseType::None, 0);
        w.set_slot(p, Slot::LeftHand, UseType::Bow, 1);
        let c = w.spawn_npc("C", Species::Human, 5, 5);
        w.set_alignment(c, Alignment::Orc);
        w.set_distance(c, p, 100.0);
        assert!(assess_player_weapon(&mut w, c));
    }

    #[test]
    fn neutral_slave_aligned_witnesses_only_observe() {
        let (mut w, p, npc) = pair();
        w.set_distance(npc, p, 500.0);
        w.set_slot(p, Slot::RightHand, UseType::OneH, 1);
        // A slave whose enclave switched sides is no longer a slave for IsSlave, but keeps the
        // Slave alignment: neutral toward the player, it only observes.
        let enc = w.add_enclave(Alignment::Reb);
        w.set_enclave_switched(enc, true);
        w.set_enclave(npc, enc);
        w.set_alignment(npc, Alignment::Slave);
        assert!(assess_player_weapon(&mut w, npc));
        assert_eq!(w.current_task(npc), Some(State::ObserveSuspect));
    }

    #[test]
    fn sleepers_wake_for_footsteps() {
        let (mut w, p, npc) = pair();
        w.set_distance(npc, p, 400.0);
        assert!(on_sleep(&mut w, npc));
        assert_eq!(w.current_task(npc), Some(State::ReactToFootsteps));
        w.set_task_directly(npc, State::Sleep);
        w.set_distance(npc, p, 700.0);
        assert!(!on_sleep(&mut w, npc), "beyond 5 m");
        w.set_distance(npc, p, 400.0);
        w.standing = true;
        assert!(!on_sleep(&mut w, npc), "a motionless player is silent");
        w.standing = false;
        w.set_ani_state(p, AniState::Sneak);
        assert!(!on_sleep(&mut w, npc), "sneaking");
        w.set_ani_state(p, AniState::Stand);
        w.los = false;
        assert!(!on_sleep(&mut w, npc), "no line of sight");
        w.los = true;
        // Friends sleep on.
        w.set_leader(npc, p, PartyMemberType::Party, false);
        assert!(!on_sleep(&mut w, npc));
        w.set_leader(npc, EntityId::NONE, PartyMemberType::None, false);
        // Outdoor sleeper, player in another zone.
        w.set_zone_of(npc, EntityId(901));
        w.set_zone_outdoor(EntityId(901), true);
        assert!(!on_sleep(&mut w, npc));
        w.set_zone_of(p, EntityId(901));
        assert!(on_sleep(&mut w, npc));
    }

    #[test]
    fn sleeping_animals_and_sneakers() {
        let (mut w, p, _) = pair();
        let wolf = w.spawn_npc("Wolf", Species::Wolf, 5, 5);
        w.set_distance(wolf, p, 900.0);
        assert!(
            on_animal_sleep(&mut w, wolf),
            "walking players wake animals within 10 m"
        );
        assert_eq!(w.attack_reason(wolf), AttackReason::Angry);
        assert_eq!(w.current_task(wolf), Some(State::Attack));
        let wolf2 = w.spawn_npc("Wolf2", Species::Wolf, 5, 5);
        w.set_distance(wolf2, p, 1100.0);
        assert!(!on_animal_sleep(&mut w, wolf2));
        w.set_distance(wolf2, p, 600.0);
        w.set_ani_state(p, AniState::Sneak);
        assert!(!on_animal_sleep(&mut w, wolf2), "sneaking: only within 5 m");
        w.set_distance(wolf2, p, 400.0);
        assert!(on_animal_sleep(&mut w, wolf2));
        // The sneak perk hides the player completely.
        let wolf3 = w.spawn_npc("Wolf3", Species::Wolf, 5, 5);
        w.set_distance(wolf3, p, 100.0);
        w.grant_skill(p, "Perk_Sneak_2");
        assert!(!on_animal_sleep(&mut w, wolf3));
        w.set_ani_state(p, AniState::Stand);
        assert!(on_animal_sleep(&mut w, wolf3));
        // Harmless animals stay asleep.
        let deer = w.spawn_npc("Rabbit", Species::Rabbit, 1, 1);
        w.set_distance(deer, p, 100.0);
        assert!(!on_animal_sleep(&mut w, deer));
    }

    #[test]
    fn gate_guard_polls_every_tenth_second() {
        let (mut w, _, npc) = pair();
        w.set_state_time(npc, 0.05);
        assert!(!on_gate_guard(&mut w, npc));
        w.set_state_time(npc, 0.2);
        assert!(on_gate_guard(&mut w, npc));
        assert_eq!(w.state_time(npc), 0.0);
    }
}
