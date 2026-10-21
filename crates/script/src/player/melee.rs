//! `PS_Melee*` (B16, melee half): the hero's fight states, ports of `Script_Game.dll`
//! (`Script/Player/Player_States/PS_Melee.cpp`).
//!
//! The key handlers (`gothic_input::player`, `OnPlayerAction` / `OnPlayerSecondaryAction`) put a
//! `gEAction` id and the focus entity into the player's action queue. [`ps_melee_loop`] (S:7e750)
//! picks the combat target, aligns the body to it while the guard is up, and turns the head of
//! the queue into the state of the move ([`state_for_action`]). The move states
//! (`PS_Melee_Attack`, `_PowerAttack`, `_QuickAttack`, `_SimpleWhirl`, `_Parade`) align the body
//! to the current target and run the matching `_AI_*` command; the pierce, hack, whirl and
//! finishing states run theirs without aligning. Every move ends in `PS_Melee_Loop`.

use crate::combat::commands::{
    MovePlan, PlanStep, begin_jump_back_command, begin_melee_command, begin_parade_command,
};
use crate::combat::select::npc_weapon_mode;
use crate::damage::is_in_parade_mode;
use crate::enums::{Action, AiMode, AniState, Slot, UseType};
use crate::helpers::{WeaponMode, weapon_mode};
use crate::host::{AiCommand, EntityId, MovePhase, ScriptHost};
use crate::player::step;
use crate::routine::state::State;
use crate::routine::work::{Def, Flow};

/// `PSCharacterControl::SetMovementConstraints(360)` of the melee states (`_DAT_101074cc`): the
/// camera does not steer the body any more, the alignment target does.
pub const CONSTRAINT_DEGREES: f32 = 360.0;
/// `Entity::GetAziElev` window of the entity a queued action names (`_DAT_10107718`).
pub const QUEUED_TARGET_WINDOW_DEGREES: f32 = 45.0;
/// How far the plain focus entity still counts as the combat target (`_DAT_10107420`, a double).
pub const FOCUS_TARGET_RANGE_CM: f32 = 500.0;
/// `gECameraMode` the melee mode asks for (`PSCamera::SetMode(2, true, true)` in `PS_Melee`).
pub const CAMERA_MODE_MELEE: u32 = 2;
/// `PSFocus.CurrentMode` of `PS_Melee` (`gECombatMode` melee).
pub const FOCUS_MODE_MELEE: u32 = 2;

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

/// The state that performs an action.
pub fn state_for_action(action: Action) -> Option<State> {
    MOVES.iter().find(|(_, a)| *a == action).map(|(s, _)| *s)
}

/// The `gEAction` ids the loop reads from the queue and what it does with them (the jump table of
/// S:7e750): the moves, the parade (16), the jump back (51), the jump (57), the sneak (77), the
/// weapon key (68), quick slots (131..140) and potions (141..143).
const ID_PARADE: u32 = 16;
const ID_JUMP_BACK: u32 = 51;
const ID_JUMP: u32 = 57;
const ID_HOLD: u32 = 68;
const ID_SNEAK: u32 = 77;
const ID_QUICK_SLOT_BASE: u32 = 131;
const ID_POTION_BASE: u32 = 141;

/// `PSNpc.LastHitTimestamp` bump of each move the loop starts (S:7e750).
const fn last_hit_bump(action: Action) -> u32 {
    match action {
        Action::Attack => 0x55,
        Action::PowerAttack => 0x3c,
        Action::QuickAttack | Action::SimpleWhirl => 0x5a,
        Action::WhirlAttack => 0x4b,
        Action::PierceAttack | Action::HackAttack => 0x28,
        _ => 0,
    }
}

/// The opponent the swing goes at: the current target the loop wrote.
fn opponent(host: &dyn ScriptHost, p: EntityId) -> EntityId {
    host.current_target(p)
}

/// Port of `PS_Melee` (S:7e280): the weapon is drawn. Step 0 clears the focus when the hero was not
/// already in combat mode, switches the camera to the melee mode, returns to `PS_Normal` when
/// the stacks to hold are not a melee weapon, sets `AIMode` to 6 and runs `_AI_HoldInventoryItems`
/// (the draw animation moves the item from the back slot into the hand). Step 1 is the
/// first-fight tutorial (HUD business, not ported). The next pass stores `LastWeaponConfig = Melee`,
/// the focus mode, drops look-at and alignment targets, sets `AIMode` to `Combat` and goes to
/// `PS_Melee_Loop`.
fn ps_melee(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_melee");
    match step(host, p) {
        0 => {
            if host.ai_mode(p) != AiMode::Combat {
                host.set_focus_entity(EntityId::NONE);
            }
            host.set_camera_mode(CAMERA_MODE_MELEE);
            // The stacks the hero is about to hold decide (`sub_2d80`): the hands are empty
            // while the weapon still hangs on the back.
            let right = host.find_stack_index(p, Slot::RightHand);
            let left = host.find_stack_index(p, Slot::LeftHand);
            let use_of = |host: &dyn ScriptHost, stack: i32| {
                if stack < 0 {
                    UseType::None
                } else {
                    host.stack_use_type(p, stack)
                }
            };
            if weapon_mode(use_of(host, left), use_of(host, right)) != WeaponMode::Melee {
                host.set_task(p, State::PsNormal);
                return Flow::Done;
            }
            host.set_ai_mode(p, AiMode::Talk);
            host.queue_ai(p, AiCommand::HoldInventoryStacks { right, left });
            Flow::Yield
        }
        _ => {
            host.set_last_weapon_config(0);
            host.set_focus_current_mode(FOCUS_MODE_MELEE);
            host.set_focus_range_rating(false);
            host.set_look_at(p, EntityId::NONE);
            host.set_alignment_target(p, EntityId::NONE);
            host.set_ai_mode(p, AiMode::Combat);
            host.set_state(p, State::PsMeleeLoop);
            Flow::Done
        }
    }
}

/// The combat target of this pass of the loop (S:7e750): the entity the queued action names when
/// it lies within 45 degrees of azimuth and elevation, else the melee-mode focus, else the plain
/// focus entity when it is not dead, not a knocked-down character while the hero is not guarding,
/// and within 500 cm.
fn pick_target(host: &dyn ScriptHost, p: EntityId) -> EntityId {
    let mut target = EntityId::NONE;
    if let Some((_, queued)) = host.action_queue_head()
        && queued.is_some()
    {
        let (azi, elev) = host.azimuth_elevation(p, queued);
        if azi.abs() <= QUEUED_TARGET_WINDOW_DEGREES && elev.abs() <= QUEUED_TARGET_WINDOW_DEGREES {
            target = queued;
        }
    }
    if target.is_none() {
        let melee = host.melee_focus();
        let plain = host.current_focus();
        if melee.is_some() {
            target = melee;
        } else if plain.is_some() {
            let mode = host.ai_mode(plain);
            let acceptable = mode != AiMode::Dead
                && (mode != AiMode::Down || is_in_parade_mode(host, p))
                && host.distance(p, plain) <= FOCUS_TARGET_RANGE_CM;
            if acceptable {
                target = plain;
            }
        }
    }
    target
}

/// Port of `PS_Melee_Loop` (S:7e750), run every pass: the focus look-at mode (0 in first person, 1
/// with the guard up, 2 otherwise), the combat target ([`pick_target`]) written to the focus and
/// `CurrentTarget`; with the guard up and a target the body aligns to it and the constraint goes
/// to 360, without a guard but with a movement key held the alignment is cleared (the body follows
/// the movement); then the head of the action queue is read and turned into the next state.
fn ps_melee_loop(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_melee_loop");
    let guarding = is_in_parade_mode(host, p);
    let look_at = if host.camera_in_first_person() {
        0
    } else if guarding {
        1
    } else {
        2
    };
    host.set_focus_look_at_mode(look_at);
    let target = pick_target(host, p);
    host.set_focus_entity(target);
    host.set_current_target(p, target);
    if guarding && target.is_some() {
        host.set_alignment_target(p, target);
        host.set_movement_constraints(p, CONSTRAINT_DEGREES);
    } else if host.movement_key_down() {
        host.set_alignment_target(p, EntityId::NONE);
        host.set_movement_constraints(p, CONSTRAINT_DEGREES);
    }
    host.set_current_action(p, Action::None);
    let Some((id, _)) = host.action_queue_head() else {
        // Nothing queued: the loop idles. The weapon put away ends the mode.
        if npc_weapon_mode(host, p) != WeaponMode::Melee {
            host.set_task(p, State::PsNormal);
            return Flow::Done;
        }
        host.queue_ai(p, AiCommand::WaitMs { ms: 20 });
        return Flow::Yield;
    };
    host.action_queue_pop();
    let action = Action::from_raw(id);
    if let Some(state) = action.and_then(state_for_action) {
        let bump = last_hit_bump(action.unwrap_or(Action::None));
        let stamp = host.last_hit_timestamp(p);
        host.set_last_hit_timestamp(p, stamp.saturating_add(bump));
        host.set_state(p, state);
        return Flow::Done;
    }
    match id {
        ID_PARADE => host.set_state(p, State::PsMeleeParade),
        ID_JUMP_BACK => host.set_state(p, State::PsMeleeJumpBack),
        ID_JUMP => host.set_state(p, State::PsMeleeJump),
        ID_SNEAK => host.set_state(p, State::PsMeleeSneak),
        ID_HOLD => crate::player::select_weapon::holster_from_melee(host, p),
        n if (ID_QUICK_SLOT_BASE..ID_QUICK_SLOT_BASE + 10).contains(&n) => {
            host.emit(crate::player::api::PlayerEvent::QuickSlot(
                n - ID_QUICK_SLOT_BASE,
            ));
        }
        n if (ID_POTION_BASE..ID_POTION_BASE + 3).contains(&n) => {
            host.emit(crate::player::api::PlayerEvent::PotionKey(
                n - ID_POTION_BASE,
            ));
        }
        _ => {}
    }
    Flow::Done
}

/// Queue the instructions of a plan (the engine runs them in order; a `Stamina` entry is the
/// script's `AddStaminaPoints` between two instructions).
fn queue_plan(host: &mut dyn ScriptHost, p: EntityId, plan: &MovePlan, target: EntityId) {
    for step in &plan.steps {
        match *step {
            PlanStep::Stamina(delta) => host.queue_ai(p, AiCommand::AddStamina(delta)),
            PlanStep::Move { phase, speed } => host.queue_ai(
                p,
                AiCommand::CombatMove {
                    action: plan.action,
                    phase,
                    speed,
                    pose: plan.pose,
                    target,
                },
            ),
            PlanStep::WaitMs(ms) => host.queue_ai(p, AiCommand::WaitMs { ms }),
        }
    }
}

/// The alignment the attack and parade states do first: unless the camera is in first person, the
/// body aligns to the current target and the constraint goes to 360.
fn align_to_target(host: &mut dyn ScriptHost, p: EntityId, needs_target: bool) {
    if host.camera_in_first_person() {
        return;
    }
    let target = host.current_target(p);
    if needs_target && target.is_none() {
        return;
    }
    host.set_alignment_target(p, target);
    host.set_movement_constraints(p, CONSTRAINT_DEGREES);
}

/// The attack states (`PS_Melee_Attack` and siblings, S:7f1a0..S:7f960): align (Attack, Power, Quick
/// and SimpleWhirl only), run the `_AI_*` command against the opponent, back to the loop.
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
            if matches!(
                action,
                Action::Attack | Action::PowerAttack | Action::QuickAttack | Action::SimpleWhirl
            ) {
                align_to_target(host, p, false);
            }
            let target = opponent(host, p);
            let plan = begin_melee_command(host, p, action);
            queue_plan(host, p, &plan, target);
            Flow::Yield
        }
        _ => {
            host.set_state(p, State::PsMeleeLoop);
            Flow::Done
        }
    }
}

/// Port of `PS_Melee_Parade` (S:7f960): align to the target, raise the guard (`_AI_Parade`), back
/// to the loop. After the instruction the command sets `AniState` to `Parade` and the movement
/// mode to walk.
fn ps_melee_parade(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_melee_parade");
    match step(host, p) {
        0 => {
            align_to_target(host, p, true);
            let target = opponent(host, p);
            let plan = begin_parade_command(host, p);
            queue_plan(host, p, &plan, target);
            Flow::Yield
        }
        _ => {
            host.set_ani_state(p, AniState::Parade);
            host.set_state(p, State::PsMeleeLoop);
            Flow::Done
        }
    }
}

/// Port of `PS_Melee_JumpBack` (S:7fc20): `_AI_JumpBack` against the opponent, no alignment.
fn ps_melee_jump_back(host: &mut dyn ScriptHost, p: EntityId) -> Flow {
    gothic_portcov::hit!("ps_melee_jump_back");
    match step(host, p) {
        0 => {
            let target = opponent(host, p);
            let plan = begin_jump_back_command(host, p);
            queue_plan(host, p, &plan, target);
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

/// The `Hit` phase token, for the hit-moment rule of the engine: `OnHit` fires when the `Hit`
/// clip reaches this fraction of its length (`sAICombatMoveItlLoop`, G:0x1016dd00, double
/// `_DAT_10308308`).
pub const HIT_FRACTION: f32 = 0.6;

/// True for the phases whose clip carries the damage moment.
pub const fn has_hit_moment(phase: MovePhase) -> bool {
    matches!(phase, MovePhase::Hit)
}

/// What `OnPlayerAction` / `OnPlayerSecondaryAction` (S:62480, S:62ff0) ask of the melee layer
/// before they queue an action: `CanDo` of each move and the weapon-class helpers
/// (`gothic_input::MeleeFacts` carries the same fields to the key router).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ControlFacts {
    /// Bit `n` is `CanDo(self, None, gEAction n)` (S:4f80) for the ids below 64.
    pub can_do: u64,
    /// `sub_4260`: the right-hand item is a one-hander.
    pub right_one_hander: bool,
    /// `sub_4220`: the right-hand item is a two-hander (3, 12, 51, 52) or a swing tool (26, 29,
    /// 36, 49).
    pub two_hander_or_tool: bool,
    /// `sub_41a0`: the left-hand item is a one-hander or a torch (2, 24).
    pub left_one_hander_or_torch: bool,
    /// The focus is knocked down, killable and within 300 cm.
    pub finishing_target: bool,
}

/// Distance within which a finishing blow is offered (`_DAT_10107478`).
pub const FINISHING_RANGE_CM: f32 = 300.0;

/// Gather the [`ControlFacts`] of the hero from the ported script functions.
pub fn control_facts(host: &dyn ScriptHost, p: EntityId) -> ControlFacts {
    use crate::combat::moves::can_do;
    use crate::enums::UseType as U;
    let mut mask = 0u64;
    for id in 1..64u32 {
        if let Some(a) = Action::from_raw(id)
            && can_do(host, p, EntityId::NONE, a)
        {
            mask |= 1u64 << id;
        }
    }
    let right = host.slot_use_type(p, Slot::RightHand);
    let left = host.slot_use_type(p, Slot::LeftHand);
    let focus = host.current_focus();
    let finishing_target = focus.is_some()
        && host.ai_mode(focus) == AiMode::Down
        && crate::damage::can_be_killed(host, focus)
        && host.distance(p, focus) <= FINISHING_RANGE_CM;
    ControlFacts {
        can_do: mask,
        right_one_hander: right == U::OneH,
        two_hander_or_tool: matches!(
            right,
            U::TwoH | U::Staff | U::Halberd | U::Axe | U::Broom | U::Rake | U::Shovel | U::Pickaxe
        ),
        left_one_hander_or_torch: matches!(left, U::OneH | U::Torch),
        finishing_target,
    }
}
