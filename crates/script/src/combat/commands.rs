//! B09: the AI command primitives of fighting (`_AI_Attack` and friends, `_AI_Parade`,
//! `_AI_JumpBack`, `_AI_StandUp`, ...) and their animation-event callbacks (`OnAI_*`).
//!
//! The DLL's commands are engine instruction builders: they set the routine properties
//! (`Action`, `AniState`, `HitDirection`, `StatePosition`), charge the stamina, enable the weapon's
//! damage strip and register the `OnAI_*` callback that the engine fires on the animation
//! events. What is gameplay and portable is the *numbers*: what each move costs, when its hit
//! window opens, how its stages count. Those live here as tables; the host plays the animation
//! (`AiCommand::Attack` and friends) and asks this module when the damage strip turns on.

use crate::enums::{Action, AniState, Slot, UseType};
use crate::host::{EntityId, MovePhase, ScriptHost};
use crate::stats::{add_stamina_points, pool};

/// Stamina a move costs when it starts (`AddStaminaPoints(-n)` inside the `_AI_*` command).
///
/// Plain attack, the quick attacks, simple whirl and pierce cost 12 (10 without
/// `ExtendedContent`); power and sprint attacks 20; hack 20 now and 20 more in its second stage
/// ([`second_stage_cost`]); whirl 40; jump 10; a jump back is free with `ExtendedContent` and
/// costs 10 without. Parading, stumbling and finishing blows cost nothing.
pub const fn stamina_cost(extended_content: bool, action: Action) -> i32 {
    match action {
        Action::Attack
        | Action::QuickAttack
        | Action::QuickAttackR
        | Action::QuickAttackL
        | Action::SimpleWhirl
        | Action::PierceAttack => {
            if extended_content {
                12
            } else {
                10
            }
        }
        Action::PowerAttack | Action::SprintAttack | Action::HackAttack => 20,
        Action::WhirlAttack => 40,
        Action::Jump => 10,
        Action::JumpBack => {
            if extended_content {
                0
            } else {
                10
            }
        }
        _ => 0,
    }
}

/// The extra stamina the second stage of a hack costs (`OnAI_HackAttack`).
pub const fn second_stage_cost(action: Action) -> i32 {
    match action {
        Action::HackAttack => 20,
        _ => 0,
    }
}

/// Seconds after which the weapon's damage strip turns on (`OnAI_*` against the state time):
/// plain attack, simple whirl and whirl 0.25; quick attacks, power attack, sprint attack and
/// get-up attack 0.1; pierce 0.8; get-up parade 0.25. The hack's window follows its parade
/// wind-up (0.5 [I]).
pub const fn hit_window_secs(action: Action) -> f32 {
    match action {
        Action::QuickAttack
        | Action::QuickAttackR
        | Action::QuickAttackL
        | Action::PowerAttack
        | Action::SprintAttack
        | Action::GetUpAttack => 0.1,
        Action::PierceAttack => 0.8,
        Action::HackAttack => 0.5,
        Action::GetUpParade => 0.25,
        _ => 0.25,
    }
}

/// Seconds a finishing blow takes before the victim dies (`OnAI_FinishingAttack`).
pub const FINISHING_SECS: f32 = 1.2;

/// What starting a move did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoveStart {
    /// The stamina points charged.
    pub cost: i32,
    /// The fighter had no stamina left (`GetStaminaPoints <= 0` after the charge): the engine
    /// plays the tired variant.
    pub exhausted: bool,
    /// Seconds until the damage strip turns on.
    pub window_secs: f32,
}

/// The script side of starting a melee command: set `Action`, the pose and the stage counter
/// and charge the stamina. A pierce that starts from the parade pose is free (the wind-up already
/// paid); everything else pays [`stamina_cost`].
pub fn start_move(host: &mut dyn ScriptHost, npc: EntityId, action: Action) -> MoveStart {
    let from_parade = host.current_action(npc) == Action::Parade;
    let mut cost = stamina_cost(host.setup().extended_content, action);
    if action == Action::PierceAttack && from_parade {
        cost = 0;
    }
    host.set_current_action(npc, action);
    host.set_ani_state(npc, AniState::Stand);
    host.set_state_position(npc, 0);
    if cost > 0 {
        add_stamina_points(host, npc, -cost);
    }
    let exhausted = crate::stats::pool(host, npc, crate::host::Pool::Stamina) <= 0;
    MoveStart {
        cost,
        exhausted,
        window_secs: hit_window_secs(action),
    }
}

/// Port of the `OnAI_*` animation-event callbacks (S:38580, S:48830, S:47970, S:4e070,
/// S:4c890, S:433d0, S:41e30, S:420e0): the first call after the hit window opened moves the
/// stage counter to 1 (damage strip on). Returns `true` exactly then. A hack counts a second
/// stage (`StatePosition` 2) and charges [`second_stage_cost`] when it begins.
pub fn on_ai_move(
    host: &mut dyn ScriptHost,
    npc: EntityId,
    action: Action,
    state_time: f32,
) -> bool {
    gothic_portcov::hit!("on_ai_move");
    let stage = host.state_position(npc);
    if stage == 0 && state_time > hit_window_secs(action) {
        host.set_state_position(npc, 1);
        return true;
    }
    if action == Action::HackAttack && stage == 1 && state_time > hit_window_secs(action) + 0.5 {
        host.set_state_position(npc, 2);
        add_stamina_points(host, npc, -second_stage_cost(action));
        return true;
    }
    false
}

/// Port of `_AI_Parade` (S:46810) and `OnAI_Parade` (S:46d80): the guard goes up (`Action =
/// Parade`, pose Parade) and holds. Returns the seconds the pose lasts before the callback
/// lowers it (0.5, 2.0 for a long guard).
pub fn start_parade(host: &mut dyn ScriptHost, npc: EntityId) -> f32 {
    gothic_portcov::hit!("start_parade");
    host.set_current_action(npc, Action::Parade);
    host.set_ani_state(npc, AniState::Parade);
    host.set_state_position(npc, 0);
    0.5
}

/// Port of `_AI_StandUp` (S:4a950): leave any sitting, lying or kneeling pose. Returns whether
/// an animation has to play.
pub fn start_stand_up(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("start_stand_up");
    let pose = host.ani_state(npc);
    host.set_ani_state(npc, AniState::Stand);
    !matches!(pose, AniState::Stand | AniState::Dummy0 | AniState::Dummy1)
}

/// Port of `_AI_Jump` / `_AI_JumpBack` (S:46220, S:46560): charge the stamina; returns the
/// cost.
pub fn start_jump(host: &mut dyn ScriptHost, npc: EntityId, back: bool) -> i32 {
    gothic_portcov::hit!("start_jump");
    let action = if back { Action::JumpBack } else { Action::Jump };
    let cost = stamina_cost(host.setup().extended_content, action);
    host.set_current_action(npc, action);
    if cost > 0 {
        add_stamina_points(host, npc, -cost);
    }
    cost
}

/// Port of `_AI_Shoot` (S:4a710): shooting consumes one arrow or bolt from the stack in the
/// off hand; returns whether a projectile was available (`PSInventory::HasItems`).
pub fn start_shoot(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    gothic_portcov::hit!("start_shoot");
    host.set_current_action(npc, Action::Shoot);
    let ammo = match host.slot_use_type(npc, Slot::LeftHand) {
        UseType::CrossBow => "Bolt",
        _ => "Arrow",
    };
    host.has_items(npc, ammo, 1)
}

/// Port of `_AI_Die` (S:3e510): the dying NPC plays `Die` and is out of the fight.
pub fn start_die(host: &mut dyn ScriptHost, npc: EntityId) {
    gothic_portcov::hit!("start_die");
    host.set_current_action(npc, Action::Die);
}

/// Port of `_AI_Stumble`, `_AI_ParadeStumble`, `_AI_QuickStumble`, `_AI_PierceStumble`,
/// `_AI_HeavyParadeStumble` (S:4aaf0, S:46f80, S:489c0, S:47a90, S:43490): the hit reaction
/// starts (`Action` = the stumble); the host plays it for the length it knows.
pub fn start_stumble(host: &mut dyn ScriptHost, npc: EntityId, action: Action) {
    gothic_portcov::hit!("start_stumble");
    host.set_current_action(npc, action);
    if matches!(action, Action::Stumble | Action::QuickStumble) {
        // The pain grunt of `_AI_Stumble` / `_AI_QuickStumble` (S:4aaf0, S:489c0 call S:528a0).
        crate::speech::start_say_aargh(host, npc, EntityId::NONE);
    }
}

/// Port of `_AI_LieKnockDown` (S:467c0): the knocked-down NPC lies on the ground.
pub fn start_lie_knock_down(host: &mut dyn ScriptHost, npc: EntityId) {
    gothic_portcov::hit!("start_lie_knock_down");
    host.set_ani_state(npc, AniState::LieKnockDown);
}

/// Port of `_AI_GetUpAttack` / `_AI_GetUpParade` (S:41bd0, S:41f20): a knocked-down fighter
/// attacks or parries on its way up; the pose becomes standing and the action the get-up move.
pub fn start_get_up(host: &mut dyn ScriptHost, npc: EntityId, attack: bool) {
    gothic_portcov::hit!("start_get_up");
    host.set_ani_state(npc, AniState::Stand);
    host.set_current_action(
        npc,
        if attack {
            Action::GetUpAttack
        } else {
            Action::GetUpParade
        },
    );
}

/// Port of `_AI_Reload` (S:4a4f0): reloading a crossbow restores the action to `Reload`.
pub fn start_reload(host: &mut dyn ScriptHost, npc: EntityId) {
    gothic_portcov::hit!("start_reload");
    host.set_current_action(npc, Action::Reload);
}

// ---- the player's melee commands as instruction plans ---------------------------------------

/// One instruction of an `_AI_*` melee command, in the order the script runs them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlanStep {
    /// `AddStaminaPoints(-n)` (a negative delta), done before the next instruction.
    Stamina(i32),
    /// `sAICombatMoveInstr` with the phase and the play speed of `sub_42a0`.
    Move {
        /// Which clip of the move.
        phase: MovePhase,
        /// Play speed.
        speed: f32,
    },
    /// `sAIWaitInstr(ms)`.
    WaitMs(u32),
}

/// The instruction list of one `_AI_*` melee command (`_AI_Attack`, `_AI_PowerAttack`, ...), after
/// the command has written `Action`, `AniState` and `HitDirection`.
#[derive(Debug, Clone, PartialEq)]
pub struct MovePlan {
    /// `PSRoutine.Action` the command set.
    pub action: Action,
    /// `PSRoutine.AniState` the command set (the clip-name pose).
    pub pose: AniState,
    /// The instructions.
    pub steps: Vec<PlanStep>,
}

/// `HandsAre(self, left, right)` (`sub_40f0`): the use types of the slot-2 and slot-1 items.
fn hands_are(host: &dyn ScriptHost, e: EntityId, left: UseType, right: UseType) -> bool {
    host.slot_use_type(e, Slot::LeftHand) == left && host.slot_use_type(e, Slot::RightHand) == right
}

/// Port of the player path of `sub_42a0` (S:42a0, `ExtendedContent` shipped): the play speed of
/// a combat move. The base follows the weapons in the hands (a one-hander with nothing, a
/// shield, a torch or a second one-hander in the other hand 0.6; a two-hander, axe, staff or
/// halberd 0.7; anything else 1.0) with per-move exceptions, and tiredness lowers it by the
/// stamina bands of the table. `heavy` is the stack argument: the `Hit` instruction of a power
/// attack passes 1, its `Raise` 0.
pub fn animation_speed(host: &dyn ScriptHost, e: EntityId, action: Action, heavy: bool) -> f32 {
    gothic_portcov::hit!("animation_speed");
    use Action as A;
    use UseType as U;
    let stamina = pool(host, e, crate::host::Pool::Stamina);
    let tired: f32 = match action {
        A::Attack | A::Parade | A::ParadeR | A::ParadeL => match stamina {
            ..=19 => -0.10,
            20..=49 => -0.05,
            _ => 0.0,
        },
        A::PowerAttack | A::HackAttack => match stamina {
            ..=29 => -0.10,
            30..=49 => -0.05,
            _ => 0.0,
        },
        A::QuickAttack | A::QuickAttackR | A::QuickAttackL => match stamina {
            ..=19 => -0.30,
            20..=39 => -0.15,
            _ => 0.0,
        },
        A::SimpleWhirl | A::PierceAttack => match stamina {
            ..=19 => -0.14,
            20..=44 => -0.07,
            _ => 0.0,
        },
        A::WhirlAttack => match stamina {
            ..=39 => -0.14,
            40..=69 => -0.07,
            _ => 0.0,
        },
        A::ParadeStumble
        | A::ParadeStumbleR
        | A::ParadeStumbleL
        | A::HeavyParadeStumble
        | A::Stumble
        | A::StumbleR
        | A::StumbleL
        | A::PierceStumble
            if stamina < 15 =>
        {
            -0.10
        }
        A::QuickStumble if stamina < 10 => -0.20,
        _ => 0.0,
    };
    let base: f32 = match action {
        A::Attack => {
            if [
                (U::None, U::OneH),
                (U::Shield, U::OneH),
                (U::Torch, U::OneH),
                (U::OneH, U::OneH),
            ]
            .iter()
            .any(|&(l, r)| hands_are(host, e, l, r))
            {
                0.6
            } else if [
                (U::None, U::TwoH),
                (U::None, U::Axe),
                (U::None, U::Staff),
                (U::None, U::Halberd),
            ]
            .iter()
            .any(|&(l, r)| hands_are(host, e, l, r))
            {
                0.7
            } else {
                1.0
            }
        }
        A::PowerAttack => {
            if !heavy {
                1.5
            } else if hands_are(host, e, U::OneH, U::OneH) {
                0.9
            } else {
                1.0
            }
        }
        A::SimpleWhirl => 1.3,
        A::Parade | A::ParadeR | A::ParadeL => {
            if host.setup().extended_content {
                2.3
            } else {
                1.3
            }
        }
        A::ParadeStumble
        | A::ParadeStumbleR
        | A::ParadeStumbleL
        | A::QuickParadeStumble
        | A::QuickStumble => 2.0,
        A::Stumble | A::StumbleR | A::StumbleL => 1.3,
        A::HeavyParadeStumble => {
            if hands_are(host, e, U::None, U::OneH) {
                0.7
            } else {
                1.0
            }
        }
        A::FinishingAttack => return 1.0,
        _ => 1.0,
    };
    base + tired
}

/// The hand a quick attack uses (`_AI_QuickAttack`, S:48340): `HitDirection` alternates the right
/// (action 4, `HitDirection` becomes 1) and the left (action 5, 0) swing; the pose of
/// `GetPrimaryPoseExt(QuickAttack, Hit)` picks fixed sides: pose 3 right, any pose other than
/// 1 and 2 left.
fn quick_attack_hand(host: &mut dyn ScriptHost, e: EntityId) -> Action {
    let alternate = |host: &mut dyn ScriptHost| {
        if host.hit_direction(e) == 0 {
            host.set_hit_direction(e, 1);
            Action::QuickAttackR
        } else {
            host.set_hit_direction(e, 0);
            Action::QuickAttackL
        }
    };
    match host.primary_pose(e) {
        1 => alternate(host),
        2 => {
            // The two `HandsAre` pairs the decompiler dropped: dual wielders alternate [I].
            if hands_are(host, e, UseType::OneH, UseType::OneH) {
                alternate(host)
            } else {
                host.set_hit_direction(e, 0);
                Action::QuickAttackL
            }
        }
        3 => {
            host.set_hit_direction(e, 1);
            Action::QuickAttackR
        }
        _ => {
            host.set_hit_direction(e, 0);
            Action::QuickAttackL
        }
    }
}

/// `HitDirection` of the pose (`GetPrimaryPoseExt(action, Hit)` of `_AI_Attack` and
/// `_AI_PowerAttack`): poses 1 and 3 right, 2 and 4 left.
fn set_hand_by_pose(host: &mut dyn ScriptHost, e: EntityId) {
    match host.primary_pose(e) {
        1 | 3 => host.set_hit_direction(e, 1),
        2 | 4 => host.set_hit_direction(e, 0),
        _ => {}
    }
}

/// Port of the melee `_AI_*` commands of the player (S:381a0 `_AI_Attack`, S:47c30
/// `_AI_PowerAttack`, S:48340 `_AI_QuickAttack`, S:4c4a0 `_AI_SimpleWhirl`, S:4dc20
/// `_AI_WhirlAttack`, S:47210 `_AI_PierceAttack`, S:42ee0 `_AI_HackAttack`, S:41400
/// `_AI_FinishingAttack`): write `Action`, `AniState` (the pierce, hack, whirl and finishing blow
/// come out of the guard pose, `Parade`; the others are `Stand`) and the hand, and return the
/// instruction list. Stamina is charged when the instruction runs: the power attack pays after
/// its `Raise` clip, the hack too, the whirl before everything, the others before their `Hit`
/// clip; a pierce out of the guard (`Action` still `Parade`) is free.
pub fn begin_melee_command(host: &mut dyn ScriptHost, e: EntityId, action: Action) -> MovePlan {
    gothic_portcov::hit!("begin_melee_command");
    use PlanStep::{Move, Stamina, WaitMs};
    let ext = host.setup().extended_content;
    let cost = |a| -stamina_cost(ext, a);
    let from_guard = host.current_action(e) == Action::Parade;
    let mut action = action;
    let mut pose = AniState::Stand;
    let mut steps: Vec<PlanStep> = Vec::new();
    match action {
        Action::Attack => {
            set_hand_by_pose(host, e);
            let speed = animation_speed(host, e, Action::Attack, false);
            steps.extend([
                Stamina(cost(Action::Attack)),
                Move {
                    phase: MovePhase::Hit,
                    speed,
                },
            ]);
        }
        Action::QuickAttack | Action::QuickAttackR | Action::QuickAttackL => {
            action = quick_attack_hand(host, e);
            let speed = animation_speed(host, e, action, false);
            steps.extend([
                Stamina(cost(Action::QuickAttack)),
                Move {
                    phase: MovePhase::Hit,
                    speed,
                },
                WaitMs(100),
            ]);
        }
        Action::PowerAttack => {
            set_hand_by_pose(host, e);
            let raise = animation_speed(host, e, Action::PowerAttack, false);
            let hit = animation_speed(host, e, Action::PowerAttack, true);
            steps.extend([
                Move {
                    phase: MovePhase::Raise,
                    speed: raise,
                },
                Stamina(cost(Action::PowerAttack)),
                Move {
                    phase: MovePhase::Hit,
                    speed: hit,
                },
            ]);
        }
        Action::SimpleWhirl => {
            let speed = animation_speed(host, e, Action::SimpleWhirl, false);
            steps.extend([
                Stamina(cost(Action::SimpleWhirl)),
                Move {
                    phase: MovePhase::Hit,
                    speed,
                },
            ]);
        }
        Action::WhirlAttack => {
            pose = AniState::Parade;
            let speed = animation_speed(host, e, Action::WhirlAttack, false);
            steps.extend([
                Stamina(cost(Action::WhirlAttack)),
                Move {
                    phase: MovePhase::Hit,
                    speed,
                },
            ]);
        }
        Action::PierceAttack => {
            pose = AniState::Parade;
            let speed = animation_speed(host, e, Action::PierceAttack, false);
            steps.push(Move {
                phase: MovePhase::Raise,
                speed,
            });
            if !from_guard {
                steps.push(Stamina(cost(Action::PierceAttack)));
            }
            steps.extend([
                Move {
                    phase: MovePhase::Hit,
                    speed,
                },
                Move {
                    phase: MovePhase::Recover,
                    speed,
                },
            ]);
        }
        Action::HackAttack => {
            pose = AniState::Parade;
            let speed = animation_speed(host, e, Action::HackAttack, false);
            steps.extend([
                Move {
                    phase: MovePhase::Raise,
                    speed,
                },
                Stamina(cost(Action::HackAttack)),
                Move {
                    phase: MovePhase::Hit,
                    speed,
                },
                Move {
                    phase: MovePhase::Recover,
                    speed,
                },
            ]);
        }
        Action::FinishingAttack => {
            pose = AniState::Parade;
            for phase in [MovePhase::Raise, MovePhase::Hit, MovePhase::Recover] {
                steps.push(Move { phase, speed: 1.0 });
            }
        }
        _ => {}
    }
    host.set_current_action(e, action);
    host.set_ani_state(e, pose);
    host.set_state_position(e, 0);
    MovePlan {
        action,
        pose,
        steps,
    }
}

/// Port of `_AI_Parade` for the player (S:46810): the guard goes up. Whether the `Begin` clip of
/// the `Parade` action plays depends on the pose and the hands: with the primary pose 1 only a
/// shield with a one-hander plays it (a sword alone, a torch or a second sword snap into the
/// guard pose); with pose 2 a shield with a one-hander, two one-handers and fists snap; any
/// other pose plays it. After the instruction the command sets `AniState` to `Parade`, the
/// movement mode to walk and the player's guard flag `DAT_10118ab0`; the clip is named with the
/// pose the hero leaves.
pub fn begin_parade_command(host: &mut dyn ScriptHost, e: EntityId) -> MovePlan {
    gothic_portcov::hit!("begin_parade_command");
    use UseType as U;
    host.set_current_action(e, Action::Parade);
    host.set_state_position(e, 0);
    let plays_clip = match host.primary_pose(e) {
        1 => hands_are(host, e, U::Shield, U::OneH),
        2 => ![(U::Shield, U::OneH), (U::OneH, U::OneH), (U::None, U::Fist)]
            .iter()
            .any(|&(l, r)| hands_are(host, e, l, r)),
        _ => true,
    };
    let speed = animation_speed(host, e, Action::Parade, false);
    let leaving = host.ani_state(e);
    MovePlan {
        action: Action::Parade,
        pose: leaving,
        steps: if plays_clip {
            vec![PlanStep::Move {
                phase: MovePhase::Begin,
                speed,
            }]
        } else {
            Vec::new()
        },
    }
}

/// Port of `_AI_JumpBack` (S:46560): the hop is a `Hit` clip of the `JumpBack` action, free with
/// `ExtendedContent`.
pub fn begin_jump_back_command(host: &mut dyn ScriptHost, e: EntityId) -> MovePlan {
    gothic_portcov::hit!("begin_jump_back_command");
    let ext = host.setup().extended_content;
    host.set_current_action(e, Action::JumpBack);
    host.set_ani_state(e, AniState::Stand);
    host.set_state_position(e, 0);
    let mut steps = Vec::new();
    let cost = stamina_cost(ext, Action::JumpBack);
    if cost > 0 {
        steps.push(PlanStep::Stamina(-cost));
    }
    steps.push(PlanStep::Move {
        phase: MovePhase::Hit,
        speed: animation_speed(host, e, Action::JumpBack, false),
    });
    MovePlan {
        action: Action::JumpBack,
        pose: AniState::Stand,
        steps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::Species;
    use crate::host::{NpcApi, Pool, StatsApi};
    use crate::testhost::TestWorld;

    #[test]
    fn costs_and_windows_follow_the_rule_table() {
        assert_eq!(stamina_cost(true, Action::Attack), 12);
        assert_eq!(stamina_cost(false, Action::Attack), 10);
        assert_eq!(stamina_cost(true, Action::PowerAttack), 20);
        assert_eq!(stamina_cost(true, Action::WhirlAttack), 40);
        assert_eq!(stamina_cost(true, Action::JumpBack), 0);
        assert_eq!(stamina_cost(false, Action::JumpBack), 10);
        assert_eq!(stamina_cost(true, Action::Parade), 0);
        assert_eq!(second_stage_cost(Action::HackAttack), 20);
        assert_eq!(hit_window_secs(Action::QuickAttack), 0.1);
        assert_eq!(hit_window_secs(Action::PierceAttack), 0.8);
        assert_eq!(hit_window_secs(Action::Attack), 0.25);
    }

    #[test]
    fn starting_a_move_charges_the_stamina_and_marks_exhaustion() {
        let mut w = TestWorld::new();
        let a = w.spawn_npc("Orc", Species::Orc, 10, 30);
        w.dr_set_pool(a, Pool::Stamina, 20);
        let s = start_move(&mut w, a, Action::Attack);
        assert_eq!(s.cost, 12);
        assert!(!s.exhausted);
        assert_eq!(w.dr_pool(a, Pool::Stamina), 8);
        assert_eq!(w.current_action(a), Action::Attack);
        let s = start_move(&mut w, a, Action::PowerAttack);
        assert!(s.exhausted);
        assert_eq!(w.dr_pool(a, Pool::Stamina), 0);
    }

    #[test]
    fn a_pierce_out_of_the_guard_is_free_and_the_hack_pays_twice() {
        let mut w = TestWorld::new();
        let a = w.spawn_npc("Orc", Species::Orc, 10, 30);
        w.set_current_action_direct(a, Action::Parade);
        let before = w.dr_pool(a, Pool::Stamina);
        assert_eq!(start_move(&mut w, a, Action::PierceAttack).cost, 0);
        assert_eq!(w.dr_pool(a, Pool::Stamina), before);
        start_move(&mut w, a, Action::HackAttack);
        assert_eq!(w.dr_pool(a, Pool::Stamina), before - 20);
        assert!(!on_ai_move(&mut w, a, Action::HackAttack, 0.2));
        assert!(on_ai_move(&mut w, a, Action::HackAttack, 0.6));
        assert_eq!(w.state_position(a), 1);
        assert!(on_ai_move(&mut w, a, Action::HackAttack, 1.2));
        assert_eq!(w.state_position(a), 2);
        assert_eq!(w.dr_pool(a, Pool::Stamina), before - 40);
    }
}
