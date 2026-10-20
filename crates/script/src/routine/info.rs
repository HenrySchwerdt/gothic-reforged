//! B22: the glue of the info-script (dialog) engine states that is script logic: the three
//! "has news" predicates that decide which opening a guard, a boss or a companion gets, and the
//! refusal states that block a dialog option (`IS_Running_NotEnoughGold` and friends).
//!
//! The executor itself (`IS_Running_SelectInfo`, `IS_Running_ProcessInfo`, the idle states) is the
//! dialog engine of the game layer: `gothic_gameplay::dialog` interprets the info scripts and
//! their command table. The refusal states are plain data here: [`gate_response`] says who speaks
//! which voice line and which HUD message appears.

use crate::enums::{Alignment, PlayerCrime};
use crate::host::{EntityId, ScriptHost};
use crate::predicates::{is_boss, is_guard, is_humanoid};

/// The voice block a guard or boss needs to be able to hold the opening speech.
pub const NEWS_SVM_BLOCK: &str = "GuardAndBoss";
/// The potion a companion needs to be able to offer healing.
pub const HEAL_POTION: &str = "It_Potion_Health";

/// Does the NPC's enclave hold news of this crime kind for the player [I: the DLL's helper
/// compares the crime count with the number of comments already made; the count is taken as
/// news while it is positive].
fn enclave_news(host: &dyn ScriptHost, npc: EntityId, crime: PlayerCrime) -> bool {
    let enclave = host.enclave(npc);
    enclave.is_some() && host.enclave_crime_count(enclave, crime) > 0
}

/// Port of `HasGuardNews` (S:bcf00): a guard with the `GuardAndBoss` voice block whose enclave
/// knows of a murder or a theft by the player has something to say.
pub fn has_guard_news(host: &dyn ScriptHost, npc: EntityId) -> bool {
    is_guard(host, npc)
        && host.has_svm_sample(npc, NEWS_SVM_BLOCK)
        && (enclave_news(host, npc, PlayerCrime::Murder)
            || enclave_news(host, npc, PlayerCrime::Theft))
}

/// Port of `HasBossNews` (S:bd050): a boss with the `GuardAndBoss` voice block whose political
/// camp has more crimes on record than comments made, as long as the player is not already
/// marked a criminal there.
pub fn has_boss_news(host: &dyn ScriptHost, npc: EntityId) -> bool {
    if !is_boss(host, npc) || !host.has_svm_sample(npc, NEWS_SVM_BLOCK) {
        return false;
    }
    let camp: Alignment = host.alignment(npc);
    host.political_crime_count(camp) > 0 && !host.political_player_crime(camp)
}

/// Port of `HasHealInfo` (S:bd2c0): a humanoid member of the player's party who carries a health
/// potion can offer to heal him.
pub fn has_heal_info(host: &dyn ScriptHost, npc: EntityId) -> bool {
    host.party_leader(npc) == host.player()
        && is_humanoid(host, npc)
        && host.has_items(npc, HEAL_POTION, 1)
}

/// A dialog option that cannot be taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoGate {
    /// `IS_Running_NotEnoughGold`.
    NotEnoughGold,
    /// `IS_Running_NotEnoughLP`.
    NotEnoughLp,
    /// `IS_Running_NoLearnRequirements`.
    NoLearnRequirements,
    /// `IS_Running_NoParty`: the party is full or the NPC cannot join.
    NoParty,
    /// `IS_Running_NoPoliticalReputation`: the camp's fame is too low.
    NoPoliticalReputation,
    /// `IS_Running_NoEnclaveReputation`: the enclave's fame is too low.
    NoEnclaveReputation,
}

/// What a refused option looks like.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateResponse {
    /// The voice line the NPC answers with (`SVM_<voice>_<key>`).
    pub npc_line: &'static str,
    /// The HUD message key (`GO_NotEnoughGold`).
    pub message: &'static str,
    /// The message also names the missing points (`HUD_MissingPoints` with `$(name)`/`$(value)`).
    pub shows_missing_points: bool,
}

/// Ports of the six refusal states `IS_Running_NotEnoughGold` (S:b7850), `IS_Running_NotEnoughLP` (S:b7bb0), `IS_Running_NoLearnRequirements` (S:b6ae0), `IS_Running_NoParty` (S:b6e00), `IS_Running_NoPoliticalReputation` (S:b7270), `IS_Running_NoEnclaveReputation` (S:b6420): each makes the NPC say a refusal and prints
/// a message, then reports the command as completed. The political refusal names the camp
/// ("I do not trust you", or "NoFriendOf<camp>" for the NPC's own camp).
pub fn gate_response(gate: InfoGate, npc_camp: Alignment) -> GateResponse {
    match gate {
        InfoGate::NotEnoughGold => GateResponse {
            npc_line: "NotEnoughGold",
            message: "GO_NotEnoughGold",
            shows_missing_points: false,
        },
        InfoGate::NotEnoughLp => GateResponse {
            npc_line: "NotEnoughLP",
            message: "GO_NotEnoughLP",
            shows_missing_points: false,
        },
        InfoGate::NoLearnRequirements => GateResponse {
            npc_line: "NotReadyYet",
            message: "GO_NoLearnRequirements",
            shows_missing_points: false,
        },
        InfoGate::NoParty => GateResponse {
            npc_line: "JoinRefuse",
            message: "GO_NoParty",
            shows_missing_points: false,
        },
        InfoGate::NoPoliticalReputation => GateResponse {
            npc_line: match npc_camp {
                Alignment::Ass => "NoFriendOfASS",
                Alignment::Mid => "NoFriendOfMID",
                Alignment::Nom => "NoFriendOfNOM",
                Alignment::Nrd => "NoFriendOfNRD",
                Alignment::Orc => "NoFriendOfORC",
                Alignment::Reb => "NoFriendOfREB",
                _ => "IDontTrustYou",
            },
            message: "GO_RequiredPoliticalReputation",
            shows_missing_points: true,
        },
        InfoGate::NoEnclaveReputation => GateResponse {
            npc_line: "NoFriendOfEnclave",
            message: "GO_RequiredEnclaveReputation",
            shows_missing_points: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{NpcType, Species};
    use crate::host::{EntityApi, WorldApi};
    use crate::testhost::TestWorld;

    #[test]
    fn heal_info_needs_a_potion_in_the_party() {
        let mut w = TestWorld::new();
        let p = w.player();
        let friend = w.spawn_npc("Friend", Species::Human, 5, 5);
        assert!(!has_heal_info(&w, friend));
        w.set_leader(friend, p, crate::enums::PartyMemberType::Party, false);
        assert!(!has_heal_info(&w, friend), "no potion");
        w.give_item(friend, HEAL_POTION);
        assert!(has_heal_info(&w, friend));
        let wolf = w.spawn_npc("Wolf", Species::Wolf, 5, 5);
        w.set_leader(wolf, p, crate::enums::PartyMemberType::Party, false);
        w.give_item(wolf, HEAL_POTION);
        assert!(!has_heal_info(&w, wolf), "animals cannot heal");
    }

    #[test]
    fn a_guard_with_a_known_crime_has_news() {
        let mut w = TestWorld::new();
        let guard = w.spawn_npc("Guard", Species::Human, 20, 20);
        let enclave = w.add_enclave(Alignment::Mid);
        w.set_enclave(guard, enclave);
        w.set_alignment(guard, Alignment::Mid);
        w.set_npc_type(guard, NpcType::Enclave);
        assert!(!has_guard_news(&w, guard));
        w.set_enclave_crime_count(enclave, PlayerCrime::Theft, 1);
        assert!(!has_guard_news(&w, guard), "needs the voice block");
        w.has_speech = true;
        assert!(has_guard_news(&w, guard));
    }

    #[test]
    fn a_boss_remarks_on_political_crimes_not_yet_commented() {
        let mut w = TestWorld::new();
        let boss = w.spawn_npc("Boss", Species::Human, 30, 30);
        let enclave = w.add_enclave(Alignment::Mid);
        w.set_enclave(boss, enclave);
        w.set_npc_type(boss, NpcType::Political);
        w.set_alignment(boss, Alignment::Mid);
        w.has_speech = true;
        assert!(!has_boss_news(&w, boss));
        w.set_political_crime_count(Alignment::Mid, 2);
        assert!(has_boss_news(&w, boss));
        w.set_political_player_crime(Alignment::Mid, true);
        assert!(!has_boss_news(&w, boss), "already an outlaw there");
    }

    #[test]
    fn the_refusals_pick_voice_and_message() {
        let r = gate_response(InfoGate::NotEnoughGold, Alignment::None);
        assert_eq!(
            (r.npc_line, r.message),
            ("NotEnoughGold", "GO_NotEnoughGold")
        );
        let r = gate_response(InfoGate::NoPoliticalReputation, Alignment::Orc);
        assert_eq!(r.npc_line, "NoFriendOfORC");
        assert!(r.shows_missing_points);
        let r = gate_response(InfoGate::NoPoliticalReputation, Alignment::None);
        assert_eq!(r.npc_line, "IDontTrustYou");
    }
}
