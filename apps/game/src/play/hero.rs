//! The animated hero: input, controller-to-animation bridge, interaction focus, attacks.
use super::anim::{CombatStance, Fallen, OverlayQueue, queue_swing};
use super::{
    HeroActor, Mode, NpcActor, PlayState, ScriptInput, angle_delta, bevy_to_sim_yaw, yaw_of_dir,
};
use crate::walk::{Player, PlayerBody, PlayerCamera, player_motion, update_camera};
use bevy::input::mouse::MouseButton;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use gothic_animation::{ActorSpawner, LocomotionInput, NpcSpec, Stance, WeaponSpec};
use gothic_runtime::{Command, CoreCommand, CoreRes, HERO_REACH, SavePath};
use gothic_sim::world::Pos;
use gothic_ui::UiState;

const CM: f32 = 0.01;
/// Talk range of the interaction prompt (cm).
pub const TALK_RANGE: f32 = 300.0;

pub fn register(app: &mut App) {
    app.add_systems(Startup, spawn_hero)
        .add_systems(
            Update,
            (place_hero, input, focus)
                .chain()
                .before(player_motion)
                .after(crate::walk::input_mode),
        )
        .add_systems(
            Update,
            (sync, super::anim::overlay_system)
                .chain()
                .after(update_camera),
        );
}

fn spawn_hero(mut spawner: ActorSpawner, mut state: ResMut<PlayState>, kit: Res<super::HeroKit>) {
    // PC_Hero wears Head_Player and Body_Player only; the sword is the start-kit item.
    let mut spec = NpcSpec::new("G3_Hero_Body_Player")
        .with_part("G3_Hero_Head_Player")
        .with_stance(Stance::Unarmed);
    if let Some(mesh) = &kit.sword_mesh {
        spec = spec.with_weapon(WeaponSpec::one_hand_sword(mesh));
    }
    match spawner.spawn(&spec, Transform::from_scale(Vec3::splat(CM))) {
        Ok(e) => {
            spawner.commands.entity(e).insert((
                HeroActor,
                OverlayQueue::default(),
                CombatStance(Stance::OneHand),
                Visibility::Hidden,
            ));
            state.hero_entity = Some(e);
        }
        Err(e) => eprintln!("hero spawn failed: {e:#}"),
    }
}

fn camera_forward_xz(cam: &Transform) -> Vec2 {
    let f = cam.rotation * Vec3::NEG_Z;
    Vec2::new(f.x, f.z).normalize_or_zero()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut player: ResMut<Player>,
    mut state: ResMut<PlayState>,
    mut script: ResMut<ScriptInput>,
    ui: Res<UiState>,
    core: Res<CoreRes>,
    save: Res<SavePath>,
    cursor: Query<&CursorOptions, With<PrimaryWindow>>,
    mut commands: MessageWriter<CoreCommand>,
    mut hero: Query<&mut OverlayQueue, With<HeroActor>>,
    time: Res<Time>,
    mut last_attack: Local<f32>,
) {
    let blocked = ui.is_open() || state.mode == Mode::Dialog || state.hero_down;
    let grabbed = cursor
        .single()
        .is_ok_and(|c| c.grab_mode != CursorGrabMode::None);
    let rot = Quat::from_rotation_y(player.yaw);
    if script.active {
        let w = if blocked { Vec3::ZERO } else { script.wish };
        player.scripted_wish = Some(rot.inverse() * Vec3::new(w.x, 0.0, w.z));
    } else {
        player.scripted_wish = blocked.then_some(Vec3::ZERO);
    }
    if keys.just_pressed(KeyCode::F5) {
        commands.write(CoreCommand(Command::Save(save.0.clone())));
    }
    if keys.just_pressed(KeyCode::F9) {
        commands.write(CoreCommand(Command::Load(save.0.clone())));
    }
    if blocked {
        if let Some(i) = script.choose.take() {
            commands.write(CoreCommand(Command::Choose { index: i }));
        }
        if std::mem::take(&mut script.end_dialog) {
            commands.write(CoreCommand(Command::EndDialog));
        }
        return;
    }
    let draw = script.draw.take().or_else(|| {
        if keys.just_pressed(KeyCode::Digit1) {
            Some(true)
        } else if keys.just_pressed(KeyCode::Digit0) {
            Some(false)
        } else {
            None
        }
    });
    if let Some(d) = draw {
        state.drawn = d;
        commands.write(CoreCommand(Command::SetDrawn(d)));
    }
    let talk = std::mem::take(&mut script.talk) || keys.just_pressed(KeyCode::KeyE);
    if talk && let Some(npc) = state.focus {
        state.mode = Mode::Dialog;
        state.dialog_npc = Some(npc);
        commands.write(CoreCommand(Command::Talk { npc }));
        return;
    }
    let attack =
        std::mem::take(&mut script.attack) || (buttons.just_pressed(MouseButton::Left) && grabbed);
    if attack && time.elapsed_secs() - *last_attack > 0.7 {
        *last_attack = time.elapsed_secs();
        state.drawn = true;
        commands.write(CoreCommand(Command::SetDrawn(true)));
        if let Ok(mut q) = hero.single_mut() {
            queue_swing(&mut q);
        }
        // Target: nearest living NPC in reach, in front of the hero.
        let hero_cm = player.ctrl.position;
        let fwd = Vec2::new(
            (-state.hero_yaw.sin()).clamp(-1.0, 1.0),
            (-state.hero_yaw.cos()).clamp(-1.0, 1.0),
        );
        let mut best: Option<(f32, gothic_sim::types::ActorId)> = None;
        for a in &core.0.sim.actors {
            if a.id == core.0.hero || !a.is_alive() {
                continue;
            }
            let Some(p) = core.0.sim.world.actors.get(&a.id).map(|w| w.0) else {
                continue;
            };
            let d = Vec2::new(p.x - hero_cm.x, p.z - hero_cm.z);
            let dist = d.length();
            if dist > HERO_REACH || dist < 1.0 {
                continue;
            }
            if d.normalize().dot(fwd) < 0.2 {
                continue;
            }
            if best.is_none_or(|b| dist < b.0) {
                best = Some((dist, a.id));
            }
        }
        if let Some((_, target)) = best {
            commands.write(CoreCommand(Command::Attack { target }));
        }
    }
}

/// The NPC the camera looks at within talking range.
fn focus(
    player: Res<Player>,
    core: Res<CoreRes>,
    mut state: ResMut<PlayState>,
    cam: Query<&Transform, With<PlayerCamera>>,
) {
    if state.mode == Mode::Dialog {
        return;
    }
    let Ok(cam) = cam.single() else { return };
    let fwd = camera_forward_xz(cam);
    let hero = player.ctrl.position;
    let mut best: Option<(f32, gothic_sim::types::ActorId)> = None;
    for a in &core.0.sim.actors {
        if a.id == core.0.hero || !a.is_alive() {
            continue;
        }
        let Some(p) = core.0.sim.world.actors.get(&a.id).map(|w| w.0) else {
            continue;
        };
        let d = Vec2::new(p.x - hero.x, p.z - hero.z);
        let dist = d.length();
        if dist > TALK_RANGE || dist < 1.0 {
            continue;
        }
        let cos = d.normalize().dot(fwd);
        if cos < 0.55 {
            continue;
        }
        let score = dist / TALK_RANGE + (1.0 - cos);
        if best.is_none_or(|b| score < b.0) {
            best = Some((score, a.id));
        }
    }
    state.focus = best.map(|b| b.1);
}

#[allow(clippy::too_many_arguments)]
fn sync(
    time: Res<Time>,
    player: Res<Player>,
    core: Res<CoreRes>,
    mut state: ResMut<PlayState>,
    mut commands: MessageWriter<CoreCommand>,
    mut q: Query<
        (&mut Transform, &mut LocomotionInput, &mut Visibility),
        (With<HeroActor>, Without<Fallen>),
    >,
    mut body: Query<&mut Visibility, (With<PlayerBody>, Without<HeroActor>)>,
    mut vel: Local<Vec3>,
    npcs: Query<(&NpcActor, &Transform), Without<HeroActor>>,
) {
    if let Ok(mut v) = body.single_mut() {
        *v = Visibility::Hidden;
    }
    let dt = time.delta_secs().clamp(1.0e-3, 0.1);
    let pos = player.ctrl.position;
    let Ok((mut tf, mut input, mut vis)) = q.single_mut() else {
        return;
    };
    *vis = if player.placed {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    let mut v = state.last_hero_cm.map_or(Vec3::ZERO, |l| (pos - l) / dt);
    if v.length() > 3000.0 {
        v = Vec3::ZERO; // teleport
    }
    state.last_hero_cm = Some(pos);
    v.y = if player.ctrl.grounded {
        0.0
    } else {
        player.ctrl.vertical_speed
    };
    *vel = vel.lerp(v, 0.5);
    let horizontal = Vec2::new(vel.x, vel.z);
    // Facing: the dialog partner, otherwise the direction of travel.
    let mut target = None;
    if state.mode == Mode::Dialog
        && let Some(id) = state.dialog_npc
        && let Some(p) = core.0.sim.world.actors.get(&id).map(|w| w.0)
    {
        target = Some(yaw_of_dir(p.x - pos.x, p.z - pos.z));
    } else if horizontal.length() > 70.0 {
        target = Some(yaw_of_dir(horizontal.x, horizontal.y));
    } else if let Some((id, t)) = state.combat_focus
        && time.elapsed_secs() - t < 2.5
        && let Some(p) = core.0.sim.world.actors.get(&id).map(|w| w.0)
    {
        target = Some(yaw_of_dir(p.x - pos.x, p.z - pos.z));
    }
    let _ = &npcs;
    let mut turn_error = 0.0;
    if let Some(t) = target {
        let delta = angle_delta(state.hero_yaw, t);
        let step = (10.0 * dt).min(delta.abs()) * delta.signum();
        state.hero_yaw += step;
        turn_error = delta - step;
    }
    tf.translation = pos * CM;
    tf.rotation = Quat::from_rotation_y(state.hero_yaw);
    tf.scale = Vec3::splat(CM);
    input.velocity = Vec3::new(vel.x, vel.y, vel.z);
    input.grounded = player.ctrl.grounded;
    input.stance = if state.drawn {
        Stance::OneHand
    } else {
        Stance::Unarmed
    };
    input.turn_error = if horizontal.length() < 40.0 {
        turn_error
    } else {
        0.0
    };
    commands.write(CoreCommand(Command::HeroTransform {
        pos: Pos::new(pos.x, pos.y, pos.z),
        yaw: bevy_to_sim_yaw(state.hero_yaw),
    }));
}

/// Puts the hero on the ground at the original start once its tile collision is resident
/// (nearest surface below the recorded height, not the roof above it).
fn place_hero(
    mut player: ResMut<Player>,
    collision: Res<crate::walk::Collision>,
    stream: Res<crate::walk::Stream>,
) {
    if player.placed || player.fly {
        return;
    }
    let p = player.ctrl.position;
    let cell = stream.settings.cell_at(p);
    if !collision
        .0
        .has_chunk(crate::walk::tile_chunk_id((cell.x, cell.z)))
    {
        return;
    }
    if let Some(y) = collision
        .0
        .ground_below(Vec3::new(p.x, p.y + 150.0, p.z), 900.0)
    {
        player.ctrl.position.y = y + 5.0;
        player.placed = true;
    }
}
