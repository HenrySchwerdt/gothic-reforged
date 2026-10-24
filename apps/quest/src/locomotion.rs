//! Locomotion: left stick = head-relative smooth movement, right stick = snap turn, A = jump.
//! Movement, gravity and jump come from the ported Gothic character controller (`gothic-physics`,
//! `MovementProfile::hero()`: jump 600 cm/s, gravity 981 cm/s^2) held in the cloud `VrPlayer` (centimetres); the XR
//! rig is its metre-scaled shadow and drives `VrPlayer.loco.origin` so the cloud body IK / physics hands get world
//! poses. Collision is the `VrWorld` triangle world the Ardea streamer fills.
use bevy::prelude::*;
use gothic_vr::locomotion::ComfortSettings;
use gothic_vr::{PendingXrFrame, Pose, VrPlayer, VrSet, VrWorld, XrFrame};
use gothic_xr_quest::{XrFrameData, XrRig};

pub const CM: f32 = 100.0;

#[derive(Resource)]
pub struct Physics {
    /// Physics only runs once the ground below the player exists.
    pub ready: bool,
    pub snap_deg: f32,
    snap_armed: bool,
    pub jump_prev: bool,
    /// Steps undone by the no-void guard (diagnostics, logged with [loco]).
    pub void_reverts: u32,
    pub aligned_recenters: u32,
    pub worn: bool,
    pub frames: u32,
    pub stick_click_prev: bool,
    /// Horizontal offset (m) of the tracking origin from the capsule so the head stays over it.
    pub offset: Vec3,
    pub safe: Vec3,
    pub fade_target: f32,
    acc: f32,
    since: f32,
    step_dt: f32,
    wish_prev: Vec3,
    disp_a: Vec3,
    disp_b: Vec3,
}

impl Physics {
    pub fn new(start_cm: Vec3) -> Self {
        Self {
            ready: false,
            snap_deg: 30.0,
            snap_armed: true,
            jump_prev: false,
            void_reverts: 0,
            aligned_recenters: 0,
            worn: false,
            frames: 0,
            stick_click_prev: false,
            offset: Vec3::ZERO,
            safe: start_cm,
            fade_target: 0.0,
            acc: 0.0,
            since: 0.0,
            step_dt: 0.025,
            wish_prev: Vec3::ZERO,
            disp_a: Vec3::ZERO,
            disp_b: Vec3::ZERO,
        }
    }
}

/// Inserts the player, the collision world and the locomotion state (feet position in cm).
pub fn init(app: &mut App, start_cm: Vec3) {
    let head = Pose::new(Vec3::new(0.0, 1.7, 0.0), Quat::IDENTITY);
    let comfort = ComfortSettings {
        room_scale: false,
        teleport: false,
        ..ComfortSettings::default()
    };
    // This module steps the controller itself; the crate's comfort locomotion must not step/drag it again.
    app.insert_resource(gothic_vr::ExternalLocomotion);
    app.insert_resource(VrWorld::default())
        .insert_resource(VrPlayer::new(start_cm / CM, comfort, &head))
        .insert_resource(Physics::new(start_cm));
}

pub struct LocomotionPlugin;

impl Plugin for LocomotionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, feed_vr_frame.in_set(VrSet::Backend))
            .add_systems(Last, play_vr_haptics)
            .add_systems(
                Update,
                locomote.after(VrSet::Locomotion).before(VrSet::Body),
            );
    }
}

/// Writes the OpenXR state into the cloud VR input model.
fn feed_vr_frame(frame: Res<XrFrameData>, mut pending: ResMut<PendingXrFrame>) {
    if !frame.valid {
        return;
    }
    let pose = |t: &Transform| Pose::new(t.translation, t.rotation);
    let mut f = XrFrame {
        head: pose(&frame.head),
        ..default()
    };
    for (i, h) in frame.hands.iter().enumerate() {
        let c = &mut f.hands[i];
        c.pose = h.active.then(|| pose(&h.grip));
        c.trigger = h.trigger;
        c.grip = h.squeeze;
        // The workspace `gothic-vr` comfort locomotion (no ExternalLocomotion switch any more) runs every frame
        // too; it must see no stick / jump input, otherwise the capsule would move twice. `locomote` below reads
        // the raw `XrFrameData` instead.
        c.thumbstick = Vec2::ZERO;
        c.primary = i == 0 && h.primary;
        c.secondary = h.secondary;
        c.stick_click = h.stick_click;
        c.angular_velocity = h.angular_velocity;
    }
    pending.0 = Some(f);
}

/// Plays the workspace `gothic-vr` haptic messages (melee hits, clashes, grabs, bow) on the Quest controllers through the
/// OpenXR output action. The crate's own player is bypassed (External backend), so the same per-hand mixer is run here.
fn play_vr_haptics(
    time: Res<Time<Real>>,
    mut reader: MessageReader<gothic_vr::HapticMessage>,
    mut mixer: Local<gothic_vr::haptics::HapticMixer>,
) {
    let now = time.elapsed_secs();
    for gothic_vr::HapticMessage(p) in reader.read() {
        mixer.push(*p, now);
    }
    let gain = gothic_xr_quest::knob_f32("haptic", 1.0).clamp(0.0, 3.0);
    for p in mixer.take(now) {
        let hand = if p.hand == gothic_vr::Hand::Left {
            0
        } else {
            1
        };
        gothic_xr_quest::haptic_pulse_ex(hand, p.amplitude * gain, p.duration, p.frequency);
    }
}

pub static LOCO_US: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

struct Timer(std::time::Instant);
impl Drop for Timer {
    fn drop(&mut self) {
        LOCO_US.store(
            self.0.elapsed().as_micros() as u32,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
}

fn locomote(
    frame: Res<XrFrameData>,
    mut phys: ResMut<Physics>,
    mut world: ResMut<VrWorld>,
    mut player: ResMut<VrPlayer>,
    mut rig: Query<&mut Transform, With<XrRig>>,
    mut fade: ResMut<gothic_xr_quest::XrFade>,
) {
    let _guard = Timer(std::time::Instant::now());
    if !frame.valid {
        return;
    }
    let Ok(mut t) = rig.single_mut() else { return };
    let mut left = frame.hands[0].stick;
    if gothic_xr_quest::knob("autowalk").as_deref() == Some("1") {
        left = Vec2::new(0.0, 1.0);
    }
    let right = frame.hands[1].stick;
    let dt = frame.dt.clamp(0.0, 0.05);

    // Face the start direction: at startup, after a system recenter (Meta button long-press), once when the
    // headset is first lifted to head height (it may have been lying on a table), and on right-stick click.
    phys.frames += 1;
    let want_yaw = gothic_xr_quest::knob_f32("start_yaw", 0.0).to_radians();
    let click = frame.hands[1].stick_click;
    let click_edge = click && !phys.stick_click_prev;
    phys.stick_click_prev = click;
    let lifted = !phys.worn && frame.head.translation.y > 1.1;
    if phys.frames == 2 || frame.recenters != phys.aligned_recenters || click_edge || lifted {
        let (hyaw, _, _) = frame.head.rotation.to_euler(EulerRot::YXZ);
        t.rotation = Quat::from_rotation_y(want_yaw - hyaw);
        let head_h =
            t.rotation * Vec3::new(frame.head.translation.x, 0.0, frame.head.translation.z);
        let p = player.ctrl.position;
        phys.offset = Vec3::new(-head_h.x, 0.0, -head_h.z);
        t.translation = Vec3::new(p.x / CM, p.y / CM, p.z / CM) + phys.offset;
        phys.aligned_recenters = frame.recenters;
        if lifted {
            phys.worn = true;
        }
        info!(
            "[align] yaw={:.0} recenters={} lifted={lifted} head_y={:.2}",
            (want_yaw - hyaw).to_degrees(),
            frame.recenters,
            frame.head.translation.y
        );
    }

    // Head inside collision geometry (the player leaned into a wall): blank the view (Meta: disable rendering in
    // invalid spaces) with a short fade instead of showing the inside of walls.
    if phys.frames % 4 == 0 {
        let head_cm = t.transform_point(frame.head.translation) * CM;
        let inner = world.0.capsule_blocked(head_cm, head_cm, 10.0);
        let outer = world.0.capsule_blocked(head_cm, head_cm, 22.0);
        phys.fade_target = if inner {
            1.0
        } else if outer {
            0.5
        } else {
            0.0
        };
    }
    {
        let k = (frame.dt * 8.0).min(1.0);
        fade.0 += (phys.fade_target - fade.0) * k;
    }

    // Snap turn about the head so the user does not swing around the rig origin.
    if right.x.abs() > 0.7 {
        if phys.snap_armed {
            let a = -right.x.signum() * phys.snap_deg.to_radians();
            let head_world = t.transform_point(frame.head.translation);
            t.rotate_y(a);
            let new_head = t.transform_point(frame.head.translation);
            let shift = head_world - new_head;
            t.translation += Vec3::new(shift.x, 0.0, shift.z);
            let p = player.ctrl.position;
            phys.offset = Vec3::new(t.translation.x - p.x / CM, 0.0, t.translation.z - p.z / CM);
            phys.snap_armed = false;
        }
    } else if right.x.abs() < 0.3 {
        phys.snap_armed = true;
    }

    if phys.frames % 72 == 0 {
        info!(
            "[loco] left_stick=({:.2},{:.2}) right_stick=({:.2},{:.2}) squeeze=({:.2},{:.2}) ready={} grounded={} void_reverts={} pos_cm=({:.0},{:.0},{:.0})",
            left.x,
            left.y,
            right.x,
            right.y,
            frame.hands[0].squeeze,
            frame.hands[1].squeeze,
            phys.ready,
            player.ctrl.grounded,
            phys.void_reverts,
            player.ctrl.position.x,
            player.ctrl.position.y,
            player.ctrl.position.z
        );
    }
    if phys.ready {
        let (yaw, _, _) = frame.head.rotation.to_euler(EulerRot::YXZ);
        let dir = t.rotation * Quat::from_rotation_y(yaw) * Vec3::new(left.x, 0.0, -left.y);
        let mag = left.length().min(1.0);
        let wish = if mag > 0.15 {
            // Full stick = 600 cm/s = ForwardSpeedMax 400 x fast modifier 1.5 of `MovementProfile::hero()` (the
            // original sprint; `run_speed_matches_forward_speed_max` / `sprint_cannot_exceed_fast_modifier` in
            // gothic-physics). `move_speed` (default 1) is a VR comfort/design multiplier, not an original value.
            dir.normalize_or_zero() * mag * 600.0 * gothic_xr_quest::knob_f32("move_speed", 1.0).clamp(0.2, 2.0)
        } else {
            Vec3::ZERO
        };
        // VR comfort (design decision, not an original value): the original jump (600 cm/s take-off, 981 cm/s^2
        // scene gravity, ~1.8 m apex in ~1.2 s) reads rushed with the head riding along. `jump_time` stretches the
        // airtime by that factor at the SAME apex height: gravity / t^2, take-off / t. Default 2 (user, 2026-10-23).
        {
            let t = gothic_xr_quest::knob_f32("jump_time", 2.0).clamp(1.0, 3.0);
            player.ctrl.config.gravity = 981.0 / (t * t);
            player.ctrl.config.jump_velocity = 600.0 / t;
        }
        let a = frame.hands[1].primary;
        let jump = a && !phys.jump_prev;
        phys.jump_prev = a;

        // The controller runs at 40 Hz with the accumulated dt (its triangle queries are the main CPU cost);
        // the rig position interpolates between the last two steps.
        phys.acc += dt;
        phys.since += dt;
        // Airborne: step every frame (a fall or jump sampled at ~27 Hz and interpolated reads choppy and, with the one-step
        // interpolation lag, as if the ground arrives early). Design decision; the ground walk keeps the cheap 40 Hz.
        let airborne = !player.ctrl.grounded;
        let step_now = jump
            || airborne && phys.acc > 0.0
            || phys.acc >= 1.0 / 40.0
            || phys.wish_prev != wish && phys.acc > 0.0;
        phys.wish_prev = wish;
        if step_now {
            let sdt = phys.acc;
            phys.acc = 0.0;
            let ctrl = &mut player.ctrl;
            let prev = ctrl.position;
            let was_grounded = ctrl.grounded;
            let idle = wish == Vec3::ZERO && !jump && was_grounded && ctrl.vertical_speed == 0.0;
            if !idle {
                ctrl.step(&world.0, wish, jump, sdt);
            }
            // Collision for that spot is not resident (edge of the streamed area, or a cell still loading): never
            // fall into the void. Undo the move like an invisible wall and keep the character standing.
            let p = ctrl.position;
            // Never during a jump or while rising: on device this guard cut every jump short ("100x gravity") and
            // reverted walking steps (18 cm/s at full stick) whenever the ground probe missed.
            if was_grounded
                && !jump
                && ctrl.vertical_speed <= 0.0
                && !ctrl.grounded
                && world.0.ground_below(p + Vec3::Y * 150.0, 2_500.0).is_none()
                && world.0.ground_below(prev + Vec3::Y * 150.0, 2_500.0).is_some()
            {
                ctrl.position = prev;
                ctrl.vertical_speed = 0.0;
                ctrl.grounded = true;
                phys.void_reverts += 1;
            }
            phys.disp_a = phys.disp_b;
            phys.disp_b = player.ctrl.position;
            phys.step_dt = sdt.max(1e-3);
            phys.since = 0.0;
        }
        // Fell out of the world (no collision there): back to the last safe spot.
        if player.ctrl.position.y < phys.safe.y - 20_000.0 {
            let s = phys.safe;
            player.ctrl.teleport(s + Vec3::Y * 100.0);
        } else if player.ctrl.grounded {
            phys.safe = player.ctrl.position;
        }
    }
    if phys.disp_a == Vec3::ZERO {
        phys.disp_a = player.ctrl.position;
        phys.disp_b = player.ctrl.position;
    }
    let alpha = (phys.since / phys.step_dt).clamp(0.0, 1.0);
    let p = phys.disp_a.lerp(phys.disp_b, alpha);
    t.translation = p / CM + phys.offset;
    // The cloud stack reads tracking -> world through `loco.origin` (metres, yaw only).
    player.loco.origin = Pose::new(t.translation, t.rotation);
}
