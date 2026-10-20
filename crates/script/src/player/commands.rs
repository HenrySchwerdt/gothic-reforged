//! Entry points for the play mode: one function per player action. The runtime commands
//! (`UseInteractObject`, `PickUp`, `Lockpick`, `Craft`, ...) call these; each sets up the player
//! entity's task/state the way the DLL's input handlers do and leaves the stepping to the
//! interpreter ([`run`] drives it to the next yield).

use crate::combat::select::npc_weapon_mode;
use crate::helpers::WeaponMode;
use crate::host::{EntityId, ScriptHost};
use crate::player::craft::{Synth, synthesize};
use crate::player::evade::{self, Direction};
use crate::player::interact::{EXIT_REQUESTED, unlock};
use crate::player::melee::state_for_action;
use crate::player::normal::{Confirm, confirm, jump_state, sneak_state};
use crate::player::use_item::{UseOutcome, use_stack};
use crate::routine::state::State;
use crate::routine::work::{Flow, run_state};

/// Run the player's current state until it yields or ends (at most a few dozen steps).
pub fn run(host: &mut dyn ScriptHost, player: EntityId) -> Flow {
    let mut flow = Flow::Done;
    for _ in 0..32 {
        flow = run_state(host, player);
        if flow == Flow::Yield {
            break;
        }
        if host.current_state(player).is_none() {
            break;
        }
    }
    flow
}

/// The confirm key on the focused entity (`PS_Normal_Loop`).
pub fn press_confirm(host: &mut dyn ScriptHost, player: EntityId, focus: EntityId) -> Confirm {
    confirm(host, player, focus)
}

/// Walk into `PS_Interact` for an object (the interaction half of the confirm key).
pub fn use_object(host: &mut dyn ScriptHost, player: EntityId, object: EntityId) {
    host.set_current_target(player, object);
    host.set_task(player, State::PsInteract);
}

/// Leave a persistent interaction (seat, bed, window): the loop notices and ends.
pub fn end_interaction(host: &mut dyn ScriptHost, player: EntityId) {
    host.set_task_position(player, EXIT_REQUESTED);
}

/// Pick a loose item up.
pub fn pick_up(host: &mut dyn ScriptHost, player: EntityId, item: EntityId) {
    host.set_current_target(player, item);
    host.set_state(player, State::PsNormalTake);
}

/// Try to open the lock of `object` with key or lockpick. `minigame` is the result of the
/// lockpicking mini-game when the UI ran one; without it `_AI_Unlock` rolls itself.
pub fn lockpick(
    host: &mut dyn ScriptHost,
    player: EntityId,
    object: EntityId,
    minigame: Option<bool>,
) -> bool {
    match minigame {
        Some(false) => {
            host.remove_item(player, crate::player::interact::LOCKPICK, 1);
            host.emit(crate::player::api::PlayerEvent::LockPicked {
                object,
                success: false,
            });
            false
        }
        Some(true) => {
            host.remove_item(player, crate::player::interact::LOCKPICK, 1);
            host.set_lock_status(object, crate::player::api::LockStatus::Unlocked);
            host.emit(crate::player::api::PlayerEvent::LockPicked {
                object,
                success: true,
            });
            true
        }
        None => unlock(host, player, object),
    }
}

/// Make recipe `id` at the current work station.
pub fn craft(host: &mut dyn ScriptHost, player: EntityId, id: i32) -> Synth {
    synthesize(host, player, id)
}

/// Use an inventory item.
pub fn use_item(host: &mut dyn ScriptHost, player: EntityId, template: &str) -> UseOutcome {
    use_stack(host, player, template)
}

/// Toggle sneaking.
pub fn toggle_sneak(host: &mut dyn ScriptHost, player: EntityId) {
    host.set_state(player, sneak_state(npc_weapon_mode(host, player)));
}

/// Jump.
pub fn jump(host: &mut dyn ScriptHost, player: EntityId) {
    host.set_state(player, jump_state(npc_weapon_mode(host, player)));
}

/// Dodge; `false` when blocked or out of stamina.
pub fn dodge(host: &mut dyn ScriptHost, player: EntityId, direction: Direction) -> bool {
    evade::start(host, player, direction)
}

/// Start a melee move (`Action` of the attack family) against `target`.
pub fn melee_move(
    host: &mut dyn ScriptHost,
    player: EntityId,
    action: crate::enums::Action,
    target: EntityId,
) -> bool {
    let Some(state) = state_for_action(action) else {
        return false;
    };
    if npc_weapon_mode(host, player) != WeaponMode::Melee {
        return false;
    }
    host.set_current_target(player, target);
    host.set_state(player, state);
    true
}

/// Raise the guard.
pub fn parade(host: &mut dyn ScriptHost, player: EntityId, target: EntityId) {
    host.set_current_target(player, target);
    host.set_state(player, State::PsMeleeParade);
}
