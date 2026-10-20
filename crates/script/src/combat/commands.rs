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
use crate::host::{EntityId, ScriptHost};
use crate::stats::add_stamina_points;

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
    host.set_current_action(npc, Action::Parade);
    host.set_ani_state(npc, AniState::Parade);
    host.set_state_position(npc, 0);
    0.5
}

/// Port of `_AI_StandUp` (S:4a950): leave any sitting, lying or kneeling pose. Returns whether
/// an animation has to play.
pub fn start_stand_up(host: &mut dyn ScriptHost, npc: EntityId) -> bool {
    let pose = host.ani_state(npc);
    host.set_ani_state(npc, AniState::Stand);
    !matches!(pose, AniState::Stand | AniState::Dummy0 | AniState::Dummy1)
}

/// Port of `_AI_Jump` / `_AI_JumpBack` (S:46220, S:46560): charge the stamina; returns the
/// cost.
pub fn start_jump(host: &mut dyn ScriptHost, npc: EntityId, back: bool) -> i32 {
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
    host.set_current_action(npc, Action::Shoot);
    let ammo = match host.slot_use_type(npc, Slot::LeftHand) {
        UseType::CrossBow => "It_Bolt",
        _ => "It_Arrow",
    };
    host.has_items(npc, ammo, 1)
}

/// Port of `_AI_Die` (S:3e510): the dying NPC plays `Die` and is out of the fight.
pub fn start_die(host: &mut dyn ScriptHost, npc: EntityId) {
    host.set_current_action(npc, Action::Die);
}

/// Port of `_AI_Stumble`, `_AI_ParadeStumble`, `_AI_QuickStumble`, `_AI_PierceStumble`,
/// `_AI_HeavyParadeStumble` (S:4aaf0, S:46f80, S:489c0, S:47a90, S:43490): the hit reaction
/// starts (`Action` = the stumble); the host plays it for the length it knows.
pub fn start_stumble(host: &mut dyn ScriptHost, npc: EntityId, action: Action) {
    host.set_current_action(npc, action);
}

/// Port of `_AI_LieKnockDown` (S:467c0): the knocked-down NPC lies on the ground.
pub fn start_lie_knock_down(host: &mut dyn ScriptHost, npc: EntityId) {
    host.set_ani_state(npc, AniState::LieKnockDown);
}

/// Port of `_AI_GetUpAttack` / `_AI_GetUpParade` (S:41bd0, S:41f20): a knocked-down fighter
/// attacks or parries on its way up; the pose becomes standing and the action the get-up move.
pub fn start_get_up(host: &mut dyn ScriptHost, npc: EntityId, attack: bool) {
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
    host.set_current_action(npc, Action::Reload);
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
