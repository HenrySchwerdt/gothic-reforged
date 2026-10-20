//! B02: decision predicates (`AI_Decision`, 38 of the 43 batch functions; the remaining five
//! routine predicates live in [`crate::routine`]).
//!
//! Pure queries over host state; the only writes are the two documented side effects
//! (`SetLastFightTimestamp`, `WantToRansack`). Species sets are explicit tables so they can be
//! diffed against the DLL's jump tables.
//!
//! **Not ported yet** (need constants the decompiler lost, see PORT_NOTES in the plan doc):
//! `CanParade`, `CanParadeMoveOf`, `CanParadeMagic`, `CanParadeMissile`, `HasStaffMagicParade`,
//! `IsInParadeMode`, `IsInParadePose` (hand-item pair arguments), `HasDangerousWeapon`,
//! `IsLivestock`, and the attitude front-ends `GetAttitude`, `GetOutlawAttitude`,
//! `GetRevolutionAttitude`, `GetAttitudeToPlayer`, `GetFocusNameAttitude`,
//! `GetSpeciesAttitude` (the simulation crate still hosts those).

use crate::enums::{
    AiMode, Alignment, AttackReason, Attitude, EnclaveStatus, GuardStatus, LastFight, NpcClass,
    NpcType, SecurityLevel, Slot, Species, UseType,
};
use crate::host::{EntityId, ScriptHost};

/// Port of `IsHumanoid` (S:10f20): the entity exists and is a Human or an Orc.
pub fn is_humanoid(host: &dyn ScriptHost, e: EntityId) -> bool {
    host.exists(e) && matches!(host.species(e), Species::Human | Species::Orc)
}

/// Port of `IsUndead` (S:112f0): Skeleton or Zombie.
pub fn is_undead(host: &dyn ScriptHost, e: EntityId) -> bool {
    species_is_undead(host.species(e))
}

/// Species half of [`is_undead`].
pub const fn species_is_undead(s: Species) -> bool {
    matches!(s, Species::Skeleton | Species::Zombie)
}

/// Port of `IsEvil` (S:10aa0): undead, Golem, Demon, Gargoyle, FireGolem, IceGolem or
/// ScorpionKing.
pub fn is_evil(host: &dyn ScriptHost, e: EntityId) -> bool {
    species_is_evil(host.species(e))
}

/// Species half of [`is_evil`].
pub const fn species_is_evil(s: Species) -> bool {
    use Species as S;
    species_is_undead(s)
        || matches!(
            s,
            S::Golem | S::Demon | S::Gargoyle | S::FireGolem | S::IceGolem | S::ScorpionKing
        )
}

/// Port of `IsAmbientCreature` (S:10140): the small harmless fauna that only wander.
pub fn is_ambient_creature(host: &dyn ScriptHost, e: EntityId) -> bool {
    species_is_ambient(host.species(e))
}

/// Species half of [`is_ambient_creature`].
pub const fn species_is_ambient(s: Species) -> bool {
    use Species as S;
    matches!(
        s,
        S::Lizard
            | S::Fish
            | S::Meatbug
            | S::Vulture
            | S::Rabbit
            | S::Chicken
            | S::Molerat
            | S::Snake
            | S::Spider
            | S::Turtle
            | S::Bird
            | S::Ray
            | S::Scorpion
            | S::Seagull
            | S::Walrus
            | S::Whale
            | S::Eagle
    )
}

/// Port of `CanSleep` (S:e060): species that use sleeping points.
pub fn can_sleep(host: &dyn ScriptHost, e: EntityId) -> bool {
    use Species as S;
    matches!(
        host.species(e),
        S::Human
            | S::Orc
            | S::Goblin
            | S::Scavenger
            | S::Wolf
            | S::Boar
            | S::Sabertooth
            | S::Shadowbeast
            | S::Bison
            | S::Rhino
            | S::Ripper
            | S::Lurker
            | S::Varan
            | S::Snapper
            | S::Alligator
            | S::FireVaran
            | S::Pig
            | S::Cow
            | S::SwampLurker
            | S::Ogre
            | S::Stalker
    )
}

/// Port of `CanThreaten` (S:e180): species that can play the threaten reaction (Goblin through
/// Bloodfly, SwampLurker, Ogre, Stalker, Dragon).
pub fn can_threaten(host: &dyn ScriptHost, e: EntityId) -> bool {
    use Species as S;
    matches!(
        host.species(e),
        S::Goblin
            | S::Troll
            | S::Minecrawler
            | S::Scavenger
            | S::Wolf
            | S::Boar
            | S::Sabertooth
            | S::Shadowbeast
            | S::Bison
            | S::Rhino
            | S::Ripper
            | S::Lurker
            | S::Varan
            | S::Snapper
            | S::Alligator
            | S::Trex
            | S::FireVaran
            | S::Bloodfly
            | S::SwampLurker
            | S::Ogre
            | S::Stalker
            | S::Dragon
    )
}

/// Port of `CanEatGround` (S:e2a0): grazing and scavenging species.
pub fn can_eat_ground(host: &dyn ScriptHost, e: EntityId) -> bool {
    use Species as S;
    matches!(
        host.species(e),
        S::Scavenger
            | S::Wolf
            | S::Boar
            | S::Sabertooth
            | S::Shadowbeast
            | S::Bison
            | S::Rhino
            | S::Ripper
            | S::Lurker
            | S::Varan
            | S::Snapper
            | S::Alligator
            | S::Trex
            | S::FireVaran
            | S::Deer
            | S::Pig
            | S::Cow
            | S::SwampLurker
            | S::Dragon
    )
}

/// Port of `CanSwim` (S:bd420): a stub that always returns false in the shipped script.
pub fn can_swim(_host: &dyn ScriptHost, _e: EntityId) -> bool {
    false
}

/// Port of `GetFollowTime` (S:10240): seconds a chaser follows before giving up.
///
/// Wolf: 15 while the player sprints, else 20. With `AIMode` every non-humanoid gives up after 9,
/// humanoids (and everyone without `AIMode`) after 5.
pub fn follow_time(host: &dyn ScriptHost, npc: EntityId) -> i32 {
    let species = host.species(npc);
    if species == Species::Wolf {
        return if host.is_sprinting(host.player()) {
            15
        } else {
            20
        };
    }
    if host.setup().ai_mode && !matches!(species, Species::Human | Species::Orc) {
        9
    } else {
        5
    }
}

/// Port of `HasAttackReasonToWatch` (S:10550): reasons 17..=18 and 20 (Duel, Arena, Kill).
pub fn has_attack_reason_to_watch(host: &dyn ScriptHost, npc: EntityId) -> bool {
    matches!(
        host.attack_reason(npc),
        AttackReason::Duel | AttackReason::Arena | AttackReason::Kill
    )
}

/// Port of `GetZoneAttitudeToPlayer` (S:f410): how the owner of `zone` regards the player.
///
/// Public zones owned by an enclave: security level Political compares the player's political
/// fame of the owner alignment with 75 (below: Angry), Enclave compares the enclave's own player
/// fame (below 75: Angry), Public is Neutral; everything else Friendly. (COMBAT_AI_RULES 10.4
/// described this by alignment; the code is keyed by the zone's security level.)
pub fn zone_attitude_to_player(host: &dyn ScriptHost, zone: EntityId) -> Attitude {
    const FRIEND_FAME: i32 = 75;
    let owner = host.zone_owner(zone);
    if owner.is_none() || !host.exists(owner) {
        return Attitude::Friendly;
    }
    match host.zone_security(zone) {
        SecurityLevel::Political => {
            if host.political_fame(host.enclave_alignment(owner)) < FRIEND_FAME {
                return Attitude::Angry;
            }
        }
        SecurityLevel::Enclave => {
            if host.enclave_player_fame(owner) < FRIEND_FAME {
                return Attitude::Angry;
            }
        }
        SecurityLevel::Public => return Attitude::Neutral,
        SecurityLevel::None => {}
    }
    Attitude::Friendly
}

/// Port of `HasAttackReasonToKill` (S:103b0): `other` is the NPC's current target and its attack
/// reason justifies lethal force. Enemy, Murder, Revolution and Kill always do; GateGuard and
/// Intruder do when the target is the player and the zone does not treat the player as a friend.
pub fn has_attack_reason_to_kill(host: &dyn ScriptHost, npc: EntityId, other: EntityId) -> bool {
    if host.current_target(npc) != other {
        return false;
    }
    match host.attack_reason(npc) {
        AttackReason::Enemy
        | AttackReason::Murder
        | AttackReason::Revolution
        | AttackReason::Kill => true,
        AttackReason::GateGuard | AttackReason::Intruder => {
            other == host.player()
                && zone_attitude_to_player(host, host.zone(other)) != Attitude::Friendly
        }
        _ => false,
    }
}

/// Port of `IsDroppableWeapon` (S:10970): an item of non-zero category whose use type is a
/// one-hander, two-hander, bow, crossbow, shield, staff, torch, halberd or axe.
pub fn is_droppable_weapon(host: &dyn ScriptHost, e: EntityId, slot: Slot) -> bool {
    host.slot_item_category(e, slot) != 0
        && matches!(
            host.slot_use_type(e, slot),
            UseType::OneH
                | UseType::TwoH
                | UseType::Bow
                | UseType::CrossBow
                | UseType::Shield
                | UseType::Staff
                | UseType::Torch
                | UseType::Halberd
                | UseType::Axe
        )
}

/// Port of `IsInFistMode` (S:db00): not a Minecrawler, and the right-hand item is a fist
/// (`Fist` or `PhysicalFist`).
pub fn is_in_fist_mode(host: &dyn ScriptHost, e: EntityId) -> bool {
    host.species(e) != Species::Minecrawler
        && matches!(
            host.slot_use_type(e, Slot::RightHand),
            UseType::Fist | UseType::PhysicalFist
        )
}

/// Port of `IsBoss` (S:10bd0): belongs to an enclave and is a political (leader) NPC.
pub fn is_boss(host: &dyn ScriptHost, e: EntityId) -> bool {
    host.enclave(e).is_some() && host.npc_type(e) == NpcType::Political
}

/// Port of `IsGuard` (S:10ce0): an enclave-type NPC whose alignment equals the enclave's (or is
/// Pirate).
pub fn is_guard(host: &dyn ScriptHost, e: EntityId) -> bool {
    let enclave = host.enclave(e);
    if enclave.is_none() || host.npc_type(e) != NpcType::Enclave {
        return false;
    }
    let own = host.alignment(e);
    own == host.enclave_alignment(enclave) || own == Alignment::Pirate
}

/// Port of `IsGateGuard` (S:10e20): guard status other than Inactive and a guard point is set.
pub fn is_gate_guard(host: &dyn ScriptHost, e: EntityId) -> bool {
    host.guard_status(e) != GuardStatus::Inactive && host.guard_point(e).is_some()
}

/// Port of `IsSlave` (S:111a0): Slave alignment, unless the enclave has switched sides or is
/// fleeing.
pub fn is_slave(host: &dyn ScriptHost, e: EntityId) -> bool {
    if host.alignment(e) != Alignment::Slave {
        return false;
    }
    let enclave = host.enclave(e);
    if enclave.is_none() {
        return true;
    }
    !(host.enclave_switched(enclave) || host.enclave_status(enclave) == EnclaveStatus::Flee)
}

/// Port of `SetLastFightTimestamp` (S:113d0): when `other` is the player, stamp the NPC's
/// `LastFightTimestamp` with the current play time.
pub fn set_last_fight_timestamp(host: &mut dyn ScriptHost, npc: EntityId, other: EntityId) -> bool {
    if other == host.player() {
        let now = host.playing_time();
        host.set_last_fight_timestamp(npc, now);
    }
    true
}

/// Window after a fight during which an NPC counts as a temporary friend (play-time units).
pub const TEMPORARY_FRIEND_WINDOW: f32 = 1.2;
/// Window within which a "low" attack reason keeps an NPC upset.
pub const UPSET_LOW_WINDOW: f32 = 0.24;
/// Window within which Enemy/Murder keeps an NPC upset.
pub const UPSET_HIGH_WINDOW: f32 = 1.2;

/// Port of `IsTemporaryFriendlyToPlayer` (S:114c0): the last fight against the player ended
/// `Friendly` less than [`TEMPORARY_FRIEND_WINDOW`] ago.
pub fn is_temporary_friendly_to_player(host: &dyn ScriptHost, npc: EntityId) -> bool {
    host.last_fight_timestamp(npc) > 0.0
        && host.last_fight_vs_player(npc) == LastFight::Friendly
        && host.playing_time() - host.last_fight_timestamp(npc) < TEMPORARY_FRIEND_WINDOW
}

/// Port of `IsUpset` (S:115d0): `other` is the player, the NPC's last fight against the player
/// was cancelled, it is not transformed, and the last attack reason against the player keeps it
/// upset: Kill always, Enemy/Murder for [`UPSET_HIGH_WINDOW`], the "low" reasons (Livestock,
/// StopThief, Theft, UseInteractObject, ReactToWeapon, StopFight, ReactToDamage, GateGuard) for
/// [`UPSET_LOW_WINDOW`].
pub fn is_upset(host: &dyn ScriptHost, npc: EntityId, other: EntityId) -> bool {
    if other != host.player()
        || host.last_fight_timestamp(npc) <= 0.0
        || host.last_fight_vs_player(npc) != LastFight::Cancel
        || host.is_transformed(npc)
    {
        return false;
    }
    let elapsed = host.playing_time() - host.last_fight_timestamp(npc);
    match host.last_player_attack_reason(npc) {
        AttackReason::Livestock
        | AttackReason::StopThief
        | AttackReason::Theft
        | AttackReason::UseInteractObject
        | AttackReason::ReactToWeapon
        | AttackReason::StopFight
        | AttackReason::ReactToDamage
        | AttackReason::GateGuard => elapsed < UPSET_LOW_WINDOW,
        AttackReason::Enemy | AttackReason::Murder => elapsed < UPSET_HIGH_WINDOW,
        AttackReason::Kill => true,
        _ => false,
    }
}

/// Port of `WantToRansack` (S:117c0): a humanoid NPC that has not ransacked yet wants to loot a
/// humanoid `other`, except when `other` is the player and the NPC is a friend-type, and never
/// for Mage or Paladin class NPCs. Succeeding sets `Ransacked` so it happens once.
pub fn want_to_ransack(host: &mut dyn ScriptHost, npc: EntityId, other: EntityId) -> bool {
    if other.is_none() || host.ransacked(npc) {
        return false;
    }
    if !is_humanoid(host, npc) || !is_humanoid(host, other) {
        return false;
    }
    let friend_of_player = other == host.player() && host.npc_type(npc) == NpcType::Friend;
    let class = host.npc_class(npc);
    if friend_of_player || matches!(class, NpcClass::Mage | NpcClass::Paladin) {
        return false;
    }
    host.set_ransacked(npc, true);
    true
}

/// Attitude row/column order of [`POLITICAL_MATRIX`]: Orc, Nrd, Reb, Mid, Ass, Nom, Out, Slave,
/// Pirate.
const fn alignment_index(a: Alignment) -> Option<usize> {
    match a {
        Alignment::None => None,
        Alignment::Orc => Some(0),
        Alignment::Nrd => Some(1),
        Alignment::Reb => Some(2),
        Alignment::Mid => Some(3),
        Alignment::Ass => Some(4),
        Alignment::Nom => Some(5),
        Alignment::Out => Some(6),
        Alignment::Slave => Some(7),
        Alignment::Pirate => Some(8),
    }
}

/// Political attitude matrix decoded from the jump tables of `GetPoliticalAttitude` (S:f620).
/// Rows are the observer, columns the target (Orc Nrd Reb Mid Ass Nom Out Slave Pirate).
/// `Attitude::None` in a cell marks the chapter-dependent Orc/Assassin pairs handled in code.
pub const POLITICAL_MATRIX: [[Attitude; 9]; 9] = {
    use Attitude::{Friendly as F, Hostile as H, Neutral as N, None as X};
    [
        [F, H, H, N, X, H, H, N, F], // Orc
        [H, F, N, N, H, N, H, N, F], // Nrd
        [H, N, F, N, H, N, H, F, F], // Reb
        [N, N, N, F, N, N, N, N, F], // Mid
        [X, H, H, N, F, H, H, N, F], // Ass
        [H, N, N, N, H, F, H, N, F], // Nom
        [H, H, H, H, H, H, F, H, N], // Out
        [N, N, N, N, N, N, N, N, N], // Slave
        [N, N, N, N, N, N, N, N, N], // Pirate (only reached for humanoids)
    ]
};

/// Port of `GetPoliticalAttitude` (S:f620): attitude of `observer` toward `target` from their
/// political alignments.
///
/// A Pirate alignment first resolves: Pirate vs Out is Neutral, Pirate vs Pirate Friendly,
/// otherwise the Pirate NPC takes its enclave's alignment. The matrix gives the rest. Orc vs
/// Assassin is Friendly except in chapter 2 where it is Hostile, and Assassin vs Orc likewise;
/// the Orc-side check in the DLL additionally exempts two named NPCs that the decompiler did not
/// preserve [U]. Missing alignments give `Attitude::None`.
pub fn political_attitude(host: &dyn ScriptHost, observer: EntityId, target: EntityId) -> Attitude {
    let mut a = host.alignment(observer);
    let mut b = host.alignment(target);
    if a == Alignment::Pirate {
        match b {
            Alignment::Out => return Attitude::Neutral,
            Alignment::Pirate => return Attitude::Friendly,
            _ => {
                let enclave = host.enclave(observer);
                if enclave.is_some() {
                    a = host.enclave_alignment(enclave);
                }
            }
        }
    }
    if b == Alignment::Pirate {
        match a {
            Alignment::Out => return Attitude::Neutral,
            Alignment::Pirate => return Attitude::Friendly,
            _ => {
                let enclave = host.enclave(target);
                if enclave.is_some() {
                    b = host.enclave_alignment(enclave);
                }
            }
        }
    }
    let (Some(r), Some(c)) = (alignment_index(a), alignment_index(b)) else {
        return Attitude::None;
    };
    if a == Alignment::Pirate && !is_humanoid(host, observer) {
        return Attitude::None;
    }
    match POLITICAL_MATRIX[r][c] {
        Attitude::None => {
            // Orc <-> Assassin: hostile only during chapter 2.
            if host.chapter() == 2 {
                Attitude::Hostile
            } else {
                Attitude::Friendly
            }
        }
        cell => cell,
    }
}

/// True when the AI mode is one of the modes in which an NPC is dead or down.
pub fn is_dead_or_down(mode: AiMode) -> bool {
    matches!(mode, AiMode::Dead | AiMode::Down)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use crate::host::{EntityApi, InventoryApi, NpcApi, ScriptCtx, StatsApi};
    use crate::testhost::TestWorld;

    fn pair() -> (TestWorld, EntityId, EntityId) {
        let mut w = TestWorld::new();
        let a = w.spawn_npc("A", Species::Human, 10, 10);
        let b = w.spawn_npc("B", Species::Human, 10, 10);
        (w, a, b)
    }

    #[test]
    fn species_tables() {
        let mut w = TestWorld::new();
        let mk = |w: &mut TestWorld, s| w.spawn_npc("x", s, 1, 1);
        let wolf = mk(&mut w, Species::Wolf);
        let deer = mk(&mut w, Species::Deer);
        let golem = mk(&mut w, Species::Golem);
        let zombie = mk(&mut w, Species::Zombie);
        let bug = mk(&mut w, Species::Meatbug);
        let orc = mk(&mut w, Species::Orc);
        assert!(can_sleep(&w, wolf) && !can_sleep(&w, deer) && !can_sleep(&w, bug));
        assert!(can_threaten(&w, wolf) && !can_threaten(&w, orc) && !can_threaten(&w, deer));
        assert!(can_eat_ground(&w, deer) && can_eat_ground(&w, wolf) && !can_eat_ground(&w, orc));
        assert!(is_evil(&w, golem) && is_evil(&w, zombie) && !is_evil(&w, wolf));
        assert!(is_undead(&w, zombie) && !is_undead(&w, golem));
        assert!(is_ambient_creature(&w, bug) && !is_ambient_creature(&w, deer));
        assert!(is_humanoid(&w, orc) && !is_humanoid(&w, wolf));
        assert!(!is_humanoid(&w, EntityId::NONE));
        assert!(!can_swim(&w, orc));
    }

    #[test]
    fn follow_time_rules() {
        let mut w = TestWorld::new();
        let wolf = w.spawn_npc("w", Species::Wolf, 1, 1);
        let boar = w.spawn_npc("b", Species::Boar, 1, 1);
        let man = w.spawn_npc("m", Species::Human, 1, 1);
        assert_eq!(follow_time(&w, wolf), 20);
        w.player_sprinting = true;
        assert_eq!(follow_time(&w, wolf), 15);
        assert_eq!(follow_time(&w, boar), 9);
        assert_eq!(follow_time(&w, man), 5);
        w.setup.ai_mode = false;
        assert_eq!(follow_time(&w, boar), 5);
    }

    #[test]
    fn political_matrix_matches_rule_doc() {
        let mut w = TestWorld::new();
        let mut npc = |al| {
            let n = w.spawn_npc("n", Species::Human, 1, 1);
            w.set_alignment(n, al);
            n
        };
        let orc = npc(Alignment::Orc);
        let nrd = npc(Alignment::Nrd);
        let reb = npc(Alignment::Reb);
        let out = npc(Alignment::Out);
        let slave = npc(Alignment::Slave);
        let pirate = npc(Alignment::Pirate);
        let ass = npc(Alignment::Ass);
        assert_eq!(political_attitude(&w, nrd, orc), Attitude::Hostile);
        assert_eq!(political_attitude(&w, reb, nrd), Attitude::Neutral);
        assert_eq!(political_attitude(&w, reb, slave), Attitude::Friendly);
        assert_eq!(political_attitude(&w, out, reb), Attitude::Hostile);
        assert_eq!(political_attitude(&w, slave, orc), Attitude::Neutral);
        assert_eq!(political_attitude(&w, pirate, out), Attitude::Neutral);
        assert_eq!(political_attitude(&w, pirate, pirate), Attitude::Friendly);
        // Orc <-> Assassin: chapter dependent, hostile only in chapter 2.
        w.chapter = 1;
        assert_eq!(political_attitude(&w, orc, ass), Attitude::Friendly);
        assert_eq!(political_attitude(&w, ass, orc), Attitude::Friendly);
        w.chapter = 2;
        assert_eq!(political_attitude(&w, orc, ass), Attitude::Hostile);
        assert_eq!(political_attitude(&w, ass, orc), Attitude::Hostile);
        w.chapter = 4;
        assert_eq!(political_attitude(&w, orc, ass), Attitude::Friendly);
        assert_eq!(
            political_attitude(&w, EntityId::NONE, orc),
            Attitude::None,
            "no alignment, no attitude"
        );
    }

    #[test]
    fn pirate_takes_enclave_alignment() {
        let mut w = TestWorld::new();
        let pirate = w.spawn_npc("p", Species::Human, 1, 1);
        let orc = w.spawn_npc("o", Species::Human, 1, 1);
        w.set_alignment(pirate, Alignment::Pirate);
        w.set_alignment(orc, Alignment::Orc);
        let enclave = w.add_enclave(Alignment::Nrd);
        w.set_enclave(pirate, enclave);
        // Pirate in an Nrd enclave is hostile toward Orcs (Nrd row).
        assert_eq!(political_attitude(&w, pirate, orc), Attitude::Hostile);
    }

    #[test]
    fn guards_bosses_slaves() {
        let (mut w, a, b) = pair();
        let enc = w.add_enclave(Alignment::Reb);
        w.set_enclave(a, enc);
        w.set_npc_type(a, NpcType::Enclave);
        w.set_alignment(a, Alignment::Reb);
        assert!(is_guard(&w, a));
        w.set_alignment(a, Alignment::Nrd);
        assert!(!is_guard(&w, a));
        w.set_alignment(a, Alignment::Pirate);
        assert!(is_guard(&w, a), "pirates count as guards");
        assert!(!is_boss(&w, a));
        w.set_npc_type(a, NpcType::Political);
        assert!(is_boss(&w, a));
        assert!(!is_boss(&w, b), "no enclave");
        w.set_alignment(b, Alignment::Slave);
        assert!(is_slave(&w, b));
        w.set_enclave(b, enc);
        assert!(is_slave(&w, b));
        w.set_enclave_status(enc, EnclaveStatus::Flee);
        assert!(!is_slave(&w, b));
        w.set_enclave_status(enc, EnclaveStatus::Routine);
        w.set_enclave_switched(enc, true);
        assert!(!is_slave(&w, b));
        assert!(!is_gate_guard(&w, a) || w.guard_point(a).is_some());
        w.set_guard(a, GuardStatus::Active, EntityId(777));
        assert!(is_gate_guard(&w, a));
        w.set_guard(a, GuardStatus::Inactive, EntityId(777));
        assert!(!is_gate_guard(&w, a));
    }

    #[test]
    fn upset_windows() {
        let (mut w, a, _) = pair();
        let p = w.player();
        w.playing_time = 100.0;
        w.set_last_fight(a, 99.9, LastFight::Cancel);
        w.set_last_player_ar(a, AttackReason::Theft);
        assert!(is_upset(&w, a, p));
        w.playing_time = 100.5;
        assert!(!is_upset(&w, a, p), "theft: only 0.24 units");
        w.set_last_player_ar(a, AttackReason::Murder);
        assert!(is_upset(&w, a, p), "murder: 1.2 units");
        w.playing_time = 105.0;
        assert!(!is_upset(&w, a, p));
        w.set_last_player_ar(a, AttackReason::Kill);
        assert!(is_upset(&w, a, p), "kill never wears off");
        assert!(!is_upset(&w, a, EntityId(999)), "only toward the player");
        w.set_last_fight(a, 99.9, LastFight::Lost);
        assert!(!is_upset(&w, a, p), "only after a cancelled fight");
    }

    #[test]
    fn temporary_friend_and_fight_stamp() {
        let (mut w, a, _) = pair();
        let p = w.player();
        w.playing_time = 50.0;
        assert!(set_last_fight_timestamp(&mut w, a, p));
        assert_eq!(w.last_fight_timestamp(a), 50.0);
        w.set_last_fight(a, 50.0, LastFight::Friendly);
        w.playing_time = 50.5;
        assert!(is_temporary_friendly_to_player(&w, a));
        w.playing_time = 51.5;
        assert!(!is_temporary_friendly_to_player(&w, a));
        set_last_fight_timestamp(&mut w, a, EntityId(5));
        assert_eq!(w.last_fight_timestamp(a), 50.0, "only for the player");
    }

    #[test]
    fn ransack_once_not_mages() {
        let (mut w, a, b) = pair();
        assert!(want_to_ransack(&mut w, a, b));
        assert!(!want_to_ransack(&mut w, a, b), "once");
        let (mut w, a, b) = pair();
        w.set_class(a, NpcClass::Mage);
        assert!(!want_to_ransack(&mut w, a, b));
        let (mut w, a, _) = pair();
        let p = w.player();
        w.set_npc_type(a, NpcType::Friend);
        assert!(
            !want_to_ransack(&mut w, a, p),
            "friends do not rob the player"
        );
        w.set_npc_type(a, NpcType::Personal);
        assert!(want_to_ransack(&mut w, a, p));
    }

    #[test]
    fn attack_reasons() {
        let (mut w, a, b) = pair();
        w.set_attack_reason(a, AttackReason::Arena);
        assert!(has_attack_reason_to_watch(&w, a));
        w.set_attack_reason(a, AttackReason::Murder);
        assert!(!has_attack_reason_to_watch(&w, a));
        w.set_target(a, b);
        assert!(has_attack_reason_to_kill(&w, a, b));
        let p = w.player();
        assert!(
            !has_attack_reason_to_kill(&w, a, p),
            "not the current target"
        );
        w.set_attack_reason(a, AttackReason::Angry);
        assert!(!has_attack_reason_to_kill(&w, a, b));
    }

    #[test]
    fn gate_guard_reason_depends_on_zone() {
        let (mut w, a, _) = pair();
        let p = w.player();
        let zone = EntityId(500);
        w.set_zone_of(p, zone);
        w.set_attack_reason(a, AttackReason::GateGuard);
        w.set_target(a, p);
        // Unowned zone: friendly, so no reason to kill.
        assert!(!has_attack_reason_to_kill(&w, a, p));
        let enc = w.add_enclave(Alignment::Reb);
        w.set_zone(zone, enc, SecurityLevel::Political);
        w.set_political_fame(Alignment::Reb, 10);
        assert_eq!(zone_attitude_to_player(&w, zone), Attitude::Angry);
        assert!(has_attack_reason_to_kill(&w, a, p));
        w.set_political_fame(Alignment::Reb, 80);
        assert_eq!(zone_attitude_to_player(&w, zone), Attitude::Friendly);
        w.set_zone(zone, enc, SecurityLevel::Public);
        assert_eq!(zone_attitude_to_player(&w, zone), Attitude::Neutral);
        assert!(has_attack_reason_to_kill(&w, a, p));
        w.set_zone(zone, enc, SecurityLevel::Enclave);
        w.set_enclave_fame(enc, 30);
        assert_eq!(zone_attitude_to_player(&w, zone), Attitude::Angry);
    }

    #[test]
    fn fist_mode_and_droppable() {
        let (mut w, a, _) = pair();
        w.set_slot(a, Slot::RightHand, UseType::Fist, 0);
        assert!(is_in_fist_mode(&w, a));
        w.set_slot(a, Slot::RightHand, UseType::OneH, 3);
        assert!(!is_in_fist_mode(&w, a));
        assert!(is_droppable_weapon(&w, a, Slot::RightHand));
        w.set_slot(a, Slot::RightHand, UseType::Potion, 3);
        assert!(!is_droppable_weapon(&w, a, Slot::RightHand));
        w.set_slot(a, Slot::RightHand, UseType::OneH, 0);
        assert!(!is_droppable_weapon(&w, a, Slot::RightHand), "category 0");
    }
}
