//! B06: crime memory and enclave bookkeeping (`GetPlayerCrime`, `MemorizePlayerCrime`,
//! `SetAttackReason`, `MemorizeEnclaveCrime`, `AddEnclaveTheftCount`, `ChangeEnclaveStatus`,
//! `NotifyEnclave` mode 1).

use crate::enums::{AiMode, AttackReason, Attitude, EnclaveStatus, NpcType, PlayerCrime};
use crate::host::{EntityId, ScriptHost};
use crate::predicates::{is_boss, is_guard, political_attitude};
use crate::routine::state::State;

/// Fame an enclave has of the player after a pardon.
pub const PARDON_FAME: i32 = 100;

/// Enclave and NPC names the DLL exempts from reporting crimes (the enclave "Faring" and a few
/// scripted orcs); see COMBAT_AI_RULES 10.5.
const EXEMPT_ENCLAVE: &str = "Faring";
const EXEMPT_NPCS: &[&str] = &[
    "Orc_Scout_05",
    "Orc_Scout_06",
    "Orc_Warrior_12",
    "Montera_Orc_02",
    "Montera_Orc_03",
    "Montera_Orc_04",
    "Montera_Orc_05",
];

fn exempt(host: &dyn ScriptHost, npc: EntityId, enclave: EntityId) -> bool {
    host.name(enclave).eq_ignore_ascii_case(EXEMPT_ENCLAVE)
        || EXEMPT_NPCS
            .iter()
            .any(|n| host.name(npc).eq_ignore_ascii_case(n))
}

/// Port of `GetPlayerCrime` (S:36fd0): the crime the NPC remembers.
pub fn get_player_crime(host: &dyn ScriptHost, npc: EntityId) -> PlayerCrime {
    host.last_player_crime(npc)
}

/// Port of `SetAttackReason` (S:371c0): record why `npc` attacks `other`.
///
/// While the NPC is already fighting that very target a *lower* reason never replaces a higher
/// one (returns false). When `other` is the player the reason is also stored as `LastPlayerAR`.
pub fn set_attack_reason(
    host: &mut dyn ScriptHost,
    npc: EntityId,
    other: EntityId,
    reason: AttackReason,
) -> bool {
    if host.ai_mode(npc) == AiMode::Combat
        && host.current_target(npc) == other
        && reason.raw() < host.attack_reason(npc).raw()
    {
        return false;
    }
    if other == host.player() {
        host.set_last_player_attack_reason(npc, reason);
    }
    host.set_attack_reason(npc, reason);
    true
}

/// Port of `MemorizePlayerCrime` (S:37080): `npc` witnessed `crime` by `other`.
///
/// Only crimes by the player count and friend-type NPCs never remember them. The enclave gets
/// the first chance ([`memorize_enclave_crime`]); otherwise the NPC keeps the worst crime it has
/// seen (`LastPlayerCrime = max(old, crime)`).
pub fn memorize_player_crime(
    host: &mut dyn ScriptHost,
    npc: EntityId,
    other: EntityId,
    crime: PlayerCrime,
) -> bool {
    if other != host.player() || host.npc_type(npc) == NpcType::Friend || crime == PlayerCrime::None
    {
        return false;
    }
    if memorize_enclave_crime(host, npc, other, crime) {
        return false;
    }
    if host.last_player_crime(npc).raw() <= crime.raw() {
        host.set_last_player_crime(npc, crime);
        return true;
    }
    false
}

/// Port of `MemorizeEnclaveCrime` (S:60670): report a crime to the witness' enclave.
///
/// Only Theft and Murder reach the enclave, never below what it already knows, theft only
/// through a boss or guard, and only when the reporter is politically friendly toward the
/// player. Success records the crime as `KnownPlayerCrime`, forgets the old suspect comment and
/// counts the crime. The scripted exemptions ([`EXEMPT_NPCS`], the enclave "Faring") never
/// report.
pub fn memorize_enclave_crime(
    host: &mut dyn ScriptHost,
    npc: EntityId,
    other: EntityId,
    crime: PlayerCrime,
) -> bool {
    if other != host.player() {
        return false;
    }
    let enclave = host.enclave(npc);
    if enclave.is_none() || exempt(host, npc, enclave) {
        return false;
    }
    if crime.raw() < host.enclave_known_crime(enclave).raw() || crime.raw() <= 2 {
        return false;
    }
    if crime == PlayerCrime::Theft && !is_boss(host, npc) && !is_guard(host, npc) {
        return false;
    }
    if political_attitude(host, npc, other) != Attitude::Friendly {
        return false;
    }
    let n = host.enclave_crime_count(enclave, crime);
    host.set_enclave_crime_count(enclave, crime, n + 1);
    host.clear_enclave_suspect_comment(enclave, crime);
    host.set_enclave_known_crime(enclave, crime);
    true
}

/// Port of `AddEnclaveTheftCount` (S:60930): add `amount` thefts to the enclave that owns the
/// zone the player stands in (or the witness' own enclave outside a secured zone).
///
/// Only for thefts by the player, and not with `Perk_MasterThief` under `ExtendedContent`.
pub fn add_enclave_theft_count(
    host: &mut dyn ScriptHost,
    npc: EntityId,
    other: EntityId,
    amount: i32,
) -> bool {
    if other != host.player() {
        return false;
    }
    if host.setup().extended_content && host.is_skill_active(other, "Perk_MasterThief") {
        return false;
    }
    let zone = host.zone(npc);
    let enclave = if zone.is_some() && host.zone_security(zone) != crate::enums::SecurityLevel::None
    {
        host.zone_owner(zone)
    } else {
        host.enclave(npc)
    };
    if enclave.is_none() || !host.exists(enclave) {
        return false;
    }
    let n = host.enclave_crime_count(enclave, PlayerCrime::Theft);
    host.set_enclave_crime_count(enclave, PlayerCrime::Theft, n + amount);
    true
}

/// Port of `ChangeEnclaveStatus` (S:602d0): switch an enclave between Routine, Attack and Flee.
///
/// Returns false when nothing changes. Any change away from **Attack** is a pardon: the player's
/// fame becomes 100, the known crime, all crime counts and suspect comments, raid and
/// revolution are cleared, and every non-outlaw member takes the enclave's alignment and forgets
/// the fight (last attack reason, attack reason, fight result, crime memory, timestamp). Flee
/// additionally notifies the quest manager and, outside the enclave "Ardea", raises the player's
/// political crime count of the enclave's alignment by one.
pub fn change_enclave_status(
    host: &mut dyn ScriptHost,
    enclave: EntityId,
    new: EnclaveStatus,
) -> bool {
    if host.enclave_status(enclave) == new {
        return false;
    }
    if new != EnclaveStatus::Attack {
        if new == EnclaveStatus::Flee {
            host.on_enclave_state_changed(enclave);
            let alignment = host.enclave_alignment(enclave);
            if !host.name(enclave).eq_ignore_ascii_case("Ardea") {
                let n = host.political_crime_count(alignment);
                host.set_political_crime_count(alignment, n + 1);
            }
        }
        host.set_enclave_player_fame(enclave, PARDON_FAME);
        host.set_enclave_known_crime(enclave, PlayerCrime::None);
        for crime in [
            PlayerCrime::None,
            PlayerCrime::Livestock,
            PlayerCrime::Attack,
            PlayerCrime::Theft,
            PlayerCrime::Murder,
        ] {
            host.set_enclave_crime_count(enclave, crime, 0);
            host.clear_enclave_suspect_comment(enclave, crime);
        }
        host.set_enclave_raid(enclave, false);
        host.set_enclave_revolution(enclave, false);
        let alignment = host.enclave_alignment(enclave);
        for member in host.enclave_members(enclave) {
            if host.alignment(member) != crate::enums::Alignment::Out {
                host.set_alignment(member, alignment);
                host.set_last_player_attack_reason(member, AttackReason::None);
                host.set_attack_reason(member, AttackReason::None);
                host.set_last_fight_vs_player(member, crate::enums::LastFight::None);
                host.set_last_player_crime(member, PlayerCrime::None);
                host.set_last_fight_timestamp(member, 0.0);
            }
        }
    }
    host.set_enclave_status_raw(enclave, new);
    true
}

/// Port of `NotifyEnclave` (S:5ef70): the witness' enclave reacts to an event.
///
/// Mode 1 records a murder (`KnownPlayerCrime = Murder`). Modes 2 (an enemy was sighted) and 3
/// (a member was attacked) count the members that side against the player and the ones that are
/// down, dead or talking: when the able ones are outnumbered five to one the enclave flees
/// (which pardons the player, see [`change_enclave_status`]), otherwise it attacks and up to ten
/// able members nearest to the witness turn on the player. The enclave "Faring" and the named
/// exempt NPCs never notify; a fleeing enclave ignores everything. [I: the DLL's member counting
/// was only partly readable; the head count and the ten-member limit are as read.]
pub fn notify_enclave(host: &mut dyn ScriptHost, witness: EntityId, mode: u32) -> bool {
    let enclave = host.enclave(witness);
    if enclave.is_none() || exempt(host, witness, enclave) {
        return false;
    }
    if host.enclave_status(enclave) == EnclaveStatus::Flee {
        return false;
    }
    match mode {
        1 => {
            host.set_enclave_known_crime(enclave, PlayerCrime::Murder);
            true
        }
        2 | 3 => escalate_enclave(host, witness, enclave),
        _ => false,
    }
}

/// Modes 2 and 3 of [`notify_enclave`].
fn escalate_enclave(host: &mut dyn ScriptHost, witness: EntityId, enclave: EntityId) -> bool {
    let player = host.player();
    let mut able = Vec::new();
    let mut down = 0;
    for m in host.enclave_members(enclave) {
        if m.is_none() || m == player {
            continue;
        }
        if political_attitude(host, m, player) == Attitude::Friendly {
            continue;
        }
        match host.ai_mode(m) {
            AiMode::Talk | AiMode::Dead | AiMode::Down => down += 1,
            _ => {
                if host.in_processing_range(m) {
                    able.push(m);
                }
            }
        }
    }
    let new = if able.len() * 5 < down {
        EnclaveStatus::Flee
    } else {
        EnclaveStatus::Attack
    };
    let changed = change_enclave_status(host, enclave, new);
    if new == EnclaveStatus::Attack && changed {
        able.sort_by(|a, b| {
            host.distance(witness, *a)
                .partial_cmp(&host.distance(witness, *b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for m in able.into_iter().take(10) {
            host.full_stop(m);
            crate::perception::assess_target(host, m, player, AttackReason::Enemy);
        }
    }
    changed
}

/// Put a dead witness to rest: the `AssessTarget` prologue of dead NPCs (`ZS_Dead`).
pub fn bury(host: &mut dyn ScriptHost, npc: EntityId) {
    host.full_stop(npc);
    host.set_task(npc, State::Dead);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{Alignment, LastFight, Species};
    #[allow(unused_imports)]
    use crate::host::{EntityApi, InventoryApi, NpcApi, ScriptCtx, StatsApi, WorldApi};
    use crate::testhost::TestWorld;

    fn guard_world() -> (TestWorld, EntityId, EntityId) {
        let mut w = TestWorld::new();
        let enc = w.add_enclave(Alignment::Reb);
        let guard = w.spawn_npc("Guard", Species::Human, 5, 5);
        w.set_alignment(guard, Alignment::Reb);
        w.set_npc_type(guard, NpcType::Enclave);
        w.set_enclave(guard, enc);
        let p = w.player();
        w.set_alignment(p, Alignment::Reb);
        (w, guard, enc)
    }

    #[test]
    fn attack_reasons_never_downgrade_in_combat() {
        let mut w = TestWorld::new();
        let p = w.player();
        let npc = w.spawn_npc("A", Species::Human, 5, 5);
        assert!(set_attack_reason(&mut w, npc, p, AttackReason::Enemy));
        assert_eq!(w.attack_reason(npc), AttackReason::Enemy);
        assert_eq!(w.last_player_attack_reason(npc), AttackReason::Enemy);
        // Not fighting yet: anything goes.
        assert!(set_attack_reason(&mut w, npc, p, AttackReason::Theft));
        w.set_ai_mode(npc, AiMode::Combat);
        w.set_current_target(npc, p);
        assert!(!set_attack_reason(&mut w, npc, p, AttackReason::Livestock));
        assert_eq!(w.attack_reason(npc), AttackReason::Theft);
        assert!(set_attack_reason(&mut w, npc, p, AttackReason::Murder));
        // Another target resets the comparison.
        let other = w.spawn_npc("B", Species::Human, 5, 5);
        assert!(set_attack_reason(
            &mut w,
            npc,
            other,
            AttackReason::Livestock
        ));
        assert_eq!(w.last_player_attack_reason(npc), AttackReason::Murder);
    }

    #[test]
    fn civilians_keep_the_worst_crime() {
        let mut w = TestWorld::new();
        let p = w.player();
        let civ = w.spawn_npc("Civ", Species::Human, 5, 5);
        assert!(memorize_player_crime(&mut w, civ, p, PlayerCrime::Theft));
        assert_eq!(get_player_crime(&w, civ), PlayerCrime::Theft);
        assert!(memorize_player_crime(&mut w, civ, p, PlayerCrime::Murder));
        assert!(!memorize_player_crime(&mut w, civ, p, PlayerCrime::Theft));
        assert_eq!(get_player_crime(&w, civ), PlayerCrime::Murder);
        // Crimes by others, no crime and friends are ignored.
        let other = w.spawn_npc("X", Species::Human, 5, 5);
        assert!(!memorize_player_crime(
            &mut w,
            civ,
            other,
            PlayerCrime::Murder
        ));
        assert!(!memorize_player_crime(&mut w, other, p, PlayerCrime::None));
        w.set_npc_type(other, NpcType::Friend);
        assert!(!memorize_player_crime(
            &mut w,
            other,
            p,
            PlayerCrime::Murder
        ));
        assert_eq!(get_player_crime(&w, other), PlayerCrime::None);
    }

    #[test]
    fn guards_report_to_the_enclave() {
        let (mut w, guard, enc) = guard_world();
        let p = w.player();
        // Reporting takes the crime out of the NPC's private memory.
        assert!(!memorize_player_crime(&mut w, guard, p, PlayerCrime::Theft));
        assert_eq!(w.enclave_known_crime(enc), PlayerCrime::Theft);
        assert_eq!(w.enclave_crime_count(enc, PlayerCrime::Theft), 1);
        assert_eq!(get_player_crime(&w, guard), PlayerCrime::None);
        // Murder replaces theft; theft never replaces murder.
        memorize_player_crime(&mut w, guard, p, PlayerCrime::Murder);
        assert_eq!(w.enclave_known_crime(enc), PlayerCrime::Murder);
        assert!(!memorize_enclave_crime(
            &mut w,
            guard,
            p,
            PlayerCrime::Theft
        ));
        // Attack and livestock never reach the enclave: the NPC remembers them itself.
        assert!(memorize_player_crime(&mut w, guard, p, PlayerCrime::Attack));
        assert_eq!(get_player_crime(&w, guard), PlayerCrime::Attack);
    }

    #[test]
    fn theft_needs_a_guard_or_boss_and_a_friendly_reporter() {
        let (mut w, _, enc) = guard_world();
        let p = w.player();
        let civ = w.spawn_npc("Civ", Species::Human, 5, 5);
        w.set_alignment(civ, Alignment::Reb);
        w.set_enclave(civ, enc);
        assert!(!memorize_enclave_crime(&mut w, civ, p, PlayerCrime::Theft));
        assert!(memorize_enclave_crime(&mut w, civ, p, PlayerCrime::Murder));
        // A guard of a hostile faction does not report.
        let (mut w2, g2, _) = guard_world();
        let p2 = w2.player();
        w2.set_alignment(p2, Alignment::Nrd);
        w2.set_alignment(g2, Alignment::Reb);
        assert!(!memorize_enclave_crime(
            &mut w2,
            g2,
            p2,
            PlayerCrime::Murder
        ));
    }

    #[test]
    fn scripted_exemptions_never_report() {
        let (mut w, _, _) = guard_world();
        let p = w.player();
        let scout = w.spawn_npc("Orc_Scout_05", Species::Orc, 5, 5);
        let enc = w.add_enclave(Alignment::Orc);
        w.set_alignment(scout, Alignment::Orc);
        w.set_alignment(p, Alignment::Orc);
        w.set_enclave(scout, enc);
        assert!(!memorize_enclave_crime(
            &mut w,
            scout,
            p,
            PlayerCrime::Murder
        ));
        let faring = w.add_enclave(Alignment::Orc);
        w.set_enclave_name(faring, "Faring");
        let grunt = w.spawn_npc("Grunt", Species::Orc, 5, 5);
        w.set_alignment(grunt, Alignment::Orc);
        w.set_enclave(grunt, faring);
        assert!(!memorize_enclave_crime(
            &mut w,
            grunt,
            p,
            PlayerCrime::Murder
        ));
    }

    #[test]
    fn theft_count_goes_to_the_zone_owner() {
        let (mut w, guard, enc) = guard_world();
        let p = w.player();
        assert!(add_enclave_theft_count(&mut w, guard, p, 2));
        assert_eq!(w.enclave_crime_count(enc, PlayerCrime::Theft), 2);
        // A secured zone owned by another enclave takes the count instead.
        let other = w.add_enclave(Alignment::Nrd);
        let zone = EntityId(900);
        w.set_zone_of(guard, zone);
        w.set_zone(zone, other, crate::enums::SecurityLevel::Enclave);
        assert!(add_enclave_theft_count(&mut w, guard, p, 1));
        assert_eq!(w.enclave_crime_count(other, PlayerCrime::Theft), 1);
        // Master thieves are never counted with ExtendedContent.
        w.grant_skill(p, "Perk_MasterThief");
        assert!(!add_enclave_theft_count(&mut w, guard, p, 1));
        w.setup.extended_content = false;
        assert!(add_enclave_theft_count(&mut w, guard, p, 1));
        assert!(!add_enclave_theft_count(&mut w, guard, guard, 1));
    }

    #[test]
    fn pardon_resets_the_enclave_and_its_members() {
        let (mut w, guard, enc) = guard_world();
        let p = w.player();
        w.set_enclave_known_crime(enc, PlayerCrime::Murder);
        w.set_enclave_fame(enc, 10);
        w.set_enclave_crime_count(enc, PlayerCrime::Theft, 7);
        w.set_enclave_raid(enc, true);
        w.set_attack_reason(guard, AttackReason::Murder);
        w.set_last_fight(guard, 5.0, LastFight::Lost);
        w.set_last_player_crime(guard, PlayerCrime::Murder);
        w.add_member(enc, guard);
        let outlaw = w.spawn_npc("Out", Species::Human, 5, 5);
        w.set_alignment(outlaw, Alignment::Out);
        w.add_member(enc, outlaw);
        assert!(!change_enclave_status(&mut w, enc, EnclaveStatus::Routine));
        assert!(change_enclave_status(&mut w, enc, EnclaveStatus::Attack));
        assert_eq!(
            w.enclave_known_crime(enc),
            PlayerCrime::Murder,
            "attack is no pardon"
        );
        assert!(change_enclave_status(&mut w, enc, EnclaveStatus::Routine));
        assert_eq!(w.enclave_player_fame(enc), 100);
        assert_eq!(w.enclave_known_crime(enc), PlayerCrime::None);
        assert_eq!(w.enclave_crime_count(enc, PlayerCrime::Theft), 0);
        assert!(!w.enclave_raid(enc));
        assert_eq!(w.attack_reason(guard), AttackReason::None);
        assert_eq!(w.last_fight_vs_player(guard), LastFight::None);
        assert_eq!(w.last_player_crime(guard), PlayerCrime::None);
        assert_eq!(w.alignment(outlaw), Alignment::Out, "outlaws stay outlaws");
        // Flight counts against the player's standing with the faction, except in Ardea.
        assert!(change_enclave_status(&mut w, enc, EnclaveStatus::Flee));
        assert_eq!(w.political_crime_count(Alignment::Reb), 1);
        w.set_enclave_name(enc, "Ardea");
        change_enclave_status(&mut w, enc, EnclaveStatus::Routine);
        change_enclave_status(&mut w, enc, EnclaveStatus::Flee);
        assert_eq!(w.political_crime_count(Alignment::Reb), 1);
        let _ = p;
    }

    #[test]
    fn a_beaten_enclave_flees_a_strong_one_attacks() {
        // Hostile members that are down or dead outnumber the able ones five to one: flight.
        let (mut w, guard, enc) = guard_world();
        w.set_alignment(w.player(), Alignment::Orc);
        let mut downed = Vec::new();
        for i in 0..6 {
            let m = w.spawn_npc(&format!("Down{i}"), Species::Human, 5, 5);
            w.set_alignment(m, Alignment::Reb);
            w.set_enclave(m, enc);
            w.add_member(enc, m);
            w.set_ai_mode(m, AiMode::Down);
            downed.push(m);
        }
        w.add_member(enc, guard);
        assert!(notify_enclave(&mut w, guard, 3));
        assert_eq!(w.enclave_status(enc), EnclaveStatus::Flee);
        // A fresh enclave with able members attacks and sends them against the player.
        let (mut w, guard, enc) = guard_world();
        w.set_alignment(w.player(), Alignment::Orc);
        w.add_member(enc, guard);
        assert!(notify_enclave(&mut w, guard, 2));
        assert_eq!(w.enclave_status(enc), EnclaveStatus::Attack);
        assert_eq!(w.current_target(guard), w.player());
    }

    #[test]
    fn notify_mode_one_sets_murder() {
        let (mut w, guard, enc) = guard_world();
        assert!(notify_enclave(&mut w, guard, 1));
        assert_eq!(w.enclave_known_crime(enc), PlayerCrime::Murder);
        w.set_enclave_status(enc, EnclaveStatus::Flee);
        assert!(
            !notify_enclave(&mut w, guard, 1),
            "fleeing enclaves ignore news"
        );
        bury(&mut w, guard);
        assert_eq!(w.current_task(guard), Some(State::Dead));
    }
}
