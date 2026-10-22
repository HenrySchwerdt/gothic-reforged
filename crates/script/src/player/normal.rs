//! `PS_Normal` and its sub-states: the hero's baseline (B15) and the confirm key.
//!
//! * [`ps_normal`]: pick the combat state that matches the weapon in hand, else idle in
//!   `PS_Normal_Loop` (the loop only waits; input arrives as commands).
//! * [`confirm`]: the decision `PS_Normal_Loop` makes on the confirm key from the focus: loot a
//!   dead or knocked-out body, pick an item up, use an interact object, talk to a character
//!   (`AssessTalk`) or rob a sleeper (`Perk_MasterThief`).
//! * [`ps_normal_take`], [`ps_gonna_loot`], jump and sneak.

use crate::combat::select::npc_weapon_mode;
use crate::crime::add_enclave_theft_count;
use crate::enums::{Action, AiMode, AniState};
use crate::helpers::WeaponMode;
use crate::host::{AiCommand, EntityId, ScriptHost};
use crate::player::api::{FocusKind, PlayerEvent, UiWindow};
use crate::player::step;
use crate::routine::state::State;
use crate::routine::work::{Def, Flow};
use crate::speech::{TalkOutcome, assess_talk, start_say_nothing_to_get};

/// What the confirm key did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirm {
    /// Nothing in focus or nothing to do.
    Nothing,
    /// The hero walks into `PS_Interact` for this object.
    Interact(EntityId),
    /// The hero takes the item.
    PickUp(EntityId),
    /// The loot window of a body opens.
    Loot(EntityId),
    /// The hero robs a sleeping character.
    Rob(EntityId),
    /// Both enter `ZS_Talk`.
    Talk(EntityId),
    /// The character did not want to talk.
    TalkRefused(EntityId),
    /// A body with nothing worth taking.
    NothingToGet(EntityId),
}

/// The jump state of the current weapon mode.
pub fn jump_state(mode: WeaponMode) -> State {
    match mode {
        WeaponMode::Melee => State::PsMeleeJump,
        WeaponMode::Ranged => State::PsRangedJump,
        WeaponMode::Magic => State::PsMagicJump,
        WeaponMode::None => State::PsNormalJump,
    }
}

/// The sneak state of the current weapon mode.
pub fn sneak_state(mode: WeaponMode) -> State {
    match mode {
        WeaponMode::Melee => State::PsMeleeSneak,
        WeaponMode::Ranged => State::PsRangedSneak,
        WeaponMode::Magic => State::PsMagicSneak,
        WeaponMode::None => State::PsNormalSneak,
    }
}

/// The combat task of a weapon mode (`None`: the hero is unarmed).
pub fn combat_task(mode: WeaponMode) -> Option<State> {
    match mode {
        WeaponMode::Melee => Some(State::PsMelee),
        WeaponMode::Ranged => Some(State::PsRanged),
        WeaponMode::Magic => Some(State::PsMagic),
        WeaponMode::None => None,
    }
}

/// Port of `PS_Normal` (S:80cc0): the hero is free to move. A weapon in hand hands the control to
/// `PS_Melee`, `PS_Ranged` or `PS_Magic`; otherwise the hero stands up (when kneeling or sitting)
/// and idles in `PS_Normal_Loop` with the focus system on.
fn ps_normal(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_normal");
    if let Some(task) = combat_task(npc_weapon_mode(host, p)) {
        host.set_task(p, task);
        return Flow::Done;
    }
    match step(host, p) {
        0 => {
            host.set_ai_mode(p, AiMode::Routine);
            if !matches!(
                host.ani_state(p),
                AniState::Stand | AniState::Sneak | AniState::Dummy0 | AniState::Dummy1
            ) {
                host.queue_ai(p, AiCommand::StandUp);
                return Flow::Yield;
            }
            host.set_state(p, State::PsNormalLoop);
            Flow::Done
        }
        _ => {
            host.set_state(p, State::PsNormalLoop);
            Flow::Done
        }
    }
}

/// Port of `PS_Normal_Loop` (S:815f0): idle. The input handlers are commands; the loop only keeps
/// the state alive (a poll command so the host does not spin).
fn ps_normal_loop(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_normal_loop");
    if let Some(task) = combat_task(npc_weapon_mode(host, p)) {
        host.set_task(p, task);
        return Flow::Done;
    }
    host.queue_ai(p, AiCommand::WaitMs { ms: 100 });
    Flow::Yield
}

/// Port of `PS_Normal_Take` (S:82240): stop, take the item (`_AI_TakeItem`) and count a theft
/// when it did not lie there as a dropped item (`AddEnclaveTheftCount`: only secured zones and
/// enclaves count it), then back to the loop.
fn ps_normal_take(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_normal_take");
    let item = host.current_target(p);
    match step(host, p) {
        0 => {
            host.full_stop(p);
            host.queue_ai(p, AiCommand::TakeItem { item });
            Flow::Yield
        }
        _ => {
            if host.exists(item) {
                let template = host.item_template(item);
                let stolen = !host.item_is_dropped(item);
                if stolen {
                    add_enclave_theft_count(host, p, p, 1);
                }
                host.take_item(p, item);
                host.emit(PlayerEvent::PickedUp { template, stolen });
            }
            host.set_current_target(p, EntityId::NONE);
            host.set_state(p, State::PsNormalLoop);
            Flow::Done
        }
    }
}

/// Port of `PS_GonnaLoot` (S:83120): stop and open the loot window of the focused body.
fn ps_gonna_loot(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_gonna_loot");
    host.full_stop(p);
    host.emit(PlayerEvent::Window(UiWindow::Loot));
    host.set_state(p, State::PsNormalLoop);
    Flow::Done
}

/// Port of `PS_*_Jump` (the five jump states, S:825f0 and siblings): pay the stamina, jump,
/// return to the loop of the weapon mode.
fn ps_jump(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_jump");
    match step(host, p) {
        0 => {
            crate::combat::commands::start_jump(host, p, false);
            host.queue_ai(
                p,
                AiCommand::Action {
                    action: Action::Jump,
                    secs: 1,
                },
            );
            Flow::Yield
        }
        _ => {
            back_to_loop(host, p);
            Flow::Done
        }
    }
}

/// Port of `PS_*_Sneak`: switch between the sneaking and the standing pose.
fn ps_sneak(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_sneak");
    let sneaking = host.ani_state(p) == AniState::Sneak;
    host.set_ani_state(
        p,
        if sneaking {
            AniState::Stand
        } else {
            AniState::Sneak
        },
    );
    back_to_loop(host, p);
    Flow::Done
}

/// Return to the idle state of the current weapon mode.
fn back_to_loop(host: &mut dyn ScriptHost, p: EntityId) {
    let state = match npc_weapon_mode(host, p) {
        WeaponMode::Melee => State::PsMeleeLoop,
        WeaponMode::Ranged => State::PsRangedAimLoop,
        WeaponMode::Magic => State::PsMagicLoop,
        WeaponMode::None => State::PsNormalLoop,
    };
    host.set_state(p, state);
}

/// Take every stack a looter may take from a body (the quick-loot key): everything but
/// merchandise and the categories 0 and 9. A sleeper's belongings count as a theft. Returns the
/// number of stacks moved; zero makes the hero say "NothingToGet".
pub fn quick_loot(host: &mut dyn ScriptHost, p: EntityId, body: EntityId) -> usize {
    let stacks = host.lootable(body);
    let theft = !matches!(host.ai_mode(body), AiMode::Dead | AiMode::Down);
    for (template, amount) in &stacks {
        host.transfer_item(body, p, template, *amount);
    }
    if stacks.is_empty() {
        start_say_nothing_to_get(host, p);
    } else if theft {
        add_enclave_theft_count(host, p, body, stacks.len() as i32);
    }
    stacks.len()
}

/// Port of the confirm branch of `PS_Normal_Loop` (S:815f0): what the confirm key does with the
/// focused entity. Objects and items switch the hero into `PS_Interact` / `PS_Normal_Take`; a
/// dead or knocked-out character opens the loot window (or "NothingToGet"); a sleeping one is
/// robbed with `Perk_MasterThief` and talked to otherwise; a living one is addressed
/// (`AssessTalk`), except fighters, who ignore the hero.
pub fn confirm(host: &mut dyn ScriptHost, p: EntityId, focus: EntityId) -> Confirm {
    gothic_portcov::hit!("confirm");
    match host.focus_kind(focus) {
        FocusKind::None => Confirm::Nothing,
        FocusKind::Item => {
            host.set_current_target(p, focus);
            host.set_state(p, State::PsNormalTake);
            Confirm::PickUp(focus)
        }
        FocusKind::Object => {
            host.set_current_target(p, focus);
            host.set_task(p, State::PsInteract);
            Confirm::Interact(focus)
        }
        FocusKind::Npc => match host.ai_mode(focus) {
            AiMode::Dead | AiMode::Down => {
                if host.lootable(focus).is_empty() {
                    start_say_nothing_to_get(host, p);
                    return Confirm::NothingToGet(focus);
                }
                host.set_current_target(p, focus);
                host.set_state(p, State::PsGonnaLoot);
                Confirm::Loot(focus)
            }
            AiMode::Sleep if host.is_skill_active(p, "Perk_MasterThief") => {
                if host.lootable(focus).is_empty() {
                    start_say_nothing_to_get(host, p);
                    return Confirm::NothingToGet(focus);
                }
                host.set_current_target(p, focus);
                host.set_state(p, State::PsGonnaLoot);
                Confirm::Rob(focus)
            }
            AiMode::Combat | AiMode::Flee | AiMode::Arena => Confirm::Nothing,
            _ => match assess_talk(host, focus, p, true) {
                TalkOutcome::Talk => Confirm::Talk(focus),
                TalkOutcome::Refused => Confirm::TalkRefused(focus),
                TalkOutcome::Ignored => Confirm::Nothing,
            },
        },
    }
}

/// Definitions of the baseline states.
pub fn definition(state: State) -> Option<Def> {
    use State as S;
    let custom = |f: fn(&mut dyn ScriptHost, EntityId) -> Flow| Some(Def::Custom(f));
    match state {
        S::PsNormal => custom(ps_normal),
        S::PsNormalLoop => custom(ps_normal_loop),
        S::PsNormalTake => custom(ps_normal_take),
        S::PsGonnaLoot => custom(ps_gonna_loot),
        S::PsNormalJump | S::PsMeleeJump | S::PsMagicJump => custom(ps_jump),
        S::PsNormalSneak | S::PsMeleeSneak | S::PsMagicSneak => custom(ps_sneak),
        _ => None,
    }
}
