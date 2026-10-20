//! Attitudes between actors: political matrix, species relations, attitude to the player
//! [CA 10.4]. `GetAttitude` (S:e3b0) tries its sources in order and the first non-zero
//! result wins; the revolution source (`GetRevolutionAttitude`) is not recovered [CA 10.4, U].
use crate::actor::{Actor, FightVsPlayer};
use crate::config::SimConfig;
use crate::crime::Enclave;
use crate::types::*;
use std::collections::BTreeMap;

/// Fame at which a guard or boss counts the player as a friend [CA GAME_RULES header: 75].
pub const FRIEND_FAME: i32 = 75;

/// Matrix cell: F friendly, N neutral, H hostile, C chapter dependent, `-` none.
const MATRIX: [[char; 9]; 9] = [
    // columns: Orc Nrd Reb Mid Ass Nom Out Slave Pirate
    ['F', 'H', 'H', 'N', 'C', 'H', 'H', 'N', 'F'], // Orc
    ['H', 'F', 'N', 'N', 'H', 'N', 'H', 'N', 'F'], // Nrd
    ['H', 'N', 'F', 'N', 'H', 'N', 'H', 'F', 'F'], // Reb
    ['N', 'N', 'N', 'F', 'N', 'N', 'N', 'N', 'F'], // Mid
    ['C', 'H', 'H', 'N', 'F', 'H', 'H', 'N', 'F'], // Ass
    ['H', 'N', 'N', 'N', 'H', 'F', 'H', 'N', 'F'], // Nom
    ['H', 'H', 'H', 'H', 'H', 'H', 'F', 'H', 'N'], // Out
    ['N', 'N', 'N', 'N', 'N', 'N', 'N', 'N', 'N'], // Slave
    ['N', 'N', 'N', 'N', 'N', 'N', 'N', 'N', 'N'], // Pirate vs humanoids
];

fn index(a: Alignment) -> Option<usize> {
    Some(match a {
        Alignment::Orc => 0,
        Alignment::Nrd => 1,
        Alignment::Reb => 2,
        Alignment::Mid => 3,
        Alignment::Ass => 4,
        Alignment::Nom => 5,
        Alignment::Out => 6,
        Alignment::Slave => 7,
        Alignment::Pirate => 8,
        Alignment::None => return None,
    })
}

/// Political attitude of `observer` towards `target` [CA 10.4 matrix]. `None` when either
/// side has no alignment. The Orc/Assassin cell is friendly except in one chapter
/// (`orc_ass_hostile_chapter`, 2 in the shipped script; the script additionally exempts two
/// named NPCs that the decompiler did not preserve [U]).
pub fn political_attitude(
    cfg: &SimConfig,
    chapter: u32,
    observer: Alignment,
    target: Alignment,
) -> Attitude {
    let (Some(r), Some(c)) = (index(observer), index(target)) else {
        return Attitude::None;
    };
    match MATRIX[r][c] {
        'F' => Attitude::Friendly,
        'N' => Attitude::Neutral,
        'H' => Attitude::Hostile,
        'C' => {
            if chapter == cfg.orc_ass_hostile_chapter {
                Attitude::Hostile
            } else {
                Attitude::Friendly
            }
        }
        _ => Attitude::None,
    }
}

/// `GetSpeciesAttitude` (S:fbc0) [CA 10.4].
pub fn species_attitude(obs: Species, tgt: Species) -> Attitude {
    use Attitude::*;
    use Species::*;
    if obs == tgt {
        return Friendly;
    }
    let hum_orc = tgt.is_humanoid();
    if obs.is_humanoid() {
        let hostile_list = matches!(
            tgt,
            Goblin
                | Troll
                | Minecrawler
                | Wolf
                | Boar
                | Sabertooth
                | Shadowbeast
                | Ripper
                | Lurker
                | Varan
                | Snapper
                | Alligator
                | Trex
                | FireVaran
                | SwampLurker
                | Ogre
                | Stalker
                | Dragon
        );
        return if tgt.is_evil() || hostile_list {
            Hostile
        } else {
            Neutral
        };
    }
    match obs {
        Goblin if hum_orc => Hostile,
        Goblin if matches!(tgt, Troll | Ogre | Stalker) => Friendly,
        Goblin if matches!(tgt, Lurker | SwampLurker) => Angry,
        Troll | Ogre | Stalker if hum_orc => Hostile,
        Troll | Ogre | Stalker if tgt == Goblin => Friendly,
        Minecrawler | Boar if hum_orc => Hostile,
        Scavenger | Rhino | Varan | Alligator | FireVaran | Bloodfly if hum_orc => Angry,
        Wolf | Sabertooth | Shadowbeast | Ripper | Snapper if hum_orc => Angry,
        Wolf | Sabertooth | Shadowbeast | Ripper | Snapper if matches!(tgt, Scavenger | Boar) => {
            Predator
        }
        Lurker | SwampLurker if tgt == Human => Hostile,
        Lurker | SwampLurker if tgt == Goblin => Predator,
        Trex | Dragon if hum_orc => Angry,
        Trex | Dragon if matches!(tgt, Bison | Rhino) => Predator,
        Deer if hum_orc => Panic,
        _ if obs.is_evil() && tgt.is_evil() => Friendly,
        _ if obs.is_evil() && hum_orc => Hostile,
        _ => Neutral,
    }
}

/// Everything `GetAttitude` needs besides the two actors.
pub struct AttitudeCtx<'a> {
    pub cfg: &'a SimConfig,
    pub chapter: u32,
    /// `GameState::fame` (`pol:<tag>` and `enc:<name>` keys).
    pub fame: &'a BTreeMap<String, i32>,
    pub enclaves: &'a BTreeMap<String, Enclave>,
    pub player_alignment: Alignment,
}

impl AttitudeCtx<'_> {
    pub fn political_fame(&self, a: Alignment) -> i32 {
        self.fame
            .get(&format!("pol:{}", a.tag()))
            .copied()
            .unwrap_or(0)
    }
    pub fn enclave_fame(&self, name: &str) -> i32 {
        self.fame
            .get(&format!("enc:{}", name.to_ascii_lowercase()))
            .copied()
            .unwrap_or(0)
    }
}

/// `GetAttitudeToPlayer` (S:ef00) [CA 10.4]. `None` when the NPC has no opinion of its own.
pub fn attitude_to_player(ctx: &AttitudeCtx<'_>, npc: &Actor, player: &Actor) -> Attitude {
    if npc.npc_type == NpcType::Friend || npc.party_leader == Some(player.id) {
        return Attitude::Friendly;
    }
    let fame_att = |fame: i32| {
        if fame >= FRIEND_FAME {
            Attitude::Friendly
        } else {
            Attitude::Neutral
        }
    };
    if npc.boss {
        return fame_att(ctx.political_fame(npc.alignment));
    }
    if npc.is_guard() {
        let enclave = npc.enclave.as_deref();
        let known = enclave
            .and_then(|e| ctx.enclaves.get(e))
            .map_or(Crime::None, |e| e.known_crime);
        return match known {
            Crime::Murder => Attitude::Hostile,
            Crime::Theft => Attitude::Angry,
            _ => fame_att(match enclave {
                Some(e) => ctx.enclave_fame(e),
                None => ctx.political_fame(npc.alignment),
            }),
        };
    }
    match npc.last_player_crime {
        Crime::Murder => return Attitude::Hostile,
        Crime::Theft => return Attitude::Angry,
        _ => {}
    }
    if matches!(
        npc.last_fight_vs_player,
        FightVsPlayer::Lost | FightVsPlayer::Cancel
    ) {
        return Attitude::Angry;
    }
    npc.base_attitude_to_player
}

/// `GetAttitude(observer -> target)` (S:e3b0) [CA 10.4].
pub fn get_attitude(
    ctx: &AttitudeCtx<'_>,
    observer: &Actor,
    target: &Actor,
    player: &Actor,
) -> Attitude {
    if observer.id == target.id {
        return Attitude::Friendly;
    }
    // 1. friend-type / party.
    if target.is_player && (observer.npc_type == NpcType::Friend || observer.summoned) {
        return Attitude::Friendly;
    }
    if observer.party_leader.is_some() && observer.party_leader == target.party_leader {
        return Attitude::Friendly;
    }
    // 2. already fleeing from this target.
    if observer.ai_mode == AiMode::Flee && observer.target == Some(target.id) {
        return Attitude::Panic;
    }
    // 3. outlaws.
    if observer.alignment == Alignment::Out
        && target.species.is_humanoid()
        && !matches!(target.alignment, Alignment::Out | Alignment::Pirate)
    {
        return Attitude::Hostile;
    }
    // 4. revolution [U].
    // 5. attitude to the player.
    if target.is_player {
        let a = attitude_to_player(ctx, observer, player);
        if a != Attitude::None {
            return a;
        }
    }
    // 6. political.
    let (oa, ta) = (
        observer.alignment,
        if target.is_player {
            ctx.player_alignment
        } else {
            target.alignment
        },
    );
    if observer.species.is_humanoid() && target.species.is_humanoid() {
        let p = political_attitude(ctx.cfg, ctx.chapter, oa, ta);
        if p != Attitude::None {
            return p;
        }
    }
    // 7. species.
    species_attitude(observer.species, target.species)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crime::Enclave;

    fn cfg() -> SimConfig {
        SimConfig::default()
    }

    #[test]
    fn political_matrix_cells() {
        let c = cfg();
        let p = |o, t| political_attitude(&c, 1, o, t);
        assert_eq!(p(Alignment::Orc, Alignment::Orc), Attitude::Friendly);
        assert_eq!(p(Alignment::Orc, Alignment::Nrd), Attitude::Hostile);
        assert_eq!(p(Alignment::Orc, Alignment::Mid), Attitude::Neutral);
        assert_eq!(p(Alignment::Reb, Alignment::Slave), Attitude::Friendly);
        assert_eq!(p(Alignment::Out, Alignment::Out), Attitude::Friendly);
        assert_eq!(p(Alignment::Out, Alignment::Nom), Attitude::Hostile);
        assert_eq!(p(Alignment::Slave, Alignment::Orc), Attitude::Neutral);
        assert_eq!(p(Alignment::Mid, Alignment::Pirate), Attitude::Friendly);
        assert_eq!(p(Alignment::None, Alignment::Orc), Attitude::None);
        // Orc <-> Assassin: hostile only in chapter 2.
        assert_eq!(p(Alignment::Orc, Alignment::Ass), Attitude::Friendly);
        assert_eq!(
            political_attitude(&c, 2, Alignment::Ass, Alignment::Orc),
            Attitude::Hostile
        );
        assert_eq!(
            political_attitude(&c, 2, Alignment::Orc, Alignment::Ass),
            Attitude::Hostile
        );
        assert_eq!(
            political_attitude(&c, 4, Alignment::Ass, Alignment::Orc),
            Attitude::Friendly
        );
    }

    #[test]
    fn species_relations() {
        use Species::*;
        assert_eq!(species_attitude(Human, Skeleton), Attitude::Hostile);
        assert_eq!(species_attitude(Human, Wolf), Attitude::Hostile);
        assert_eq!(species_attitude(Human, Orc), Attitude::Neutral);
        assert_eq!(species_attitude(Wolf, Human), Attitude::Angry);
        assert_eq!(species_attitude(Wolf, Boar), Attitude::Predator);
        assert_eq!(species_attitude(Goblin, Troll), Attitude::Friendly);
        assert_eq!(species_attitude(Deer, Human), Attitude::Panic);
        assert_eq!(species_attitude(Skeleton, Zombie), Attitude::Friendly);
        assert_eq!(species_attitude(Skeleton, Human), Attitude::Hostile);
        assert_eq!(species_attitude(Trex, Rhino), Attitude::Predator);
        assert_eq!(species_attitude(Meatbug, Human), Attitude::Neutral);
        assert_eq!(species_attitude(Wolf, Wolf), Attitude::Friendly);
    }

    #[test]
    fn guard_attitude_follows_enclave_crime_and_fame() {
        let c = cfg();
        let mut enclaves = BTreeMap::new();
        enclaves.insert("trelis".to_string(), Enclave::default());
        let mut fame = BTreeMap::new();
        let hero = Actor::hero(ActorId(0), &c);
        let mut guard = Actor::npc(ActorId(1), "Guard", Species::Human, 20, 20);
        guard.npc_type = NpcType::Enclave;
        guard.alignment = Alignment::Mid;
        guard.enclave = Some("trelis".into());
        let at = |fame: &BTreeMap<String, i32>, enclaves: &BTreeMap<String, Enclave>| {
            attitude_to_player(
                &AttitudeCtx {
                    cfg: &c,
                    chapter: 1,
                    fame,
                    enclaves,
                    player_alignment: Alignment::Mid,
                },
                &guard,
                &hero,
            )
        };
        assert_eq!(at(&fame, &enclaves), Attitude::Neutral);
        fame.insert("enc:trelis".into(), 75);
        assert_eq!(at(&fame, &enclaves), Attitude::Friendly);
        enclaves.get_mut("trelis").unwrap().known_crime = Crime::Theft;
        assert_eq!(at(&fame, &enclaves), Attitude::Angry);
        enclaves.get_mut("trelis").unwrap().known_crime = Crime::Murder;
        assert_eq!(at(&fame, &enclaves), Attitude::Hostile);
    }

    #[test]
    fn get_attitude_order() {
        let c = cfg();
        let fame = BTreeMap::new();
        let enclaves = BTreeMap::new();
        let ctx = AttitudeCtx {
            cfg: &c,
            chapter: 1,
            fame: &fame,
            enclaves: &enclaves,
            player_alignment: Alignment::Reb,
        };
        let hero = Actor::hero(ActorId(0), &c);
        let mut orc = Actor::npc(ActorId(1), "Orc", Species::Orc, 10, 10);
        orc.alignment = Alignment::Orc;
        // Political attitude of an Orc to a Rebel player: Hostile.
        assert_eq!(get_attitude(&ctx, &orc, &hero, &hero), Attitude::Hostile);
        // A thief outlaw is hostile to humanoids of other guilds.
        let mut out = Actor::npc(ActorId(2), "Bandit", Species::Human, 10, 10);
        out.alignment = Alignment::Out;
        assert_eq!(get_attitude(&ctx, &out, &hero, &hero), Attitude::Hostile);
        // Crime memory beats politics: a Rebel NPC that remembers a theft is Angry.
        let mut reb = Actor::npc(ActorId(3), "Reb", Species::Human, 10, 10);
        reb.alignment = Alignment::Reb;
        assert_eq!(get_attitude(&ctx, &reb, &hero, &hero), Attitude::Friendly);
        reb.last_player_crime = Crime::Theft;
        assert_eq!(get_attitude(&ctx, &reb, &hero, &hero), Attitude::Angry);
        // Fleeing observers are in Panic against their target.
        reb.ai_mode = AiMode::Flee;
        reb.target = Some(hero.id);
        assert_eq!(get_attitude(&ctx, &reb, &hero, &hero), Attitude::Panic);
        // Monsters: wolf vs player.
        let wolf = Actor::npc(ActorId(4), "Wolf", Species::Wolf, 5, 5);
        assert_eq!(get_attitude(&ctx, &wolf, &hero, &hero), Attitude::Angry);
    }
}
