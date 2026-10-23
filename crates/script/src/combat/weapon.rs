//! `_AI_SelectWeapon` (S:49600) of the NPCs: which weapon configuration the fighter wants for
//! its opponent, and which spell a mage readies.
//!
//! The script command asks [`choose_weapon_mode`] (S:4a030) for the wanted mode (melee, ranged,
//! magic); when the hands already hold that mode nothing happens (S:49600 compares
//! `WeaponMode` with it), otherwise the hold instruction swaps the items: a ranged request
//! without a bow in the bow slot falls back to melee, a magic request asks [`choose_spell`]
//! (S:49890) for the spell template, the melee request takes the weapon of the back slots
//! (S:49e50). The host applies the result to the inventory (`_AI_HoldInventoryItems`).
//!
//! Addresses are `Script_Game.dll` RVAs.

use crate::enums::{NpcClass, Species};
use crate::helpers::WeaponMode;
use crate::host::{EntityId, ScriptHost, dir};

/// Mana an NPC needs to cast (`PSNpc::CanCast(600)` of the frost golem rule, S:4a1ef).
pub const CAST_MANA: i32 = 600;
/// Below this distance a non-mage fights in melee (S:4a232, `700.0`).
pub const MELEE_RANGE: f32 = 700.0;
/// Above this distance a non-mage readies a ranged weapon (S:4a25f, `1000.0`).
pub const RANGED_RANGE: f32 = 1000.0;
/// A frost golem (species 49) casts from this distance on (S:4a1cb, `750.0`).
pub const GOLEM_CAST_RANGE: f32 = 750.0;
/// Mage thresholds toward a humanoid (S:4a0c7 `400.0`, S:4a107 `250.0`, S:4a168 `470.0`).
pub const MAGE_CLOSE: f32 = 400.0;
pub const MAGE_FLINCH: f32 = 250.0;
pub const MAGE_MELEE_SWITCH: f32 = 470.0;

/// The story mages whose fights start with a summoning or high spell instead of the stock
/// fireball (S:4a43a..4a49e, the names compared in S:49890).
pub const STORY_CASTERS: [&str; 6] = ["Xardas", "Grok", "Ningal", "Tizgar", "Saturas", "Runak"];

/// Port of `IsStoryCaster` (S:4a370, helper `sub_4a370`): a story mage (see [`STORY_CASTERS`]) fighting the player alone (no party
/// members), in a game with `ExtendedContent` or `AIMode`.
pub fn is_story_caster(host: &dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("is_story_caster");
    let setup = host.setup();
    if !(setup.extended_content || setup.ai_mode) {
        return false;
    }
    if host.current_target(npc) != host.player() {
        return false;
    }
    if !host.party_members(npc).is_empty() {
        return false;
    }
    let name = host.name(npc);
    STORY_CASTERS.iter().any(|n| name == *n)
}

/// Port of `ChooseWeaponMode` (S:4a030, helper `sub_4a030`): the weapon mode `npc` wants against `target` (`1` melee, `2` ranged, `3`
/// magic of the original, here [`WeaponMode`]). No target: melee.
pub fn choose_weapon_mode(host: &dyn ScriptHost, npc: EntityId, target: EntityId) -> WeaponMode {
    gothic_portcov::hit!("choose_weapon_mode");
    use WeaponMode as M;
    if target.is_none() {
        return M::Melee;
    }
    let current = crate::combat::select::npc_weapon_mode(host, npc);
    let d = host.distance(npc, target);
    let species = host.species(npc);
    if host.npc_class(npc) == NpcClass::Mage {
        if !crate::predicates::is_humanoid(host, target) {
            return M::Magic;
        }
        if d < MAGE_CLOSE {
            if target == host.player() {
                if d <= MAGE_FLINCH || !is_story_caster(host, npc) {
                    return M::Melee;
                }
                return M::Magic;
            }
            if !host.can_evade(npc, target, dir::BACK) {
                return M::Melee;
            }
        }
        if d >= MAGE_MELEE_SWITCH {
            return M::Magic;
        }
        return if current == M::Melee {
            M::Melee
        } else {
            M::Magic
        };
    }
    if species == Species::IceGolem {
        let frozen = host.has_status(npc, 2);
        let mana = crate::stats::pool(host, npc, crate::host::Pool::Mana);
        if d < GOLEM_CAST_RANGE || frozen || mana < CAST_MANA {
            return M::Melee;
        }
        return M::Magic;
    }
    if d < MELEE_RANGE {
        return M::Melee;
    }
    if d < RANGED_RANGE {
        // Between 7 and 10 m the fighter keeps what it holds.
        return if current == M::None {
            M::Melee
        } else {
            current
        };
    }
    if host.npc_class(npc) == NpcClass::Paladin {
        return M::Magic;
    }
    match species {
        Species::Skeleton | Species::Golem | Species::FireGolem | Species::IceGolem => M::Melee,
        _ => M::Ranged,
    }
}

/// Port of `ChooseSpell` (S:49890, helper `sub_49890`): the spell template a mage readies. Story casters take their signature
/// spell; goblins, fire golems and dragons the fireball, frost golems the ice lance; everyone
/// else heals below 40 percent hit points half of the time, fights fire golems with ice and
/// frost golems with fire, keeps the fireball or ice lance it already holds and otherwise
/// picks one of the two at random.
pub fn choose_spell(host: &mut dyn ScriptHost, npc: EntityId, held_spell: &str) -> &'static str {
    gothic_portcov::hit!("choose_spell");
    if is_story_caster(host, npc) {
        let name = host.name(npc);
        let signature = match name.as_str() {
            "Xardas" => Some("It_Spell_ArmyOfDarkness"),
            "Tizgar" | "Grok" => Some("It_Spell_SummonDemon"),
            "Ningal" => Some("It_Spell_SummonSkeleton"),
            "Runak" => Some("It_Spell_SummonMonster"),
            "Saturas" => Some("It_Spell_SummonGolem"),
            _ => None,
        };
        if let Some(s) = signature {
            return s;
        }
    }
    match host.species(npc) {
        Species::Goblin | Species::FireGolem | Species::Dragon => return "It_Spell_Fireball",
        Species::IceGolem => return "It_Spell_Icelance",
        _ => {}
    }
    if crate::stats::pool_percent(host, npc, crate::host::Pool::Hit) < 40 && host.random(100) < 50 {
        return "It_Spell_Healing";
    }
    let target = host.current_target(npc);
    match host.species(target) {
        Species::IceGolem => return "It_Spell_Fireball",
        Species::FireGolem => return "It_Spell_Icelance",
        _ => {}
    }
    if held_spell.eq_ignore_ascii_case("It_Spell_Fireball") {
        return "It_Spell_Fireball";
    }
    if held_spell.eq_ignore_ascii_case("It_Spell_Icelance") {
        return "It_Spell_Icelance";
    }
    if host.random(100) < 50 {
        "It_Spell_Fireball"
    } else {
        "It_Spell_Icelance"
    }
}

/// Port of `_AI_SelectWeapon` (S:49600): what the command decides for `npc` against `target`: the mode to ready, or
/// `None` when the hands already hold it. A ranged request without a ranged weapon at hand
/// (`has_ranged`) becomes a melee request; a mage always re-readies its spell.
pub fn select_weapon_mode(
    host: &dyn ScriptHost,
    npc: EntityId,
    target: EntityId,
    has_ranged: bool,
) -> Option<WeaponMode> {
    gothic_portcov::hit!("select_weapon_mode");
    let want = choose_weapon_mode(host, npc, target);
    let current = crate::combat::select::npc_weapon_mode(host, npc);
    if want != WeaponMode::Magic && current == want {
        return None;
    }
    let mut want = want;
    if want == WeaponMode::Ranged && !has_ranged && current != WeaponMode::Ranged {
        want = WeaponMode::Melee;
    }
    if want != WeaponMode::Magic && current == want {
        return None;
    }
    Some(want)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{NpcClass, Species, UseType};
    use crate::testhost::TestWorld;

    fn pair(species: Species, class: NpcClass, dist: f32) -> (TestWorld, EntityId, EntityId) {
        let mut w = TestWorld::new();
        let me = w.spawn_npc("Fighter", species, 10, 30);
        w.set_class(me, class);
        let foe = w.spawn_npc("Foe", Species::Human, 10, 30);
        w.set_target(me, foe);
        w.set_distance(me, foe, dist);
        (w, me, foe)
    }

    #[test]
    fn a_fighter_chooses_by_distance_and_keeps_its_weapon_in_between() {
        use WeaponMode as M;
        let (w, me, foe) = pair(Species::Human, NpcClass::Warrior, 500.0);
        assert_eq!(choose_weapon_mode(&w, me, foe), M::Melee);
        let (w, me, foe) = pair(Species::Human, NpcClass::Warrior, 1500.0);
        assert_eq!(choose_weapon_mode(&w, me, foe), M::Ranged);
        // 7 to 10 m: whatever is in the hands stays.
        let (mut w, me, foe) = pair(Species::Human, NpcClass::Warrior, 850.0);
        assert_eq!(choose_weapon_mode(&w, me, foe), M::Melee);
        w.set_slot(me, crate::enums::Slot::LeftHand, UseType::Bow, 0);
        w.set_slot(me, crate::enums::Slot::RightHand, UseType::Arrow, 0);
        assert_eq!(choose_weapon_mode(&w, me, foe), M::Ranged);
        // Skeletons and golems never draw a bow.
        let (w, me, foe) = pair(Species::Skeleton, NpcClass::Warrior, 1500.0);
        assert_eq!(choose_weapon_mode(&w, me, foe), M::Melee);
        // No target: melee.
        assert_eq!(choose_weapon_mode(&w, me, EntityId::NONE), M::Melee);
    }

    #[test]
    fn a_mage_casts_from_range_and_without_an_opponent_that_can_be_hit_in_melee() {
        use WeaponMode as M;
        let (w, me, foe) = pair(Species::Orc, NpcClass::Mage, 1400.0);
        assert_eq!(choose_weapon_mode(&w, me, foe), M::Magic);
        let (w, me, foe) = pair(Species::Orc, NpcClass::Mage, 480.0);
        assert_eq!(choose_weapon_mode(&w, me, foe), M::Magic);
        // Between 4 and 4.7 m a mage that holds a melee weapon keeps it; from the empty hands it
        // casts.
        let (w, me, foe) = pair(Species::Orc, NpcClass::Mage, 440.0);
        assert!(matches!(
            choose_weapon_mode(&w, me, foe),
            M::Magic | M::Melee
        ));
    }

    #[test]
    fn the_spell_follows_species_and_opponent() {
        let (mut w, me, _) = pair(Species::Orc, NpcClass::Mage, 1400.0);
        let s = choose_spell(&mut w, me, "");
        assert!(
            matches!(s, "It_Spell_Fireball" | "It_Spell_Icelance"),
            "{s}"
        );
        // A held ice lance is kept.
        assert_eq!(
            choose_spell(&mut w, me, "It_Spell_Icelance"),
            "It_Spell_Icelance"
        );
        let (mut w, goblin, _) = pair(Species::Goblin, NpcClass::Mage, 1400.0);
        assert_eq!(
            choose_spell(&mut w, goblin, "It_Spell_Icelance"),
            "It_Spell_Fireball"
        );
        let (mut w, golem, _) = pair(Species::IceGolem, NpcClass::None, 1400.0);
        assert_eq!(choose_spell(&mut w, golem, ""), "It_Spell_Icelance");
    }

    #[test]
    fn the_hands_decide_whether_anything_changes() {
        let (w, me, foe) = pair(Species::Human, NpcClass::Warrior, 1500.0);
        // Empty hands, a bow in the bag: ready the bow; without a bow stay with melee.
        assert_eq!(
            select_weapon_mode(&w, me, foe, true),
            Some(WeaponMode::Ranged)
        );
        assert_eq!(
            select_weapon_mode(&w, me, foe, false),
            Some(WeaponMode::Melee)
        );
    }
}
