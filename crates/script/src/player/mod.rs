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
//! * [`magic`]: the `PS_Magic*` spell states and the `BS_*` book stands.
//! * [`ranged`]: bow and crossbow, `Shoot`.
//! * [`select_weapon`]: `OnPlayerSelectWeapon`, the weapon key.
//! * [`boulder`]: orc boulders and carried stones.
//!
//! The states are plain [`Def`]s run by the same interpreter as the NPC states (the player is an
//! entity with a current task and state); the game layer never calls them directly but through
//! [`commands`], the safe entry points the runtime commands map to.

pub mod api;
pub mod boulder;
pub mod commands;
pub mod craft;
pub mod evade;
pub mod interact;
pub mod magic;
pub mod melee;
pub mod normal;
pub mod ranged;
pub mod select_weapon;
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
        .or_else(|| magic::definition(state))
        .or_else(|| ranged::definition(state))
        .or_else(|| boulder::definition(state))
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
mod magic_tests;

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

    fn melee_hero(w: &mut TestWorld) -> (EntityId, EntityId) {
        let p = w.player();
        let foe = w.spawn_npc("Foe", crate::enums::Species::Human, 5, 5);
        w.set_slot(p, crate::enums::Slot::RightHand, UseType::OneH, 0);
        w.set_primary_pose(p, 1);
        w.set_task_directly(p, State::PsMeleeLoop);
        w.pl.melee_focus = foe;
        (p, foe)
    }

    #[test]
    fn the_melee_loop_turns_queued_actions_into_moves() {
        use crate::host::{AiCommand, MovePhase};
        let mut w = TestWorld::new();
        let (p, foe) = melee_hero(&mut w);
        commands::push_action(&mut w, Action::QuickAttack.raw(), foe);
        commands::run(&mut w, p);
        assert_eq!(w.current_state(p), Some(State::PsMeleeQuickAttack));
        assert_eq!(w.current_target(p), foe, "the loop wrote the combat target");
        commands::run(&mut w, p);
        let cmds = w.finish_commands(p);
        // `_AI_QuickAttack`: pay, one Hit clip with the right hand first, wait 100 ms.
        assert!(matches!(cmds[0], AiCommand::AddStamina(-12)));
        assert!(matches!(
            cmds[1],
            AiCommand::CombatMove {
                action: Action::QuickAttackR,
                phase: MovePhase::Hit,
                ..
            }
        ));
        assert!(matches!(cmds[2], AiCommand::WaitMs { ms: 100 }));
        commands::run(&mut w, p);
        assert_eq!(w.current_state(p), Some(State::PsMeleeLoop));
        // The next quick attack uses the other hand (HitDirection alternates).
        commands::push_action(&mut w, Action::QuickAttack.raw(), foe);
        commands::run(&mut w, p);
        commands::run(&mut w, p);
        let cmds = w.finish_commands(p);
        assert!(cmds.iter().any(|c| matches!(
            c,
            AiCommand::CombatMove {
                action: Action::QuickAttackL,
                ..
            }
        )));
    }

    #[test]
    fn a_power_attack_pays_after_its_raise_clip_and_aligns_to_the_target() {
        use crate::host::{AiCommand, MovePhase};
        let mut w = TestWorld::new();
        let (p, foe) = melee_hero(&mut w);
        commands::push_action(&mut w, Action::PowerAttack.raw(), foe);
        commands::run(&mut w, p);
        commands::run(&mut w, p);
        let cmds = w.finish_commands(p);
        assert!(matches!(
            cmds[0],
            AiCommand::CombatMove { phase: MovePhase::Raise, speed, .. } if (speed - 1.5).abs() < 1e-4
        ));
        assert!(matches!(cmds[1], AiCommand::AddStamina(-20)));
        assert!(matches!(
            cmds[2],
            AiCommand::CombatMove { phase: MovePhase::Hit, speed, .. } if (speed - 1.0).abs() < 1e-4
        ));
        assert_eq!(w.pl.alignment.get(&p), Some(&foe));
        assert_eq!(w.pl.constraints.get(&p), Some(&360.0));
    }

    #[test]
    fn the_guard_aligns_the_body_and_sets_the_pose() {
        use crate::host::{AiCommand, MovePhase};
        let mut w = TestWorld::new();
        let (p, foe) = melee_hero(&mut w);
        commands::push_action(&mut w, Action::Parade.raw(), foe);
        commands::run(&mut w, p);
        assert_eq!(w.current_state(p), Some(State::PsMeleeParade));
        commands::run(&mut w, p);
        let cmds = w.finish_commands(p);
        // A sword without a shield has no `Begin` clip: the pose snaps (`_AI_Parade`).
        assert!(cmds.iter().all(|c| !matches!(
            c,
            AiCommand::CombatMove {
                phase: MovePhase::Begin,
                ..
            }
        )));
        commands::run(&mut w, p);
        assert_eq!(w.ani_state(p), crate::enums::AniState::Parade);
        assert_eq!(w.pl.alignment.get(&p), Some(&foe));
        // In the guard the loop keeps the alignment up each pass.
        commands::run(&mut w, p);
        assert_eq!(w.pl.constraints.get(&p), Some(&360.0));
    }

    #[test]
    fn the_weapon_key_holsters_from_the_melee_loop_and_draws_from_normal() {
        let mut w = TestWorld::new();
        let (p, foe) = melee_hero(&mut w);
        commands::push_action(&mut w, 68, foe);
        commands::run(&mut w, p);
        assert_eq!(w.current_task(p), Some(State::PsNormal));
        assert!(w.pl.queue.is_empty());
        w.set_task_directly(p, State::PsNormalLoop);
        assert!(commands::select_weapon(&mut w, p));
        assert_eq!(w.current_task(p), Some(State::PsMelee));
    }
}
