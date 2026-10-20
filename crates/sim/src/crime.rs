//! Crime bookkeeping and the reaction mapping [CA 10.2, 10.5].
//!
//! Crimes are remembered per NPC (`LastPlayerCrime`) and per enclave (`KnownPlayerCrime`,
//! crime counters). Witnessing a crime is turned into a [`Reaction`] by [`react_to_crime`].
use crate::actor::Actor;
use crate::rng::Rng;
use crate::types::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Enclave status [CA 10.5]: Routine, Attack, Flee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EnclaveStatus {
    #[default]
    Routine,
    Attack,
    Flee,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Enclave {
    pub alignment: Alignment,
    pub known_crime: Crime,
    pub theft_count: u32,
    pub murder_count: u32,
    pub status: EnclaveStatus,
    pub suspect_comment: bool,
    /// Highest guard-news level already spoken (each level is spoken once) [CA 10.5].
    pub news_spoken: u8,
}

impl Enclave {
    /// Guard news level: 1 at Theft 20 / Murder 1, 2 at 40 / 2, 3 at 60 / 3 or crime already
    /// known [CA 10.5, S:b8b0].
    pub fn news_level(&self) -> u8 {
        if self.theft_count >= 60 || self.murder_count >= 3 || self.known_crime != Crime::None {
            3
        } else if self.theft_count >= 40 || self.murder_count >= 2 {
            2
        } else if self.theft_count >= 20 || self.murder_count >= 1 {
            1
        } else {
            0
        }
    }

    /// Next unspoken news level, marking it spoken.
    pub fn take_news(&mut self) -> Option<u8> {
        let l = self.news_level();
        if l > self.news_spoken {
            self.news_spoken = l;
            Some(l)
        } else {
            None
        }
    }

    /// Pardon (`ChangeEnclaveStatus` to Routine, S:602d0): forget the crime [CA 10.5]. The
    /// caller also resets the members' memories and sets the player fame to 100.
    pub fn pardon(&mut self) {
        self.known_crime = Crime::None;
        self.suspect_comment = false;
        self.status = EnclaveStatus::Routine;
    }
}

/// Crime implied by an attack reason [CA 10.2 `AssessTarget`].
pub fn crime_for_reason(reason: AttackReason) -> Crime {
    match reason {
        AttackReason::Theft | AttackReason::UseInteractObject => Crime::Theft,
        AttackReason::Murder => Crime::Murder,
        _ => Crime::None,
    }
}

/// Outcome of `MemorizePlayerCrime` (S:37080).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Memorized {
    Ignored,
    Enclave,
    Personal,
}

/// `MemorizePlayerCrime` [CA 10.5]. Friend-type NPCs ignore it. Only Theft and Murder reach
/// the enclave (`MemorizeEnclaveCrime`); theft only through guards and bosses, and the
/// reporter needs a friendly political attitude to the player. Otherwise the NPC keeps
/// `LastPlayerCrime = max(old, crime)`.
pub fn memorize_player_crime(
    witness: &mut Actor,
    enclaves: &mut BTreeMap<String, Enclave>,
    crime: Crime,
    reporter_friendly: bool,
) -> Memorized {
    if crime == Crime::None || witness.npc_type == NpcType::Friend {
        return Memorized::Ignored;
    }
    if matches!(crime, Crime::Theft | Crime::Murder) && reporter_friendly {
        let reports = crime == Crime::Murder || witness.is_guard() || witness.boss;
        if reports && let Some(name) = witness.enclave.clone() {
            let e = enclaves.entry(name).or_default();
            e.known_crime = e.known_crime.max(crime);
            e.suspect_comment = true;
            match crime {
                Crime::Theft => e.theft_count += 1,
                _ => e.murder_count += 1,
            }
            witness.last_player_crime = witness.last_player_crime.max(crime);
            return Memorized::Enclave;
        }
    }
    witness.last_player_crime = witness.last_player_crime.max(crime);
    Memorized::Personal
}

/// `IsBotheredByPlayerTheftOf` (S:30840): plants, `_Teleport_` items, bolts, arrows,
/// `It_Bacon` and `It_Gancka_Wood` never bother anybody; neither do items the player
/// dropped earlier [CA 10.2]. Plant names are matched by the `Plant` / `It_Pl_` stems [I].
pub fn is_bothered_by_theft_of(item: &str, marked_dropped: bool) -> bool {
    if marked_dropped {
        return false;
    }
    let n = item.to_ascii_lowercase();
    !(n.contains("plant")
        || n.starts_with("it_pl_")
        || n.contains("_teleport_")
        || n.contains("bolt")
        || n.contains("arrow")
        || n.starts_with("it_bacon")
        || n.starts_with("it_gancka_wood"))
}

/// `_AI_Excuse` (S:3d130): needs `Perk_Excuse`; crimes up to Attack always work; Theft and
/// Murder succeed when `Random(130) <= 117` and `<= Theft` skill [CA 10.5].
pub fn excuse_succeeds(rng: &mut Rng, crime: Crime, theft_skill: i32, has_perk: bool) -> bool {
    if !has_perk {
        return false;
    }
    if crime <= Crime::Attack {
        return true;
    }
    let r = rng.below(130);
    r <= 117 && r <= theft_skill
}

/// Probability (0..=1) of [`excuse_succeeds`] for Theft / Murder: `(min(Theft,117)+1)/130`
/// since `Random(130)` is `0..=129` and both clauses are `<=`.
pub fn excuse_probability(theft_skill: i32) -> f32 {
    ((theft_skill.clamp(-1, 117) + 1) as f32) / 130.0
}

/// What a witness does about a crime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reaction {
    /// Not bothered (too far, cannot see, friendly to the culprit's cause ...).
    Ignore,
    /// `AssessTarget(reason)`: attack, with the memorised crime.
    Attack(AttackReason),
    /// `ZS_Flee`.
    Flee,
    /// Observe / comment only.
    Watch,
}

/// Map a witnessed crime to a reaction for an NPC whose attitude to the culprit is
/// `attitude` [CA 10.2]. Theft and object use trigger `AssessTarget` with the matching
/// reason; murder of a friend triggers `Murder`; livestock is not a crime; same-enclave
/// peers stop fights. Panic or an inability to attack means flight.
pub fn react_to_crime(witness: &Actor, crime_reason: AttackReason, attitude: Attitude) -> Reaction {
    if !witness.species.is_humanoid() {
        return Reaction::Ignore;
    }
    if attitude == Attitude::Panic || !witness.species.can_attack() {
        return Reaction::Flee;
    }
    match crime_reason {
        AttackReason::Livestock => Reaction::Watch,
        AttackReason::Theft | AttackReason::UseInteractObject | AttackReason::Murder => {
            Reaction::Attack(crime_reason)
        }
        AttackReason::StopFight => Reaction::Watch,
        other => Reaction::Attack(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> Actor {
        let mut g = Actor::npc(ActorId(1), "Guard", Species::Human, 20, 20);
        g.npc_type = NpcType::Enclave;
        g.alignment = Alignment::Mid;
        g.enclave = Some("trelis".into());
        g
    }

    #[test]
    fn guard_news_levels() {
        let mut e = Enclave::default();
        assert_eq!(e.news_level(), 0);
        e.theft_count = 20;
        assert_eq!(e.take_news(), Some(1));
        assert_eq!(e.take_news(), None, "each level is spoken once");
        e.theft_count = 40;
        assert_eq!(e.take_news(), Some(2));
        e.murder_count = 3;
        assert_eq!(e.take_news(), Some(3));
        let k = Enclave {
            known_crime: Crime::Theft,
            ..Enclave::default()
        };
        assert_eq!(k.news_level(), 3);
    }

    #[test]
    fn memorize_through_guard_reaches_enclave() {
        let mut enclaves = BTreeMap::new();
        let mut g = guard();
        let m = memorize_player_crime(&mut g, &mut enclaves, Crime::Theft, true);
        assert_eq!(m, Memorized::Enclave);
        let e = &enclaves["trelis"];
        assert_eq!(e.known_crime, Crime::Theft);
        assert_eq!(e.theft_count, 1);
        assert!(e.suspect_comment);
        // A civilian only remembers it herself.
        let mut civ = Actor::npc(ActorId(2), "Civ", Species::Human, 5, 5);
        civ.enclave = Some("trelis".into());
        let m = memorize_player_crime(&mut civ, &mut enclaves, Crime::Theft, true);
        assert_eq!(m, Memorized::Personal);
        assert_eq!(civ.last_player_crime, Crime::Theft);
        // Reporter must be friendly, friend-type NPCs ignore everything.
        let mut g2 = guard();
        let mut empty = BTreeMap::new();
        assert_eq!(
            memorize_player_crime(&mut g2, &mut empty, Crime::Theft, false),
            Memorized::Personal
        );
        let mut f = guard();
        f.npc_type = NpcType::Friend;
        assert_eq!(
            memorize_player_crime(&mut f, &mut empty, Crime::Murder, true),
            Memorized::Ignored
        );
        // Worst crime sticks.
        civ.last_player_crime = Crime::Murder;
        memorize_player_crime(&mut civ, &mut empty, Crime::Theft, false);
        assert_eq!(civ.last_player_crime, Crime::Murder);
    }

    #[test]
    fn theft_bother_list() {
        assert!(is_bothered_by_theft_of("It_Sword_01", false));
        assert!(!is_bothered_by_theft_of("It_Sword_01", true));
        assert!(!is_bothered_by_theft_of("It_Bolt", false));
        assert!(!is_bothered_by_theft_of("It_Arrow_Fire", false));
        assert!(!is_bothered_by_theft_of("It_Bacon", false));
        assert!(!is_bothered_by_theft_of("It_Gancka_Wood", false));
        assert!(!is_bothered_by_theft_of("It_Pl_Health_Herb_01", false));
        assert!(!is_bothered_by_theft_of("It_Ru_Teleport_Ardea", false));
    }

    #[test]
    fn excuse_rule() {
        let mut rng = Rng::new(5);
        assert!(!excuse_succeeds(&mut rng, Crime::Attack, 100, false));
        assert!(excuse_succeeds(&mut rng, Crime::Attack, 0, true));
        // Theft skill 10: succeeds for r in 0..=10, i.e. 11/130.
        assert!((excuse_probability(10) - 11.0 / 130.0).abs() < 1e-6);
        assert!((excuse_probability(500) - 118.0 / 130.0).abs() < 1e-6);
        let hits = (0..13_000)
            .filter(|_| excuse_succeeds(&mut rng, Crime::Theft, 10, true))
            .count();
        assert!((900..1400).contains(&hits), "{hits}");
    }

    #[test]
    fn reactions() {
        let g = guard();
        assert_eq!(
            react_to_crime(&g, AttackReason::Theft, Attitude::Friendly),
            Reaction::Attack(AttackReason::Theft)
        );
        assert_eq!(
            react_to_crime(&g, AttackReason::Livestock, Attitude::Friendly),
            Reaction::Watch
        );
        assert_eq!(
            react_to_crime(&g, AttackReason::Theft, Attitude::Panic),
            Reaction::Flee
        );
        let wolf = Actor::npc(ActorId(2), "W", Species::Wolf, 5, 5);
        assert_eq!(
            react_to_crime(&wolf, AttackReason::Theft, Attitude::Neutral),
            Reaction::Ignore
        );
        assert_eq!(
            crime_for_reason(AttackReason::UseInteractObject),
            Crime::Theft
        );
        assert_eq!(crime_for_reason(AttackReason::Enemy), Crime::None);
    }

    #[test]
    fn pardon_clears() {
        let mut e = Enclave {
            known_crime: Crime::Murder,
            suspect_comment: true,
            status: EnclaveStatus::Attack,
            ..Enclave::default()
        };
        e.pardon();
        assert_eq!(e.known_crime, Crime::None);
        assert_eq!(e.status, EnclaveStatus::Routine);
    }
}
