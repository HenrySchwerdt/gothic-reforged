//! `PS_Melee*` (B16, melee half): the hero's fight states.
//!
//! `PS_Melee` draws the weapon and settles in `PS_Melee_Loop`; every attack, the parade and the
//! jump back are one-shot states that run the matching `_AI_*` command of B09 against the focus
//! enemy and return to the loop. The input handler (`OnPlayerAction`) decides which move a key
//! press means; [`move_for_press`] is its rule (tap = quick attack, hold = power attack).

use crate::combat::commands::{start_jump, start_move, start_parade};
use crate::combat::select::npc_weapon_mode;
use crate::enums::Action;
use crate::helpers::WeaponMode;
use crate::host::{AiCommand, EntityId, ScriptHost};
use crate::player::step;
use crate::routine::state::State;
use crate::routine::work::{Def, Flow};

/// A press of the attack key shorter than this many milliseconds is a tap.
pub const TAP_MS: u32 = 200;

/// The state of each attack move.
pub const MOVES: &[(State, Action)] = &[
    (State::PsMeleeAttack, Action::Attack),
    (State::PsMeleeQuickAttack, Action::QuickAttack),
    (State::PsMeleePowerAttack, Action::PowerAttack),
    (State::PsMeleeSimpleWhirl, Action::SimpleWhirl),
    (State::PsMeleeWhirlAttack, Action::WhirlAttack),
    (State::PsMeleePierceAttack, Action::PierceAttack),
    (State::PsMeleeHackAttack, Action::HackAttack),
    (State::PsMeleeFinishingAttack, Action::FinishingAttack),
];

/// Port of the key-release rule of `OnPlayerAction` (S:62480): a tap is a quick attack, a long
/// press a power attack. Whirl, pierce and hack follow from the held direction and the combo
/// window; those are chosen by the caller ([I]).
pub fn move_for_press(held_ms: u32) -> Action {
    if held_ms < TAP_MS {
        Action::QuickAttack
    } else {
        Action::PowerAttack
    }
}

/// The state that performs an action.
pub fn state_for_action(action: Action) -> Option<State> {
    MOVES.iter().find(|(_, a)| *a == action).map(|(s, _)| *s)
}

/// The enemy the hero swings at: the state's target or the focus entity.
fn opponent(host: &dyn ScriptHost, p: EntityId) -> EntityId {
    let t = host.current_target(p);
    if t.is_some() { t } else { host.focus_entity() }
}

/// Port of `PS_Melee` (S:7e280): without a melee weapon in hand the hero returns to
/// `PS_Normal`; otherwise it settles in the loop (the first fight also shows the `TUT_Fight`
/// tutorial in the DLL; that is HUD business).
fn ps_melee(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    if npc_weapon_mode(host, p) != WeaponMode::Melee {
        host.set_task(p, State::PsNormal);
    } else {
        host.set_state(p, State::PsMeleeLoop);
    }
    Flow::Done
}

/// Port of `PS_Melee_Loop` (S:7e750): wait for the next input; the weapon put away ends the mode.
fn ps_melee_loop(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    if npc_weapon_mode(host, p) != WeaponMode::Melee {
        host.set_task(p, State::PsNormal);
        return Flow::Done;
    }
    host.queue_ai(p, AiCommand::WaitMs { ms: 100 });
    Flow::Yield
}

/// The attack states (`PS_Melee_Attack` and siblings): start the move (stamina, hit window) and
/// swing at the opponent; then back to the loop.
fn ps_melee_move(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    let Some(action) = host
        .current_state(p)
        .and_then(|s| MOVES.iter().find(|(st, _)| *st == s))
        .map(|(_, a)| *a)
    else {
        return Flow::Done;
    };
    match step(host, p) {
        0 => {
            let target = opponent(host, p);
            start_move(host, p, action);
            host.queue_ai(p, AiCommand::Attack { action, target });
            Flow::Yield
        }
        _ => {
            host.set_state(p, State::PsMeleeLoop);
            Flow::Done
        }
    }
}

/// Port of `PS_Melee_Parade` (S:7f960): raise the guard against the opponent.
fn ps_melee_parade(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    match step(host, p) {
        0 => {
            let target = opponent(host, p);
            start_parade(host, p);
            host.queue_ai(p, AiCommand::Parade { target });
            Flow::Yield
        }
        _ => {
            host.set_state(p, State::PsMeleeLoop);
            Flow::Done
        }
    }
}

/// Port of `PS_Melee_JumpBack` (S:7fc20): hop away from the opponent (stamina per the B09 table).
fn ps_melee_jump_back(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    match step(host, p) {
        0 => {
            let target = opponent(host, p);
            start_jump(host, p, true);
            host.queue_ai(p, AiCommand::JumpBack { target });
            Flow::Yield
        }
        _ => {
            host.set_state(p, State::PsMeleeLoop);
            Flow::Done
        }
    }
}

/// Definitions of the melee states.
pub fn definition(state: State) -> Option<Def> {
    use State as S;
    let custom = |f: fn(&mut dyn ScriptHost, EntityId) -> Flow| Some(Def::Custom(f));
    match state {
        S::PsMelee => custom(ps_melee),
        S::PsMeleeLoop => custom(ps_melee_loop),
        S::PsMeleeParade => custom(ps_melee_parade),
        S::PsMeleeJumpBack => custom(ps_melee_jump_back),
        s if MOVES.iter().any(|(st, _)| *st == s) => custom(ps_melee_move),
        _ => None,
    }
}
