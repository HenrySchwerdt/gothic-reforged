//! B12: the dialog commands of the guards and thieves: `_AI_Excuse`, `PickPocket` and
//! `GetGuardAttackReason`.
//!
//! The DLL runs them as coroutines inside the info (dialog) manager, speaking voice lines and
//! ending the dialog; here they are plain functions that return what happened and queue the
//! voice lines. `_AI_Bribe` only decides what the guard does with an offer; the price is dialog
//! data, so the caller passes whether the dialog accepted it.

use crate::enums::{AttackReason, PlayerCrime};
use crate::host::{AiCommand, EntityId, ScriptHost};
use crate::perception::assess_target;
use crate::stats::{attribute, level_max};

/// Port of `GetGuardAttackReason` (S:3d050): why a guard attacks somebody whose crime it knows:
/// a murderer is fought to the death (reason Revolution, 19), a thief with reason Theft, anybody
/// else not at all.
pub fn guard_attack_reason(crime: PlayerCrime) -> AttackReason {
    gothic_portcov::hit!("guard_attack_reason");
    match crime {
        PlayerCrime::Murder => AttackReason::Revolution,
        PlayerCrime::Theft => AttackReason::Theft,
        _ => AttackReason::None,
    }
}

/// How an excuse ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExcuseOutcome {
    /// The player has no `Perk_Excuse`: "NoIdea", nothing happens.
    NoSkill,
    /// The guard bought it: a warning ("NextTimeYoureGuilty" for minor crimes, "WeObserveYou"
    /// for theft and murder).
    Believed,
    /// The guard did not: "IDontBelieveYou", the guard attacks.
    Rejected,
}

/// Port of `_AI_Excuse` (S:3d130): the player excuses a `crime` to a guard.
///
/// Needs `Perk_Excuse`. Crimes up to Attack always work; for theft and murder the guard rolls
/// `Random(130)`: above 117 or above the player's theft skill and the excuse fails, i.e. it works
/// with probability `min(Theft, 117) / 130`. A failed excuse makes the guard attack with
/// [`guard_attack_reason`]; for a murder the enclave also learns of it (`NotifyEnclave` mode 1).
pub fn excuse(
    host: &mut dyn ScriptHost,
    guard: EntityId,
    player: EntityId,
    crime: PlayerCrime,
) -> ExcuseOutcome {
    gothic_portcov::hit!("excuse");
    if !host.is_skill_active(player, "Perk_Excuse") {
        host.queue_ai(
            player,
            AiCommand::SaySvm {
                family: "NoIdea",
                index: 0,
            },
        );
        return ExcuseOutcome::NoSkill;
    }
    let serious = crime.raw() > 2;
    let mut believed = true;
    if serious {
        let roll = host.random(130);
        believed = roll <= 117 && attribute(host, player, crate::host::Attribute::Theft) >= roll;
    }
    if believed {
        let family = if serious {
            "WeObserveYou"
        } else {
            "NextTimeYoureGuilty"
        };
        host.queue_ai(guard, AiCommand::SaySvm { family, index: 0 });
        return ExcuseOutcome::Believed;
    }
    host.queue_ai(
        guard,
        AiCommand::SaySvm {
            family: "IDontBelieveYou",
            index: 0,
        },
    );
    let reason = guard_attack_reason(crime);
    if reason == AttackReason::Revolution {
        crate::crime::notify_enclave(host, guard, 1);
    }
    if reason != AttackReason::None {
        assess_target(host, guard, player, reason);
    }
    ExcuseOutcome::Rejected
}

/// How a bribe ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BribeOutcome {
    /// The guard takes the money and forgets the crime.
    Accepted,
    /// The guard refuses ("No") and acts on what he knows.
    Refused,
}

/// Port of `_AI_Bribe` (S:3d7d0): the player offers a guard money to forget a `crime`.
///
/// Accepted (the dialog took the gold): the guard says `LetsForgetThis`, `ThisTimeYoureLucky` and
/// `NoMoreTroubleHere`, and forgets what the player did to him (`LastPlayerCrime` cleared).
/// Refused: the guard says `No`, then attacks as in `_AI_Excuse` ([`guard_attack_reason`]; a
/// murder also alerts the enclave). The dialog ends either way.
pub fn bribe(
    host: &mut dyn ScriptHost,
    guard: EntityId,
    player: EntityId,
    crime: PlayerCrime,
    accepted: bool,
) -> BribeOutcome {
    gothic_portcov::hit!("bribe");
    let say = |host: &mut dyn ScriptHost, key: &'static str| {
        host.queue_ai(guard, AiCommand::SayLine { key });
    };
    if accepted {
        say(host, "LetsForgetThis");
        say(host, "ThisTimeYoureLucky");
        say(host, "NoMoreTroubleHere");
        host.set_last_player_crime(guard, PlayerCrime::None);
        return BribeOutcome::Accepted;
    }
    say(host, "No");
    let reason = guard_attack_reason(crime);
    if reason == AttackReason::Revolution {
        crate::crime::notify_enclave(host, guard, 1);
    }
    if reason != AttackReason::None {
        assess_target(host, guard, player, reason);
    }
    BribeOutcome::Refused
}

/// How a pickpocket attempt ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickpocketOutcome {
    /// The skill does not reach the victim's level: "TooHard".
    TooHard,
    /// Nobody of this level can be robbed: "Impossible".
    Impossible,
    /// The victim noticed: it attacks with reason Theft.
    Caught,
    /// The pockets are emptied into the player's reach. `counted` is whether the theft is
    /// added to the enclave's theft count.
    Success {
        /// The theft counts against the victim's enclave.
        counted: bool,
    },
}

/// The success chance in percent of a pickpocket attempt, or why it cannot be tried.
///
/// Victims below level 30 (`LevelMax`) can be tried by anybody, level 30 to 44 needs
/// `Perk_PickPocket_2`, 45 to 49 `Perk_PickPocket_3`, and nobody of level 50 or more can be
/// robbed. The chance is `85 + Theft/2 - LevelMax`.
pub fn pickpocket_chance(
    host: &dyn ScriptHost,
    victim: EntityId,
    thief: EntityId,
) -> Result<i32, PickpocketOutcome> {
    let lm = level_max(host, victim) as i32;
    if lm >= 50 {
        return Err(PickpocketOutcome::Impossible);
    }
    let skill = |perk: &str| host.is_skill_active(thief, perk);
    let allowed = if lm > 44 {
        skill("Perk_PickPocket_3")
    } else if lm >= 30 {
        skill("Perk_PickPocket_2")
    } else {
        true
    };
    if !allowed {
        return Err(PickpocketOutcome::TooHard);
    }
    let theft = attribute(host, thief, crate::host::Attribute::Theft);
    Ok(85 + theft / 2 - lm)
}

/// Port of `PickPocket` (S:3dd70): one attempt. A caught thief is attacked (reason Theft); a
/// successful one counts toward the victim enclave's thefts unless `ExtendedContent` is on and
/// the thief has `Perk_MasterThief`.
pub fn pick_pocket(
    host: &mut dyn ScriptHost,
    victim: EntityId,
    thief: EntityId,
) -> PickpocketOutcome {
    gothic_portcov::hit!("pick_pocket");
    let chance = match pickpocket_chance(host, victim, thief) {
        Ok(c) => c,
        Err(refusal) => return refusal,
    };
    let roll = host.random(100);
    if chance < roll {
        assess_target(host, victim, thief, AttackReason::Theft);
        return PickpocketOutcome::Caught;
    }
    let enclave = host.enclave(victim);
    let master = host.setup().extended_content && host.is_skill_active(thief, "Perk_MasterThief");
    let counted = enclave.is_some() && !master;
    if counted {
        let n = host.enclave_crime_count(enclave, PlayerCrime::Theft);
        host.set_enclave_crime_count(enclave, PlayerCrime::Theft, n + 1);
    }
    PickpocketOutcome::Success { counted }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{Alignment, Species};
    use crate::host::{EntityApi, NpcApi, WorldApi};
    use crate::testhost::TestWorld;

    #[test]
    fn guards_attack_murderers_to_the_death_and_thieves_for_theft() {
        assert_eq!(
            guard_attack_reason(PlayerCrime::Murder),
            AttackReason::Revolution
        );
        assert_eq!(guard_attack_reason(PlayerCrime::Theft), AttackReason::Theft);
        assert_eq!(guard_attack_reason(PlayerCrime::Attack), AttackReason::None);
    }

    #[test]
    fn an_excuse_needs_the_perk_and_a_good_roll_for_serious_crimes() {
        let mut w = TestWorld::new();
        let guard = w.spawn_npc("Guard", Species::Human, 5, 5);
        let p = w.player();
        assert_eq!(
            excuse(&mut w, guard, p, PlayerCrime::Theft),
            ExcuseOutcome::NoSkill
        );
        w.grant_skill(p, "Perk_Excuse");
        assert_eq!(
            excuse(&mut w, guard, p, PlayerCrime::Attack),
            ExcuseOutcome::Believed
        );
        // Roll 50 against a theft skill of 100: believed. Roll 120: rejected, the guard attacks.
        w.pm.attributes.insert(crate::host::Attribute::Theft, 100);
        w.rolls = vec![50];
        assert_eq!(
            excuse(&mut w, guard, p, PlayerCrime::Theft),
            ExcuseOutcome::Believed
        );
        w.rolls = vec![120];
        assert_eq!(
            excuse(&mut w, guard, p, PlayerCrime::Theft),
            ExcuseOutcome::Rejected
        );
        assert_eq!(w.attack_reason(guard), AttackReason::Theft);
    }

    #[test]
    fn pickpocket_chances_follow_level_and_perks() {
        let mut w = TestWorld::new();
        let weak = w.spawn_npc("Peasant", Species::Human, 10, 10);
        let mid = w.spawn_npc("Guard", Species::Human, 35, 35);
        let tough = w.spawn_npc("Boss", Species::Human, 60, 60);
        let p = w.player();
        w.pm.attributes.insert(crate::host::Attribute::Theft, 40);
        assert_eq!(pickpocket_chance(&w, weak, p), Ok(85 + 20 - 10));
        assert_eq!(
            pickpocket_chance(&w, mid, p),
            Err(PickpocketOutcome::TooHard)
        );
        w.grant_skill(p, "Perk_PickPocket_2");
        assert_eq!(pickpocket_chance(&w, mid, p), Ok(85 + 20 - 35));
        assert_eq!(
            pickpocket_chance(&w, tough, p),
            Err(PickpocketOutcome::Impossible)
        );
    }

    #[test]
    fn a_failed_attempt_is_a_crime_a_good_one_counts_for_the_enclave() {
        let mut w = TestWorld::new();
        let enc = w.add_enclave(Alignment::Reb);
        let victim = w.spawn_npc("Peasant", Species::Human, 10, 10);
        w.set_enclave(victim, enc);
        let p = w.player();
        w.pm.attributes.insert(crate::host::Attribute::Theft, 40);
        w.rolls = vec![99];
        assert_eq!(pick_pocket(&mut w, victim, p), PickpocketOutcome::Caught);
        assert_eq!(w.attack_reason(victim), AttackReason::Theft);
        w.rolls = vec![10];
        assert_eq!(
            pick_pocket(&mut w, victim, p),
            PickpocketOutcome::Success { counted: true }
        );
        assert_eq!(w.enclave_crime_count(enc, PlayerCrime::Theft), 1);
        w.grant_skill(p, "Perk_MasterThief");
        w.rolls = vec![10];
        assert_eq!(
            pick_pocket(&mut w, victim, p),
            PickpocketOutcome::Success { counted: false }
        );
    }
}
