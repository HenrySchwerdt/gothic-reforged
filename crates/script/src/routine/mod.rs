//! Phase 1: daily routines.
//!
//! * [`time`]: `GetRoutineTime` and the walk-mode table.
//! * [`state`]: state and callback identifiers.
//! * [`select`]: B03 routine engine (task and destination selection, `ContinueRoutine`).
//! * [`party`]: B03 player party join/leave.
//! * [`work`]: B04 + B05 task bodies as data plus the step interpreter.

pub mod dialog;
pub mod follow;
pub mod info;
pub mod party;
pub mod select;
pub mod state;
pub mod time;
pub mod work;

use crate::host::{EntityId, ScriptHost};
use state::Callback;

/// Run the per-tick callbacks of the routine and companion states (`OnSleep`, `OnAnimalSleep`,
/// `OnFollowPlayer`, `OnFollowNPC`, `OnGuidePlayer`); `true` means the callback changed the
/// state. `OnGateGuard` is not run here: it only decides when the guard's active perception scan
/// runs ([`crate::perception::on_gate_guard`], asked by the host before the scan).
pub fn run_callback(host: &mut dyn ScriptHost, npc: EntityId, cb: Callback) -> bool {
    match cb {
        Callback::OnSleep => crate::perception::on_sleep(host, npc),
        Callback::OnAnimalSleep => crate::perception::on_animal_sleep(host, npc),
        Callback::OnFollowPlayer => follow::on_follow_player(host, npc),
        Callback::OnFollowNpc => {
            follow::on_follow_npc(host, npc);
            false
        }
        Callback::OnGuidePlayer => follow::on_guide_player(host, npc),
        _ => false,
    }
}
