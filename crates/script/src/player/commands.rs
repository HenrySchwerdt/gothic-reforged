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
use crate::player::normal::{Confirm, confirm, jump_state, sneak_state};
use crate::player::use_item::{UseOutcome, use_stack};
use crate::routine::state::State;
use crate::routine::work::{Flow, run_state};

/// Run the player's current state until it yields or ends (at most a few dozen steps).
pub fn run(host: &mut dyn ScriptHost, player: EntityId) -> Flow {
    let mut flow = Flow::Done;
    for _ in 0..32 {
        flow = run_state(host, player);
        if flow == Flow::Yield || host.has_pending_ai(player) {
            // The engine runs the queued instructions before it resumes a state function.
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

/// The key handlers of `OnPlayerGameKeyPressed` (`gothic_input::player::PlayerInput::route`) do
/// not act: they put a `gEAction` id and the focus entity into the player's action queue
/// (`sub_7940`) and the loop state of the current task (`PS_Melee_Loop`, ...) reads it.
pub fn push_action(host: &mut dyn ScriptHost, id: u32, focus: EntityId) {
    host.action_queue_push(id, focus);
}

/// `FullStop` + `SetState(state)` (what the handlers do for the moves that bypass the queue:
/// the riposte out of the guard, the chains of `OnPlayerAction`).
pub fn enter_state(host: &mut dyn ScriptHost, player: EntityId, state: State) {
    host.full_stop(player);
    host.set_state(player, state);
}

/// The weapon key out of `PS_Normal_Loop` (`OnPlayerSelectWeapon`, S:63cc0).
pub fn select_weapon(host: &mut dyn ScriptHost, player: EntityId) -> bool {
    crate::player::select_weapon::select_weapon(host, player)
}

/// The use key during an orc boulder interaction: ask for the next stage (tilt, hold, lift).
pub fn advance_interaction(host: &mut dyn ScriptHost, player: EntityId) {
    host.set_task_position(player, crate::player::boulder::ADVANCE_REQUESTED);
}

// ---- magic -----------------------------------------------------------------------------------------

/// Why a spell command did nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastError {
    /// The hero holds no spell (`select_spell` first).
    NoSpell,
    /// The hero is busy with something other than the magic loop.
    Busy,
}

/// Select a spell item (rune, scroll, potion): it goes into the right hand and the hero is
/// in the `PS_Magic` task. `false` when the item cannot be equipped (requirements unmet) or has
/// no spell.
pub fn select_spell(host: &mut dyn ScriptHost, player: EntityId, template: &str) -> bool {
    if !host.equip_item(player, template) {
        return false;
    }
    let stack = host.find_stack_index(player, crate::enums::Slot::RightHand);
    if host.stack_spell(player, stack).is_empty() {
        return false;
    }
    host.set_task(player, State::PsMagic);
    true
}

/// The cast key: pick the cast state by the animation of the spell in hand
/// ([`crate::magic::cast::get_cast_action`]). `power` is a long press; a charged spell cast with
/// a short press is a plain cast. Only works from the magic loop.
pub fn cast_spell(
    host: &mut dyn ScriptHost,
    player: EntityId,
    target: EntityId,
    power: bool,
) -> Result<State, CastError> {
    use crate::enums::Action;
    if npc_weapon_mode(host, player) != WeaponMode::Magic {
        return Err(CastError::NoSpell);
    }
    if !matches!(
        host.current_state(player),
        Some(State::PsMagicLoop | State::PsMagic)
    ) {
        return Err(CastError::Busy);
    }
    let mut action = crate::magic::cast::get_cast_action(host, player);
    if action == Action::PowerCast && !power {
        action = Action::Cast;
    }
    let Some(state) = crate::player::magic::state_for_action(action) else {
        return Err(CastError::NoSpell);
    };
    host.set_current_target(player, target);
    host.set_state(player, state);
    Ok(state)
}

/// Let go of a cast in progress: the mana paid comes back.
pub fn abort_cast(host: &mut dyn ScriptHost, player: EntityId) {
    host.set_state(player, State::PsMagicSummonAbort);
}

/// Put a ranged weapon up (the draw key): bow or crossbow in the left hand, ammunition in the
/// right; `false` when the hero does not hold a ranged weapon.
pub fn ready_ranged(host: &mut dyn ScriptHost, player: EntityId) -> bool {
    if npc_weapon_mode(host, player) != WeaponMode::Ranged {
        return false;
    }
    host.set_task(player, State::PsRanged);
    true
}

/// Aim (true) or give up the draw (false): the ranged loops read the same queue entries the key
/// handlers of `OnPlayerAction` push (`gEAction` 40 `Aim`, 39 `AbortAttack`); this entry point
/// is for callers that have no key (the VR controllers).
pub fn aim_ranged(host: &mut dyn ScriptHost, player: EntityId, aim: bool) {
    if npc_weapon_mode(host, player) != WeaponMode::Ranged {
        return;
    }
    host.action_queue_push(if aim { 40 } else { 39 }, EntityId::NONE);
}

/// Release the string or pull the trigger at the focus `target` (`gEAction` 41 `Shoot`).
pub fn release_ranged(host: &mut dyn ScriptHost, player: EntityId, target: EntityId) -> bool {
    if npc_weapon_mode(host, player) != WeaponMode::Ranged {
        return false;
    }
    host.set_current_target(player, target);
    host.action_queue_push(41, target);
    true
}

/// Offer a guard money to forget a crime; `accepted` is the dialog's verdict on the price.
pub fn bribe(
    host: &mut dyn ScriptHost,
    player: EntityId,
    guard: EntityId,
    crime: crate::enums::PlayerCrime,
    accepted: bool,
) -> crate::routine::dialog::BribeOutcome {
    crate::routine::dialog::bribe(host, guard, player, crime, accepted)
}
