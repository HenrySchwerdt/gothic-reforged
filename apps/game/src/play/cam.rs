//! Third-person follow camera, dialog framing and scripted cinematic shots.
use super::{Mode, PlayState};
use crate::walk::{Collision, Player, PlayerCamera, setup, update_camera};
use bevy::audio::SpatialListener;
use bevy::prelude::*;
use gothic_runtime::CoreRes;

const CM: f32 = 0.01;

pub fn register(app: &mut App) {
    app.add_systems(Startup, add_listener.after(setup))
        .add_systems(Update, play_camera.after(update_camera));
}

fn add_listener(mut commands: Commands, cam: Query<Entity, With<PlayerCamera>>) {
    if let Ok(e) = cam.single() {
        commands.entity(e).insert(SpatialListener::new(0.2));
    }
}

fn play_camera(
    time: Res<Time>,
    player: Res<Player>,
    collision: Res<Collision>,
    core: Res<CoreRes>,
    mut state: ResMut<PlayState>,
    mut cam: Query<&mut Transform, With<PlayerCamera>>,
) {
    let Ok(mut cam) = cam.single_mut() else {
        return;
    };
    if let Some((pos, look)) = state.cinematic {
        *cam = Transform::from_translation(pos).looking_at(look, Vec3::Y);
        return;
    }
    if player.fly {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    let hero = player.ctrl.position;
    if state.mode == Mode::Dialog
        && let Some(id) = state.dialog_npc
        && let Some(p) = core.0.sim.world.actors.get(&id).map(|w| w.0)
    {
        let npc = Vec3::new(p.x, p.y, p.z);
        let d = Vec3::new(npc.x - hero.x, 0.0, npc.z - hero.z).normalize_or_zero();
        let mid = (hero + npc) * 0.5 + Vec3::Y * 150.0;
        let right = Vec3::new(d.z, 0.0, -d.x);
        let mut best = mid - d * 60.0 + Vec3::Y * 10.0 + right * 190.0;
        for side in [1.0_f32, -1.0] {
            let cand = mid - d * 60.0 + Vec3::Y * 10.0 + right * 190.0 * side;
            let dir = (cand - mid).normalize_or_zero();
            let len = (cand - mid).length();
            if collision.0.raycast(mid, dir, len).is_none() {
                best = cand;
                break;
            }
        }
        let want = best * CM;
        let cur = state.dialog_cam.unwrap_or(cam.translation);
        let next = cur.lerp(want, 1.0 - (-4.0 * dt).exp());
        state.dialog_cam = Some(next);
        *cam = Transform::from_translation(next).looking_at((mid + Vec3::Y * 5.0) * CM, Vec3::Y);
        return;
    }
    // Follow camera orbiting the hero's shoulders.
    let look = Quat::from_euler(EulerRot::YXZ, player.yaw, player.pitch, 0.0);
    let focus = hero + Vec3::Y * 150.0;
    let back = look * Vec3::Z;
    let want = 280.0;
    let dist = collision
        .0
        .raycast(focus, back, want)
        .map_or(want, |d| (d - 25.0).max(40.0));
    let pos = focus + back * dist + Vec3::Y * 20.0;
    cam.translation = pos * CM;
    cam.rotation = look;
}
