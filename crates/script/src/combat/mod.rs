//! B08-B10: the combat brain. `ZS_Attack` and its sub-states, the combat-move selection
//! (`SelectCombatMove`, `FAI_*`), the reaction states and the AI command primitives.

pub mod attack;
pub mod commands;
pub mod misc;
pub mod moves;
pub mod range;
pub mod react;
pub mod select;

use crate::host::{EntityId, ScriptHost};
use crate::routine::state::{Callback, State};
use crate::routine::work::Def;

/// The definition of a combat or reaction state (B08 and B10), `None` for every other state.
pub fn definition(state: State) -> Option<Def> {
    attack::definition(state)
        .or_else(|| react::definition(state))
        .or_else(|| misc::definition(state))
        .or_else(|| crate::player::definition(state))
}

/// Does this task belong to the fight and reaction machinery of this module?
pub fn is_combat_task(state: State) -> bool {
    definition(state).is_some()
}

/// Run a registered callback (`OnAttack_Goto`, `OnReactToWeapon`, ...). Local callbacks end a
/// movement instruction, task callbacks run every tick; `true` means the callback changed the
/// state.
pub fn run_callback(host: &mut dyn ScriptHost, npc: EntityId, cb: Callback) -> bool {
    attack::run_local_callback(host, npc, cb)
        || react::run_callback(host, npc, cb)
        || misc::run_callback(host, npc, cb)
}

#[cfg(test)]
mod react_tests;
#[cfg(test)]
mod select_tests;
