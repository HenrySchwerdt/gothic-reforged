//! Anvil hammering, pickpocketing and two-handed lockpicking against synthetic poses.
use glam::{Quat, Vec3};
use gothic_vr_interact::craft_gestures::*;
use gothic_vr_interact::lockpick::{LockSpec, Turn};
use gothic_vr_interact::lockpick_two::*;

const DT: f32 = 1.0 / 72.0;

/// A hammer head falling from `from_y` to the anvil top at `speed` m/s, then lifting again.
fn blow(h: &mut Hammer, x: f32, speed: f32) -> Vec<HammerStep> {
    let mut out = Vec::new();
    let top = h.anvil_top.y;
    let mut y = top + 0.3;
    while y > top - 0.01 {
        out.push(h.step(Vec3::new(h.anvil_top.x + x, y, h.anvil_top.z), DT));
        y -= speed * DT;
    }
    while y < top + 0.25 {
        y += 1.0 * DT;
        out.push(h.step(Vec3::new(h.anvil_top.x + x, y, h.anvil_top.z), DT));
    }
    out
}

#[test]
fn hammering_counts_hard_blows_on_the_anvil_and_finishes_the_recipe() {
    let mut h = Hammer::new(Vec3::new(2.0, 0.9, -1.0), [0.2, 0.1], 3, 42);
    assert!(blow(&mut h, 0.0, 0.3).iter().all(|s| *s == HammerStep::Idle), "a soft tap is no blow");
    assert!(blow(&mut h, 0.5, 3.0).iter().all(|s| *s == HammerStep::Idle), "off the anvil");
    assert!(blow(&mut h, 0.0, 3.0).contains(&HammerStep::Strike(1)));
    assert!(blow(&mut h, 0.05, 3.0).contains(&HammerStep::Strike(2)));
    let last = blow(&mut h, 0.0, 3.0);
    assert!(last.contains(&HammerStep::Finished(42)));
    assert!(matches!(Hammer::command(42), gothic_runtime::Command::Craft { recipe: 42 }));
}

#[test]
fn pressing_the_head_into_the_anvil_does_not_count_twice_without_lifting() {
    let mut h = Hammer::new(Vec3::ZERO, [0.3, 0.3], 5, 1);
    let mut y = 0.3;
    let mut strikes = 0;
    for _ in 0..200 {
        // hammer jitters on the surface
        y = if y > 0.0 { y - 0.05 } else { 0.01 + 0.2 * ((strikes as f32) * 0.0) };
        if matches!(h.step(Vec3::new(0.0, y, 0.0), DT), HammerStep::Strike(_)) {
            strikes += 1;
        }
    }
    assert!(strikes <= 1);
}

#[test]
fn pickpocketing_needs_a_still_hand_in_the_pouch_from_behind() {
    let pouch = Vec3::new(0.0, 1.0, 0.0);
    let p = Pickpocket::new(pouch, 0.0, 1.5); // NPC faces -Z
    assert!(p.behind(Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO));
    assert!(!p.behind(Vec3::new(0.0, 0.0, -1.0), Vec3::ZERO));

    // Steady hand: success after the hold time.
    let mut p = Pickpocket::new(pouch, 0.0, 1.5);
    let mut last = PocketStep::Waiting;
    for _ in 0..(1.6 / DT) as usize {
        last = p.step(true, pouch + Vec3::new(0.001, 0.0, 0.0), 0.0, DT);
        if last == PocketStep::Success {
            break;
        }
    }
    assert_eq!(last, PocketStep::Success);

    // Trembling hand: caught.
    let mut p = Pickpocket::new(pouch, 0.0, 1.5);
    let mut caught = false;
    for i in 0..200 {
        let wobble = if i % 2 == 0 { 0.03 } else { -0.03 };
        if p.step(true, pouch + Vec3::new(wobble, 0.0, 0.0), 0.0, DT) == PocketStep::Caught {
            caught = true;
            break;
        }
    }
    assert!(caught);

    // The NPC turns round: caught. Letting go inside the pouch: caught.
    let mut p = Pickpocket::new(pouch, 0.0, 1.5);
    p.step(true, pouch, 0.0, DT);
    assert_eq!(p.step(true, pouch, 1.2, DT), PocketStep::Caught);
    let mut p = Pickpocket::new(pouch, 0.0, 1.5);
    p.step(true, pouch, 0.0, DT);
    assert_eq!(p.step(false, pouch, 0.0, DT), PocketStep::Caught);
    // Nothing happens outside the pouch.
    let mut p = Pickpocket::new(pouch, 0.0, 1.5);
    assert_eq!(p.step(true, pouch + Vec3::Y, 0.0, DT), PocketStep::Waiting);
}

fn roll_z(a: f32) -> Quat {
    Quat::from_rotation_z(a)
}

#[test]
fn two_hands_the_pick_only_turns_under_tension() {
    let spec = LockSpec::combination(&[Turn::Left, Turn::Right]);
    let at = Vec3::ZERO;
    let mut l = TwoHandLock::new(7, spec, Quat::IDENTITY, at, Quat::IDENTITY, at);
    let step = |l: &mut TwoHandLock, pick: f32, tension: f32| l.step(true, roll_z(pick), at, true, roll_z(tension), at, 0.5, 0.15, 0.2);
    // No tension: a full left turn of the pick does nothing.
    assert!(matches!(step(&mut l, 0.7, 0.0), TwoHandStep::Continue { load } if load == 0.0));
    step(&mut l, 0.0, 0.0);
    // Tension on: the left turn registers, back to neutral, then right opens it.
    assert!(matches!(step(&mut l, 0.0, 0.3), TwoHandStep::Continue { load } if load > 0.3 && load < 0.5));
    assert_eq!(step(&mut l, 0.7, 0.3), TwoHandStep::Turned);
    step(&mut l, 0.0, 0.3);
    assert_eq!(step(&mut l, -0.7, 0.3), TwoHandStep::Done(Some(true)));
}

#[test]
fn over_torque_snaps_the_pick_and_letting_go_aborts() {
    let at = Vec3::ZERO;
    let mut l = TwoHandLock::new(1, LockSpec::combination(&[Turn::Left]), Quat::IDENTITY, at, Quat::IDENTITY, at);
    assert_eq!(l.step(true, Quat::IDENTITY, at, true, roll_z(0.9), at, 0.5, 0.15, 0.2), TwoHandStep::Done(Some(false)));
    let mut l = TwoHandLock::new(1, LockSpec::combination(&[Turn::Left]), Quat::IDENTITY, at, Quat::IDENTITY, at);
    assert_eq!(l.step(true, Quat::IDENTITY, at, false, Quat::IDENTITY, at, 0.5, 0.15, 0.2), TwoHandStep::Aborted);
    // A lock without a combination hands the roll to the script once loaded.
    let mut l = TwoHandLock::new(1, LockSpec { locked: true, has_key: false, combination: None }, Quat::IDENTITY, at, Quat::IDENTITY, at);
    assert_eq!(l.step(true, roll_z(0.7), at, true, roll_z(0.3), at, 0.5, 0.15, 0.2), TwoHandStep::Done(None));
}
