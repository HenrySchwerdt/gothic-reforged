//! Sword and bow on the hero's body (original slot bones), physical grab/swing/draw through the cloud VR crates
//! (`vr-physics`, `vr-melee`, `vr-archery`), real Gothic weapon meshes, test dummies and a quiver.
//!
//! Holsters: the sword rests in `Slot_MeleeBackRight`, the bow in `Slot_Bow` (gCInventorySlot bones of the hero
//! skeleton). The weapon mesh is attached with no extra offset (native mesh coordinates in the slot frame), so the
//! physics prop frame is `slot * inverse(visual)`. Squeezing the grip next to a holstered weapon pulls it out (the
//! physics hand's own grab does the rest); letting go near its holster puts it back.
use bevy::asset::RenderAssetUsages;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use gothic_vr::{Hand, Pose, VrBody, VrButton, VrInputRes, VrPlayer, VrWorld, XrHand, XrHead};
use std::collections::HashMap;
use gothic_vr_archery::{FlyingArrow, HeldArrow};
use gothic_vr_melee::{HitzoneSet, PlayerAvatar, spawn_dummy};
use gothic_vr_physics::avian3d::prelude::*;
use gothic_vr_physics::props::{self, PropKind};
use gothic_vr_physics::{HandSet, Held, Prop, VrLayer, spawn_prop};

use crate::actor::{PartOptions, spawn_body};
use crate::ardea::Ardea;
use crate::locomotion::Physics;
use crate::qmat::QuestMaterial;

/// Weapon resting in a hero slot bone.
#[derive(Component)]
pub struct Holstered;

/// Where a weapon belongs (hero slot node) and its visual transform in prop space.
#[derive(Component)]
pub struct HolsterHome {
    pub slot: &'static str,
    pub visual: Transform,
}

#[derive(Component)]
struct GroundBox;
#[derive(Component)]
struct QuiverVisual;

pub struct WeaponsPlugin;

impl Plugin for WeaponsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (spawn_weapons.run_if(resource_exists::<Ardea>), holster_return, weapon_recall, grab_haptics, bow_haptics, holster_feedback, ground_follow, quiver_follow, dummy_visuals, arrow_visuals),
        )
        .add_systems(Startup, tune_archery)
        .add_systems(FixedUpdate, holster_pin.before(HandSet::Grab))
        .add_systems(FixedUpdate, weapon_stabilize.after(HandSet::Drive));
    }
}

/// Generous arrow and nock reach for VR: the quiver opening over the shoulder (radius 26 cm) and the string nock (14 cm).
fn tune_archery(mut commands: Commands, archery: Option<ResMut<gothic_vr_archery::Archery>>) {
    if let Some(mut a) = archery {
        a.archer.quiver.radius = 0.26;
        a.archer.quiver.offset = Vec3::new(0.12, -0.12, 0.16);
        a.archer.nock_radius = 0.14;
    }
    let _ = &mut commands;
}

/// The player's body frame, derived from the tracked head and the player's feet (not from the avatar bones, so the
/// holsters work wherever the avatar happens to be): yaw of the torso (IK body yaw, else head yaw), eye height.
#[derive(Clone, Copy)]
struct BodyFrame {
    feet: Vec3,
    eye: f32,
    yaw: Quat,
}

impl BodyFrame {
    fn new(head: &Transform, feet: Vec3, body: Option<&VrBody>) -> Self {
        let (hy, _, _) = head.rotation.to_euler(EulerRot::YXZ);
        let yaw = body.and_then(|b| b.pose.as_ref()).map_or(hy, |p| p.body_yaw);
        Self { feet, eye: (head.translation.y - feet.y).clamp(1.0, 2.1), yaw: Quat::from_rotation_y(yaw) }
    }
    /// Body-local point (x right, height as a fraction of eye height, z back) to world.
    fn at(&self, x: f32, y_frac: f32, z_back: f32) -> Vec3 {
        Vec3::new(self.feet.x, self.feet.y + y_frac * self.eye, self.feet.z) + self.yaw * Vec3::new(x, 0.0, z_back)
    }
}

/// Holster zone of a weapon: where its grip frame rests on the body and the grab radius (m).
///
/// VR comfort layout (design decision, not the original slot layout): the sword hangs at the LEFT hip (cross draw
/// with the right hand), the bow rides over the left shoulder on the back, the quiver (vr-archery) sits over the
/// right shoulder.
fn zone(frame: &BodyFrame, slot: &str) -> (Transform, f32) {
    match slot {
        "Slot_Bow" => {
            let p = frame.at(-0.15, 0.84, 0.14);
            (Transform::from_translation(p).with_rotation(frame.yaw * Quat::from_rotation_z(0.30)), 0.28)
        }
        _ => {
            // Grip at the hip, blade hanging down and back.
            // Hung from the belt at the left hip: hilt at belt height, the blade angled back and down (about 30 degrees
            // below horizontal) so the tip stays above the knee.
            let p = frame.at(-0.20, 0.62, 0.0);
            let dir = frame.yaw * Vec3::new(-0.10, -0.35, 0.93).normalize();
            (Transform::from_translation(p).with_rotation(Quat::from_rotation_arc(Vec3::NEG_Z, dir)), 0.24)
        }
    }
}

/// All grab spheres of a weapon: the bow can be taken from over either shoulder, with either hand.
fn zone_set(frame: &BodyFrame, slot: &str) -> Vec<(Vec3, f32)> {
    let (t, r) = zone(frame, slot);
    let mut v = vec![(t.translation, r)];
    if slot == "Slot_Bow" {
        v.push((frame.at(0.15, 0.84, 0.14), r));
    }
    v
}

/// Holster zone for a standing player (`feet`, `eye` = eye height above the feet, torso `yaw` about Y), for tools.
pub fn holster_zone(feet: Vec3, eye: f32, yaw: f32, slot: &str) -> (Transform, f32) {
    zone(&BodyFrame { feet, eye, yaw: Quat::from_rotation_y(yaw) }, slot)
}

#[derive(Component)]
struct ZoneMarker {
    slot: &'static str,
    mat: Handle<StandardMaterial>,
}
#[derive(Component)]
struct Scabbard;

#[allow(clippy::type_complexity)]
fn holster_pin(
    mut commands: Commands,
    player: Option<Res<VrPlayer>>,
    body: Option<Res<VrBody>>,
    input: Res<VrInputRes>,
    head: Query<&Transform, (With<XrHead>, Without<XrHand>)>,
    hands: Query<(&Transform, &XrHand), Without<XrHead>>,
    mut props: Query<(Entity, &HolsterHome, &Held, Option<&Holstered>, &mut Position, &mut Rotation, &mut LinearVelocity, &mut AngularVelocity)>,
) {
    let (Some(player), Ok(head)) = (player, head.single()) else { return };
    let frame = BodyFrame::new(head, player.ctrl.position / 100.0, body.as_deref());
    for (e, home, held, holstered, mut pos, mut rot, mut lv, mut av) in &mut props {
        if holstered.is_none() {
            continue;
        }
        if held.count() > 0 {
            commands.entity(e).remove::<Holstered>();
            continue;
        }
        let (t, _radius) = zone(&frame, home.slot);
        // Drawing: a squeezed hand inside the zone takes the weapon (its frame jumps into the hand's grip frame, so
        // the physics grab sees it at distance zero): no precision needed at the body.
        let zs = zone_set(&frame, home.slot);
        let draw = hands.iter().find(|(ht, h)| {
            h.tracked && input.0.held(h.hand, VrButton::Grip) && zs.iter().any(|(c, r)| ht.translation.distance(*c) < *r)
        });
        if let Some((ht, h)) = draw {
            pos.0 = ht.translation;
            rot.0 = ht.rotation;
            lv.0 = Vec3::ZERO;
            av.0 = Vec3::ZERO;
            commands.entity(e).remove::<Holstered>();
            gothic_xr_quest::alog(&format!("draw {} with {:?}", home.slot, h.hand));
            continue;
        }
        pos.0 = t.translation;
        rot.0 = t.rotation;
        lv.0 = Vec3::ZERO;
        av.0 = Vec3::ZERO;
    }
}

/// A released weapon near its holster snaps back.
#[allow(clippy::type_complexity)]
fn holster_return(
    mut commands: Commands,
    player: Option<Res<VrPlayer>>,
    body: Option<Res<VrBody>>,
    head: Query<&Transform, (With<XrHead>, Without<HolsterHome>)>,
    props: Query<(Entity, &HolsterHome, &Held, &Transform), Without<Holstered>>,
) {
    let (Some(player), Ok(head)) = (player, head.single()) else { return };
    let frame = BodyFrame::new(head, player.ctrl.position / 100.0, body.as_deref());
    for (e, home, held, t) in &props {
        if held.count() > 0 {
            continue;
        }
        let (a, radius) = zone(&frame, home.slot);
        if a.translation.distance(t.translation) < radius {
            commands.entity(e).insert(Holstered);
        }
    }
}

/// A weapon that is not held and not at its holster (dropped, thrown, fallen through the ground, out of reach)
/// returns to its holster: after 2 s lying around, at once when it is more than 6 m away or below the floor.
#[allow(clippy::type_complexity)]
fn weapon_recall(
    mut commands: Commands,
    time: Res<Time>,
    player: Option<Res<VrPlayer>>,
    mut lost: Local<HashMap<Entity, f32>>,
    props: Query<(Entity, &Held, &Transform), (With<HolsterHome>, Without<Holstered>)>,
) {
    let Some(player) = player else { return };
    let feet = player.ctrl.position / 100.0;
    lost.retain(|e, _| props.contains(*e));
    for (e, held, t) in &props {
        if held.count() > 0 {
            lost.remove(&e);
            continue;
        }
        let w = lost.entry(e).or_insert(0.0);
        *w += time.delta_secs();
        let far = t.translation.distance(feet) > 6.0 || t.translation.y < feet.y - 1.5 || !t.translation.is_finite();
        if *w > 2.0 || far {
            commands.entity(e).insert(Holstered);
            lost.remove(&e);
            gothic_xr_quest::haptic_pulse(0, 0.2);
            gothic_xr_quest::haptic_pulse(1, 0.2);
        }
    }
}

/// Bow haptics: a buzz on the string hand growing with the draw, a light one on the bow hand, a kick on release.
fn bow_haptics(
    archery: Option<Res<gothic_vr_archery::Archery>>,
    mut fired: MessageReader<gothic_vr_archery::ArrowFiredMessage>,
    time: Res<Time>,
    mut acc: Local<f32>,
    mut was_holding: Local<bool>,
) {
    for f in fired.read() {
        let a = (0.5 + 0.5 * f.draw_factor).clamp(0.0, 1.0);
        gothic_xr_quest::haptic_pulse(0, a);
        gothic_xr_quest::haptic_pulse(1, a);
    }
    let Some(ar) = archery else { return };
    use gothic_vr_archery::bow::ArcherState;
    match ar.archer.state {
        ArcherState::Holding(h) => {
            if !*was_holding {
                gothic_xr_quest::haptic_pulse(h.index(), 0.3);
            }
            *was_holding = true;
        }
        ArcherState::Nocked { hand, draw } => {
            *was_holding = true;
            *acc += time.delta_secs();
            if *acc > 0.04 {
                *acc = 0.0;
                let f = (draw / ar.archer.bow.max_draw.max(1e-3)).clamp(0.0, 1.0);
                gothic_xr_quest::haptic_pulse(hand.index(), 0.08 + 0.45 * f);
                gothic_xr_quest::haptic_pulse(hand.other().index(), 0.05 + 0.2 * f);
            }
        }
        ArcherState::Idle => *was_holding = false,
    }
}

/// Keeps held weapons sane when they collide with something heavy (another weapon, a body, the world): the classic
/// PD + joint blow-up shows as a weapon spinning wildly. Per step: angular and linear speed caps (`debug.gr.max_ang_vel`,
/// `max_lin_vel`), and a separation rule - if the weapon's grip drifts more than 30 cm or 60 degrees from the hand that
/// holds it, it snaps back into the hand and the velocities are cleared.
#[allow(clippy::type_complexity)]
fn weapon_stabilize(
    hands: Query<(&gothic_vr_physics::PhysicsHand, &Position, &Rotation), Without<HolsterHome>>,
    mut props: Query<(&Held, &mut Position, &mut Rotation, &mut LinearVelocity, &mut AngularVelocity), With<HolsterHome>>,
) {
    let max_ang = gothic_xr_quest::knob_f32("max_ang_vel", 25.0);
    let max_lin = gothic_xr_quest::knob_f32("max_lin_vel", 25.0);
    for (held, mut pos, mut rot, mut lv, mut av) in &mut props {
        if !pos.0.is_finite() || !rot.0.is_finite() {
            continue;
        }
        av.0 = av.0.clamp_length_max(max_ang);
        lv.0 = lv.0.clamp_length_max(max_lin);
        if held.count() == 0 {
            continue;
        }
        let Some((_, hp, hr)) = hands.iter().find(|(h, _, _)| held.hands[h.hand.index()].is_some()) else { continue };
        let ang = rot.0.angle_between(hr.0);
        if pos.0.distance(hp.0) > 0.30 || ang > 1.05 {
            pos.0 = hp.0;
            rot.0 = hr.0;
            lv.0 = Vec3::ZERO;
            av.0 = Vec3::ZERO;
            gothic_xr_quest::alog("[grab] held weapon separated from the hand: snapped back");
        }
    }
}

/// A pulse when a hand takes hold of a weapon or lets go.
fn grab_haptics(mut state: Local<HashMap<Entity, [bool; 2]>>, props: Query<(Entity, &Held), With<HolsterHome>>) {
    for (e, held) in &props {
        let now = [held.hands[0].is_some(), held.hands[1].is_some()];
        let old = state.insert(e, now).unwrap_or([false; 2]);
        for k in 0..2 {
            if now[k] != old[k] {
                gothic_xr_quest::haptic_pulse(k, if now[k] { 0.55 } else { 0.2 });
            }
        }
    }
}

/// Feedback: zone markers and the scabbard follow the body; a hand entering a zone gets a haptic pulse and the
/// marker lights up.
#[allow(clippy::type_complexity)]
fn holster_feedback(
    player: Option<Res<VrPlayer>>,
    body: Option<Res<VrBody>>,
    head: Query<&Transform, (With<XrHead>, Without<ZoneMarker>, Without<Scabbard>)>,
    hands: Query<(&Transform, &XrHand), (Without<XrHead>, Without<ZoneMarker>, Without<Scabbard>)>,
    mut markers: Query<(&ZoneMarker, &mut Transform), (Without<Scabbard>, Without<XrHand>, Without<XrHead>)>,
    mut scab: Query<&mut Transform, (With<Scabbard>, Without<ZoneMarker>, Without<XrHand>, Without<XrHead>)>,
    mut state: Local<HashMap<&'static str, [bool; 2]>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    holstered: Query<&HolsterHome, With<Holstered>>,
) {
    let (Some(player), Ok(head)) = (player, head.single()) else { return };
    let frame = BodyFrame::new(head, player.ctrl.position / 100.0, body.as_deref());
    for (m, mut t) in &mut markers {
        let (z, _radius) = zone(&frame, m.slot);
        let has = holstered.iter().any(|h| h.slot == m.slot);
        t.translation = z.translation;
        t.scale = Vec3::splat(if has { 0.05 } else { 0.03 });
        let st = state.entry(m.slot).or_default();
        let mut any = false;
        for (ht, h) in &hands {
            if !h.tracked {
                continue;
            }
            let zs = zone_set(&frame, m.slot);
            let inside = has && zs.iter().any(|(c, r)| ht.translation.distance(*c) < *r);
            any |= inside;
            let k = h.hand.index();
            if inside && !st[k] {
                gothic_xr_quest::haptic_pulse(k, 0.4);
                gothic_xr_quest::alog(&format!("[grab] {:?} hand entered zone {}", h.hand, m.slot));
            }
            st[k] = inside;
        }
        if let Some(mut mat) = mats.get_mut(&m.mat) {
            mat.base_color = if any { Color::srgba(1.0, 0.85, 0.2, 0.9) } else { Color::srgba(0.8, 0.8, 0.8, 0.35) };
        }
    }
    let (z, _) = zone(&frame, "Slot_MeleeBackRight");
    let dir = z.rotation * Vec3::NEG_Z;
    for mut t in &mut scab {
        t.translation = z.translation + dir * 0.42;
        t.rotation = Quat::from_rotation_arc(Vec3::Z, dir);
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_weapons(
    mut commands: Commands,
    mut done: Local<bool>,
    mut a: ResMut<Ardea>,
    phys: Res<Physics>,
    player: Res<VrPlayer>,
    world: Res<VrWorld>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut qmats: ResMut<Assets<QuestMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut inv: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
) {
    if *done || !phys.ready {
        return;
    }
    *done = true;
    let feet = player.ctrl.position / 100.0;
    // Static ground collider for physics props, hands and arrows (follows the player).
    commands.spawn((
        GroundBox,
        RigidBody::Static,
        Collider::cuboid(40.0, 1.0, 40.0),
        CollisionLayers::new(VrLayer::World, [VrLayer::Hand, VrLayer::Prop, VrLayer::Body]),
        Transform::from_translation(feet - Vec3::Y * 0.5),
    ));

    // Sword: the grip frame comes from the mesh (handle centre at the prop origin, blade -Z, edge -Y, flat -X; see grip.rs).
    let sword_vis = a
        .cache()
        .load_mesh(gothic_cook::AssetId::of("g3_weapon_1h_sword_orcslayer_01.xcmsh"))
        .ok()
        .and_then(|md| crate::grip::analyse(&md))
        .map(|g| {
            info!(
                "[weapons] sword grip: blade {:?} edge {:?} flat {:?} centre {:?} handle axis {:?} (guard {:.1}, pommel {:.1}, tip {:.1})",
                g.blade, g.edge, g.flat, g.center, g.handle_axis, g.guard, g.pommel, g.tip
            );
            g.visual(0.01)
        })
        .unwrap_or_else(|| Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::PI)).with_scale(Vec3::splat(0.01)));
    let sword = spawn_prop(&mut commands, &props::sword(), Pose::new(feet + Vec3::new(0.0, 1.0, 0.5), Quat::IDENTITY));
    commands.entity(sword).insert((Holstered, HolsterHome { slot: "Slot_MeleeBackRight", visual: sword_vis }, Visibility::default()));
    a.spawn_static_mirrored(&mut commands, "g3_weapon_1h_sword_orcslayer_01.xcmsh", sword, sword_vis, &mut meshes, &mut qmats, &mut images);

    // Bow (actor, mirrored): stave native Z -> prop Y, shoot direction -X (mirrored) -> prop -Z.
    let bow_rot = Quat::from_mat3(&Mat3::from_cols(Vec3::Z, Vec3::X, Vec3::Y));
    let bow_vis = Transform::from_rotation(bow_rot).with_scale(Vec3::splat(0.01));
    let bow = spawn_prop(&mut commands, &props::bow(), Pose::new(feet + Vec3::new(0.3, 1.0, 0.5), Quat::IDENTITY));
    commands.entity(bow).insert((Holstered, HolsterHome { slot: "Slot_Bow", visual: bow_vis }, Visibility::default()));
    let holder = commands.spawn((bow_vis, Visibility::default(), ChildOf(bow))).id();
    let bowroot = commands.spawn((Transform::IDENTITY, Visibility::default(), ChildOf(holder))).id();
    let _ = spawn_body(
        &mut commands,
        &mut a,
        "it_weapon_ranged_bow_hunting_01.xact",
        bowroot,
        PartOptions { cutout: false, hide_from_eyes: false },
        &mut meshes,
        &mut qmats,
        &mut images,
        &mut inv,
    );
    // The holder carries the visual transform already; undo the actor's own root scale (the actor is spawned in
    // centimetres, the holder scales by 0.01).
    // Quiver (visual only; the archery logic derives its position from the head).
    spawn_quiver(&mut commands, &mut a, &mut meshes, &mut qmats, &mut images, &mut std_mats);

    // Visible holsters: a marker per zone (lights up when a hand is inside).
    for slot in ["Slot_MeleeBackRight", "Slot_Bow"] {
        let mat = std_mats.add(StandardMaterial { base_color: Color::srgba(0.8, 0.8, 0.8, 0.35), unlit: true, alpha_mode: AlphaMode::Blend, ..default() });
        commands.spawn((ZoneMarker { slot, mat: mat.clone() }, Mesh3d(meshes.add(Sphere::new(1.0))), MeshMaterial3d(mat), Transform::IDENTITY));
    }

    // Targets: training dummies (melee) near the start, archery targets further on.
    let fwd = Vec3::NEG_Z;
    for (right, ahead) in [(-2.0f32, 4.0f32), (2.0, 5.0), (0.0, 10.0), (3.0, 20.0), (-3.0, 30.0)] {
        let p = feet + fwd * ahead + Vec3::X * right;
        let g = world.0.ground_below(Vec3::new(p.x, p.y + 3.0, p.z) * 100.0, 800.0).map_or(feet.y, |y| y / 100.0);
        spawn_dummy(&mut commands, Vec3::new(p.x, g, p.z));
    }
    info!("[weapons] spawned sword, bow and dummies");
}

fn ground_follow(
    player: Option<Res<VrPlayer>>,
    world: Res<VrWorld>,
    mut q: Query<&mut Transform, With<GroundBox>>,
) {
    let Some(player) = player else { return };
    let p = player.ctrl.position / 100.0;
    let g = world.0.ground_below(Vec3::new(p.x, p.y + 1.0, p.z) * 100.0, 400.0).map_or(p.y, |y| y / 100.0);
    for mut t in &mut q {
        t.translation = Vec3::new(p.x, g - 0.5, p.z);
    }
}

fn quiver_follow(
    head: Query<&Transform, (With<XrHead>, Without<QuiverVisual>)>,
    mut q: Query<&mut Transform, With<QuiverVisual>>,
) {
    let Ok(h) = head.single() else { return };
    let pose = Pose::new(h.translation, h.rotation);
    let c = gothic_vr_archery::bow::Quiver { offset: Vec3::new(0.12, -0.12, 0.16), radius: 0.26 }.center(&pose, Hand::Right);
    for mut t in &mut q {
        t.translation = c - Vec3::Y * 0.1;
        t.rotation = Quat::from_rotation_y(pose.yaw()) * Quat::from_rotation_z(-0.2);
    }
}

/// Capsule visuals for the training dummies' hitzones.
#[allow(clippy::type_complexity)]
fn dummy_visuals(
    mut commands: Commands,
    sets: Query<(Entity, &HitzoneSet), (Added<HitzoneSet>, Without<PlayerAvatar>)>,
    npcs: Query<(), With<gothic_vr_npc::VrNpc>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let mat = mats.add(StandardMaterial { base_color: Color::srgb(0.75, 0.62, 0.35), unlit: true, ..default() });
    for (e, set) in &sets {
        if npcs.get(e).is_ok() {
            continue;
        }
        for z in &set.zones {
            commands.spawn((
                Mesh3d(meshes.add(Capsule3d::new(z.radius, z.half_length * 2.0))),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(z.pose.position).with_rotation(z.pose.rotation),
            ));
        }
    }
}

/// Arrow visuals: the real arrow mesh (tip at the mesh origin, shaft toward -Z in native coordinates).
#[allow(clippy::type_complexity)]
fn arrow_visuals(
    mut commands: Commands,
    arrows: Query<Entity, Or<(Added<FlyingArrow>, Added<HeldArrow>)>>,
    mut a: Option<ResMut<Ardea>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut qmats: ResMut<Assets<QuestMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(a) = a.as_mut() else { return };
    for e in &arrows {
        commands.entity(e).insert(Visibility::default());
        // Tip toward -Z (prop frame), nock +Z; mesh length ~0.94 m scaled to the 0.75 m prop arrow.
        let s = 0.0080;
        let vis = Transform::from_translation(Vec3::new(0.0, 0.0, -0.375))
            .with_rotation(Quat::from_rotation_y(std::f32::consts::PI))
            .with_scale(Vec3::splat(s));
        let ok = a.spawn_static(&mut commands, "g3_weapons_ranged_bow_arrow_01.xcmsh", e, vis, &mut meshes, &mut qmats, &mut images);
        if !ok {
            let _ = PropKind::Arrow;
        }
    }
}

/// Quiver visual: a leather tube with real arrow meshes (`g3_weapons_ranged_bow_arrow_01`) sticking out, nocks up.
#[allow(clippy::too_many_arguments)]
pub fn spawn_quiver(
    commands: &mut Commands,
    a: &mut Ardea,
    meshes: &mut Assets<Mesh>,
    qmats: &mut Assets<QuestMaterial>,
    images: &mut Assets<Image>,
    std_mats: &mut Assets<StandardMaterial>,
) -> Entity {
    let root = commands.spawn((QuiverVisual, Transform::IDENTITY, Visibility::default())).id();
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(0.055, 0.42))),
        MeshMaterial3d(std_mats.add(StandardMaterial { base_color: Color::srgb(0.20, 0.12, 0.07), unlit: true, ..default() })),
        Transform::IDENTITY,
        ChildOf(root),
    ));
    for (i, (dx, dz, tilt)) in [(0.02f32, 0.0f32, 0.05f32), (-0.025, 0.01, -0.08), (0.0, 0.03, 0.02), (0.03, -0.025, 0.1), (-0.01, -0.03, -0.04)].into_iter().enumerate() {
        let rot = Quat::from_rotation_z(tilt) * Quat::from_rotation_x(tilt * 0.5) * Quat::from_rotation_arc(Vec3::NEG_Z, Vec3::Y);
        let _ = i;
        let vis = Transform { translation: Vec3::new(dx, -0.19, dz), rotation: rot, scale: Vec3::splat(0.006) };
        a.spawn_static_mirrored(commands, "g3_weapons_ranged_bow_arrow_01.xcmsh", root, vis, meshes, qmats, images);
    }
    root
}
