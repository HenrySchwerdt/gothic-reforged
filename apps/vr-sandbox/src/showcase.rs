//! Scripted showcase scenes for screenshots of the physical melee and the bow without a headset
//! (`--showcase <scene> --screenshot <png> [--screenshot-frame N]`).
//!
//! The scene writes the tracked head and hands itself (external backend) at a fixed 90 Hz step,
//! so the result does not depend on how fast the machine renders. Scenes (training dummy at
//! [`DUMMY`]):
//!
//! * `slash`: a slow sword push into the dummy: the blade is held up at the chest;
//! * `stab`: a fast thrust: the blade sticks in the chest (side view);
//! * `block`: a fast swing into an upright blade held in front of the dummy;
//! * `bow`: arrow from the quiver, nocked and drawn (first person).
//!
//! Each prints the frame to capture on start.
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use gothic_vr::{Hand, PendingXrFrame, Pose, VrPlayer, VrSet, XrFrame};
use gothic_vr_archery::bow::{BowSpec, Quiver};
use gothic_vr_melee::MeleeWeapon;
use gothic_vr_melee::profiles::profile_for;
use gothic_vr_physics::avian3d::prelude::RigidBody;
use gothic_vr_physics::props::{self, PropKind};
use gothic_vr_physics::{Prop, spawn_prop};
use std::time::Duration;

/// Feet of the training dummy the melee scenes use (one of the sandbox's dummies).
pub const DUMMY: Vec3 = Vec3::new(2.0, 0.0, 2.6);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    Slash,
    Stab,
    Block,
    Bow,
}

impl Scene {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "slash" => Scene::Slash,
            "stab" => Scene::Stab,
            "block" => Scene::Block,
            "bow" => Scene::Bow,
            _ => return None,
        })
    }

    /// The frame that shows the point of the scene.
    pub fn capture_frame(self) -> u32 {
        match self {
            Scene::Slash => 190,
            Scene::Stab => 118,
            Scene::Block => 91,
            Scene::Bow => 190,
        }
    }
}

#[derive(Resource)]
struct Showcase {
    scene: Scene,
    frame: u32,
}

pub fn install(app: &mut App, scene: Scene) {
    info!(
        "showcase {scene:?}: capture at frame {}",
        scene.capture_frame()
    );
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 90.0,
    )))
    .insert_resource(Showcase { scene, frame: 0 })
    .add_systems(Update, drive.in_set(VrSet::Backend));
}

/// Edge facing -X (the swing direction), blade forward (-Z).
fn edge_left() -> Quat {
    Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)
}

fn look(from: Vec3, at: Vec3) -> Pose {
    Pose::new(
        from,
        Transform::from_translation(from)
            .looking_at(at, Vec3::Y)
            .rotation,
    )
}

fn lerp(a: Vec3, b: Vec3, f: u32, from: u32, frames: u32) -> Vec3 {
    let t = (f.saturating_sub(from) as f32 / frames as f32).clamp(0.0, 1.0);
    a.lerp(b, t)
}

/// What the scene wants this frame (world space): head, right and left hand with grip.
struct Want {
    head: Pose,
    right: (Pose, bool),
    left: (Pose, bool),
}

fn rest_hand(head: &Pose, hand: Hand) -> Pose {
    let yaw = Quat::from_rotation_y(head.yaw());
    Pose::new(
        head.position + yaw * Vec3::new(0.25 * hand.sign(), -0.6, -0.2),
        yaw,
    )
}

fn melee_want(scene: Scene, f: u32) -> Want {
    let d = DUMMY;
    let (head, start, end, rot, from, frames) = match scene {
        Scene::Slash => (
            look(
                d + Vec3::new(0.15, 1.75, 1.75),
                d + Vec3::new(0.1, 1.2, 0.3),
            ),
            d + Vec3::new(0.7, 1.25, 0.75),
            d + Vec3::new(-0.7, 1.25, 0.75),
            edge_left(),
            80,
            225,
        ),
        Scene::Stab => (
            look(
                d + Vec3::new(1.1, 1.45, 0.55),
                d + Vec3::new(0.0, 1.25, 0.25),
            ),
            d + Vec3::new(0.0, 1.25, 1.15),
            d + Vec3::new(0.0, 1.25, 0.7),
            Quat::IDENTITY,
            80,
            11,
        ),
        _ => (
            look(d + Vec3::new(0.0, 1.9, 1.5), d + Vec3::new(0.15, 1.25, 0.4)),
            d + Vec3::new(0.7, 1.25, 0.75),
            d + Vec3::new(-0.7, 1.25, 0.75),
            edge_left(),
            80,
            16,
        ),
    };
    Want {
        head,
        right: (Pose::new(lerp(start, end, f, from, frames), rot), f > 41),
        left: (rest_hand(&head, Hand::Left), false),
    }
}

fn bow_want(f: u32, bow: Option<Pose>) -> Want {
    // The archer stands at the sandbox start, facing the range (+Z).
    let archer = Pose::new(
        Vec3::new(0.0, 1.7, 4.0),
        Quat::from_rotation_y(std::f32::consts::PI),
    );
    let yaw = archer.rotation;
    let bow_hand = Pose::new(archer.position + yaw * Vec3::new(-0.05, -0.15, -0.65), yaw);
    let quiver = Quiver::default().center(&archer, Hand::Right);
    let (rest, back) = bow.map_or((bow_hand.position, yaw * Vec3::Z), |b| {
        (
            b.transform_point(BowSpec::default().string_rest),
            b.rotation * Vec3::Z,
        )
    });
    let start = rest_hand(&archer, Hand::Right).position;
    let right = match f {
        0..60 => start,
        60..90 => lerp(start, quiver, f, 60, 25),
        90..130 => lerp(quiver, rest + back * 0.11, f, 100, 25),
        _ => lerp(rest + back * 0.11, rest + back * 0.55, f, 135, 35),
    };
    // First person: the avatar's body follows the head, so the head stays with the archer.
    let head = archer;
    Want {
        head,
        right: (Pose::new(right, yaw), f >= 88),
        left: (bow_hand, f > 41),
    }
}

fn drive(
    mut commands: Commands,
    mut show: ResMut<Showcase>,
    player: Res<VrPlayer>,
    props_q: Query<(&Transform, &Prop)>,
    mut pending: ResMut<PendingXrFrame>,
) {
    show.frame += 1;
    let f = show.frame;
    let bow = props_q
        .iter()
        .filter(|(t, p)| p.0.kind == PropKind::Bow && t.translation.z > 3.5)
        .map(|(t, _)| Pose::new(t.translation, t.rotation))
        .next();
    let want = match show.scene {
        Scene::Bow => bow_want(f, bow),
        s => melee_want(s, f),
    };
    if f == 40 {
        match show.scene {
            Scene::Bow => {
                spawn_prop(&mut commands, &props::bow(), want.left.0);
            }
            s => {
                spawn_prop(&mut commands, &props::sword(), want.right.0);
                if s == Scene::Block {
                    // An upright blade in front of the dummy, held by "someone else".
                    let e = spawn_prop(
                        &mut commands,
                        &props::sword(),
                        Pose::new(
                            DUMMY + Vec3::new(0.15, 1.25, 0.35),
                            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                        ),
                    );
                    let mut w = MeleeWeapon::new(profile_for(PropKind::Sword).unwrap());
                    w.wielder = Some(Entity::PLACEHOLDER);
                    commands.entity(e).insert((RigidBody::Kinematic, w));
                }
            }
        }
    }
    // World -> tracking space.
    let to_tracking = player.world(&Pose::IDENTITY).inverse();
    let mut frame = XrFrame {
        head: to_tracking.mul(&want.head),
        ..XrFrame::default()
    };
    for (hand, (pose, grip)) in [(Hand::Right, want.right), (Hand::Left, want.left)] {
        let h = frame.hand_mut(hand);
        h.pose = Some(to_tracking.mul(&pose));
        h.grip = if grip { 1.0 } else { 0.0 };
    }
    pending.0 = Some(frame);
}
