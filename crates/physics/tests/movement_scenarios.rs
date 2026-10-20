//! Headless scenarios for the movement pipeline (E01) and the mode machine (E02).
use glam::Vec3;
use gothic_physics::movement::{MoveKeys, wish_direction};
use gothic_physics::{
    CharacterController, CollisionWorld, Gait, ModeChange, MoveCommand, MoveMode, MovementMode,
    MovementProfile, Tri,
};

const DT: f32 = 1.0 / 60.0;

fn quad(y: f32, half: f32) -> Vec<Tri> {
    let a = Vec3::new(-half, y, -half);
    let b = Vec3::new(half, y, -half);
    let c = Vec3::new(half, y, half);
    let d = Vec3::new(-half, y, half);
    vec![[a, c, b], [a, d, c]]
}

/// Box with top at `top` spanning x0..x1, z -800..800.
fn ledge(x0: f32, x1: f32, top: f32) -> Vec<Tri> {
    let (zn, zp) = (-800.0, 800.0);
    let a = Vec3::new(x0, top, zn);
    let b = Vec3::new(x1, top, zn);
    let c = Vec3::new(x1, top, zp);
    let d = Vec3::new(x0, top, zp);
    let lo = Vec3::new(x0, -100.0, zn);
    let lo2 = Vec3::new(x0, -100.0, zp);
    vec![[a, c, b], [a, d, c], [lo, a, d], [lo, d, lo2]]
}

fn ramp(deg: f32, len: f32) -> Vec<Tri> {
    let s = deg.to_radians().tan();
    let p = |x: f32, z: f32| Vec3::new(x, x.max(0.0) * s, z);
    vec![
        [p(-2000.0, -800.0), p(0.0, 800.0), p(0.0, -800.0)],
        [p(-2000.0, -800.0), p(-2000.0, 800.0), p(0.0, 800.0)],
        [p(0.0, -800.0), p(len, 800.0), p(len, -800.0)],
        [p(0.0, -800.0), p(0.0, 800.0), p(len, 800.0)],
    ]
}

fn world(chunks: &[Vec<Tri>]) -> CollisionWorld {
    let mut w = CollisionWorld::new();
    for (i, c) in chunks.iter().enumerate() {
        w.insert_chunk(i as u64 + 1, c.clone());
    }
    w
}

fn hero_at(p: Vec3) -> CharacterController {
    CharacterController::new(MovementProfile::hero(), p)
}

fn cmd(dir: Vec3, gait: Gait) -> MoveCommand {
    MoveCommand {
        wish: dir,
        gait,
        ..MoveCommand::default()
    }
}

fn run(c: &mut CharacterController, w: &CollisionWorld, cmd: &MoveCommand, secs: f32) {
    for _ in 0..(secs / DT).round() as usize {
        c.step_command(w, cmd, DT);
    }
}

fn modes(changes: &[ModeChange]) -> Vec<MovementMode> {
    changes.iter().map(|c| c.mode).collect()
}

/// Face -Z (heading 0) with the wish pointing forward.
const FWD: Vec3 = Vec3::NEG_Z;

#[test]
fn walk_run_sprint_walk_stand_sequence_and_speeds() {
    let w = world(&[quad(0.0, 6_000.0)]);
    let mut c = hero_at(Vec3::new(0.0, 5.0, 0.0));
    run(&mut c, &w, &MoveCommand::default(), 1.0);
    c.drain_mode_changes();
    let mut seq = Vec::new();
    for (gait, expect) in [
        (Gait::Run, 400.0),
        (Gait::Sprint, 600.0),
        (Gait::Walk, 150.0),
        (Gait::Sneak, 280.0),
    ] {
        run(&mut c, &w, &cmd(FWD, gait), 1.5);
        assert!(
            (c.velocity.length() - expect).abs() < 2.0,
            "{gait:?}: {} vs {expect}",
            c.velocity.length()
        );
        seq.extend(modes(&c.drain_mode_changes()));
    }
    run(&mut c, &w, &MoveCommand::default(), 0.5);
    seq.extend(modes(&c.drain_mode_changes()));
    assert_eq!(
        seq,
        vec![
            MovementMode::Running,
            MovementMode::Sprinting,
            MovementMode::Walking,
            MovementMode::Sneaking,
            MovementMode::Standing
        ]
    );
    assert_eq!(
        c.velocity,
        Vec3::ZERO,
        "the player stops at once without input"
    );
}

#[test]
fn acceleration_takes_0_4_seconds_to_run_speed() {
    let w = world(&[quad(0.0, 6_000.0)]);
    let mut c = hero_at(Vec3::new(0.0, 5.0, 0.0));
    run(&mut c, &w, &MoveCommand::default(), 1.0);
    let x0 = c.position.z;
    run(&mut c, &w, &cmd(FWD, Gait::Run), 0.4);
    let d = x0 - c.position.z;
    // 0.5 * 1000 * 0.4^2 = 80 cm (turn start is already aligned).
    assert!((d - 80.0).abs() < 8.0, "{d}");
    assert!((c.velocity.length() - 400.0).abs() < 5.0);
}

#[test]
fn turning_follows_the_rate_and_limits_speed_by_direction() {
    let w = world(&[quad(0.0, 6_000.0)]);
    let mut c = hero_at(Vec3::new(0.0, 5.0, 0.0));
    run(&mut c, &w, &cmd(FWD, Gait::Run), 2.0);
    assert!((c.velocity.length() - 400.0).abs() < 2.0);
    // U-turn: wish straight back. Heading turns at 200 deg/s (0.9 s for 180 degrees).
    let back = cmd(-FWD, Gait::Run);
    let mut min_speed = f32::MAX;
    let mut t = 0.0;
    while t < 2.0 {
        c.step_command(&w, &back, DT);
        min_speed = min_speed.min(c.velocity.length());
        t += DT;
        if c.heading.abs() > std::f32::consts::PI - 0.02 && c.turn.velocity_deg.abs() < 1.0 {
            break;
        }
    }
    assert!((t - 0.9).abs() < 0.12, "turned in {t}");
    // While the wish is behind the heading the backward maximum (280) caps the speed.
    assert!(min_speed < 300.0, "{min_speed}");
    run(&mut c, &w, &back, 1.5);
    assert!((c.velocity.length() - 400.0).abs() < 2.0);
    assert!(c.velocity.z > 390.0, "now running +Z: {}", c.velocity);
}

#[test]
fn sprint_turns_at_half_rate() {
    let w = world(&[quad(0.0, 6_000.0)]);
    let turn_time = |gait: Gait| {
        let mut c = hero_at(Vec3::new(0.0, 5.0, 0.0));
        run(&mut c, &w, &cmd(FWD, gait), 1.0);
        let mut t = 0.0;
        let right = cmd(Vec3::X, gait);
        while (c.heading + std::f32::consts::FRAC_PI_2).abs() > 0.03 && t < 4.0 {
            c.step_command(&w, &right, DT);
            t += DT;
        }
        t
    };
    let run_t = turn_time(Gait::Run);
    let sprint_t = turn_time(Gait::Sprint);
    assert!((run_t - 0.45).abs() < 0.1, "{run_t}");
    assert!((sprint_t - 0.9).abs() < 0.15, "{sprint_t}");
}

#[test]
fn camera_relative_keys_drive_the_wish_direction() {
    // Camera looking along +X: forward key goes +X, right key goes +Z.
    let fwd = Vec3::X;
    let right = Vec3::Z;
    let w = world(&[quad(0.0, 6_000.0)]);
    let mut c = hero_at(Vec3::new(0.0, 5.0, 0.0));
    let keys = MoveKeys {
        forward: 1.0,
        ..MoveKeys::default()
    };
    run(
        &mut c,
        &w,
        &cmd(wish_direction(fwd, right, &keys), Gait::Run),
        2.0,
    );
    assert!(
        c.velocity.x > 390.0 && c.velocity.z.abs() < 5.0,
        "{}",
        c.velocity
    );
    let keys = MoveKeys {
        right: 1.0,
        ..MoveKeys::default()
    };
    run(
        &mut c,
        &w,
        &cmd(wish_direction(fwd, right, &keys), Gait::Run),
        2.0,
    );
    assert!(
        c.velocity.z > 390.0 && c.velocity.x.abs() < 5.0,
        "{}",
        c.velocity
    );
}

#[test]
fn ani_velocity_follows_the_filtered_motion() {
    let w = world(&[quad(0.0, 6_000.0)]);
    let mut c = hero_at(Vec3::new(0.0, 5.0, 0.0));
    run(&mut c, &w, &MoveCommand::default(), 1.0);
    run(&mut c, &w, &cmd(FWD, Gait::Run), 0.2);
    let (fwd, right) = c.local_ani_velocity();
    // Still catching up after 0.2 s (125 ms filter on top of the 0.4 s acceleration).
    assert!(
        fwd > 100.0 && fwd < 380.0 && right.abs() < 5.0,
        "{fwd} {right}"
    );
    run(&mut c, &w, &cmd(FWD, Gait::Run), 1.5);
    let (fwd, _) = c.local_ani_velocity();
    assert!((fwd - 400.0).abs() < 3.0, "{fwd}");
}

#[test]
fn jump_and_landing_mode_sequence() {
    let w = world(&[quad(0.0, 6_000.0)]);
    let mut c = hero_at(Vec3::new(0.0, 5.0, 0.0));
    run(&mut c, &w, &cmd(FWD, Gait::Run), 1.0);
    c.drain_mode_changes();
    let mut jump = cmd(FWD, Gait::Run);
    jump.jump = true;
    c.step_command(&w, &jump, DT);
    jump.jump = false;
    run(&mut c, &w, &jump, 2.0);
    let seq = modes(&c.drain_mode_changes());
    assert_eq!(
        seq,
        vec![
            MovementMode::Jumping,
            MovementMode::Falling,
            MovementMode::Running
        ]
    );
    let last = c.last_landing.expect("landed");
    assert!(
        last.fall_time > 0.3 && last.fall_time < 0.8,
        "{}",
        last.fall_time
    );
    assert!(
        c.machine.last_fall_velocity < -500.0,
        "{}",
        c.machine.last_fall_velocity
    );
    assert_eq!(c.movement_mode(), MovementMode::Running);
}

#[test]
fn sprint_jump_carries_sprint_steering() {
    let w = world(&[quad(0.0, 6_000.0)]);
    let mut c = hero_at(Vec3::new(0.0, 5.0, 0.0));
    run(&mut c, &w, &cmd(FWD, Gait::Sprint), 2.0);
    let mut j = cmd(FWD, Gait::Sprint);
    j.jump = true;
    c.step_command(&w, &j, DT);
    assert!(c.machine.sprint_jump);
    j.jump = false;
    run(&mut c, &w, &j, 0.3);
    assert!(c.velocity.length() > 590.0, "{}", c.velocity.length());
}

#[test]
fn ledge_climb_and_drop_events() {
    // 60 cm step is climbed without a mode change; an 80 cm drop falls.
    let w = world(&[quad(0.0, 6_000.0), ledge(300.0, 1200.0, 60.0)]);
    let mut c = hero_at(Vec3::new(0.0, 5.0, 0.0));
    run(&mut c, &w, &MoveCommand::default(), 1.0);
    c.drain_mode_changes();
    run(&mut c, &w, &cmd(Vec3::X, Gait::Run), 1.5);
    assert!((c.position.y - 60.0).abs() < 2.0, "{}", c.position.y);
    let seq = modes(&c.drain_mode_changes());
    assert!(!seq.contains(&MovementMode::Falling), "{seq:?}");
    let w2 = world(&[quad(0.0, 6_000.0), ledge(-2000.0, 300.0, 120.0)]);
    let mut c = hero_at(Vec3::new(0.0, 125.0, 0.0));
    run(&mut c, &w2, &MoveCommand::default(), 1.0);
    c.drain_mode_changes();
    run(&mut c, &w2, &cmd(Vec3::X, Gait::Run), 2.0);
    let seq = modes(&c.drain_mode_changes());
    assert_eq!(
        seq,
        vec![
            MovementMode::Running,
            MovementMode::Falling,
            MovementMode::Running
        ]
    );
}

#[test]
fn steep_slope_slides_and_reports_the_modes() {
    let w = world(&[ramp(55.0, 3000.0)]);
    let mut c = hero_at(Vec3::new(400.0, 0.0, 0.0));
    c.position.y = 400.0 * 55.0_f32.to_radians().tan() + 2.0;
    run(&mut c, &w, &MoveCommand::default(), 0.8);
    let seq = modes(&c.drain_mode_changes());
    assert!(seq.contains(&MovementMode::Sliding), "{seq:?}");
    assert_eq!(c.mode, MoveMode::Slide);
}

#[test]
fn walkable_slope_stays_walkable_and_slows_uphill() {
    let w = world(&[ramp(35.0, 3000.0)]);
    let mut c = hero_at(Vec3::new(-300.0, 5.0, 0.0));
    run(&mut c, &w, &MoveCommand::default(), 0.5);
    c.drain_mode_changes();
    run(&mut c, &w, &cmd(Vec3::X, Gait::Run), 2.0);
    let seq = modes(&c.drain_mode_changes());
    assert!(
        !seq.contains(&MovementMode::Sliding) && !seq.contains(&MovementMode::Falling),
        "{seq:?}"
    );
    assert!(c.position.y > 100.0, "{}", c.position.y);
    assert!(c.velocity.length() < 280.0 && c.velocity.length() > 100.0);
}

#[test]
fn wading_through_shallow_water_is_slow_and_ends_on_the_shore() {
    let w = world(&[quad(0.0, 6_000.0)]);
    let mut c = hero_at(Vec3::new(0.0, 5.0, 0.0));
    run(&mut c, &w, &MoveCommand::default(), 1.0);
    c.drain_mode_changes();
    let mut wade = cmd(FWD, Gait::Run);
    wade.water_level = Some(80.0);
    run(&mut c, &w, &wade, 1.5);
    assert_eq!(c.movement_mode(), MovementMode::Wading);
    assert!(
        (c.velocity.length() - 150.0).abs() < 3.0,
        "{}",
        c.velocity.length()
    );
    wade.water_level = None;
    run(&mut c, &w, &wade, 1.5);
    assert_eq!(c.movement_mode(), MovementMode::Running);
    let seq = modes(&c.drain_mode_changes());
    assert_eq!(
        seq,
        vec![
            MovementMode::Running,
            MovementMode::Wading,
            MovementMode::Running
        ]
    );
}

#[test]
fn deep_water_swims_at_the_swim_depth_and_dives_on_request() {
    // Pool floor 600 cm below the surface at y = 0.
    let w = world(&[quad(-600.0, 6_000.0)]);
    let mut c = hero_at(Vec3::new(0.0, 60.0, 0.0));
    let mut swim = cmd(Vec3::ZERO, Gait::Run);
    swim.water_level = Some(0.0);
    run(&mut c, &w, &swim, 3.0);
    let seq = modes(&c.drain_mode_changes());
    assert_eq!(seq, vec![MovementMode::Swimming]);
    assert_eq!(c.movement_mode(), MovementMode::Swimming);
    assert!(
        (c.position.y + 155.0).abs() < 8.0,
        "feet at {}",
        c.position.y
    );
    // Swim forward at the walk speed.
    swim.wish = FWD;
    run(&mut c, &w, &swim, 1.5);
    assert!(
        c.velocity.length() > 100.0 && c.velocity.length() < 160.0,
        "{}",
        c.velocity.length()
    );
    assert!((c.position.y + 155.0).abs() < 8.0);
    // Dive.
    swim.dive = true;
    swim.vertical = -1.0;
    run(&mut c, &w, &swim, 1.0);
    assert_eq!(c.movement_mode(), MovementMode::Diving);
    assert!(c.position.y < -250.0, "{}", c.position.y);
    // Surface again.
    swim.dive = false;
    swim.vertical = 0.0;
    run(&mut c, &w, &swim, 2.0);
    assert_eq!(c.movement_mode(), MovementMode::Swimming);
    assert!((c.position.y + 155.0).abs() < 10.0, "{}", c.position.y);
}

#[test]
fn mode_changes_carry_previous_mode_for_the_script_event() {
    let w = world(&[quad(0.0, 6_000.0)]);
    let mut c = hero_at(Vec3::new(0.0, 5.0, 0.0));
    run(&mut c, &w, &MoveCommand::default(), 1.0);
    c.drain_mode_changes();
    run(&mut c, &w, &cmd(FWD, Gait::Sprint), 0.5);
    let ch = c.drain_mode_changes();
    assert_eq!(
        ch,
        vec![ModeChange {
            previous: MovementMode::Standing,
            mode: MovementMode::Sprinting
        }]
    );
    assert_eq!(c.machine.previous(), MovementMode::Standing);
}
