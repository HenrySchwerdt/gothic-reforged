//! The five `ZS_Evade*` states (B08, the part that was left for the player's roll): a dodge in
//! one of four directions.
//!
//! The DLL starts a root-motion animation (`EvadeLeft`, `Hero_EvadeLeftSpin...`) with the
//! character's own movement switched off, lets the motion drive the body while four ray probes
//! at knee and chest height keep it out of walls, and when the animation has fewer than a
//! millisecond left restores walking, puts the body back on the floor and returns to the melee
//! loop. The port keeps the decision logic: whether the dodge may start (free space in that
//! direction, stamina left), what it costs, how long it lasts and where the state returns to.
//! The motion itself and the probes belong to the host (`CombatApi::can_evade`).
//!
//! `[I]`: the stamina price and the duration are not readable from the decompilation (the entry
//! calls a script function through a name table); the price of a jump back without
//! `ExtendedContent` (10) and one second are used.

use crate::combat::select::npc_weapon_mode;
use crate::enums::Action;
use crate::helpers::WeaponMode;
use crate::host::{AiCommand, EntityId, ScriptHost, dir};
use crate::player::step;
use crate::routine::state::State;
use crate::routine::work::{Def, Flow};
use crate::stats::{add_stamina_points, pool};

/// Stamina a dodge costs [I].
pub const EVADE_STAMINA: i32 = 10;
/// Seconds the dodge animation takes [I].
pub const EVADE_SECS: i32 = 1;

/// Direction of a dodge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// To the left (`ZS_EvadeLeft`).
    Left,
    /// To the right.
    Right,
    /// Forward, through the enemy's guard.
    Forward,
    /// Straight back.
    Backward,
}

impl Direction {
    /// The state that starts this dodge.
    pub const fn state(self) -> State {
        match self {
            Direction::Left => State::EvadeLeft,
            Direction::Right => State::EvadeRight,
            Direction::Forward => State::EvadeForward,
            Direction::Backward => State::EvadeBackward,
        }
    }

    /// The direction code `CombatApi::can_evade` probes.
    pub const fn probe(self) -> u32 {
        match self {
            Direction::Left => dir::LEFT_SIDE,
            Direction::Right => dir::RIGHT_SIDE,
            Direction::Forward => dir::FORWARD,
            Direction::Backward => dir::BACK,
        }
    }

    fn of_state(state: State) -> Option<Direction> {
        match state {
            State::EvadeLeft => Some(Direction::Left),
            State::EvadeRight => Some(Direction::Right),
            State::EvadeForward => Some(Direction::Forward),
            State::EvadeBackward => Some(Direction::Backward),
            _ => None,
        }
    }
}

/// Start a dodge: the entry state of that direction becomes the current state (the caller runs
/// it). Refused (`false`) when the character is out of stamina or the space is blocked.
pub fn start(host: &mut dyn ScriptHost, e: EntityId, direction: Direction) -> bool {
    let from = opponent(host, e);
    if pool(host, e, crate::host::Pool::Stamina) <= 0 || !host.can_evade(e, from, direction.probe())
    {
        return false;
    }
    host.set_state(e, direction.state());
    true
}

fn opponent(host: &dyn ScriptHost, e: EntityId) -> EntityId {
    let t = host.current_target(e);
    if t.is_some() { t } else { host.focus_entity() }
}

/// Port of `ZS_EvadeLeft` (S:401d0), `ZS_EvadeRight` (S:405b0), `ZS_EvadeForward` (S:40970) and
/// `ZS_EvadeBackward` (S:40d10): pay the stamina, play the dodge and hand over to
/// `ZS_EvadeLoop`. A dodge into a wall or without stamina falls back to the loop at once.
fn evade_entry(host: &mut dyn ScriptHost, e: EntityId) -> Flow {
    let Some(direction) = host.current_state(e).and_then(Direction::of_state) else {
        return Flow::Done;
    };
    match step(host, e) {
        0 => {
            let from = opponent(host, e);
            if pool(host, e, crate::host::Pool::Stamina) <= 0
                || !host.can_evade(e, from, direction.probe())
            {
                finish(host, e);
                return Flow::Done;
            }
            add_stamina_points(host, e, -EVADE_STAMINA);
            host.set_current_action(e, Action::Evade);
            host.queue_ai(
                e,
                AiCommand::Action {
                    action: Action::Evade,
                    secs: EVADE_SECS,
                },
            );
            Flow::Yield
        }
        _ => {
            host.set_state(e, State::EvadeLoop);
            Flow::Done
        }
    }
}

/// Port of `ZS_EvadeLoop` (S:3fb00): the motion runs; once it is over the character walks again
/// and returns to `PS_Melee_Loop` (the hero) or to its previous task (everybody else).
fn evade_loop(host: &mut dyn ScriptHost, e: EntityId) -> Flow {
    match step(host, e) {
        0 => {
            host.queue_ai(
                e,
                AiCommand::WaitMs {
                    ms: (EVADE_SECS * 1000) as u32,
                },
            );
            Flow::Yield
        }
        _ => {
            finish(host, e);
            Flow::Done
        }
    }
}

fn finish(host: &mut dyn ScriptHost, e: EntityId) {
    host.set_current_action(e, Action::None);
    if e == host.player() {
        if npc_weapon_mode(host, e) == WeaponMode::Melee {
            host.set_state(e, State::PsMeleeLoop);
        } else {
            host.set_task(e, State::PsNormal);
        }
    } else {
        let back = host.last_task(e).unwrap_or(State::Attack);
        host.set_task(e, back);
    }
}

/// Definitions of the five evade states.
pub fn definition(state: State) -> Option<Def> {
    match state {
        State::EvadeLeft | State::EvadeRight | State::EvadeForward | State::EvadeBackward => {
            Some(Def::Custom(evade_entry))
        }
        State::EvadeLoop => Some(Def::Custom(evade_loop)),
        _ => None,
    }
}
