//! Phase 1: daily routines.
//!
//! * [`time`]: `GetRoutineTime` and the walk-mode table.
//! * [`state`]: state and callback identifiers.
//! * [`select`]: B03 routine engine (task and destination selection, `ContinueRoutine`).
//! * [`party`]: B03 player party join/leave.
//! * [`work`]: B04 + B05 task bodies as data plus the step interpreter.

pub mod party;
pub mod select;
pub mod state;
pub mod time;
pub mod work;
