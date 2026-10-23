//! Orc boulders and carried stones (B18): `PS_Interact_TiltOrcBoulder`, `..._HoldOrcBoulder`,
//! `..._LiftOrcBoulder` with their loops, the weight classes `OB_Light`, `OB_Medium`, `OB_Heavy`
//! and `OB_Faring_Boulder`, and `PS_Interact_CarryStone`.
//!
//! A boulder is worked in three stages. Tilting is free; the use key then asks for the next
//! stage, holding (action 111), and again for lifting (action 112). The object's use function
//! (its weight class) decides how strong the hero has to be for each; a weaker hero hears
//! `TooHeavy` and ends up in `PS_Normal`. The first lift of a boulder grants +5 Strength once
//! (the loop adds it while the object is not yet `UsedByPlayer`). The exit key leaves.

use crate::enums::Action;
use crate::host::{AiCommand, Attribute, EntityId, ScriptHost};
use crate::player::api::{BoulderClass, PlayerEvent, Refusal};
use crate::player::interact::EXIT_REQUESTED;
use crate::player::{back_to_normal, step};
use crate::routine::state::State;
use crate::routine::work::{Def, Flow};
use crate::speech::say;
use crate::stats;

/// `PSRoutine.TaskPosition` value the use key writes to ask for the next stage.
pub const ADVANCE_REQUESTED: u32 = 1001;
/// Strength a first lift grants.
pub const LIFT_REWARD: i32 = 5;
/// Template of the stone a stone carrier holds.
pub const CARRY_STONE: &str = "It_CarryFront_Stone";
/// Game event the Faring boulder sets when it is lifted.
pub const FARING_EVENT: &str = "Faring_Boulder_Success";

/// The stages of working a boulder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Tilt it (action 110).
    Tilt,
    /// Hold it up (action 111).
    Hold,
    /// Lift it (action 112).
    Lift,
}

impl Stage {
    const fn action(self) -> Action {
        match self {
            Self::Tilt => Action::TiltOrcBoulder,
            Self::Hold => Action::HoldOrcBoulder,
            Self::Lift => Action::LiftOrcBoulder,
        }
    }
    const fn loop_state(self) -> State {
        match self {
            Self::Tilt => State::PsInteractTiltOrcBoulderLoop,
            Self::Hold => State::PsInteractHoldOrcBoulderLoop,
            Self::Lift => State::PsInteractLiftOrcBoulderLoop,
        }
    }
    const fn next(self) -> Option<(Self, State)> {
        match self {
            Self::Tilt => Some((Self::Hold, State::PsInteractHoldOrcBoulder)),
            Self::Hold => Some((Self::Lift, State::PsInteractLiftOrcBoulder)),
            Self::Lift => None,
        }
    }
}

/// Strength a class demands for holding (`.0`) and lifting (`.1`): `OB_Light` 100/150,
/// `OB_Medium` 150/200, `OB_Heavy` and `OB_Faring_Boulder` 200/250.
pub const fn strength_needed(class: BoulderClass) -> (i32, i32) {
    match class {
        BoulderClass::Light => (100, 150),
        BoulderClass::Medium => (150, 200),
        BoulderClass::Heavy | BoulderClass::Faring => (200, 250),
    }
}

/// Port of `OB_Light` (S:777b0), `OB_Medium` (S:778e0), `OB_Heavy` (S:77a10) and
/// `OB_Faring_Boulder` (S:77b40): the strength check of the stage the hero asks for. A weaker
/// hero says `TooHeavy`. Lifting the Faring boulder sets its game event.
pub fn can_handle(
    host: &mut dyn ScriptHost,
    p: EntityId,
    class: BoulderClass,
    stage: Stage,
) -> bool {
    gothic_portcov::hit!("can_handle");
    let (hold, lift) = strength_needed(class);
    let needed = match stage {
        Stage::Tilt => 0,
        Stage::Hold => hold,
        Stage::Lift => lift,
    };
    if stats::attribute(host, p, Attribute::Strength) < needed {
        say(host, p, "TooHeavy");
        host.emit(PlayerEvent::Refused(Refusal::TooHard));
        return false;
    }
    if stage == Stage::Lift && class == BoulderClass::Faring {
        host.raise_game_event(FARING_EVENT);
    }
    true
}

fn stage_of(state: Option<State>) -> Stage {
    match state {
        Some(State::PsInteractHoldOrcBoulder | State::PsInteractHoldOrcBoulderLoop) => Stage::Hold,
        Some(State::PsInteractLiftOrcBoulder | State::PsInteractLiftOrcBoulderLoop) => Stage::Lift,
        _ => Stage::Tilt,
    }
}

/// Port of `PS_Interact_TiltOrcBoulder` (S:765b0), `PS_Interact_HoldOrcBoulder` (S:76c70) and
/// `PS_Interact_LiftOrcBoulder` (S:77210): hands empty (tilting), the strength check of the
/// stage (failure: back to `PS_Normal`), then the pose of the stage until the next key.
fn ps_boulder(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_boulder");
    let object = host.current_target(p);
    let stage = stage_of(host.current_state(p));
    match step(host, p) {
        0 => {
            let class = host.boulder_class(object);
            if !can_handle(host, p, class, stage) {
                back_to_normal(host, p);
                return Flow::Done;
            }
            host.queue_ai(
                p,
                AiCommand::HoldInventoryStacks {
                    right: -1,
                    left: -1,
                },
            );
            Flow::Yield
        }
        1 => {
            host.queue_ai(p, AiCommand::AniStateAction(stage.action()));
            Flow::Yield
        }
        _ => {
            host.set_task_position(p, 0);
            host.set_state(p, stage.loop_state());
            Flow::Done
        }
    }
}

/// Port of the three boulder loops (`PS_Interact_*OrcBoulder_Loop`, `OnInteract_*OrcBoulder_Loop`
/// S:76950 and siblings): hold the pose; the use key asks for the next stage, the exit key ends
/// the interaction. A lift pays the first-time Strength bonus.
fn ps_boulder_loop(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_boulder_loop");
    let object = host.current_target(p);
    let stage = stage_of(host.current_state(p));
    if stage == Stage::Lift && !host.used_by_player(object) {
        stats::add_attribute(host, p, Attribute::Strength, LIFT_REWARD);
        host.set_used_by_player(object, true);
    }
    match host.task_position(p) {
        EXIT_REQUESTED => {
            host.set_task(p, State::PsInteractEnd);
            Flow::Done
        }
        ADVANCE_REQUESTED => {
            host.set_task_position(p, 0);
            if let Some((_, state)) = stage.next() {
                host.set_state(p, state);
                Flow::Done
            } else {
                host.queue_ai(p, AiCommand::WaitMs { ms: 200 });
                Flow::Yield
            }
        }
        _ => {
            host.queue_ai(p, AiCommand::WaitMs { ms: 200 });
            Flow::Yield
        }
    }
}

/// Port of `PS_Interact_CarryStone` (S:77cc0): take the stone in both hands and walk
/// (movement mode 3); `PS_Interact_CarryStone_Loop` waits for the exit key and
/// `PS_Interact_CarryStone_End` puts it down.
fn ps_carry_stone(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_carry_stone");
    match step(host, p) {
        0 => {
            host.queue_ai(
                p,
                AiCommand::HoldItems {
                    right: CARRY_STONE,
                    left: "",
                },
            );
            Flow::Yield
        }
        _ => {
            host.full_stop(p);
            host.set_state(p, State::PsInteractCarryStoneLoop);
            Flow::Done
        }
    }
}

fn ps_carry_stone_loop(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    if host.task_position(p) == EXIT_REQUESTED {
        host.set_state(p, State::PsInteractCarryStoneEnd);
        return Flow::Done;
    }
    host.queue_ai(p, AiCommand::WaitMs { ms: 200 });
    Flow::Yield
}

fn ps_carry_stone_end(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    match step(host, p) {
        0 => {
            host.queue_ai(
                p,
                AiCommand::HoldInventoryStacks {
                    right: -1,
                    left: -1,
                },
            );
            Flow::Yield
        }
        _ => {
            let object = host.current_target(p);
            host.emit(PlayerEvent::InteractEnded { object });
            back_to_normal(host, p);
            Flow::Done
        }
    }
}

/// Definitions of the boulder and stone states.
pub fn definition(state: State) -> Option<Def> {
    use State as S;
    let custom = |f: fn(&mut dyn ScriptHost, EntityId) -> Flow| Some(Def::Custom(f));
    match state {
        S::PsInteractTiltOrcBoulder | S::PsInteractHoldOrcBoulder | S::PsInteractLiftOrcBoulder => {
            custom(ps_boulder)
        }
        S::PsInteractTiltOrcBoulderLoop
        | S::PsInteractHoldOrcBoulderLoop
        | S::PsInteractLiftOrcBoulderLoop => custom(ps_boulder_loop),
        S::PsInteractCarryStone => custom(ps_carry_stone),
        S::PsInteractCarryStoneLoop => custom(ps_carry_stone_loop),
        S::PsInteractCarryStoneEnd => custom(ps_carry_stone_end),
        _ => None,
    }
}
