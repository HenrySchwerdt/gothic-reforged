//! B02/B06: the attitude front-ends (`GetAttitude` and its helpers).
//!
//! `GetAttitude(a, b)` is "how does `a` regard `b`". Sources are tried in order and the first
//! non-`None` result wins: friend-type NPCs, flight, outlaws, revolution, the attitude toward the
//! player, the political matrix ([`crate::predicates::political_attitude`]) and finally the
//! species relations.

use crate::enums::{
    Alignment, Attitude, LastFight, NpcType, PartyMemberType, PlayerCrime, Species,
};
use crate::host::{EntityId, ScriptHost};
use crate::predicates::{is_boss, is_guard, is_humanoid, political_attitude, species_is_evil};

/// Player fame at or above which bosses and guards count the player as a friend.
pub const FRIEND_FAME: i32 = 75;

/// Port of `GetOutlawAttitude` (S:e900): an outlaw (`Out`) and a humanoid that is neither outlaw
/// nor pirate are hostile to each other (either direction).
pub fn outlaw_attitude(host: &dyn ScriptHost, a: EntityId, b: EntityId) -> Attitude {
    let outlaw_vs = |outlaw: EntityId, other: EntityId| {
        host.alignment(outlaw) == Alignment::Out
            && !matches!(host.alignment(other), Alignment::Out | Alignment::Pirate)
            && is_humanoid(host, other)
    };
    if outlaw_vs(a, b) || outlaw_vs(b, a) {
        Attitude::Hostile
    } else {
        Attitude::None
    }
}

/// Port of `GetSpeciesAttitude` (S:fbc0): relations by species when no political attitude
/// applies. Same species are friends; humanoids are hostile to evil beings and to the listed
/// predators and monsters (the Sabertooth named "Zuben" is neutral); each animal family has its
/// own stance toward humanoids and prey (see COMBAT_AI_RULES 10.4).
pub fn species_attitude(host: &dyn ScriptHost, a: EntityId, b: EntityId) -> Attitude {
    let (obs, tgt) = (host.species(a), host.species(b));
    if obs == Species::Sabertooth && host.name(a).eq_ignore_ascii_case("Zuben") {
        return Attitude::Neutral;
    }
    species_relation(obs, tgt)
}

/// Pure species table behind [`species_attitude`].
pub fn species_relation(obs: Species, tgt: Species) -> Attitude {
    use Attitude::*;
    use Species as S;
    if obs == tgt {
        return Friendly;
    }
    let hum_orc = matches!(tgt, S::Human | S::Orc);
    if matches!(obs, S::Human | S::Orc) {
        let hostile = species_is_evil(tgt)
            || matches!(
                tgt,
                S::Goblin
                    | S::Troll
                    | S::Minecrawler
                    | S::Wolf
                    | S::Boar
                    | S::Sabertooth
                    | S::Shadowbeast
                    | S::Ripper
                    | S::Lurker
                    | S::Varan
                    | S::Snapper
                    | S::Alligator
                    | S::Trex
                    | S::FireVaran
                    | S::SwampLurker
                    | S::Ogre
                    | S::Stalker
                    | S::Dragon
            );
        return if hostile { Hostile } else { Neutral };
    }
    match obs {
        S::Goblin if hum_orc => Hostile,
        S::Goblin if matches!(tgt, S::Troll | S::Ogre | S::Stalker) => Friendly,
        S::Goblin if matches!(tgt, S::Lurker | S::SwampLurker) => Angry,
        S::Troll | S::Ogre | S::Stalker if hum_orc => Hostile,
        S::Troll | S::Ogre | S::Stalker if tgt == S::Goblin => Friendly,
        S::Minecrawler | S::Boar if hum_orc => Hostile,
        S::Scavenger | S::Rhino | S::Varan | S::Alligator | S::FireVaran | S::Bloodfly
            if hum_orc =>
        {
            Angry
        }
        S::Wolf | S::Sabertooth | S::Shadowbeast | S::Ripper | S::Snapper if hum_orc => Angry,
        S::Wolf | S::Sabertooth | S::Shadowbeast | S::Ripper | S::Snapper
            if matches!(tgt, S::Scavenger | S::Boar) =>
        {
            Predator
        }
        S::Lurker | S::SwampLurker if tgt == S::Human => Hostile,
        S::Lurker | S::SwampLurker if tgt == S::Goblin => Predator,
        S::Trex | S::Dragon if hum_orc => Angry,
        S::Trex | S::Dragon if matches!(tgt, S::Bison | S::Rhino) => Predator,
        S::Deer if hum_orc => Panic,
        _ if species_is_evil(obs) && species_is_evil(tgt) => Friendly,
        _ if species_is_evil(obs) && hum_orc => Hostile,
        _ => Neutral,
    }
}

/// Port of `GetAttitudeToPlayer` (S:ef00): how `npc` regards the player.
///
/// Friends and the player's party members are Friendly; a Controlled member of someone else is
/// Neutral and a foreign Summoned creature Hostile; a transformed NPC has no opinion. An NPC
/// whose faction the player is wanted by is Hostile. Bosses compare the player's political fame
/// with 75 (Friendly, else Neutral). Guards read the enclave's known crime (Murder Hostile, Theft
/// Angry) and otherwise its player fame. Everybody else remembers its own crime (Murder Hostile,
/// Theft Angry) and lost or cancelled fights (Angry). `None` means "no opinion, ask the
/// political matrix".
pub fn attitude_to_player(host: &dyn ScriptHost, npc: EntityId) -> Attitude {
    let player = host.player();
    if npc == player || host.npc_type(npc) == NpcType::Friend {
        return Attitude::Friendly;
    }
    if host.party_leader(npc) == player {
        return Attitude::Friendly;
    }
    match host.party_member_type(npc) {
        PartyMemberType::Controlled => return Attitude::Neutral,
        PartyMemberType::Summoned => return Attitude::Hostile,
        _ => {}
    }
    if host.is_transformed(npc) {
        return Attitude::None;
    }
    let enclave = host.enclave(npc);
    let mut alignment = host.alignment(npc);
    if alignment == Alignment::Pirate && enclave.is_some() {
        alignment = host.enclave_alignment(enclave);
    }
    if host.political_player_crime(alignment) {
        return Attitude::Hostile;
    }
    if is_boss(host, npc) {
        return if host.political_fame(alignment) < FRIEND_FAME {
            Attitude::Neutral
        } else {
            Attitude::Friendly
        };
    }
    if is_guard(host, npc) {
        return match host.enclave_known_crime(enclave) {
            PlayerCrime::Murder => Attitude::Hostile,
            PlayerCrime::Theft => Attitude::Angry,
            _ if host.enclave_player_fame(enclave) < FRIEND_FAME => Attitude::Neutral,
            _ => Attitude::Friendly,
        };
    }
    match host.last_player_crime(npc) {
        PlayerCrime::Murder => return Attitude::Hostile,
        PlayerCrime::Theft => return Attitude::Angry,
        _ => {}
    }
    if matches!(
        host.last_fight_vs_player(npc),
        LastFight::Lost | LastFight::Cancel
    ) {
        return Attitude::Angry;
    }
    Attitude::None
}

/// Port of `GetAttitude` (S:e3b0): how `a` regards `b`.
///
/// `None` when either entity is missing or `a` is the player (the player has no attitudes). In
/// order: friend-type NPCs mirror how `b` stands with the player (Friendly toward their leader);
/// an NPC fleeing from `b` panics; outlaws; revolution (host hook, `GetRevolutionAttitude`
/// S:ea50 is not ported); members of the same party are Friendly; a Controlled member is Neutral
/// to humanoids; toward the player [`attitude_to_player`]; then the political matrix and the
/// species relations.
pub fn get_attitude(host: &dyn ScriptHost, a: EntityId, b: EntityId) -> Attitude {
    let player = host.player();
    if a.is_none() || b.is_none() || a == player || !host.exists(a) || !host.exists(b) {
        return Attitude::None;
    }
    if host.npc_type(a) == NpcType::Friend {
        let toward_player = get_attitude(host, b, player);
        if host.party_leader(a) == b || toward_player == Attitude::Friendly {
            return Attitude::Friendly;
        }
        if toward_player == Attitude::Hostile {
            return Attitude::Hostile;
        }
    }
    if host.ai_mode(a) == crate::enums::AiMode::Flee && host.current_target(a) == b {
        return Attitude::Panic;
    }
    let outlaw = outlaw_attitude(host, a, b);
    if outlaw != Attitude::None {
        return outlaw;
    }
    let revolution = host.revolution_attitude(a, b);
    if revolution != Attitude::None {
        return revolution;
    }
    if host.party_leader(a) == b || (host.party_leader(b) == a && b != player) {
        return Attitude::Friendly;
    }
    if host.party_member_type(a) == PartyMemberType::Controlled && is_humanoid(host, b) {
        return Attitude::Neutral;
    }
    if b == player {
        let own = attitude_to_player(host, a);
        if own != Attitude::None {
            return own;
        }
    }
    let political = political_attitude(host, a, b);
    if political != Attitude::None {
        return political;
    }
    species_attitude(host, a, b)
}

/// Port of `GetFocusNameAttitude` (S:f340): the attitude shown on the focus name of `npc`, i.e.
/// how the NPC regards the player [I: the argument order of the inner call is not recoverable].
pub fn focus_name_attitude(host: &dyn ScriptHost, npc: EntityId) -> Attitude {
    get_attitude(host, npc, host.player())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{AiMode, NpcType};
    #[allow(unused_imports)]
    use crate::host::{EntityApi, InventoryApi, NpcApi, ScriptCtx, StatsApi, WorldApi};
    use crate::testhost::TestWorld;

    #[test]
    fn species_table_matches_the_rule_doc() {
        use Attitude::*;
        use Species::*;
        assert_eq!(species_relation(Human, Wolf), Hostile);
        assert_eq!(species_relation(Human, Deer), Neutral);
        assert_eq!(species_relation(Human, Zombie), Hostile);
        assert_eq!(species_relation(Wolf, Human), Angry);
        assert_eq!(species_relation(Wolf, Boar), Predator);
        assert_eq!(species_relation(Wolf, Wolf), Friendly);
        assert_eq!(species_relation(Deer, Orc), Panic);
        assert_eq!(species_relation(Goblin, Troll), Friendly);
        assert_eq!(species_relation(Goblin, Lurker), Angry);
        assert_eq!(species_relation(Lurker, Goblin), Predator);
        assert_eq!(species_relation(Trex, Rhino), Predator);
        assert_eq!(species_relation(Golem, Skeleton), Friendly);
        assert_eq!(species_relation(Golem, Human), Hostile);
        assert_eq!(species_relation(Rabbit, Human), Neutral);
        assert_eq!(species_relation(Minecrawler, Orc), Hostile);
        assert_eq!(species_relation(Bloodfly, Human), Angry);
    }

    #[test]
    fn zuben_the_sabertooth_is_neutral() {
        let mut w = TestWorld::new();
        let zuben = w.spawn_npc("Zuben", Species::Sabertooth, 5, 5);
        let hero = w.spawn_npc("Man", Species::Human, 5, 5);
        assert_eq!(species_attitude(&w, zuben, hero), Attitude::Neutral);
        let other = w.spawn_npc("Cat", Species::Sabertooth, 5, 5);
        assert_eq!(species_attitude(&w, other, hero), Attitude::Angry);
    }

    #[test]
    fn outlaws_and_humanoids_hate_each_other() {
        let mut w = TestWorld::new();
        let out = w.spawn_npc("Bandit", Species::Human, 5, 5);
        let man = w.spawn_npc("Man", Species::Human, 5, 5);
        let wolf = w.spawn_npc("Wolf", Species::Wolf, 5, 5);
        let pirate = w.spawn_npc("Pirate", Species::Human, 5, 5);
        w.set_alignment(out, Alignment::Out);
        w.set_alignment(man, Alignment::Nrd);
        w.set_alignment(pirate, Alignment::Pirate);
        assert_eq!(outlaw_attitude(&w, out, man), Attitude::Hostile);
        assert_eq!(outlaw_attitude(&w, man, out), Attitude::Hostile);
        assert_eq!(outlaw_attitude(&w, out, pirate), Attitude::None);
        assert_eq!(outlaw_attitude(&w, out, wolf), Attitude::None);
        assert_eq!(outlaw_attitude(&w, man, pirate), Attitude::None);
    }

    #[test]
    fn attitude_to_player_rules() {
        let mut w = TestWorld::new();
        let p = w.player();
        let civ = w.spawn_npc("Civ", Species::Human, 5, 5);
        assert_eq!(attitude_to_player(&w, p), Attitude::Friendly);
        assert_eq!(attitude_to_player(&w, civ), Attitude::None);
        // Own crime memory.
        w.set_last_player_crime(civ, PlayerCrime::Theft);
        assert_eq!(attitude_to_player(&w, civ), Attitude::Angry);
        w.set_last_player_crime(civ, PlayerCrime::Murder);
        assert_eq!(attitude_to_player(&w, civ), Attitude::Hostile);
        w.set_last_player_crime(civ, PlayerCrime::None);
        w.set_last_fight(civ, 1.0, LastFight::Lost);
        assert_eq!(attitude_to_player(&w, civ), Attitude::Angry);
        // Party.
        w.set_last_fight(civ, 0.0, LastFight::None);
        w.set_leader(civ, p, PartyMemberType::Party, false);
        assert_eq!(attitude_to_player(&w, civ), Attitude::Friendly);
        // Foreign summons and controlled followers.
        let sum = w.spawn_npc("Sum", Species::Skeleton, 5, 5);
        w.set_leader(sum, EntityId::NONE, PartyMemberType::Summoned, false);
        assert_eq!(attitude_to_player(&w, sum), Attitude::Hostile);
        let thrall = w.spawn_npc("Thrall", Species::Human, 5, 5);
        w.set_leader(thrall, civ, PartyMemberType::Controlled, false);
        assert_eq!(attitude_to_player(&w, thrall), Attitude::Neutral);
        // Friends.
        let friend = w.spawn_npc("Friend", Species::Human, 5, 5);
        w.set_npc_type(friend, NpcType::Friend);
        assert_eq!(attitude_to_player(&w, friend), Attitude::Friendly);
    }

    #[test]
    fn bosses_and_guards_use_fame_and_enclave_crime() {
        let mut w = TestWorld::new();
        let enc = w.add_enclave(Alignment::Reb);
        let boss = w.spawn_npc("Boss", Species::Human, 5, 5);
        w.set_alignment(boss, Alignment::Reb);
        w.set_enclave(boss, enc);
        w.set_npc_type(boss, NpcType::Political);
        w.set_political_fame(Alignment::Reb, 74);
        assert_eq!(attitude_to_player(&w, boss), Attitude::Neutral);
        w.set_political_fame(Alignment::Reb, 75);
        assert_eq!(attitude_to_player(&w, boss), Attitude::Friendly);
        let guard = w.spawn_npc("Guard", Species::Human, 5, 5);
        w.set_alignment(guard, Alignment::Reb);
        w.set_enclave(guard, enc);
        w.set_npc_type(guard, NpcType::Enclave);
        w.set_enclave_fame(enc, 50);
        assert_eq!(attitude_to_player(&w, guard), Attitude::Neutral);
        w.set_enclave_fame(enc, 90);
        assert_eq!(attitude_to_player(&w, guard), Attitude::Friendly);
        w.set_enclave_known_crime(enc, PlayerCrime::Theft);
        assert_eq!(attitude_to_player(&w, guard), Attitude::Angry);
        w.set_enclave_known_crime(enc, PlayerCrime::Murder);
        assert_eq!(attitude_to_player(&w, guard), Attitude::Hostile);
        // Wanted by the faction: everybody of it is hostile.
        let civ = w.spawn_npc("Civ", Species::Human, 5, 5);
        w.set_alignment(civ, Alignment::Reb);
        w.set_political_player_crime(Alignment::Reb, true);
        assert_eq!(attitude_to_player(&w, civ), Attitude::Hostile);
    }

    #[test]
    fn get_attitude_order() {
        let mut w = TestWorld::new();
        let p = w.player();
        let a = w.spawn_npc("A", Species::Human, 5, 5);
        let b = w.spawn_npc("B", Species::Human, 5, 5);
        w.set_alignment(a, Alignment::Nrd);
        w.set_alignment(b, Alignment::Orc);
        assert_eq!(get_attitude(&w, a, b), Attitude::Hostile, "political");
        assert_eq!(get_attitude(&w, a, EntityId::NONE), Attitude::None);
        assert_eq!(
            get_attitude(&w, p, a),
            Attitude::None,
            "the player has none"
        );
        // Species fallback without alignments.
        let wolf = w.spawn_npc("Wolf", Species::Wolf, 5, 5);
        let man = w.spawn_npc("Man", Species::Human, 5, 5);
        assert_eq!(get_attitude(&w, man, wolf), Attitude::Hostile);
        assert_eq!(get_attitude(&w, wolf, man), Attitude::Angry);
        // Toward the player the personal opinion comes first.
        w.set_last_player_crime(a, PlayerCrime::Theft);
        assert_eq!(get_attitude(&w, a, p), Attitude::Angry);
        // Flight beats everything.
        w.set_ai_mode(a, AiMode::Flee);
        w.set_current_target(a, b);
        assert_eq!(get_attitude(&w, a, b), Attitude::Panic);
        w.set_ai_mode(a, AiMode::Routine);
        // Party members are friends, controlled ones neutral to humanoids.
        w.set_leader(man, b, PartyMemberType::Party, false);
        assert_eq!(get_attitude(&w, man, b), Attitude::Friendly);
        w.set_leader(man, EntityId::NONE, PartyMemberType::Controlled, false);
        assert_eq!(get_attitude(&w, man, a), Attitude::Neutral);
        // Revolution hook has priority over the party and political sources.
        w.set_revolution_attitude(a, Attitude::Hostile);
        assert_eq!(get_attitude(&w, a, man), Attitude::Hostile);
    }

    #[test]
    fn friend_type_mirrors_the_players_standing() {
        let mut w = TestWorld::new();
        let p = w.player();
        let friend = w.spawn_npc("Friend", Species::Human, 5, 5);
        let foe = w.spawn_npc("Foe", Species::Human, 5, 5);
        let pal = w.spawn_npc("Pal", Species::Human, 5, 5);
        w.set_npc_type(friend, NpcType::Friend);
        w.set_last_player_crime(foe, PlayerCrime::Murder);
        assert_eq!(get_attitude(&w, friend, foe), Attitude::Hostile);
        w.set_leader(pal, p, PartyMemberType::Party, false);
        assert_eq!(get_attitude(&w, friend, pal), Attitude::Friendly);
        assert_eq!(focus_name_attitude(&w, foe), Attitude::Hostile);
    }
}
