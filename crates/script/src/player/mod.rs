//! Phase 4: the player states (`PS_*`, batches B13-B16 and the interaction part of B18/B19).
//!
//! * [`interact`]: `PS_Interact` and the object states (chests, doors, lock picking, work
//!   stations, beds, seats, shrines), `CanOpen`, `_AI_Unlock`, `PlayerSleep`, `PS_DuskToDawn`.
//! * [`normal`]: `PS_Normal`, the pick-up (`PS_Normal_Take`), looting (`PS_GonnaLoot`), sneaking
//!   and jumping, `PS_QuickUse`.
//! * [`craft`]: synthesis (`OnPlayerSynthRecipe`), frying and transforming stacks, `HasRecipes`.
//! * [`use_item`]: `OnPlayerUseStack`, the item-use dispatcher of the HUD.
//! * [`evade`]: the five `ZS_Evade*` states (the player's roll).
//! * [`melee`]: the `PS_Melee*` input-to-attack mapping.
//!
//! The states are plain [`Def`]s run by the same interpreter as the NPC states (the player is an
//! entity with a current task and state); the game layer never calls them directly but through
//! [`commands`], the safe entry points the runtime commands map to.

pub mod api;
pub mod commands;
pub mod craft;
pub mod evade;
pub mod interact;
pub mod melee;
pub mod normal;
pub mod use_item;

use crate::enums::AiMode;
use crate::host::{EntityId, ScriptHost};
use crate::routine::state::State;
use crate::routine::work::Def;

/// Definitions of all player states (chained behind the NPC states by `combat::definition`).
pub fn definition(state: State) -> Option<Def> {
    interact::definition(state)
        .or_else(|| normal::definition(state))
        .or_else(|| evade::definition(state))
        .or_else(|| melee::definition(state))
}

/// Return the counter of the current step and advance it (the state interpreter contract).
pub(crate) fn step(host: &mut dyn ScriptHost, e: EntityId) -> u32 {
    let n = host.state_progress(e);
    host.set_state_progress(e, n + 1);
    n
}

/// `ContinueRoutine` for the player: back to `PS_Normal`.
pub fn back_to_normal(host: &mut dyn ScriptHost, player: EntityId) {
    host.set_ai_mode(player, AiMode::Routine);
    host.set_current_target(player, EntityId::NONE);
    host.set_task(player, State::PsNormal);
}

#[cfg(test)]
mod tests {
    use super::api::{Lock, LockStatus, PlayerApi, PlayerEvent, Refusal, SkillCategory};
    use super::commands;
    use super::evade::Direction;
    use crate::enums::{Action, UseType};
    use crate::host::{AiCommand, EntityApi, EntityId, NpcApi, StatsApi};
    use crate::routine::state::State;
    use crate::routine::work::Flow;
    use crate::testhost::TestWorld;
    use crate::testhost_player::TestObject;

    fn run_all(w: &mut TestWorld, p: EntityId) {
        for _ in 0..60 {
            let flow = commands::run(w, p);
            w.finish_commands(p);
            if flow == Flow::Done && w.current_task(p) == Some(State::PsNormal) {
                break;
            }
        }
    }

    fn locked_chest(w: &mut TestWorld, difficulty: u32, key: &str) -> EntityId {
        w.add_object(
            UseType::Chest,
            TestObject {
                lock: Some(Lock {
                    status: LockStatus::Locked,
                    key: key.into(),
                    key_amount: 1,
                    difficulty,
                }),
                loot: vec![("It_Gold".into(), 5)],
                ..Default::default()
            },
        )
    }

    #[test]
    fn locked_chest_demands_lockpick_and_perk() {
        let mut w = TestWorld::new();
        let p = w.player();
        let chest = locked_chest(&mut w, 1, "");
        commands::use_object(&mut w, p, chest);
        run_all(&mut w, p);
        assert!(
            w.events()
                .contains(&PlayerEvent::Refused(Refusal::NeedLockpick))
        );
        assert_eq!(w.current_task(p), Some(State::PsNormal));
        w.give_item(p, "It_Key_Lockpick");
        w.pl.events.clear();
        commands::use_object(&mut w, p, chest);
        run_all(&mut w, p);
        assert!(
            w.events()
                .contains(&PlayerEvent::Refused(Refusal::MissingPerk(
                    "Perk_LockPick_1"
                )))
        );
    }

    #[test]
    fn picking_a_lock_opens_the_loot_window() {
        let mut w = TestWorld::new();
        let p = w.player();
        w.grant_skill(p, "Perk_LockPick_1");
        w.grant_skill(p, "Perk_LockPick_2");
        w.give_item(p, "It_Key_Lockpick");
        let chest = locked_chest(&mut w, 1, "");
        w.gui_open = true;
        commands::use_object(&mut w, p, chest);
        for _ in 0..12 {
            commands::run(&mut w, p);
            w.finish_commands(p);
            if w.current_state(p) == Some(State::PsInteractChestLoop) {
                break;
            }
        }
        assert_eq!(w.current_state(p), Some(State::PsInteractChestLoop));
        assert!(
            w.lock(chest)
                .is_some_and(|l| l.status == LockStatus::Unlocked)
        );
        assert_eq!(w.item_count(p, "It_Key_Lockpick"), 0);
        assert!(
            w.events()
                .iter()
                .any(|e| matches!(e, PlayerEvent::Window(_)))
        );
    }

    #[test]
    fn a_key_opens_without_being_used_up() {
        let mut w = TestWorld::new();
        let p = w.player();
        w.give_item(p, "It_Key_Cellar");
        let chest = locked_chest(&mut w, 5, "It_Key_Cellar");
        assert!(super::interact::can_open(&w, p, chest).is_ok());
        assert!(super::interact::unlock(&mut w, p, chest));
        assert_eq!(w.item_count(p, "It_Key_Cellar"), 1);
        let hard = locked_chest(&mut w, 5, "");
        assert_eq!(
            super::interact::can_open(&w, p, hard),
            Err(Refusal::Impossible)
        );
    }

    #[test]
    fn risky_picks_can_fail_but_still_cost_the_pick() {
        let mut w = TestWorld::new();
        let p = w.player();
        w.grant_skill(p, "Perk_LockPick_1");
        w.give_item(p, "It_Key_Lockpick");
        let chest = locked_chest(&mut w, 1, "");
        w.rolls = vec![95];
        assert!(!super::interact::unlock(&mut w, p, chest));
        assert_eq!(w.item_count(p, "It_Key_Lockpick"), 0);
        w.give_item(p, "It_Key_Lockpick");
        w.rolls = vec![10];
        assert!(super::interact::unlock(&mut w, p, chest));
    }

    #[test]
    fn doors_toggle() {
        let mut w = TestWorld::new();
        let p = w.player();
        let door = w.add_object(UseType::Door, TestObject::default());
        for expect in [true, false] {
            commands::use_object(&mut w, p, door);
            run_all(&mut w, p);
            assert_eq!(w.door_open(door), expect);
        }
    }

    #[test]
    fn the_anvil_needs_recipes_and_hammer() {
        let mut w = TestWorld::new();
        let p = w.player();
        let anvil = w.add_object(UseType::Anvil, TestObject::default());
        commands::use_object(&mut w, p, anvil);
        run_all(&mut w, p);
        assert!(
            w.events()
                .contains(&PlayerEvent::Refused(Refusal::NoRecipes(
                    SkillCategory::Smith
                )))
        );
        w.grant_recipes(SkillCategory::Smith);
        w.give_item(p, "It_Hammer_Smith");
        w.pl.events.clear();
        commands::use_object(&mut w, p, anvil);
        for _ in 0..12 {
            commands::run(&mut w, p);
            w.finish_commands(p);
            if w.current_state(p) == Some(State::PsInteractLoop) {
                break;
            }
        }
        assert!(
            w.events()
                .contains(&PlayerEvent::Window(super::api::UiWindow::Smithing))
        );
    }

    #[test]
    fn sleeping_restores_the_hero_and_skips_the_night() {
        let mut w = TestWorld::new();
        let p = w.player();
        w.hour = 22.0;
        w.pm_set_pool(crate::host::Pool::Hit, 10);
        let bed = w.add_object(UseType::Bed, TestObject::default());
        commands::use_object(&mut w, p, bed);
        run_all(&mut w, p);
        assert_eq!(w.pm_pool(crate::host::Pool::Hit), 200);
        assert!((w.pl.hours - 11.0).abs() < 0.01);
        assert!(
            w.events()
                .iter()
                .any(|e| matches!(e, PlayerEvent::Slept { .. }))
        );
    }

    #[test]
    fn pickup_and_confirm_dispatch() {
        let mut w = TestWorld::new();
        let p = w.player();
        let item = w.add_item("It_Apple");
        w.set_item_dropped(item, true);
        assert!(matches!(
            commands::press_confirm(&mut w, p, item),
            super::normal::Confirm::PickUp(_)
        ));
        run_all(&mut w, p);
        assert_eq!(w.item_count(p, "It_Apple"), 1);
        assert!(w.events().contains(&PlayerEvent::PickedUp {
            template: "It_Apple".into(),
            stolen: false
        }));
        let npc = w.spawn_npc("Citizen", crate::enums::Species::Human, 5, 5);
        assert_eq!(
            commands::press_confirm(&mut w, p, npc),
            super::normal::Confirm::Talk(npc)
        );
        let body = w.spawn_npc("Body", crate::enums::Species::Human, 5, 5);
        w.set_ai_mode(body, crate::enums::AiMode::Dead);
        assert_eq!(
            commands::press_confirm(&mut w, p, body),
            super::normal::Confirm::NothingToGet(body)
        );
        w.give_item(body, "It_Gold");
        assert_eq!(super::normal::quick_loot(&mut w, p, body), 1);
        assert_eq!(w.item_count(p, "It_Gold"), 1);
    }

    #[test]
    fn evade_costs_stamina_and_returns_to_the_loop() {
        let mut w = TestWorld::new();
        let p = w.player();
        w.set_slot(p, crate::enums::Slot::RightHand, UseType::OneH, 0);
        w.set_task_directly(p, State::PsMeleeLoop);
        let before = crate::stats::pool(&w, p, crate::host::Pool::Stamina);
        assert!(commands::dodge(&mut w, p, Direction::Left));
        for _ in 0..10 {
            commands::run(&mut w, p);
            w.finish_commands(p);
        }
        assert!(crate::stats::pool(&w, p, crate::host::Pool::Stamina) < before);
        assert_eq!(w.current_state(p), Some(State::PsMeleeLoop));
        w.evade_blocked.insert(p);
        assert!(!commands::dodge(&mut w, p, Direction::Right));
    }

    #[test]
    fn melee_moves_swing_at_the_target() {
        let mut w = TestWorld::new();
        let p = w.player();
        let foe = w.spawn_npc("Foe", crate::enums::Species::Human, 5, 5);
        assert!(
            !commands::melee_move(&mut w, p, Action::QuickAttack, foe),
            "unarmed"
        );
        w.set_slot(p, crate::enums::Slot::RightHand, UseType::OneH, 0);
        assert!(commands::melee_move(&mut w, p, Action::PowerAttack, foe));
        commands::run(&mut w, p);
        let cmds = w.finish_commands(p);
        assert!(cmds.iter().any(|c| matches!(
            c,
            AiCommand::Attack { action: Action::PowerAttack, target } if *target == foe
        )));
    }
}
