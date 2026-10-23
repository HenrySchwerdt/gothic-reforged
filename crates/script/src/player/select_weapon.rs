//! `OnPlayerSelectWeapon` (S:63cc0, `Script/Player/Player_Interface/OnPlayerGameKeyPressed.cpp`):
//! what the weapon key does. The key handler queues action 68 (`Hold`); the loop state of the
//! current task reads it and calls this.
//!
//! * From `PS_Normal_Loop` the remembered `LastWeaponConfig` (0 melee, 1 ranged, 2 magic) picks
//!   the combat task, the queue is emptied (`sub_7a00`) and the task is set; the combat task
//!   itself runs `_AI_HoldInventoryItems` (the draw animation moves the item from the back slot
//!   into the hand).
//! * From `PS_Melee_Loop`: a transformed hero does nothing; a staff in the right hand and a torch
//!   in the left hand stay in the hand, everything else is put away
//!   (`_AI_HoldInventoryItems(-1, -1)`), the queue is emptied and the task is `PS_Normal`.
//!
//! Not ported: the equip search of the draw (`PrintMissingRequirements`, `AssureItems` for
//! fists, the ranged and magic stack choice), which needs the requirement texts of the HUD.

use crate::enums::{Slot, UseType};
use crate::host::{AiCommand, EntityId, ScriptHost};
use crate::routine::state::State;

/// Port of the `PS_Melee_Loop` branch (S:63cc0): holster.
pub fn holster_from_melee(host: &mut dyn ScriptHost, p: EntityId) {
    gothic_portcov::hit!("holster_from_melee");
    if host.is_transformed(p) {
        return;
    }
    let mut right = -1;
    let mut left = -1;
    if host.slot_use_type(p, Slot::RightHand) == UseType::Staff {
        right = host.find_stack_index(p, Slot::RightHand);
    }
    if host.slot_use_type(p, Slot::LeftHand) == UseType::Torch {
        left = host.find_stack_index(p, Slot::LeftHand);
    }
    host.queue_ai(p, AiCommand::HoldInventoryStacks { right, left });
    host.action_queue_clear();
    host.set_task(p, State::PsNormal);
}

/// Port of the `PS_Normal_Loop` branch (S:63cc0): the combat task of the remembered weapon
/// configuration.
pub fn draw_from_normal(host: &mut dyn ScriptHost, p: EntityId) {
    gothic_portcov::hit!("draw_from_normal");
    let task = match host.last_weapon_config() {
        1 => State::PsRanged,
        2 => State::PsMagic,
        _ => State::PsMelee,
    };
    host.action_queue_clear();
    host.set_task(p, task);
}

/// Port of `OnPlayerSelectWeapon` (S:63cc0): the weapon key in the current state of the hero.
/// Returns whether it changed the task.
pub fn select_weapon(host: &mut dyn ScriptHost, p: EntityId) -> bool {
    gothic_portcov::hit!("select_weapon");
    match host.current_state(p) {
        Some(State::PsNormalLoop) => {
            draw_from_normal(host, p);
            true
        }
        Some(State::PsMeleeLoop) => {
            holster_from_melee(host, p);
            host.current_task(p) == Some(State::PsNormal)
        }
        _ => false,
    }
}
