//! The hero's bow and crossbow presentation: aim state, special bow camera, crosshair, aim clips
//! and the aim point for the simulation (`gothic-projectile::aim`, `gothic-camera::aim`, engine
//! port E51 / E52).
//!
//! The state machine is the simulation's (`PS_Ranged_Aim` -> `PS_Ranged_PowerAim` ->
//! `PS_Ranged_Shoot`); this module reads the hero's script state and shows it:
//!
//! * the camera goes to the ranged preset and the special bow mode (the body fades out within
//!   0.5 s, the bow and arrow stay) while an aim state runs;
//! * the point under the screen centre ([`gothic_camera::aim::pick_point`]) is the aim of the
//!   arrow: every frame `Sim::set_hero_aim` gets the arrow in the right hand and that point, the
//!   crosshair scales with its distance ([`gothic_projectile::aim::crosshair_size`]);
//! * the aim clips (`Hero_Stand_Bow_Arrow_P0_Aim_Raise_O_Fwd_..`) play on the upper body from the
//!   state time, with the `Up` / `Down` variant blended by the pitch from the bow to the pick
//!   ([`gothic_projectile::aim::AimPitch`]).
use super::{HeroActor, PlayState};
use crate::walk::{Collision, PlayerCamera};
use bevy::prelude::*;
use gothic_animation::runtime::CharacterComposition;
use gothic_animation::{ActorLibrary, Animator, Overlay};
use gothic_camera::aim::{Pick, pick_point};
use gothic_gameplay::inventory::Slot;
use gothic_projectile::aim::{
    self, AimPitch, Phase, Pitch, clip_name, crosshair_size, hero_aim_sine,
};
use gothic_runtime::CoreRes;
use gothic_sim::item::Use;
use gothic_sim::script::routine::state::State;
use gothic_sim::shots::HeroAim;

const CM: f32 = 0.01;

/// What the hero's ranged weapon is doing, read from the script state.
#[derive(Resource, Default)]
pub struct RangedView {
    /// An aim state runs: the camera is in the special bow mode.
    pub aiming: bool,
    /// The held weapon is a crossbow.
    pub crossbow: bool,
    /// The point under the crosshair of this frame.
    pub pick: Option<Pick>,
    /// Draw multiplier of the sim (0.2..1).
    pub draw: f32,
    pitch: AimPitch,
    /// The clips currently set on the animator, to skip reloading them every frame.
    pub clips: [Option<String>; 2],
    /// Seconds since an aim state was last left (the crosshair fades after the shot).
    since_aim: f32,
    /// The raise glow (`eff_ani_fight_bow_raise_01`) runs.
    glow: bool,
    /// Entity at the bow (left hand) the glow follows.
    anchor: Option<Entity>,
    last_state: Option<State>,
}

/// Fade of the crosshair window: `GUI2::ShowCrossHair` animates it with `AnimateWindow(200 ms,
/// AW_BLEND | AW_ACTIVATE / AW_HIDE)` (Game 0x1007d970).
#[derive(Resource, Default)]
struct CrosshairFade(f32);

const FADE_SECS: f32 = 0.2;
/// `gCHUDCrossHair` is a 32 x 32 static whose minimum is 8 x 8 (fields 0x10..0x1c set in
/// `FUN_1008aa40`); `SetCrossHairSize` (0x1007d9b0 -> 0x100a6bf0) resizes it between them and
/// centres it.
const CROSS_MIN: f32 = 8.0;
const CROSS_MAX: f32 = 32.0;

/// The picture of the crosshair (`G3_HUD_CrosssHair.tga`, `FUN_100a6b30`), once the UI atlas that
/// holds it is loaded.
#[derive(Component)]
struct CrosshairPic;

fn crosshair_texture(
    assets: Option<Res<gothic_ui::UiAssets>>,
    root: Query<(Entity, Option<&Children>), With<Crosshair>>,
    pic: Query<(), With<CrosshairPic>>,
    mut commands: Commands,
    mut done: Local<bool>,
) {
    if *done || !pic.is_empty() {
        return;
    }
    let (Some(assets), Ok((root, _))) = (assets, root.single()) else {
        return;
    };
    let Some(p) = assets.igl.get("g3_hud_crossshair.tga").copied() else {
        eprintln!("crosshair: g3_hud_crossshair.tga is not in the loaded UI atlases");
        *done = true;
        return;
    };
    let Some(h) = assets.page(p.page).cloned() else {
        eprintln!("crosshair: atlas page {:?} is not loaded", p.page);
        *done = true;
        return;
    };
    *done = true;
    eprintln!(
        "crosshair: {:?} rect {} {} {} {}",
        p.page, p.x, p.y, p.w, p.h
    );
    commands.entity(root).with_children(|c| {
        c.spawn((
            CrosshairPic,
            Node {
                width: Val::Px(CROSS_MAX),
                height: Val::Px(CROSS_MAX),
                position_type: PositionType::Absolute,
                ..default()
            },
            ImageNode {
                image: h,
                rect: Some(Rect::new(p.x, p.y, p.x + p.w, p.y + p.h)),
                ..default()
            },
        ));
    });
}

/// An entity that sits at the hero's casting hand (`Slot_RightHand_Weapon`); the hand effects of a
/// cast (`..._load`, `..._hand`, `..._hold`) follow it.
#[derive(Resource, Default)]
pub struct HandAnchor(pub Option<Entity>);

/// The crosshair node (screen centre).
#[derive(Component)]
struct Crosshair;

pub fn register(app: &mut App) {
    app.init_resource::<RangedView>()
        .init_resource::<CrosshairFade>()
        .init_resource::<HandAnchor>()
        .add_systems(Update, crosshair_texture)
        .add_systems(Startup, spawn_crosshair)
        .add_systems(Update, ranged_state.before(super::cam::play_camera))
        .add_systems(Update, shot_animations)
        .add_systems(Update, slot_gizmos)
        .add_systems(Startup, |mut store: ResMut<GizmoConfigStore>| {
            if std::env::var_os("GR_GIZMO").is_some() {
                store.config_mut::<DefaultGizmoConfigGroup>().0.depth_bias = -1.0;
            }
        })
        .add_systems(Update, bow_sync.after(aim_frame))
        .add_systems(Update, aim_frame.after(super::cam::play_camera));
}

fn spawn_crosshair(mut commands: Commands) {
    // A ring and a dot; the texture of the original (`G3_HUD_CrosssHair.tga`) is not resolved
    // to a tile of the loaded atlases yet. The container fills the screen and centres a box
    // whose children are placed from its middle.
    commands
        .spawn((
            Crosshair,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(5),
        ))
        .with_children(|root| {
            root.spawn(Node {
                width: Val::Px(CROSS_BOX),
                height: Val::Px(CROSS_BOX),
                ..default()
            })
            .with_children(|p| {
                let c = CROSS_BOX * 0.5;
                p.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(c - 2.0),
                        top: Val::Px(c - 2.0),
                        width: Val::Px(4.0),
                        height: Val::Px(4.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(1.0, 0.92, 0.7, 0.95)),
                ));
                for (dx, dy) in [(1.0f32, 0.0f32), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                    p.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(c + dx * 14.0 - 2.0),
                            top: Val::Px(c + dy * 14.0 - 2.0),
                            width: Val::Px(4.0),
                            height: Val::Px(4.0),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(1.0, 0.92, 0.7, 0.85)),
                        CrosshairTick { dx, dy },
                    ));
                }
            });
        });
}

const CROSS_BOX: f32 = 80.0;

/// The nodes of the crosshair (grouped to keep `aim_frame` under the system parameter limit).
#[derive(bevy::ecs::system::SystemParam)]
struct CrossParts<'w, 's> {
    ticks: Query<'w, 's, (&'static mut Node, &'static CrosshairTick), Without<CrosshairPic>>,
    pic_nodes: Query<'w, 's, (&'static mut Node, &'static CrosshairPic), Without<CrosshairTick>>,
    pic_imgs: Query<'w, 's, (&'static mut ImageNode, &'static CrosshairPic)>,
}

#[derive(Component)]
struct CrosshairTick {
    dx: f32,
    dy: f32,
}

/// The clip of `Aim_Reload` at `t` seconds into the state.
fn reload_phase(t: f32) -> Phase {
    use gothic_projectile::aim::clips::bow::{RELOAD_BEGIN, RELOAD_RAISE};
    if t < RELOAD_BEGIN {
        Phase::ReloadBegin
    } else if t < RELOAD_BEGIN + RELOAD_RAISE {
        Phase::ReloadRaise
    } else {
        Phase::ReloadEnd
    }
}

/// The hero's aim state from the script state machine.
fn hero_state(core: &CoreRes) -> Option<State> {
    core.0.sim.script.npc(core.0.hero).state
}

fn ranged_state(core: Res<CoreRes>, state: Res<PlayState>, mut view: ResMut<RangedView>) {
    let hero = core.0.sim.actor(core.0.hero);
    let left = hero.equipment.left.as_ref().map(|i| i.use_kind());
    view.crossbow = left == Some(Use::CrossBow);
    let s = hero_state(&core);
    let aiming = state.drawn
        && matches!(left, Some(Use::Bow | Use::CrossBow))
        && matches!(
            s,
            Some(State::PsRangedAim | State::PsRangedPowerAim | State::PsRangedAimLoop)
        );
    view.aiming = aiming;
}

#[allow(clippy::too_many_arguments)]
fn aim_frame(
    time: Res<Time>,
    mut core: ResMut<CoreRes>,
    collision: Res<Collision>,
    mut lib: ResMut<ActorLibrary>,
    mut view: ResMut<RangedView>,
    cam: Query<&Transform, With<PlayerCamera>>,
    mut hero: Query<
        (
            Entity,
            &mut Animator,
            &CharacterComposition,
            &GlobalTransform,
        ),
        With<HeroActor>,
    >,
    mut fx: MessageWriter<gothic_fx::plugin::SpawnEffect>,
    mut stop: MessageWriter<gothic_fx::plugin::StopEffects>,
    mut crosshair: Query<&mut Visibility, With<Crosshair>>,
    mut cross: CrossParts,
    mut fade: ResMut<CrosshairFade>,
    mut commands: Commands,
    mut anchors: Query<&mut Transform, (Without<PlayerCamera>, Without<CrosshairTick>)>,
    mut hand_anchor: ResMut<HandAnchor>,
) {
    let dt = time.delta_secs().min(0.1);
    let Ok(cam) = cam.single() else { return };
    let Ok((hero_e, mut animator, comp, root)) = hero.single_mut() else {
        return;
    };
    let s = hero_state(&core);
    if s != view.last_state {
        if std::env::var_os("GR_RANGED_DEBUG").is_some() {
            let st = core.0.sim.script.npc(core.0.hero);
            eprintln!(
                "ranged: t={:.2} hero state {:?} -> {:?} (task {:?}, drawn {}, quiver {})",
                time.elapsed_secs(),
                view.last_state,
                s,
                st.task,
                core.0.sim.actor(core.0.hero).drawn,
                core.0.sim.actor(core.0.hero).inventory.count("Arrow")
            );
        }
        view.last_state = s;
    }
    // Aim, power aim and the hold of the loop, and the shot itself.
    let in_aim = view.aiming;
    let phase = match s {
        Some(State::PsRangedAim) => Some(Phase::Raise),
        Some(State::PsRangedPowerAim) => Some(Phase::PowerRaise),
        // The bow keeps the last frame of the `PowerRaise` clip while the full draw is held
        // (the aim command of `PS_Ranged_PowerAim` ended, nothing replaces the pose); a crossbow
        // holds the `Aim` pose.
        Some(State::PsRangedAimLoop) if !view.crossbow => Some(Phase::PowerRaise),
        Some(State::PsRangedAimLoop) => Some(Phase::Aim),
        // `PS_Ranged_Aim_Reload`: the next arrow is nocked with the `Reload` clips one after
        // the other (`Begin`, `Raise`, `End`; their lengths are the clips' own).
        Some(State::PsRangedAimReload) if !view.crossbow => {
            let t = core.0.sim.script.npc(core.0.hero).state_time;
            Some(reload_phase(t))
        }
        _ => None,
    };
    let reloading = matches!(s, Some(State::PsRangedAimReload))
        && core.0.sim.actor(core.0.hero).drawn
        && !view.crossbow;
    if in_aim {
        view.since_aim = 0.0;
    } else {
        view.since_aim += dt;
    }

    // The glow on the bow while it is drawn (`PS_Ranged_PowerAim`); the shot stops it.
    let glow_key = 0x3_0000_0000 | u64::from(core.0.hero.0);
    let power = matches!(s, Some(State::PsRangedPowerAim)) && !view.crossbow;
    // The anchor sits at the bow (the left-hand weapon slot) and carries the glow.
    let bow_cm = comp
        .0
        .joint_by_name
        .get("slot_lefthand_weapon")
        .and_then(|&j| animator.joints.get(j as usize))
        .map(|j| (root.affine() * *j).translation.to_vec3());
    let _ = hero_e;
    if power && !view.glow {
        view.glow = true;
        let a = *view.anchor.get_or_insert_with(|| {
            commands
                .spawn((Transform::default(), Visibility::default()))
                .id()
        });
        fx.write(gothic_fx::plugin::SpawnEffect {
            name: aim::RAISE_EFFECT.to_owned(),
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            follow: Some(a),
            permanent: true,
            key: Some(glow_key),
        });
    } else if !power && view.glow {
        view.glow = false;
        stop.write(gothic_fx::plugin::StopEffects { key: glow_key });
    }

    if let (Some(a), Some(p)) = (view.anchor, bow_cm)
        && let Ok(mut t) = anchors.get_mut(a)
    {
        t.translation = p;
    }

    // ---- the pick and the aim handed to the simulation ------------------------------------------
    let cam_cm = cam.translation / CM;
    let pick = pick_point(cam_cm, cam.rotation, &collision.0);
    view.pick = in_aim.then_some(pick);
    // The arrow sits in the right hand (`Slot_RightHand_Weapon` of `gCInventorySlot::GetBone`).
    let (bone, _) = gothic_gameplay::inventory::slot::bone(Slot::RightHand, 1);
    let hand_cm = comp
        .0
        .joint_by_name
        .get(&bone.to_ascii_lowercase())
        .and_then(|&j| animator.joints.get(j as usize))
        .map(|j| (root.affine() * *j).translation.to_vec3() / CM);
    // The casting hand anchor.
    let a = *hand_anchor.0.get_or_insert_with(|| {
        commands
            .spawn((Transform::default(), Visibility::default()))
            .id()
    });
    if let (Ok(mut t), Some(h)) = (anchors.get_mut(a), hand_cm) {
        t.translation = h * CM;
    }
    if core.0.sim.script.magic.hero_hand != hand_cm {
        core.bypass_change_detection().0.sim.set_hero_hand(hand_cm);
    }
    if in_aim && let Some(origin) = hand_cm {
        core.bypass_change_detection()
            .0
            .sim
            .set_hero_aim(Some(HeroAim {
                origin,
                pick_point: pick.point,
            }));
    } else {
        if core.0.sim.script.magic.hero_aim.is_some() {
            core.bypass_change_detection().0.sim.set_hero_aim(None);
        }
    }
    view.draw = aim::draw_multiplier(core.0.sim.script.npc(core.0.hero).state_time);

    // ---- crosshair -----------------------------------------------------------------------------
    // Only touched while it is (or just was) visible: writing a UI node every frame makes the
    // layout pass run for it.
    let step = dt / FADE_SECS;
    let before = fade.0;
    fade.0 = (fade.0 + if in_aim { step } else { -step }).clamp(0.0, 1.0);
    if fade.0 > 0.0 || before > 0.0 {
        if let Ok(mut v) = crosshair.single_mut() {
            let want = if fade.0 > 0.0 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            v.set_if_neq(want);
        }
        // `OnGetAimAngle`: the size follows the pick distance (1 near .. 0 far), the window is
        // resized between its minimum and maximum edge.
        let edge = CROSS_MIN + (CROSS_MAX - CROSS_MIN) * crosshair_size(pick.distance);
        for (mut n, _) in &mut cross.pic_nodes {
            n.width = Val::Px(edge);
            n.height = Val::Px(edge);
        }
        for (mut img, _) in &mut cross.pic_imgs {
            img.color = Color::srgba(1.0, 1.0, 1.0, fade.0);
        }
        if !cross.pic_imgs.is_empty() {
            // The picture replaces the stand-in ticks.
            for (mut n, _) in &mut cross.ticks {
                n.display = Display::None;
            }
        } else {
            let k = 0.5 + 1.2 * crosshair_size(pick.distance);
            for (mut n, t) in &mut cross.ticks {
                n.left = Val::Px(CROSS_BOX * 0.5 + t.dx * 14.0 * k - 2.0);
                n.top = Val::Px(CROSS_BOX * 0.5 + t.dy * 14.0 * k - 2.0);
            }
        }
    }

    // ---- aim clips -----------------------------------------------------------------------------
    let Some(phase) = phase.filter(|_| in_aim || reloading) else {
        if view.clips.iter().any(Option::is_some) {
            animator.aim = [None, None];
            view.clips = [None, None];
            view.pitch.reset();
        }
        return;
    };
    let aim_sine = hand_cm.map_or(0.0, |h| hero_aim_sine(h, pick.point));
    let blend = view.pitch.update(aim_sine);
    let (left, right) = if view.crossbow {
        ("CrossBow", "Bolt")
    } else {
        ("Bow", "Arrow")
    };
    // The crossbow has no draw: its loop holds the aim pose.
    let phase = if view.crossbow && phase == Phase::PowerRaise {
        Phase::Aim
    } else {
        phase
    };
    let family = animator.rig.family.clone();
    // Each clip of the reload starts at its own phase.
    let held_bow = matches!(s, Some(State::PsRangedAimLoop)) && !view.crossbow;
    let t = if held_bow {
        aim::clips::bow::POWER_RAISE
    } else {
        core.0.sim.script.npc(core.0.hero).state_time
    } - match phase {
        Phase::ReloadRaise => aim::clips::bow::RELOAD_BEGIN,
        Phase::ReloadEnd => aim::clips::bow::RELOAD_BEGIN + aim::clips::bow::RELOAD_RAISE,
        _ => 0.0,
    };
    let variants = [
        (Pitch::Fwd, 1.0f32),
        (
            blend.overlay.unwrap_or(Pitch::Up),
            if blend.overlay.is_some() {
                1.0 - blend.forward_weight
            } else {
                0.0
            },
        ),
    ];
    let mask: std::sync::Arc<[f32]> = animator.rig.upper_body_mask().into();
    for (i, (pitch, weight)) in variants.into_iter().enumerate() {
        let name = clip_name(&family, left, right, phase, pitch);
        if weight <= 0.001 {
            animator.aim[i] = None;
            view.clips[i] = None;
            continue;
        }
        let rig = animator.rig.clone();
        if view.clips[i].as_deref() != Some(name.as_str()) || animator.aim[i].is_none() {
            match lib.motion(&rig, &name) {
                Ok(motion) => {
                    animator.aim[i] = Some(Overlay {
                        motion,
                        time: t,
                        rate: 0.0,
                        weight,
                        mask: mask.clone(),
                    });
                    view.clips[i] = Some(name);
                }
                Err(e) => {
                    if view.clips[i].as_deref() != Some("!") {
                        eprintln!("ranged: aim clip {name}: {e:#}");
                    }
                    animator.aim[i] = None;
                    view.clips[i] = Some("!".into());
                }
            }
        }
        if let Some(o) = &mut animator.aim[i] {
            o.time = t;
            o.weight = weight;
        }
    }
}

trait ToVec3 {
    fn to_vec3(self) -> Vec3;
}

impl ToVec3 for bevy::math::Vec3A {
    fn to_vec3(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }
}

/// Upper-body clips of shots and casts: the hero's recovery after an arrow, the cast phases of
/// the hero (`Cast_Raise` -> `Cast_Hit` -> `Cast_Recover`) and the same for NPC archers and
/// casters (their launch event starts the clip; the NPC AI has no animation phases of its own).
fn shot_animations(
    mut events: MessageReader<gothic_runtime::CoreEvent>,
    core: Res<CoreRes>,
    roster: Res<super::Roster>,
    mut q: Query<&mut super::anim::OverlayQueue>,
    hero: Query<Entity, With<HeroActor>>,
) {
    use gothic_runtime::Event;
    let hero_id = core.0.hero;
    let entity_of = |id| {
        if id == hero_id {
            hero.single().ok()
        } else {
            roster.by_actor.get(&id).copied()
        }
    };
    for e in events.read() {
        match &e.0 {
            Event::Projectile { owner, spell, .. } => {
                let Some(en) = entity_of(*owner) else {
                    continue;
                };
                let Ok(mut queue) = q.get_mut(en) else {
                    continue;
                };
                if *spell {
                    // The hero's raise already ran; the missile leaves, hit and recover follow.
                    let steps: &[(&'static str, &'static str)] = if *owner == hero_id {
                        &[("Cast", "Hit"), ("Cast", "Recover")]
                    } else {
                        &[("Cast", "Raise"), ("Cast", "Hit"), ("Cast", "Recover")]
                    };
                    super::anim::queue_steps(&mut queue, steps);
                } else if *owner == hero_id {
                    super::anim::queue_steps(&mut queue, &[("Aim", "Recover")]);
                } else {
                    super::anim::queue_steps(&mut queue, &[("Aim", "Aim"), ("Aim", "Recover")]);
                }
            }
            Event::CastStarted(_) => {
                if let Some(en) = hero.single().ok()
                    && let Ok(mut queue) = q.get_mut(en)
                {
                    super::anim::queue_steps(&mut queue, &[("Cast", "Raise")]);
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The bow actor
// ---------------------------------------------------------------------------------------------

/// The equipped bow or crossbow as the original shows it: an animated actor (`Bow_Hunting_01.xact`
/// with the clips `_Relax` and `_Cock` for the string) linked to the slot bone of its item type,
/// `Slot_LeftHand_Weapon` while the weapon is drawn and `Slot_Bow` / `Slot_CrossBow` on the back
/// (`gCInventorySlot::GetBone`, `AttachItemToSkeleton`).
#[derive(Component)]
pub struct BowVisual {
    pub template: String,
    actor: String,
    clip: String,
}

/// Half turn of a drawn bow about the Z axis of `Slot_LeftHand_Weapon` [I], see `bow_sync`.
const BOW_IN_HAND_TURN: Quat = Quat::from_xyzw(0.0, 0.0, 1.0, 0.0);

/// Candidate actor names of an item template (`It_Bow_Hunting` -> `Bow_Hunting_01`).
fn actor_candidates(template: &str) -> Vec<String> {
    let rest = template.strip_prefix("It_").unwrap_or(template);
    vec![
        format!("{rest}_01"),
        rest.to_owned(),
        format!("G3_Weapons_Ranged_{rest}_01"),
    ]
}

#[allow(clippy::too_many_arguments)]
pub(super) fn bow_sync(
    time: Res<Time>,
    core: Res<CoreRes>,
    state: Res<PlayState>,
    mut spawner: gothic_animation::ActorSpawner,
    hero: Query<(&Animator, &CharacterComposition, &GlobalTransform), With<HeroActor>>,
    mut bow: Query<
        (
            Entity,
            &mut BowVisual,
            &mut Transform,
            &mut Animator,
            &mut Visibility,
        ),
        Without<HeroActor>,
    >,
    mut failed: Local<std::collections::HashSet<String>>,
) {
    let Ok((hero_anim, comp, root)) = hero.single() else {
        return;
    };
    let eq = &core.0.sim.actor(core.0.hero).equipment;
    let want = eq
        .left
        .as_ref()
        .filter(|i| matches!(i.use_kind(), Use::Bow | Use::CrossBow))
        .map(|i| (i.name.clone(), i.use_kind()));
    let have = bow.iter().next().map(|(e, b, ..)| (e, b.template.clone()));
    match (&want, &have) {
        (Some((w, _)), Some((_, h))) if w == h => {}
        (_, Some((e, _))) => {
            spawner.commands.entity(*e).despawn();
        }
        _ => {}
    }
    let Some((template, kind)) = want else { return };
    if have.as_ref().is_some_and(|(_, h)| *h == template) {
        // fall through to the per-frame update below
    } else {
        if failed.contains(&template) {
            return;
        }
        let Some(name) = actor_candidates(&template).into_iter().find_map(|n| {
            let l = n.to_lowercase();
            spawner
                .library
                .actor_names(|a| a.to_lowercase().contains(&l))
                .into_iter()
                .min_by_key(String::len)
        }) else {
            eprintln!("bow: no actor for {template}");
            failed.insert(template);
            return;
        };
        let spec = gothic_animation::NpcSpec::new(name.clone());
        match spawner.spawn(&spec, Transform::from_scale(Vec3::splat(CM))) {
            Ok(e) => {
                spawner.commands.entity(e).insert(BowVisual {
                    template: template.clone(),
                    actor: name.clone(),
                    clip: String::new(),
                });
                eprintln!("bow: {template} shown as actor {name}");
            }
            Err(e) => {
                eprintln!("bow: actor {name} of {template}: {e:#}");
                failed.insert(template);
            }
        }
        return;
    }
    let Ok((_, mut b, mut tf, mut anim, mut vis)) = bow.single_mut() else {
        return;
    };
    // The slot bone: drawn in the left hand, otherwise on the back.
    let slot = if state.drawn {
        "slot_lefthand_weapon"
    } else if kind == Use::CrossBow {
        "slot_crossbow"
    } else {
        "slot_bow"
    };
    if let Some(j) = comp
        .0
        .joint_by_name
        .get(slot)
        .and_then(|&j| hero_anim.joints.get(j as usize))
    {
        let m = root.affine() * *j;
        // The engine links the item to the slot bone without an offset (`GetBone` of
        // `gCInventorySlot`, 0x100bd8d0; `AttachItemToSekelton`, 0x100bdb70 passes none). With
        // that identity the drawn string lies between the right hand and the shooter, which no
        // drawn bow does: the arrow line (string hand -> chord of the limb tips) points 171 degrees
        // away from the view in the `bow_faithful` check. A half turn about the slot's Z axis
        // gives 9 degrees, the hand at the middle of the string. [I] The offset is inferred from
        // that geometry (probe `bow_orient`); the item entity's own link transform in the
        // original is not recovered yet. The hanging bow on the back keeps the identity.
        let rot = if state.drawn {
            BOW_IN_HAND_TURN
        } else {
            Quat::IDENTITY
        };
        *tf = Transform::from_matrix(Mat4::from(m) * Mat4::from_quat(rot));
    }
    vis.set_if_neq(Visibility::Inherited);
    // The string: relaxed, or drawn while the power aim runs (`Cock` played with the draw time).
    let st = core.0.sim.script.npc(core.0.hero);
    // The string is pulled by the draw (`PS_Ranged_PowerAim`) and stays pulled while the full
    // draw is held (`PS_Ranged_Aim_Loop` of a bow); the shot lets it go.
    let drawing = state.drawn
        && (st.state == Some(State::PsRangedPowerAim)
            || (st.state == Some(State::PsRangedAimLoop) && kind == Use::Bow));
    let base = b.actor.clone();
    let want_clip = format!("{base}_{}", if drawing { "Cock" } else { "Relax" });
    if b.clip != want_clip && spawner.library.has_motion(&want_clip) {
        let rig = anim.rig.clone();
        if let Ok(m) = spawner.library.motion(&rig, &want_clip) {
            anim.play(m, 0.0, if drawing { 0.0 } else { 1.0 }, 0.0);
            b.clip = want_clip;
        }
    }
    if drawing {
        // `PS_Ranged_PowerAim` pulls the string for the length of the clip; in the aim loop it
        // stays at the end of it.
        let held = if st.state == Some(State::PsRangedAimLoop) {
            gothic_projectile::aim::clips::bow::POWER_RAISE
        } else {
            st.state_time
                .min(gothic_projectile::aim::clips::bow::POWER_RAISE)
        };
        anim.seek(held);
    }
    let _ = time;
}

/// Screen position (logical pixels) of a world point in cm, if it is in front of the camera.
pub fn screen_of(cam: &Camera, gt: &GlobalTransform, cm: Vec3) -> Option<Vec2> {
    cam.world_to_viewport(gt, cm * CM).ok()
}

/// Debug view (`GR_GIZMO=1`): axes at the two weapon slot bones of the hero, red = X, green = Y,
/// blue = Z of the bone as the evaluator places it, drawn over the meshes.
fn slot_gizmos(
    mut gizmos: Gizmos,
    hero: Query<(&Animator, &CharacterComposition, &GlobalTransform), With<HeroActor>>,
    mut on: Local<Option<bool>>,
) {
    let want = *on.get_or_insert_with(|| std::env::var_os("GR_GIZMO").is_some());
    if !want {
        return;
    }
    let Ok((anim, comp, root)) = hero.single() else {
        return;
    };
    for name in ["slot_lefthand_weapon", "slot_righthand_weapon"] {
        if let Some(j) = comp
            .0
            .joint_by_name
            .get(name)
            .and_then(|&j| anim.joints.get(j as usize))
        {
            let m = root.affine() * *j;
            let (_, rot, pos) = m.to_scale_rotation_translation();
            gizmos.axes(
                Transform::from_translation(pos.into()).with_rotation(rot),
                0.25,
            );
        }
    }
}
