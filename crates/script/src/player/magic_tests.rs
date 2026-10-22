//! Tests of the ranged, book stand, boulder and bribe ports.

use crate::enums::{AttackReason, DamageType, PlayerCrime, Slot, Species, UseType};
use crate::host::{AiCommand, Attribute, EntityApi, EntityId, NpcApi, Pos, StatsApi};
use crate::magic::api::{MagicApi, PathStyle};
use crate::player::api::{Book, BoulderClass, PlayerApi, PlayerEvent};
use crate::player::commands;
use crate::player::ranged::{ARROW_SPEED, POWER_RAISE_SECS, draw_multiplier};
use crate::routine::dialog::{BribeOutcome, bribe};
use crate::routine::state::State;
use crate::testhost::TestWorld;
use crate::testhost_player::TestObject;

fn drive(w: &mut TestWorld, p: EntityId, rounds: usize) -> Vec<AiCommand> {
    let mut all = Vec::new();
    for _ in 0..rounds {
        commands::run(w, p);
        all.extend(w.finish_commands(p));
    }
    all
}

/// The hero with a bow (left) and arrows (right) and an orc in front.
fn archer(crossbow: bool) -> (TestWorld, EntityId, EntityId) {
    let mut w = TestWorld::new();
    let p = w.player();
    let (weapon, ammo) = if crossbow {
        (UseType::CrossBow, UseType::Bolt)
    } else {
        (UseType::Bow, UseType::Arrow)
    };
    w.set_slot(p, Slot::LeftHand, weapon, 1);
    w.set_slot(p, Slot::RightHand, ammo, 1);
    w.mg.slot_damage
        .insert((p, Slot::RightHand), (DamageType::Missile, 30));
    w.mg.slot_damage
        .insert((p, Slot::LeftHand), (DamageType::Missile, 20));
    w.mg.amounts.insert((p, Slot::RightHand as i32 + 1), 3);
    let orc = w.spawn_npc("Orc", Species::Orc, 10, 10);
    w.set_position(orc, Pos::new(0.0, 0.0, 1500.0));
    w.set_task_directly(p, State::PsRanged);
    (w, p, orc)
}

#[test]
fn the_draw_multiplier_grows_for_1_95_seconds() {
    assert!((draw_multiplier(0.0) - 0.2).abs() < 1e-6);
    assert!((draw_multiplier(0.975) - 0.6).abs() < 1e-6);
    assert!((draw_multiplier(1.95) - 1.0).abs() < 1e-6);
    assert!((draw_multiplier(5.0) - 1.0).abs() < 1e-6);
}

#[test]
fn a_fully_drawn_bow_shoots_a_fast_arrow_and_uses_one_up() {
    let (mut w, p, orc) = archer(false);
    drive(&mut w, p, 4);
    assert_eq!(w.current_state(p), Some(State::PsRangedWalkLoop));
    assert!((w.hit_multiplier(p) - 0.2).abs() < 1e-6, "an undrawn bow");
    commands::aim_ranged(&mut w, p, true);
    drive(&mut w, p, 4);
    assert_eq!(w.current_state(p), Some(State::PsRangedPowerAim));
    w.set_state_time(p, 1.95);
    drive(&mut w, p, 1);
    assert!((w.hit_multiplier(p) - 1.0).abs() < 1e-6);
    assert!(commands::release_ranged(&mut w, p, orc));
    drive(&mut w, p, 4);
    assert_eq!(w.mg.projectiles.len(), 1);
    let arrow = &w.mg.projectiles[0];
    assert_eq!(arrow.owner, p);
    assert_eq!(arrow.target, orc);
    assert_eq!(arrow.amount, 50, "arrow 30 plus bow 20");
    assert_eq!(arrow.damage_type, DamageType::Missile);
    assert_eq!(arrow.path, PathStyle::Physics);
    assert!((arrow.velocity - ARROW_SPEED).abs() < 1e-3);
    assert_eq!(w.stack_amount(p, Slot::RightHand as i32 + 1), 2);
    // The next arrow is nocked, then the hero walks again.
    drive(&mut w, p, 6);
    assert_eq!(w.current_state(p), Some(State::PsRangedWalkLoop));
}

#[test]
fn the_raise_clip_ends_in_the_aim_loop() {
    let (mut w, p, _) = archer(false);
    drive(&mut w, p, 4);
    commands::aim_ranged(&mut w, p, true);
    drive(&mut w, p, 4);
    assert_eq!(w.current_state(p), Some(State::PsRangedPowerAim));
    w.set_state_time(p, POWER_RAISE_SECS + 0.01);
    drive(&mut w, p, 2);
    assert_eq!(w.current_state(p), Some(State::PsRangedAimLoop));
    assert!(
        (w.hit_multiplier(p) - 1.0).abs() < 1e-6,
        "full draw is held"
    );
}

#[test]
fn the_abort_key_gives_up_the_draw() {
    let (mut w, p, _) = archer(false);
    drive(&mut w, p, 4);
    commands::aim_ranged(&mut w, p, true);
    drive(&mut w, p, 4);
    commands::aim_ranged(&mut w, p, false);
    drive(&mut w, p, 6);
    assert_eq!(w.current_state(p), Some(State::PsRangedWalkLoop));
    assert!((w.hit_multiplier(p) - 0.2).abs() < 1e-6);
    assert!(w.mg.projectiles.is_empty(), "no arrow left the bow");
}

#[test]
fn the_ranged_task_gives_the_body_to_the_camera() {
    let (mut w, p, _) = archer(false);
    drive(&mut w, p, 4);
    assert_eq!(w.pl.constraints.get(&p).copied(), Some(0.0));
    assert_eq!(w.pl.alignment.get(&p).copied(), Some(EntityId::NONE));
}

#[test]
fn a_short_draw_is_weaker() {
    let (mut w, p, orc) = archer(false);
    drive(&mut w, p, 4);
    commands::aim_ranged(&mut w, p, true);
    drive(&mut w, p, 4);
    w.set_state_time(p, 0.65);
    drive(&mut w, p, 1);
    commands::release_ranged(&mut w, p, orc);
    drive(&mut w, p, 4);
    let arrow = &w.mg.projectiles[0];
    assert!((arrow.hit_mult - 0.467).abs() < 0.01, "{}", arrow.hit_mult);
    assert!((arrow.velocity - 0.467 * ARROW_SPEED).abs() < 3.0);
}

#[test]
fn a_crossbow_always_shoots_at_full_strength_and_is_cocked_again() {
    let (mut w, p, orc) = archer(true);
    drive(&mut w, p, 4);
    assert!((w.hit_multiplier(p) - 1.0).abs() < 1e-6);
    // The crossbow is aimed from the start of the task (`PS_Ranged` -> `PS_Ranged_Aim_Loop`).
    assert_eq!(w.current_state(p), Some(State::PsRangedAimLoop));
    assert!(commands::release_ranged(&mut w, p, orc));
    drive(&mut w, p, 8);
    assert!((w.mg.projectiles[0].hit_mult - 1.0).abs() < 1e-6);
    // After the bolt the hero walks; the reload key cocks and aims again.
    assert_eq!(w.current_state(p), Some(State::PsRangedWalkLoop));
    PlayerApi::action_queue_push(&mut w, 42, EntityId::NONE);
    let cmds = drive(&mut w, p, 8);
    assert!(
        cmds.iter().any(|c| matches!(
            c,
            AiCommand::Action {
                action: crate::enums::Action::Cock,
                ..
            }
        )),
        "the bolt is cocked"
    );
    assert_eq!(w.current_state(p), Some(State::PsRangedAimLoop));
}

#[test]
fn an_empty_quiver_ends_the_ranged_task() {
    let (mut w, p, orc) = archer(false);
    w.mg.amounts.insert((p, Slot::RightHand as i32 + 1), 1);
    drive(&mut w, p, 4);
    commands::aim_ranged(&mut w, p, true);
    drive(&mut w, p, 4);
    commands::release_ranged(&mut w, p, orc);
    // The task passes through `PS_Normal` (the host's world keeps the bow in the hand, so the
    // draw task of the weapon mode takes over again; the real hold puts it away).
    let mut left_ranged = false;
    for _ in 0..16 {
        drive(&mut w, p, 1);
        if w.current_task(p) == Some(State::PsNormal) {
            left_ranged = true;
            break;
        }
    }
    assert_eq!(w.mg.projectiles.len(), 1);
    assert!(left_ranged, "the last arrow ends the ranged task");
}

#[test]
fn an_npc_archer_is_resupplied_and_shoots_in_a_line() {
    let (mut w, _p, orc) = archer(false);
    let archer = w.spawn_npc("Archer", Species::Human, 5, 5);
    w.set_slot(archer, Slot::LeftHand, UseType::Bow, 1);
    w.set_slot(archer, Slot::RightHand, UseType::Arrow, 1);
    w.mg.slot_damage
        .insert((archer, Slot::RightHand), (DamageType::Missile, 10));
    assert!(crate::player::ranged::shoot(&mut w, archer, orc));
    let arrow = w.mg.projectiles.last().expect("arrow");
    assert_eq!(arrow.path, PathStyle::Line);
    assert_eq!(arrow.amount, 10);
    assert_eq!(w.stack_amount(archer, Slot::RightHand as i32 + 1), 1);
}

// ---- book stands ----------------------------------------------------------------------------------------

#[test]
fn the_ancient_wisdom_stand_reads_flood_then_rhobar_then_zuben() {
    let mut w = TestWorld::new();
    let p = w.player();
    let stand = w.add_object(UseType::Bookstand, TestObject::default());
    w.set_current_target(p, stand);
    w.set_task_directly(p, State::BsAncientWisdom);
    drive(&mut w, p, 8);
    assert_eq!(w.book_progress(Book::Flood), 2, "two pages per visit");
    assert_eq!(w.pm_attribute(Attribute::Intelligence), 3);
    assert!(w.used_by_player(stand));
    assert!(w.events().iter().any(|e| matches!(
        e,
        PlayerEvent::BookRead {
            book: "BS_BookOfFlood"
        }
    )));
    w.set_book_progress(Book::Flood, 18);
    w.set_task_directly(p, State::BsAncientWisdom);
    drive(&mut w, p, 8);
    assert_eq!(w.book_progress(Book::Rhobar), 2);
    w.set_book_progress(Book::Rhobar, 8);
    w.set_book_progress(Book::Zuben, 8);
    w.set_task_directly(p, State::BsAncientWisdom);
    drive(&mut w, p, 6);
    assert_eq!(w.pm_attribute(Attribute::Intelligence), 6);
}

#[test]
fn skill_books_add_alchemy_and_smithing() {
    let mut w = TestWorld::new();
    let p = w.player();
    w.set_task_directly(p, State::BsAlchemy);
    drive(&mut w, p, 4);
    assert_eq!(w.pm_attribute(Attribute::Alchemy), 3);
    w.set_task_directly(p, State::BsSmith);
    drive(&mut w, p, 4);
    assert_eq!(w.pm_attribute(Attribute::Smithing), 3);
    w.setup.extended_content = false;
    w.set_task_directly(p, State::BsSmith);
    drive(&mut w, p, 4);
    assert_eq!(
        w.pm_attribute(Attribute::Smithing),
        8,
        "five without extended content"
    );
}

// ---- boulders ----------------------------------------------------------------------------------------------

#[test]
fn a_boulder_is_tilted_held_and_lifted_by_the_strength_of_its_class() {
    let mut w = TestWorld::new();
    let p = w.player();
    let boulder = w.add_object(
        UseType::OrcBoulder,
        TestObject {
            event: "Boulder_Used".into(),
            ..Default::default()
        },
    );
    w.pl.boulders.insert(boulder, BoulderClass::Heavy);
    w.pm_set_attribute(Attribute::Strength, 150);
    commands::use_object(&mut w, p, boulder);
    drive(&mut w, p, 8);
    assert_eq!(
        w.current_state(p),
        Some(State::PsInteractTiltOrcBoulderLoop)
    );
    commands::advance_interaction(&mut w, p);
    drive(&mut w, p, 6);
    assert_eq!(
        w.current_task(p),
        Some(State::PsNormal),
        "holding a heavy one takes 200"
    );

    w.pm_set_attribute(Attribute::Strength, 200);
    commands::use_object(&mut w, p, boulder);
    drive(&mut w, p, 8);
    commands::advance_interaction(&mut w, p);
    drive(&mut w, p, 8);
    assert_eq!(
        w.current_state(p),
        Some(State::PsInteractHoldOrcBoulderLoop)
    );
    commands::advance_interaction(&mut w, p);
    drive(&mut w, p, 6);
    assert_eq!(
        w.current_task(p),
        Some(State::PsNormal),
        "lifting it takes 250"
    );

    w.pm_set_attribute(Attribute::Strength, 250);
    commands::use_object(&mut w, p, boulder);
    drive(&mut w, p, 8);
    commands::advance_interaction(&mut w, p);
    drive(&mut w, p, 8);
    commands::advance_interaction(&mut w, p);
    drive(&mut w, p, 8);
    assert_eq!(
        w.current_state(p),
        Some(State::PsInteractLiftOrcBoulderLoop)
    );
    assert_eq!(
        w.pm_attribute(Attribute::Strength),
        255,
        "the first lift pays 5"
    );
    commands::end_interaction(&mut w, p);
    drive(&mut w, p, 8);
    assert_eq!(w.current_task(p), Some(State::PsNormal));
}

#[test]
fn light_boulders_are_easier_and_the_faring_boulder_raises_its_event() {
    let mut w = TestWorld::new();
    let p = w.player();
    let light = w.add_object(UseType::OrcBoulder, TestObject::default());
    w.pl.boulders.insert(light, BoulderClass::Light);
    w.pm_set_attribute(Attribute::Strength, 100);
    assert!(crate::player::boulder::can_handle(
        &mut w,
        p,
        BoulderClass::Light,
        crate::player::boulder::Stage::Hold
    ));
    assert!(!crate::player::boulder::can_handle(
        &mut w,
        p,
        BoulderClass::Light,
        crate::player::boulder::Stage::Lift
    ));
    assert!(!crate::player::boulder::can_handle(
        &mut w,
        p,
        BoulderClass::Medium,
        crate::player::boulder::Stage::Hold
    ));
    w.pm_set_attribute(Attribute::Strength, 250);
    assert!(crate::player::boulder::can_handle(
        &mut w,
        p,
        BoulderClass::Faring,
        crate::player::boulder::Stage::Lift
    ));
    assert!(w.pl.raised.contains(&"Faring_Boulder_Success".to_string()));
}

// ---- bribe ---------------------------------------------------------------------------------------------------

#[test]
fn an_accepted_bribe_buys_silence_a_refused_one_a_fight() {
    let mut w = TestWorld::new();
    let p = w.player();
    let guard = w.spawn_npc("Guard", Species::Human, 20, 20);
    w.set_last_player_crime(guard, PlayerCrime::Theft);
    let out = commands::bribe(&mut w, p, guard, PlayerCrime::Theft, true);
    assert_eq!(out, BribeOutcome::Accepted);
    assert_eq!(w.last_player_crime(guard), PlayerCrime::None);
    let said = w.finish_commands(guard);
    assert_eq!(
        said,
        vec![
            AiCommand::SayLine {
                key: "LetsForgetThis"
            },
            AiCommand::SayLine {
                key: "ThisTimeYoureLucky"
            },
            AiCommand::SayLine {
                key: "NoMoreTroubleHere"
            },
        ]
    );
    let out = bribe(&mut w, guard, p, PlayerCrime::Theft, false);
    assert_eq!(out, BribeOutcome::Refused);
    assert_eq!(w.attack_reason(guard), AttackReason::Theft);
    assert_eq!(w.current_task(guard), Some(State::Attack));
}

#[test]
fn only_a_chest_pod_that_was_just_opened_is_a_basket() {
    let mut w = TestWorld::new();
    let p = w.player();
    let pod = w.spawn_npc("ChestPod_01", Species::Human, 1, 1);
    let chest = w.spawn_npc("Chest_Wood", Species::Human, 1, 1);
    assert!(!crate::player::interact::is_basket(&w, p, pod));
    w.set_current_action_direct(p, crate::enums::Action::OpenChest);
    assert!(crate::player::interact::is_basket(&w, p, pod));
    assert!(!crate::player::interact::is_basket(&w, p, chest));
}
