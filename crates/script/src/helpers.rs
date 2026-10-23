//! B00: shared helpers that more than one script batch calls (the DLL's unregistered `sub_*`
//! functions). Only the gameplay helpers are ported; the array/string plumbing disappears in Rust.

use crate::enums::UseType;
use crate::host::{EntityId, ScriptHost};
use crate::routine::time::{RoutineTime, routine_time};

/// Hand-item combination class from the DLL's static lookup table (COMBAT_AI_RULES 2.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandClass {
    /// 0: invalid combination.
    Invalid,
    /// 1: using or holding an ordinary item.
    UseItem,
    /// 2: one-hand carry or tool idle.
    Idle,
    /// 3: carrying (stone, sack).
    Carry,
    /// 4: melee weapon ready.
    MeleeReady,
    /// 5: ranged weapon ready.
    RangedReady,
    /// 6: magic ready.
    MagicReady,
    /// 7: ammunition only.
    AmmoOnly,
    /// 8: nothing held.
    Nothing,
}

/// `WeaponMode` (0 none, 1 melee, 2 ranged, 3 magic).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponMode {
    /// Unarmed / not a weapon pose.
    None,
    /// Melee ready.
    Melee,
    /// Ranged ready.
    Ranged,
    /// Magic ready.
    Magic,
}

/// Port of the hand-item lookup (S:2dd0; table built at S:3865..40de): classify the
/// `(left, right)` hand item use types.
///
/// Rows follow the table in COMBAT_AI_RULES 2.12; combinations not listed are `Invalid`.
pub fn hand_class(left: UseType, right: UseType) -> HandClass {
    gothic_portcov::hit!("hand_class");
    use HandClass::*;
    use UseType as U;
    let tool = |u: UseType| matches!(u, U::Broom | U::Rake | U::Shovel | U::Pickaxe);
    let usable = |u: UseType| {
        matches!(
            u,
            U::Potion
                | U::Plant
                | U::Meat
                | U::Fruit
                | U::Bread
                | U::Bottle
                | U::Cup
                | U::Bowl
                | U::Brush
                | U::Smoke
                | U::OrcPipe
                | U::Scoop
                | U::Fan
                | U::Saw
                | U::Map
                | U::Book
                | U::Letter
                | U::Hammer
        )
    };
    match (left, right) {
        (U::None, U::None) => Idle,
        // Melee ready.
        (U::None | U::Shield | U::Torch | U::OneH, U::OneH) => MeleeReady,
        (U::None, U::TwoH | U::Axe | U::Staff | U::Halberd | U::Fist) => MeleeReady,
        (U::PhysicalFist, U::PhysicalFist) => MeleeReady,
        (U::None, r) if tool(r) => MeleeReady,
        // Ranged ready / ammo only.
        (U::Bow, U::None | U::Arrow) | (U::CrossBow, U::None | U::Bolt) => RangedReady,
        (U::None, U::Arrow | U::Bolt) => AmmoOnly,
        // Magic ready.
        (U::None | U::Staff, U::Cast) => MagicReady,
        // Idle.
        (U::OneH, U::Hammer) => Idle,
        (l, U::None) if tool(l) => Idle,
        (U::Torch | U::TwoH | U::Axe | U::Staff | U::Halberd, U::None) => Idle,
        // Carry.
        (U::CarryFront | U::CarryShoulder, U::None) => Carry,
        (U::Shield, U::None) => Invalid,
        // Use items.
        (U::OneH, U::None) => UseItem,
        (U::None | U::Torch | U::Staff, r) if usable(r) => UseItem,
        (l, U::None) if usable(l) => UseItem,
        _ => Invalid,
    }
}

/// Port of `WeaponMode` (S:3240): map a hand class to the weapon mode.
pub fn weapon_mode(left: UseType, right: UseType) -> WeaponMode {
    gothic_portcov::hit!("weapon_mode");
    match hand_class(left, right) {
        HandClass::MeleeReady => WeaponMode::Melee,
        HandClass::RangedReady => WeaponMode::Ranged,
        HandClass::MagicReady => WeaponMode::Magic,
        _ => WeaponMode::None,
    }
}

/// Port of the work-duration helper (S:76b0): long work animations last `(rand(3) + 2) * 60`
/// seconds, 10 s when the debug byte is set. Used by Cook, Grind, Barbecue, Bard and others.
pub fn long_work_secs(host: &mut dyn ScriptHost) -> i32 {
    gothic_portcov::hit!("long_work_secs");
    if host.variants().fast_routines {
        10
    } else {
        (host.random(3) + 2) * 60
    }
}

/// Port of the duration helper (S:76e0): ambient creatures idle `rand(11) + 10` seconds, every
/// other NPC `rand(31) + 30`; 10 s with the debug byte. `ambient` is `IsAmbientCreature`.
pub fn ambient_action_secs(host: &mut dyn ScriptHost, ambient: bool) -> i32 {
    gothic_portcov::hit!("ambient_action_secs");
    if host.variants().fast_routines {
        10
    } else if ambient {
        host.random(11) + 10
    } else {
        host.random(31) + 30
    }
}

/// Medium work duration used inline by most use-type states: `rand(31) + 30` seconds (10 s with
/// the debug byte).
pub fn medium_work_secs(host: &mut dyn ScriptHost) -> i32 {
    if host.variants().fast_routines {
        10
    } else {
        host.random(31) + 30
    }
}

/// Port of the routine-point helper (S:8600): the NPC's working, relaxing or sleeping point
/// depending on `GetRoutineTime` (0 sleeping, 2 relaxing, otherwise working).
pub fn routine_point(host: &dyn ScriptHost, npc: EntityId) -> EntityId {
    gothic_portcov::hit!("routine_point");
    let time = routine_time(host.species(npc), host.world_hour());
    host.routine_point(npc, time)
}

/// Port of the use-object accessor (S:8190): the interaction object currently found for `npc`
/// for a use type (the scanner S:7c90 fills the table), or `NONE`.
pub fn use_object(host: &dyn ScriptHost, npc: EntityId, use_type: UseType) -> EntityId {
    gothic_portcov::hit!("use_object");
    host.find_use_object(npc, use_type)
}

/// Routine point selector exposed for hosts that store the three points directly.
pub fn point_for_time(
    working: EntityId,
    relaxing: EntityId,
    sleeping: EntityId,
    time: RoutineTime,
) -> EntityId {
    match time {
        RoutineTime::Sleep => sleeping,
        RoutineTime::Relax => relaxing,
        RoutineTime::Work => working,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use UseType as U;

    #[test]
    fn weapon_mode_table() {
        assert_eq!(weapon_mode(U::None, U::OneH), WeaponMode::Melee);
        assert_eq!(weapon_mode(U::Shield, U::OneH), WeaponMode::Melee);
        assert_eq!(weapon_mode(U::None, U::TwoH), WeaponMode::Melee);
        assert_eq!(
            weapon_mode(U::PhysicalFist, U::PhysicalFist),
            WeaponMode::Melee
        );
        assert_eq!(weapon_mode(U::Bow, U::Arrow), WeaponMode::Ranged);
        assert_eq!(weapon_mode(U::CrossBow, U::None), WeaponMode::Ranged);
        assert_eq!(weapon_mode(U::Staff, U::Cast), WeaponMode::Magic);
        assert_eq!(weapon_mode(U::None, U::Arrow), WeaponMode::None);
        assert_eq!(hand_class(U::None, U::Arrow), HandClass::AmmoOnly);
        assert_eq!(hand_class(U::None, U::None), HandClass::Idle);
        assert_eq!(hand_class(U::OneH, U::None), HandClass::UseItem);
        assert_eq!(hand_class(U::OneH, U::Hammer), HandClass::Idle);
        assert_eq!(hand_class(U::Shield, U::None), HandClass::Invalid);
        assert_eq!(hand_class(U::CarryFront, U::None), HandClass::Carry);
        assert_eq!(hand_class(U::None, U::Potion), HandClass::UseItem);
    }

    #[test]
    fn point_by_time() {
        let (w, r, s) = (EntityId(1), EntityId(2), EntityId(3));
        assert_eq!(point_for_time(w, r, s, RoutineTime::Work), w);
        assert_eq!(point_for_time(w, r, s, RoutineTime::Relax), r);
        assert_eq!(point_for_time(w, r, s, RoutineTime::Sleep), s);
    }
}
