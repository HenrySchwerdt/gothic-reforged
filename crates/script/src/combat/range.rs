//! Distances of the combat AI: attack reach, close range, move lengths, walk modes.

use crate::enums::{Action, AttackReason, Species};
use crate::host::{EntityId, ScriptHost};
use crate::predicates::is_in_fist_mode;

/// Port of `GetCombatMoveLength` (S:aa270): how far a move reaches beyond the species' base
/// range, or `None` where the DLL returns -1 (the caller then uses 120).
///
/// Zombies reach 150 with the plain attack; plain attack, simple whirl, pierce, hack and
/// get-up attack 120; power, sprint and whirl 180; the three quick attacks 60; the stumble
/// family 100.
pub fn combat_move_length(species: Species, mv: Action) -> Option<i32> {
    if species == Species::Zombie && mv == Action::Attack {
        return Some(150);
    }
    use Action as A;
    match mv {
        A::Attack | A::SimpleWhirl | A::PierceAttack | A::HackAttack | A::GetUpAttack => Some(120),
        A::PowerAttack | A::SprintAttack | A::WhirlAttack => Some(180),
        A::QuickAttack | A::QuickAttackR | A::QuickAttackL => Some(60),
        A::QuickParadeStumble
        | A::ParadeStumble
        | A::ParadeStumbleR
        | A::ParadeStumbleL
        | A::HeavyParadeStumble
        | A::QuickStumble
        | A::Stumble
        | A::StumbleR
        | A::StumbleL
        | A::PierceStumble => Some(100),
        _ => None,
    }
}

/// Base reach of a species in centimetres (the first half of [`attack_range`]).
fn base_reach(host: &dyn ScriptHost, attacker: EntityId, mv: Action) -> i32 {
    let species = host.species(attacker);
    if !host.setup().ai_mode || species == Species::Gargoyle {
        return if matches!(species, Species::Troll | Species::Trex | Species::Dragon) {
            300
        } else {
            200
        };
    }
    use Species as S;
    match species {
        S::Golem | S::FireGolem | S::IceGolem | S::Bloodfly => 110,
        S::Goblin => 140,
        S::Troll | S::Trex | S::Dragon => 300,
        S::Minecrawler | S::Wolf | S::Ripper => 170,
        S::Scavenger => 115,
        S::Boar => 180,
        S::Sabertooth | S::Shadowbeast | S::Varan | S::Snapper | S::Alligator | S::Gargoyle => 200,
        S::Bison | S::Rhino => 150,
        S::Ogre => 220,
        S::ScorpionKing => 120,
        S::Lurker | S::SwampLurker => {
            if mv == Action::QuickAttack {
                200
            } else {
                150
            }
        }
        _ => {
            if is_in_fist_mode(host, attacker) {
                50
            } else {
                200
            }
        }
    }
}

/// Port of `GetAttackRange` (S:4e940): distance within which `attacker` reaches `victim` with
/// `mv`: species base plus the move length (120 when the move has none).
pub fn attack_range(
    host: &dyn ScriptHost,
    attacker: EntityId,
    _victim: EntityId,
    mv: Action,
) -> i32 {
    let species = host.species(attacker);
    let len = combat_move_length(species, mv).unwrap_or(120);
    base_reach(host, attacker, mv) + len
}

/// Port of `GetCloseRange` (S:4eb70): stop distance of the approach (600 for the big
/// monsters, else 500).
pub fn close_range(host: &dyn ScriptHost, npc: EntityId) -> i32 {
    if matches!(
        host.species(npc),
        Species::Troll | Species::Trex | Species::Dragon
    ) {
        600
    } else {
        500
    }
}

/// Port of `GetCombatWalkMode` (S:1b060): how fast the NPC closes in (1 slow, 2 walk, 3 run).
pub fn combat_walk_mode(host: &dyn ScriptHost, npc: EntityId) -> u8 {
    if host.attack_reason(npc) == AttackReason::Prey {
        return 3;
    }
    use Species as S;
    match host.species(npc) {
        S::Zombie => 1,
        S::Scavenger | S::Boar | S::Sabertooth | S::Rhino | S::Snapper => 3,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testhost::TestWorld;

    #[test]
    fn ranges_follow_species_and_move() {
        let mut w = TestWorld::new();
        let orc = w.spawn_npc("Orc", Species::Orc, 10, 30);
        let troll = w.spawn_npc("Troll", Species::Troll, 10, 30);
        let lurker = w.spawn_npc("Lurker", Species::Lurker, 10, 30);
        let zombie = w.spawn_npc("Zombie", Species::Zombie, 10, 30);
        let t = EntityId::NONE;
        assert_eq!(attack_range(&w, orc, t, Action::Attack), 320);
        assert_eq!(attack_range(&w, orc, t, Action::PowerAttack), 380);
        assert_eq!(attack_range(&w, orc, t, Action::QuickAttack), 260);
        assert_eq!(
            attack_range(&w, orc, t, Action::Fwd),
            320,
            "no length means 120"
        );
        assert_eq!(attack_range(&w, troll, t, Action::Attack), 420);
        assert_eq!(attack_range(&w, lurker, t, Action::QuickAttack), 260);
        assert_eq!(attack_range(&w, lurker, t, Action::Attack), 270);
        assert_eq!(attack_range(&w, zombie, t, Action::Attack), 350);
        assert_eq!(close_range(&w, troll), 600);
        assert_eq!(close_range(&w, orc), 500);
        assert_eq!(combat_walk_mode(&w, zombie), 1);
        assert_eq!(combat_walk_mode(&w, orc), 2);
    }
}
