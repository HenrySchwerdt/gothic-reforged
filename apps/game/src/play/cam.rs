//! The gameplay camera: `gothic-camera` (engine port E00) drives the render camera. Mouse look,
//! wheel zoom, camera collision through the physics world, mode switches by hero state, the
//! dialog director and first person all live in the camera crate; this module only feeds it.
use super::pose::Posed;
use super::{HeroActor, Mode, PlayState, ScriptInput};
use crate::walk::{Collision, Player, PlayerCamera, setup, update_camera};
use bevy::audio::SpatialListener;
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
/// Enable `enable_special_bow_mode` while a bow aims (hero fades out within 0.5 s).
const SPECIAL_BOW_HIDES_HERO: bool = false;
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
    options_rev: Option<u32>,
    special_bow: bool,
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
        options_rev: None,
        special_bow: false,
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
    // `PSCamera::SetMode` is called by the combat tasks: the hero's script task decides.
    let weapon = match core.0.sim.hero_script_state().0 {
        Some(gothic_sim::script::routine::state::State::PsMelee) => WeaponMode::Melee,
        Some(gothic_sim::script::routine::state::State::PsRanged) => WeaponMode::Ranged,
        Some(gothic_sim::script::routine::state::State::PsMagic) => WeaponMode::Magic,
        _ if state.drawn => core.0.sim.actor(core.0.hero).equipment.weapon_mode(),
        _ => WeaponMode::None,
    };
    match weapon {
        WeaponMode::Melee => CameraMode::Melee,
        WeaponMode::Ranged => CameraMode::Ranged,
        WeaponMode::Magic => CameraMode::Magic,
        WeaponMode::None => CameraMode::Normal,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn play_camera(
    time: Res<Time>,
    mut player: ResMut<Player>,
    collision: Res<Collision>,
    core: Res<CoreRes>,
    script: Res<ScriptInput>,
    keys: Res<ButtonInput<KeyCode>>,
    frame: Res<super::controls::InputFrame>,
    ranged: Res<super::ranged::RangedView>,
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
    if c.options_rev != Some(frame.rev) {
        // `[Options.Controls]`: mouse sensitivity and smoothing.
        c.state
            .config
            .set_mouse_options(frame.options.sensitivity, frame.options.smoothing);
        c.options_rev = Some(frame.rev);
    }
    let mode_now = player.ctrl.machine.mode();
    let speed = player.ctrl.velocity.length();
    let hero = HeroView {
        position: hero_pos,
        box_height: HERO_BOX_HEIGHT,
        head: None,
        moving: speed > 20.0,
        speed,
        in_combat: state.drawn
            || matches!(
                core.0.sim.hero_script_state().0,
                Some(gothic_sim::script::routine::state::State::PsMelee)
            ),
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
    let toggle = keys.just_pressed(KeyCode::KeyV) || frame.first_person;
    if toggle {
        c.first_person = !c.first_person;
    }
    let input = CameraInput {
        dt,
        hero,
        look: if free {
            Vec2::new(frame.look.x, frame.look.y)
        } else {
            Vec2::ZERO
        },
        zoom: if free { frame.zoom } else { 0.0 },
        look_back: frame.look_back,
        toggle_first_person: toggle,
        gui_open: ui.is_open(),
        invert_y: false,
        dialog: dialog_pair,
    };
    // The aim states of the bow use the special bow mode (the body fades, the weapon stays).
    // The special bow mode fades the body out (`gCCameraAI_PS::OnProcess`); the user report and
    // the capture of the original keep the hero visible while aiming, so the mode stays off
    // until `.local/ref/ranged_magic/SPEC.md` says otherwise.
    let want_special = SPECIAL_BOW_HIDES_HERO && ranged.aiming;
    if want_special != c.special_bow {
        c.special_bow = want_special;
        c.state.enable_special_bow_mode(want_special);
    }
    if frame.reset_camera {
        let heading = Vec3::new(-player.yaw.sin(), 0.0, -player.yaw.cos());
        c.state.reset(&hero, heading);
    }
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
    // First person and the special bow mode hide the body but keep the held items.
    state.keep_items = state.hide_hero && out.items_visible;
    // The follow camera owns yaw / pitch (movement keys are relative to the view).
    if !in_dialog && !script.active {
        player.yaw = c.state.view_yaw();
        player.pitch = -c.state.orbit_degrees().1.to_radians();
    }
}
