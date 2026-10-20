//! The gameplay camera: `gothic-camera` (engine port E00) drives the render camera. Mouse look,
//! wheel zoom, camera collision through the physics world, mode switches by hero state, the
//! dialog director and first person all live in the camera crate; this module only feeds it.
use super::pose::Posed;
use super::{HeroActor, Mode, PlayState, ScriptInput};
use crate::walk::{Collision, Player, PlayerCamera, setup, update_camera};
use bevy::audio::SpatialListener;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use gothic_camera::{
    CameraConfig, CameraInput, CameraMode, CameraState, CameraWorld, DialogActor, HeroView,
    PresetTable,
};
use gothic_physics::MovementMode;
use gothic_runtime::CoreRes;
use gothic_sim::item::WeaponMode;

const CM: f32 = 0.01;
/// Height of the hero's local node box (cm) [I: PC_Hero template].
const HERO_BOX_HEIGHT: f32 = 175.0;
/// A hero displacement above this per frame is a teleport (cm).
const TELEPORT_DISTANCE: f32 = 600.0;
/// The body is hidden once the camera fade drops below this (no per-actor alpha yet).
const HIDE_ALPHA: f32 = 0.3;

/// The camera solver and the bookkeeping around it.
#[derive(Resource)]
pub struct PlayCam {
    pub state: CameraState,
    placed: bool,
    last_hero: Vec3,
    last_cut: u32,
    in_dialog: bool,
    wanted: CameraMode,
    after_cinematic: bool,
    first_person: bool,
}

pub fn register(app: &mut App) {
    // The preset table is the built-in copy of `Camera_PC_Camera.tple` (equal to the installed
    // file, checked by a camera test); the tuning is the shipped `ge3.INI` default.
    let mut state = CameraState::new(CameraConfig::default(), PresetTable::default());
    state.seed(0x6A7E);
    app.insert_resource(PlayCam {
        state,
        placed: false,
        last_hero: Vec3::ZERO,
        last_cut: 0,
        in_dialog: false,
        wanted: CameraMode::Normal,
        after_cinematic: false,
        first_person: false,
    })
    .add_systems(Startup, add_listener.after(setup))
    .add_systems(
        Update,
        play_camera
            .after(update_camera)
            .after(crate::walk::player_motion),
    );
}

fn add_listener(mut commands: Commands, cam: Query<Entity, With<PlayerCamera>>) {
    if let Ok(e) = cam.single() {
        commands.entity(e).insert(SpatialListener::new(0.2));
    }
}

fn dialog_actor(id: u64, origin: Vec3) -> DialogActor {
    DialogActor {
        id,
        origin,
        head: origin + Vec3::Y * 165.0,
        volume: 60.0 * 60.0 * 175.0,
        box_height: 175.0,
    }
}

/// Camera mode the hero's state asks for (`PSCamera.SetMode` callers).
fn wanted_mode(player: &Player, state: &PlayState, core: &CoreRes, posed: bool) -> CameraMode {
    if state.hero_down {
        return CameraMode::Death;
    }
    match player.ctrl.machine.mode() {
        MovementMode::Swimming => return CameraMode::Swim,
        MovementMode::Diving => return CameraMode::Dive,
        MovementMode::Falling => return CameraMode::Fall,
        _ => {}
    }
    if posed {
        return CameraMode::Interaction;
    }
    let weapon = if state.drawn {
        core.0.sim.actor(core.0.hero).equipment.weapon_mode()
    } else {
        WeaponMode::None
    };
    match weapon {
        WeaponMode::Melee => CameraMode::Melee,
        WeaponMode::Ranged => CameraMode::Ranged,
        WeaponMode::Magic => CameraMode::Magic,
        WeaponMode::None => CameraMode::Normal,
    }
}

#[allow(clippy::too_many_arguments)]
fn play_camera(
    time: Res<Time>,
    mut player: ResMut<Player>,
    collision: Res<Collision>,
    core: Res<CoreRes>,
    script: Res<ScriptInput>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    cursor: Query<&CursorOptions, With<PrimaryWindow>>,
    ui: Res<gothic_ui::UiState>,
    mut cam_res: ResMut<PlayCam>,
    mut state: ResMut<PlayState>,
    mut cam: Query<(&mut Transform, &mut Projection), With<PlayerCamera>>,
    posed: Query<(), (With<Posed>, With<HeroActor>)>,
) {
    let Ok((mut tf, mut proj)) = cam.single_mut() else {
        return;
    };
    player.command = true;
    if let Some((pos, look)) = state.cinematic {
        *tf = Transform::from_translation(pos).looking_at(look, Vec3::Y);
        cam_res.after_cinematic = true;
        return;
    }
    if player.fly || !player.placed {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    let hero_pos = player.ctrl.position;
    let c = &mut *cam_res;
    let mode_now = player.ctrl.machine.mode();
    let speed = player.ctrl.velocity.length();
    let hero = HeroView {
        position: hero_pos,
        box_height: HERO_BOX_HEIGHT,
        head: None,
        moving: speed > 20.0,
        speed,
        in_combat: state.drawn,
        ranged: state.drawn
            && core.0.sim.actor(core.0.hero).equipment.weapon_mode() == WeaponMode::Ranged,
        down: state.hero_down,
        keep_body_visible: matches!(
            mode_now,
            MovementMode::Jumping
                | MovementMode::Swimming
                | MovementMode::Diving
                | MovementMode::Falling
        ),
    };
    if !c.placed {
        let heading = Vec3::new(-player.yaw.sin(), 0.0, -player.yaw.cos());
        c.state.reset(&hero, heading);
        c.placed = true;
        c.last_hero = hero_pos;
    }
    if c.after_cinematic || hero_pos.distance(c.last_hero) > TELEPORT_DISTANCE {
        c.state.notify_teleport(&hero);
        c.after_cinematic = false;
    }
    c.last_hero = hero_pos;
    let world: &dyn CameraWorld = &collision.0;

    // ---- modes and the dialog director -------------------------------------------------------
    let in_dialog = state.mode == Mode::Dialog;
    let hero_actor = dialog_actor(u64::MAX, hero_pos);
    let npc_actor = state
        .dialog_npc
        .and_then(|id| {
            core.0
                .sim
                .world
                .actors
                .get(&id)
                .map(|w| dialog_actor(id.0 as u64, Vec3::new(w.0.x, w.0.y, w.0.z)))
        })
        .filter(|_| in_dialog);
    let mut dialog_pair = None;
    if let Some(npc) = npc_actor {
        let (listener, speaker) = if state.dialog_player_speaks {
            (npc, hero_actor)
        } else {
            (hero_actor, npc)
        };
        if !c.in_dialog || c.last_cut != state.dialog_cut {
            c.state.set_dialog_roles(listener, speaker, world);
            c.last_cut = state.dialog_cut;
        }
        c.in_dialog = true;
        dialog_pair = Some((listener, speaker));
    } else if c.in_dialog {
        c.state.stop_dialog();
        c.in_dialog = false;
    }
    if !c.in_dialog && !c.first_person {
        let m = wanted_mode(&player, &state, &core, posed.single().is_ok());
        if m != c.wanted {
            c.wanted = m;
            c.state.set_mode(m, false);
        }
    }

    // ---- input -------------------------------------------------------------------------------
    let grabbed = cursor
        .single()
        .is_ok_and(|o| o.grab_mode != CursorGrabMode::None);
    let free = grabbed && !ui.is_open() && !in_dialog && !script.active;
    let toggle = keys.just_pressed(KeyCode::KeyV);
    if toggle {
        c.first_person = !c.first_person;
    }
    let input = CameraInput {
        dt,
        hero,
        look: if free { motion.delta } else { Vec2::ZERO },
        zoom: if free { scroll.delta.y } else { 0.0 },
        look_back: false,
        toggle_first_person: toggle,
        gui_open: ui.is_open(),
        invert_y: false,
        dialog: dialog_pair,
    };
    if script.active && !in_dialog {
        // The smoke script steers the view through yaw / pitch.
        c.state.set_orbit(player.yaw, -player.pitch);
    }
    let out = c.state.update(&input, world);

    // ---- apply -------------------------------------------------------------------------------
    tf.translation = out.position * CM;
    tf.rotation = out.rotation;
    if let Projection::Perspective(p) = &mut *proj {
        p.fov = out.fov_deg.to_radians();
        p.near = (out.z_near * CM).max(0.01);
    }
    state.hide_hero = out.hide_hero || out.hero_alpha < HIDE_ALPHA;
    // The follow camera owns yaw / pitch (movement keys are relative to the view).
    if !in_dialog && !script.active {
        player.yaw = c.state.view_yaw();
        player.pitch = -c.state.orbit_degrees().1.to_radians();
    }
}
